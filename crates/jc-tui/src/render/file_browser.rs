use super::{clip, overlays::panel};
use crate::{
    app::TuiApp,
    screen::{CellStyle, Screen},
    state::PromptKind,
};

pub(super) fn draw(app: &TuiApp, screen: &mut Screen, kind: PromptKind, input: &str) {
    let bounds = crate::app::mouse::browser_bounds(screen.width(), screen.height());
    let (x, y, width, height) = (bounds.x, bounds.y, bounds.width, bounds.height);
    let title = if kind == PromptKind::OpenFolder {
        "Abrir carpeta — explorador"
    } else {
        "Abrir archivo — explorador"
    };
    panel(
        screen,
        x,
        y,
        width,
        height,
        &clip(title, width.saturating_sub(4)),
    );
    if height < 7 || width < 20 {
        screen.hide_cursor();
        return;
    }
    let inner = width.saturating_sub(4);
    screen.text_styled(
        x + 2,
        y + 1,
        "Ruta: ",
        if app.file_browser.path_focused {
            CellStyle::Accent
        } else {
            CellStyle::Dim
        },
    );
    draw_path(app, screen, input, x + 8, y + 1, inner.saturating_sub(6));
    let Some(frame) = &app.file_browser.frame else {
        screen.hide_cursor();
        return;
    };
    screen.text_styled(
        x + 2,
        y + 2,
        &clip(
            &format!(
                "{} elementos · {} · Tab cambia ruta/lista",
                frame.total.saturating_sub(1),
                if app.file_browser.path_focused {
                    "editando ruta"
                } else {
                    "lista activa"
                }
            ),
            inner,
        ),
        CellStyle::PanelTitle,
    );
    for (offset, row) in frame
        .rows
        .iter()
        .take(usize::from(height.saturating_sub(8)))
        .enumerate()
    {
        let style = if row.selected && !app.file_browser.path_focused {
            CellStyle::Reverse
        } else if kind == PromptKind::OpenFolder && !row.is_dir {
            CellStyle::Dim
        } else if row.is_dir {
            CellStyle::Accent
        } else {
            CellStyle::Panel
        };
        let line = y + 3 + offset as u16;
        screen.hline_styled(x + 1, line, width - 2, ' ', style);
        screen.text_styled(x + 2, line, &clip(&row.label, inner), style);
    }
    screen.text_styled(
        x + 2,
        y + height - 5,
        &clip(&frame.message, inner),
        CellStyle::Dim,
    );
    screen.text_styled(
        x + 2,
        y + height - 4,
        &clip("↑↓ elegir · Intro entrar/abrir · Alt+↑ superior", inner),
        CellStyle::Accent,
    );
    let help = if kind == PromptKind::OpenFolder && app.file_browser.path_focused {
        "Ctrl+Intro elegir esta ruta · Ctrl+L ruta · F5 · Esc cancelar"
    } else if kind == PromptKind::OpenFolder {
        "Ctrl+Intro elegir carpeta actual · Ctrl+L ruta · F5 · Esc"
    } else {
        "Ctrl+Intro abrir al lado · Ctrl+L ruta · F5 · Esc cancelar"
    };
    screen.text_styled(x + 2, y + height - 3, &clip(help, inner), CellStyle::Dim);
    let mut column = x + 2;
    for &(label, _) in crate::app::mouse::browser_buttons(kind == PromptKind::OpenFolder) {
        let available = (x + width - 2).saturating_sub(column);
        if available == 0 {
            break;
        }
        let label = clip(label, available);
        screen.text_styled(column, y + height - 2, &label, CellStyle::Accent);
        column += jc_text::grapheme::width(&label) as u16 + 1;
    }
    if !app.file_browser.path_focused {
        screen.hide_cursor();
    }
}

fn draw_path(app: &TuiApp, screen: &mut Screen, input: &str, x: u16, y: u16, width: u16) {
    let window = crate::app::file_browser::path_window(
        input,
        app.file_browser.field.cursor,
        usize::from(width),
    );
    let selection = app.file_browser.field.range();
    for atom in window.atoms {
        let selected = app.file_browser.path_focused
            && selection
                .as_ref()
                .is_some_and(|range| atom.range.start < range.end && atom.range.end > range.start);
        screen.text_styled(
            x + atom.column as u16,
            y,
            &input[atom.range],
            if selected {
                CellStyle::Reverse
            } else {
                CellStyle::Panel
            },
        );
    }
    if app.file_browser.path_focused {
        screen.set_cursor(x + window.cursor as u16, y);
    }
}
