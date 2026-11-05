use super::TuiApp;
use crate::state::{LaunchOptions, UiOverlay};
use jc_app::{git_diff::GitDiffState, CommandId};
use jc_text::TextEncoding;

#[test]
fn git_commands_require_repository_context_but_no_executable_configuration() {
    let mut app = TuiApp::new(LaunchOptions::default());
    app.dispatch(CommandId::ViewGit);
    assert!(matches!(app.overlay, UiOverlay::None));
    assert_eq!(app.tasks.worker_count(), 0);
    app.workbench.new_file("unwritten.p", TextEncoding::Utf8);
    app.workbench.insert_text("original\r\n").unwrap();
    app.project_root = Some(app.git_cwd.clone());
    app.dispatch(CommandId::GitConfigureExecutable);
    assert!(matches!(app.overlay, UiOverlay::None));
    assert!(app.status.contains("integrado"));
    assert_eq!(app.tasks.worker_count(), 0);
    assert_eq!(app.workbench.active_bytes().unwrap(), b"original\r\n");
}

#[test]
fn failed_git_task_is_delivered_without_input_and_frame_composition_preserves_document() {
    if !cfg!(windows) {
        return;
    }
    let mut app = TuiApp::new(LaunchOptions::default());
    let root = app.git_cwd.join("work/test-temp").join(format!(
        "tui-not-a-repository-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    app.workbench
        .new_file(root.join("unwritten-git-test.p"), TextEncoding::Utf8);
    app.workbench.insert_text("dirty\r\n").unwrap();
    app.project_root = Some(root);
    app.dispatch(CommandId::ViewGit);
    assert_eq!(app.git_diff.state(), GitDiffState::Running);
    let end = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while app.tasks.outstanding() > 0 {
        app.poll_tasks();
        assert!(std::time::Instant::now() < end);
        std::thread::yield_now();
    }
    assert_eq!(app.git_diff.state(), GitDiffState::Failed);
    assert!(!app.status.is_empty());
    let revision = app
        .workbench
        .active_document()
        .unwrap()
        .session
        .document()
        .revision();
    for (width, height) in [(45, 15), (140, 40)] {
        let prepared = app.prepare_frame(width, height);
        let first = crate::render::render(&prepared);
        let second = crate::render::render(&prepared);
        for y in 0..height {
            assert_eq!(first.row_text(y), second.row_text(y));
        }
    }
    assert_eq!(app.workbench.active_bytes().unwrap(), b"dirty\r\n");
    assert_eq!(
        app.workbench
            .active_document()
            .unwrap()
            .session
            .document()
            .revision(),
        revision
    );
    assert_eq!(app.tasks.artifact_bytes(), 0);
    assert_eq!(app.tasks.snapshot_bytes(), 0);
    app.workbench.undo().unwrap();
    assert!(app.workbench.active_bytes().unwrap().is_empty());
}
