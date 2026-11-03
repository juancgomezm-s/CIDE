use super::TuiApp;
use crate::{
    render,
    state::{LaunchOptions, PromptKind, UiOverlay},
};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use jc_app::{explorer::ExplorerLoadState, CommandId, PrimaryFocus, Wakeup};
use jc_text::TextEncoding;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
pub(super) struct Fixture {
    pub(super) root: PathBuf,
    base: PathBuf,
}
impl Fixture {
    pub(super) fn new() -> Self {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .unwrap()
            .canonicalize()
            .unwrap();
        let base = workspace.join("work/test-temp");
        fs::create_dir_all(&base).unwrap();
        assert!(!super::file_browser::reparse(
            &fs::symlink_metadata(&base).unwrap()
        ));
        let root = base.join(format!(
            "tui-browser-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self {
            root: root.canonicalize().unwrap(),
            base: base.canonicalize().unwrap(),
        }
    }
    pub(super) fn put(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.root.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        use std::io::Write;
        fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .unwrap()
            .write_all(bytes)
            .unwrap();
        path
    }
    pub(super) fn app(&self) -> (TuiApp, Wakeup) {
        let wake = Wakeup::default();
        let mut app = TuiApp::with_wakeup(
            LaunchOptions {
                files: vec![],
                encoding: Some(TextEncoding::Utf8),
            },
            wake.clone(),
        );
        app.set_explorer_root(self.root.clone()).unwrap();
        (app, wake)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(self.root.parent(), Some(self.base.as_path()));
        assert!(self
            .root
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("tui-browser-"));
        assert!(!super::file_browser::reparse(
            &fs::symlink_metadata(&self.root).unwrap()
        ));
        assert_eq!(self.root.canonicalize().unwrap(), self.root);
        fs::remove_dir_all(&self.root).unwrap();
    }
}
fn key(app: &mut TuiApp, code: KeyCode, modifiers: KeyModifiers) {
    app.handle_event(Event::Key(KeyEvent::new(code, modifiers)));
}
pub(super) fn wait(app: &mut TuiApp, wake: &Wakeup) {
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        let ticket = wake.ticket();
        app.poll_tasks();
        let id = app.file_browser.catalog.root_id().unwrap();
        let state = app.file_browser.catalog.node(id).unwrap().state();
        if !matches!(
            state,
            ExplorerLoadState::Queued | ExplorerLoadState::Loading
        ) {
            assert!(matches!(
                state,
                ExplorerLoadState::Ready | ExplorerLoadState::Limited
            ));
            break;
        }
        assert!(Instant::now() < end);
        wake.wait(ticket, Some(Duration::from_millis(10)));
    }
}
fn select(app: &mut TuiApp, path: &Path) {
    let id = app
        .file_browser
        .catalog
        .nodes()
        .iter()
        .find_map(|node| {
            (app.file_browser.catalog.path(node.id()).as_deref() == Some(path)).then_some(node.id())
        })
        .expect("browser path");
    app.file_browser.view.select(id);
}
fn text(app: &TuiApp) -> &str {
    match &app.overlay {
        UiOverlay::Prompt { input, .. } => input,
        _ => panic!("path prompt"),
    }
}

