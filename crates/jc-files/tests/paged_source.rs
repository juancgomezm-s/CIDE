//! Sparse offsets test is explicitly Windows x64 and fails if sparse creation is
//! unavailable. It does not silently count an unexecuted GB test as acceptance.
#[cfg(all(windows, target_pointer_width = "64"))]
mod windows {
    use jc_core::source::{ImmutableByteSource, SourceReadError, MAX_READ_CHUNK_BYTES};
    use jc_files::paged_source::{PageCacheLimits, PagedFileSource};
    use std::{
        ffi::c_void,
        fs::{self, File, OpenOptions},
        os::windows::{
            fs::{FileExt, MetadataExt},
            io::AsRawHandle,
        },
        path::{Path, PathBuf},
        ptr::{null, null_mut},
        sync::{
            atomic::{AtomicU64, Ordering},
            Arc, Barrier,
        },
    };

    #[repr(C)]
    #[derive(Default)]
    struct StandardInfo {
        allocated: i64,
        eof: i64,
        links: u32,
        delete_pending: u8,
        directory: u8,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn DeviceIoControl(
            file: *mut c_void,
            code: u32,
            input: *const c_void,
            input_len: u32,
            output: *mut c_void,
            output_len: u32,
            returned: *mut u32,
            overlapped: *mut c_void,
        ) -> i32;
        fn GetFileInformationByHandleEx(
            file: *mut c_void,
            class: i32,
            info: *mut c_void,
            size: u32,
        ) -> i32;
        fn CreateFileMappingW(
            file: *mut c_void,
            attributes: *const c_void,
            protect: u32,
            maximum_high: u32,
            maximum_low: u32,
            name: *const u16,
        ) -> *mut c_void;
        fn MapViewOfFile(
            mapping: *mut c_void,
            access: u32,
            offset_high: u32,
            offset_low: u32,
            bytes: usize,
        ) -> *mut c_void;
        fn UnmapViewOfFile(view: *const c_void) -> i32;
        fn FlushViewOfFile(view: *const c_void, bytes: usize) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Workspace {
        root: PathBuf,
        base: PathBuf,
    }
    impl Workspace {
        fn new() -> Self {
            let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .canonicalize()
                .unwrap();
            let base = workspace
                .join("work/test-temp")
                .canonicalize()
                .expect("Debe existir work/test-temp para los fixtures propios");
            assert!(base.starts_with(&workspace));
            let metadata = fs::symlink_metadata(&base).unwrap();
            assert!(metadata.is_dir() && metadata.file_attributes() & 0x400 == 0);
            let root = base.join(format!(
                "paged-source-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            Self { root, base }
        }
        fn path(&self, name: &str) -> PathBuf {
            self.root.join(name)
        }
    }
    impl Drop for Workspace {
        fn drop(&mut self) {
            if self.root.parent() != Some(self.base.as_path()) {
                return;
            }
            if !fs::symlink_metadata(&self.root)
                .is_ok_and(|m| m.is_dir() && m.file_attributes() & 0x400 == 0)
            {
                return;
            }
            if let Ok(entries) = fs::read_dir(&self.root) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.parent() == Some(self.root.as_path())
                        && fs::symlink_metadata(&path)
                            .is_ok_and(|m| m.is_file() && m.file_attributes() & 0x400 == 0)
                    {
                        let _ = fs::remove_file(path);
                    }
                }
            }
            let _ = fs::remove_dir(&self.root);
        }
    }
    fn write_at(file: &File, mut offset: u64, mut bytes: &[u8]) {
        while !bytes.is_empty() {
            let count = file.seek_write(bytes, offset).unwrap();
            assert!(count > 0);
            offset += count as u64;
            bytes = &bytes[count..];
        }
    }
    struct WritableView(*mut c_void);
    impl Drop for WritableView {
        fn drop(&mut self) {
            unsafe {
                UnmapViewOfFile(self.0);
            }
        }
    }

