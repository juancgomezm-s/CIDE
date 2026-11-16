//! Bounded private snapshots backed by an exclusively held file. Capture is a
//! blocking operation for a worker. The source is exposed only after copying
//! and synchronizing the complete expected length.
use crate::{paged_source::PagedFileSource, ContentFingerprint, FileError};
use jc_text::EncodingProbe;
#[cfg(all(windows, target_pointer_width = "64"))]
use jc_text::{EncodingSource, TextEncoding};
use std::{
    io::{self, Read},
    path::Path,
    sync::Arc,
};

#[derive(Clone)]
pub struct PrivateSnapshot {
    pub source: Arc<PagedFileSource>,
    pub fingerprint: ContentFingerprint,
    pub(crate) encoding: EncodingProbe,
}

impl PrivateSnapshot {
    /// The original is opened with read sharing only for the duration of the
    /// capture. Once it returns, changes to the original cannot affect the snapshot.
    /// The progress callback may return an error to abandon the temporary.
    pub fn capture(
        path: &Path,
        depot: &Path,
        max_bytes: u64,
        progress: impl FnMut(u64, u64) -> io::Result<()>,
    ) -> Result<Self, FileError> {
        Self::capture_scoped(path, None, depot, max_bytes, progress)
    }

    pub(crate) fn capture_scoped(
        path: &Path,
        root: Option<&Path>,
        depot: &Path,
        max_bytes: u64,
        progress: impl FnMut(u64, u64) -> io::Result<()>,
    ) -> Result<Self, FileError> {
        #[cfg(all(windows, target_pointer_width = "64"))]
        {
            use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
            let input = std::fs::OpenOptions::new()
                .read(true)
                .share_mode(FILE_SHARE_READ)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_SEQUENTIAL_SCAN)
                .open(path)?;
            if let Some(root) = root {
                crate::verify_file_in_root(root, &input)?;
            }
            let metadata = input.metadata()?;
            if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "La instantánea requiere un archivo regular",
                )
                .into());
            }
            if metadata.len() > max_bytes {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("El archivo excede el límite de instantánea de {max_bytes} bytes"),
                )
                .into());
            }
            let mut reader = EncodingReader::new(input);
            let mut snapshot = Self::capture_reader(&mut reader, depot, metadata.len(), progress)?;
            snapshot.encoding = reader.probe();
            Ok(snapshot)
        }
        #[cfg(not(all(windows, target_pointer_width = "64")))]
        {
            let _ = (path, root, depot, max_bytes, progress);
            Err(unsupported())
        }
    }

    /// Stage exactly this many bytes from a reader, leaving any trailing bytes
    /// for the caller. This also permits restoring an arena from a larger record.
    pub fn capture_reader(
        input: &mut (impl Read + ?Sized),
        depot: &Path,
        len: u64,
        progress: impl FnMut(u64, u64) -> io::Result<()>,
    ) -> Result<Self, FileError> {
        #[cfg(all(windows, target_pointer_width = "64"))]
        {
            use crate::{fingerprint::FingerprintingWriter, paged_source::PageCacheLimits};
            use jc_core::source::MAX_READ_CHUNK_BYTES;
            use std::io::Write;

            let mut progress = progress;
            progress(0, len)?;
            let mut file = private_file(depot)?;
            let mut buffer = [0; MAX_READ_CHUNK_BYTES];
            let mut writer = FingerprintingWriter::new(&mut file);
            let mut copied = 0;
            while copied < len {
                let count = (len - copied).min(buffer.len() as u64) as usize;
                let count = match input.read(&mut buffer[..count]) {
                    Ok(0) => return Err(io::Error::from(io::ErrorKind::UnexpectedEof).into()),
                    Ok(count) => count,
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(error) => return Err(error.into()),
                };
                writer.write_all(&buffer[..count])?;
                copied += count as u64;
                progress(copied, len)?;
            }
            writer.flush()?;
            let fingerprint = writer.fingerprint();
            file.sync_all()?;
            progress(copied, len)?;
            let source = PagedFileSource::from_file(file, PageCacheLimits::default())
                .map_err(io::Error::other)?;
            Ok(Self {
                source: Arc::new(source),
                fingerprint,
                encoding: unknown_encoding(),
            })
        }
        #[cfg(not(all(windows, target_pointer_width = "64")))]
        {
            let _ = (input, depot, len, progress);
            Err(unsupported())
        }
    }
}

