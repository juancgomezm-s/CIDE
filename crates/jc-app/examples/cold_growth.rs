//! Distribution of the first newline edit in independently opened Workbenches.
//! Model/cache-cold phases, not filesystem-cache cold, process RAM or UI latency.
use jc_app::{TabId, TextReplacement, Workbench};
use jc_text::TextEncoding;
use std::{
    fs,
    hint::black_box,
    io::Write,
    path::{Path, PathBuf},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

const SAMPLES: usize = 31;
const LINE: &[u8; 8] = b"abcdefg\n";
const HASH_INITIAL: u64 = 0xcbf29ce484222325;
fn hash_byte(hash: u64, byte: u8) -> u64 {
    (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
}

struct Fixture {
    root: PathBuf,
    paths: Vec<PathBuf>,
}
impl Fixture {
    fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("jc-cold-growth-{}-{stamp}", std::process::id()));
        fs::create_dir(&root).unwrap();
        Self {
            root,
            paths: Vec::new(),
        }
    }
    fn source(&mut self, bytes: usize) -> (PathBuf, u64) {
        let path = self.root.join(format!("dense-{bytes}.txt"));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        self.paths.push(path.clone());
        let block = LINE.repeat(8192);
        let mut remaining = bytes;
        let mut hash = HASH_INITIAL;
        while remaining > 0 {
            let count = remaining.min(block.len());
            file.write_all(&block[..count]).unwrap();
            hash = block[..count]
                .iter()
                .fold(hash, |hash, byte| hash_byte(hash, *byte));
            remaining -= count;
        }
        (path, hash)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for path in &self.paths {
            fs::remove_file(path).unwrap();
        }
        fs::remove_dir(&self.root).unwrap();
    }
}

#[derive(Clone, Copy, Default)]
struct State {
    bytes: usize,
    revision: u64,
    scanned: u64,
    decoded: u64,
    projection: usize,
    source: usize,
    history: usize,
    pieces: usize,
    undo: usize,
    redo: usize,
}
fn state(app: &Workbench, tab: TabId) -> State {
    let source = app.document_by_tab(tab).unwrap().session.document();
    let (scanned, decoded, projection) = app.text_statistics(tab).unwrap();
    State {
        bytes: source.len(),
        revision: source.revision(),
        scanned,
        decoded,
        projection,
        source: source.retained_source_bytes(),
        history: source.history_bytes(),
        pieces: source.piece_count(),
        undo: source.undo_steps(),
        redo: source.redo_steps(),
    }
}
struct Observation {
    elapsed_ns: u128,
    before: State,
    after: State,
}
fn measure<T>(
    app: &mut Workbench,
    tab: TabId,
    action: impl FnOnce(&mut Workbench) -> T,
) -> (Observation, T) {
    let before = state(app, tab);
    let start = Instant::now();
    let value = action(app);
    let elapsed_ns = start.elapsed().as_nanos();
    let after = state(app, tab);
    (
        Observation {
            elapsed_ns,
            before,
            after,
        },
        value,
    )
}
struct Row {
    bytes: usize,
    location: &'static str,
    sample: usize,
    stage: &'static str,
    anchor: usize,
    expected_lines: usize,
    change_edits: usize,
    mapped_old_eof: Option<usize>,
    observation: Observation,
}

