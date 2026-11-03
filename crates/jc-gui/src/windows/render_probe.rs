//! Explicit own-HWND smoke measurement, never an idle/background benchmark.
//! Both paths draw the same owned frame on memory DIBs; this does not measure
//! input-to-physical-display latency or certify global process memory targets.
use super::{back_buffer::Buffer, ffi::*, owner, paint};
use std::{ptr::null_mut, time::Instant};

#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentProcess() -> Handle;
}
#[link(name = "user32")]
extern "system" {
    fn GetGuiResources(process: Handle, flags: u32) -> u32;
}

const MAX_DIMENSION: i32 = 2000;
const SAMPLES: usize = 21;

struct Surface {
    hwnd: Hwnd,
    window_dc: Hdc,
    dc: Hdc,
    bitmap: Handle,
    previous: Handle,
    bits: *mut u32,
    pixels: usize,
}
impl Surface {
    unsafe fn new(hwnd: Hwnd, width: i32, height: i32) -> Result<Self, String> {
        if !(2..=MAX_DIMENSION).contains(&width) || !(2..=MAX_DIMENSION).contains(&height) {
            return Err("El probe GUI requiere dimensiones de 2–2000 píxeles".into());
        }
        let window_dc = GetDC(hwnd);
        if window_dc.is_null() {
            return Err("El probe GUI no obtuvo su contexto de ventana".into());
        }
        let mut surface = Self {
            hwnd,
            window_dc,
            dc: null_mut(),
            bitmap: null_mut(),
            previous: null_mut(),
            bits: null_mut(),
            pixels: 0,
        };
        surface.dc = CreateCompatibleDC(window_dc);
        if surface.dc.is_null() {
            return Err("El probe GUI no creó su contexto de memoria".into());
        }
        let bytes = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4))
            .filter(|bytes| *bytes <= 32 * 1024 * 1024)
            .ok_or("El probe GUI excede 32 MiB de superficie")?;
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
        surface.bitmap = CreateDIBSection(window_dc, &info, 0, &mut bits, null_mut(), 0);
        if surface.bitmap.is_null() || bits.is_null() {
            return Err("El probe GUI no reservó su DIB propio".into());
        }
        surface.bits = bits.cast();
        surface.pixels = bytes / 4;
        std::ptr::write_bytes(surface.bits, 0, surface.pixels);
        surface.previous = SelectObject(surface.dc, surface.bitmap);
        if surface.previous.is_null() || surface.previous as isize == -1 {
            return Err("El probe GUI no seleccionó su DIB propio".into());
        }
        Ok(surface)
    }
}
impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            if !self.dc.is_null() {
                if !self.previous.is_null() && self.previous as isize != -1 {
                    SelectObject(self.dc, self.previous);
                }
                DeleteDC(self.dc);
            }
            if !self.bitmap.is_null() {
                DeleteObject(self.bitmap);
            }
            if !self.window_dc.is_null() {
                ReleaseDC(self.hwnd, self.window_dc);
            }
        }
    }
}

fn percentiles(samples: &mut [u128]) -> (u128, u128) {
    samples.sort_unstable();
    (
        samples[samples.len() / 2],
        samples[(samples.len() * 95).div_ceil(100) - 1],
    )
}

unsafe fn timed(dc: Hdc, frame: &paint::Frame, reference: bool) -> Result<u128, String> {
    if GdiFlush() == 0 {
        return Err("El probe GUI no pudo sincronizar el lote GDI previo".into());
    }
    let start = Instant::now();
    if reference {
        paint::draw_reference_cells(dc, frame)?;
    } else {
        paint::draw(dc, frame)?;
    }
    if GdiFlush() == 0 {
        return Err("El probe GUI no pudo sincronizar la composición GDI".into());
    }
    Ok(start.elapsed().as_nanos())
}

