use super::clip;
use crate::app::TuiApp;
use crate::screen::{CellStyle, Screen};
use crate::state::{PromptKind, ReplaceField, UiOverlay};
use jc_app::MenuId;

pub fn draw(app: &TuiApp, screen: &mut Screen) {
    match &app.overlay {
        UiOverlay::DataConnections => super::data_connections::draw(app, screen),
        UiOverlay::BrowseRecords => super::browse_records::draw(app, screen),
        #[cfg(windows)]
        UiOverlay::SqlRecords => super::sql_browser::draw(app, screen),
        UiOverlay::QuickOpen { input, selected } => draw_quick_open(app, screen, input, *selected),
        UiOverlay::ProjectReplacePreview { selected } => {
            draw_project_preview(app, screen, *selected)
        }
        UiOverlay::None => {}
        UiOverlay::Prompt { kind, input } => draw_prompt(app, screen, *kind, input),
        UiOverlay::Find { input } => draw_find(app, screen, input),
        UiOverlay::Replace {
            find,
            replacement,
            field,
        } => draw_replace(app, screen, find, replacement, *field),
        UiOverlay::Palette { input, selected } => draw_palette(app, screen, input, *selected),
        UiOverlay::Menu { menu, selected } => draw_menu(app, screen, *menu, *selected),
        UiOverlay::CloseDialog { .. } => draw_close(screen),
        UiOverlay::EncodingChoice => draw_encoding(screen),
    }
}

fn draw_prompt(app: &TuiApp, screen: &mut Screen, kind: PromptKind, input: &str) {
    if app.file_browser.active() {
        super::file_browser::draw(app, screen, kind, input);
        return;
    }
    let title = match kind {
        PromptKind::Open => "Abrir archivo",
        PromptKind::New => "Ruta del archivo nuevo",
        PromptKind::SaveAs => "Guardar como",
        PromptKind::GitExportPatch => {
            "Exportar parche · ruta absoluta .patch fuera del repositorio"
        }
        PromptKind::OpenFolder => "Abrir carpeta",
        PromptKind::ProjectQuery => {
            if app.workbench.search().regex {
                "Buscar en proyecto: regex, distingue mayúsculas"
            } else {
                "Buscar en proyecto: literal, distingue mayúsculas"
            }
        }
        PromptKind::ProjectInclude => "Incluir patrones, separados por comas (vacío = **)",
        PromptKind::ProjectExclude => "Excluir patrones (.git/target/work siempre excluidos)",
        PromptKind::ProjectReplacement => "Texto de reemplazo (vacío elimina coincidencias)",
        PromptKind::GoToPosition if app.workbench.active_paged_byte().is_some() => {
            "Ir a @byte (desde cero)"
        }
        PromptKind::GoToPosition => "Ir a línea[:columna] (escalares desde uno)",
        PromptKind::RenameSymbol => "Renombrar símbolo ABL · nombre nuevo",
    };
    let (x, y, width, height) = centered(screen, 64, 5);
    panel(screen, x, y, width, height, title);
    screen.text(x + 2, y + 2, &clip(input, width.saturating_sub(4)));
    screen.set_cursor(
        (x + 2 + jc_text::grapheme::width(input).min(usize::from(u16::MAX)) as u16)
            .min(x + width.saturating_sub(2)),
        y + 2,
    );
}

fn draw_quick_open(app: &TuiApp, screen: &mut Screen, input: &str, selected: usize) {
    let width = screen.width().saturating_sub(2).min(96);
    let height = screen.height().saturating_sub(2).min(20);
    if width < 16 || height < 7 {
        screen.hide_cursor();
        return;
    }
    let x = (screen.width() - width) / 2;
    let y = (screen.height() - height) / 2;
    panel(
        screen,
        x,
        y,
        width,
        height,
        "Apertura rápida — archivos del proyecto",
    );
    screen.text(x + 2, y + 1, &clip(&format!("> {input}"), width - 4));
    let rows = app.quick_open.results();
    screen.text_styled(
        x + 2,
        y + 2,
        &clip(
            &format!(
                "{} / {} rutas; fragmento literal",
                app.quick_open.message(),
                rows.len()
            ),
            width - 4,
        ),
        CellStyle::Dim,
    );
    let available = usize::from(height - 6);
    let start = selected.saturating_sub(available.saturating_sub(1));
    for (row, candidate) in rows.iter().skip(start).take(available).enumerate() {
        screen.text_styled(
            x + 2,
            y + 3 + row as u16,
            &clip(candidate.label(), width - 4),
            if candidate.ordinal() == selected {
                CellStyle::Reverse
            } else {
                CellStyle::Normal
            },
        );
    }
    screen.text_styled(
        x + 2,
        y + height - 2,
        &clip(
            "Intro abrir | Ctrl+Intro al lado | F5 actualizar | Esc cancelar",
            width - 4,
        ),
        CellStyle::Dim,
    );
    screen.set_cursor(
        (x + 4 + jc_text::grapheme::width(input).min(usize::from(width - 6)) as u16)
            .min(x + width - 2),
        y + 1,
    );
}

