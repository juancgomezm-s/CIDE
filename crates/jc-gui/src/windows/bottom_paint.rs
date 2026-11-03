use super::{
    bottom_panel::BottomFrame,
    ffi::{Hdc, Rect},
    paint, wide,
};
pub(super) unsafe fn draw(dc: Hdc, frame: &BottomFrame, ch: i32) -> Result<(), String> {
    let rect = frame.rect;
    let ch = ch.clamp(1, 8192);
    paint::fill(dc, rect, 0x00222222)?;
    let line = |i: usize| {
        let top = (rect.top + (i.min(131) as i32) * ch).min(rect.bottom);
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
        &wide(&frame.title),
        0x00d5d7d8,
        if frame.focused {
            0x00534b45
        } else {
            0x00222222
        },
    )?;
    paint::text(dc, line(1), &wide(&frame.message), 0x008d8b87, 0x00222222)?;
    for (i, row) in frame.rows.iter().enumerate() {
        paint::text(
            dc,
            line(i + 2),
            &wide(&row.label),
            0x00d5d7d8,
            if row.selected && frame.focused {
                0x00534b45
            } else {
                0x00222222
            },
        )?;
    }
    let top = (rect.bottom - ch).max(rect.top);
    paint::text(
        dc,
        Rect {
            left: rect.left + 6,
            top,
            right: rect.right - 3,
            bottom: rect.bottom,
        },
        &wide(
            "F6 focus | Alt+1 Problems / Alt+2 Output | Enter navigate | Esc Editor | Ctrl+J hide",
        ),
        0x008d8b87,
        0x00222222,
    )
}
