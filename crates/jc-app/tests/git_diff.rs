#![cfg(windows)]
use jc_app::{
    diff_model::DiffSide,
    git_diff::{GitDiffController, GitDiffOptions, GitDiffState, GIT_DIFF_JOB_KEY},
    git_read::GitBlobSource,
    Cancellation, JobKey, JobLimits, JobSystem, PaneId, TaskFailure, TaskOutput, TaskSnapshot,
    Wakeup, Workbench,
};
use jc_text::TextEncoding;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Mutex,
    },
    thread,
    time::{Duration, Instant},
};
static SERIAL: Mutex<()> = Mutex::new(());
static NEXT: AtomicU64 = AtomicU64::new(0);
static GATE: AtomicBool = AtomicBool::new(true);
fn installed_git() -> PathBuf {
    if let Some(path) = std::env::var_os("JC_GIT_TEST_EXE") {
        return PathBuf::from(path).canonicalize().unwrap();
    }
    std::env::split_paths(&std::env::var_os("PATH").expect("Installed Git required"))
        .map(|p| p.join("git.exe"))
        .find(|p| p.is_file())
        .expect("Installed Git required; tests never download it")
        .canonicalize()
        .unwrap()
}
struct Fixture {
    base: PathBuf,
    container: PathBuf,
    root: PathBuf,
    git: PathBuf,
    path: PathBuf,
    relative: &'static str,
}
impl Fixture {
    fn new(head: &[u8], index: &[u8]) -> Self {
        let project = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let base = project.join("work/test-temp");
        fs::create_dir_all(&base).unwrap();
        let base = base.canonicalize().unwrap();
        let container = base.join(format!(
            "jc git diff {}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&container).unwrap();
        let container = container.canonicalize().unwrap();
        let root = container.join("repo");
        fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let relative = "niño[1].p";
        let path = root.join(relative);
        let fixture = Self {
            base,
            container,
            root,
            git: installed_git(),
            path,
            relative,
        };
        fixture.git(&["init", "--quiet", "--template=", "--initial-branch=main"]);
        let oid = fixture.blob(head);
        fixture.cache(&oid);
        let tree = fixture.git(&["write-tree"]);
        let tree = std::str::from_utf8(&tree).unwrap().trim();
        let commit = fixture.git(&["commit-tree", tree, "-m", "synthetic diff fixture"]);
        let commit = std::str::from_utf8(&commit).unwrap().trim();
        fixture.git(&["update-ref", "HEAD", commit]);
        let oid = fixture.blob(index);
        fixture.cache(&oid);
        fs::write(&fixture.path, head).unwrap();
        fixture
    }
    // Intentional writes only in this fixture: synthetic objects/index/ref.
    // The production controller/provider never invokes these commands.
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
        assert!(
            output.status.success(),
            "Fixture command failed: {:?}",
            output.stderr
        );
        output.stdout
    }
    fn blob(&self, bytes: &[u8]) -> String {
        fs::write(self.root.join("fixture-object"), bytes).unwrap();
        String::from_utf8(self.git(&["hash-object", "-w", "--no-filters", "--", "fixture-object"]))
            .unwrap()
            .trim()
            .into()
    }
    fn cache(&self, oid: &str) {
        self.git(&[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("100644,{oid},{}", self.relative),
        ]);
    }
    fn app(&self) -> Workbench {
        let mut app = Workbench::default();
        app.open_file(&self.path, Some(TextEncoding::Utf8)).unwrap();
        app
    }
    fn options(&self, source: GitBlobSource) -> GitDiffOptions {
        GitDiffOptions {
            git_executable: self.git.clone(),
            root: self.root.clone(),
            cwd: self.container.clone(),
            source,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let target = self.container.canonicalize().unwrap();
        assert_eq!(target, self.container);
        assert_eq!(target.parent(), Some(self.base.as_path()));
        assert!(target
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("jc git diff "));
        fs::remove_dir_all(target).unwrap();
    }
}
fn jobs() -> JobSystem {
    JobSystem::new(
        JobLimits {
            workers: 1,
            ..JobLimits::default()
        },
        Wakeup::default(),
    )
    .unwrap()
}
fn pump(app: &Workbench, jobs: &mut JobSystem, controller: &mut GitDiffController) -> usize {
    let end = Instant::now() + Duration::from_secs(20);
    let mut replies = 0;
    loop {
        for mut completed in jobs.poll(app, 8) {
            if completed.key == GIT_DIFF_JOB_KEY {
                replies += 1;
                assert!(controller.accept(app, &mut completed));
            } else {
                assert!(!controller.accept(app, &mut completed));
            }
        }
        controller.tick(app, jobs);
        if jobs.outstanding() == 0
            && !matches!(
                controller.state(),
                GitDiffState::Queued | GitDiffState::Running
            )
        {
            return replies;
        }
        assert!(
            Instant::now() < end,
            "Comparison did not settle: {:?}: {}",
            controller.state(),
            controller.message()
        );
        thread::sleep(Duration::from_millis(1));
    }
}
fn wait_completion(jobs: &JobSystem) {
    let end = Instant::now() + Duration::from_secs(15);
    while !jobs.has_completed() {
        assert!(Instant::now() < end);
        thread::sleep(Duration::from_millis(1));
    }
}
fn gated(_: &TaskSnapshot, cancel: &Cancellation, _: &mut TaskOutput) -> Result<(), TaskFailure> {
    while !GATE.load(Ordering::Acquire) {
        if cancel.is_cancelled() {
            return Err(TaskFailure::Cancelled);
        }
        thread::sleep(Duration::from_millis(1));
    }
    Ok(())
}

#[test]
fn dirty_document_snapshot_compares_head_or_index_and_hunk_navigation_preserves_bytes_and_undo() {
    let _serial = SERIAL.lock().unwrap();
    let head = b"\xef\xbb\xbfold \xff\r\nlast";
    let staged = b"\xef\xbb\xbfINDEX \xff\r\nlast";
    let fixture = Fixture::new(head, staged);
    let mut app = fixture.app();
    app.insert_text("dirty ").unwrap();
    let bytes = app.active_bytes().unwrap();
    let revision = app.active_document().unwrap().session.document().revision();
    let index = fs::read(fixture.root.join(".git/index")).unwrap();
    let mut jobs = jobs();
    let mut controller = GitDiffController::new();
    for (source, expected) in [
        (GitBlobSource::Head, head.as_slice()),
        (GitBlobSource::Index, staged.as_slice()),
    ] {
        controller
            .request(&app, &mut jobs, fixture.options(source))
            .unwrap();
        pump(&app, &mut jobs, &mut controller);
        assert_eq!(
            controller.state(),
            GitDiffState::Ready,
            "{}",
            controller.message()
        );
        let result = controller.result().unwrap();
        assert_eq!(result.identity.source, source);
        assert_eq!(result.model.source_bytes(DiffSide::Left), expected);
        assert_eq!(result.model.source_bytes(DiffSide::Right), bytes);
        assert_eq!(result.model.hunks().count(), 1);
        assert!(result.model.memory_bytes() < 12 * 1024 * 1024);
        controller.select_relative(100);
        assert_eq!(controller.selected(), 0);
        controller.select_hunk(0, &mut app).unwrap();
        assert_eq!(app.active_editor().unwrap().cursor, 3);
        assert_eq!(app.active_bytes().unwrap(), bytes);
        assert_eq!(
            app.active_document().unwrap().session.document().revision(),
            revision
        );
        assert_eq!(fs::read(&fixture.path).unwrap(), head);
        assert_eq!(fs::read(fixture.root.join(".git/index")).unwrap(), index);
    }
    app.undo().unwrap();
    assert_eq!(app.active_bytes().unwrap(), head);
    assert!(controller.select_hunk(0, &mut app).is_err());
    assert_eq!(controller.state(), GitDiffState::Stale);
}

#[test]
fn supersession_keeps_one_snapshot_and_only_latest_pending_metadata_until_acknowledgement() {
    let _serial = SERIAL.lock().unwrap();
    let fixture = Fixture::new(b"HEAD\r\n", b"INDEX\r\n");
    let mut app = fixture.app();
    app.insert_text("dirty ").unwrap();
    let old_len = app.active_document().unwrap().session.document().len();
    let mut jobs = jobs();
    let mut controller = GitDiffController::new();
    GATE.store(false, Ordering::Release);
    jobs.submit(
        &app,
        JobKey(42),
        app.active_document().unwrap().id,
        None,
        0..0,
        gated,
    )
    .unwrap();
    controller
        .request(&app, &mut jobs, fixture.options(GitBlobSource::Head))
        .unwrap();
    assert_eq!(jobs.snapshot_bytes(), old_len);
    assert_eq!(jobs.artifact_bytes(), 12 * 1024 * 1024);
    app.insert_text("new ").unwrap();
    for _ in 0..8 {
        controller
            .request(&app, &mut jobs, fixture.options(GitBlobSource::Head))
            .unwrap();
    }
    controller
        .request(&app, &mut jobs, fixture.options(GitBlobSource::Index))
        .unwrap();
    assert_eq!(controller.state(), GitDiffState::Queued);
    assert!(controller.result().is_none());
    assert_eq!(jobs.outstanding(), 2);
    assert_eq!(jobs.snapshot_bytes(), old_len);
    assert_eq!(jobs.artifact_bytes(), 12 * 1024 * 1024);
    GATE.store(true, Ordering::Release);
    assert_eq!(pump(&app, &mut jobs, &mut controller), 2);
    assert_eq!(
        controller.state(),
        GitDiffState::Ready,
        "{}",
        controller.message()
    );
    let result = controller.result().unwrap();
    assert_eq!(result.identity.source, GitBlobSource::Index);
    assert_eq!(result.model.source_bytes(DiffSide::Left), b"INDEX\r\n");
    assert_eq!(
        result.model.source_bytes(DiffSide::Right),
        app.active_bytes().unwrap()
    );
    assert_eq!(jobs.artifact_bytes(), 0);
    assert_eq!(jobs.snapshot_bytes(), 0);
}

#[test]
fn cancelled_or_cleared_delivery_never_resurrects_and_a_new_request_waits_for_its_ack() {
    let _serial = SERIAL.lock().unwrap();
    let fixture = Fixture::new(b"old\n", b"index\n");
    let app = fixture.app();
    let mut jobs = jobs();
    let mut controller = GitDiffController::new();
    GATE.store(false, Ordering::Release);
    jobs.submit(
        &app,
        JobKey(43),
        app.active_document().unwrap().id,
        None,
        0..0,
        gated,
    )
    .unwrap();
    controller
        .request(&app, &mut jobs, fixture.options(GitBlobSource::Head))
        .unwrap();
    controller.cancel(&mut jobs);
    assert_eq!(controller.state(), GitDiffState::Cancelled);
    assert!(controller.result().is_none());
    assert_eq!(jobs.artifact_bytes(), 12 * 1024 * 1024);
    GATE.store(true, Ordering::Release);
    pump(&app, &mut jobs, &mut controller);
    assert_eq!(controller.state(), GitDiffState::Cancelled);
    assert!(controller.result().is_none());
    controller
        .request(&app, &mut jobs, fixture.options(GitBlobSource::Index))
        .unwrap();
    controller.clear(&mut jobs);
    assert_eq!(controller.state(), GitDiffState::Idle);
    controller
        .request(&app, &mut jobs, fixture.options(GitBlobSource::Head))
        .unwrap();
    assert_eq!(controller.state(), GitDiffState::Queued);
    pump(&app, &mut jobs, &mut controller);
    assert_eq!(controller.state(), GitDiffState::Ready);
    assert_eq!(
        controller.result().unwrap().identity.source,
        GitBlobSource::Head
    );
}

#[test]
fn completed_but_undelivered_artifacts_reject_revision_encoding_view_document_path_and_close_changes(
) {
    let _serial = SERIAL.lock().unwrap();
    let fixture = Fixture::new(b"a\r\nb", b"index\r\nb");
    for change in 0..6 {
        let mut app = fixture.app();
        let mut jobs = jobs();
        let mut controller = GitDiffController::new();
        controller
            .request(&app, &mut jobs, fixture.options(GitBlobSource::Head))
            .unwrap();
        wait_completion(&jobs);
        assert_eq!(jobs.artifact_bytes(), 12 * 1024 * 1024); // completion has not been polled
        assert_eq!(jobs.outstanding(), 1);
        match change {
            0 => app.insert_text("edit").unwrap(),
            1 => app.set_encoding(TextEncoding::Iso8859_1).unwrap(),
            2 => {
                app.split_editor_right();
                app.focus_editor_pane(PaneId::Secondary);
            }
            3 => {
                app.new_file(fixture.root.join("other.p"), TextEncoding::Utf8);
            }
            4 => app
                .save_active_as(fixture.root.join(format!("renamed-{change}.p")))
                .unwrap(),
            _ => {
                app.close_active(true).unwrap();
            }
        }
        pump(&app, &mut jobs, &mut controller);
        assert_eq!(
            controller.state(),
            GitDiffState::Stale,
            "change={change}: {}",
            controller.message()
        );
        assert!(controller.result().is_none());
        assert!(controller.select_hunk(0, &mut app).is_err());
    }
}

#[test]
fn byte_and_artifact_quotas_fail_before_copy_or_worker_creation() {
    let _serial = SERIAL.lock().unwrap();
    let mut app = Workbench::default();
    app.new_file("C:/unused/large.p", TextEncoding::Utf8);
    app.insert_text(&"x".repeat(2 * 1024 * 1024 + 1)).unwrap();
    let options = GitDiffOptions {
        git_executable: "C:/unused/git.exe".into(),
        root: "C:/unused".into(),
        cwd: "C:/unused".into(),
        source: GitBlobSource::Head,
    };
    let mut jobs = jobs();
    let mut controller = GitDiffController::new();
    assert!(controller
        .request(&app, &mut jobs, options.clone())
        .is_err());
    assert_eq!(controller.state(), GitDiffState::Limited);
    assert_eq!(jobs.worker_count(), 0);
    assert_eq!(jobs.snapshot_bytes(), 0);
    assert_eq!(jobs.artifact_bytes(), 0);
    let mut app = Workbench::default();
    app.new_file("C:/unused/small.p", TextEncoding::Utf8);
    let mut jobs = JobSystem::new(
        JobLimits {
            artifact_bytes: 11 * 1024 * 1024,
            ..JobLimits::default()
        },
        Wakeup::default(),
    )
    .unwrap();
    assert!(controller.request(&app, &mut jobs, options).is_err());
    assert_eq!(controller.state(), GitDiffState::Limited);
    assert_eq!(jobs.worker_count(), 0);
    assert_eq!(jobs.outstanding(), 0);
}

#[test]
fn temporary_shared_snapshot_pressure_keeps_git_queued_until_plain_completion_is_polled() {
    let _serial = SERIAL.lock().unwrap();
    let fixture = Fixture::new(b"old\n", b"idx\n");
    let app = fixture.app();
    let mut jobs = JobSystem::new(
        JobLimits {
            workers: 1,
            snapshot_bytes: 4,
            total_snapshot_bytes: 4,
            ..JobLimits::default()
        },
        Wakeup::default(),
    )
    .unwrap();
    let mut controller = GitDiffController::new();
    GATE.store(false, Ordering::Release);
    jobs.submit(
        &app,
        JobKey(44),
        app.active_document().unwrap().id,
        None,
        0..4,
        gated,
    )
    .unwrap();
    controller
        .request(&app, &mut jobs, fixture.options(GitBlobSource::Head))
        .unwrap();
    for _ in 0..3 {
        controller.tick(&app, &mut jobs);
        assert_eq!(controller.state(), GitDiffState::Queued);
        assert!(controller.result().is_none());
        assert_eq!(jobs.outstanding(), 1); // only the Plain job was admitted
        assert_eq!(jobs.snapshot_bytes(), 4);
        assert_eq!(jobs.artifact_bytes(), 0);
    }
    GATE.store(true, Ordering::Release);
    wait_completion(&jobs);
    controller.tick(&app, &mut jobs);
    assert_eq!(controller.state(), GitDiffState::Queued);
    assert_eq!(jobs.snapshot_bytes(), 4); // completed but not consumed yet
    for mut completion in jobs.poll(&app, 4) {
        assert_eq!(completion.key, JobKey(44));
        assert!(!controller.accept(&app, &mut completion));
    }
    assert_eq!(jobs.outstanding(), 0);
    assert_eq!(jobs.snapshot_bytes(), 0);
    controller.tick(&app, &mut jobs);
    assert_eq!(controller.state(), GitDiffState::Running);
    assert_eq!(jobs.outstanding(), 1);
    assert_eq!(jobs.snapshot_bytes(), 4);
    assert_eq!(jobs.artifact_bytes(), 12 * 1024 * 1024);
    assert_eq!(pump(&app, &mut jobs, &mut controller), 1);
    assert_eq!(
        controller.state(),
        GitDiffState::Ready,
        "{}",
        controller.message()
    );
    let result = controller.result().unwrap();
    assert_eq!(result.model.source_bytes(DiffSide::Left), b"old\n");
    assert_eq!(result.model.source_bytes(DiffSide::Right), b"old\n");
    assert_eq!(jobs.snapshot_bytes(), 0);
    assert_eq!(jobs.artifact_bytes(), 0);
}

#[test]
fn root_escape_and_missing_executable_are_worker_failures_without_source_writes() {
    let _serial = SERIAL.lock().unwrap();
    let fixture = Fixture::new(b"source\n", b"index\n");
    let outside = fixture.container.join("outside.p");
    fs::write(&outside, b"outside\r\n").unwrap();
    let mut app = Workbench::default();
    app.open_file(&outside, Some(TextEncoding::Utf8)).unwrap();
    let mut jobs = jobs();
    let mut controller = GitDiffController::new();
    controller
        .request(&app, &mut jobs, fixture.options(GitBlobSource::Head))
        .unwrap();
    pump(&app, &mut jobs, &mut controller);
    assert_eq!(controller.state(), GitDiffState::Failed);
    assert!(controller.message().contains("outside"));
    assert!(controller.result().is_none());
    assert_eq!(fs::read(&outside).unwrap(), b"outside\r\n");
    let app = fixture.app();
    let mut options = fixture.options(GitBlobSource::Head);
    options.git_executable = fixture.container.join("missing-git.exe");
    assert!(controller.request(&app, &mut jobs, options).is_ok()); // filesystem work is not in request
    assert_eq!(controller.state(), GitDiffState::Running);
    pump(&app, &mut jobs, &mut controller);
    assert_eq!(controller.state(), GitDiffState::Failed);
    assert!(controller.message().len() <= 512);
    assert_eq!(fs::read(&fixture.path).unwrap(), b"source\n");
}

#[test]
fn deleted_hunk_at_eof_navigates_without_replacing_bytes() {
    let _serial = SERIAL.lock().unwrap();
    let fixture = Fixture::new(b"a\nb\n", b"a\nb\n");
    let mut app = fixture.app();
    app.select_all().unwrap();
    app.insert_text("a\n").unwrap();
    let revision = app.active_document().unwrap().session.document().revision();
    let mut jobs = jobs();
    let mut controller = GitDiffController::new();
    controller
        .request(&app, &mut jobs, fixture.options(GitBlobSource::Head))
        .unwrap();
    pump(&app, &mut jobs, &mut controller);
    assert_eq!(controller.state(), GitDiffState::Ready);
    controller.select_hunk(0, &mut app).unwrap();
    assert_eq!(app.active_editor().unwrap().cursor, 2);
    assert_eq!(app.active_bytes().unwrap(), b"a\n");
    assert_eq!(
        app.active_document().unwrap().session.document().revision(),
        revision
    );
}

#[test]
fn legacy_dirty_snapshot_stays_in_its_page_and_encoding_change_invalidates_navigation() {
    let _serial = SERIAL.lock().unwrap();
    let original = b"ni\xf1o\r\nlast";
    let fixture = Fixture::new(original, original);
    let mut app = Workbench::default();
    app.open_file(&fixture.path, Some(TextEncoding::Windows1252))
        .unwrap();
    app.insert_text("á ").unwrap();
    let expected = b"\xe1 ni\xf1o\r\nlast";
    let mut jobs = jobs();
    let mut controller = GitDiffController::new();
    controller
        .request(&app, &mut jobs, fixture.options(GitBlobSource::Head))
        .unwrap();
    pump(&app, &mut jobs, &mut controller);
    assert_eq!(
        controller.state(),
        GitDiffState::Ready,
        "{}",
        controller.message()
    );
    assert_eq!(
        controller
            .result()
            .unwrap()
            .model
            .source_bytes(DiffSide::Right),
        expected
    );
    assert_eq!(app.active_bytes().unwrap(), expected);
    assert_eq!(fs::read(&fixture.path).unwrap(), original);
    app.set_encoding(TextEncoding::Ibm850).unwrap();
    assert!(controller.select_hunk(0, &mut app).is_err());
    assert_eq!(controller.state(), GitDiffState::Stale);
    assert_eq!(app.active_bytes().unwrap(), expected);
}
