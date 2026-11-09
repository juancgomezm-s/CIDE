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

#[test]
fn tui_local_merge_lists_conflicts_and_only_applies_a_confirmed_preview() {
    let _serial = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new();
    let base = fixture.oid(&["rev-parse", "HEAD"]);
    fixture.git(&["config", "--local", "user.name", "CIDE Fixture"]);
    fixture.git(&["config", "--local", "user.email", "cide@example.test"]);
    let next = |text: &[u8], parent: &str| {
        fixture.rewrite(TRACKED, text);
        let blob = fixture.oid(&["hash-object", "-w", "--no-filters", "--", TRACKED]);
        fixture.cache(&blob);
        let tree = fixture.oid(&["write-tree"]);
        fixture.oid(&["commit-tree", &tree, "-p", parent, "-m", "Cambio de prueba"])
    };
    let active = next(b"activa\r\nend", &base);
    let conflicting = next(b"conflicto\r\nend", &base);
    let forward = next(b"avance\r\nend", &active);
    fixture.git(&["update-ref", "HEAD", &active]);
    fixture.git(&["update-ref", "refs/heads/conflicto", &conflicting]);
    fixture.git(&["update-ref", "refs/heads/avance", &forward]);
    fixture.git(&["read-tree", &active]);
    fixture.rewrite(TRACKED, b"activa\r\nend");
    let before = fixture.disk();
    let (mut app, wake) = fixture.app(true);
    request_ready(&mut app, &wake);
    let wait_for = |app: &mut TuiApp, ready: fn(&TuiApp) -> bool| {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !ready(app) {
            let ticket = wake.ticket();
            app.poll_tasks();
            assert!(Instant::now() < deadline, "{}", app.status);
            wake.wait(ticket, Some(Duration::from_millis(10)));
        }
    };
    key(&mut app, KeyCode::Char('g'));
    wait_for(&mut app, |app| app.git_revision.graph().is_some());
    let select = |app: &mut TuiApp, label: &str| {
        key(app, KeyCode::Char('r'));
        let row = app
            .prepare_frame(100, 24)
            .graph
            .unwrap()
            .rows
            .into_iter()
            .find(|row| row.path == label)
            .unwrap()
            .ordinal;
        key(app, KeyCode::Home);
        for _ in 0..row {
            key(app, KeyCode::Down);
        }
        key(app, KeyCode::Char('m'));
        key(app, KeyCode::Enter);
        wait_for(app, |app| !app.git_commit.busy());
    };
    select(&mut app, "Rama conflicto");
    let preview = app.prepare_frame(100, 24).commit.unwrap();
    assert!(preview
        .rows
        .iter()
        .any(|row| row.contains("Conflicto") && row.contains(TRACKED)));
    assert!(!preview.hint.contains("F2"));
    key(&mut app, KeyCode::F(2));
    assert!(!app.git_commit.busy());
    assert_eq!(fixture.disk(), before);
    key(&mut app, KeyCode::Esc);
    key(&mut app, KeyCode::Esc);
    select(&mut app, "Rama avance");
    let view = app.git_commit.view().unwrap();
    let review = view
        .merge_review()
        .unwrap_or_else(|| panic!("{:?}", view.error));
    assert_eq!(review.result().unwrap().to_string(), forward);
    assert_eq!(fixture.disk(), before);
    app.workbench.insert_text("edición sin guardar").unwrap();
    key(&mut app, KeyCode::F(2));
    assert_eq!(fixture.disk(), before);
    assert!(app
        .git_commit
        .view()
        .unwrap()
        .error
        .as_ref()
        .unwrap()
        .contains("Guarda"));
    app.workbench.undo().unwrap();
    key(&mut app, KeyCode::F(2));
    wait_for(&mut app, |app| !app.git_commit.busy());
    assert!(app.git_commit.view().is_none(), "{}", app.status);
    assert_eq!(fixture.oid(&["rev-parse", "HEAD"]), forward);
    let after = fixture.disk();
    assert_eq!(after.head, before.head);
    assert_eq!(after.tracked, b"avance\r\nend");
    assert_eq!(after.untracked, before.untracked);
}

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

