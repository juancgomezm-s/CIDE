use jc_app::{PaneId, Workbench};
use jc_text::TextEncoding;
use std::path::PathBuf;

fn scratch(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "jc-ide-pane-{}-{}",
        std::process::id(),
        name
    ))
}

#[test]
fn editor_groups_share_one_workbench_without_duplicating_document_bytes() {
    let mut workbench = Workbench::default();
    let left = workbench.new_file(scratch("left.txt"), TextEncoding::Utf8);
    workbench.insert_text("primary").unwrap();
    assert!(workbench.split_editor_right());
    assert_eq!(
        workbench.panes().view(PaneId::Secondary).unwrap().location().unwrap().tab,
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
        workbench.panes().view(PaneId::Secondary).unwrap().location().unwrap().tab,
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
        workbench.panes().view(PaneId::Primary).unwrap().location().unwrap().tab,
        tab
    );
    assert_eq!(
        workbench.panes().view(PaneId::Secondary).unwrap().location().unwrap().tab,
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
