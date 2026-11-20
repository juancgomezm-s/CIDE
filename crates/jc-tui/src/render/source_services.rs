use crate::{
    app::Presentation,
    screen::{CellStyle, Screen},
};
use jc_app::source_services::SourceRow;

fn row(screen: &mut Screen, (x, y, width): (u16, u16, u16), row: &SourceRow, selected: bool) {
    let text_style = if selected {
        CellStyle::Reverse
    } else {
        CellStyle::Normal
    };
    let mut indent = 0;
    if let Some(mark) = row.relation.filter(|_| width >= 2) {
        screen.text_styled(x, y, mark.icon(), CellStyle::Relation(mark));
        indent = 2;
    }
    screen.text_styled(
        x + indent,
        y,
        &super::clip(&row.label, width.saturating_sub(indent)),
        text_style,
    );
}

pub(super) fn sidebar(
    frame: &Presentation<'_>,
    screen: &mut Screen,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
) {
    let p = &frame.source;
    screen.text_styled(x, y, &p.title, CellStyle::Dim);
    if height > 1 {
        screen.text_styled(x + 1, y + 1, &p.message, CellStyle::Dim);
    }
    for (i, item) in p
        .rows
        .iter()
        .take(usize::from(height.saturating_sub(2)))
        .enumerate()
    {
        row(
            screen,
            (x + 1, y + 2 + i as u16, width.saturating_sub(2)),
            item,
            item.selected && frame.app.focus.primary() == jc_app::PrimaryFocus::Sidebar,
        );
    }
}
pub(super) fn popup(frame: &Presentation<'_>, screen: &mut Screen) {
    if !frame.app.source_popup {
        return;
    }
    let width = screen.width().saturating_sub(4).min(100);
    let height = (frame.source.rows.len() as u16 + 5).min(screen.height().saturating_sub(4));
    let x = (screen.width() - width) / 2;
    let y = (screen.height() - height) / 2;
    for line in y..y + height {
        screen.text(x, line, &" ".repeat(usize::from(width)));
    }
    screen.box_outline(x, y, width, height);
    screen.hide_cursor();
    screen.text_styled(x + 2, y, &frame.source.title, CellStyle::Reverse);
    screen.text_styled(x + 2, y + 1, &frame.source.message, CellStyle::Dim);
    for (i, item) in frame
        .source
        .rows
        .iter()
        .take(usize::from(height.saturating_sub(4)))
        .enumerate()
    {
        row(
            screen,
            (x + 2, y + 2 + i as u16, width.saturating_sub(4)),
            item,
            item.selected,
        );
    }
    if height > 2 {
        screen.text_styled(
            x + 2,
            y + height - 2,
            &super::clip(
                if matches!(
                    frame.app.source.mode(),
                    Some(jc_app::source_services::SourceMode::ReviewEdit(_))
                ) {
                    "F9/Ctrl+Intro aplicar | Flechas revisar | Esc"
                } else if frame.app.source.mode()
                    == Some(jc_app::source_services::SourceMode::Rename)
                {
                    "F9/Ctrl+Intro aplicar | Flechas revisar | F5 editar | Esc"
                } else {
                    "Intro abrir | Ctrl+Intro al lado | Flechas elegir | Esc"
                },
                width.saturating_sub(4),
            ),
            CellStyle::Dim,
        );
    }
}