#[test]
fn tui_graph_pages_to_merge_and_returns_from_commit_detail() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let initial = fixture.oid(&["rev-parse", "HEAD"]);
    let tree = fixture.oid(&["rev-parse", "HEAD^{tree}"]);
    let left = fixture.oid(&["commit-tree", &tree, "-p", &initial, "-m", "Rama uno"]);
    let original = fixture.disk();
    let alternate = b"texto del segundo padre\r\n";
    fixture.rewrite(TRACKED, alternate);
    fixture.cache(&fixture.oid(&["hash-object", "-w", "--no-filters", "--", TRACKED]));
    let other_tree = fixture.oid(&["write-tree"]);
    fixture.rewrite(TRACKED, &original.tracked);
    fixture.rewrite(".git/index", &original.index);
    let right = fixture.oid(&["commit-tree", &other_tree, "-p", &initial, "-m", "Rama dos"]);
    fixture.git(&["update-ref", "refs/heads/lateral", &right]);
    fixture.git(&["tag", "-a", "prueba", &left, "-m", "Etiqueta anotada"]);
    let blob = fixture.oid(&["rev-parse", "HEAD:tracked.txt"]);
    fixture.git(&["update-ref", "refs/tags/z-sin-commit", &blob]);
    let merge = fixture.oid(&[
        "commit-tree",
        &tree,
        "-p",
        &left,
        "-p",
        &right,
        "-m",
        "Integración",
    ]);
    let mut tip = merge.clone();
    for i in 0..24 {
        tip = fixture.oid(&[
            "commit-tree",
            &tree,
            "-p",
            &tip,
            "-m",
            &format!("Después {i}"),
        ]);
    }
    fixture.git(&["update-ref", "HEAD", &tip, &initial]);
    fixture.git(&["update-ref", "refs/heads/segura", &merge]);
    fixture.git(&["update-ref", "refs/heads/distinta", &right]);
    fixture.git(&["config", "--local", "user.name", "CIDE Fixture"]);
    fixture.git(&["config", "--local", "user.email", "cide@example.test"]);
    let before = fixture.disk();
    let (mut app, wake) = fixture.app(true);
    request_ready(&mut app, &wake);
    let settle = |app: &mut TuiApp| {
        let deadline = Instant::now() + Duration::from_secs(10);
        while matches!(
            app.git_revision.state(),
            jc_app::git_revision::GitRevisionState::Queued
                | jc_app::git_revision::GitRevisionState::Running
        ) {
            let ticket = wake.ticket();
            app.poll_tasks();
            assert!(Instant::now() < deadline, "{}", app.status);
            wake.wait(ticket, Some(Duration::from_millis(10)));
        }
    };
    key(&mut app, KeyCode::Char('g'));
    settle(&mut app);
    let graph = app.git_revision.graph().unwrap();
    assert!(graph.has_next());
    assert_eq!(graph.entries.len(), 24);
    assert_eq!(graph.entries[0].commit.id, tip);
    key(&mut app, KeyCode::Char('r'));
    let refs = app.prepare_frame(100, 24).graph.unwrap();
    let branch = refs
        .rows
        .iter()
        .find(|row| row.path == "Rama lateral")
        .unwrap()
        .ordinal;
    assert!(refs.rows.iter().any(|row| row.path == "Etiqueta prueba"));
    for _ in 0..branch {
        key(&mut app, KeyCode::Down);
    }
    fixture.git(&["update-ref", "refs/heads/lateral", &left, &right]);
    key(&mut app, KeyCode::Enter);
    settle(&mut app);
    assert_eq!(
        app.git_revision.graph().unwrap().entries[0].commit.id,
        right
    );
    key(&mut app, KeyCode::Char('r'));
    key(&mut app, KeyCode::End);
    key(&mut app, KeyCode::Enter);
    settle(&mut app);
    assert_eq!(
        app.git_revision.state(),
        jc_app::git_revision::GitRevisionState::Failed
    );
    assert_eq!(
        app.git_revision.graph().unwrap().entries[0].commit.id,
        right
    );
    key(&mut app, KeyCode::Char('r'));
    key(&mut app, KeyCode::End);
    key(&mut app, KeyCode::Up);
    key(&mut app, KeyCode::Enter);
    settle(&mut app);
    assert_eq!(app.git_revision.graph().unwrap().entries[0].commit.id, left);
    key(&mut app, KeyCode::Char('r'));
    key(&mut app, KeyCode::Enter);
    settle(&mut app);
    key(&mut app, KeyCode::Char('n'));
    settle(&mut app);
    let presented = app.prepare_frame(100, 24).graph.unwrap();
    assert!(presented.rows[0].path.contains('◆'));
    assert!(presented.rows[0].path.contains(&merge[..8]));
    key(&mut app, KeyCode::Enter);
    settle(&mut app);
    assert_eq!(app.git_revision.revision().unwrap().commit, merge);
    assert!(app.git_revision.revision().unwrap().files.is_empty());
    key(&mut app, KeyCode::Char('p'));
    settle(&mut app);
    assert_eq!(
        app.git_revision.revision().unwrap().parent.as_deref(),
        Some(right.as_str())
    );
    assert_eq!(
        app.git_revision.revision().unwrap().files[0].path,
        TRACKED.as_bytes()
    );
    key(&mut app, KeyCode::Enter);
    settle(&mut app);
    let pair = app.git_revision.comparison().unwrap();
    assert_eq!(
        pair.model.source_bytes(jc_app::diff_model::DiffSide::Left),
        alternate
    );
    assert!(
        matches!(&pair.origin, jc_app::git_revision::ComparisonOrigin::Historical { parent: Some(parent), .. } if *parent == right)
    );
    key(&mut app, KeyCode::Esc);
    key(&mut app, KeyCode::Esc);
    assert!(app.graph_showing());
    assert_eq!(
        app.git_revision.graph().unwrap().entries[0].commit.id,
        merge
    );
    key(&mut app, KeyCode::Char('t'));
    app.handle_event(Event::Paste("integracion-verificada".into()));
    key(&mut app, KeyCode::Enter);
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.git_commit.busy() {
        let ticket = wake.ticket();
        app.poll_tasks();
        assert!(Instant::now() < deadline, "{}", app.status);
        wake.wait(ticket, Some(Duration::from_millis(10)));
    }
    let view = app.git_commit.view().unwrap();
    assert!(view.reference_review().is_some(), "{:?}", view.error);
    let tag = fixture.path(".git/refs/tags/integracion-verificada");
    assert!(!tag.exists());
    key(&mut app, KeyCode::F(2));
    while app.git_commit.busy() {
        let ticket = wake.ticket();
        app.poll_tasks();
        assert!(Instant::now() < deadline, "{}", app.status);
        wake.wait(ticket, Some(Duration::from_millis(10)));
    }
    assert_eq!(fs::read_to_string(tag).unwrap().trim(), merge);
    assert_eq!(fixture.disk(), before);

    settle(&mut app);
    key(&mut app, KeyCode::Char('r'));
    let row = app
        .prepare_frame(100, 24)
        .graph
        .unwrap()
        .rows
        .into_iter()
        .find(|row| row.path == "Etiqueta integracion-verificada")
        .unwrap()
        .ordinal;
    for _ in 0..row {
        key(&mut app, KeyCode::Down);
    }
    key(&mut app, KeyCode::Delete);
    key(&mut app, KeyCode::Enter);
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.git_commit.busy() {
        let ticket = wake.ticket();
        app.poll_tasks();
        assert!(Instant::now() < deadline, "{}", app.status);
        wake.wait(ticket, Some(Duration::from_millis(10)));
    }
    let view = app.git_commit.view().unwrap();
    assert!(view.reference_change_review().is_some(), "{:?}", view.error);
    assert!(fixture
        .path(".git/refs/tags/integracion-verificada")
        .is_file());
    key(&mut app, KeyCode::F(2));
    while app.git_commit.busy() {
        let ticket = wake.ticket();
        app.poll_tasks();
        assert!(Instant::now() < deadline, "{}", app.status);
        wake.wait(ticket, Some(Duration::from_millis(10)));
    }
    assert!(
        !fixture
            .path(".git/refs/tags/integracion-verificada")
            .exists(),
        "{}",
        app.status
    );
    assert_eq!(fixture.disk(), before);

    settle(&mut app);
    key(&mut app, KeyCode::Char('r'));
    let row = app
        .prepare_frame(100, 24)
        .graph
        .unwrap()
        .rows
        .into_iter()
        .find(|row| row.path == "Rama segura")
        .unwrap()
        .ordinal;
    for _ in 0..row {
        key(&mut app, KeyCode::Down);
    }
    key(&mut app, KeyCode::Char('c'));
    key(&mut app, KeyCode::Enter);
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.git_commit.busy() {
        let ticket = wake.ticket();
        app.poll_tasks();
        assert!(Instant::now() < deadline, "{}", app.status);
        wake.wait(ticket, Some(Duration::from_millis(10)));
    }
    let view = app.git_commit.view().unwrap();
    let review = view
        .branch_switch_review()
        .unwrap_or_else(|| panic!("{:?}", view.error));
    assert_eq!(review.target().to_string(), merge);
    assert_eq!(fixture.disk(), before);
    app.workbench.insert_text("sin guardar").unwrap();
    key(&mut app, KeyCode::F(2));
    assert!(app
        .git_commit
        .view()
        .unwrap()
        .error
        .as_ref()
        .unwrap()
        .contains("Guarda"));
    assert_eq!(fixture.disk(), before);
    app.workbench.undo().unwrap();
    key(&mut app, KeyCode::F(2));
    while app.git_commit.busy() {
        let ticket = wake.ticket();
        app.poll_tasks();
        assert!(Instant::now() < deadline, "{}", app.status);
        wake.wait(ticket, Some(Duration::from_millis(10)));
    }
    let after = fixture.disk();
    assert_eq!(after.head, b"ref: refs/heads/segura\n", "{}", app.status);
    assert_eq!(after.index, before.index);
    assert_eq!(after.branch, before.branch);
    assert_eq!(after.tracked, before.tracked);
    assert_eq!(after.untracked, before.untracked);

    fixture.git(&["read-tree", &merge]);
    fixture.rewrite(TRACKED, b"head \xff\r\nend");
    settle(&mut app);
    key(&mut app, KeyCode::Char('r'));
    let row = app
        .prepare_frame(100, 24)
        .graph
        .unwrap()
        .rows
        .into_iter()
        .find(|row| row.path == "Rama distinta")
        .unwrap()
        .ordinal;
    for _ in 0..row {
        key(&mut app, KeyCode::Down);
    }
    key(&mut app, KeyCode::Char('c'));
    key(&mut app, KeyCode::Enter);
    let settle_write = |app: &mut TuiApp| {
        let deadline = Instant::now() + Duration::from_secs(10);
        while app.git_commit.busy() {
            let ticket = wake.ticket();
            app.poll_tasks();
            assert!(Instant::now() < deadline, "{}", app.status);
            wake.wait(ticket, Some(Duration::from_millis(10)));
        }
    };
    settle_write(&mut app);
    let view = app.git_commit.view().unwrap();
    assert!(view.branch_switch_review().is_some(), "{:?}", view.error);
    let preview = app.prepare_frame(100, 24).commit.unwrap();
    assert!(preview.rows.iter().any(|row| row.contains(TRACKED)));
    assert_eq!(
        fs::read(fixture.path(TRACKED)).unwrap(),
        b"head \xff\r\nend"
    );
    key(&mut app, KeyCode::F(2));
    settle_write(&mut app);
    let after = fixture.disk();
    assert_eq!(after.head, b"ref: refs/heads/distinta\n", "{}", app.status);
    assert_eq!(after.tracked, alternate);
    assert_eq!(after.untracked, before.untracked);
}

