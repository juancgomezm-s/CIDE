//! Bounded, retained minimap raster. Coverage masks are independent of theme.
use super::{appearance::Theme, paint, scale, MapRow, Track};
use crate::windows::{
    dpi::Dpi,
    ffi::{BitmapHeader, BitmapInfo, Hdc, Rect, RectVisible, SetDIBitsToDevice},
};
use jc_app::SyntaxKind;
use std::sync::Arc;

const MAX_PIXELS: usize = 2 * 1024 * 1024;

// Three rows of two coverage samples. At 96 DPI the two samples are averaged
// into one column, preserving the source column count and indentation.
fn glyph_mask(glyph: u8) -> [u8; 3] {
    match glyph {
        b'.' | b',' => [0, 0, 1],
        b':' | b';' => [1, 0, 1],
        b'-' | b'~' => [0, 3, 0],
        b'_' => [0, 0, 3],
        b'=' => [3, 0, 3],
        b'/' | b'<' => [2, 3, 1],
        b'\\' | b'>' => [1, 3, 2],
        b'[' | b'(' | b'{' => [3, 1, 3],
        b']' | b')' | b'}' => [3, 2, 3],
        b'|' | b'l' | b'I' => [1, 1, 1],
        b'i' | b'!' => [1, 1, 0],
        b'\'' | b'"' | b'`' => [3, 0, 0],
        b'+' | b'*' | b'#' => [1, 3, 1],
        b'o' | b'a' | b'e' | b's' | b'c' => [0, 3, 3],
        b'g' | b'p' | b'q' | b'y' | b'j' => [0, 3, 2],
        b'h' | b'b' | b'd' | b'k' | b't' | b'f' => [1, 3, 2],
        b'm' | b'n' | b'r' | b'u' | b'v' | b'w' | b'x' | b'z' => [0, 3, 1],
        b'A'..=b'Z' | b'0'..=b'9' => [3, 3, 2],
        _ => [2, 1, 3],
    }
}

fn color(kind: SyntaxKind, theme: Theme) -> u32 {
    if kind == SyntaxKind::Plain {
        theme.muted()
    } else {
        theme.syntax(kind)
    }
}

fn dib(color: u32) -> u32 {
    ((color & 0xff) << 16) | (color & 0xff00) | (color >> 16)
}

fn mix(background: u32, foreground: u32, coverage: u32) -> u32 {
    let mut result = 0;
    for shift in [0, 8, 16] {
        let bg = (background >> shift) & 255;
        let fg = (foreground >> shift) & 255;
        result |= ((bg * (4 - coverage) + fg * coverage + 2) / 4) << shift;
    }
    result
}

#[derive(Clone)]
pub(super) struct Frame {
    channel: Rect,
    pub(super) track: Track,
    pub(super) samples: Arc<[MapRow]>,
    raster: Option<Arc<[u32]>>,
}

#[derive(Default)]
pub(super) struct Cache {
    previous: Option<(Dpi, Theme, Frame)>,
}

impl Cache {
    pub(super) fn prepare(
        &mut self,
        channel: Rect,
        track: Track,
        samples: Arc<[MapRow]>,
        dpi: Dpi,
        theme: Theme,
    ) -> Frame {
        let thumb = track.thumb_rect();
        let raster = self
            .previous
            .as_ref()
            .filter(|(old_dpi, old_theme, old)| {
                *old_dpi == dpi
                    && *old_theme == theme
                    && Arc::ptr_eq(&old.samples, &samples)
                    && bounds(old.channel) == bounds(channel)
                    && bounds(old.track.rect) == bounds(track.rect)
                    && bounds(old.track.thumb_rect()) == bounds(thumb)
            })
            .map_or_else(
                || rasterize(channel, track, &samples, dpi, theme).map(Arc::from),
                |(_, _, old)| old.raster.clone(),
            );
        let frame = Frame {
            channel,
            track,
            samples,
            raster,
        };
        self.previous = Some((dpi, theme, frame.clone()));
        frame
    }
}

fn bounds(rect: Rect) -> [i32; 4] {
    [rect.left, rect.top, rect.right, rect.bottom]
}

fn rows(
    channel: Rect,
    track: Track,
    samples: &[MapRow],
    dpi: Dpi,
    mut draw: impl FnMut(Rect, i32, &MapRow),
) {
    let edge = dpi.pixels(1);
    let left = channel.left + dpi.pixels(5);
    let right = channel.right - dpi.pixels(4);
    for (index, row) in samples.iter().enumerate() {
        let top = track.rect.top + scale(index, samples.len(), track.length());
        let next = track.rect.top + scale(index + 1, samples.len(), track.length());
        let bottom = (next - edge).max(top + 1).min(track.rect.bottom);
        if top < bottom && left < right {
            draw(
                Rect {
                    left,
                    right,
                    top,
                    bottom,
                },
                next - top,
                row,
            );
        }
    }
}

