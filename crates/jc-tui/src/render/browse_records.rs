use super::{
    clip,
    data_connections::value_offset,
    overlays::{centered, panel},
};
use crate::{
    app::{
        browse_records::{RecordBrowser, ValueField},
        TuiApp,
    },
    screen::{CellStyle, Screen},
};
use jc_progress::editor::BROWSE_PAGE_SIZE;
use jc_text::grapheme;
use std::borrow::Cow;

pub(super) fn draw(app: &TuiApp, screen: &mut Screen) {
    let Some(browser) = &app.record_browser else {
        return;
    };
    screen.hide_cursor();
    if screen.width() < 14 || screen.height() < 10 {
        screen.text_styled(
            0,
            0,
            &clip("Ver registros ABL · Esc cerrar", screen.width()),
            CellStyle::Accent,
        );
        return;
    }

    let (x, y, width, height) = centered(screen, 120, 29);
    panel(
        screen,
        x,
        y,
        width,
        height,
        &clip("Ver registros ABL", width - 6),
    );
    let left = x + 2;
    let columns = width - 4;
    let rows = usize::from(height - 7);
    let status = y + height - 4;
    let (navigation, actions) = if let Some(accepted) = &browser.accepted {
        let page = &accepted.page;
        let table = browser.tables.get(browser.table);
        let title = table.map_or_else(String::new, |table| {
            format!("{}.{}", table.database, table.name)
        });
        let order = if accepted.descending() {
            "descendente"
        } else {
            "ascendente"
        };
        let page_number = page.offset / BROWSE_PAGE_SIZE + 1;
        let heading = if browser.stale() {
            format!("Página {page_number} sin actualizar · {title}")
        } else {
            format!("Página {page_number} · Índice {order} · {title}")
        };
        draw_line(screen, left, y + 1, columns, &heading, CellStyle::Dim);

        if let Some(review) = &browser.review {
            if !review.presented {
                draw_line(
                    screen,
                    left,
                    y + 2,
                    columns,
                    "Amplía la terminal",
                    CellStyle::Accent,
                );
                draw_line(screen, left, y + 3, columns, "Esc cancelar", CellStyle::Dim);
                return;
            }
            let action = if review.preview.restoring {
                "Restaurar"
            } else {
                "Editar"
            };
            let label = format!(
                "{action} · {} · Fila {}",
                review.preview.column,
                page.offset + browser.row + 1
            );
            draw_line(screen, left, y + 2, columns, &label, CellStyle::Accent);
            for (after, label, value) in [
                (false, "Actual", review.preview.before.as_deref()),
                (true, "Propuesto", review.preview.after.as_deref()),
            ] {
                let active = after == review.after_selected;
                let offset = if active { review.offset } else { 0 };
                let value = match value {
                    Some("") => Cow::Borrowed("⟨texto vacío⟩"),
                    Some(value) => {
                        let byte = grapheme::graphemes(value)
                            .take(offset)
                            .map(str::len)
                            .sum::<usize>();
                        Cow::Owned(format!("«{}»", printable(&value[byte..])))
                    }
                    None => Cow::Borrowed("⟨desconocido⟩"),
                };
                draw_line(
                    screen,
                    left,
                    y + 3 + u16::from(after),
                    columns,
                    &format!("{label} · {value}"),
                    if active {
                        CellStyle::Reverse
                    } else {
                        CellStyle::Normal
                    },
                );
            }
            (
                "F9 confirmar · Esc cancelar",
                "↑↓ elegir valor · ←→ desplazar",
            )
        } else if let Some(field) = &browser.edit {
            let label = format!("Nuevo valor · {}", page.columns[browser.column]);
            draw_line(screen, left, y + 2, columns, &label, CellStyle::Accent);
            draw_input(
                screen,
                field,
                left,
                y + 3,
                columns,
                !app.progress_records_busy(),
            );
            ("Intro revisar · Esc cancelar", "Ctrl+U borrar valor")
        } else if let Some(field) = &browser.filter {
            let label = format!("Filtro exacto · {}", page.columns[browser.column]);
            draw_line(screen, left, y + 2, columns, &label, CellStyle::Accent);
            draw_input(screen, field, left, y + 3, columns, true);
            (
                "Intro aplicar · Esc cancelar",
                "Coincidencia exacta en texto o enteros",
            )
        } else if let Some(offset) = browser.detail {
            let label = format!(
                "Fila {} · {}",
                page.offset + browser.row + 1,
                page.columns[browser.column]
            );
            draw_line(screen, left, y + 2, columns, &label, CellStyle::Accent);
            draw_value(
                screen,
                browser.selected_value(),
                offset,
                (left, y + 3, columns),
                usize::from(height - 7),
            );
            ("↑↓/RePág/AvPág desplazar", "Esc volver a registros")
        } else {
            draw_grid(screen, browser, left, y + 2, columns, rows);
            (
                "↑↓ filas · ←→ columnas · Intro valor · F5 actualizar · Esc tablas",
                if browser.can_edit() {
                    "E editar · R restaurar · RePág/AvPág · F filtrar · X quitar · O orden"
                } else if browser.can_restore() {
                    "R restaurar · RePág/AvPág · F filtrar · X quitar · O orden"
                } else {
                    "RePág/AvPág página · F filtrar · X quitar · O orden"
                },
            )
        }
    } else {
        draw_line(
            screen,
            left,
            y + 1,
            columns,
            "Selecciona una tabla del esquema ABL",
            CellStyle::Dim,
        );
        let start = browser.table.min(browser.tables.len().saturating_sub(rows));
        for (index, table) in browser.tables.iter().enumerate().skip(start).take(rows) {
            let label = format!("{}.{}", table.database, table.name);
            draw_line(
                screen,
                left,
                y + 2 + (index - start) as u16,
                columns,
                &label,
                if index == browser.table {
                    CellStyle::Reverse
                } else {
                    CellStyle::Normal
                },
            );
        }
        ("↑↓ elegir · Intro abrir · Esc cerrar", "")
    };

    if app.progress_records_busy() {
        draw_line(
            screen,
            left,
            status,
            columns,
            "Operación de registros ABL en curso…",
            CellStyle::Accent,
        );
        return;
    } else if browser.loading() {
        draw_line(
            screen,
            left,
            status,
            columns,
            "Consultando registros… · Esc cancelar",
            CellStyle::Accent,
        );
        return;
    } else if let Some(message) = &browser.message {
        draw_line(
            screen,
            left,
            status,
            columns,
            &printable(message),
            CellStyle::Accent,
        );
    } else if browser.detail.is_none()
        && browser.filter.is_none()
        && browser.edit.is_none()
        && browser.review.is_none()
    {
        if let Some((page, filter)) = browser
            .accepted
            .as_ref()
            .and_then(|accepted| Some((&accepted.page, accepted.filter()?)))
        {
            let label = format!(
                "Filtro · {} = «{}»",
                page.columns[filter.column],
                printable(&filter.value)
            );
            draw_line(screen, left, status, columns, &label, CellStyle::Dim);
        }
    }
    draw_line(
        screen,
        left,
        y + height - 3,
        columns,
        navigation,
        CellStyle::Dim,
    );
    draw_line(
        screen,
        left,
        y + height - 2,
        columns,
        actions,
        CellStyle::Dim,
    );
}

