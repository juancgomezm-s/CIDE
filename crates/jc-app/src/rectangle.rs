//! Deliberately limited one-cell geometry for rectangular editing in the TUI.
//! This is not Unicode grapheme or East Asian width support. A layout owns only
//! bounded offset/column metadata, never a source String or Document. Build once
//! per visited line and reuse it; callers should process large rectangles row by
//! row rather than retaining every line layout or cloning source lines on moves.
use jc_text::{DecodedScalars, TextEncoding};
use std::{fmt, ops::Range};

pub const TAB_STOP: usize = 4;
pub const MAX_COLUMNS: usize = 4096;
pub const MAX_LINE_BYTES: usize = 64 * 1024;
/// Presentation-only mapping for a padded virtual cell, never an edit offset.
pub const VIRTUAL_BYTE: usize = usize::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct VisualColumn(pub usize);

/// A logical line and a visual TAB4 cell. Never a scalar/LSP/display position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisualPosition {
    pub line: usize,
    pub column: VisualColumn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColumnLocation {
    /// Absolute valid source boundary. Inside a TAB this is the TAB byte start.
    pub byte: usize,
    pub tab_left: usize,
    pub tab_right: usize,
    pub virtual_spaces: usize,
}
impl ColumnLocation {
    pub fn inside_tab(self) -> bool {
        self.tab_left != 0 && self.tab_right != 0
    }
    pub fn is_virtual(self) -> bool {
        self.virtual_spaces != 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowReplacement {
    /// Absolute byte coordinates, to apply against the caller's expected revision.
    pub range: Range<usize>,
    /// Encode strictly with the document encoding at the transaction boundary.
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RectangleError {
    LineTooLong(usize),
    ColumnLimit(usize),
    InvalidColumns {
        start: VisualColumn,
        end: VisualColumn,
    },
    InvalidDecode(usize),
    LineEnding,
    UnsupportedScalar(char),
    ByteNotBoundary(usize),
    OffsetOverflow,
}
impl fmt::Display for RectangleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LineTooLong(bytes) => {
                write!(f, "Rectangle line exceeds {MAX_LINE_BYTES} bytes ({bytes})")
            }
            Self::ColumnLimit(column) => {
                write!(f, "Rectangle column exceeds {MAX_COLUMNS} ({column})")
            }
            Self::InvalidColumns { start, end } => {
                write!(f, "Rectangle columns are reversed: {}..{}", start.0, end.0)
            }
            Self::InvalidDecode(byte) => {
                write!(f, "Rectangle source has invalid encoding at byte {byte}")
            }
            Self::LineEnding => f.write_str("Rectangle line/replacement must exclude CR and LF"),
            Self::UnsupportedScalar(ch) => write!(f, "Rectangle width is unsupported for {ch:?}"),
            Self::ByteNotBoundary(byte) => write!(
                f,
                "Rectangle byte {byte} is not a supported scalar boundary"
            ),
            Self::OffsetOverflow => f.write_str("Rectangle source offset overflow"),
        }
    }
}
impl std::error::Error for RectangleError {}

struct Atom {
    bytes: Range<usize>,
    columns: Range<usize>,
    character: char,
}
pub struct LineLayout {
    source: Range<usize>,
    width: usize,
    atoms: Vec<Atom>,
}
impl fmt::Debug for LineLayout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LineLayout")
            .field("source", &self.source)
            .field("width", &self.width)
            .field("atoms", &self.atoms.len())
            .finish()
    }
}

