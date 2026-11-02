//! Opt-in Windows save probe; touches only its own temporary directory.
#[cfg(windows)]
fn main() {
    use jc_files::{FileError, FileSession};
    use jc_text::TextEncoding;
    use std::{fs, os::windows::fs::OpenOptionsExt};
    let root = std::env::temp_dir().join(format!("jc-save-probe-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let path = root.join("source.p");
    println!("scenario,attempts,errors,codes,original_preserved,dirty_preserved");
    for mode in 0..3 {
        let mut codes = std::collections::BTreeMap::new();
        let mut errors = 0;
        let mut preserved = true;
        let mut dirty = true;
        for _ in 0..100 {
            fs::write(&path, b"original\r\n").unwrap();
            let mut session = FileSession::open(&path, Some(TextEncoding::Utf8)).unwrap();
            session.insert_text(0, "new ").unwrap();
            let reader = (mode == 1).then(|| {
                fs::OpenOptions::new()
                    .read(true)
                    .share_mode(3)
                    .open(&path)
                    .unwrap()
            });
            let mapping = (mode == 2).then(|| Mapping::new(&path));
            if let Err(error) = session.save() {
                errors += 1;
                if let FileError::Io(ref io) = error {
                    *codes.entry(io.raw_os_error().unwrap_or(-1)).or_insert(0) += 1;
                }
                preserved &= fs::read(&path).unwrap() == b"original\r\n";
                dirty &= session.document().is_modified();
            }
            drop(reader);
            drop(mapping);
        }
        println!(
            "{},{},{},{:?},{preserved},{dirty}",
            ["uncontended", "reader_denies_delete", "mapped_target"][mode],
            100,
            errors,
            codes
        );
    }
    fs::remove_file(path).unwrap();
    fs::remove_dir(root).unwrap();
}
#[cfg(windows)]
struct Mapping {
    handle: *mut std::ffi::c_void,
    view: *mut std::ffi::c_void,
}
#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn CreateFileMappingW(
        file: *mut std::ffi::c_void,
        attributes: *mut std::ffi::c_void,
        protect: u32,
        high: u32,
        low: u32,
        name: *const u16,
    ) -> *mut std::ffi::c_void;
    fn MapViewOfFile(
        mapping: *mut std::ffi::c_void,
        access: u32,
        high: u32,
        low: u32,
        bytes: usize,
    ) -> *mut std::ffi::c_void;
    fn UnmapViewOfFile(view: *mut std::ffi::c_void) -> i32;
    fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
}
#[cfg(windows)]
impl Mapping {
    fn new(path: &std::path::Path) -> Self {
        use std::os::windows::io::AsRawHandle;
        let file = std::fs::File::open(path).unwrap();
        let handle = unsafe {
            CreateFileMappingW(
                file.as_raw_handle(),
                std::ptr::null_mut(),
                2,
                0,
                0,
                std::ptr::null(),
            )
        };
        assert!(!handle.is_null());
        let view = unsafe { MapViewOfFile(handle, 4, 0, 0, 0) };
        assert!(!view.is_null());
        Self { handle, view }
    }
}
#[cfg(windows)]
impl Drop for Mapping {
    fn drop(&mut self) {
        unsafe {
            UnmapViewOfFile(self.view);
            CloseHandle(self.handle);
        }
    }
}
#[cfg(not(windows))]
fn main() {
    eprintln!("Windows-only save probe");
}
