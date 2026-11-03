//! Save intentions survive dialogs without borrowing or refocusing the editor.
use jc_app::{DocumentId, PaneId, Workbench};
use jc_text::{TextEncoding, TextPosition};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
const NAMES: [&str; 6] = [
    "utf8.txt",
    "legacy.txt",
    "generated.txt",
    "chosen.txt",
    "renamed.txt",
    "rejected.txt",
];

struct Fixture {
    workspace: PathBuf,
    base: PathBuf,
    root: PathBuf,
}
fn ordinary(path: &Path) -> std::io::Result<bool> {
    let metadata = fs::symlink_metadata(path)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        Ok(metadata.file_attributes() & 0x400 == 0)
    }
    #[cfg(not(windows))]
    {
        Ok(!metadata.file_type().is_symlink())
    }
}
fn ensure_dir(path: &Path) {
    match fs::create_dir(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => panic!("create test directory: {error}"),
    }
    assert!(ordinary(path).unwrap());
    assert!(path.is_dir());
}
impl Fixture {
    fn new() -> Self {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .unwrap()
            .canonicalize()
            .unwrap();
        let work = workspace.join("work");
        ensure_dir(&work);
        let work = work.canonicalize().unwrap();
        assert!(work.starts_with(&workspace));
        let base = work.join("test-temp");
        ensure_dir(&base);
        let base = base.canonicalize().unwrap();
        assert_eq!(base.parent(), Some(work.as_path()));
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = base.join(format!(
            "save-targets-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self {
            workspace,
            base,
            root: root.canonicalize().unwrap(),
        }
    }
    fn path(&self, name: &str) -> PathBuf {
        assert!(NAMES.contains(&name));
        self.root.join(name)
    }
    fn write(&self, name: &str, bytes: &[u8]) {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.path(name))
            .unwrap();
        for chunk in bytes.chunks(64 * 1024) {
            file.write_all(chunk).unwrap();
        }
    }
    fn cleanup(&self) -> std::io::Result<()> {
        if self.root.canonicalize()? != self.root
            || self.root.parent() != Some(self.base.as_path())
            || !self.base.canonicalize()?.starts_with(&self.workspace)
            || !ordinary(&self.root)?
        {
            return Err(std::io::Error::other("fixture ownership changed"));
        }
        // Validate every exact, known leaf before deleting any. Unknown recovery
        // files are deliberately retained by the final non-recursive remove_dir.
        for name in NAMES {
            let path = self.path(name);
            match fs::symlink_metadata(&path) {
                Ok(metadata) => {
                    if !metadata.is_file()
                        || !ordinary(&path)?
                        || path.canonicalize()?.parent() != Some(self.root.as_path())
                    {
                        return Err(std::io::Error::other("fixture leaf ownership changed"));
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        for name in NAMES {
            match fs::remove_file(self.path(name)) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        fs::remove_dir(&self.root)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(error) = self.cleanup() {
            eprintln!("retained fixture {}: {error}", self.root.display());
        }
    }
}
fn active(w: &Workbench) -> (DocumentId, u64) {
    let document = w.active_document().unwrap();
    (document.id, document.session.document().revision())
}
fn bytes(w: &Workbench, id: DocumentId) -> Vec<u8> {
    w.document_by_id(id).unwrap().session.document().to_bytes()
}

#[test]
fn save_one_captured_document_then_cancel_preserves_other_sources_views_and_undo() {
    let fixture = Fixture::new();
    let original = b"\xef\xbb\xbfalpha\r\nbeta\nend\r";
    let legacy = b"ni\xf1o\r\nfin\n";
    fixture.write("utf8.txt", original);
    fixture.write("legacy.txt", legacy);
    let mut w = Workbench::default();
    let first_tab = w
        .open_file(fixture.path("utf8.txt"), Some(TextEncoding::Utf8))
        .unwrap();
    w.go_to_position(TextPosition { line: 0, column: 0 })
        .unwrap();
    w.insert_text("X").unwrap();
    let (first, _) = active(&w);
    w.open_file(fixture.path("legacy.txt"), Some(TextEncoding::Windows1252))
        .unwrap();
    w.go_to_position(TextPosition { line: 0, column: 0 })
        .unwrap();
    w.insert_text("ñ").unwrap();
    let (second, _) = active(&w);
    let new_tab = w.new_file(fixture.path("generated.txt"), TextEncoding::Utf8);
    w.insert_text("new\r\ntext\n").unwrap();
    let (third, _) = active(&w);
    assert!(w.split_editor_right());
    assert!(w.focus_editor_pane(PaneId::Secondary));
    w.move_home(false).unwrap();
    w.move_right(true).unwrap();
    let metadata: Vec<_> = w.modified_documents().collect();
    assert_eq!(metadata.len(), 3);
    assert!(metadata.contains(&(
        first,
        w.document_by_id(first)
            .unwrap()
            .session
            .document()
            .revision(),
        false
    )));
    assert!(metadata.iter().any(|&(id, _, new)| id == second && !new));
    assert!(metadata.iter().any(|&(id, _, new)| id == third && new));
    let targets: Vec<_> = metadata
        .iter()
        .map(|&(id, rev, _)| w.capture_save_target(id, rev).unwrap())
        .collect();
    let index = metadata.iter().position(|&(id, _, _)| id == first).unwrap();
    let mut targets: Vec<_> = targets.into_iter().map(Some).collect();
    let before_views = format!("{:?}", w.panes());
    let before_bytes = [bytes(&w, first), bytes(&w, second), bytes(&w, third)];
    w.save_target(targets[index].take().unwrap(), None).unwrap();
    // Simulate cancelling the following native dialog: never invoke its save.
    drop(targets);
    assert_eq!(format!("{:?}", w.panes()), before_views);
    assert_eq!(w.tabs().active_id(), Some(new_tab));
    assert_eq!(
        [bytes(&w, first), bytes(&w, second), bytes(&w, third)],
        before_bytes
    );
    assert_eq!(
        fs::read(fixture.path("utf8.txt")).unwrap(),
        b"\xef\xbb\xbfXalpha\r\nbeta\nend\r"
    );
    assert_eq!(fs::read(fixture.path("legacy.txt")).unwrap(), legacy);
    assert!(!fixture.path("generated.txt").exists());
    let remaining: Vec<_> = w.modified_documents().map(|(id, _, _)| id).collect();
    assert_eq!(remaining.len(), 2);
    assert!(remaining.contains(&second) && remaining.contains(&third));
    assert!(w.jump_to_location(first_tab, 3));
    w.undo().unwrap();
    assert_eq!(w.active_bytes().unwrap(), original);
    w.redo().unwrap();
    assert_eq!(
        w.active_bytes().unwrap(),
        b"\xef\xbb\xbfXalpha\r\nbeta\nend\r"
    );
}

#[test]
fn stale_revision_renamed_path_and_closed_identity_are_rejected_before_destination_io() {
    let fixture = Fixture::new();
    let original = b"start\r\nend\n";
    fixture.write("utf8.txt", original);
    let mut w = Workbench::default();
    w.open_file(fixture.path("utf8.txt"), Some(TextEncoding::Utf8))
        .unwrap();
    let (id, rev) = active(&w);
    let stale_revision = w.capture_save_target(id, rev).unwrap();
    w.insert_text("edit").unwrap();
    let views = format!("{:?}", w.panes());
    let edited = w.active_bytes().unwrap();
    assert!(w
        .save_target(stale_revision, Some(fixture.path("rejected.txt")))
        .is_err());
    assert_eq!(w.active_bytes().unwrap(), edited);
    assert_eq!(format!("{:?}", w.panes()), views);
    assert!(!fixture.path("rejected.txt").exists());
    assert_eq!(fs::read(fixture.path("utf8.txt")).unwrap(), original);

    let (_, rev) = active(&w);
    let stale_path = w.capture_save_target(id, rev).unwrap();
    w.save_active_as(fixture.path("renamed.txt")).unwrap();
    assert_eq!(active(&w), (id, rev)); // A save changes identity metadata, not revision.
    let views = format!("{:?}", w.panes());
    assert!(w
        .save_target(stale_path, Some(fixture.path("rejected.txt")))
        .is_err());
    assert_eq!(format!("{:?}", w.panes()), views);
    assert!(!fixture.path("rejected.txt").exists());
    assert_eq!(fs::read(fixture.path("renamed.txt")).unwrap(), edited);
    assert_eq!(fs::read(fixture.path("utf8.txt")).unwrap(), original);
    let closed = w.capture_save_target(id, rev).unwrap();
    assert_eq!(w.close_active(true).unwrap(), None);
    assert!(w.document_by_id(id).is_none());
    assert!(w
        .save_target(closed, Some(fixture.path("rejected.txt")))
        .is_err());
    assert!(!fixture.path("rejected.txt").exists());
    assert!(w.capture_save_target(id, rev).is_err());
}

#[test]
fn new_document_requires_explicit_destination_and_save_as_preserves_geometry_and_history() {
    let fixture = Fixture::new();
    let mut w = Workbench::default();
    let tab = w.new_file(fixture.path("generated.txt"), TextEncoding::Windows1252);
    w.insert_text("niño\r\nsecond\nlast\r").unwrap();
    w.go_to_position(TextPosition { line: 1, column: 2 })
        .unwrap();
    w.move_right(true).unwrap();
    let (id, rev) = active(&w);
    let target = w.capture_save_target(id, rev).unwrap();
    let views = format!("{:?}", w.panes());
    let original = w.active_bytes().unwrap();
    assert!(w.save_target(target, None).is_err());
    assert!(!fixture.path("generated.txt").exists());
    assert_eq!(w.active_bytes().unwrap(), original);
    assert_eq!(format!("{:?}", w.panes()), views);
    assert_eq!(active(&w), (id, rev));
    assert!(w.modified_documents().any(|item| item == (id, rev, true)));
    let target = w.capture_save_target(id, rev).unwrap();
    w.save_target(target, Some(fixture.path("chosen.txt")))
        .unwrap();
    assert_eq!(format!("{:?}", w.panes()), views);
    assert_eq!(active(&w), (id, rev));
    assert_eq!(w.active_path(), Some(fixture.path("chosen.txt").as_path()));
    assert_eq!(
        fs::read(fixture.path("chosen.txt")).unwrap(),
        b"ni\xf1o\r\nsecond\nlast\r"
    );
    assert!(!fixture.path("generated.txt").exists());
    assert!(!w.document_by_id(id).unwrap().session.is_new());
    assert_eq!(w.modified_documents().count(), 0);
    let metadata = w
        .tabs()
        .tabs()
        .iter()
        .find(|entry| entry.id == tab)
        .unwrap();
    assert_eq!(metadata.title, "chosen.txt");
    assert!(!metadata.dirty);
    w.undo().unwrap();
    assert!(w.active_bytes().unwrap().is_empty());
    assert_eq!(fs::read(fixture.path("chosen.txt")).unwrap(), original);
    w.redo().unwrap();
    assert_eq!(w.active_bytes().unwrap(), original);
}
