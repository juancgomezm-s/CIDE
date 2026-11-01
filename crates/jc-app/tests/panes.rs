use jc_app::{PaneId, Workbench};
use jc_text::TextEncoding;
use std::path::PathBuf;

fn scratch(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("jc-ide-pane-{}-{}", std::process::id(), name))
}

#[test]
fn editor_groups_share_one_workbench_without_duplicating_document_bytes() {
    let mut workbench = Workbench::default();
    let left = workbench.new_file(scratch("left.txt"), TextEncoding::Utf8);
    workbench.insert_text("primary").unwrap();
    assert!(workbench.split_editor_right());
    assert_eq!(
        workbench
            .panes()
            .view(PaneId::Secondary)
            .unwrap()
            .location()
            .unwrap()
            .tab,
        left
    );
    assert!(workbench.focus_editor_pane(PaneId::Secondary));
    let right = workbench.new_file(scratch("right.txt"), TextEncoding::Utf8);
    workbench.insert_text("reference").unwrap();
    assert_ne!(left, right);

    assert!(workbench.focus_editor_pane(PaneId::Primary));
    assert_eq!(workbench.active_bytes().unwrap(), b"primary");
    assert!(workbench.focus_editor_pane(PaneId::Secondary));
    assert_eq!(workbench.active_bytes().unwrap(), b"reference");
    assert_eq!(workbench.tabs().tabs().len(), 2);
}

#[test]
fn pinning_reference_redirects_automatic_open_and_keeps_back_history() {
    let mut workbench = Workbench::default();
    let reference = workbench.new_file(scratch("reference.p"), TextEncoding::Utf8);
    assert!(workbench.split_editor_right());
    assert!(workbench.focus_editor_pane(PaneId::Secondary));
    assert_eq!(workbench.toggle_reference_pin(), Some(true));

    let target = workbench.new_file(scratch("target.p"), TextEncoding::Utf8);
    assert_ne!(reference, target);
    assert_eq!(workbench.panes().active(), PaneId::Primary);
    assert_eq!(
        workbench
            .panes()
            .view(PaneId::Secondary)
            .unwrap()
            .location()
            .unwrap()
            .tab,
        reference
    );
    assert_eq!(workbench.tabs().active_id(), Some(target));
    assert!(workbench.navigation_back());
    assert_eq!(workbench.tabs().active_id(), Some(reference));
    assert!(workbench.navigation_forward());
    assert_eq!(workbench.tabs().active_id(), Some(target));
}

#[test]
fn closing_split_restores_the_primary_document() {
    let mut workbench = Workbench::default();
    let first = workbench.new_file(scratch("one.p"), TextEncoding::Utf8);
    assert!(workbench.split_editor_right());
    assert!(workbench.focus_editor_pane(PaneId::Secondary));
    let second = workbench.new_file(scratch("two.p"), TextEncoding::Utf8);
    assert_eq!(workbench.tabs().active_id(), Some(second));
    assert!(workbench.close_split_editor());
    assert_eq!(workbench.tabs().active_id(), Some(first));
    assert_eq!(workbench.panes().active(), PaneId::Primary);
    assert!(!workbench.panes().is_split());
}

#[test]
fn both_panes_show_same_tab_id_while_only_one_document_exists() {
    let mut workbench = Workbench::default();
    let tab = workbench.new_file(scratch("one-document.p"), TextEncoding::Utf8);
    workbench.insert_text("abc").unwrap();
    workbench.split_editor_right();
    assert_eq!(
        workbench
            .panes()
            .view(PaneId::Primary)
            .unwrap()
            .location()
            .unwrap()
            .tab,
        tab
    );
    assert_eq!(
        workbench
            .panes()
            .view(PaneId::Secondary)
            .unwrap()
            .location()
            .unwrap()
            .tab,
        tab
    );
    assert_eq!(workbench.tabs().tabs().len(), 1);
    workbench.focus_editor_pane(PaneId::Secondary);
    workbench.insert_text("def").unwrap();
    workbench.focus_editor_pane(PaneId::Primary);
    assert_eq!(workbench.active_bytes().unwrap(), b"abcdef");
    workbench.undo().unwrap();
    assert_eq!(workbench.active_bytes().unwrap(), b"abc");
}

#[test]
fn selection_and_scroll_survive_focus_changes_on_a_shared_document() {
    let mut bench = Workbench::default();
    bench.new_file(scratch("independent.txt"), TextEncoding::Utf8);
    bench.insert_text(&"abcdef\n".repeat(40)).unwrap();
    bench.move_home(false).unwrap();
    bench.reveal_active_pane(5, 20);
    let primary_scroll = bench.active_document().unwrap().scroll_line;
    bench.split_editor_right();
    bench.focus_editor_pane(PaneId::Secondary);
    bench.move_vertical(-30, false).unwrap();
    bench.move_right(true).unwrap();
    bench.reveal_active_pane(5, 20);
    let secondary_cursor = bench.active_document().unwrap().cursor;
    let secondary_selection = bench.active_document().unwrap().selection();
    let secondary_scroll = bench.active_document().unwrap().scroll_line;
    assert_ne!(primary_scroll, secondary_scroll);
    bench.focus_editor_pane(PaneId::Primary);
    assert_eq!(bench.active_document().unwrap().scroll_line, primary_scroll);
    assert_eq!(bench.active_document().unwrap().selection(), None);
    bench.focus_editor_pane(PaneId::Secondary);
    assert_eq!(bench.active_document().unwrap().cursor, secondary_cursor);
    assert_eq!(
        bench.active_document().unwrap().selection(),
        secondary_selection
    );
    assert_eq!(
        bench.active_document().unwrap().scroll_line,
        secondary_scroll
    );
}

