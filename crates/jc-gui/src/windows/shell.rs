//! Pixel geometry and proportional interface chrome, independent of the code grid.
use super::{ffi::*, paint};
pub(super) const TABS: i32 = super::chrome::TOP + 26;
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
pub(super) const BORDER: u32 = color(jc_app::ui_theme::BORDER);
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
    fn RoundRect(dc: Hdc, l: i32, t: i32, r: i32, b: i32, w: i32, h: i32) -> i32;
    fn SetBkMode(dc: Hdc, mode: i32) -> i32;
}
pub(super) unsafe fn rounded(dc: Hdc, rect: Rect, radius: i32, color: u32) -> Result<(), String> {
    if rect.right <= rect.left || rect.bottom <= rect.top {
        return Ok(());
    }
    let brush = CreateSolidBrush(paint::ui_color(color));
    if brush.is_null() {
        return Err(super::os_error("Preparar superficie"));
    }
    let old_brush = SelectObject(dc, brush);
    let old_pen = SelectObject(dc, GetStockObject(8));
    let result = RoundRect(
        dc,
        rect.left,
        rect.top,
        rect.right,
        rect.bottom,
        radius * 2,
        radius * 2,
    );
    SelectObject(dc, old_pen);
    SelectObject(dc, old_brush);
    DeleteObject(brush);
    if result == 0 {
        Err(super::os_error("Dibujar superficie"))
    } else {
        Ok(())
    }
}
pub(super) unsafe fn label(dc: Hdc, rect: Rect, label: &str, color: u32) -> Result<(), String> {
    if rect.right <= rect.left || rect.bottom <= rect.top {
        return Ok(());
    }
    let old = SetBkMode(dc, 1);
    SetTextColor(dc, paint::ui_ink(color));
    let text = super::wide(label);
    let result = ExtTextOutW(
        dc,
        rect.left,
        rect.top,
        4,
        &rect,
        text.as_ptr(),
        text.len().saturating_sub(1) as u32,
        std::ptr::null(),
    );
    SetBkMode(dc, old);
    if result == 0 {
        Err(super::os_error("Dibujar etiqueta"))
    } else {
        Ok(())
    }
}
pub(super) fn blend(a: u32, b: u32, percent: u32) -> u32 {
    let t = percent.min(100);
    [0, 8, 16].into_iter().fold(0, |color, shift| {
        color | (((((a >> shift) & 255) * (100 - t) + ((b >> shift) & 255) * t) / 100) << shift)
    })
}
pub(super) unsafe fn rounded_tab(dc: Hdc, rect: Rect, color: u32) -> Result<(), String> {
    let top = rect.top + 2;
    let radius = ((rect.bottom - top) / 8)
        .min((rect.right - rect.left) / 2)
        .max(1);
    let curve = (radius * 55 + 50) / 100;
    // One closed contour avoids the bottom-edge seam from overlapping a
    // rounded rectangle with a second rectangular fill.
    let brush = CreateSolidBrush(paint::ui_color(color));
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
    size: i32,
) -> Result<(), String> {
    let size = size.clamp(8, 64);
    let px = |v: i32| (v * size + 8) / 16;
    let rect = |l, t, r, b| Rect {
        left: x + px(l),
        top: y + px(t),
        right: x + px(r),
        bottom: y + px(b),
    };
    let dark = blend(color, 0, 48);
    let light = blend(color, 0x00ffffff, 36);
    if folder {
        // Back cover, tab, paper edge, raised front and a lower shadow.
        rounded(dc, rect(0, 4, 16, 15), px(2).max(1), blend(dark, 0, 25))?;
        rounded(dc, rect(1, 1, 8, 6), px(2).max(1), light)?;
        rounded(dc, rect(0, 3, 15, 13), px(2).max(1), dark)?;
        paint::fill(dc, rect(2, 4, 13, 7), blend(light, 0x00ffffff, 26))?;
        rounded(dc, rect(0, 6, 16, 14), px(2).max(1), color)?;
        paint::fill(dc, rect(2, 6, 14, 7), light)?;
        paint::fill(dc, rect(2, 12, 14, 13), blend(color, 0, 18))?;
    } else {
        // Folded sheet with a tinted spine and distinct content strokes.
        rounded(dc, rect(3, 1, 15, 16), px(2).max(1), dark)?;
        rounded(
            dc,
            rect(2, 0, 14, 15),
            px(2).max(1),
            blend(color, 0x00ffffff, 70),
        )?;
        paint::fill(dc, rect(2, 2, 4, 13), color)?;
        paint::fill(dc, rect(10, 0, 14, 4), dark)?;
        paint::fill(dc, rect(10, 0, 12, 2), light)?;
        for (line, end) in [(6, 12), (9, 11), (12, 12)] {
            paint::fill(dc, rect(6, line, end, line + 1), dark)?;
        }
    }
    Ok(())
}
pub(super) fn file_color(name: &str) -> u32 {
    match name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "rs" => 0x006da5e8,
        "cs" | "php" => 0x00e0a29d,
        "java" => 0x0052adef,
        "js" | "jsx" => 0x0065d8ec,
        "ts" | "tsx" => 0x00e5b25c,
        "p" | "w" | "i" | "cls" => 0x00d5ccb0,
        "md" | "json" | "toml" | "xml" => 0x00b8c490,
        _ => 0x00b8a99b,
    }
}
#[link(name = "dwmapi")]
extern "system" {
    fn DwmSetWindowAttribute(hwnd: Hwnd, attribute: u32, value: *const i32, size: u32) -> i32;
    fn DwmGetWindowAttribute(hwnd: Hwnd, attribute: u32, value: *mut u32, size: u32) -> i32;
}
pub(super) unsafe fn set_caption_theme(hwnd: Hwnd, theme: super::appearance::Theme) {
    let enabled = i32::from(theme != super::appearance::Theme::Light);
    let _ = DwmSetWindowAttribute(hwnd, 20, &enabled, 4);
    let caption = theme.control_color(SURFACE) as i32;
    let text = theme.control_color(TEXT) as i32;
    let border = theme.control_color(SIDEBAR) as i32;
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
