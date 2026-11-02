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

pub use state::LaunchOptions;

mod event_loop;
pub fn run(options: LaunchOptions) -> std::io::Result<()> {
    event_loop::run(options)
}
