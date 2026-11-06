use jc_app::{PaneId, Workbench};
use jc_text::TextEncoding;

fn app(text: &str, encoding: TextEncoding) -> Workbench {
    let mut app = Workbench::default();
    app.new_file("seleccion.rs", encoding);
    app.insert_text(text).unwrap();
    app
}
fn selection(app: &Workbench, pane: PaneId) -> (usize, Option<usize>) {
    let state = app.panes().view(pane).unwrap().state().unwrap();
    (state.cursor, state.anchor)
}

#[test]
fn undo_redo_restore_oriented_selection_in_utf8_bom_crlf_and_legacy() {
    for (text, encoding, start, end) in [
        ("abc\r\ndef", TextEncoding::Utf8, 0, 3),
        ("\u{feff}niño\r\n", TextEncoding::Utf8, 3, 8),
        ("niño\r\n", TextEncoding::Windows1252, 0, 4),
        ("e\u{301}界\r\n", TextEncoding::Utf8, 0, 6),
    ] {
        for reverse in [false, true] {
            let mut app = app(text, encoding);
            let original = app.active_bytes().unwrap();
            let (anchor, head) = if reverse { (end, start) } else { (start, end) };
            app.set_linear_selection_bytes(anchor, head).unwrap();
            let before = selection(&app, PaneId::Primary);
            app.insert_text("x").unwrap();
            let changed = app.active_bytes().unwrap();
            let after = selection(&app, PaneId::Primary);
            for _ in 0..2 {
                app.undo().unwrap();
                assert_eq!(app.active_bytes().unwrap(), original);
                assert_eq!(selection(&app, PaneId::Primary), before);
                app.redo().unwrap();
                assert_eq!(app.active_bytes().unwrap(), changed);
                assert_eq!(selection(&app, PaneId::Primary), after);
            }
        }
    }
}

#[test]
fn undo_from_another_view_restores_only_the_initiator_without_changing_focus() {
    let mut app = app("abcdef\r\n", TextEncoding::Utf8);
    app.split_editor_right();
    app.focus_editor_pane(PaneId::Primary);
    let origin = app.panes().view(PaneId::Primary).unwrap().view_id();
    app.set_linear_selection_bytes(4, 1).unwrap();
    let before = selection(&app, PaneId::Primary);
    app.insert_text("X").unwrap();
    let after = selection(&app, PaneId::Primary);
    app.focus_editor_pane(PaneId::Secondary);
    app.set_linear_selection_bytes(0, 0).unwrap();
    app.undo().unwrap();
    assert_eq!(app.panes().active(), PaneId::Secondary);
    assert_eq!(app.panes().view(PaneId::Primary).unwrap().view_id(), origin);
    assert_eq!(selection(&app, PaneId::Primary), before);
    assert_eq!(selection(&app, PaneId::Secondary).0, 0);
    app.redo().unwrap();
    assert_eq!(selection(&app, PaneId::Primary), after);
    assert_eq!(app.panes().active(), PaneId::Secondary);
}

#[test]
fn deleted_origin_view_does_not_transfer_historical_selection_to_another_view() {
    let mut app = app("abcdef", TextEncoding::Utf8);
    app.split_editor_right();
    app.focus_editor_pane(PaneId::Primary);
    app.set_linear_selection_bytes(4, 1).unwrap();
    app.insert_text("X").unwrap();
    app.close_active(false).unwrap();
    app.focus_editor_pane(PaneId::Secondary);
    app.set_linear_selection_bytes(0, 0).unwrap();
    app.undo().unwrap();
    assert_eq!(app.active_bytes().unwrap(), b"abcdef");
    assert_eq!(selection(&app, PaneId::Secondary).0, 0);
    assert_ne!(selection(&app, PaneId::Secondary), (1, Some(4)));
}

#[test]
fn noop_rejected_edit_and_new_branch_keep_selection_history_consistent() {
    let mut app = app("abc", TextEncoding::Windows1252);
    app.set_linear_selection_bytes(3, 0).unwrap();
    app.insert_text("x").unwrap();
    app.undo().unwrap();
    let before = selection(&app, PaneId::Primary);
    assert!(app.insert_text("界").is_err());
    assert_eq!(selection(&app, PaneId::Primary), before);
    assert!(app.active_document().unwrap().session.document().can_redo());
    app.redo().unwrap();
    assert_eq!(app.active_bytes().unwrap(), b"x");
    app.undo().unwrap();
    app.insert_text("abc").unwrap(); // Exact same replacement keeps redo.
    assert!(app.active_document().unwrap().session.document().can_redo());
    app.set_linear_selection_bytes(2, 1).unwrap();
    app.insert_text("Y").unwrap();
    assert!(!app.active_document().unwrap().session.document().can_redo());
    app.undo().unwrap();
    assert_eq!(selection(&app, PaneId::Primary), (1, Some(2)));
    app.redo().unwrap();
    assert_eq!(app.active_bytes().unwrap(), b"aYc");
}

