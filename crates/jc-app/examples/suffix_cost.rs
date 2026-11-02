//! Opt-in suffix/projection workload. Uses the real Workbench path; no process
//! RAM instrumentation, terminal rendering, document clone or per-edit flat copy.
use jc_app::{TabId, TextReplacement, Workbench};
use jc_text::TextEncoding;
use std::{
    fs,
    hint::black_box,
    io::Write,
    path::{Path, PathBuf},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

const WARMUPS: usize = 5;
const SAMPLES: usize = 31;
const LINE: &[u8; 8] = b"abcdefg\n";

struct Fixture {
    root: PathBuf,
    files: Vec<PathBuf>,
}
impl Fixture {
    fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("jc-suffix-{}-{stamp}", std::process::id()));
        fs::create_dir(&root).unwrap();
        Self {
            root,
            files: Vec::new(),
        }
    }
    fn source(&mut self, bytes: usize) -> (PathBuf, u64) {
        let path = self.root.join(format!("dense-{bytes}.txt"));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        self.files.push(path.clone());
        let block = LINE.repeat(8192); // bounded 64 KiB fixture generation
        let mut remaining = bytes;
        let mut checksum = 0xcbf29ce484222325u64;
        while remaining > 0 {
            let count = remaining.min(block.len());
            file.write_all(&block[..count]).unwrap();
            checksum = block[..count].iter().fold(checksum, |hash, byte| {
                (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
            });
            remaining -= count;
        }
        (path, checksum)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for path in &self.files {
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
    // This API only observes counters/capacity. It does not ensure the projection.
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
    change_edits: usize,
    mapped_old_eof: Option<usize>,
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
    let (change_edits, mapped_old_eof) = if before.revision != after.revision {
        let document = app.document_by_tab(tab).unwrap().session.document();
        let change = document
            .last_change()
            .expect("transaction/undo must publish a position map");
        assert_eq!(change.before_revision, before.revision);
        assert_eq!(change.after_revision, after.revision);
        let mapped = change.map_offset(before.bytes);
        assert_eq!(mapped, after.bytes);
        (change.edits.len(), Some(mapped))
    } else {
        (0, None)
    };
    (
        Observation {
            elapsed_ns,
            before,
            after,
            change_edits,
            mapped_old_eof,
        },
        value,
    )
}

#[derive(Clone, Copy)]
struct Edit {
    name: &'static str,
    start: usize,
    end: usize,
    text: &'static str,
    lines_delta: isize,
}
struct Row {
    operation: &'static str,
    phase: &'static str,
    sample: usize,
    stage: &'static str,
    edit: Edit,
    expected_lines: usize,
    observation: Observation,
}

fn probe(app: &Workbench, tab: TabId, offset: usize) -> usize {
    let lines = app.line_count_for_tab(tab).unwrap();
    let position = app
        .position_for_tab(tab, offset)
        .expect("probe must be a scalar/CRLF boundary");
    black_box(app.visible_line(tab, position.line, 0, 80).unwrap());
    black_box(position);
    lines
}

fn cycle(
    app: &mut Workbench,
    tab: TabId,
    anchor: usize,
    baseline_lines: usize,
    edit: Edit,
) -> [Observation; 4] {
    let before = state(app, tab);
    let id = app.document_by_tab(tab).unwrap().id;
    // Proposal allocation is outside the timer; encoding/validation, source edit,
    // maps/view rebasing and normalization are part of the measured real route.
    let proposal = vec![TextReplacement {
        range: edit.start..edit.end,
        text: edit.text.into(),
    }];
    let (apply, ()) = measure(app, tab, |app| {
        app.apply_text_edits(id, before.revision, proposal).unwrap()
    });
    assert_eq!(apply.change_edits, 1);
    let mapped_anchor = app
        .document_by_tab(tab)
        .unwrap()
        .session
        .document()
        .last_change()
        .unwrap()
        .map_offset(anchor);
    let (read, lines) = measure(app, tab, |app| probe(app, tab, mapped_anchor));
    assert_eq!(
        lines,
        baseline_lines.saturating_add_signed(edit.lines_delta)
    );
    let (undo, ()) = measure(app, tab, |app| app.undo().unwrap());
    assert_eq!(undo.change_edits, 1);
    assert_eq!(undo.after.revision, before.revision);
    assert_eq!(undo.after.bytes, before.bytes);
    let (undo_read, lines) = measure(app, tab, |app| probe(app, tab, anchor));
    assert_eq!(lines, baseline_lines);
    [apply, read, undo, undo_read]
}

fn edits(bytes: usize, anchor: usize) -> [Edit; 4] {
    let scalar = anchor.min(bytes - 2); // last non-newline scalar for deletion at end
    let line = anchor.min(bytes - LINE.len());
    [
        Edit {
            name: "insert_scalar",
            start: anchor,
            end: anchor,
            text: "x",
            lines_delta: 0,
        },
        Edit {
            name: "insert_line",
            start: anchor,
            end: anchor,
            text: "x\n",
            lines_delta: 1,
        },
        Edit {
            name: "delete_scalar",
            start: scalar,
            end: scalar + 1,
            text: "",
            lines_delta: 0,
        },
        Edit {
            name: "delete_line",
            start: line,
            end: line + LINE.len(),
            text: "",
            lines_delta: -1,
        },
    ]
}

fn checksum(app: &Workbench, tab: TabId) -> u64 {
    app.document_by_tab(tab)
        .unwrap()
        .session
        .document()
        .chunks()
        .flatten()
        .fold(0xcbf29ce484222325u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        })
}

fn run(path: &Path, baseline_hash: u64, bytes: usize, location: &'static str, anchor: usize) {
    let mut app = Workbench::default();
    let open = Instant::now();
    let tab = app.open_file(path, Some(TextEncoding::Utf8)).unwrap();
    let open_ns = open.elapsed().as_nanos();
    let lines = bytes / LINE.len() + 1; // editor has a logical empty line after final LF
    let none = Edit {
        name: "read",
        start: anchor,
        end: anchor,
        text: "",
        lines_delta: 0,
    };
    let mut rows = Vec::with_capacity(600);
    let opened = state(&app, tab);
    rows.push(Row {
        operation: "open",
        phase: "cold_model",
        sample: 0,
        stage: "open",
        edit: none,
        expected_lines: lines,
        observation: Observation {
            elapsed_ns: open_ns,
            before: State::default(),
            after: opened,
            change_edits: 0,
            mapped_old_eof: None,
        },
    });
    let (cold_index, count) = measure(&mut app, tab, |app| app.line_count_for_tab(tab).unwrap());
    assert_eq!(count, lines);
    rows.push(Row {
        operation: "cold_index",
        phase: "cold_model",
        sample: 0,
        stage: "read",
        edit: none,
        expected_lines: lines,
        observation: cold_index,
    });
    let (cold_target, ()) = measure(&mut app, tab, |app| {
        black_box(app.position_for_tab(tab, anchor).unwrap());
    });
    rows.push(Row {
        operation: "cold_target_layout",
        phase: "cold_model",
        sample: 0,
        stage: "read",
        edit: none,
        expected_lines: lines,
        observation: cold_target,
    });
    assert!(app.jump_to_location(tab, anchor));
    for sample in 0..WARMUPS + SAMPLES {
        let (observation, count) = measure(&mut app, tab, |app| {
            if anchor == bytes {
                app.move_left(false).unwrap();
                app.move_right(false).unwrap();
            } else {
                app.move_right(false).unwrap();
                app.move_left(false).unwrap();
            }
            probe(app, tab, anchor)
        });
        assert_eq!(count, lines);
        assert_eq!(app.active_editor().unwrap().cursor, anchor);
        if sample >= WARMUPS {
            rows.push(Row {
                operation: "warm_move_probe",
                phase: "warm",
                sample: sample - WARMUPS,
                stage: "read",
                edit: none,
                expected_lines: lines,
                observation,
            });
        }
    }
    for edit in edits(bytes, anchor) {
        for iteration in 0..1 + WARMUPS + SAMPLES {
            let metrics = cycle(&mut app, tab, anchor, lines, edit);
            if iteration > 0 && iteration <= WARMUPS {
                continue;
            }
            let (phase, sample) = if iteration == 0 {
                ("first_cycle", 0)
            } else {
                ("warm", iteration - WARMUPS - 1)
            };
            for (stage, observation) in
                ["apply", "projection_probe", "undo", "undo_projection_probe"]
                    .into_iter()
                    .zip(metrics)
            {
                let expected_lines = if stage == "apply" || stage == "projection_probe" {
                    lines.saturating_add_signed(edit.lines_delta)
                } else {
                    lines
                };
                rows.push(Row {
                    operation: edit.name,
                    phase,
                    sample,
                    stage,
                    edit,
                    expected_lines,
                    observation,
                });
            }
        }
    }
    assert_eq!(checksum(&app, tab), baseline_hash); // one borrowed verification after the entire case
    emit(bytes, location, &rows);
}

fn emit(bytes: usize, location: &str, rows: &[Row]) {
    for row in rows {
        let m = &row.observation;
        let suffix_lines = if row.edit.name == "read" {
            0
        } else {
            (bytes - row.edit.end.min(bytes)) / LINE.len()
        };
        let mapped = m
            .mapped_old_eof
            .map_or_else(String::new, |value| value.to_string());
        println!("{bytes},{location},{},{},{},{},{},{},{},{},{suffix_lines},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            row.operation,row.phase,row.sample,row.stage,row.edit.start,row.edit.end,row.edit.text.len(),row.expected_lines,
            m.elapsed_ns,m.after.scanned-m.before.scanned,m.after.decoded-m.before.decoded,
            m.before.projection,m.after.projection,m.before.source,m.after.source,m.before.history,m.after.history,
            m.before.pieces,m.after.pieces,m.before.undo,m.after.undo,m.before.redo,m.after.redo,
            m.before.revision,m.after.revision,m.change_edits,mapped,m.after.bytes);
    }
    for operation in [
        "warm_move_probe",
        "insert_scalar",
        "insert_line",
        "delete_scalar",
        "delete_line",
    ] {
        for stage in [
            "read",
            "apply",
            "projection_probe",
            "undo",
            "undo_projection_probe",
        ] {
            let mut times: Vec<_> = rows
                .iter()
                .filter(|row| {
                    row.phase == "warm" && row.operation == operation && row.stage == stage
                })
                .map(|row| row.observation.elapsed_ns)
                .collect();
            if times.is_empty() {
                continue;
            }
            assert_eq!(times.len(), SAMPLES);
            times.sort_unstable();
            eprintln!(
                "{bytes},{location},{operation},{stage},{SAMPLES},{},{}",
                times[15], times[29]
            );
        }
    }
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    assert!(
        args.len() <= 2,
        "usage: suffix_cost [1|4|8] [start|middle|end]"
    );
    let size = args
        .first()
        .map(|value| value.parse::<usize>().expect("size in MiB"));
    assert!(size.is_none_or(|size| [1, 4, 8].contains(&size)));
    let place = args.get(1).map(String::as_str);
    assert!(place.is_none_or(|place| ["start", "middle", "end"].contains(&place)));
    println!("corpus_bytes,location,operation,phase,sample,stage,edit_start,edit_end,insert_bytes,expected_lines,estimated_suffix_lines,elapsed_ns,scanned_bytes,decoded_bytes,projection_bytes_before,projection_bytes_after,source_capacity_before,source_capacity_after,history_charge_before,history_charge_after,pieces_before,pieces_after,undo_steps_before,undo_steps_after,redo_steps_before,redo_steps_after,revision_before,revision_after,change_edits,mapped_old_eof,result_bytes");
    eprintln!("corpus_bytes,location,operation,stage,samples,p50_ns,p95_ns");
    let mut fixture = Fixture::new();
    for mib in [1, 4, 8] {
        if size.is_some_and(|size| size != mib) {
            continue;
        }
        let bytes = mib * 1024 * 1024;
        let (path, baseline_hash) = fixture.source(bytes);
        for (location, anchor) in [("start", 0), ("middle", bytes / 2), ("end", bytes)] {
            if place.is_some_and(|place| place != location) {
                continue;
            }
            run(&path, baseline_hash, bytes, location, anchor);
        }
    }
}
