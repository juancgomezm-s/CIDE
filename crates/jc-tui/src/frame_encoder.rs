//! VT output has no application access; only acknowledged frames become a baseline.
use crate::screen::{CellStyle, Screen};
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    style::{Attribute, ResetColor, SetAttribute, SetBackgroundColor, SetForegroundColor},
    terminal::{Clear, ClearType},
    Command,
};
use std::io::{self, Write};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DrawStats {
    pub full: bool,
    pub cells: usize,
    pub runs: usize,
    pub bytes: usize,
}
#[derive(Default)]
pub struct FrameEncoder {
    previous: Option<Screen>,
}
fn command(out: &mut String, value: impl Command) {
    value.write_ansi(out).expect("format to String");
}
fn apply_style(out: &mut String, role: CellStyle, color: bool) {
    let style = crate::theme::style(role);
    if role == CellStyle::Dim {
        command(out, SetAttribute(Attribute::Dim));
    }
    if color {
        command(out, SetForegroundColor(style.foreground));
        command(out, SetBackgroundColor(style.background));
    } else if style.reverse {
        command(out, SetAttribute(Attribute::Reverse));
    }
    if style.bold {
        command(out, SetAttribute(Attribute::Bold));
    }
    if style.underline {
        command(out, SetAttribute(Attribute::Underlined));
    }
}
impl FrameEncoder {
    pub fn invalidate(&mut self) {
        self.previous = None;
    }
    pub fn draw_to(&mut self, writer: &mut impl Write, screen: &Screen) -> io::Result<DrawStats> {
        let full = self
            .previous
            .as_ref()
            .is_none_or(|old| old.width() != screen.width() || old.height() != screen.height());
        let cursor_changed = self
            .previous
            .as_ref()
            .is_none_or(|old| old.cursor() != screen.cursor());
        let mut stats = DrawStats {
            full,
            ..DrawStats::default()
        };
        let color = std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty());
        let mut out = String::new();
        if full {
            command(&mut out, Hide);
            command(&mut out, ResetColor);
            command(&mut out, SetAttribute(Attribute::Reset));
            apply_style(&mut out, CellStyle::Normal, color);
            command(&mut out, Clear(ClearType::All));
        }
        let mut active = None;
        for y in 0..screen.height() {
            let mut x = 0;
            let mut in_run = false;
            while x < screen.width() {
                let width = screen.atom_width(x, y);
                if width == 0 {
                    x += 1;
                    continue;
                }
                let changed = full
                    || (0..width).any(|n| {
                        !self
                            .previous
                            .as_ref()
                            .unwrap()
                            .same_cell(screen, x + n as u16, y)
                    });
                if !changed {
                    in_run = false;
                    x += width as u16;
                    continue;
                }
                if out.is_empty() {
                    command(&mut out, Hide);
                }
                if !in_run {
                    command(&mut out, MoveTo(x, y));
                    stats.runs += 1;
                    in_run = true;
                }
                let cell = screen.cell(x, y);
                if active != Some(cell.style) {
                    command(&mut out, ResetColor);
                    command(&mut out, SetAttribute(Attribute::Reset));
                    apply_style(&mut out, cell.style, color);
                    active = Some(cell.style);
                }
                screen.append_atom(x, y, &mut out);
                stats.cells += width;
                x += width as u16;
            }
        }
        if full || stats.cells > 0 || cursor_changed {
            if out.is_empty() {
                command(&mut out, Hide);
            }
            command(&mut out, ResetColor);
            command(&mut out, SetAttribute(Attribute::Reset));
            if let Some((x, y)) = screen.cursor() {
                command(&mut out, MoveTo(x, y));
                command(&mut out, Show);
            } else {
                command(&mut out, Hide);
            }
        }
        stats.bytes = out.len();
        if out.is_empty() {
            return Ok(stats);
        }
        if let Err(error) = writer
            .write_all(out.as_bytes())
            .and_then(|()| writer.flush())
        {
            self.invalidate();
            return Err(error);
        }
        self.previous = Some(screen.clone());
        Ok(stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cluster_payload_changes_and_wide_to_narrow_erase_old_coverage_incrementally() {
        let mut encoder = FrameEncoder::default();
        let mut first = Screen::new(10, 1);
        first.text(0, 0, "e\u{301}界Z");
        encoder.draw_to(&mut Vec::new(), &first).unwrap();
        let mut second = Screen::new(10, 1);
        second.text(0, 0, "e\u{302}x Z");
        let mut out = Vec::new();
        let stats = encoder.draw_to(&mut out, &second).unwrap();
        assert!(!stats.full);
        let output = std::str::from_utf8(&out).unwrap();
        assert!(output.contains("e\u{302}x "));
        assert!(!output.contains("[2J"));
        assert_eq!(
            encoder.draw_to(&mut Vec::new(), &second).unwrap(),
            DrawStats::default()
        );
        let mut emoji = Screen::new(10, 1);
        emoji.text(0, 0, "👨‍👩‍👧‍👦");
        let mut out = Vec::new();
        encoder.draw_to(&mut out, &emoji).unwrap();
        assert!(std::str::from_utf8(&out).unwrap().contains("👨‍👩‍👧‍👦"));
    }
    #[test]
    fn unchanged_frames_emit_nothing_and_cursor_only_change_does_not_repaint() {
        let mut encoder = FrameEncoder::default();
        let mut screen = Screen::new(80, 24);
        let mut out = Vec::new();
        let first = encoder.draw_to(&mut out, &screen).unwrap();
        assert!(first.full);
        assert_eq!(first.cells, 1920);
        out.clear();
        assert_eq!(
            encoder.draw_to(&mut out, &screen).unwrap(),
            DrawStats::default()
        );
        assert!(out.is_empty());
        screen.set_cursor(2, 3);
        let stats = encoder.draw_to(&mut out, &screen).unwrap();
        assert_eq!(stats.cells, 0);
        assert!(!out.is_empty());
        assert!(!String::from_utf8(out).unwrap().contains("[2J"));
    }
    #[test]
    fn erasing_text_and_changing_style_emit_cells_and_reset_attributes() {
        let mut encoder = FrameEncoder::default();
        let mut screen = Screen::new(10, 2);
        screen.text_styled(0, 0, "éab", CellStyle::Reverse);
        encoder.draw_to(&mut Vec::new(), &screen).unwrap();
        screen.text_styled(0, 0, "é  ", CellStyle::Dim);
        let mut out = Vec::new();
        let stats = encoder.draw_to(&mut out, &screen).unwrap();
        assert_eq!(stats.cells, 3);
        assert!(!stats.full);
        let out = String::from_utf8(out).unwrap();
        assert!(out.contains("\x1b[0m\x1b[2m"));
        assert!(out.contains("é  "));
        assert!(!out.contains("[2J"));
    }
    #[test]
    fn resize_invalidation_and_output_failure_require_a_full_frame() {
        struct Fail;
        impl Write for Fail {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::Error::other("test failure"))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut encoder = FrameEncoder::default();
        let mut screen = Screen::new(4, 2);
        encoder.draw_to(&mut Vec::new(), &screen).unwrap();
        screen.put(0, 0, 'x');
        assert!(encoder.draw_to(&mut Fail, &screen).is_err());
        assert!(encoder.draw_to(&mut Vec::new(), &screen).unwrap().full);
        encoder.invalidate();
        assert!(encoder.draw_to(&mut Vec::new(), &screen).unwrap().full);
        assert!(
            encoder
                .draw_to(&mut Vec::new(), &Screen::new(3, 2))
                .unwrap()
                .full
        );
    }
}
