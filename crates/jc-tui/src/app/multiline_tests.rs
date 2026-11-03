//! Real TUI events over exact multiline strings. These tests construct their
//! own tiny sources, never read the user's workspace sources or OS clipboard.
use super::TuiApp;
use crate::{
    render,
    state::{LaunchOptions, PromptKind, ReplaceField, UiOverlay},
};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use jc_app::{CommandId, SearchStatus};
use jc_text::{TextEncoding, TextPosition};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

fn key(app: &mut TuiApp, code: KeyCode, modifiers: KeyModifiers) {
    app.handle_event(Event::Key(KeyEvent::new(code, modifiers)));
}
fn plain(app: &mut TuiApp, code: KeyCode) {
    key(app, code, KeyModifiers::NONE);
}
fn paste(app: &mut TuiApp, text: &str) {
    app.handle_event(Event::Paste(text.into()));
}
fn clear(app: &mut TuiApp) {
    key(app, KeyCode::Char('u'), KeyModifiers::CONTROL);
}
fn finish(app: &mut TuiApp) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.workbench.search_running() {
        assert!(
            Instant::now() < deadline,
            "small multiline document search did not finish"
        );
        app.poll_tasks();
    }
    assert_eq!(app.workbench.search().status, SearchStatus::Complete);
}
fn editor(text: &str, encoding: TextEncoding) -> TuiApp {
    let mut app = TuiApp::new(LaunchOptions::default());
    app.panels.hide_sidebar();
    app.workbench.new_file("multiline-memory.txt", encoding);
    app.workbench.insert_text(text).unwrap();
    app.workbench
        .go_to_position(TextPosition { line: 0, column: 0 })
        .unwrap();
    app
}
fn revision(app: &TuiApp) -> u64 {
    app.workbench
        .active_document()
        .unwrap()
        .session
        .document()
        .revision()
}
fn query(app: &TuiApp) -> &str {
    match &app.overlay {
        UiOverlay::Find { input } => input,
        UiOverlay::Replace { find, .. } => find,
        _ => panic!("search field closed"),
    }
}
fn replacement(app: &TuiApp) -> &str {
    match &app.overlay {
        UiOverlay::Replace { replacement, .. } => replacement,
        _ => panic!("replacement field closed"),
    }
}

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    workspace: PathBuf,
    base: PathBuf,
    root: PathBuf,
    path: PathBuf,
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
        Err(error) => panic!("create fixture directory: {error}"),
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
        ensure_dir(&work);
        assert!(ordinary(&work).unwrap());
        let work = work.canonicalize().unwrap();
        assert!(work.starts_with(&workspace));
        let base = work.join("test-temp");
        ensure_dir(&base);
        assert!(ordinary(&base).unwrap());
        let base = base.canonicalize().unwrap();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = base.join(format!(
            "multiline-tui-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let path = root.join("source.txt");
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        for chunk in bytes.chunks(64 * 1024) {
            file.write_all(chunk).unwrap();
        }
        Self {
            workspace,
            base,
            root,
            path,
        }
    }
    fn cleanup(&self) -> std::io::Result<()> {
        let root = self.root.canonicalize()?;
        if root != self.root
            || root.parent() != Some(self.base.as_path())
            || !self.base.canonicalize()?.starts_with(&self.workspace)
            || !ordinary(&root)?
            || !ordinary(&self.path)?
            || !self.path.canonicalize()?.starts_with(&root)
        {
            return Err(std::io::Error::other(
                "fixture ownership changed; cleanup refused",
            ));
        }
        fs::remove_file(&self.path)?;
        // Do not remove unknown save-recovery files if the test fails.
        fs::remove_dir(&root)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(error) = self.cleanup() {
            eprintln!(
                "retained multiline fixture {}: {error}",
                self.root.display()
            );
        }
    }
}