#[test]
fn line_transforms_and_external_cut_capture_the_complete_logical_command() {
    type Edit = fn(&mut Workbench) -> Result<(), jc_app::WorkbenchError>;
    for command in [
        Workbench::indent_lines as Edit,
        Workbench::outdent_lines,
        Workbench::duplicate_selection_or_line,
        Workbench::toggle_line_comment,
        Workbench::toggle_block_comment,
        Workbench::delete_line,
        Workbench::join_lines,
    ] {
        let mut app = app("    alpha\r\n    beta\r\n", TextEncoding::Utf8);
        app.set_linear_selection_bytes(19, 4).unwrap();
        let original = app.active_bytes().unwrap();
        let before = selection(&app, PaneId::Primary);
        command(&mut app).unwrap();
        let changed = app.active_bytes().unwrap();
        let after = selection(&app, PaneId::Primary);
        assert_ne!(changed, original);
        app.undo().unwrap();
        assert_eq!(app.active_bytes().unwrap(), original);
        assert_eq!(selection(&app, PaneId::Primary), before);
        app.redo().unwrap();
        assert_eq!(app.active_bytes().unwrap(), changed);
        assert_eq!(selection(&app, PaneId::Primary), after);
    }
    let mut app = app("abc", TextEncoding::Utf8);
    app.set_linear_selection_bytes(3, 0).unwrap();
    let export = app.export_selection(100).unwrap().unwrap();
    app.delete_exported_selection(export).unwrap();
    app.undo().unwrap();
    assert_eq!(selection(&app, PaneId::Primary), (0, Some(3)));
}

#[test]
fn rectangle_geometry_survives_undo_redo_as_one_content_transaction() {
    use jc_app::rectangle::{VisualColumn, VisualPosition};
    let mut app = app("abcd\r\nefgh\r\n", TextEncoding::Utf8);
    app.set_rectangle(
        VisualPosition {
            line: 1,
            column: VisualColumn(3),
        },
        VisualPosition {
            line: 0,
            column: VisualColumn(1),
        },
    )
    .unwrap();
    let before = app.rectangle_for_pane(PaneId::Primary).unwrap();
    let original = app.active_bytes().unwrap();
    app.insert_text("X").unwrap();
    let changed = app.active_bytes().unwrap();
    let after = app.rectangle_for_pane(PaneId::Primary);
    app.undo().unwrap();
    assert_eq!(app.active_bytes().unwrap(), original);
    let restored = app.rectangle_for_pane(PaneId::Primary).unwrap();
    assert_eq!(
        (
            restored.first_line,
            restored.last_line,
            restored.left,
            restored.right,
            restored.head_column
        ),
        (
            before.first_line,
            before.last_line,
            before.left,
            before.right,
            before.head_column
        )
    );
    app.redo().unwrap();
    assert_eq!(app.active_bytes().unwrap(), changed);
    assert_eq!(
        app.rectangle_for_pane(PaneId::Primary)
            .map(|r| (r.left, r.right)),
        after.map(|r| (r.left, r.right))
    );
}

#[test]
fn changing_codec_does_not_restore_stale_geometry_and_zero_budget_releases_metadata() {
    let mut app = app("abc", TextEncoding::Utf8);
    app.set_linear_selection_bytes(3, 0).unwrap();
    app.insert_text("X").unwrap();
    app.set_encoding(TextEncoding::Iso8859_1).unwrap();
    app.undo().unwrap();
    assert_eq!(app.active_bytes().unwrap(), b"abc");
    assert_eq!(app.active_editor().unwrap().anchor, None);
    let id = app.active_document().unwrap().id;
    assert!(app.active_document().unwrap().selection_history_bytes() > 0);
    app.set_history_budget(id, 0).unwrap();
    assert_eq!(app.active_document().unwrap().selection_history_bytes(), 0);
    app.insert_text("Y").unwrap();
    assert_eq!(app.active_document().unwrap().selection_history_bytes(), 0);
    assert!(!app.active_document().unwrap().session.document().can_undo());
}

#[test]
fn typing_after_collapsed_shift_selection_does_not_select_or_replace_new_characters() {
    for text in ["", "abc\r\n"] {
        let mut app = app(text, TextEncoding::Utf8);
        app.move_home(true).unwrap(); // At the empty last line: anchor == cursor.
        let before = app.active_bytes().unwrap();
        for character in ["X", "Y", "Z"] {
            app.insert_text(character).unwrap();
            assert_eq!(app.active_editor().unwrap().anchor, None);
        }
        assert_eq!(
            app.active_bytes().unwrap(),
            [before.as_slice(), b"XYZ"].concat()
        );
        for _ in 0..3 {
            app.undo().unwrap();
        }
        assert_eq!(app.active_bytes().unwrap(), before);
    }
}

#[test]
fn document_boundary_navigation_preserves_source_history_bom_and_selection_orientation() {
    use jc_app::CommandId;
    let mut app = app("\u{feff}a\r\nb\r\n", TextEncoding::Utf8);
    let original = app.active_bytes().unwrap();
    let revision = app.active_document().unwrap().session.document().revision();
    let history = app
        .active_document()
        .unwrap()
        .session
        .document()
        .undo_steps();
    app.execute(CommandId::EditDocumentStart).unwrap();
    assert_eq!(selection(&app, PaneId::Primary).0, 3);
    app.execute(CommandId::EditSelectDocumentEnd).unwrap();
    assert_eq!(selection(&app, PaneId::Primary), (original.len(), Some(3)));
    app.execute(CommandId::EditDocumentEnd).unwrap();
    app.execute(CommandId::EditSelectDocumentStart).unwrap();
    assert_eq!(selection(&app, PaneId::Primary), (3, Some(original.len())));
    assert_eq!(app.active_bytes().unwrap(), original);
    let source = app.active_document().unwrap().session.document();
    assert_eq!(
        (source.revision(), source.undo_steps()),
        (revision, history)
    );
}