fn draw_input(
    screen: &mut Screen,
    field: &ValueField,
    left: u16,
    y: u16,
    columns: u16,
    focused: bool,
) {
    let offset = value_offset(&field.value, field.cursor, usize::from(columns - 1));
    draw_line(
        screen,
        left,
        y,
        columns,
        &field.value[offset..],
        CellStyle::Reverse,
    );
    if focused {
        let cursor = grapheme::width(&field.value[offset..field.cursor]) as u16;
        screen.set_cursor(left + cursor, y);
    }
}

fn draw_grid(
    screen: &mut Screen,
    browser: &RecordBrowser,
    x: u16,
    y: u16,
    width: u16,
    rows: usize,
) {
    let Some(accepted) = &browser.accepted else {
        return;
    };
    let page = &accepted.page;
    let cell_count = usize::from((width / 17).max(1)).min(page.columns.len());
    if cell_count == 0 {
        return;
    }
    let cell_width = width / cell_count as u16;
    let first_column = browser.column.min(page.columns.len() - cell_count);
    let first_row = browser.row.min(page.rows.len().saturating_sub(rows));
    for (index, label) in page
        .columns
        .iter()
        .enumerate()
        .skip(first_column)
        .take(cell_count)
    {
        let column = x + (index - first_column) as u16 * cell_width;
        let selected = index == browser.column;
        draw_line(
            screen,
            column,
            y,
            cell_width - 1,
            label,
            if selected {
                CellStyle::Reverse
            } else {
                CellStyle::Accent
            },
        );
        for (row, data) in page.rows.iter().enumerate().skip(first_row).take(rows) {
            let value = data.get(index).and_then(Option::as_deref);
            let value = value.map_or(Cow::Borrowed("?"), printable);
            let display = if grapheme::width(&value) > usize::from(cell_width - 1) {
                Cow::Owned(format!("{}…", clip(&value, cell_width - 2)))
            } else {
                value
            };
            let y = y + 1 + (row - first_row) as u16;
            let style = if selected && row == browser.row {
                CellStyle::Reverse
            } else {
                CellStyle::Normal
            };
            screen.hline_styled(column, y, cell_width - 1, ' ', style);
            draw_line(screen, column, y, cell_width - 1, &display, style);
        }
    }
    if page.rows.is_empty() {
        draw_line(screen, x, y + 1, width, "Sin registros", CellStyle::Dim);
    }
}

fn draw_value(
    screen: &mut Screen,
    value: Option<&str>,
    offset: usize,
    area: (u16, u16, u16),
    rows: usize,
) {
    let (x, y, width) = area;
    let value = match value {
        None => "Valor desconocido (?)",
        Some("") => "Texto vacío",
        Some(value) => value,
    };
    let mut row = 0;
    let mut line = String::new();
    let mut columns = 0;
    for cluster in grapheme::graphemes(value).skip(offset) {
        let sanitized = printable(cluster);
        let cluster_width = grapheme::width(&sanitized);
        if columns + cluster_width > usize::from(width) {
            draw_line(screen, x, y + row as u16, width, &line, CellStyle::Normal);
            row += 1;
            if row == rows {
                return;
            }
            columns = 0;
            line.clear();
        }
        line.push_str(&sanitized);
        columns += cluster_width;
    }
    draw_line(screen, x, y + row as u16, width, &line, CellStyle::Normal);
}

fn printable(value: &str) -> Cow<'_, str> {
    if !value.chars().any(char::is_control) {
        return Cow::Borrowed(value);
    }
    let mut visible = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_control() {
            visible.extend(ch.escape_default());
        } else {
            visible.push(ch);
        }
    }
    Cow::Owned(visible)
}

fn draw_line(screen: &mut Screen, x: u16, y: u16, width: u16, text: &str, style: CellStyle) {
    screen.text_styled(x, y, &clip(text, width), style);
}
