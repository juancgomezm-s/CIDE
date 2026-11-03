//! Render bounded owned presentation only; no navigation or completion delivery.
use super::{ffi::*, navigation_panel::NavigationPresentation, paint, wide};
pub(super) unsafe fn draw(
    dc: Hdc,
    p: &NavigationPresentation,
    width: i32,
    height: i32,
    cw: i32,
    ch: i32,
) -> Result<(), String> {
    let left = 20.min(width.max(0));
    let right = (width - 20).max(left);
    let top = (super::chrome::TOP + 20).min(height.max(0));
    let bottom = (top + ch * 7).min((height - 30).max(top));
    let rect = Rect {
        left,
        top,
        right,
        bottom,
    };
    paint::fill(dc, rect, super::shell::SURFACE)?;
    let lines = [
        "Ir a línea[:columna] · escalares del fuente desde1",
        p.field.rows.first().map_or("", String::as_str),
        "Columna predeterminada1 · 64 bytes · sin vista previa",
        &p.message,
        "Intro acepta | Esc cancela | flechas editan | Ctrl+U vacía | Ctrl+V pega",
    ];
    for (index, line) in lines.iter().enumerate() {
        let y = top + ch * (index as i32 + 1);
        if y + ch <= bottom {
            paint::text(
                dc,
                Rect {
                    left: left + 8,
                    top: y,
                    right: right - 8,
                    bottom: y + ch,
                },
                &wide(line),
                0x00d5d7d8,
                super::shell::SURFACE,
            )?;
        }
    }
    let x = left + 8 + p.field.cursor_column as i32 * cw;
    let y = top + ch * 2;
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
