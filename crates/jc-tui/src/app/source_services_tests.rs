use super::TuiApp;
use crate::{render, state::LaunchOptions};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use jc_app::{source_services::SourceState, CommandId, PrimaryFocus, SidebarView};
use jc_text::{TextEncoding, TextPosition};
use std::path::PathBuf;
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
#[ignore = "requiere el proyecto sintético de esquema de OpenEdge"]
fn progress_real_schema_completion_tui() {
    let root = PathBuf::from(std::env::var_os("CIDE_TEST_PROGRESS_SCHEMA_PROJECT").unwrap());
    let mut app = TuiApp::new(LaunchOptions::default());
    app.set_explorer_root(root.clone()).unwrap();
    app.workbench
        .new_file(root.join("completar-tui.p"), TextEncoding::Utf8);
    assert!(!app.command_context().source_schema_completion_available);
    app.dispatch(CommandId::ProgressRefreshSchema);
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.progress_schema.busy() {
        app.poll_tasks();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    app.focus.focus(PrimaryFocus::Editor, &app.panels);
    assert!(
        app.command_context().source_schema_completion_available,
        "{}",
        app.status
    );
    for (case, (source, label, suffix)) in [
        (
            "FOR EACH negocio.cust",
            "Tabla · devlogica.customer",
            "omer",
        ),
        ("DISPLAY negocio.customer.na", "Campo · name", "me"),
        (
            "FIND FIRST negocio.customer USE-INDEX cust",
            "Índice · cust-num",
            "-num",
        ),
        (
            "{declaraciones.i}\nFOR EACH ttTra",
            "Tabla temporal · ttTrabajo",
            "bajo",
        ),
        (
            "{declaraciones.i}\nFOR EACH bTra",
            "Buffer · bTrabajo",
            "bajo",
        ),
        (
            "{declaraciones.i}\nDISPLAY bCliente.na",
            "Campo · name",
            "me",
        ),
        (
            "{declaraciones.i}\nDISPLAY bTrabajo.Co",
            "Campo · Codigo",
            "digo",
        ),
        (
            "{declaraciones.i}\nFIND FIRST bTrabajo USE-INDEX porCo",
            "Índice · porCodigo",
            "digo",
        ),
        (
            "{declaraciones.i}\nFIND FIRST ttCopia USE-INDEX cust",
            "Índice · cust-num",
            "-num",
        ),
        (
            "{declaraciones.i}\nDISPLAY ttCopia.na",
            "Campo · name",
            "me",
        ),
        (
            "{declaraciones.i}\nDISPLAY vIncl",
            "Variable · vIncluido",
            "uido",
        ),
        (
            "{declaraciones.i}\nDISPLAY fIncl",
            "Función · fIncluida",
            "uida",
        ),
        (
            "{declaraciones.i}\nRUN pIncl",
            "Procedimiento · pIncluido",
            "uido",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        app.workbench
            .new_file(root.join(format!("tui-{case}.p")), TextEncoding::Utf8);
        app.workbench.insert_text(source).unwrap();
        key(
            &mut app,
            KeyCode::Char(' '),
            KeyModifiers::CONTROL | KeyModifiers::ALT,
        );
        finish(&mut app);
        assert!(app.showing_source_popup(), "{}", app.status);
        let index = app
            .source
            .items()
            .iter()
            .position(|item| item.label.to_lowercase() == label.to_lowercase())
            .unwrap_or_else(|| panic!("{source}: {:?}", app.source.items()));
        app.source.select_relative(index as isize);
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(!app.showing_source_popup());
        assert_eq!(
            String::from_utf8(app.workbench.active_bytes().unwrap())
                .unwrap()
                .to_lowercase(),
            format!("{source}{suffix}").to_lowercase()
        );
        app.workbench.undo().unwrap();
        assert_eq!(app.workbench.active_bytes().unwrap(), source.as_bytes());
    }
}

#[test]
#[ignore = "requires a configured local LSP server and fixture project"]
fn source_tui_lsp_language_services() {
    let root = PathBuf::from(std::env::var_os("CIDE_LSP_TEST_PROJECT").unwrap());
    let path = root.join(std::env::var_os("CIDE_LSP_TEST_FILE").unwrap());
    let text = std::fs::read_to_string(&path).unwrap();
    let mut app = TuiApp::new(LaunchOptions {
        files: vec![path.clone()],
        encoding: Some(TextEncoding::Utf8),
    });
    app.set_explorer_root(root).unwrap();
    let tab = app.workbench.tabs().active_id().unwrap();
    assert!(app
        .workbench
        .jump_to_location(tab, text.find(".calcu").unwrap() + 6));
    app.dispatch(CommandId::SourceLspComplete);
    finish(&mut app);
    assert!(app.showing_source_popup());
    let index = app
        .source
        .items()
        .iter()
        .position(|item| item.label == "calcular")
        .unwrap();
    app.source.select_relative(index as isize);
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(String::from_utf8(app.workbench.active_bytes().unwrap())
        .unwrap()
        .contains("data.calcular"));
    app.workbench.undo().unwrap();
    assert_eq!(app.workbench.active_bytes().unwrap(), text.as_bytes());
    app.dispatch(CommandId::SourceLspDiagnostics);
    finish(&mut app);
    assert_eq!(app.panels.bottom(), jc_app::BottomPanel::Problems);
    assert!(
        app.workbench
            .task_panels()
            .rows(jc_app::task_panels::PanelKind::Problems)
            .iter()
            .any(|row| row.text.contains("missingCideSymbol")),
        "{}",
        app.status
    );
    assert_eq!(std::fs::read_to_string(path).unwrap(), text);
}

#[test]
#[ignore = "requires a configured local LSP server and fixture project"]
fn source_tui_lsp_navigation() {
    let root = PathBuf::from(std::env::var_os("CIDE_LSP_TEST_PROJECT").unwrap());
    let path = root.join(std::env::var_os("CIDE_LSP_TEST_FILE").unwrap());
    let destination = root.join(std::env::var_os("CIDE_LSP_TEST_DEST").unwrap());
    let text = std::fs::read_to_string(&path).unwrap();
    let mut app = TuiApp::new(LaunchOptions {
        files: vec![path],
        encoding: Some(TextEncoding::Utf8),
    });
    app.set_explorer_root(root).unwrap();
    let tab = app.workbench.tabs().active_id().unwrap();
    let pane = app.workbench.panes().active();
    assert!(app
        .workbench
        .jump_to_pane_location(pane, tab, text.rfind("calcular").unwrap()));
    key(&mut app, KeyCode::F(12), KeyModifiers::SHIFT);
    finish(&mut app);
    assert!(app.source.items().len() >= 2, "{}", app.source.message());
    app.dispatch(CommandId::SourceDefinition);
    finish(&mut app);
    assert_eq!(app.source.items().len(), 1, "{}", app.source.message());
    app.dispatch(CommandId::SourceOpenToSide);
    assert!(app.workbench.panes().is_split());
    assert_eq!(
        app.workbench.active_path().unwrap(),
        destination.canonicalize().unwrap()
    );
}

#[test]
fn source_tui_multilanguage_outline_and_side_navigation() {
    let root = std::env::temp_dir().join(format!(
        "cide-outline-languages-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    std::fs::create_dir(root.join(".cide")).unwrap();
    std::fs::write(root.join(".cide/source-roots"), b"sources\n").unwrap();
    for (name, text, symbol, target) in [
        (
            "code.mjs",
            "import './mod.js';\nfunction calcular() {}\n",
            "Función: calcular",
            "mod.js",
        ),
        (
            "code.cjs",
            "const calcular = require('./lib');\n",
            "Constante: calcular",
            "lib/index.js",
        ),
        (
            "code.tsx",
            "import type { Item } from './item.js';\ninterface Consulta {}\n",
            "Interfaz: Consulta",
            "item.ts",
        ),
        (
            "code.php",
            "<p>class Falsa</p>\n<?php $ejemplo = <<<'FIN'\nclass Falsa2\nFIN;\nrequire_once __DIR__ . '/vista.php';\nfunction calcular() {}\n",
            "Función: calcular",
            "vista.php",
        ),
        (
            "Main.java",
            "import pkg.Item;\nclass Main { void calcular() {} }\n",
            "Método: calcular",
            "sources/pkg/Item.java",
        ),
        (
            "Main.cs",
            "using static App.Util;\nclass Main { int Calcular() => 2; }\n",
            "Método: Calcular",
            "sources/App/Util.cs",
        ),
    ] {
        let source = root.join(name);
        let dest = root.join(target);
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::write(&source, text).unwrap();
        std::fs::write(&dest, b"// destino\n").unwrap();
        let mut app = TuiApp::new(LaunchOptions {
            files: vec![source.clone()],
            encoding: Some(TextEncoding::Utf8),
        });
        app.set_explorer_root(root.clone()).unwrap();
        let tab = app.workbench.tabs().active_id().unwrap();
        let keyword = text.find("require_once").unwrap_or(0);
        assert_eq!(
            app.workbench.syntax_rows(tab, 0, 8).kind_at(keyword),
            jc_app::SyntaxKind::Keyword,
            "{name}"
        );
        app.dispatch(CommandId::ViewSymbols);
        finish(&mut app);
        assert!(!app.source.items().iter().any(|item| item.label.contains("Falsa")));
        assert!(
            app.source.items().iter().any(|item| item.label == symbol),
            "{name}: {:?}",
            app.source.items()
        );
        app.dispatch(CommandId::SourceDependencies);
        finish(&mut app);
        assert_eq!(
            app.source.items().len(),
            1,
            "{name}: {}",
            app.source.message()
        );
        app.dispatch(CommandId::SourceOpenToSide);
        assert!(app.workbench.panes().is_split(), "{name}: {}", app.status);
        assert_eq!(
            app.workbench.active_path().unwrap(),
            dest.canonicalize().unwrap(),
            "{name}"
        );
        assert_eq!(std::fs::read(&source).unwrap(), text.as_bytes());
    }
    std::fs::write(root.join("reverse.js"), b"import './item.ts';\n").unwrap();
    let mut app = TuiApp::new(LaunchOptions {
        files: vec![root.join("item.ts")],
        encoding: Some(TextEncoding::Utf8),
    });
    app.set_explorer_root(root.clone()).unwrap();
    app.dispatch(CommandId::SourceIncoming);
    finish(&mut app);
    assert_eq!(
        app.source.state(),
        SourceState::Ready,
        "{}",
        app.source.message()
    );
    assert!(app
        .source
        .items()
        .iter()
        .any(|item| item.label == "reverse.js:1"));
    assert!(app
        .source
        .items()
        .iter()
        .any(|item| item.label == "code.tsx:1"));
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
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
