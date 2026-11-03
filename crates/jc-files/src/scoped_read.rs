//! Confined, byte-preserving project reads shared by search and explicit opening.
//! root must be the canonical absolute root admitted by the caller. Handle
//! validation precedes reading payload bytes; final path is not display authority.
use std::{
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
};
fn reparse(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}
fn read_retry(file: &mut File, buffer: &mut [u8]) -> io::Result<usize> {
    loop {
        match file.read(buffer) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => return result,
        }
    }
}
pub fn read_in_root(root: &Path, path: &Path, limit: usize) -> io::Result<(PathBuf, Vec<u8>)> {
    if !root.is_absolute() {
        return Err(io::Error::other(
            "Project root must be canonical and absolute",
        ));
    }
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || reparse(&metadata) {
        return Err(io::Error::other("Links/reparse points are excluded"));
    }
    let resolved = path.canonicalize()?;
    if !resolved.starts_with(root) {
        return Err(io::Error::other("File is outside the search root"));
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000);
    }
    let file = options.open(&resolved)?;
    if reparse(&file.metadata()?) {
        return Err(io::Error::other("File became a reparse point"));
    }
    verify_file_in_root(root, &file)?;
    if file.metadata()?.len() > limit as u64 {
        return Err(io::Error::other("File exceeds the search byte budget"));
    }
    let initial = usize::try_from(file.metadata()?.len())
        .unwrap_or(limit)
        .min(limit);
    let mut file = file;
    let mut bytes = vec![0; initial];
    let mut filled = 0;
    loop {
        while filled < bytes.len() {
            let count = read_retry(&mut file, &mut bytes[filled..])?;
            if count == 0 {
                bytes.truncate(filled);
                return Ok((resolved, bytes));
            }
            filled += count;
        }
        // Probe growth in a stack byte: a full Vec must not double just to read
        // the sentinel beyond its admitted payload budget.
        let mut guard = [0u8; 1];
        if read_retry(&mut file, &mut guard)? == 0 {
            return Ok((resolved, bytes));
        }
        if filled == limit {
            return Err(io::Error::other("File grew beyond the search byte budget"));
        }
        let target = bytes.len().saturating_mul(2).max(filled + 1).min(limit);
        bytes.reserve_exact(target - bytes.len());
        bytes.resize(target, 0);
        if bytes.capacity() > limit {
            return Err(io::Error::other(
                "File buffer exceeds the search byte budget",
            ));
        }
        bytes[filled] = guard[0];
        filled += 1;
    }
}
#[cfg(windows)]
pub fn verify_file_in_root(root: &Path, file: &File) -> io::Result<()> {
    use std::os::windows::{ffi::OsStringExt, io::AsRawHandle};
    #[link(name = "kernel32")]
    extern "system" {
        fn GetFinalPathNameByHandleW(
            handle: *mut std::ffi::c_void,
            path: *mut u16,
            length: u32,
            flags: u32,
        ) -> u32;
    }
    let mut buffer = vec![0u16; 32768];
    let len = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle(),
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            0,
        )
    };
    if len == 0 {
        return Err(io::Error::last_os_error());
    }
    if len >= buffer.len() as u32 {
        return Err(io::Error::other("Resolved path too long"));
    }
    let path = PathBuf::from(std::ffi::OsString::from_wide(&buffer[..len as usize]));
    if !path.starts_with(root) {
        return Err(io::Error::other("Opened handle escaped the search root"));
    }
    Ok(())
}
#[cfg(target_os = "linux")]
pub fn verify_file_in_root(root: &Path, file: &File) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    if !fs::read_link(format!("/proc/self/fd/{}", file.as_raw_fd()))?.starts_with(root) {
        return Err(io::Error::other("Opened handle escaped the search root"));
    }
    Ok(())
}
#[cfg(not(any(windows, target_os = "linux")))]
pub fn verify_file_in_root(_: &Path, _: &File) -> io::Result<()> {
    Err(io::Error::other(
        "Secure project file traversal not supported on this platform yet",
    ))
}
