use super::{clipboard::ClipboardTransport, TuiApp};
use crate::state::{LaunchOptions, PromptKind, UiOverlay};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use jc_app::{CommandId, PrimaryFocus};
use jc_text::{TextEncoding, TextPosition};
use std::{cell::RefCell, rc::Rc};
#[derive(Default)]
struct Memory {
    text: Option<String>,
    reads: Vec<usize>,
    writes: usize,
    fail_read: bool,
    fail_write: bool,
}
struct Transport(Rc<RefCell<Memory>>);
impl ClipboardTransport for Transport {
    fn read(&mut self, limit: usize) -> Result<Option<String>, String> {
        let mut m = self.0.borrow_mut();
        m.reads.push(limit);
        if m.fail_read {
            return Err("controlled read failure".into());
        }
        // Deliberately return oversize too: caller must also enforce its quota.
        Ok(m.text.clone())
    }
    fn write(&mut self, text: &str) -> Result<(), String> {
        let mut m = self.0.borrow_mut();
        m.writes += 1;
        if m.fail_write {
            return Err("controlled write failure".into());
        }
        m.text = Some(text.into());
        Ok(())
    }
}
fn editor(source: &str, encoding: TextEncoding) -> (TuiApp, Rc<RefCell<Memory>>) {
    let mut a = TuiApp::new(LaunchOptions::default());
    a.workbench.new_file("clipboard.p", encoding);
    a.workbench.insert_text(source).unwrap();
    let m = Rc::new(RefCell::new(Memory::default()));
    a.clipboard_transport = Some(Box::new(Transport(m.clone())));
    (a, m)
}
fn paste(a: &mut TuiApp) {
    a.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('v'),
        KeyModifiers::CONTROL,
    )));
}
fn key(a: &mut TuiApp, code: KeyCode) {
    a.handle_event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
}

