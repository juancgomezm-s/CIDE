//! File lifecycle for JC-IDE.
//!
//! A FileSession owns the byte-preserving document plus the selected text
//! encoding. Saving verifies that the on-disk content still matches the version
//! opened/saved by the session, writes a same-directory temporary file, flushes
//! it, and replaces the target atomically on supported platforms.

use jc_core::{Document, EditError};
use jc_text::{encode, inspect_text, EncodeError, EncodingSource, TextEncoding, TextInspection};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Fingerprint {
    len: u64,
    first: u64,
    second: u64,
}

impl Fingerprint {
    fn of(bytes: &[u8]) -> Self {
        let mut first = 0xcbf29ce484222325u64;
        let mut second = 0x9e3779b97f4a7c15u64;
        for byte in bytes {
            first ^= u64::from(*byte);
            first = first.wrapping_mul(0x100000001b3);
            second ^= u64::from(*byte).wrapping_add(0x9e3779b97f4a7c15);
            second = second.rotate_left(13).wrapping_mul(0xbf58476d1ce4e5b9);
        }
        Self {
            len: bytes.len() as u64,
            first,
            second,
        }
    }
}

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
    Encode(EncodeError),
    Edit(EditError),
}

impl fmt::Display for FileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::ExternalModification(ExternalState::Changed) => {
                write!(f, "file changed on disk since it was opened or saved")
            }
            Self::ExternalModification(ExternalState::Missing) => {
                write!(f, "file was removed from disk since it was opened or saved")
            }
            Self::ExternalModification(ExternalState::Unchanged) => {
                write!(f, "unexpected unchanged external state")
            }
            Self::EncodingRequired => {
                write!(f, "an explicit encoding is required for text insertion")
            }
            Self::Encode(error) => write!(f, "{error}"),
            Self::Edit(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for FileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Encode(error) => Some(error),
            Self::Edit(error) => Some(error),
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
        let inspection = inspect_text(&bytes, explicit);
        Ok(Self {
            path,
            document: Document::from_bytes(bytes.clone()),
            encoding: inspection.probe.encoding,
            encoding_source: inspection.probe.source,
            disk_fingerprint: Some(Fingerprint::of(&bytes)),
        })
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

    pub fn inspection(&self) -> TextInspection {
        let bytes = self.document.to_bytes();
        let mut inspection = inspect_text(&bytes, self.encoding);
        inspection.probe.source = self.encoding_source;
        inspection
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
        match (self.disk_fingerprint, fs::read(&self.path)) {
            (Some(expected), Ok(bytes)) => Ok(if Fingerprint::of(&bytes) == expected {
                ExternalState::Unchanged
            } else {
                ExternalState::Changed
            }),
            (Some(_), Err(error)) if error.kind() == io::ErrorKind::NotFound => {
                Ok(ExternalState::Missing)
            }
            (None, Err(error)) if error.kind() == io::ErrorKind::NotFound => {
                Ok(ExternalState::Unchanged)
            }
            (None, Ok(_)) => Ok(ExternalState::Changed),
            (_, Err(error)) => Err(FileError::Io(error)),
        }
    }

    pub fn save(&mut self) -> Result<(), FileError> {
        let state = self.external_state()?;
        if state != ExternalState::Unchanged {
            return Err(FileError::ExternalModification(state));
        }
        let create_only = self.disk_fingerprint.is_none();
        self.write_current(create_only)
    }

    pub fn force_save(&mut self) -> Result<(), FileError> {
        self.write_current(false)
    }

    fn write_current(&mut self, create_only: bool) -> Result<(), FileError> {
        let bytes = self.document.to_bytes();
        atomic_replace_with(&self.path, create_only, |file| file.write_all(&bytes))?;
        self.disk_fingerprint = Some(Fingerprint::of(&bytes));
        self.document.mark_saved();
        Ok(())
    }
}

fn atomic_replace_with(
    target: &Path,
    create_only: bool,
    write: impl FnOnce(&mut File) -> io::Result<()>,
) -> io::Result<()> {
    let parent = target
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_name = target
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "target has no file name"))?;

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
            Err(error) => return Err(error),
        }
    }
    let (temporary_path, mut temporary_file) = temporary.ok_or_else(|| {
        io::Error::new(io::ErrorKind::AlreadyExists, "unable to allocate save file")
    })?;

    let operation = (|| {
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
            replace_path(&temporary_path, target)?;
        }
        sync_parent(parent)?;
        Ok(())
    })();

    if operation.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    operation
}

fn commit_new_path(temporary: &Path, target: &Path) -> io::Result<()> {
    fs::hard_link(temporary, target)?;
    fs::remove_file(temporary)?;
    Ok(())
}

#[cfg(unix)]
fn replace_path(temporary: &Path, target: &Path) -> io::Result<()> {
    fs::rename(temporary, target)
}

#[cfg(windows)]
fn replace_path(temporary: &Path, target: &Path) -> io::Result<()> {
    if !target.exists() {
        return fs::rename(temporary, target);
    }
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;

    #[link(name = "kernel32")]
    extern "system" {
        fn ReplaceFileW(
            replaced_file_name: *const u16,
            replacement_file_name: *const u16,
            backup_file_name: *const u16,
            replace_flags: u32,
            exclude: *mut c_void,
            reserved: *mut c_void,
        ) -> i32;
    }

    let replaced: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    let replacement: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
    let ok = unsafe {
        ReplaceFileW(
            replaced.as_ptr(),
            replacement.as_ptr(),
            ptr::null(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(any(unix, windows)))]
fn replace_path(temporary: &Path, target: &Path) -> io::Result<()> {
    if target.exists() {
        fs::remove_file(target)?;
    }
    fs::rename(temporary, target)
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

    fn test_dir() -> PathBuf {
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
}
