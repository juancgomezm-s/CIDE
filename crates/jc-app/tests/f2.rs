use jc_app::{PaneId, Workbench};
use jc_text::TextEncoding;

#[test]
fn warm_navigation_and_viewport_visit_only_bounded_text_and_no_global_source_scan() {
    let mut app = Workbench::default();
    let tab = app.new_file("f2-memory.txt", TextEncoding::Utf8);
    app.insert_text(&"abcdefg\r\n".repeat(100000)).unwrap();
    app.jump_to_location(tab, 0);
    for line in 0..24 {
        app.visible_line(tab, line, 0, 80);
    }
    let before = app.text_statistics(tab).unwrap();
    for _ in 0..100 {
        app.move_right(false).unwrap();
        app.active_position();
    }
    for line in 0..24 {
        app.visible_line(tab, line, 0, 80);
    }
    let after = app.text_statistics(tab).unwrap();
    assert_eq!(after.0, before.0);
    assert!(after.1 - before.1 < 65536);
    app.split_editor_right();
    app.focus_editor_pane(PaneId::Secondary);
    assert_eq!(app.active_bytes().unwrap().len(), 900000);
}

#[test]
fn local_edit_and_shared_undo_reuse_line_index_instead_of_scanning_the_whole_document() {
    let mut app = Workbench::default();
    let tab = app.new_file("f2-edit.txt", TextEncoding::Utf8);
    app.insert_text(&"abcdefg\n".repeat(20000)).unwrap();
    app.jump_to_location(tab, 80002);
    app.split_editor_right();
    let before = app.text_statistics(tab).unwrap();
    app.insert_text("é\r\n").unwrap();
    let after = app.text_statistics(tab).unwrap();
    assert!(after.0 - before.0 < 256);
    app.undo().unwrap();
    assert!(app.active_position().is_some());
    app.focus_editor_pane(PaneId::Secondary);
    assert!(app.active_position().is_some());
    app.redo().unwrap();
    assert!(app.active_position().is_some());
}
