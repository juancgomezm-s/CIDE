//! File lifecycle for CIDE.
//!
//! A FileSession owns the byte-preserving document plus the selected text
//! encoding. Saving verifies that the on-disk content still matches the version
//! opened/saved by the session, writes a same-directory temporary file, flushes
//! it, and replaces the target atomically on supported platforms.

use jc_core::{Document, EditError};
use jc_text::{
    encode, inspect_text, probe_encoding, EncodeError, EncodingSource, TextEncoding, TextInspection,
};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
mod scoped_read;
pub use jc_fs::{capture_in_root, Capture, FileToken, MissingToken};
pub use scoped_read::{read_in_root, verify_file_in_root};
mod directory;
pub use directory::{list_directory_in_root, DirectoryEntry, DirectoryListing};
mod fingerprint;
pub use fingerprint::ContentFingerprint;
use fingerprint::{fingerprint_file, FingerprintingWriter};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
#[cfg(windows)]
mod windows;
#[cfg(all(windows, test))]
use windows::replace_path;
pub mod paged_source;
pub mod private_snapshot;
pub mod recovery;
mod session_recovery;
pub use session_recovery::{
    read_recovery_document_in, read_recovery_path, write_recovery_path, SessionRecoveryError,
    SessionRecoveryInfo, SessionRecoveryLimits,
};

type Fingerprint = ContentFingerprint;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalState {
    Unchanged,
    Changed,
    Missing,
}

#[derive(Debug)]
pub enum FileError {
    Io(io::Error),
    ExternalModification(ExternalState),
    EncodingRequired,
    TargetExists(PathBuf),
    Encode(EncodeError),
    Edit(EditError),
    SaveRecovery {
        target: PathBuf,
        temporary: PathBuf,
        error: io::Error,
    },
}

impl fmt::Display for FileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::ExternalModification(ExternalState::Changed) => {
                write!(f, "El archivo cambió en disco desde que se abrió o guardó")
            }
            Self::ExternalModification(ExternalState::Missing) => {
                write!(
                    f,
                    "El archivo se eliminó del disco desde que se abrió o guardó"
                )
            }
            Self::ExternalModification(ExternalState::Unchanged) => {
                write!(f, "Estado externo sin cambios inesperado")
            }
            Self::EncodingRequired => {
                write!(
                    f,
                    "Debes elegir una codificación explícita para insertar texto"
                )
            }
            Self::TargetExists(path) => {
                write!(f, "El destino ya existe: {}", path.display())
            }
            Self::Encode(error) => write!(f, "{error}"),
            Self::Edit(error) => write!(f, "{error}"),
            Self::SaveRecovery {
                target,
                temporary,
                error,
            } => write!(
                f,
                "No se pudo guardar {}; los bytes de recuperación se conservan en {}: {error}",
                target.display(),
                temporary.display()
            ),
        }
    }
}

impl std::error::Error for FileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Encode(error) => Some(error),
            Self::Edit(error) => Some(error),
            Self::SaveRecovery { error, .. } => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for FileError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<EncodeError> for FileError {
    fn from(value: EncodeError) -> Self {
        Self::Encode(value)
    }
}

impl From<EditError> for FileError {
    fn from(value: EditError) -> Self {
        Self::Edit(value)
    }
}

#[derive(Debug)]
pub struct FileSession {
    path: PathBuf,
    document: Document,
    encoding: Option<TextEncoding>,
    encoding_source: EncodingSource,
    disk_fingerprint: Option<Fingerprint>,
}

impl FileSession {
    pub fn open(
        path: impl Into<PathBuf>,
        explicit: Option<TextEncoding>,
    ) -> Result<Self, FileError> {
        let path = path.into();
        let bytes = fs::read(&path)?;
        Ok(Self::from_open_bytes(path, bytes, explicit))
    }

    /// Create a byte-exact, independently owned disk snapshot on a worker.
    /// The source's original path remains the save target.
    pub fn open_private_snapshot(
        path: impl Into<PathBuf>,
        explicit: Option<TextEncoding>,
        depot: &Path,
        max_bytes: u64,
        progress: impl FnMut(u64, u64) -> io::Result<()>,
    ) -> Result<Self, FileError> {
        Self::open_private_snapshot_scoped(path, explicit, None, depot, max_bytes, progress)
    }

