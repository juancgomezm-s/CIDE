use jc_app::{
    source_services::{
        SourceAction, SourceController, SourceItem, SourceMode, SourceProviderDescriptor,
        SourceRegistry, SourceState,
    },
    Cancellation, JobSystem, TaskFailure, TaskItem, TaskOutput, TaskSnapshot, Wakeup, Workbench,
};
use jc_text::{TextEncoding, TextPosition};
use std::time::{Duration, Instant};
fn app(text: &str) -> Workbench {
    let mut a = Workbench::default();
    a.new_file("source-memory.txt", TextEncoding::Utf8);
    a.insert_text(text).unwrap();
    a
}
fn finish(controller: &mut SourceController, a: &Workbench, jobs: &mut JobSystem) {
    let until = Instant::now() + Duration::from_secs(5);
    while jobs.outstanding() > 0
        || matches!(
            controller.state(),
            SourceState::Queued | SourceState::Running
        )
    {
        for mut task in jobs.poll(a, 4) {
            controller.accept(a, None, &mut task);
        }
        controller.tick(a, None, jobs);
        assert!(Instant::now() < until);
        std::thread::yield_now();
    }
}
fn symbols(_: &TaskSnapshot, _: &Cancellation, out: &mut TaskOutput) -> Result<(), TaskFailure> {
    out.push(TaskItem::Source(SourceItem {
        range: 0..1,
        label: "one (lexical)".into(),
        action: SourceAction::Navigate,
    }))
}
fn registry() -> SourceRegistry {
    let mut r = SourceRegistry::default();
    r.register(SourceProviderDescriptor {
        id: "test.lexical",
        language: "text",
        symbols: Some(symbols),
        references: None,
        completion: None,
        snippets: &[],
    })
    .unwrap();
    r
}
#[test]
fn source_registry_capabilities_are_optional_namespaced_and_default_completion_needs_no_provider_process(
) {
    let mut r = registry();
    assert!(r.resolve("text", SourceMode::Symbols).is_some());
    assert!(r.resolve("rust", SourceMode::Symbols).is_none());
    assert_eq!(
        r.resolve("rust", SourceMode::Completion).unwrap().id,
        "core.words"
    );
    assert!(r
        .register(SourceProviderDescriptor {
            id: "bad id",
            language: "rust",
            symbols: Some(symbols),
            references: None,
            completion: None,
            snippets: &[],
        })
        .is_err());
    assert!(r
        .register(SourceProviderDescriptor {
            id: "test.lexical",
            language: "rust",
            symbols: Some(symbols),
            references: None,
            completion: None,
            snippets: &[],
        })
        .is_err());
    let a = Workbench::default();
    let mut c = SourceController::default();
    let mut jobs = JobSystem::new(Default::default(), Wakeup::default()).unwrap();
    assert!(c
        .request(SourceMode::Symbols, &r, &a, None, &mut jobs)
        .is_err());
    assert_eq!(jobs.worker_count(), 0);
}
#[test]
fn source_local_word_completion_is_revision_checked_strict_and_one_undo() {
    for encoding in [
        TextEncoding::Utf8,
        TextEncoding::Windows1252,
        TextEncoding::Iso8859_1,
        TextEncoding::Ibm850,
    ] {
        let mut a = Workbench::default();
        a.new_file("words.txt", encoding);
        a.insert_text("niño cliente customer custom\r\nni").unwrap();
        let before = a.active_bytes().unwrap();
        let r = SourceRegistry::default();
        let mut c = SourceController::default();
        let mut jobs = JobSystem::new(Default::default(), Wakeup::default()).unwrap();
        c.request(SourceMode::Completion, &r, &a, None, &mut jobs)
            .unwrap();
        finish(&mut c, &a, &mut jobs);
        assert_eq!(c.state(), SourceState::Ready);
        assert_eq!(c.items().len(), 1);
        assert_eq!(c.items()[0].action, SourceAction::Replace("niño".into()));
        c.activate(&mut a, None, false).unwrap();
        assert!(a
            .active_bytes()
            .unwrap()
            .ends_with(&jc_text::encode("niño", encoding).unwrap()));
        a.undo().unwrap();
        assert_eq!(a.active_bytes().unwrap(), before);
    }
}
#[test]
fn source_completion_rejects_changed_caret_selection_revision_and_root() {
    for change in 0..4 {
        let mut a = app("customer custom cus");
        let r = SourceRegistry::default();
        let mut c = SourceController::default();
        let mut jobs = JobSystem::new(Default::default(), Wakeup::default()).unwrap();
        c.request(SourceMode::Completion, &r, &a, None, &mut jobs)
            .unwrap();
        finish(&mut c, &a, &mut jobs);
        match change {
            0 => a.move_left(false).unwrap(),
            1 => a.select_all().unwrap(),
            2 => a.insert_text("x").unwrap(),
            _ => {}
        };
        let root = (change == 3).then_some(std::path::Path::new("different-root"));
        let before = a.active_bytes().unwrap();
        assert!(!c.ready(&a, root));
        assert!(c.activate(&mut a, root, false).is_err());
        assert_eq!(a.active_bytes().unwrap(), before);
        assert!(c.validate_context(&a, root, &mut jobs));
        assert_eq!(c.state(), SourceState::Stale);
    }
}
#[test]
fn source_refresh_storm_cancel_and_bounded_presentation_do_not_retain_extra_snapshots() {
    let a = app("abcdef");
    let r = registry();
    let mut c = SourceController::default();
    let mut jobs = JobSystem::new(Default::default(), Wakeup::default()).unwrap();
    c.request(SourceMode::Symbols, &r, &a, None, &mut jobs)
        .unwrap();
    let charge = jobs.snapshot_bytes();
    for _ in 0..10 {
        c.request(SourceMode::Symbols, &r, &a, None, &mut jobs)
            .unwrap();
        assert_eq!(jobs.outstanding(), 1);
        assert_eq!(jobs.snapshot_bytes(), charge);
    }
    finish(&mut c, &a, &mut jobs);
    assert_eq!(c.state(), SourceState::Ready);
    for width in [0, 1, 20, usize::MAX] {
        let p = c.present(usize::MAX, width);
        assert!(p.rows.len() <= 64);
        assert!(p
            .rows
            .iter()
            .all(|r| r.label.is_ascii() && r.label.len() <= width.min(256)));
    }
    c.cancel(&mut jobs);
    assert!(c.items().is_empty());
    assert_eq!(c.state(), SourceState::Cancelled);
    assert_eq!(jobs.snapshot_bytes(), 0);
}
fn wrong_mode(_: &TaskSnapshot, _: &Cancellation, out: &mut TaskOutput) -> Result<(), TaskFailure> {
    out.push(TaskItem::Source(SourceItem {
        range: 0..1,
        label: "unexpected edit".into(),
        action: SourceAction::Replace("X".into()),
    }))
}
fn too_many(_: &TaskSnapshot, _: &Cancellation, out: &mut TaskOutput) -> Result<(), TaskFailure> {
    for _ in 0..65 {
        out.push(TaskItem::Source(SourceItem {
            range: 0..1,
            label: "symbol".into(),
            action: SourceAction::Navigate,
        }))?;
    }
    Ok(())
}
#[test]
fn source_provider_wrong_action_and_result_quota_cannot_publish_or_edit() {
    for (provider, expected) in [
        (wrong_mode as jc_app::TaskProvider, SourceState::Failed),
        (too_many, SourceState::Limited),
    ] {
        let mut r = SourceRegistry::default();
        r.register(SourceProviderDescriptor {
            id: "test.invalid",
            language: "text",
            symbols: Some(provider),
            references: None,
            completion: None,
            snippets: &[],
        })
        .unwrap();
        let a = app("abc");
        let mut c = SourceController::default();
        let mut jobs = JobSystem::new(Default::default(), Wakeup::default()).unwrap();
        c.request(SourceMode::Symbols, &r, &a, None, &mut jobs)
            .unwrap();
        finish(&mut c, &a, &mut jobs);
        assert_eq!(c.state(), expected);
        assert!(c.items().is_empty());
        assert_eq!(a.active_bytes().unwrap(), b"abc");
    }
}
#[test]
fn source_snapshot_limit_is_checked_before_worker_start_and_completion_uses_only_prefix() {
    let mut a = app(&"x".repeat(2 * 1024 * 1024 + 8));
    let r = registry();
    let mut c = SourceController::default();
    let mut jobs = JobSystem::new(Default::default(), Wakeup::default()).unwrap();
    assert!(c
        .request(SourceMode::Symbols, &r, &a, None, &mut jobs)
        .is_err());
    assert_eq!(c.state(), SourceState::Limited);
    assert_eq!(jobs.worker_count(), 0);
    a.go_to_position(TextPosition { line: 0, column: 8 })
        .unwrap();
    c.request(SourceMode::Completion, &r, &a, None, &mut jobs)
        .unwrap();
    assert_eq!(jobs.snapshot_bytes(), 8);
    finish(&mut c, &a, &mut jobs);
    assert_eq!(c.state(), SourceState::Ready);
    assert!(c.items().is_empty());
}

