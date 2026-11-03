//! Synthetic range/layout costs, not physical display latency or RAM acceptance.
mod support;
use jc_app::{PaneId, Workbench};
use jc_text::{TextEncoding, TextPosition};
use std::{hint::black_box, time::Instant};
#[global_allocator]
static ALLOCATOR: support::Meter = support::Meter;
fn main() {
    println!("shape,source_bytes,scalar_reference_ns,scalar_reference_allocs,cold_ns,cold_alloc_calls,cold_alloc_bytes,cold_peak_heap,cold_live_heap,private_bytes,working_bytes,peak_commit,warm_p50_ns,warm_p95_ns,warm_source_read,layout_cache_bytes,edit_at_start_row_ns");
    for (shape, text) in [
        ("ascii-long", "a".repeat(2 * 1024 * 1024)),
        ("unicode-long", "Ae\u{301}界😀👨‍👩‍👧‍👦🇲🇽\t".repeat(32 * 1024)),
        ("ascii-lines", "abcdefg\r\n".repeat(100000)),
    ] {
        let mut app = Workbench::default();
        let tab = app.new_file("unicode-bench.txt", TextEncoding::Utf8);
        app.insert_text(&text).unwrap();
        app.go_to_position(TextPosition { line: 0, column: 0 })
            .unwrap();
        let reference = support::Sample::start();
        black_box(app.visible_line(tab, 0, 0, 80).unwrap());
        let reference = reference.finish();
        let sample = support::Sample::start();
        black_box(app.visual_row(tab, 0, 0, 80).unwrap());
        let cold = sample.finish();
        let before = app.unicode_statistics(tab).unwrap();
        let mut times = Vec::new();
        for _ in 0..101 {
            let start = Instant::now();
            black_box(app.visual_row(tab, 0, 0, 80).unwrap());
            times.push(start.elapsed().as_nanos());
        }
        times.sort_unstable();
        let after = app.unicode_statistics(tab).unwrap();
        let start = Instant::now();
        app.insert_text("x").unwrap();
        black_box(app.visual_row(tab, 0, 0, 80));
        let edit = start.elapsed().as_nanos();
        assert!(app.visual_column_for_pane(PaneId::Primary).is_some());
        println!(
            "{shape},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{edit}",
            text.len(),
            reference[0],
            reference[1],
            cold[0],
            cold[1],
            cold[2],
            cold[3],
            cold[4],
            cold[5],
            cold[6],
            cold[7],
            times[50],
            times[95],
            after.0 - before.0,
            after.1
        );
    }
}
