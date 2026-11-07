//! One retained, bounded DIB owned by the UI thread. The renderer receives only
//! its memory DC. Large clients reuse the same DIB across horizontal bands.
//! No model, source, window dispatch or file access belongs to this module.
use super::ffi::*;
use std::ptr::null_mut;

const MAX_BYTES: usize = 32 * 1024 * 1024;
const MAX_DIMENSION: i32 = 8192;
const SRCCOPY: u32 = 0x00cc_0020;

#[link(name = "gdi32")]
extern "system" {
    fn BitBlt(
        destination: Hdc,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        source: Hdc,
        source_x: i32,
        source_y: i32,
        operation: u32,
    ) -> i32;
    fn SaveDC(dc: Hdc) -> i32;
    fn RestoreDC(dc: Hdc, saved: i32) -> i32;
    fn SetViewportOrgEx(dc: Hdc, x: i32, y: i32, old: *mut Point) -> i32;
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Statistics {
    /// Logical DIB pixel bytes; excludes handles/driver bookkeeping and is not RSS.
    pub allocated_bytes: usize,
    pub allocations: usize,
    pub compositions: usize,
    pub blits: usize,
}

pub(super) struct Buffer {
    dc: Hdc,
    bitmap: Handle,
    previous: Handle,
    width: i32,
    height: i32,
    statistics: Statistics,
}
impl Default for Buffer {
    fn default() -> Self {
        Self {
            dc: null_mut(),
            bitmap: null_mut(),
            previous: null_mut(),
            width: 0,
            height: 0,
            statistics: Statistics::default(),
        }
    }
}

fn byte_count(width: i32, height: i32) -> Result<usize, String> {
    if !(1..=MAX_DIMENSION).contains(&width) || !(1..=MAX_DIMENSION).contains(&height) {
        return Err("Dimensiones de superficie GUI fuera del límite (1–8192 píxeles)".into());
    }
    let bytes = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .filter(|bytes| *bytes <= MAX_BYTES)
        .ok_or("La superficie GUI supera su presupuesto de 32 MiB")?;
    Ok(bytes)
}

fn clipped(rect: Rect, width: i32, height: i32) -> Option<Rect> {
    let rect = Rect {
        left: rect.left.max(0),
        top: rect.top.max(0),
        right: rect.right.min(width),
        bottom: rect.bottom.min(height),
    };
    (rect.left < rect.right && rect.top < rect.bottom).then_some(rect)
}

fn invalid_object(object: Handle) -> bool {
    object.is_null() || object as isize == -1
}

impl Buffer {
    pub(super) fn statistics(&self) -> Statistics {
        self.statistics
    }

    unsafe fn release_bitmap(&mut self) {
        if !self.bitmap.is_null() {
            if !self.dc.is_null()
                && !invalid_object(self.previous)
                && invalid_object(SelectObject(self.dc, self.previous))
            {
                // Deleting the owning DC deselects the bitmap even if its
                // saved GDI state could not be restored after a draw error.
                DeleteDC(self.dc);
                self.dc = null_mut();
            }
            DeleteObject(self.bitmap);
        }
        self.bitmap = null_mut();
        self.previous = null_mut();
        self.width = 0;
        self.height = 0;
        self.statistics.allocated_bytes = 0;
    }

    unsafe fn ensure(&mut self, target: Hdc, width: i32, height: i32) -> Result<(), String> {
        let bytes = byte_count(width, height)?;
        if self.width == width && self.height == height && !self.bitmap.is_null() {
            return Ok(());
        }
        // Free before allocating the replacement: resize never owns two DIBs.
        // A failed resize leaves no unusable old-size pixel allocation retained.
        self.release_bitmap();
        if self.dc.is_null() {
            self.dc = CreateCompatibleDC(target);
            if self.dc.is_null() {
                return Err("No se pudo crear el contexto de composición GUI".into());
            }
        }
        let info = BitmapInfo {
            header: BitmapHeader {
                size: std::mem::size_of::<BitmapHeader>() as u32,
                width,
                height: -height,
                planes: 1,
                bit_count: 32,
                compression: 0,
                image_size: bytes as u32,
                x_meter: 0,
                y_meter: 0,
                used: 0,
                important: 0,
            },
            color: 0,
        };
        let mut bits = null_mut();
        let bitmap = CreateDIBSection(target, &info, 0, &mut bits, null_mut(), 0);
        if bitmap.is_null() || bits.is_null() {
            if !bitmap.is_null() {
                DeleteObject(bitmap);
            }
            return Err("No se pudo reservar la superficie de composición GUI".into());
        }
        let previous = SelectObject(self.dc, bitmap);
        if invalid_object(previous) {
            DeleteObject(bitmap);
            return Err("No se pudo seleccionar la superficie de composición GUI".into());
        }
        self.bitmap = bitmap;
        self.previous = previous;
        self.width = width;
        self.height = height;
        self.statistics.allocated_bytes = bytes;
        self.statistics.allocations = self.statistics.allocations.saturating_add(1);
        Ok(())
    }

    /// Use only on the owning UI thread, with a live target DC and no model
    /// borrow held across `draw`. `draw` must repaint its clip from the supplied
    /// immutable frame; old pixels are never a substitute for model validation.
    pub(super) unsafe fn paint(
        &mut self,
        target: Hdc,
        width: i32,
        height: i32,
        mut draw: impl FnMut(Hdc) -> Result<(), String>,
    ) -> Result<(), String> {
        if target.is_null() {
            return Err("La composición GUI no recibió un contexto de destino".into());
        }
        if !(1..=MAX_DIMENSION).contains(&width) || !(1..=MAX_DIMENSION).contains(&height) {
            return Err("Dimensiones de superficie GUI fuera del límite (1–8192 píxeles)".into());
        }
        let mut update = Rect::default();
        match GetClipBox(target, &mut update) {
            0 => return Err("No se pudo consultar la región de repintado GUI".into()),
            1 => return Ok(()), // Empty update: no allocation or composition.
            _ => {}
        }
        let Some(update) = clipped(update, width, height) else {
            return Ok(());
        };
        let buffer_height = height.min((MAX_BYTES / (width as usize * 4)) as i32);
        self.ensure(target, width, buffer_height)?;
        if buffer_height == height {
            return self.paint_band(target, update, 0, &mut draw);
        }
        // Anchor the bands at the start of the invalid area. A small update
        // needs only one pass even when it crosses a full-frame band boundary.
        let mut band = update;
        while band.top < update.bottom {
            band.bottom = (band.top + buffer_height).min(update.bottom);
            self.paint_band(target, band, band.top, &mut draw)?;
            band.top = band.bottom;
        }
        Ok(())
    }

    unsafe fn paint_band(
        &mut self,
        target: Hdc,
        update: Rect,
        origin_y: i32,
        draw: &mut impl FnMut(Hdc) -> Result<(), String>,
    ) -> Result<(), String> {
        let saved = SaveDC(self.dc);
        if saved == 0 {
            return Err("No se pudo preservar el contexto de composición GUI".into());
        }
        let result = (|| {
            if origin_y != 0 && SetViewportOrgEx(self.dc, 0, -origin_y, null_mut()) == 0 {
                return Err("No se pudo situar la banda de composición GUI".into());
            }
            if IntersectClipRect(
                self.dc,
                update.left,
                update.top,
                update.right,
                update.bottom,
            ) == 0
            {
                return Err("No se pudo limitar la región de composición GUI".into());
            }
            draw(self.dc)
        })();
        // Restore selected fonts, colors, clipping and other DC state even when
        // drawing fails. Only completed bands are published to the HWND.
        let restored = RestoreDC(self.dc, saved);
        if restored == 0 {
            self.release_bitmap();
            if !self.dc.is_null() {
                DeleteDC(self.dc);
                self.dc = null_mut();
            }
            return Err("No se pudo restaurar el contexto de composición GUI".into());
        }
        result?;
        self.statistics.compositions = self.statistics.compositions.saturating_add(1);
        if BitBlt(
            target,
            update.left,
            update.top,
            update.right - update.left,
            update.bottom - update.top,
            self.dc,
            update.left,
            update.top - origin_y,
            SRCCOPY,
        ) == 0
        {
            return Err("No se pudo presentar la composición GUI".into());
        }
        self.statistics.blits = self.statistics.blits.saturating_add(1);
        Ok(())
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        unsafe {
            self.release_bitmap();
            if !self.dc.is_null() {
                DeleteDC(self.dc);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Canvas {
        dc: Hdc,
        bitmap: Handle,
        previous: Handle,
        bits: *mut u32,
    }
    impl Canvas {
        unsafe fn new(height: i32) -> Self {
            let dc = CreateCompatibleDC(null_mut());
            assert!(!dc.is_null());
            let info = BitmapInfo {
                header: BitmapHeader {
                    size: std::mem::size_of::<BitmapHeader>() as u32,
                    width: 32,
                    height: -height,
                    planes: 1,
                    bit_count: 32,
                    compression: 0,
                    image_size: 32 * height as u32 * 4,
                    x_meter: 0,
                    y_meter: 0,
                    used: 0,
                    important: 0,
                },
                color: 0,
            };
            let mut bits = null_mut();
            let bitmap = CreateDIBSection(dc, &info, 0, &mut bits, null_mut(), 0);
            assert!(!bitmap.is_null() && !bits.is_null());
            let previous = SelectObject(dc, bitmap);
            assert!(!invalid_object(previous));
            std::ptr::write_bytes(bits, 0, 32 * height as usize * 4);
            Self {
                dc,
                bitmap,
                previous,
                bits: bits.cast(),
            }
        }
        unsafe fn pixel(&self, x: usize, y: usize) -> u32 {
            GdiFlush();
            *self.bits.add(y * 32 + x) & 0x00ff_ffff
        }
    }
    impl Drop for Canvas {
        fn drop(&mut self) {
            unsafe {
                SelectObject(self.dc, self.previous);
                DeleteObject(self.bitmap);
                DeleteDC(self.dc);
            }
        }
    }

    #[test]
    fn retained_surface_publishes_only_valid_update_and_reuses_allocation() {
        // Own memory DCs only; no window, display interaction or personal data.
        unsafe {
            let target = Canvas::new(32);
            let mut buffer = Buffer::default();
            assert_ne!(IntersectClipRect(target.dc, 8, 8, 16, 16), 0);
            buffer
                .paint(target.dc, 32, 32, |dc| {
                    let brush = CreateSolidBrush(0x0000_00ff);
                    assert!(!brush.is_null());
                    let filled = FillRect(
                        dc,
                        &Rect {
                            left: 0,
                            top: 0,
                            right: 32,
                            bottom: 32,
                        },
                        brush,
                    );
                    DeleteObject(brush);
                    assert_ne!(filled, 0);
                    Ok(())
                })
                .unwrap();
            assert_eq!(target.pixel(10, 10), 0x00ff_0000);
            assert_eq!(target.pixel(0, 0), 0);
            assert_eq!(target.pixel(20, 20), 0);
            buffer.paint(target.dc, 32, 32, |_| Ok(())).unwrap();
            assert_eq!(buffer.statistics().allocations, 1);
            assert_eq!(buffer.statistics().blits, 2);
            let before = buffer.statistics();
            assert!(buffer
                .paint(target.dc, 32, 32, |dc| {
                    let brush = CreateSolidBrush(0x0000_ff00);
                    assert!(!brush.is_null());
                    FillRect(
                        dc,
                        &Rect {
                            left: 0,
                            top: 0,
                            right: 32,
                            bottom: 32,
                        },
                        brush,
                    );
                    DeleteObject(brush);
                    Err("Error de composición inyectado".into())
                })
                .is_err());
            assert_eq!(buffer.statistics(), before);
            assert_eq!(target.pixel(10, 10), 0x00ff_0000);
            assert!(buffer
                .paint(target.dc, 8193, 8192, |_| panic!(
                    "invalid dimensions admitted"
                ))
                .is_err());
            assert_eq!(buffer.statistics().allocated_bytes, 4096);
        }
    }

    #[test]
    fn oversized_surface_composes_at_original_coordinates_across_band_seam() {
        unsafe {
            // Narrow destination with a nonzero origin for the invalid area.
            let target = Canvas::new(1026);
            let mut buffer = Buffer::default();
            assert_ne!(IntersectClipRect(target.dc, 8, 1, 16, 1026), 0);
            let draw = |dc| {
                for (top, bottom, color) in [(0, 1025, 0x0000_00ff), (1025, 1026, 0x0000_ff00)] {
                    let brush = CreateSolidBrush(color);
                    assert!(!brush.is_null());
                    let filled = FillRect(
                        dc,
                        &Rect {
                            left: 0,
                            top,
                            right: 8192,
                            bottom,
                        },
                        brush,
                    );
                    DeleteObject(brush);
                    assert_ne!(filled, 0);
                }
                Ok(())
            };
            buffer.paint(target.dc, 8192, 1026, draw).unwrap();
            assert_eq!(target.pixel(10, 1024), 0x00ff_0000);
            assert_eq!(target.pixel(10, 1025), 0x0000_ff00);
            assert_eq!(target.pixel(10, 0), 0);
            assert_eq!(target.pixel(7, 1025), 0);
            assert_eq!(buffer.statistics().allocated_bytes, MAX_BYTES);
            assert_eq!(buffer.statistics().allocations, 1);
            assert_eq!(buffer.statistics().blits, 2);

            assert_ne!(IntersectClipRect(target.dc, 8, 1000, 16, 1026), 0);
            buffer.paint(target.dc, 8192, 1026, draw).unwrap();
            assert_eq!(target.pixel(10, 1025), 0x0000_ff00);
            assert_eq!(buffer.statistics().allocations, 1);
            assert_eq!(buffer.statistics().blits, 3);
        }
    }

    #[test]
    fn surface_budget_rejects_invalid_and_excessive_sizes_before_allocation() {
        assert_eq!(byte_count(1000, 650).unwrap(), 2_600_000);
        assert_eq!(byte_count(4096, 2048).unwrap(), MAX_BYTES);
        for size in [
            (0, 1),
            (-1, 650),
            (1, 8193),
            (8192, 8192),
            (i32::MAX, i32::MAX),
        ] {
            assert!(byte_count(size.0, size.1).is_err(), "{size:?}");
        }
    }

    #[test]
    fn update_rectangle_is_clamped_and_empty_regions_do_not_publish() {
        let region = clipped(
            Rect {
                left: -5,
                top: -20,
                right: 1200,
                bottom: 700,
            },
            1000,
            650,
        )
        .unwrap();
        assert_eq!(
            (region.left, region.top, region.right, region.bottom),
            (0, 0, 1000, 650)
        );
        assert!(clipped(
            Rect {
                left: 1000,
                top: 1,
                right: 1200,
                bottom: 30
            },
            1000,
            650
        )
        .is_none());
        assert!(clipped(
            Rect {
                left: 20,
                top: 20,
                right: 10,
                bottom: 10
            },
            1000,
            650
        )
        .is_none());
    }
}
