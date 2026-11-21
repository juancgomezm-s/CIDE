//! Small native pictograms on a 22 × 16 logical-pixel grid.
use super::super::{
    dpi::Dpi,
    ffi::{
        CreateSolidBrush, DeleteObject, GetStockObject, Handle, Hdc, Point, Rect, RectVisible,
        SelectObject,
    },
    os_error, paint, shell,
};
use jc_app::source_services::ValueType;

#[link(name = "gdi32")]
extern "system" {
    fn CreatePen(style: i32, width: i32, color: u32) -> Handle;
    fn Polyline(dc: Hdc, points: *const Point, count: i32) -> i32;
    fn RoundRect(dc: Hdc, l: i32, t: i32, r: i32, b: i32, w: i32, h: i32) -> i32;
    fn Ellipse(dc: Hdc, l: i32, t: i32, r: i32, b: i32) -> i32;
}

const CYAN: u32 = 0x00eed322;
const BLUE: u32 = 0x00faa560;
const VIOLET: u32 = 0x00fa9ab2;
const GREEN: u32 = 0x008be858;
const NULL_BRUSH: i32 = 5;

fn valid(object: Handle) -> bool {
    !object.is_null() && object as isize != -1
}

struct Icon {
    dc: Hdc,
    bounds: Rect,
    dpi: Dpi,
}
impl Icon {
    fn point(&self, x: i32, y: i32) -> Point {
        Point {
            x: self.bounds.left + self.dpi.pixels(x),
            y: self.bounds.top + self.dpi.pixels(y),
        }
    }

    unsafe fn line<const N: usize>(&self, points: [(i32, i32); N]) -> bool {
        let points = points.map(|(x, y)| self.point(x, y));
        Polyline(self.dc, points.as_ptr(), N as i32) != 0
    }

    unsafe fn rounded(&self, l: i32, t: i32, r: i32, b: i32, radius: i32) -> bool {
        let a = self.point(l, t);
        let z = self.point(r, b);
        let size = self.dpi.pixels(radius);
        RoundRect(self.dc, a.x, a.y, z.x, z.y, size, size) != 0
    }

    unsafe fn oval(&self, l: i32, t: i32, r: i32, b: i32) -> bool {
        let a = self.point(l, t);
        let z = self.point(r, b);
        Ellipse(self.dc, a.x, a.y, z.x, z.y) != 0
    }

    unsafe fn dot(&self, x: i32, y: i32, color: u32) -> Result<(), String> {
        let a = self.point(x, y);
        let b = self.point(x + 2, y + 2);
        paint::fill(
            self.dc,
            Rect {
                left: a.x,
                top: a.y,
                right: b.x,
                bottom: b.y,
            },
            paint::ui_ink(color),
        )
    }

    unsafe fn calendar(&self, right: i32, bottom: i32) -> bool {
        self.rounded(4, 2, right, bottom, 4)
            && self.line([(4, 6), (right - 1, 6)])
            && self.line([(8, 0), (8, 4)])
            && self.line([(right - 5, 0), (right - 5, 4)])
    }

    unsafe fn switch(&self, color: u32, background: u32) -> Result<bool, String> {
        let track = self.rounded(1, 3, 21, 12, 9);
        let brush = CreateSolidBrush(paint::ui_ink(color));
        if !valid(brush) {
            return Err(os_error("Preparar relleno del interruptor"));
        }
        let old_brush = SelectObject(self.dc, brush);
        let knob = valid(old_brush) && self.oval(12, 3, 21, 12);
        if valid(old_brush) {
            SelectObject(self.dc, old_brush);
        }
        DeleteObject(brush);

        let pen = CreatePen(0, self.dpi.pixels(1).max(1), paint::ui_color(background));
        if !valid(pen) {
            return Err(os_error("Preparar marca del interruptor"));
        }
        let old_pen = SelectObject(self.dc, pen);
        let check = valid(old_pen) && self.line([(14, 7), (16, 9), (19, 5)]);
        if valid(old_pen) {
            SelectObject(self.dc, old_pen);
        }
        DeleteObject(pen);
        Ok(track && knob && check)
    }
}

