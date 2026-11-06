//! CIDE-owned themed filesystem pickers. Call outside any Workbench borrow.
use super::ffi::Hwnd;
use std::path::{Path, PathBuf};
#[path = "folder_picker.rs"]
mod picker;

pub(super) unsafe fn choose_folder(
    hwnd: Hwnd,
    initial: Option<&Path>,
) -> Result<Option<PathBuf>, String> {
    picker::choose(hwnd, initial, false)
}
pub(super) unsafe fn choose_file(
    hwnd: Hwnd,
    initial: Option<&Path>,
) -> Result<Option<PathBuf>, String> {
    picker::choose(hwnd, initial, true)
}
