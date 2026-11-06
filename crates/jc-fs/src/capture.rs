//! Stable bounded captures. The Windows implementation fixes the admitted root
//! and every existing ancestor with non-reparse handles denying write/delete
//! sharing. An absence is an observation with ancestor identity, not a promise
//! that the namespace cannot change after the capture returns.
use std::{
    io,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Eq, PartialEq)]
struct ObjectIdentity {
    volume: u32,
    index: u64,
    attributes: u32,
}
#[derive(Clone, Debug, Eq, PartialEq)]
struct FileState {
    identity: ObjectIdentity,
    length: u64,
    links: u32,
    write_time: i64,
    change_time: i64,
}
/// Identity/version of the file and its admitted path. Access time is omitted:
/// reading our own snapshot must not invalidate its token.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileToken {
    path: PathBuf,
    ancestors: Vec<ObjectIdentity>,
    state: FileState,
}
impl FileToken {
    /// Logical retained heap, excluding the containing value's inline size.
    pub fn memory_bytes(&self) -> usize {
        self.path.capacity() + self.ancestors.capacity() * std::mem::size_of::<ObjectIdentity>()
    }
    pub fn len(&self) -> u64 {
        self.state.length
    }
    pub fn is_empty(&self) -> bool {
        self.state.length == 0
    }
}
/// Proof of a missing component below an admitted, fixed ancestor chain.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MissingToken {
    path: PathBuf,
    missing_at: PathBuf,
    ancestors: Vec<FileState>,
}
impl MissingToken {
    /// Logical retained heap, excluding the containing value's inline size.
    pub fn memory_bytes(&self) -> usize {
        self.path.capacity()
            + self.missing_at.capacity()
            + self.ancestors.capacity() * std::mem::size_of::<FileState>()
    }
}
#[derive(Debug)]
pub enum Capture {
    Present {
        path: PathBuf,
        bytes: Vec<u8>,
        token: FileToken,
    },
    Absent {
        token: MissingToken,
    },
}

