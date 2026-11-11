//! Read-only Git diff composition. All source interpretation and viewport work
//! happens before these bounded rows reach the renderer.
use crate::app::{GitPresentation, GitRowKind};
use crate::screen::{CellStyle, Screen};

#[derive(Clone, Copy)]
struct Area {
    x: u16,
    y: u16,
    width: u16,
    height: u16,
}

impl Area {
    fn clipped(screen: &Screen, x: u16, y: u16, width: u16, height: u16) -> Option<Self> {
        let width = width.min(screen.width().saturating_sub(x));
        let height = height.min(screen.height().saturating_sub(y));
        (width > 0 && height > 0).then_some(Self {
            x,
            y,
            width,
            height,
        })
    }

    fn fill_row(self, screen: &mut Screen, row: u16, style: CellStyle) {
        if row < self.height {
            for column in 0..self.width {
                screen.put_styled(self.x + column, self.y + row, ' ', style);
            }
        }
    }

    fn clear(self, screen: &mut Screen) {
        for row in 0..self.height {
            self.fill_row(screen, row, CellStyle::Normal);
        }
    }

    fn write(
        self,
        screen: &mut Screen,
        row: u16,
        column: u16,
        limit: u16,
        text: &str,
        style: CellStyle,
    ) -> u16 {
        if row >= self.height {
            return column;
        }
        let end = limit.min(self.width);
        let width = end.min(self.width).saturating_sub(column);
        screen.text_styled_in(self.x + column, self.y + row, width, text, style) - self.x
    }

    fn line(self, screen: &mut Screen, row: u16, text: &str, style: CellStyle) {
        self.write(screen, row, 0, self.width, text, style);
    }
}

/// Status and snapshot identity are already formatted by the preparation layer.
/// Keep some room for the actual list when the caller supplies a short panel.
fn summary(screen: &mut Screen, prepared: &GitPresentation, area: Area, reserve_rows: u16) -> u16 {
    area.line(screen, 0, "Diferencias Git (solo lectura)", CellStyle::Dim);
    if area.height == 1 {
        return 1;
    }
    area.line(screen, 1, &prepared.status, CellStyle::Normal);
    let details = usize::from(area.height.saturating_sub(2).saturating_sub(reserve_rows));
    let mut row = 2;
    for detail in prepared.details.iter().take(details) {
        area.line(screen, row, detail, CellStyle::Dim);
        row += 1;
    }
    row
}

pub(super) fn sidebar(
    screen: &mut Screen,
    prepared: &GitPresentation,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
) {
    let Some(area) = Area::clipped(screen, x, y, width, height) else {
        return;
    };
    area.clear(screen);
    let reserve = if prepared.hunks.is_empty() { 1 } else { 2 };
    let mut row = summary(screen, prepared, area, reserve);
    if row >= area.height {
        return;
    }
    area.line(
        screen,
        row,
        &format!("Bloques: {}", prepared.total_hunks),
        CellStyle::Dim,
    );
    row += 1;
    if row >= area.height {
        return;
    }
    if prepared.hunks.is_empty() {
        area.line(screen, row, "No hay bloques visibles", CellStyle::Dim);
        return;
    }
    for (target_row, index) in git_sidebar_targets(prepared, area.height) {
        row = target_row;
        let Some(hunk) = prepared.hunks.iter().find(|hunk| hunk.index == index) else {
            continue;
        };
        let style = if hunk.selected {
            CellStyle::Reverse
        } else {
            CellStyle::Normal
        };
        area.fill_row(screen, row, style);
        let marker = if hunk.selected { ">" } else { " " };
        let label = format!("{marker} {} ", hunk.index.saturating_add(1));
        let column = area.write(screen, row, 0, area.width, &label, style);
        area.write(screen, row, column, area.width, &hunk.label, style);
    }
}

pub(crate) fn git_sidebar_targets(prepared: &GitPresentation, height: u16) -> Vec<(u16, usize)> {
    let reserve = if prepared.hunks.is_empty() { 1 } else { 2 };
    let first = 2usize
        + prepared.details.len().min(usize::from(
            height.saturating_sub(2).saturating_sub(reserve),
        ))
        + 1;
    let available = usize::from(height).saturating_sub(first);
    if available == 0 {
        return Vec::new();
    }
    let start = prepared
        .hunks
        .iter()
        .position(|hunk| hunk.selected)
        .map_or(0, |selected| selected.saturating_sub(available - 1));
    prepared
        .hunks
        .iter()
        .skip(start)
        .take(available)
        .enumerate()
        .map(|(row, hunk)| ((first + row) as u16, hunk.index))
        .collect()
}

