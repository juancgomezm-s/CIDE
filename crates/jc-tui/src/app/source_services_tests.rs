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
fn tui_routine_relationships_show_both_direction_icons() {
    use crate::screen::{CellStyle, Screen};
    use jc_app::source_services::RelationMark;
    let assert_mark = |screen: &Screen, label: &str, mark: RelationMark| {
        let icon = mark.icon().chars().next().unwrap();
        assert!((0..screen.height()).any(|y| {
            screen.row_text(y).contains(label)
                && (0..screen.width()).any(|x| {
                    let cell = screen.cell(x, y);
                    cell.ch == icon && cell.style == CellStyle::Relation(mark)
                })
        }));
    };
    let source = "fn principal() { destino(); externo(); } fn destino() {}";
    let mut app = editor(source, "relaciones.rs");
    let first = source.find("principal").unwrap();
    app.workbench
        .set_linear_selection_bytes(first, first)
        .unwrap();
    app.dispatch(CommandId::SourceCalls);
    finish(&mut app);
    let screen = render::render(&app.prepare_frame(140, 30));
    assert_mark(&screen, "↗ destino", RelationMark::Outgoing);
    assert_mark(&screen, "? externo", RelationMark::Unresolved);
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    app.dispatch(CommandId::SourceIncoming);
    finish(&mut app);
    let screen = render::render(&app.prepare_frame(140, 30));
    assert_mark(&screen, "↙ principal", RelationMark::Incoming);
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
#[ignore = "requiere el proyecto sintético de OpenEdge con el ejemplo de asistencia"]
fn progress_real_definition_tui_f12_opens_exact_include_and_local_routine() {
    let root = PathBuf::from(std::env::var_os("CIDE_TEST_PROGRESS_SCHEMA_PROJECT").unwrap());
    let path = root.join("asistencia.p");
    let include = root.join("primero/detalle.i");
    let text = std::fs::read_to_string(&path).unwrap();
    let declarations = std::fs::read_to_string(&include).unwrap();
    let mut app = TuiApp::new(LaunchOptions {
        files: vec![path],
        encoding: Some(TextEncoding::Utf8),
    });
    app.set_explorer_root(root.clone()).unwrap();
    let tab = app.workbench.tabs().active_id().unwrap();
    assert!(app
        .workbench
        .jump_to_location(tab, text.find("bCliente.name").unwrap() + 2));
    key(&mut app, KeyCode::F(12), KeyModifiers::NONE);
    finish(&mut app);
    assert!(app.showing_source_popup());
    assert_eq!(app.source.items().len(), 1, "{}", app.source.message());
    key(&mut app, KeyCode::Enter, KeyModifiers::CONTROL);
    assert!(app.workbench.panes().is_split());
    assert_eq!(
        app.workbench.active_path().unwrap(),
        include.canonicalize().unwrap()
    );
    assert_eq!(
        app.workbench.active_editor().unwrap().cursor,
        declarations.find("bCliente").unwrap()
    );

    let source = "RUN posterior.\nPROCEDURE posterior:\nEND PROCEDURE.";
    let tab = app
        .workbench
        .new_file(root.join("navegacion-tui.p"), TextEncoding::Utf8);
    app.workbench.insert_text(source).unwrap();
    assert!(app.workbench.jump_to_location(tab, 6));
    key(&mut app, KeyCode::F(12), KeyModifiers::NONE);
    finish(&mut app);
    assert_eq!(app.source.items().len(), 1, "{}", app.source.message());
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(app.workbench.tabs().active_id(), Some(tab));
    assert_eq!(
        app.workbench.active_editor().unwrap().cursor,
        source.rfind("posterior").unwrap()
    );
    assert_eq!(app.workbench.active_bytes().unwrap(), source.as_bytes());
}

#[test]
#[ignore = "requiere el proyecto sintético de OpenEdge con el ejemplo de asistencia"]
fn progress_real_references_tui_uses_shift_f12_and_opens_the_selected_result() {
    let root = PathBuf::from(std::env::var_os("CIDE_TEST_PROGRESS_SCHEMA_PROJECT").unwrap());
    let path = root.join("asistencia.p");
    let consumer = root.join("consumidores/consumidor.p");
    let text = std::fs::read_to_string(&path).unwrap();
    let declarations = std::fs::read_to_string(&consumer).unwrap();
    let mut app = TuiApp::new(LaunchOptions {
        files: vec![path],
        encoding: Some(TextEncoding::Utf8),
    });
    app.set_explorer_root(root).unwrap();
    let tab = app.workbench.tabs().active_id().unwrap();
    assert!(app
        .workbench
        .jump_to_location(tab, text.find("vIncluido").unwrap() + 3));
    key(&mut app, KeyCode::F(12), KeyModifiers::SHIFT);
    finish(&mut app);
    assert!(app.showing_source_popup());
    assert_eq!(app.source.items().len(), 3, "{}", app.source.message());
    key(&mut app, KeyCode::Down, KeyModifiers::NONE);
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(app.workbench.tabs().active_id(), Some(tab));
    assert_eq!(
        app.workbench.active_editor().unwrap().cursor,
        text.find("vIncluido").unwrap()
    );
    key(&mut app, KeyCode::F(12), KeyModifiers::SHIFT);
    finish(&mut app);
    app.source.select_relative(2);
    key(&mut app, KeyCode::Enter, KeyModifiers::CONTROL);
    assert!(app.workbench.panes().is_split());
    assert_eq!(
        app.workbench.active_path().unwrap(),
        consumer.canonicalize().unwrap()
    );
    assert_eq!(
        app.workbench.active_editor().unwrap().cursor,
        declarations.rfind("vIncluido").unwrap()
    );
}

#[test]
#[ignore = "requiere el proyecto sintético de OpenEdge con el ejemplo de asistencia"]
fn progress_real_outline_tui_lists_program_and_nested_include_and_opens_to_side() {
    let root = PathBuf::from(std::env::var_os("CIDE_TEST_PROGRESS_SCHEMA_PROJECT").unwrap());
    let path = root.join("asistencia.p");
    let include = root.join("primero/detalle.i");
    let declarations = std::fs::read_to_string(&include).unwrap();
    let mut app = TuiApp::new(LaunchOptions {
        files: vec![path],
        encoding: Some(TextEncoding::Utf8),
    });
    app.set_explorer_root(root).unwrap();
    app.dispatch(CommandId::ViewSymbols);
    finish(&mut app);
    assert_eq!(app.panels.sidebar(), SidebarView::Symbols);
    let items = app.source.items();
    assert!(items.iter().any(|item| item.label.contains("vPrograma")));
    assert!(items
        .iter()
        .any(|item| item.label.contains("ttTrabajo.Codigo")));
    assert!(!items.iter().any(|item| item.label.contains("vIncorrecto")));
    let index = items
        .iter()
        .position(|item| item.label.contains("pIncluido"))
        .unwrap();
    app.source.select_relative(index as isize);
    key(&mut app, KeyCode::Enter, KeyModifiers::CONTROL);
    assert!(app.workbench.panes().is_split());
    assert_eq!(
        app.workbench.active_path().unwrap(),
        include.canonicalize().unwrap()
    );
    assert_eq!(
        app.workbench.active_editor().unwrap().cursor,
        declarations.find("pIncluido").unwrap()
    );
    assert_eq!(
        app.workbench.active_bytes().unwrap(),
        declarations.as_bytes()
    );
}

#[test]
fn source_tui_abl_breadcrumbs_follow_the_cursor_and_open_the_scope() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../progress/examples/schema-assistance")
        .canonicalize()
        .unwrap();
    let path = root.join("primero/paquete/Derivada.cls");
    let code = std::fs::read_to_string(&path).unwrap();
    let mut app = TuiApp::new(LaunchOptions {
        files: vec![path],
        encoding: Some(TextEncoding::Utf8),
    });
    app.set_explorer_root(root).unwrap();
    let tab = app.workbench.tabs().active_id().unwrap();
    let first = code.find("RETURN SUPER").unwrap();
    let second = code.find("THIS-OBJECT:Titulo").unwrap();
    assert!(app.workbench.jump_to_location(tab, first));
    app.poll_tasks();
    finish(&mut app);
    assert_eq!(app.focus.primary(), PrimaryFocus::Editor);
    assert_eq!(app.panels.sidebar(), SidebarView::Explorer);
    let old_click = {
        let frame = app.prepare_frame(120, 24);
        let row = render::render(&frame).row_text(2);
        assert!(
            row.contains("Derivada.cls › paquete.Derivada › Saludo"),
            "{row}"
        );
        let symbol = frame
            .editor(jc_app::PaneId::Primary)
            .symbol_path
            .last()
            .unwrap();
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: symbol.x + symbol.width - 1,
            row: 2,
            modifiers: KeyModifiers::CONTROL,
        }
    };
    assert!(app.workbench.jump_to_location(tab, second));
    assert!(!app.handle_mouse(old_click));
    assert!(app.workbench.jump_to_location(tab, 0));
    assert!(!app.frame(120, 24).row_text(2).contains('›'));
    assert!(app.workbench.jump_to_location(tab, second));
    let click = {
        let frame = app.prepare_frame(120, 24);
        let row = render::render(&frame).row_text(2);
        assert!(
            row.contains(" › Probar") && !row.contains(" › Saludo"),
            "{row}"
        );
        let symbol = frame
            .editor(jc_app::PaneId::Primary)
            .symbol_path
            .last()
            .unwrap();
        MouseEvent {
            column: symbol.x + symbol.width - 1,
            ..old_click
        }
    };
    assert_eq!(app.tasks.outstanding(), 0);
    assert!(app.handle_mouse(click));
    assert!(app.workbench.panes().is_split());
    assert_eq!(
        app.workbench.active_editor().unwrap().cursor,
        code.find("Probar").unwrap()
    );
    assert_eq!(app.workbench.active_bytes().unwrap(), code.as_bytes());

    app.poll_tasks();
    finish(&mut app);
    assert!(app.workbench.jump_to_location(tab, second));
    app.workbench.insert_text("/* edición */ ").unwrap();
    assert!(!app.frame(160, 24).row_text(2).contains(" › Probar"));
    app.poll_tasks();
    finish(&mut app);
    assert!(app.frame(160, 24).row_text(2).contains(" › Probar"));
}