/// Captures bytes or a proved absence. root must be the caller's canonical
/// absolute root; path is absolute and lexically inside it. No lossy conversion.
/// Unsupported platforms fail closed; read_in_root retains its existing scope.
pub fn capture_in_root(
    root: &Path,
    path: &Path,
    limit: usize,
    cancelled: &mut dyn FnMut() -> bool,
) -> io::Result<Capture> {
    #[cfg(windows)]
    {
        windows::capture(root, path, limit, cancelled)
    }
    #[cfg(not(windows))]
    {
        let _ = (root, path, limit, cancelled);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Esta plataforma aún no admite capturas estables confinadas",
        ))
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::{
        ffi::{c_void, OsString},
        fs::{self, File, OpenOptions},
        io::Read,
        os::windows::{
            ffi::{OsStrExt, OsStringExt},
            fs::OpenOptionsExt,
            io::AsRawHandle,
        },
        path::Component,
    };
    const REPARSE: u32 = 0x400;
    const DIRECTORY: u32 = 0x10;
    const FLAGS: u32 = 0x0020_0000 | 0x0200_0000;
    const READ_ATTRIBUTES: u32 = 0x80;
    const MAX_DEPTH: usize = 256;

    #[repr(C)]
    struct FileTime {
        low: u32,
        high: u32,
    }
    #[repr(C)]
    struct HandleInfo {
        attributes: u32,
        creation: FileTime,
        access: FileTime,
        write: FileTime,
        volume: u32,
        size_high: u32,
        size_low: u32,
        links: u32,
        index_high: u32,
        index_low: u32,
    }
    #[repr(C)]
    struct BasicInfo {
        creation: i64,
        access: i64,
        write: i64,
        change: i64,
        attributes: u32,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetFileInformationByHandle(handle: *mut c_void, info: *mut HandleInfo) -> i32;
        fn GetFileType(handle: *mut c_void) -> u32;
        fn GetFileInformationByHandleEx(
            handle: *mut c_void,
            class: i32,
            info: *mut c_void,
            size: u32,
        ) -> i32;
        fn GetFinalPathNameByHandleW(
            handle: *mut c_void,
            path: *mut u16,
            length: u32,
            flags: u32,
        ) -> u32;
    }
    fn changed() -> io::Error {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "El archivo o sus ancestros cambiaron durante la captura",
        )
    }
    fn excluded() -> io::Error {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Se excluyen enlaces y puntos de reanálisis",
        )
    }
    fn check(cancelled: &mut dyn FnMut() -> bool) -> io::Result<()> {
        if cancelled() {
            Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Captura de archivo cancelada",
            ))
        } else {
            Ok(())
        }
    }
    fn state(file: &File) -> io::Result<FileState> {
        let mut info: HandleInfo = unsafe { std::mem::zeroed() };
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut basic: BasicInfo = unsafe { std::mem::zeroed() };
        if unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                0,
                (&mut basic as *mut BasicInfo).cast(),
                std::mem::size_of::<BasicInfo>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        if info.attributes & REPARSE != 0 || basic.attributes & REPARSE != 0 {
            return Err(excluded());
        }
        if info.attributes != basic.attributes {
            return Err(changed());
        }
        Ok(FileState {
            identity: ObjectIdentity {
                volume: info.volume,
                index: (u64::from(info.index_high) << 32) | u64::from(info.index_low),
                attributes: info.attributes,
            },
            length: (u64::from(info.size_high) << 32) | u64::from(info.size_low),
            links: info.links,
            write_time: basic.write,
            change_time: basic.change,
        })
    }
    fn final_path(file: &File) -> io::Result<PathBuf> {
        let mut buffer = vec![0u16; 32768];
        let length = unsafe {
            GetFinalPathNameByHandleW(
                file.as_raw_handle(),
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                0,
            )
        };
        if length == 0 {
            return Err(io::Error::last_os_error());
        }
        if length >= buffer.len() as u32 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "La ruta resuelta es demasiado larga",
            ));
        }
        Ok(PathBuf::from(OsString::from_wide(
            &buffer[..length as usize],
        )))
    }
    fn open(path: &Path, payload: bool) -> io::Result<File> {
        let mut options = OpenOptions::new();
        options.read(true).share_mode(1).custom_flags(FLAGS);
        if !payload {
            options.access_mode(READ_ATTRIBUTES);
        }
        options.open(path)
    }
    fn validate_dir(root: &Path, file: &File) -> io::Result<FileState> {
        let current = state(file)?;
        if current.identity.attributes & DIRECTORY == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Un ancestro no es una carpeta",
            ));
        }
        if !final_path(file)?.starts_with(root) {
            return Err(excluded());
        }
        Ok(current)
    }
    fn validate_ancestors(root: &Path, dirs: &[File], initial: &[FileState]) -> io::Result<()> {
        for (dir, before) in dirs.iter().zip(initial) {
            if validate_dir(root, dir)? != *before {
                return Err(changed());
            }
        }
        Ok(())
    }
    pub(super) fn capture(
        root: &Path,
        path: &Path,
        limit: usize,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> io::Result<Capture> {
        check(cancelled)?;
        if !root.is_absolute()
            || !path.is_absolute()
            || root.as_os_str().encode_wide().count() >= 32768
            || path.as_os_str().encode_wide().count() >= 32768
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "La captura necesita rutas canónicas absolutas y acotadas",
            ));
        }
        let relative = path.strip_prefix(root).map_err(|_| excluded())?;
        let components = relative.components().collect::<Vec<_>>();
        if components.is_empty()
            || components.len() > MAX_DEPTH
            || components
                .iter()
                .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "La ruta de captura contiene componentes inválidos",
            ));
        }
        let root_handle = open(root, false)?;
        let root_state = validate_dir(root, &root_handle)?;
        if final_path(&root_handle)? != root {
            return Err(excluded());
        }
        let mut dirs = vec![root_handle];
        let mut initial = vec![root_state];
        let mut current = root.to_path_buf();
        for (ordinal, component) in components.iter().enumerate() {
            check(cancelled)?;
            current.push(component.as_os_str());
            let leaf = ordinal + 1 == components.len();
            let mut file = match open(&current, leaf) {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    validate_ancestors(root, &dirs, &initial)?;
                    match fs::symlink_metadata(&current) {
                        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                        Ok(_) => return Err(changed()),
                        Err(error) => return Err(error),
                    }
                    check(cancelled)?;
                    validate_ancestors(root, &dirs, &initial)?;
                    return Ok(Capture::Absent {
                        token: MissingToken {
                            path: path.to_path_buf(),
                            missing_at: current,
                            ancestors: initial,
                        },
                    });
                }
                Err(error) => return Err(error),
            };
            if unsafe { GetFileType(file.as_raw_handle()) } != 1 {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "La captura requiere un archivo o carpeta de disco",
                ));
            }
            if !leaf {
                initial.push(validate_dir(root, &file)?);
                dirs.push(file);
                continue;
            }
            let before = state(&file)?;
            if before.identity.attributes & DIRECTORY != 0 || !file.metadata()?.is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "La fuente no es un archivo regular",
                ));
            }
            let resolved = final_path(&file)?;
            if !resolved.starts_with(root) {
                return Err(excluded());
            }
            if before.length > limit as u64 {
                return Err(io::Error::new(
                    io::ErrorKind::FileTooLarge,
                    "El archivo supera la cuota de captura",
                ));
            }
            let size = usize::try_from(before.length).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::FileTooLarge,
                    "La longitud no cabe en esta plataforma",
                )
            })?;
            let mut bytes = Vec::new();
            bytes.try_reserve_exact(size).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::OutOfMemory,
                    "No se pudo reservar la captura acotada",
                )
            })?;
            bytes.resize(size, 0);
            let mut filled = 0;
            while filled < size {
                check(cancelled)?;
                let end = (filled + 64 * 1024).min(size);
                match file.read(&mut bytes[filled..end]) {
                    Ok(0) => return Err(changed()),
                    Ok(count) => filled += count,
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(error) => return Err(error),
                }
            }
            check(cancelled)?;
            let mut eof = [0; 1];
            loop {
                match file.read(&mut eof) {
                    Ok(0) => break,
                    Ok(_) => return Err(changed()),
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                        check(cancelled)?;
                    }
                    Err(error) => return Err(error),
                }
            }
            if state(&file)? != before || final_path(&file)? != resolved {
                return Err(changed());
            }
            validate_ancestors(root, &dirs, &initial)?;
            check(cancelled)?;
            let ancestors = initial.into_iter().map(|s| s.identity).collect();
            return Ok(Capture::Present {
                path: resolved.clone(),
                bytes,
                token: FileToken {
                    path: resolved,
                    ancestors,
                    state: before,
                },
            });
        }
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "La captura no tiene un archivo de destino",
        ))
    }
}