    pub fn open_private_snapshot_scoped(
        path: impl Into<PathBuf>,
        explicit: Option<TextEncoding>,
        root: Option<&Path>,
        depot: &Path,
        max_bytes: u64,
        progress: impl FnMut(u64, u64) -> io::Result<()>,
    ) -> Result<Self, FileError> {
        let path = path.into();
        let snapshot = private_snapshot::PrivateSnapshot::capture_scoped(
            &path, root, depot, max_bytes, progress,
        )?;
        let (encoding, encoding_source) = explicit.map_or(
            (snapshot.encoding.encoding, snapshot.encoding.source),
            |encoding| (Some(encoding), EncodingSource::Explicit),
        );
        Ok(Self {
            path,
            document: Document::from_source(snapshot.source)?,
            encoding,
            encoding_source,
            disk_fingerprint: Some(snapshot.fingerprint),
        })
    }

    /// Bound the resident read even if the file grew after admission.
    pub fn open_bounded(
        path: impl Into<PathBuf>,
        explicit: Option<TextEncoding>,
        max_bytes: usize,
    ) -> Result<Self, FileError> {
        let path = path.into();
        let file = File::open(&path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() > max_bytes as u64 {
            return Err(
                io::Error::other("El archivo requiere apertura con instantánea privada").into(),
            );
        }
        let mut bytes = Vec::new();
        file.take((max_bytes as u64).saturating_add(1))
            .read_to_end(&mut bytes)?;
        if bytes.len() > max_bytes {
            return Err(io::Error::other(
                "El archivo creció durante la apertura; vuelve a abrirlo",
            )
            .into());
        }
        Ok(Self::from_open_bytes(path, bytes, explicit))
    }

    /// Explicit project opening validates the actual handle before reading bytes.
    /// The byte limit also applies if the file grows after its initial metadata.
    pub fn open_in_root(
        root: &Path,
        path: impl Into<PathBuf>,
        explicit: Option<TextEncoding>,
        max_bytes: usize,
    ) -> Result<Self, FileError> {
        let root = root.canonicalize()?;
        if !root.is_dir() {
            return Err(io::Error::other("La raíz del proyecto no es un directorio").into());
        }
        let (path, bytes) = read_in_root(&root, &path.into(), max_bytes)?;
        Ok(Self::from_open_bytes(path, bytes, explicit))
    }
    /// Compare the actual loaded bytes before admitting a session to a catalog.
    /// The stored initial fingerprint avoids a second source read/hash/copy.
    pub fn open_in_root_at_version(
        root: &Path,
        path: impl Into<PathBuf>,
        explicit: Option<TextEncoding>,
        max_bytes: usize,
        expected: ContentFingerprint,
    ) -> Result<Self, FileError> {
        let session = Self::open_in_root(root, path, explicit, max_bytes)?;
        if session.disk_fingerprint != Some(expected) {
            return Err(io::Error::other(
                "El contenido difiere de la versión esperada; actualiza el resultado",
            )
            .into());
        }
        Ok(session)
    }

    fn from_open_bytes(path: PathBuf, bytes: Vec<u8>, explicit: Option<TextEncoding>) -> Self {
        let probe = probe_encoding(&bytes, explicit);
        let fingerprint = Fingerprint::of(&bytes);
        Self {
            path,
            document: Document::from_bytes(bytes),
            encoding: probe.encoding,
            encoding_source: probe.source,
            disk_fingerprint: Some(fingerprint),
        }
    }

    pub fn new(path: impl Into<PathBuf>, encoding: TextEncoding) -> Self {
        Self {
            path: path.into(),
            document: Document::default(),
            encoding: Some(encoding),
            encoding_source: EncodingSource::Explicit,
            disk_fingerprint: None,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn is_new(&self) -> bool {
        self.disk_fingerprint.is_none()
    }

    pub fn document(&self) -> &Document {
        &self.document
    }

    pub fn document_mut(&mut self) -> &mut Document {
        &mut self.document
    }

    pub fn encoding(&self) -> Option<TextEncoding> {
        self.encoding
    }

    pub fn encoding_source(&self) -> EncodingSource {
        self.encoding_source
    }

    pub fn set_encoding(&mut self, encoding: TextEncoding) {
        self.encoding = Some(encoding);
        self.encoding_source = EncodingSource::Explicit;
    }

    pub fn inspection(&self) -> Result<TextInspection, FileError> {
        if self.document.is_paged() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "La inspección completa requiere un documento residente",
            )
            .into());
        }
        let bytes = self.document.to_bytes();
        let mut inspection = inspect_text(&bytes, self.encoding);
        inspection.probe.source = self.encoding_source;
        Ok(inspection)
    }

