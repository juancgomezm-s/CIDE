//! Opt-in local corpus benchmark; excludes terminal transport and physical display.
use jc_app::*;
use std::{
    fs,
    time::{Duration, Instant},
};
fn main() {
    let root = std::env::temp_dir().join(format!("jc-search-bench-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let mut corpus = b"01234567\n".repeat(512 * 1024 / 9);
    for chunk in corpus.chunks_mut(32 * 1024) {
        chunk[..6].copy_from_slice(b"needle");
    }
    for i in 0..4 {
        fs::write(root.join(format!("source-{i}.p")), &corpus).unwrap();
    }
    let root = root.canonicalize().unwrap();
    let app = Workbench::default();
    let wake = Wakeup::default();
    let mut search = ProjectSearch::new(wake.clone());
    let mut first = Vec::new();
    let mut complete = Vec::new();
    let mut cancel = Vec::new();
    for index in 0..36 {
        let start = Instant::now();
        search
            .start(&app, &root, ProjectSearchOptions::literal("needle"))
            .unwrap();
        let mut first_at = None;
        let deadline = start + Duration::from_secs(5);
        while search.state == ProjectSearchState::Running {
            let ticket = wake.ticket();
            search.poll(&app, 1);
            if !search.rows().is_empty() && first_at.is_none() {
                first_at = Some(start.elapsed().as_micros());
            }
            assert!(Instant::now() < deadline);
            if search.state == ProjectSearchState::Running && !search.ready() {
                wake.wait(ticket, Some(Duration::from_millis(100)));
            }
        }
        assert_eq!(search.state, ProjectSearchState::Complete);
        assert_eq!(search.rows().len(), 64);
        if index >= 5 {
            first.push(first_at.unwrap());
            complete.push(start.elapsed().as_micros());
        }
    }
    drop(search);
    for index in 0..36 {
        let mut search = ProjectSearch::new(wake.clone());
        search
            .start(&app, &root, ProjectSearchOptions::literal("0"))
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !search.ready() {
            let ticket = wake.ticket();
            if !search.ready() {
                wake.wait(ticket, Some(Duration::from_millis(100)));
            }
            assert!(Instant::now() < deadline);
        }
        let start = Instant::now();
        search.cancel();
        drop(search);
        if index >= 5 {
            cancel.push(start.elapsed().as_micros());
        }
    }
    println!("operation,corpus_bytes,files,samples,p50_us,p95_us");
    for (name, mut times) in [
        ("first_batch", first),
        ("complete", complete),
        ("cancel_and_join", cancel),
    ] {
        times.sort_unstable();
        println!(
            "{name},{},4,31,{},{}",
            corpus.len() * 4,
            times[15],
            times[29]
        );
    }
    for i in 0..4 {
        fs::remove_file(root.join(format!("source-{i}.p"))).unwrap();
    }
    fs::remove_dir(root).unwrap();
}