/// Requires the application's explicit smoke HWND. It creates only owned memory
/// surfaces and returns small numeric JSON; no file, source or clipboard access.
pub(super) unsafe fn measure(hwnd: Hwnd, frame: &paint::Frame) -> Result<String, String> {
    if !owner(hwnd).is_some_and(|owner| owner.state.borrow().smoke) {
        return Err("El probe GUI solo admite el HWND propio del smoke".into());
    }
    let resources_before = GetGuiResources(GetCurrentProcess(), 0);
    let mut reference = [0u128; SAMPLES];
    let mut current = [0u128; SAMPLES];
    let retained;
    let resized;
    {
        let surface = Surface::new(hwnd, frame.width, frame.height)?;
        for warmup in 0..3 {
            timed(surface.dc, frame, warmup % 2 == 0)?;
            timed(surface.dc, frame, warmup % 2 != 0)?;
        }
        for sample in 0..SAMPLES {
            if sample % 2 == 0 {
                reference[sample] = timed(surface.dc, frame, true)?;
                current[sample] = timed(surface.dc, frame, false)?;
            } else {
                current[sample] = timed(surface.dc, frame, false)?;
                reference[sample] = timed(surface.dc, frame, true)?;
            }
        }
        // Compare the exact same owned frame, including cluster fallback paths,
        // before testing the retained buffer. The bounded temporary copy is
        // dropped before another DIB can be allocated and is outside timings.
        timed(surface.dc, frame, true)?;
        let expected = std::slice::from_raw_parts(surface.bits, surface.pixels).to_vec();
        timed(surface.dc, frame, false)?;
        let actual = std::slice::from_raw_parts(surface.bits, surface.pixels);
        if actual != expected.as_slice() {
            let first = actual
                .iter()
                .zip(&expected)
                .position(|(a, b)| a != b)
                .unwrap_or(0);
            return Err(format!(
                "Render por tramos difiere del render por celdas en el píxel {first}"
            ));
        }
        drop(expected);
        if frame.width < 700 || frame.height < super::chrome::TOP {
            return Err("La prueba del marco requiere una superficie completa".into());
        }
        for y in [16usize, 46] {
            let pixel = actual[y * frame.width as usize + frame.width as usize / 2] & 0x00ff_ffff;
            if pixel != 0x002e_3440 {
                return Err(format!(
                    "El marco no comparte el fondo #2E3440 en y={y}: {pixel:x}"
                ));
            }
        }
        let mut buffer = Buffer::default();
        buffer.paint(surface.dc, frame.width, frame.height, |dc| {
            paint::draw(dc, frame)
        })?;
        buffer.paint(surface.dc, frame.width, frame.height, |dc| {
            paint::draw(dc, frame)
        })?;
        retained = buffer.statistics();
        if retained.allocations != 1 || retained.compositions != 2 || retained.blits != 2 {
            return Err("La superficie GUI no reutilizó su reserva entre composiciones".into());
        }
        buffer.paint(surface.dc, frame.width - 1, frame.height, |dc| {
            paint::draw(dc, frame)
        })?;
        resized = buffer.statistics();
        if resized.allocations != 2
            || resized.compositions != 3
            || resized.blits != 3
            || resized.allocated_bytes != (frame.width as usize - 1) * frame.height as usize * 4
        {
            return Err("La superficie GUI no reemplazó su reserva al redimensionar".into());
        }
        GdiFlush();
    }
    let resources_after = GetGuiResources(GetCurrentProcess(), 0);
    let (reference_p50, reference_p95) = percentiles(&mut reference);
    let (current_p50, current_p95) = percentiles(&mut current);
    let (source_cells, source_runs) = frame.source_draw_counts();
    Ok(format!(
        concat!(
            "{{\"samples_per_route\":21,\"warmup_pairs\":3,",
            "\"width\":{},\"height\":{},",
            "\"source_cells\":{},\"source_runs\":{},\"exact_pixels_equal\":true,\"client_title_and_toolbar_rgb\":\"#2E3440\",",
            "\"reference_cell_ns\":{{\"p50\":{},\"p95\":{}}},",
            "\"current_run_ns\":{{\"p50\":{},\"p95\":{}}},",
            "\"retained\":{{\"allocations\":{},\"compositions\":{},\"blits\":{},\"pixel_bytes\":{}}},",
            "\"resized\":{{\"allocations\":{},\"compositions\":{},\"blits\":{},\"pixel_bytes\":{}}},",
            "\"gdi_handles_before\":{},\"gdi_handles_after_drop\":{},",
            "\"physical_display_latency_verified\":false}}"
        ),
        frame.width,
        frame.height,
        source_cells,
        source_runs,
        reference_p50,
        reference_p95,
        current_p50,
        current_p95,
        retained.allocations,
        retained.compositions,
        retained.blits,
        retained.allocated_bytes,
        resized.allocations,
        resized.compositions,
        resized.blits,
        resized.allocated_bytes,
        resources_before,
        resources_after,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timing_percentiles_use_interleaved_sample_population() {
        let mut samples: Vec<_> = (1..=21u128).rev().collect();
        assert_eq!(percentiles(&mut samples), (11, 20));
    }
}