    pub fn insert_text(&mut self, offset: usize, text: &str) -> Result<(), FileError> {
        let encoding = self.encoding.ok_or(FileError::EncodingRequired)?;
        let bytes = encode(text, encoding)?;
        self.document.insert(offset, &bytes)?;
        Ok(())
    }

    pub fn replace_text(
        &mut self,
        range: std::ops::Range<usize>,
        text: &str,
    ) -> Result<(), FileError> {
        let encoding = self.encoding.ok_or(FileError::EncodingRequired)?;
        let bytes = encode(text, encoding)?;
        self.document.replace(range, &bytes)?;
        Ok(())
    }

    pub fn external_state(&self) -> Result<ExternalState, FileError> {
        match (self.disk_fingerprint, fingerprint_file(&self.path)) {
            (Some(expected), Ok(actual)) => Ok(if actual == expected {
                ExternalState::Unchanged
            } else {
                ExternalState::Changed
            }),
            (Some(_), Err(FileError::Io(error))) if error.kind() == io::ErrorKind::NotFound => {
                Ok(ExternalState::Missing)
            }
            (None, Err(FileError::Io(error))) if error.kind() == io::ErrorKind::NotFound => {
                Ok(ExternalState::Unchanged)
            }
            (None, Ok(_)) => Ok(ExternalState::Changed),
            (_, Err(FileError::ExternalModification(_))) => Ok(ExternalState::Changed),
            (_, Err(error)) => Err(error),
        }
    }

    pub fn save(&mut self) -> Result<(), FileError> {
        let state = self.external_state()?;
        if state != ExternalState::Unchanged {
            return Err(FileError::ExternalModification(state));
        }
        let create_only = self.disk_fingerprint.is_none();
        self.write_current(create_only, self.disk_fingerprint)
    }

    pub fn force_save(&mut self) -> Result<(), FileError> {
        self.write_current(false, None)
    }

    pub fn save_as(&mut self, path: impl Into<PathBuf>) -> Result<(), FileError> {
        let path = path.into();
        if path == self.path {
            return self.save();
        }
        if path.exists() {
            return Err(FileError::TargetExists(path));
        }
        self.write_to_path(&path, true, None)?;
        self.path = path;
        Ok(())
    }

    pub fn force_save_as(&mut self, path: impl Into<PathBuf>) -> Result<(), FileError> {
        let path = path.into();
        if path == self.path {
            return self.force_save();
        }
        self.write_to_path(&path, false, None)?;
        self.path = path;
        Ok(())
    }

    fn write_current(
        &mut self,
        create_only: bool,
        expected: Option<Fingerprint>,
    ) -> Result<(), FileError> {
        let path = self.path.clone();
        self.write_to_path(&path, create_only, expected)
    }

    fn write_to_path(
        &mut self,
        path: &Path,
        create_only: bool,
        expected: Option<Fingerprint>,
    ) -> Result<(), FileError> {
        let mut saved_fingerprint = None;
        atomic_replace_with_commit(
            path,
            create_only,
            |file| {
                let mut writer = FingerprintingWriter::new(file);
                self.document.write_to(&mut writer)?;
                saved_fingerprint = Some(writer.fingerprint());
                Ok(())
            },
            |temporary, target| replace_expected(temporary, target, expected),
        )?;
        self.disk_fingerprint = saved_fingerprint;
        self.document.mark_saved();
        Ok(())
    }
}

fn replace_expected(
    temporary: &Path,
    target: &Path,
    expected: Option<Fingerprint>,
) -> Result<(), FileError> {
    #[cfg(windows)]
    {
        windows::replace_expected(temporary, target, expected)
    }
    #[cfg(not(windows))]
    {
        if let Some(expected) = expected {
            match fingerprint_file(target) {
                Ok(actual) if actual == expected => {}
                Ok(_) => return Err(FileError::ExternalModification(ExternalState::Changed)),
                Err(FileError::Io(e)) if e.kind() == io::ErrorKind::NotFound => {
                    return Err(FileError::ExternalModification(ExternalState::Missing))
                }
                Err(e) => return Err(e),
            }
        }
        replace_path(temporary, target)
    }
}

#[cfg(test)]
fn atomic_replace_with(
    target: &Path,
    create_only: bool,
    write: impl FnOnce(&mut File) -> io::Result<()>,
) -> Result<(), FileError> {
    atomic_replace_with_commit(target, create_only, write, replace_path)
}

/// Publish an already prepared byte artifact as a new file. A destination
/// created by another process before commit is left intact.
pub fn publish_new_bytes(target: &Path, bytes: &[u8]) -> Result<(), FileError> {
    atomic_replace_with_commit(
        target,
        true,
        |file| file.write_all(bytes),
        |_, _| Err(io::Error::from(io::ErrorKind::AlreadyExists).into()),
    )
}

