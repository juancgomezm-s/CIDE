use crate::{
    app::{git_code::CodeDiffPresentation, GitDiffRow, GitRowKind},
    screen::{CellStyle, Screen},
};

fn style(row: &GitDiffRow) -> CellStyle {
    match row.kind {
        GitRowKind::Added => CellStyle::DiffAdded,
        GitRowKind::Deleted => CellStyle::DiffDeleted,
        _ => CellStyle::Normal,
    }
}

fn line(screen: &mut Screen, x: u16, y: u16, width: u16, row: Option<&GitDiffRow>) {
    let color = row.map_or(CellStyle::Normal, style);
    for column in 0..width {
        screen.put_styled(x + column, y, ' ', color);
    }
    if let Some(row) = row {
        let number = row.right_line.or(row.left_line).unwrap_or(0);
        let sign = match row.kind {
            GitRowKind::Added => '+',
            GitRowKind::Deleted => '-',
            _ => ' ',
        };
        let prefix = format!("{sign}{number:>5} │ ");
        let end = screen.text_styled_in(x, y, width, &prefix, color);
        let available = width.saturating_sub(end - x);
        let badge = format!(" [{}]", row.eol);
        let badge_width = (badge.len() as u16).min(available / 2);
        screen.text_styled_in(
            end,
            y,
            available.saturating_sub(badge_width),
            &row.text,
            color,
        );
        screen.text_styled_in(x + width - badge_width, y, badge_width, &badge, color);
    }
}

pub(super) fn draw(
    screen: &mut Screen,
    frame: &CodeDiffPresentation,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
) {
    if width == 0 || height < 3 {
        return;
    }
    screen.hide_cursor();
    for row in y..y + height {
        for column in x..x + width {
            screen.put_styled(column, row, ' ', CellStyle::Normal);
        }
    }
    let left_width = width.saturating_sub(1) / 2;
    if frame.unified {
        screen.text_styled_in(
            x,
            y,
            width,
            &format!("{} → {}", frame.left_title, frame.right_title),
            CellStyle::PanelTitle,
        );
    } else {
        screen.text_styled_in(x, y, left_width, &frame.left_title, CellStyle::PanelTitle);
        screen.text_styled_in(
            x + left_width + 1,
            y,
            width - left_width - 1,
            &frame.right_title,
            CellStyle::PanelTitle,
        );
        screen.vline(x + left_width, y, height - 1, '│');
    }
    for (index, pair) in frame.rows.iter().take(usize::from(height - 3)).enumerate() {
        let row = y + 1 + index as u16;
        if frame.unified {
            line(
                screen,
                x,
                row,
                width,
                pair.right.as_ref().or(pair.left.as_ref()),
            );
        } else {
            line(screen, x, row, left_width, pair.left.as_ref());
            line(
                screen,
                x + left_width + 1,
                row,
                width - left_width - 1,
                pair.right.as_ref(),
            );
        }
    }
    if frame.rows.is_empty() {
        screen.text_styled_in(x, y + 1, width, &frame.message, CellStyle::Dim);
    }
    screen.text_styled_in(
        x,
        y + height - 2,
        width,
        "F4 historial · V cambiar vista · Esc editar",
        CellStyle::Accent,
    );
    screen.text_styled_in(
        x,
        y + height - 1,
        width,
        "↑↓/Pág desplazar ambas versiones · F5 actualizar",
        CellStyle::Dim,
    );
}