#[test]
fn edits_and_shared_undo_rebase_the_other_groups_selection_and_history() {
    let mut bench = Workbench::default();
    let tab = bench.new_file(scratch("rebase.txt"), TextEncoding::Utf8);
    bench.insert_text("aébcdef").unwrap();
    bench.move_home(false).unwrap();
    bench.split_editor_right();
    bench.focus_editor_pane(PaneId::Secondary);
    bench.move_right(false).unwrap(); // byte 1
    bench.move_right(true).unwrap(); // byte 3, selects é
    bench.focus_editor_pane(PaneId::Primary);
    bench.insert_text("ñ").unwrap(); // two bytes before the other selection
    assert_eq!(bench.selection_for_pane(PaneId::Secondary), Some(3..5));
    bench.undo().unwrap();
    assert_eq!(bench.selection_for_pane(PaneId::Secondary), Some(1..3));
    bench.redo().unwrap();
    assert_eq!(bench.selection_for_pane(PaneId::Secondary), Some(3..5));
    bench.focus_editor_pane(PaneId::Secondary);
    assert_eq!(bench.active_document().unwrap().selection(), Some(3..5));
    bench.jump_to_location(tab, 8);
    bench.focus_editor_pane(PaneId::Primary);
    bench.insert_text("x").unwrap();
    bench.focus_editor_pane(PaneId::Secondary);
    assert!(bench.navigation_back());
    assert_eq!(bench.active_document().unwrap().cursor, 6);
    assert!(!bench.jump_to_location(tab, 5)); // inside é
}

#[test]
fn linked_scroll_moves_viewports_without_moving_the_other_caret() {
    let mut bench = Workbench::default();
    bench.new_file(scratch("linked.txt"), TextEncoding::Utf8);
    bench.insert_text(&"abc\n".repeat(50)).unwrap();
    bench.move_vertical(-50, false).unwrap();
    bench.split_editor_right();
    bench.toggle_scroll_link();
    bench.move_vertical(20, false).unwrap();
    bench.reveal_active_pane(5, 20);
    let secondary = bench.panes().view(PaneId::Secondary).unwrap();
    assert_eq!(secondary.scroll_line(), 16);
    assert_eq!(secondary.location().unwrap().byte_offset, 0);
    bench.toggle_scroll_link();
    bench.move_vertical(10, false).unwrap();
    bench.reveal_active_pane(5, 20);
    assert_eq!(
        bench.panes().view(PaneId::Secondary).unwrap().scroll_line(),
        16
    );
}

#[test]
fn open_to_side_preserves_a_pinned_target_and_closing_a_shared_tab_clears_both() {
    let mut bench = Workbench::default();
    let reference = bench.new_file(scratch("side-reference.txt"), TextEncoding::Utf8);
    bench.split_editor_right();
    bench.focus_editor_pane(PaneId::Secondary);
    bench.toggle_reference_pin();
    bench.focus_editor_pane(PaneId::Primary);
    let target = bench.new_file(scratch("side-target.txt"), TextEncoding::Utf8);
    bench.open_active_to_side();
    assert_eq!(
        bench
            .panes()
            .view(PaneId::Secondary)
            .unwrap()
            .location()
            .unwrap()
            .tab,
        reference
    );
    assert_eq!(bench.tabs().active_id(), Some(target));
    bench.focus_editor_pane(PaneId::Secondary);
    bench.toggle_reference_pin();
    bench.focus_editor_pane(PaneId::Primary);
    bench.open_active_to_side();
    assert_eq!(bench.panes().active(), PaneId::Secondary);
    assert_eq!(bench.tabs().tabs().len(), 2);
    bench.close_active(true).unwrap();
    for pane in [PaneId::Primary, PaneId::Secondary] {
        assert_eq!(
            bench.panes().view(pane).unwrap().location().unwrap().tab,
            reference
        );
        bench.focus_editor_pane(pane);
        assert!(!bench.navigation_forward());
    }
}

#[test]
fn reopening_an_open_path_reuses_dirty_bytes_and_undo() {
    let path = scratch("reuse-existing.txt");
    std::fs::write(&path, b"disk").unwrap();
    let mut bench = Workbench::default();
    let first = bench.open_file(&path, Some(TextEncoding::Utf8)).unwrap();
    bench.insert_text("dirty ").unwrap();
    let second = bench
        .open_file_to_side(&path, Some(TextEncoding::Utf8))
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(bench.tabs().tabs().len(), 1);
    assert_eq!(bench.active_bytes().unwrap(), b"dirty disk");
    bench.undo().unwrap();
    assert_eq!(bench.active_bytes().unwrap(), b"disk");
    std::fs::remove_file(path).unwrap();
}
