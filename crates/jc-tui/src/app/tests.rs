use super::*;

#[test]
fn keyboard_edit_matrix_keeps_legacy_bytes_and_non_editor_tab_scope() {
    for encoding in [
        TextEncoding::Utf8,
        TextEncoding::Windows1252,
        TextEncoding::Iso8859_1,
        TextEncoding::Ibm850,
    ] {
        let mut app = TuiApp::new(LaunchOptions::default());
        app.workbench.new_file("matrix.p", encoding);
        app.workbench
            .insert_text("niño Straße\r\nnext\r\n")
            .unwrap();
        let original = app.workbench.active_bytes().unwrap();
        app.workbench
            .go_to_position(jc_text::TextPosition { line: 0, column: 0 })
            .unwrap();
        for (code, modifiers) in [
            (KeyCode::Char('d'), KeyModifiers::CONTROL),
            (
                KeyCode::Char('d'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            ),
            (KeyCode::Tab, KeyModifiers::NONE),
            (KeyCode::Char('/'), KeyModifiers::CONTROL),
        ] {
            key(&mut app, code, modifiers);
            assert_ne!(app.workbench.active_bytes().unwrap(), original);
            key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
            assert_eq!(app.workbench.active_bytes().unwrap(), original);
            app.workbench
                .go_to_position(jc_text::TextPosition { line: 0, column: 0 })
                .unwrap();
        }
        for command in [
            CommandId::ViewExplorer,
            CommandId::ViewOutput,
            CommandId::SearchFind,
            CommandId::SearchReplace,
            CommandId::PaletteOpen,
            CommandId::MenuView,
        ] {
            app.dispatch(command);
            let revision = app
                .workbench
                .active_document()
                .unwrap()
                .session
                .document()
                .revision();
            key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
            key(&mut app, KeyCode::BackTab, KeyModifiers::SHIFT);
            assert_eq!(
                app.workbench
                    .active_document()
                    .unwrap()
                    .session
                    .document()
                    .revision(),
                revision
            );
            assert_eq!(app.workbench.active_bytes().unwrap(), original);
            key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
            app.focus.focus(PrimaryFocus::Editor, &app.panels);
        }
    }
}

#[test]
fn document_search_runs_between_events_with_unicode_markers_history_and_goto() {
    use jc_app::SearchStatus;
    let mut app = TuiApp::new(LaunchOptions::default());
    app.workbench.new_file("unicode.txt", TextEncoding::Utf8);
    text(&mut app, "Straße NIÑO\r\nxßx ss ß");
    app.frame(100, 25);
    key(&mut app, KeyCode::Char('f'), KeyModifiers::CONTROL);
    text(&mut app, "STRASSE");
    assert_eq!(app.workbench.search().status, SearchStatus::Running);
    assert_eq!(app.workbench.search().scanned_bytes, 0);
    app.frame(100, 25);
    assert_eq!(app.workbench.search().scanned_bytes, 0); // preparation/render do not advance search
    while app.workbench.search_running() {
        app.poll_tasks();
    }
    assert_eq!(app.workbench.search().matches.len(), 1);
    let screen = app.frame(100, 25);
    let row = screen.row_text(3);
    let x = row[..row.find("Straße").unwrap()].chars().count() as u16;
    assert_eq!(screen.cell(x, 3).style, crate::CellStyle::Reverse);
    key(&mut app, KeyCode::F(3), KeyModifiers::NONE);
    assert_eq!(
        app.workbench.active_editor().unwrap().selection(),
        Some(0..7)
    );
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    key(&mut app, KeyCode::F(3), KeyModifiers::CONTROL);
    while app.workbench.search_running() {
        app.poll_tasks();
    }
    assert_eq!(app.workbench.search().query, "Straße");
    key(&mut app, KeyCode::Char('g'), KeyModifiers::CONTROL);
    assert!(matches!(
        app.overlay,
        UiOverlay::Prompt {
            kind: crate::state::PromptKind::GoToPosition,
            ..
        }
    ));
    app.submit_prompt(crate::state::PromptKind::GoToPosition, "2:2".into());
    assert_eq!(
        app.workbench.active_position().unwrap(),
        jc_text::TextPosition { line: 1, column: 1 }
    );
    let before = app.workbench.active_editor().unwrap().cursor;
    app.open_prompt(crate::state::PromptKind::GoToPosition);
    app.submit_prompt(crate::state::PromptKind::GoToPosition, "0:0".into());
    assert_eq!(app.workbench.active_editor().unwrap().cursor, before);
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    key(&mut app, KeyCode::Char('f'), KeyModifiers::CONTROL);
    key(&mut app, KeyCode::Up, KeyModifiers::ALT);
    assert!(matches!(&app.overlay,UiOverlay::Find{input} if input==&app.workbench.search().query));
}

#[test]
fn document_search_escape_and_input_limits_leave_no_hidden_work_or_edits() {
    let mut app = TuiApp::new(LaunchOptions::default());
    app.workbench.new_file("large.txt", TextEncoding::Utf8);
    app.workbench.insert_text(&"x".repeat(256 * 1024)).unwrap();
    key(&mut app, KeyCode::Char('f'), KeyModifiers::CONTROL);
    text(&mut app, "needle");
    assert!(app.workbench.search_running());
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(!app.workbench.search_running());
    assert_eq!(
        app.workbench.search().status,
        jc_app::SearchStatus::Cancelled
    );
    key(&mut app, KeyCode::Char('f'), KeyModifiers::CONTROL);
    assert!(app.workbench.search_running());
    let before = app.workbench.search().query.clone();
    app.handle_event(Event::Paste("q".repeat(5000)));
    assert_eq!(app.workbench.search().query, before);
    assert!(app
        .registry
        .is_enabled(CommandId::SearchToggleRegex, &app.command_context()));
    app.dispatch(CommandId::SearchToggleRegex);
    assert!(app.workbench.search().regex);
}

#[test]
fn project_replace_preview_cancel_confirmation_and_file_scheduling_are_explicit() {
    use crate::state::PromptKind;
    use jc_app::ProjectSearchState;
    use std::time::{Duration, Instant};
    let root = std::env::temp_dir().join(format!("jc-tui-project-replace-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    for index in 0..3 {
        std::fs::write(root.join(format!("{index}.p")), b"a a\r\n").unwrap();
    }
    let mut app = TuiApp::new(LaunchOptions::default());
    app.submit_prompt(PromptKind::OpenFolder, root.to_str().unwrap().into());
    app.project_search
        .start(
            &app.workbench,
            &root,
            jc_app::ProjectSearchOptions::literal("a"),
        )
        .unwrap();
    app.project_results = true;
    let deadline = Instant::now() + Duration::from_secs(3);
    while app.project_search.state == ProjectSearchState::Running {
        app.poll_tasks();
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(app
        .registry
        .is_enabled(CommandId::SearchReplaceProject, &app.command_context()));
    app.dispatch(CommandId::SearchReplaceProject);
    app.submit_prompt(PromptKind::ProjectReplacement, "b".into());
    assert!(matches!(
        app.overlay,
        UiOverlay::ProjectReplacePreview { .. }
    ));
    let screen = app.frame(100, 30);
    assert!((0..30).any(|row| screen.row_text(row).contains("+ b")));
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    for index in 0..3 {
        assert_eq!(
            std::fs::read(root.join(format!("{index}.p"))).unwrap(),
            b"a a\r\n"
        );
    }
    app.submit_prompt(PromptKind::ProjectReplacement, "b".into());
    key(&mut app, KeyCode::Enter, KeyModifiers::CONTROL);
    assert!(app.project_replace.is_some());
    app.poll_tasks();
    assert_eq!(app.project_replace.as_ref().unwrap().files_remaining(), 2);
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.project_replace.is_none());
    let changed = (0..3)
        .filter(|index| std::fs::read(root.join(format!("{index}.p"))).unwrap() == b"b b\r\n")
        .count();
    assert_eq!(changed, 1);
    drop(app);
    for index in 0..3 {
        std::fs::remove_file(root.join(format!("{index}.p"))).unwrap();
    }
    std::fs::remove_dir(root).unwrap();
}

#[test]
fn project_search_prompts_cancel_and_deliver_navigable_rows_without_more_input() {
    use crate::state::PromptKind;
    use jc_app::{ProjectSearchState, Wakeup};
    use std::time::{Duration, Instant};
    let root = std::env::temp_dir().join(format!("jc-tui-project-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("source.p");
    std::fs::write(&path, "a niño\r\n").unwrap();
    let wake = Wakeup::default();
    let mut app = TuiApp::with_wakeup(LaunchOptions::default(), wake.clone());
    app.submit_prompt(PromptKind::OpenFolder, root.to_str().unwrap().into());
    assert!(app
        .registry
        .is_enabled(CommandId::SearchProject, &app.command_context()));
    assert!(!app
        .registry
        .is_enabled(CommandId::SearchReplaceProject, &app.command_context()));
    for cancel in [true, false] {
        key(
            &mut app,
            KeyCode::Char('f'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        );
        text(&mut app, "niño");
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        text(&mut app, "*.p");
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.project_results);
        assert_eq!(app.project_search.state, ProjectSearchState::Running);
        if cancel {
            key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
            assert_eq!(app.project_search.state, ProjectSearchState::Cancelled);
            continue;
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        while app.project_search.state == ProjectSearchState::Running {
            let ticket = wake.ticket();
            app.poll_tasks();
            assert!(Instant::now() < deadline);
            if app.project_search.state == ProjectSearchState::Running
                && !app.project_search.ready()
            {
                wake.wait(ticket, Some(Duration::from_millis(100)));
            }
        }
        assert_eq!(app.project_search.rows().len(), 1);
        let screen = app.frame(100, 30);
        assert!((0..30).any(|row| screen.row_text(row).contains("source.p:1:3")));
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.focus.primary(), PrimaryFocus::Editor);
        assert_eq!(app.workbench.active_position().unwrap().column, 2);
        assert_eq!(
            app.workbench.active_bytes().unwrap(),
            "a niño\r\n".as_bytes()
        );
    }
    drop(app);
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(root).unwrap();
}
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use jc_app::{CommandId, PaneId, PrimaryFocus};

fn key(app: &mut TuiApp, code: KeyCode, modifiers: KeyModifiers) {
    app.handle_event(Event::Key(KeyEvent::new(code, modifiers)));
}

#[test]
fn opening_legacy_from_prompt_keeps_encoding_choice_until_selection() {
    use crate::state::PromptKind;
    let path = std::env::temp_dir().join(format!("jc-f1-legacy-{}.p", std::process::id()));
    std::fs::write(&path, b"ni\xf1o\r\n").unwrap();
    let mut app = TuiApp::new(LaunchOptions::default());
    app.open_prompt(PromptKind::Open);
    app.submit_prompt(PromptKind::Open, path.to_str().unwrap().to_owned());
    assert!(matches!(app.overlay, UiOverlay::EncodingChoice));
    assert_eq!(app.workbench.active_encoding(), None);
    app.close_overlay();
    app.dispatch(CommandId::FileClose);
    app.dispatch(CommandId::FileReopenClosed);
    assert!(matches!(app.overlay, UiOverlay::EncodingChoice));
    app.choose_encoding(TextEncoding::Iso8859_1);
    assert!(matches!(app.overlay, UiOverlay::None));
    assert_eq!(app.workbench.active_bytes().unwrap(), b"ni\xf1o\r\n");
    app.workbench.insert_text("é").unwrap();
    assert_eq!(app.workbench.active_bytes().unwrap(), b"\xe9ni\xf1o\r\n");
    std::fs::remove_file(path).unwrap();
}

fn text(app: &mut TuiApp, value: &str) {
    for ch in value.chars() {
        key(app, KeyCode::Char(ch), KeyModifiers::NONE);
    }
}

#[test]
fn composing_prepared_frames_changes_neither_views_nor_text_caches() {
    let mut app = TuiApp::new(LaunchOptions::default());
    let tab = app
        .workbench
        .new_file("readonly-frame.txt", TextEncoding::Utf8);
    app.workbench.insert_text(&"abc\r\n".repeat(100)).unwrap();
    app.workbench.split_editor_right();
    let presentation = app.prepare_frame(140, 30);
    let views = format!("{:?}", presentation.app.workbench.panes());
    let stats = presentation.app.workbench.text_statistics(tab);
    let first = crate::render::render(&presentation);
    let second = crate::render::render(&presentation);
    assert_eq!(first, second);
    assert_eq!(views, format!("{:?}", presentation.app.workbench.panes()));
    assert_eq!(stats, presentation.app.workbench.text_statistics(tab));
}

fn diagnostic_provider(
    snapshot: &jc_app::TaskSnapshot,
    _: &jc_app::Cancellation,
    out: &mut jc_app::TaskOutput,
) -> Result<(), jc_app::TaskFailure> {
    out.push(jc_app::TaskItem::Diagnostic {
        range: snapshot.range.start..snapshot.range.start + 1,
        message: "background diagnostic".to_owned(),
    })?;
    out.push(jc_app::TaskItem::ProposedEdit {
        range: snapshot.range.clone(),
        text: "proposed".to_owned(),
    })
}
#[test]
fn background_completion_wakes_and_updates_panels_without_input_or_applying_edits() {
    use std::time::Duration;
    let wakeup = jc_app::Wakeup::default();
    let mut app = TuiApp::with_wakeup(LaunchOptions::default(), wakeup.clone());
    app.workbench.new_file("task-frame.txt", TextEncoding::Utf8);
    app.workbench.insert_text("abc").unwrap();
    let id = app.workbench.active_document().unwrap().id;
    let ticket = wakeup.ticket();
    app.tasks
        .submit(
            &app.workbench,
            jc_app::JobKey(1),
            id,
            None,
            0..3,
            diagnostic_provider,
        )
        .unwrap();
    wakeup.wait(ticket, Some(Duration::from_secs(1)));
    assert_ne!(wakeup.ticket(), ticket);
    assert!(app.poll_tasks().changed);
    assert!(app.workbench.problems()[0].contains("background diagnostic"));
    assert!(app.workbench.output()[0].contains("proposed"));
    assert_eq!(app.workbench.active_bytes().unwrap(), b"abc");
    let target = app.workbench.problems()[0].target.as_ref().unwrap();
    assert_eq!(target.stamp.document, id);
    app.dispatch(jc_app::CommandId::ViewProblems);
    let p = app.prepare_frame(100, 24);
    let first = crate::render::render(&p);
    assert_eq!(first, crate::render::render(&p));
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(
        app.workbench.active_position(),
        Some(jc_text::TextPosition { line: 0, column: 0 })
    );
    assert_eq!(app.focus.primary(), jc_app::PrimaryFocus::Editor);
    app.workbench.insert_text("X").unwrap();
    app.dispatch(jc_app::CommandId::ViewProblems);
    let before = format!("{:?}", app.workbench.panes());
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(before, format!("{:?}", app.workbench.panes()));
    assert_eq!(app.focus.primary(), jc_app::PrimaryFocus::BottomPanel);
}

#[test]
fn drawing_shared_views_reads_visible_ranges_without_rescanning_the_document() {
    let mut app = TuiApp::new(LaunchOptions::default());
    let tab = app.workbench.new_file("f2-render.txt", TextEncoding::Utf8);
    app.workbench
        .insert_text(&"abcdefg\r\n".repeat(100000))
        .unwrap();
    app.workbench.split_editor_right();
    app.frame(140, 30);
    let before = app.workbench.text_statistics(tab).unwrap();
    let screen = app.frame(140, 30);
    let after = app.workbench.text_statistics(tab).unwrap();
    assert_eq!(before.0, after.0);
    assert!(after.1 - before.1 < 65536);
    assert!((0..screen.height()).any(|row| screen.row_text(row).contains("Group 1")));
}

fn palette(app: &mut TuiApp, label: &str) {
    key(
        app,
        KeyCode::Char('p'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    text(app, label);
    key(app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(matches!(app.overlay, UiOverlay::None));
}

#[test]
fn keyboard_and_palette_drive_split_pin_history_and_responsive_rendering() {
    let mut app = TuiApp::new(LaunchOptions::default());
    let first = app.workbench.new_file("left.txt", TextEncoding::Utf8);
    text(&mut app, "left text");
    palette(&mut app, "Split Right");
    key(&mut app, KeyCode::Char('2'), KeyModifiers::CONTROL);
    assert_eq!(app.workbench.panes().active(), PaneId::Secondary);
    palette(&mut app, "Pin / Unpin Reference");
    assert!(app
        .workbench
        .panes()
        .view(PaneId::Secondary)
        .unwrap()
        .is_pinned());
    let second = app.workbench.new_file("right.txt", TextEncoding::Utf8);
    assert_eq!(app.workbench.panes().active(), PaneId::Primary);
    text(&mut app, "right text");
    let wide = app.frame(140, 30);
    let headers = wide.row_text(2);
    assert!(headers.contains("Group 1: right.txt"));
    assert!(headers.contains("Group 2: left.txt [PIN]"));
    assert!(wide.row_text(3).contains("right text"));
    assert!(wide.row_text(3).contains("left text"));
    let narrow = app.frame(45, 15);
    assert!(narrow.row_text(2).contains("Group 1: right.txt"));
    assert!(!narrow.row_text(2).contains("Group 2"));
    key(&mut app, KeyCode::Char('2'), KeyModifiers::CONTROL);
    let narrow = app.frame(45, 15);
    assert!(narrow.row_text(2).contains("Group 2: left.txt"));
    key(&mut app, KeyCode::Char('1'), KeyModifiers::CONTROL);
    key(&mut app, KeyCode::Left, KeyModifiers::ALT);
    assert_eq!(app.workbench.tabs().active_id(), Some(first));
    key(&mut app, KeyCode::Right, KeyModifiers::ALT);
    assert_eq!(app.workbench.tabs().active_id(), Some(second));
    key(&mut app, KeyCode::F(6), KeyModifiers::NONE);
    assert_ne!(app.focus.primary(), PrimaryFocus::Editor);
    palette(&mut app, "Focus Group 2");
    assert_eq!(app.focus.primary(), PrimaryFocus::Editor);
    assert_eq!(app.workbench.panes().active(), PaneId::Secondary);
    palette(&mut app, "One Group");
    assert!(!app.workbench.panes().is_split());
    assert_eq!(app.workbench.tabs().active_id(), Some(second));
}

#[test]
fn explorer_open_to_side_works_without_an_existing_document() {
    let root = std::env::temp_dir().join(format!("jc-tui-side-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("from-explorer.txt");
    std::fs::write(&path, b"explorer").unwrap();
    let mut app = TuiApp::new(LaunchOptions::default());
    app.set_explorer_root(root.clone()).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        app.poll_tasks();
        let ready = app
            .explorer
            .root_id()
            .and_then(|id| app.explorer.node(id))
            .is_some_and(|n| n.state() == jc_app::explorer::ExplorerLoadState::Ready);
        if ready {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "Explorer notification deadline"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    app.explorer_view.last(&app.explorer);
    app.dispatch(CommandId::ViewExplorer);
    assert!(app
        .registry
        .is_enabled(CommandId::EditorOpenToSide, &app.command_context()));
    key(&mut app, KeyCode::Enter, KeyModifiers::CONTROL);
    assert!(app.workbench.panes().is_split());
    assert_eq!(app.workbench.active_bytes().unwrap(), b"explorer");
    assert_eq!(app.focus.primary(), PrimaryFocus::Editor);
    // Opening an empty secondary must still make that group the active target.
    assert_eq!(app.workbench.panes().active(), PaneId::Secondary);
    drop(app);
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(root).unwrap();
}

#[test]
fn view_menu_scrolls_to_the_selected_command_in_a_short_terminal() {
    let mut app = TuiApp::new(LaunchOptions::default());
    app.workbench.new_file("menu.txt", TextEncoding::Utf8);
    app.workbench
        .insert_text(&"XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX\n".repeat(12))
        .unwrap();
    app.dispatch(CommandId::MenuView);
    key(&mut app, KeyCode::Up, KeyModifiers::NONE); // wrap to the final registered command
    let screen = app.frame(45, 10);
    assert!((1..9).any(|row| screen.row_text(row).contains("Open Comparison Hunk")));
    // Menu interiors must be opaque even when shorter labels leave spare cells.
    for row in 2..8 {
        assert!(!screen
            .row_text(row)
            .chars()
            .skip(3)
            .take(39)
            .any(|ch| ch == 'X'));
    }
}
