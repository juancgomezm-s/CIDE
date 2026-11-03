//! Integrated event -> shared syntax -> cells -> VT flows, not lexer internals.
use super::TuiApp;
use crate::{
    screen::{CellStyle, Screen},
    state::LaunchOptions,
    FrameEncoder,
};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use jc_app::{PaneId, SyntaxKind};
use jc_text::{TextEncoding, TextPosition};
fn key(app: &mut TuiApp, code: KeyCode, mods: KeyModifiers) {
    app.handle_event(Event::Key(KeyEvent::new(code, mods)));
}
fn settled(app: &mut TuiApp) -> Screen {
    for _ in 0..512 {
        let screen = app.frame(140, 32);
        if !app.syntax_pending {
            return screen;
        }
        app.poll_tasks();
    }
    panic!("bounded syntax work failed to settle");
}
fn style(screen: &Screen, needle: &str) -> CellStyle {
    for row in 2..screen.height() - 1 {
        let text = screen.row_text(row);
        if let Some(at) = text.find(needle) {
            return screen.cell(text[..at].chars().count() as u16, row).style;
        }
    }
    panic!("source text not visible: {needle}");
}
#[test]
fn syntax_tui_multiline_edits_undo_encodings_and_vt_colors() {
    for encoding in [
        TextEncoding::Utf8,
        TextEncoding::Windows1252,
        TextEncoding::Iso8859_1,
        TextEncoding::Ibm850,
    ] {
        let mut app = TuiApp::new(LaunchOptions::default());
        let bom = if encoding == TextEncoding::Utf8 {
            "\u{feff}"
        } else {
            ""
        };
        app.workbench.new_file("syntax.p", encoding);
        let text=format!("{bom}DEFINE VARIABLE n AS INTEGER NO-UNDO.\r\n/* outer\r\n/* inner */\r\nstill */\r\nDISPLAY \"niño~\"tail\" n + 42.\r\n");
        app.workbench.insert_text(&text).unwrap();
        app.workbench
            .go_to_position(TextPosition { line: 0, column: 0 })
            .unwrap();
        let original = app.workbench.active_bytes().unwrap();
        let doc = app.workbench.active_document().unwrap().id;
        let first = settled(&mut app);
        assert_eq!(
            style(&first, "DEFINE"),
            CellStyle::Syntax(SyntaxKind::Keyword)
        );
        assert_eq!(
            style(&first, "still"),
            CellStyle::Syntax(SyntaxKind::Comment)
        );
        assert_eq!(style(&first, "niño"), CellStyle::Syntax(SyntaxKind::String));
        assert_eq!(style(&first, "42"), CellStyle::Syntax(SyntaxKind::Number));
        let mut encoder = FrameEncoder::default();
        let mut vt = Vec::new();
        encoder.draw_to(&mut vt, &first).unwrap();
        let vt = String::from_utf8(vt).unwrap();
        if std::env::var("NO_COLOR").is_ok_and(|value| !value.is_empty()) {
            assert!(!vt.contains("38;2;"));
        } else {
            assert!(vt.contains("38;2;249;38;114"));
            assert!(vt.contains("38;2;230;219;116"));
        }
        let before = app
            .workbench
            .active_document()
            .unwrap()
            .session
            .document()
            .revision();
        app.handle_event(Event::Paste("/*".into()));
        assert_eq!(
            style(&settled(&mut app), "DEFINE"),
            CellStyle::Syntax(SyntaxKind::Comment)
        );
        key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(
            style(&settled(&mut app), "DEFINE"),
            CellStyle::Syntax(SyntaxKind::Keyword)
        );
        assert_eq!(
            app.workbench
                .active_document()
                .unwrap()
                .session
                .document()
                .revision(),
            before
        );
        key(&mut app, KeyCode::Char('y'), KeyModifiers::CONTROL);
        assert_eq!(
            style(&settled(&mut app), "DEFINE"),
            CellStyle::Syntax(SyntaxKind::Comment)
        );
        key(&mut app, KeyCode::Backspace, KeyModifiers::NONE);
        key(&mut app, KeyCode::Backspace, KeyModifiers::NONE);
        assert_eq!(
            style(&settled(&mut app), "DEFINE"),
            CellStyle::Syntax(SyntaxKind::Keyword)
        );
        assert_eq!(app.workbench.active_bytes().unwrap(), original);
        assert_eq!(app.workbench.active_document().unwrap().id, doc);
        assert_eq!(app.workbench.active_encoding(), Some(encoding));
        app.workbench.split_editor_right();
        app.workbench.toggle_reference_pin();
        let before = format!("{:?}", app.workbench.panes());
        let presentation = app.prepare_frame(140, 32);
        let a = crate::render::render(&presentation);
        let b = crate::render::render(&presentation);
        assert_eq!(a, b);
        assert_eq!(format!("{:?}", presentation.app.workbench.panes()), before);
    }
}
#[test]
fn syntax_tui_selection_search_invisibles_document_switch_and_plain_fallback() {
    let mut app = TuiApp::new(LaunchOptions::default());
    let tab = app.workbench.new_file("source.p", TextEncoding::Utf8);
    app.workbench
        .insert_text("DEFINE\tVARIABLE n AS INTEGER.\nDISPLAY \"hello\".")
        .unwrap();
    app.workbench
        .go_to_position(TextPosition { line: 0, column: 0 })
        .unwrap();
    settled(&mut app);
    app.workbench.set_search_query("DEFINE").unwrap();
    while app.workbench.search_running() {
        app.poll_tasks();
    }
    assert_eq!(style(&settled(&mut app), "DEFINE"), CellStyle::Reverse);
    app.workbench.set_search_query(String::new()).unwrap();
    app.workbench.set_linear_selection_bytes(0, 6).unwrap();
    assert_eq!(style(&settled(&mut app), "DEFINE"), CellStyle::Reverse);
    app.workbench.set_linear_selection_bytes(0, 0).unwrap();
    app.workbench
        .execute(jc_app::CommandId::ViewToggleInvisibles)
        .unwrap();
    let frame = settled(&mut app);
    assert_eq!(
        style(&frame, "DEFINE"),
        CellStyle::Syntax(SyntaxKind::Keyword)
    );
    assert_eq!(style(&frame, "\\t"), CellStyle::Dim);
    app.workbench.new_file("unsupported.cs", TextEncoding::Utf8);
    app.workbench
        .insert_text("DEFINE 123 /* comment */")
        .unwrap();
    app.workbench
        .go_to_position(TextPosition { line: 0, column: 0 })
        .unwrap();
    assert_eq!(style(&settled(&mut app), "DEFINE"), CellStyle::Normal);
    app.workbench.new_file("data.json", TextEncoding::Utf8);
    app.workbench
        .insert_text("{\"key\": true, \"number\": 42}")
        .unwrap();
    app.workbench
        .go_to_position(TextPosition { line: 0, column: 0 })
        .unwrap();
    let json = settled(&mut app);
    assert_eq!(style(&json, "key"), CellStyle::Syntax(SyntaxKind::String));
    assert_eq!(style(&json, "true"), CellStyle::Syntax(SyntaxKind::Keyword));
    assert!(app.workbench.select_tab(1));
    assert_eq!(app.workbench.active_document().unwrap().tab_id, tab);
    assert_eq!(
        style(&settled(&mut app), "DEFINE"),
        CellStyle::Syntax(SyntaxKind::Keyword)
    );
}
#[test]
fn syntax_tui_deep_scroll_resumes_without_another_key_and_cache_is_bounded() {
    let mut app = TuiApp::new(LaunchOptions::default());
    let tab = app.workbench.new_file("large.p", TextEncoding::Utf8);
    let text = format!(
        "/* context\n{}*/\nDEFINE VARIABLE last AS INTEGER.\n",
        "comment line\n".repeat(12000)
    );
    app.workbench.insert_text(&text).unwrap();
    app.workbench
        .go_to_position(TextPosition {
            line: 12002,
            column: 0,
        })
        .unwrap();
    let revision = app
        .workbench
        .active_document()
        .unwrap()
        .session
        .document()
        .revision();
    app.frame(140, 32);
    assert!(app.syntax_pending);
    let frame = settled(&mut app);
    assert_eq!(
        style(&frame, "DEFINE"),
        CellStyle::Syntax(SyntaxKind::Keyword)
    );
    let rows = app.workbench.syntax_rows(tab, 12002, 1);
    assert!(!rows.pending);
    assert!(rows.stats.cache_bytes <= 128 * 1024);
    assert!(rows.stats.processed_bytes <= 64 * 1024);
    assert_eq!(
        app.workbench
            .active_document()
            .unwrap()
            .session
            .document()
            .revision(),
        revision
    );
    app.workbench.scroll_view_vertical(PaneId::Primary, -20, 30);
    settled(&mut app);
    assert_eq!(
        app.workbench
            .active_document()
            .unwrap()
            .session
            .document()
            .revision(),
        revision
    );
}
#[test]
fn syntax_tui_rust_raw_comments_char_lifetime_and_delimiter_edit_roundtrip() {
    let mut app = TuiApp::new(LaunchOptions::default());
    app.workbench.new_file("module.rs", TextEncoding::Utf8);
    let code="fn borrow<'a>(x: &'a str) {\nlet raw = r##\"first\nsecond\"##;\nlet ch = 'x'; let n = 0xff_u8;\n/* outer /* inner */ keep */\n}\n";
    app.workbench.insert_text(code).unwrap();
    app.workbench
        .go_to_position(TextPosition { line: 0, column: 0 })
        .unwrap();
    let source = app.workbench.active_bytes().unwrap();
    let revision = app
        .workbench
        .active_document()
        .unwrap()
        .session
        .document()
        .revision();
    let frame = settled(&mut app);
    for (word, kind) in [
        ("fn", SyntaxKind::Keyword),
        ("'a", SyntaxKind::Preprocessor),
        ("first", SyntaxKind::String),
        ("second", SyntaxKind::String),
        ("'x'", SyntaxKind::String),
        ("0xff_u8", SyntaxKind::Number),
        ("keep", SyntaxKind::Comment),
    ] {
        assert_eq!(style(&frame, word), CellStyle::Syntax(kind), "{word}");
    }
    app.workbench
        .set_caret_position(TextPosition { line: 2, column: 8 }, false)
        .unwrap();
    key(&mut app, KeyCode::Delete, KeyModifiers::NONE);
    assert_eq!(
        style(&settled(&mut app), "let ch"),
        CellStyle::Syntax(SyntaxKind::String)
    );
    key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(
        style(&settled(&mut app), "let ch"),
        CellStyle::Syntax(SyntaxKind::Keyword)
    );
    assert_eq!(
        app.workbench
            .active_document()
            .unwrap()
            .session
            .document()
            .revision(),
        revision
    );
    key(&mut app, KeyCode::Char('y'), KeyModifiers::CONTROL);
    app.handle_event(Event::Paste("#".into()));
    assert_eq!(
        style(&settled(&mut app), "let ch"),
        CellStyle::Syntax(SyntaxKind::Keyword)
    );
    assert_eq!(app.workbench.active_bytes().unwrap(), source);
}
#[test]
fn syntax_tui_external_transaction_recolors_inactive_document_and_one_undo_restores_it() {
    let mut app = TuiApp::new(LaunchOptions::default());
    app.workbench.new_file("external.rs", TextEncoding::Utf8);
    let source = "/* c */\nfn f() {}\n";
    app.workbench.insert_text(source).unwrap();
    app.workbench
        .go_to_position(TextPosition { line: 0, column: 0 })
        .unwrap();
    let left = app.workbench.active_document().unwrap().id;
    let revision = app
        .workbench
        .active_document()
        .unwrap()
        .session
        .document()
        .revision();
    app.workbench.split_editor_right();
    app.workbench.focus_editor_pane(PaneId::Secondary);
    app.workbench.new_file("other.p", TextEncoding::Utf8);
    app.workbench.insert_text("DISPLAY 42.\n").unwrap();
    app.workbench
        .go_to_position(TextPosition { line: 0, column: 0 })
        .unwrap();
    assert_eq!(
        style(&settled(&mut app), "fn f"),
        CellStyle::Syntax(SyntaxKind::Keyword)
    );
    app.workbench
        .apply_text_edits(
            left,
            revision,
            vec![jc_app::TextReplacement {
                range: 5..7,
                text: String::new(),
            }],
        )
        .unwrap();
    assert_eq!(app.workbench.panes().active(), PaneId::Secondary);
    assert_eq!(
        style(&settled(&mut app), "fn f"),
        CellStyle::Syntax(SyntaxKind::Comment)
    );
    assert!(app
        .workbench
        .apply_text_edits(
            left,
            revision,
            vec![jc_app::TextReplacement {
                range: 0..0,
                text: "X".into()
            }]
        )
        .is_err());
    app.workbench.focus_editor_pane(PaneId::Primary);
    key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(
        style(&settled(&mut app), "fn f"),
        CellStyle::Syntax(SyntaxKind::Keyword)
    );
    assert_eq!(app.workbench.active_bytes().unwrap(), source.as_bytes());
    app.workbench.focus_editor_pane(PaneId::Secondary);
    assert_eq!(app.workbench.active_bytes().unwrap(), b"DISPLAY 42.\n");
}
