//! Opt-in model/allocator evidence. Fresh process per case; not physical display latency.
mod support;
#[global_allocator]
static ALLOCATOR: support::Meter = support::Meter;
use jc_app::{PaneId, SearchStatus, TextReplacement, Workbench};
use jc_text::{encode, TextEncoding};
use std::{fs, hint::black_box, path::PathBuf};

fn emit(case: &str, operation: &str, sample: usize, metrics: [u128; 8], app: &Workbench) {
    let doc = app.active_document();
    let (len, pieces, history, retained, projection, scanned, decoded) =
        doc.map_or((0, 0, 0, 0, 0, 0, 0), |d| {
            let source = d.session.document();
            let stats = app.text_statistics(d.tab_id).unwrap_or_default();
            (
                source.len(),
                source.piece_count(),
                source.history_bytes(),
                source.retained_source_bytes(),
                stats.2,
                stats.0,
                stats.1,
            )
        });
    print!("{case},{operation},{sample},{len},{pieces},{history},{retained},{projection},{scanned},{decoded}");
    for metric in metrics {
        print!(",{metric}");
    }
    println!();
}
fn viewport(app: &mut Workbench) {
    for pane in [PaneId::Primary, PaneId::Secondary] {
        let Some(tab) = app
            .panes()
            .view(pane)
            .and_then(|v| v.location())
            .map(|p| p.tab)
        else {
            continue;
        };
        black_box(app.position_for_pane(pane));
        for line in 0..24 {
            black_box(app.visible_line(tab, line, 0, 80));
        }
    }
}
struct Fixture {
    root: PathBuf,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for path in fs::read_dir(&self.root).unwrap() {
            fs::remove_file(path.unwrap().path()).unwrap();
        }
        fs::remove_dir(&self.root).unwrap();
    }
}
fn main() {
    let case = std::env::args()
        .nth(1)
        .expect("case: ascii/unicode/cp1252/latin1/ibm850/long/pieces/multi");
    let encoding = match case.as_str() {
        "cp1252" => TextEncoding::Windows1252,
        "latin1" => TextEncoding::Iso8859_1,
        "ibm850" => TextEncoding::Ibm850,
        _ => TextEncoding::Utf8,
    };
    let unit = match case.as_str() {
        "unicode" => "é😀e\u{301}界 Σς İ ﬃ\r\n",
        "cp1252" | "latin1" | "ibm850" => "niño Straße été\r\n",
        "long" => "é😀e\u{301}界",
        _ => "abcdefg\n",
    };
    assert!(
        ["ascii", "unicode", "cp1252", "latin1", "ibm850", "long", "pieces", "multi"]
            .contains(&case.as_str())
    );
    let target = if case == "long" {
        16 * 1024 * 1024
    } else {
        4 * 1024 * 1024
    };
    let root = std::env::temp_dir().join(format!("jc-m2-acceptance-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let fixture = Fixture { root };
    let count = if case == "multi" { 8 } else { 1 };
    let mut paths = Vec::new();
    {
        let unit = encode(unit, encoding).unwrap();
        let bytes = unit.repeat(target / unit.len());
        for i in 0..count {
            let path = fixture.root.join(format!("document-{i}.txt"));
            fs::write(&path, &bytes).unwrap();
            paths.push(path);
        }
    }
    println!("case,operation,sample,active_bytes,pieces,history_charge,retained_source_capacity,active_projection_bytes,raw_scanned,decoded,elapsed_ns,allocation_calls,allocated_bytes,peak_live_bytes,live_bytes,private_bytes,working_set_bytes,process_peak_commit_bytes");
    let mut app = Workbench::default();
    let start = support::Sample::start();
    emit(&case, "baseline", 0, start.finish(), &app);
    let start = support::Sample::start();
    let mut tabs = Vec::new();
    for path in &paths {
        tabs.push(app.open_file(path, Some(encoding)).unwrap());
    }
    emit(&case, "open", 0, start.finish(), &app);
    if case == "pieces" {
        let d = app.active_document().unwrap();
        let (id, rev, len) = (
            d.id,
            d.session.document().revision(),
            d.session.document().len(),
        );
        let edits = (1..=2048)
            .map(|i| TextReplacement {
                range: (i * (len / 2049))..(i * (len / 2049)),
                text: "x".into(),
            })
            .collect();
        let start = support::Sample::start();
        app.apply_text_edits(id, rev, edits).unwrap();
        emit(&case, "fragment", 0, start.finish(), &app);
    }
    let start = support::Sample::start();
    for tab in &tabs {
        app.jump_to_location(*tab, if case == "long" { 0 } else { 8 });
        black_box(app.visible_line(*tab, 0, 0, 80));
    }
    emit(&case, "cold_projection", 0, start.finish(), &app);
    let start = support::Sample::start();
    app.split_editor_right();
    app.focus_editor_pane(PaneId::Primary);
    if count > 1 {
        app.jump_to_location(tabs[0], 0);
    }
    viewport(&mut app);
    emit(&case, "two_views", 0, start.finish(), &app);
    for i in 0..36 {
        let start = support::Sample::start();
        app.move_right(false).unwrap();
        black_box(app.active_position());
        viewport(&mut app);
        let metrics = start.finish();
        if i >= 5 {
            emit(&case, "warm_move_viewport", i - 5, metrics, &app);
        }
    }
    for i in 0..36 {
        let start = support::Sample::start();
        app.set_search_query(format!("missing-{i}")).unwrap();
        while app.search_running() {
            app.poll_document_search();
        }
        assert_eq!(app.search().status, SearchStatus::Complete);
        let metrics = start.finish();
        if i >= 5 {
            emit(&case, "search_absent", i - 5, metrics, &app);
        }
    }
    for i in 0..36 {
        let start = support::Sample::start();
        app.insert_text("x").unwrap();
        app.undo().unwrap();
        app.redo().unwrap();
        app.undo().unwrap();
        let metrics = start.finish();
        if i >= 5 {
            emit(&case, "edit_undo_redo_undo", i - 5, metrics, &app);
        }
    }
    app.cancel_search();
    let start = support::Sample::start();
    for tab in &tabs {
        let id = app.document_by_tab(*tab).unwrap().id;
        app.set_history_budget(id, 0).unwrap();
    }
    emit(&case, "release_history", 0, start.finish(), &app);
    let start = support::Sample::start();
    app.close_split_editor();
    while app.active_document().is_some() {
        app.close_active(true).unwrap();
    }
    emit(&case, "close_all", 0, start.finish(), &app);
    let start = support::Sample::start();
    drop(app);
    let app = Workbench::default();
    emit(&case, "drop_workbench", 0, start.finish(), &app);
}
