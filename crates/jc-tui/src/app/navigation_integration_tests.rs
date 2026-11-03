//! Real event/menu/palette navigation over one byte-preserving Workbench.
use super::TuiApp;
use crate::{
    render,
    state::{LaunchOptions, PromptKind, ReplaceField, UiOverlay},
};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use jc_app::{CommandId, MenuId, PaneId, SearchStatus};
use jc_text::{TextEncoding, TextPosition};
use std::time::{Duration, Instant};

const TEXT: &str = "\u{feff}αé\r\nβ\tZ\nlast\r";
fn key(app: &mut TuiApp, code: KeyCode, mods: KeyModifiers) {
    app.handle_event(Event::Key(KeyEvent::new(code, mods)));
}
fn plain(app: &mut TuiApp, code: KeyCode) {
    key(app, code, KeyModifiers::NONE);
}
fn paste(app: &mut TuiApp, text: &str) {
    app.handle_event(Event::Paste(text.into()));
}
fn editor(text: &str) -> TuiApp {
    let mut app = TuiApp::new(LaunchOptions::default());
    app.panels.hide_sidebar();
    app.workbench
        .new_file("navigation-memory.txt", TextEncoding::Utf8);
    app.workbench.insert_text(text).unwrap();
    app.workbench
        .go_to_position(TextPosition { line: 0, column: 0 })
        .unwrap();
    app
}
fn goto(app: &mut TuiApp, text: &str) {
    key(app, KeyCode::Char('g'), KeyModifiers::CONTROL);
    paste(app, text);
    plain(app, KeyCode::Enter);
}
fn revision(app: &TuiApp) -> u64 {
    app.workbench
        .active_document()
        .unwrap()
        .session
        .document()
        .revision()
}
fn complete(app: &mut TuiApp) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.workbench.search_running() {
        assert!(Instant::now() < deadline);
        app.poll_tasks();
    }
    assert_eq!(app.workbench.search().status, SearchStatus::Complete);
}
fn palette(app: &mut TuiApp, label: &str) {
    key(
        app,
        KeyCode::Char('p'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    paste(app, label);
    plain(app, KeyCode::Enter);
}
fn menu_goto(app: &mut TuiApp) {
    app.dispatch(CommandId::MenuSearch);
    let entries = app.menu_commands(MenuId::Search);
    let target = entries
        .iter()
        .position(|id| *id == Some(CommandId::SearchGoToPosition))
        .unwrap();
    for _ in 0..entries.len() {
        if matches!(app.overlay,UiOverlay::Menu{menu:MenuId::Search,selected} if selected==target) {
            plain(app, KeyCode::Enter);
            return;
        }
        plain(app, KeyCode::Down);
    }
    panic!("Search menu did not reach Go to Position");
}

#[test]
fn goto_entrypoints_use_source_scalars_with_bom_crlf_and_invisible_display() {
    let mut empty = TuiApp::new(LaunchOptions::default());
    empty.panels.hide_sidebar();
    empty
        .workbench
        .new_file("navigation-empty.txt", TextEncoding::Utf8);
    key(&mut empty, KeyCode::Char('g'), KeyModifiers::CONTROL);
    paste(&mut empty, "1:1");
    let views = format!("{:?}", empty.workbench.panes());
    empty.dispatch(CommandId::SearchHistoryPrevious);
    empty.dispatch(CommandId::NavigateBack);
    assert!(
        matches!(&empty.overlay,UiOverlay::Prompt{kind:PromptKind::GoToPosition,input} if input=="1:1")
    );
    assert_eq!(format!("{:?}", empty.workbench.panes()), views);
    assert!(empty.workbench.active_bytes().unwrap().is_empty());
    for entry in 0..3 {
        let mut app = editor(TEXT);
        app.dispatch(CommandId::ViewToggleInvisibles);
        let rev = revision(&app);
        match entry {
            0 => key(&mut app, KeyCode::Char('g'), KeyModifiers::CONTROL),
            1 => menu_goto(&mut app),
            _ => palette(&mut app, "Ir a línea/columna"),
        }
        assert!(matches!(
            app.overlay,
            UiOverlay::Prompt {
                kind: PromptKind::GoToPosition,
                ..
            }
        ));
        let before = app.workbench.active_position();
        paste(&mut app, "2:3");
        assert_eq!(app.workbench.active_position(), before);
        plain(&mut app, KeyCode::Enter);
        assert_eq!(
            app.workbench.active_position(),
            Some(TextPosition { line: 1, column: 2 })
        );
        assert_eq!(app.workbench.active_editor().unwrap().cursor, 12);
        assert!(matches!(app.overlay, UiOverlay::None));
        let views = format!("{:?}", app.workbench.panes());
        goto(&mut app, "2:3");
        assert_eq!(format!("{:?}", app.workbench.panes()), views);
        for invalid in [
            "0:0",
            "1:0",
            "1:2:3",
            "99:1",
            "1:99",
            "184467440737095516160",
            "1\n2",
        ] {
            key(&mut app, KeyCode::Char('g'), KeyModifiers::CONTROL);
            paste(&mut app, invalid);
            plain(&mut app, KeyCode::Enter);
            assert_eq!(
                app.workbench.active_position(),
                Some(TextPosition { line: 1, column: 2 })
            );
            assert!(matches!(app.overlay, UiOverlay::Prompt { .. }));
            plain(&mut app, KeyCode::Esc);
        }
        for width in [45, 140] {
            let p = app.prepare_frame(width, 24);
            let first = render::render(&p);
            assert_eq!(first, render::render(&p));
            assert_eq!(p.app.workbench.active_bytes().unwrap(), TEXT.as_bytes());
        }
        assert_eq!(revision(&app), rev);
        app.workbench.undo().unwrap();
        assert!(app.workbench.active_bytes().unwrap().is_empty());
    }
}

#[test]
fn back_forward_are_group_local_and_do_not_replace_the_pinned_shared_view() {
    let mut app = editor(TEXT);
    goto(&mut app, "2:3");
    goto(&mut app, "3:2");
    key(&mut app, KeyCode::Left, KeyModifiers::ALT);
    assert_eq!(
        app.workbench.active_position(),
        Some(TextPosition { line: 1, column: 2 })
    );
    key(&mut app, KeyCode::Right, KeyModifiers::ALT);
    assert_eq!(
        app.workbench.active_position(),
        Some(TextPosition { line: 2, column: 1 })
    );
    app.dispatch(CommandId::EditorSplitRight);
    app.dispatch(CommandId::EditorFocusSecondary);
    goto(&mut app, "1:2");
    app.dispatch(CommandId::EditorTogglePin);
    goto(&mut app, "2:2");
    // GoTo is automatic navigation: a pinned invoker redirects to the other
    // group. History traversal itself remains local to the focused group.
    assert_eq!(app.workbench.panes().active(), PaneId::Primary);
    assert_eq!(
        app.workbench.active_position(),
        Some(TextPosition { line: 1, column: 1 })
    );
    assert_eq!(
        app.workbench.position_for_pane(PaneId::Secondary),
        Some(TextPosition { line: 0, column: 1 })
    );
    key(&mut app, KeyCode::Left, KeyModifiers::ALT);
    assert_eq!(
        app.workbench.active_position(),
        Some(TextPosition { line: 2, column: 1 })
    );
    assert_eq!(
        app.workbench.position_for_pane(PaneId::Secondary),
        Some(TextPosition { line: 0, column: 1 })
    );
    assert!(app
        .workbench
        .panes()
        .view(PaneId::Secondary)
        .unwrap()
        .is_pinned());
    key(&mut app, KeyCode::Right, KeyModifiers::ALT);
    assert_eq!(
        app.workbench.active_position(),
        Some(TextPosition { line: 1, column: 1 })
    );
    app.dispatch(CommandId::EditorFocusSecondary);
    key(&mut app, KeyCode::Left, KeyModifiers::ALT);
    assert_eq!(
        app.workbench.active_position(),
        Some(TextPosition { line: 2, column: 1 })
    );
    assert_eq!(
        app.workbench.position_for_pane(PaneId::Primary),
        Some(TextPosition { line: 1, column: 1 })
    );
    key(&mut app, KeyCode::Right, KeyModifiers::ALT);
    assert_eq!(
        app.workbench.active_position(),
        Some(TextPosition { line: 0, column: 1 })
    );
    assert_eq!(
        app.workbench.position_for_pane(PaneId::Primary),
        Some(TextPosition { line: 1, column: 1 })
    );
    assert_eq!(app.workbench.tabs().tabs().len(), 1);
    assert_eq!(app.workbench.active_bytes().unwrap(), TEXT.as_bytes());
}

#[test]
fn multiline_history_restores_flags_and_cursor_without_replacing_replacement_draft() {
    let mut app = editor("alpha\r\nbeta\nALPHA\r\nbeta");
    key(&mut app, KeyCode::Char('f'), KeyModifiers::CONTROL);
    paste(&mut app, "alpha\r\nbeta");
    key(&mut app, KeyCode::Char('c'), KeyModifiers::ALT);
    key(&mut app, KeyCode::Char('w'), KeyModifiers::ALT);
    complete(&mut app);
    plain(&mut app, KeyCode::F(3));
    plain(&mut app, KeyCode::Esc);
    key(&mut app, KeyCode::Char('f'), KeyModifiers::CONTROL);
    key(&mut app, KeyCode::Char('u'), KeyModifiers::CONTROL);
    paste(&mut app, "β\nnohit");
    key(&mut app, KeyCode::Char('c'), KeyModifiers::ALT);
    key(&mut app, KeyCode::Char('w'), KeyModifiers::ALT);
    complete(&mut app);
    plain(&mut app, KeyCode::F(3));
    plain(&mut app, KeyCode::Esc);
    key(&mut app, KeyCode::Char('h'), KeyModifiers::CONTROL);
    plain(&mut app, KeyCode::Tab);
    paste(&mut app, "replacement\r\nΩ");
    let replacement_cursor = app.replacement_cursor;
    let before = format!("{:?}", app.workbench.panes());
    for (code, query, sensitive, word) in [
        (KeyCode::Up, "β\nnohit", false, false),
        (KeyCode::Up, "alpha\r\nbeta", true, true),
        (KeyCode::Down, "β\nnohit", false, false),
    ] {
        key(&mut app, code, KeyModifiers::ALT);
        complete(&mut app);
        assert_eq!(app.workbench.search().query, query);
        assert_eq!(app.workbench.search().case_sensitive, sensitive);
        assert_eq!(app.workbench.search().whole_word, word);
        assert_eq!(app.find_cursor, query.len());
        assert_eq!(app.replacement_cursor, replacement_cursor);
        assert!(
            matches!(&app.overlay,UiOverlay::Replace{find,replacement,field:ReplaceField::Replacement} if find==query && replacement=="replacement\r\nΩ")
        );
        assert_eq!(format!("{:?}", app.workbench.panes()), before);
    }
}

#[test]
fn goto_open_cancel_and_accept_cancel_deferred_find_navigation_without_stopping_scan() {
    let mut app = editor("head\r\nneedle\r\nend");
    for accept in [false, true] {
        goto(&mut app, "1:1");
        key(&mut app, KeyCode::Char('f'), KeyModifiers::CONTROL);
        key(&mut app, KeyCode::Char('u'), KeyModifiers::CONTROL);
        paste(&mut app, "needle");
        plain(&mut app, KeyCode::F(3));
        app.dispatch(CommandId::SearchGoToPosition);
        paste(&mut app, "3:2");
        plain(&mut app, if accept { KeyCode::Enter } else { KeyCode::Esc });
        complete(&mut app);
        assert_eq!(
            app.workbench.active_position(),
            Some(if accept {
                TextPosition { line: 2, column: 1 }
            } else {
                TextPosition { line: 0, column: 0 }
            })
        );
        assert!(app.workbench.active_editor().unwrap().selection().is_none());
        assert_eq!(
            app.workbench.active_bytes().unwrap(),
            b"head\r\nneedle\r\nend"
        );
    }
}
