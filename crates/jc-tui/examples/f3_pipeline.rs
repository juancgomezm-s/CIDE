//! Input dispatch -> preparation -> composition -> VT encoding to an in-memory sink.
//! Excludes OS input delivery, terminal transport/rendering and physical display.
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use jc_text::TextEncoding;
use jc_tui::{FrameEncoder, LaunchOptions, TuiApp};
use std::{fs, time::Instant};

fn main() {
    println!("bytes,columns,rows,pipeline_p50_us,pipeline_p95_us,delta_bytes_p50,unchanged_bytes");
    for size in [4096, 1048576, 4194304] {
        let path =
            std::env::temp_dir().join(format!("jc-f3-pipeline-{}-{size}.txt", std::process::id()));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        std::io::Write::write_all(&mut file, &b"abcdefg\n".repeat(size / 8)).unwrap();
        drop(file);
        for (width, height) in [(80, 24), (140, 40)] {
            let mut app = TuiApp::new(LaunchOptions {
                files: vec![path.clone()],
                encoding: Some(TextEncoding::Utf8),
            });
            let mut encoder = FrameEncoder::default();
            let first = app.frame(width, height);
            encoder.draw_to(&mut Vec::new(), &first).unwrap();
            let mut times = Vec::new();
            let mut bytes = Vec::new();
            for index in 0..36 {
                let started = Instant::now();
                app.handle_event(Event::Key(KeyEvent::new(
                    KeyCode::Right,
                    KeyModifiers::NONE,
                )));
                let screen = app.frame(width, height);
                let mut out = Vec::new();
                let stats = encoder.draw_to(&mut out, &screen).unwrap();
                if index >= 5 {
                    times.push(started.elapsed().as_micros());
                    bytes.push(stats.bytes);
                }
            }
            let screen = app.frame(width, height);
            let unchanged = encoder.draw_to(&mut Vec::new(), &screen).unwrap().bytes;
            times.sort_unstable();
            bytes.sort_unstable();
            println!(
                "{size},{width},{height},{},{},{},{}",
                times[15], times[29], bytes[15], unchanged
            );
        }
        fs::remove_file(path).unwrap();
    }
}
