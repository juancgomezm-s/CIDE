//! Read-only native selector composition. No fonts, HWND ownership, input,
//! controller calls, source reads or persistent buffers belong in this module.
use super::{
    ffi::{Hdc, Rect},
    paint,
    selector::SelectorPresentation,
};

const PANEL: u32 = super::shell::BACKGROUND;
const EDGE: u32 = 0x00625b53;
const HEADER: u32 = super::shell::SURFACE;
const QUERY: u32 = 0x00333130;
const SELECTED: u32 = super::shell::SELECTED;
const TEXT: u32 = 0x00d5d7d8;
const DIM: u32 = 0x008d8b87;
const ACCENT: u32 = 0x00a6d9e6;

#[derive(Clone, Copy)]
struct Layout {
    popup: Rect,
    cell_width: i64,
    cell_height: i64,
    padding: i32,
    columns: usize,
    title: Option<usize>,
    query: Option<usize>,
    message: Option<usize>,
    footer: Option<usize>,
    first: usize,
    count: usize,
}
impl Layout {
    fn new(width: i32, height: i32, cell_width: i32, cell_height: i32) -> Option<Self> {
        if width <= 0 || height <= super::chrome::TOP {
            return None;
        }
        // Compute in i64 before narrowing: even bogus i32::MAX font metrics and
        // client sizes cannot overflow or place a rectangle outside the client.
        let cw = i64::from(cell_width.max(1));
        let ch = i64::from(cell_height.max(1));
        let width = i64::from(width);
        let height = i64::from(height - super::chrome::TOP);
        let rows = (height / ch).clamp(1, 20) as usize;
        let popup_width = width.min(cw * 96);
        let popup_height = height.min(ch * rows as i64);
        let left = (width - popup_width) / 2;
        let top = i64::from(super::chrome::TOP) + (height - popup_height) / 2;
        let popup = Rect {
            left: left as i32,
            top: top as i32,
            right: (left + popup_width) as i32,
            bottom: (top + popup_height) as i32,
        };
        let padding = cw.min(popup_width / 4).min(12) as i32;
        let columns = ((popup_width - i64::from(padding) * 2) / cw).clamp(0, 96) as usize;
        let (title, query, message, footer, first, count) = match rows {
            1 => (None, None, None, None, 0, 1),
            2 => (None, Some(0), None, None, 1, 1),
            3 => (None, Some(0), None, Some(2), 1, 1),
            4 => (Some(0), Some(1), None, Some(3), 2, 1),
            _ => (Some(0), Some(1), Some(2), Some(rows - 1), 3, rows - 4),
        };
        Some(Self {
            popup,
            cell_width: cw,
            cell_height: ch,
            padding,
            columns,
            title,
            query,
            message,
            footer,
            first,
            count,
        })
    }

    fn row(self, ordinal: usize) -> Rect {
        let top = (i64::from(self.popup.top) + ordinal as i64 * self.cell_height)
            .min(i64::from(self.popup.bottom));
        let bottom = (top + self.cell_height).min(i64::from(self.popup.bottom));
        Rect {
            left: self.popup.left,
            top: top as i32,
            right: self.popup.right,
            bottom: bottom as i32,
        }
    }

    fn content(self, ordinal: usize) -> Rect {
        let row = self.row(ordinal);
        Rect {
            left: row.left + self.padding,
            right: row.right - self.padding,
            ..row
        }
    }
}

/// The owner passes this capacity into Selector::prepare before drawing. The
/// same layout removes title/query/message/footer exactly once, including tiny
/// clients; normal geometry exposes at most sixteen candidate rows.
pub(super) fn list_capacity(width: i32, height: i32, cell_width: i32, cell_height: i32) -> usize {
    Layout::new(width, height, cell_width, cell_height).map_or(0, |layout| layout.count)
}

fn safe(ch: char) -> char {
    if ch.is_control() {
        '·'
    } else {
        ch
    }
}
fn units(text: impl Iterator<Item = char>, max: usize) -> Vec<u16> {
    let mut result = Vec::with_capacity(max.min(96) * 2 + 1);
    for ch in text.take(max.min(96)) {
        let mut buffer = [0u16; 2];
        result.extend_from_slice(safe(ch).encode_utf16(&mut buffer));
    }
    result.push(0);
    result
}

