//! Native semantic pictograms. Logical groups are symbols, never filesystem folders.
use super::{
    super::{dpi::Dpi, ffi::*, os_error, paint, shell},
    tree::Kind,
};

#[link(name = "gdi32")]
extern "system" {
    fn CreatePen(style: i32, width: i32, color: u32) -> Handle;
    fn Polyline(dc: Hdc, points: *const Point, count: i32) -> i32;
    fn Polygon(dc: Hdc, points: *const Point, count: i32) -> i32;
    fn RoundRect(dc: Hdc, l: i32, t: i32, r: i32, b: i32, w: i32, h: i32) -> i32;
    fn Ellipse(dc: Hdc, l: i32, t: i32, r: i32, b: i32) -> i32;
    fn SaveDC(dc: Hdc) -> i32;
    fn RestoreDC(dc: Hdc, saved: i32) -> i32;
}
fn valid(h: Handle) -> bool {
    !h.is_null() && h as isize != -1
}
struct Style {
    dc: Hdc,
    pen: Handle,
    brush: Handle,
    old_pen: Handle,
    old_brush: Handle,
    owned_brush: bool,
}
impl Drop for Style {
    fn drop(&mut self) {
        unsafe {
            if valid(self.old_brush) {
                SelectObject(self.dc, self.old_brush);
            }
            if valid(self.old_pen) {
                SelectObject(self.dc, self.old_pen);
            }
            if self.owned_brush && valid(self.brush) {
                DeleteObject(self.brush);
            }
            if valid(self.pen) {
                DeleteObject(self.pen);
            }
        }
    }
}
struct Canvas {
    dc: Hdc,
    bounds: Rect,
    dpi: Dpi,
}
impl Canvas {
    fn point(&self, x: i32, y: i32) -> Point {
        Point {
            x: self.bounds.left + self.dpi.pixels(x),
            y: self.bounds.top + self.dpi.pixels(y),
        }
    }
    unsafe fn styled(
        &self,
        ink: u32,
        fill: Option<u32>,
        draw: impl FnOnce() -> bool,
    ) -> Result<(), String> {
        let pen = CreatePen(0, self.dpi.pixels(1).max(1), ink);
        let brush = fill.map_or_else(|| GetStockObject(5), |c| CreateSolidBrush(c));
        let mut style = Style {
            dc: self.dc,
            pen,
            brush,
            old_pen: std::ptr::null_mut(),
            old_brush: std::ptr::null_mut(),
            owned_brush: fill.is_some(),
        };
        if !valid(pen) || !valid(brush) {
            return Err(os_error("Preparar icono de símbolo"));
        }
        style.old_pen = SelectObject(self.dc, pen);
        style.old_brush = SelectObject(self.dc, brush);
        if !valid(style.old_pen) || !valid(style.old_brush) {
            return Err(os_error("Seleccionar icono de símbolo"));
        }
        draw()
            .then_some(())
            .ok_or_else(|| os_error("Dibujar icono de símbolo"))
    }
    unsafe fn line<const N: usize>(&self, p: [(i32, i32); N]) -> bool {
        let p = p.map(|(x, y)| self.point(x, y));
        Polyline(self.dc, p.as_ptr(), N as i32) != 0
    }
    unsafe fn polygon<const N: usize>(
        &self,
        p: [(i32, i32); N],
        ink: u32,
        fill: u32,
    ) -> Result<(), String> {
        let p = p.map(|(x, y)| self.point(x, y));
        self.styled(ink, Some(fill), || {
            Polygon(self.dc, p.as_ptr(), N as i32) != 0
        })
    }
    unsafe fn oval(&self, l: i32, t: i32, r: i32, b: i32) -> bool {
        let a = self.point(l, t);
        let z = self.point(r, b);
        Ellipse(self.dc, a.x, a.y, z.x, z.y) != 0
    }
    unsafe fn rounded(&self, l: i32, t: i32, r: i32, b: i32) -> bool {
        let a = self.point(l, t);
        let z = self.point(r, b);
        RoundRect(
            self.dc,
            a.x,
            a.y,
            z.x,
            z.y,
            self.dpi.pixels(3),
            self.dpi.pixels(3),
        ) != 0
    }
    unsafe fn cube(&self, ink: u32, bg: u32) -> Result<(), String> {
        self.polygon(
            [(2, 5), (10, 9), (10, 17), (2, 13)],
            shell::blend(ink, bg, 22),
            shell::blend(ink, bg, 83),
        )?;
        self.polygon(
            [(10, 9), (18, 5), (18, 13), (10, 17)],
            ink,
            shell::blend(ink, bg, 65),
        )?;
        self.polygon(
            [(2, 5), (10, 1), (18, 5), (10, 9)],
            ink,
            shell::blend(ink, bg, 42),
        )?;
        // Recessed opening and a lighter rim make a hollow object at 16–20 px.
        self.polygon(
            [(5, 5), (10, 3), (15, 5), (10, 7)],
            shell::blend(ink, bg, 12),
            bg,
        )?;
        self.styled(shell::blend(ink, 0x00ffffff, 30), None, || {
            self.line([(2, 6), (2, 13), (10, 17)])
        })
    }
    unsafe fn function(&self, prototype: bool) -> bool {
        self.line([(8, 3), (6, 2), (4, 3), (4, 13), (2, 15)])
            && self.line([(2, 7), (8, 7)])
            && self.line([(12, 4), (10, 6), (10, 12), (12, 14)])
            && self.line([(17, 4), (19, 6), (19, 12), (17, 14)])
            && self.line([(13, 7), (16, 11)])
            && self.line([(16, 7), (13, 11)])
            && (!prototype
                || self.line([(7, 16), (9, 16)])
                    && self.line([(12, 16), (14, 16)])
                    && self.line([(17, 16), (19, 16)]))
    }
}