fn borrowed_equals(app: &Workbench, tab: TabId, range: std::ops::Range<usize>, expected: &[u8]) {
    let document = app.document_by_tab(tab).unwrap().session.document();
    assert!(document
        .range_chunks(range)
        .unwrap()
        .flatten()
        .copied()
        .eq(expected.iter().copied()));
}
fn verify_map(
    app: &Workbench,
    tab: TabId,
    observation: &Observation,
    old_anchor: usize,
    new_anchor: usize,
) -> (usize, Option<usize>) {
    let document = app.document_by_tab(tab).unwrap().session.document();
    let change = document
        .last_change()
        .expect("first edit/undo must publish a position map");
    assert_eq!(change.before_revision, observation.before.revision);
    assert_eq!(change.after_revision, observation.after.revision);
    assert_eq!(change.edits.len(), 1);
    assert_eq!(change.map_offset(old_anchor), new_anchor);
    let mapped = change.map_offset(observation.before.bytes);
    assert_eq!(mapped, observation.after.bytes);
    (change.edits.len(), Some(mapped))
}
fn verify_lines(app: &Workbench, tab: TabId, expected: usize) {
    let before = state(app, tab);
    assert_eq!(app.line_count_for_tab(tab), Some(expected));
    let after = state(app, tab);
    // Do not silently hide deferred projection work in untimed verification.
    assert_eq!(before.scanned, after.scanned);
    assert_eq!(before.decoded, after.decoded);
    assert_eq!(before.projection, after.projection);
}
fn verify_original(app: &Workbench, tab: TabId, bytes: usize, expected_hash: u64) {
    let document = app.document_by_tab(tab).unwrap().session.document();
    let (hash, count, lf, cr) = document.chunks().flatten().fold(
        (HASH_INITIAL, 0usize, 0usize, 0usize),
        |(hash, count, lf, cr), byte| {
            (
                hash_byte(hash, *byte),
                count + 1,
                lf + usize::from(*byte == b'\n'),
                cr + usize::from(*byte == b'\r'),
            )
        },
    );
    assert_eq!(
        (hash, count, lf, cr),
        (expected_hash, bytes, bytes / LINE.len(), 0)
    );
    assert!(!document.is_modified());
    borrowed_equals(app, tab, 0..LINE.len(), LINE);
    borrowed_equals(app, tab, bytes - LINE.len()..bytes, LINE);
}

fn trial(
    path: &Path,
    bytes: usize,
    location: &'static str,
    anchor: usize,
    sample: usize,
    expected_hash: u64,
) -> Vec<Row> {
    let mut app = Workbench::default();
    let start = Instant::now();
    let tab = app.open_file(path, Some(TextEncoding::Utf8)).unwrap();
    let elapsed_ns = start.elapsed().as_nanos();
    let opened = state(&app, tab);
    let baseline_lines = bytes / LINE.len() + 1;
    let mut rows = Vec::with_capacity(5);
    rows.push(Row {
        bytes,
        location,
        sample,
        stage: "open",
        anchor,
        expected_lines: baseline_lines,
        change_edits: 0,
        mapped_old_eof: None,
        observation: Observation {
            elapsed_ns,
            before: State::default(),
            after: opened,
        },
    });
    let (index, count) = measure(&mut app, tab, |app| app.line_count_for_tab(tab).unwrap());
    assert_eq!(count, baseline_lines);
    rows.push(Row {
        bytes,
        location,
        sample,
        stage: "index",
        anchor,
        expected_lines: baseline_lines,
        change_edits: 0,
        mapped_old_eof: None,
        observation: index,
    });
    let (layout, ()) = measure(&mut app, tab, |app| {
        black_box(app.position_for_tab(tab, anchor).unwrap());
        assert!(app.jump_to_location(tab, anchor));
    });
    rows.push(Row {
        bytes,
        location,
        sample,
        stage: "layout_and_caret",
        anchor,
        expected_lines: baseline_lines,
        change_edits: 0,
        mapped_old_eof: None,
        observation: layout,
    });

    let before = state(&app, tab);
    let id = app.document_by_tab(tab).unwrap().id;
    let proposal = vec![TextReplacement {
        range: anchor..anchor,
        text: "x\n".into(),
    }];
    let (apply, ()) = measure(&mut app, tab, |app| {
        app.apply_text_edits(id, before.revision, proposal).unwrap()
    });
    assert_eq!(apply.after.bytes, bytes + 2);
    let (change_edits, mapped_old_eof) = verify_map(&app, tab, &apply, anchor, anchor + 2);
    verify_lines(&app, tab, baseline_lines + 1);
    if anchor == 0 {
        borrowed_equals(&app, tab, 0..10, b"x\nabcdefg\n");
    } else {
        borrowed_equals(&app, tab, bytes - LINE.len()..bytes + 2, b"abcdefg\nx\n");
    }
    rows.push(Row {
        bytes,
        location,
        sample,
        stage: "first_apply",
        anchor,
        expected_lines: baseline_lines + 1,
        change_edits,
        mapped_old_eof,
        observation: apply,
    });
    let (undo, ()) = measure(&mut app, tab, |app| app.undo().unwrap());
    assert_eq!(undo.after.bytes, bytes);
    assert_eq!(undo.after.revision, before.revision);
    let (change_edits, mapped_old_eof) = verify_map(&app, tab, &undo, anchor + 2, anchor);
    verify_lines(&app, tab, baseline_lines);
    verify_original(&app, tab, bytes, expected_hash);
    rows.push(Row {
        bytes,
        location,
        sample,
        stage: "undo",
        anchor,
        expected_lines: baseline_lines,
        change_edits,
        mapped_old_eof,
        observation: undo,
    });
    rows
}

