use super::TuiApp;
use crate::{render, state::LaunchOptions};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use jc_app::CommandId;
use jc_text::TextEncoding;
fn key(a: &mut TuiApp, code: KeyCode, mods: KeyModifiers) {
    a.handle_event(Event::Key(KeyEvent::new(code, mods)));
}
fn plain(a: &mut TuiApp, code: KeyCode) {
    key(a, code, KeyModifiers::NONE);
}
fn paste(a: &mut TuiApp, text: &str) {
    a.handle_event(Event::Paste(text.into()));
}
fn clear(a: &mut TuiApp) {
    key(a, KeyCode::Char('u'), KeyModifiers::CONTROL);
}
fn editor(text: &str, encoding: TextEncoding) -> TuiApp {
    let mut a = TuiApp::new(LaunchOptions::default());
    a.workbench.new_file("snippets.p", encoding);
    a.workbench.insert_text(text).unwrap();
    a
}
#[test]
fn snippet_tui_insert_parameters_preview_confirmation_and_shared_undo() {
    let original = b"DISPLAY TRUE.\n";
    let mut a = editor(std::str::from_utf8(original).unwrap(), TextEncoding::Utf8);
    a.dispatch(CommandId::EditInsertSnippet);
    paste(&mut a, "Procedimiento");
    plain(&mut a, KeyCode::Enter);
    clear(&mut a);
    paste(&mut a, "hellxo");
    plain(&mut a, KeyCode::Left);
    plain(&mut a, KeyCode::Backspace);
    plain(&mut a, KeyCode::Enter);
    assert_eq!(a.workbench.active_bytes().unwrap(), original);
    let p = a.prepare_frame(80, 24);
    let screen = render::render(&p);
    assert_eq!(screen, render::render(&p));
    assert!(p
        .snippet
        .as_ref()
        .unwrap()
        .rows
        .iter()
        .any(|(row, _)| row.contains("PROCEDURE hello")));
    paste(&mut a, "not source");
    assert_eq!(a.workbench.active_bytes().unwrap(), original);
    plain(&mut a, KeyCode::F(9));
    assert!(a.snippet_ui.is_none());
    let expected = b"DISPLAY TRUE.\nPROCEDURE hello:\n    \nEND PROCEDURE.\n";
    assert_eq!(a.workbench.active_bytes().unwrap(), expected);
    key(&mut a, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(a.workbench.active_bytes().unwrap(), original);
    key(&mut a, KeyCode::Char('y'), KeyModifiers::CONTROL);
    assert_eq!(a.workbench.active_bytes().unwrap(), expected);
}
#[test]
fn snippet_tui_surround_keeps_bom_mixed_eol_terminal_newline_and_cancel() {
    let original = "\u{feff}DISPLAY \"niño\".\r\nDISPLAY TRUE.\n";
    let mut a = editor(original, TextEncoding::Utf8);
    a.workbench.select_all().unwrap();
    a.dispatch(CommandId::EditSurroundWith);
    paste(&mut a, "bloque DO");
    plain(&mut a, KeyCode::Enter);
    assert_eq!(a.workbench.active_bytes().unwrap(), original.as_bytes());
    plain(&mut a, KeyCode::Esc);
    assert!(a.snippet_ui.is_none());
    assert_eq!(a.workbench.active_bytes().unwrap(), original.as_bytes());
    a.dispatch(CommandId::EditSurroundWith);
    paste(&mut a, "bloque DO");
    plain(&mut a, KeyCode::Enter);
    key(&mut a, KeyCode::Enter, KeyModifiers::CONTROL);
    assert_eq!(
        a.workbench.active_bytes().unwrap(),
        "\u{feff}DO:\r\nDISPLAY \"niño\".\r\nDISPLAY TRUE.\r\nEND.\n".as_bytes()
    );
    a.workbench.undo().unwrap();
    assert_eq!(a.workbench.active_bytes().unwrap(), original.as_bytes());
}
#[test]
fn snippet_tui_stale_preview_never_overwrites_new_source_intent() {
    let mut a = editor("DISPLAY TRUE.", TextEncoding::Utf8);
    a.dispatch(CommandId::EditInsertSnippet);
    paste(&mut a, "Procedimiento");
    plain(&mut a, KeyCode::Enter);
    plain(&mut a, KeyCode::Enter);
    a.workbench.insert_text("changed").unwrap();
    let before = a.workbench.active_bytes().unwrap();
    let p = a.prepare_frame(80, 24);
    assert!(p
        .snippet
        .as_ref()
        .unwrap()
        .message
        .contains("La fuente cambió"));
    plain(&mut a, KeyCode::F(9));
    assert_eq!(a.workbench.active_bytes().unwrap(), before);
    assert!(a.snippet_ui.is_some());
    plain(&mut a, KeyCode::Esc);
    assert!(a.snippet_ui.is_none());
}
#[test]
fn snippet_tui_tiny_picker_multi_parameter_back_edit_validation_and_preview_scroll() {
    let mut a = editor("", TextEncoding::Windows1252);
    a.dispatch(CommandId::EditInsertSnippet);
    let _ = a.frame(40, 10);
    plain(&mut a, KeyCode::Down);
    let p = a.prepare_frame(40, 10);
    assert!(p
        .snippet
        .as_ref()
        .unwrap()
        .rows
        .iter()
        .any(|(row, selected)| *selected && row.contains("Función")));
    plain(&mut a, KeyCode::Enter);
    clear(&mut a);
    paste(&mut a, "hello");
    plain(&mut a, KeyCode::Tab);
    clear(&mut a);
    paste(&mut a, "CHARACTER");
    key(&mut a, KeyCode::BackTab, KeyModifiers::SHIFT);
    let p = a.prepare_frame(80, 24);
    assert!(p
        .snippet
        .as_ref()
        .unwrap()
        .field
        .as_ref()
        .unwrap()
        .rows
        .iter()
        .any(|r| r.contains("hello")));
    plain(&mut a, KeyCode::Tab);
    plain(&mut a, KeyCode::Enter);
    let _ = a.frame(40, 10);
    plain(&mut a, KeyCode::End);
    let last = a.prepare_frame(40, 10).snippet.unwrap().rows;
    plain(&mut a, KeyCode::Up);
    let up = a.prepare_frame(40, 10).snippet.unwrap().rows;
    assert_ne!(last, up);
    plain(&mut a, KeyCode::F(2));
    clear(&mut a);
    plain(&mut a, KeyCode::Enter);
    assert!(a.status.contains("Parámetro inválido"));
    assert!(a.workbench.active_bytes().unwrap().is_empty());
    paste(&mut a, "LOGICAL");
    plain(&mut a, KeyCode::Enter);
    plain(&mut a, KeyCode::F(9));
    assert!(std::str::from_utf8(&a.workbench.active_bytes().unwrap())
        .unwrap()
        .contains("RETURNS LOGICAL"));
    a.workbench.undo().unwrap();
    assert!(a.workbench.active_bytes().unwrap().is_empty());
}
