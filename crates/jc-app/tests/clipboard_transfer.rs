//! External clipboard proposals reuse live views and the core transaction path.
use jc_app::{PaneId, Workbench};
use jc_text::{encode, TextEncoding};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

fn memory(text: &str, encoding: TextEncoding) -> Workbench {
    let mut app = Workbench::default();
    app.new_file("clipboard-memory.txt", encoding);
    app.insert_text(text).unwrap();
    app
}
fn select(app: &mut Workbench, start: usize, end: usize) {
    let tab = app.tabs().active_id().unwrap();
    assert!(app.jump_to_location(tab, start));
    let budget = app.active_bytes().unwrap().len() * 2 + 4;
    for _ in 0..budget {
        let cursor = app.active_editor().unwrap().cursor;
        if cursor == end {
            return;
        }
        if cursor < end {
            app.move_right(true).unwrap();
        } else {
            app.move_left(true).unwrap();
        }
    }
    panic!("fixture selection boundary unreachable");
}
fn state(
    app: &Workbench,
) -> (
    Vec<u8>,
    u64,
    usize,
    usize,
    Option<usize>,
    Option<TextEncoding>,
) {
    let editor = app.active_editor().unwrap();
    let document = app.active_document().unwrap();
    (
        app.active_bytes().unwrap(),
        document.session.document().revision(),
        document.session.document().undo_steps(),
        editor.cursor,
        editor.anchor,
        app.active_encoding(),
    )
}

#[test]
fn mixed_eol_bom_cut_and_internal_paste_are_one_undo_each_shared_across_views() {
    let original = "\u{feff}niño\r\nsecond\nlast\r";
    let source = original.as_bytes();
    let content = "niño\r\nsecond\nlast\r";
    let mut app = memory(original, TextEncoding::Utf8);
    assert!(app.split_editor_right());
    select(&mut app, 3, source.len());
    let before = state(&app);
    let export = app.export_selection(1024).unwrap().unwrap();
    assert_eq!(export.text, content);
    assert_eq!(state(&app), before);
    let id = app.active_document().unwrap().id;
    app.delete_exported_selection(export).unwrap();
    assert_eq!(app.active_bytes().unwrap(), b"\xef\xbb\xbf");
    assert_eq!(state(&app).2, before.2 + 1);
    assert!(app.focus_editor_pane(PaneId::Secondary));
    assert_eq!(app.active_document().unwrap().id, id);
    app.undo().unwrap();
    assert_eq!(app.active_bytes().unwrap(), source);
    assert!(app.focus_editor_pane(PaneId::Primary));
    app.redo().unwrap();
    assert_eq!(app.active_bytes().unwrap(), b"\xef\xbb\xbf");
    let tab = app.tabs().active_id().unwrap();
    assert!(app.jump_to_location(tab, 3));
    let steps = state(&app).2;
    app.paste().unwrap();
    assert_eq!(app.active_bytes().unwrap(), source);
    assert_eq!(state(&app).2, steps + 1);
    app.undo().unwrap();
    assert_eq!(app.active_bytes().unwrap(), b"\xef\xbb\xbf");
    app.redo().unwrap();
    assert_eq!(app.active_bytes().unwrap(), source);
}

#[test]
fn external_unicode_text_encodes_strictly_in_legacy_and_undo_is_shared() {
    let original = "año\r\n£ €\nend";
    let encoding = TextEncoding::Windows1252;
    let source = encode(original, encoding).unwrap();
    let mut app = memory(original, encoding);
    assert!(app.split_editor_right());
    app.select_all().unwrap();
    let export = app.export_selection(1024).unwrap().unwrap();
    assert_eq!(export.text, original);
    let target = export.target;
    let before = state(&app);
    let id = app.active_document().unwrap().id;
    app.paste_external_text(target, "niño €\r\nlast\r", 1024)
        .unwrap();
    assert_eq!(
        app.active_bytes().unwrap(),
        encode("niño €\r\nlast\r", encoding).unwrap()
    );
    assert_eq!(state(&app).2, before.2 + 1);
    assert!(app.focus_editor_pane(PaneId::Secondary));
    assert_eq!(app.active_document().unwrap().id, id);
    app.undo().unwrap();
    assert_eq!(app.active_bytes().unwrap(), source);
    app.redo().unwrap();
    assert_eq!(
        app.active_bytes().unwrap(),
        encode("niño €\r\nlast\r", encoding).unwrap()
    );
}

