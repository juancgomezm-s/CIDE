//! Real metadata fixtures; no source file opens until explicit acceptance.
use super::TuiApp;
use crate::{
    render,
    state::{LaunchOptions, UiOverlay},
};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use jc_app::{quick_open::QuickOpenState, CommandId, MenuId, PaneId, Wakeup};
use jc_text::TextEncoding;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    base: PathBuf,
    root: PathBuf,
}
fn link(path: &Path) -> bool {
    let metadata = fs::symlink_metadata(path).unwrap();
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
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("fixture directory: {error}"),
            }
            assert!(!link(&base));
            base = base.canonicalize().unwrap();
            assert!(base.starts_with(&workspace));
        }
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = base.join(format!(
            "tui-quick-open-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self {
            base,
            root: root.canonicalize().unwrap(),
        }
    }
    fn put(&self, name: &str, bytes: &[u8]) -> PathBuf {
        assert!(Path::new(name)
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_))));
        let path = self.root.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap()
            .write_all(bytes)
            .unwrap();
        path
    }
    fn app(&self, options: LaunchOptions) -> (TuiApp, Wakeup) {
        let wake = Wakeup::default();
        let mut app = TuiApp::with_wakeup(options, wake.clone());
        app.project_root = Some(self.root.clone());
        (app, wake)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        assert!(!link(&self.root));
        assert_eq!(self.root.canonicalize().unwrap(), self.root);
        assert_eq!(self.root.parent(), Some(self.base.as_path()));
        assert!(self
            .root
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("tui-quick-open-"));
        fs::remove_dir_all(&self.root).unwrap();
    }
}
fn key(app: &mut TuiApp, code: KeyCode, modifiers: KeyModifiers) {
    app.handle_event(Event::Key(KeyEvent::new(code, modifiers)));
}
fn input(app: &TuiApp) -> (&str, usize) {
    match &app.overlay {
        UiOverlay::QuickOpen { input, selected } => (input, *selected),
        _ => panic!("expected Quick Open overlay"),
    }
}
fn wait(app: &mut TuiApp, wake: &Wakeup) {
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        let ticket = wake.ticket();
        app.poll_tasks();
        if !matches!(
            app.quick_open.state(),
            QuickOpenState::Queued | QuickOpenState::Running
        ) {
            break;
        }
        assert!(Instant::now() < end, "Quick Open completion missing");
        wake.wait(ticket, Some(Duration::from_millis(10)));
    }
    assert!(
        matches!(
            app.quick_open.state(),
            QuickOpenState::Complete | QuickOpenState::Limited
        ),
        "{}",
        app.quick_open.message()
    );
}
fn query(app: &mut TuiApp, wake: &Wakeup, text: &str) {
    key(app, KeyCode::Char('p'), KeyModifiers::CONTROL);
    app.handle_event(Event::Paste(text.into()));
    wait(app, wake);
}

