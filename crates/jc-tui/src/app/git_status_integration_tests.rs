//! Windows-only real Git fixtures. Preparation writes only their owned repository.
#![cfg(windows)]
use super::TuiApp;
use crate::state::{LaunchOptions, UiOverlay};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use jc_app::{
    git_diff::GitDiffState, git_status_tasks::GitStatusTaskState, CommandId, PaneId, PrimaryFocus,
    SidebarView, Wakeup,
};
use jc_text::TextEncoding;
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
static SERIAL: Mutex<()> = Mutex::new(());
const TRACKED: &str = "tracked.txt";
const UNTRACKED: &str = "nuevo-é[1].txt";

fn installed_git() -> PathBuf {
    if let Some(path) = std::env::var_os("JC_GIT_TEST_EXE") {
        return PathBuf::from(path).canonicalize().unwrap();
    }
    std::env::split_paths(&std::env::var_os("PATH").expect("Installed Git required"))
        .map(|path| path.join("git.exe"))
        .find(|path| path.is_file())
        .expect("Installed Git required; fixtures never install it")
        .canonicalize()
        .unwrap()
}
struct Fixture {
    workspace: PathBuf,
    base: PathBuf,
    root: PathBuf,
    git: PathBuf,
}
#[derive(Debug, PartialEq, Eq)]
struct DiskState {
    index: Vec<u8>,
    config: Vec<u8>,
    head: Vec<u8>,
    branch: Vec<u8>,
    tracked: Vec<u8>,
    untracked: Vec<u8>,
}
impl Fixture {
    fn new() -> Self {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .canonicalize()
            .unwrap();
        let work = workspace.join("work");
        if !work.exists() {
            fs::create_dir(&work).unwrap();
        }
        let ordinary = |path: &Path| {
            use std::os::windows::fs::MetadataExt;
            fs::symlink_metadata(path).unwrap().file_attributes() & 0x400 == 0
        };
        assert!(ordinary(&work));
        let work = work.canonicalize().unwrap();
        assert!(work.starts_with(&workspace));
        let base = work.join("test-temp");
        if !base.exists() {
            fs::create_dir(&base).unwrap();
        }
        assert!(ordinary(&base));
        let base = base.canonicalize().unwrap();
        assert!(base.starts_with(&workspace));
        let root = base.join(format!(
            "jc git status tui {}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let fixture = Self {
            root: root.canonicalize().unwrap(),
            workspace,
            base,
            git: installed_git(),
        };
        fixture.git(&["init", "--quiet", "--template=", "--initial-branch=main"]);
        fixture.create(TRACKED, b"head \xff\r\nend");
        let head_blob = fixture.oid(&["hash-object", "-w", "--no-filters", "--", TRACKED]);
        fixture.cache(&head_blob);
        let tree = fixture.oid(&["write-tree"]);
        let commit = fixture.oid(&["commit-tree", &tree, "-m", "synthetic status fixture"]);
        fixture.git(&["update-ref", "HEAD", &commit]);
        fixture.rewrite(TRACKED, b"index content \x81\r\n");
        let index_blob = fixture.oid(&["hash-object", "-w", "--no-filters", "--", TRACKED]);
        fixture.cache(&index_blob);
        fixture.rewrite(TRACKED, b"work \xfe\r\nlast\n");
        fixture.create(UNTRACKED, b"untracked \x80\xef\xbb\xbf\r");
        fixture
    }
    fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }
    fn create(&self, name: &str, bytes: &[u8]) {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.path(name))
            .unwrap()
            .write_all(bytes)
            .unwrap();
    }
    fn rewrite(&self, name: &str, bytes: &[u8]) {
        OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(self.path(name))
            .unwrap()
            .write_all(bytes)
            .unwrap();
    }
    // Known small-output plumbing, solely to prepare an owned fixture. No shell,
    // global configuration, hooks, filters or writes in the project repository.
    fn git(&self, args: &[&str]) -> Vec<u8> {
        let mut command = Command::new(&self.git);
        command.env_clear();
        for key in ["SYSTEMROOT", "WINDIR", "TEMP", "TMP"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        let output = command
            .current_dir(&self.root)
            .args([
                "--no-pager",
                "-c",
                "core.hooksPath=NUL",
                "-c",
                "core.fsmonitor=false",
                "-c",
                "core.autocrlf=false",
                "-c",
                "gc.auto=0",
                "-c",
                "maintenance.auto=false",
            ])
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_SYSTEM", "NUL")
            .env("GIT_CONFIG_GLOBAL", "NUL")
            .env("GIT_CONFIG_COUNT", "0")
            .env("GIT_AUTHOR_NAME", "JC fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
            .env("GIT_COMMITTER_NAME", "JC fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(output.stdout.len() <= 64 * 1024 && output.stderr.len() <= 16 * 1024);
        assert!(
            output.status.success(),
            "Fixture plumbing failed: {:?}",
            output.stderr
        );
        output.stdout
    }
    fn oid(&self, args: &[&str]) -> String {
        std::str::from_utf8(&self.git(args))
            .unwrap()
            .trim()
            .to_owned()
    }
    fn cache(&self, oid: &str) {
        self.git(&[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("100644,{oid},{TRACKED}"),
        ]);
    }
    fn disk(&self) -> DiskState {
        DiskState {
            index: fs::read(self.path(".git/index")).unwrap(),
            config: fs::read(self.path(".git/config")).unwrap(),
            head: fs::read(self.path(".git/HEAD")).unwrap(),
            branch: fs::read(self.path(".git/refs/heads/main")).unwrap(),
            tracked: fs::read(self.path(TRACKED)).unwrap(),
            untracked: fs::read(self.path(UNTRACKED)).unwrap(),
        }
    }
    fn app(&self, editor: bool) -> (TuiApp, Wakeup) {
        let wake = Wakeup::default();
        let options = if editor {
            LaunchOptions {
                files: vec![self.path(TRACKED)],
                encoding: Some(TextEncoding::Windows1252),
            }
        } else {
            LaunchOptions::default()
        };
        let mut app = TuiApp::with_wakeup(options, wake.clone());
        app.project_root = Some(self.root.clone());
        (app, wake)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let target = self.root.canonicalize().unwrap();
        assert_eq!(target, self.root);
        assert_eq!(target.parent(), Some(self.base.as_path()));
        assert!(target.starts_with(&self.workspace));
        assert_ne!(target, self.workspace);
        assert!(target
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("jc git status tui "));
        // Git's own fixture setup can mark loose objects read-only. Inspect the
        // complete private tree without following reparse points before changing
        // only that attribute on ordinary files; production Git stays read-only.
        use std::os::windows::fs::MetadataExt;
        let mut directories = vec![target.clone()];
        let mut files = Vec::new();
        let mut visited = 0usize;
        while let Some(directory) = directories.pop() {
            assert!(directory.starts_with(&target));
            let metadata = fs::symlink_metadata(&directory).unwrap();
            assert_eq!(metadata.file_attributes() & 0x400, 0);
            assert!(metadata.is_dir());
            for entry in fs::read_dir(directory).unwrap() {
                let path = entry.unwrap().path();
                assert!(path.starts_with(&target));
                visited += 1;
                assert!(visited <= 16_384, "owned fixture cleanup quota");
                let metadata = fs::symlink_metadata(&path).unwrap();
                assert!(!metadata.file_type().is_symlink());
                assert_eq!(metadata.file_attributes() & 0x400, 0);
                if metadata.is_dir() {
                    directories.push(path);
                } else {
                    assert!(metadata.is_file());
                    files.push((path, metadata.permissions()));
                }
            }
        }
        for (path, mut permissions) in files {
            if permissions.readonly() {
                #[allow(clippy::permissions_set_readonly_false)]
                // Windows readonly bit; fixture only.
                permissions.set_readonly(false);
                fs::set_permissions(path, permissions).unwrap();
            }
        }
        fs::remove_dir_all(target).unwrap();
    }
}
fn key(app: &mut TuiApp, code: KeyCode) {
    app.handle_event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
}
fn wait_status(app: &mut TuiApp, wake: &Wakeup) -> GitStatusTaskState {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let ticket = wake.ticket();
        app.poll_tasks();
        let state = app.git_status.state();
        if !matches!(
            state,
            GitStatusTaskState::Queued | GitStatusTaskState::Running
        ) {
            return state;
        }
        assert!(
            Instant::now() < deadline,
            "status delivery timed out: {}",
            app.git_status.message()
        );
        wake.wait(ticket, Some(Duration::from_millis(20)));
    }
}
fn request_ready(app: &mut TuiApp, wake: &Wakeup) {
    let before = wake.ticket();
    app.dispatch(CommandId::GitRepositoryStatus);
    assert_eq!(
        wait_status(app, wake),
        GitStatusTaskState::Ready,
        "{}",
        app.git_status.message()
    );
    assert!(
        wake.ticket() > before,
        "completion must wake without further keyboard input"
    );
}
fn rows(app: &mut TuiApp) -> BTreeMap<String, String> {
    app.git_status
        .prepare(256, 256)
        .rows
        .into_iter()
        .map(|row| (row.path, row.flags))
        .collect()
}
fn assert_status_rows(app: &mut TuiApp) {
    assert_eq!(
        rows(app),
        BTreeMap::from([
            (TRACKED.into(), "MM".into()),
            (UNTRACKED.into(), "??".into())
        ])
    );
}

#[test]
fn blame_keys_page_owner_revision_return_and_raw_sources_preserve_editor_and_git() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let mut original = Vec::new();
    for n in 0..140 {
        original.extend_from_slice(format!("fila {n}\r\n").as_bytes());
        if n == 1 {
            original.extend_from_slice(b"byte \xff\r\n");
        }
    }
    fixture.rewrite(TRACKED, &original);
    let old = fixture.oid(&["rev-parse", "HEAD"]);
    let blob = fixture.oid(&["hash-object", "-w", "--no-filters", "--", TRACKED]);
    fixture.cache(&blob);
    let tree = fixture.oid(&["write-tree"]);
    let base = fixture.oid(&[
        "commit-tree",
        &tree,
        "-p",
        &old,
        "-m",
        "Origen de las lineas",
    ]);
    let mut changed = b"agregada\r\n".to_vec();
    changed.extend_from_slice(&original);
    fixture.rewrite(TRACKED, &changed);
    let blob = fixture.oid(&["hash-object", "-w", "--no-filters", "--", TRACKED]);
    fixture.cache(&blob);
    let tree = fixture.oid(&["write-tree"]);
    let latest = fixture.oid(&["commit-tree", &tree, "-p", &base, "-m", "Inserta cabecera"]);
    fixture.git(&["update-ref", "HEAD", &latest]);
    fixture.rewrite(TRACKED, b"disco independiente\r\n");
    let disk = fixture.disk();
    let (mut app, wake) = fixture.app(true);
    app.workbench.insert_text("sin guardar ").unwrap();
    let editor = app.workbench.active_bytes().unwrap();
    let panes = format!("{:?}", app.workbench.panes());
    request_ready(&mut app, &wake);
    let tracked = app
        .git_status
        .prepare(256, 256)
        .rows
        .iter()
        .find(|r| r.path == TRACKED)
        .unwrap()
        .ordinal;
    app.git_status.first();
    app.git_status.move_selection(tracked as isize);
    app.git_history.encoding = Some(TextEncoding::Utf8);
    let wait = |app: &mut TuiApp| {
        let until = Instant::now() + Duration::from_secs(15);
        loop {
            let ticket = wake.ticket();
            app.poll_tasks();
            match app.git_revision.state() {
                jc_app::git_revision::GitRevisionState::Ready => break,
                jc_app::git_revision::GitRevisionState::Queued
                | jc_app::git_revision::GitRevisionState::Running => {}
                s => panic!("blame delivery: {s:?}"),
            };
            assert!(Instant::now() < until);
            wake.wait(ticket, Some(Duration::from_millis(20)));
        }
    };
    key(&mut app, KeyCode::Char('l'));
    wait(&mut app);
    let page = app.git_revision.blame().unwrap();
    assert_eq!(page.start, latest);
    assert_eq!(page.rows.len(), 128);
    assert_eq!(page.source.as_slice(), changed);
    assert_eq!(page.commits[page.rows[0].commit.unwrap()].commit, latest);
    assert_eq!(page.commits[page.rows[1].commit.unwrap()].commit, base);
    assert_eq!(page.rows[1].original_line, 0);
    let list = app.prepare_git_history(100, 10);
    assert_eq!(list.rows.len(), 10);
    assert!(list.title.contains("Anotaciones"));
    let detail = app.prepare_historical_diff(120, 6, 10);
    assert!(detail.rows.iter().any(|r| r.text.contains("agregada")));
    assert!(detail.rows.iter().any(|r| r.text.contains("\\xFF")));
    assert!(!app.git_revision.ready());
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Enter);
    wait(&mut app);
    assert!(
        matches!(&app.git_revision.comparison().unwrap().origin,jc_app::git_revision::ComparisonOrigin::Historical{commit,..}if commit==&base)
    );
    assert!(app.git_revision.blame().is_some());
    assert_eq!(app.focus.primary(), PrimaryFocus::BottomPanel);
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.focus.primary(), PrimaryFocus::Sidebar);
    key(&mut app, KeyCode::Char('n'));
    wait(&mut app);
    let page = app.git_revision.blame().unwrap();
    assert_eq!(page.first, 128);
    assert_eq!(page.rows.len(), 14);
    key(&mut app, KeyCode::Char('p'));
    wait(&mut app);
    assert_eq!(app.git_revision.blame().unwrap().first, 0);
    key(&mut app, KeyCode::Char('r'));
    wait(&mut app);
    assert_eq!(app.git_revision.blame().unwrap().start, latest);
    key(&mut app, KeyCode::Esc);
    assert!(!app.git_history.active);
    assert!(app.git_revision.blame().is_none());
    assert_eq!(app.workbench.active_bytes().unwrap(), editor);
    assert_eq!(format!("{:?}", app.workbench.panes()), panes);
    assert_eq!(fixture.disk(), disk);
}

