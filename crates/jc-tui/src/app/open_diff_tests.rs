use super::TuiApp;
use crate::{render, state::LaunchOptions};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use jc_app::{open_diff::OpenDiffState, CommandId, PaneId};
use jc_text::TextEncoding;
use std::time::{Duration, Instant};
fn pair() -> TuiApp {
    let mut a = TuiApp::new(LaunchOptions::default());
    a.workbench.new_file("left.p", TextEncoding::Utf8);
    a.workbench.insert_text("same\r\nleft\n").unwrap();
    a.workbench.split_editor_right();
    a.workbench.focus_editor_pane(PaneId::Secondary);
    a.workbench.new_file("right.p", TextEncoding::Windows1252);
    a.workbench.insert_text("same\r\nright\r\n").unwrap();
    a
}
fn key(a: &mut TuiApp, key: KeyCode, modifiers: KeyModifiers) {
    a.handle_event(Event::Key(KeyEvent::new(key, modifiers)));
}
fn finish(a: &mut TuiApp) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while matches!(
        a.open_diff.state(),
        OpenDiffState::Queued | OpenDiffState::Running
    ) {
        a.poll_tasks();
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(a.open_diff.state(), OpenDiffState::Ready);
}
#[test]
fn open_diff_tui_completes_without_input_readonly_paint_blocks_edits_and_navigates_both_groups() {
    let mut a = pair();
    let bytes = a.workbench.active_bytes().unwrap();
    let views = format!("{:?}", a.workbench.panes());
    a.dispatch(CommandId::DiffCompareOpen);
    finish(&mut a);
    assert!(a.open_diff_visible);
    for width in [45, 140, 80, 140] {
        let p = a.prepare_frame(width, 30);
        let s = render::render(&p);
        assert_eq!(s, render::render(&p));
        assert!((2..s.height() - 1).any(|row| s.row_text(row).contains("@@")));
    }
    assert_eq!(views, format!("{:?}", a.workbench.panes()));
    a.handle_event(Event::Paste("do not edit".into()));
    key(&mut a, KeyCode::Char('x'), KeyModifiers::NONE);
    key(&mut a, KeyCode::Delete, KeyModifiers::NONE);
    a.dispatch(CommandId::EditDeleteLine);
    assert_eq!(a.workbench.active_bytes().unwrap(), bytes);
    key(&mut a, KeyCode::Enter, KeyModifiers::SHIFT);
    assert_eq!(a.workbench.panes().active(), PaneId::Primary);
    assert!(!a.open_diff_visible);
    assert_eq!(a.workbench.active_editor().unwrap().cursor, 6);
    a.dispatch(CommandId::DiffCompareOpen);
    finish(&mut a);
    key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(a.workbench.panes().active(), PaneId::Secondary);
    assert_eq!(a.workbench.active_editor().unwrap().cursor, 6);
    assert_eq!(a.workbench.active_bytes().unwrap(), bytes);
}
#[test]
fn open_diff_tui_palette_stale_refresh_cancel_and_shared_undo_keep_source_identity() {
    let mut a = pair();
    a.dispatch(CommandId::DiffCompareOpen);
    finish(&mut a);
    a.dispatch(CommandId::PaletteOpen);
    a.handle_event(Event::Paste(
        "Abrir bloque de diferencias en el grupo izquierdo".into(),
    ));
    key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(a.workbench.panes().active(), PaneId::Primary);
    a.dispatch(CommandId::DiffCompareOpen);
    finish(&mut a);
    a.workbench.insert_text("changed").unwrap();
    let _ = a.frame(140, 30);
    assert_eq!(a.open_diff.state(), OpenDiffState::Stale);
    assert!(a.open_diff.result().is_none());
    key(&mut a, KeyCode::F(5), KeyModifiers::NONE);
    finish(&mut a);
    key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(a.open_diff.state(), OpenDiffState::Cancelled);
    assert!(a.status.contains("cancelada"));
    assert!(!a.open_diff_visible);
    a.workbench.undo().unwrap();
    assert_eq!(a.workbench.active_bytes().unwrap(), b"same\r\nleft\n");
    a.dispatch(CommandId::DiffCompareOpen);
    finish(&mut a);
    assert_eq!(
        a.open_diff
            .result()
            .unwrap()
            .model
            .source_bytes(jc_app::diff_model::DiffSide::Left),
        b"same\r\nleft\n"
    );
}