fn draw_project_preview(app: &TuiApp, screen: &mut Screen, selected: usize) {
    let Some(preview) = &app.replace_preview else {
        return;
    };
    let (x, y, width, height) = centered(screen, 92, 22);
    panel(
        screen,
        x,
        y,
        width,
        height,
        "Reemplazar en proyecto — vista previa",
    );
    screen.text(
        x + 2,
        y + 1,
        &clip(
            &format!(
                "{} archivos / {} coincidencias; copia de búsqueda",
                preview.files_remaining(),
                preview.matches()
            ),
            width.saturating_sub(4),
        ),
    );
    let available = usize::from(height.saturating_sub(5));
    let start = selected.saturating_sub(available.saturating_sub(1));
    for (offset, line) in preview
        .lines()
        .iter()
        .skip(start)
        .take(available)
        .enumerate()
    {
        screen.text_styled(
            x + 2,
            y + 2 + offset as u16,
            &clip(line, width.saturating_sub(4)),
            if start + offset == selected {
                CellStyle::Reverse
            } else {
                CellStyle::Normal
            },
        );
    }
    screen.text(
        x + 2,
        y + height.saturating_sub(3),
        &clip(
            "Se guardan los archivos cerrados; los abiertos quedan sin guardar.",
            width.saturating_sub(4),
        ),
    );
    screen.text(
        x + 2,
        y + height.saturating_sub(2),
        &clip(
            "F9 / Ctrl+Intro aplicar; Esc cancelar",
            width.saturating_sub(4),
        ),
    );
}

fn draw_find(app: &TuiApp, screen: &mut Screen, input: &str) {
    draw_search_fields(app, screen, input, None);
}
fn draw_replace(
    app: &TuiApp,
    screen: &mut Screen,
    find: &str,
    replacement: &str,
    field: ReplaceField,
) {
    draw_search_fields(app, screen, find, Some((replacement, field)));
}
fn draw_search_fields(
    app: &TuiApp,
    screen: &mut Screen,
    find: &str,
    replacement: Option<(&str, ReplaceField)>,
) {
    let (x, y, width, height) = centered(screen, 96, if replacement.is_some() { 22 } else { 16 });
    panel(
        screen,
        x,
        y,
        width,
        height,
        "Buscar/reemplazar — finales de línea exactos",
    );
    let columns = usize::from(width.saturating_sub(4)).max(1);
    let count = if replacement.is_some() { 2 } else { 1 };
    let rows = usize::from(height.saturating_sub(8))
        .checked_div(count)
        .unwrap_or(1)
        .clamp(1, 6);
    let field = jc_app::present_field(find, app.find_cursor, columns, rows);
    let replacement_active = replacement.is_some_and(|(_, f)| f == ReplaceField::Replacement);
    screen.text_styled(
        x + 2,
        y + 1,
        &clip(
            &format!(
                "Buscar R:{} M:{} P:{} {}/{} {}, {}",
                u8::from(app.workbench.search().regex),
                u8::from(app.workbench.search().case_sensitive),
                u8::from(app.workbench.search().whole_word),
                field.first_row + 1,
                field.total_rows,
                app.workbench.search().status.label(),
                app.workbench.search_result_summary()
            ),
            width.saturating_sub(4),
        ),
        if replacement_active {
            CellStyle::Dim
        } else {
            CellStyle::Reverse
        },
    );
    for (row, text) in field.rows.iter().enumerate() {
        screen.text(x + 2, y + 2 + row as u16, text);
    }
    let mut caret = (
        x + 2 + field.cursor_column as u16,
        y + 2 + field.cursor_row as u16,
    );
    if let Some((text, _)) = replacement {
        let field = jc_app::present_field(text, app.replacement_cursor, columns, rows);
        let top = y + rows as u16 + 3;
        screen.text_styled(
            x + 2,
            top,
            &clip(
                &format!("Reemplazar {}/{}", field.first_row + 1, field.total_rows),
                width.saturating_sub(4),
            ),
            if replacement_active {
                CellStyle::Reverse
            } else {
                CellStyle::Dim
            },
        );
        for (row, text) in field.rows.iter().enumerate() {
            screen.text(x + 2, top + 1 + row as u16, text);
        }
        if replacement_active {
            caret = (
                x + 2 + field.cursor_column as u16,
                top + 1 + field.cursor_row as u16,
            );
        }
    }
    for (row, text) in [
        "F2 LF | F4 CRLF | F8 CR | F7 TAB | Flechas editar",
        "Intro/F6 uno | Ctrl+Intro/F9 todos | F3/Mayús+F3 sig./ant.",
        "Tab campo | Mayús/Alt+Intro LF | Esc | 4096 bytes/campo",
    ]
    .iter()
    .enumerate()
    {
        screen.text_styled(
            x + 2,
            y + height.saturating_sub(4) + row as u16,
            &clip(text, width.saturating_sub(4)),
            CellStyle::Dim,
        );
    }
    if caret.0 < x + width.saturating_sub(1) && caret.1 < y + height.saturating_sub(4) {
        screen.set_cursor(caret.0, caret.1);
    } else {
        screen.hide_cursor();
    }
}