#[test]
fn path_history_keys_page_compare_and_return_without_mutating_buffer_or_git() {
    let _serial = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new();
    let head_blob = fixture.oid(&["rev-parse", "HEAD:tracked.txt"]);
    let index_blob = fixture.oid(&["rev-parse", ":tracked.txt"]);
    let mut parent = fixture.oid(&["rev-parse", "HEAD"]);
    // CLI plumbing prepares only this owned repository; all subsequent history
    // and comparison requests go through the embedded Rust provider.
    for number in 0..27 {
        fixture.cache(if number % 2 == 0 {
            &index_blob
        } else {
            &head_blob
        });
        let tree = fixture.oid(&["write-tree"]);
        let summary = format!("Cambio del archivo {number}");
        parent = fixture.oid(&["commit-tree", &tree, "-p", &parent, "-m", &summary]);
    }
    fixture.git(&["update-ref", "HEAD", &parent]);
    let disk = fixture.disk();
    let (mut app, wake) = fixture.app(true);
    app.workbench.insert_text("sin guardar ").unwrap();
    let original = app.workbench.active_bytes().unwrap();
    let before_panes = format!("{:?}", app.workbench.panes());
    request_ready(&mut app, &wake);
    let tracked = app
        .git_status
        .prepare(256, 256)
        .rows
        .iter()
        .find(|row| row.path == TRACKED)
        .unwrap()
        .ordinal;
    app.git_status.first();
    app.git_status.move_selection(tracked as isize);
    key(&mut app, KeyCode::Char('a'));
    assert!(app.git_history.active);
    let wait = |app: &mut TuiApp| {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let ticket = wake.ticket();
            app.poll_tasks();
            match app.git_revision.state() {
                jc_app::git_revision::GitRevisionState::Ready => break,
                jc_app::git_revision::GitRevisionState::Queued
                | jc_app::git_revision::GitRevisionState::Running => {}
                other => panic!(
                    "Path history delivery failed: {other:?}: {}",
                    app.git_revision.message()
                ),
            }
            assert!(Instant::now() < deadline);
            wake.wait(ticket, Some(Duration::from_millis(20)));
        }
    };
    wait(&mut app);
    let page = app.git_revision.path_history().unwrap();
    assert_eq!(page.start, parent);
    assert_eq!(page.path, TRACKED.as_bytes());
    assert_eq!(page.entries.len(), 24);
    assert!(page.next.is_some());
    let first_page_commits = page
        .entries
        .iter()
        .map(|entry| entry.commit.clone())
        .collect::<Vec<_>>();
    let presentation = app.prepare_git_history(100, 10);
    assert!(presentation.title.contains("Historial del archivo"));
    assert!(presentation.message.contains("N más"));
    assert_eq!(presentation.rows.len(), 10);
    assert_eq!(presentation.total, 24);
    key(&mut app, KeyCode::Enter);
    wait(&mut app);
    assert_eq!(app.focus.primary(), PrimaryFocus::BottomPanel);
    assert_eq!(
        app.git_revision.comparison().unwrap().path,
        TRACKED.as_bytes()
    );
    assert_eq!(
        app.git_revision
            .comparison()
            .unwrap()
            .model
            .source_bytes(jc_app::diff_model::DiffSide::Right),
        b"index content \x81\r\n"
    );
    assert!(app.git_revision.path_history().is_some());
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.focus.primary(), PrimaryFocus::Sidebar);
    key(&mut app, KeyCode::Char('n'));
    wait(&mut app);
    let page = app.git_revision.path_history().unwrap();
    assert_eq!(page.entries.len(), 4);
    assert!(page.next.is_none());
    assert!(page
        .entries
        .iter()
        .all(|entry| !first_page_commits.contains(&entry.commit)));
    let last = page.entries.last().unwrap();
    assert!(last.parent.is_none());
    key(&mut app, KeyCode::End);
    key(&mut app, KeyCode::Enter);
    wait(&mut app);
    assert!(app.git_revision.comparison().unwrap().before_oid.is_none());
    key(&mut app, KeyCode::Esc);
    key(&mut app, KeyCode::Char('r'));
    wait(&mut app);
    assert_eq!(
        app.git_revision.path_history().unwrap().entries[0].commit,
        parent
    );
    key(&mut app, KeyCode::Char('h'));
    assert!(app.git_history.active);
    key(&mut app, KeyCode::Char('h'));
    assert!(!app.git_history.active);
    assert_eq!(app.workbench.active_bytes().unwrap(), original);
    assert_eq!(format!("{:?}", app.workbench.panes()), before_panes);
    assert_eq!(fixture.disk(), disk);

    // With a genuinely clean status, A uses the open file only as a fallback.
    fixture.rewrite(TRACKED, b"index content \x81\r\n");
    fs::create_dir_all(fixture.path(".git/info")).unwrap();
    fs::write(fixture.path(".git/info/exclude"), b"*\n").unwrap();
    let clean_disk = fixture.disk();
    request_ready(&mut app, &wake);
    assert!(app.git_status.selected_path().is_none());
    key(&mut app, KeyCode::Char('a'));
    wait(&mut app);
    assert_eq!(
        app.git_revision.path_history().unwrap().path,
        TRACKED.as_bytes()
    );
    assert_eq!(app.workbench.active_bytes().unwrap(), original);
    assert_eq!(fixture.disk(), clean_disk);
}

