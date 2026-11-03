use jc_app::{
    default_keymap, default_registry, CommandId, Key, KeyScope, KeyStroke, PaneId, PrimaryFocus,
    Resolution, TextReplacement, Workbench,
};
use jc_text::{encode, TextEncoding, TextPosition};

fn bench(text: &str, encoding: TextEncoding) -> Workbench {
    let mut b = Workbench::default();
    b.new_file("rectangle-test.txt", encoding);
    b.insert_text(text).unwrap();
    at(&mut b, 0, 0);
    b
}
fn at(b: &mut Workbench, line: usize, column: usize) {
    b.go_to_position(TextPosition { line, column }).unwrap();
}
fn steps(b: &Workbench) -> (u64, usize, usize) {
    let d = b.active_document().unwrap().session.document();
    (d.revision(), d.undo_steps(), d.redo_steps())
}

#[test]
fn partial_tabs_copy_cut_and_paste_are_rectangular_single_undo_steps() {
    for encoding in [
        TextEncoding::Utf8,
        TextEncoding::Windows1252,
        TextEncoding::Iso8859_1,
        TextEncoding::Ibm850,
    ] {
        let text = "a\tb\r\né\r\nfin";
        let mut b = bench(text, encoding);
        at(&mut b, 0, 2); // scalar2 is after TAB: visual4
        b.start_rectangle().unwrap();
        b.extend_rectangle(-2, 1).unwrap(); // visual[2,4), incl virtual second row
        let original = b.active_bytes().unwrap();
        let before = steps(&b);
        b.copy_selection().unwrap();
        assert_eq!(steps(&b), before);
        b.cut_selection().unwrap();
        assert_eq!(
            b.active_bytes().unwrap(),
            encode("a b\r\né\r\nfin", encoding).unwrap()
        );
        assert_eq!(steps(&b).1, before.1 + 1);
        b.undo().unwrap();
        assert_eq!(b.active_bytes().unwrap(), original);
        b.redo().unwrap();
        at(&mut b, 0, 0);
        let before_paste = b.active_bytes().unwrap();
        let count = steps(&b).1;
        b.paste().unwrap(); // rectangular clip preserves two rows, no line endings inserted
        assert_eq!(
            b.active_bytes().unwrap(),
            encode("  a b\r\n  é\r\nfin", encoding).unwrap()
        );
        assert_eq!(steps(&b).1, count + 1);
        b.undo().unwrap();
        assert_eq!(b.active_bytes().unwrap(), before_paste);
    }
}

#[test]
fn broadcast_and_multirow_paste_preserve_mixed_eol_and_terminal_newline() {
    let mut b = bench("ab\r\nc\nd\r", TextEncoding::Utf8);
    at(&mut b, 0, 1);
    b.start_rectangle().unwrap();
    b.extend_rectangle(1, 2).unwrap();
    let original = b.active_bytes().unwrap();
    let before = steps(&b);
    b.insert_text("X\r\nY\nZ").unwrap();
    assert_eq!(b.active_bytes().unwrap(), b"aX\r\ncY\ndZ\r");
    assert_eq!(steps(&b).1, before.1 + 1);
    assert!(b.rectangle_for_pane(PaneId::Primary).is_none());
    b.undo().unwrap();
    assert_eq!(b.active_bytes().unwrap(), original);
    at(&mut b, 0, 1);
    b.start_rectangle().unwrap();
    b.extend_rectangle(3, 2).unwrap();
    b.insert_text("!").unwrap();
    assert_eq!(b.active_bytes().unwrap(), b"a!\r\nc!\nd!\r");
}

#[test]
fn rejected_rows_unicode_encoding_and_commands_leave_the_transaction_untouched() {
    let mut b = bench("ab\r\ncd\r\nef", TextEncoding::Iso8859_1);
    b.start_rectangle().unwrap();
    b.extend_rectangle(1, 2).unwrap();
    let original = b.active_bytes().unwrap();
    let before = steps(&b);
    for text in ["x\ny", "€", "e\u{301}", "界", "👩\u{200d}💻", "\u{0}"] {
        assert!(b.insert_text(text).is_err(), "{text:?}");
        assert_eq!(b.active_bytes().unwrap(), original);
        assert_eq!(steps(&b), before);
        assert!(b.rectangle_for_pane(PaneId::Primary).is_some());
    }
    assert!(b.insert_newline().is_err());
    assert!(b.delete_line().is_err());
    let registry = default_registry();
    for command in [
        CommandId::EditDuplicate,
        CommandId::EditDeleteLine,
        CommandId::EditOutdent,
        CommandId::EditToggleLineComment,
    ] {
        assert!(!registry.is_enabled(command, &b.context()));
    }
    b.execute(CommandId::SearchCancel).unwrap();
    assert!(b.rectangle_for_pane(PaneId::Primary).is_none());
    assert_eq!(b.active_bytes().unwrap(), original);
}

