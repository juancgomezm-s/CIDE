use jc_app::{PaneId, Workbench};
use jc_text::{TextEncoding, TextPosition};

#[test]
fn ordinary_caret_and_selection_placement_preserve_navigation_history_and_byte_boundaries() {
    let mut app = Workbench::default();
    app.new_file("caret-placement.txt", TextEncoding::Utf8);
    app.insert_text("abc\r\néx").unwrap();
    let original = app.active_bytes().unwrap();
    let history = app
        .active_document()
        .unwrap()
        .session
        .document()
        .undo_steps();
    app.set_caret_position(TextPosition { line: 0, column: 1 }, false)
        .unwrap();
    assert!(!app.context().nav_back_available);
    app.set_caret_position(TextPosition { line: 1, column: 1 }, true)
        .unwrap();
    assert_eq!(app.selection_for_pane(PaneId::Primary), Some(1..7));
    assert!(!app.context().nav_back_available);
    assert!(app
        .set_caret_position(
            TextPosition {
                line: 1,
                column: 999
            },
            false
        )
        .is_err());
    assert_eq!(app.selection_for_pane(PaneId::Primary), Some(1..7));
    app.go_to_position(TextPosition { line: 0, column: 0 })
        .unwrap();
    assert!(app.context().nav_back_available);
    assert!(app.navigation_back());
    assert_eq!(app.active_editor().unwrap().cursor, 7);
    assert_eq!(app.active_bytes().unwrap(), original);
    assert_eq!(
        app.active_document()
            .unwrap()
            .session
            .document()
            .undo_steps(),
        history
    );
}
