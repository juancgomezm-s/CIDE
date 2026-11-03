//! Versioned generic project opening must validate before changing the catalog
//! or view. Every disk path here belongs to a tiny fixture created by this test.
use jc_app::{DocumentId, PaneId, Workbench};
use jc_files::ContentFingerprint;
use jc_text::{TextEncoding, TextPosition};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    base: PathBuf,
    root: PathBuf,
    project: PathBuf,
    source: PathBuf,
    outside: PathBuf,
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
fn directory(path: &Path) {
    match fs::create_dir(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => panic!("fixture directory: {e}"),
    }
    assert!(ordinary(path).unwrap() && path.is_dir());
}
fn create(path: &Path, bytes: &[u8]) {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    for chunk in bytes.chunks(64 * 1024) {
        file.write_all(chunk).unwrap();
    }
}
impl Fixture {
    fn new(bytes: &[u8]) -> Self {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .unwrap()
            .canonicalize()
            .unwrap();
        let work = workspace.join("work");
        directory(&work);
        let work = work.canonicalize().unwrap();
        assert!(work.starts_with(&workspace));
        let base = work.join("test-temp");
        directory(&base);
        let base = base.canonicalize().unwrap();
        assert_eq!(base.parent(), Some(work.as_path()));
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = base.join(format!(
            "project-version-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let project = root.join("project");
        fs::create_dir(&project).unwrap();
        let source = project.join("source.txt");
        let outside = root.join("outside.txt");
        create(&source, bytes);
        create(&outside, bytes);
        Self {
            base,
            root,
            project,
            source,
            outside,
        }
    }
    fn cleanup(&self) -> std::io::Result<()> {
        if self.root.canonicalize()? != self.root
            || self.root.parent() != Some(self.base.as_path())
            || !ordinary(&self.root)?
            || !ordinary(&self.project)?
            || self.project.canonicalize()?.parent() != Some(self.root.as_path())
        {
            return Err(std::io::Error::other("fixture root ownership changed"));
        }
        for (path, parent) in [(&self.source, &self.project), (&self.outside, &self.root)] {
            if !ordinary(path)?
                || !path.is_file()
                || path.canonicalize()?.parent() != Some(parent.as_path())
            {
                return Err(std::io::Error::other("fixture leaf ownership changed"));
            }
        }
        fs::remove_file(&self.source)?;
        fs::remove_file(&self.outside)?;
        fs::remove_dir(&self.project)?;
        fs::remove_dir(&self.root)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(e) = self.cleanup() {
            eprintln!("retained project fixture {}: {e}", self.root.display());
        }
    }
}
fn active_id(w: &Workbench) -> DocumentId {
    w.active_document().unwrap().id
}
fn snapshot(w: &Workbench) -> String {
    format!("{:?}|{:?}", w.panes(), w.tabs())
}
fn bytes(w: &Workbench, id: DocumentId) -> Vec<u8> {
    w.document_by_id(id).unwrap().session.document().to_bytes()
}
fn beginning(w: &mut Workbench) {
    w.go_to_position(TextPosition { line: 0, column: 0 })
        .unwrap();
}

#[test]
fn existing_dirty_content_and_encoding_mismatches_reject_before_activation() {
    let original = b"ni\xf1o\r\nfin\n";
    let fixture = Fixture::new(original);
    let mut w = Workbench::default();
    w.open_file(fixture.source.clone(), Some(TextEncoding::Windows1252))
        .unwrap();
    beginning(&mut w);
    w.insert_text("X").unwrap();
    let source = active_id(&w);
    let edited = bytes(&w, source);
    let revision = w
        .document_by_id(source)
        .unwrap()
        .session
        .document()
        .revision();
    w.new_file(fixture.project.join("memory.txt"), TextEncoding::Utf8);
    w.insert_text("keep focus").unwrap();
    w.move_left(true).unwrap();
    let before = snapshot(&w);
    let current = active_id(&w);
    // First differs in both dimensions; the following calls isolate each gate.
    for (encoding, expected, limit) in [
        (TextEncoding::Utf8, ContentFingerprint::of(original), 1024),
        (
            TextEncoding::Windows1252,
            ContentFingerprint::of(original),
            1024,
        ),
        (TextEncoding::Utf8, ContentFingerprint::of(&edited), 1024),
        (
            TextEncoding::Windows1252,
            ContentFingerprint::of(&edited),
            edited.len() - 1,
        ),
    ] {
        assert!(w
            .open_project_file_at_version(
                &fixture.project,
                fixture.source.clone(),
                Some(encoding),
                true,
                limit,
                expected
            )
            .is_err());
        assert_eq!(snapshot(&w), before);
        assert_eq!(active_id(&w), current);
        assert_eq!(bytes(&w, source), edited);
        let document = w.document_by_id(source).unwrap();
        assert_eq!(document.session.document().revision(), revision);
        assert!(document.session.document().is_modified());
        assert!(document.session.document().can_undo());
    }
    assert_eq!(fs::read(&fixture.source).unwrap(), original);
}

#[test]
fn matching_dirty_open_source_reuses_document_to_side_and_retains_history() {
    let original = b"\xef\xbb\xbfalpha\r\nbeta\nlast\r";
    let fixture = Fixture::new(original);
    let mut w = Workbench::default();
    let source_tab = w
        .open_file(fixture.source.clone(), Some(TextEncoding::Utf8))
        .unwrap();
    beginning(&mut w);
    w.insert_text("X").unwrap();
    w.backspace().unwrap();
    let source = active_id(&w);
    assert_eq!(bytes(&w, source), original);
    assert!(w
        .document_by_id(source)
        .unwrap()
        .session
        .document()
        .is_modified());
    let revision = w
        .document_by_id(source)
        .unwrap()
        .session
        .document()
        .revision();
    let other_tab = w.new_file(fixture.project.join("memory.txt"), TextEncoding::Utf8);
    w.insert_text("other").unwrap();
    let before_tabs = w.tabs().tabs().to_vec();
    let opened = w
        .open_project_file_at_version(
            &fixture.project,
            fixture.source.clone(),
            Some(TextEncoding::Utf8),
            true,
            original.len(),
            ContentFingerprint::of(original),
        )
        .unwrap();
    assert_eq!(opened, source_tab);
    assert_eq!(active_id(&w), source);
    assert_eq!(w.tabs().tabs(), before_tabs.as_slice());
    assert_eq!(w.panes().active(), PaneId::Secondary);
    assert_eq!(
        w.panes()
            .view(PaneId::Primary)
            .unwrap()
            .location()
            .unwrap()
            .tab,
        other_tab
    );
    assert_eq!(
        w.panes()
            .view(PaneId::Secondary)
            .unwrap()
            .location()
            .unwrap()
            .tab,
        source_tab
    );
    let document = w.active_document().unwrap();
    assert_eq!(document.session.document().revision(), revision);
    assert!(document.session.document().is_modified());
    assert_eq!(w.active_bytes().unwrap(), original);
    w.undo().unwrap();
    assert_eq!(
        w.active_bytes().unwrap(),
        b"\xef\xbb\xbfXalpha\r\nbeta\nlast\r"
    );
    w.undo().unwrap();
    assert_eq!(w.active_bytes().unwrap(), original);
    assert_eq!(fs::read(&fixture.source).unwrap(), original);
}

#[test]
fn closed_changed_file_limit_and_outside_root_reject_before_catalog_insertion() {
    let original = b"ni\xf1o\r\nfin\n";
    let changed = b"ni\xe9o\r\nfin\n"; // Same length: content validation cannot be a size check.
    let fixture = Fixture::new(original);
    fs::write(&fixture.source, changed).unwrap(); // This test owns this exact leaf.
    let mut w = Workbench::default();
    w.new_file(fixture.project.join("memory.txt"), TextEncoding::Utf8);
    w.insert_text("stay here").unwrap();
    w.move_home(false).unwrap();
    w.move_right(true).unwrap();
    let before = snapshot(&w);
    let active = active_id(&w);
    for (path, limit, expected) in [
        (
            fixture.source.clone(),
            1024,
            ContentFingerprint::of(original),
        ),
        (
            fixture.source.clone(),
            changed.len() - 1,
            ContentFingerprint::of(changed),
        ),
        (
            fixture.outside.clone(),
            1024,
            ContentFingerprint::of(original),
        ),
    ] {
        assert!(w
            .open_project_file_at_version(
                &fixture.project,
                path,
                Some(TextEncoding::Windows1252),
                false,
                limit,
                expected
            )
            .is_err());
        assert_eq!(snapshot(&w), before);
        assert_eq!(active_id(&w), active);
        assert_eq!(w.active_bytes().unwrap(), b"stay here");
    }
    let tab = w
        .open_project_file_at_version(
            &fixture.project,
            fixture.source.clone(),
            Some(TextEncoding::Windows1252),
            false,
            changed.len(),
            ContentFingerprint::of(changed),
        )
        .unwrap();
    assert_eq!(w.tabs().active_id(), Some(tab));
    assert_eq!(w.tabs().tabs().len(), 2);
    assert_eq!(w.active_bytes().unwrap(), changed);
    assert_eq!(w.active_encoding(), Some(TextEncoding::Windows1252));
    assert!(!w
        .active_document()
        .unwrap()
        .session
        .document()
        .is_modified());
    assert_eq!(fs::read(&fixture.source).unwrap(), changed);
    assert_eq!(fs::read(&fixture.outside).unwrap(), original);
}