fn draw_palette(app: &TuiApp, screen: &mut Screen, input: &str, selected: usize) {
    let (x, y, width, height) = centered(screen, 78, 16);
    panel(screen, x, y, width, height, "Paleta de comandos");
    screen.text(x + 2, y + 1, "> ");
    screen.text(x + 4, y + 1, &clip(input, width.saturating_sub(6)));
    let commands = app.palette_commands(input);
    let context = app.command_context();
    let available = usize::from(height.saturating_sub(4));
    let start = selected.saturating_sub(available.saturating_sub(1));
    for (row, command) in commands.iter().skip(start).take(available).enumerate() {
        let Some(spec) = app.registry.get(*command) else {
            continue;
        };
        let enabled = app.registry.is_enabled(*command, &context);
        let shortcut = app
            .keymap
            .primary_shortcut(*command)
            .map(|stroke| stroke.to_string())
            .unwrap_or_default();
        let label = format!("{:<42} {:>18}", spec.label, shortcut);
        let index = start + row;
        let style = if index == selected {
            CellStyle::Reverse
        } else if enabled {
            CellStyle::Normal
        } else {
            CellStyle::Dim
        };
        screen.text_styled(x + 2, y + 3 + row as u16, &clip(&label, width - 4), style);
    }
    screen.set_cursor(
        (x + 4 + jc_text::grapheme::width(input).min(usize::from(u16::MAX)) as u16)
            .min(x + width.saturating_sub(2)),
        y + 1,
    );
}

pub(crate) fn menu_bounds(
    app: &TuiApp,
    screen_width: u16,
    screen_height: u16,
    menu: MenuId,
) -> (u16, u16, u16, u16) {
    let context = app.command_context();
    let entries = app
        .menus
        .entries(menu, &app.registry, &app.keymap, &context);
    let width = entries
        .iter()
        .flatten()
        .map(|entry| {
            jc_text::grapheme::width(entry.label)
                + usize::from(menu_check(app, entry.command).is_some()) * 4
                + entry.shortcut.map_or(0, |s| s.to_string().len())
                + 5
        })
        .max()
        .unwrap_or(12)
        .clamp(18, 52) as u16;
    let width = width.min(screen_width.saturating_sub(2));
    let height = (entries.len() as u16 + 2).min(screen_height.saturating_sub(2));
    let menu_index = app
        .menus
        .menus()
        .iter()
        .position(|m| m.id == menu)
        .unwrap_or(0);
    let x = app
        .menus
        .menus()
        .iter()
        .take(menu_index)
        .map(|m| jc_text::grapheme::width(m.title) as u16 + 2)
        .sum::<u16>()
        .min(screen_width.saturating_sub(width));
    let y = 1;
    (x, y, width, height)
}