fn rasterize(
    channel: Rect,
    track: Track,
    samples: &[MapRow],
    dpi: Dpi,
    theme: Theme,
) -> Option<Vec<u32>> {
    let width = usize::try_from(channel.right - channel.left).ok()?;
    let height = usize::try_from(channel.bottom - channel.top).ok()?;
    let size = width.checked_mul(height)?;
    if size == 0 || size > MAX_PIXELS {
        return None;
    }
    let background = dib(theme.map_background());
    let viewport = dib(theme.map_viewport());
    let mut pixels = vec![background; size];
    let thumb = track.thumb_rect();
    let top = (thumb.top - channel.top).clamp(0, height as i32) as usize;
    let bottom = (thumb.bottom - channel.top).clamp(0, height as i32) as usize;
    pixels[top * width..bottom * width].fill(viewport);
    rows(channel, track, samples, dpi, |rect, pitch, row| {
        for mark in &row.marks {
            let fg = dib(color(mark.kind, theme));
            if pitch < 2 {
                let x = rect.left + dpi.pixels(i32::from(mark.start));
                let end = rect.left + dpi.pixels(i32::from(mark.end));
                for y in rect.top..rect.bottom {
                    for px in x..end.max(x + 1).min(rect.right) {
                        pixels[(y - channel.top) as usize * width + (px - channel.left) as usize] =
                            fg;
                    }
                }
                continue;
            }
            let glyph_bottom = (rect.top + (pitch - 1).max(2)).min(channel.bottom);
            let draw_height = glyph_bottom - rect.top;
            for col in mark.start..mark.end {
                let x = rect.left + dpi.pixels(i32::from(col));
                let end = rect.left + dpi.pixels(i32::from(col) + 1);
                let draw_width = end - x;
                let mask = glyph_mask(row.glyphs[usize::from(col)]);
                for y in rect.top..glyph_bottom {
                    let pattern = if draw_height == 2 && y == rect.top {
                        mask[0] | mask[1]
                    } else {
                        mask[if draw_height == 2 {
                            2
                        } else {
                            ((y - rect.top) * 3 / draw_height) as usize
                        }]
                    };
                    let bg = if y >= thumb.top && y < thumb.bottom {
                        viewport
                    } else {
                        background
                    };
                    for px in x..end.min(rect.right) {
                        let coverage = if draw_width == 1 {
                            pattern.count_ones()
                        } else {
                            u32::from((pattern >> ((px - x) * 2 / draw_width)) & 1) * 2
                        };
                        if coverage > 0 {
                            pixels[(y - channel.top) as usize * width
                                + (px - channel.left) as usize] = mix(bg, fg, coverage + 1);
                        }
                    }
                }
            }
        }
    });
    Some(pixels)
}

