//! Pixel geometry and proportional interface chrome, independent of the code grid.
use super::{ffi::*, paint};
pub(super) const TABS: i32 = super::chrome::TOP + 36;
pub(super) const HEADER: i32 = 8;
pub(super) const TREE_ROW: i32 = 25;
pub(super) const TREE_HEAD: i32 = 62;
const fn color(rgb: jc_app::syntax_theme::Rgb) -> u32 {
    rgb.r as u32 | ((rgb.g as u32) << 8) | ((rgb.b as u32) << 16)
}
pub(super) const BACKGROUND: u32 = color(jc_app::ui_theme::BACKGROUND);
pub(super) const SURFACE: u32 = color(jc_app::ui_theme::CHROME);
pub(super) const SIDEBAR: u32 = color(jc_app::ui_theme::PANEL);
pub(super) const RAISED: u32 = color(jc_app::ui_theme::RAISED);
pub(super) const SELECTED: u32 = color(jc_app::ui_theme::SELECTED);
pub(super) const ACCENT: u32 = 0x0050b4e6;
pub(super) const TEXT: u32 = 0x00e0dedb;
pub(super) const MUTED: u32 = 0x009e9a94;
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Abc {
    a: i32,
    b: u32,
    c: i32,
}
#[link(name = "gdi32")]
extern "system" {
    fn GetCharABCWidthsW(dc: Hdc, first: u32, last: u32, widths: *mut Abc) -> i32;
}
/// Retain atomic clipping for ink near a cell edge, including ClearType pixels.
pub(super) unsafe fn safe_ascii_runs(dc: Hdc, cell_width: i32) -> [bool; 128] {
    let mut result = [false; 128];
    let mut widths = [Abc::default(); 95];
    if GetCharABCWidthsW(dc, 32, 126, widths.as_mut_ptr()) != 0 {
        for (n, w) in widths.iter().enumerate() {
            result[n + 32] = w.b == 0
                || (w.a >= 1 && i64::from(w.a) + i64::from(w.b) <= i64::from(cell_width) - 1);
        }
    }
    result
}
#[link(name = "gdi32")]
extern "system" {
    fn BeginPath(dc: Hdc) -> i32;
    fn EndPath(dc: Hdc) -> i32;
    fn AbortPath(dc: Hdc) -> i32;
    fn CloseFigure(dc: Hdc) -> i32;
    fn FillPath(dc: Hdc) -> i32;
    fn MoveToEx(dc: Hdc, x: i32, y: i32, old: *mut Point) -> i32;
    fn LineTo(dc: Hdc, x: i32, y: i32) -> i32;
    fn PolyBezierTo(dc: Hdc, points: *const Point, count: u32) -> i32;
}
pub(super) unsafe fn rounded_tab(dc: Hdc, rect: Rect, color: u32) -> Result<(), String> {
    let top = rect.top + 2;
    let radius = ((rect.bottom - top) / 8)
        .min((rect.right - rect.left) / 2)
        .max(1);
    let curve = (radius * 55 + 50) / 100;
    // One closed contour avoids the bottom-edge seam from overlapping a
    // rounded rectangle with a second rectangular fill.
    let brush = CreateSolidBrush(color);
    if brush.is_null() {
        return Err("No se pudo preparar la pestaña gráfica".into());
    }
    let previous_brush = SelectObject(dc, brush);
    let left_corner = [
        Point {
            x: rect.left,
            y: top + radius - curve,
        },
        Point {
            x: rect.left + radius - curve,
            y: top,
        },
        Point {
            x: rect.left + radius,
            y: top,
        },
    ];
    let right_corner = [
        Point {
            x: rect.right - radius + curve,
            y: top,
        },
        Point {
            x: rect.right,
            y: top + radius - curve,
        },
        Point {
            x: rect.right,
            y: top + radius,
        },
    ];
    let ok = BeginPath(dc) != 0
        && MoveToEx(dc, rect.left, rect.bottom, std::ptr::null_mut()) != 0
        && LineTo(dc, rect.left, top + radius) != 0
        && PolyBezierTo(dc, left_corner.as_ptr(), 3) != 0
        && LineTo(dc, rect.right - radius, top) != 0
        && PolyBezierTo(dc, right_corner.as_ptr(), 3) != 0
        && LineTo(dc, rect.right, rect.bottom) != 0
        && CloseFigure(dc) != 0
        && EndPath(dc) != 0
        && FillPath(dc) != 0;
    if !ok {
        AbortPath(dc);
    }
    SelectObject(dc, previous_brush);
    DeleteObject(brush);
    if !ok {
        Err("No se pudo dibujar la pestaña gráfica".into())
    } else {
        Ok(())
    }
}

