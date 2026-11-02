//! Synthetic model costs. Batched means, not key-to-screen latency or process RSS.
use jc_app::Workbench;
use jc_text::TextEncoding;
use std::{hint::black_box, time::Instant};

fn sample(mut operation: impl FnMut()) -> (u128, u128) {
    for _ in 0..5 {
        operation();
    }
    let mut times = Vec::new();
    for _ in 0..31 {
        let start = Instant::now();
        for _ in 0..50 {
            operation();
        }
        times.push(start.elapsed().as_nanos() / 50);
    }
    times.sort_unstable();
    (times[15], times[29])
}

fn main() {
    println!("bytes,shape,setup_us,right_p50_ns,right_p95_ns,position_p50_ns,position_p95_ns,viewport_p50_ns,viewport_p95_ns,hot_raw_scanned,hot_decoded,projection_bytes,edit_undo_p50_us,edit_undo_p95_us,retained_source_capacity,history_charge");
    for size in [4096, 65536, 1048576, 4194304, 16777216] {
        for long_line in [false, true] {
            let text = if long_line {
                "é😀".repeat(size / 6)
            } else {
                "abcdefg\n".repeat(size / 8)
            };
            let mut app = Workbench::default();
            let tab = app.new_file("f2-memory.txt", TextEncoding::Utf8);
            let started = Instant::now();
            app.insert_text(&text).unwrap();
            let setup = started.elapsed().as_micros();
            app.jump_to_location(tab, 0);
            app.visible_line(tab, 0, 0, 80);
            let before = app.text_statistics(tab).unwrap();
            let movement = sample(|| app.move_right(false).unwrap());
            let position = sample(|| {
                black_box(app.active_position());
            });
            let viewport = sample(|| {
                for line in 0..24 {
                    black_box(app.visible_line(tab, line, 0, 80));
                }
            });
            let after = app.text_statistics(tab).unwrap();
            let mut edits = Vec::new();
            for sample in 0..12 {
                let start = Instant::now();
                app.insert_text("x").unwrap();
                app.undo().unwrap();
                if sample > 0 {
                    edits.push(start.elapsed().as_micros());
                }
            }
            edits.sort_unstable();
            let doc = app.active_document().unwrap().session.document();
            println!(
                "{},{},{setup},{},{},{},{},{},{},{},{},{},{},{},{},{}",
                text.len(),
                if long_line { "long" } else { "lines" },
                movement.0,
                movement.1,
                position.0,
                position.1,
                viewport.0,
                viewport.1,
                after.0 - before.0,
                after.1 - before.1,
                after.2,
                edits[5],
                edits[10],
                doc.retained_source_bytes(),
                doc.history_bytes()
            );
        }
    }
}
