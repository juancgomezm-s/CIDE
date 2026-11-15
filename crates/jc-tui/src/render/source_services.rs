use crate::{
    app::Presentation,
    screen::{CellStyle, Screen},
};
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
    for (i, row) in p
        .rows
        .iter()
        .take(usize::from(height.saturating_sub(2)))
        .enumerate()
    {
        screen.text_styled(
            x + 1,
            y + 2 + i as u16,
            &row.label,
            if row.selected && frame.app.focus.primary() == jc_app::PrimaryFocus::Sidebar {
                CellStyle::Reverse
            } else {
                CellStyle::Normal
            },
        );
    }
    let _ = width;
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
    for (i, row) in frame
        .source
        .rows
        .iter()
        .take(usize::from(height.saturating_sub(4)))
        .enumerate()
    {
        screen.text_styled(
            x + 2,
            y + 2 + i as u16,
            &row.label,
            if row.selected {
                CellStyle::Reverse
            } else {
                CellStyle::Normal
            },
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
