//! Event/menu/palette integration for the shared Join Lines / Split Line actions.
//! All documents are in memory; no filesystem fixture, Git or process is required.
use super::TuiApp;
use crate::{
    render,
    state::{LaunchOptions, UiOverlay},
};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use jc_app::{CommandId, MenuId, PaneId, PrimaryFocus};
use jc_text::{encode, TextEncoding, TextPosition};

fn key(app: &mut TuiApp, code: KeyCode, modifiers: KeyModifiers) {
    app.handle_event(Event::Key(KeyEvent::new(code, modifiers)));
}
fn editor(source: &str, encoding: TextEncoding) -> TuiApp {
    let mut app = TuiApp::new(LaunchOptions::default());
    app.panels.hide_sidebar();
    app.workbench.new_file("line-commands.txt", encoding);
    app.workbench.insert_text(source).unwrap();
    app.workbench
        .go_to_position(TextPosition { line: 0, column: 0 })
        .unwrap();
    app
}
fn revision(app: &TuiApp) -> u64 {
    app.workbench
        .active_document()
        .unwrap()
        .session
        .document()
        .revision()
}
fn palette(app: &mut TuiApp, label: &str) {
    key(
        app,
        KeyCode::Char('p'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    app.handle_event(Event::Paste(label.into()));
    key(app, KeyCode::Enter, KeyModifiers::NONE);
}
fn menu(app: &mut TuiApp, command: CommandId) {
    app.dispatch(CommandId::MenuEdit);
    let entries = app.menu_commands(MenuId::Edit);
    let target = entries
        .iter()
        .position(|entry| *entry == Some(command))
        .expect("registered Edit entry");
    for _ in 0..entries.len() {
        if matches!(app.overlay, UiOverlay::Menu { menu: MenuId::Edit, selected } if selected == target)
        {
            key(app, KeyCode::Enter, KeyModifiers::NONE);
            return;
        }
        key(app, KeyCode::Down, KeyModifiers::NONE);
    }
    panic!("Edit menu did not reach its line command");
}
fn menu_enabled(app: &TuiApp, command: CommandId) -> bool {
    app.menus
        .entries(
            MenuId::Edit,
            &app.registry,
            &app.keymap,
            &app.command_context(),
        )
        .into_iter()
        .flatten()
        .find(|entry| entry.command == command)
        .expect("Edit entry")
        .enabled
}

#[test]
fn join_shortcut_and_palette_preserve_encoding_bom_final_eol_and_one_undo() {
    for encoding in [TextEncoding::Utf8, TextEncoding::Windows1252] {
        let bom = if encoding == TextEncoding::Utf8 {
            "\u{feff}"
        } else {
            ""
        };
        let source = format!("{bom}niño\r\n\t segundo\r\nfin\r\n");
        let joined = encode(&format!("{bom}niño segundo\r\nfin\r\n"), encoding).unwrap();
        let original = encode(&source, encoding).unwrap();
        for via_palette in [false, true] {
            let mut app = editor(&source, encoding);
            assert!(menu_enabled(&app, CommandId::EditJoinLines));
            let before = revision(&app);
            if via_palette {
                palette(&mut app, "Unir líneas");
            } else {
                key(
                    &mut app,
                    KeyCode::Char('j'),
                    KeyModifiers::CONTROL | KeyModifiers::SHIFT,
                );
            }
            assert!(matches!(app.overlay, UiOverlay::None));
            assert_eq!(app.workbench.active_bytes().unwrap(), joined);
            assert_eq!(revision(&app), before + 1);
            assert_eq!(
                app.workbench.active_document().unwrap().session.encoding(),
                Some(encoding)
            );
            key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
            assert_eq!(app.workbench.active_bytes().unwrap(), original);

            // A selection wholly within one logical line is intentionally a no-op.
            app.workbench
                .go_to_position(TextPosition { line: 0, column: 0 })
                .unwrap();
            for _ in 0..3 {
                app.workbench.move_right(true).unwrap();
            }
            let before = revision(&app);
            key(
                &mut app,
                KeyCode::Char('j'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            );
            assert_eq!(revision(&app), before);
            assert_eq!(app.workbench.active_bytes().unwrap(), original);

            // Multiple selected lines join their interior separators, not EOF's
            // synthetic empty line; the last real CRLF and BOM remain bytes.
            app.workbench.select_all().unwrap();
            palette(&mut app, "Unir líneas");
            assert_eq!(
                app.workbench.active_bytes().unwrap(),
                encode(&format!("{bom}niño segundo fin\r\n"), encoding).unwrap()
            );
            key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
            assert_eq!(app.workbench.active_bytes().unwrap(), original);
            app.workbench
                .go_to_position(TextPosition { line: 2, column: 0 })
                .unwrap();
            let before = revision(&app);
            key(
                &mut app,
                KeyCode::Char('j'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            );
            assert_eq!(revision(&app), before);
            assert_eq!(app.workbench.active_bytes().unwrap(), original);
        }
    }
}

#[test]
fn split_enter_palette_and_edit_menu_share_selection_replacement_one_undo_and_views() {
    for encoding in [TextEncoding::Utf8, TextEncoding::Windows1252] {
        let bom = if encoding == TextEncoding::Utf8 {
            "\u{feff}"
        } else {
            ""
        };
        let source = format!("{bom}niño cola\r\nnext\r\n");
        let original = encode(&source, encoding).unwrap();
        let expected = encode(&format!("{bom}niño\r\ncola\r\nnext\r\n"), encoding).unwrap();
        for route in 0..3 {
            let mut app = editor(&source, encoding);
            let tab = app.workbench.active_document().unwrap().tab_id;
            assert!(app.workbench.split_editor_right());
            assert!(app.workbench.focus_editor_pane(PaneId::Primary));
            app.workbench
                .go_to_position(TextPosition { line: 0, column: 4 })
                .unwrap();
            app.workbench.move_right(true).unwrap(); // Replace exactly the space.
            assert!(menu_enabled(&app, CommandId::EditSplitLine));
            let before = revision(&app);
            match route {
                0 => key(&mut app, KeyCode::Enter, KeyModifiers::NONE),
                1 => palette(&mut app, "Dividir línea"),
                _ => menu(&mut app, CommandId::EditSplitLine),
            }
            assert!(matches!(app.overlay, UiOverlay::None));
            assert_eq!(app.workbench.active_bytes().unwrap(), expected);
            assert_eq!(revision(&app), before + 1);
            for pane in [PaneId::Primary, PaneId::Secondary] {
                assert_eq!(
                    app.workbench
                        .panes()
                        .view(pane)
                        .unwrap()
                        .location()
                        .unwrap()
                        .tab,
                    tab
                );
                assert!(app.workbench.position_for_pane(pane).is_some());
            }
            for width in [45, 140] {
                let p = app.prepare_frame(width, 15);
                let views = format!("{:?}", p.app.workbench.panes());
                let stats = p.app.workbench.text_statistics(tab);
                let first = render::render(&p);
                assert_eq!(first, render::render(&p));
                assert!(first.row_text(3).contains("niño"));
                assert!(first.row_text(4).contains("cola"));
                assert_eq!(p.layout.two_columns, width == 140);
                assert_eq!(views, format!("{:?}", p.app.workbench.panes()));
                assert_eq!(stats, p.app.workbench.text_statistics(tab));
            }
            key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
            assert_eq!(app.workbench.active_bytes().unwrap(), original);
            key(&mut app, KeyCode::Char('y'), KeyModifiers::CONTROL);
            assert_eq!(app.workbench.active_bytes().unwrap(), expected);
        }
    }
}

#[test]
fn sidebar_overlay_enter_and_global_control_j_never_fall_through_to_split_or_join() {
    let mut app = editor("left\r\nright", TextEncoding::Utf8);
    let original = app.workbench.active_bytes().unwrap();
    let before = revision(&app);
    app.dispatch(CommandId::ViewExplorer); // Empty Explorer: no root or disk activation.
    assert_eq!(app.focus.primary(), PrimaryFocus::Sidebar);
    key(
        &mut app,
        KeyCode::Char('j'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(app.workbench.active_bytes().unwrap(), original);
    assert_eq!(revision(&app), before);
    key(&mut app, KeyCode::F(6), KeyModifiers::NONE);
    assert_eq!(app.focus.primary(), PrimaryFocus::Editor);

    key(
        &mut app,
        KeyCode::Char('p'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    app.handle_event(Event::Paste("__no_matching_line_command__".into()));
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(matches!(app.overlay, UiOverlay::Palette { .. }));
    assert_eq!(revision(&app), before);
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    key(&mut app, KeyCode::Char('f'), KeyModifiers::CONTROL);
    assert!(matches!(app.overlay, UiOverlay::Find { .. }));
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(revision(&app), before);
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(!app.panels.bottom_visible());
    key(&mut app, KeyCode::Char('j'), KeyModifiers::CONTROL);
    assert!(app.panels.bottom_visible());
    assert_eq!(app.workbench.active_bytes().unwrap(), original);
    assert_eq!(revision(&app), before);
}

#[test]
fn rectangles_disable_line_actions_in_keys_menu_palette_and_direct_model() {
    let mut app = editor("left\r\nright", TextEncoding::Utf8);
    app.workbench
        .go_to_position(TextPosition { line: 0, column: 1 })
        .unwrap();
    app.workbench.start_rectangle().unwrap();
    app.workbench.extend_rectangle(2, 1).unwrap();
    let rectangle = app.workbench.rectangle_for_pane(PaneId::Primary);
    let original = app.workbench.active_bytes().unwrap();
    let before = revision(&app);
    for (command, label) in [
        (CommandId::EditJoinLines, "Unir líneas"),
        (CommandId::EditSplitLine, "Dividir línea"),
    ] {
        assert!(!app.registry.is_enabled(command, &app.command_context()));
        assert!(!menu_enabled(&app, command));
        menu(&mut app, command);
        assert!(matches!(app.overlay, UiOverlay::None));
        palette(&mut app, label);
        assert!(matches!(app.overlay, UiOverlay::Palette { .. }));
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    }
    key(
        &mut app,
        KeyCode::Char('j'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.workbench.join_lines().is_err());
    assert!(app.workbench.split_line().is_err());
    assert_eq!(app.workbench.active_bytes().unwrap(), original);
    assert_eq!(revision(&app), before);
    assert_eq!(app.workbench.rectangle_for_pane(PaneId::Primary), rectangle);
}
