use super::TuiApp;
use crate::{state::LaunchOptions, CellStyle, FrameEncoder};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use jc_app::{CommandId, PaneId};
use jc_text::{TextEncoding, TextPosition};
fn key(a: &mut TuiApp, code: KeyCode, mods: KeyModifiers) {
    a.handle_event(Event::Key(KeyEvent::new(code, mods)));
}
fn editor(text: &str) -> TuiApp {
    let mut a = TuiApp::new(LaunchOptions::default());
    a.panels.hide_sidebar();
    a.workbench.new_file("unicode.p", TextEncoding::Utf8);
    a.workbench.insert_text(text).unwrap();
    a.workbench
        .go_to_position(TextPosition { line: 0, column: 0 })
        .unwrap();
    a
}
#[test]
fn unicode_tui_cluster_caret_selection_and_readonly_composition_share_visible_geometry() {
    let text = "e\u{301}界😀👨‍👩‍👧‍👦Z";
    let mut a = editor(text);
    let mut encoder = FrameEncoder::default();
    let mut bytes = Vec::new();
    for column in [0, 1, 3, 5, 7, 8] {
        let p = a.prepare_frame(80, 24);
        let screen = crate::render::render(&p);
        assert_eq!(screen, crate::render::render(&p));
        assert!(screen.row_text(3).contains(text));
        assert_eq!(screen.cursor(), Some((5 + column, 3)));
        encoder.draw_to(&mut bytes, &screen).unwrap();
        key(&mut a, KeyCode::Right, KeyModifiers::NONE);
    }
    assert!(std::str::from_utf8(&bytes).unwrap().contains("👨‍👩‍👧‍👦"));
    assert_eq!(a.workbench.active_bytes().unwrap(), text.as_bytes());
    a.workbench
        .go_to_position(TextPosition { line: 0, column: 0 })
        .unwrap();
    key(&mut a, KeyCode::Right, KeyModifiers::SHIFT);
    let screen = a.frame(80, 24);
    assert_eq!(screen.cell(5, 3).style, CellStyle::Reverse);
    assert_eq!(screen.cell(6, 3).style, CellStyle::Normal);
}
#[test]
fn unicode_tui_rectangular_wide_edit_and_undo_survive_split_and_tiny_viewport() {
    let text = "界e\u{301}Z\r\n😀aQ";
    let mut a = editor(text);
    a.workbench.split_editor_right();
    a.workbench.focus_editor_pane(PaneId::Primary);
    a.dispatch(CommandId::EditRectangleStart);
    a.workbench.extend_rectangle(1, 1).unwrap();
    let _ = a.frame(40, 10);
    key(&mut a, KeyCode::Char('x'), KeyModifiers::NONE);
    assert_eq!(
        a.workbench.active_bytes().unwrap(),
        "x e\u{301}Z\r\nx aQ".as_bytes()
    );
    key(&mut a, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(a.workbench.active_bytes().unwrap(), text.as_bytes());
    assert!(a.frame(140, 24).row_text(3).contains("界e\u{301}Z"));
}
#[test]
fn unicode_tui_palette_and_prompt_backspace_remove_one_grapheme_without_source_edit() {
    let mut a = editor("source");
    a.dispatch(CommandId::PaletteOpen);
    a.handle_event(Event::Paste("e\u{301}".into()));
    key(&mut a, KeyCode::Backspace, KeyModifiers::NONE);
    assert!(matches!(&a.overlay,crate::state::UiOverlay::Palette {input,..} if input.is_empty()));
    key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
    a.dispatch(CommandId::FileOpen);
    a.handle_event(Event::Paste("👨‍👩‍👧‍👦".into()));
    key(&mut a, KeyCode::Backspace, KeyModifiers::NONE);
    assert!(matches!(&a.overlay,crate::state::UiOverlay::Prompt {input,..} if input.is_empty()));
    assert_eq!(a.workbench.active_bytes().unwrap(), b"source");
}

#[test]
fn unicode_tui_rectangular_warm_frame_reuses_range_layout_instead_of_decoding_whole_line() {
    let mut a = editor(&"a".repeat(4096));
    a.dispatch(CommandId::EditRectangleStart);
    let tab = a.workbench.tabs().active_id().unwrap();
    a.frame(80, 24);
    let before = a.workbench.unicode_statistics(tab).unwrap();
    a.frame(80, 24);
    let after = a.workbench.unicode_statistics(tab).unwrap();
    assert!(after.0 - before.0 < 2048);
    assert_eq!(a.workbench.active_bytes().unwrap(), vec![b'a'; 4096]);
}

#[test]
fn unicode_tui_owned_source_payload_is_bounded_across_a_tall_frame() {
    let cluster = format!("e{}\n", "\u{301}".repeat(511));
    let source = cluster.repeat(2300);
    let mut a = editor(&source);
    let p = a.prepare_frame(40, 2400);
    let bytes = p
        .primary
        .rows
        .iter()
        .flatten()
        .filter(|a| !a.source.is_empty())
        .map(|a| a.glyph.byte_len())
        .sum::<usize>();
    assert!(bytes <= jc_app::MAX_PRESENTATION_TEXT_BYTES);
    assert!(p.primary.rows.iter().any(|row| {
        let mut text = String::new();
        for atom in row {
            atom.glyph.with_str(|s| text.push_str(s));
        }
        text.contains("Unicode layout")
    }));
    assert_eq!(p.app.workbench.active_bytes().unwrap(), source.as_bytes());
}