pub(super) unsafe fn draw(
    dc: Hdc,
    bounds: Rect,
    dpi: Dpi,
    kind: Kind,
    background: u32,
    unresolved: bool,
) -> Result<(), String> {
    if RectVisible(dc, &bounds) == 0 {
        return Ok(());
    }
    let saved = SaveDC(dc);
    if saved == 0 {
        return Err(os_error("Conservar área de icono"));
    }
    let result = (|| {
        if IntersectClipRect(dc, bounds.left, bounds.top, bounds.right, bounds.bottom) == 0 {
            return Err(os_error("Recortar icono"));
        }
        let c = Canvas { dc, bounds, dpi };
        let ink = paint::ui_ink(if unresolved {
            shell::MUTED
        } else {
            kind.icon().1
        });
        let bg = paint::ui_color(background);
        let tint = shell::blend(ink, bg, 84);
        use Kind::*;
        if matches!(kind, Type | Implementation) {
            c.cube(ink, bg)?;
            if kind == Implementation {
                c.styled(ink, None, || c.line([(13, 12), (15, 14), (19, 10)]))?;
            }
            return Ok(());
        }
        if kind == Module {
            c.polygon([(2, 5), (10, 1), (18, 5), (10, 9)], ink, tint)?;
            return c.styled(ink, None, || {
                c.line([(2, 9), (10, 13), (18, 9)]) && c.line([(2, 13), (10, 17), (18, 13)])
            });
        }
        if kind == Method {
            c.polygon(
                [
                    (8, 1),
                    (12, 1),
                    (12, 3),
                    (14, 4),
                    (16, 3),
                    (18, 6),
                    (16, 8),
                    (16, 10),
                    (18, 12),
                    (16, 15),
                    (14, 14),
                    (12, 15),
                    (12, 17),
                    (8, 17),
                    (8, 15),
                    (6, 14),
                    (4, 15),
                    (2, 12),
                    (4, 10),
                    (4, 8),
                    (2, 6),
                    (4, 3),
                    (6, 4),
                    (8, 3),
                ],
                ink,
                tint,
            )?;
            return c.styled(ink, Some(bg), || c.oval(7, 6, 13, 12));
        }
        if matches!(kind, Include | Import) {
            c.styled(shell::blend(ink, bg, 28), Some(tint), || {
                c.rounded(5, 1, 18, 14)
            })?;
            c.styled(ink, Some(bg), || c.rounded(1, 4, 15, 17))?;
            return c.styled(ink, None, || {
                if kind == Include {
                    c.line([(6, 8), (3, 10), (6, 12)]) && c.line([(10, 8), (13, 10), (10, 12)])
                } else {
                    c.line([(4, 13), (13, 4)]) && c.line([(8, 4), (13, 4), (13, 9)])
                }
            });
        }
        if matches!(kind, Table | WorkTable) {
            c.styled(ink, Some(tint), || c.rounded(2, 2, 18, 16))?;
            c.styled(ink, None, || {
                c.line([(2, 6), (18, 6)])
                    && c.line([(2, 11), (18, 11)])
                    && c.line([(7, 6), (7, 16)])
                    && c.line([(12, 6), (12, 16)])
            })?;
            if kind == Table {
                c.styled(ink, Some(bg), || c.oval(11, 9, 19, 17))?;
                c.styled(ink, None, || c.line([(15, 11), (15, 14), (17, 14)]))?;
            } else {
                c.styled(ink, None, || c.line([(1, 14), (1, 17), (17, 17)]))?;
            }
            return Ok(());
        }
        if kind == Procedure {
            c.styled(ink, Some(tint), || c.rounded(2, 1, 17, 17))?;
            c.styled(ink, None, || c.line([(5, 4), (13, 4)]))?;
            return c.polygon([(7, 7), (13, 11), (7, 15)], ink, ink);
        }
        c.styled(ink, None, || match kind {
            Function => c.function(false),
            Prototype => c.function(true),
            Namespace => {
                c.rounded(7, 1, 13, 6)
                    && c.rounded(1, 12, 7, 17)
                    && c.rounded(13, 12, 19, 17)
                    && c.line([(10, 6), (10, 9), (4, 9), (4, 12)])
                    && c.line([(10, 9), (16, 9), (16, 12)])
            }
            Buffer => {
                c.rounded(2, 4, 14, 17)
                    && c.rounded(6, 1, 18, 14)
                    && c.line([(9, 5), (15, 5)])
                    && c.line([(9, 8), (15, 8)])
            }
            Parameter => {
                c.rounded(10, 2, 19, 16)
                    && c.line([(1, 9), (14, 9)])
                    && c.line([(10, 5), (14, 9), (10, 13)])
            }
            Variable => {
                c.rounded(2, 2, 7, 7)
                    && c.rounded(2, 11, 7, 16)
                    && c.line([(10, 4), (18, 4)])
                    && c.line([(10, 13), (18, 13)])
            }
            Field => {
                c.rounded(2, 4, 18, 14) && c.line([(7, 4), (7, 14)]) && c.line([(10, 9), (15, 9)])
            }
            Variant => {
                c.line([(10, 1), (17, 8), (10, 15), (3, 8), (10, 1)])
                    && c.line([(10, 5), (13, 8), (10, 11), (7, 8), (10, 5)])
            }
            Index => {
                c.line([(8, 2), (5, 16)])
                    && c.line([(15, 2), (12, 16)])
                    && c.line([(3, 6), (18, 6)])
                    && c.line([(2, 12), (17, 12)])
            }
            Macro => c.line([
                (11, 1),
                (3, 10),
                (9, 10),
                (7, 17),
                (17, 7),
                (11, 7),
                (11, 1),
            ]),
            Data => {
                c.oval(2, 2, 18, 8)
                    && c.line([(2, 5), (2, 14)])
                    && c.line([(18, 5), (17, 14)])
                    && c.oval(2, 10, 18, 17)
            }
            Widget => {
                c.rounded(2, 2, 18, 16) && c.line([(2, 6), (18, 6)]) && c.rounded(5, 9, 10, 13)
            }
            CallOutgoing => c.line([(2, 9), (18, 9)]) && c.line([(12, 3), (18, 9), (12, 15)]),
            CallIncoming => c.line([(18, 9), (2, 9)]) && c.line([(8, 3), (2, 9), (8, 15)]),
            CallUnresolved => {
                c.line([
                    (5, 5),
                    (7, 2),
                    (13, 2),
                    (16, 5),
                    (15, 8),
                    (10, 11),
                    (10, 13),
                ]) && c.oval(9, 15, 11, 17)
            }
            _ => c.line([(10, 2), (17, 9), (10, 16), (3, 9), (10, 2)]) && c.oval(8, 7, 12, 11),
        })
    })();
    RestoreDC(dc, saved);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{mem::size_of, ptr::null_mut};
    const KINDS: [Kind; 24] = [
        Kind::Type,
        Kind::Method,
        Kind::Function,
        Kind::Procedure,
        Kind::Prototype,
        Kind::Table,
        Kind::WorkTable,
        Kind::Include,
        Kind::Import,
        Kind::Variable,
        Kind::Parameter,
        Kind::Buffer,
        Kind::Module,
        Kind::Implementation,
        Kind::Namespace,
        Kind::Field,
        Kind::Variant,
        Kind::Index,
        Kind::Macro,
        Kind::Data,
        Kind::Widget,
        Kind::Other,
        Kind::CallOutgoing,
        Kind::CallIncoming,
    ];
    struct Surface {
        dc: Hdc,
        bitmap: Handle,
        old: Handle,
        bits: *mut u32,
        width: i32,
        height: i32,
    }
    impl Surface {
        unsafe fn new(width: i32, height: i32) -> Self {
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
            assert!(valid(bitmap) && !bits.is_null());
            let old = SelectObject(dc, bitmap);
            assert!(valid(old));
            Self {
                dc,
                bitmap,
                old,
                bits: bits.cast(),
                width,
                height,
            }
        }
        fn rect(&self) -> Rect {
            Rect {
                left: 0,
                top: 0,
                right: self.width,
                bottom: self.height,
            }
        }
        unsafe fn pixels(&self) -> &[u32] {
            assert_ne!(GdiFlush(), 0);
            std::slice::from_raw_parts(self.bits, (self.width * self.height) as usize)
        }
    }
    impl Drop for Surface {
        fn drop(&mut self) {
            unsafe {
                SelectObject(self.dc, self.old);
                DeleteObject(self.bitmap);
                DeleteDC(self.dc);
            }
        }
    }
    #[test]
    fn semantic_icons_are_distinct_clipped_and_restore_gdi_at_all_dpi_and_themes() {
        for dpi in [96, 144, 192].map(Dpi::from_raw) {
            let surface = unsafe { Surface::new(dpi.pixels(30), dpi.pixels(26)) };
            let bounds = Rect {
                left: dpi.pixels(4),
                top: dpi.pixels(4),
                right: dpi.pixels(24),
                bottom: dpi.pixels(22),
            };
            for theme in [
                super::super::super::appearance::Theme::Current,
                super::super::super::appearance::Theme::Light,
                super::super::super::appearance::Theme::Contrast,
            ] {
                let _colors = paint::UiColors::enter(theme);
                for bg in [shell::SIDEBAR, shell::SELECTED] {
                    let mut masks = std::collections::HashSet::new();
                    for kind in KINDS.into_iter().chain([Kind::CallUnresolved]) {
                        unsafe {
                            paint::fill(surface.dc, surface.rect(), bg).unwrap();
                            let sentinel = surface.pixels()[0] & 0xffffff;
                            draw(surface.dc, bounds, dpi, kind, bg, false).unwrap();
                            let pixels = surface.pixels();
                            let color = paint::ui_ink(kind.icon().1);
                            let ink =
                                ((color & 0xff) << 16) | (color & 0xff00) | ((color >> 16) & 0xff);
                            // Compare edge/fill geometry independent of hue. A binary
                            // silhouette loses the detail inside shaded objects.
                            let mask: Vec<u8> = pixels
                                .iter()
                                .map(|pixel| {
                                    let value = pixel & 0xffffff;
                                    if value == sentinel {
                                        0
                                    } else if value == ink {
                                        2
                                    } else {
                                        1
                                    }
                                })
                                .collect();
                            assert!(mask.iter().filter(|&&v| v != 0).count() >= 14, "{kind:?}");
                            assert!(masks.insert(mask), "{kind:?} at {}", dpi.pixels(20));
                            for (i, pixel) in pixels.iter().enumerate() {
                                let x = i as i32 % surface.width;
                                let y = i as i32 / surface.width;
                                if x < bounds.left
                                    || x >= bounds.right
                                    || y < bounds.top
                                    || y >= bounds.bottom
                                {
                                    assert_eq!(
                                        pixel & 0xffffff,
                                        sentinel,
                                        "{kind:?}: escaped bounds"
                                    );
                                }
                            }
                            // RestoreDC must also restore the caller's clip before the next symbol.
                            paint::fill(
                                surface.dc,
                                Rect {
                                    left: 0,
                                    top: 0,
                                    right: 1,
                                    bottom: 1,
                                },
                                0x00010203,
                            )
                            .unwrap();
                            assert_eq!(surface.pixels()[0] & 0xffffff, 0x00030201);
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn export_native_semantic_icon_gallery_when_requested() {
        let Some(path) = std::env::var_os("CIDE_OUTLINE_ICON_PREVIEW") else {
            return;
        };
        let path = std::path::PathBuf::from(path);
        let work = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../work")
            .canonicalize()
            .unwrap();
        assert!(path
            .parent()
            .unwrap()
            .canonicalize()
            .unwrap()
            .starts_with(work));
        let entries = [
            ("Clases y objetos", Kind::Type),
            ("Métodos", Kind::Method),
            ("Funciones", Kind::Function),
            ("Procedimientos", Kind::Procedure),
            ("Tablas temporales", Kind::Table),
            ("Inclusiones", Kind::Include),
            ("Variables y constantes", Kind::Variable),
            ("Parámetros", Kind::Parameter),
            ("Búferes", Kind::Buffer),
            ("Módulos", Kind::Module),
            ("Prototipos de función", Kind::Prototype),
            ("Conjuntos de datos", Kind::Data),
        ];
        unsafe {
            let surface = Surface::new(800, 340);
            paint::fill(surface.dc, surface.rect(), shell::SIDEBAR).unwrap();
            let font = SelectObject(surface.dc, GetStockObject(17));
            assert!(valid(font));
            paint::text(
                surface.dc,
                Rect {
                    left: 20,
                    top: 10,
                    right: 790,
                    bottom: 34,
                },
                &super::super::super::wide("CIDE · Iconos semánticos nativos · 20 px / 40 px"),
                shell::TEXT,
                shell::SIDEBAR,
            )
            .unwrap();
            for (i, (label, kind)) in entries.iter().enumerate() {
                let x = 12 + (i % 4) as i32 * 196;
                let y = 46 + (i / 4) as i32 * 96;
                shell::rounded(
                    surface.dc,
                    Rect {
                        left: x,
                        top: y,
                        right: x + 186,
                        bottom: y + 86,
                    },
                    8,
                    shell::BACKGROUND,
                )
                .unwrap();
                paint::text(
                    surface.dc,
                    Rect {
                        left: x + 10,
                        top: y + 9,
                        right: x + 180,
                        bottom: y + 29,
                    },
                    &super::super::super::wide(label),
                    shell::TEXT,
                    shell::BACKGROUND,
                )
                .unwrap();
                draw(
                    surface.dc,
                    Rect {
                        left: x + 22,
                        top: y + 47,
                        right: x + 42,
                        bottom: y + 65,
                    },
                    Dpi::from_raw(96),
                    *kind,
                    shell::BACKGROUND,
                    false,
                )
                .unwrap();
                draw(
                    surface.dc,
                    Rect {
                        left: x + 96,
                        top: y + 34,
                        right: x + 136,
                        bottom: y + 70,
                    },
                    Dpi::from_raw(192),
                    *kind,
                    shell::BACKGROUND,
                    false,
                )
                .unwrap();
            }
            SelectObject(surface.dc, font);
            let pixels = surface.pixels();
            let bytes = std::slice::from_raw_parts(pixels.as_ptr().cast::<u8>(), pixels.len() * 4);
            let mut header = [0u8; 54];
            header[..2].copy_from_slice(b"BM");
            header[2..6].copy_from_slice(&(54 + bytes.len() as u32).to_le_bytes());
            header[10..14].copy_from_slice(&54u32.to_le_bytes());
            header[14..18].copy_from_slice(&40u32.to_le_bytes());
            header[18..22].copy_from_slice(&surface.width.to_le_bytes());
            header[22..26].copy_from_slice(&(-surface.height).to_le_bytes());
            header[26..28].copy_from_slice(&1u16.to_le_bytes());
            header[28..30].copy_from_slice(&32u16.to_le_bytes());
            use std::io::Write;
            let mut output = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(path)
                .unwrap();
            output.write_all(&header).unwrap();
            output.write_all(bytes).unwrap();
        }
    }
}
