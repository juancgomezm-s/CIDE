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
        Ok(Self {
            stdout,
            restored: false,
        })
    }

    pub fn size(&self) -> io::Result<(u16, u16)> {
        terminal::size()
    }

    pub fn draw(&mut self, screen: &Screen) -> io::Result<()> {
        queue!(self.stdout, Hide, MoveTo(0, 0), Clear(ClearType::All))?;
        for y in 0..screen.height() {
            queue!(self.stdout, MoveTo(0, y))?;
            let mut active = CellStyle::Normal;
            queue!(self.stdout, SetAttribute(Attribute::Reset))?;
            for x in 0..screen.width() {
                let cell = screen.cell(x, y);
                if cell.style != active {
                    active = cell.style;
                    let attribute = match active {
                        CellStyle::Normal => Attribute::Reset,
                        CellStyle::Reverse => Attribute::Reverse,
                        CellStyle::Dim => Attribute::Dim,
                    };
                    queue!(self.stdout, SetAttribute(attribute))?;
                }
                queue!(self.stdout, Print(cell.ch))?;
            }
            queue!(self.stdout, SetAttribute(Attribute::Reset))?;
        }
        if let Some((x, y)) = screen.cursor() {
            queue!(self.stdout, MoveTo(x, y), Show)?;
        } else {
            queue!(self.stdout, Hide)?;
        }
        self.stdout.flush()
    }

    pub fn restore(&mut self) -> io::Result<()> {
        if self.restored {
            return Ok(());
        }
        self.restored = true;
        let screen_result = execute!(
            self.stdout,
            SetAttribute(Attribute::Reset),
            Show,
            LeaveAlternateScreen
        );
        let raw_result = terminal::disable_raw_mode();
        screen_result.and(raw_result)
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}
