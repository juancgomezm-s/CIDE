//! Opt-in internal-provider latency; excludes terminal and real language services.
use jc_app::*;
use jc_text::TextEncoding;
use std::{
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};

static STARTED: AtomicBool = AtomicBool::new(false);
fn message(_: &TaskSnapshot, _: &Cancellation, output: &mut TaskOutput) -> Result<(), TaskFailure> {
    output.push(TaskItem::Message("ready".to_owned()))
}
fn cooperative(
    _: &TaskSnapshot,
    cancel: &Cancellation,
    _: &mut TaskOutput,
) -> Result<(), TaskFailure> {
    STARTED.store(true, Ordering::Release);
    while !cancel.is_cancelled() {
        thread::sleep(Duration::from_millis(1));
    }
    Err(TaskFailure::Cancelled)
}
fn finish(jobs: &mut JobSystem, app: &Workbench, wake: &Wakeup) -> CompletedTask {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let ticket = wake.ticket();
        if let Some(result) = jobs.poll(app, 1).pop() {
            return result;
        }
        assert!(Instant::now() < deadline);
        wake.wait(ticket, Some(Duration::from_millis(100)));
    }
}
fn main() {
    let mut app = Workbench::default();
    app.new_file("f3-jobs.txt", TextEncoding::Utf8);
    app.insert_text("abc").unwrap();
    let doc = app.active_document().unwrap().id;
    let wake = Wakeup::default();
    let mut jobs = JobSystem::new(JobLimits::default(), wake.clone()).unwrap();
    println!("operation,samples,p50_us,p95_us");
    for (name, provider) in [
        ("submit_to_delivery", message as TaskProvider),
        ("cancel_to_delivery", cooperative as TaskProvider),
    ] {
        let mut times = Vec::new();
        for index in 0..36 {
            STARTED.store(false, Ordering::Release);
            let mut started = Instant::now();
            jobs.submit(&app, JobKey(1), doc, None, 0..3, provider)
                .unwrap();
            if name == "cancel_to_delivery" {
                let deadline = Instant::now() + Duration::from_secs(3);
                while !STARTED.load(Ordering::Acquire) {
                    assert!(Instant::now() < deadline);
                    thread::yield_now();
                }
                started = Instant::now();
                jobs.cancel(JobKey(1));
            }
            let result = finish(&mut jobs, &app, &wake);
            if name == "submit_to_delivery" {
                assert!(result.applicable);
            } else {
                assert_eq!(result.status, TaskStatus::Cancelled);
            }
            if index >= 5 {
                times.push(started.elapsed().as_micros());
            }
        }
        times.sort_unstable();
        println!("{name},31,{},{}", times[15], times[29]);
    }
}
