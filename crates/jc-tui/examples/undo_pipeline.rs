//! In-process event/frame/VT wall times. Excludes OS input, ConPTY and display.
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use jc_text::TextEncoding;
use jc_tui::{CellStyle, DrawStats, FrameEncoder, LaunchOptions, Screen, TuiApp};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Instant,
};

const DOCUMENTS: usize = 8;
const DOCUMENT_BYTES: usize = 4 * 1024 * 1024;
const WARMUPS: usize = 5;
const SAMPLES: usize = 31;
const WIDTH: u16 = 140;
const HEIGHT: u16 = 40;
const VERIFY_WIDTH: u16 = 300;
const UNIT: &[u8; 8] = b"abcdefg\n";

struct Fixture {
    directory: PathBuf,
    files: Vec<PathBuf>,
    relative: Vec<PathBuf>,
}
impl Fixture {
    fn new() -> Self {
        // Run from a project root so relative status paths fit the measured VT.
        // This also permits the same source/binary to probe an archived build.
        let project = std::env::current_dir().unwrap();
        assert!(project.join("Cargo.toml").is_file());
        assert!(project.join("crates/jc-tui/src/lib.rs").is_file());
        let relative_root =
            PathBuf::from("work/test-temp").join(format!("undo-pipeline-{}", std::process::id()));
        let directory = project.join(&relative_root);
        fs::create_dir_all(directory.parent().unwrap()).unwrap();
        fs::create_dir(&directory).expect("refuse an existing probe fixture");
        let mut fixture = Self {
            directory,
            files: Vec::new(),
            relative: Vec::new(),
        };
        let block = block();
        for document in 0..DOCUMENTS {
            let relative = relative_root.join(format!("document-{document}.txt"));
            let path = project.join(&relative);
            fixture.files.push(path.clone());
            fixture.relative.push(relative);
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .unwrap();
            let first = prefix(document);
            file.write_all(&first).unwrap();
            let mut remaining = DOCUMENT_BYTES - first.len();
            while remaining > 0 {
                let count = remaining.min(block.len());
                file.write_all(&block[..count]).unwrap();
                remaining -= count;
            }
        }
        fixture.verify();
        fixture
    }
    fn verify_document(&self, document: usize) {
        verify_file(&self.files[document], document);
    }
    fn verify(&self) {
        for document in 0..DOCUMENTS {
            self.verify_document(document);
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // Explicitly owned files only; never recursively remove a directory.
        for path in &self.files {
            let _ = fs::remove_file(path);
        }
        let _ = fs::remove_dir(&self.directory);
    }
}
fn prefix(document: usize) -> Vec<u8> {
    format!("M2_VISIBLE_{document} {}\n", "x".repeat(128)).into_bytes()
}
fn block() -> Vec<u8> {
    let mut block = vec![0; 64 * 1024];
    for unit in block.chunks_exact_mut(8) {
        unit.copy_from_slice(UNIT);
    }
    block
}
fn verify_file(path: &Path, document: usize) {
    let mut file = File::open(path).unwrap();
    let expected_prefix = prefix(document);
    let mut first = vec![0; expected_prefix.len()];
    file.read_exact(&mut first).unwrap();
    assert_eq!(first, expected_prefix);
    let expected = block();
    let mut actual = vec![0; expected.len()];
    let mut remaining = DOCUMENT_BYTES - expected_prefix.len();
    while remaining > 0 {
        let count = remaining.min(actual.len());
        file.read_exact(&mut actual[..count]).unwrap();
        assert_eq!(
            &actual[..count],
            &expected[..count],
            "disk bytes: document {document}"
        );
        remaining -= count;
    }
    assert_eq!(
        file.read(&mut actual[..1]).unwrap(),
        0,
        "unexpected file suffix"
    );
}

fn event(code: KeyCode, modifiers: KeyModifiers) -> Event {
    Event::Key(KeyEvent::new(code, modifiers))
}
fn key(app: &mut TuiApp, code: KeyCode, modifiers: KeyModifiers) {
    app.handle_event(event(code, modifiers));
}
fn row_text(screen: &Screen, row: u16) -> String {
    (0..screen.width())
        .map(|column| screen.cell(column, row).ch)
        .collect()
}
fn text(screen: &Screen) -> String {
    (0..screen.height())
        .map(|row| row_text(screen, row))
        .collect::<Vec<_>>()
        .join("\n")
}
fn assert_scene(screen: &Screen, document: usize, line: usize) {
    assert_eq!(screen.width(), WIDTH);
    assert_eq!(screen.height(), HEIGHT);
    let text = text(screen);
    assert!(
        text.contains(&format!("> Group 1: document-{document}.txt")),
        "{text}"
    );
    assert!(text.contains("Group 2: document-7.txt"), "{text}");
    let status = row_text(screen, HEIGHT - 1);
    assert!(status.contains(&format!("Ln {line} Col 1 ")), "{status}");
    assert!(!text.contains("Go to line[:column]"));
    assert!(!text.contains("Unsaved changes"));
}
fn baseline(app: &mut TuiApp, encoder: &mut FrameEncoder, sink: &mut Vec<u8>) -> Screen {
    let screen = app.frame(WIDTH, HEIGHT);
    sink.clear();
    encoder.draw_to(sink, &screen).unwrap();
    screen
}
fn verify_dirty_and_restore(
    app: &mut TuiApp,
    encoder: &mut FrameEncoder,
    sink: &mut Vec<u8>,
    document: usize,
    dirty: bool,
) {
    // All eight titles fit only in this verification composition. It is never
    // timed/encoded as a sample; restore the measured geometry and baseline.
    let wide = app.frame(VERIFY_WIDTH, HEIGHT);
    let tabs = row_text(&wide, 1);
    let title = format!(
        "{}:document-{document}.txt{} ",
        document + 1,
        if dirty { "*" } else { "" }
    );
    let start = tabs
        .find(&title)
        .unwrap_or_else(|| panic!("missing tab/dirty state: {tabs}"));
    assert_eq!(wide.cell(start as u16, 1).style, CellStyle::Reverse);
    assert_eq!(tabs.matches('*').count(), usize::from(dirty));
    let restored = baseline(app, encoder, sink);
    assert_scene(&restored, document, if dirty { 2 } else { 1 });
}
fn go_to_start(app: &mut TuiApp, encoder: &mut FrameEncoder, sink: &mut Vec<u8>, document: usize) {
    let digit = char::from_digit((document + 1) as u32, 10).unwrap();
    key(app, KeyCode::Char(digit), KeyModifiers::ALT);
    let selected = baseline(app, encoder, sink);
    assert!(text(&selected).contains(&format!("Group 1: document-{document}.txt")));
    key(app, KeyCode::Char('g'), KeyModifiers::CONTROL);
    let prompt = baseline(app, encoder, sink);
    assert!(text(&prompt).contains("Go to line[:column]"));
    app.handle_event(Event::Paste("1:1".into()));
    key(app, KeyCode::Enter, KeyModifiers::NONE);
    let screen = baseline(app, encoder, sink);
    assert_scene(&screen, document, 1);
    verify_dirty_and_restore(app, encoder, sink, document, false);
}

struct Sample {
    document: usize,
    operation: &'static str,
    sample: usize,
    handle_event_ns: u128,
    frame_ns: u128,
    encode_ns: u128,
    total_ns: u128,
    draw: DrawStats,
}
fn measure(
    app: &mut TuiApp,
    encoder: &mut FrameEncoder,
    sink: &mut Vec<u8>,
    input: Event,
    document: usize,
    operation: &'static str,
    sample: usize,
) -> (Sample, Screen) {
    sink.clear();
    let total = Instant::now();
    let started = Instant::now();
    app.handle_event(input);
    let handle_event_ns = started.elapsed().as_nanos();
    let started = Instant::now();
    let screen = app.frame(WIDTH, HEIGHT);
    let frame_ns = started.elapsed().as_nanos();
    let started = Instant::now();
    let draw = encoder.draw_to(sink, &screen).unwrap();
    let encode_ns = started.elapsed().as_nanos();
    let total_ns = total.elapsed().as_nanos();
    assert_eq!(draw.bytes, sink.len());
    assert!(
        !draw.full,
        "steady operation must retain the acknowledged frame"
    );
    assert!(total_ns >= handle_event_ns + frame_ns + encode_ns);
    (
        Sample {
            document,
            operation,
            sample,
            handle_event_ns,
            frame_ns,
            encode_ns,
            total_ns,
            draw,
        },
        screen,
    )
}

fn close_cleanly(app: &mut TuiApp, encoder: &mut FrameEncoder, sink: &mut Vec<u8>) {
    key(
        app,
        KeyCode::Char('1'),
        KeyModifiers::ALT | KeyModifiers::SHIFT,
    );
    assert!(!text(&baseline(app, encoder, sink)).contains("Group 2"));
    for _ in 0..DOCUMENTS {
        key(app, KeyCode::Char('w'), KeyModifiers::CONTROL);
        assert!(!text(&baseline(app, encoder, sink)).contains("Unsaved changes"));
    }
    assert!(text(&baseline(app, encoder, sink)).contains("Group 1: Empty"));
    key(app, KeyCode::Char('f'), KeyModifiers::ALT);
    assert!(text(&baseline(app, encoder, sink)).contains("Exit"));
    key(app, KeyCode::Up, KeyModifiers::NONE);
    key(app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(
        app.should_exit(),
        "central File/Exit must accept the clean state"
    );
}

fn main() {
    let fixture = Fixture::new();
    let mut app = TuiApp::new(LaunchOptions {
        files: fixture.relative.clone(),
        encoding: Some(TextEncoding::Utf8),
    });
    let mut encoder = FrameEncoder::default();
    let mut sink = Vec::new();
    assert!(text(&baseline(&mut app, &mut encoder, &mut sink)).contains("M2_VISIBLE_7"));
    key(
        &mut app,
        KeyCode::Char('2'),
        KeyModifiers::ALT | KeyModifiers::SHIFT,
    );
    key(&mut app, KeyCode::Char('1'), KeyModifiers::CONTROL);
    assert!(text(&baseline(&mut app, &mut encoder, &mut sink)).contains("Group 2"));
    // Visit all files before measuring, as in the dense ConPTY scene.
    for document in 0..DOCUMENTS {
        go_to_start(&mut app, &mut encoder, &mut sink, document);
    }
    let mut rows = Vec::with_capacity(DOCUMENTS * SAMPLES * 2);
    for document in 0..DOCUMENTS {
        go_to_start(&mut app, &mut encoder, &mut sink, document);
        for round in 0..WARMUPS + SAMPLES {
            let sample = round.saturating_sub(WARMUPS);
            let (enter, screen) = measure(
                &mut app,
                &mut encoder,
                &mut sink,
                event(KeyCode::Enter, KeyModifiers::NONE),
                document,
                "enter",
                sample,
            );
            assert_scene(&screen, document, 2);
            verify_dirty_and_restore(&mut app, &mut encoder, &mut sink, document, true);
            let (undo, screen) = measure(
                &mut app,
                &mut encoder,
                &mut sink,
                event(KeyCode::Char('z'), KeyModifiers::CONTROL),
                document,
                "undo",
                sample,
            );
            assert_scene(&screen, document, 1);
            verify_dirty_and_restore(&mut app, &mut encoder, &mut sink, document, false);
            if round >= WARMUPS {
                rows.push(enter);
                rows.push(undo);
            }
        }
        fixture.verify_document(document);
    }
    fixture.verify();
    close_cleanly(&mut app, &mut encoder, &mut sink);
    // No CSV I/O inside or between measured pairs; all checks passed first.
    println!("document,operation,sample,handle_event_ns,frame_ns,encode_ns,total_ns,cells,runs,bytes,full");
    for row in rows {
        println!(
            "{},{},{},{},{},{},{},{},{},{},{}",
            row.document,
            row.operation,
            row.sample,
            row.handle_event_ns,
            row.frame_ns,
            row.encode_ns,
            row.total_ns,
            row.draw.cells,
            row.draw.runs,
            row.draw.bytes,
            row.draw.full
        );
    }
    eprintln!("undo pipeline checks passed: 8 documents, 31 enter/undo pairs each; disk intact; clean close/exit");
}