#[test]
fn multiline_replace_all_is_one_undo_and_save_preserves_exact_unicode_mixed_eol_bytes() {
    let source = "\u{feff}début\r\nniño\tA\nfin\rtail\r\nniño\tA\nfin";
    let block = "niño\tA\nfin";
    let inserted = "Ω\r\nx\ny";
    let expected = "\u{feff}début\r\nΩ\r\nx\ny\rtail\r\nΩ\r\nx\ny";
    for (action, modifiers) in [
        (KeyCode::F(9), KeyModifiers::NONE),
        (KeyCode::Enter, KeyModifiers::CONTROL),
    ] {
        let fixture = Fixture::new(source.as_bytes());
        let mut app = TuiApp::new(LaunchOptions {
            files: vec![fixture.path.clone()],
            encoding: Some(TextEncoding::Utf8),
        });
        app.panels.hide_sidebar();
        key(&mut app, KeyCode::Char('h'), KeyModifiers::CONTROL);
        paste(&mut app, block);
        finish(&mut app);
        assert_eq!(query(&app), block);
        assert_eq!(app.workbench.search().matches.len(), 2);
        plain(&mut app, KeyCode::Tab);
        paste(&mut app, inserted);
        assert_eq!(replacement(&app), inserted);
        assert!(matches!(
            &app.overlay,
            UiOverlay::Replace {
                field: ReplaceField::Replacement,
                ..
            }
        ));
        let cursors = (app.find_cursor, app.replacement_cursor);
        let visible = app.workbench.invisibles_visible();
        key(
            &mut app,
            KeyCode::Char('i'),
            KeyModifiers::CONTROL | KeyModifiers::ALT,
        );
        assert_eq!(app.workbench.invisibles_visible(), !visible);
        assert_eq!(query(&app), block);
        assert_eq!(replacement(&app), inserted);
        assert_eq!((app.find_cursor, app.replacement_cursor), cursors);
        {
            let p = app.prepare_frame(140, 30);
            let first = render::render(&p);
            assert_eq!(first, render::render(&p));
            assert!((0..first.height()).any(|row| first.row_text(row).contains(r"\u{3A9}")));
            assert!(
                (0..first.height()).all(|row| !first.row_text(row).chars().any(char::is_control))
            );
            assert_eq!((p.app.find_cursor, p.app.replacement_cursor), cursors);
        }
        key(
            &mut app,
            KeyCode::Char('i'),
            KeyModifiers::CONTROL | KeyModifiers::ALT,
        );
        assert_eq!(app.workbench.invisibles_visible(), visible);
        assert_eq!(query(&app), block);
        assert_eq!(replacement(&app), inserted);
        let before = revision(&app);
        key(&mut app, action, modifiers);
        assert_eq!(app.workbench.active_bytes().unwrap(), expected.as_bytes());
        assert_eq!(revision(&app), before + 1);
        assert_eq!(fs::read(&fixture.path).unwrap(), source.as_bytes());
        plain(&mut app, KeyCode::Esc);
        key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(app.workbench.active_bytes().unwrap(), source.as_bytes());
        key(&mut app, KeyCode::Char('y'), KeyModifiers::CONTROL);
        assert_eq!(app.workbench.active_bytes().unwrap(), expected.as_bytes());
        key(&mut app, KeyCode::Char('s'), KeyModifiers::CONTROL);
        assert_eq!(fs::read(&fixture.path).unwrap(), expected.as_bytes());
        assert!(!app
            .workbench
            .active_document()
            .unwrap()
            .session
            .document()
            .is_modified());
    }
}

