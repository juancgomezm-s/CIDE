use super::TuiApp;
use crate::{
    screen::Screen,
    state::{LaunchOptions, PromptKind, UiOverlay},
};
use crossterm::event::{Event, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use jc_app::{BottomPanel, CommandId, PaneId, PrimaryFocus};
use jc_text::{TextEncoding, TextPosition};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

fn app(text: &str) -> TuiApp {
    let mut app = TuiApp::new(LaunchOptions {
        files: vec![],
        encoding: Some(TextEncoding::Utf8),
    });
    app.workbench.new_file("ratón.txt", TextEncoding::Utf8);
    app.workbench.insert_text(text).unwrap();
    app.workbench
        .go_to_position(TextPosition { line: 0, column: 0 })
        .unwrap();
    app.panels.hide_sidebar();
    app
}
fn mouse(app: &mut TuiApp, kind: MouseEventKind, x: u16, y: u16, modifiers: KeyModifiers) {
    app.handle_event(Event::Mouse(MouseEvent {
        kind,
        column: x,
        row: y,
        modifiers,
    }));
}
fn click(app: &mut TuiApp, x: u16, y: u16, modifiers: KeyModifiers) {
    mouse(
        app,
        MouseEventKind::Down(MouseButton::Left),
        x,
        y,
        modifiers,
    );
    mouse(app, MouseEventKind::Up(MouseButton::Left), x, y, modifiers);
}
fn find(screen: &Screen, text: &str) -> (u16, u16) {
    for y in 0..screen.height() {
        let row = screen.row_text(y);
        if let Some(at) = row.find(text) {
            return (jc_text::grapheme::width(&row[..at]) as u16, y);
        }
    }
    panic!("visible text missing: {text}")
}
#[test]
fn mouse_scrollbars_move_the_view_without_moving_the_caret() {
    let source = format!("{}\r\n", "abcd".repeat(40)).repeat(80);
    let mut app = app(&source);
    let caret = app.workbench.active_position();
    let [vertical, horizontal] = app.prepare_frame(100, 24).primary.bars.unwrap();
    assert!(vertical.maximum > 0 && horizontal.maximum > 0);
    click(
        &mut app,
        vertical.x,
        vertical.y + vertical.length - 1,
        KeyModifiers::NONE,
    );
    assert_eq!(
        app.workbench
            .panes()
            .view(PaneId::Primary)
            .unwrap()
            .scroll_line(),
        vertical.maximum
    );
    let [vertical, horizontal] = app.prepare_frame(100, 24).primary.bars.unwrap();
    mouse(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        vertical.x,
        vertical.y + vertical.thumb_start,
        KeyModifiers::NONE,
    );
    mouse(
        &mut app,
        MouseEventKind::Drag(MouseButton::Left),
        vertical.x,
        vertical.y,
        KeyModifiers::NONE,
    );
    mouse(
        &mut app,
        MouseEventKind::Up(MouseButton::Left),
        vertical.x,
        vertical.y,
        KeyModifiers::NONE,
    );
    assert_eq!(
        app.workbench
            .panes()
            .view(PaneId::Primary)
            .unwrap()
            .scroll_line(),
        0
    );
    app.frame(100, 24);
    click(
        &mut app,
        horizontal.x + horizontal.length - 1,
        horizontal.y,
        KeyModifiers::NONE,
    );
    assert_eq!(
        app.workbench.horizontal_scroll_for_pane(PaneId::Primary),
        Some(horizontal.maximum)
    );
    assert_eq!(app.workbench.active_position(), caret);
    assert_eq!(app.workbench.active_bytes().unwrap(), source.as_bytes());
}

#[test]
fn mouse_unicode_click_shift_drag_use_graphemes_tabs_crlf_and_preserve_one_undo() {
    let mut app = app("\u{feff}Ae\u{301}\t界Z\r\nsegunda\r\nfinal");
    let bytes = app.workbench.active_bytes().unwrap();
    let revision = app
        .workbench
        .active_document()
        .unwrap()
        .session
        .document()
        .revision();
    let undo = app
        .workbench
        .active_document()
        .unwrap()
        .session
        .document()
        .undo_steps();
    app.frame(100, 24);
    click(&mut app, 10, 3, KeyModifiers::NONE); // second cell of 界
    assert_eq!(app.workbench.active_editor().unwrap().cursor, 8);
    app.frame(100, 24);
    click(&mut app, 12, 3, KeyModifiers::SHIFT);
    assert_eq!(
        app.workbench.selection_for_pane(PaneId::Primary),
        Some(8..12)
    );
    app.frame(100, 24);
    mouse(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        5,
        3,
        KeyModifiers::NONE,
    );
    mouse(
        &mut app,
        MouseEventKind::Drag(MouseButton::Left),
        8,
        4,
        KeyModifiers::NONE,
    );
    mouse(
        &mut app,
        MouseEventKind::Up(MouseButton::Left),
        8,
        4,
        KeyModifiers::NONE,
    );
    assert_eq!(
        app.workbench.selection_for_pane(PaneId::Primary),
        Some(3..17)
    );
    assert_eq!(app.workbench.active_bytes().unwrap(), bytes);
    assert_eq!(
        app.workbench
            .active_document()
            .unwrap()
            .session
            .document()
            .revision(),
        revision
    );
    assert_eq!(
        app.workbench
            .active_document()
            .unwrap()
            .session
            .document()
            .undo_steps(),
        undo
    );
    app.workbench.insert_text("Q").unwrap();
    assert_eq!(
        app.workbench
            .active_document()
            .unwrap()
            .session
            .document()
            .undo_steps(),
        undo + 1
    );
    app.workbench.undo().unwrap();
    assert_eq!(app.workbench.active_bytes().unwrap(), bytes);
}
#[test]
fn mouse_invisible_markers_snap_combining_bom_and_crlf_to_legal_source_boundaries() {
    let mut app = app("\u{feff}Ae\u{301}\tX\r\nB");
    app.dispatch(CommandId::ViewToggleInvisibles);
    let bytes = app.workbench.active_bytes().unwrap();
    let columns = {
        let prepared = app.prepare_frame(100, 24);
        prepared.primary.display_rows.as_ref().unwrap()[0].len()
    };
    for column in 0..columns {
        app.frame(100, 24);
        click(&mut app, 5 + column as u16, 3, KeyModifiers::NONE);
        let byte = app.workbench.active_editor().unwrap().cursor;
        assert!(
            [3, 4, 7, 8, 9].contains(&byte),
            "column{column} mapped to illegal grapheme/CRLF byte{byte}"
        );
    }
    assert_eq!(app.workbench.active_bytes().unwrap(), bytes);
}
#[test]
fn mouse_wheel_preserves_independent_carets_focus_pin_and_linked_vertical_scroll() {
    let mut app = app(&"fila\r\n".repeat(80));
    assert!(app.workbench.split_editor_right());
    app.workbench.focus_editor_pane(PaneId::Secondary);
    app.workbench
        .go_to_position(TextPosition { line: 2, column: 1 })
        .unwrap();
    app.workbench.toggle_reference_pin();
    app.workbench.focus_editor_pane(PaneId::Primary);
    app.workbench.toggle_scroll_link();
    let before = [PaneId::Primary, PaneId::Secondary]
        .map(|pane| app.workbench.panes().view(pane).unwrap().location());
    app.frame(140, 24);
    for _ in 0..2 {
        mouse(
            &mut app,
            MouseEventKind::ScrollDown,
            100,
            6,
            KeyModifiers::NONE,
        );
    }
    assert_eq!(app.workbench.panes().active(), PaneId::Primary);
    assert_eq!(app.focus.primary(), PrimaryFocus::Editor);
    for pane in [PaneId::Primary, PaneId::Secondary] {
        assert_eq!(app.workbench.panes().view(pane).unwrap().scroll_line(), 6);
    }
    assert!(app
        .workbench
        .panes()
        .view(PaneId::Secondary)
        .unwrap()
        .is_pinned());
    assert_eq!(
        [PaneId::Primary, PaneId::Secondary].map(|pane| app
            .workbench
            .panes()
            .view(pane)
            .unwrap()
            .location()),
        before
    );
    app.frame(140, 24);
    mouse(
        &mut app,
        MouseEventKind::ScrollDown,
        100,
        6,
        KeyModifiers::CONTROL,
    );
    assert_eq!(
        app.workbench
            .panes()
            .view(PaneId::Secondary)
            .unwrap()
            .scroll_line(),
        6
    );
}
#[test]
fn mouse_drag_cancels_on_resize_focus_overlay_revision_root_and_view_changes() {
    for change in 0..6 {
        let mut app = app("abc\r\ndef\r\nghi");
        app.frame(100, 24);
        mouse(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            6,
            3,
            KeyModifiers::NONE,
        );
        match change {
            0 => app.handle_event(Event::Resize(80, 12)),
            1 => app.handle_event(Event::FocusLost),
            2 => app.dispatch(CommandId::PaletteOpen),
            3 => {
                app.workbench.insert_text("R").unwrap();
            }
            4 => app.project_root = Some(PathBuf::from("unrelated-root")),
            _ => {
                app.workbench.new_file("another.txt", TextEncoding::Utf8);
            }
        }
        let bytes = app.workbench.active_bytes().unwrap();
        let cursor = app.workbench.active_editor().unwrap().cursor;
        mouse(
            &mut app,
            MouseEventKind::Drag(MouseButton::Left),
            8,
            4,
            KeyModifiers::NONE,
        );
        mouse(
            &mut app,
            MouseEventKind::Up(MouseButton::Left),
            8,
            4,
            KeyModifiers::NONE,
        );
        assert_eq!(
            app.workbench.active_bytes().unwrap(),
            bytes,
            "change{change}"
        );
        assert_eq!(
            app.workbench.active_editor().unwrap().cursor,
            cursor,
            "change{change}"
        );
    }
    let mut app = app("one\r\ntwo");
    app.frame(100, 24);
    app.workbench.insert_text("new").unwrap();
    let cursor = app.workbench.active_editor().unwrap().cursor;
    click(&mut app, 8, 4, KeyModifiers::NONE);
    assert_eq!(
        app.workbench.active_editor().unwrap().cursor,
        cursor,
        "a stale prepared click must not select a changed source"
    );
}
#[test]
fn mouse_menus_tabs_and_search_panel_use_visible_targets_without_source_edits() {
    let mut app = app("a a\r\n");
    let first = app.workbench.tabs().active_id().unwrap();
    app.workbench.new_file("界.rs", TextEncoding::Utf8);
    app.workbench.insert_text("second").unwrap();
    let screen = app.frame(100, 24);
    let (x, y) = find(&screen, "Ver");
    click(&mut app, x, y, KeyModifiers::NONE);
    assert!(matches!(app.overlay, UiOverlay::Menu { .. }));
    let screen = app.frame(100, 24);
    let (x, y) = find(&screen, "invisibles");
    click(&mut app, x, y, KeyModifiers::NONE);
    assert!(app.workbench.invisibles_visible());
    assert!(matches!(app.overlay, UiOverlay::None));
    app.frame(100, 24);
    click(&mut app, 2, 1, KeyModifiers::NONE);
    assert_eq!(app.workbench.tabs().active_id(), Some(first));
    let bytes = app.workbench.active_bytes().unwrap();
    app.workbench.set_search_query("a").unwrap();
    while app.workbench.search_running() {
        app.workbench.poll_document_search();
    }
    app.panels.show_bottom(BottomPanel::SearchResults);
    let expected = app.workbench.search().matches[1].range.clone();
    let screen = app.frame(100, 24);
    let (_, title_row) = find(&screen, "Resultados de búsqueda");
    click(&mut app, 3, title_row + 2, KeyModifiers::NONE);
    assert_eq!(
        app.workbench.selection_for_pane(PaneId::Primary),
        Some(expected)
    );
    assert_eq!(app.workbench.active_bytes().unwrap(), bytes);
    assert_eq!(app.focus.primary(), PrimaryFocus::Editor);
}
#[test]
fn mouse_browser_buttons_path_and_explorer_click_preserve_native_paths_and_bytes() {
    let fixture = super::file_browser_tests::Fixture::new();
    let path = fixture.put("carpeta/niño.md", b"mouse source\r\n");
    let (mut app, wake) = fixture.app();
    app.open_prompt(PromptKind::OpenFolder);
    super::file_browser_tests::wait(&mut app, &wake);
    let screen = app.frame(100, 30);
    let (x, y) = find(&screen, "[+] carpeta");
    click(&mut app, x + 5, y, KeyModifiers::NONE);
    let screen = app.frame(100, 30);
    let (x, y) = find(&screen, "[Entrar]");
    click(&mut app, x + 1, y, KeyModifiers::NONE);
    super::file_browser_tests::wait(&mut app, &wake);
    assert_eq!(app.file_browser.catalog.root(), path.parent());
    let screen = app.frame(100, 30);
    let (x, y) = find(&screen, "Ruta:");
    click(&mut app, x + 7, y, KeyModifiers::NONE);
    assert!(app.file_browser.path_focused);
    let UiOverlay::Prompt { input, .. } = &app.overlay else {
        panic!("browser")
    };
    assert!(input.is_char_boundary(app.file_browser.field.cursor));
    let screen = app.frame(100, 30);
    let (x, y) = find(&screen, "[Elegir carpeta actual]");
    click(&mut app, x + 1, y, KeyModifiers::NONE);
    assert_eq!(app.project_root.as_deref(), path.parent());
    assert!(matches!(app.overlay, UiOverlay::None));
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.explorer.nodes().len() < 2 {
        let ticket = wake.ticket();
        app.poll_tasks();
        assert!(Instant::now() < deadline);
        wake.wait(ticket, Some(Duration::from_millis(10)));
    }
    let screen = app.frame(100, 30);
    // The sidebar keeps its diagnostic label for opaque native names.
    let (x, y) = find(&screen, "ni");
    click(&mut app, x, y, KeyModifiers::NONE);
    assert_eq!(app.workbench.active_path(), Some(path.as_path()));
    assert_eq!(app.workbench.active_bytes().unwrap(), b"mouse source\r\n");
    assert_eq!(std::fs::read(path).unwrap(), b"mouse source\r\n");
    app.open_prompt(PromptKind::Open);
    super::file_browser_tests::wait(&mut app, &wake);
    let screen = app.frame(100, 30);
    let (x, y) = find(&screen, "[Cancelar]");
    click(&mut app, x, y, KeyModifiers::NONE);
    assert!(matches!(app.overlay, UiOverlay::None));
    assert_eq!(app.focus.primary(), PrimaryFocus::Editor);
}
