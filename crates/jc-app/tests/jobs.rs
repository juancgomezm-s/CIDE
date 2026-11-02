use jc_app::*;
use jc_text::TextEncoding;
use std::{
    ops::Range,
    sync::{Condvar, Mutex},
    thread,
    time::{Duration, Instant},
};

fn app() -> Workbench {
    let mut app = Workbench::default();
    app.new_file("jobs-memory.txt", TextEncoding::Utf8);
    app.insert_text("abc").unwrap();
    app
}
fn target(app: &Workbench) -> DocumentId {
    app.active_document().unwrap().id
}
fn echo(
    snapshot: &TaskSnapshot,
    _: &Cancellation,
    output: &mut TaskOutput,
) -> Result<(), TaskFailure> {
    output.push(TaskItem::Message(format!("{:x?}", snapshot.bytes())))
}
fn wait_cancel(
    _: &TaskSnapshot,
    cancel: &Cancellation,
    _: &mut TaskOutput,
) -> Result<(), TaskFailure> {
    while !cancel.is_cancelled() {
        thread::sleep(Duration::from_millis(1));
    }
    Err(TaskFailure::Cancelled)
}
fn collect(jobs: &mut JobSystem, app: &Workbench, wakeup: &Wakeup) -> Vec<CompletedTask> {
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut events = Vec::new();
    while jobs.outstanding() > 0 {
        let ticket = wakeup.ticket();
        events.extend(jobs.poll(app, 64));
        assert!(Instant::now() < deadline, "tasks did not finish");
        if jobs.outstanding() > 0 && !jobs.has_completed() {
            wakeup.wait(ticket, Some(Duration::from_millis(50)));
        }
    }
    events
}
fn submit(
    jobs: &mut JobSystem,
    app: &Workbench,
    key: u64,
    range: Range<usize>,
    provider: TaskProvider,
) -> Result<JobId, SubmitError> {
    jobs.submit(app, JobKey(key), target(app), None, range, provider)
}

#[test]
fn admission_is_lazy_and_bounded_including_completed_work_until_delivery() {
    let app = app();
    let wake = Wakeup::default();
    let limits = JobLimits {
        workers: 1,
        outstanding: 1,
        snapshot_bytes: 3,
        total_snapshot_bytes: 3,
        ..JobLimits::default()
    };
    let mut jobs = JobSystem::new(limits, wake.clone()).unwrap();
    assert_eq!(jobs.worker_count(), 0);
    assert_eq!(
        submit(&mut jobs, &app, 0, 0..4, echo),
        Err(SubmitError::SnapshotLimit)
    );
    assert_eq!(jobs.worker_count(), 0);
    submit(&mut jobs, &app, 0, 0..3, echo).unwrap();
    assert_eq!(
        submit(&mut jobs, &app, 1, 0..1, echo),
        Err(SubmitError::Busy)
    );
    assert_eq!(jobs.snapshot_bytes(), 3);
    let events = collect(&mut jobs, &app, &wake);
    assert!(events[0].applicable);
    assert_eq!(jobs.snapshot_bytes(), 0);
    assert_eq!(jobs.worker_count(), 1);
    assert_eq!(
        events[0].output.items(),
        &[TaskItem::Message("[61, 62, 63]".to_owned())]
    );
}

#[test]
fn replacing_a_generation_cancels_the_old_job_and_delivers_only_the_latest() {
    let app = app();
    let wake = Wakeup::default();
    let mut jobs = JobSystem::new(
        JobLimits {
            workers: 1,
            ..JobLimits::default()
        },
        wake.clone(),
    )
    .unwrap();
    let old = submit(&mut jobs, &app, 5, 0..3, wait_cancel).unwrap();
    let latest = submit(&mut jobs, &app, 5, 0..3, echo).unwrap();
    let events = collect(&mut jobs, &app, &wake);
    assert!(!events.iter().find(|e| e.id == old).unwrap().applicable);
    assert!(events.iter().find(|e| e.id == latest).unwrap().applicable);
}

static GATE: Mutex<(bool, bool)> = Mutex::new((false, false));
static SIGNAL: Condvar = Condvar::new();
fn snapshot_after_edit(
    snapshot: &TaskSnapshot,
    cancel: &Cancellation,
    output: &mut TaskOutput,
) -> Result<(), TaskFailure> {
    let mut gate = GATE.lock().unwrap();
    gate.0 = true;
    SIGNAL.notify_all();
    while !gate.1 && !cancel.is_cancelled() {
        gate = SIGNAL
            .wait_timeout(gate, Duration::from_millis(1))
            .unwrap()
            .0;
    }
    drop(gate);
    assert_eq!(snapshot.bytes(), b"abc");
    echo(snapshot, cancel, output)
}
#[test]
fn worker_snapshot_is_owned_and_revision_staleness_prevents_delivery_after_edit() {
    let mut app = app();
    let wake = Wakeup::default();
    let mut jobs = JobSystem::new(JobLimits::default(), wake.clone()).unwrap();
    submit(&mut jobs, &app, 1, 0..3, snapshot_after_edit).unwrap();
    let guard = GATE.lock().unwrap();
    let (guard, timeout) = SIGNAL
        .wait_timeout_while(guard, Duration::from_secs(2), |g| !g.0)
        .unwrap();
    assert!(!timeout.timed_out());
    drop(guard);
    app.insert_text("x").unwrap();
    GATE.lock().unwrap().1 = true;
    SIGNAL.notify_all();
    let events = collect(&mut jobs, &app, &wake);
    assert!(!events[0].applicable);
    assert!(events[0].output.items().is_empty());
    assert_ne!(events[0].status, TaskStatus::Panicked);
    assert_eq!(app.active_bytes().unwrap(), b"abcx");
}