#[cfg(all(windows, target_pointer_width = "64"))]
fn unknown_encoding() -> EncodingProbe {
    EncodingProbe {
        encoding: None,
        source: EncodingSource::UndeterminedLegacy,
        bom_len: 0,
    }
}

/// Tracks at most three bytes of an incomplete scalar and the three BOM bytes.
/// A valid prefix never classifies the remainder of a large file as UTF-8.
#[cfg(all(windows, target_pointer_width = "64"))]
struct EncodingReader<R> {
    input: R,
    prefix: [u8; 3],
    prefix_len: usize,
    partial: [u8; 4],
    partial_len: usize,
    ascii: bool,
    invalid: bool,
}

#[cfg(all(windows, target_pointer_width = "64"))]
impl<R> EncodingReader<R> {
    fn new(input: R) -> Self {
        Self {
            input,
            prefix: [0; 3],
            prefix_len: 0,
            partial: [0; 4],
            partial_len: 0,
            ascii: true,
            invalid: false,
        }
    }

    fn probe(&self) -> EncodingProbe {
        if self.prefix == [0xef, 0xbb, 0xbf] {
            return EncodingProbe {
                encoding: Some(TextEncoding::Utf8),
                source: EncodingSource::Utf8Bom,
                bom_len: 3,
            };
        }
        if self.invalid || self.partial_len != 0 {
            return unknown_encoding();
        }
        EncodingProbe {
            encoding: Some(TextEncoding::Utf8),
            source: if self.ascii {
                EncodingSource::AsciiCompatible
            } else {
                EncodingSource::ValidUtf8
            },
            bom_len: 0,
        }
    }

    fn observe(&mut self, mut bytes: &[u8]) {
        let prefix = bytes.len().min(self.prefix.len() - self.prefix_len);
        self.prefix[self.prefix_len..self.prefix_len + prefix].copy_from_slice(&bytes[..prefix]);
        self.prefix_len += prefix;
        if self.invalid {
            return;
        }
        self.ascii &= bytes.is_ascii();
        if self.partial_len != 0 {
            let width = match self.partial[0] {
                0xc2..=0xdf => 2,
                0xe0..=0xef => 3,
                _ => 4, // An incomplete prefix already validated by from_utf8.
            };
            let remaining = bytes.len().min(width - self.partial_len);
            self.partial[self.partial_len..self.partial_len + remaining]
                .copy_from_slice(&bytes[..remaining]);
            self.partial_len += remaining;
            if self.partial_len < width {
                return;
            }
            if std::str::from_utf8(&self.partial[..width]).is_err() {
                self.invalid = true;
                return;
            }
            self.partial_len = 0;
            bytes = &bytes[remaining..];
        }
        if let Err(error) = std::str::from_utf8(bytes) {
            if error.error_len().is_some() {
                self.invalid = true;
            } else {
                let tail = &bytes[error.valid_up_to()..];
                self.partial[..tail.len()].copy_from_slice(tail);
                self.partial_len = tail.len();
            }
        }
    }
}

#[cfg(all(windows, target_pointer_width = "64"))]
impl<R: Read> Read for EncodingReader<R> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let count = self.input.read(bytes)?;
        self.observe(&bytes[..count]);
        Ok(count)
    }
}

#[cfg(not(all(windows, target_pointer_width = "64")))]
fn unsupported() -> FileError {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "Las instantáneas privadas en disco requieren Windows de 64 bits",
    )
    .into()
}