#[test]
fn tui_recovers_a_deleted_file_only_after_review() {
    tui_restore_case(false);
}

#[test]
fn tui_restores_an_existing_file_and_preserves_its_local_bytes() {
    tui_restore_case(true);
}

fn tui_restore_case(existing: bool) {
    use jc_app::git_revision::GitRevisionState;
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let before = fixture.disk();
    if !existing {
        fs::remove_file(fixture.path(TRACKED)).unwrap();
    }
    let (mut app, wake) = fixture.app(false);
    request_ready(&mut app, &wake);
    key(&mut app, KeyCode::End);
    assert_eq!(app.git_status.selected_path_ref(), Some(TRACKED.as_bytes()));
    key(&mut app, KeyCode::Char('t'));
    let wait = |app: &mut TuiApp, pending: fn(&TuiApp) -> bool| {
        let deadline = Instant::now() + Duration::from_secs(10);
        while pending(app) {
            let ticket = wake.ticket();
            app.poll_tasks();
            assert!(Instant::now() < deadline, "{}", app.status);
            wake.wait(ticket, Some(Duration::from_millis(10)));
        }
    };
    wait(&mut app, |app| {
        matches!(
            app.git_revision.state(),
            GitRevisionState::Queued | GitRevisionState::Running
        )
    });
    let expected_label = if existing {
        "Restaurar archivo"
    } else {
        "Recuperar archivo"
    };
    assert_eq!(
        app.git_restore_label(),
        Some(expected_label),
        "{}",
        app.status
    );
    assert!(app
        .prepare_historical_diff(45, 8, 8)
        .status
        .starts_with(&format!("U {expected_label}")));
    key(&mut app, KeyCode::Char('u'));
    key(&mut app, KeyCode::Enter);
    wait(&mut app, |app| app.git_commit.busy());
    let view = app.git_commit.view().unwrap();
    assert!(view.restore_review().is_some(), "{:?}", view.error);
    assert_eq!(
        fs::read(fixture.path(TRACKED)).ok().as_deref(),
        existing.then_some(before.tracked.as_slice())
    );
    key(&mut app, KeyCode::F(2));
    wait(&mut app, |app| app.git_commit.busy());
    assert_eq!(
        fs::read(fixture.path(TRACKED)).unwrap(),
        b"index content \x81\r\n"
    );
    let after = fixture.disk();
    assert_eq!(after.index, before.index);
    assert_eq!(after.head, before.head);
    assert_eq!(after.untracked, before.untracked);
    if existing {
        let backup = fs::read_dir(fixture.path(".git"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| path.extension().is_some_and(|ext| ext == "bak"))
            .expect("backed up local bytes");
        assert_eq!(fs::read(backup).unwrap(), before.tracked);
        assert!(app.status.contains("Respaldo en .git/"), "{}", app.status);
    }
}

#[test]
fn tui_create_commit_reviews_only_the_index_and_requires_f2_confirmation() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    fixture.git(&["config", "--local", "user.name", "CIDE Fixture"]);
    fixture.git(&["config", "--local", "user.email", "cide@example.test"]);
    let before = fixture.disk();
    let (mut app, wake) = fixture.app(true);
    request_ready(&mut app, &wake);
    key(&mut app, KeyCode::Char('c'));
    app.handle_event(Event::Paste("Commit desde TUI".into()));
    let settle = |app: &mut TuiApp| {
        let deadline = Instant::now() + Duration::from_secs(10);
        while app.git_commit.busy() {
            let ticket = wake.ticket();
            app.poll_tasks();
            assert!(Instant::now() < deadline, "{}", app.status);
            wake.wait(ticket, Some(Duration::from_millis(10)));
        }
    };
    key(&mut app, KeyCode::Enter);
    settle(&mut app);
    let presentation = app.prepare_frame(96, 24);
    let commit = presentation.commit.unwrap();
    assert!(commit.rows.iter().any(|row| row.contains(TRACKED)));
    assert!(!commit.rows.iter().any(|row| row.contains(UNTRACKED)));
    assert!(commit.hint.contains("F2 confirma"), "{}", commit.hint);
    let id = app.git_commit.view().unwrap().review().unwrap().id();
    key(&mut app, KeyCode::Enter);
    assert!(!app.git_commit.busy());
    assert_eq!(fixture.disk(), before);
    key(&mut app, KeyCode::F(2));
    settle(&mut app);
    assert!(app.git_commit.view().is_none(), "{}", app.status);
    let after = fixture.disk();
    assert_eq!(after.branch, format!("{id}\n").as_bytes());
    assert_eq!(after.index, before.index);
    assert_eq!(after.tracked, before.tracked);
    assert_eq!(after.untracked, before.untracked);
    request_ready(&mut app, &wake);
    key(&mut app, KeyCode::Char('m'));
    app.handle_event(Event::Paste("Mensaje corregido".into()));
    key(&mut app, KeyCode::Enter);
    settle(&mut app);
    let commit = app.prepare_frame(96, 24).commit.unwrap();
    assert_eq!(commit.title, "Modificar último commit");
    assert!(commit
        .rows
        .iter()
        .any(|row| row == &format!("Reemplaza {id}")));
    let replacement = app.git_commit.view().unwrap().review().unwrap().id();
    key(&mut app, KeyCode::Enter);
    assert_eq!(fixture.disk(), after);
    key(&mut app, KeyCode::F(2));
    settle(&mut app);
    let amended = fixture.disk();
    assert_eq!(amended.branch, format!("{replacement}\n").as_bytes());
    assert_eq!(amended.index, before.index);
}