#[test]
fn historical_keys_deliver_root_file_and_diff_without_editing_dirty_buffer_or_git() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let disk = fixture.disk();
    let (mut app, wake) = fixture.app(true);
    app.workbench.insert_text("sin guardar ").unwrap();
    let original = app.workbench.active_bytes().unwrap();
    let revision = app
        .workbench
        .active_document()
        .unwrap()
        .session
        .document()
        .revision();
    request_ready(&mut app, &wake);
    key(&mut app, KeyCode::Char('h'));
    assert!(app.git_history.active);
    key(&mut app, KeyCode::Enter);
    assert!(app.git_history.files);
    let wait = |app: &mut TuiApp| {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let ticket = wake.ticket();
            app.poll_tasks();
            match app.git_revision.state() {
                jc_app::git_revision::GitRevisionState::Ready => break,
                jc_app::git_revision::GitRevisionState::Queued
                | jc_app::git_revision::GitRevisionState::Running => {}
                other => panic!(
                    "Historical delivery failed: {other:?}: {}",
                    app.git_revision.message()
                ),
            }
            assert!(Instant::now() < deadline);
            wake.wait(ticket, Some(Duration::from_millis(20)));
        }
    };
    wait(&mut app);
    let r = app.git_revision.revision().unwrap();
    assert!(r.parent.is_none());
    app.git_history.selected = r
        .files
        .iter()
        .position(|f| f.path == TRACKED.as_bytes())
        .unwrap();
    key(&mut app, KeyCode::Enter);
    wait(&mut app);
    assert_eq!(app.focus.primary(), PrimaryFocus::BottomPanel);
    let c = app.git_revision.comparison().unwrap();
    assert!(c.before_oid.is_none());
    assert!(c.after_oid.is_some());
    assert_eq!(
        c.model.source_bytes(jc_app::diff_model::DiffSide::Right),
        b"head \xff\r\nend"
    );
    let frame = app.prepare_frame(120, 35);
    assert!(frame.git_status.title.contains("Archivos"));
    drop(frame);
    key(&mut app, KeyCode::Char('e'));
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.focus.primary(), PrimaryFocus::Sidebar);
    key(&mut app, KeyCode::Esc);
    assert!(!app.git_history.files);
    key(&mut app, KeyCode::Char('h'));
    assert!(!app.git_history.active);
    assert_eq!(app.workbench.active_bytes().unwrap(), original);
    assert_eq!(
        app.workbench
            .active_document()
            .unwrap()
            .session
            .document()
            .revision(),
        revision
    );
    assert_eq!(fixture.disk(), disk);
}
#[test]
fn repository_status_without_document_or_encoding_delivers_mm_and_untracked_via_wakeup() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let before = fixture.disk();
    let (mut app, wake) = fixture.app(false);
    assert!(app.workbench.active_document().is_none());
    assert!(app
        .registry
        .is_enabled(CommandId::GitRepositoryStatus, &app.command_context()));
    let ticket_before = wake.ticket();
    app.dispatch(CommandId::GitRepositoryStatus);
    let deadline = Instant::now() + Duration::from_secs(15);
    while !app.git_status.ready() {
        let ticket = wake.ticket();
        assert!(
            Instant::now() < deadline,
            "status worker did not signal completion"
        );
        if !app.git_status.ready() {
            wake.wait(ticket, Some(Duration::from_millis(20)));
        }
    }
    // A ready completion is not accepted through preparation; poll_tasks owns
    // delivery and its panic recovery decision even with no subsequent input.
    {
        let prepared = app.prepare_frame(140, 40);
        assert_eq!(prepared.git_status.total, 0);
    }
    assert_ne!(app.git_status.state(), GitStatusTaskState::Ready);
    assert_eq!(wait_status(&mut app, &wake), GitStatusTaskState::Ready);
    assert!(wake.ticket() > ticket_before);
    assert_status_rows(&mut app);
    assert_eq!(app.panels.sidebar(), SidebarView::GitStatus);
    assert_eq!(app.focus.primary(), PrimaryFocus::Sidebar);
    assert_eq!(app.tasks.worker_count(), 0);
    assert_eq!(app.git_diff.state(), GitDiffState::Idle);
    assert!(app.workbench.tabs().tabs().is_empty());
    assert_eq!(fixture.disk(), before);
}

