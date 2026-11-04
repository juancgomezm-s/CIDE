//! Read-only Screen evidence for the explicitly restricted one-cell rectangle.
//! Physical glyph widths, general Unicode graphemes and clipboard are not inferred.
use super::TuiApp;
use crate::{render, screen::CellStyle, state::LaunchOptions};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use jc_app::{PaneId, RectangleBounds};
use jc_text::{TextEncoding, TextPosition};

fn editor(source: &str, encoding: TextEncoding) -> TuiApp {
    let mut app = TuiApp::new(LaunchOptions::default());
    app.panels.hide_sidebar();
    app.workbench.new_file("rectangle-screen.txt", encoding);
    app.workbench.insert_text(source).unwrap();
    app.workbench
        .go_to_position(TextPosition { line: 0, column: 0 })
        .unwrap();
    app
}

fn geometry(bounds: RectangleBounds) -> (usize, usize, usize, usize, usize) {
    (
        bounds.first_line,
        bounds.last_line,
        bounds.left,
        bounds.right,
        bounds.head_column,
    )
}

#[test]
fn partial_tab_latin1_punctuation_and_virtual_cells_use_visual_selection() {
    for encoding in [TextEncoding::Utf8, TextEncoding::Windows1252] {
        let mut app = editor("a\tb\r\né€\r\n", encoding);
        app.workbench
            .go_to_position(TextPosition { line: 0, column: 1 })
            .unwrap();
        app.workbench.start_rectangle().unwrap();
        app.workbench.extend_rectangle(2, 1).unwrap();
        let source = app.workbench.active_bytes().unwrap();
        for width in [45, 140] {
            let p = app.prepare_frame(width, 12);
            let bounds = p.editor(PaneId::Primary).rectangle.unwrap();
            assert_eq!(geometry(bounds), (0, 1, 1, 3, 3));
            assert_eq!(bounds.scroll_column, 0);
            let x = p.layout.editor_x + 5; // Three line-number digits + two cells.
            let tab = p.app.workbench.active_document().unwrap().tab_id;
            let views = format!("{:?}", p.app.workbench.panes());
            let stats = p.app.workbench.text_statistics(tab);
            let first = render::render(&p);
            let second = render::render(&p);
            assert_eq!(first, second);
            assert_eq!(first.cursor(), Some((x + 3, 4)));
            for row in [3, 4] {
                assert_eq!(first.cell(x, row).style, CellStyle::Normal);
                assert_eq!(first.cell(x + 1, row).style, CellStyle::Reverse);
                assert_eq!(first.cell(x + 2, row).style, CellStyle::Reverse);
                assert_eq!(first.cell(x + 3, row).style, CellStyle::Normal);
            }
            assert_eq!(first.cell(x, 3).ch, 'a');
            assert_eq!(first.cell(x + 1, 3).ch, ' ');
            assert_eq!(first.cell(x + 2, 3).ch, ' ');
            assert_eq!(first.cell(x + 3, 3).ch, ' '); // Third tab cell is unselected.
            assert_eq!(first.cell(x + 4, 3).ch, 'b');
            assert_eq!(first.cell(x, 4).ch, 'é');
            assert_eq!(first.cell(x + 1, 4).ch, '€');
            assert_eq!(first.cell(x + 2, 4).ch, ' '); // Selected beyond EOL.
            assert_eq!(views, format!("{:?}", p.app.workbench.panes()));
            assert_eq!(stats, p.app.workbench.text_statistics(tab));
            assert_eq!(p.app.workbench.active_bytes().unwrap(), source);
        }
    }
}

#[test]
fn zero_width_after_a_tab_has_visual_cursor_and_keyboard_extension_can_be_cancelled() {
    let mut app = editor("a\tb", TextEncoding::Utf8);
    app.workbench
        .go_to_position(TextPosition { line: 0, column: 2 })
        .unwrap();
    app.workbench.start_rectangle().unwrap();
    {
        let p = app.prepare_frame(45, 12);
        let x = p.layout.editor_x + 5;
        let bounds = p.editor(PaneId::Primary).rectangle.unwrap();
        assert_eq!((bounds.left, bounds.right, bounds.head_column), (4, 4, 4));
        let screen = render::render(&p);
        assert_eq!(screen.cursor(), Some((x + 4, 3)));
        assert_eq!(screen.cell(x + 4, 3).ch, 'b');
        for column in 0..8 {
            assert_eq!(screen.cell(x + column, 3).style, CellStyle::Normal);
        }
    }
    app.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Right,
        KeyModifiers::ALT | KeyModifiers::SHIFT,
    )));
    let bounds = app.workbench.rectangle_for_pane(PaneId::Primary).unwrap();
    assert_eq!((bounds.left, bounds.right, bounds.head_column), (4, 5, 5));
    let screen = app.frame(45, 12);
    assert_eq!(screen.cell(9, 3).style, CellStyle::Reverse); // text x5 + visual4.
    assert!(app.workbench.cancel_rectangle());
    assert!(app.workbench.rectangle_for_pane(PaneId::Primary).is_none());
    assert_eq!(app.frame(45, 12).cell(9, 3).style, CellStyle::Normal);
    assert_eq!(app.workbench.active_bytes().unwrap(), b"a\tb");
}