fn column_limit(column: usize) -> Result<(), RectangleError> {
    if column > MAX_COLUMNS {
        Err(RectangleError::ColumnLimit(column))
    } else {
        Ok(())
    }
}
fn columns(start: VisualColumn, end: VisualColumn) -> Result<(), RectangleError> {
    column_limit(start.0)?;
    column_limit(end.0)?;
    if start > end {
        Err(RectangleError::InvalidColumns { start, end })
    } else {
        Ok(())
    }
}
/// Closed explicit repertoire; do not extend by equating all scalars to cells.
fn one_cell(ch: char) -> bool {
    matches!(ch, ' '..='~' | '\u{a0}'..='\u{ac}' | '\u{ae}'..='\u{ff}')
        || matches!(
            ch,
            '\u{20ac}'
                | '\u{201a}'
                | '\u{201e}'
                | '\u{2026}'
                | '\u{2020}'
                | '\u{2021}'
                | '\u{02c6}'
                | '\u{2030}'
                | '\u{2039}'
                | '\u{2018}'
                | '\u{2019}'
                | '\u{201c}'
                | '\u{201d}'
                | '\u{2022}'
                | '\u{2013}'
                | '\u{2014}'
                | '\u{02dc}'
                | '\u{2122}'
                | '\u{203a}'
        )
}
fn advance(column: usize, ch: char) -> Result<usize, RectangleError> {
    let width = match ch {
        '\r' | '\n' => return Err(RectangleError::LineEnding),
        '\t' => TAB_STOP - column % TAB_STOP,
        ch if one_cell(ch) => 1,
        ch => return Err(RectangleError::UnsupportedScalar(ch)),
    };
    let end = column
        .checked_add(width)
        .ok_or(RectangleError::OffsetOverflow)?;
    column_limit(end)?;
    Ok(end)
}
fn spaces(text: &mut String, count: usize) {
    text.extend(std::iter::repeat_n(' ', count));
}

