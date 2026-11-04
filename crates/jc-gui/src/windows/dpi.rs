//! Per-window DPI is sampled at creation and updated by WM_DPICHANGED.
//! Win32 geometry is stored in physical pixels, independent of the text model.
use super::{ffi::*, shell, wide, State};
use std::{ffi::c_void, ptr::null_mut};

pub(super) const CHANGED: u32 = 0x02e0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Dpi(i32);

impl Dpi {
    pub(super) const BASE: Self = Self(96);

    pub(super) fn from_raw(raw: u32) -> Self {
        match raw {
            48..=768 => Self(raw as i32),
            _ => Self::BASE,
        }
    }

    pub(super) fn pixels(self, design: i32) -> i32 {
        ((i64::from(design) * i64::from(self.0) + 48) / 96) as i32
    }
}

#[link(name = "kernel32")]
extern "system" {
    fn GetProcAddress(module: Handle, name: *const u8) -> *mut c_void;
}

#[link(name = "user32")]
extern "system" {
    fn SetWindowPos(
        hwnd: Hwnd,
        after: Hwnd,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        flags: u32,
    ) -> i32;
}

unsafe fn user32(name: &[u8]) -> *mut c_void {
    let module = GetModuleHandleW(wide("user32.dll").as_ptr());
    if module.is_null() {
        null_mut()
    } else {
        GetProcAddress(module, name.as_ptr())
    }
}

type SetThreadContext = unsafe extern "system" fn(Handle) -> Handle;

/// Restore the caller's DPI context if the GUI is hosted by another frontend.
pub(super) struct ThreadContext(Option<(SetThreadContext, Handle)>);

impl ThreadContext {
    pub(super) unsafe fn enter() -> Self {
        let address = user32(b"SetThreadDpiAwarenessContext\0");
        if address.is_null() {
            return Self(None);
        }
        let set: SetThreadContext = std::mem::transmute(address);
        let mut previous = set(-4isize as Handle); // PER_MONITOR_AWARE_V2
        if previous.is_null() {
            previous = set(-3isize as Handle); // PER_MONITOR_AWARE
        }
        Self((!previous.is_null()).then_some((set, previous)))
    }
}

impl Drop for ThreadContext {
    fn drop(&mut self) {
        if let Some((set, previous)) = self.0 {
            unsafe { set(previous) };
        }
    }
}

pub(super) unsafe fn for_window(hwnd: Hwnd) -> Dpi {
    let address = user32(b"GetDpiForWindow\0");
    if address.is_null() {
        return Dpi::BASE;
    }
    let get: unsafe extern "system" fn(Hwnd) -> u32 = std::mem::transmute(address);
    Dpi::from_raw(get(hwnd))
}

pub(super) unsafe fn apply_rect(hwnd: Hwnd, rect: Rect) {
    let width = rect.right.saturating_sub(rect.left);
    let height = rect.bottom.saturating_sub(rect.top);
    if width > 0 && height > 0 {
        // SWP_NOZORDER | SWP_NOACTIVATE
        SetWindowPos(hwnd, null_mut(), rect.left, rect.top, width, height, 0x14);
    }
}

impl State {
    pub(super) fn chrome_layout(&self) -> super::chrome::Layout {
        super::chrome::Layout::at_dpi(self.width, self.height, self.maximized, self.dpi)
    }

    pub(super) unsafe fn refresh_dpi(&mut self, hwnd: Hwnd, dpi: Dpi) {
        if self.dpi != dpi {
            let create = |size: i32, face: &str, pitch| {
                CreateFontW(
                    -dpi.pixels(size),
                    0,
                    0,
                    0,
                    400,
                    0,
                    0,
                    0,
                    1,
                    0,
                    0,
                    5,
                    pitch,
                    wide(face).as_ptr(),
                )
            };
            let editor = create(17, "Consolas", 0x31);
            let ui = create(14, "Segoe UI", 0);
            if !editor.is_null() && !ui.is_null() {
                if self.owned_font {
                    DeleteObject(self.font);
                }
                if self.owned_ui_font {
                    DeleteObject(self.ui_font);
                }
                self.font = editor;
                self.ui_font = ui;
                self.owned_font = true;
                self.owned_ui_font = true;
            } else {
                for created in [editor, ui] {
                    if !created.is_null() {
                        DeleteObject(created);
                    }
                }
            }
            self.dpi = dpi;
            self.cancel_pointer();
            self.interaction_epoch = self.interaction_epoch.saturating_add(1);
            self.explorer_hits = None;
            self.outline.hits = None;
            self.bottom_hits = None;
            self.tab_hits = None;
            self.chrome_hover = None;
        }

        let dc = GetDC(hwnd);
        if !dc.is_null() {
            let old = SelectObject(dc, self.font);
            let mut metrics: TextMetric = std::mem::zeroed();
            if GetTextMetricsW(dc, &mut metrics) != 0 {
                // Use the same advance for glyphs and hit testing.
                self.cell_width = metrics.average.saturating_add(dpi.pixels(1)).max(6);
                self.cell_height = (metrics.height + metrics.external).max(12);
            }
            self.ascii_runs = shell::safe_ascii_runs(dc, self.cell_width);
            SelectObject(dc, old);
            ReleaseDC(hwnd, dc);
        }
    }
}