fn wait_cancel(
    _: &TaskSnapshot,
    cancel: &Cancellation,
    _: &mut TaskOutput,
) -> Result<(), TaskFailure> {
    while !cancel.is_cancelled() {
        std::thread::yield_now();
    }
    Err(TaskFailure::Cancelled)
}
#[test]
fn source_temporary_shared_pressure_keeps_only_metadata_until_capacity_is_released() {
    let a = app(&"x".repeat(64));
    let r = registry();
    let mut c = SourceController::default();
    let mut jobs = JobSystem::new(
        jc_app::JobLimits {
            workers: 1,
            snapshot_bytes: 64,
            total_snapshot_bytes: 64,
            ..Default::default()
        },
        Wakeup::default(),
    )
    .unwrap();
    jobs.submit(
        &a,
        jc_app::JobKey(42),
        a.active_document().unwrap().id,
        None,
        0..60,
        wait_cancel,
    )
    .unwrap();
    c.request(SourceMode::Symbols, &r, &a, None, &mut jobs)
        .unwrap();
    assert_eq!(c.state(), SourceState::Queued);
    assert_eq!(jobs.snapshot_bytes(), 60);
    assert_eq!(jobs.outstanding(), 1);
    jobs.cancel(jc_app::JobKey(42));
    finish(&mut c, &a, &mut jobs);
    assert_eq!(c.state(), SourceState::Ready);
    assert_eq!(c.items().len(), 1);
    assert_eq!(jobs.snapshot_bytes(), 0);
}