#[test]
fn shared_views_own_rectangles_independently_and_all_edits_invalidate_geometry() {
    let mut b = bench("ab\r\ncd\r\nef", TextEncoding::Utf8);
    let id = b.active_document().unwrap().id;
    b.start_rectangle().unwrap();
    b.extend_rectangle(1, 1).unwrap();
    assert!(b.split_editor_right());
    let primary = b.rectangle_for_pane(PaneId::Primary).unwrap();
    b.focus_editor_pane(PaneId::Secondary);
    b.extend_rectangle(1, 1).unwrap();
    assert_eq!(b.rectangle_for_pane(PaneId::Primary), Some(primary));
    assert_ne!(b.rectangle_for_pane(PaneId::Secondary), Some(primary));
    let original = b.active_bytes().unwrap();
    let revision = steps(&b).0;
    let before = steps(&b).1;
    b.apply_text_edits(
        id,
        revision,
        vec![TextReplacement {
            range: 0..0,
            text: "X".into(),
        }],
    )
    .unwrap();
    assert_eq!(b.active_document().unwrap().id, id);
    assert!(b.rectangle_for_pane(PaneId::Primary).is_none());
    assert!(b.rectangle_for_pane(PaneId::Secondary).is_none());
    assert_eq!(steps(&b).1, before + 1);
    b.focus_editor_pane(PaneId::Primary);
    b.undo().unwrap();
    assert_eq!(b.active_bytes().unwrap(), original);
}

#[test]
fn virtual_insert_delete_and_clipboard_do_not_add_line_endings() {
    let mut b = bench("a\r\nb", TextEncoding::Utf8);
    b.start_rectangle().unwrap();
    b.extend_rectangle(4, 1).unwrap();
    b.extend_rectangle(-4, 0).unwrap(); // zero width at0 across2rows
    b.extend_rectangle(3, 0).unwrap(); // [0,3) includes virtual
    b.copy_selection().unwrap();
    b.cancel_rectangle();
    at(&mut b, 0, 1);
    let before = steps(&b);
    b.start_rectangle().unwrap();
    b.extend_rectangle(4, 1).unwrap();
    b.extend_rectangle(-4, 0).unwrap(); // zero width at visual1
    b.delete_forward().unwrap(); // both are EOL, never joins lines
    assert_eq!(steps(&b), before);
    assert_eq!(b.active_bytes().unwrap(), b"a\r\nb");
    b.cancel_rectangle();
    at(&mut b, 0, 1);
    b.paste().unwrap();
    assert_eq!(b.active_bytes().unwrap(), b"aa  \r\nbb  ");
    b.undo().unwrap();
    at(&mut b, 1, 1);
    let before = steps(&b);
    assert!(b.paste().is_err()); // no implicit new rows after EOF
    assert_eq!(steps(&b), before);
    assert_eq!(b.active_bytes().unwrap(), b"a\r\nb");
}

#[test]
fn rectangle_admission_caps_and_unsupported_source_are_atomic() {
    for text in ["e\u{301}", "界", "🙂", "a\u{7f}", "a\u{ad}"] {
        let mut b = bench(text, TextEncoding::Utf8);
        let before = steps(&b);
        assert!(b.start_rectangle().is_err());
        assert_eq!(steps(&b), before);
        assert!(!b.context().rectangular_selection);
    }
    let mut b = bench("ok\n界", TextEncoding::Utf8);
    b.start_rectangle().unwrap();
    let rect = b.rectangle_for_pane(PaneId::Primary);
    assert!(b.extend_rectangle(1, 1).is_err());
    assert_eq!(b.rectangle_for_pane(PaneId::Primary), rect);
    assert!(b.extend_rectangle(4097, 0).is_err());
    let mut b = bench(&"a".repeat(64 * 1024 + 1), TextEncoding::Utf8);
    assert!(b.start_rectangle().is_err());
    let mut b = bench(&"x\n".repeat(1024), TextEncoding::Utf8);
    let before = steps(&b);
    assert!(b.extend_rectangle(1, 1024).is_err());
    assert_eq!(steps(&b), before);
    assert!(!b.context().rectangular_selection);
}

