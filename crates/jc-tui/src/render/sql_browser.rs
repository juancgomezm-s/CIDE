use super::{
    clip,
    overlays::{centered, panel},
};
use crate::{
    app::TuiApp,
    screen::{CellStyle, Screen},
};
use jc_platform::sql::{RelationDetails, RelationKind};

pub(super) fn draw(app: &TuiApp, screen: &mut Screen) {
    let Some(browser) = &app.sql_browser else {
        return;
    };
    screen.hide_cursor();
    if screen.width() < 22 || screen.height() < 10 {
        line(
            screen,
            0,
            0,
            screen.width(),
            "Registros SQL · Esc cerrar",
            CellStyle::Accent,
        );
        return;
    }
    let (x, y, width, height) = centered(screen, 108, 26);
    let left = x + 2;
    let columns = width - 4;
    let rows = usize::from(height - 7);
    panel(screen, x, y, width, height, "Registros SQL · Solo lectura");
    line(
        screen,
        left,
        y + 1,
        columns,
        &browser.profile,
        CellStyle::Accent,
    );
    let footer = if let Some(details) = &browser.details {
        let labels = metadata_lines(details);
        let start = browser.detail_row.saturating_sub(rows.saturating_sub(1));
        for (index, label) in labels.iter().enumerate().skip(start).take(rows) {
            line(
                screen,
                left,
                y + 2 + (index - start) as u16,
                columns,
                label,
                selected(index == browser.detail_row),
            );
        }
        "↑↓ recorrer · Esc volver"
    } else if let Some(page) = &browser.page {
        let count = page
            .columns
            .len()
            .min(usize::from((columns / 14).max(1)))
            .max(1);
        let start = browser.column.saturating_sub(count - 1);
        let cell_width = columns / count as u16;
        for (index, field) in page.columns.iter().enumerate().skip(start).take(count) {
            let cell_x = left + (index - start) as u16 * cell_width;
            line(
                screen,
                cell_x,
                y + 2,
                cell_width.saturating_sub(1),
                field,
                selected(index == browser.column),
            );
        }
        let first = browser.row.saturating_sub(rows.saturating_sub(1));
        for (index, row) in page.rows.iter().enumerate().skip(first).take(rows) {
            for (col, value) in row.iter().enumerate().skip(start).take(count) {
                let cell_x = left + (col - start) as u16 * cell_width;
                let cell_y = y + 3 + (index - first) as u16;
                line(
                    screen,
                    cell_x,
                    cell_y,
                    cell_width.saturating_sub(1),
                    display(value.as_deref()),
                    selected(index == browser.row && col == browser.column),
                );
            }
        }
        let row_y = y + height - 4;
        if let Some((filter, cursor)) = &browser.filter {
            let prefix = "Filtro exacto: ";
            let visible = usize::from(columns).saturating_sub(prefix.len() + 1);
            let text = &filter[..*cursor];
            let hidden = jc_text::grapheme::width(text).saturating_sub(visible);
            let offset = jc_text::grapheme::graphemes(text)
                .scan(0, |count, cluster| {
                    *count += jc_text::grapheme::width(cluster);
                    Some((*count, cluster.len()))
                })
                .take_while(|(count, _)| *count <= hidden)
                .map(|(_, bytes)| bytes)
                .sum::<usize>();
            line(
                screen,
                left,
                row_y,
                columns,
                &format!("{prefix}{}", &filter[offset..]),
                CellStyle::Accent,
            );
            let caret = prefix.len() + jc_text::grapheme::width(&filter[offset..*cursor]);
            screen.set_cursor(left + (caret as u16).min(columns - 1), row_y);
            "Intro aplicar · Esc cancelar"
        } else {
            let value = page
                .rows
                .get(browser.row)
                .and_then(|row| row.get(browser.column))
                .and_then(Option::as_deref);
            let cell = format!(
                "Fila {} · {}",
                page.offset + browser.row + 1,
                if page.rows.is_empty() {
                    "Sin resultados"
                } else {
                    display(value)
                }
            );
            line(screen, left, row_y, columns, &cell, CellStyle::Dim);
            "RePág/AvPág · F filtrar · X quitar · O orden · M esquema · Esc tablas"
        }
    } else {
        let first = browser.table.saturating_sub(rows.saturating_sub(1));
        for (index, relation) in browser.tables.iter().enumerate().skip(first).take(rows) {
            let kind = if relation.kind() == RelationKind::View {
                "Vista"
            } else {
                "Tabla"
            };
            let label = format!("{kind} · {}.{}", relation.schema(), relation.name());
            line(
                screen,
                left,
                y + 2 + (index - first) as u16,
                columns,
                &label,
                selected(index == browser.table),
            );
        }
        "Intro ver registros · M esquema · F5 actualizar catálogo · Esc cerrar"
    };
    line(
        screen,
        left,
        y + height - 3,
        columns,
        &browser.message,
        CellStyle::Accent,
    );
    line(
        screen,
        left,
        y + height - 2,
        columns,
        footer,
        CellStyle::Dim,
    );
}

fn selected(active: bool) -> CellStyle {
    if active {
        CellStyle::Reverse
    } else {
        CellStyle::Normal
    }
}

fn display(value: Option<&str>) -> &str {
    match value {
        None => "NULL",
        Some("") => "«»",
        Some(text) => text,
    }
}

fn line(screen: &mut Screen, x: u16, y: u16, width: u16, text: &str, style: CellStyle) {
    let safe: String = text
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect();
    screen.text_styled(x, y, &clip(&safe, width), style);
}

fn metadata_lines(details: &RelationDetails) -> Vec<String> {
    details
        .columns
        .iter()
        .map(|column| {
            format!(
                "{} · {}{}",
                column.name,
                column.data_type,
                if column.nullable { " · NULL" } else { "" }
            )
        })
        .chain(details.indexes.iter().map(|index| {
            format!(
                "{} {} · {}",
                if index.primary {
                    "Clave primaria"
                } else if index.unique {
                    "Índice único"
                } else {
                    "Índice"
                },
                index.index,
                index.definition
            )
        }))
        .chain(details.foreign_keys.iter().map(|key| {
            format!(
                "Foránea {} · {} → {}.{}.{}",
                key.constraint,
                key.column,
                key.referenced_schema,
                key.referenced_table,
                key.referenced_column
            )
        }))
        .collect()
}