    #[test]
    fn persistent_writable_mapping_cannot_mutate_a_source_admitted_as_immutable() {
        let workspace = Workspace::new();
        let path = workspace.path("mapping-previo.bin");
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        use std::io::Write;
        file.write_all(&vec![b'Q'; 3 * MAX_READ_CHUNK_BYTES])
            .unwrap();
        file.sync_all().unwrap();
        let mapping =
            unsafe { CreateFileMappingW(file.as_raw_handle(), null(), 0x04, 0, 0, null()) };
        assert!(
            !mapping.is_null(),
            "CreateFileMappingW: {}",
            std::io::Error::last_os_error()
        );
        let view = WritableView(unsafe { MapViewOfFile(mapping, 0x02, 0, 0, 0) });
        assert!(
            !view.0.is_null(),
            "MapViewOfFile: {}",
            std::io::Error::last_os_error()
        );
        assert_ne!(unsafe { CloseHandle(mapping) }, 0);
        drop(file);
        // Only the writable view survives. A plain open-handle sharing test does
        // not cover this case, and metadata cannot certify its dirty pages.
        match PagedFileSource::open(&path, PageCacheLimits { max_pages: 1 }) {
            Err(SourceReadError::Io(error)) if error.raw_os_error() == Some(32) => {
                println!("{{\"probe\":\"paged_source_prior_writable_mapping\",\"mapping_rejected\":true,\"immutable_source_admitted\":false}}");
            }
            Err(error) => {
                panic!("El mapping previo se rechazó por un motivo no verificado: {error}")
            }
            Ok(source) => {
                assert_eq!(&source.read_chunk(0, 1).unwrap()[..], b"Q");
                let offset = MAX_READ_CHUNK_BYTES + 17;
                unsafe {
                    (view.0 as *mut u8).add(offset).write_volatile(b'R');
                }
                assert_ne!(unsafe { FlushViewOfFile(view.0, 0) }, 0);
                let read = source.read_chunk(offset as u64, 1).unwrap();
                println!("{{\"probe\":\"paged_source_prior_writable_mapping\",\"mapping_rejected\":false,\"immutable_source_admitted\":true,\"uncached_byte_after_mapping_write\":{}}}",read[0]);
                panic!("La fuente admitió una vista escribible previa; byte original81, byte observado{}. No hay garantía de inmutabilidad",read[0]);
            }
        }
        drop(view);
    }

