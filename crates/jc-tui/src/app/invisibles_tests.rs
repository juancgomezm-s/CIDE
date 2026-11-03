//! Escaped diagnostic display is presentation, never replacement source text.
use super::TuiApp;
use crate::{
    render,
    screen::CellStyle,
    state::{LaunchOptions, UiOverlay},
};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use jc_app::{CommandId, MenuId, PaneId};
use jc_text::{TextEncoding, TextPosition};

fn editor(text: &str) -> TuiApp {
    let mut app = TuiApp::new(LaunchOptions::default());
    app.panels.hide_sidebar();
    app.workbench
        .new_file("invisible-screen.txt", TextEncoding::Utf8);
    app.workbench.insert_text(text).unwrap();
    app.workbench
        .go_to_position(TextPosition { line: 0, column: 0 })
        .unwrap();
    app
}
fn display_text(cells: &[jc_app::DisplayCell]) -> String {
    cells.iter().map(|cell| cell.ch).collect()
}
fn palette_toggle(app: &mut TuiApp) {
    let label = app
        .registry
        .get(CommandId::ViewToggleInvisibles)
        .unwrap()
        .label;
    assert_eq!(
        app.palette_commands(label),
        vec![CommandId::ViewToggleInvisibles]
    );
    app.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    )));
    app.handle_event(Event::Paste(label.into()));
    app.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )));
    assert!(matches!(app.overlay, UiOverlay::None));
}

#[test]
fn view_and_palette_toggle_ascii_spans_without_changing_source_revision_or_undo() {
    let source = "\u{feff} A\té😀\u{1b}\u{7f}\r\nZ\n";
    let mut app = editor(source);
    let revision = app
        .workbench
        .active_document()
        .unwrap()
        .session
        .document()
        .revision();
    let menu = app.menus.entries(
        MenuId::View,
        &app.registry,
        &app.keymap,
        &app.command_context(),
    );
    assert!(menu
        .into_iter()
        .flatten()
        .any(|entry| entry.command == CommandId::ViewToggleInvisibles && entry.enabled));
    assert!(!app.workbench.invisibles_visible());
    app.dispatch(CommandId::ViewToggleInvisibles);
    assert!(app.workbench.invisibles_visible());
    for width in [45, 140] {
        let p = app.prepare_frame(width, 12);
        let text = p.editor(PaneId::Primary);
        assert!(text.rows.is_empty()); // One presentation route, no mirrored source rows.
        let display = text.display_rows.as_ref().unwrap();
        assert!(display
            .iter()
            .flatten()
            .all(|cell| cell.ch.is_ascii() && !cell.ch.is_control()));
        assert!(display[0].len() <= usize::from(p.layout.pane_width - 5));
        assert!(display_text(&display[0]).starts_with(r"\u{FEFF}\sA\t~~~\u{E9}"));
        if width == 140 {
            assert_eq!(
                display_text(&display[0]),
                r"\u{FEFF}\sA\t~~~\u{E9}\u{1F600}\x1B\x7F\r\n"
            );
            assert_eq!(display_text(&display[1]), r"Z\n");
        }
        let screen = render::render(&p);
        assert_eq!(screen, render::render(&p));
        assert_eq!(screen.cell(5, 3).style, CellStyle::Dim); // BOM marker.
        assert_eq!(screen.cell(15, 3).ch, 'A');
        assert_eq!(screen.cell(15, 3).style, CellStyle::Normal);
        assert_eq!(p.app.workbench.active_bytes().unwrap(), source.as_bytes());
    }
    palette_toggle(&mut app);
    assert!(!app.workbench.invisibles_visible());
    let screen = app.frame(140, 12);
    assert!(screen.row_text(3).contains('é'));
    assert!(!screen.row_text(3).contains(r"\u{E9}"));
    assert_eq!(
        app.workbench
            .active_document()
            .unwrap()
            .session
            .document()
            .revision(),
        revision
    );
    app.workbench.undo().unwrap();
    assert!(app.workbench.active_bytes().unwrap().is_empty()); // Undo is still source insertion.
}

