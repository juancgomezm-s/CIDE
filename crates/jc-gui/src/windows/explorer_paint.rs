//! Draw the bounded shared Explorer projection, never the live catalog or disk.
use super::{
    explorer_panel::ExplorerFrame,
    ffi::{Hdc, Rect},
    paint, wide,
};
pub(super) unsafe fn draw(dc: Hdc, frame: &ExplorerFrame, ch: i32) -> Result<(), String> {
    let rect = frame.rect;
    let ch = ch.clamp(1, 8192);
    paint::fill(dc, rect, 0x00222222)?;
    let line = |row: usize| {
        let top = (i64::from(rect.top) + (row.min(130) as i64) * i64::from(ch))
            .min(i64::from(rect.bottom)) as i32;
        Rect {
            left: rect.left + 6,
            top,
            right: rect.right - 3,
            bottom: (top + ch).min(rect.bottom),
        }
    };
    paint::text(
        dc,
        line(0),
        &wide(&frame.presentation.title),
        0x00d5d7d8,
        if frame.focused {
            0x00534b45
        } else {
            0x00222222
        },
    )?;
    paint::text(
        dc,
        line(1),
        &wide(&frame.presentation.message),
        0x008d8b87,
        0x00222222,
    )?;
    for (index, row) in frame.presentation.rows.iter().take(128).enumerate() {
        paint::text(
            dc,
            line(index + 2),
            &wide(&row.label),
            if row.is_dir { 0x00a6d9e6 } else { 0x00d5d7d8 },
            if row.selected && frame.focused {
                0x00534b45
            } else {
                0x00222222
            },
        )?;
    }
    Ok(())
}
