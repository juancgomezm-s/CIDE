//! Observable event routing for existing generic editing operations, not ABL execution.
use super::TuiApp;
use crate::{render, state::LaunchOptions};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use jc_app::{CommandId, PaneId};
use jc_text::{encode, TextEncoding, TextPosition};

fn key(app: &mut TuiApp, code: KeyCode, mods: KeyModifiers) {
    app.handle_event(Event::Key(KeyEvent::new(code, mods)));
}
fn editor(path: &str, text: &str, encoding: TextEncoding) -> TuiApp {
    let mut app = TuiApp::new(LaunchOptions::default());
    app.panels.hide_sidebar();
    app.workbench.new_file(path, encoding);
    app.workbench.insert_text(text).unwrap();
    at(&mut app, 0, 0);
    app
}
fn at(app: &mut TuiApp, line: usize, column: usize) {
    app.workbench
        .go_to_position(TextPosition { line, column })
        .unwrap();
}
fn revision(app: &TuiApp) -> u64 {
    app.workbench
        .active_document()
        .unwrap()
        .session
        .document()
        .revision()
}
fn cycle(app: &mut TuiApp, original: &[u8], changed: &[u8], before: u64) {
    assert_eq!(app.workbench.active_bytes().unwrap(), changed);
    assert_ne!(revision(app), before);
    key(app, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(app.workbench.active_bytes().unwrap(), original);
    key(app, KeyCode::Char('y'), KeyModifiers::CONTROL);
    assert_eq!(app.workbench.active_bytes().unwrap(), changed);
    key(app, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(app.workbench.active_bytes().unwrap(), original);
}
const ENCODINGS: [TextEncoding; 4] = [
    TextEncoding::Utf8,
    TextEncoding::Windows1252,
    TextEncoding::Iso8859_1,
    TextEncoding::Ibm850,
];

#[test]
fn duplicate_selection_line_and_delete_eof_preserve_four_encodings_and_one_undo() {
    for encoding in ENCODINGS {
        let bom = if encoding == TextEncoding::Utf8 {
            "\u{feff}"
        } else {
            ""
        };
        let text = format!("{bom}niño\r\nlast");
        let original = encode(&text, encoding).unwrap();
        let mut app = editor("line-edit.txt", &text, encoding);
        at(&mut app, 0, 2);
        key(&mut app, KeyCode::Right, KeyModifiers::SHIFT);
        let before = revision(&app);
        key(&mut app, KeyCode::Char('d'), KeyModifiers::CONTROL);
        let start = encode(&format!("{bom}niñ"), encoding).unwrap().len();
        let len = encode("ñ", encoding).unwrap().len();
        assert_eq!(
            app.workbench.active_editor().unwrap().selection(),
            Some(start..start + len)
        );
        cycle(
            &mut app,
            &original,
            &encode(&format!("{bom}niñño\r\nlast"), encoding).unwrap(),
            before,
        );
        at(&mut app, 1, 2);
        let before = revision(&app);
        key(&mut app, KeyCode::Char('d'), KeyModifiers::CONTROL);
        assert_eq!(
            app.workbench.active_position(),
            Some(TextPosition { line: 2, column: 2 })
        );
        cycle(
            &mut app,
            &original,
            &encode(&format!("{bom}niño\r\nlast\r\nlast"), encoding).unwrap(),
            before,
        );
        at(&mut app, 1, 1);
        let before = revision(&app);
        key(
            &mut app,
            KeyCode::Char('d'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        );
        cycle(
            &mut app,
            &original,
            &encode(&format!("{bom}niño"), encoding).unwrap(),
            before,
        );
        at(&mut app, 0, 0);
        let before = revision(&app);
        key(&mut app, KeyCode::Up, KeyModifiers::ALT);
        assert_eq!(revision(&app), before);
        at(&mut app, 1, 0);
        key(&mut app, KeyCode::Down, KeyModifiers::ALT);
        assert_eq!(revision(&app), before);
        assert_eq!(app.workbench.active_bytes().unwrap(), original);
    }
}

#[test]
fn comment_events_use_profile_metadata_with_bom_legacy_and_selected_block_contracts() {
    for encoding in ENCODINGS {
        let bom = if encoding == TextEncoding::Utf8 {
            "\u{feff}"
        } else {
            ""
        };
        for (path, commented) in [
            ("test.p", Some("  /* niño */\r\nfin\n")),
            ("test.cs", Some("  // niño\r\nfin\n")),
            ("test.py", Some("  # niño\r\nfin\n")),
            ("test.txt", None),
        ] {
            let text = format!("{bom}  niño\r\nfin\n");
            let original = encode(&text, encoding).unwrap();
            let mut app = editor(path, &text, encoding);
            at(&mut app, 0, 2);
            let before = revision(&app);
            key(&mut app, KeyCode::Char('/'), KeyModifiers::CONTROL);
            if let Some(commented) = commented {
                cycle(
                    &mut app,
                    &original,
                    &encode(&format!("{bom}{commented}"), encoding).unwrap(),
                    before,
                );
            } else {
                assert_eq!(revision(&app), before);
                assert_eq!(app.workbench.active_bytes().unwrap(), original);
            }
            if matches!(path, "test.p" | "test.cs") {
                key(&mut app, KeyCode::Char('a'), KeyModifiers::CONTROL);
                let before = revision(&app);
                key(
                    &mut app,
                    KeyCode::Char('/'),
                    KeyModifiers::CONTROL | KeyModifiers::SHIFT,
                );
                cycle(
                    &mut app,
                    &original,
                    &encode(&format!("{bom}  /*niño\r\nfin*/\n"), encoding).unwrap(),
                    before,
                );
            } else {
                let before = revision(&app);
                key(
                    &mut app,
                    KeyCode::Char('/'),
                    KeyModifiers::CONTROL | KeyModifiers::SHIFT,
                );
                assert_eq!(revision(&app), before);
                assert_eq!(app.workbench.active_bytes().unwrap(), original);
            }
        }
    }
}

#[test]
fn move_caret_line_selection_eof_and_inactive_pinned_rebase_are_observable() {
    let text = "one\r\ntwo\nlast";
    let mut app = editor("move.p", text, TextEncoding::Utf8);
    at(&mut app, 0, 1);
    key(&mut app, KeyCode::Down, KeyModifiers::SHIFT);
    let before = revision(&app);
    key(&mut app, KeyCode::Down, KeyModifiers::ALT);
    assert!(app.workbench.active_editor().unwrap().selection().is_none());
    assert_eq!(
        app.workbench.active_position(),
        Some(TextPosition { line: 2, column: 1 })
    );
    cycle(&mut app, text.as_bytes(), b"one\r\nlast\ntwo", before);
    at(&mut app, 0, 1);
    key(&mut app, KeyCode::Down, KeyModifiers::SHIFT);
    let before = revision(&app);
    key(&mut app, KeyCode::Up, KeyModifiers::ALT);
    cycle(&mut app, text.as_bytes(), b"two\none\r\nlast", before);
    app.dispatch(CommandId::EditorSplitRight);
    app.dispatch(CommandId::EditorFocusSecondary);
    at(&mut app, 2, 1);
    key(&mut app, KeyCode::Right, KeyModifiers::SHIFT);
    app.dispatch(CommandId::EditorTogglePin);
    app.dispatch(CommandId::EditorFocusPrimary);
    at(&mut app, 0, 0);
    let document = app.workbench.active_document().unwrap().id;
    let before = revision(&app);
    key(&mut app, KeyCode::Char('d'), KeyModifiers::CONTROL);
    assert_ne!(revision(&app), before);
    assert_eq!(
        app.workbench.position_for_pane(PaneId::Secondary),
        Some(TextPosition { line: 3, column: 2 })
    );
    assert_eq!(
        app.workbench.selection_for_pane(PaneId::Secondary),
        Some(15..16)
    );
    assert!(app
        .workbench
        .panes()
        .view(PaneId::Secondary)
        .unwrap()
        .is_pinned());
    for width in [45, 140] {
        let p = app.prepare_frame(width, 24);
        let first = render::render(&p);
        assert_eq!(first, render::render(&p));
        assert_eq!(p.app.workbench.active_document().unwrap().id, document);
    }
    key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(app.workbench.active_bytes().unwrap(), text.as_bytes());
    assert_eq!(
        app.workbench.selection_for_pane(PaneId::Secondary),
        Some(10..11)
    );
    let mut eof = editor("eof.txt", "one\r\n", TextEncoding::Utf8);
    at(&mut eof, 0, 1);
    let before = revision(&eof);
    key(&mut eof, KeyCode::Down, KeyModifiers::ALT);
    cycle(&mut eof, b"one\r\n", b"\r\none", before);
    key(&mut app, KeyCode::Char('f'), KeyModifiers::CONTROL);
    let before = revision(&app);
    key(&mut app, KeyCode::Char('d'), KeyModifiers::CONTROL);
    assert_eq!(revision(&app), before);
    assert_eq!(app.workbench.active_bytes().unwrap(), text.as_bytes());
}
