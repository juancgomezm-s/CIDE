//! Presentation never becomes source; literal blocks retain exact line endings.
use jc_app::{CommandId, PaneId, SearchStatus, Workbench};
use jc_text::{TextEncoding, TextPosition};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new(bytes: &[u8]) -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .canonicalize()
            .unwrap();
        let base = root.join("work/test-temp");
        fs::create_dir_all(&base).unwrap();
        assert!(base.canonicalize().unwrap().starts_with(root));
        let path = base.join(format!(
            "visible-blocks-{}-{}.txt",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap()
            .write_all(bytes)
            .unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_file(&self.0).unwrap();
    }
}
fn complete(app: &mut Workbench) {
    for _ in 0..10000 {
        if !app.search_running() {
            assert_eq!(app.search().status, SearchStatus::Complete);
            return;
        }
        app.poll_document_search();
    }
    panic!("bounded fixture search did not complete");
}

#[test]
fn invisible_view_preserves_source_dirty_undo_copy_encoding_and_exact_save() {
    let text="\u{feff}a \t\0\x01\x1b\x7f\u{a0}\u{feff}\u{200b}\u{200d}\u{202e}\u{2028}\u{2029}\r\nb\nc\rlast";
    let fixture = Fixture::new(text.as_bytes());
    let mut app = Workbench::default();
    let tab = app.open_file(&fixture.0, Some(TextEncoding::Utf8)).unwrap();
    let revision = app.active_document().unwrap().session.document().revision();
    let undo = app
        .active_document()
        .unwrap()
        .session
        .document()
        .undo_steps();
    app.select_all().unwrap();
    app.copy_selection().unwrap();
    let selected = app.selection_for_pane(PaneId::Primary);
    app.execute(CommandId::ViewToggleInvisibles).unwrap();
    assert!(app.invisibles_visible());
    let rows: Vec<_> = (0..app.line_count_for_tab(tab).unwrap())
        .map(|line| {
            app.display_row(tab, line, 0, 4096)
                .unwrap()
                .iter()
                .map(|c| c.ch)
                .collect::<String>()
        })
        .collect();
    let rendered = rows.join("|");
    for indicator in [
        "\\u{FEFF}",
        "\\s",
        "\\t",
        "\\x00",
        "\\x01",
        "\\x1B",
        "\\x7F",
        "\\u{A0}",
        "\\u{200B}",
        "\\u{200D}",
        "\\u{202E}",
        "\\u{2028}",
        "\\u{2029}",
        "\\r\\n",
        "b\\n",
        "c\\r",
    ] {
        assert!(rendered.contains(indicator), "{indicator}: {rendered}");
    }
    assert!(rendered.chars().all(|c| c.is_ascii() && !c.is_control()));
    app.execute(CommandId::ViewToggleInvisibles).unwrap();
    assert_eq!(app.active_bytes().unwrap(), text.as_bytes());
    assert_eq!(app.selection_for_pane(PaneId::Primary), selected);
    let doc = app.active_document().unwrap();
    assert_eq!(doc.session.document().revision(), revision);
    assert_eq!(doc.session.document().undo_steps(), undo);
    assert!(!doc.session.document().is_modified());
    assert_eq!(app.active_encoding(), Some(TextEncoding::Utf8));
    app.save_active().unwrap();
    assert_eq!(fs::read(&fixture.0).unwrap(), text.as_bytes());
    app.new_file("invisible-copy-target.txt", TextEncoding::Utf8);
    app.paste().unwrap();
    assert_eq!(
        app.active_bytes().unwrap(),
        text.trim_start_matches('\u{feff}').as_bytes()
    );
}
#[test]
fn visual_spans_map_caret_selection_tabs_and_crlf_without_scalar_column_aliasing() {
    let mut app = Workbench::default();
    let tab = app.new_file("span-memory.txt", TextEncoding::Utf8);
    app.insert_text("a é\tz\r\n").unwrap();
    app.execute(CommandId::ViewToggleInvisibles).unwrap();
    app.set_caret_position(TextPosition { line: 0, column: 3 }, false)
        .unwrap();
    assert_eq!(app.active_editor().unwrap().cursor, 4);
    assert_eq!(app.display_column_for_pane(PaneId::Primary), Some(9));
    let cells = app.display_row(tab, 0, 0, 80).unwrap();
    let unicode: Vec<_> = cells
        .iter()
        .enumerate()
        .filter(|(_, c)| c.source == (2..4))
        .collect();
    assert_eq!(unicode.len(), 6);
    for (column, _) in unicode {
        assert_eq!(app.display_byte_at(tab, 0, column), Some(2));
    }
    let eol = app.line_range_for_tab(tab, 0).unwrap().1;
    assert_eq!(eol, 6..8);
    let indicator: Vec<_> = cells
        .iter()
        .enumerate()
        .filter(|(_, c)| c.source == eol)
        .collect();
    assert_eq!(indicator.len(), 4);
    for (column, _) in indicator {
        assert_eq!(app.display_byte_at(tab, 0, column), Some(6));
    }
    assert!(!app.jump_to_location(tab, 7));
    app.reveal_active_pane(10, 4);
    assert!(app.panes().view(PaneId::Primary).unwrap().scroll_column() > 0);
    assert_eq!(app.display_row(tab, 0, 4, 4).unwrap(), cells[4..8]);
}
#[test]
fn literal_multiline_replace_all_is_one_undo_exact_eol_and_strict_legacy() {
    let original = "\u{feff}α\r\nβ\nend\rα\r\nβ";
    let fixture = Fixture::new(original.as_bytes());
    let mut app = Workbench::default();
    app.open_file(&fixture.0, Some(TextEncoding::Utf8)).unwrap();
    app.execute(CommandId::ViewToggleInvisibles).unwrap();
    app.split_editor_right();
    app.set_search_query("α\nβ").unwrap();
    complete(&mut app);
    assert!(app.search().matches.is_empty());
    app.set_search_query("α\r\nβ").unwrap();
    complete(&mut app);
    assert_eq!(app.search().matches.len(), 2);
    let before = app
        .active_document()
        .unwrap()
        .session
        .document()
        .undo_steps();
    assert_eq!(app.replace_all("é\nQ\r\nR").unwrap(), 2);
    let expected = "\u{feff}é\nQ\r\nR\nend\ré\nQ\r\nR";
    assert_eq!(app.active_bytes().unwrap(), expected.as_bytes());
    assert_eq!(
        app.active_document()
            .unwrap()
            .session
            .document()
            .undo_steps(),
        before + 1
    );
    app.save_active().unwrap();
    assert_eq!(fs::read(&fixture.0).unwrap(), expected.as_bytes());
    app.focus_editor_pane(PaneId::Secondary);
    app.undo().unwrap();
    assert_eq!(app.active_bytes().unwrap(), original.as_bytes());
    app.save_active().unwrap();
    assert_eq!(fs::read(&fixture.0).unwrap(), original.as_bytes());
    app.set_search_query("β").unwrap();
    complete(&mut app);
    let tab = app.tabs().active_id().unwrap();
    assert!(app.jump_to_location(tab, original.len()));
    app.find_previous().unwrap();
    assert!(app.replace_current("last\nline").unwrap());
    assert!(app.active_bytes().unwrap().ends_with(b"last\nline"));
    app.undo().unwrap();
    let mut legacy = Workbench::default();
    legacy.new_file("legacy-blocks.p", TextEncoding::Windows1252);
    legacy.insert_text("niño\r\n€").unwrap();
    legacy.set_search_query("niño\r\n€").unwrap();
    complete(&mut legacy);
    let bytes = legacy.active_bytes().unwrap();
    assert!(legacy.replace_all("😀\n").is_err());
    assert_eq!(legacy.active_bytes().unwrap(), bytes);
}
