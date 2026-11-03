use crate::{
    app::Presentation,
    screen::{CellStyle, Screen},
};
pub(super) fn draw(frame: &Presentation<'_>, screen: &mut Screen) {
    let Some(p) = &frame.snippet else {
        return;
    };
    let width = screen.width().saturating_sub(4).min(100);
    let height = screen.height().saturating_sub(4).min(30);
    let x = (screen.width() - width) / 2;
    let y = (screen.height() - height) / 2;
    for row in y..y + height {
        screen.text(x, row, &" ".repeat(usize::from(width)));
    }
    screen.box_outline(x, y, width, height);
    screen.hide_cursor();
    screen.text_styled(
        x + 2,
        y,
        &super::clip(&p.title, width.saturating_sub(4)),
        CellStyle::Reverse,
    );
    screen.text_styled(x + 2, y + 1, &p.message, CellStyle::Dim);
    let mut start = y + 2;
    if let Some(field) = &p.field {
        for row in &field.rows {
            screen.text(x + 2, start, row);
            start += 1;
        }
        screen.set_cursor(
            x + 2 + field.cursor_column as u16,
            y + 2 + field.cursor_row as u16,
        );
    }
    for (i, (row, selected)) in p
        .rows
        .iter()
        .take(usize::from((y + height).saturating_sub(start + 2)))
        .enumerate()
    {
        screen.text_styled(
            x + 2,
            start + i as u16,
            row,
            if *selected {
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
            &super::clip(p.footer, width.saturating_sub(4)),
            CellStyle::Dim,
        );
    }
}
