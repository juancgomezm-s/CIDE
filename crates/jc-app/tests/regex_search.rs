use jc_app::{
    CommandId, ProjectSearch, ProjectSearchOptions, ProjectSearchRow, ProjectSearchState,
    SearchLimits, SearchStatus, Wakeup, Workbench,
};
use jc_text::{encode, TextEncoding, TextPosition};
use std::{
    ops::Range,
    time::{Duration, Instant},
};
fn app(text: &str, encoding: TextEncoding) -> Workbench {
    let mut app = Workbench::default();
    app.new_file("regex-memory.txt", encoding);
    app.insert_text(text).unwrap();
    app.toggle_search_regex().unwrap();
    app.toggle_search_case().unwrap();
    app
}
fn finish(app: &mut Workbench) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.search_running() {
        app.poll_document_search();
        assert!(Instant::now() < deadline);
    }
}
fn query(app: &mut Workbench, text: &str) -> Vec<Range<usize>> {
    app.set_search_query(text).unwrap();
    finish(app);
    assert_eq!(app.search().status, SearchStatus::Complete);
    app.search()
        .matches
        .iter()
        .map(|m| m.range.clone())
        .collect()
}
#[test]
fn regex_alternation_greediness_anchors_multiline_and_unicode_cross_chunks() {
    for size in [4, 5, 7, 16, 4096] {
        let mut a = app("\u{feff}niño12 niño345\r\nαβ\nend", TextEncoding::Utf8);
        a.set_search_limits(SearchLimits {
            step_bytes: size,
            ..SearchLimits::default()
        })
        .unwrap();
        assert_eq!(query(&mut a, r"niño\d+"), vec![3..10, 11..19]);
        assert_eq!(query(&mut a, r"(?m)^α.+$"), vec![21..25]);
        assert_eq!(
            a.search().matches[0].position,
            TextPosition { line: 1, column: 0 }
        );
        assert_eq!(query(&mut a, r"(?s)12.*end"), vec![8..29]);
        assert_eq!(query(&mut a, r"niño|niño12"), vec![3..8, 11..16]);
        assert_eq!(query(&mut a, r"niño\d+?"), vec![3..9, 11..17]);
        assert_eq!(query(&mut a, r"\Aniño"), vec![3..8]);
        assert_eq!(query(&mut a, r"end\z"), vec![26..29]);
        assert!(query(&mut a, r"\r").is_empty());
        assert!(query(&mut a, r"\n").iter().all(|r| r != &(20..21)));
        assert_eq!(query(&mut a, r"\r\n"), vec![19..21]);
    }
}
#[test]
fn regex_empty_matches_are_finite_legal_and_undoable_insertions() {
    let mut a = app("a\r\nb", TextEncoding::Utf8);
    assert_eq!(query(&mut a, r"(?m)^"), vec![0..0, 3..3]);
    a.go_to_position(TextPosition { line: 0, column: 0 })
        .unwrap();
    assert_eq!(a.find_next().unwrap(), Some(0..0));
    assert_eq!(a.find_next().unwrap(), Some(3..3));
    assert_eq!(a.find_previous().unwrap(), Some(0..0));
    assert_eq!(a.replace_all("X").unwrap(), 2);
    assert_eq!(a.active_bytes().unwrap(), b"Xa\r\nXb");
    a.undo().unwrap();
    assert_eq!(a.active_bytes().unwrap(), b"a\r\nb");
    assert_eq!(query(&mut a, r"$").last(), Some(&(4..4)));
    let mut empty = app("", TextEncoding::Utf8);
    assert_eq!(query(&mut empty, r"^$"), vec![0..0]);
}

