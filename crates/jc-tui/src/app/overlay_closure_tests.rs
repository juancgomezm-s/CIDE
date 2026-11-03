use super::TuiApp;
use crate::state::{LaunchOptions, PromptKind, UiOverlay};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use jc_app::{CommandId, PrimaryFocus};
use jc_text::TextEncoding;

fn key(a: &mut TuiApp, code: KeyCode) {
    a.handle_event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
}
fn paste(a: &mut TuiApp, value: &str) {
    a.handle_event(Event::Paste(value.into()));
}
fn editor() -> TuiApp {
    let mut a = TuiApp::new(LaunchOptions::default());
    a.workbench.new_file("overlay.p", TextEncoding::Utf8);
    a.workbench.insert_text("customer custom cus").unwrap();
    a
}
fn field(a: &mut TuiApp) -> Vec<String> {
    a.prepare_frame(80, 24).snippet.unwrap().field.unwrap().rows
}

#[test]
fn overlay_closure_paste_follows_palette_and_menu_above_snippet_parameter() {
    let mut a = editor();
    let before = a.workbench.active_bytes().unwrap();
    a.dispatch(CommandId::EditInsertSnippet);
    paste(&mut a, "Procedure");
    key(&mut a, KeyCode::Enter);
    let parameter = field(&mut a);
    a.dispatch(CommandId::PaletteOpen);
    paste(&mut a, "Snippet Next");
    assert!(matches!(&a.overlay,UiOverlay::Palette { input, .. } if input == "Snippet Next"));
    key(&mut a, KeyCode::Esc);
    assert_eq!(field(&mut a), parameter);
    a.dispatch(CommandId::MenuEdit);
    paste(&mut a, "never_parameter_or_source");
    key(&mut a, KeyCode::Esc);
    assert_eq!(field(&mut a), parameter);
    assert_eq!(a.workbench.active_bytes().unwrap(), before);
    key(&mut a, KeyCode::Esc);
    assert!(a.snippet_ui.is_none());
}

#[test]
fn overlay_closure_palette_paste_remains_available_above_completion() {
    let mut a = editor();
    a.dispatch(CommandId::SourceComplete);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while a.tasks.outstanding() > 0 {
        a.poll_tasks();
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(a.source_popup);
    let before = a.workbench.active_bytes().unwrap();
    a.dispatch(CommandId::PaletteOpen);
    paste(&mut a, "Cancel Source");
    assert!(matches!(&a.overlay,UiOverlay::Palette { input, .. } if input == "Cancel Source"));
    key(&mut a, KeyCode::Esc);
    assert!(a.source_popup);
    paste(&mut a, "not_source");
    assert_eq!(a.workbench.active_bytes().unwrap(), before);
}

#[test]
fn overlay_closure_all_prompt_ingress_is_atomic_and_byte_bounded() {
    let mut a = editor();
    let before = a.workbench.active_bytes().unwrap();
    for kind in [
        PromptKind::New,
        PromptKind::Open,
        PromptKind::OpenFolder,
        PromptKind::SaveAs,
        PromptKind::GitExecutable,
        PromptKind::ProjectQuery,
        PromptKind::ProjectInclude,
        PromptKind::ProjectExclude,
        PromptKind::ProjectReplacement,
        PromptKind::GoToPosition,
    ] {
        a.open_prompt(kind);
        let limit = if kind == PromptKind::GoToPosition {
            64
        } else {
            4096
        };
        let full = "é".repeat(limit / 2);
        paste(&mut a, &full);
        paste(&mut a, "x");
        key(&mut a, KeyCode::Char('é'));
        assert!(matches!(&a.overlay,UiOverlay::Prompt {input, ..} if input == &full));
        a.handle_event(Event::Key(KeyEvent::new(
            KeyCode::Char('u'),
            KeyModifiers::CONTROL,
        )));
        assert!(matches!(&a.overlay,UiOverlay::Prompt {input, ..} if input.is_empty()));
        a.submit_prompt(kind, "x".repeat(limit + 1));
        assert!(a.status.contains("exceeds"));
        assert_eq!(a.workbench.active_bytes().unwrap(), before);
        assert_eq!(a.workbench.tabs().tabs().len(), 1);
    }
    a.dispatch(CommandId::PaletteOpen);
    paste(&mut a, &"é".repeat(128));
    paste(&mut a, "overflow");
    key(&mut a, KeyCode::Char('é'));
    assert!(matches!(&a.overlay,UiOverlay::Palette {input, ..} if input.len() == 256));
    a.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('u'),
        KeyModifiers::CONTROL,
    )));
    assert!(matches!(&a.overlay,UiOverlay::Palette {input, selected:0} if input.is_empty()));
}

#[test]
fn overlay_closure_every_menu_selection_is_visible_after_page_and_wrap_in_small_terminal() {
    let mut a = editor();
    let before = a.workbench.active_bytes().unwrap();
    for command in [
        CommandId::MenuFile,
        CommandId::MenuEdit,
        CommandId::MenuSearch,
        CommandId::MenuProject,
        CommandId::MenuBuild,
        CommandId::MenuTools,
        CommandId::MenuView,
        CommandId::MenuHelp,
    ] {
        a.dispatch(command);
        a.frame(40, 10);
        for code in [
            KeyCode::End,
            KeyCode::PageDown,
            KeyCode::PageUp,
            KeyCode::Home,
            KeyCode::Up,
            KeyCode::Down,
        ] {
            key(&mut a, code);
            let UiOverlay::Menu { menu, selected } = a.overlay else {
                panic!("menu closed")
            };
            let entries = a.menu_commands(menu);
            if entries.is_empty() {
                assert_eq!(selected, 0);
                key(&mut a, KeyCode::Enter);
                assert!(matches!(a.overlay, UiOverlay::Menu { .. }));
                continue;
            }
            let id = entries[selected].unwrap();
            let label = a.registry.get(id).unwrap().label;
            let screen = a.frame(40, 10);
            let prefix: String = label.chars().take(12).collect();
            assert!(
                (1..9).any(|row| screen.row_text(row).contains(&prefix)),
                "{label}"
            );
        }
        key(&mut a, KeyCode::Esc);
    }
    assert_eq!(a.workbench.active_bytes().unwrap(), before);
    assert_eq!(a.focus.primary(), PrimaryFocus::Editor);
}

#[test]
fn overlay_closure_palette_page_clamps_and_disabled_command_remains_inert() {
    let mut a = editor();
    let before = a.workbench.active_bytes().unwrap();
    a.dispatch(CommandId::PaletteOpen);
    a.frame(40, 10);
    key(&mut a, KeyCode::PageDown);
    assert!(matches!(a.overlay, UiOverlay::Palette { selected: 4, .. }));
    key(&mut a, KeyCode::PageUp);
    assert!(matches!(a.overlay, UiOverlay::Palette { selected: 0, .. }));
    key(&mut a, KeyCode::End);
    let last = a.palette_commands("").len() - 1;
    assert!(matches!(a.overlay,UiOverlay::Palette {selected,..} if selected == last));
    key(&mut a, KeyCode::PageDown);
    assert!(matches!(a.overlay,UiOverlay::Palette {selected,..} if selected == last));
    key(&mut a, KeyCode::Home);
    paste(&mut a, "Database / Schema");
    key(&mut a, KeyCode::Enter);
    assert!(matches!(a.overlay, UiOverlay::Palette { .. }));
    assert!(a.status.contains("disabled"));
    assert_eq!(a.workbench.active_bytes().unwrap(), before);
}