#[test]
fn status_selection_enter_and_repeated_composition_preserve_shared_editor_and_disk() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let disk = fixture.disk();
    let (mut app, wake) = fixture.app(true);
    app.workbench.insert_text("unsaved £\r\n").unwrap();
    let tab = app.workbench.tabs().active_id().unwrap();
    assert!(app.workbench.split_editor_right());
    assert!(app.workbench.focus_editor_pane(PaneId::Secondary));
    assert!(app.workbench.jump_to_location(tab, 3));
    assert!(app.workbench.focus_editor_pane(PaneId::Primary));
    let bytes = app.workbench.active_bytes().unwrap();
    let doc = app.workbench.active_document().unwrap();
    let id = doc.id;
    let revision = doc.session.document().revision();
    let undo = doc.session.document().undo_steps();
    let locations: Vec<_> = [PaneId::Primary, PaneId::Secondary]
        .into_iter()
        .map(|pane| {
            let view = app.workbench.panes().view(pane).unwrap();
            (view.view_id(), view.location(), view.anchor())
        })
        .collect();
    request_ready(&mut app, &wake);
    assert_status_rows(&mut app);
    for code in [
        KeyCode::Down,
        KeyCode::Up,
        KeyCode::End,
        KeyCode::Home,
        KeyCode::Enter,
    ] {
        key(&mut app, code);
    }
    assert!(matches!(app.overlay, UiOverlay::None));
    for (width, height) in [(45, 15), (140, 40)] {
        let prepared = app.prepare_frame(width, height);
        let metadata = format!("{:?}", prepared.git_status);
        let first = crate::render::render(&prepared);
        let second = crate::render::render(&prepared);
        assert_eq!(first, second);
        assert_eq!(format!("{:?}", prepared.git_status), metadata);
        assert_eq!(prepared.git_status.total, 2);
    }
    assert_eq!(app.workbench.tabs().tabs().len(), 1);
    assert_eq!(app.workbench.active_document().unwrap().id, id);
    assert_eq!(app.workbench.active_bytes().unwrap(), bytes);
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
    let after: Vec<_> = [PaneId::Primary, PaneId::Secondary]
        .into_iter()
        .map(|pane| {
            let view = app.workbench.panes().view(pane).unwrap();
            (view.view_id(), view.location(), view.anchor())
        })
        .collect();
    assert_eq!(after, locations);
    assert_eq!(fixture.disk(), disk);
    app.workbench.focus_editor_pane(PaneId::Secondary);
    app.workbench.undo().unwrap();
    assert_eq!(app.workbench.active_bytes().unwrap(), disk.tracked);
}

