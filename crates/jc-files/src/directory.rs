//! Immediate-child metadata only. Windows enumerates a verified directory handle;
//! pinned no-delete-share ancestors keep that admitted path chain stable.
use std::{
    ffi::OsString,
    fs, io,
    path::{Component, Path},
};

#[derive(Debug)]
pub struct DirectoryEntry {
    pub name: OsString,
    pub is_dir: bool,
}
#[derive(Debug, Default)]
pub struct DirectoryListing {
    pub entries: Vec<DirectoryEntry>,
    pub limited: bool,
    pub examined: usize,
    pub skipped: usize,
}
impl DirectoryListing {
    fn accept(
        &mut self,
        name: OsString,
        is_dir: bool,
        max_entries: usize,
        max_bytes: usize,
        names: &mut usize,
    ) -> bool {
        if name == "."
            || name == ".."
            || name
                .to_str()
                .is_some_and(|s| s.eq_ignore_ascii_case(".git"))
        {
            self.skipped += 1;
            return true;
        }
        if name.is_empty()
            || name.len() > 4096
            || Path::new(&name).components().count() != 1
            || !matches!(
                Path::new(&name).components().next(),
                Some(Component::Normal(_))
            )
        {
            self.skipped += 1;
            return true;
        }
        let count = self.entries.len() + 1;
        if count > max_entries
            || count
                .saturating_mul(std::mem::size_of::<DirectoryEntry>())
                .saturating_add(*names)
                .saturating_add(name.capacity())
                > max_bytes
        {
            self.limited = true;
            return false;
        }
        if self.entries.len() == self.entries.capacity() {
            let reserved = if max_entries == usize::MAX {
                self.entries.try_reserve(1)
            } else {
                self.entries.try_reserve_exact(1)
            };
            if reserved.is_err() {
                self.limited = true;
                return false;
            }
        }
        let charge = self
            .entries
            .capacity()
            .saturating_mul(std::mem::size_of::<DirectoryEntry>())
            + *names
            + name.capacity();
        if charge > max_bytes {
            self.limited = true;
            return false;
        }
        *names += name.capacity();
        self.entries.push(DirectoryEntry { name, is_dir });
        true
    }
}
fn interrupted() -> io::Error {
    io::Error::new(
        io::ErrorKind::Interrupted,
        "La lectura del directorio se canceló o agotó su tiempo",
    )
}
pub fn list_directory_in_root(
    root: &Path,
    path: &Path,
    max_entries: usize,
    max_bytes: usize,
    cancelled: impl FnMut() -> bool,
) -> io::Result<DirectoryListing> {
    if !(1..=2048).contains(&max_entries) || !(1..=512 * 1024).contains(&max_bytes) {
        return Err(io::Error::other("El límite del directorio no es válido"));
    }
    list_directory(root, path, max_entries, max_bytes, cancelled)
}
/// Background enumeration with no row count cutoff. Handle and reparse checks
/// are shared with the interactive browser; memory exhaustion stays explicit.
pub fn list_directory_complete_in_root(
    root: &Path,
    path: &Path,
    cancelled: impl FnMut() -> bool,
) -> io::Result<DirectoryListing> {
    list_directory(root, path, usize::MAX, 64 * 1024 * 1024, cancelled)
}
fn list_directory(
    root: &Path,
    path: &Path,
    max_entries: usize,
    max_bytes: usize,
    mut cancelled: impl FnMut() -> bool,
) -> io::Result<DirectoryListing> {
    if !root.is_absolute()
        || !path.is_absolute()
        || root.as_os_str().len() > 4096
        || path.as_os_str().len() > 4096
    {
        return Err(io::Error::other(
            "La raíz, ruta o límite del directorio no es válido",
        ));
    }
    let relative = path
        .strip_prefix(root)
        .map_err(|_| io::Error::other("El directorio está fuera de la raíz del proyecto"))?;
    if relative.components().count() > 32
        || relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(io::Error::other(
            "Los componentes del directorio no son válidos",
        ));
    }
    if cancelled() {
        return Err(interrupted());
    }
    let listing = platform_list(root, path, max_entries, max_bytes, &mut cancelled)?;
    if cancelled() {
        return Err(interrupted());
    }
    Ok(listing)
}