/// Simple vector geometry; no font-icon dependency or fixed text pseudo-icons.
pub(super) unsafe fn chevron(
    dc: Hdc,
    x: i32,
    y: i32,
    open: bool,
    color: u32,
) -> Result<(), String> {
    for n in 0..4 {
        let (left, top, width) = if open {
            (x + n, y + n, 7 - 2 * n)
        } else {
            (x + n, y + n, 1)
        };
        paint::fill(
            dc,
            Rect {
                left,
                top,
                right: left + width,
                bottom: top + 1,
            },
            color,
        )?;
        if !open {
            paint::fill(
                dc,
                Rect {
                    left,
                    top: y + 6 - n,
                    right: left + 1,
                    bottom: y + 7 - n,
                },
                color,
            )?;
        }
    }
    Ok(())
}
pub(super) unsafe fn file_icon(
    dc: Hdc,
    x: i32,
    y: i32,
    folder: bool,
    color: u32,
) -> Result<(), String> {
    if folder {
        paint::fill(
            dc,
            Rect {
                left: x,
                top: y + 3,
                right: x + 14,
                bottom: y + 12,
            },
            color,
        )?;
        paint::fill(
            dc,
            Rect {
                left: x + 1,
                top: y + 1,
                right: x + 7,
                bottom: y + 4,
            },
            color,
        )?;
        paint::fill(
            dc,
            Rect {
                left: x + 1,
                top: y + 5,
                right: x + 13,
                bottom: y + 11,
            },
            SIDEBAR,
        )?;
    } else {
        paint::fill(
            dc,
            Rect {
                left: x + 2,
                top: y,
                right: x + 12,
                bottom: y + 14,
            },
            color,
        )?;
        paint::fill(
            dc,
            Rect {
                left: x + 3,
                top: y + 1,
                right: x + 11,
                bottom: y + 13,
            },
            SIDEBAR,
        )?;
        for n in 0..3 {
            paint::fill(
                dc,
                Rect {
                    left: x + 5,
                    top: y + 4 + n * 3,
                    right: x + 9,
                    bottom: y + 5 + n * 3,
                },
                color,
            )?;
        }
    }
    Ok(())
}
#[link(name = "dwmapi")]
extern "system" {
    fn DwmSetWindowAttribute(hwnd: Hwnd, attribute: u32, value: *const i32, size: u32) -> i32;
    fn DwmGetWindowAttribute(hwnd: Hwnd, attribute: u32, value: *mut u32, size: u32) -> i32;
}
pub(super) unsafe fn dark_caption(hwnd: Hwnd) {
    let enabled = 1;
    let _ = DwmSetWindowAttribute(hwnd, 20, &enabled, 4);
    let caption = SURFACE as i32;
    let text = TEXT as i32;
    let border = SIDEBAR as i32;
    // Official per-window attributes; unsupported Windows versions keep the
    // immersive-dark fallback. No system-wide color or registry changes.
    let _ = DwmSetWindowAttribute(hwnd, 35, &caption, 4);
    let _ = DwmSetWindowAttribute(hwnd, 36, &text, 4);
    let _ = DwmSetWindowAttribute(hwnd, 34, &border, 4);
}
pub(super) unsafe fn caption_color(hwnd: Hwnd) -> Option<u32> {
    let mut color = 0;
    (DwmGetWindowAttribute(hwnd, 35, &mut color, 4) >= 0).then_some(color)
}
