//! Native frontend over the existing application model. No editable widget owns
//! another source buffer. Platform support and smoke evidence are explicit.
use jc_text::TextEncoding;
use std::path::PathBuf;

#[derive(Debug, Default)]
pub struct GuiOptions {
    pub paths: Vec<PathBuf>,
    pub project_root: Option<PathBuf>,
    pub encoding: Option<TextEncoding>,
    pub search_workers: Option<usize>,
    pub git_executable: Option<PathBuf>,
    pub smoke: Option<SmokeOptions>,
}

/// Internal automation is confined to an owned hidden HWND and a small fixture
/// beneath workspace/work. It never sends input to another application's window.
#[derive(Debug)]
pub struct SmokeOptions {
    pub fixture: PathBuf,
    pub report: PathBuf,
    pub bitmap: Option<PathBuf>,
    pub navigation_only: bool,
    pub line_edit_only: bool,
    pub selection_reopen_only: bool,
    pub rectangle_only: bool,
    pub parity_only: bool,
    pub bottom_only: bool,
    pub selector_input_only: bool,
    pub interaction_only: bool,
    pub menu_invoker_only: bool,
    pub syntax_only: bool,
}

#[cfg(windows)]
mod windows;

pub fn run(options: GuiOptions) -> Result<(), String> {
    #[cfg(windows)]
    {
        windows::run(options)
    }
    #[cfg(not(windows))]
    {
        let _ = options;
        Err("The native Win32/GDI frontend is unavailable on this platform".into())
    }
}
