//! Hidden native composition at large client sizes, with separately charged probe DIBs.
use super::{cpu, memory, samples, GetCurrentProcess, GetPixel, IsWindowVisible, Surface};
use crate::windows::{dpi, ffi::*, interaction_smoke::Resize, owner, paint, shell};
use crate::SmokeOptions;
use std::{fs, io::Write, time::Instant};

const DIB_LIMIT: usize = 32 * 1024 * 1024;
const SENTINEL: u32 = 0x00ff00ff;
const TILE: i32 = 64;
const HOT_SAMPLES: usize = 21;

#[link(name = "user32")]
extern "system" {
    fn GetWindowRect(hwnd: Hwnd, rect: *mut Rect) -> i32;
    fn GetGuiResources(process: Handle, flags: u32) -> u32;
}

struct Geometry {
    label: &'static str,
    size: (i32, i32),
    dpi: usize,
    full_frames: usize,
}

// The host monitor's tracking maximum would otherwise clamp SetWindowPos.
// Widen only the test window's tracking range and keep its real WM_SIZE path.
unsafe extern "system" fn geometry_procedure(
    hwnd: Hwnd,
    message: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    let result = crate::windows::procedure(hwnd, message, wparam, lparam);
    if message == 0x0024 && lparam != 0 {
        #[repr(C)]
        struct MinMaxInfo {
            reserved: Point,
            max_size: Point,
            max_position: Point,
            min_track_size: Point,
            max_track_size: Point,
        }
        (*(lparam as *mut MinMaxInfo)).max_track_size = Point { x: 8192, y: 8192 };
    }
    result
}

struct TrackingLimit {
    hwnd: Hwnd,
    previous: isize,
}
impl TrackingLimit {
    unsafe fn install(hwnd: Hwnd) -> Result<Self, String> {
        let previous = SetWindowLongPtrW(hwnd, -4, geometry_procedure as *const () as isize);
        if previous == 0 {
            return Err("No se pudo ampliar el límite de tamaño de la ventana de prueba".into());
        }
        Ok(Self { hwnd, previous })
    }
}
impl Drop for TrackingLimit {
    fn drop(&mut self) {
        unsafe { SetWindowLongPtrW(self.hwnd, -4, self.previous) };
    }
}

unsafe fn set_dpi(hwnd: Hwnd, dpi: usize) -> Result<(), String> {
    let mut rect = Rect::default();
    if GetWindowRect(hwnd, &mut rect) == 0 {
        return Err("No se pudo obtener el rectángulo de la ventana propia".into());
    }
    SendMessageW(
        hwnd,
        dpi::CHANGED,
        dpi | (dpi << 16),
        &rect as *const Rect as isize,
    );
    Ok(())
}

struct Print {
    elapsed_ms: f64,
    compositions: usize,
    blits: usize,
    errors: usize,
    allocated_bytes: usize,
    allocations: usize,
    frame: (i32, i32),
}

unsafe fn print(hwnd: Hwnd, surface: &Surface) -> Result<Print, String> {
    let owner = owner(hwnd).ok_or("Se perdió el HWND de medición")?;
    if IsWindowVisible(hwnd) != 0 {
        return Err("El HWND de medición dejó de estar oculto".into());
    }
    paint::fill(
        surface.dc,
        Rect {
            left: 0,
            top: 0,
            right: surface.width,
            bottom: surface.height,
        },
        SENTINEL,
    )?;
    if GdiFlush() == 0 {
        return Err("No se pudo sincronizar el centinela del DIB".into());
    }
    let before = owner.buffer.borrow().statistics();
    let errors_before = owner.state.borrow().paint_errors;
    owner.frame_dirty.set(true);
    let start = Instant::now();
    SendMessageW(hwnd, 0x0318, surface.dc as usize, 4);
    if GdiFlush() == 0 {
        return Err("No se pudo sincronizar la composición geométrica".into());
    }
    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
    let after = owner.buffer.borrow().statistics();
    let errors = owner
        .state
        .borrow()
        .paint_errors
        .saturating_sub(errors_before);
    let frame = owner
        .frame
        .borrow()
        .as_ref()
        .map(|frame| (frame.width, frame.height))
        .ok_or("No se preparó una escena para la geometría")?;
    Ok(Print {
        elapsed_ms,
        compositions: after.compositions.saturating_sub(before.compositions),
        blits: after.blits.saturating_sub(before.blits),
        errors,
        allocated_bytes: after.allocated_bytes,
        allocations: after.allocations.saturating_sub(before.allocations),
        frame,
    })
}

impl Print {
    fn passes(&self, size: (i32, i32), bands: usize) -> bool {
        self.frame == size
            && self.compositions == bands
            && self.blits == bands
            && self.errors == 0
            && (1..=DIB_LIMIT).contains(&self.allocated_bytes)
            && self.allocations <= 1
    }

