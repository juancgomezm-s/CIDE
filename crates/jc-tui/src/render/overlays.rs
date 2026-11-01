use super::clip;
use crate::app::TuiApp;
use crate::screen::{CellStyle, Screen};
use crate::state::{PromptKind, ReplaceField, UiOverlay};
use jc_app::MenuId;

pub fn draw(app: &TuiApp, screen: &mut Screen) {
    match &app.overlay {
        UiOverlay::None => {}
        UiOverlay::Prompt { kind, input } => draw_prompt(screen, *kind, input),
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

fn draw_prompt(screen: &mut Screen, kind: PromptKind, input: &str) {
    let title = match kind {
        PromptKind::Open => "Open file",
        PromptKind::New => "New file path",
        PromptKind::SaveAs => "Save As",
        PromptKind::OpenFolder => "Open folder",
    };
    let (x, y, width, height) = centered(screen, 64, 5);
    panel(screen, x, y, width, height, title);
    screen.text(x + 2, y + 2, &clip(input, width.saturating_sub(4)));
    screen.set_cursor(
        (x + 2 + input.chars().count() as u16).min(x + width.saturating_sub(2)),
        y + 2,
    );
}

fn draw_find(app: &TuiApp, screen: &mut Screen, input: &str) {
    let width = screen.width().saturating_sub(2).min(90);
    let x = screen.width().saturating_sub(width) / 2;
    let y = screen.height().saturating_sub(4);
    panel(screen, x, y, width, 3, "Find");
    let flags = format!(
        " [{}]Case [{}]Word [{}]Regex  {}/{}",
        yes(app.workbench.search().case_sensitive),
        yes(app.workbench.search().whole_word),
        yes(app.workbench.search().regex),
        app.workbench.search().current.map_or(0, |value| value + 1),
        app.workbench.search().matches.len()
    );
    screen.text(
        x + 2,
        y + 1,
        &clip(input, width.saturating_sub(flags.len() as u16 + 5)),
    );
    let fx = x + width.saturating_sub(flags.chars().count() as u16 + 2);
    screen.text_styled(fx, y + 1, &flags, CellStyle::Dim);
    screen.set_cursor(
        (x + 2 + input.chars().count() as u16).min(fx.saturating_sub(1)),
        y + 1,
    );
}

fn draw_replace(
    app: &TuiApp,
    screen: &mut Screen,
    find: &str,
    replacement: &str,
    field: ReplaceField,
) {
    let (x, y, width, height) = centered(screen, 76, 7);
    panel(screen, x, y, width, height, "Replace");
    screen.text(x + 2, y + 2, "Find:");
    screen.text(x + 11, y + 2, &clip(find, width.saturating_sub(14)));
    screen.text(x + 2, y + 3, "Replace:");
    screen.text(x + 11, y + 3, &clip(replacement, width.saturating_sub(14)));
    screen.text_styled(
        x + 2,
        y + 5,
        &format!(
            "Enter replace current | Ctrl+Enter replace all | Tab field | {} matches",
            app.workbench.search().matches.len()
        ),
        CellStyle::Dim,
    );
    let (row, value) = match field {
        ReplaceField::Find => (y + 2, find),
        ReplaceField::Replacement => (y + 3, replacement),
    };
    screen.set_cursor(
        (x + 11 + value.chars().count() as u16).min(x + width.saturating_sub(2)),
        row,
    );
}

fn draw_palette(app: &TuiApp, screen: &mut Screen, input: &str, selected: usize) {
    let (x, y, width, height) = centered(screen, 78, 16);
    panel(screen, x, y, width, height, "Command Palette");
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
        (x + 4 + input.chars().count() as u16).min(x + width.saturating_sub(2)),
        y + 1,
    );
}

fn draw_menu(app: &TuiApp, screen: &mut Screen, menu: MenuId, selected: usize) {
    let Some(model) = app.menus.get(menu) else {
        return;
    };
    let context = app.command_context();
    let entries = app
        .menus
        .entries(menu, &app.registry, &app.keymap, &context);
    let width = entries
        .iter()
        .flatten()
        .map(|entry| entry.label.len() + entry.shortcut.map_or(0, |s| s.to_string().len()) + 5)
        .max()
        .unwrap_or(12)
        .clamp(18, 52) as u16;
    let width = width.min(screen.width().saturating_sub(2));
    let height = (entries.len() as u16 + 2).min(screen.height().saturating_sub(2));
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
        .map(|m| m.title.len() as u16 + 2)
        .sum::<u16>()
        .min(screen.width().saturating_sub(width));
    let y = 1;
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
                let text = if shortcut.is_empty() {
                    entry.label.to_owned()
                } else {
                    let gap = usize::from(width.saturating_sub(4))
                        .saturating_sub(entry.label.len() + shortcut.len());
                    format!("{}{}{}", entry.label, " ".repeat(gap), shortcut)
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
    let _ = model;
}

fn draw_close(screen: &mut Screen) {
    let (x, y, width, height) = centered(screen, 58, 7);
    panel(screen, x, y, width, height, "Unsaved changes");
    screen.text(x + 2, y + 2, "The active document has unsaved changes.");
    screen.text_styled(
        x + 2,
        y + 4,
        "S Save    D Don't Save    Esc Cancel",
        CellStyle::Dim,
    );
    screen.hide_cursor();
}

fn draw_encoding(screen: &mut Screen) {
    let (x, y, width, height) = centered(screen, 62, 9);
    panel(screen, x, y, width, height, "Select source encoding");
    screen.text(x + 2, y + 2, "JC-IDE will not guess this legacy source.");
    screen.text(x + 2, y + 4, "1  Windows-1252");
    screen.text(x + 2, y + 5, "2  ISO-8859-1");
    screen.text(x + 30, y + 4, "3  IBM850");
    screen.text(x + 30, y + 5, "4  UTF-8");
    screen.text_styled(
        x + 2,
        y + 7,
        "Esc leaves the file byte-preserved but not text-editable.",
        CellStyle::Dim,
    );
    screen.hide_cursor();
}

fn panel(screen: &mut Screen, x: u16, y: u16, width: u16, height: u16, title: &str) {
    clear_area(screen, x, y, width, height);
    screen.box_outline(x, y, width, height);
    screen.text_styled(x + 2, y, &format!(" {title} "), CellStyle::Reverse);
}

fn clear_area(screen: &mut Screen, x: u16, y: u16, width: u16, height: u16) {
    for row in y..y.saturating_add(height) {
        screen.hline(x, row, width, ' ');
    }
}

fn centered(screen: &Screen, preferred_width: u16, preferred_height: u16) -> (u16, u16, u16, u16) {
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

fn yes(value: bool) -> &'static str {
    if value {
        "x"
    } else {
        " "
    }
}