#[test]
fn closed_view_encoding_change_and_reopened_document_invalidate_stamps() {
    let mut app = app();
    app.split_editor_right();
    let id = target(&app);
    let view = app.panes().view(PaneId::Primary).unwrap().view_id();
    let stamp = TaskStamp {
        document: id,
        revision: app.active_document().unwrap().session.document().revision(),
        encoding: TextEncoding::Utf8,
        view,
    };
    assert!(app.task_stamp_is_current(stamp));
    app.close_active(false).unwrap();
    assert!(!app.task_stamp_is_current(stamp));
    let stamp = TaskStamp {
        view: None,
        ..stamp
    };
    assert!(app.task_stamp_is_current(stamp));
    app.set_encoding(TextEncoding::Iso8859_1).unwrap();
    assert!(!app.task_stamp_is_current(stamp));
    app.close_active(true).unwrap();
    app.reopen_closed();
    assert!(!app.task_stamp_is_current(stamp));
    assert_ne!(target(&app), id);
}

fn too_many(_: &TaskSnapshot, _: &Cancellation, out: &mut TaskOutput) -> Result<(), TaskFailure> {
    for _ in 0..10 {
        let _ = out.push(TaskItem::Message("x".to_owned()));
    }
    Ok(())
}
fn huge_capacity(
    _: &TaskSnapshot,
    _: &Cancellation,
    out: &mut TaskOutput,
) -> Result<(), TaskFailure> {
    let mut s = String::with_capacity(1_000_000);
    s.push('x');
    out.push(TaskItem::Message(s))
}
fn invalid_range(
    _: &TaskSnapshot,
    _: &Cancellation,
    out: &mut TaskOutput,
) -> Result<(), TaskFailure> {
    out.push(TaskItem::ProposedEdit {
        range: 0..99,
        text: "x".to_owned(),
    })
}
#[test]
fn output_quotas_are_latched_and_reject_partial_results_and_invalid_ranges() {
    let app = app();
    let wake = Wakeup::default();
    let mut jobs = JobSystem::new(
        JobLimits {
            result_items: 2,
            ..JobLimits::default()
        },
        wake.clone(),
    )
    .unwrap();
    for (provider, status) in [
        (too_many as TaskProvider, TaskStatus::LimitExceeded),
        (huge_capacity, TaskStatus::LimitExceeded),
        (invalid_range, TaskStatus::InvalidRange),
    ] {
        submit(&mut jobs, &app, 1, 0..3, provider).unwrap();
        let result = collect(&mut jobs, &app, &wake).pop().unwrap();
        assert_eq!(result.status, status);
        assert!(result.current);
        assert!(!result.applicable);
        assert!(result.output.items().is_empty());
    }
}

fn panic_provider(
    _: &TaskSnapshot,
    _: &Cancellation,
    _: &mut TaskOutput,
) -> Result<(), TaskFailure> {
    panic!("test provider panic")
}
#[test]
fn deadline_cancel_and_provider_panic_release_admission_and_keep_worker_usable() {
    let app = app();
    let wake = Wakeup::default();
    let mut jobs = JobSystem::new(
        JobLimits {
            workers: 1,
            timeout: Duration::from_millis(30),
            ..JobLimits::default()
        },
        wake.clone(),
    )
    .unwrap();
    submit(&mut jobs, &app, 1, 0..3, wait_cancel).unwrap();
    assert_eq!(
        collect(&mut jobs, &app, &wake)[0].status,
        TaskStatus::TimedOut
    );
    submit(&mut jobs, &app, 1, 0..3, wait_cancel).unwrap();
    jobs.cancel(JobKey(1));
    assert_eq!(
        collect(&mut jobs, &app, &wake)[0].status,
        TaskStatus::Cancelled
    );
    submit(&mut jobs, &app, 1, 0..3, panic_provider).unwrap();
    let deadline = Instant::now() + Duration::from_secs(1);
    while !jobs.has_completed() {
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    // A delayed UI must still learn that the panic hook may have damaged its
    // terminal, even when cancellation/deadline now prevents result publication.
    jobs.cancel(JobKey(1));
    thread::sleep(Duration::from_millis(40));
    assert_eq!(
        collect(&mut jobs, &app, &wake)[0].status,
        TaskStatus::Panicked
    );
    submit(&mut jobs, &app, 1, 0..3, echo).unwrap();
    assert!(collect(&mut jobs, &app, &wake)[0].applicable);
}

#[test]
fn completion_wakes_a_waiter_without_keyboard_input_or_polling_the_provider() {
    let app = app();
    let wake = Wakeup::default();
    let mut jobs = JobSystem::new(JobLimits::default(), wake.clone()).unwrap();
    let ticket = wake.ticket();
    submit(&mut jobs, &app, 1, 0..3, echo).unwrap();
    wake.wait(ticket, Some(Duration::from_secs(1)));
    assert_ne!(wake.ticket(), ticket);
    assert!(jobs.has_completed());
    assert!(jobs.poll(&app, 1)[0].applicable);
}

#[test]
fn panel_logs_bound_bytes_and_mark_truncated_lines_without_changing_documents() {
    let mut app = app();
    for _ in 0..500 {
        app.push_output("é".repeat(5000));
        app.push_problem("x".repeat(5000));
    }
    for log in [app.output(), app.problems()] {
        assert!(
            log.iter()
                .map(|s| s.capacity() + std::mem::size_of::<String>())
                .sum::<usize>()
                <= 256 * 1024
        );
        assert!(log
            .iter()
            .all(|s| s.len() <= 4096 && s.ends_with("[truncated]")));
    }
    assert_eq!(app.active_bytes().unwrap(), b"abc");
}
