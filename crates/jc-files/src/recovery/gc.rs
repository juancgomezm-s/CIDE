//! Bounded obsolete-generation cleanup. Deletion is by pinned Win32 handle,
//! never recursive and never through a filename obtained from document metadata.
use super::*;
use std::collections::BTreeSet;

const GENERATIONS_PER_PASS: usize = 64;
const FILES_PER_PASS: usize = 8192;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PruneReport {
    pub protected_generations: usize,
    pub scanned_generations: usize,
    pub removed_generations: usize,
    pub removed_files: usize,
    pub reclaimed_bytes: u64,
    pub deferred_generations: usize,
    pub conflicted_generations: usize,
    pub interrupted_generations: usize,
    pub last_os_error: Option<i32>,
}
struct DeleteFile {
    handle: File,
    bytes: u64,
}
struct Candidate {
    directory: File,
    files: Vec<DeleteFile>,
}
struct Protected {
    generation: u64,
    _handles: Vec<File>,
}

impl RecoveryStore {
    /// Keep both confirmed copies. A damaged HEAD/BACKUP fails before deletion.
    /// Each pass admits at most64 generations/8192 obsolete file handles; locked
    /// generations are deferred intact and partial removals are resumable.
    pub fn prune_obsolete(&mut self) -> Result<PruneReport, RecoveryError> {
        self.retained_bytes = None;
        if !cfg!(windows) {
            return Err(RecoveryError::Unsupported);
        }
        let _pins = pin_slot(self)?;
        let head = protect(&self.slot, "HEAD", self.limits)?.ok_or(RecoveryError::Corrupt(
            "no existe HEAD confirmado para limpiar copias",
        ))?;
        let backup = protect(&self.slot, "BACKUP", self.limits)?;
        let mut kept = BTreeSet::from([head.generation]);
        if let Some(backup) = &backup {
            kept.insert(backup.generation);
        }
        let mut report = PruneReport {
            protected_generations: kept.len(),
            ..Default::default()
        };
        let mut selected = BTreeSet::new();
        for (examined, entry) in fs::read_dir(&self.slot)?.enumerate() {
            if examined >= MAX_SCAN {
                return Err(RecoveryError::Limit(
                    "demasiadas entradas para limpiar recuperación",
                ));
            }
            let entry = entry?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if !generation_name(&name) {
                continue;
            }
            let generation = name[11..]
                .parse::<u64>()
                .map_err(|_| RecoveryError::UnsafePath)?;
            if generation == 0 {
                return Err(RecoveryError::UnsafePath);
            }
            if kept.contains(&generation) {
                continue;
            }
            report.scanned_generations += 1;
            selected.insert(generation);
            if selected.len() > GENERATIONS_PER_PASS {
                selected.pop_last();
            }
        }
        report.deferred_generations = report.scanned_generations.saturating_sub(selected.len());
        let mut candidates = Vec::with_capacity(selected.len());
        let mut opened = 0;
        // Complete preflight of the admitted batch before marking any file.
        for generation in selected {
            let path = generation_path(&self.slot, generation);
            match candidate(
                &self.slot,
                &path,
                self.limits,
                FILES_PER_PASS - opened,
                None,
            ) {
                Ok(Some(candidate)) => {
                    opened += candidate.files.len();
                    candidates.push(candidate);
                }
                Ok(None) => {
                    report.deferred_generations += 1;
                }
                Err(RecoveryError::Io(error)) if conflict(&error) => {
                    report.conflicted_generations += 1;
                    report.deferred_generations += 1;
                    report.last_os_error = error.raw_os_error();
                }
                Err(error) => return Err(error),
            }
        }
        for candidate in candidates {
            remove_candidate(candidate, &mut report);
        }
        drop(backup);
        drop(head);
        Ok(report)
    }
    /// Caller must have just created this exact generation directory exclusively.
    /// It is armed before any writes and committed immediately after publishing
    /// HEAD. A failed first checkpoint can therefore clean its own unpublished
    /// files even when no earlier HEAD exists. Never use for an arbitrary old ID.
    pub(super) fn unpublished_guard(
        &self,
        generation: u64,
    ) -> Result<UnpublishedGuard, RecoveryError> {
        if generation == 0 || !cfg!(windows) {
            return Err(RecoveryError::UnsafePath);
        }
        let pins = pin_slot(self)?;
        let path = generation_path(&self.slot, generation);
        let directory = open_directory(&path, &self.slot, true)?;
        Ok(UnpublishedGuard {
            slot: self.slot.clone(),
            generation,
            limits: self.limits,
            directory: Some(directory),
            _pins: pins,
            armed: true,
        })
    }
}

