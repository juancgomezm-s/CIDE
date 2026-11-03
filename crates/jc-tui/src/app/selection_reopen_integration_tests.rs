//! Event-level selected-text search, closed metadata and indexed-tab regressions.
use super::TuiApp;
use crate::state::{LaunchOptions, UiOverlay};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use jc_app::{CommandId, SearchStatus};
use jc_text::{encode, TextEncoding, TextPosition};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
fn key(a: &mut TuiApp, c: KeyCode, m: KeyModifiers) {
    a.handle_event(Event::Key(KeyEvent::new(c, m)));
}
fn plain(a: &mut TuiApp, c: KeyCode) {
    key(a, c, KeyModifiers::NONE);
}
fn at(a: &mut TuiApp, line: usize, column: usize) {
    a.workbench
        .go_to_position(TextPosition { line, column })
        .unwrap();
}
fn editor(path: &str, text: &str, encoding: TextEncoding) -> TuiApp {
    let mut a = TuiApp::new(LaunchOptions::default());
    a.panels.hide_sidebar();
    a.workbench.new_file(path, encoding);
    a.workbench.insert_text(text).unwrap();
    at(&mut a, 0, 0);
    a
}
fn complete(a: &mut TuiApp) {
    let end = Instant::now() + Duration::from_secs(5);
    while a.workbench.search_running() {
        assert!(Instant::now() < end);
        a.poll_tasks();
    }
    assert_eq!(a.workbench.search().status, SearchStatus::Complete);
}
fn selected_block(a: &mut TuiApp) {
    at(a, 0, 0);
    key(a, KeyCode::Down, KeyModifiers::SHIFT);
    for _ in 0..3 {
        key(a, KeyCode::Right, KeyModifiers::SHIFT);
    }
}
fn state(a: &TuiApp) -> String {
    format!("{:?}|{:?}", a.workbench.panes(), a.workbench.tabs())
}
fn close(a: &mut TuiApp) {
    key(a, KeyCode::Char('w'), KeyModifiers::CONTROL);
}
fn reopen(a: &mut TuiApp) {
    key(
        a,
        KeyCode::Char('t'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
}

struct Fixture {
    root: PathBuf,
    a: PathBuf,
    b: PathBuf,
    invalid: PathBuf,
}
fn ordinary(p: &Path) -> bool {
    let m = fs::symlink_metadata(p).unwrap();
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        m.file_attributes() & 0x400 == 0
    }
    #[cfg(not(windows))]
    {
        !m.file_type().is_symlink()
    }
}
fn create(p: &Path, bytes: &[u8]) {
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(p)
        .unwrap();
    for chunk in bytes.chunks(64 * 1024) {
        f.write_all(chunk).unwrap();
    }
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .unwrap()
            .canonicalize()
            .unwrap();
        let mut base = workspace.clone();
        for name in ["work", "test-temp"] {
            base.push(name);
            match fs::create_dir(&base) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => panic!("fixture: {e}"),
            }
            assert!(ordinary(&base) && base.is_dir());
            base = base.canonicalize().unwrap();
            assert!(base.starts_with(&workspace));
        }
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = base.join(format!(
            "selection-reopen-{}-{time}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let a = root.join("a.p");
        let b = root.join("b.txt");
        let invalid = root.join("invalid.txt");
        create(&a, b"ni\xf1o\r\nfirst\n");
        create(&b, b"\xef\xbb\xbfsecond\r\n");
        create(&invalid, b"a\xffa");
        Self {
            root,
            a,
            b,
            invalid,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if self.root.canonicalize().ok().as_ref() != Some(&self.root) || !ordinary(&self.root) {
            return;
        }
        for p in [&self.a, &self.b, &self.invalid] {
            if let Ok(m) = fs::symlink_metadata(p) {
                if !m.is_file()
                    || !ordinary(p)
                    || p.canonicalize()
                        .ok()
                        .and_then(|p| p.parent().map(Path::to_owned))
                        != Some(self.root.clone())
                {
                    return;
                }
            }
        }
        for p in [&self.a, &self.b, &self.invalid] {
            let _ = fs::remove_file(p);
        }
        let _ = fs::remove_dir(&self.root);
    }
}

#[test]
fn selected_multiline_search_preserves_encoding_flags_and_rejects_invalid_raw_ranges() {
    for encoding in [
        TextEncoding::Utf8,
        TextEncoding::Windows1252,
        TextEncoding::Iso8859_1,
        TextEncoding::Ibm850,
    ] {
        let text = "niño\r\nfin|niño\r\nfin";
        let query = "niño\r\nfin";
        let mut a = editor("selected.p", text, encoding);
        selected_block(&mut a);
        let flags = (
            a.workbench.search().case_sensitive,
            a.workbench.search().whole_word,
        );
        key(&mut a, KeyCode::F(3), KeyModifiers::CONTROL);
        complete(&mut a);
        let len = encode(query, encoding).unwrap().len();
        assert_eq!(a.workbench.search().query, query);
        assert_eq!(
            a.workbench.active_editor().unwrap().selection(),
            Some(len + 1..len + 1 + len)
        );
        key(
            &mut a,
            KeyCode::F(3),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        );
        complete(&mut a);
        assert_eq!(
            a.workbench.active_editor().unwrap().selection(),
            Some(0..len)
        );
        assert_eq!(
            (
                a.workbench.search().case_sensitive,
                a.workbench.search().whole_word
            ),
            flags
        );
        assert_eq!(
            a.workbench.active_bytes().unwrap(),
            encode(text, encoding).unwrap()
        );
    }
    let mut a = editor("budget.txt", &"x".repeat(4096), TextEncoding::Utf8);
    key(&mut a, KeyCode::Char('a'), KeyModifiers::CONTROL);
    key(&mut a, KeyCode::F(3), KeyModifiers::CONTROL);
    complete(&mut a);
    assert_eq!(a.workbench.search().query.len(), 4096);
    a.workbench.new_file("oversize.txt", TextEncoding::Utf8);
    a.workbench.insert_text(&"é".repeat(2049)).unwrap();
    key(&mut a, KeyCode::Char('a'), KeyModifiers::CONTROL);
    let query = a.workbench.search().query.clone();
    let before = state(&a);
    key(&mut a, KeyCode::F(3), KeyModifiers::CONTROL);
    assert_eq!(a.workbench.search().query, query);
    assert_eq!(state(&a), before);
    at(&mut a, 0, 0);
    let before = state(&a);
    key(&mut a, KeyCode::F(3), KeyModifiers::CONTROL);
    assert_eq!(state(&a), before);
    a.workbench
        .new_file("legacy-query-budget.txt", TextEncoding::Iso8859_1);
    a.workbench.insert_text(&"ñ".repeat(4096)).unwrap();
    key(&mut a, KeyCode::Char('a'), KeyModifiers::CONTROL);
    let before = state(&a);
    key(&mut a, KeyCode::F(3), KeyModifiers::CONTROL);
    assert_eq!(a.workbench.search().query, query);
    assert_eq!(state(&a), before);
    let fixture = Fixture::new();
    a.workbench
        .open_file(fixture.invalid.clone(), Some(TextEncoding::Utf8))
        .unwrap();
    key(&mut a, KeyCode::Char('a'), KeyModifiers::CONTROL);
    let before = state(&a);
    key(&mut a, KeyCode::F(3), KeyModifiers::CONTROL);
    assert_eq!(a.workbench.search().query, query);
    assert_eq!(state(&a), before);
    assert_eq!(a.workbench.active_bytes().unwrap(), b"a\xffa");
    a.workbench.new_file("rectangle.txt", TextEncoding::Utf8);
    a.workbench.insert_text("abc").unwrap();
    at(&mut a, 0, 0);
    a.workbench.start_rectangle().unwrap();
    a.workbench.extend_rectangle(2, 0).unwrap();
    let before = state(&a);
    key(&mut a, KeyCode::F(3), KeyModifiers::CONTROL);
    assert_eq!(state(&a), before);
    assert_eq!(a.workbench.search().query, query);
}

#[test]
fn selected_search_cancel_and_late_view_revision_delivery_do_not_replay_navigation() {
    let text = "niño\r\nfin|niño\r\nfin";
    let mut a = editor("late.txt", text, TextEncoding::Utf8);
    selected_block(&mut a);
    key(&mut a, KeyCode::F(3), KeyModifiers::CONTROL);
    let before = state(&a);
    a.dispatch(CommandId::SearchCancel);
    for _ in 0..3 {
        a.poll_tasks();
    }
    assert_eq!(a.workbench.search().status, SearchStatus::Cancelled);
    assert_eq!(state(&a), before);
    selected_block(&mut a);
    key(&mut a, KeyCode::F(3), KeyModifiers::CONTROL);
    a.dispatch(CommandId::EditorSplitRight);
    a.dispatch(CommandId::EditorFocusSecondary);
    let before = state(&a);
    complete(&mut a);
    assert_eq!(state(&a), before);
    assert_eq!(
        a.workbench.active_editor().unwrap().selection(),
        Some(0..10)
    );
    a.dispatch(CommandId::EditorFocusPrimary);
    a.dispatch(CommandId::EditorOneGroup);
    // Reusing a Complete query can navigate synchronously. Force a fresh scan
    // so the following mutation really races a deferred navigation request.
    a.workbench.set_search_query("").unwrap();
    selected_block(&mut a);
    key(&mut a, KeyCode::F(3), KeyModifiers::CONTROL);
    assert!(a.workbench.search_running());
    assert_eq!(a.workbench.search().status, SearchStatus::Running);
    assert_eq!(
        a.workbench.active_editor().unwrap().selection(),
        Some(0..10)
    );
    a.workbench.insert_text("X").unwrap();
    let before = state(&a);
    complete(&mut a);
    assert_eq!(state(&a), before);
    assert!(a.workbench.active_editor().unwrap().selection().is_none());
    assert_eq!(
        a.workbench.active_bytes().unwrap(),
        "X|niño\r\nfin".as_bytes()
    );
}

#[test]
fn reopen_observes_disk_lifo_retry_collision_last_view_and_new_blank_contracts() {
    let fixture = Fixture::new();
    let mut a = editor("base.txt", "base", TextEncoding::Utf8);
    let tab = a
        .workbench
        .open_file(fixture.a.clone(), Some(TextEncoding::Windows1252))
        .unwrap();
    let old = a.workbench.active_document().unwrap().id;
    close(&mut a);
    fs::write(&fixture.a, b"changed\r\n").unwrap();
    reopen(&mut a);
    assert_eq!(a.workbench.tabs().active_id(), Some(tab));
    assert_ne!(a.workbench.active_document().unwrap().id, old);
    assert_eq!(a.workbench.active_bytes().unwrap(), b"changed\r\n");
    assert_eq!(
        a.workbench.active_encoding(),
        Some(TextEncoding::Windows1252)
    );
    assert!(!a
        .workbench
        .active_document()
        .unwrap()
        .session
        .document()
        .can_undo());
    a.workbench.insert_text("Q").unwrap();
    let dirty = a.workbench.active_document().unwrap().id;
    a.dispatch(CommandId::EditorSplitRight);
    a.dispatch(CommandId::EditorFocusSecondary);
    a.dispatch(CommandId::EditorTogglePin);
    let count = a.workbench.tabs().tabs().len();
    close(&mut a);
    assert_eq!(a.workbench.tabs().tabs().len(), count);
    assert!(a.workbench.document_by_id(dirty).is_some());
    assert!(!matches!(a.overlay, UiOverlay::CloseDialog { .. }));
    a.dispatch(CommandId::EditorOneGroup);
    close(&mut a);
    assert!(matches!(a.overlay, UiOverlay::CloseDialog { .. }));
    plain(&mut a, KeyCode::Esc);
    assert!(a.workbench.document_by_id(dirty).is_some());
    close(&mut a);
    plain(&mut a, KeyCode::Char('d'));
    reopen(&mut a);
    assert_eq!(a.workbench.active_bytes().unwrap(), b"changed\r\n");
    assert_ne!(a.workbench.active_document().unwrap().id, dirty);
    close(&mut a);
    a.workbench
        .new_file(fixture.root.join("never-saved.txt"), TextEncoding::Utf8);
    a.workbench.insert_text("discarded draft").unwrap();
    let blank = a.workbench.tabs().active_id().unwrap();
    close(&mut a);
    plain(&mut a, KeyCode::Char('d'));
    reopen(&mut a);
    assert_eq!(a.workbench.tabs().active_id(), Some(blank));
    assert!(a.workbench.active_bytes().unwrap().is_empty());
    assert!(a.workbench.active_document().unwrap().session.is_new());
    assert!(!a
        .workbench
        .active_document()
        .unwrap()
        .session
        .document()
        .can_undo());
    close(&mut a);
    let b = a
        .workbench
        .open_file(fixture.b.clone(), Some(TextEncoding::Utf8))
        .unwrap();
    close(&mut a);
    fs::remove_file(&fixture.b).unwrap();
    let before = state(&a);
    reopen(&mut a);
    assert_eq!(state(&a), before);
    create(&fixture.b, b"\xef\xbb\xbfretry\r\n");
    reopen(&mut a);
    assert_eq!(a.workbench.tabs().active_id(), Some(b));
    assert_eq!(a.workbench.active_editor().unwrap().cursor, 3);
    close(&mut a);
    let a_tab = a
        .workbench
        .open_file(fixture.a.clone(), Some(TextEncoding::Windows1252))
        .unwrap();
    close(&mut a);
    let b_tab = a
        .workbench
        .open_file(fixture.b.clone(), Some(TextEncoding::Utf8))
        .unwrap();
    close(&mut a);
    reopen(&mut a);
    assert_eq!(a.workbench.tabs().active_id(), Some(b_tab));
    reopen(&mut a);
    assert_eq!(a.workbench.tabs().active_id(), Some(a_tab));
    close(&mut a);
    close(&mut a);
    a.workbench
        .open_file(fixture.b.clone(), Some(TextEncoding::Utf8))
        .unwrap();
    a.workbench.insert_text("Q").unwrap();
    let before = state(&a);
    let bytes = a.workbench.active_bytes().unwrap();
    reopen(&mut a);
    assert_eq!(state(&a), before);
    assert_eq!(a.workbench.active_bytes().unwrap(), bytes);
    assert!(a
        .workbench
        .active_document()
        .unwrap()
        .session
        .document()
        .is_modified());
}

#[test]
fn tab_indices_last_and_alt_keys_preserve_per_document_carets_selection_and_flags() {
    let mut a = editor("base.txt", "base", TextEncoding::Utf8);
    let mut records = Vec::new();
    for i in 0..9 {
        let tab = a
            .workbench
            .new_file(format!("tab-{i}.txt"), TextEncoding::Utf8);
        a.workbench.insert_text(&format!("tab{i}\r\nniño")).unwrap();
        at(&mut a, 1, 2);
        key(&mut a, KeyCode::Right, KeyModifiers::SHIFT);
        let e = a.workbench.active_editor().unwrap();
        records.push((tab, e.document_id, e.cursor, e.selection()));
    }
    let flags = (
        a.workbench.search().case_sensitive,
        a.workbench.search().whole_word,
    );
    let count = a.workbench.tabs().tabs().len();
    for (index, command) in [
        CommandId::Tab2,
        CommandId::Tab3,
        CommandId::Tab4,
        CommandId::Tab5,
        CommandId::Tab6,
        CommandId::Tab7,
        CommandId::Tab8,
    ]
    .into_iter()
    .enumerate()
    {
        a.dispatch(command);
        let e = a.workbench.active_editor().unwrap();
        assert_eq!(
            (e.tab_id, e.document_id, e.cursor, e.selection()),
            records[index]
        );
    }
    a.dispatch(CommandId::Tab9);
    assert_eq!(a.workbench.tabs().active_id(), Some(records[8].0));
    key(&mut a, KeyCode::Char('1'), KeyModifiers::ALT);
    assert_eq!(a.workbench.active_bytes().unwrap(), b"base");
    key(&mut a, KeyCode::Char('9'), KeyModifiers::ALT);
    let e = a.workbench.active_editor().unwrap();
    assert_eq!(
        (e.tab_id, e.document_id, e.cursor, e.selection()),
        records[8]
    );
    assert_eq!(a.workbench.tabs().tabs().len(), count);
    assert_eq!(
        (
            a.workbench.search().case_sensitive,
            a.workbench.search().whole_word
        ),
        flags
    );
}