    #[test]
    fn sparse_five_gib_source_reads_offsets_beyond_four_gib_without_materializing_the_file() {
        let workspace = Workspace::new();
        let path = workspace.path("cinco-gib.bin");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        let mut returned = 0u32;
        // FSCTL_SET_SPARSE: FILE_DEVICE_FILE_SYSTEM(9), function49, METHOD_BUFFERED.
        let ok = unsafe {
            DeviceIoControl(
                file.as_raw_handle(),
                0x0009_00c4,
                null(),
                0,
                null_mut(),
                0,
                &mut returned,
                null_mut(),
            )
        };
        assert_ne!(
            ok,
            0,
            "El filesystem no admitió FSCTL_SET_SPARSE: {}",
            std::io::Error::last_os_error()
        );
        const LEN: u64 = 5 * 1024 * 1024 * 1024;
        const MIDDLE: u64 = (1u64 << 32) + 17;
        file.set_len(LEN).unwrap();
        write_at(&file, 0, b"\xef\xbb\xbf\x00INI\n");
        write_at(&file, MIDDLE, b"MAS4GiB\0");
        write_at(&file, LEN - 8, b"ULTIMO!\n");
        file.sync_all().unwrap();
        let mut info = StandardInfo::default();
        assert_ne!(
            unsafe {
                GetFileInformationByHandleEx(
                    file.as_raw_handle(),
                    1,
                    (&mut info as *mut StandardInfo).cast(),
                    std::mem::size_of::<StandardInfo>() as u32,
                )
            },
            0
        );
        assert_eq!(info.eof as u64, LEN);
        assert!(
            (0..=4 * 1024 * 1024).contains(&info.allocated),
            "El fixture sparse reservó demasiado disco: {}",
            info.allocated
        );
        assert_ne!(file.metadata().unwrap().file_attributes() & 0x200, 0);
        drop(file); // No writable handle remains when sealing the read source.
        let source = PagedFileSource::open(&path, PageCacheLimits { max_pages: 16 }).unwrap();
        assert_eq!(source.len(), LEN);
        assert_eq!(
            &source.read_chunk(0, 8).unwrap()[..],
            b"\xef\xbb\xbf\x00INI\n"
        );
        assert_eq!(&source.read_chunk(MIDDLE, 8).unwrap()[..], b"MAS4GiB\0");
        assert_eq!(
            &source.read_chunk(LEN - 8, MAX_READ_CHUNK_BYTES).unwrap()[..],
            b"ULTIMO!\n"
        );
        let boundary = source
            .read_chunk(MAX_READ_CHUNK_BYTES as u64 - 2, 8)
            .unwrap();
        assert_eq!(boundary.len(), 2);
        assert_eq!(&boundary[..], &[0, 0]);
        drop(boundary);
        assert_eq!(
            &source.read_chunk(MAX_READ_CHUNK_BYTES as u64, 8).unwrap()[..],
            &[0; 8]
        );
        assert!(source.read_chunk(LEN, 8).unwrap().is_empty());
        assert!(matches!(
            source.read_chunk(LEN + 1, 1),
            Err(SourceReadError::InvalidRange { .. })
        ));
        assert!(matches!(
            source.read_chunk(0, MAX_READ_CHUNK_BYTES + 1),
            Err(SourceReadError::InvalidRange { .. })
        ));
        let statistics = source.statistics().unwrap();
        assert!(statistics.bytes_read <= 5 * MAX_READ_CHUNK_BYTES as u64);
        assert!(statistics.retained_bytes <= statistics.budget_bytes);
        println!(
            concat!("{{\"probe\":\"paged_source_sparse_gb\",\"gb_fixture_executed\":true,",
            "\"logical_bytes\":{},\"allocated_file_bytes\":{},\"fixture_bytes_written\":24,",
            "\"bytes_read\":{},\"read_calls\":{},\"cache_pages\":{},\"retained_page_bytes\":{},",
            "\"cache_budget_bytes\":{},\"document_gb_supported\":false}}"),
            LEN,
            info.allocated,
            statistics.bytes_read,
            statistics.read_calls,
            statistics.pages,
            statistics.retained_bytes,
            statistics.budget_bytes
        );
    }

    #[test]
    fn pinned_leases_enforce_page_budget_until_released_and_cache_hits_do_not_reread() {
        let workspace = Workspace::new();
        let path = workspace.path("cuota.bin");
        fs::write(&path, vec![0x5a; 3 * MAX_READ_CHUNK_BYTES]).unwrap();
        let source = PagedFileSource::open(&path, PageCacheLimits { max_pages: 1 }).unwrap();
        let first = source.read_chunk(0, 1).unwrap();
        let again = source.read_chunk(0, 2).unwrap();
        assert_eq!(first.as_slice().as_ptr(), again.as_slice().as_ptr());
        assert_eq!(first.retained_bytes(), MAX_READ_CHUNK_BYTES);
        let before = source.statistics().unwrap();
        assert_eq!(before.bytes_read, MAX_READ_CHUNK_BYTES as u64);
        assert_eq!(before.pages, 1);
        assert_eq!(before.pinned_pages, 1);
        assert_eq!(before.cache_hits, 1);
        assert!(matches!(
            source.read_chunk(MAX_READ_CHUNK_BYTES as u64, 1),
            Err(SourceReadError::Busy { .. })
        ));
        let blocked = source.statistics().unwrap();
        assert_eq!(blocked.read_calls, before.read_calls);
        assert_eq!(blocked.retained_bytes, before.retained_bytes);
        drop(first);
        assert!(matches!(
            source.read_chunk(MAX_READ_CHUNK_BYTES as u64, 1),
            Err(SourceReadError::Busy { .. })
        ));
        drop(again);
        let next = source.read_chunk(MAX_READ_CHUNK_BYTES as u64, 1).unwrap();
        let after = source.statistics().unwrap();
        assert_eq!(after.pages, 1);
        assert_eq!(after.bytes_read, 2 * MAX_READ_CHUNK_BYTES as u64);
        assert_eq!(after.busy, 2);
        drop(source);
        assert_eq!(&next[..], &[0x5a]);
    }

