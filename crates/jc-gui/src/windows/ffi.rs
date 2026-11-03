//! Minimal Win32 ABI declarations. Handles stay on the owning UI thread.
#![allow(non_snake_case)]
use std::ffi::c_void;
pub type Handle = *mut c_void;
pub type Hwnd = Handle;
pub type Hdc = Handle;
pub type WndProc = unsafe extern "system" fn(Hwnd, u32, usize, isize) -> isize;
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}
#[repr(C)]
pub struct Msg {
    pub hwnd: Hwnd,
    pub message: u32,
    pub wparam: usize,
    pub lparam: isize,
    pub time: u32,
    pub pt: Point,
    pub private: u32,
}
#[repr(C)]
pub struct WndClass {
    pub style: u32,
    pub proc: Option<WndProc>,
    pub class_extra: i32,
    pub window_extra: i32,
    pub instance: Handle,
    pub icon: Handle,
    pub cursor: Handle,
    pub background: Handle,
    pub menu_name: *const u16,
    pub class_name: *const u16,
}
#[repr(C)]
pub struct CreateStruct {
    pub parameter: *mut c_void,
    pub instance: Handle,
    pub menu: Handle,
    pub parent: Hwnd,
    pub cy: i32,
    pub cx: i32,
    pub y: i32,
    pub x: i32,
    pub style: i32,
    pub name: *const u16,
    pub class: *const u16,
    pub ex_style: u32,
}
#[repr(C)]
pub struct PaintStruct {
    pub dc: Hdc,
    pub erase: i32,
    pub paint: Rect,
    pub restore: i32,
    pub incremental: i32,
    pub reserved: [u8; 32],
}
#[repr(C)]
pub struct TextMetric {
    pub height: i32,
    pub ascent: i32,
    pub descent: i32,
    pub internal: i32,
    pub external: i32,
    pub average: i32,
    pub max_width: i32,
    pub weight: i32,
    pub overhang: i32,
    pub aspect_x: i32,
    pub aspect_y: i32,
    pub first: u16,
    pub last: u16,
    pub default_char: u16,
    pub break_char: u16,
    pub italic: u8,
    pub underlined: u8,
    pub struck: u8,
    pub pitch: u8,
    pub charset: u8,
}
#[repr(C)]
pub struct OpenFileName {
    pub size: u32,
    pub owner: Hwnd,
    pub instance: Handle,
    pub filter: *const u16,
    pub custom_filter: *mut u16,
    pub custom_filter_len: u32,
    pub filter_index: u32,
    pub file: *mut u16,
    pub file_len: u32,
    pub file_title: *mut u16,
    pub file_title_len: u32,
    pub initial_dir: *const u16,
    pub title: *const u16,
    pub flags: u32,
    pub file_offset: u16,
    pub extension_offset: u16,
    pub default_extension: *const u16,
    pub custom_data: isize,
    pub hook: *mut c_void,
    pub template: *const u16,
    pub reserved: *mut c_void,
    pub reserved_size: u32,
    pub flags_ex: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct BitmapHeader {
    pub size: u32,
    pub width: i32,
    pub height: i32,
    pub planes: u16,
    pub bit_count: u16,
    pub compression: u32,
    pub image_size: u32,
    pub x_meter: i32,
    pub y_meter: i32,
    pub used: u32,
    pub important: u32,
}
#[repr(C)]
pub struct BitmapInfo {
    pub header: BitmapHeader,
    pub color: u32,
}

pub const WM_CREATE: u32 = 1;
pub const WM_DESTROY: u32 = 2;
pub const WM_SIZE: u32 = 5;
pub const WM_SETFOCUS: u32 = 7;
pub const WM_KILLFOCUS: u32 = 8;
pub const WM_PAINT: u32 = 15;
pub const WM_CLOSE: u32 = 16;
pub const WM_ERASEBKGND: u32 = 20;
pub const WM_NCCREATE: u32 = 0x81;
pub const WM_NCDESTROY: u32 = 0x82;
pub const WM_KEYDOWN: u32 = 0x100;
pub const WM_CHAR: u32 = 0x102;
pub const WM_SYSKEYDOWN: u32 = 0x104;
pub const WM_UNICHAR: u32 = 0x109;
pub const WM_COMMAND: u32 = 0x111;
pub const WM_INITMENU: u32 = 0x116;
pub const WM_LBUTTONDOWN: u32 = 0x201;
pub const WM_MOUSEMOVE: u32 = 0x200;
pub const WM_LBUTTONUP: u32 = 0x202;
pub const WM_CAPTURECHANGED: u32 = 0x215;
pub const WM_CANCELMODE: u32 = 0x1f;
pub const WM_LBUTTONDBLCLK: u32 = 0x203;
pub const GWLP_USERDATA: i32 = -21;
pub const SMOKE_MESSAGE: u32 = 0x8001;
pub const TASKS_MESSAGE: u32 = 0x8002;
#[link(name = "kernel32")]
extern "system" {
    pub fn GetModuleHandleW(name: *const u16) -> Handle;
}
#[link(name = "user32")]
extern "system" {
    pub fn RegisterClassW(class: *const WndClass) -> u16;
    pub fn UnregisterClassW(name: *const u16, instance: Handle) -> i32;
    pub fn CreateWindowExW(
        ex_style: u32,
        class: *const u16,
        title: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: Hwnd,
        menu: Handle,
        instance: Handle,
        parameter: *mut c_void,
    ) -> Hwnd;
    pub fn DefWindowProcW(hwnd: Hwnd, message: u32, wparam: usize, lparam: isize) -> isize;
    pub fn SetWindowLongPtrW(hwnd: Hwnd, index: i32, value: isize) -> isize;
    pub fn GetWindowLongPtrW(hwnd: Hwnd, index: i32) -> isize;
    pub fn DestroyWindow(hwnd: Hwnd) -> i32;
    pub fn IsWindow(hwnd: Hwnd) -> i32;
    pub fn ShowWindow(hwnd: Hwnd, show: i32) -> i32;
    pub fn UpdateWindow(hwnd: Hwnd) -> i32;
    pub fn SetFocus(hwnd: Hwnd) -> Hwnd;
    pub fn SetCapture(hwnd: Hwnd) -> Hwnd;
    pub fn GetCapture() -> Hwnd;
    pub fn ReleaseCapture() -> i32;
    pub fn GetMessageW(message: *mut Msg, hwnd: Hwnd, min: u32, max: u32) -> i32;
    pub fn PeekMessageW(message: *mut Msg, hwnd: Hwnd, min: u32, max: u32, remove: u32) -> i32;
    pub fn TranslateMessage(message: *const Msg) -> i32;
    pub fn DispatchMessageW(message: *const Msg) -> isize;
    pub fn PostQuitMessage(code: i32);
    pub fn PostMessageW(hwnd: Hwnd, message: u32, wparam: usize, lparam: isize) -> i32;
    pub fn SendMessageW(hwnd: Hwnd, message: u32, wparam: usize, lparam: isize) -> isize;
    pub fn GetClientRect(hwnd: Hwnd, rect: *mut Rect) -> i32;
    pub fn InvalidateRect(hwnd: Hwnd, rect: *const Rect, erase: i32) -> i32;
    pub fn BeginPaint(hwnd: Hwnd, paint: *mut PaintStruct) -> Hdc;
    pub fn EndPaint(hwnd: Hwnd, paint: *const PaintStruct) -> i32;
    pub fn GetDC(hwnd: Hwnd) -> Hdc;
    pub fn ReleaseDC(hwnd: Hwnd, dc: Hdc) -> i32;
    pub fn FillRect(dc: Hdc, rect: *const Rect, brush: Handle) -> i32;
    pub fn LoadCursorW(instance: Handle, name: *const u16) -> Handle;
    pub fn LoadIconW(instance: Handle, name: *const u16) -> Handle;
    pub fn GetClassLongPtrW(hwnd: Hwnd, index: i32) -> usize;
    pub fn GetKeyState(key: i32) -> i16;
    pub fn CreateMenu() -> Handle;
    pub fn CreatePopupMenu() -> Handle;
    pub fn AppendMenuW(menu: Handle, flags: u32, item: usize, text: *const u16) -> i32;
    pub fn DestroyMenu(menu: Handle) -> i32;
    pub fn EnableMenuItem(menu: Handle, item: u32, flags: u32) -> u32;
    pub fn CheckMenuItem(menu: Handle, item: u32, flags: u32) -> u32;
    pub fn GetMenu(hwnd: Hwnd) -> Handle;
    pub fn MessageBoxW(hwnd: Hwnd, text: *const u16, caption: *const u16, kind: u32) -> i32;
    pub fn GetWindow(hwnd: Hwnd, relation: u32) -> Hwnd;
    pub fn GetWindowTextW(hwnd: Hwnd, text: *mut u16, max: i32) -> i32;
}
#[link(name = "gdi32")]
extern "system" {
    pub fn CreateFontW(
        height: i32,
        width: i32,
        escapement: i32,
        orientation: i32,
        weight: i32,
        italic: u32,
        underline: u32,
        strike: u32,
        charset: u32,
        output: u32,
        clip: u32,
        quality: u32,
        pitch: u32,
        face: *const u16,
    ) -> Handle;
    pub fn GetStockObject(kind: i32) -> Handle;
    pub fn DeleteObject(object: Handle) -> i32;
    pub fn SelectObject(dc: Hdc, object: Handle) -> Handle;
    pub fn GetTextMetricsW(dc: Hdc, metrics: *mut TextMetric) -> i32;
    pub fn GetClipBox(dc: Hdc, rect: *mut Rect) -> i32;
    pub fn RectVisible(dc: Hdc, rect: *const Rect) -> i32;
    pub fn IntersectClipRect(dc: Hdc, left: i32, top: i32, right: i32, bottom: i32) -> i32;
    pub fn SelectClipRgn(dc: Hdc, region: Handle) -> i32;
    pub fn CreateSolidBrush(color: u32) -> Handle;
    pub fn SetTextColor(dc: Hdc, color: u32) -> u32;
    pub fn SetBkColor(dc: Hdc, color: u32) -> u32;
    pub fn ExtTextOutW(
        dc: Hdc,
        x: i32,
        y: i32,
        flags: u32,
        rect: *const Rect,
        text: *const u16,
        length: u32,
        spacing: *const i32,
    ) -> i32;
    pub fn CreateCompatibleDC(dc: Hdc) -> Hdc;
    pub fn DeleteDC(dc: Hdc) -> i32;
    pub fn CreateDIBSection(
        dc: Hdc,
        info: *const BitmapInfo,
        usage: u32,
        bits: *mut *mut c_void,
        section: Handle,
        offset: u32,
    ) -> Handle;
    pub fn GdiFlush() -> i32;
}
#[link(name = "comdlg32")]
extern "system" {
    pub fn GetOpenFileNameW(options: *mut OpenFileName) -> i32;
    pub fn GetSaveFileNameW(options: *mut OpenFileName) -> i32;
    pub fn CommDlgExtendedError() -> u32;
}
