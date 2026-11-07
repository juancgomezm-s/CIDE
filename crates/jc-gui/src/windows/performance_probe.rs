//! Explicit complete-editor measurement on the existing owned hidden smoke HWND.
//! Source files are never changed. The extra probe DIB is reported, not subtracted.
use super::{ffi::*, owner, smoke};
use crate::SmokeOptions;
use jc_app::CommandId;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    ptr::null_mut,
    time::{Duration, Instant},
};

mod geometry;
mod services;

#[repr(C)]
#[derive(Default)]
struct Memory {
    size: u32,
    faults: u32,
    peak_working: usize,
    working: usize,
    peak_paged: usize,
    paged: usize,
    peak_nonpaged: usize,
    nonpaged: usize,
    pagefile: usize,
    peak_pagefile: usize,
    private: usize,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FileTime {
    low: u32,
    high: u32,
}
#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentProcess() -> Handle;
    fn GetProcessTimes(
        process: Handle,
        create: *mut FileTime,
        exit: *mut FileTime,
        kernel: *mut FileTime,
        user: *mut FileTime,
    ) -> i32;
}
#[link(name = "psapi")]
extern "system" {
    fn GetProcessMemoryInfo(process: Handle, counters: *mut Memory, size: u32) -> i32;
}
#[link(name = "gdi32")]
extern "system" {
    fn GetPixel(dc: Hdc, x: i32, y: i32) -> u32;
}
#[link(name = "user32")]
extern "system" {
    fn IsWindowVisible(hwnd: Hwnd) -> i32;
    fn ClientToScreen(hwnd: Hwnd, point: *mut Point) -> i32;
    fn MsgWaitForMultipleObjects(
        count: u32,
        handles: *const Handle,
        wait_all: i32,
        timeout: u32,
        wake_mask: u32,
    ) -> u32;
}

pub(super) fn validate_sources(paths: &[PathBuf], fixture: &Path) -> Result<(), String> {
    use std::os::windows::fs::MetadataExt;
    if paths.is_empty() || paths.len() > 8 {
        return Err("Performance probe requires 1..8 owned files".into());
    }
    let parent = fixture
        .parent()
        .ok_or("Missing fixture parent")?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    for path in paths {
        let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
        let path = path.canonicalize().map_err(|e| e.to_string())?;
        if !meta.is_file()
            || meta.file_attributes() & 0x400 != 0
            || meta.len() > 32 * 1024 * 1024
            || path.parent() != Some(parent.as_path())
            || path == fixture
        {
            return Err("Performance sources must be ordinary owned sibling files <=32MiB".into());
        }
    }
    Ok(())
}
struct Surface {
    dc: Hdc,
    bitmap: Handle,
    previous: Handle,
    width: i32,
    height: i32,
}
impl Surface {
    unsafe fn new(width: i32, height: i32) -> Result<Self, String> {
        let bytes = (width as usize)
            .checked_mul(height as usize)
            .and_then(|v| v.checked_mul(4))
            .filter(|v| *v <= 64 * 1024 * 1024)
            .ok_or("Probe DIB exceeds 64 MiB")?;
        if !(8..=8192).contains(&width) || !(8..=8192).contains(&height) {
            return Err("Invalid probe geometry".into());
        }
        let dc = CreateCompatibleDC(null_mut());
        if dc.is_null() {
            return Err("Probe memory DC unavailable".into());
        }
        let mut surface = Self {
            dc,
            bitmap: null_mut(),
            previous: null_mut(),
            width,
            height,
        };
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
        surface.bitmap = CreateDIBSection(dc, &info, 0, &mut bits, null_mut(), 0);
        if surface.bitmap.is_null() || bits.is_null() {
            return Err("Probe DIB unavailable".into());
        }
        surface.previous = SelectObject(dc, surface.bitmap);
        if surface.previous.is_null() || surface.previous as isize == -1 {
            return Err("Probe DIB selection failed".into());
        }
        std::ptr::write_bytes(bits.cast::<u8>(), 0, bytes);
        Ok(surface)
    }
    unsafe fn paint(&self, hwnd: Hwnd) -> Result<(), String> {
        if IsWindowVisible(hwnd) != 0 {
            return Err("Probe HWND became visible".into());
        }
        SendMessageW(hwnd, 0x0318, self.dc as usize, 4);
        GdiFlush();
        let pixel = GetPixel(self.dc, self.width / 2, self.height / 2);
        if pixel == 0 || pixel == u32::MAX {
            return Err("Native print did not produce a raster".into());
        }
        Ok(())
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
        }
    }
}
unsafe fn memory(phase: &str) -> Result<String, String> {
    let mut value = Memory {
        size: std::mem::size_of::<Memory>() as u32,
        ..Default::default()
    };
    if GetProcessMemoryInfo(
        GetCurrentProcess(),
        &mut value,
        std::mem::size_of::<Memory>() as u32,
    ) == 0
    {
        return Err("Process memory unavailable".into());
    }
    Ok(format!("{{\"phase\":\"{phase}\",\"private\":{},\"working\":{},\"peak_working\":{},\"peak_pagefile\":{}}}",value.private,value.working,value.peak_working,value.peak_pagefile))
}
unsafe fn cpu() -> Result<u64, String> {
    let mut values = [FileTime::default(); 4];
    let [a, b, c, d] = &mut values;
    if GetProcessTimes(GetCurrentProcess(), a, b, c, d) == 0 {
        return Err("Process CPU unavailable".into());
    }
    Ok(values[2..]
        .iter()
        .map(|v| ((v.high as u64) << 32) | v.low as u64)
        .sum())
}
unsafe fn document_state(hwnd: Hwnd) -> Result<(u64, usize), String> {
    let state = owner(hwnd).ok_or("Probe owner unavailable")?.state.borrow();
    let doc = state
        .workbench
        .active_document()
        .ok_or("Probe source closed")?
        .session
        .document();
    Ok((doc.revision(), doc.len()))
}
fn samples(values: &[f64]) -> String {
    values
        .iter()
        .map(|v| format!("{v:.6}"))
        .collect::<Vec<_>>()
        .join(",")
}