#[test]
fn resizing_keeps_virtual_cursor_visible_without_reinterpreting_visual_bounds() {
    let mut app = editor("ab\r\né", TextEncoding::Utf8);
    app.workbench
        .go_to_position(TextPosition { line: 0, column: 1 })
        .unwrap();
    app.workbench.start_rectangle().unwrap();
    app.workbench.extend_rectangle(80, 1).unwrap();
    let expected = geometry(app.workbench.rectangle_for_pane(PaneId::Primary).unwrap());
    assert_eq!(expected, (0, 1, 1, 81, 81));
    for width in [45, 140, 45] {
        let p = app.prepare_frame(width, 12);
        let bounds = p.editor(PaneId::Primary).rectangle.unwrap();
        assert_eq!(geometry(bounds), expected);
        let x = p.layout.editor_x + 5;
        let text_width = usize::from(p.layout.editor_width - 6);
        let screen = render::render(&p);
        let cell = bounds.head_column - bounds.scroll_column;
        assert!(cell < text_width);
        assert_eq!(screen.cursor(), Some((x + cell as u16, 4)));
        for target in 0..text_width {
            let selected = (bounds.left..bounds.right).contains(&(target + bounds.scroll_column));
            let style = if selected {
                CellStyle::Reverse
            } else {
                CellStyle::Normal
            };
            for row in [3, 4] {
                assert_eq!(screen.cell(x + target as u16, row).style, style);
            }
        }
        assert_eq!(screen, render::render(&p));
    }
    assert_eq!(app.workbench.active_bytes().unwrap(), "ab\r\né".as_bytes());
}

#[test]
fn split_views_keep_independent_rectangles_through_narrow_fallback() {
    let mut app = editor("a\tb\r\né\r\nlast", TextEncoding::Utf8);
    assert!(app.workbench.split_editor_right());
    assert!(app.workbench.focus_editor_pane(PaneId::Primary));
    app.workbench
        .go_to_position(TextPosition { line: 0, column: 1 })
        .unwrap();
    app.workbench.start_rectangle().unwrap();
    app.workbench.extend_rectangle(2, 1).unwrap();
    assert!(app
        .workbench
        .rectangle_for_pane(PaneId::Secondary)
        .is_none());
    assert!(app.workbench.focus_editor_pane(PaneId::Secondary));
    app.workbench
        .go_to_position(TextPosition { line: 0, column: 2 })
        .unwrap();
    app.workbench.start_rectangle().unwrap();
    app.workbench.extend_rectangle(2, 0).unwrap();
    let primary = geometry(app.workbench.rectangle_for_pane(PaneId::Primary).unwrap());
    let secondary = geometry(app.workbench.rectangle_for_pane(PaneId::Secondary).unwrap());
    assert_eq!(primary, (0, 1, 1, 3, 3));
    assert_eq!(secondary, (0, 0, 4, 6, 6));
    for width in [140, 45, 140] {
        let p = app.prepare_frame(width, 12);
        let screen = render::render(&p);
        assert_eq!(screen, render::render(&p));
        if p.layout.two_columns {
            let left = p.layout.editor_x + 5;
            let right = p.layout.editor_x + p.layout.pane_width + 1 + 5;
            assert_eq!(screen.cell(left + 1, 3).style, CellStyle::Reverse);
            assert_eq!(screen.cell(left + 4, 3).style, CellStyle::Normal);
            assert_eq!(screen.cell(right + 1, 3).style, CellStyle::Normal);
            assert_eq!(screen.cell(right + 4, 3).style, CellStyle::Reverse);
            assert_eq!(screen.cell(right + 5, 3).style, CellStyle::Reverse);
            assert_eq!(screen.cursor(), Some((right + 6, 3)));
        } else {
            assert_eq!(width, 45);
            assert_eq!(
                screen.cell(p.layout.editor_x + 9, 3).style,
                CellStyle::Reverse
            );
        }
        assert_eq!(
            geometry(p.app.workbench.rectangle_for_pane(PaneId::Primary).unwrap()),
            primary
        );
        assert_eq!(
            geometry(
                p.app
                    .workbench
                    .rectangle_for_pane(PaneId::Secondary)
                    .unwrap()
            ),
            secondary
        );
    }
    assert_eq!(
        app.workbench.active_bytes().unwrap(),
        "a\tb\r\né\r\nlast".as_bytes()
    );
}