#[test]
fn regex_streaming_matches_flat_engine_oracle_without_chunk_anchors_or_lost_greediness() {
    for text in [
        "",
        "a",
        "aaaa",
        "aab ab xab",
        "áa βb😀 ab",
        "a\nb\na",
        "xyz123z45",
    ] {
        for pattern in [
            "a*",
            "a+",
            "a*?",
            "a|ab",
            "ab|a",
            "(?:ab)+",
            "[a-z]+[0-9]*",
            "(?s).*",
            "(?m)^a|b$",
            "[^x]+",
            r"\A.*\z",
            "a.*b|a",
        ] {
            let flat = regex_automata::dfa::regex::Regex::new(pattern).unwrap();
            let expected: Vec<_> = flat.find_iter(text.as_bytes()).map(|m| m.range()).collect();
            for chunk in [4, 7, 16] {
                let mut a = app(text, TextEncoding::Utf8);
                a.set_search_limits(SearchLimits {
                    step_bytes: chunk,
                    ..SearchLimits::default()
                })
                .unwrap();
                assert_eq!(
                    query(&mut a, pattern),
                    expected,
                    "text={text:?} pattern={pattern} chunk={chunk}"
                );
            }
        }
    }
}
#[test]
fn regex_legacy_offsets_strict_literal_replacement_history_and_selection() {
    for encoding in [
        TextEncoding::Utf8,
        TextEncoding::Windows1252,
        TextEncoding::Iso8859_1,
        TextEncoding::Ibm850,
    ] {
        let original = "NIÑO12\r\nniño3";
        let mut a = app(original, encoding);
        a.toggle_search_case().unwrap();
        assert_eq!(query(&mut a, r"niño[0-9]+").len(), 2);
        a.remember_search();
        if encoding != TextEncoding::Utf8 {
            assert!(a.replace_all("😀").is_err());
        }
        assert_eq!(a.replace_all("$1").unwrap(), 2); // explicit literal replacement
        assert_eq!(a.active_bytes().unwrap(), b"$1\r\n$1");
        a.undo().unwrap();
        assert_eq!(
            a.active_bytes().unwrap(),
            encode(original, encoding).unwrap()
        );
        a.toggle_search_regex().unwrap();
        a.cycle_search_history(true).unwrap();
        assert!(a.search().regex);
        finish(&mut a);
        a.find_next().unwrap();
        a.execute(CommandId::SearchSelectionNext).unwrap();
        assert!(!a.search().regex);
    }
}
#[test]
fn regex_limits_invalid_patterns_cancellation_and_revisions_prevent_partial_replace() {
    let mut a = app(&"a".repeat(4096), TextEncoding::Utf8);
    a.set_search_limits(SearchLimits {
        max_scan_bytes: 128,
        step_bytes: 4,
        ..SearchLimits::default()
    })
    .unwrap();
    a.set_search_query("a*b").unwrap();
    a.poll_document_search();
    assert!(a.search_running());
    a.cancel_search();
    assert_eq!(a.search().status, SearchStatus::Cancelled);
    a.set_search_query("a*b").unwrap();
    finish(&mut a);
    assert_eq!(a.search().status, SearchStatus::Limited);
    assert!(a.replace_all("X").is_err());
    let before = a.active_bytes().unwrap();
    for pattern in ["[", r"(a)\1", r"(?=a)", r"\bniño\b", "a{100000000}"] {
        assert!(a.set_search_query(pattern).is_err(), "{pattern}");
        assert!(!a.search_running());
        assert_eq!(a.search().status, SearchStatus::Unavailable);
        assert!(a.replace_all("X").is_err());
    }
    assert_eq!(a.active_bytes().unwrap(), before);
    a.set_search_limits(SearchLimits::default()).unwrap_err(); // still invalid query
    a.set_search_query("a+").unwrap();
    finish(&mut a);
    assert_eq!(a.search().matches.len(), 1);
    a.insert_text("z").unwrap();
    assert!(a.search().matches.is_empty());
    assert!(a.replace_all("x").is_err());
}
#[test]
fn regex_project_scans_dirty_buffers_and_disk_then_previews_literal_edits() {
    let root = std::env::temp_dir().join(format!("cide-regex-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("source.txt");
    std::fs::write(&path, b"disk12\r\n").unwrap();
    let mut a = Workbench::default();
    a.open_file(&path, Some(TextEncoding::Utf8)).unwrap();
    a.select_all().unwrap();
    a.insert_text("dirty34\r\n").unwrap();
    let mut search = ProjectSearch::new(Wakeup::default());
    let mut options = ProjectSearchOptions::literal(r"[a-z]+\d+");
    options.regex = true;
    search.start(&a, &root, options).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while search.state == ProjectSearchState::Running {
        search.poll(&a, 4);
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(search.state, ProjectSearchState::Complete);
    assert!(matches!(&search.rows()[0],ProjectSearchRow::Match(m) if m.range==(0..7)));
    let mut preview = search.preview_replace(&a, "$1".into()).unwrap();
    let outcome = search.apply_next_replace(&mut preview, &mut a).unwrap();
    assert!(outcome.error.is_none());
    assert_eq!(a.active_bytes().unwrap(), b"$1\r\n");
    a.undo().unwrap();
    assert_eq!(a.active_bytes().unwrap(), b"dirty34\r\n");
    assert_eq!(std::fs::read(&path).unwrap(), b"disk12\r\n");
    drop(search);
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(root).unwrap();
}