#[test]
fn embedded_git_opens_status_and_diff_without_an_executable_prompt() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let before = fixture.disk();
    let (mut status, wake) = fixture.app(false);
    status.dispatch(CommandId::GitRepositoryStatus);
    assert!(matches!(status.overlay, UiOverlay::None));
    assert_eq!(wait_status(&mut status, &wake), GitStatusTaskState::Ready);
    assert_status_rows(&mut status);
    assert_eq!(status.tasks.worker_count(), 0);
    assert_eq!(status.git_diff.state(), GitDiffState::Idle);

    let (mut manual, manual_wake) = fixture.app(true);
    manual.dispatch(CommandId::GitRepositoryStatus);
    assert_eq!(
        wait_status(&mut manual, &manual_wake),
        GitStatusTaskState::Ready
    );
    key(&mut manual, KeyCode::Esc);
    assert!(matches!(manual.overlay, UiOverlay::None));
    manual.dispatch(CommandId::ViewExplorer);
    manual.dispatch(CommandId::GitConfigureExecutable);
    assert_eq!(manual.git_diff.state(), GitDiffState::Idle);
    assert_eq!(manual.tasks.worker_count(), 0);
    assert_eq!(manual.git_status.state(), GitStatusTaskState::Ready);

    let (mut diff, wake) = fixture.app(true);
    diff.dispatch(CommandId::GitDiffHead);
    assert!(matches!(diff.overlay, UiOverlay::None));
    let deadline = Instant::now() + Duration::from_secs(15);
    while diff.tasks.outstanding() > 0 {
        let ticket = wake.ticket();
        diff.poll_tasks();
        assert!(Instant::now() < deadline);
        if diff.tasks.outstanding() > 0 {
            wake.wait(ticket, Some(Duration::from_millis(20)));
        }
    }
    assert_eq!(
        diff.git_diff.state(),
        GitDiffState::Ready,
        "{}",
        diff.git_diff.message()
    );
    assert!(!matches!(
        diff.git_status.state(),
        GitStatusTaskState::Queued | GitStatusTaskState::Running | GitStatusTaskState::Ready
    ));
    assert_eq!(fixture.disk(), before);
}

