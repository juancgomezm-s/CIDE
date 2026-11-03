//! Read-only composition of the prepared repository status viewport.
//! Paths/flags are display strings, already escaped and bounded by preparation.
use crate::app::{git_status_list_height, GitStatusPresentation};
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

    fn fill(self, screen: &mut Screen, row: u16, style: CellStyle) {
        if row < self.height {
            for column in 0..self.width {
                screen.put_styled(self.x + column, self.y + row, ' ', style);
            }
        }
    }

    fn write(
        self,
        screen: &mut Screen,
        row: u16,
        column: u16,
        end: u16,
        text: &str,
        style: CellStyle,
    ) {
        if row >= self.height {
            return;
        }
        let width = end.min(self.width).saturating_sub(column);
        screen.text_styled_in(self.x + column, self.y + row, width, text, style);
    }
}

/// Normal geometry uses two summary rows. Short areas preserve a list row:
/// height1 => no header, height2 => title/counter only, height>=3 => +message.
/// Preparation subtracts the same headers before choosing its visible rows.
pub(super) fn draw_git_status(
    screen: &mut Screen,
    p: &GitStatusPresentation,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
    focused: bool,
) {
    let Some(area) = Area::clipped(screen, x, y, width, height) else {
        return;
    };
    for row in 0..area.height {
        area.fill(screen, row, CellStyle::Normal);
    }
    let headers = area.height - git_status_list_height(area.height);
    if headers > 0 {
        let style = if focused {
            CellStyle::Reverse
        } else {
            CellStyle::Dim
        };
        area.fill(screen, 0, style);
        let selected = if p.total == 0 {
            0
        } else {
            p.selected.saturating_add(1).min(p.total)
        };
        let counter = format!("{selected}/{}", p.total);
        let counter_width = counter.chars().count().min(usize::from(area.width)) as u16;
        let counter_x = area.width - counter_width;
        area.write(screen, 0, 0, counter_x.saturating_sub(1), &p.title, style);
        area.write(screen, 0, counter_x, area.width, &counter, style);
    }
    if headers == 2 {
        area.write(screen, 1, 0, area.width, &p.message, CellStyle::Dim);
    }
    let available = usize::from(area.height - headers);
    if p.rows.is_empty() {
        let text = if headers == 0 && !p.message.is_empty() {
            p.message.as_str()
        } else {
            "No hay filas visibles"
        };
        area.write(screen, headers, 0, area.width, text, CellStyle::Dim);
        return;
    }
    // The physical Screen may be smaller than preparation's requested area.
    // Select a borrowed sub-window that keeps the selected prepared row visible;
    // never change the model's selection/scroll or fetch additional source rows.
    let first = p
        .rows
        .iter()
        .position(|row| row.selected)
        .map_or(0, |selected| selected.saturating_sub(available - 1));
    for (offset, row) in p.rows.iter().skip(first).take(available).enumerate() {
        let line = headers + offset as u16;
        let style = if row.selected {
            if focused {
                CellStyle::Reverse
            } else {
                CellStyle::Dim
            }
        } else {
            CellStyle::Normal
        };
        area.fill(screen, line, style);
        let marker = if row.selected { ">" } else { " " };
        area.write(screen, line, 0, 1, marker, style);
        // XY is exactly two ASCII cells (including conflict codes, ?? and !!).
        // Keep the fixed prefix even for an added/deleted blank flag position.
        area.write(screen, line, 1, 3, &row.flags, style);
        area.write(screen, line, 4, area.width, &row.path, style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::GitStatusRow;

    fn fixture() -> GitStatusPresentation {
        GitStatusPresentation {
            title: "Git status (read-only)".into(),
            message: "Ready: isolated configuration".into(),
            rows: vec![
                GitStatusRow {
                    ordinal: 5,
                    flags: "MM".into(),
                    path: "src/first.p".into(),
                    selected: false,
                },
                GitStatusRow {
                    ordinal: 6,
                    flags: " M".into(),
                    path: "src/niño.p".into(),
                    selected: true,
                },
                GitStatusRow {
                    ordinal: 7,
                    flags: "??".into(),
                    path: "src/new.p".into(),
                    selected: false,
                },
            ],
            total: 50,
            selected: 6,
            scroll: 5,
        }
    }

    #[derive(Debug, PartialEq, Eq)]
    struct Snapshot {
        title: String,
        message: String,
        total: usize,
        selected: usize,
        scroll: usize,
        rows: Vec<(usize, String, String, bool)>,
    }
    fn snapshot(p: &GitStatusPresentation) -> Snapshot {
        Snapshot {
            title: p.title.clone(),
            message: p.message.clone(),
            total: p.total,
            selected: p.selected,
            scroll: p.scroll,
            rows: p
                .rows
                .iter()
                .map(|row| {
                    (
                        row.ordinal,
                        row.flags.clone(),
                        row.path.clone(),
                        row.selected,
                    )
                })
                .collect(),
        }
    }

    #[test]
    fn compact_and_wide_panels_show_counter_flags_and_focused_selection() {
        for width in [45, 140] {
            let mut screen = Screen::new(width, 10);
            let p = fixture();
            draw_git_status(&mut screen, &p, 2, 1, width - 4, 7, true);
            assert!(screen.row_text(1).contains("Git status"));
            assert!(screen.row_text(1).contains("7/50"));
            assert!(screen.row_text(2).contains("Ready"));
            assert!(screen.row_text(3).contains(" MM src/first.p"));
            assert!(screen.row_text(4).contains("> M src/niño.p"));
            assert!(screen.row_text(5).contains(" ?? src/new.p"));
            for column in 2..width - 2 {
                assert_eq!(screen.cell(column, 4).style, CellStyle::Reverse);
            }
        }
    }

    #[test]
    fn clipping_tiny_and_outside_rectangles_never_write_neighbor_cells() {
        for (width, height) in [(0, 0), (1, 1), (10, 5), (45, 10)] {
            for (x, y, w, h) in [
                (0, 0, 0, 0),
                (1, 1, 1, 1),
                (2, 2, 4, 2),
                (2, 2, u16::MAX, u16::MAX),
                (u16::MAX, u16::MAX, 5, 5),
            ] {
                let mut screen = Screen::new(width, height);
                for row in 0..height {
                    screen.fill_row(row, '#');
                }
                draw_git_status(&mut screen, &fixture(), x, y, w, h, true);
                let right = u32::from(x) + u32::from(w);
                let bottom = u32::from(y) + u32::from(h);
                for row in 0..height {
                    for column in 0..width {
                        if column < x
                            || row < y
                            || u32::from(column) >= right
                            || u32::from(row) >= bottom
                        {
                            assert_eq!(screen.cell(column, row).ch, '#');
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn smaller_physical_view_keeps_selection_and_repeated_composition_is_read_only() {
        let mut p = fixture();
        p.rows.extend((8..15).map(|ordinal| GitStatusRow {
            ordinal,
            flags: "UU".into(),
            path: format!("conflict-{ordinal}.p"),
            selected: ordinal == 14,
        }));
        p.rows[1].selected = false;
        p.selected = 14;
        let before = snapshot(&p);
        for height in [1, 2, 4] {
            let mut screen = Screen::new(45, height);
            screen.set_cursor(0, 0);
            draw_git_status(&mut screen, &p, 0, 0, 45, 20, false);
            assert!(screen.row_text(height - 1).contains(">UU conflict-14.p"));
            assert_eq!(screen.cell(0, height - 1).style, CellStyle::Dim);
            let first = screen.clone();
            draw_git_status(&mut screen, &p, 0, 0, 45, 20, false);
            assert_eq!(screen, first);
            assert_eq!(screen.cursor(), Some((0, 0)));
        }
        assert_eq!(snapshot(&p), before);
    }

    #[test]
    fn unicode_and_preescaped_paths_are_composed_as_text_not_terminal_controls() {
        let mut p = fixture();
        p.rows.truncate(1);
        p.rows[0].selected = true;
        p.rows[0].flags = "!!".into();
        p.rows[0].path = "src/niño_λ_😀_\\x1b_\\n_\\xFF.p".into();
        p.selected = p.rows[0].ordinal;
        let before = snapshot(&p);
        let mut screen = Screen::new(140, 5);
        draw_git_status(&mut screen, &p, 1, 0, 138, 5, true);
        assert!(screen
            .row_text(2)
            .contains(">!! src/niño_λ_😀_\\x1b_\\n_\\xFF.p"));
        assert!(!screen.row_text(2).chars().any(char::is_control));
        assert_eq!(snapshot(&p), before);
    }
}