impl LineLayout {
    /// Input is a single content range (no EOL/BOM in normal Workbench use).
    /// DecodedScalars preserves absolute UTF-8/legacy byte positions and validity;
    /// invalid input is rejected rather than searching/editing replacement glyphs.
    pub fn new(
        bytes: &[u8],
        encoding: TextEncoding,
        source_start: usize,
    ) -> Result<Self, RectangleError> {
        if bytes.len() > MAX_LINE_BYTES {
            return Err(RectangleError::LineTooLong(bytes.len()));
        }
        let source_end = source_start
            .checked_add(bytes.len())
            .ok_or(RectangleError::OffsetOverflow)?;
        let decoded = DecodedScalars::new(bytes, encoding, source_start);
        let source_start = decoded.content_start();
        let mut atoms = Vec::new();
        let mut width = 0;
        for scalar in decoded {
            if !scalar.valid {
                return Err(RectangleError::InvalidDecode(scalar.source_range.start));
            }
            let end = advance(width, scalar.character)?;
            atoms.push(Atom {
                bytes: scalar.source_range,
                columns: width..end,
                character: scalar.character,
            });
            width = end;
        }
        Ok(Self {
            source: source_start..source_end,
            width,
            atoms,
        })
    }
    pub fn width(&self) -> VisualColumn {
        VisualColumn(self.width)
    }
    pub fn source_range(&self) -> Range<usize> {
        self.source.clone()
    }
    pub fn column(&self, column: VisualColumn) -> Result<ColumnLocation, RectangleError> {
        column_limit(column.0)?;
        if column.0 >= self.width {
            return Ok(ColumnLocation {
                byte: self.source.end,
                tab_left: 0,
                tab_right: 0,
                virtual_spaces: column.0 - self.width,
            });
        }
        let index = self
            .atoms
            .partition_point(|atom| atom.columns.end <= column.0);
        let atom = &self.atoms[index];
        let inside = column.0 > atom.columns.start;
        Ok(ColumnLocation {
            byte: atom.bytes.start,
            tab_left: if inside {
                column.0 - atom.columns.start
            } else {
                0
            },
            tab_right: if inside {
                atom.columns.end - column.0
            } else {
                0
            },
            virtual_spaces: 0,
        })
    }
    pub fn visual_column_at_byte(&self, byte: usize) -> Result<VisualColumn, RectangleError> {
        if byte == self.source.end {
            return Ok(self.width());
        }
        if byte < self.source.start || byte > self.source.end {
            return Err(RectangleError::ByteNotBoundary(byte));
        }
        let index = self.atoms.partition_point(|atom| atom.bytes.end <= byte);
        let atom = self
            .atoms
            .get(index)
            .ok_or(RectangleError::ByteNotBoundary(byte))?;
        if byte != atom.bytes.start {
            return Err(RectangleError::ByteNotBoundary(byte));
        }
        Ok(VisualColumn(atom.columns.start))
    }
    /// TAB cells share their real byte start. Virtual padding has VIRTUAL_BYTE,
    /// explicitly presentation-only so it cannot highlight a source match at EOF.
    /// Start and width are each capped at4096; the padded presentation may end
    /// past editable column4096 to keep a caret at that boundary visible.
    pub fn expanded_visible(
        &self,
        start: VisualColumn,
        width: usize,
    ) -> Result<Vec<(char, usize)>, RectangleError> {
        self.expanded_cells(start, width, false)
    }
    /// Diagnostic marks stay in the exact same visual grid as rectangular edits.
    pub fn expanded_cells(
        &self,
        start: VisualColumn,
        width: usize,
        markers: bool,
    ) -> Result<Vec<(char, usize)>, RectangleError> {
        column_limit(start.0)?;
        column_limit(width)?;
        let end = start
            .0
            .checked_add(width)
            .ok_or(RectangleError::OffsetOverflow)?;
        let mut result = Vec::with_capacity(width);
        let mut atom_index = self
            .atoms
            .partition_point(|atom| atom.columns.end <= start.0);
        for column in start.0..end {
            while atom_index < self.atoms.len() && self.atoms[atom_index].columns.end <= column {
                atom_index += 1;
            }
            if let Some(atom) = self.atoms.get(atom_index) {
                result.push((
                    if atom.character == '\t' {
                        if markers {
                            if column == atom.columns.start {
                                '>'
                            } else {
                                '.'
                            }
                        } else {
                            ' '
                        }
                    } else if markers && atom.character == ' ' {
                        '.'
                    } else {
                        atom.character
                    },
                    atom.bytes.start,
                ));
            } else {
                result.push((if markers { '.' } else { ' ' }, VIRTUAL_BYTE));
            }
        }
        Ok(result)
    }
    /// Clipboard row normalized to cells; complete and partial TABs become
    /// spaces so pasting at a different tab phase preserves the copied width.
    pub fn copy_columns(
        &self,
        start: VisualColumn,
        end: VisualColumn,
    ) -> Result<String, RectangleError> {
        columns(start, end)?;
        let mut text = String::new();
        let first = self
            .atoms
            .partition_point(|atom| atom.columns.end <= start.0);
        for atom in self.atoms[first..]
            .iter()
            .take_while(|atom| atom.columns.start < end.0)
        {
            let from = atom.columns.start.max(start.0);
            let to = atom.columns.end.min(end.0);
            if atom.character == '\t' {
                spaces(&mut text, to - from);
            } else {
                text.push(atom.character);
            }
        }
        spaces(&mut text, end.0.saturating_sub(start.0.max(self.width)));
        Ok(text)
    }
    /// Return only the byte slice that actually changes. Whole source TABs
    /// outside the cut remain bytes in the original buffer, not reconstructed.
    pub fn replace_columns(
        &self,
        start: VisualColumn,
        end: VisualColumn,
        replacement: &str,
    ) -> Result<RowReplacement, RectangleError> {
        columns(start, end)?;
        if replacement.len() > MAX_LINE_BYTES {
            return Err(RectangleError::LineTooLong(replacement.len()));
        }
        let mut replacement_end = start.0;
        for ch in replacement.chars() {
            replacement_end = advance(replacement_end, ch)?;
        }
        let left = self.column(start)?;
        let right = self.column(end)?;
        if replacement.is_empty() && (start == end || start.0 >= self.width) {
            return Ok(RowReplacement {
                range: left.byte..left.byte,
                text: String::new(),
            });
        }
        let from = left.byte;
        let to = if right.inside_tab() {
            right.byte + 1
        } else {
            right.byte
        };
        let mut text = String::new();
        if left.inside_tab() {
            spaces(&mut text, left.tab_left);
        } else if !replacement.is_empty() {
            spaces(&mut text, left.virtual_spaces);
        }
        text.push_str(replacement);
        if right.inside_tab() {
            spaces(&mut text, right.tab_right);
        }
        // Recompute resulting columns from metadata, including preserved TABs
        // whose phase can change after an insertion. No complete source copy.
        let mut resulting = 0;
        for atom in self.atoms.iter().take_while(|a| a.bytes.end <= from) {
            resulting = advance(resulting, atom.character)?;
        }
        for ch in text.chars() {
            resulting = advance(resulting, ch)?;
        }
        for atom in self.atoms.iter().filter(|a| a.bytes.start >= to) {
            resulting = advance(resulting, atom.character)?;
        }
        // All arithmetic/source lengths stay bounded even for a legacy caller
        // that will reject encoding the proposed Unicode replacement later.
        let result_bytes = (from - self.source.start)
            .checked_add(text.len())
            .and_then(|n| n.checked_add(self.source.end - to))
            .ok_or(RectangleError::OffsetOverflow)?;
        if result_bytes > MAX_LINE_BYTES {
            return Err(RectangleError::LineTooLong(result_bytes));
        }
        Ok(RowReplacement {
            range: from..to,
            text,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn layout(text: &str) -> LineLayout {
        LineLayout::new(text.as_bytes(), TextEncoding::Utf8, 100).unwrap()
    }
    fn apply(text: &str, edit: RowReplacement) -> String {
        let mut bytes = text.as_bytes().to_vec();
        bytes.splice(
            edit.range.start - 100..edit.range.end - 100,
            edit.text.bytes(),
        );
        String::from_utf8(bytes).unwrap()
    }
    #[test]
    fn partial_tab_copy_mapping_and_replacement_preserve_unaffected_tabs() {
        let source = "a\tb\tZ";
        let line = layout(source);
        assert_eq!(line.width(), VisualColumn(9));
        assert_eq!(
            line.column(VisualColumn(2)).unwrap(),
            ColumnLocation {
                byte: 101,
                tab_left: 1,
                tab_right: 2,
                virtual_spaces: 0
            }
        );
        assert_eq!(line.column(VisualColumn(4)).unwrap().byte, 102);
        assert_eq!(
            line.copy_columns(VisualColumn(2), VisualColumn(6)).unwrap(),
            "  b "
        );
        let edit = line
            .replace_columns(VisualColumn(2), VisualColumn(3), "Q")
            .unwrap();
        assert_eq!(edit.range, 101..102);
        assert_eq!(edit.text, " Q ");
        assert_eq!(apply(source, edit), "a Q b\tZ");
        let edit = line
            .replace_columns(VisualColumn(0), VisualColumn(1), "x")
            .unwrap();
        assert_eq!(apply(source, edit), "x\tb\tZ");
        let edit = line
            .replace_columns(VisualColumn(2), VisualColumn(6), "X")
            .unwrap();
        assert_eq!(apply(source, edit), "a X  Z");
    }
    #[test]
    fn zero_width_inside_tab_preserves_left_right_cells_and_empty_insert_is_noop() {
        let line = layout("\tX");
        let edit = line
            .replace_columns(VisualColumn(2), VisualColumn(2), "q")
            .unwrap();
        assert_eq!(edit.range, 100..101);
        assert_eq!(apply("\tX", edit), "  q  X");
        let edit = line
            .replace_columns(VisualColumn(2), VisualColumn(2), "")
            .unwrap();
        assert_eq!(edit.range, 100..100);
        assert!(edit.text.is_empty());
        let edit = line
            .replace_columns(VisualColumn(0), VisualColumn(0), "a")
            .unwrap();
        assert_eq!(apply("\tX", edit), "a\tX");
        let edit = layout("abc")
            .replace_columns(VisualColumn(1), VisualColumn(2), "\t")
            .unwrap();
        assert_eq!(apply("abc", edit), "a\tc");
    }
    #[test]
    fn virtual_copy_pads_but_deletion_does_not_extend_source() {
        let line = layout("ab");
        assert_eq!(
            line.copy_columns(VisualColumn(1), VisualColumn(6)).unwrap(),
            "b    "
        );
        assert_eq!(line.column(VisualColumn(5)).unwrap().virtual_spaces, 3);
        let edit = line
            .replace_columns(VisualColumn(5), VisualColumn(7), "")
            .unwrap();
        assert_eq!(edit.range, 102..102);
        assert!(edit.text.is_empty());
        let edit = line
            .replace_columns(VisualColumn(5), VisualColumn(7), "X")
            .unwrap();
        assert_eq!(apply("ab", edit), "ab   X");
        let edit = line
            .replace_columns(VisualColumn(1), VisualColumn(7), "")
            .unwrap();
        assert_eq!(apply("ab", edit), "a");
        assert_eq!(
            line.expanded_visible(VisualColumn(1), 4).unwrap(),
            vec![
                ('b', 101),
                (' ', VIRTUAL_BYTE),
                (' ', VIRTUAL_BYTE),
                (' ', VIRTUAL_BYTE)
            ]
        );
    }
    #[test]
    fn supported_utf8_and_legacy_have_explicit_distinct_byte_coordinates() {
        let line = layout("é£\t€");
        assert_eq!(line.width(), VisualColumn(5));
        assert_eq!(line.column(VisualColumn(1)).unwrap().byte, 102);
        assert_eq!(line.visual_column_at_byte(104).unwrap(), VisualColumn(2));
        assert!(matches!(
            line.visual_column_at_byte(101),
            Err(RectangleError::ByteNotBoundary(101))
        ));
        assert_eq!(
            line.expanded_visible(VisualColumn(0), 5).unwrap(),
            vec![('é', 100), ('£', 102), (' ', 104), (' ', 104), ('€', 105)]
        );
        let legacy = LineLayout::new(b"\xe9\xa3\t\x80", TextEncoding::Windows1252, 20).unwrap();
        assert_eq!(legacy.width(), line.width());
        assert_eq!(legacy.column(VisualColumn(4)).unwrap().byte, 23);
        assert_eq!(
            legacy
                .copy_columns(VisualColumn(0), VisualColumn(5))
                .unwrap(),
            "é£  €"
        );
    }
    #[test]
    fn unsupported_widths_controls_and_invalid_decoding_fail_explicitly() {
        for text in [
            "e\u{301}",
            "界",
            "😀",
            "x\u{200d}",
            "\u{ad}",
            "\u{7f}",
            "\0",
            "\u{fffd}",
        ] {
            assert!(matches!(
                LineLayout::new(text.as_bytes(), TextEncoding::Utf8, 0),
                Err(RectangleError::UnsupportedScalar(_))
            ));
        }
        assert!(matches!(
            LineLayout::new(b"\xff", TextEncoding::Utf8, 19),
            Err(RectangleError::InvalidDecode(19))
        ));
        for text in ["x\r", "x\n"] {
            assert_eq!(
                LineLayout::new(text.as_bytes(), TextEncoding::Utf8, 0).unwrap_err(),
                RectangleError::LineEnding
            );
        }
        assert!(matches!(
            layout("abc").replace_columns(VisualColumn(0), VisualColumn(1), "😀"),
            Err(RectangleError::UnsupportedScalar('😀'))
        ));
    }
    #[test]
    fn line_columns_input_and_result_limits_do_not_overflow() {
        assert!(matches!(
            LineLayout::new(&vec![b'a'; MAX_LINE_BYTES + 1], TextEncoding::Utf8, 0),
            Err(RectangleError::LineTooLong(_))
        ));
        assert!(matches!(
            LineLayout::new(b"a", TextEncoding::Utf8, usize::MAX),
            Err(RectangleError::OffsetOverflow)
        ));
        let line = layout(&"a".repeat(MAX_COLUMNS));
        assert_eq!(line.width(), VisualColumn(MAX_COLUMNS));
        assert!(matches!(
            line.replace_columns(VisualColumn(0), VisualColumn(0), "x"),
            Err(RectangleError::ColumnLimit(_))
        ));
        assert_eq!(
            line.expanded_visible(VisualColumn(MAX_COLUMNS), 1).unwrap(),
            vec![(' ', VIRTUAL_BYTE)]
        );
        assert!(matches!(
            line.expanded_visible(VisualColumn(MAX_COLUMNS + 1), 1),
            Err(RectangleError::ColumnLimit(_))
        ));
        assert!(matches!(
            line.expanded_visible(VisualColumn(0), MAX_COLUMNS + 1),
            Err(RectangleError::ColumnLimit(_))
        ));
        assert!(matches!(
            line.copy_columns(VisualColumn(2), VisualColumn(1)),
            Err(RectangleError::InvalidColumns { .. })
        ));
        assert_eq!(
            layout("")
                .replace_columns(VisualColumn(MAX_COLUMNS), VisualColumn(MAX_COLUMNS), "")
                .unwrap()
                .text,
            ""
        );
    }
}