#[cfg(all(windows, target_pointer_width = "64"))]
const FILE_SHARE_READ: u32 = 0x1;
#[cfg(all(windows, target_pointer_width = "64"))]
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
#[cfg(all(windows, target_pointer_width = "64"))]
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
#[cfg(all(windows, target_pointer_width = "64"))]
const FILE_FLAG_SEQUENTIAL_SCAN: u32 = 0x0800_0000;
#[cfg(all(windows, target_pointer_width = "64"))]
const FILE_FLAG_RANDOM_ACCESS: u32 = 0x1000_0000;
#[cfg(all(windows, target_pointer_width = "64"))]
const FILE_FLAG_DELETE_ON_CLOSE: u32 = 0x0400_0000;
#[cfg(all(windows, target_pointer_width = "64"))]
const GENERIC_READ: u32 = 0x8000_0000;
#[cfg(all(windows, target_pointer_width = "64"))]
const GENERIC_WRITE: u32 = 0x4000_0000;
#[cfg(all(windows, target_pointer_width = "64"))]
const DELETE: u32 = 0x0001_0000;

#[cfg(all(windows, target_pointer_width = "64"))]
fn private_file(depot: &Path) -> io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::sync::atomic::Ordering;

    for _ in 0..100 {
        let suffix = crate::NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = depot.join(format!(".cide-snapshot-{}-{suffix}", std::process::id()));
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .access_mode(GENERIC_READ | GENERIC_WRITE | DELETE)
            .share_mode(0)
            .custom_flags(FILE_FLAG_DELETE_ON_CLOSE | FILE_FLAG_RANDOM_ACCESS)
            .open(path);
        match file {
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            result => return result,
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "No se pudo reservar la instantánea privada",
    ))
}