#[test]
fn browser_folder_frame_navigates_selects_current_folder_and_cancel_restores_focus() {
    let fixture = Fixture::new();
    let file = fixture.put("carpeta/niño.md", b"# fuente\r\n");
    let child = file.parent().unwrap();
    let (mut app, wake) = fixture.app();
    // A normal launch/current-directory spelling is not necessarily the
    // canonical verbatim Win32 path used by final-handle confinement checks.
    app.project_root = None;
    app.workbench.new_file("borrador.txt", TextEncoding::Utf8);
    app.workbench.insert_text("Borrador sin guardar").unwrap();
    let bytes = app.workbench.active_bytes().unwrap();
    let undo = app
        .workbench
        .active_document()
        .unwrap()
        .session
        .document()
        .undo_steps();
    #[cfg(windows)]
    {
        app.git_cwd = PathBuf::from(
            fixture
                .root
                .to_str()
                .unwrap()
                .strip_prefix(r"\\?\")
                .unwrap(),
        );
    }
    let _ = app.focus.focus(PrimaryFocus::Sidebar, &app.panels);
    app.open_prompt(PromptKind::OpenFolder);
    assert!(app.file_browser.active());
    assert_eq!(
        app.file_browser.catalog.root(),
        Some(fixture.root.as_path())
    );
    let generation = app.file_browser.catalog.generation();
    // Preparation/painting cannot accept pending directory completions.
    {
        let frame = app.prepare_frame(100, 30);
        assert_eq!(render::render(&frame), render::render(&frame));
    }
    assert_eq!(app.file_browser.catalog.nodes().len(), 1);
    wait(&mut app, &wake);
    for (width, height) in [(40, 10), (100, 30)] {
        let frame = app.frame(width, height);
        assert!((0..frame.height()).any(|row| frame.row_text(row).contains("Abrir carpeta")));
    }
    select(&mut app, child);
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.file_browser.catalog.generation() > generation);
    wait(&mut app, &wake);
    assert_eq!(app.file_browser.catalog.root(), Some(child));
    key(&mut app, KeyCode::Up, KeyModifiers::ALT);
    wait(&mut app, &wake);
    assert_eq!(
        app.file_browser.catalog.root(),
        Some(fixture.root.as_path())
    );
    select(&mut app, child);
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    wait(&mut app, &wake);
    key(&mut app, KeyCode::Enter, KeyModifiers::CONTROL);
    assert_eq!(app.project_root.as_deref(), Some(child));
    assert_eq!(app.workbench.active_bytes().unwrap(), bytes);
    assert_eq!(
        app.workbench
            .active_document()
            .unwrap()
            .session
            .document()
            .undo_steps(),
        undo
    );
    assert!(matches!(app.overlay, UiOverlay::None));
    assert_eq!(app.focus.primary(), PrimaryFocus::Sidebar);
    assert!(!app.file_browser.active());
    assert!(app.file_browser.catalog.nodes().is_empty());
    app.open_prompt(PromptKind::OpenFolder);
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    app.poll_tasks();
    assert_eq!(app.focus.primary(), PrimaryFocus::Sidebar);
    assert!(app.file_browser.catalog.nodes().is_empty());
    assert_eq!(fs::read(file).unwrap(), b"# fuente\r\n");
}

#[test]
fn browser_file_acceptance_reuses_dirty_document_and_rejects_deleted_selection() {
    let fixture = Fixture::new();
    let shared = fixture.put("shared.txt", b"original\r\n");
    let removed = fixture.put("removed.txt", b"no abrir");
    let (mut app, wake) = fixture.app();
    app.open_path(shared.clone()).unwrap();
    app.workbench.insert_text("cambio ").unwrap();
    let before = app.workbench.active_bytes().unwrap();
    let revision = app
        .workbench
        .active_document()
        .unwrap()
        .session
        .document()
        .revision();
    let undo = app
        .workbench
        .active_document()
        .unwrap()
        .session
        .document()
        .undo_steps();
    app.open_prompt(PromptKind::Open);
    wait(&mut app, &wake);
    select(&mut app, &removed);
    fs::remove_file(&removed).unwrap();
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.file_browser.active());
    assert_eq!(app.workbench.tabs().tabs().len(), 1);
    select(&mut app, &shared);
    key(&mut app, KeyCode::Enter, KeyModifiers::CONTROL);
    assert!(matches!(app.overlay, UiOverlay::None));
    assert!(app.workbench.panes().is_split());
    assert_eq!(app.workbench.tabs().tabs().len(), 1);
    assert_eq!(app.workbench.active_bytes().unwrap(), before);
    assert_eq!(
        app.workbench
            .active_document()
            .unwrap()
            .session
            .document()
            .revision(),
        revision
    );
    assert_eq!(
        app.workbench
            .active_document()
            .unwrap()
            .session
            .document()
            .undo_steps(),
        undo
    );
    app.workbench.undo().unwrap();
    assert_eq!(app.workbench.active_bytes().unwrap(), b"original\r\n");
    assert_eq!(fs::read(shared).unwrap(), b"original\r\n");
}