#[test]
fn find_blocks_preserve_exact_eol_controls_eof_navigation_and_case_word_toggles() {
    let source = "α\r\nβ\nγ\rδ\tZ\u{1}";
    let mut app = editor(source, TextEncoding::Utf8);
    key(&mut app, KeyCode::Char('f'), KeyModifiers::CONTROL);
    plain(&mut app, KeyCode::F(2));
    finish(&mut app);
    assert_eq!(query(&app), "\n");
    assert_eq!(
        app.workbench
            .search()
            .matches
            .iter()
            .map(|hit| hit.range.clone())
            .collect::<Vec<_>>(),
        vec![6..7]
    );
    clear(&mut app);
    plain(&mut app, KeyCode::F(4));
    finish(&mut app);
    assert_eq!(query(&app), "\r\n");
    assert_eq!(app.workbench.search().matches[0].range, 2..4);
    assert_eq!(app.workbench.search().matches.len(), 1);
    clear(&mut app);
    plain(&mut app, KeyCode::F(8));
    finish(&mut app);
    assert_eq!(query(&app), "\r");
    assert_eq!(
        app.workbench
            .search()
            .matches
            .iter()
            .map(|hit| hit.range.clone())
            .collect::<Vec<_>>(),
        vec![9..10]
    );
    clear(&mut app);
    paste(&mut app, "β\nγ\rδ\tZ\u{1}");
    finish(&mut app);
    assert_eq!(app.workbench.search().matches[0].range, 4..15);
    assert_eq!(app.workbench.search().matches.len(), 1);
    plain(&mut app, KeyCode::F(3));
    assert_eq!(
        app.workbench.active_editor().unwrap().selection(),
        Some(4..15)
    );
    key(&mut app, KeyCode::F(3), KeyModifiers::SHIFT);
    assert_eq!(
        app.workbench.active_editor().unwrap().selection(),
        Some(4..15)
    );
    let screen = app.frame(140, 24);
    assert!((0..screen.height()).any(|row| screen.row_text(row).contains(r"\x01")));
    clear(&mut app);
    paste(&mut app, "β\r\nγ");
    finish(&mut app);
    assert!(app.workbench.search().matches.is_empty());
    let before = revision(&app);
    plain(&mut app, KeyCode::Enter);
    assert_eq!(revision(&app), before);
    let sensitive = app.workbench.search().case_sensitive;
    key(&mut app, KeyCode::Char('c'), KeyModifiers::ALT);
    finish(&mut app);
    assert_eq!(app.workbench.search().case_sensitive, !sensitive);
    let word = app.workbench.search().whole_word;
    key(&mut app, KeyCode::Char('w'), KeyModifiers::ALT);
    finish(&mut app);
    assert_eq!(app.workbench.search().whole_word, !word);
    key(&mut app, KeyCode::Char('r'), KeyModifiers::ALT);
    assert!(app.workbench.search().regex);
    finish(&mut app);
    assert_eq!(app.workbench.active_bytes().unwrap(), source.as_bytes());
    assert_eq!(revision(&app), before);
}

#[test]
fn field_navigation_paste_in_middle_token_keys_and_overflow_do_not_edit_the_document() {
    let mut app = editor("unchanged", TextEncoding::Utf8);
    key(&mut app, KeyCode::Char('h'), KeyModifiers::CONTROL);
    paste(&mut app, "é\r\nxy");
    plain(&mut app, KeyCode::Home);
    plain(&mut app, KeyCode::Left);
    plain(&mut app, KeyCode::Left);
    plain(&mut app, KeyCode::Right);
    assert_eq!(app.find_cursor, 2);
    paste(&mut app, "Q");
    plain(&mut app, KeyCode::Delete);
    plain(&mut app, KeyCode::Backspace);
    assert_eq!(query(&app), "éxy");
    assert_eq!(app.find_cursor, 2);
    plain(&mut app, KeyCode::End);
    plain(&mut app, KeyCode::F(4));
    paste(&mut app, "z");
    plain(&mut app, KeyCode::Up);
    assert_eq!(app.find_cursor, 2);
    plain(&mut app, KeyCode::Down);
    assert_eq!(app.find_cursor, 7);
    plain(&mut app, KeyCode::Home);
    plain(&mut app, KeyCode::F(7));
    plain(&mut app, KeyCode::F(8));
    key(&mut app, KeyCode::Enter, KeyModifiers::SHIFT);
    plain(&mut app, KeyCode::F(2));
    plain(&mut app, KeyCode::Backspace);
    plain(&mut app, KeyCode::Delete);
    assert_eq!(query(&app), "éxy\r\n\t\r\n");
    assert_eq!(app.find_cursor, 9);
    plain(&mut app, KeyCode::Tab);
    paste(&mut app, "12\n34");
    plain(&mut app, KeyCode::Home);
    plain(&mut app, KeyCode::Right);
    paste(&mut app, "Ω\r\n");
    assert_eq!(replacement(&app), "12\n3Ω\r\n4");
    assert_eq!(app.replacement_cursor, 8);
    plain(&mut app, KeyCode::Tab);
    assert_eq!(app.find_cursor, 9);
    key(&mut app, KeyCode::BackTab, KeyModifiers::SHIFT);
    let before = replacement(&app).to_owned();
    paste(&mut app, &"x".repeat(4096));
    assert_eq!(replacement(&app), before);
    assert_eq!(app.replacement_cursor, 8);
    assert!(app.status.contains("límite"));
    plain(&mut app, KeyCode::Tab);
    clear(&mut app);
    assert_eq!(query(&app), "");
    assert_eq!(app.find_cursor, 0);
    assert_eq!(replacement(&app), before);
    paste(&mut app, &"é".repeat(2048));
    assert_eq!(query(&app).len(), 4096);
    assert_eq!(app.find_cursor, 4096);
    let find = query(&app).to_owned();
    paste(&mut app, "q");
    assert_eq!(query(&app), find);
    assert_eq!(app.find_cursor, 4096);
    app.dispatch(CommandId::ViewToggleInvisibles);
    for width in [45, 140] {
        let p = app.prepare_frame(width, 30);
        let first = render::render(&p);
        assert_eq!(first, render::render(&p));
        assert!((0..first.height()).all(|row| !first.row_text(row).chars().any(char::is_control)));
        assert!((0..first.height()).any(|row| first.row_text(row).contains(r"\u{E9}")));
        assert!((0..first.height()).all(|row| !first.row_text(row).contains('Ω')));
        assert_eq!((p.app.find_cursor, p.app.replacement_cursor), (4096, 8));
    }
    assert_eq!(app.workbench.active_bytes().unwrap(), b"unchanged");
    app.workbench.undo().unwrap();
    assert!(app.workbench.active_bytes().unwrap().is_empty());
}

