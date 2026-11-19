//! Pixel positions in the font used by the panel. The caret is drawn separately.
use super::super::{
    dpi::Dpi,
    ffi::{
        CreateCompatibleDC, CreateSolidBrush, DeleteDC, DeleteObject, ExtTextOutW, GetStockObject,
        Handle, Hdc, Rect, SelectObject, SetTextColor,
    },
    paint, shell, wide,
};
use jc_app::FieldSelection;
use jc_text::grapheme::graphemes;
use std::{ops::Range, ptr::null_mut};

#[repr(C)]
#[derive(Default)]
struct TextSize {
    x: i32,
    y: i32,
}

#[link(name = "gdi32")]
extern "system" {
    fn GetTextExtentExPointW(
        dc: Hdc,
        text: *const u16,
        len: i32,
        max: i32,
        fit: *mut i32,
        advances: *mut i32,
        size: *mut TextSize,
    ) -> i32;
    fn SetBkMode(dc: Hdc, mode: i32) -> i32;
    fn RoundRect(dc: Hdc, left: i32, top: i32, right: i32, bottom: i32, w: i32, h: i32) -> i32;
}

pub(super) unsafe fn text(dc: Hdc, rect: Rect, label: &str, color: u32) -> Result<(), String> {
    if label.is_empty() || rect.right <= rect.left || rect.bottom <= rect.top {
        return Ok(());
    }
    let mode = SetBkMode(dc, 1);
    SetTextColor(dc, color);
    let label = wide(label);
    let drawn = ExtTextOutW(
        dc,
        rect.left,
        rect.top,
        4,
        &rect,
        label.as_ptr(),
        label.len().saturating_sub(1) as u32,
        std::ptr::null(),
    );
    SetBkMode(dc, mode);
    if drawn == 0 {
        return Err(super::super::os_error("Dibujar OmniCore"));
    }
    Ok(())
}

pub(super) unsafe fn rounded(dc: Hdc, rect: Rect, radius: i32, color: u32) -> Result<(), String> {
    if rect.right <= rect.left || rect.bottom <= rect.top {
        return Ok(());
    }
    let brush = CreateSolidBrush(color);
    if brush.is_null() {
        return Err(super::super::os_error("Preparar el fondo de OmniCore"));
    }
    let old_brush = SelectObject(dc, brush);
    let old_pen = SelectObject(dc, GetStockObject(8)); // NULL_PEN
    let size = radius * 2;
    let drawn = RoundRect(dc, rect.left, rect.top, rect.right, rect.bottom, size, size);
    SelectObject(dc, old_pen);
    SelectObject(dc, old_brush);
    DeleteObject(brush);
    if drawn == 0 {
        return Err(super::super::os_error("Dibujar el fondo de OmniCore"));
    }
    Ok(())
}

#[derive(Clone)]
pub(super) struct HitRow {
    top: i32,
    stops: Vec<(usize, i32)>,
}

struct Row {
    label: String,
    stops: Vec<(usize, i32)>,
}

impl Row {
    fn x(&self, at: usize) -> i32 {
        self.stops
            .iter()
            .rfind(|(byte, _)| *byte <= at)
            .map_or(0, |&(_, x)| x)
    }
}

pub(super) struct Layout {
    area: Rect,
    rows: Vec<Row>,
    height: i32,
    caret: Option<(usize, usize)>,
    selection: Option<Range<usize>>,
    placeholder: bool,
    dpi: Dpi,
}
impl Layout {
    pub(super) fn new(
        area: Rect,
        draft: &str,
        selection: FieldSelection,
        focused: bool,
        font: Handle,
        dpi: Dpi,
    ) -> Self {
        let height = dpi.pixels(18).max(1);
        let count = ((area.bottom - area.top) / height).max(1) as usize;
        // A private measurement DC uses the same UI font as the paint pass.
        let dc = unsafe { CreateCompatibleDC(null_mut()) };
        let old_font = if dc.is_null() {
            null_mut()
        } else {
            unsafe { SelectObject(dc, font) }
        };
        let mut rows = Vec::new();
        let mut offset = 0;
        for line in draft.split('\n') {
            wrap(
                dc,
                &mut rows,
                line,
                offset,
                (area.right - area.left - 2).max(1),
                dpi,
            );
            offset += line.len() + 1;
        }
        if !dc.is_null() {
            unsafe {
                if !old_font.is_null() {
                    SelectObject(dc, old_font);
                }
                DeleteDC(dc);
            }
        }
        let caret_row = rows
            .iter()
            .rposition(|row| row.stops[0].0 <= selection.cursor)
            .unwrap_or(0);
        let first = if focused {
            caret_row.saturating_sub(count - 1)
        } else {
            0
        };
        Self {
            area,
            dpi,
            height,
            rows: rows.into_iter().skip(first).take(count).collect(),
            caret: focused.then_some((caret_row - first, selection.cursor)),
            selection: if focused { selection.range() } else { None },
            placeholder: draft.is_empty() && !focused,
        }
    }

