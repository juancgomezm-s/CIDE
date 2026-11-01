#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CellStyle {
    #[default]
    Normal,
    Reverse,
    Dim,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Cell {
    pub ch: char,
    pub style: CellStyle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screen {
    width: u16,
    height: u16,
    cells: Vec<Cell>,
    cursor: Option<(u16, u16)>,
}

impl Screen {
    pub fn new(width: u16, height: u16) -> Self {
        let size = usize::from(width) * usize::from(height);
        Self {
            width,
            height,
            cells: vec![
                Cell {
                    ch: ' ',
                    style: CellStyle::Normal
                };
                size
            ],
            cursor: None,
        }
    }

    pub fn width(&self) -> u16 {
        self.width
    }
    pub fn height(&self) -> u16 {
        self.height
    }
    pub fn cursor(&self) -> Option<(u16, u16)> {
        self.cursor
    }

    pub fn set_cursor(&mut self, x: u16, y: u16) {
        self.cursor = (x < self.width && y < self.height).then_some((x, y));
    }

    pub fn hide_cursor(&mut self) {
        self.cursor = None;
    }

    pub fn put(&mut self, x: u16, y: u16, ch: char) {
        self.put_styled(x, y, ch, CellStyle::Normal);
    }

    pub fn put_styled(&mut self, x: u16, y: u16, ch: char, style: CellStyle) {
        if x >= self.width || y >= self.height {
            return;
        }
        let index = usize::from(y) * usize::from(self.width) + usize::from(x);
        self.cells[index] = Cell {
            ch: sanitize(ch),
            style,
        };
    }

    pub fn text(&mut self, x: u16, y: u16, text: &str) {
        self.text_styled(x, y, text, CellStyle::Normal);
    }

    pub fn text_styled(&mut self, x: u16, y: u16, text: &str, style: CellStyle) {
        if y >= self.height || x >= self.width {
            return;
        }
        let mut column = x;
        for ch in text.chars() {
            if column >= self.width {
                break;
            }
            self.put_styled(column, y, ch, style);
            column = column.saturating_add(1);
        }
    }

    pub fn fill_row(&mut self, y: u16, ch: char) {
        self.fill_row_styled(y, ch, CellStyle::Normal);
    }

    pub fn fill_row_styled(&mut self, y: u16, ch: char, style: CellStyle) {
        if y >= self.height {
            return;
        }
        for x in 0..self.width {
            self.put_styled(x, y, ch, style);
        }
    }

    pub fn hline(&mut self, x: u16, y: u16, width: u16, ch: char) {
        for offset in 0..width {
            self.put(x.saturating_add(offset), y, ch);
        }
    }

    pub fn vline(&mut self, x: u16, y: u16, height: u16, ch: char) {
        for offset in 0..height {
            self.put(x, y.saturating_add(offset), ch);
        }
    }

    pub fn box_outline(&mut self, x: u16, y: u16, width: u16, height: u16) {
        if width < 2 || height < 2 {
            return;
        }
        self.put(x, y, '┌');
        self.put(x + width - 1, y, '┐');
        self.put(x, y + height - 1, '└');
        self.put(x + width - 1, y + height - 1, '┘');
        self.hline(x + 1, y, width - 2, '─');
        self.hline(x + 1, y + height - 1, width - 2, '─');
        self.vline(x, y + 1, height - 2, '│');
        self.vline(x + width - 1, y + 1, height - 2, '│');
    }

    pub fn cell(&self, x: u16, y: u16) -> Cell {
        if x >= self.width || y >= self.height {
            return Cell::default();
        }
        self.cells[usize::from(y) * usize::from(self.width) + usize::from(x)]
    }

    #[cfg(test)]
    pub fn row_text(&self, y: u16) -> String {
        if y >= self.height {
            return String::new();
        }
        (0..self.width).map(|x| self.cell(x, y).ch).collect()
    }
}

fn sanitize(ch: char) -> char {
    if ch == '\t' {
        '→'
    } else if ch.is_control() {
        '·'
    } else {
        ch
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_clipped_and_control_chars_are_visible() {
        let mut screen = Screen::new(5, 2);
        screen.text(3, 0, "abXYZ");
        screen.text(0, 1, "a\tb");
        assert_eq!(screen.row_text(0), "   ab");
        assert_eq!(screen.row_text(1), "a→b  ");
    }

    #[test]
    fn styles_and_cursor_are_part_of_render_model() {
        let mut screen = Screen::new(4, 2);
        screen.put_styled(1, 0, 'x', CellStyle::Reverse);
        screen.set_cursor(2, 1);
        assert_eq!(screen.cell(1, 0).style, CellStyle::Reverse);
        assert_eq!(screen.cursor(), Some((2, 1)));
        screen.set_cursor(9, 9);
        assert_eq!(screen.cursor(), None);
    }

    #[test]
    fn box_does_not_write_outside_screen() {
        let mut screen = Screen::new(5, 4);
        screen.box_outline(0, 0, 5, 4);
        assert_eq!(screen.cell(0, 0).ch, '┌');
        assert_eq!(screen.cell(4, 3).ch, '┘');
    }
}
