//! Combined frontend regression for source-edit gates shared with the GUI.
use super::TuiApp;
use crate::state::{LaunchOptions, UiOverlay};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use jc_app::{CommandId, PrimaryFocus, SidebarView};
use jc_text::{encode, TextEncoding, TextPosition};

fn key(app: &mut TuiApp, code: KeyCode, modifiers: KeyModifiers) {
    app.handle_event(Event::Key(KeyEvent::new(code, modifiers)));
}
const EDITS: [CommandId; 4] = [
    CommandId::EditIndent,
    CommandId::EditOutdent,
    CommandId::EditJoinLines,
    CommandId::EditSplitLine,
];

#[test]
fn edit_focus_parity_preserves_modal_source_then_editor_undo_in_four_encodings() {
    for encoding in [
        TextEncoding::Utf8,
        TextEncoding::Windows1252,
        TextEncoding::Iso8859_1,
        TextEncoding::Ibm850,
    ] {
        let mut app = TuiApp::new(LaunchOptions::default());
        app.workbench.new_file("edit-focus-parity.txt", encoding);
        let original = encode("ñ\r\nlast", encoding).unwrap();
        app.workbench.insert_text("ñ\r\nlast").unwrap();
        app.workbench
            .go_to_position(TextPosition { line: 0, column: 0 })
            .unwrap();
        app.dispatch(CommandId::SearchFind);
        app.handle_event(Event::Paste("draft".into()));
        let rev = app
            .workbench
            .active_document()
            .unwrap()
            .session
            .document()
            .revision();
        let views = format!("{:?}", app.workbench.panes());
        for command in EDITS {
            app.dispatch(command);
            assert!(matches!(&app.overlay, UiOverlay::Find { input } if input == "draft"));
            assert_eq!(app.workbench.active_bytes().unwrap(), original);
            assert_eq!(format!("{:?}", app.workbench.panes()), views);
            assert_eq!(
                app.workbench
                    .active_document()
                    .unwrap()
                    .session
                    .document()
                    .revision(),
                rev
            );
        }
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        app.panels.show_sidebar(SidebarView::Explorer);
        assert!(app.focus.focus(PrimaryFocus::Sidebar, &app.panels));
        for command in EDITS {
            app.dispatch(command);
        }
        assert_eq!(app.workbench.active_bytes().unwrap(), original);
        assert_eq!(
            app.workbench
                .active_document()
                .unwrap()
                .session
                .document()
                .revision(),
            rev
        );
        assert!(app.focus.focus(PrimaryFocus::Editor, &app.panels));
        key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(
            app.workbench.active_bytes().unwrap(),
            encode("    ñ\r\nlast", encoding).unwrap()
        );
        key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(app.workbench.active_bytes().unwrap(), original);
        app.workbench
            .go_to_position(TextPosition { line: 0, column: 0 })
            .unwrap();
        key(
            &mut app,
            KeyCode::Char('j'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        );
        assert_eq!(
            app.workbench.active_bytes().unwrap(),
            encode("ñ last", encoding).unwrap()
        );
        key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(app.workbench.active_bytes().unwrap(), original);
    }
}
