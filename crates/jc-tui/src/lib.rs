mod app;
mod frame_encoder;
mod input;
pub use app::TuiApp;
pub use frame_encoder::{DrawStats, FrameEncoder};
pub use screen::{CellStyle, Screen};
mod render;
mod screen;
mod state;
mod terminal;
mod theme;

pub use state::LaunchOptions;

mod event_loop;
pub fn run(options: LaunchOptions) -> std::io::Result<()> {
    run_with_search_workers(options, 2)
}

/// Set a project-search CPU budget without changing system affinity/priorities.
pub fn run_with_search_workers(options: LaunchOptions, workers: usize) -> std::io::Result<()> {
    validate_search_workers(workers)?;
    event_loop::run(options, workers)
}

pub(crate) fn validate_search_workers(workers: usize) -> std::io::Result<()> {
    if !(1..=4).contains(&workers) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "La búsqueda del proyecto requiere entre 1 y 4 trabajadores",
        ));
    }
    Ok(())
}