pub(super) struct UnpublishedGuard {
    slot: PathBuf,
    generation: u64,
    limits: RecoveryLimits,
    directory: Option<File>,
    _pins: Vec<File>,
    armed: bool,
}
impl UnpublishedGuard {
    pub(super) fn commit(&mut self) {
        self.armed = false;
        drop(self.directory.take());
    }
    fn cleanup(&mut self) -> Result<PruneReport, RecoveryError> {
        let mut controls = Vec::new();
        for name in ["HEAD", "BACKUP"] {
            if let Some((head, handle)) = pinned_head(&self.slot, name, self.limits)? {
                controls.push(handle);
                if head.generation == self.generation {
                    return Ok(PruneReport {
                        protected_generations: 1,
                        ..Default::default()
                    });
                }
            }
        }
        let path = generation_path(&self.slot, self.generation);
        let Some(directory) = self.directory.take() else {
            return Ok(PruneReport::default());
        };
        let mut report = PruneReport {
            scanned_generations: 1,
            ..Default::default()
        };
        match candidate(
            &self.slot,
            &path,
            self.limits,
            self.limits.max_documents + 2,
            Some(directory),
        ) {
            Ok(Some(candidate)) => remove_candidate(candidate, &mut report),
            Ok(None) => report.deferred_generations = 1,
            Err(RecoveryError::Io(error)) if conflict(&error) => {
                report.conflicted_generations = 1;
                report.deferred_generations = 1;
                report.last_os_error = error.raw_os_error();
            }
            Err(error) => return Err(error),
        }
        Ok(report)
    }
}
impl Drop for UnpublishedGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.cleanup();
        }
    }
}