#[cfg(all(test, windows, target_pointer_width = "64"))]
mod tests {
    use super::*;
    use jc_core::source::{ImmutableByteSource, MAX_READ_CHUNK_BYTES};
    use std::{
        fs,
        os::windows::fs::MetadataExt,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    struct Fixture {
        path: PathBuf,
        depot: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .and_then(Path::parent)
                .unwrap()
                .canonicalize()
                .unwrap();
            let temp = root.join("work/test-temp").canonicalize().unwrap();
            let metadata = fs::symlink_metadata(&temp).unwrap();
            assert!(temp.starts_with(&root) && metadata.is_dir());
            assert_eq!(metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT, 0);
            let path = temp.join(format!(
                "private-snapshot-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            let depot = path.join("depot");
            fs::create_dir(&depot).unwrap();
            Self { path, depot }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_file(self.path.join("source.bin"));
            let _ = fs::remove_dir(&self.depot);
            let _ = fs::remove_dir(&self.path);
        }
    }

    #[test]
    fn private_snapshot_is_exclusive_only_during_capture_and_is_deleted_with_source() {
        let fixture = Fixture::new();
        let path = fixture.path.join("source.bin");
        let bytes = [vec![b'a'; MAX_READ_CHUNK_BYTES], b"\x00\xff\r\n".to_vec()].concat();
        fs::write(&path, &bytes).unwrap();
        let snapshot = PrivateSnapshot::capture(
            &path,
            &fixture.depot,
            bytes.len() as u64,
            |copied, total| {
                if copied == 0 {
                    assert!(std::fs::OpenOptions::new().write(true).open(&path).is_err());
                }
                assert_eq!(total, bytes.len() as u64);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(snapshot.fingerprint, ContentFingerprint::of(&bytes));
        fs::write(&path, b"cambio externo").unwrap();
        let retained = snapshot.source.clone();
        drop(snapshot);
        let tail = retained
            .read_chunk(MAX_READ_CHUNK_BYTES as u64, MAX_READ_CHUNK_BYTES)
            .unwrap();
        assert_eq!(tail.as_slice(), b"\x00\xff\r\n");
        let temporary = fs::read_dir(&fixture.depot)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert!(std::fs::File::open(&temporary).is_err());
        drop(retained);
        assert!(fs::read_dir(&fixture.depot).unwrap().next().is_none());
        assert_eq!(tail.as_slice(), b"\x00\xff\r\n");
    }

    #[test]
    fn private_snapshot_discards_partial_data_on_cancel_or_short_read() {
        let fixture = Fixture::new();
        let path = fixture.path.join("source.bin");
        let bytes = vec![1; MAX_READ_CHUNK_BYTES + 7];
        fs::write(&path, &bytes).unwrap();
        let result =
            PrivateSnapshot::capture(&path, &fixture.depot, bytes.len() as u64, |copied, _| {
                if copied == 0 {
                    Ok(())
                } else {
                    Err(io::Error::from(io::ErrorKind::Interrupted))
                }
            });
        assert!(
            matches!(result, Err(FileError::Io(error)) if error.kind() == io::ErrorKind::Interrupted)
        );
        assert!(fs::read_dir(&fixture.depot).unwrap().next().is_none());
        let too_short =
            PrivateSnapshot::capture_reader(&mut &b"a"[..], &fixture.depot, 2, |_, _| Ok(()));
        assert!(
            matches!(too_short, Err(FileError::Io(error)) if error.kind() == io::ErrorKind::UnexpectedEof)
        );
        assert!(fs::read_dir(&fixture.depot).unwrap().next().is_none());
        let exceeded = PrivateSnapshot::capture(&path, &fixture.depot, 1, |_, _| unreachable!());
        assert!(
            matches!(exceeded, Err(FileError::Io(error)) if error.kind() == io::ErrorKind::InvalidData)
        );
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn private_snapshot_reader_leaves_trailing_record_bytes() {
        let fixture = Fixture::new();
        let mut input = &b"fuenteprivada;continuacion"[..];
        let snapshot =
            PrivateSnapshot::capture_reader(&mut input, &fixture.depot, 13, |_, _| Ok(())).unwrap();
        assert_eq!(
            snapshot.source.read_chunk(0, 13).unwrap().as_slice(),
            b"fuenteprivada"
        );
        assert_eq!(input, b";continuacion");
    }

    #[test]
    fn private_snapshot_session_detects_encoding_across_pages_and_keeps_recovery_paged() {
        let fixture = Fixture::new();
        let path = fixture.path.join("source.bin");
        let mut initial = vec![b'a'; MAX_READ_CHUNK_BYTES - 1];
        initial.extend_from_slice("é\r\n".as_bytes());
        fs::write(&path, &initial).unwrap();
        let open = || {
            crate::FileSession::open_private_snapshot(
                &path,
                None,
                &fixture.depot,
                u64::MAX,
                |_, _| Ok(()),
            )
            .unwrap()
        };
        let mut session = open();
        assert_eq!(session.encoding_source(), EncodingSource::ValidUtf8);
        assert_eq!(session.encoding(), Some(TextEncoding::Utf8));
        assert!(session.inspection().is_err());
        session.insert_text(0, "inicio ").unwrap();
        let mut wire = Vec::new();
        session.document().write_recovery(&mut wire).unwrap();
        let recovered = crate::read_recovery_document_in(
            &mut wire.as_slice(),
            jc_core::RecoveryLimits::default(),
            MAX_READ_CHUNK_BYTES,
            &fixture.depot,
        )
        .unwrap();
        assert!(recovered.is_paged());
        assert_eq!(recovered.read_range(0..7).unwrap(), b"inicio ");
        assert_eq!(
            recovered
                .read_range(recovered.len() - 4..recovered.len())
                .unwrap(),
            "é\r\n".as_bytes()
        );
        session.save().unwrap();
        assert_eq!(
            fs::read(&path).unwrap(),
            [b"inicio ", initial.as_slice()].concat()
        );

        initial.push(0xff);
        fs::write(&path, initial).unwrap();
        let session = open();
        assert_eq!(
            session.encoding_source(),
            EncodingSource::UndeterminedLegacy
        );
        assert_eq!(session.encoding(), None);
    }
}
