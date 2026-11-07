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
    pub workflow_only: bool,
    pub shell_only: bool,
    pub git_only: bool,
    pub save_only: bool,
    pub performance_only: bool,
    pub performance_services_only: bool,
}

impl SmokeOptions {
    pub fn measures_performance(&self) -> bool {
        self.performance_only || self.performance_services_only
    }
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
        Err("La interfaz nativa Win32/GDI no está disponible en esta plataforma".into())
    }
}