    fn json(&self) -> String {
        let Self {
            elapsed_ms,
            compositions,
            blits,
            errors,
            allocated_bytes,
            allocations,
            frame: (width, height),
        } = self;
        format!("{{\"elapsed_ms\":{elapsed_ms:.6},\"compositions\":{compositions},\"blits\":{blits},\"paint_errors\":{errors},\"editor_dib_bytes\":{allocated_bytes},\"allocations\":{allocations},\"frame_pixels\":[{width},{height}]}}")
    }
}

fn pixels_drawn(pixels: &[u32]) -> bool {
    pixels
        .iter()
        .all(|pixel| !matches!(*pixel, SENTINEL | u32::MAX))
}

unsafe fn measure_case(
    hwnd: Hwnd,
    resize: &Resize,
    tile: &Surface,
    case: &Geometry,
) -> Result<(String, bool, (i32, i32)), String> {
    let owner = owner(hwnd).ok_or("Se perdió el HWND de medición")?;
    let (width, height) = case.size;
    let mut phases = vec![memory("before_resize")?];
    let cpu_before = cpu()?;
    resize.set(width, height)?;
    set_dpi(hwnd, case.dpi)?;
    let mut rect = Rect::default();
    if GetClientRect(hwnd, &mut rect) == 0 {
        return Err("No se pudo leer el cliente redimensionado".into());
    }
    let (dpi, cells, state_size) = {
        let state = owner.state.borrow();
        (
            state.dpi.pixels(96),
            (state.cell_width, state.cell_height),
            (state.width, state.height),
        )
    };
    let actual = (rect.right - rect.left, rect.bottom - rect.top);
    let exact = actual == case.size && state_size == case.size && dpi as usize == case.dpi;
    let mut ok = exact;
    phases.push(memory("after_resize_before_composition")?);
    let clipped = print(hwnd, tile)?;
    let tile_pixel = GetPixel(tile.dc, TILE / 2, TILE / 2);
    ok &= clipped.passes(case.size, 1) && pixels_drawn(&[tile_pixel]);
    phases.push(memory("after_clipped_composition")?);

    let expected_bands = (height as usize).div_ceil(DIB_LIMIT / (width as usize * 4));
    let mut full_bytes = 0;
    let mut full_prints = Vec::new();
    let mut times = Vec::new();
    let mut pixel_rows = Vec::new();
    let mut full_pixels = Vec::new();
    if exact && case.full_frames != 0 {
        let full = Surface::new(width, height)?;
        full_bytes = width as usize * height as usize * 4;
        phases.push(memory("full_probe_allocated")?);
        pixel_rows.extend([100, height - 10, height - 1]);
        if expected_bands > 1 {
            let seam = (DIB_LIMIT / (width as usize * 4)) as i32;
            pixel_rows.extend([seam - 1, seam]);
        }
        pixel_rows.sort_unstable();
        pixel_rows.dedup();
        for sample in 0..case.full_frames {
            let frame = print(hwnd, &full)?;
            let colors: Vec<_> = pixel_rows
                .iter()
                .map(|&y| GetPixel(full.dc, width / 2, y))
                .collect();
            let status_pixel = GetPixel(full.dc, width / 2, height - 1);
            let passed = frame.passes(case.size, expected_bands)
                && pixels_drawn(&colors)
                && status_pixel == shell::SURFACE;
            ok &= passed;
            if sample == 0 || !passed {
                full_prints.push(frame.json());
                full_pixels = colors;
            }
            if !passed {
                break;
            }
            if sample > 0 {
                times.push(frame.elapsed_ms);
            }
        }
        phases.push(memory("after_full_composition")?);
        drop(full);
        phases.push(memory("after_full_probe_released")?);
    }
    let cpu_ms = cpu()?.saturating_sub(cpu_before) as f64 / 10_000.0;
    let (actual_width, actual_height) = actual;
    let (state_width, state_height) = state_size;
    let (cell_width, cell_height) = cells;
    let label = case.label;
    let join = |values: &[i32]| {
        values
            .iter()
            .map(i32::to_string)
            .collect::<Vec<_>>()
            .join(",")
    };
    let colors = full_pixels
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let json = format!(
        "{{\"case\":\"{label}\",\"ok\":{ok},\"requested_pixels\":[{width},{height}],\"client_pixels\":[{actual_width},{actual_height}],\"state_pixels\":[{state_width},{state_height}],\"dpi\":{dpi},\"cell_pixels\":[{cell_width},{cell_height}],\"cpu_ms\":{cpu_ms:.3},\"clipped\":{},\"tile_pixel\":{tile_pixel},\"expected_full_bands\":{expected_bands},\"full_probe_dib_bytes\":{full_bytes},\"full_prints\":[{}],\"hot_full_samples_ms\":[{}],\"full_pixel_rows\":[{}],\"full_pixels\":[{colors}],\"memory\":[{}]}}",
        clipped.json(), full_prints.join(","), samples(&times), join(&pixel_rows), phases.join(","),
    );
    Ok((json, ok, cells))
}

