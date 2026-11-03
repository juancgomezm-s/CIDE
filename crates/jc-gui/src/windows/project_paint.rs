//! Read-only GDI composition of prepared project search fields and S1 rows.
use super::{
    ffi::{Hdc, Rect},
    paint,
    project_panel::ProjectPresentation,
    wide,
};

const PANEL: u32 = 0x00202122;
const FIELD: u32 = 0x00333130;
const SELECTED: u32 = 0x00534b45;
const TEXT: u32 = 0x00d5d7d8;
const DIM: u32 = 0x008d8b87;
const ACCENT: u32 = 0x00a6d9e6;

struct Layout {
    rect: Rect,
    cw: i32,
    ch: i32,
    padding: i32,
    columns: usize,
    header: Option<usize>,
    policy: Option<usize>,
    status: Option<usize>,
    footer: Option<usize>,
    body: std::ops::Range<usize>,
}
impl Layout {
    fn new(width: i32, height: i32, cw: i32, ch: i32) -> Option<Self> {
        if width <= 0 || height <= 0 {
            return None;
        }
        let width = width.clamp(1, 8192);
        let height = height.clamp(1, 8192);
        let cw = cw.clamp(1, 8192);
        let ch = ch.clamp(1, 8192);
        let left = 20.min(width / 8);
        let top = 50.min(height / 8);
        let right = (width - left).min(left + 100 * cw + 16);
        let bottom = height.min(top + 24 * ch);
        let padding = 8.min((right - left) / 4);
        let columns = ((right - left - 2 * padding) / cw).clamp(0, 100) as usize;
        let rows = ((bottom - top + ch - 1) / ch).clamp(1, 24) as usize;
        let header = (rows >= 4).then_some(0);
        let policy = (rows >= 8).then_some(1);
        let footer = (rows >= 3).then_some(rows - 1);
        let status = (rows >= 5).then(|| rows - 2);
        let first = usize::from(header.is_some()) + usize::from(policy.is_some());
        let end = status.or(footer).unwrap_or(rows);
        Some(Self {
            rect: Rect {
                left,
                top,
                right,
                bottom,
            },
            cw,
            ch,
            padding,
            columns,
            header,
            policy,
            status,
            footer,
            body: first..end,
        })
    }
    fn row(&self, row: usize) -> Rect {
        let top = (i64::from(self.rect.top) + row.min(24) as i64 * i64::from(self.ch))
            .min(i64::from(self.rect.bottom)) as i32;
        Rect {
            left: self.rect.left + self.padding,
            top,
            right: self.rect.right - self.padding,
            bottom: (top + self.ch).min(self.rect.bottom),
        }
    }
    fn field_rows(&self, fields: usize) -> usize {
        if self.body.is_empty() {
            return 0;
        }
        let fields = fields.clamp(1, 3);
        if fields == 1 {
            1
        } else {
            (self.body.len() / fields).saturating_sub(1).clamp(1, 6)
        }
    }
}

/// Use this geometry before jc_app::present_field. On tiny clients draw keeps
/// only the active field visible; a zero capacity means there is no body area.
pub(super) fn field_geometry(
    width: i32,
    height: i32,
    cw: i32,
    ch: i32,
    fields: usize,
) -> (usize, usize) {
    Layout::new(width, height, cw, ch).map_or((0, 0), |l| (l.columns, l.field_rows(fields)))
}
/// Results mode reserves one label and one query row in the same body layout.
pub(super) fn result_capacity(width: i32, height: i32, cw: i32, ch: i32) -> usize {
    Layout::new(width, height, cw, ch).map_or(0, |l| l.body.len().saturating_sub(2).min(32))
}

fn clipped(value: &str, columns: usize) -> Vec<u16> {
    clipped_parts(&[value], columns)
}
fn clipped_parts(values: &[&str], columns: usize) -> Vec<u16> {
    let mut text = String::with_capacity(columns.min(100));
    for ch in values.iter().flat_map(|value| value.chars()) {
        if text.len() >= columns.min(100) {
            break;
        }
        if ch.is_ascii() && !ch.is_control() {
            text.push(ch);
            continue;
        }
        // Prepared fields/results already contain ASCII. Defensive escaping of
        // status/title strings cannot emit controls or an invisible bidi glyph.
        for escaped in ch.escape_default() {
            if text.len() >= columns.min(100) {
                return wide(&text);
            }
            text.push(escaped);
        }
    }
    wide(&text)
}
unsafe fn line(
    dc: Hdc,
    l: &Layout,
    row: usize,
    value: &str,
    fg: u32,
    bg: u32,
) -> Result<(), String> {
    paint::text(dc, l.row(row), &clipped(value, l.columns), fg, bg)
}