#[cfg(all(windows, test))]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);
    fn fixture() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "cide-fs-capture-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        dir.canonicalize().unwrap()
    }
    fn present(root: &Path, path: &Path, limit: usize) -> (PathBuf, Vec<u8>, FileToken) {
        match capture_in_root(root, path, limit, &mut || false).unwrap() {
            Capture::Present { path, bytes, token } => (path, bytes, token),
            Capture::Absent { .. } => panic!("expected present owned fixture"),
        }
    }
    #[test]
    fn repeated_reads_keep_identity_and_exact_bom_invalid_bytes_and_endings() {
        let root = fixture();
        fs::create_dir(root.join("nested")).unwrap();
        let path = root.join("nested/raw.bin");
        let bytes = b"\xef\xbb\xbfraw\xff\r\nlast\r";
        fs::write(&path, bytes).unwrap();
        let (actual, first, token) = present(&root, &path, bytes.len());
        assert_eq!(actual, path);
        assert_eq!(first, bytes);
        let (_, second, again) = present(&root, &path, bytes.len());
        assert_eq!(token, again);
        assert_eq!(second, bytes);
        assert_eq!(token.len(), bytes.len() as u64);
        assert!(!token.is_empty());
        assert!(token.memory_bytes() >= path.as_os_str().len());
        fs::write(&path, b"a different source length").unwrap();
        assert_ne!(token, present(&root, &path, 100).2);
    }
    #[test]
    fn absence_is_proved_at_first_missing_component_and_empty_file_is_present() {
        let root = fixture();
        let path = root.join("missing/child.bin");
        let first = match capture_in_root(&root, &path, 10, &mut || false).unwrap() {
            Capture::Absent { token } => token,
            Capture::Present { .. } => panic!("unexpected file"),
        };
        let second = match capture_in_root(&root, &path, 10, &mut || false).unwrap() {
            Capture::Absent { token } => token,
            Capture::Present { .. } => panic!("unexpected file"),
        };
        assert_eq!(first, second);
        assert!(first.memory_bytes() > path.capacity());
        fs::create_dir(root.join("missing")).unwrap();
        let later = match capture_in_root(&root, &path, 10, &mut || false).unwrap() {
            Capture::Absent { token } => token,
            Capture::Present { .. } => panic!("unexpected file"),
        };
        assert_ne!(first, later);
        fs::write(&path, []).unwrap();
        let (_, bytes, token) = present(&root, &path, 0);
        assert!(bytes.is_empty());
        assert!(token.is_empty());
    }
    #[test]
    fn limits_cancellation_and_invalid_paths_never_publish_partial_bytes() {
        let root = fixture();
        let path = root.join("source.bin");
        let original = vec![b'x'; 128 * 1024];
        fs::write(&path, &original).unwrap();
        assert_eq!(
            capture_in_root(&root, &path, original.len() - 1, &mut || false)
                .unwrap_err()
                .kind(),
            io::ErrorKind::FileTooLarge
        );
        let mut calls = 0;
        let error = capture_in_root(&root, &path, original.len(), &mut || {
            calls += 1;
            calls >= 4
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        assert!(capture_in_root(&root, &root.join("../outside"), 100, &mut || false).is_err());
        assert!(capture_in_root(&root, &root, 100, &mut || false).is_err());
        fs::write(root.join("ordinary-file"), b"x").unwrap();
        assert!(
            capture_in_root(&root, &root.join("ordinary-file/child"), 100, &mut || false).is_err()
        );
        assert_eq!(fs::read(path).unwrap(), original);
    }
    #[test]
    fn capture_denies_hardlink_writers_and_ancestor_rename_while_reading() {
        let root = fixture();
        let path = root.join("source.bin");
        let alias = root.join("same-file.bin");
        fs::write(&path, b"owned sharing fixture").unwrap();
        fs::hard_link(&path, &alias).unwrap();
        let mut calls = 0;
        let mut writer_denied = false;
        let mut rename_denied = false;
        let renamed = root.with_extension("moved");
        let captured = capture_in_root(&root, &path, 100, &mut || {
            calls += 1;
            if calls == 3 {
                writer_denied = fs::OpenOptions::new().write(true).open(&alias).is_err();
                rename_denied = fs::rename(&root, &renamed).is_err();
            }
            false
        })
        .unwrap();
        assert!(writer_denied);
        assert!(rename_denied);
        assert!(matches!(captured, Capture::Present { .. }));
        assert_eq!(fs::read(alias).unwrap(), b"owned sharing fixture");
    }
}