#[test]
fn tui_index_reviews_then_stages_and_unstages_the_original_file() {
    use jc_app::{
        git_index::{IndexAction, IndexState},
        git_revision::GitRevisionState,
    };

    fn wait(app: &mut TuiApp, wake: &Wakeup, pending: impl Fn(&TuiApp) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while pending(app) {
            let ticket = wake.ticket();
            app.poll_tasks();
            assert!(Instant::now() < deadline, "{}", app.status);
            if pending(app) {
                wake.wait(ticket, Some(Duration::from_millis(20)));
            }
        }
    }

    fn compare(app: &mut TuiApp, wake: &Wakeup, key_code: char) {
        key(app, KeyCode::Char(key_code));
        wait(app, wake, |a| {
            matches!(
                a.git_revision.state(),
                GitRevisionState::Queued | GitRevisionState::Running
            )
        });
        assert_eq!(app.git_revision.state(), GitRevisionState::Ready);
        assert_eq!(app.focus.primary(), PrimaryFocus::BottomPanel);
    }

    fn review(app: &mut TuiApp, wake: &Wakeup, action: IndexAction) {
        key(app, KeyCode::Char('i'));
        wait(app, wake, |a| a.git_index.state() == IndexState::Preparing);
        assert_eq!(
            app.git_index.state(),
            IndexState::Reviewing,
            "{}",
            app.status
        );
        assert_eq!(app.git_index.review().unwrap().target.action, action);
        assert!(app
            .prepare_historical_diff(80, 8, 8)
            .status
            .starts_with("Intro confirma"));
    }

    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let before = fixture.disk();
    let (mut app, wake) = fixture.app(true);
    request_ready(&mut app, &wake);
    key(&mut app, KeyCode::End);
    assert_eq!(app.git_status.selected_path_ref(), Some(TRACKED.as_bytes()));
    compare(&mut app, &wake, 't');
    assert!(app
        .prepare_historical_diff(80, 8, 8)
        .status
        .contains("I preparar"));

    app.workbench.insert_text("sin guardar").unwrap();
    key(&mut app, KeyCode::Char('i'));
    assert_eq!(app.git_index.state(), IndexState::Idle);
    assert!(app.status.contains("Guarda los documentos"));
    app.workbench.undo().unwrap();

    review(&mut app, &wake, IndexAction::Stage);
    key(&mut app, KeyCode::Char('i'));
    assert_eq!(app.git_index.state(), IndexState::Reviewing);
    key(&mut app, KeyCode::Esc);
    assert!(app.git_index.review().is_none());
    assert_eq!(fixture.disk(), before);

    review(&mut app, &wake, IndexAction::Stage);
    let _ = app.focus.focus(PrimaryFocus::Sidebar, &app.panels);
    key(&mut app, KeyCode::Home);
    let _ = app.focus.focus(PrimaryFocus::BottomPanel, &app.panels);
    key(&mut app, KeyCode::Enter);
    assert!(app.git_index.review().is_none());
    assert_eq!(fixture.disk(), before);

    let _ = app.focus.focus(PrimaryFocus::Sidebar, &app.panels);
    key(&mut app, KeyCode::End);
    let _ = app.focus.focus(PrimaryFocus::BottomPanel, &app.panels);
    review(&mut app, &wake, IndexAction::Stage);
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.git_index.state(), IndexState::Applying);
    let _ = app.focus.focus(PrimaryFocus::Sidebar, &app.panels);
    key(&mut app, KeyCode::Home);
    wait(&mut app, &wake, |a| {
        a.git_index.state() == IndexState::Applying
    });
    assert_eq!(
        app.git_index.state(),
        IndexState::Published,
        "{}",
        app.status
    );
    assert_eq!(wait_status(&mut app, &wake), GitStatusTaskState::Ready);
    assert_eq!(
        app.git_status.selected_path_ref(),
        Some(UNTRACKED.as_bytes())
    );
    assert!(app
        .prepare_local_git_status(80, 8)
        .message
        .starts_with("Preparado: tracked.txt"));
    assert_eq!(fixture.git(&["show", ":tracked.txt"]), before.tracked);

    key(&mut app, KeyCode::End);
    compare(&mut app, &wake, 'p');
    assert!(app
        .prepare_historical_diff(80, 8, 8)
        .status
        .starts_with("I retirar"));
    review(&mut app, &wake, IndexAction::Unstage);
    key(&mut app, KeyCode::Enter);
    wait(&mut app, &wake, |a| {
        a.git_index.state() == IndexState::Applying
    });
    assert_eq!(
        app.git_index.state(),
        IndexState::Published,
        "{}",
        app.status
    );
    assert_eq!(fixture.git(&["show", ":tracked.txt"]), b"head \xff\r\nend");
    assert!(app
        .prepare_local_git_status(80, 8)
        .message
        .starts_with("Retirado: tracked.txt"));
    let after = fixture.disk();
    assert_eq!(after.tracked, before.tracked);
    assert_eq!(after.untracked, before.untracked);
    assert_eq!(after.config, before.config);
    assert_eq!(after.head, before.head);
    assert_eq!(after.branch, before.branch);
    assert_eq!(app.workbench.active_bytes().unwrap(), before.tracked);

    let baseline: String = (0..32).map(|i| format!("linea {i:02}\r\n")).collect();
    fixture.rewrite(TRACKED, baseline.as_bytes());
    fixture.cache(&fixture.oid(&["hash-object", "-w", "--no-filters", "--", TRACKED]));
    let tree = fixture.oid(&["write-tree"]);
    let previous = fixture.oid(&["rev-parse", "HEAD"]);
    let base_commit = fixture.oid(&[
        "commit-tree",
        &tree,
        "-p",
        &previous,
        "-m",
        "Base de bloques",
    ]);
    fixture.git(&["update-ref", "HEAD", &base_commit, &previous]);
    let second_only = baseline.replace("linea 27", "segundo cambio");
    let worktree = second_only.replace("linea 02", "primer cambio");
    fixture.rewrite(TRACKED, worktree.as_bytes());
    request_ready(&mut app, &wake);
    key(&mut app, KeyCode::End);
    compare(&mut app, &wake, 't');
    assert!(app
        .prepare_historical_diff(100, 8, 8)
        .status
        .contains("K preparar bloque"));
    key(&mut app, KeyCode::Char('k'));
    wait(&mut app, &wake, |a| {
        a.git_index.state() == IndexState::Preparing
    });
    assert_eq!(app.git_index.review().unwrap().target.hunk, Some(0));
    key(&mut app, KeyCode::Right);
    key(&mut app, KeyCode::Enter);
    assert!(app.git_index.review().is_none());
    assert_eq!(fixture.git(&["show", ":tracked.txt"]), baseline.as_bytes());
    key(&mut app, KeyCode::Char('k'));
    wait(&mut app, &wake, |a| {
        a.git_index.state() == IndexState::Preparing
    });
    assert_eq!(app.git_index.review().unwrap().target.hunk, Some(1));
    key(&mut app, KeyCode::Enter);
    wait(&mut app, &wake, |a| {
        a.git_index.state() == IndexState::Applying
    });
    assert_eq!(
        app.git_index.state(),
        IndexState::Published,
        "{}",
        app.status
    );
    assert_eq!(
        fixture.git(&["show", ":tracked.txt"]),
        second_only.as_bytes()
    );
    assert_eq!(
        fs::read(fixture.path(TRACKED)).unwrap(),
        worktree.as_bytes()
    );

    fixture.cache(&fixture.oid(&["hash-object", "-w", "--no-filters", "--", TRACKED]));
    request_ready(&mut app, &wake);
    key(&mut app, KeyCode::End);
    compare(&mut app, &wake, 'p');
    assert!(app
        .prepare_historical_diff(100, 8, 8)
        .status
        .contains("K retirar bloque"));
    key(&mut app, KeyCode::Char('k'));
    wait(&mut app, &wake, |a| {
        a.git_index.state() == IndexState::Preparing
    });
    let review = app.git_index.review().unwrap();
    assert_eq!(review.target.action, IndexAction::Unstage);
    assert_eq!(review.target.hunk, Some(0));
    key(&mut app, KeyCode::Enter);
    wait(&mut app, &wake, |a| {
        a.git_index.state() == IndexState::Applying
    });
    assert_eq!(
        app.git_index.state(),
        IndexState::Published,
        "{}",
        app.status
    );
    assert_eq!(
        fixture.git(&["show", ":tracked.txt"]),
        second_only.as_bytes()
    );
    assert_eq!(
        fs::read(fixture.path(TRACKED)).unwrap(),
        worktree.as_bytes()
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

fn wait_patch_comparison(app: &mut TuiApp, wake: &Wakeup) {
    use jc_app::git_revision::GitRevisionState;
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let ticket = wake.ticket();
        app.poll_tasks();
        match app.git_revision.state() {
            GitRevisionState::Ready => return,
            GitRevisionState::Queued | GitRevisionState::Running => {}
            state => panic!("Falló la comparación para el parche: {state:?}"),
        }
        assert!(Instant::now() < deadline);
        wake.wait(ticket, Some(Duration::from_millis(20)));
    }
}

