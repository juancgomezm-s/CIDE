use super::{clip, overlays::centered, overlays::panel};
use crate::{
    app::{
        data_connections::{Form, Stage},
        TuiApp,
    },
    screen::{CellStyle, Screen},
};
use jc_app::data_connections::Engine;

pub(super) fn draw(app: &TuiApp, screen: &mut Screen) {
    let Some(dialog) = &app.data_connections else {
        return;
    };
    screen.hide_cursor();
    if screen.width() < 12 || screen.height() < 8 {
        screen.text_styled(
            0,
            0,
            &clip("Conexiones de datos · Esc cerrar", screen.width()),
            CellStyle::Accent,
        );
        return;
    }
    let (x, y, width, height) = centered(screen, 84, 17);
    let columns = width - 4;
    let rows = usize::from(height - 5);
    let (title, information, keys) = match &dialog.stage {
        Stage::Profiles => (
            "Conexiones de datos",
            "Perfiles locales del proyecto",
            if app.can_refresh_connection_schema() {
                "↑↓ elegir · Intro abrir · Esc cerrar · F5 actualizar esquema"
            } else {
                "↑↓ elegir · Intro abrir · Esc cerrar"
            },
        ),
        Stage::Engines { selected } => (
            "Nueva conexión · motor",
            availability(Engine::ALL[*selected]),
            "↑↓ elegir · Intro continuar · Esc volver",
        ),
        Stage::Form(form) => (
            form.draft.engine().label(),
            availability(form.draft.engine()),
            "↑↓/Tab campo · F9 guardar · Esc cancelar",
        ),
    };
    panel(screen, x, y, width, height, title);
    screen.text_styled(x + 2, y + 1, &clip(information, columns), CellStyle::Dim);
    screen.text_styled(x + 2, y + height - 2, &clip(keys, columns), CellStyle::Dim);

    let left = x + 2;
    let top = y + 2;
    match &dialog.stage {
        Stage::Profiles => {
            let mut rows = rows;
            let action = app
                .can_browse_connection_records()
                .then_some("F6 ver registros ABL");
            #[cfg(windows)]
            let action = action.or_else(|| {
                app.selected_sql_connection()
                    .map(|_| "F6 conectar y explorar SQL")
            });
            if let Some(action) = action {
                screen.text_styled(left, y + height - 3, &clip(action, columns), CellStyle::Dim);
                rows = rows.saturating_sub(1);
            }
            let selected = dialog.profile_row();
            let start = first_row(selected, rows, dialog.store.profiles().len() + 1);
            let labels = std::iter::once("Nueva conexión…".to_owned()).chain(
                dialog
                    .store
                    .profiles()
                    .iter()
                    .map(|draft| format!("{} · {}", draft.value(0), draft.engine().label())),
            );
            for (index, label) in labels.enumerate().skip(start).take(rows) {
                screen.text_styled(
                    left,
                    top + (index - start) as u16,
                    &clip(&label, columns),
                    row_style(index == selected),
                );
            }
        }
        Stage::Engines { selected } => {
            let start = first_row(*selected, rows, Engine::ALL.len());
            for (index, engine) in Engine::ALL.iter().enumerate().skip(start).take(rows) {
                screen.text_styled(
                    left,
                    top + (index - start) as u16,
                    engine.label(),
                    row_style(index == *selected),
                );
            }
        }
        Stage::Form(form) => {
            let label = form.draft.fields()[form.field].label();
            let extra = form.error.as_deref().or_else(|| {
                (jc_text::grapheme::width(label) >= usize::from(label_width(columns)))
                    .then_some(label)
            });
            if let Some(text) = extra {
                screen.text_styled(
                    left,
                    y + height - 3,
                    &clip(text, columns),
                    CellStyle::Accent,
                );
            }
            draw_form(screen, form, left, top, columns, rows);
        }
    }
}

fn draw_form(screen: &mut Screen, form: &Form, x: u16, y: u16, columns: u16, rows: usize) {
    let fields = form.draft.fields();
    let start = first_row(form.field, rows, fields.len());
    let label_width = label_width(columns);
    let value_width = columns - label_width;
    for (index, field) in fields.iter().enumerate().skip(start).take(rows) {
        let y = y + (index - start) as u16;
        let active = index == form.field;
        let style = row_style(active);
        screen.text_styled(x, y, &clip(field.label(), label_width - 1), style);
        let value = form.draft.value(index);
        let value_column = x + label_width;
        if active {
            let offset = value_offset(value, form.cursor, usize::from(value_width - 1));
            let visible = &value[offset..];
            let cursor = jc_text::grapheme::width(&value[offset..form.cursor]) as u16;
            screen.text_styled(value_column, y, &clip(visible, value_width), style);
            screen.set_cursor(value_column + cursor, y);
        } else {
            screen.text_styled(value_column, y, &clip(value, value_width), style);
        }
    }
}

fn label_width(columns: u16) -> u16 {
    (columns * 2 / 3).min(31)
}

pub(super) fn value_offset(value: &str, cursor: usize, columns: usize) -> usize {
    let prefix = &value[..cursor];
    let mut hidden = jc_text::grapheme::width(prefix).saturating_sub(columns);
    let mut offset = 0;
    for cluster in jc_text::grapheme::graphemes(prefix) {
        if hidden == 0 {
            break;
        }
        hidden = hidden.saturating_sub(jc_text::grapheme::width(cluster));
        offset += cluster.len();
    }
    offset
}

fn row_style(selected: bool) -> CellStyle {
    if selected {
        CellStyle::Reverse
    } else {
        CellStyle::Normal
    }
}

fn first_row(selected: usize, rows: usize, total: usize) -> usize {
    selected.min(total.saturating_sub(rows))
}

fn availability(engine: Engine) -> &'static str {
    match engine {
        Engine::OpenEdge => "Esquema ABL con .cide/progress.local.ini",
        Engine::PostgreSql | Engine::MySql if cfg!(windows) => "SQL de solo lectura mediante ODBC",
        Engine::PostgreSql | Engine::MySql => "Perfiles SQL · conexión disponible en Windows",
    }
}