#[cfg(windows)]
fn platform_list(
    root: &Path,
    path: &Path,
    max_entries: usize,
    max_bytes: usize,
    cancelled: &mut impl FnMut() -> bool,
) -> io::Result<DirectoryListing> {
    use std::os::windows::{
        ffi::OsStringExt,
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    };
    #[link(name = "kernel32")]
    extern "system" {
        fn GetFileInformationByHandleEx(
            handle: *mut std::ffi::c_void,
            class: u32,
            buffer: *mut std::ffi::c_void,
            size: u32,
        ) -> i32;
    }
    // FILE_FULL_DIR_INFO / FileFullDirectory[Restart]Info, documented at:
    // https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_full_dir_info
    // https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-getfileinformationbyhandleex
    // Every ancestor is opened without FILE_SHARE_DELETE, including the root.
    // No fallback to path enumeration if a filesystem does not implement this API.
    let mut ancestors = Vec::new();
    let relative = path
        .strip_prefix(root)
        .map_err(|_| io::Error::other("El directorio quedó fuera de la raíz"))?;
    let mut current = root.to_path_buf();
    for component in std::iter::once(None).chain(relative.components().map(Some)) {
        if cancelled() {
            return Err(interrupted());
        }
        if let Some(component) = component {
            current.push(component.as_os_str());
        }
        let metadata = fs::symlink_metadata(&current)?;
        if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 {
            return Err(io::Error::other(
                "Se excluyen enlaces de directorio y puntos de reanálisis",
            ));
        }
        let file = fs::OpenOptions::new()
            .read(true)
            .share_mode(1 | 2)
            .custom_flags(0x0200_0000 | 0x0020_0000)
            .open(&current)?;
        let metadata = file.metadata()?;
        if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 {
            return Err(io::Error::other(
                "El directorio se convirtió en un punto de reanálisis",
            ));
        }
        crate::verify_file_in_root(root, &file)?;
        ancestors.push(file);
    }
    let file = ancestors
        .last()
        .ok_or_else(|| io::Error::other("Falta el identificador del directorio"))?;
    let mut buffer = vec![0u64; 8192]; // 64KiB, 8-byte aligned, separate OS scratch.
    let mut first = true;
    let mut listing = DirectoryListing::default();
    let mut names = 0;
    'enumerate: loop {
        if cancelled() {
            return Err(interrupted());
        }
        buffer.fill(0);
        let success = unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                if first { 15 } else { 14 },
                buffer.as_mut_ptr().cast(),
                (buffer.len() * 8) as u32,
            )
        };
        first = false;
        if success == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(18) {
                break;
            } // ERROR_NO_MORE_FILES
            return Err(error);
        }
        let bytes =
            unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), buffer.len() * 8) };
        let mut at = 0usize;
        loop {
            if cancelled() {
                return Err(interrupted());
            }
            if bytes.len() - at < 68 {
                return Err(io::Error::other("El registro del directorio está truncado"));
            }
            let word = |offset| {
                u32::from_le_bytes(bytes[at + offset..at + offset + 4].try_into().unwrap()) as usize
            };
            let next = word(0);
            let attributes = word(56);
            let name_bytes = word(60);
            let end = at
                .checked_add(68)
                .and_then(|n| n.checked_add(name_bytes))
                .ok_or_else(|| io::Error::other("Desbordamiento del registro del directorio"))?;
            if name_bytes == 0
                || name_bytes % 2 != 0
                || end > bytes.len()
                || (next != 0
                    && (next % 8 != 0 || next < 68 + name_bytes || next > bytes.len() - at))
            {
                return Err(io::Error::other(
                    "Los límites del registro del directorio no son válidos",
                ));
            }
            listing.examined += 1;
            if max_entries != usize::MAX && listing.examined > 50_000 {
                listing.limited = true;
                break 'enumerate;
            }
            if attributes & (0x400 | 0x40) != 0 || name_bytes / 2 > 4096 {
                listing.skipped += 1;
            } else {
                let units: Vec<u16> = bytes[at + 68..end]
                    .chunks_exact(2)
                    .map(|p| u16::from_le_bytes([p[0], p[1]]))
                    .collect();
                let name = OsString::from_wide(&units);
                if !listing.accept(
                    name,
                    attributes & 0x10 != 0,
                    max_entries,
                    max_bytes,
                    &mut names,
                ) {
                    break 'enumerate;
                }
            }
            if next == 0 {
                break;
            }
            at += next;
        }
    }
    // Recheck the actual anchored directory before publication. Ancestor handles
    // stay alive through this check and are closed by RAII on every exit.
    crate::verify_file_in_root(root, file)?;
    Ok(listing)
}

#[cfg(not(windows))]
fn platform_list(
    root: &Path,
    path: &Path,
    max_entries: usize,
    max_bytes: usize,
    cancelled: &mut impl FnMut() -> bool,
) -> io::Result<DirectoryListing> {
    // Non-Windows compatibility uses metadata revalidation, not anchored name
    // enumeration. It does not claim atomic isolation against namespace changes.
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || !path.canonicalize()?.starts_with(root)
    {
        return Err(io::Error::other(
            "El directorio no es un directorio ordinario del proyecto",
        ));
    }
    let mut listing = DirectoryListing::default();
    let mut names = 0;
    for entry in fs::read_dir(path)? {
        if cancelled() {
            return Err(interrupted());
        }
        listing.examined += 1;
        if max_entries != usize::MAX && listing.examined > 50_000 {
            listing.limited = true;
            break;
        }
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_symlink() || !(kind.is_file() || kind.is_dir()) {
            listing.skipped += 1;
            continue;
        }
        if !listing.accept(
            entry.file_name(),
            kind.is_dir(),
            max_entries,
            max_bytes,
            &mut names,
        ) {
            break;
        }
    }
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || !path.canonicalize()?.starts_with(root)
    {
        return Err(io::Error::other("El directorio cambió durante su lectura"));
    }
    Ok(listing)
}