fn atomic_replace_with_commit(
    target: &Path,
    create_only: bool,
    write: impl FnOnce(&mut File) -> io::Result<()>,
    commit: impl FnOnce(&Path, &Path) -> Result<(), FileError>,
) -> Result<(), FileError> {
    let parent = target
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_name = target.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "El destino no tiene nombre de archivo",
        )
    })?;

    let mut temporary = None;
    for _ in 0..100 {
        let suffix = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(
            ".{}.jcide-save-{}-{suffix}",
            file_name.to_string_lossy(),
            std::process::id()
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => {
                temporary = Some((candidate, file));
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    let (temporary_path, mut temporary_file) = temporary.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "No se pudo reservar un archivo para guardar",
        )
    })?;

    let operation: Result<(), FileError> = (|| {
        if let Ok(metadata) = fs::metadata(target) {
            temporary_file.set_permissions(metadata.permissions())?;
        }
        write(&mut temporary_file)?;
        temporary_file.flush()?;
        temporary_file.sync_all()?;
        drop(temporary_file);
        if create_only {
            commit_new_path(&temporary_path, target)?;
        } else {
            commit(&temporary_path, target)?;
        }
        sync_parent(parent)?;
        Ok(())
    })();

    if operation.is_err() && !matches!(operation, Err(FileError::SaveRecovery { .. })) {
        let _ = fs::remove_file(&temporary_path);
    }
    operation
}

#[cfg(windows)]
fn commit_new_path(temporary: &Path, target: &Path) -> io::Result<()> {
    windows::commit_new_path(temporary, target)
}

#[cfg(not(windows))]
fn commit_new_path(temporary: &Path, target: &Path) -> io::Result<()> {
    fs::hard_link(temporary, target)?;
    fs::remove_file(temporary)?;
    Ok(())
}

#[cfg(unix)]
fn replace_path(temporary: &Path, target: &Path) -> Result<(), FileError> {
    fs::rename(temporary, target).map_err(Into::into)
}

#[cfg(not(any(unix, windows)))]
fn replace_path(temporary: &Path, target: &Path) -> Result<(), FileError> {
    if target.exists() {
        fs::remove_file(target)?;
    }
    fs::rename(temporary, target).map_err(Into::into)
}

#[cfg(unix)]
fn sync_parent(parent: &Path) -> io::Result<()> {
    File::open(parent)?.sync_all()
}

#[cfg(not(unix))]
fn sync_parent(_parent: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST: AtomicU64 = AtomicU64::new(0);

    pub(crate) fn test_dir() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "jc-ide-files-{}-{}",
            std::process::id(),
            NEXT_TEST.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        path
    }

    #[test]
    fn create_only_commit_never_overwrites_an_existing_target() {
        let dir = test_dir();
        let target = dir.join("source.p");
        fs::write(&target, b"external").unwrap();
        let result = atomic_replace_with(&target, true, |file| file.write_all(b"mine"));
        assert!(result.is_err());
        assert_eq!(fs::read(&target).unwrap(), b"external");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failed_writer_never_replaces_the_original() {
        let dir = test_dir();
        let target = dir.join("source.p");
        fs::write(&target, b"original\r\n").unwrap();
        let result = atomic_replace_with(&target, false, |file| {
            file.write_all(b"partial")?;
            Err(io::Error::other("simulated failure"))
        });
        assert!(result.is_err());
        assert_eq!(fs::read(&target).unwrap(), b"original\r\n");
        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .filter(|name| name.to_string_lossy().contains(".jcide-save-"))
            .collect();
        assert!(leftovers.is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn external_change_while_streaming_temporary_is_not_adopted_as_the_expected_version() {
        let dir = test_dir();
        let target = dir.join("source.p");
        fs::write(&target, b"original\r\n").unwrap();
        let expected = Fingerprint::of(b"original\r\n");
        let result = atomic_replace_with_commit(
            &target,
            false,
            |file| {
                file.write_all(b"mine\r\n")?;
                fs::write(&target, b"external\r\n")?;
                Ok(())
            },
            |temp, path| replace_expected(temp, path, Some(expected)),
        );
        assert!(matches!(
            result,
            Err(FileError::ExternalModification(ExternalState::Changed))
        ));
        assert_eq!(fs::read(&target).unwrap(), b"external\r\n");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_file(&target).unwrap();
        fs::remove_dir(&dir).unwrap();
    }
}