pub(super) unsafe fn draw(
    dc: Hdc,
    p: &ProjectPresentation,
    width: i32,
    height: i32,
    cw: i32,
    ch: i32,
) -> Result<(), String> {
    let Some(l) = Layout::new(width, height, cw, ch) else {
        return Ok(());
    };
    paint::fill(dc, l.rect, PANEL)?;
    if let Some(row) = l.header {
        let heading = if let Some(results) = &p.results {
            let selected = if results.total == 0 {
                0
            } else {
                results.selected.saturating_add(1).min(results.total)
            };
            format!("{} [{selected}/{}]", results.title, results.total)
        } else {
            "Project Search Plus - draft".into()
        };
        line(dc, &l, row, &heading, TEXT, PANEL)?;
    }
    if let Some(row) = l.policy {
        line(
            dc,
            &l,
            row,
            "Literal case-sensitive | exact EOL | no permanent index",
            DIM,
            PANEL,
        )?;
    }
    let field_count = p.fields.len().min(3);
    let active = p.active.min(field_count.saturating_sub(1));
    let compact = l.body.len() < field_count.saturating_mul(2);
    let mut row = l.body.start;
    for (index, (name, field)) in p.fields.iter().take(3).enumerate() {
        if compact && index != active {
            continue;
        }
        if row >= l.body.end {
            break;
        }
        let data_rows = if compact {
            l.body.len().saturating_sub(1).max(1)
        } else {
            l.field_rows(field_count)
        };
        if row + 1 < l.body.end {
            let first = if field.total_rows == 0 {
                0
            } else {
                field.first_row.saturating_add(1)
            };
            let last = field
                .first_row
                .saturating_add(field.rows.len())
                .min(field.total_rows);
            line(
                dc,
                &l,
                row,
                &format!("{name} [{first}..{last}/{}]", field.total_rows),
                if index == active { ACCENT } else { DIM },
                PANEL,
            )?;
            row += 1;
        }
        let count = data_rows.min(l.body.end - row).min(field.rows.len());
        let first = field
            .cursor_row
            .saturating_sub(count.saturating_sub(1))
            .min(field.rows.len().saturating_sub(count));
        let background = if index == active { FIELD } else { PANEL };
        for offset in 0..count {
            line(
                dc,
                &l,
                row + offset,
                &field.rows[first + offset],
                TEXT,
                background,
            )?;
        }
        if index == active
            && count > 0
            && (first..first + count).contains(&field.cursor_row)
            && field.cursor_column < l.columns
        {
            let cell = l.row(row + field.cursor_row - first);
            let x = (i64::from(cell.left) + field.cursor_column as i64 * i64::from(l.cw))
                .min(i64::from(cell.right)) as i32;
            paint::fill(
                dc,
                Rect {
                    left: x,
                    top: cell.top,
                    right: (x + 2).min(cell.right),
                    bottom: cell.bottom,
                },
                ACCENT,
            )?;
        }
        row = row.saturating_add(data_rows).min(l.body.end);
    }
    if let Some(results) = &p.results {
        let count = (l.body.end - row).min(results.rows.len()).min(32);
        let first = results
            .rows
            .iter()
            .take(32)
            .position(|r| r.selected)
            .map_or(0, |selected| {
                selected.saturating_sub(count.saturating_sub(1))
            });
        for item in results.rows.iter().skip(first).take(count) {
            // Ordinal remains the core row index, including issues; never filter
            // or reinterpret this display string as a filesystem path.
            line(
                dc,
                &l,
                row,
                &item.label,
                if item.is_match { TEXT } else { DIM },
                if item.selected && item.index == results.selected {
                    SELECTED
                } else {
                    PANEL
                },
            )?;
            row += 1;
        }
    }
    if let Some(row) = l.status {
        let status = if let Some(results) = &p.results {
            clipped_parts(&[&results.message, " | ", &p.status], l.columns)
        } else {
            clipped(&p.status, l.columns)
        };
        paint::text(dc, l.row(row), &status, DIM, PANEL)?;
    }
    if let Some(row) = l.footer {
        let help = if p.results.is_some() {
            "Enter open | arrows select | F5 refresh | F2 edit | Esc cancel"
        } else {
            "Enter search | Tab field | F2 LF F4 CRLF | Ctrl+V paste | Esc"
        };
        line(dc, &l, row, help, DIM, PANEL)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn geometry_stays_inside_client_for_tiny_and_extreme_metrics() {
        assert!(Layout::new(0, 10, 8, 16).is_none());
        for (width, height) in [
            (1, 1),
            (45, 30),
            (140, 100),
            (800, 600),
            (i32::MAX, i32::MAX),
        ] {
            for (cw, ch) in [(0, 0), (8, 16), (i32::MAX, i32::MAX)] {
                let l = Layout::new(width, height, cw, ch).unwrap();
                assert!(l.rect.left >= 0 && l.rect.top >= 0);
                assert!(l.rect.right <= width.min(8192) && l.rect.bottom <= height.min(8192));
                assert!(l.columns <= 100 && l.body.len() <= 24);
                for row in 0..24 {
                    let r = l.row(row);
                    assert!(r.left <= r.right && r.top <= r.bottom && r.bottom <= l.rect.bottom);
                }
                assert!(result_capacity(width, height, cw, ch) <= l.body.len());
            }
        }
    }
    #[test]
    fn field_and_result_budgets_match_normal_layout_and_output_is_control_free() {
        let l = Layout::new(1000, 700, 8, 16).unwrap();
        let (columns, rows) = field_geometry(1000, 700, 8, 16, 3);
        assert_eq!(columns, l.columns);
        assert!((rows + 1) * 3 <= l.body.len());
        assert_eq!(result_capacity(1000, 700, 8, 16), l.body.len() - 2);
        let prepared = clipped(r"\r\n\u{E9}", 100);
        assert_eq!(
            String::from_utf16(&prepared[..prepared.len() - 1]).unwrap(),
            r"\r\n\u{E9}"
        );
        let units = clipped("line\r\n\t\u{202e}é", 100);
        assert_eq!(units.last(), Some(&0));
        assert!(units[..units.len() - 1]
            .iter()
            .all(|&u| (32..=126).contains(&u)));
        assert!(clipped(&"x".repeat(1000), usize::MAX).len() <= 101);
    }
}
