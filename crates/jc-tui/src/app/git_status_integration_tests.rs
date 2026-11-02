//! Windows-only real Git fixtures. Preparation writes only their owned repository.
#![cfg(windows)]
use super::{git_repository::GitConfigurationTarget, TuiApp};
use crate::state::{LaunchOptions, PromptKind, UiOverlay};
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
        app.git_executable = Some(self.git.clone());
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
fn submit_git(app: &mut TuiApp, git: &Path) {
    app.submit_prompt(PromptKind::GitExecutable, git.to_str().unwrap().to_owned());
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
fn executable_prompt_resumes_only_its_status_or_diff_intent_and_escape_removes_it() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let before = fixture.disk();
    let (mut status, wake) = fixture.app(false);
    status.git_executable = None;
    status.dispatch(CommandId::GitRepositoryStatus);
    assert!(matches!(
        status.git_configuration_target,
        Some(GitConfigurationTarget::Status)
    ));
    assert!(matches!(
        status.overlay,
        UiOverlay::Prompt {
            kind: PromptKind::GitExecutable,
            ..
        }
    ));
    submit_git(&mut status, &fixture.git);
    assert_eq!(wait_status(&mut status, &wake), GitStatusTaskState::Ready);
    assert_status_rows(&mut status);
    assert_eq!(status.tasks.worker_count(), 0);
    assert_eq!(status.git_diff.state(), GitDiffState::Idle);

    let (mut manual, _) = fixture.app(true);
    manual.git_executable = None;
    manual.dispatch(CommandId::GitRepositoryStatus);
    key(&mut manual, KeyCode::Esc);
    assert!(manual.git_configuration_target.is_none());
    assert!(matches!(manual.overlay, UiOverlay::None));
    manual.dispatch(CommandId::ViewExplorer);
    manual.dispatch(CommandId::GitConfigureExecutable);
    assert!(manual.git_configuration_target.is_none());
    submit_git(&mut manual, &fixture.git);
    assert_eq!(manual.git_diff.state(), GitDiffState::Idle);
    assert_eq!(manual.tasks.worker_count(), 0);
    assert!(!matches!(
        manual.git_status.state(),
        GitStatusTaskState::Queued | GitStatusTaskState::Running
    ));

    let (mut diff, wake) = fixture.app(true);
    diff.git_executable = None;
    diff.dispatch(CommandId::GitDiffHead);
    assert!(matches!(
        diff.git_configuration_target,
        Some(GitConfigurationTarget::Diff)
    ));
    submit_git(&mut diff, &fixture.git);
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
    app.git_executable = Some(second.root.join("not-a-git-executable.exe"));
    {
        let prepared = app.prepare_frame(140, 40);
        assert!(prepared.git_status.rows.is_empty());
    }
    assert_eq!(app.git_status.state(), GitStatusTaskState::Stale);
    app.git_executable = Some(second.git.clone());
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
    assert_eq!(app.git_status.message(), "Filters unsupported");
    let prepared = app.prepare_frame(140, 40);
    assert_eq!(prepared.git_status.total, 0);
    assert!(prepared.git_status.rows.is_empty());
    assert!(prepared
        .git_status
        .message
        .starts_with("Filters unsupported"));
    assert!(!prepared.git_status.message.contains("jc-fixture-driver"));
    assert_eq!(fixture.disk(), before);
}