fn patch_destination(app: &mut TuiApp, path: &Path) {
    use crate::state::PromptKind;
    assert!(matches!(
        app.overlay,
        UiOverlay::Prompt {
            kind: PromptKind::GitExportPatch,
            ..
        }
    ));
    app.handle_event(Event::Paste(path.to_str().unwrap().into()));
    key(app, KeyCode::Enter);
}

fn accept_patch_result(app: &mut TuiApp, wake: &Wakeup) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let ticket = wake.ticket();
        if app.git_patch.ready() {
            break;
        }
        assert!(Instant::now() < deadline, "{}", app.status);
        wake.wait(ticket, Some(Duration::from_millis(20)));
    }
    assert!(app
        .prepare_frame(100, 32)
        .git
        .status
        .contains("Exportando parche"));
    assert!(app.git_patch.ready(), "la pintura solo lee estado");
    assert!(app.poll_tasks().changed);
}

fn patch_artifact(fixture: &Fixture) -> PathBuf {
    fixture.base.join(format!(
        "cide-export-{}-{}.patch",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

#[test]
fn tui_patch_local_guards_the_prompt_identity_and_publishes_exact_bytes() {
    let _serial = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new();
    let (mut app, wake) = fixture.app(true);
    let disk = fixture.disk();
    app.workbench
        .insert_text("buffer independiente\r\n")
        .unwrap();
    let editor = app.workbench.active_bytes().unwrap();
    let destination = patch_artifact(&fixture);
    request_ready(&mut app, &wake);
    key(&mut app, KeyCode::End);
    key(&mut app, KeyCode::Char('t'));
    wait_patch_comparison(&mut app, &wake);
    assert!(app
        .prepare_frame(100, 32)
        .git
        .status
        .starts_with("X exportar parche"));

    key(&mut app, KeyCode::Char('x'));
    key(&mut app, KeyCode::Esc);
    assert!(matches!(app.overlay, UiOverlay::None));
    assert!(app.status.contains("cancelada"));
    key(&mut app, KeyCode::Char('x'));
    app.git_revision.cancel();
    patch_destination(&mut app, &destination);
    assert!(app.status.contains("comparación cambió"));
    assert!(!destination.exists());

    key(&mut app, KeyCode::F(5));
    wait_patch_comparison(&mut app, &wake);
    key(&mut app, KeyCode::Char('x'));
    patch_destination(&mut app, &destination);
    accept_patch_result(&mut app, &wake);
    assert!(app.status.starts_with("Parche exportado"));
    let expected = b"diff --git a/tracked.txt b/tracked.txt\n--- a/tracked.txt\n+++ b/tracked.txt\n@@ -1,1 +1,2 @@\n-index content \x81\r\n+work \xfe\r\n+last\n";
    assert_eq!(fs::read(&destination).unwrap(), expected);

    key(&mut app, KeyCode::Char('x'));
    patch_destination(&mut app, &destination);
    accept_patch_result(&mut app, &wake);
    assert!(app.status.contains("ya existe"));
    assert_eq!(fs::read(&destination).unwrap(), expected);

    let denied = fixture.path("dentro-del-repo.patch");
    key(&mut app, KeyCode::Char('x'));
    patch_destination(&mut app, &denied);
    accept_patch_result(&mut app, &wake);
    assert!(app.status.contains("fuera del repositorio"));
    assert!(!denied.exists());
    assert_eq!(app.workbench.active_bytes().unwrap(), editor);
    assert_eq!(fixture.disk(), disk);
    fs::remove_file(destination).unwrap();
}

#[test]
fn tui_patch_historical_root_uses_pinned_blob_and_keeps_missing_final_lf() {
    let _serial = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new();
    let (mut app, wake) = fixture.app(true);
    let disk = fixture.disk();
    let destination = patch_artifact(&fixture);
    request_ready(&mut app, &wake);
    key(&mut app, KeyCode::Char('h'));
    key(&mut app, KeyCode::Enter);
    wait_patch_comparison(&mut app, &wake);
    key(&mut app, KeyCode::Enter);
    wait_patch_comparison(&mut app, &wake);
    assert_eq!(app.focus.primary(), PrimaryFocus::BottomPanel);
    key(&mut app, KeyCode::Char('x'));
    patch_destination(&mut app, &destination);
    accept_patch_result(&mut app, &wake);
    assert!(app.status.starts_with("Parche exportado"));
    let expected = b"diff --git a/tracked.txt b/tracked.txt\nnew file mode 100644\n--- /dev/null\n+++ b/tracked.txt\n@@ -0,0 +1,2 @@\n+head \xff\r\n+end\n\\ No newline at end of file\n";
    assert_eq!(fs::read(&destination).unwrap(), expected);
    assert_eq!(app.workbench.active_bytes().unwrap(), disk.tracked);
    assert_eq!(fixture.disk(), disk);
    fs::remove_file(destination).unwrap();
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

#[test]
fn commit_search_keys_page_open_compare_back_and_edit_preserve_sources() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let mut parent = fixture.oid(&["rev-parse", "HEAD"]);
    for n in 0..27 {
        fixture.rewrite(TRACKED, format!("revisión {n}\r\n").as_bytes());
        let blob = fixture.oid(&["hash-object", "-w", "--no-filters", "--", TRACKED]);
        fixture.cache(&blob);
        let tree = fixture.oid(&["write-tree"]);
        parent = fixture.oid(&[
            "commit-tree",
            &tree,
            "-p",
            &parent,
            "-m",
            &format!("Resumen {n}"),
            "-m",
            "ENCONTRAR Ñ en el cuerpo",
        ]);
    }
    fixture.git(&["update-ref", "HEAD", &parent]);
    let disk = fixture.disk();
    let (mut app, wake) = fixture.app(true);
    app.workbench.insert_text("sin guardar ").unwrap();
    let source = app.workbench.active_bytes().unwrap();
    let panes = format!("{:?}", app.workbench.panes());
    request_ready(&mut app, &wake);
    let wait = |app: &mut TuiApp| {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let ticket = wake.ticket();
            app.poll_tasks();
            match app.git_revision.state() {
                jc_app::git_revision::GitRevisionState::Ready => break,
                jc_app::git_revision::GitRevisionState::Queued
                | jc_app::git_revision::GitRevisionState::Running => {}
                state => panic!(
                    "commit search failed: {state:?}: {}",
                    app.git_revision.message()
                ),
            }
            assert!(Instant::now() < deadline);
            wake.wait(ticket, Some(Duration::from_millis(20)));
        }
    };
    key(&mut app, KeyCode::Char('f'));
    app.handle_event(Event::Paste("encontrar ñ".into()));
    assert!(!matches!(
        app.git_revision.state(),
        jc_app::git_revision::GitRevisionState::Queued
            | jc_app::git_revision::GitRevisionState::Running
    ));
    key(&mut app, KeyCode::Enter);
    wait(&mut app);
    assert_eq!(app.git_revision.commit_search().unwrap().entries.len(), 24);
    assert!(app
        .prepare_git_history(120, 20)
        .title
        .contains("encontrar ñ"));
    key(&mut app, KeyCode::Char('n'));
    wait(&mut app);
    assert_eq!(app.git_revision.commit_search().unwrap().entries.len(), 3);
    let selected = app.git_revision.commit_search().unwrap().entries[0]
        .commit
        .clone();
    key(&mut app, KeyCode::Enter);
    wait(&mut app);
    assert!(app.git_history.files);
    assert_eq!(app.git_revision.revision().unwrap().commit, selected);
    key(&mut app, KeyCode::Enter);
    wait(&mut app);
    assert!(app.git_revision.comparison().is_some());
    assert_eq!(app.focus.primary(), PrimaryFocus::BottomPanel);
    key(&mut app, KeyCode::Esc);
    key(&mut app, KeyCode::Esc);
    assert!(!app.git_history.files);
    assert_eq!(app.prepare_git_history(120, 20).total, 3);
    key(&mut app, KeyCode::Tab);
    key(&mut app, KeyCode::Char('x'));
    assert_eq!(app.prepare_git_history(120, 20).total, 0);
    assert!(!matches!(
        app.git_revision.state(),
        jc_app::git_revision::GitRevisionState::Queued
            | jc_app::git_revision::GitRevisionState::Running
    ));
    key(&mut app, KeyCode::F(2)); // Todo -> Mensaje -> Autor.
    key(&mut app, KeyCode::F(2));
    app.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('u'),
        KeyModifiers::CONTROL,
    )));
    app.handle_event(Event::Paste("FIXTURE@EXAMPLE.INVALID".into()));
    key(&mut app, KeyCode::Enter);
    wait(&mut app);
    assert_eq!(
        app.git_revision.commit_search().unwrap().field,
        jc_app::git_revision::SearchField::Author
    );
    assert_eq!(app.git_revision.commit_search().unwrap().entries.len(), 24);
    key(&mut app, KeyCode::F(2));
    app.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('u'),
        KeyModifiers::CONTROL,
    )));
    app.handle_event(Event::Paste(parent[..12].into()));
    key(&mut app, KeyCode::Enter);
    wait(&mut app);
    let page = app.git_revision.commit_search().unwrap();
    assert_eq!(page.field, jc_app::git_revision::SearchField::Commit);
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].commit, parent);
    key(&mut app, KeyCode::Esc);
    assert!(app.git_history.search.is_none());
    assert_eq!(app.workbench.active_bytes().unwrap(), source);
    assert_eq!(format!("{:?}", app.workbench.panes()), panes);
    assert_eq!(fixture.disk(), disk);
}
