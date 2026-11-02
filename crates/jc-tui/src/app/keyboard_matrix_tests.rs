//! Event-level gaps in the keyboard matrix. The integrator declares this test module.
use super::TuiApp;
use crate::state::{LaunchOptions, ReplaceField, UiOverlay};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use jc_app::{BottomPanel, PaneId, PrimaryFocus, SidebarView};
use jc_text::{TextEncoding, TextPosition};

fn key(app: &mut TuiApp, code: KeyCode, modifiers: KeyModifiers) {
    app.handle_event(Event::Key(KeyEvent::new(code, modifiers)));
}

fn editor(text: &str) -> TuiApp {
    let mut app = TuiApp::new(LaunchOptions::default());
    app.workbench
        .new_file("keyboard-matrix.txt", TextEncoding::Utf8);
    app.workbench.insert_text(text).unwrap();
    app.workbench
        .go_to_position(TextPosition { line: 0, column: 0 })
        .unwrap();
    app
}

fn finish_search(app: &mut TuiApp) {
    for _ in 0..100 {
        if !app.workbench.search_running() {
            return;
        }
        app.poll_tasks();
    }
    panic!("small keyboard fixture search did not finish");
}

#[test]
fn replace_tab_and_backtab_route_paste_to_fields_before_one_undoable_commit() {
    let mut app = editor("a a\r\n");
    key(&mut app, KeyCode::Char('h'), KeyModifiers::CONTROL);
    app.handle_event(Event::Paste("a".into()));
    finish_search(&mut app);
    key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
    app.handle_event(Event::Paste("b".into()));
    assert!(matches!(
        &app.overlay,
        UiOverlay::Replace { find, replacement, field: ReplaceField::Replacement }
            if find == "a" && replacement == "b"
    ));
    key(&mut app, KeyCode::BackTab, KeyModifiers::SHIFT);
    key(&mut app, KeyCode::Backspace, KeyModifiers::NONE);
    app.handle_event(Event::Paste("a".into()));
    finish_search(&mut app);
    assert!(matches!(
        &app.overlay,
        UiOverlay::Replace { find, replacement, field: ReplaceField::Find }
            if find == "a" && replacement == "b"
    ));
    assert_eq!(app.workbench.active_bytes().unwrap(), b"a a\r\n");
    key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
    key(&mut app, KeyCode::Enter, KeyModifiers::CONTROL);
    assert_eq!(app.workbench.active_bytes().unwrap(), b"b b\r\n");
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(app.workbench.active_bytes().unwrap(), b"a a\r\n");
}

