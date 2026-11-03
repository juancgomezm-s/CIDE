//! Event/Screen parity for the restricted TAB4 rectangle, not physical glyph widths.
use super::TuiApp;
use crate::{render, screen::CellStyle, state::LaunchOptions};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use jc_app::{CommandId, PaneId};
use jc_text::{encode, TextEncoding, TextPosition};

fn key(app: &mut TuiApp, code: KeyCode, mods: KeyModifiers) {
    app.handle_event(Event::Key(KeyEvent::new(code, mods)));
}
fn editor(text: &str, encoding: TextEncoding) -> TuiApp {
    let mut app = TuiApp::new(LaunchOptions::default());
    app.panels.hide_sidebar();
    app.workbench.new_file("rectangle-parity.txt", encoding);
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
fn palette(app: &mut TuiApp, command: CommandId) {
    let label = app.registry.get(command).unwrap().label;
    key(
        app,
        KeyCode::Char('p'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    app.handle_event(Event::Paste(label.into()));
    key(app, KeyCode::Enter, KeyModifiers::NONE);
}
fn cycle(app: &mut TuiApp, before: &[u8], after: &[u8], rev: u64) {
    assert_eq!(app.workbench.active_bytes().unwrap(), after);
    assert_ne!(revision(app), rev);
    key(app, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(app.workbench.active_bytes().unwrap(), before);
    key(app, KeyCode::Char('y'), KeyModifiers::CONTROL);
    assert_eq!(app.workbench.active_bytes().unwrap(), after);
    key(app, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(app.workbench.active_bytes().unwrap(), before);
}

#[test]
fn rectangle_commands_broadcast_partial_tab_and_virtual_preserve_four_encodings() {
    for encoding in [
        TextEncoding::Utf8,
        TextEncoding::Windows1252,
        TextEncoding::Iso8859_1,
        TextEncoding::Ibm850,
    ] {
        let bom = if encoding == TextEncoding::Utf8 {
            "\u{feff}"
        } else {
            ""
        };
        let text = format!("{bom}a\tb\r\nñ\rX");
        let mut app = editor(&text, encoding);
        at(&mut app, 0, 1);
        palette(&mut app, CommandId::EditRectangleStart);
        for _ in 0..2 {
            key(
                &mut app,
                KeyCode::Right,
                KeyModifiers::ALT | KeyModifiers::SHIFT,
            );
        }
        app.dispatch(CommandId::EditRectangleDown);
        let rect = app.workbench.rectangle_for_pane(PaneId::Primary).unwrap();
        assert_eq!((rect.left, rect.right, rect.last_line), (1, 3, 1));
        assert!(app.workbench.selection_for_pane(PaneId::Primary).is_none());
        let rev = revision(&app);
        app.handle_event(Event::Paste("Q".into()));
        assert!(app.workbench.rectangle_for_pane(PaneId::Primary).is_none());
        cycle(
            &mut app,
            &encode(&text, encoding).unwrap(),
            &encode(&format!("{bom}aQ b\r\nñQ\rX"), encoding).unwrap(),
            rev,
        );
    }
}

#[test]
fn rectangle_internal_copy_height_and_eof_failures_do_not_publish_partial_edits() {
    let mut app = editor("a\tb\r\nñ\rX", TextEncoding::Utf8);
    at(&mut app, 0, 1);
    app.dispatch(CommandId::EditRectangleStart);
    app.dispatch(CommandId::EditRectangleRight);
    app.dispatch(CommandId::EditRectangleRight);
    app.dispatch(CommandId::EditRectangleDown);
    key(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);
    app.dispatch(CommandId::EditRectangleCancel);
    at(&mut app, 2, 0);
    let rev = revision(&app);
    key(&mut app, KeyCode::Char('v'), KeyModifiers::CONTROL);
    assert_eq!(revision(&app), rev);
    assert_eq!(
        app.workbench.active_bytes().unwrap(),
        "a\tb\r\nñ\rX".as_bytes()
    );
    at(&mut app, 0, 0);
    key(&mut app, KeyCode::Char('v'), KeyModifiers::CONTROL);
    cycle(
        &mut app,
        "a\tb\r\nñ\rX".as_bytes(),
        "  a\tb\r\n  ñ\rX".as_bytes(),
        rev,
    );
    at(&mut app, 0, 0);
    app.dispatch(CommandId::EditRectangleStart);
    app.workbench.extend_rectangle(1, 2).unwrap();
    let rev = revision(&app);
    app.handle_event(Event::Paste("one\ntwo".into()));
    assert_eq!(revision(&app), rev);
    assert!(app.workbench.rectangle_for_pane(PaneId::Primary).is_some());
    assert_eq!(
        app.workbench.active_bytes().unwrap(),
        "a\tb\r\nñ\rX".as_bytes()
    );
    app.dispatch(CommandId::EditRectangleCancel);
    at(&mut app, 2, 1);
    app.workbench.start_rectangle().unwrap();
    app.workbench.extend_rectangle(8, 0).unwrap();
    // Make a zero-width virtual rectangle rather than a selection containing X.
    app.workbench.cancel_rectangle();
    app.workbench
        .set_rectangle(
            jc_app::rectangle::VisualPosition {
                line: 2,
                column: jc_app::rectangle::VisualColumn(9),
            },
            jc_app::rectangle::VisualPosition {
                line: 2,
                column: jc_app::rectangle::VisualColumn(9),
            },
        )
        .unwrap();
    key(&mut app, KeyCode::Delete, KeyModifiers::NONE);
    assert_eq!(revision(&app), rev);
}

#[test]
fn rectangle_invisible_grid_is_readonly_through_resize_split_and_cancel() {
    let mut app = editor("a\tb\r\nñ\rX", TextEncoding::Utf8);
    app.dispatch(CommandId::ViewToggleInvisibles);
    assert!(app.workbench.split_editor_right());
    app.workbench.focus_editor_pane(PaneId::Primary);
    at(&mut app, 0, 1);
    app.workbench.start_rectangle().unwrap();
    app.workbench.extend_rectangle(2, 1).unwrap();
    let rev = revision(&app);
    for width in [45, 140, 45] {
        let p = app.prepare_frame(width, 12);
        let views = format!("{:?}", p.app.workbench.panes());
        let screen = render::render(&p);
        assert_eq!(screen, render::render(&p));
        let x = p.layout.editor_x + 5;
        assert_eq!(screen.cell(x + 1, 3).ch, '>');
        assert_eq!(screen.cell(x + 2, 3).ch, '.');
        assert_eq!(screen.cell(x + 1, 3).style, CellStyle::Reverse);
        assert_eq!(screen.cell(x, 4).ch, 'ñ');
        assert_eq!(views, format!("{:?}", p.app.workbench.panes()));
    }
    assert_eq!(revision(&app), rev);
    assert!(app.workbench.invisibles_visible());
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.workbench.rectangle_for_pane(PaneId::Primary).is_none());
    let screen = app.frame(140, 12);
    assert!((0..screen.height()).any(|row| screen.row_text(row).contains("\\u{F1}")));
    assert_eq!(
        app.workbench.active_bytes().unwrap(),
        "a\tb\r\nñ\rX".as_bytes()
    );
}

#[test]
fn rectangle_rejections_and_shared_edit_invalidation_keep_source_and_single_history() {
    for text in ["e\u{301}", "中", "😀", "a\u{1b}b"] {
        let mut app = editor(text, TextEncoding::Utf8);
        let rev = revision(&app);
        app.dispatch(CommandId::EditRectangleStart);
        assert!(app.workbench.rectangle_for_pane(PaneId::Primary).is_none());
        assert_eq!(revision(&app), rev);
        assert_eq!(app.workbench.active_bytes().unwrap(), text.as_bytes());
    }
    let mut app = editor("a\tb\r\nñ", TextEncoding::Utf8);
    app.workbench.split_editor_right();
    app.workbench.focus_editor_pane(PaneId::Primary);
    app.workbench.start_rectangle().unwrap();
    let bounds = app.workbench.rectangle_for_pane(PaneId::Primary);
    assert!(app.workbench.extend_rectangle(4097, 0).is_err());
    assert_eq!(app.workbench.rectangle_for_pane(PaneId::Primary), bounds);
    key(&mut app, KeyCode::Char('f'), KeyModifiers::CONTROL);
    let rev = revision(&app);
    app.dispatch(CommandId::EditRectangleRight);
    assert_eq!(app.workbench.rectangle_for_pane(PaneId::Primary), bounds);
    assert_eq!(revision(&app), rev);
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    app.workbench.focus_editor_pane(PaneId::Secondary);
    app.workbench.insert_text("Q").unwrap();
    assert!(app.workbench.rectangle_for_pane(PaneId::Primary).is_none());
    app.workbench.undo().unwrap();
    assert_eq!(
        app.workbench.active_bytes().unwrap(),
        "a\tb\r\nñ".as_bytes()
    );
    app.workbench.start_rectangle().unwrap();
    app.workbench
        .set_encoding(TextEncoding::Windows1252)
        .unwrap();
    assert!(app
        .workbench
        .rectangle_for_pane(PaneId::Secondary)
        .is_none());
}