#[test]
fn keyboard_palette_and_file_menu_open_without_document_and_wakeup_owns_delivery() {
    let fixture = Fixture::new();
    let path = fixture.put("one.txt", b"one\xff\r\n");
    let (mut app, wake) = fixture.app(LaunchOptions::default());
    let mut no_project = TuiApp::new(LaunchOptions::default());
    assert!(!no_project
        .registry
        .is_enabled(CommandId::FileQuickOpen, &no_project.command_context()));
    key(&mut no_project, KeyCode::Char('p'), KeyModifiers::CONTROL);
    assert!(!matches!(no_project.overlay, UiOverlay::QuickOpen { .. }));
    assert!(app.workbench.active_document().is_none());
    assert!(app
        .registry
        .is_enabled(CommandId::FileQuickOpen, &app.command_context()));
    let ticket = wake.ticket();
    key(&mut app, KeyCode::Char('p'), KeyModifiers::CONTROL);
    let end = Instant::now() + Duration::from_secs(5);
    while !app.quick_open.ready() {
        let ticket = wake.ticket();
        assert!(Instant::now() < end);
        wake.wait(ticket, Some(Duration::from_millis(10)));
    }
    {
        let prepared = app.prepare_frame(140, 40);
        let first = render::render(&prepared);
        assert_eq!(first, render::render(&prepared));
    }
    assert!(app.quick_open.results().is_empty());
    assert!(app.quick_open.ready());
    wait(&mut app, &wake);
    assert!(wake.ticket() > ticket);
    assert_eq!(app.quick_open.results()[0].path(), path);
    for (width, height) in [(45, 15), (140, 40)] {
        let before = (
            input(&app).0.to_owned(),
            input(&app).1,
            app.quick_open.counters(),
        );
        {
            let prepared = app.prepare_frame(width, height);
            assert_eq!(render::render(&prepared), render::render(&prepared));
        }
        assert_eq!(
            (
                input(&app).0.to_owned(),
                input(&app).1,
                app.quick_open.counters()
            ),
            before
        );
    }
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    key(
        &mut app,
        KeyCode::Char('p'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    app.handle_event(Event::Paste(
        "Abrir archivo del proyecto rápidamente".into(),
    ));
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(matches!(app.overlay, UiOverlay::QuickOpen { .. }));
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    app.dispatch(CommandId::MenuFile);
    let entries = app.menu_commands(MenuId::File);
    let wanted = entries
        .iter()
        .position(|entry| *entry == Some(CommandId::FileQuickOpen))
        .unwrap();
    for _ in 0..entries.len() {
        if matches!(app.overlay, UiOverlay::Menu { menu: MenuId::File, selected } if selected == wanted)
        {
            break;
        }
        key(&mut app, KeyCode::Down, KeyModifiers::NONE);
    }
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(matches!(app.overlay, UiOverlay::QuickOpen { .. }));
    assert!(app.workbench.active_document().is_none());
    assert_eq!(fs::read(path).unwrap(), b"one\xff\r\n");
}

#[test]
fn paste_matches_full_fold_path_and_rejects_control_and_oversized_queries_without_editing_input() {
    let fixture = Fixture::new();
    let target = fixture.put("nested/Straße-Σς.txt", b"\xff");
    fixture.put("another.txt", b"x");
    let (mut app, wake) = fixture.app(LaunchOptions::default());
    query(&mut app, &wake, "NESTED/STRASSE-ΣΣ");
    assert_eq!(app.quick_open.results().len(), 1);
    assert_eq!(app.quick_open.results()[0].path(), target);
    let before = input(&app).0.to_owned();
    for paste in ["\n", "\0", "\u{202e}", &"x".repeat(257)] {
        app.handle_event(Event::Paste(paste.into()));
        assert_eq!(input(&app).0, before);
        assert_eq!(app.quick_open.results().len(), 1);
    }
    assert!(app.workbench.active_document().is_none());
}

#[test]
fn enter_opens_original_unknown_legacy_bytes_then_requires_encoding_choice() {
    let fixture = Fixture::new();
    let bytes = b"legacy \x81\xff\r\nfinal";
    let target = fixture.put("legacy.txt", bytes);
    let (mut app, wake) = fixture.app(LaunchOptions::default());
    query(&mut app, &wake, "legacy");
    assert!(app.workbench.active_document().is_none());
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(matches!(app.overlay, UiOverlay::EncodingChoice));
    assert_eq!(app.workbench.active_bytes().unwrap(), bytes);
    assert_eq!(app.workbench.active_encoding(), None);
    key(&mut app, KeyCode::Char('1'), KeyModifiers::NONE);
    assert_eq!(
        app.workbench.active_encoding(),
        Some(TextEncoding::Windows1252)
    );
    assert_eq!(fs::read(target).unwrap(), bytes);
}

#[test]
fn control_enter_reuses_dirty_document_undo_and_reference_pin_without_reloading_disk() {
    let fixture = Fixture::new();
    let original = b"original\r\n";
    let target = fixture.put("shared.txt", original);
    let (mut app, wake) = fixture.app(LaunchOptions {
        files: vec![target.clone()],
        encoding: Some(TextEncoding::Windows1252),
    });
    app.workbench.insert_text("dirty £").unwrap();
    let tab = app.workbench.tabs().active_id().unwrap();
    let (id, revision, undo) = {
        let doc = app.workbench.active_document().unwrap();
        (
            doc.id,
            doc.session.document().revision(),
            doc.session.document().undo_steps(),
        )
    };
    let bytes = app.workbench.active_bytes().unwrap();
    query(&mut app, &wake, "shared");
    // An external fixture write would be visible if acceptance reloaded an
    // existing session. Both views must instead retain the dirty live bytes.
    fs::write(&target, b"external replacement").unwrap();
    key(&mut app, KeyCode::Enter, KeyModifiers::CONTROL);
    assert!(matches!(app.overlay, UiOverlay::None));
    assert_eq!(app.workbench.panes().active(), PaneId::Secondary);
    assert!(app.workbench.panes().is_split());
    assert_eq!(app.workbench.tabs().tabs().len(), 1);
    let doc = app.workbench.active_document().unwrap();
    assert_eq!(doc.id, id);
    assert_eq!(doc.session.document().revision(), revision);
    assert_eq!(doc.session.document().undo_steps(), undo);
    assert_eq!(app.workbench.active_bytes().unwrap(), bytes);
    for pane in [PaneId::Primary, PaneId::Secondary] {
        assert_eq!(
            app.workbench
                .panes()
                .view(pane)
                .unwrap()
                .location()
                .unwrap()
                .tab,
            tab
        );
    }
    app.dispatch(CommandId::EditorTogglePin);
    assert!(app
        .workbench
        .panes()
        .view(PaneId::Secondary)
        .unwrap()
        .is_pinned());
    app.workbench.undo().unwrap();
    assert_eq!(app.workbench.active_bytes().unwrap(), original);
    assert!(app.workbench.focus_editor_pane(PaneId::Primary));
    assert_eq!(app.workbench.active_bytes().unwrap(), original);
    app.workbench.redo().unwrap();
    assert_eq!(app.workbench.active_bytes().unwrap(), bytes);
    assert_eq!(app.workbench.active_document().unwrap().id, id);
    assert!(app
        .workbench
        .panes()
        .view(PaneId::Secondary)
        .unwrap()
        .is_pinned());
    let unrelated = app
        .workbench
        .new_file("untitled-history.txt", TextEncoding::Utf8);
    assert_ne!(unrelated, tab);
    assert_eq!(
        app.workbench
            .panes()
            .view(PaneId::Secondary)
            .unwrap()
            .location()
            .unwrap()
            .tab,
        tab
    );
    key(&mut app, KeyCode::Left, KeyModifiers::ALT);
    assert_eq!(app.workbench.tabs().active_id(), Some(tab));
    assert_eq!(app.workbench.active_document().unwrap().id, id);
    assert_eq!(app.workbench.active_bytes().unwrap(), bytes);
    assert_eq!(fs::read(target).unwrap(), b"external replacement");
}

#[test]
fn supersession_root_change_before_prepare_and_escape_cannot_open_stale_candidates() {
    let fixture = Fixture::new();
    fixture.put("old.txt", b"old");
    let latest = fixture.put("latest.txt", b"latest");
    let other = Fixture::new();
    let (mut app, wake) = fixture.app(LaunchOptions::default());
    key(&mut app, KeyCode::Char('p'), KeyModifiers::CONTROL);
    app.handle_event(Event::Paste("old".into()));
    for _ in 0..3 {
        key(&mut app, KeyCode::Backspace, KeyModifiers::NONE);
    }
    app.handle_event(Event::Paste("latest".into()));
    wait(&mut app, &wake);
    assert_eq!(app.quick_open.results().len(), 1);
    assert_eq!(app.quick_open.results()[0].path(), latest);
    app.project_root = Some(other.root.clone());
    {
        let _ = app.prepare_frame(140, 40);
    }
    assert!(app.quick_open.results().is_empty());
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.workbench.active_document().is_none());
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(matches!(app.overlay, UiOverlay::None));
    assert_eq!(app.quick_open.state(), QuickOpenState::Idle);
    app.project_root = Some(fixture.root.clone());
    query(&mut app, &wake, "old");
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    app.poll_tasks();
    assert!(app.quick_open.results().is_empty());
    assert!(app.workbench.active_document().is_none());
}

#[test]
fn deleted_or_link_replaced_target_keeps_overlay_and_opens_no_document() {
    let fixture = Fixture::new();
    let target = fixture.put("target.txt", b"source");
    let outside = Fixture::new();
    let external = outside.put("outside.txt", b"external");
    let (mut app, wake) = fixture.app(LaunchOptions::default());
    query(&mut app, &wake, "target");
    fs::remove_file(&target).unwrap();
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(matches!(app.overlay, UiOverlay::QuickOpen { .. }));
    assert!(app.workbench.active_document().is_none());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        assert!(std::process::Command::new("cmd.exe")
            .raw_arg("/D /V:OFF /C mklink /J \"%JC_TEST_LINK%\" \"%JC_TEST_TARGET%\"")
            .env("JC_TEST_LINK", &target)
            .env("JC_TEST_TARGET", &outside.root)
            .creation_flags(0x0800_0000)
            .output()
            .unwrap()
            .status
            .success());
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(matches!(app.overlay, UiOverlay::QuickOpen { .. }));
        assert!(app.workbench.active_document().is_none());
        fs::remove_dir(&target).unwrap();
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&external, &target).unwrap();
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(matches!(app.overlay, UiOverlay::QuickOpen { .. }));
        assert!(app.workbench.active_document().is_none());
    }
    assert_eq!(fs::read(external).unwrap(), b"external");
}