#[test]
fn f6_and_escape_follow_visible_areas_and_restore_the_area_under_a_palette() {
    let mut app = editor("unchanged");
    key(&mut app, KeyCode::Char('j'), KeyModifiers::CONTROL);
    assert!(app.panels.bottom_visible());
    key(&mut app, KeyCode::F(6), KeyModifiers::NONE);
    assert_eq!(app.focus.primary(), PrimaryFocus::Sidebar);
    key(&mut app, KeyCode::F(6), KeyModifiers::NONE);
    assert_eq!(app.focus.primary(), PrimaryFocus::BottomPanel);
    key(
        &mut app,
        KeyCode::Char('p'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    key(&mut app, KeyCode::F(6), KeyModifiers::NONE);
    key(&mut app, KeyCode::F(6), KeyModifiers::SHIFT);
    assert!(matches!(app.overlay, UiOverlay::Palette { .. }));
    assert_eq!(app.focus.primary(), PrimaryFocus::BottomPanel);
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(matches!(app.overlay, UiOverlay::None));
    assert_eq!(app.focus.primary(), PrimaryFocus::BottomPanel);
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(app.focus.primary(), PrimaryFocus::Editor);
    key(&mut app, KeyCode::Char('j'), KeyModifiers::CONTROL);
    assert!(!app.panels.bottom_visible());
    key(&mut app, KeyCode::F(6), KeyModifiers::SHIFT);
    assert_eq!(app.focus.primary(), PrimaryFocus::Sidebar);
    key(&mut app, KeyCode::F(6), KeyModifiers::NONE);
    assert_eq!(app.focus.primary(), PrimaryFocus::Editor);
    assert_eq!(app.workbench.active_bytes().unwrap(), b"unchanged");
}

#[test]
fn disabled_database_command_has_no_effect_from_shortcut_palette_or_menu() {
    let mut app = editor("unchanged");
    let revision = app
        .workbench
        .active_document()
        .unwrap()
        .session
        .document()
        .revision();
    key(
        &mut app,
        KeyCode::Char('d'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    );
    assert_eq!(app.panels.sidebar(), SidebarView::Explorer);
    key(
        &mut app,
        KeyCode::Char('p'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    app.handle_event(Event::Paste("Database / Schema".into()));
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(matches!(app.overlay, UiOverlay::Palette { .. }));
    assert_eq!(app.panels.sidebar(), SidebarView::Explorer);
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    key(&mut app, KeyCode::Char('v'), KeyModifiers::ALT);
    key(&mut app, KeyCode::Down, KeyModifiers::NONE); // Explorer -> disabled Database / Schema
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(app.panels.sidebar(), SidebarView::Explorer);
    assert_eq!(
        app.workbench
            .active_document()
            .unwrap()
            .session
            .document()
            .revision(),
        revision
    );
    assert_eq!(app.workbench.active_bytes().unwrap(), b"unchanged");
}

#[test]
fn closing_a_shared_view_preserves_dirty_document_then_last_view_requires_a_choice() {
    let mut app = editor("abc\r\n");
    let id = app.workbench.active_document().unwrap().id;
    key(&mut app, KeyCode::Right, KeyModifiers::SHIFT);
    key(
        &mut app,
        KeyCode::Char('2'),
        KeyModifiers::ALT | KeyModifiers::SHIFT,
    );
    key(&mut app, KeyCode::Char('2'), KeyModifiers::CONTROL);
    key(&mut app, KeyCode::Home, KeyModifiers::NONE);
    key(&mut app, KeyCode::Char('w'), KeyModifiers::CONTROL);
    assert!(matches!(app.overlay, UiOverlay::None));
    assert_eq!(app.workbench.active_document().unwrap().id, id);
    assert!(app
        .workbench
        .active_document()
        .unwrap()
        .session
        .document()
        .is_modified());
    assert_eq!(app.workbench.panes().active(), PaneId::Primary);
    assert_eq!(
        app.workbench.active_editor().unwrap().selection(),
        Some(0..1)
    );
    key(&mut app, KeyCode::Char('w'), KeyModifiers::CONTROL);
    assert!(matches!(app.overlay, UiOverlay::CloseDialog { .. }));
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(app.workbench.active_document().unwrap().id, id);
    assert_eq!(app.workbench.active_bytes().unwrap(), b"abc\r\n");
    key(&mut app, KeyCode::Char('w'), KeyModifiers::CONTROL);
    key(&mut app, KeyCode::Char('d'), KeyModifiers::NONE);
    assert!(app.workbench.active_document().is_none());
    assert!(matches!(app.overlay, UiOverlay::None));
    assert!(!app.should_exit());
}

#[test]
fn tab_aliases_preserve_selection_and_bottom_panel_alt_one_does_not_select_a_tab() {
    let mut app = editor("first");
    let first = app.workbench.tabs().active_id().unwrap();
    let second = app.workbench.new_file("second.txt", TextEncoding::Utf8);
    app.workbench.insert_text("second").unwrap();
    app.workbench
        .go_to_position(TextPosition { line: 0, column: 0 })
        .unwrap();
    key(&mut app, KeyCode::Right, KeyModifiers::SHIFT);
    key(&mut app, KeyCode::Tab, KeyModifiers::CONTROL);
    assert_eq!(app.workbench.tabs().active_id(), Some(first));
    key(&mut app, KeyCode::BackTab, KeyModifiers::CONTROL);
    assert_eq!(app.workbench.tabs().active_id(), Some(second));
    assert_eq!(
        app.workbench.active_editor().unwrap().selection(),
        Some(0..1)
    );
    key(&mut app, KeyCode::PageUp, KeyModifiers::CONTROL);
    assert_eq!(app.workbench.tabs().active_id(), Some(first));
    key(&mut app, KeyCode::PageDown, KeyModifiers::CONTROL);
    assert_eq!(app.workbench.tabs().active_id(), Some(second));
    app.dispatch(jc_app::CommandId::ViewOutput);
    key(&mut app, KeyCode::Char('1'), KeyModifiers::ALT);
    assert_eq!(app.panels.bottom(), BottomPanel::Problems);
    assert_eq!(app.workbench.tabs().active_id(), Some(second));
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    key(&mut app, KeyCode::Char('1'), KeyModifiers::ALT);
    assert_eq!(app.workbench.tabs().active_id(), Some(first));
}

#[test]
fn enhanced_keyboard_release_does_not_repeat_an_edit_but_repeat_events_do() {
    let mut app = editor("");
    let mut event = KeyEvent::new(KeyCode::Char('é'), KeyModifiers::NONE);
    app.handle_event(Event::Key(event));
    event.kind = KeyEventKind::Release;
    app.handle_event(Event::Key(event));
    assert_eq!(app.workbench.active_bytes().unwrap(), "é".as_bytes());
    event.kind = KeyEventKind::Repeat;
    app.handle_event(Event::Key(event));
    assert_eq!(app.workbench.active_bytes().unwrap(), "éé".as_bytes());
}
