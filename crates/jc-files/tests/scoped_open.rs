#![cfg(any(windows, target_os = "linux"))]
use jc_files::{verify_file_in_root, FileSession};
use jc_text::TextEncoding;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    root: PathBuf,
    workspace: PathBuf,
    base: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .canonicalize()
            .unwrap();
        let base = workspace.join("work/test-temp");
        fs::create_dir_all(&base).unwrap();
        let base = base.canonicalize().unwrap();
        assert!(base.starts_with(&workspace));
        let root = base.join(format!(
            "scoped-open-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self {
            root,
            workspace,
            base,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Ok(target) = self.root.canonicalize() {
            if target == self.root
                && target.parent() == Some(self.base.as_path())
                && target.starts_with(&self.workspace)
                && target != self.workspace
                && target
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with("scoped-open-"))
            {
                let _ = fs::remove_dir_all(&target);
            }
        }
    }
}

#[test]
fn confined_open_reads_exact_bytes_and_budget_guards_without_modifying_disk() {
    let f = Fixture::new();
    let root = f.root.join("inside");
    fs::create_dir(&root).unwrap();
    let path = root.join("source.p");
    let bytes: Vec<u8> = (0..=255).collect();
    fs::write(&path, &bytes).unwrap();
    let session = FileSession::open_in_root(&root, &path, None, bytes.len()).unwrap();
    assert_eq!(session.document().to_bytes(), bytes);
    assert_eq!(session.path(), path.canonicalize().unwrap());
    assert!(FileSession::open_in_root(&root, &path, None, bytes.len() - 1).is_err());
    assert_eq!(fs::read(&path).unwrap(), bytes);
    let empty = root.join("empty");
    fs::write(&empty, []).unwrap();
    assert!(
        FileSession::open_in_root(&root, &empty, Some(TextEncoding::Utf8), 0)
            .unwrap()
            .document()
            .is_empty()
    );
}

#[test]
fn outside_paths_and_actual_outside_handles_are_rejected() {
    let f = Fixture::new();
    let root = f.root.join("inside");
    fs::create_dir(&root).unwrap();
    let outside = f.root.join("outside.txt");
    fs::write(&outside, b"outside").unwrap();
    assert!(FileSession::open_in_root(&root, &outside, Some(TextEncoding::Utf8), 1024).is_err());
    assert!(FileSession::open_in_root(&root, root.join("../outside.txt"), None, 1024).is_err());
    let file = fs::File::open(&outside).unwrap();
    assert!(verify_file_in_root(&root.canonicalize().unwrap(), &file).is_err());
    assert_eq!(fs::read(&outside).unwrap(), b"outside");
}
