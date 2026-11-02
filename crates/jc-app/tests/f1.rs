use jc_app::{PaneId, TextReplacement, Workbench};
use jc_text::TextEncoding;

fn app(text: &str) -> Workbench {
    let mut app = Workbench::default();
    app.new_file("f1-memory.txt", TextEncoding::Utf8);
    app.insert_text(text).unwrap();
    app
}
fn edit(range: std::ops::Range<usize>, text: &str) -> TextReplacement {
    TextReplacement {
        range,
        text: text.to_owned(),
    }
}

#[test]
fn closing_empty_primary_group_does_not_leave_document_without_active_view() {
    let mut app = app("shared");
    app.split_editor_right();
    app.close_active(false).unwrap();
    assert_eq!(app.panes().active(), PaneId::Secondary);
    assert!(app.close_split_editor());
    app.insert_text("!").unwrap();
    assert_eq!(app.active_bytes().unwrap(), b"!shared");
}

#[test]
fn selecting_bom_document_in_another_group_starts_after_bom() {
    let path = std::env::temp_dir().join(format!("jc-f1-bom-{}.txt", std::process::id()));
    std::fs::write(&path, b"\xef\xbb\xbfabc").unwrap();
    let mut app = Workbench::default();
    app.open_file(&path, None).unwrap();
    app.new_file("second-memory.txt", TextEncoding::Utf8);
    app.split_editor_right();
    app.focus_editor_pane(PaneId::Secondary);
    app.select_tab(1);
    assert_eq!(app.active_editor().unwrap().cursor, 3);
    app.insert_text("x").unwrap();
    assert_eq!(app.active_bytes().unwrap(), b"\xef\xbb\xbfxabc");
    let mut reinterpreted = Workbench::default();
    reinterpreted
        .open_file(&path, Some(TextEncoding::Iso8859_1))
        .unwrap();
    reinterpreted.split_editor_right();
    reinterpreted.set_encoding(TextEncoding::Utf8).unwrap();
    assert_eq!(reinterpreted.active_editor().unwrap().cursor, 3);
    assert_eq!(
        reinterpreted
            .panes()
            .view(PaneId::Secondary)
            .unwrap()
            .location()
            .unwrap()
            .byte_offset,
        3
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn replace_all_reverts_once_and_rebases_every_view() {
    let mut app = app("a a end");
    let tab = app.tabs().active_id().unwrap();
    app.split_editor_right();
    let first = app.panes().view(PaneId::Primary).unwrap().view_id();
    let second = app.panes().view(PaneId::Secondary).unwrap().view_id();
    assert_ne!(first, second);
    app.jump_to_location(tab, 0);
    app.set_search_query("a").unwrap();
    app.replace_all("long").unwrap();
    assert_eq!(app.active_bytes().unwrap(), b"long long end");
    assert_eq!(
        app.panes()
            .view(PaneId::Secondary)
            .unwrap()
            .location()
            .unwrap()
            .byte_offset,
        13
    );
    app.undo().unwrap();
    assert_eq!(app.active_bytes().unwrap(), b"a a end");
    assert_eq!(
        app.panes()
            .view(PaneId::Secondary)
            .unwrap()
            .location()
            .unwrap()
            .byte_offset,
        7
    );
    app.redo().unwrap();
    assert_eq!(app.active_bytes().unwrap(), b"long long end");
}

#[test]
fn failed_or_stale_text_batch_preserves_content_selection_and_history() {
    let mut app = app("aé\r\nb");
    app.move_left(true).unwrap();
    let doc = app.active_document().unwrap();
    let (id, revision, undos) = (
        doc.id,
        doc.session.document().revision(),
        doc.session.document().undo_steps(),
    );
    let selection = app.active_editor().unwrap().selection();
    for edits in [
        vec![edit(0..1, "x"), edit(2..3, "y")],
        vec![edit(0..1, "x"), edit(4..5, "y")],
        vec![edit(0..2, "x"), edit(1..3, "y")],
    ] {
        assert!(app.apply_text_edits(id, revision, edits).is_err());
        assert_eq!(app.active_bytes().unwrap(), "aé\r\nb".as_bytes());
        assert_eq!(app.active_editor().unwrap().selection(), selection);
        assert_eq!(
            app.active_document()
                .unwrap()
                .session
                .document()
                .undo_steps(),
            undos
        );
    }
    assert!(app
        .apply_text_edits(id, revision - 1, vec![edit(0..1, "z")])
        .is_err());
}

#[test]
fn crlf_navigation_delete_and_jump_never_leave_an_internal_caret() {
    let mut app = app("a\r\nb");
    let tab = app.tabs().active_id().unwrap();
    app.jump_to_location(tab, 0);
    app.move_right(false).unwrap();
    app.move_right(false).unwrap();
    assert_eq!(app.active_editor().unwrap().cursor, 3);
    assert!(app.active_position().is_some());
    assert!(!app.jump_to_location(tab, 2));
    app.backspace().unwrap();
    assert_eq!(app.active_bytes().unwrap(), b"ab");
    app.undo().unwrap();
    assert_eq!(app.active_bytes().unwrap(), b"a\r\nb");
    app.jump_to_location(tab, 1);
    app.delete_forward().unwrap();
    assert_eq!(app.active_bytes().unwrap(), b"ab");
}

#[test]
fn saved_view_selection_and_scroll_survive_switching_documents_in_each_group() {
    let mut app = app("abcdef\nsecond\nthird");
    let first = app.tabs().active_id().unwrap();
    app.jump_to_location(first, 2);
    app.move_right(true).unwrap();
    app.reveal_active_pane(1, 3);
    let scroll = app.active_editor().unwrap().scroll_line;
    let view = app.panes().view(PaneId::Primary).unwrap().view_id();
    app.new_file("another-memory.txt", TextEncoding::Utf8);
    app.select_tab(1);
    assert_eq!(app.panes().view(PaneId::Primary).unwrap().view_id(), view);
    assert_eq!(app.active_editor().unwrap().selection(), Some(2..3));
    assert_eq!(app.active_editor().unwrap().scroll_line, scroll);
    app.split_editor_right();
    app.focus_editor_pane(PaneId::Secondary);
    app.move_right(false).unwrap();
    app.focus_editor_pane(PaneId::Primary);
    assert_eq!(app.active_editor().unwrap().selection(), Some(2..3));
}

#[test]
fn provider_edits_inactive_document_without_focus_change_and_encodes_atomically() {
    let mut app = Workbench::default();
    app.new_file("legacy-memory.p", TextEncoding::Iso8859_1);
    app.insert_text("niño\r\n").unwrap();
    let d = app.active_document().unwrap();
    let (id, rev) = (d.id, d.session.document().revision());
    app.new_file("other-memory.txt", TextEncoding::Utf8);
    let active = app.tabs().active_id();
    assert!(app
        .apply_text_edits(id, rev, vec![edit(0..1, "x"), edit(1..2, "😀")])
        .is_err());
    app.apply_text_edits(id, rev, vec![edit(0..4, "café")])
        .unwrap();
    assert_eq!(app.tabs().active_id(), active);
    app.select_tab(1);
    assert_eq!(app.active_bytes().unwrap(), b"caf\xe9\r\n");
    app.undo().unwrap();
    assert_eq!(app.active_bytes().unwrap(), b"ni\xf1o\r\n");
}

#[test]
fn close_one_dirty_view_keeps_shared_document_then_discard_reopens_clean_disk() {
    let path = std::env::temp_dir().join(format!("jc-f1-close-{}.txt", std::process::id()));
    std::fs::write(&path, b"disk\r\n").unwrap();
    let mut app = Workbench::default();
    app.open_file(&path, Some(TextEncoding::Utf8)).unwrap();
    let id = app.active_document().unwrap().id;
    app.insert_text("dirty").unwrap();
    app.split_editor_right();
    assert_eq!(app.close_active(false).unwrap(), None);
    assert_eq!(app.active_document().unwrap().id, id);
    assert!(app.context().document_dirty);
    assert_eq!(app.close_active(true).unwrap(), None);
    assert!(app.active_document().is_none());
    assert!(app.reopen_closed());
    assert_ne!(app.active_document().unwrap().id, id);
    assert_eq!(app.active_bytes().unwrap(), b"disk\r\n");
    assert!(!app.context().document_dirty);
    assert!(!app.tabs().active().unwrap().dirty);
    assert!(!app.context().can_undo);
    std::fs::remove_file(path).unwrap();
}
