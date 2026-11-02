use super::*;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use jc_app::{CommandId, PaneId, PrimaryFocus};

fn key(app: &mut TuiApp, code: KeyCode, modifiers: KeyModifiers) {
    app.handle_event(Event::Key(KeyEvent::new(code, modifiers)));
}

#[test]
fn opening_legacy_from_prompt_keeps_encoding_choice_until_selection() {
    use crate::state::PromptKind;
    let path = std::env::temp_dir().join(format!("jc-f1-legacy-{}.p", std::process::id()));
    std::fs::write(&path, b"ni\xf1o\r\n").unwrap();
    let mut app = TuiApp::new(LaunchOptions::default());
    app.open_prompt(PromptKind::Open);
    app.submit_prompt(PromptKind::Open, path.to_str().unwrap().to_owned());
    assert!(matches!(app.overlay, UiOverlay::EncodingChoice));
    assert_eq!(app.workbench.active_encoding(), None);
    app.close_overlay();
    app.dispatch(CommandId::FileClose);
    app.dispatch(CommandId::FileReopenClosed);
    assert!(matches!(app.overlay, UiOverlay::EncodingChoice));
    app.choose_encoding(TextEncoding::Iso8859_1);
    assert!(matches!(app.overlay, UiOverlay::None));
    assert_eq!(app.workbench.active_bytes().unwrap(), b"ni\xf1o\r\n");
    app.workbench.insert_text("é").unwrap();
    assert_eq!(app.workbench.active_bytes().unwrap(), b"\xe9ni\xf1o\r\n");
    std::fs::remove_file(path).unwrap();
}

fn text(app: &mut TuiApp, value: &str) {
    for ch in value.chars() {
        key(app, KeyCode::Char(ch), KeyModifiers::NONE);
    }
}

#[test]
fn drawing_shared_views_reads_visible_ranges_without_rescanning_the_document() {
    let mut app = TuiApp::new(LaunchOptions::default());
    let tab = app.workbench.new_file("f2-render.txt", TextEncoding::Utf8);
    app.workbench
        .insert_text(&"abcdefg\r\n".repeat(100000))
        .unwrap();
    app.workbench.split_editor_right();
    app.render(140, 30);
    let before = app.workbench.text_statistics(tab).unwrap();
    let screen = app.render(140, 30);
    let after = app.workbench.text_statistics(tab).unwrap();
    assert_eq!(before.0, after.0);
    assert!(after.1 - before.1 < 65536);
    assert!((0..screen.height()).any(|row| screen.row_text(row).contains("Group 1")));
}

fn palette(app: &mut TuiApp, label: &str) {
    key(
        app,
        KeyCode::Char('p'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    text(app, label);
    key(app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(matches!(app.overlay, UiOverlay::None));
}

#[test]
fn keyboard_and_palette_drive_split_pin_history_and_responsive_rendering() {
    let mut app = TuiApp::new(LaunchOptions::default());
    let first = app.workbench.new_file("left.txt", TextEncoding::Utf8);
    text(&mut app, "left text");
    palette(&mut app, "Split Right");
    key(&mut app, KeyCode::Char('2'), KeyModifiers::CONTROL);
    assert_eq!(app.workbench.panes().active(), PaneId::Secondary);
    palette(&mut app, "Pin / Unpin Reference");
    assert!(app
        .workbench
        .panes()
        .view(PaneId::Secondary)
        .unwrap()
        .is_pinned());
    let second = app.workbench.new_file("right.txt", TextEncoding::Utf8);
    assert_eq!(app.workbench.panes().active(), PaneId::Primary);
    text(&mut app, "right text");
    let wide = app.render(140, 30);
    let headers = wide.row_text(2);
    assert!(headers.contains("Group 1: right.txt"));
    assert!(headers.contains("Group 2: left.txt [PIN]"));
    assert!(wide.row_text(3).contains("right text"));
    assert!(wide.row_text(3).contains("left text"));
    let narrow = app.render(45, 15);
    assert!(narrow.row_text(2).contains("Group 1: right.txt"));
    assert!(!narrow.row_text(2).contains("Group 2"));
    key(&mut app, KeyCode::Char('2'), KeyModifiers::CONTROL);
    let narrow = app.render(45, 15);
    assert!(narrow.row_text(2).contains("Group 2: left.txt"));
    key(&mut app, KeyCode::Char('1'), KeyModifiers::CONTROL);
    key(&mut app, KeyCode::Left, KeyModifiers::ALT);
    assert_eq!(app.workbench.tabs().active_id(), Some(first));
    key(&mut app, KeyCode::Right, KeyModifiers::ALT);
    assert_eq!(app.workbench.tabs().active_id(), Some(second));
    key(&mut app, KeyCode::F(6), KeyModifiers::NONE);
    assert_ne!(app.focus.primary(), PrimaryFocus::Editor);
    palette(&mut app, "Focus Group 2");
    assert_eq!(app.focus.primary(), PrimaryFocus::Editor);
    assert_eq!(app.workbench.panes().active(), PaneId::Secondary);
    palette(&mut app, "One Group");
    assert!(!app.workbench.panes().is_split());
    assert_eq!(app.workbench.tabs().active_id(), Some(second));
}

#[test]
fn explorer_open_to_side_works_without_an_existing_document() {
    let root = std::env::temp_dir().join(format!("jc-tui-side-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("from-explorer.txt");
    std::fs::write(&path, b"explorer").unwrap();
    let mut app = TuiApp::new(LaunchOptions::default());
    app.explorer.set_root(root.clone()).unwrap();
    app.dispatch(CommandId::ViewExplorer);
    assert!(app
        .registry
        .is_enabled(CommandId::EditorOpenToSide, &app.command_context()));
    key(&mut app, KeyCode::Enter, KeyModifiers::CONTROL);
    assert!(app.workbench.panes().is_split());
    assert_eq!(app.workbench.active_bytes().unwrap(), b"explorer");
    assert_eq!(app.focus.primary(), PrimaryFocus::Editor);
    // Opening an empty secondary must still make that group the active target.
    assert_eq!(app.workbench.panes().active(), PaneId::Secondary);
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(root).unwrap();
}

#[test]
fn view_menu_scrolls_to_the_selected_command_in_a_short_terminal() {
    let mut app = TuiApp::new(LaunchOptions::default());
    app.workbench.new_file("menu.txt", TextEncoding::Utf8);
    app.workbench
        .insert_text(&"XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX\n".repeat(12))
        .unwrap();
    app.dispatch(CommandId::MenuView);
    key(&mut app, KeyCode::Up, KeyModifiers::NONE); // wrap to Navigate Forward
    let screen = app.render(45, 10);
    assert!((1..9).any(|row| screen.row_text(row).contains("Navigate Forward")));
    // Menu interiors must be opaque even when shorter labels leave spare cells.
    for row in 2..8 {
        assert!(!screen
            .row_text(row)
            .chars()
            .skip(3)
            .take(39)
            .any(|ch| ch == 'X'));
    }
}