#[test]
fn cancel_refresh_and_context_changes_invalidate_before_prepare_without_consuming_completion() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let second = Fixture::new();
    second.create("second-only.txt", b"another \xff\r\n");
    let disk = fixture.disk();
    let other_disk = second.disk();
    let (mut app, wake) = fixture.app(false);
    request_ready(&mut app, &wake);
    app.project_root = Some(second.root.clone());
    {
        let prepared = app.prepare_frame(140, 40);
        assert_eq!(prepared.git_status.total, 0);
        assert!(prepared.git_status.rows.is_empty());
    }
    assert_eq!(app.git_status.state(), GitStatusTaskState::Stale);
    app.dispatch(CommandId::GitStatusRefresh);
    assert_eq!(wait_status(&mut app, &wake), GitStatusTaskState::Ready);
    let refreshed = rows(&mut app);
    assert_eq!(refreshed.len(), 3);
    assert_eq!(
        refreshed.get("second-only.txt").map(String::as_str),
        Some("??")
    );
    app.project_root = None;
    {
        let prepared = app.prepare_frame(140, 40);
        assert!(prepared.git_status.rows.is_empty());
    }
    assert_eq!(app.git_status.state(), GitStatusTaskState::Stale);
    app.project_root = Some(second.root.clone());
    app.dispatch(CommandId::GitStatusRefresh);
    app.dispatch(CommandId::GitStatusCancel);
    assert_eq!(app.git_status.state(), GitStatusTaskState::Cancelled);
    assert!(app.git_status.prepare(40, 10).rows.is_empty());
    app.dispatch(CommandId::GitStatusRefresh);
    assert_eq!(wait_status(&mut app, &wake), GitStatusTaskState::Ready);
    app.project_root = None;
    app.prepare_frame(140, 40);
    assert_eq!(app.git_status.state(), GitStatusTaskState::Stale);
    assert_eq!(fixture.disk(), disk);
    assert_eq!(second.disk(), other_disk);
    assert_eq!(
        fs::read(second.path("second-only.txt")).unwrap(),
        b"another \xff\r\n"
    );
}

