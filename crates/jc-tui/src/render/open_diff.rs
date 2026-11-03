use crate::{
    app::Presentation,
    screen::{CellStyle, Screen},
};
pub(super) fn draw(frame: &Presentation<'_>, screen: &mut Screen) {
    let Some(prepared) = &frame.open_diff else {
        return;
    };
    let end = screen.height().saturating_sub(1);
    for y in 2..end {
        screen.fill_row(y, ' ');
    }
    screen.hide_cursor();
    if end <= 2 {
        return;
    }
    screen.text_styled(1, 2, &prepared.title, CellStyle::Dim);
    if end > 3 {
        screen.text(1, 3, &prepared.message);
    }
    let capacity = end.saturating_sub(5) as usize;
    for (index, row) in prepared.rows.iter().take(capacity).enumerate() {
        let style = if row.selected {
            CellStyle::Reverse
        } else {
            CellStyle::Normal
        };
        screen.text_styled(1, index as u16 + 4, &row.label, style);
    }
    if end > 4 {
        screen.text_styled(
            1,
            end - 1,
            "↑↓ bloque | Pág contenido | Intro derecha / Mayús+Intro izquierda | F5 actualizar | Esc",
            CellStyle::Dim,
        );
    }
}