    pub(super) fn hits(&self) -> Vec<HitRow> {
        self.rows
            .iter()
            .enumerate()
            .map(|(i, row)| HitRow {
                top: self.area.top + i as i32 * self.height,
                stops: row
                    .stops
                    .iter()
                    .map(|&(byte, x)| (byte, self.area.left + x))
                    .collect(),
            })
            .collect()
    }

    pub(super) unsafe fn draw(&self, dc: Hdc) -> Result<(), String> {
        for (i, row) in self.rows.iter().enumerate() {
            let top = self.area.top + i as i32 * self.height;
            let area = Rect {
                top,
                bottom: (top + self.height).min(self.area.bottom),
                ..self.area
            };
            if let Some(selection) = self.selection.as_ref() {
                let left = area.left + row.x(selection.start);
                let right = (area.left + row.x(selection.end)).min(area.right);
                paint::fill(
                    dc,
                    Rect {
                        left,
                        right,
                        ..area
                    },
                    shell::SELECTED,
                )?;
            }
            if self.placeholder {
                text(dc, area, "Escribe a OmniCore…", shell::MUTED)?;
            } else {
                text(dc, area, &row.label, shell::TEXT)?;
            }
            if let Some((_, byte)) = self.caret.filter(|(line, _)| *line == i) {
                let left = (area.left + row.x(byte)).min(area.right - 1);
                paint::fill(
                    dc,
                    Rect {
                        left,
                        right: (left + self.dpi.pixels(1).max(1)).min(area.right),
                        top: area.top + self.dpi.pixels(1),
                        bottom: area.bottom - self.dpi.pixels(2),
                    },
                    shell::ACCENT,
                )?;
            }
        }
        Ok(())
    }
}

pub(super) fn at(rows: &[HitRow], x: i32, y: i32) -> Option<usize> {
    let row = rows
        .iter()
        .rfind(|row| row.top <= y)
        .or_else(|| rows.first())?;
    row.stops
        .iter()
        .min_by_key(|&&(_, edge)| x.abs_diff(edge))
        .map(|&(byte, _)| byte)
}

fn wrap(dc: Hdc, out: &mut Vec<Row>, line: &str, offset: usize, width: i32, dpi: Dpi) {
    let mut units = Vec::new();
    let mut boundaries = vec![(0, offset, 0)];
    let mut display = String::new();
    let mut byte = offset;
    for cluster in graphemes(line) {
        for ch in cluster.chars() {
            match ch {
                '\t' => display.push_str("    "),
                ch if ch.is_control() => display.push('�'),
                ch => display.push(ch),
            }
        }
        let from = boundaries.last().unwrap().2;
        units.extend(display[from..].encode_utf16());
        byte += cluster.len();
        boundaries.push((units.len(), byte, display.len()));
    }
    let mut advances = vec![0; units.len()];
    let measured = !units.is_empty()
        && !dc.is_null()
        && unsafe {
            GetTextExtentExPointW(
                dc,
                units.as_ptr(),
                units.len() as i32,
                i32::MAX,
                null_mut(),
                advances.as_mut_ptr(),
                &mut TextSize::default(),
            ) != 0
        };
    let x = |unit: usize| {
        if unit == 0 {
            0
        } else if measured {
            advances[unit - 1]
        } else {
            unit as i32 * dpi.pixels(6)
        }
    };
    let mut first = 0;
    loop {
        let left = x(boundaries[first].0);
        let mut end = first;
        while end + 1 < boundaries.len() {
            if end > first && x(boundaries[end + 1].0) - left > width {
                break;
            }
            end += 1;
        }
        out.push(Row {
            label: display[boundaries[first].2..boundaries[end].2].to_owned(),
            stops: boundaries[first..=end]
                .iter()
                .map(|&(unit, byte, _)| (byte, x(unit) - left))
                .collect(),
        });
        if end + 1 == boundaries.len() {
            break;
        }
        first = end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn omnicore_caret_and_clicks_keep_text_and_wrap_identical() {
        let area = Rect {
            left: 9,
            top: 30,
            right: 48,
            bottom: 100,
        };
        let source = "Aé bcdef\nx";
        let dpi = Dpi::from_raw(96);
        let make = |cursor| {
            Layout::new(
                area,
                source,
                FieldSelection {
                    cursor,
                    anchor: None,
                },
                true,
                null_mut(),
                dpi,
            )
        };
        let start = make(0);
        let inside = make(3);
        assert_eq!(
            start.rows.iter().map(|row| &row.label).collect::<Vec<_>>(),
            inside.rows.iter().map(|row| &row.label).collect::<Vec<_>>()
        );
        assert_eq!(inside.caret, Some((0, 3)));
        let hits = inside.hits();
        let x = hits[0]
            .stops
            .iter()
            .find(|&&(byte, _)| byte == 3)
            .unwrap()
            .1;
        assert_eq!(at(&hits, x, area.top), Some(3));
    }
}
