use crate::{
    app::Presentation,
    screen::{CellStyle, Screen},
};
pub(super) fn draw(frame: &Presentation<'_>, screen: &mut Screen) {
    let Some(p) = &frame.markdown else {
        return;
    };
    let width = screen.width();
    let height = screen.height();
    if height < 8 || width < 12 {
        return;
    }
    for y in 2..height - 1 {
        screen.hline_styled(0, y, width, ' ', CellStyle::Normal);
    }
    screen.text_styled(2, 2, "Markdown · vista previa", CellStyle::PanelTitle);
    screen.text_styled(
        2,
        3,
        &super::clip(
            "↑↓ desplazar · Tab enlaces · Intro abrir · Ctrl+Intro al lado · Esc fuente",
            width - 4,
        ),
        CellStyle::Accent,
    );
    for (i, row) in p
        .rows
        .iter()
        .take(usize::from(height.saturating_sub(7)))
        .enumerate()
    {
        let mut x = 3;
        for span in &row.spans {
            x = screen.text_styled_in(
                x,
                5 + i as u16,
                width.saturating_sub(3 + x),
                &span.text,
                if span.target.as_ref().is_some_and(|target| {
                    frame.app.markdown_ui.as_ref().is_some_and(|ui| {
                        ui.selected
                            .and_then(|index| ui.targets.get(index))
                            .is_some_and(|selected| std::sync::Arc::ptr_eq(target, selected))
                    })
                }) {
                    CellStyle::Reverse
                } else if span.kind == jc_app::SyntaxKind::Plain {
                    CellStyle::Normal
                } else {
                    CellStyle::Syntax(span.kind)
                },
            );
        }
    }
    screen.text_styled(
        2,
        height - 2,
        &super::clip(&p.message, width - 4),
        CellStyle::Dim,
    );
    screen.hide_cursor();
}