#[test]
fn browser_path_field_is_grapheme_editable_bounded_and_paste_never_edits_source() {
    let fixture = Fixture::new();
    let path = fixture.put("niño.txt", b"datos\xff\r\n");
    let (mut app, wake) = fixture.app();
    app.workbench.new_file("mem.txt", TextEncoding::Utf8);
    app.workbench.insert_text("sin cambios").unwrap();
    let source = app.workbench.active_bytes().unwrap();
    app.open_prompt(PromptKind::Open);
    wait(&mut app, &wake);
    key(&mut app, KeyCode::Char('l'), KeyModifiers::CONTROL);
    app.handle_event(Event::Paste("e\u{301}x".into()));
    key(&mut app, KeyCode::Home, KeyModifiers::NONE);
    key(&mut app, KeyCode::Delete, KeyModifiers::NONE);
    assert_eq!(text(&app), "x");
    let before = text(&app).to_owned();
    let field = app.file_browser.field;
    app.handle_event(Event::Paste("linea\nrota".into()));
    assert_eq!(text(&app), before);
    assert_eq!(app.file_browser.field, field);
    app.handle_event(Event::Paste("z".repeat(4096)));
    assert_eq!(text(&app), before);
    key(&mut app, KeyCode::Char('l'), KeyModifiers::CONTROL);
    assert_eq!(app.browser_field_budget(text(&app)), 4096);
    #[cfg(windows)]
    let typed_path = path.to_str().unwrap().strip_prefix(r"\\?\").unwrap();
    #[cfg(not(windows))]
    let typed_path = path.to_str().unwrap();
    app.handle_event(Event::Paste(typed_path.into()));
    assert_eq!(app.workbench.active_bytes().unwrap(), source);
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(matches!(app.overlay, UiOverlay::None));
    assert_eq!(app.workbench.active_path(), Some(path.as_path()));
    assert_eq!(app.workbench.active_bytes().unwrap(), b"datos\xff\r\n");
    assert_eq!(fs::read(path).unwrap(), b"datos\xff\r\n");
}

#[test]
fn browser_cancel_and_reopen_clear_old_metadata_and_do_not_mutate_project() {
    let first = Fixture::new();
    first.put("old.txt", b"old");
    let other = Fixture::new();
    let latest = other.put("latest.txt", b"latest");
    let (mut app, wake) = first.app();
    app.dispatch(CommandId::FileOpen);
    let id = app.file_browser.catalog.root_id().unwrap();
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.file_browser.catalog.path(id).is_none());
    app.open_prompt(PromptKind::OpenFolder);
    key(&mut app, KeyCode::Char('l'), KeyModifiers::CONTROL);
    app.handle_event(Event::Paste(other.root.to_str().unwrap().into()));
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    wait(&mut app, &wake);
    assert_eq!(app.file_browser.catalog.root(), Some(other.root.as_path()));
    assert!(app.file_browser.catalog.nodes().iter().any(|node| app
        .file_browser
        .catalog
        .path(node.id())
        .as_deref()
        == Some(latest.as_path())));
    assert!(app.file_browser.catalog.path(id).is_none());
    assert_eq!(app.project_root.as_deref(), Some(first.root.as_path()));
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    app.poll_tasks();
    assert_eq!(app.project_root.as_deref(), Some(first.root.as_path()));
    assert!(app.workbench.active_document().is_none());
}