fn query_line(
    p: &SelectorPresentation,
    columns: usize,
) -> (Vec<u16>, Vec<std::ops::Range<usize>>, usize) {
    let field = jc_app::present_selected_field(
        &p.query,
        p.input_selection,
        columns.saturating_sub(2).max(1),
        1,
    );
    let text = field.field.rows.first().map_or("", String::as_str);
    let selected = field
        .selections
        .first()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|r| (r.start + 2).min(columns)..(r.end + 2).min(columns))
        .filter(|r| !r.is_empty())
        .collect();
    (
        units("> ".chars().chain(text.chars()), columns),
        selected,
        (field.field.cursor_column + 2).min(columns.saturating_sub(1)),
    )
}

fn row_window(p: &SelectorPresentation, capacity: usize) -> std::ops::Range<usize> {
    if capacity == 0 {
        return 0..0;
    }
    // Normally preparation already made this viewport. If the physical client
    // shrank meanwhile, retain its selected prepared row without changing state.
    let start = p
        .rows
        .iter()
        .position(|row| row.selected)
        .map_or(0, |selected| selected.saturating_sub(capacity - 1));
    start..p.rows.len().min(start.saturating_add(capacity))
}

pub(super) unsafe fn draw(
    dc: Hdc,
    presentation: &SelectorPresentation,
    width: i32,
    height: i32,
    cell_width: i32,
    cell_height: i32,
) -> Result<(), String> {
    let Some(layout) = Layout::new(width, height, cell_width, cell_height) else {
        return Ok(());
    };
    paint::fill(dc, layout.popup, EDGE)?;
    let inset_left = (layout.popup.left + 1).min(layout.popup.right);
    let inset_top = (layout.popup.top + 1).min(layout.popup.bottom);
    paint::fill(
        dc,
        Rect {
            left: inset_left,
            top: inset_top,
            right: (layout.popup.right - 1).max(inset_left),
            bottom: (layout.popup.bottom - 1).max(inset_top),
        },
        PANEL,
    )?;
    if let Some(row) = layout.title {
        paint::fill(dc, layout.row(row), HEADER)?;
        let current = if presentation.total == 0 {
            0
        } else {
            presentation
                .selected
                .saturating_add(1)
                .min(presentation.total)
        };
        let counter = format!("{current}/{}", presentation.total);
        let count = counter.chars().count().min(layout.columns);
        let mut title = layout.content(row);
        let counter_x = (i64::from(title.right) - count as i64 * layout.cell_width)
            .max(i64::from(title.left)) as i32;
        title.right = (counter_x - layout.padding).max(title.left);
        paint::text(
            dc,
            title,
            &units(
                presentation.title.chars(),
                layout.columns.saturating_sub(count + 1),
            ),
            TEXT,
            HEADER,
        )?;
        paint::text(
            dc,
            Rect {
                left: counter_x,
                ..layout.content(row)
            },
            &units(counter.chars(), count),
            DIM,
            HEADER,
        )?;
    }
    if let Some(row) = layout.query {
        paint::fill(dc, layout.row(row), QUERY)?;
        let (text, selected, cursor) = query_line(presentation, layout.columns);
        let content = layout.content(row);
        paint::text(dc, content, &text, TEXT, QUERY)?;
        for range in selected {
            let left = (i64::from(content.left) + range.start as i64 * layout.cell_width)
                .min(i64::from(content.right)) as i32;
            let right = (i64::from(content.left) + range.end as i64 * layout.cell_width)
                .min(i64::from(content.right)) as i32;
            if left < right {
                paint::text(
                    dc,
                    Rect {
                        left,
                        right,
                        ..content
                    },
                    &text[range],
                    TEXT,
                    SELECTED,
                )?;
            }
        }
        if content.right > content.left && content.bottom > content.top {
            let x = (i64::from(content.left) + cursor as i64 * layout.cell_width)
                .min(i64::from(content.right) - 1) as i32;
            paint::fill(
                dc,
                Rect {
                    left: x,
                    top: content.top,
                    right: x.saturating_add(2).min(content.right),
                    bottom: content.bottom,
                },
                ACCENT,
            )?;
        }
    }
    if let Some(row) = layout.message {
        paint::text(
            dc,
            layout.content(row),
            &units(presentation.message.chars(), layout.columns),
            DIM,
            PANEL,
        )?;
    }
    let window = row_window(presentation, layout.count);
    if window.is_empty() {
        let message = if layout.message.is_none() {
            presentation.message.as_str()
        } else {
            "No hay elementos visibles"
        };
        paint::text(
            dc,
            layout.content(layout.first),
            &units(message.chars(), layout.columns),
            DIM,
            PANEL,
        )?;
    }
    for (offset, row) in presentation.rows[window].iter().enumerate() {
        let line = layout.first + offset;
        let background = if row.ordinal == presentation.selected {
            SELECTED
        } else {
            PANEL
        };
        paint::fill(dc, layout.row(line), background)?;
        let marker = if row.selected { "> " } else { "  " };
        let disabled = if row.enabled { "" } else { "[deshabilitado] " };
        let label = units(
            marker
                .chars()
                .chain(disabled.chars())
                .chain(row.label.chars()),
            layout.columns,
        );
        paint::text(
            dc,
            layout.content(line),
            &label,
            if row.enabled { TEXT } else { DIM },
            background,
        )?;
    }
    if let Some(row) = layout.footer {
        paint::text(
            dc,
            layout.content(row),
            &units(presentation.footer.chars(), layout.columns),
            DIM,
            PANEL,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::selector::SelectorRow;
    use super::*;

    #[test]
    fn popup_and_every_row_stay_in_the_client_for_tiny_and_extreme_geometry() {
        for (width, height, cw, ch) in [
            (0, 0, 9, 20),
            (-1, 100, 9, 20),
            (1, 1, 9, 20),
            (405, 400, 9, 20),
            (1260, 600, 9, 20),
            (i32::MAX, i32::MAX, i32::MAX, i32::MAX),
            (i32::MAX, i32::MAX, 0, -1),
        ] {
            let Some(layout) = Layout::new(width, height, cw, ch) else {
                assert!(width <= 0 || height <= super::super::chrome::TOP);
                continue;
            };
            assert!(layout.popup.left >= 0 && layout.popup.top >= 0);
            assert!(layout.popup.right <= width && layout.popup.bottom <= height);
            assert!(layout.columns <= 96 && layout.count <= 16);
            for row in [
                layout.first,
                layout.first + layout.count - 1,
                layout.footer.unwrap_or(0),
            ] {
                let rect = layout.content(row);
                assert!(
                    rect.left >= 0 && rect.top >= 0 && rect.right <= width && rect.bottom <= height
                );
                assert!(rect.left <= rect.right && rect.top <= rect.bottom);
            }
            assert_eq!(list_capacity(width, height, cw, ch), layout.count);
        }
        assert_eq!(list_capacity(1260, 600, 9, 20), 16);
    }

    #[test]
    fn query_cursor_and_selected_escape_share_bounded_field_projection() {
        let p = SelectorPresentation {
            title: "Palette",
            query: "aéz".into(),
            input_selection: jc_app::FieldSelection {
                cursor: 3,
                anchor: Some(1),
            },
            message: String::new(),
            footer: "",
            rows: Vec::new(),
            total: 0,
            selected: 0,
        };
        let (text, ranges, caret) = query_line(&p, 96);
        let text = String::from_utf16(&text[..text.len() - 1]).unwrap();
        assert_eq!(text, "> a\\u{E9}z");
        assert_eq!(ranges, vec![3..9]);
        assert_eq!(caret, 9);
        let before = p.query.clone();
        let _ = query_line(&p, 4);
        assert_eq!(p.query, before);
    }

    #[test]
    fn shrinking_an_owned_presentation_keeps_selected_row_without_mutation() {
        let p = SelectorPresentation {
            title: "Quick Open",
            query: "é".into(),
            input_selection: jc_app::FieldSelection {
                cursor: 2,
                anchor: None,
            },
            message: "Listo".into(),
            footer: "Enter open | Esc",
            rows: (0..20)
                .map(|ordinal| SelectorRow {
                    label: format!("src/niño-{ordinal}.p"),
                    selected: ordinal == 19,
                    enabled: ordinal != 19,
                    ordinal,
                })
                .collect(),
            total: 40,
            selected: 19,
        };
        let before: Vec<_> = p
            .rows
            .iter()
            .map(|row| (row.ordinal, row.label.clone(), row.selected, row.enabled))
            .collect();
        for capacity in [1, 4, 16] {
            let window = row_window(&p, capacity);
            assert_eq!(window.len(), capacity);
            assert!(p.rows[window].iter().any(|row| row.selected));
        }
        assert_eq!(
            before,
            p.rows
                .iter()
                .map(|row| (row.ordinal, row.label.clone(), row.selected, row.enabled))
                .collect::<Vec<_>>()
        );
        assert_eq!((p.total, p.selected, p.query.as_str()), (40, 19, "é"));
    }
}
