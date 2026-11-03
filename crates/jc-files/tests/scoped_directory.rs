use jc_files::list_directory_in_root;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    root: PathBuf,
    base: PathBuf,
    files: Vec<PathBuf>,
    dirs: Vec<PathBuf>,
    links: Vec<PathBuf>,
}
fn reparse(path: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return true;
    };
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
impl Fixture {
    fn new() -> Self {
        let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let base = workspace.join("work/test-temp");
        fs::create_dir_all(&base).unwrap();
        let base = base.canonicalize().unwrap();
        assert!(base.starts_with(&workspace) && !reparse(&base));
        let root = base.join(format!(
            "cide-directory-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self {
            root: root.canonicalize().unwrap(),
            base,
            files: Vec::new(),
            dirs: Vec::new(),
            links: Vec::new(),
        }
    }
    fn file(&mut self, name: impl AsRef<Path>) -> PathBuf {
        let path = self.root.join(name);
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        file.write_all(b"bytes\xff\r\n").unwrap();
        self.files.push(path.clone());
        path
    }
    fn dir(&mut self, name: &str) -> PathBuf {
        let path = self.root.join(name);
        fs::create_dir(&path).unwrap();
        self.dirs.push(path.clone());
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if reparse(&self.root)
            || !self.root.starts_with(&self.base)
            || self.root.parent() != Some(self.base.as_path())
        {
            return;
        }
        for path in &self.links {
            if path.starts_with(&self.root) && reparse(path) {
                #[cfg(windows)]
                let _ = fs::remove_dir(path);
                #[cfg(not(windows))]
                let _ = fs::remove_file(path);
            }
        }
        for path in self.files.iter().rev() {
            if path.starts_with(&self.root) && !reparse(path) && path.is_file() {
                let _ = fs::remove_file(path);
            }
        }
        for path in self.dirs.iter().rev() {
            if path.starts_with(&self.root) && !reparse(path) {
                let _ = fs::remove_dir(path);
            }
        }
        let _ = fs::remove_dir(&self.root);
    }
}
#[test]
fn ordinary_native_names_are_exact_and_listing_limits_are_explicit() {
    let mut f = Fixture::new();
    let path = f.file("niño.txt");
    f.file("z.txt");
    f.dir("folder");
    f.dir(".git");
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        let raw = std::ffi::OsString::from_wide(&[b'x' as u16, 0xd800, b'.' as u16, b'p' as u16]);
        f.file(PathBuf::from(raw.clone()));
        let listing = list_directory_in_root(&f.root, &f.root, 2048, 512 * 1024, || false).unwrap();
        assert!(listing.entries.iter().any(|e| e.name == raw));
    }
    let listing = list_directory_in_root(&f.root, &f.root, 2048, 512 * 1024, || false).unwrap();
    assert!(!listing.limited);
    assert!(listing
        .entries
        .iter()
        .any(|e| e.name == "niño.txt" && !e.is_dir));
    assert!(listing
        .entries
        .iter()
        .any(|e| e.name == "folder" && e.is_dir));
    assert!(!listing.entries.iter().any(|e| e.name == ".git"));
    let small = list_directory_in_root(&f.root, &f.root, 1, 512 * 1024, || false).unwrap();
    assert!(small.limited && small.entries.len() <= 1);
    assert_eq!(fs::read(path).unwrap(), b"bytes\xff\r\n");
    let err = list_directory_in_root(&f.root, &f.root, 2048, 512 * 1024, || true).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::Interrupted);
}
#[test]
fn reparse_entries_and_root_escapes_never_become_directory_targets() {
    let mut f = Fixture::new();
    let mut outside = Fixture::new();
    outside.file("external.txt");
    let link = f.root.join("redirect");
    #[cfg(windows)]
    {
        let result = std::process::Command::new("cmd.exe")
            .args(["/c", "mklink", "/J"])
            .arg(&link)
            .arg(&outside.root)
            .output()
            .unwrap();
        assert!(result.status.success(), "fixture junction setup failed");
    }
    #[cfg(not(windows))]
    std::os::unix::fs::symlink(&outside.root, &link).unwrap();
    f.links.push(link.clone());
    let listing = list_directory_in_root(&f.root, &f.root, 2048, 512 * 1024, || false).unwrap();
    assert!(!listing.entries.iter().any(|e| e.name == "redirect"));
    assert!(list_directory_in_root(&f.root, &link, 2048, 512 * 1024, || false).is_err());
    assert!(list_directory_in_root(&f.root, &outside.root, 2048, 512 * 1024, || false).is_err());
    assert_eq!(
        fs::read(outside.root.join("external.txt")).unwrap(),
        b"bytes\xff\r\n"
    );
}
#[cfg(windows)]
#[test]
fn enumerated_directory_identity_is_pinned_during_the_os_read() {
    let mut f = Fixture::new();
    let dir = f.dir("child");
    f.file("child/entry.txt");
    let target = f.root.join("renamed");
    assert!(target.starts_with(&f.root));
    let mut calls = 0;
    let mut rename = None;
    let listing = list_directory_in_root(&f.root, &dir, 2048, 512 * 1024, || {
        calls += 1;
        if calls >= 4 && rename.is_none() {
            rename = Some(fs::rename(&dir, &target));
        }
        false
    })
    .unwrap();
    assert!(
        rename.is_some_and(|r| r.is_err()),
        "directory rename must not replace the admitted identity during enumeration"
    );
    assert!(listing.entries.iter().any(|e| e.name == "entry.txt"));
    assert!(dir.exists() && !target.exists());
}