#[test]
fn linear_selection_converts_to_visual_columns_and_plain_movement_exits_rectangle() {
    let mut b = bench("a\tb\r\néf", TextEncoding::Utf8);
    at(&mut b, 0, 2);
    b.move_vertical(1, true).unwrap();
    b.start_rectangle().unwrap();
    let rect = b.rectangle_for_pane(PaneId::Primary).unwrap();
    assert_eq!(
        (rect.first_line, rect.last_line, rect.left, rect.right),
        (0, 1, 2, 4)
    );
    assert!(b.selection_for_pane(PaneId::Primary).is_none());
    b.move_left(false).unwrap();
    assert!(b.rectangle_for_pane(PaneId::Primary).is_none());
    assert!(!b.context().has_selection);

    let mut edge = bench("a", TextEncoding::Utf8);
    at(&mut edge, 0, 1);
    let history = steps(&edge);
    edge.start_rectangle().unwrap();
    edge.extend_rectangle(3, 0).unwrap();
    edge.move_right(false).unwrap();
    assert!(edge.rectangle_for_pane(PaneId::Primary).is_none());
    assert_eq!(steps(&edge), history);
    assert_eq!(edge.active_bytes().unwrap(), b"a");
    at(&mut edge, 0, 0);
    edge.start_rectangle().unwrap();
    edge.move_left(false).unwrap();
    assert!(edge.rectangle_for_pane(PaneId::Primary).is_none());
    assert_eq!(steps(&edge), history);
}

#[test]
fn alt_shift_and_palette_commands_share_registry_and_respect_editor_focus() {
    let mut b = bench("ab\ncd", TextEncoding::Utf8);
    let registry = default_registry();
    let keys = default_keymap();
    assert!(keys.validate(&registry).is_empty());
    let stroke = KeyStroke::alt_shift(Key::Down);
    assert_eq!(
        keys.resolve(KeyScope::Editor, stroke, &registry, &b.context()),
        Resolution::Enabled(CommandId::EditRectangleDown)
    );
    b.execute(CommandId::EditRectangleDown).unwrap();
    b.execute(CommandId::EditRectangleRight).unwrap();
    assert!(registry.is_enabled(CommandId::EditCopy, &b.context()));
    let mut context = b.context();
    context.focus = PrimaryFocus::Sidebar;
    assert!(!registry.is_enabled(CommandId::EditRectangleDown, &context));
    assert_eq!(
        keys.resolve(KeyScope::Sidebar, stroke, &registry, &context),
        Resolution::None
    );
    b.execute(CommandId::EditRectangleCancel).unwrap();
    assert!(!b.context().has_selection);
}

#[test]
fn bom_and_invalid_bytes_outside_the_rectangle_are_never_rewritten() {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Fixture(std::path::PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let fixture = Fixture(std::env::temp_dir().join(format!(
        "jc-rectangle-{}-{}.bin",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )));
    let source = b"\xef\xbb\xbfa\tb\r\n\xff\nend";
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&fixture.0)
        .unwrap();
    file.write_all(source).unwrap();
    drop(file);
    let mut b = Workbench::default();
    b.open_file(&fixture.0, Some(TextEncoding::Utf8)).unwrap();
    at(&mut b, 0, 2);
    b.start_rectangle().unwrap();
    b.extend_rectangle(-2, 0).unwrap();
    b.cut_selection().unwrap();
    assert_eq!(b.active_bytes().unwrap(), b"\xef\xbb\xbfa b\r\n\xff\nend");
    b.undo().unwrap();
    assert_eq!(b.active_bytes().unwrap(), source);
    at(&mut b, 0, 0);
    let before = steps(&b);
    assert!(b.extend_rectangle(1, 1).is_err());
    assert_eq!(steps(&b), before);
    assert_eq!(b.active_bytes().unwrap(), source);
    assert_eq!(std::fs::read(&fixture.0).unwrap(), source);
}

#[test]
fn visual_selection_intent_cancels_deferred_find_even_when_byte_offsets_do_not_move() {
    let mut b = bench("\tneedle", TextEncoding::Utf8);
    b.start_rectangle().unwrap();
    let cursor = b.active_editor().unwrap().cursor;
    let anchor = b.active_editor().unwrap().anchor;
    b.set_search_query("needle").unwrap();
    assert_eq!(b.find_next().unwrap(), None);
    b.extend_rectangle(1, 0).unwrap(); // Interior of TAB, same byte boundary.
    assert_eq!(b.active_editor().unwrap().cursor, cursor);
    assert_eq!(b.active_editor().unwrap().anchor, anchor);
    let rectangle = b.rectangle_for_pane(PaneId::Primary);
    for _ in 0..8 {
        if !b.search_running() {
            break;
        }
        b.poll_document_search();
    }
    assert!(!b.search_running());
    assert_eq!(b.search().matches.len(), 1);
    assert_eq!(b.rectangle_for_pane(PaneId::Primary), rectangle);
    assert_eq!(b.active_editor().unwrap().cursor, cursor);
}