pub(super) unsafe fn run(hwnd: Hwnd, options: &SmokeOptions) -> Result<(), String> {
    if options.performance_geometry_only {
        return geometry::run(hwnd, options);
    }
    if options.performance_services_only {
        return services::run(hwnd, options);
    }
    let mut report = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&options.report)
        .map_err(|e| e.to_string())?;
    let started = Instant::now();
    let mut phases = vec![memory("source_open_before_probe")?];
    let (width, height, documents) = {
        let state = owner(hwnd).ok_or("Probe owner unavailable")?.state.borrow();
        (
            state.width,
            state.height,
            state.workbench.tabs().tabs().len(),
        )
    };
    let surface = Surface::new(width, height)?;
    phases.push(memory("probe_target_allocated")?);
    let before_compositions = owner(hwnd)
        .ok_or("Probe owner unavailable")?
        .buffer
        .borrow()
        .statistics()
        .compositions;
    surface.paint(hwnd)?;
    phases.push(memory("native_frame_painted")?);
    for _ in 0..documents {
        smoke::command(hwnd, CommandId::TabNext)?;
        surface.paint(hwnd)?;
    }
    phases.push(memory("all_documents_visited")?);
    let mut point = Point {
        x: width / 2,
        y: height / 2,
    };
    if ClientToScreen(hwnd, &mut point) == 0 {
        return Err("Probe coordinate conversion failed".into());
    }
    let packed = ((point.x as u16 as usize) | ((point.y as u16 as usize) << 16)) as isize;
    let mut wheel = Vec::with_capacity(120);
    for index in 0..120 {
        let time = Instant::now();
        let delta = if index < 60 { -480i16 } else { 480i16 };
        SendMessageW(hwnd, WM_MOUSEWHEEL, (delta as u16 as usize) << 16, packed);
        surface.paint(hwnd)?;
        wheel.push(time.elapsed().as_secs_f64() * 1000.0);
    }
    phases.push(memory("after_scroll")?);
    let original = document_state(hwnd)?;
    let mut edits = Vec::with_capacity(32);
    for _ in 0..32 {
        let time = Instant::now();
        SendMessageW(hwnd, WM_UNICHAR, b'x' as usize, 0);
        if document_state(hwnd)? == original {
            return Err("Probe insertion was not applied".into());
        }
        surface.paint(hwnd)?;
        smoke::command(hwnd, CommandId::EditUndo)?;
        surface.paint(hwnd)?;
        if document_state(hwnd)? != original {
            return Err("Probe Undo did not restore revision and length".into());
        }
        edits.push(time.elapsed().as_secs_f64() * 1000.0);
    }
    phases.push(memory("after_edit_undo")?);
    let idle_start = Instant::now();
    let cpu_start = cpu()?;
    while idle_start.elapsed() < Duration::from_secs(2) {
        let mut message: Msg = std::mem::zeroed();
        while PeekMessageW(&mut message, hwnd, 0, 0, 1) != 0 {
            DispatchMessageW(&message);
        }
        let remaining = Duration::from_secs(2).saturating_sub(idle_start.elapsed());
        if !remaining.is_zero()
            && MsgWaitForMultipleObjects(
                0,
                std::ptr::null(),
                0,
                remaining.as_millis().max(1) as u32,
                0x04ff, // QS_ALLINPUT: sleep until this queue has real work.
            ) == u32::MAX
        {
            return Err("Probe message wait failed".into());
        }
    }
    let idle = idle_start.elapsed().as_secs_f64();
    let cpu_seconds = (cpu()?.saturating_sub(cpu_start)) as f64 / 10_000_000.0;
    phases.push(memory("idle_after_edit_undo")?);
    for _ in 0..documents {
        smoke::command(hwnd, CommandId::FileClose)?;
        surface.paint(hwnd)?;
    }
    phases.push(memory("documents_closed")?);
    let after_compositions = owner(hwnd)
        .ok_or("Probe owner unavailable")?
        .buffer
        .borrow()
        .statistics()
        .compositions;
    let errors = owner(hwnd)
        .ok_or("Probe owner unavailable")?
        .state
        .borrow()
        .paint_errors;
    if after_compositions <= before_compositions || errors != 0 {
        return Err("Native probe did not compose cleanly".into());
    }
    let probe_bytes = width as usize * height as usize * 4;
    drop(surface);
    phases.push(memory("probe_target_released")?);
    writeln!(report,"{{\"ok\":true,\"visible\":false,\"physical_display_verified\":false,\"endpoint\":\"same_process_WM_PRINTCLIENT_retained_native_DIB\",\"probe_dib_bytes_included_in_process\":{probe_bytes},\"width\":{width},\"height\":{height},\"documents\":{documents},\"native_compositions\":{},\"paint_errors\":{errors},\"memory\":[{}],\"scroll_samples_ms\":[{}],\"edit_undo_samples_ms\":[{}],\"idle\":{{\"elapsed_s\":{idle},\"cpu_s\":{cpu_seconds},\"percent_of_one_core\":{}}},\"wall_ms\":{}}}",after_compositions-before_compositions,phases.join(","),samples(&wheel),samples(&edits),100.0*cpu_seconds/idle,started.elapsed().as_millis()).map_err(|e|e.to_string())?;
    Ok(())
}