fn pinned_head(
    slot: &Path,
    name: &str,
    limits: RecoveryLimits,
) -> Result<Option<(Head, File)>, RecoveryError> {
    let handle = match read_file(&slot.join(name), slot) {
        Ok(file) => file,
        Err(RecoveryError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(None)
        }
        Err(error) => return Err(error),
    };
    let head = read_head(slot, name, limits)?.ok_or(RecoveryError::Corrupt(
        "el registro de recuperación desapareció",
    ))?;
    Ok(Some((head, handle)))
}
fn protect(
    slot: &Path,
    name: &str,
    limits: RecoveryLimits,
) -> Result<Option<Protected>, RecoveryError> {
    let Some((head, control)) = pinned_head(slot, name, limits)? else {
        return Ok(None);
    };
    let directory = generation_path(slot, head.generation);
    let mut handles = vec![
        control,
        open_directory(&directory, slot, false)?,
        read_file(&directory.join("manifest.bin"), slot)?,
        read_file(&directory.join("metadata.bin"), slot)?,
    ];
    let loaded = load_generation(slot, head, limits, name == "BACKUP")?;
    // Keep immutable data handles, not a second owned copy of session metadata.
    handles.extend(loaded.documents);
    Ok(Some(Protected {
        generation: head.generation,
        _handles: handles,
    }))
}
fn known_file(name: &str, limits: RecoveryLimits) -> bool {
    if matches!(name, "metadata.bin" | "manifest.bin") {
        return true;
    }
    if name.len() != 12
        || !name.starts_with("doc-")
        || !name.ends_with(".bin")
        || !name[4..8].bytes().all(|byte| byte.is_ascii_digit())
    {
        return false;
    }
    name[4..8]
        .parse::<usize>()
        .is_ok_and(|index| index < limits.max_documents)
}
fn candidate(
    slot: &Path,
    path: &Path,
    limits: RecoveryLimits,
    remaining: usize,
    opened: Option<File>,
) -> Result<Option<Candidate>, RecoveryError> {
    let directory = match opened {
        Some(file) => file,
        None => open_directory(path, slot, true)?,
    };
    let resolved = path.canonicalize()?;
    if resolved != path || resolved.parent() != Some(slot) {
        return Err(RecoveryError::UnsafePath);
    }
    let mut files = Vec::new();
    let mut names = BTreeSet::new();
    let mut deferred = false;
    for (examined, entry) in fs::read_dir(path)?.enumerate() {
        if examined >= limits.max_documents + 2 {
            return Err(RecoveryError::Limit(
                "demasiados archivos en generación obsoleta",
            ));
        }
        let entry = entry?;
        let name = entry
            .file_name()
            .to_str()
            .map(str::to_owned)
            .ok_or(RecoveryError::UnsafePath)?;
        if !known_file(&name, limits) || !names.insert(name.clone()) {
            return Err(RecoveryError::UnsafePath);
        }
        let file_path = path.join(&name);
        let metadata = fs::symlink_metadata(&file_path)?;
        if !metadata.is_file() || reparse(&metadata) {
            return Err(RecoveryError::UnsafePath);
        }
        if files.len() >= remaining {
            deferred = true;
            continue;
        }
        let handle = open_delete_file(&file_path, slot)?;
        let bytes = handle.metadata()?.len();
        files.push(DeleteFile { handle, bytes });
    }
    if deferred {
        return Ok(None);
    }
    Ok(Some(Candidate { directory, files }))
}
fn remove_candidate(candidate: Candidate, report: &mut PruneReport) {
    let Candidate { directory, files } = candidate;
    let mut interrupted = false;
    for file in files {
        match delete_handle(file.handle) {
            Ok(()) => {
                report.removed_files += 1;
                report.reclaimed_bytes = report.reclaimed_bytes.saturating_add(file.bytes);
            }
            Err(error) => {
                report.last_os_error = error.raw_os_error();
                interrupted = true;
            }
        }
    }
    if !interrupted {
        match delete_handle(directory) {
            Ok(()) => report.removed_generations += 1,
            Err(error) => {
                report.last_os_error = error.raw_os_error();
                interrupted = true;
            }
        }
    }
    if interrupted {
        report.interrupted_generations += 1;
        report.deferred_generations += 1;
    }
}
fn conflict(error: &io::Error) -> bool {
    matches!(error.raw_os_error(), Some(5 | 32 | 33))
        || matches!(
            error.kind(),
            io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied
        )
}