pub(super) fn unified(
    screen: &mut Screen,
    prepared: &GitPresentation,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
) {
    let Some(area) = Area::clipped(screen, x, y, width, height) else {
        return;
    };
    area.clear(screen);
    // Preparation budgets rows after title/status. Identity details use only the
    // remaining space, so every prepared row remains reachable by its viewport.
    let reserve = prepared.rows.len().max(1).min(usize::from(u16::MAX)) as u16;
    let mut row = summary(screen, prepared, area, reserve);
    if row >= area.height {
        return;
    }
    let available = usize::from(area.height - row);
    let digits = prepared
        .rows
        .iter()
        .take(available)
        .flat_map(|line| [line.left_line, line.right_line])
        .flatten()
        .max()
        .unwrap_or(1)
        .to_string()
        .len()
        .clamp(3, 7);
    if prepared.rows.is_empty() {
        area.line(
            screen,
            row,
            "No hay filas de diferencias visibles",
            CellStyle::Dim,
        );
    }
    for line in prepared.rows.iter().take(available) {
        let sign = match line.kind {
            GitRowKind::Context => ' ',
            GitRowKind::Deleted => '-',
            GitRowKind::Added => '+',
            GitRowKind::Header | GitRowKind::Notice => {
                area.line(screen, row, &line.text, CellStyle::Dim);
                row += 1;
                continue;
            }
        };
        let badge = format!("[{}]", line.eol);
        let row_style = match line.kind {
            GitRowKind::Deleted => CellStyle::DiffDeleted,
            GitRowKind::Added => CellStyle::DiffAdded,
            _ => CellStyle::Normal,
        };
        area.fill_row(screen, row, row_style);
        // Keep the +/- marker even in a one-cell area. EOL takes priority over
        // the end of source text, so long lines cannot hide an EOL-only change.
        let badge_width = (badge.chars().count() as u16).min(area.width.saturating_sub(1));
        let badge_x = area.width - badge_width;
        let body_width = badge_x.saturating_sub(u16::from(badge_x > 1));
        let prefix = if area.width >= 28 {
            let left = line.left_line.map_or_else(String::new, |n| n.to_string());
            let right = line.right_line.map_or_else(String::new, |n| n.to_string());
            format!("{sign} {left:>digits$} {right:>digits$} | ")
        } else {
            format!("{sign} ")
        };
        let column = area.write(screen, row, 0, body_width.max(1), &prefix, row_style);
        area.write(screen, row, column, body_width, &line.text, row_style);
        if badge_width > 0 {
            area.write(screen, row, badge_x, area.width, &badge, row_style);
        }
        row += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{GitDiffRow, GitHunkRow};

    fn fixture() -> GitPresentation {
        GitPresentation {
            status: "Ready: HEAD snapshot".into(),
            details: vec![
                "source.p @ document revision 7".into(),
                "OID abcdef012345".into(),
            ],
            hunks: vec![
                GitHunkRow {
                    index: 0,
                    label: "@@ -1,2 +1,2 @@".into(),
                    selected: false,
                },
                GitHunkRow {
                    index: 1,
                    label: "@@ -12,1 +12,1 @@".into(),
                    selected: true,
                },
                GitHunkRow {
                    index: 2,
                    label: "@@ -40,1 +40,2 @@".into(),
                    selected: false,
                },
            ],
            rows: vec![
                GitDiffRow {
                    kind: GitRowKind::Header,
                    left_line: None,
                    right_line: None,
                    text: "@@ -12,2 +12,2 @@".into(),
                    eol: "",
                },
                GitDiffRow {
                    kind: GitRowKind::Context,
                    left_line: Some(12),
                    right_line: Some(12),
                    text: "unchanged".into(),
                    eol: "CRLF",
                },
                GitDiffRow {
                    kind: GitRowKind::Deleted,
                    left_line: Some(13),
                    right_line: None,
                    text: "old é\\xFF".into(),
                    eol: "CRLF",
                },
                GitDiffRow {
                    kind: GitRowKind::Added,
                    left_line: None,
                    right_line: Some(13),
                    text: "new ".repeat(80),
                    eol: "sin fin de línea",
                },
                GitDiffRow {
                    kind: GitRowKind::Notice,
                    left_line: None,
                    right_line: None,
                    text: "Immutable sources; working revision 7".into(),
                    eol: "",
                },
            ],
            total_hunks: 3,
        }
    }

    fn contents(prepared: &GitPresentation) -> Vec<String> {
        let mut data = vec![prepared.status.clone(), prepared.total_hunks.to_string()];
        data.extend(prepared.details.iter().cloned());
        for hunk in &prepared.hunks {
            data.push(format!("{}:{}:{}", hunk.index, hunk.selected, hunk.label));
        }
        for row in &prepared.rows {
            let kind = match row.kind {
                GitRowKind::Context => "context",
                GitRowKind::Deleted => "deleted",
                GitRowKind::Added => "added",
                GitRowKind::Header => "header",
                GitRowKind::Notice => "notice",
            };
            data.push(format!(
                "{kind}:{:?}:{:?}:{}:{}",
                row.left_line, row.right_line, row.text, row.eol
            ));
        }
        data
    }

    #[test]
    fn unified_signs_line_numbers_and_eol_remain_visible_at_45_and_140_columns() {
        let prepared = fixture();
        for width in [45, 140] {
            let mut screen = Screen::new(width, 14);
            unified(&mut screen, &prepared, 0, 0, width, 14);
            assert!(screen.row_text(0).contains("solo lectura"));
            assert!(screen.row_text(1).contains("Ready"));
            assert!(screen.row_text(2).contains("revision 7"));
            assert!(screen.row_text(3).contains("OID abcdef012345"));
            assert!(screen.row_text(4).starts_with("@@"));
            assert!(screen.row_text(5).contains("12"));
            assert!(screen.row_text(6).starts_with('-'));
            assert!(screen.row_text(6).contains("13"));
            assert!(screen.row_text(6).ends_with("[CRLF]"));
            assert!(screen.row_text(7).starts_with('+'));
            assert!(screen.row_text(7).ends_with("[sin fin de línea]"));
        }
    }

    #[test]
    fn short_sidebar_keeps_selected_hunk_visible_without_changing_selection() {
        let mut prepared = fixture();
        prepared.hunks[1].selected = false;
        prepared.hunks[2].selected = true;
        let before = contents(&prepared);
        let mut screen = Screen::new(45, 10);
        sidebar(&mut screen, &prepared, 2, 1, 28, 4);
        assert!(screen.row_text(3).contains("Bloques: 3"));
        assert!(screen.row_text(4).contains("> 3 "));
        assert_eq!(screen.cell(2, 4).style, CellStyle::Reverse);
        assert_eq!(screen.cell(29, 4).style, CellStyle::Reverse);
        assert_eq!(contents(&prepared), before);
    }

    #[test]
    fn both_components_clip_to_their_rectangle_even_when_it_exceeds_the_screen() {
        type Draw = fn(&mut Screen, &GitPresentation, u16, u16, u16, u16);
        let prepared = fixture();
        for draw in [sidebar as Draw, unified as Draw] {
            for (x, y, width, height) in [
                (3, 2, 21, 6),
                (43, 9, 20, 20),
                (0, 0, 0, 0),
                (1, 1, 1, 1),
                (u16::MAX, u16::MAX, u16::MAX, u16::MAX),
            ] {
                let mut screen = Screen::new(45, 12);
                for row in 0..screen.height() {
                    screen.fill_row_styled(row, '#', CellStyle::Dim);
                }
                screen.set_cursor(0, 0);
                draw(&mut screen, &prepared, x, y, width, height);
                assert_eq!(screen.cursor(), Some((0, 0)));
                let right = usize::from(x) + usize::from(width);
                let bottom = usize::from(y) + usize::from(height);
                for row in 0..screen.height() {
                    for column in 0..screen.width() {
                        if column < x
                            || row < y
                            || usize::from(column) >= right
                            || usize::from(row) >= bottom
                        {
                            assert_eq!(screen.cell(column, row).ch, '#');
                            assert_eq!(screen.cell(column, row).style, CellStyle::Dim);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn repeated_composition_changes_neither_prepared_content_nor_selection() {
        let prepared = fixture();
        let before = contents(&prepared);
        let mut first = Screen::new(140, 30);
        sidebar(&mut first, &prepared, 0, 0, 28, 30);
        unified(&mut first, &prepared, 29, 0, 111, 30);
        let mut second = Screen::new(140, 30);
        sidebar(&mut second, &prepared, 0, 0, 28, 30);
        unified(&mut second, &prepared, 29, 0, 111, 30);
        assert_eq!(first, second);
        assert_eq!(contents(&prepared), before);
    }
}
