//! Rendering only: the owned field rows contain ASCII, never raw controls.
use super::{ffi::*, paint, search_panel::SearchPresentation, wide};
pub(super) unsafe fn draw(
    dc: Hdc,
    p: &SearchPresentation,
    width: i32,
    height: i32,
    cw: i32,
    ch: i32,
) -> Result<(), String> {
    let left = 20.min(width.max(0));
    let right = (width - 20).max(left);
    let top = 60.min(height.max(0));
    let bottom = (top + ch * 20).min((height - 30).max(top));
    let rect = Rect {
        left,
        top,
        right,
        bottom,
    };
    paint::fill(dc, rect, 0x00202122)?;
    let mut row = top + ch;
    let mut line = |value: &str, selected: bool| -> Result<(), String> {
        if row + ch <= bottom {
            paint::text(
                dc,
                Rect {
                    left: left + 8,
                    top: row,
                    right: right - 8,
                    bottom: row + ch,
                },
                &wide(value),
                0x00d5d7d8,
                if selected { 0x00534b45 } else { 0x00202122 },
            )?;
        }
        row += ch;
        Ok(())
    };
    line(
        "Document literal search / replacement · exact LF / CRLF / CR",
        false,
    )?;
    line(
        &format!(
            "Find: rows {}..{} / {}",
            p.find.first_row + 1,
            p.find.first_row + p.find.rows.len(),
            p.find.total_rows
        ),
        !p.replacement_active,
    )?;
    for value in &p.find.rows {
        line(value, false)?;
    }
    if let Some(field) = &p.replacement {
        line(
            &format!(
                "Replace: rows {}..{} / {}",
                field.first_row + 1,
                field.first_row + field.rows.len(),
                field.total_rows
            ),
            p.replacement_active,
        )?;
        for value in &field.rows {
            line(value, false)?;
        }
    }
    line(&p.message, false)?;
    line(
        "F2 LF | F4 CRLF | F8 CR | F7 TAB | arrows edit | Ctrl+V paste",
        false,
    )?;
    line(
        "Enter/F6 replace one | Ctrl+Enter/F9 all | F3/Shift+F3 next/prev",
        false,
    )?;
    line(
        "Alt+Up/Down history | Alt+C/W options | Tab field | Esc cancel | 4096B",
        false,
    )?;
    let (field, offset) = if p.replacement_active {
        (
            p.replacement.as_ref().unwrap_or(&p.find),
            p.find.rows.len() + 3,
        )
    } else {
        (&p.find, 2)
    };
    let x = left + 8 + field.cursor_column as i32 * cw;
    let y = top + ch * (offset as i32 + field.cursor_row as i32 + 1);
    if x < right - 8 && y + ch <= bottom {
        paint::fill(
            dc,
            Rect {
                left: x,
                top: y,
                right: x + 2,
                bottom: y + ch,
            },
            0x00a6d9e6,
        )?;
    }
    Ok(())
}