    #[test]
    fn immutable_handle_denies_writers_and_keeps_original_bytes_after_path_replacement() {
        let workspace = Workspace::new();
        let path = workspace.path("original.bin");
        let replacement = workspace.path("reemplazo.bin");
        let mut original = vec![b'O'; 3 * MAX_READ_CHUNK_BYTES];
        original[MAX_READ_CHUNK_BYTES] = b'P';
        fs::write(&path, &original).unwrap();
        let source = PagedFileSource::open(&path, PageCacheLimits { max_pages: 1 }).unwrap();
        assert_eq!(&source.read_chunk(0, 1).unwrap()[..], b"O");
        let attempt = OpenOptions::new().write(true).open(&path);
        assert!(attempt.is_err(), "Un escritor accedió a la fuente sellada");
        assert_eq!(attempt.unwrap_err().raw_os_error(), Some(32));
        fs::write(&replacement, b"nuevo archivo\r\n").unwrap();
        fs::rename(&replacement, &path)
            .expect("La sustitución debe conservar el handle de origen compartido para delete");
        assert_eq!(fs::read(&path).unwrap(), b"nuevo archivo\r\n");
        // Uncached original page proves the handle still reads the old object,
        // not just a retained copy of the first page or a reopened pathname.
        assert_eq!(
            &source.read_chunk(MAX_READ_CHUNK_BYTES as u64, 1).unwrap()[..],
            b"P"
        );
        assert_eq!(source.len(), original.len() as u64);
        println!("{{\"probe\":\"paged_source_identity\",\"writer_rejected\":true,\"original_backing_survived_path_replacement\":true}}");
    }

    #[test]
    fn concurrent_reads_share_one_cached_page_and_empty_source_allocates_none() {
        let workspace = Workspace::new();
        let path = workspace.path("concurrente.bin");
        let empty = workspace.path("vacio.bin");
        fs::write(&path, vec![0x7e; MAX_READ_CHUNK_BYTES]).unwrap();
        fs::write(&empty, []).unwrap();
        let source =
            Arc::new(PagedFileSource::open(&path, PageCacheLimits { max_pages: 1 }).unwrap());
        let barrier = Arc::new(Barrier::new(8));
        let mut handles = Vec::new();
        for _ in 0..8 {
            let source = source.clone();
            let barrier = barrier.clone();
            handles.push(std::thread::spawn(move || {
                barrier.wait();
                assert_eq!(&source.read_chunk(17, 3).unwrap()[..], &[0x7e; 3]);
            }));
        }
        for handle in handles {
            handle.join().unwrap();
        }
        let statistics = source.statistics().unwrap();
        assert_eq!(statistics.bytes_read, MAX_READ_CHUNK_BYTES as u64);
        assert_eq!(statistics.cache_misses, 1);
        assert_eq!(statistics.cache_hits, 7);
        assert_eq!(statistics.pages, 1);
        let empty = PagedFileSource::open(&empty, PageCacheLimits { max_pages: 1 }).unwrap();
        assert!(empty.read_chunk(0, 1).unwrap().is_empty());
        assert_eq!(empty.statistics().unwrap().retained_bytes, 0);
        assert!(matches!(
            PagedFileSource::open(&workspace.root, PageCacheLimits::default()),
            Err(SourceReadError::Io(_))
        ));
    }
}

#[cfg(not(all(windows, target_pointer_width = "64")))]
#[test]
fn paged_file_source_reports_unsupported_platform_without_claiming_gb_execution() {
    use jc_core::source::SourceReadError;
    use jc_files::paged_source::{PageCacheLimits, PagedFileSource};
    assert!(
        matches!(PagedFileSource::open(std::path::Path::new("not-opened"),PageCacheLimits::default()),
        Err(SourceReadError::Io(error)) if error.kind()==std::io::ErrorKind::Unsupported)
    );
    println!("{{\"probe\":\"paged_source_sparse_gb\",\"gb_fixture_executed\":false,\"platform_supported\":false}}");
}