#[test]
fn configured_filter_is_a_static_ui_failure_and_never_a_partial_ready_list() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    // Fixture-local unsupported driver definition; production must reject it
    // before status, not invoke it or silently report a different normalization.
    fixture.git(&[
        "config",
        "filter.mustnotrun.clean",
        "jc-fixture-driver-must-not-run",
    ]);
    let before = fixture.disk();
    let (mut app, wake) = fixture.app(false);
    app.dispatch(CommandId::GitRepositoryStatus);
    assert_eq!(wait_status(&mut app, &wake), GitStatusTaskState::Failed);
    assert_eq!(app.git_status.message(), "Filtros no admitidos");
    let prepared = app.prepare_frame(140, 40);
    assert_eq!(prepared.git_status.total, 0);
    assert!(prepared.git_status.rows.is_empty());
    assert!(prepared
        .git_status
        .message
        .starts_with("Filtros no admitidos"));
    assert!(!prepared.git_status.message.contains("jc-fixture-driver"));
    assert_eq!(fixture.disk(), before);
}

#[test]
fn embedded_git_status_and_diff_work_in_child_with_no_git_on_path() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let before = fixture.disk();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "app::git_status_integration_tests::embedded_git_child_without_git_on_path",
            "--ignored",
        ])
        .env("PATH", "")
        .env_remove("JC_GIT_TEST_EXE")
        .env("CIDE_EMBEDDED_TUI_GIT_FIXTURE", &fixture.root)
        .env("CIDE_NO_SESSION", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("Owned embedded-Git TUI child exceeded its deadline");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(status.success(), "Embedded-Git child failed: {status}");
    assert_eq!(fixture.disk(), before);
}

