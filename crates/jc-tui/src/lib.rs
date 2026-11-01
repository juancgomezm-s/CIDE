mod app;
mod input;
mod render;
mod screen;
mod state;
mod terminal;

pub use state::LaunchOptions;

use crossterm::event;
use std::io;
use terminal::TerminalSession;

pub fn run(options: LaunchOptions) -> io::Result<()> {
    let mut terminal = TerminalSession::enter()?;
    let mut app = app::TuiApp::new(options);
    while !app.should_exit() {
        let (width, height) = terminal.size()?;
        let screen = app.render(width, height);
        terminal.draw(&screen)?;
        app.handle_event(event::read()?);
    }
    terminal.restore()
}