#[test]
#[ignore = "requiere OpenEdge y el proyecto sintético de asistencia"]
fn progress_real_xref_tui_opens_program_with_spaces_to_side() {
    let root = PathBuf::from(std::env::var_os("CIDE_TEST_PROGRESS_SCHEMA_PROJECT").unwrap());
    let source = root.join("asistencia.p");
    let target = root.join("consumidores/xref consumidor.p");
    let text = std::fs::read_to_string(&source).unwrap();
    let contents = std::fs::read_to_string(&target).unwrap();
    let mut app = TuiApp::new(LaunchOptions {
        files: vec![source],
        encoding: Some(TextEncoding::Utf8),
    });
    app.set_explorer_root(root).unwrap();
    let tab = app.workbench.tabs().active_id().unwrap();
    assert!(app.workbench.jump_to_location(
        tab,
        text.find("bCliente.name").unwrap() + "bCliente.na".len()
    ));
    key(
        &mut app,
        KeyCode::F(12),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    finish(&mut app);
    assert!(app.showing_source_popup(), "{}", app.status);
    assert!(
        app.source.message().contains("OpenEdge 11.7"),
        "{}",
        app.source.message()
    );
    let index = app
        .source
        .items()
        .iter()
        .position(|item| item.label.contains("xref consumidor.p"))
        .unwrap_or_else(|| panic!("{:?}", app.source.items()));
    app.source.select_relative(index as isize);
    key(&mut app, KeyCode::Enter, KeyModifiers::CONTROL);
    assert!(app.workbench.panes().is_split());
    assert_eq!(
        app.workbench.active_path().unwrap(),
        target.canonicalize().unwrap()
    );
    assert_eq!(
        app.workbench.active_editor().unwrap().cursor,
        contents.find("    DISPLAY").unwrap()
    );
    assert_eq!(std::fs::read_to_string(target).unwrap(), contents);
    let tab = app.workbench.tabs().active_id().unwrap();
    for token in ["customer", "cust-num"] {
        assert!(app
            .workbench
            .jump_to_location(tab, contents.find(token).unwrap() + 2));
        key(
            &mut app,
            KeyCode::F(12),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        );
        finish(&mut app);
        let hits = app.source.items();
        assert!(
            hits.iter().any(|item| item.label.contains("asistencia.p")),
            "{token}: {}",
            app.source.message()
        );
        assert!(
            hits.iter()
                .any(|item| item.label.contains("xref consumidor.p")),
            "{token}: {}",
            app.source.message()
        );
        if token == "cust-num" {
            assert_eq!(hits.len(), 2, "{hits:?}");
        }
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    }
}

#[test]
#[ignore = "requiere el proyecto sintético de OpenEdge con las clases de herencia"]
fn progress_real_inheritance_tui_selects_override_and_opens_grandparent() {
    let root = PathBuf::from(std::env::var_os("CIDE_TEST_PROGRESS_SCHEMA_PROJECT").unwrap());
    let path = root.join("primero/paquete/Derivada.cls");
    let target = root
        .join("primero/paquete/Raiz.cls")
        .canonicalize()
        .unwrap();
    let source = std::fs::read_to_string(&path).unwrap();
    let method = std::fs::read_to_string(&target)
        .unwrap()
        .find("Saludo")
        .unwrap();
    let mut app = TuiApp::new(LaunchOptions {
        files: vec![path],
        encoding: Some(TextEncoding::Utf8),
    });
    app.set_explorer_root(root.clone()).unwrap();
    let tab = app.workbench.tabs().active_id().unwrap();
    assert!(app
        .workbench
        .jump_to_location(tab, source.find("THIS-OBJECT:Saludo").unwrap() + 14));
    key(&mut app, KeyCode::F(12), KeyModifiers::NONE);
    finish(&mut app);
    assert_eq!(app.source.items().len(), 1, "{}", app.source.message());
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(
        app.workbench.active_editor().unwrap().cursor,
        source.find("Saludo").unwrap()
    );
    assert!(app
        .workbench
        .jump_to_location(tab, source.find("SUPER:Saludo").unwrap() + 8));
    key(&mut app, KeyCode::F(12), KeyModifiers::NONE);
    finish(&mut app);
    key(&mut app, KeyCode::Enter, KeyModifiers::CONTROL);
    assert!(app.workbench.panes().is_split());
    assert_eq!(app.workbench.active_path(), Some(target.as_path()));
    assert_eq!(app.workbench.active_editor().unwrap().cursor, method);
    let caller = root.join("consumidores/herencia.p");
    let code = std::fs::read_to_string(&caller).unwrap();
    let class = std::fs::read_to_string(&target).unwrap();
    for (reference, name, declaration) in [
        ("objeto:Saludar(\"Ana\")", "Saludar", "Saludar(INPUT nombre"),
        ("objeto:Saludar(12)", "Saludar", "Saludar(INPUT cantidad"),
        (
            "objeto:Saludar(numero)",
            "Saludar",
            "Saludar(INPUT cantidad",
        ),
        ("objeto:Titulo", "Titulo", "Titulo AS"),
        ("objeto:Hijo:Titulo", "Titulo", "Titulo AS"),
        (
            "objeto:Obtener():Hijo:Saludar(numero)",
            "Saludar",
            "Saludar(INPUT cantidad",
        ),
        (
            "objeto:Ajustar(INPUT numero)",
            "Ajustar",
            "Ajustar(INPUT cantidad",
        ),
        (
            "objeto:Ajustar(OUTPUT numero)",
            "Ajustar",
            "Ajustar(OUTPUT cantidad",
        ),
        (
            "objeto:Ajustar(INPUT-OUTPUT numero)",
            "Ajustar",
            "Ajustar(INPUT-OUTPUT cantidad",
        ),
    ] {
        let tab = app
            .workbench
            .open_file(&caller, Some(TextEncoding::Utf8))
            .unwrap();
        let offset = code.find(reference).unwrap() + reference.find(name).unwrap();
        assert!(app.workbench.jump_to_location(tab, offset + 1));
        key(&mut app, KeyCode::F(12), KeyModifiers::NONE);
        finish(&mut app);
        assert_eq!(app.source.items().len(), 1, "{}", app.source.message());
        key(&mut app, KeyCode::Enter, KeyModifiers::CONTROL);
        assert_eq!(app.workbench.active_path(), Some(target.as_path()));
        assert_eq!(
            app.workbench.active_editor().unwrap().cursor,
            class.find(declaration).unwrap()
        );
    }
}

#[test]
#[ignore = "requiere el proyecto sintético de OpenEdge con los ejemplos USING"]
fn progress_real_using_tui_resolves_wildcard_from_include_and_opens_to_side() {
    let root = PathBuf::from(std::env::var_os("CIDE_TEST_PROGRESS_SCHEMA_PROJECT").unwrap());
    let path = root.join("consumidores/using-comodin.p");
    let target = root.join("primero/paquete/Elemento.cls");
    let text = std::fs::read_to_string(&path).unwrap();
    let class = std::fs::read_to_string(&target).unwrap();
    let mut app = TuiApp::new(LaunchOptions {
        files: vec![path],
        encoding: Some(TextEncoding::Utf8),
    });
    app.set_explorer_root(root).unwrap();
    let tab = app.workbench.tabs().active_id().unwrap();
    assert!(app
        .workbench
        .jump_to_location(tab, text.find("AS Elemento").unwrap() + 5));
    key(&mut app, KeyCode::F(12), KeyModifiers::NONE);
    finish(&mut app);
    assert_eq!(app.source.items().len(), 1, "{}", app.source.message());
    key(&mut app, KeyCode::Enter, KeyModifiers::CONTROL);
    assert!(app.workbench.panes().is_split());
    assert_eq!(
        app.workbench.active_path().unwrap(),
        target.canonicalize().unwrap()
    );
    assert_eq!(
        app.workbench.active_editor().unwrap().cursor,
        class.find("Elemento").unwrap()
    );
    assert_eq!(app.workbench.active_bytes().unwrap(), class.as_bytes());
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
    std::fs::write(
        root.join("reverse.js"),
        b"import './item.ts';\nexport * from './item.ts';\n",
    )
    .unwrap();
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
    let callers: Vec<_> = app
        .source
        .items()
        .iter()
        .filter(|item| item.label.starts_with("reverse.js:"))
        .map(|item| item.label.as_str())
        .collect();
    assert_eq!(callers, ["reverse.js:1"]);
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
fn source_tui_project_rename_previews_and_undoes_include_chain() {
    let root = std::env::temp_dir().join(format!("cide-tui-project-rename-{}", std::process::id()));
    let include = root.join("shared.i");
    let bridge = root.join("bridge.i");
    let main = root.join("main.p");
    let include_text = "DEFINE VARIABLE total AS INTEGER NO-UNDO INITIAL {&initial}.\n";
    let bridge_text = "{shared.i &initial={1}}\nASSIGN total = 2.\n";
    let main_text = "{bridge.i 0}\nASSIGN total = 1.\n";
    std::fs::create_dir(&root).unwrap();
    std::fs::write(&include, include_text).unwrap();
    std::fs::write(&bridge, bridge_text).unwrap();
    std::fs::write(&main, main_text).unwrap();
    let mut app = TuiApp::new(LaunchOptions::default());
    app.project_root = Some(root.canonicalize().unwrap());
    app.workbench
        .open_file(&include, Some(TextEncoding::Utf8))
        .unwrap();
    app.workbench
        .go_to_position(TextPosition {
            line: 0,
            column: 17,
        })
        .unwrap();
    key(&mut app, KeyCode::F(2), KeyModifiers::NONE);
    for ch in "sum-value".chars() {
        key(&mut app, KeyCode::Char(ch), KeyModifiers::NONE);
    }
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    finish(&mut app);
    assert_eq!(app.source.items().len(), 3, "{}", app.status);
    assert!(app
        .source
        .items()
        .iter()
        .any(|row| row.label.contains("bridge.i:2")));
    assert!(app
        .source
        .items()
        .iter()
        .any(|row| row.label.contains("main.p:2")));
    assert_eq!(
        app.workbench.active_bytes().unwrap(),
        include_text.as_bytes()
    );
    key(&mut app, KeyCode::F(9), KeyModifiers::NONE);
    finish(&mut app);
    assert!(!app.source_popup, "{}", app.status);
    assert_eq!(
        app.workbench.active_bytes().unwrap(),
        include_text.replace("total", "sum-value").as_bytes()
    );
    app.workbench.open_file(&main, None).unwrap();
    assert_eq!(
        app.workbench.active_bytes().unwrap(),
        main_text.replace("total", "sum-value").as_bytes()
    );
    app.workbench.undo().unwrap();
    assert_eq!(app.workbench.active_bytes().unwrap(), main_text.as_bytes());
    app.workbench.open_file(&bridge, None).unwrap();
    assert_eq!(
        app.workbench.active_bytes().unwrap(),
        bridge_text.as_bytes()
    );
    app.workbench.open_file(&include, None).unwrap();
    assert_eq!(
        app.workbench.active_bytes().unwrap(),
        include_text.as_bytes()
    );
    assert_eq!(std::fs::read(&main).unwrap(), main_text.as_bytes());
    drop(app);
    std::fs::remove_file(&main).unwrap();
    std::fs::remove_file(&bridge).unwrap();
    std::fs::remove_file(&include).unwrap();
    std::fs::remove_dir(&root).unwrap();
}
#[test]
fn source_tui_reviewed_imports_previews_and_applies_in_one_undo() {
    let root = std::env::temp_dir().join(format!(
        "cide-reviewed-imports-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let package = root.join("pack");
    std::fs::create_dir_all(&package).unwrap();
    for name in ["Alfa", "Beta"] {
        std::fs::write(
            package.join(format!("{name}.cls")),
            format!("CLASS pack.{name}:\nEND CLASS.\n"),
        )
        .unwrap();
    }
    let text = "/* Ítems. */\r\n  USING pack.Beta FROM PROPATH.\n\tUSING pack.Alfa FROM PROPATH.\r\nDEFINE VARIABLE dato AS CLASS Alfa NO-UNDO.";
    let expected = "/* Ítems. */\r\n  USING pack.Alfa FROM PROPATH.\n\tUSING pack.Beta FROM PROPATH.\r\nDEFINE VARIABLE dato AS CLASS Alfa NO-UNDO.";
    let mut app = TuiApp::new(LaunchOptions::default());
    app.workbench
        .new_file(root.join("imports.p"), TextEncoding::Utf8);
    app.workbench.insert_text(text).unwrap();
    app.dispatch(CommandId::SourceOrganizeImports);
    finish(&mut app);
    assert!(
        app.source
            .items()
            .iter()
            .any(|item| item.label == "- USING pack.Beta FROM PROPATH."),
        "{}",
        app.source.message()
    );
    assert!(app
        .source
        .items()
        .iter()
        .any(|item| item.label == "+ USING pack.Alfa FROM PROPATH."));
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(app.workbench.active_bytes().unwrap(), text.as_bytes());
    key(&mut app, KeyCode::Enter, KeyModifiers::CONTROL);
    assert_eq!(app.workbench.active_bytes().unwrap(), expected.as_bytes());
    app.workbench.undo().unwrap();
    assert_eq!(app.workbench.active_bytes().unwrap(), text.as_bytes());
    for name in ["Alfa", "Beta"] {
        std::fs::remove_file(package.join(format!("{name}.cls"))).unwrap();
    }
    std::fs::remove_dir(package).unwrap();
    std::fs::remove_dir(root).unwrap();
}
#[test]
fn rust_outline_in_tui_renders_and_opens_the_selected_method() {
    let text = "pub struct Datos { total: usize }\nimpl Datos { pub fn sumar(&self) {} }";
    let mut a = editor(text, "esquema.rs");
    a.dispatch(CommandId::ViewSymbols);
    finish(&mut a);
    assert_eq!(a.source.state(), SourceState::Ready, "{}", a.status);
    assert_eq!(a.panels.sidebar(), SidebarView::Symbols);
    let screen = render::render(&a.prepare_frame(140, 30));
    assert!((0..screen.height()).any(|row| screen.row_text(row).contains("sumar · Método")));
    key(&mut a, KeyCode::End, KeyModifiers::NONE);
    key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(a.focus.primary(), PrimaryFocus::Editor);
    assert_eq!(
        a.workbench.active_editor().unwrap().cursor,
        text.find("sumar").unwrap()
    );
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
fn abl_run_history_and_last_edit_work_with_tui_keys_and_mouse() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let root = std::env::temp_dir().join(format!("cide-run-tui-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let source = root.join("origen.p");
    let target = root.join("destino.p");
    std::fs::write(
        &source,
        b"/* origen */\nRUN destino.p.\nRUN Local.\nPROCEDURE Local:\nEND PROCEDURE.\n",
    )
    .unwrap();
    std::fs::write(&target, b"RETURN.\n").unwrap();
    let mut app = TuiApp::new(LaunchOptions {
        files: vec![source.clone()],
        encoding: Some(TextEncoding::Utf8),
    });
    app.project_root = Some(root.canonicalize().unwrap());
    app.workbench.insert_text(" ").unwrap();
    let origin = app.workbench.active_document().unwrap().tab_id;
    app.workbench
        .go_to_position(TextPosition { line: 1, column: 4 })
        .unwrap();
    let original = app.workbench.active_editor().unwrap().cursor;
    key(&mut app, KeyCode::Enter, KeyModifiers::CONTROL);
    finish(&mut app);
    assert_eq!(
        app.workbench.active_path().unwrap().file_name(),
        target.file_name(),
        "{}",
        app.status
    );
    key(&mut app, KeyCode::Left, KeyModifiers::CONTROL);
    assert_eq!(app.workbench.active_document().unwrap().tab_id, origin);
    assert_eq!(app.workbench.active_editor().unwrap().cursor, original);
    key(&mut app, KeyCode::Right, KeyModifiers::CONTROL);
    assert_eq!(
        app.workbench.active_path().unwrap().file_name(),
        target.file_name()
    );
    key(&mut app, KeyCode::Char('q'), KeyModifiers::CONTROL);
    assert_eq!(app.workbench.active_document().unwrap().tab_id, origin);
    assert_eq!(app.workbench.active_position().unwrap().line, 0);

    let screen = render::render(&app.prepare_frame(120, 28));
    let (column, row) = (0..screen.height())
        .find_map(|row| {
            let line = screen.row_text(row);
            let start = line.find("RUN Local.")? + "RUN ".len();
            Some((line[..start].chars().count() as u16, row))
        })
        .unwrap();
    for _ in 0..2 {
        app.prepare_frame(120, 28);
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            app.handle_event(Event::Mouse(MouseEvent {
                kind,
                column,
                row,
                modifiers: KeyModifiers::NONE,
            }));
        }
    }
    finish(&mut app);
    assert_eq!(
        app.workbench.active_position().unwrap().line,
        3,
        "{}",
        app.status
    );

    assert!(app.workbench.split_editor_right());
    assert_eq!(app.workbench.toggle_reference_pin(), Some(true));
    app.workbench
        .go_to_position(TextPosition { line: 1, column: 4 })
        .unwrap();
    key(&mut app, KeyCode::Enter, KeyModifiers::CONTROL);
    finish(&mut app);
    assert_eq!(app.workbench.panes().active(), jc_app::PaneId::Secondary);
    assert_eq!(
        app.workbench.active_path().unwrap().file_name(),
        target.file_name()
    );
    key(&mut app, KeyCode::Left, KeyModifiers::CONTROL);
    assert_eq!(app.workbench.active_document().unwrap().tab_id, origin);
    assert_eq!(app.workbench.active_editor().unwrap().cursor, original);
    key(&mut app, KeyCode::Right, KeyModifiers::CONTROL);
    assert_eq!(
        app.workbench.active_path().unwrap().file_name(),
        target.file_name()
    );
    drop(app);
    std::fs::remove_file(source).unwrap();
    std::fs::remove_file(target).unwrap();
    std::fs::remove_dir(root).unwrap();
}

#[test]
fn code_method_links_follow_and_return_through_tui_input_for_each_language() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    for (name, source) in [
        (
            "metodos.rs",
            "fn principal() { let f = destino; } fn destino() {}",
        ),
        (
            "metodos.cs",
            "class Demo { void principal() { destino(); } void destino() {} }",
        ),
        (
            "Metodos.java",
            "class Demo { void principal() { destino(); } void destino() {} }",
        ),
        (
            "metodos.php",
            "<?php function principal() { destino(); } function destino() {}",
        ),
        (
            "metodos.js",
            "function principal() { usar(destino); } function destino() {}",
        ),
        (
            "metodos.ts",
            "function principal(): void { usar(destino); } function destino(): void {}",
        ),
    ] {
        let mut app = TuiApp::new(LaunchOptions::default());
        app.workbench.new_file(name, TextEncoding::Utf8);
        app.workbench.insert_text(source).unwrap();
        let origin = source.find("destino").unwrap();
        let target = source.rfind("destino").unwrap();
        app.workbench
            .go_to_position(TextPosition {
                line: 0,
                column: origin,
            })
            .unwrap();
        app.prepare_frame(140, 28);
        key(&mut app, KeyCode::Enter, KeyModifiers::CONTROL);
        finish(&mut app);
        assert_eq!(
            app.workbench.active_editor().unwrap().cursor,
            target,
            "{name}: {}",
            app.status
        );
        key(&mut app, KeyCode::Left, KeyModifiers::CONTROL);
        assert_eq!(
            app.workbench.active_editor().unwrap().cursor,
            origin,
            "{name}"
        );
        let screen = render::render(&app.prepare_frame(140, 28));
        let (column, row) = (0..screen.height())
            .find_map(|row| {
                let line = screen.row_text(row);
                let offset = line.find(source)?;
                Some(((line[..offset].chars().count() + origin) as u16, row))
            })
            .unwrap();
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::CONTROL,
        }));
        finish(&mut app);
        assert_eq!(
            app.workbench.active_editor().unwrap().cursor,
            target,
            "{name}: {}",
            app.status
        );
    }
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