#[test]
fn legacy_multiline_replacement_rejects_unrepresentable_text_before_any_mutation() {
    let source = "niño\r\nfin\r\nniño\r\nfin";
    for action in [KeyCode::Enter, KeyCode::F(6)] {
        let mut app = editor(source, TextEncoding::Windows1252);
        let original = app.workbench.active_bytes().unwrap();
        key(&mut app, KeyCode::Char('h'), KeyModifiers::CONTROL);
        paste(&mut app, "niño\r\nfin");
        finish(&mut app);
        assert_eq!(app.workbench.search().matches.len(), 2);
        plain(&mut app, KeyCode::Tab);
        paste(&mut app, "Ω\nreplacement");
        let before = revision(&app);
        let views = format!("{:?}", app.workbench.panes());
        key(&mut app, KeyCode::Enter, KeyModifiers::CONTROL);
        assert_eq!(app.workbench.active_bytes().unwrap(), original);
        assert_eq!(revision(&app), before);
        assert_eq!(views, format!("{:?}", app.workbench.panes()));
        clear(&mut app);
        paste(&mut app, "sí\r\nX");
        plain(&mut app, action);
        assert_eq!(
            app.workbench.active_bytes().unwrap(),
            b"s\xed\r\nX\r\nni\xf1o\r\nfin"
        );
        assert_eq!(revision(&app), before + 1);
        plain(&mut app, KeyCode::Esc);
        key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(app.workbench.active_bytes().unwrap(), original);
    }
}

#[test]
fn regex_find_replace_events_project_draft_and_readonly_frames_share_the_capability() {
    let mut app = editor("niño12\r\nniño345", TextEncoding::Windows1252);
    key(&mut app, KeyCode::Char('h'), KeyModifiers::CONTROL);
    paste(&mut app, r"niño\d+");
    key(&mut app, KeyCode::Char('r'), KeyModifiers::ALT);
    finish(&mut app);
    assert!(app.workbench.search().regex);
    assert_eq!(app.workbench.search().matches.len(), 2);
    let before = app.workbench.search().clone();
    let p = app.prepare_frame(140, 30);
    assert_eq!(render::render(&p), render::render(&p));
    assert_eq!(p.app.workbench.search(), &before);
    plain(&mut app, KeyCode::Tab);
    paste(&mut app, "$1");
    key(&mut app, KeyCode::Enter, KeyModifiers::CONTROL);
    assert_eq!(app.workbench.active_bytes().unwrap(), b"$1\r\n$1");
    plain(&mut app, KeyCode::Esc);
    app.workbench.undo().unwrap();
    assert_eq!(
        app.workbench.active_bytes().unwrap(),
        b"ni\xf1o12\r\nni\xf1o345"
    );
    app.submit_project_prompt(PromptKind::ProjectQuery, r"niño\d+".into());
    assert!(app.project_draft.as_ref().unwrap().regex);
}