#[test]
#[ignore = "invoked by embedded_git_status_and_diff_work_in_child_with_no_git_on_path"]
fn embedded_git_child_without_git_on_path() {
    assert_eq!(std::env::var_os("PATH"), Some("".into()));
    let root = PathBuf::from(std::env::var_os("CIDE_EMBEDDED_TUI_GIT_FIXTURE").unwrap());
    let before = fs::read(root.join(TRACKED)).unwrap();
    let wake = Wakeup::default();
    let mut app = TuiApp::with_wakeup(
        LaunchOptions {
            files: vec![root.join(TRACKED)],
            encoding: Some(TextEncoding::Windows1252),
        },
        wake.clone(),
    );
    app.project_root = Some(root);
    request_ready(&mut app, &wake);
    assert_status_rows(&mut app);
    assert!(matches!(app.overlay, UiOverlay::None));
    app.dispatch(CommandId::GitDiffHead);
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let ticket = wake.ticket();
        app.poll_tasks();
        if !matches!(
            app.git_diff.state(),
            GitDiffState::Queued | GitDiffState::Running
        ) {
            break;
        }
        assert!(Instant::now() < deadline, "Embedded diff did not complete");
        wake.wait(ticket, Some(Duration::from_millis(20)));
    }
    assert_eq!(
        app.git_diff.state(),
        GitDiffState::Ready,
        "{}",
        app.git_diff.message()
    );
    assert_eq!(app.workbench.active_bytes().unwrap(), before);
    assert!(matches!(app.overlay, UiOverlay::None));
}

#[test]
fn local_pair_keys_keep_head_index_disk_and_dirty_buffer_separate() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let disk = fixture.disk();
    let (mut app, wake) = fixture.app(true);
    app.workbench.insert_text("sin guardar ").unwrap();
    let original = app.workbench.active_bytes().unwrap();
    let revision = app
        .workbench
        .active_document()
        .unwrap()
        .session
        .document()
        .revision();
    request_ready(&mut app, &wake);
    let selected = app
        .git_status
        .prepare(256, 256)
        .rows
        .iter()
        .find(|r| r.path == TRACKED)
        .unwrap()
        .ordinal;
    app.git_status.first();
    app.git_status.move_selection(selected as isize);
    for (keycode, left, right) in [
        (
            'p',
            b"head \xff\r\nend".as_slice(),
            b"index content \x81\r\n".as_slice(),
        ),
        (
            't',
            b"index content \x81\r\n".as_slice(),
            b"work \xfe\r\nlast\n".as_slice(),
        ),
    ] {
        key(&mut app, KeyCode::Char(keycode));
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let ticket = wake.ticket();
            app.poll_tasks();
            match app.git_revision.state() {
                jc_app::git_revision::GitRevisionState::Ready => break,
                jc_app::git_revision::GitRevisionState::Queued
                | jc_app::git_revision::GitRevisionState::Running => {}
                state => panic!("local pair failed: {state:?}"),
            }
            assert!(Instant::now() < deadline);
            wake.wait(ticket, Some(Duration::from_millis(20)));
        }
        let c = app.git_revision.comparison().unwrap();
        assert_eq!(
            c.model.source_bytes(jc_app::diff_model::DiffSide::Left),
            left
        );
        assert_eq!(
            c.model.source_bytes(jc_app::diff_model::DiffSide::Right),
            right
        );
        assert_eq!(app.focus.primary(), PrimaryFocus::BottomPanel);
        let frame = app.prepare_frame(120, 35);
        assert!(!frame.git.details.is_empty());
        drop(frame);
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.workbench.active_bytes().unwrap(), original);
        key(&mut app, KeyCode::Esc);
        assert_eq!(app.focus.primary(), PrimaryFocus::Sidebar);
    }
    assert_eq!(
        app.workbench
            .active_document()
            .unwrap()
            .session
            .document()
            .revision(),
        revision
    );
    assert_eq!(fixture.disk(), disk);
}
