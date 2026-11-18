//! Measure navigation and GDI output on the native smoke window.
use super::{document_state, memory, samples, Surface};
use crate::{
    windows::{ffi::*, owner, smoke},
    SmokeOptions,
};
use jc_app::{CommandId, Modifiers};
use std::{
    fs::OpenOptions,
    io::Write,
    time::{Duration, Instant},
};

const START_MARKER: &[u8] = b"MARCADOR_INICIO_CIDE_GB\r\n";
const END_MARKER: &[u8] = b"MARCADOR_FINAL_CIDE_GB\r\n";
const FAR_OFFSET: u64 = (1 << 32) + 65536;
pub(super) const FILE_BYTES: u64 = FAR_OFFSET + END_MARKER.len() as u64;

unsafe fn wait_for_byte(hwnd: Hwnd, offset: usize, timeout: Duration) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    loop {
        let gui = owner(hwnd).ok_or("La ventana de prueba se cerró")?;
        {
            let state = gui.state.borrow();
            if state.workbench.active_paged_byte() == Some(offset) {
                return Ok(());
            }
            if offset == 0
                && state.workbench.active_document().is_some()
                && !state.workbench.large_open_pending()
                && state.workbench.active_paged_byte().is_none()
            {
                return Err("La apertura no entregó un documento paginado".into());
            }
        }
        if Instant::now() >= deadline {
            return Err(format!("La GUI no llegó al byte {offset} dentro del plazo"));
        }
        let mut message: Msg = std::mem::zeroed();
        if PeekMessageW(&mut message, hwnd, TASKS_MESSAGE, TASKS_MESSAGE, 1) != 0 {
            DispatchMessageW(&message);
        } else {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

unsafe fn check_marker(hwnd: Hwnd, offset: usize, marker: &[u8]) -> Result<(), String> {
    let gui = owner(hwnd).ok_or("La ventana de prueba se cerró")?;
    let state = gui.state.borrow();
    let doc = state
        .workbench
        .active_document()
        .ok_or("Falta el documento paginado")?;
    let end = offset + marker.len();
    let viewport = state
        .workbench
        .paged_viewport_for_tab(doc.tab_id)
        .ok_or("Falta la vista paginada")?;
    if !viewport.rows().iter().any(|row| {
        row.content.start <= offset
            && row
                .ending
                .as_ref()
                .map_or(row.content.end, |ending| ending.end)
                >= end
    }) || doc
        .session
        .document()
        .read_range(offset..end)
        .map_err(|e| e.to_string())?
        != marker
    {
        return Err(format!("Falta el marcador en la vista del byte {offset}"));
    }
    Ok(())
}

pub(super) unsafe fn run(hwnd: Hwnd, options: &SmokeOptions) -> Result<(), String> {
    let started = Instant::now();
    let far_offset = usize::try_from(FAR_OFFSET).map_err(|_| "La medición requiere Windows x64")?;
    let gui = owner(hwnd).ok_or("La ventana de prueba se cerró")?;
    let (width, height) = {
        let state = gui.state.borrow();
        (state.width, state.height)
    };
    let mut phases = vec![memory("opening")?];
    let surface = Surface::new(width, height)?;
    let base = gui.buffer.borrow().statistics().compositions;
    wait_for_byte(hwnd, 0, Duration::from_secs(150))?;
    surface.paint(hwnd)?;
    let open_ms = started.elapsed().as_secs_f64() * 1000.0;
    let original = document_state(hwnd)?;
    if original.1 as u64 != FILE_BYTES {
        return Err("La extensión del documento de prueba no coincide".into());
    }
    check_marker(hwnd, 0, START_MARKER)?;
    phases.push(memory("opened")?);
    let mut jumps = Vec::with_capacity(12);
    for (offset, marker) in [(far_offset, END_MARKER), (0, START_MARKER)]
        .into_iter()
        .cycle()
        .take(12)
    {
        smoke::command(hwnd, CommandId::SearchGoToPosition)?;
        for digit in offset.to_string().bytes() {
            SendMessageW(hwnd, WM_CHAR, usize::from(digit), 0);
        }
        let sent = Instant::now();
        smoke::key(hwnd, 0x0d, Modifiers::NONE)?;
        wait_for_byte(hwnd, offset, Duration::from_secs(5))?;
        surface.paint(hwnd)?;
        jumps.push(sent.elapsed().as_secs_f64() * 1000.0);
        check_marker(hwnd, offset, marker)?;
    }
    phases.push(memory("after_jumps")?);
    let compositions = gui.buffer.borrow().statistics().compositions - base;
    let errors = gui.state.borrow().paint_errors;
    if errors != 0 || compositions < 13 || document_state(hwnd)? != original {
        return Err("La medición paginada no completó su composición de solo lectura".into());
    }
    smoke::command(hwnd, CommandId::FileClose)?;
    drop(surface);
    phases.push(memory("closed")?);
    let mut report = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&options.report)
        .map_err(|e| e.to_string())?;
    writeln!(
        report,
        concat!(
            "{{\"ok\":true,\"visible\":false,\"physical_display_verified\":false,",
            "\"endpoint\":\"WM_PRINTCLIENT\",\"file_bytes\":{},\"far_offset\":{},",
            "\"open_ms\":{:.6},\"jump_samples_ms\":[{}],\"native_compositions\":{},",
            "\"paint_errors\":{},\"probe_dib_bytes\":{},\"memory\":[{}]}}"
        ),
        FILE_BYTES,
        FAR_OFFSET,
        open_ms,
        samples(&jumps),
        compositions,
        errors,
        width as usize * height as usize * 4,
        phases.join(",")
    )
    .map_err(|e| e.to_string())
}