#[test]
fn deferred_transfer_rejects_caret_anchor_view_revision_encoding_and_document_changes() {
    for change in 0..6 {
        let mut app = memory("abcdef", TextEncoding::Utf8);
        select(&mut app, 1, 4);
        let export = app.export_selection(64).unwrap().unwrap();
        let target = export.target;
        match change {
            0 => app.move_right(false).unwrap(),
            1 => app.move_left(true).unwrap(),
            2 => {
                assert!(app.split_editor_right());
                assert!(app.focus_editor_pane(PaneId::Secondary));
            }
            3 => app.insert_text("changed").unwrap(),
            4 => app.set_encoding(TextEncoding::Windows1252).unwrap(),
            _ => {
                app.new_file("other-clipboard.txt", TextEncoding::Utf8);
            }
        }
        let current = state(&app);
        assert!(
            app.paste_external_text(target, "external", 64).is_err(),
            "stale cause {change}"
        );
        assert_eq!(state(&app), current);
        assert!(
            app.delete_exported_selection(export).is_err(),
            "stale cut cause {change}"
        );
        assert_eq!(state(&app), current);
    }
}

#[test]
fn quotas_encoding_and_rectangles_reject_atomically_and_export_preserves_internal_clipboard() {
    let mut app = memory("seed|é", TextEncoding::Windows1252);
    select(&mut app, 0, 4);
    app.copy_selection().unwrap();
    select(&mut app, 5, 6);
    let before = state(&app);
    assert!(app.export_selection(1).is_err());
    assert_eq!(state(&app), before);
    let export = app.export_selection(2).unwrap().unwrap();
    assert_eq!(export.text, "é");
    assert_eq!(state(&app), before);
    let target = app.capture_clipboard_target().unwrap();
    assert!(app.paste_external_text(target, "é", 1).is_err());
    assert_eq!(state(&app), before);
    assert!(app.paste_external_text(target, "😀", 64).is_err());
    assert_eq!(state(&app), before);
    // Export is an observation, so paste must still use the earlier internal
    // payload rather than the externally exported selection.
    let tab = app.tabs().active_id().unwrap();
    assert!(app.jump_to_location(tab, 6));
    let target = app.capture_clipboard_target().unwrap();
    app.paste_external_text(target, "!", 64).unwrap();
    app.paste().unwrap();
    assert_eq!(app.active_bytes().unwrap(), b"seed|\xe9!seed");
    app.start_rectangle().unwrap();
    let before = state(&app);
    assert!(app.capture_clipboard_target().is_err());
    assert!(app.export_selection(64).is_err());
    assert_eq!(state(&app), before);
    assert!(app.rectangle_for_pane(PaneId::Primary).is_some());
    let mut utf8 = memory("é", TextEncoding::Utf8);
    utf8.select_all().unwrap();
    let before = state(&utf8);
    assert!(utf8.export_selection(1).is_err());
    assert_eq!(state(&utf8), before);
    let empty = memory("plain", TextEncoding::Utf8);
    let before = state(&empty);
    assert!(empty.export_selection(64).unwrap().is_none());
    assert_eq!(state(&empty), before);
}

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    base: PathBuf,
    path: PathBuf,
}
fn ordinary(path: &Path) -> bool {
    let metadata = fs::symlink_metadata(path).unwrap();
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 == 0
    }
    #[cfg(not(windows))]
    {
        !metadata.file_type().is_symlink()
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
        let mut base = workspace.clone();
        for component in ["work", "test-temp"] {
            base.push(component);
            match fs::create_dir(&base) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("fixture directory: {error}"),
            }
            assert!(ordinary(&base));
            base = base.canonicalize().unwrap();
            assert!(base.starts_with(&workspace));
        }
        let path = base.join(format!(
            "clipboard-transfer-{}-{}.txt",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap()
            .write_all(bytes)
            .unwrap();
        Self { base, path }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        assert!(ordinary(&self.path));
        assert_eq!(
            self.path.canonicalize().unwrap().parent(),
            Some(self.base.as_path())
        );
        assert!(self
            .path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("clipboard-transfer-"));
        fs::remove_file(&self.path).unwrap();
    }
}
#[test]
fn invalid_utf8_and_unknown_encoding_are_never_exported_as_lossy_text() {
    let raw = b"raw \xff\r\nlast";
    let fixture = Fixture::new(raw);
    for encoding in [Some(TextEncoding::Utf8), None] {
        let mut app = Workbench::default();
        app.open_file(&fixture.path, encoding).unwrap();
        app.select_all().unwrap();
        let before = state(&app);
        assert!(app.export_selection(1024).is_err());
        assert_eq!(state(&app), before);
        if encoding.is_none() {
            assert!(app.capture_clipboard_target().is_err());
        }
        assert_eq!(fs::read(&fixture.path).unwrap(), raw);
    }
}