fn emit(rows: &[Row]) {
    println!("corpus_bytes,location,sample,stage,anchor,expected_lines,elapsed_ns,scanned_bytes,decoded_bytes,projection_bytes_before,projection_bytes_after,source_capacity_before,source_capacity_after,history_charge_before,history_charge_after,pieces_before,pieces_after,undo_steps_before,undo_steps_after,redo_steps_before,redo_steps_after,revision_before,revision_after,bytes_before,bytes_after,change_edits,mapped_old_eof");
    for row in rows {
        let m = &row.observation;
        let mapped = row
            .mapped_old_eof
            .map_or_else(String::new, |value| value.to_string());
        println!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            row.bytes,
            row.location,
            row.sample,
            row.stage,
            row.anchor,
            row.expected_lines,
            m.elapsed_ns,
            m.after.scanned - m.before.scanned,
            m.after.decoded - m.before.decoded,
            m.before.projection,
            m.after.projection,
            m.before.source,
            m.after.source,
            m.before.history,
            m.after.history,
            m.before.pieces,
            m.after.pieces,
            m.before.undo,
            m.after.undo,
            m.before.redo,
            m.after.redo,
            m.before.revision,
            m.after.revision,
            m.before.bytes,
            m.after.bytes,
            row.change_edits,
            mapped
        );
    }
    eprintln!("corpus_bytes,location,stage,samples,p50_ns,p95_ns");
    for bytes in [1024 * 1024, 4 * 1024 * 1024, 8 * 1024 * 1024] {
        for location in ["start", "end"] {
            for stage in ["open", "index", "layout_and_caret", "first_apply", "undo"] {
                let mut values: Vec<_> = rows
                    .iter()
                    .filter(|row| {
                        row.bytes == bytes && row.location == location && row.stage == stage
                    })
                    .map(|row| row.observation.elapsed_ns)
                    .collect();
                if values.is_empty() {
                    continue;
                }
                assert_eq!(values.len(), SAMPLES);
                values.sort_unstable();
                eprintln!(
                    "{bytes},{location},{stage},{SAMPLES},{},{}",
                    values[15], values[29]
                );
            }
        }
    }
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    assert!(args.len() <= 2, "usage: cold_growth [1|4|8] [start|end]");
    let size = args
        .first()
        .map(|value| value.parse::<usize>().expect("size in MiB"));
    assert!(size.is_none_or(|size| [1, 4, 8].contains(&size)));
    let place = args.get(1).map(String::as_str);
    assert!(place.is_none_or(|place| ["start", "end"].contains(&place)));
    let mut fixture = Fixture::new();
    let mut rows = Vec::with_capacity(6 * SAMPLES * 5);
    for mib in [1, 4, 8] {
        if size.is_some_and(|size| size != mib) {
            continue;
        }
        let bytes = mib * 1024 * 1024;
        let (path, expected_hash) = fixture.source(bytes);
        for (location, anchor) in [("start", 0), ("end", bytes)] {
            if place.is_some_and(|place| place != location) {
                continue;
            }
            drop(trial(&path, bytes, location, anchor, 0, expected_hash)); // one discarded warmup session
            for sample in 0..SAMPLES {
                rows.extend(trial(&path, bytes, location, anchor, sample, expected_hash));
            }
        }
    }
    emit(&rows);
}
