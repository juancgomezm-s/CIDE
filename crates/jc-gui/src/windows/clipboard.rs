//! Shared strict native transport; GUI supplies its existing owned window.
use super::ffi::Hwnd;
pub(super) fn read(hwnd: Hwnd) -> Result<Option<String>, String> {
    jc_platform::clipboard::read(hwnd as usize)
}
pub(super) fn read_limited(hwnd: Hwnd, limit: usize) -> Result<Option<String>, String> {
    jc_platform::clipboard::read_limited(hwnd as usize, limit)
}
pub(super) fn write(hwnd: Hwnd, text: &str) -> Result<(), String> {
    jc_platform::clipboard::write(hwnd as usize, text)
}