pub(super) unsafe fn draw(
    dc: Hdc,
    bounds: Rect,
    dpi: Dpi,
    value: Option<ValueType>,
    background: u32,
) -> Result<(), String> {
    use ValueType::*;
    if RectVisible(dc, &bounds) == 0 {
        return Ok(());
    }
    let icon = Icon { dc, bounds, dpi };
    let color = match value {
        Some(Text | Object | Array) => CYAN,
        Some(Integer | Decimal) => BLUE,
        Some(Date | DateTime) => VIOLET,
        Some(Boolean | Binary) => GREEN,
        None => shell::MUTED,
    };
    let pen = CreatePen(0, dpi.pixels(1).max(1), paint::ui_ink(color));
    if !valid(pen) {
        return Err(os_error("Preparar icono de tipo"));
    }
    let old_pen = SelectObject(dc, pen);
    if !valid(old_pen) {
        DeleteObject(pen);
        return Err(os_error("Seleccionar trazo de icono"));
    }
    let old_brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
    if !valid(old_brush) {
        SelectObject(dc, old_pen);
        DeleteObject(pen);
        return Err(os_error("Seleccionar fondo de icono"));
    }
    let result = (|| {
        let ok = match value {
            Some(Text) => {
                // Two curly quotation marks, each with its own filled bowl.
                for x in [5, 13] {
                    icon.dot(x, 7, color)?;
                    icon.dot(x + 1, 10, color)?;
                }
                icon.line([
                    (8, 2),
                    (6, 4),
                    (5, 7),
                    (5, 10),
                    (8, 11),
                    (9, 9),
                    (8, 7),
                    (5, 7),
                ]) && icon.line([
                    (16, 2),
                    (14, 4),
                    (13, 7),
                    (13, 10),
                    (16, 11),
                    (17, 9),
                    (16, 7),
                    (13, 7),
                ])
            }
            Some(Integer) => {
                icon.line([(9, 1), (6, 13)])
                    && icon.line([(15, 1), (12, 13)])
                    && icon.line([(4, 5), (17, 5)])
                    && icon.line([(3, 9), (16, 9)])
            }
            Some(Decimal) => {
                icon.dot(6, 10, color)?;
                icon.line([(0, 2), (4, 2), (4, 7), (1, 7), (4, 7), (4, 12), (0, 12)])
                    && icon.line([(9, 5), (12, 2), (12, 12)])
                    && icon.line([(17, 2), (15, 8), (21, 8)])
                    && icon.line([(19, 4), (19, 12)])
            }
            Some(Boolean) => icon.switch(color, background)?,
            Some(Date) => {
                icon.dot(7, 8, color)?;
                icon.dot(12, 8, color)?;
                icon.dot(7, 11, color)?;
                icon.dot(12, 11, color)?;
                icon.calendar(18, 14)
            }
            Some(DateTime) => {
                let calendar = icon.calendar(16, 12);
                let a = icon.point(11, 7);
                let b = icon.point(21, 14);
                paint::fill(
                    dc,
                    Rect {
                        left: a.x,
                        top: a.y,
                        right: b.x,
                        bottom: b.y,
                    },
                    background,
                )?;
                calendar && icon.oval(11, 6, 21, 15) && icon.line([(16, 8), (16, 11), (19, 11)])
            }
            Some(Array) => {
                icon.dot(7, 3, color)?;
                icon.dot(7, 6, color)?;
                icon.dot(7, 9, color)?;
                icon.line([(6, 1), (3, 1), (3, 13), (6, 13)])
                    && icon.line([(16, 1), (19, 1), (19, 13), (16, 13)])
                    && icon.line([(10, 4), (15, 4)])
                    && icon.line([(10, 7), (15, 7)])
                    && icon.line([(10, 10), (15, 10)])
            }
            Some(Object) => {
                icon.line([
                    (11, 0),
                    (18, 4),
                    (18, 10),
                    (11, 14),
                    (4, 10),
                    (4, 4),
                    (11, 0),
                ]) && icon.line([(4, 4), (11, 8), (18, 4)])
                    && icon.line([(11, 8), (11, 14)])
            }
            Some(Binary) => {
                icon.rounded(3, 1, 19, 14, 4)
                    && icon.line([(6, 4), (9, 4), (9, 10), (6, 10), (6, 4)])
                    && icon.line([(12, 6), (14, 4), (14, 10)])
            }
            None => {
                for x in [5, 10, 15] {
                    icon.dot(x, 7, color)?;
                }
                true
            }
        };
        ok.then_some(())
            .ok_or_else(|| os_error("Dibujar icono de tipo"))
    })();
    SelectObject(dc, old_brush);
    SelectObject(dc, old_pen);
    DeleteObject(pen);
    result
}

#[cfg(test)]
mod tests {
    use super::{super::super::ffi::*, *};
    use std::{mem::size_of, ptr::null_mut};

    #[test]
    fn outline_value_icons_draw_distinct_native_shapes_at_each_dpi() {
        use ValueType::*;
        let types = [
            Text, Integer, Decimal, Boolean, Date, DateTime, Array, Object, Binary,
        ];
        for dpi in [96, 144, 192].map(Dpi::from_raw) {
            let width = dpi.pixels(26);
            let height = dpi.pixels(20);
            let bounds = Rect {
                left: 2,
                top: 2,
                right: width,
                bottom: height,
            };
            unsafe {
                let dc = CreateCompatibleDC(null_mut());
                assert!(!dc.is_null());
                let mut bits = null_mut();
                let info = BitmapInfo {
                    header: BitmapHeader {
                        size: size_of::<BitmapHeader>() as u32,
                        width,
                        height: -height,
                        planes: 1,
                        bit_count: 32,
                        compression: 0,
                        image_size: (width * height * 4) as u32,
                        x_meter: 0,
                        y_meter: 0,
                        used: 0,
                        important: 0,
                    },
                    color: 0,
                };
                let bitmap = CreateDIBSection(dc, &info, 0, &mut bits, null_mut(), 0);
                assert!(!bitmap.is_null() && !bits.is_null());
                let previous = SelectObject(dc, bitmap);
                assert!(valid(previous));
                let mut masks = std::collections::HashSet::new();
                let surface = Rect {
                    left: 0,
                    top: 0,
                    right: width,
                    bottom: height,
                };
                for value in types.map(Some).into_iter().chain([None]) {
                    paint::fill(dc, surface, shell::SIDEBAR).unwrap();
                    draw(dc, bounds, dpi, value, shell::SIDEBAR).unwrap();
                    assert_ne!(GdiFlush(), 0);
                    let pixels =
                        std::slice::from_raw_parts(bits.cast::<u32>(), (width * height) as usize);
                    let mask: Vec<_> = pixels
                        .iter()
                        .map(|pixel| pixel & 0x00ffffff != pixels[0] & 0x00ffffff)
                        .collect();
                    assert!(mask.iter().filter(|&&ink| ink).count() >= 10, "{value:?}");
                    assert!(masks.insert(mask), "{value:?}");
                }
                SelectObject(dc, previous);
                DeleteObject(bitmap);
                DeleteDC(dc);
            }
        }
    }
}
