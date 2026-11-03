use jc_app::{CommandId, SearchLimits, SearchStatus, Workbench};
use jc_text::{encode, TextEncoding, TextPosition};
use std::{
    fs,
    ops::Range,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
static NEXT: AtomicU64 = AtomicU64::new(0);

#[test]
fn deferred_find_never_overwrites_a_new_view_caret_or_selection_intent() {
    let mut app = app(&format!("{}needle", "x".repeat(64 * 1024)));
    app.go_to_position(TextPosition { line: 0, column: 0 })
        .unwrap();
    app.set_search_query("needle").unwrap();
    app.find_next().unwrap();
    app.split_editor_right();
    app.focus_editor_pane(jc_app::PaneId::Secondary);
    finish(&mut app);
    assert_eq!(app.active_editor().unwrap().cursor, 0);
    app.set_search_query("need").unwrap();
    app.find_next().unwrap();
    app.move_right(false).unwrap();
    finish(&mut app);
    assert_eq!(app.active_editor().unwrap().cursor, 1);
    app.move_end(false).unwrap();
    let len = app.active_document().unwrap().session.document().len();
    app.set_search_query("needle").unwrap();
    app.find_next().unwrap();
    app.select_all().unwrap();
    finish(&mut app);
    assert_eq!(app.active_editor().unwrap().selection(), Some(0..len));
}

#[test]
fn navigation_and_replace_current_follow_the_active_view_not_a_global_match_index() {
    let mut app = app("a a a");
    app.go_to_position(TextPosition { line: 0, column: 0 })
        .unwrap();
    query(&mut app, "a");
    assert_eq!(app.find_next().unwrap(), Some(0..1));
    app.split_editor_right();
    app.focus_editor_pane(jc_app::PaneId::Secondary);
    app.go_to_position(TextPosition { line: 0, column: 4 })
        .unwrap();
    assert_eq!(app.find_next().unwrap(), Some(4..5));
    app.focus_editor_pane(jc_app::PaneId::Primary);
    app.replace_current("X").unwrap();
    assert_eq!(app.active_bytes().unwrap(), b"X a a");
    app.undo().unwrap();
    assert_eq!(app.active_bytes().unwrap(), b"a a a");
}
fn app(text: &str) -> Workbench {
    let mut app = Workbench::default();
    app.new_file("s0-memory.txt", TextEncoding::Utf8);
    app.insert_text(text).unwrap();
    app
}
fn finish(app: &mut Workbench) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.search_running() {
        app.poll_document_search();
        assert!(Instant::now() < deadline);
    }
    assert_eq!(app.search().status, SearchStatus::Complete);
}
fn query(app: &mut Workbench, text: &str) -> Vec<Range<usize>> {
    app.set_search_query(text).unwrap();
    finish(app);
    app.search()
        .matches
        .iter()
        .map(|m| m.range.clone())
        .collect()
}
fn tiny(app: &mut Workbench) {
    app.set_search_limits(SearchLimits {
        step_bytes: 4,
        ..SearchLimits::default()
    })
    .unwrap();
}
struct Fixture(std::path::PathBuf);
impl Fixture {
    fn new(bytes: &[u8]) -> Self {
        let path = std::env::temp_dir().join(format!(
            "jc-s0-{}-{}.txt",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        std::io::Write::write_all(&mut file, bytes).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_file(&self.0).unwrap();
    }
}

#[test]
fn full_fold_maps_expansions_and_never_matches_part_of_an_original_scalar() {
    let text = "Straße STRASSE Σςσ İ i\u{307} K k ﬃ ffi é e\u{301}";
    let mut app = app(text);
    tiny(&mut app);
    assert_eq!(query(&mut app, "strasse"), vec![0..7, 8..15]);
    assert_eq!(query(&mut app, "SS"), vec![4..6, 12..14]);
    let results = query(&mut app, "σ");
    assert_eq!(results.len(), 3);
    for hit in &app.search().matches {
        assert_eq!(
            hit.position,
            TextPosition {
                line: 0,
                column: text[..hit.range.start].chars().count()
            }
        );
    }
    let results = query(&mut app, "i\u{307}");
    assert_eq!(results.len(), 2);
    assert_eq!(&text[results[0].clone()], "İ");
    assert_eq!(&text[results[1].clone()], "i\u{307}");
    for range in query(&mut app, "i") {
        assert_ne!(&text[range], "İ");
    }
    let ranges = query(&mut app, "ffi");
    assert_eq!(ranges.len(), 2);
    assert_eq!(&text[ranges[0].clone()], "ﬃ");
    let ranges = query(&mut app, "é");
    assert_eq!(ranges.len(), 1);
    assert_eq!(&text[ranges[0].clone()], "é"); // no NFC/NFD normalization
    app.toggle_search_case().unwrap();
    assert_eq!(query(&mut app, "strasse"), Vec::<Range<usize>>::new());
}

#[test]
fn unicode_whole_words_include_marks_numbers_connectors_and_join_controls() {
    let text = "niño niño2 (NIÑO) niÑo_ foo\u{301} foo \u{203f}foo 𐐀foo 😀foo";
    let mut app = app(text);
    tiny(&mut app);
    app.toggle_search_whole_word().unwrap();
    assert_eq!(query(&mut app, "NIÑO").len(), 2);
    let ranges = query(&mut app, "foo");
    assert_eq!(ranges.len(), 2);
    for range in ranges {
        assert_eq!(&text[range.clone()], "foo");
        assert!(!text[..range.start].ends_with('𐐀'));
    }
}

#[test]
fn scheduling_is_range_bounded_cancelable_versioned_and_incomplete_replace_is_rejected() {
    let mut app = app(&format!("a{}needle", "z".repeat(1024 * 1024)));
    app.set_search_query("needle").unwrap();
    assert_eq!(app.search().scanned_bytes, 0);
    assert!(app.search().matches.is_empty());
    app.poll_document_search();
    assert!(app.search().scanned_bytes <= 16 * 1024);
    assert!(app.search_running());
    let generation = app.search().generation;
    app.cancel_search();
    assert!(!app.search_running());
    assert_eq!(app.search().status, SearchStatus::Cancelled);
    app.insert_text("edited while cancelled").unwrap();
    assert!(!app.search_running());
    assert_eq!(app.search().status, SearchStatus::Cancelled);
    assert!(app.replace_all("X").is_err());
    app.set_search_query("a").unwrap();
    assert!(app.search().generation > generation);
    app.poll_document_search();
    assert!(!app.search().matches.is_empty());
    app.insert_text("change").unwrap();
    assert!(app.search().matches.is_empty());
    assert_eq!(app.search().scanned_bytes, 0);
    let old = app.search().target.unwrap();
    app.new_file("other.txt", TextEncoding::Utf8);
    assert_ne!(app.search().target.unwrap().document, old.document);
    app.set_search_limits(SearchLimits {
        max_scan_bytes: 4,
        ..SearchLimits::default()
    })
    .unwrap();
    app.insert_text("aaaaaaa").unwrap();
    app.set_search_query("a").unwrap();
    while app.search_running() {
        app.poll_document_search();
    }
    assert_eq!(app.search().status, SearchStatus::Limited);
    assert!(app.replace_current("b").is_err());
    app.set_search_limits(SearchLimits {
        max_results: 2,
        ..SearchLimits::default()
    })
    .unwrap();
    while app.search_running() {
        app.poll_document_search();
    }
    assert_eq!(app.search().status, SearchStatus::Limited);
    assert_eq!(app.search().matches.len(), 2);
    app.toggle_search_regex().unwrap();
    assert!(app.search().regex);
    assert_eq!(app.search().status, SearchStatus::Running);
}

#[test]
fn source_pages_keep_bytes_eol_and_one_atomic_undo_with_strict_replacement() {
    for encoding in [
        TextEncoding::Windows1252,
        TextEncoding::Iso8859_1,
        TextEncoding::Ibm850,
    ] {
        let original = encode("NIÑO niño Straße K\r\n", encoding).unwrap();
        let fixture = Fixture::new(&original);
        let mut app = Workbench::default();
        app.open_file(&fixture.0, Some(encoding)).unwrap();
        tiny(&mut app);
        assert_eq!(query(&mut app, "niño"), vec![0..4, 5..9]);
        assert!(app.replace_all("😀").is_err());
        assert_eq!(app.active_bytes().unwrap(), original);
        assert_eq!(app.replace_all("niña").unwrap(), 2);
        assert_eq!(
            app.active_bytes().unwrap(),
            encode("niña niña Straße K\r\n", encoding).unwrap()
        );
        app.execute(CommandId::EditUndo).unwrap();
        assert_eq!(app.active_bytes().unwrap(), original);
        assert_eq!(fs::read(&fixture.0).unwrap(), original);
        assert_eq!(query(&mut app, "strasse").len(), 1);
        assert_eq!(query(&mut app, "K").len(), 1); // query need not encode in the source page to fold to K
    }
}

#[test]
fn invalid_utf8_is_a_barrier_and_crlf_bom_and_chunk_boundaries_are_exact() {
    let bytes = b"\xef\xbb\xbfa\xe1\x80a\xef\xbf\xbd\r\na";
    let fixture = Fixture::new(bytes);
    let mut app = Workbench::default();
    app.open_file(&fixture.0, Some(TextEncoding::Utf8)).unwrap();
    tiny(&mut app);
    assert!(query(&mut app, "aa").is_empty());
    assert_eq!(query(&mut app, "�"), vec![7..10]);
    assert!(query(&mut app, "\n").is_empty());
    assert!(query(&mut app, "\r").is_empty());
    assert_eq!(query(&mut app, "\r\n"), vec![10..12]);
    assert_eq!(query(&mut app, "a"), vec![3..4, 6..7, 12..13]);
    assert_eq!(
        app.search().matches[2].position,
        TextPosition { line: 1, column: 0 }
    );
    assert_eq!(app.active_bytes().unwrap(), bytes);
}

#[test]
fn selection_navigation_history_and_scalar_goto_are_shared_operations() {
    let mut app = app("aa bb aa\r\ná😀\n");
    let tab = app.tabs().active_id().unwrap();
    app.jump_to_location(tab, 0);
    query(&mut app, "aa");
    app.find_next().unwrap();
    app.execute(CommandId::SearchSelectionNext).unwrap();
    finish(&mut app);
    assert_eq!(app.active_editor().unwrap().selection(), Some(6..8));
    app.execute(CommandId::SearchSelectionPrevious).unwrap();
    finish(&mut app);
    assert_eq!(app.active_editor().unwrap().selection(), Some(0..2));
    query(&mut app, "bb");
    app.find_next().unwrap();
    app.execute(CommandId::SearchHistoryPrevious).unwrap();
    finish(&mut app);
    assert_eq!(app.search().query, "bb");
    app.execute(CommandId::SearchHistoryPrevious).unwrap();
    finish(&mut app);
    assert_eq!(app.search().query, "aa");
    let generation = app.search().generation;
    app.go_to_position(TextPosition { line: 1, column: 1 })
        .unwrap();
    assert_eq!(app.active_editor().unwrap().cursor, 12);
    assert_eq!(app.search().generation, generation);
    let before = app.active_editor().unwrap().cursor;
    assert!(app
        .go_to_position(TextPosition { line: 1, column: 3 })
        .is_err());
    assert_eq!(app.active_editor().unwrap().cursor, before);
    for i in 0..30 {
        query(&mut app, &format!("query{i}"));
        app.find_next().unwrap();
    }
    assert_eq!(app.search().history.len(), 20);
}

#[test]
fn deadlines_input_and_replacement_budgets_fail_without_mutating_source() {
    let mut app = app(&"a".repeat(10_000));
    app.set_search_limits(SearchLimits {
        total_time: Duration::from_nanos(1),
        ..SearchLimits::default()
    })
    .unwrap();
    app.set_search_query("a").unwrap();
    app.poll_document_search();
    assert_eq!(app.search().status, SearchStatus::TimedOut);
    assert_eq!(app.search().scanned_bytes, 0);
    app.set_search_limits(SearchLimits::default()).unwrap();
    finish(&mut app);
    assert_eq!(app.search().matches.len(), 10_000);
    let revision = app.active_document().unwrap().session.document().revision();
    assert!(app.replace_all(&"b".repeat(4096)).is_err());
    assert_eq!(
        app.active_document().unwrap().session.document().revision(),
        revision
    );
    assert!(app.set_search_query("q".repeat(4097)).is_err());
    assert!(app.search().matches.is_empty());
    assert!(!app.search_running());
    assert!(app.replace_all("b").is_err());
    assert!(app
        .set_search_limits(SearchLimits {
            step_bytes: 1,
            ..SearchLimits::default()
        })
        .is_err());
}