#[test]
fn clipboard_tui_linear_copy_cut_paste_preserves_encoding_eol_and_atomic_history() {
    for enc in [
        TextEncoding::Utf8,
        TextEncoding::Windows1252,
        TextEncoding::Iso8859_1,
        TextEncoding::Ibm850,
    ] {
        let source = "niño\r\nnext\nlast";
        let (mut a, m) = editor(source, enc);
        let bytes = a.workbench.active_bytes().unwrap();
        a.workbench.select_all().unwrap();
        a.dispatch(CommandId::EditCopy);
        assert_eq!(m.borrow().text.as_deref(), Some(source));
        assert_eq!(a.workbench.active_bytes().unwrap(), bytes);
        a.dispatch(CommandId::EditCut);
        assert!(a.workbench.active_bytes().unwrap().is_empty());
        a.workbench.undo().unwrap();
        assert_eq!(a.workbench.active_bytes().unwrap(), bytes);
        a.workbench.redo().unwrap();
        paste(&mut a);
        assert_eq!(a.workbench.active_bytes().unwrap(), bytes);
        a.workbench.undo().unwrap();
        assert!(a.workbench.active_bytes().unwrap().is_empty());
        assert_eq!(m.borrow().reads, [2 * 1024 * 1024]);
        assert_eq!(m.borrow().writes, 2);
    }
}
#[test]
fn clipboard_tui_transport_failure_keeps_source_and_never_falls_back_to_old_internal_text() {
    let (mut a, m) = editor("original", TextEncoding::Utf8);
    a.workbench.select_all().unwrap();
    a.workbench.execute(CommandId::EditCopy).unwrap();
    let before = a.workbench.active_bytes().unwrap();
    m.borrow_mut().fail_write = true;
    a.dispatch(CommandId::EditCut);
    assert_eq!(a.workbench.active_bytes().unwrap(), before);
    assert!(a.status.contains("write failure"));
    m.borrow_mut().fail_read = true;
    paste(&mut a);
    assert_eq!(a.workbench.active_bytes().unwrap(), before);
    assert!(a.status.contains("read failure"));
    m.borrow_mut().fail_read = false;
    m.borrow_mut().text = None;
    paste(&mut a);
    assert_eq!(a.workbench.active_bytes().unwrap(), before);
}
#[test]
fn clipboard_tui_bad_encoding_or_oversize_paste_is_atomic_and_bom_is_retained() {
    let (mut a, m) = editor("niño\r\n", TextEncoding::Windows1252);
    let before = a.workbench.active_bytes().unwrap();
    m.borrow_mut().text = Some("😀".into());
    paste(&mut a);
    assert_eq!(a.workbench.active_bytes().unwrap(), before);
    assert!(a.status.contains("codificación"));
    m.borrow_mut().text = Some("x".repeat(2 * 1024 * 1024 + 1));
    paste(&mut a);
    assert_eq!(a.workbench.active_bytes().unwrap(), before);
    let (mut a, m) = editor("\u{feff}tail\r\n", TextEncoding::Utf8);
    a.workbench
        .go_to_position(TextPosition { line: 0, column: 0 })
        .unwrap();
    let before = a.workbench.active_bytes().unwrap();
    m.borrow_mut().text = Some("é\r\n".into());
    paste(&mut a);
    assert_eq!(
        a.workbench.active_bytes().unwrap(),
        "\u{feff}é\r\ntail\r\n".as_bytes()
    );
    a.workbench.undo().unwrap();
    assert_eq!(a.workbench.active_bytes().unwrap(), before);
}
#[test]
fn clipboard_tui_availability_render_and_disabled_areas_do_not_access_transport() {
    let (mut a, m) = editor("source", TextEncoding::Utf8);
    for _ in 0..3 {
        assert!(a
            .registry
            .is_enabled(CommandId::EditPaste, &a.command_context()));
        a.frame(80, 24);
    }
    a.dispatch(CommandId::ViewOutput);
    paste(&mut a);
    assert_eq!(a.focus.primary(), PrimaryFocus::BottomPanel);
    assert!(m.borrow().reads.is_empty());
    assert_eq!(m.borrow().writes, 0);
    assert_eq!(a.workbench.active_bytes().unwrap(), b"source");
}
#[test]
fn clipboard_tui_paste_goes_to_topmost_fields_with_destination_limits() {
    let (mut a, m) = editor("source", TextEncoding::Utf8);
    m.borrow_mut().text = Some("Procedimiento".into());
    a.dispatch(CommandId::EditInsertSnippet);
    paste(&mut a);
    key(&mut a, KeyCode::Enter);
    a.dispatch(CommandId::PaletteOpen);
    m.borrow_mut().text = Some("Envolver selección".into());
    paste(&mut a);
    assert!(matches!(&a.overlay,UiOverlay::Palette {input,..} if input=="Envolver selección"));
    key(&mut a, KeyCode::Esc);
    key(&mut a, KeyCode::Enter);
    assert!(a
        .prepare_frame(80, 24)
        .snippet
        .unwrap()
        .rows
        .iter()
        .any(|(r, _)| r.contains("PROCEDURE newProcedure:")));
    let reads = m.borrow().reads.len();
    paste(&mut a);
    assert_eq!(m.borrow().reads.len(), reads); // preview is read-only
    key(&mut a, KeyCode::Esc);
    a.open_prompt(PromptKind::Open);
    let before = a.overlay.clone();
    m.borrow_mut().text = Some("x".repeat(4097));
    paste(&mut a);
    assert!(
        matches!((&before, &a.overlay), (UiOverlay::Prompt {input: old, ..}, UiOverlay::Prompt {input, ..}) if old == input)
    );
    key(&mut a, KeyCode::Esc);
    a.dispatch(CommandId::SearchFind);
    m.borrow_mut().text = Some("exact\r\nquery".into());
    paste(&mut a);
    assert!(matches!(&a.overlay,UiOverlay::Find {input} if input=="exact\r\nquery"));
    assert_eq!(a.workbench.active_bytes().unwrap(), b"source");
    assert_eq!(&m.borrow().reads[..2], &[256, 256]);
}
#[test]
fn clipboard_tui_rectangle_keeps_internal_rows_and_never_calls_unicode_transport() {
    let (mut a, m) = editor("ab\r\ncd", TextEncoding::Utf8);
    a.workbench
        .go_to_position(TextPosition { line: 0, column: 0 })
        .unwrap();
    a.workbench.start_rectangle().unwrap();
    a.workbench.extend_rectangle(1, 1).unwrap();
    a.dispatch(CommandId::EditCopy);
    assert!(a.workbench.clipboard_is_rectangular());
    a.dispatch(CommandId::EditCut);
    assert_eq!(a.workbench.active_bytes().unwrap(), b"b\r\nd");
    a.workbench.undo().unwrap();
    assert_eq!(a.workbench.active_bytes().unwrap(), b"ab\r\ncd");
    assert_eq!(m.borrow().writes, 0);
    assert!(m.borrow().reads.is_empty());
}
#[test]
fn clipboard_tui_headless_model_retains_internal_transport_by_default() {
    let mut a = TuiApp::new(LaunchOptions::default());
    assert!(a.clipboard_transport.is_none());
    a.workbench.new_file("internal.txt", TextEncoding::Utf8);
    a.workbench.insert_text("abc").unwrap();
    a.workbench.select_all().unwrap();
    a.dispatch(CommandId::EditCut);
    paste(&mut a);
    assert_eq!(a.workbench.active_bytes().unwrap(), b"abc");
}