#[cfg(windows)]
fn pin_slot(store: &RecoveryStore) -> Result<Vec<File>, RecoveryError> {
    use std::os::windows::fs::OpenOptionsExt;
    ensure_directory(&store.slot, false)?;
    let mut current = PathBuf::new();
    let mut pins = Vec::new();
    for component in store.slot.components() {
        current.push(component.as_os_str());
        if matches!(component, Component::Prefix(_)) {
            continue;
        }
        let file = open_directory(&current, &current, false)?;
        pins.push(file);
    }
    // Attributes-only access is compatible with the existing share0 lease.
    // Compare stable volume/file identity after pinning the whole path chain.
    let lease_path = store.slot.join("lease.lock");
    let metadata = fs::symlink_metadata(&lease_path)?;
    if !metadata.is_file() || reparse(&metadata) {
        return Err(RecoveryError::UnsafePath);
    }
    let lease = OpenOptions::new()
        .access_mode(0x80)
        .share_mode(7)
        .custom_flags(0x0020_0000)
        .open(lease_path)?;
    if native::identity(&lease)? != native::identity(&store._lease)? {
        return Err(RecoveryError::UnsafePath);
    }
    pins.push(lease);
    Ok(pins)
}
#[cfg(not(windows))]
fn pin_slot(_: &RecoveryStore) -> Result<Vec<File>, RecoveryError> {
    Err(RecoveryError::Unsupported)
}
#[cfg(windows)]
fn open_directory(path: &Path, root: &Path, delete: bool) -> Result<File, RecoveryError> {
    use std::os::windows::fs::OpenOptionsExt;
    ensure_directory(path, false)?;
    let file = OpenOptions::new()
        .access_mode(0x80 | if delete { 0x10000 } else { 0 })
        .share_mode(3)
        .custom_flags(0x0200_0000 | 0x0020_0000)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_dir() || reparse(&metadata) {
        return Err(RecoveryError::UnsafePath);
    }
    crate::verify_file_in_root(root, &file)?;
    Ok(file)
}
#[cfg(not(windows))]
fn open_directory(_: &Path, _: &Path, _: bool) -> Result<File, RecoveryError> {
    Err(RecoveryError::Unsupported)
}
#[cfg(windows)]
fn open_delete_file(path: &Path, root: &Path) -> Result<File, RecoveryError> {
    use std::os::windows::fs::OpenOptionsExt;
    let file = OpenOptions::new()
        .access_mode(0x10000 | 0x80)
        .share_mode(0)
        .custom_flags(0x0020_0000)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || reparse(&metadata) || native::links(&file)? != 1 {
        return Err(RecoveryError::UnsafePath);
    }
    crate::verify_file_in_root(root, &file)?;
    Ok(file)
}
#[cfg(not(windows))]
fn open_delete_file(_: &Path, _: &Path) -> Result<File, RecoveryError> {
    Err(RecoveryError::Unsupported)
}
fn delete_handle(file: File) -> io::Result<()> {
    #[cfg(windows)]
    {
        native::delete(&file)?;
        drop(file);
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = file;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Borrado seguro por identificador no disponible",
        ))
    }
}

#[cfg(windows)]
mod native {
    use std::{ffi::c_void, fs::File, io, os::windows::io::AsRawHandle};
    #[repr(C)]
    #[derive(Default)]
    struct Information {
        attributes: u32,
        creation: [u32; 2],
        access: [u32; 2],
        write: [u32; 2],
        volume: u32,
        size_high: u32,
        size_low: u32,
        links: u32,
        index_high: u32,
        index_low: u32,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetFileInformationByHandle(file: *mut c_void, information: *mut Information) -> i32;
        fn SetFileInformationByHandle(
            file: *mut c_void,
            class: i32,
            information: *mut c_void,
            size: u32,
        ) -> i32;
    }
    fn info(file: &File) -> io::Result<Information> {
        let mut info = Information::default();
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(info)
    }
    pub(super) fn identity(file: &File) -> io::Result<(u32, u64)> {
        let info = info(file)?;
        Ok((
            info.volume,
            (u64::from(info.index_high) << 32) | u64::from(info.index_low),
        ))
    }
    pub(super) fn links(file: &File) -> io::Result<u32> {
        Ok(info(file)?.links)
    }
    pub(super) fn delete(file: &File) -> io::Result<()> {
        // FILE_DISPOSITION_INFO::DeleteFile is a BOOLEAN (one byte), class4.
        // https://learn.microsoft.com/windows/win32/api/fileapi/nf-fileapi-setfileinformationbyhandle
        // https://learn.microsoft.com/windows/win32/api/winbase/ns-winbase-file_disposition_info
        let mut delete = 1u8;
        if unsafe {
            SetFileInformationByHandle(file.as_raw_handle(), 4, (&mut delete as *mut u8).cast(), 1)
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}
