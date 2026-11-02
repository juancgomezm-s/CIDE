//! Document scheduling and cooperative slots, not physical input-to-display latency.
use jc_app::{SearchStatus, Workbench};
use jc_text::TextEncoding;
use std::time::Instant;
fn main() {
    println!("bytes,samples,schedule_p50_us,schedule_p95_us,slot_p95_us,complete_p50_us,complete_p95_us,cancel_p95_us");
    for size in [4096, 1024 * 1024, 4 * 1024 * 1024, 16 * 1024 * 1024] {
        let mut app = Workbench::default();
        app.new_file("document-search-benchmark.txt", TextEncoding::Utf8);
        app.insert_text(&"abcdefg\n".repeat(size / 8)).unwrap();
        let mut schedule = Vec::new();
        let mut slots = Vec::new();
        let mut complete = Vec::new();
        let mut cancel = Vec::new();
        for i in 0..36 {
            let start = Instant::now();
            app.set_search_query(format!("missing-{i}")).unwrap();
            if i >= 5 {
                schedule.push(start.elapsed().as_micros());
            }
            while app.search_running() {
                let slot = Instant::now();
                app.poll_document_search();
                if i >= 5 {
                    slots.push(slot.elapsed().as_micros());
                }
            }
            assert_eq!(app.search().status, SearchStatus::Complete);
            if i >= 5 {
                complete.push(start.elapsed().as_micros());
            }
            app.set_search_query("cancel-query").unwrap();
            app.poll_document_search();
            let start = Instant::now();
            app.cancel_search();
            if i >= 5 {
                cancel.push(start.elapsed().as_micros());
            }
        }
        for values in [&mut schedule, &mut slots, &mut complete, &mut cancel] {
            values.sort_unstable();
        }
        println!(
            "{size},31,{},{},{},{},{},{}",
            schedule[15],
            schedule[29],
            slots[(slots.len() - 1) * 95 / 100],
            complete[15],
            complete[29],
            cancel[29]
        );
    }
}