fn draw_menu(app: &TuiApp, screen: &mut Screen, menu: MenuId, selected: usize) {
    if app.menus.get(menu).is_none() {
        return;
    }
    let context = app.command_context();
    let entries = app
        .menus
        .entries(menu, &app.registry, &app.keymap, &context);
    let (x, y, width, height) = menu_bounds(app, screen.width(), screen.height(), menu);
    clear_area(screen, x, y, width, height);
    screen.box_outline(x, y, width, height);
    let available = usize::from(height.saturating_sub(2));
    let start = selected.saturating_sub(available.saturating_sub(1));
    for (index, entry) in entries.iter().enumerate().skip(start).take(available) {
        let row = y + 1 + (index - start) as u16;
        match entry {
            None => screen.hline(x + 1, row, width - 2, '─'),
            Some(entry) => {
                let shortcut = entry.shortcut.map(|s| s.to_string()).unwrap_or_default();
                let checked_label = if let Some(checked) = menu_check(app, entry.command) {
                    format!("[{}] {}", if checked { "x" } else { " " }, entry.label)
                } else {
                    entry.label.to_owned()
                };
                let text = if shortcut.is_empty() {
                    checked_label
                } else {
                    let gap = usize::from(width.saturating_sub(4)).saturating_sub(
                        jc_text::grapheme::width(&checked_label)
                            + jc_text::grapheme::width(&shortcut),
                    );
                    format!("{}{}{}", checked_label, " ".repeat(gap), shortcut)
                };
                let style = if index == selected {
                    CellStyle::Reverse
                } else if entry.enabled {
                    CellStyle::Normal
                } else {
                    CellStyle::Dim
                };
                screen.text_styled(x + 2, row, &clip(&text, width - 4), style);
            }
        }
    }
    screen.hide_cursor();
}

fn menu_check(app: &TuiApp, command: jc_app::CommandId) -> Option<bool> {
    match command {
        jc_app::CommandId::ViewToggleInvisibles => Some(app.workbench.invisibles_visible()),
        jc_app::CommandId::ViewMarkdownPreview => {
            Some(app.markdown_ui.as_ref().is_some_and(|ui| {
                app.workbench
                    .active_document()
                    .is_some_and(|doc| doc.id == ui.stamp.document)
            }))
        }
        _ => None,
    }
}

fn draw_close(screen: &mut Screen) {
    let (x, y, width, height) = centered(screen, 58, 7);
    panel(screen, x, y, width, height, "Cambios sin guardar");
    screen.text(
        x + 2,
        y + 2,
        "El documento activo tiene cambios sin guardar.",
    );
    screen.text_styled(
        x + 2,
        y + 4,
        "G Guardar    N No guardar    Esc Cancelar",
        CellStyle::Dim,
    );
    screen.hide_cursor();
}

fn draw_encoding(screen: &mut Screen) {
    let (x, y, width, height) = centered(screen, 62, 9);
    panel(
        screen,
        x,
        y,
        width,
        height,
        "Elegir codificación del documento",
    );
    screen.text(x + 2, y + 2, "CIDE no adivina la codificación heredada.");
    screen.text(x + 2, y + 4, "1  Windows-1252");
    screen.text(x + 2, y + 5, "2  ISO-8859-1");
    screen.text(x + 30, y + 4, "3  IBM850");
    screen.text(x + 30, y + 5, "4  UTF-8");
    screen.text_styled(
        x + 2,
        y + 7,
        "Esc conserva los bytes y deja sin habilitar la edición de texto.",
        CellStyle::Dim,
    );
    screen.hide_cursor();
}

pub(super) fn panel(screen: &mut Screen, x: u16, y: u16, width: u16, height: u16, title: &str) {
    clear_area(screen, x, y, width, height);
    screen.box_outline(x, y, width, height);
    screen.text_styled(x + 2, y, &format!(" {title} "), CellStyle::PanelTitle);
}

fn clear_area(screen: &mut Screen, x: u16, y: u16, width: u16, height: u16) {
    for row in y..y.saturating_add(height) {
        screen.hline_styled(x, row, width, ' ', CellStyle::Panel);
    }
}

pub(super) fn centered(
    screen: &Screen,
    preferred_width: u16,
    preferred_height: u16,
) -> (u16, u16, u16, u16) {
    let width = preferred_width.min(screen.width().saturating_sub(2)).max(4);
    let height = preferred_height
        .min(screen.height().saturating_sub(2))
        .max(3);
    (
        (screen.width() - width) / 2,
        (screen.height() - height) / 2,
        width,
        height,
    )
}
