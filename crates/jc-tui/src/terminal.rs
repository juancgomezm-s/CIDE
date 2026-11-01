use crate::screen::{CellStyle, Screen};
use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::style::{Attribute, Print, SetAttribute};
use crossterm::terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{execute, queue};
use std::io::{self, Stdout, Write};

pub struct TerminalSession {
    stdout: Stdout,
    restored: bool,
}

impl TerminalSession {
    pub fn enter() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        let mut stdout = io::stdout();
        if let Err(error) = execute!(stdout, EnterAlternateScreen, Hide) {
            let _ = terminal::disable_raw_mode();
            return Err(error);
        }
        Ok(Self { stdout, restored: false })
    }

    pub fn size(&self) -> io::Result<(u16, u16)> { terminal::size() }

    pub fn draw(&mut self, screen: &Screen