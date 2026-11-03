use jc_app::Glyph;
use jc_text::grapheme;
use std::collections::BTreeMap;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CellStyle {
    #[default]
    Normal,
    Reverse,
    Dim,
    Syntax(jc_languages::syntax::SyntaxKind),
    Menu,
    MenuActive,
    MenuMnemonic,
    MenuMnemonicActive,
    Panel,
    PanelTitle,
    Status,
    Accent,
    Border,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Cell {
    pub ch: char,
    pub style: CellStyle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StoredGlyph {
    glyph: Glyph,
    width: u16,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screen {
    width: u16,
    height: u16,
    cells: Vec<Cell>,
    cursor: Option<(u16, u16)>,
    glyphs: BTreeMap<usize, StoredGlyph>,
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
            glyphs: BTreeMap::new(),
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
        let ch = sanitize(ch);
        let mut buffer = [0; 4];
        let text = ch.encode_utf8(&mut buffer);
        let width = grapheme::width(text).max(1);
        self.put_glyph(x, y, &Glyph::Scalar(ch), width, style);
    }
    fn clear_atom(&mut self, index: usize) {
        let covering = self
            .glyphs
            .range(..=index)
            .next_back()
            .and_then(|(&start, g)| (index < start + usize::from(g.width)).then_some(start));
        if let Some(start) = covering {
            let glyph = self.glyphs.remove(&start).unwrap();
            for cell in &mut self.cells[start..start + usize::from(glyph.width)] {
                cell.ch = ' ';
            }
        }
    }
    pub(crate) fn put_glyph(
        &mut self,
        x: u16,
        y: u16,
        glyph: &Glyph,
        width: usize,
        style: CellStyle,
    ) {
        if x >= self.width || y >= self.height || width == 0 {
            return;
        }
        if width > usize::from(self.width - x) {
            // Never emit a partial wide/cluster atom across the right border.
            for column in x..self.width {
                self.put_styled(column, y, ' ', style);
            }
            return;
        }
        let index = usize::from(y) * usize::from(self.width) + usize::from(x);
        for offset in 0..width {
            self.clear_atom(index + offset);
        }
        self.cells[index] = Cell {
            ch: glyph.first(),
            style,
        };
        if width > 1 || matches!(glyph, Glyph::Cluster(_)) {
            self.glyphs.insert(
                index,
                StoredGlyph {
                    glyph: glyph.clone(),
                    width: width as u16,
                },
            );
            for offset in 1..width {
                self.cells[index + offset] = Cell { ch: '\0', style };
            }
        }
    }
    pub(crate) fn atom_width(&self, x: u16, y: u16) -> usize {
        let index = usize::from(y) * usize::from(self.width) + usize::from(x);
        if self.cells[index].ch == '\0' {
            return 0;
        }
        self.glyphs.get(&index).map_or(1, |g| usize::from(g.width))
    }
    pub(crate) fn append_atom(&self, x: u16, y: u16, out: &mut String) {
        let index = usize::from(y) * usize::from(self.width) + usize::from(x);
        if let Some(g) = self.glyphs.get(&index) {
            g.glyph.with_str(|s| out.push_str(s));
        } else if self.cells[index].ch != '\0' {
            out.push(self.cells[index].ch);
        }
    }
    pub(crate) fn same_cell(&self, other: &Self, x: u16, y: u16) -> bool {
        let index = usize::from(y) * usize::from(self.width) + usize::from(x);
        self.cells[index] == other.cells[index]
            && self.glyphs.get(&index) == other.glyphs.get(&index)
    }

    pub fn text(&mut self, x: u16, y: u16, text: &str) {
        self.text_styled(x, y, text, CellStyle::Normal);
    }

    pub fn text_styled(&mut self, x: u16, y: u16, text: &str, style: CellStyle) {
        self.text_styled_in(x, y, self.width.saturating_sub(x), text, style);
    }
    pub(crate) fn text_styled_in(
        &mut self,
        x: u16,
        y: u16,
        width: u16,
        text: &str,
        style: CellStyle,
    ) -> u16 {
        if y >= self.height || x >= self.width {
            return x;
        }
        let end = x.saturating_add(width).min(self.width);
        let mut column = x;
        for cluster in grapheme::graphemes(text) {
            if column >= end {
                break;
            }
            if cluster.len() > grapheme::MAX_CLUSTER_BYTES {
                break;
            }
            let cells = grapheme::width(cluster);
            if cells == 0 || cluster.chars().any(char::is_control) {
                for ch in cluster.chars() {
                    if column >= end {
                        break;
                    }
                    self.put_styled(column, y, ch, style);
                    column = column.saturating_add(1);
                }
            } else {
                if cells > usize::from(end - column) {
                    break;
                }
                let glyph = if cluster.chars().count() == 1 {
                    Glyph::Scalar(cluster.chars().next().unwrap())
                } else {
                    Glyph::Cluster(std::sync::Arc::from(cluster))
                };
                self.put_glyph(column, y, &glyph, cells, style);
                column = column.saturating_add(cells as u16);
            }
        }
        column
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
    pub fn hline_styled(&mut self, x: u16, y: u16, width: u16, ch: char, style: CellStyle) {
        for offset in 0..width.min(self.width.saturating_sub(x)) {
            self.put_styled(x + offset, y, ch, style);
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
        let mut row = String::new();
        for x in 0..self.width {
            self.append_atom(x, y, &mut row);
        }
        row
    }
}

fn sanitize(ch: char) -> char {
    if ch == '\t' {
        '→'
    } else if ch.is_control() || jc_text::grapheme::width(ch.encode_utf8(&mut [0; 4])) == 0 {
        '·'
    } else {
        ch
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clusters_keep_sparse_payload_and_wide_continuations_clear_as_a_unit() {
        let mut screen = Screen::new(12, 2);
        screen.text(0, 0, "e\u{301}界👨‍👩‍👧‍👦Z");
        assert_eq!(screen.row_text(0), "e\u{301}界👨‍👩‍👧‍👦Z      ");
        assert_eq!(screen.atom_width(0, 0), 1);
        assert_eq!(screen.atom_width(1, 0), 2);
        assert_eq!(screen.atom_width(2, 0), 0);
        screen.put(2, 0, 'x');
        assert_eq!(screen.cell(1, 0).ch, ' ');
        assert_eq!(screen.cell(2, 0).ch, 'x');
        screen.text(11, 1, "界");
        assert_eq!(screen.cell(11, 1).ch, ' ');
        let ascii = Screen::new(80, 24);
        assert!(ascii.glyphs.is_empty());
        assert_eq!(std::mem::size_of::<Cell>(), 8);
    }

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
