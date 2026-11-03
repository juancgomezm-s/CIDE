use super::*;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use jc_app::{
    explorer::{ExplorerLoadState, NodeId},
    CommandId, PrimaryFocus, Wakeup,
};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    root: PathBuf,
    base: PathBuf,
    files: Vec<PathBuf>,
    dirs: Vec<PathBuf>,
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
        assert!(base.starts_with(&workspace));
        let root = base.join(format!(
            "cide-tui-tree-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self {
            root: root.canonicalize().unwrap(),
            base,
            files: Vec::new(),
            dirs: Vec::new(),
        }
    }
    fn dir(&mut self, name: &str) {
        let path = self.root.join(name);
        fs::create_dir(&path).unwrap();
        self.dirs.push(path);
    }
    fn file(&mut self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.root.join(name);
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        f.write_all(bytes).unwrap();
        self.files.push(path.clone());
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if self.root.parent() != Some(self.base.as_path()) || self.root.is_symlink() {
            return;
        }
        for p in self.files.iter().rev() {
            if p.starts_with(&self.root) && !p.is_symlink() {
                let _ = fs::remove_file(p);
            }
        }
        for p in self.dirs.iter().rev() {
            if p.starts_with(&self.root) && !p.is_symlink() {
                let _ = fs::remove_dir(p);
            }
        }
        let _ = fs::remove_dir(&self.root);
    }
}
fn settle(app: &mut TuiApp, wake: &Wakeup, id: NodeId) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let ticket = wake.ticket();
        app.poll_tasks();
        match app.explorer.node(id).map(|n| n.state()) {
            Some(ExplorerLoadState::Ready | ExplorerLoadState::Limited) => return,
            Some(ExplorerLoadState::Queued | ExplorerLoadState::Loading) => {}
            other => panic!("Explorer did not finish: {other:?}"),
        }
        assert!(Instant::now() < deadline, "Explorer delivery deadline");
        wake.wait(ticket, Some(Duration::from_millis(20)));
    }
}
fn select(app: &mut TuiApp, suffix: &str) -> NodeId {
    let presentation = app.explorer_view.prepare(&app.explorer, 128, 256);
    let id = presentation
        .rows
        .iter()
        .find(|r| r.label.ends_with(suffix))
        .unwrap()
        .id;
    app.explorer_view.select(id);
    id
}
fn key(app: &mut TuiApp, code: KeyCode, modifiers: KeyModifiers) {
    app.handle_event(Event::Key(KeyEvent::new(code, modifiers)));
}
#[test]
fn terminal_tree_delivers_expands_and_reuses_dirty_shared_documents_with_side_and_pin() {
    let mut f = Fixture::new();
    f.dir("folder");
    let path = f.file("folder/niño.p", b"alpha\r\nbeta\r\n");
    let wake = Wakeup::default();
    let mut app = TuiApp::with_wakeup(LaunchOptions::default(), wake.clone());
    app.set_explorer_root(f.root.clone()).unwrap();
    let root = app.explorer.root_id().unwrap();
    settle(&mut app, &wake, root);
    app.dispatch(CommandId::ViewExplorer);
    let dir = select(&mut app, "folder");
    key(&mut app, KeyCode::Right, KeyModifiers::NONE);
    settle(&mut app, &wake, dir);
    select(&mut app, r"ni\u{F1}o.p");
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    let original = app.workbench.active_document().unwrap().id;
    app.workbench.insert_text("Q").unwrap();
    app.dispatch(CommandId::ViewExplorer);
    select(&mut app, r"ni\u{F1}o.p");
    key(&mut app, KeyCode::Enter, KeyModifiers::CONTROL);
    assert!(app.workbench.panes().is_split());
    assert_eq!(app.workbench.active_document().unwrap().id, original);
    assert_eq!(app.workbench.tabs().tabs().len(), 1);
    assert_eq!(app.workbench.active_bytes().unwrap(), b"Qalpha\r\nbeta\r\n");
    app.dispatch(CommandId::EditorTogglePin);
    let pin = app.workbench.panes().active();
    app.dispatch(CommandId::ViewExplorer);
    select(&mut app, r"ni\u{F1}o.p");
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.workbench.panes().view(pin).unwrap().is_pinned());
    app.workbench.undo().unwrap();
    assert_eq!(app.workbench.active_bytes().unwrap(), b"alpha\r\nbeta\r\n");
    assert_eq!(fs::read(path).unwrap(), b"alpha\r\nbeta\r\n");
}
#[test]
fn terminal_tree_keyboard_scope_refresh_cancel_resize_and_root_ids_are_coherent() {
    let mut first = Fixture::new();
    first.file("a.txt", b"source\r\n");
    first.dir("folder");
    let mut second = Fixture::new();
    second.file("new.txt", b"other\n");
    let wake = Wakeup::default();
    let mut app = TuiApp::with_wakeup(LaunchOptions::default(), wake.clone());
    app.set_explorer_root(first.root.clone()).unwrap();
    let oldroot = app.explorer.root_id().unwrap();
    settle(&mut app, &wake, oldroot);
    app.dispatch(CommandId::ViewExplorer);
    let oldfile = select(&mut app, "a.txt");
    key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
    assert!(app.workbench.active_document().is_none());
    key(&mut app, KeyCode::F(5), KeyModifiers::NONE);
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(app.focus.primary(), PrimaryFocus::Editor);
    assert_eq!(
        app.explorer.node(oldroot).unwrap().state(),
        ExplorerLoadState::Cancelled
    );
    app.set_explorer_root(second.root.clone()).unwrap();
    let newroot = app.explorer.root_id().unwrap();
    settle(&mut app, &wake, newroot);
    assert_ne!(oldroot, newroot);
    assert!(app
        .explorer
        .open_file(oldfile, &mut app.workbench, None, false, 1024)
        .is_err());
    assert!(app.workbench.active_document().is_none());
    app.dispatch(CommandId::ViewExplorer);
    let revision = app.explorer.revision();
    let bytes = app.explorer.owned_bytes();
    for (w, h) in [(120, 32), (45, 12), (80, 24), (120, 32)] {
        let _ = app.frame(w, h);
    }
    assert_eq!(app.explorer.revision(), revision);
    assert_eq!(app.explorer.owned_bytes(), bytes);
    select(&mut app, "new.txt");
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(app.workbench.active_bytes().unwrap(), b"other\n");
}
