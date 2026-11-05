//! Observable terminal flows: real key routing, prepared style roles and inert
//! Markdown reading over the same byte-preserving document/history.
use super::TuiApp;
use crate::{
    render,
    state::{LaunchOptions, UiOverlay},
    CellStyle, FrameEncoder,
};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use jc_app::{CommandId, MenuId, PrimaryFocus, SidebarView, SyntaxKind};
use jc_text::{TextEncoding, TextPosition};

fn key(app: &mut TuiApp, code: KeyCode, modifiers: KeyModifiers) {
    app.handle_event(Event::Key(KeyEvent::new(code, modifiers)));
}
fn markdown_key(app: &mut TuiApp) {
    key(
        app,
        KeyCode::Char('m'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    );
}
fn editor(name: &str, source: &str) -> TuiApp {
    let mut app = TuiApp::new(LaunchOptions::default());
    app.panels.hide_sidebar();
    app.workbench.new_file(name, TextEncoding::Utf8);
    app.workbench.insert_text(source).unwrap();
    app.workbench
        .go_to_position(TextPosition { line: 0, column: 0 })
        .unwrap();
    app
}
fn revision(app: &TuiApp) -> u64 {
    app.workbench
        .active_document()
        .unwrap()
        .session
        .document()
        .revision()
}
fn reading(app: &mut TuiApp, width: u16, height: u16) -> String {
    let frame = app.prepare_frame(width, height);
    frame
        .markdown
        .as_ref()
        .unwrap()
        .rows
        .iter()
        .map(|row| {
            row.spans
                .iter()
                .map(|span| span.text.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn spanish_menu_accelerators_open_the_underlined_visible_choice_and_f10_keeps_keyboard_access() {
    let mut app = editor("menus.txt", "source remains exact\r\n");
    let original = app.workbench.active_bytes().unwrap();
    for (letter, menu) in [
        ('a', MenuId::File),
        ('e', MenuId::Edit),
        ('b', MenuId::Search),
        ('p', MenuId::Project),
        ('h', MenuId::Tools),
        ('v', MenuId::View),
    ] {
        key(&mut app, KeyCode::Char(letter), KeyModifiers::ALT);
        assert!(matches!(app.overlay,UiOverlay::Menu {menu:active,..} if active==menu));
        let frame = app.prepare_frame(140, 30);
        let screen = render::render(&frame);
        assert_eq!(screen, render::render(&frame));
        let marked = (0..screen.width())
            .filter_map(|x| {
                let cell = screen.cell(x, 0);
                (cell.style == CellStyle::MenuMnemonicActive).then_some(cell.ch)
            })
            .collect::<Vec<_>>();
        assert_eq!(marked.len(), 1);
        assert!(marked[0].eq_ignore_ascii_case(&letter));
        let mut bytes = Vec::new();
        let mut encoder = FrameEncoder::default();
        encoder.draw_to(&mut bytes, &screen).unwrap();
        let vt = std::str::from_utf8(&bytes).unwrap();
        assert!(vt.contains("\x1b[4m"));
        if std::env::var_os("NO_COLOR").is_none_or(|value| value.is_empty()) {
            assert!(vt.contains("38;2;") && vt.contains("48;2;"));
        }
        assert_eq!(encoder.draw_to(&mut Vec::new(), &screen).unwrap().bytes, 0);
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    }
    key(&mut app, KeyCode::F(10), KeyModifiers::NONE);
    assert!(matches!(
        app.overlay,
        UiOverlay::Menu {
            menu: MenuId::File,
            ..
        }
    ));
    assert_eq!(app.workbench.active_bytes().unwrap(), original);
}

#[test]
fn unavailable_build_and_help_menus_do_not_open_empty_overlays_or_change_source() {
    let mut app = editor("pendientes.p", "DISPLAY 'fuente'.\r\n");
    let before = app.workbench.active_bytes().unwrap();
    let before_revision = revision(&app);
    assert!(app.menus.get(MenuId::Build).is_none());
    assert!(app.menus.get(MenuId::Help).is_none());
    for (command, explanation) in [
        (CommandId::MenuBuild, "AVM"),
        (CommandId::MenuHelp, "ayuda"),
    ] {
        assert!(app.palette_commands("").contains(&command));
        app.dispatch(command);
        assert!(matches!(app.overlay, UiOverlay::None));
        assert!(app.status.contains(explanation));
    }
    assert_eq!(app.workbench.active_bytes().unwrap(), before);
    assert_eq!(revision(&app), before_revision);
}

#[test]
fn markdown_reading_formats_source_blocks_input_and_leaves_the_original_undo_step_available() {
    let source =
        "\u{feff}# Árbol\r\nTexto **fuerte** y [sitio](javascript:inerte)\n- [x] Listo\r\nfinal";
    let mut app = editor("lectura.md", source);
    app.workbench.move_end(false).unwrap();
    app.workbench.insert_text(" añadido").unwrap();
    let expected = app.workbench.active_bytes().unwrap();
    app.workbench
        .go_to_position(TextPosition { line: 0, column: 0 })
        .unwrap();
    let before_revision = revision(&app);
    let before_views = format!("{:?}", app.workbench.panes());
    markdown_key(&mut app);
    assert!(app.showing_markdown());
    let frame = app.prepare_frame(120, 32);
    let preview = frame.markdown.as_ref().unwrap();
    assert!(preview
        .rows
        .iter()
        .flat_map(|row| &row.spans)
        .any(|span| span.kind == SyntaxKind::Heading && span.text.contains("Árbol")));
    let screen = render::render(&frame);
    assert_eq!(screen, render::render(&frame));
    assert!(screen.cursor().is_none());
    let text = reading(&mut app, 120, 32);
    assert!(text.contains("fuerte") && !text.contains("**fuerte**"));
    assert!(text.contains("javascript:inerte") && text.contains("☑ Listo"));
    for (code, modifiers) in [
        (KeyCode::Char('Z'), KeyModifiers::NONE),
        (KeyCode::Backspace, KeyModifiers::NONE),
        (KeyCode::Delete, KeyModifiers::NONE),
        (KeyCode::Tab, KeyModifiers::NONE),
        (KeyCode::Char('z'), KeyModifiers::CONTROL),
        (KeyCode::Char('y'), KeyModifiers::CONTROL),
    ] {
        key(&mut app, code, modifiers);
    }
    app.handle_event(Event::Paste("no debe editar".into()));
    app.dispatch(CommandId::EditDeleteLine);
    assert_eq!(app.workbench.active_bytes().unwrap(), expected);
    assert_eq!(revision(&app), before_revision);
    assert_eq!(format!("{:?}", app.workbench.panes()), before_views);
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(!app.showing_markdown());
    key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(app.workbench.active_bytes().unwrap(), source.as_bytes());
}

#[test]
fn markdown_stale_refresh_and_palette_return_never_apply_old_reading_as_source() {
    let mut app = editor("versionada.md", "# Primera\r\nContenido\n");
    markdown_key(&mut app);
    let _ = reading(&mut app, 100, 28);
    app.workbench.insert_text("cambio ").unwrap();
    let bytes = app.workbench.active_bytes().unwrap();
    let frame = app.prepare_frame(100, 28);
    assert!(frame.markdown.as_ref().unwrap().rows.is_empty());
    assert!(frame.markdown.as_ref().unwrap().message.contains("cambió"));
    key(&mut app, KeyCode::F(5), KeyModifiers::NONE);
    assert!(reading(&mut app, 100, 28).contains("cambio"));
    key(&mut app, KeyCode::Char('a'), KeyModifiers::ALT);
    assert!(matches!(
        app.overlay,
        UiOverlay::Menu {
            menu: MenuId::File,
            ..
        }
    ));
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.showing_markdown());
    key(
        &mut app,
        KeyCode::Char('p'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    app.handle_event(Event::Paste("Vista previa de Markdown".into()));
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(!app.showing_markdown());
    assert_eq!(app.workbench.active_bytes().unwrap(), bytes);
}

#[test]
fn sidebar_keeps_spanish_key_guidance_visible_across_focus_and_compact_resize() {
    let mut app = editor("guia.txt", "contenido\r\n");
    app.panels.show_sidebar(SidebarView::Explorer);
    let bytes = app.workbench.active_bytes().unwrap();
    for width in [120, 40, 120] {
        let frame = app.prepare_frame(width, 24);
        let screen = render::render(&frame);
        let help = (2..screen.height() - 1)
            .filter_map(|y| {
                let row = (0..frame.layout.sidebar_width)
                    .filter_map(|x| {
                        let cell = screen.cell(x, y);
                        (cell.style == CellStyle::Accent).then_some(cell.ch)
                    })
                    .collect::<String>();
                (!row.is_empty()).then_some(row)
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(help.contains("Intro abrir"));
    }
    key(&mut app, KeyCode::F(6), KeyModifiers::NONE);
    assert_eq!(app.focus.primary(), PrimaryFocus::Sidebar);
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(app.focus.primary(), PrimaryFocus::Editor);
    assert_eq!(app.workbench.active_bytes().unwrap(), bytes);
}