fn json_error(error: &str) -> String {
    error
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\r', "\\r")
        .replace('\n', "\\n")
        .replace('\t', "\\t")
}

pub(super) unsafe fn run(hwnd: Hwnd, options: &SmokeOptions) -> Result<(), String> {
    let _tracking_limit = TrackingLimit::install(hwnd)?;
    let owner = owner(hwnd).ok_or("Se perdió el HWND de medición")?;
    let (identity, original_dpi) = {
        let state = owner.state.borrow();
        let active = state
            .workbench
            .active_document()
            .ok_or("Falta el documento de prueba")?;
        (
            (
                active.id,
                active.session.document().revision(),
                active.session.document().len(),
                state.workbench.active_position(),
            ),
            state.dpi.pixels(96) as usize,
        )
    };
    let mut report = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&options.report)
        .map_err(|error| error.to_string())?;
    let before = memory("before_probe_dib")?;
    let gdi_before = GetGuiResources(GetCurrentProcess(), 0);
    let resize = Resize::new(hwnd)?;
    let tile = Surface::new(TILE, TILE)?;
    let mut rows = Vec::new();
    let mut failures = Vec::new();
    let mut dpi_cells = Vec::new();
    let cases = [
        Geometry {
            label: "base",
            size: (1000, 650),
            dpi: 96,
            full_frames: 1,
        },
        Geometry {
            label: "4k",
            size: (3840, 2160),
            dpi: 96,
            full_frames: HOT_SAMPLES + 1,
        },
        Geometry {
            label: "exact_limit",
            size: (4096, 2048),
            dpi: 96,
            full_frames: 1,
        },
        Geometry {
            label: "one_row_over",
            size: (4096, 2049),
            dpi: 96,
            full_frames: 1,
        },
        Geometry {
            label: "ultrawide",
            size: (5120, 2160),
            dpi: 96,
            full_frames: HOT_SAMPLES + 1,
        },
        Geometry {
            label: "4k_dpi_144",
            size: (3840, 2160),
            dpi: 144,
            full_frames: 0,
        },
        Geometry {
            label: "4k_dpi_192",
            size: (3840, 2160),
            dpi: 192,
            full_frames: 0,
        },
        Geometry {
            label: "back_to_base",
            size: (1000, 650),
            dpi: original_dpi,
            full_frames: 1,
        },
    ];
    for case in cases {
        match measure_case(hwnd, &resize, &tile, &case) {
            Ok((row, ok, cells)) => {
                rows.push(row);
                if matches!(case.label, "4k" | "4k_dpi_144" | "4k_dpi_192") {
                    dpi_cells.push(cells);
                }
                if !ok {
                    failures.push(case.label.to_owned());
                }
            }
            Err(error) => {
                rows.push(format!(
                    "{{\"case\":\"{}\",\"ok\":false,\"error\":\"{}\"}}",
                    case.label,
                    json_error(&error)
                ));
                failures.push(case.label.to_owned());
            }
        }
    }
    let dpi_cells_scaled = dpi_cells.len() == 3
        && dpi_cells
            .windows(2)
            .all(|pair| pair[0].0 < pair[1].0 && pair[0].1 < pair[1].1);
    if !dpi_cells_scaled {
        failures.push("dpi_cells_scaled".into());
    }
    let source_preserved = {
        let state = owner.state.borrow();
        state.workbench.active_document().is_some_and(|active| {
            identity
                == (
                    active.id,
                    active.session.document().revision(),
                    active.session.document().len(),
                    state.workbench.active_position(),
                )
        })
    };
    if !source_preserved {
        failures.push("source_identity".into());
    }
    drop(tile);
    let after = memory("after_probe_dib_released")?;
    let gdi_after = GetGuiResources(GetCurrentProcess(), 0);
    let ok = failures.is_empty();
    writeln!(report,
        "{{\"ok\":{ok},\"desktop_used\":false,\"physical_display_verified\":false,\"synthetic_dpi\":true,\"tile_dib_bytes\":{},\"gdi_objects_before\":{gdi_before},\"gdi_objects_after\":{gdi_after},\"dpi_cells_scaled\":{dpi_cells_scaled},\"source_preserved\":{source_preserved},\"memory\":[{before},{after}],\"cases\":[{}]}}",
        TILE * TILE * 4, rows.join(","),
    ).map_err(|error| error.to_string())?;
    report.sync_all().map_err(|error| error.to_string())?;
    if ok {
        Ok(())
    } else {
        Err(format!(
            "Falló la composición geométrica en {}",
            failures.join(", ")
        ))
    }
}