impl Frame {
    pub(super) unsafe fn draw(&self, dc: Hdc, dpi: Dpi, theme: Theme) -> Result<(), String> {
        if RectVisible(dc, &self.channel) == 0 {
            return Ok(());
        }
        let window = self.track.thumb_rect();
        if let Some(pixels) = &self.raster {
            let width = self.channel.right - self.channel.left;
            let height = self.channel.bottom - self.channel.top;
            let info = BitmapInfo {
                header: BitmapHeader {
                    size: 40,
                    width,
                    height: -height,
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
            if SetDIBitsToDevice(
                dc,
                self.channel.left,
                self.channel.top,
                width as u32,
                height as u32,
                0,
                0,
                0,
                height as u32,
                pixels.as_ptr().cast(),
                &info,
                0,
            ) == 0
            {
                return Err(crate::windows::os_error("Dibujar el minimapa"));
            }
        } else {
            paint::fill(dc, self.channel, theme.map_background())?;
            paint::fill(dc, window, theme.map_viewport())?;
            let mut marks = Vec::new();
            rows(
                self.channel,
                self.track,
                &self.samples,
                dpi,
                |rect, _, row| {
                    marks.extend(row.marks.iter().map(|mark| {
                        let left = rect.left + dpi.pixels(i32::from(mark.start));
                        let right = rect.left + dpi.pixels(i32::from(mark.end));
                        (
                            Rect {
                                left,
                                right: right.max(left + 1).min(rect.right),
                                ..rect
                            },
                            color(mark.kind, theme),
                        )
                    }));
                },
            );
            paint::fill_color_marks(dc, marks.into_iter())?;
        }
        let edge = dpi.pixels(1);
        for border in [
            Rect {
                bottom: (window.top + edge).min(window.bottom),
                ..window
            },
            Rect {
                top: (window.bottom - edge).max(window.top),
                ..window
            },
            Rect {
                right: window.left + edge,
                ..window
            },
        ] {
            paint::fill(dc, border, theme.accent())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::windows::ffi::*;
    use std::{io::Write, ptr::null_mut};

    #[test]
    fn minimap_microglyphs_match_geometry_and_preserve_punctuation() {
        let source = include_str!("../../../../jc-core/src/document.rs");
        let lines: Vec<_> = source.lines().collect();
        let channel = Rect {
            left: 0,
            top: 0,
            right: 160,
            bottom: 648,
        };
        let dpi = Dpi::BASE;
        let count = (channel.bottom / 2) as usize;
        let samples: Arc<[_]> = (0..count)
            .map(|index| {
                let line = lines[super::super::sample_position(index, count, lines.len() - 1)];
                super::super::row_marks(
                    &line.as_bytes()[..line.len().min(super::super::PREFIX_BYTES)],
                    line.len() <= super::super::PREFIX_BYTES,
                    jc_app::SyntaxProfile::Rust,
                    Some(jc_text::TextEncoding::Utf8),
                )
            })
            .collect();
        let track = Track {
            axis: super::super::Axis::Vertical,
            rect: channel,
            offset: 0,
            limit: lines.len() - 27,
            page: 27,
            minimum_thumb: 12,
        };
        let theme = Theme::Current;
        let mut cache = Cache::default();
        let micro = cache.prepare(channel, track, Arc::clone(&samples), dpi, theme);
        let same = cache.prepare(channel, track, Arc::clone(&samples), dpi, theme);
        assert!(Arc::ptr_eq(
            micro.raster.as_ref().unwrap(),
            same.raster.as_ref().unwrap()
        ));
        let blocks = Frame {
            raster: None,
            ..micro.clone()
        };
        let (old_pixels, new_pixels) =
            unsafe { (render(&blocks, dpi, theme), render(&micro, dpi, theme)) };
        assert_eq!(old_pixels.len(), new_pixels.len());
        assert_eq!(old_pixels[0] & 0xffffff, new_pixels[0] & 0xffffff);
        assert_ne!(old_pixels, new_pixels);
        assert_ne!(glyph_mask(b'.'), glyph_mask(b'a'));
        let expected = micro.raster.as_ref().unwrap();
        let start = 100 * 160 + 15;
        assert!(new_pixels[start..start + 120]
            .iter()
            .zip(&expected[start..start + 120])
            .all(|(a, b)| a & 0xffffff == b & 0xffffff));
        assert!(new_pixels.iter().zip(&old_pixels).any(|(new, old)| {
            new & 0xffffff != old & 0xffffff
                && new & 0xffffff != dib(theme.map_background())
                && new & 0xffffff != dib(theme.map_viewport())
        }));
        if let Some(directory) = std::env::var_os("CIDE_MINIMAP_COMPARISON") {
            for (name, pixels) in [("blocks.ppm", old_pixels), ("microglyphs.ppm", new_pixels)] {
                let mut file =
                    std::fs::File::create(std::path::Path::new(&directory).join(name)).unwrap();
                writeln!(file, "P6\n160 648\n255").unwrap();
                let bytes: Vec<_> = pixels
                    .iter()
                    .flat_map(|p| [(*p >> 16) as u8, (*p >> 8) as u8, *p as u8])
                    .collect();
                file.write_all(&bytes).unwrap();
            }
        }
    }

    unsafe fn render(frame: &Frame, dpi: Dpi, theme: Theme) -> Vec<u32> {
        let dc = CreateCompatibleDC(null_mut());
        assert!(!dc.is_null());
        let info = BitmapInfo {
            header: BitmapHeader {
                size: 40,
                width: 160,
                height: -648,
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
        assert!(!bitmap.is_null() && !bits.is_null());
        let old = SelectObject(dc, bitmap);
        let result = frame.draw(dc, dpi, theme);
        assert_ne!(GdiFlush(), 0);
        let pixels = std::slice::from_raw_parts(bits.cast::<u32>(), 160 * 648).to_vec();
        SelectObject(dc, old);
        DeleteObject(bitmap);
        DeleteDC(dc);
        result.unwrap();
        pixels
    }
}
