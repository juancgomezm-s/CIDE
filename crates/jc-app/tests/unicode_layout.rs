use jc_app::rectangle::{LineLayout, VisualColumn};
use jc_app::{edit_selected_field, CommandId, FieldEdit, FieldSelection, PaneId, Workbench};
use jc_text::{TextEncoding, TextPosition};
fn editor(text: &str) -> Workbench {
    let mut b = Workbench::default();
    b.new_file("unicode.txt", TextEncoding::Utf8);
    b.insert_text(text).unwrap();
    b.go_to_position(TextPosition { line: 0, column: 0 })
        .unwrap();
    b
}
#[test]
fn unicode_keyboard_movement_and_delete_use_complete_clusters_bom_crlf_and_shared_undo() {
    let text = "\u{feff}e\u{301}界👨‍👩‍👧‍👦\r\nZ";
    let mut b = editor(text);
    b.split_editor_right();
    b.focus_editor_pane(PaneId::Primary);
    assert_eq!(b.active_editor().unwrap().cursor, 3);
    let starts = [3, 6, 9, 34, 36, 37];
    for expected in &starts[1..] {
        b.move_right(false).unwrap();
        assert_eq!(b.active_editor().unwrap().cursor, *expected);
    }
    for expected in starts[..starts.len() - 1].iter().rev() {
        b.move_left(false).unwrap();
        assert_eq!(b.active_editor().unwrap().cursor, *expected);
    }
    b.delete_forward().unwrap();
    assert_eq!(b.active_bytes().unwrap(), "\u{feff}界👨‍👩‍👧‍👦\r\nZ".as_bytes());
    b.undo().unwrap();
    assert_eq!(b.active_bytes().unwrap(), text.as_bytes());
    b.move_right(false).unwrap();
    b.backspace().unwrap();
    b.focus_editor_pane(PaneId::Secondary);
    b.undo().unwrap();
    assert_eq!(b.active_bytes().unwrap(), text.as_bytes());
    // Explicit scalar APIs/search may locate a scalar within a cluster. A
    // subsequent keyboard deletion still removes the complete source cluster.
    b.go_to_position(TextPosition { line: 0, column: 1 })
        .unwrap();
    b.delete_forward().unwrap();
    assert_eq!(b.active_bytes().unwrap(), "\u{feff}界👨‍👩‍👧‍👦\r\nZ".as_bytes());
}
#[test]
fn unicode_vertical_movement_uses_visual_tab4_and_wide_columns_not_scalar_count() {
    let mut b = editor("a\t界Z\r\n123456789");
    b.go_to_position(TextPosition { line: 0, column: 3 })
        .unwrap();
    assert_eq!(b.visual_column_for_pane(PaneId::Primary), Some(6));
    b.move_vertical(1, true).unwrap();
    assert_eq!(
        b.active_position(),
        Some(TextPosition { line: 1, column: 6 })
    );
    b.move_vertical(-1, false).unwrap();
    assert_eq!(
        b.active_position(),
        Some(TextPosition { line: 0, column: 3 })
    );
    let tab = b.tabs().active_id().unwrap();
    assert_eq!(b.visual_byte_at(tab, 0, 5), Some(2)); // inside wide glyph
}
#[test]
fn unicode_rectangle_full_and_partial_clusters_preserve_cell_width_and_one_undo() {
    let mut b = editor("界e\u{301}Z\r\n😀aQ");
    let original = b.active_bytes().unwrap();
    b.start_rectangle().unwrap();
    b.extend_rectangle(2, 1).unwrap();
    b.execute(CommandId::EditCut).unwrap();
    assert_eq!(b.active_bytes().unwrap(), "e\u{301}Z\r\naQ".as_bytes());
    b.undo().unwrap();
    assert_eq!(b.active_bytes().unwrap(), original);
    b.go_to_position(TextPosition { line: 0, column: 0 })
        .unwrap();
    b.start_rectangle().unwrap();
    b.extend_rectangle(1, 1).unwrap();
    b.execute(CommandId::EditCut).unwrap();
    assert_eq!(b.active_bytes().unwrap(), " e\u{301}Z\r\n aQ".as_bytes());
    b.undo().unwrap();
    assert_eq!(b.active_bytes().unwrap(), original);
    let line = LineLayout::new("界e\u{301}".as_bytes(), TextEncoding::Utf8, 10).unwrap();
    assert_eq!(
        line.copy_columns(VisualColumn(0), VisualColumn(2)).unwrap(),
        "界"
    );
    assert_eq!(
        line.copy_columns(VisualColumn(1), VisualColumn(3)).unwrap(),
        " e\u{301}"
    );
    assert_eq!(line.column(VisualColumn(1)).unwrap().atom_end, 13);
}
#[test]
fn unicode_rectangle_resegments_actual_joins_and_rejects_invalid_legacy_transaction() {
    let line = LineLayout::new("👨👩Z".as_bytes(), TextEncoding::Utf8, 0).unwrap();
    assert_eq!(line.width(), VisualColumn(5));
    // Replace second emoji with a cluster ending in ZWJ: joining suffix is
    // validated from the reconstructed bounded line, not independent widths.
    let change = line
        .replace_columns(VisualColumn(2), VisualColumn(4), "👨‍")
        .unwrap();
    assert_eq!(change.range, 4..8);
    let mut b = Workbench::default();
    b.new_file("legacy.p", TextEncoding::Windows1252);
    b.insert_text("aa\r\nbb").unwrap();
    b.go_to_position(TextPosition { line: 0, column: 0 })
        .unwrap();
    b.start_rectangle().unwrap();
    b.extend_rectangle(1, 1).unwrap();
    let before = b.active_bytes().unwrap();
    assert!(b.insert_text("界").is_err());
    assert_eq!(b.active_bytes().unwrap(), before);
}
#[test]
fn unicode_selected_fields_move_delete_clusters_and_keep_quota_failures_atomic() {
    let mut text = String::from("e\u{301}界👨‍👩‍👧‍👦\r\nZ");
    let mut state = FieldSelection::default();
    edit_selected_field(&mut text, &mut state, FieldEdit::Right, true, 4096).unwrap();
    assert_eq!(state.range(), Some(0..3));
    edit_selected_field(&mut text, &mut state, FieldEdit::Right, true, 4096).unwrap();
    assert_eq!(state.range(), Some(0..6));
    let before = (text.clone(), state);
    assert!(edit_selected_field(
        &mut text,
        &mut state,
        FieldEdit::Insert(&"x".repeat(4097)),
        false,
        4096
    )
    .is_err());
    assert_eq!((text.clone(), state), before);
    edit_selected_field(&mut text, &mut state, FieldEdit::Delete, false, 4096).unwrap();
    assert_eq!(text, "👨‍👩‍👧‍👦\r\nZ");
    edit_selected_field(&mut text, &mut state, FieldEdit::Delete, false, 4096).unwrap();
    assert_eq!(text, "\r\nZ");
}
