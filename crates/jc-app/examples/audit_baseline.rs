//! Opt-in architecture audit. Reports observations; it does not certify correctness.
//! Run in release mode; generated documents never touch user files.
use jc_app::Workbench;
use jc_core::Document;
use jc_text::TextEncoding;
use std::hint::black_box;
use std::time::Instant;

fn bench(text: &str) -> Workbench {
    let mut app = Workbench::default();
    let tab = app.new_file("audit-memory-only.txt", TextEncoding::Utf8);
    app.insert_text(text).unwrap();
    app.jump_to_location(tab, 0);
    app
}

fn measure(mut operation: impl FnMut()) -> (u128, u128) {
    for _ in 0..5 {
        operation();
    }
    let mut samples = Vec::with_capacity(31);
    for _ in 0..31 {
        let started = Instant::now();
        operation();
        samples.push(started.elapsed().as_micros());
    }
    samples.sort_unstable();
    (samples[15], samples[29])
}

fn main() {
    let mut app = bench("a a");
    app.set_search_query("a").unwrap();
    while app.search_running() {
        app.poll_document_search();
    }
    app.replace_all("b").unwrap();
    app.undo().unwrap();
    println!(
        "replace_all_one_undo: expected={:?} actual={:?}",
        b"a a",
        app.active_bytes().unwrap()
    );

    let mut app = bench("a\r\nb");
    app.move_right(false).unwrap();
    app.move_right(false).unwrap();
    println!(
        "crlf_right: cursor={} logical_position={:?}",
        app.active_editor().unwrap().cursor,
        app.active_position()
    );
    let tab = app.tabs().active_id().unwrap();
    println!("jump_inside_crlf_accepted={}", app.jump_to_location(tab, 2));

    let mut app = bench("unsaved");
    app.close_active(true).unwrap();
    app.reopen_closed();
    println!(
        "reopen_discard: document_dirty={} tab_dirty={}",
        app.context().document_dirty,
        app.tabs().active().unwrap().dirty
    );

    let mut doc = Document::from_bytes(vec![b'a'; 65536]);
    doc.set_history_limit(0);
    for i in 0..100 {
        doc.replace(0..65536, &vec![if i % 2 == 0 { b'b' } else { b'a' }; 65536])
            .unwrap();
    }
    println!(
        "history_zero: document_bytes={} retained_added_bytes={}",
        doc.len(),
        doc.retained_added_bytes()
    );

    println!("bytes,move_right_p50_us,move_right_p95_us,position_p50_us,position_p95_us");
    for size in [4096, 65536, 1048576, 4194304] {
        let text = "abcdefg\n".repeat(size / 8);
        let mut app = bench(&text);
        let movement = measure(|| {
            app.move_right(false).unwrap();
        });
        let position = measure(|| {
            black_box(app.active_position());
        });
        println!(
            "{size},{},{},{},{}",
            movement.0, movement.1, position.0, position.1
        );
    }
}
