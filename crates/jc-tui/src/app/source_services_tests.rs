use super::TuiApp;
use crate::{render, state::LaunchOptions};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use jc_app::{source_services::SourceState, CommandId, PrimaryFocus, SidebarView};
use jc_text::{TextEncoding, TextPosition};
use std::time::{Duration, Instant};
fn key(a: &mut TuiApp, code: KeyCode, mods: KeyModifiers) {
    a.handle_event(Event::Key(KeyEvent::new(code, mods)));
}
fn finish(a: &mut TuiApp) {
    let until = Instant::now() + Duration::from_secs(5);
    while matches!(a.source.state(), SourceState::Queued | SourceState::Running)
        || a.tasks.outstanding() > 0
    {
        a.poll_tasks();
        assert!(Instant::now() < until);
        std::thread::yield_now();
    }
}
fn editor(text: &str, path: &str) -> TuiApp {
    let mut a = TuiApp::new(LaunchOptions::default());
    a.workbench.new_file(path, TextEncoding::Utf8);
    a.workbench.insert_text(text).unwrap();
    a
}

#[test]
fn source_tui_renames_internal_procedures_and_functions_after_preview() {
    let text = "FUNCTION total RETURNS INTEGER() FORWARD.\r\nDISPLAY total().\r\nFUNCTION total RETURNS INTEGER():\r\nRETURN 1.\r\nEND FUNCTION.\r\nPROCEDURE report:\r\nDISPLAY total().\r\nEND PROCEDURE.\r\nRUN report.";
    let mut a = editor(text, "routines.p");
    for (line, column, old, new, changes) in [
        (8, 5, "report", "show-report", 2),
        (1, 9, "total", "sum-value", 4),
    ] {
        a.workbench
            .go_to_position(TextPosition { line, column })
            .unwrap();
        key(&mut a, KeyCode::F(2), KeyModifiers::NONE);
        for ch in new.chars() {
            key(&mut a, KeyCode::Char(ch), KeyModifiers::NONE);
        }
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        finish(&mut a);
        assert_eq!(
            a.source.state(),
            SourceState::Ready,
            "{}",
            a.source.message()
        );
        assert_eq!(a.source.items().len(), changes);
        assert_eq!(a.workbench.active_bytes().unwrap(), text.as_bytes());
        key(&mut a, KeyCode::F(9), KeyModifiers::NONE);
        assert_eq!(
            a.workbench.active_bytes().unwrap(),
            text.replace(old, new).as_bytes()
        );
        a.workbench.undo().unwrap();
        assert_eq!(a.workbench.active_bytes().unwrap(), text.as_bytes());
    }
}
#[test]
fn source_tui_rename_previews_scoped_ranges_and_applies_one_undo() {
    let text = "DEFINE VARIABLE total AS INTEGER NO-UNDO.\r\nPROCEDURE worker:\r\nDEFINE VARIABLE total AS INTEGER NO-UNDO.\r\ntotal = 1.\r\nEND PROCEDURE.\r\ntotal = 2.";
    let mut a = editor(text, "scope.p");
    a.workbench
        .go_to_position(TextPosition { line: 3, column: 1 })
        .unwrap();
    key(&mut a, KeyCode::F(2), KeyModifiers::NONE);
    for ch in "local-total".chars() {
        key(&mut a, KeyCode::Char(ch), KeyModifiers::NONE);
    }
    key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
    finish(&mut a);
    assert_eq!(
        a.source.state(),
        SourceState::Ready,
        "{}",
        a.source.message()
    );
    assert_eq!(a.source.items().len(), 2);
    key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(a.workbench.active_bytes().unwrap(), text.as_bytes());
    key(&mut a, KeyCode::F(9), KeyModifiers::NONE);
    assert_eq!(
        a.workbench.active_bytes().unwrap(),
        text.replace(
            "DEFINE VARIABLE total AS INTEGER NO-UNDO.\r\ntotal = 1.",
            "DEFINE VARIABLE local-total AS INTEGER NO-UNDO.\r\nlocal-total = 1."
        )
        .as_bytes()
    );
    a.workbench.undo().unwrap();
    assert_eq!(a.workbench.active_bytes().unwrap(), text.as_bytes());
}
#[test]
fn source_tui_symbols_keyboard_navigation_readonly_render_and_explicit_refresh() {
    let text="/* PROCEDURE hidden: */\r\nPROCEDURE alpha:\r\nEND PROCEDURE.\r\nFUNCTION beta RETURNS INTEGER():\r\nEND FUNCTION.";
    let mut a = editor(text, "source.p");
    a.dispatch(CommandId::ViewSymbols);
    finish(&mut a);
    assert_eq!(a.source.state(), SourceState::Ready);
    assert_eq!(a.source.items().len(), 2);
    assert_eq!(a.panels.sidebar(), SidebarView::Symbols);
    assert_eq!(a.focus.primary(), PrimaryFocus::Sidebar);
    let before = a.workbench.active_bytes().unwrap();
    let p = a.prepare_frame(140, 30);
    let screen = render::render(&p);
    assert_eq!(screen, render::render(&p));
    assert!((0..screen.height()).any(|row| screen.row_text(row).contains("Procedimiento alpha")));
    key(&mut a, KeyCode::Down, KeyModifiers::NONE);
    key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(a.focus.primary(), PrimaryFocus::Editor);
    assert_eq!(
        a.workbench.active_editor().unwrap().cursor,
        text.find("beta RETURNS").unwrap()
    );
    assert_eq!(a.workbench.active_bytes().unwrap(), before);
    a.workbench.insert_text("changed").unwrap();
    let _ = a.frame(140, 30);
    assert_eq!(a.source.state(), SourceState::Stale);
    a.dispatch(CommandId::SourceRefresh);
    finish(&mut a);
    assert_eq!(a.source.state(), SourceState::Ready);
}
#[test]
fn source_tui_completion_selector_blocks_source_input_accepts_one_transaction_and_keeps_encoding() {
    let text = "customer custom\r\ncus";
    let mut a = editor(text, "source.rs");
    a.dispatch(CommandId::SourceComplete);
    finish(&mut a);
    assert_eq!(a.source.items().len(), 2);
    assert!(a.source_popup);
    let before = a.workbench.active_bytes().unwrap();
    a.handle_event(Event::Paste("not source".into()));
    key(&mut a, KeyCode::Char('x'), KeyModifiers::NONE);
    assert_eq!(a.workbench.active_bytes().unwrap(), before);
    key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
    assert!(!a.source_popup);
    assert_eq!(
        a.workbench.active_bytes().unwrap(),
        b"customer custom\r\ncustomer"
    );
    key(&mut a, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(a.workbench.active_bytes().unwrap(), before);
    assert_eq!(a.workbench.active_encoding(), Some(TextEncoding::Utf8));
}
#[test]
fn source_tui_stale_completion_and_cancel_never_overwrite_new_caret_intent() {
    let mut a = editor("customer custom cus", "source.txt");
    a.dispatch(CommandId::SourceComplete);
    a.workbench.move_left(false).unwrap();
    finish(&mut a);
    let _ = a.frame(80, 24);
    assert_eq!(a.source.state(), SourceState::Stale);
    assert!(!a.source_popup);
    let before = a.workbench.active_bytes().unwrap();
    a.dispatch(CommandId::SourceAccept);
    assert_eq!(a.workbench.active_bytes().unwrap(), before);
    a.workbench
        .go_to_position(TextPosition {
            line: 0,
            column: before.len(),
        })
        .unwrap();
    a.dispatch(CommandId::SourceComplete);
    finish(&mut a);
    key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(a.source.state(), SourceState::Cancelled);
    assert!(!a.source_popup);
    assert_eq!(a.workbench.active_bytes().unwrap(), before);
}
#[test]
fn source_tui_static_reference_ambiguity_requires_choice_before_confined_open() {
    let base = std::env::temp_dir().join(format!("cide-source-tui-{}", std::process::id()));
    std::fs::create_dir(&base).unwrap();
    let sub = base.join("sub");
    std::fs::create_dir(&sub).unwrap();
    let source = sub.join("source.p");
    let first = sub.join("target.p");
    let second = base.join("target.p");
    std::fs::write(&source, b"RUN target.p.\r\n").unwrap();
    std::fs::write(&first, b"LOCAL_TARGET\r\n").unwrap();
    std::fs::write(&second, b"ROOT_TARGET\r\n").unwrap();
    let mut a = TuiApp::new(LaunchOptions {
        files: vec![source.clone()],
        encoding: None,
    });
    a.project_root = Some(base.canonicalize().unwrap());
    a.dispatch(CommandId::SourceReferences);
    finish(&mut a);
    assert_eq!(a.source.items().len(), 1);
    let original = a.workbench.active_document().unwrap().id;
    key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
    assert!(a.source_popup);
    assert_eq!(a.source_candidates.as_ref().unwrap().0.len(), 2);
    assert_eq!(a.workbench.active_document().unwrap().id, original);
    let p = a.prepare_frame(140, 30);
    assert!(p
        .source
        .rows
        .iter()
        .any(|r| r.label == "sub\\\\target.p" || r.label == "sub/target.p"));
    key(&mut a, KeyCode::Down, KeyModifiers::NONE);
    key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(a.workbench.active_bytes().unwrap(), b"ROOT_TARGET\r\n");
    assert_ne!(a.workbench.active_document().unwrap().id, original);
    drop(a);
    for p in [&source, &first, &second] {
        std::fs::remove_file(p).unwrap();
    }
    std::fs::remove_dir(sub).unwrap();
    std::fs::remove_dir(base).unwrap();
}

#[test]
fn source_tui_reference_reuses_dirty_pinned_document_and_its_own_legacy_encoding() {
    let base = std::env::temp_dir().join(format!("cide-source-dirty-{}", std::process::id()));
    std::fs::create_dir(&base).unwrap();
    let source = base.join("source.p");
    let target = base.join("target.p");
    std::fs::write(&source, b"RUN target.p.\r\n").unwrap();
    std::fs::write(&target, b"ni\xf1o\r\n").unwrap();
    let mut a = TuiApp::new(LaunchOptions {
        files: vec![source.clone()],
        encoding: Some(TextEncoding::Utf8),
    });
    a.project_root = Some(base.canonicalize().unwrap());
    a.workbench.split_editor_right();
    a.workbench.focus_editor_pane(jc_app::PaneId::Secondary);
    a.workbench
        .open_file(&target, Some(TextEncoding::Windows1252))
        .unwrap();
    a.workbench.insert_text("DIRTY_").unwrap();
    let target_id = a.workbench.active_document().unwrap().id;
    let tab = a.workbench.tabs().active_id().unwrap();
    a.workbench.toggle_reference_pin();
    a.workbench.focus_editor_pane(jc_app::PaneId::Primary);
    a.dispatch(CommandId::SourceReferences);
    finish(&mut a);
    a.dispatch(CommandId::SourceOpenToSide);
    assert_eq!(a.workbench.active_document().unwrap().id, target_id);
    assert_eq!(
        a.workbench.active_encoding(),
        Some(TextEncoding::Windows1252)
    );
    assert_eq!(a.workbench.active_bytes().unwrap(), b"DIRTY_ni\xf1o\r\n");
    assert_eq!(
        a.workbench
            .panes()
            .view(jc_app::PaneId::Secondary)
            .unwrap()
            .location()
            .unwrap()
            .tab,
        tab
    );
    a.workbench.undo().unwrap();
    assert_eq!(a.workbench.active_bytes().unwrap(), b"ni\xf1o\r\n");
    assert_eq!(std::fs::read(&target).unwrap(), b"ni\xf1o\r\n");
    drop(a);
    std::fs::remove_file(source).unwrap();
    std::fs::remove_file(target).unwrap();
    std::fs::remove_dir(base).unwrap();
}
