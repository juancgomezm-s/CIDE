//! Tiny coverage atlas sampled from the actual editor font, without owning it.
use crate::windows::{editor_fonts::Face, ffi::*, State};
use std::{ptr::null_mut, sync::Arc};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::windows) struct FontSpec {
    font: Handle,
    width: i32,
    height: i32,
    face: Face,
    size: u8,
    zoom: u8,
}
impl FontSpec {
    pub(in crate::windows) fn from_state(state: &State) -> Option<Self> {
        (!state.font.is_null()).then_some(Self {
            font: state.font,
            width: state.cell_width,
            height: state.cell_height,
            face: state.editor_face,
            size: state.editor_text_size,
            zoom: state.editor_zoom,
        })
    }
}
pub(super) struct Atlas([[u8; 6]; 128]);
impl Atlas {
    pub(super) fn coverage(&self, glyph: u8, x: usize, y: usize) -> u32 {
        u32::from(self.0[usize::from(glyph)][y * 2 + x])
    }
}
pub(super) unsafe fn build(font: FontSpec) -> Option<Arc<Atlas>> {
    let (w, h) = (font.width, font.height);
    if !(2..=128).contains(&w) || !(3..=256).contains(&h) || w * h * 96 > 2 * 1024 * 1024 {
        return None;
    }
    let dc = CreateCompatibleDC(null_mut());
    if dc.is_null() {
        return None;
    }
    let info = BitmapInfo {
        header: BitmapHeader {
            size: 40,
            width: w * 16,
            height: -h * 6,
            planes: 1,
            bit_count: 32,
            compression: 0,
            image_size: 0,
            x_meter: 0,
            y_meter: 0,
            used: 0,
            important: 0,
        },
        color: 0,
    };
    let mut bits = null_mut();
    let bitmap = CreateDIBSection(dc, &info, 0, &mut bits, null_mut(), 0);
    if bitmap.is_null() || bits.is_null() {
        if !bitmap.is_null() {
            DeleteObject(bitmap);
        }
        DeleteDC(dc);
        return None;
    }
    let old_bitmap = SelectObject(dc, bitmap);
    let old_font = SelectObject(dc, font.font);
    let result = (|| {
        if old_bitmap.is_null() || old_font.is_null() {
            return None;
        }
        SetTextColor(dc, 0x00ffffff);
        SetBkColor(dc, 0);
        for i in 0..96 {
            let x = (i % 16) * w;
            let y = (i / 16) * h;
            let rect = Rect {
                left: x,
                top: y,
                right: x + w,
                bottom: y + h,
            };
            let ch = if i == 95 { 0xfffd } else { (i + 32) as u16 };
            if ExtTextOutW(dc, x, y, 6, &rect, &ch, 1, std::ptr::null()) == 0 {
                return None;
            }
        }
        if GdiFlush() == 0 {
            return None;
        }
        let pixels = std::slice::from_raw_parts(bits.cast::<u32>(), (w * h * 96) as usize);
        let mut atlas = Atlas([[0; 6]; 128]);
        for glyph in 32..128 {
            let i = glyph - 32;
            let left = i % 16 * w as usize;
            let top = i / 16 * h as usize;
            for y in 0..3 {
                for x in 0..2 {
                    let mut sum = 0;
                    let mut count = 0;
                    for py in y * h as usize / 3..(y + 1) * h as usize / 3 {
                        for px in x * w as usize / 2..(x + 1) * w as usize / 2 {
                            let p = pixels[(top + py) * (w as usize * 16) + left + px];
                            sum += ((p & 255) + ((p >> 8) & 255) + ((p >> 16) & 255)) / 3;
                            count += 1;
                        }
                    }
                    atlas.0[glyph][y * 2 + x] = ((sum / count.max(1)) * 3).min(255) as u8;
                }
            }
        }
        Some(Arc::new(atlas))
    })();
    SelectObject(dc, old_font);
    SelectObject(dc, old_bitmap);
    DeleteObject(bitmap);
    DeleteDC(dc);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::windows::dpi::{self, Dpi};

    #[test]
    fn minimap_coverage_uses_the_selected_editor_font() {
        unsafe {
            let dc = CreateCompatibleDC(null_mut());
            assert!(!dc.is_null());
            let mut atlases = Vec::new();
            for name in ["Consolas", "Courier New"] {
                let face = Face::from_name(name).unwrap();
                let font = dpi::editor_font(Dpi::BASE, 20, 100, face);
                assert!(!font.is_null());
                let old = SelectObject(dc, font);
                let mut metric: TextMetric = std::mem::zeroed();
                assert_ne!(GetTextMetricsW(dc, &mut metric), 0);
                SelectObject(dc, old);
                let atlas = build(FontSpec {
                    font,
                    width: metric.average + 1,
                    height: metric.height + metric.external,
                    face,
                    size: 20,
                    zoom: 100,
                })
                .unwrap();
                assert!(atlas.0[usize::from(b' ')].iter().all(|&a| a == 0));
                assert!(atlas.0[usize::from(b'M')].iter().any(|&a| a > 0));
                assert_ne!(atlas.0[usize::from(b'.')], atlas.0[usize::from(b'M')]);
                atlases.push(atlas);
                // The coverage copy remains valid after the owner releases its font.
                assert_ne!(DeleteObject(font), 0);
            }
            assert_ne!(atlases[0].0, atlases[1].0);
            DeleteDC(dc);
        }
    }
}