#[test]
fn linear_byte_selection_marks_complete_tab_unicode_and_crlf_atoms() {
    let mut app = editor("A\té\r\nB");
    app.workbench
        .go_to_position(TextPosition { line: 0, column: 1 })
        .unwrap();
    app.workbench.move_right(true).unwrap(); // TAB bytes1..2, not seven display cells.
    app.dispatch(CommandId::ViewToggleInvisibles);
    {
        let p = app.prepare_frame(140, 12);
        let text = p.editor(PaneId::Primary);
        assert_eq!(text.cursor_column, Some(8));
        let cells = &text.display_rows.as_ref().unwrap()[0];
        let screen = render::render(&p);
        assert_eq!(screen.cursor(), Some((13, 3)));
        let tab: Vec<_> = cells
            .iter()
            .enumerate()
            .filter(|(_, cell)| cell.source == (1..2))
            .collect();
        assert_eq!(tab.len(), 7);
        for (column, _) in tab {
            assert_eq!(screen.cell(5 + column as u16, 3).style, CellStyle::Reverse);
        }
        for (column, cell) in cells
            .iter()
            .enumerate()
            .filter(|(_, cell)| cell.source == (2..4))
        {
            assert!(cell.marker);
            assert_eq!(screen.cell(5 + column as u16, 3).style, CellStyle::Dim);
        }
    }
    app.workbench
        .go_to_position(TextPosition { line: 0, column: 2 })
        .unwrap();
    app.workbench.move_right(true).unwrap(); // é bytes2..4.
    app.workbench.move_right(true).unwrap(); // CRLF bytes4..6 as one boundary.
    let p = app.prepare_frame(140, 12);
    let cells = &p.editor(PaneId::Primary).display_rows.as_ref().unwrap()[0];
    let screen = render::render(&p);
    let ending: Vec<_> = cells
        .iter()
        .enumerate()
        .filter(|(_, cell)| cell.source == (4..6))
        .collect();
    assert_eq!(ending.len(), 4);
    for (column, _) in ending {
        assert_eq!(screen.cell(5 + column as u16, 3).style, CellStyle::Reverse);
    }
    for (column, _) in cells
        .iter()
        .enumerate()
        .filter(|(_, cell)| cell.source == (2..4))
    {
        assert_eq!(screen.cell(5 + column as u16, 3).style, CellStyle::Reverse);
    }
    assert_eq!(screen.cursor(), Some((5, 4)));
    assert_eq!(
        p.app.workbench.active_bytes().unwrap(),
        "A\té\r\nB".as_bytes()
    );
}

#[test]
fn rectangle_invisibles_share_visual_grid_and_highlight_partial_tab_and_virtual_cells() {
    let mut app = editor("A\tB\r\né\r\n");
    app.workbench
        .go_to_position(TextPosition { line: 0, column: 1 })
        .unwrap();
    app.workbench.start_rectangle().unwrap();
    app.workbench.extend_rectangle(2, 1).unwrap(); // Source visual columns1..3.
    let original = app.workbench.rectangle_for_pane(PaneId::Primary).unwrap();
    app.dispatch(CommandId::ViewToggleInvisibles);
    let p = app.prepare_frame(140, 12);
    let text = p.editor(PaneId::Primary);
    let rect = text.rectangle.unwrap();
    assert_eq!(
        (rect.first_line, rect.last_line, rect.left, rect.right),
        (
            original.first_line,
            original.last_line,
            original.left,
            original.right
        )
    );
    assert!(text.display_rows.is_none());
    assert_eq!(
        text.rows[0].iter().take(5).map(|c| c.0).collect::<String>(),
        "A>..B"
    );
    let screen = render::render(&p);
    for row in [3, 4] {
        for column in 0..10 {
            assert_eq!(
                screen.cell(5 + column, row).style == CellStyle::Reverse,
                (1..3).contains(&column)
            );
        }
    }
    assert_eq!(
        p.app.workbench.active_bytes().unwrap(),
        "A\tB\r\né\r\n".as_bytes()
    );
    assert_eq!(screen, render::render(&p));
}

#[test]
fn horizontal_display_cursor_and_shared_views_survive_resize_with_read_only_composition() {
    let source = format!("{} END\r\nshort", "é".repeat(40));
    let mut app = editor(&source);
    let tab = app.workbench.active_document().unwrap().tab_id;
    assert!(app.workbench.split_editor_right());
    assert!(app.workbench.focus_editor_pane(PaneId::Primary));
    app.workbench
        .go_to_position(TextPosition {
            line: 0,
            column: 40,
        })
        .unwrap();
    app.dispatch(CommandId::ViewToggleInvisibles);
    for width in [45, 140, 45] {
        let p = app.prepare_frame(width, 15);
        let text = p.editor(PaneId::Primary);
        assert_eq!(text.position.unwrap().column, 40); // Source scalars remain unchanged.
        assert_eq!(text.cursor_column, Some(240));
        let scroll = p
            .app
            .workbench
            .panes()
            .view(PaneId::Primary)
            .unwrap()
            .scroll_column();
        assert!(scroll > 0);
        let columns = usize::from(p.layout.pane_width - 5);
        assert!(240 - scroll < columns);
        let views = format!("{:?}", p.app.workbench.panes());
        let stats = p.app.workbench.text_statistics(tab);
        let display = text.display_rows.clone();
        let screen = render::render(&p);
        assert_eq!(
            screen.cursor(),
            Some((p.layout.editor_x + 5 + (240 - scroll) as u16, 3))
        );
        assert_eq!(screen, render::render(&p));
        assert_eq!(views, format!("{:?}", p.app.workbench.panes()));
        assert_eq!(stats, p.app.workbench.text_statistics(tab));
        assert_eq!(display, p.editor(PaneId::Primary).display_rows);
        if width == 140 {
            assert!(p.layout.two_columns);
            assert!(p.editor(PaneId::Secondary).display_rows.is_some());
            assert_eq!(p.editor(PaneId::Secondary).position.unwrap().column, 0);
        }
        assert_eq!(p.app.workbench.active_bytes().unwrap(), source.as_bytes());
    }
}
