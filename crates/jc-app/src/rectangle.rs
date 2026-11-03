//! Extended-grapheme geometry for transactional rectangular edits.
//! Narrow display widths and TAB4 are explicit; partial atoms become spaces. A layout owns only
//! bounded offset/column metadata, never a source String or Document. Build once
//! per visited line and reuse it; callers should process large rectangles row by
//! row rather than retaining every line layout or cloning source lines on moves.
use crate::Glyph;
use jc_text::{grapheme, DecodedScalars, TextEncoding};
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
    pub left_cells: usize,
    pub atom_end: usize,
    pub right_cells: usize,
    pub virtual_spaces: usize,
}
impl ColumnLocation {
    pub fn inside_atom(self) -> bool {
        self.left_cells != 0 && self.right_cells != 0
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
    ClusterTooLong(usize),
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
                write!(
                    f,
                    "La línea rectangular excede {MAX_LINE_BYTES} bytes ({bytes})"
                )
            }
            Self::ClusterTooLong(bytes) => write!(
                f,
                "El grafema rectangular excede {} bytes UTF8 ({bytes})",
                grapheme::MAX_CLUSTER_BYTES
            ),
            Self::ColumnLimit(column) => {
                write!(f, "La columna rectangular excede {MAX_COLUMNS} ({column})")
            }
            Self::InvalidColumns { start, end } => {
                write!(
                    f,
                    "Las columnas rectangulares están invertidas: {}..{}",
                    start.0, end.0
                )
            }
            Self::InvalidDecode(byte) => {
                write!(
                    f,
                    "El fuente rectangular tiene codificación inválida en el byte {byte}"
                )
            }
            Self::LineEnding => f.write_str("La línea/reemplazo rectangular debe excluir CR y LF"),
            Self::UnsupportedScalar(ch) => {
                write!(f, "El ancho rectangular no está admitido para {ch:?}")
            }
            Self::ByteNotBoundary(byte) => write!(
                f,
                "El byte rectangular {byte} no es una frontera escalar admitida"
            ),
            Self::OffsetOverflow => f.write_str("Desbordamiento del offset de fuente rectangular"),
        }
    }
}
impl std::error::Error for RectangleError {}

struct Atom {
    bytes: Range<usize>,
    columns: Range<usize>,
    glyph: Glyph,
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
fn advance(column: usize, text: &str) -> Result<usize, RectangleError> {
    let width = if text == "\t" {
        TAB_STOP - column % TAB_STOP
    } else {
        if let Some(ch) = text.chars().find(|ch| ch.is_control()) {
            return Err(if matches!(ch, '\r' | '\n') {
                RectangleError::LineEnding
            } else {
                RectangleError::UnsupportedScalar(ch)
            });
        }
        if text.len() > grapheme::MAX_CLUSTER_BYTES {
            return Err(RectangleError::ClusterTooLong(text.len()));
        }
        let width = grapheme::width(text);
        if width == 0 {
            return Err(RectangleError::UnsupportedScalar(
                text.chars().next().unwrap_or('\0'),
            ));
        }
        width
    };
    let end = column
        .checked_add(width)
        .ok_or(RectangleError::OffsetOverflow)?;
    column_limit(end)?;
    Ok(end)
}
fn advance_text(mut column: usize, text: &str) -> Result<usize, RectangleError> {
    for cluster in grapheme::graphemes(text) {
        column = advance(column, cluster)?;
    }
    Ok(column)
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
        let mut text = String::new();
        for scalar in decoded {
            if !scalar.valid {
                return Err(RectangleError::InvalidDecode(scalar.source_range.start));
            }
            text.push(scalar.character);
        }
        let mut atoms = Vec::new();
        let mut width = 0;
        let mut source = source_start;
        for cluster in grapheme::graphemes(&text) {
            let end = advance(width, cluster)?;
            let bytes = if encoding == TextEncoding::Utf8 {
                cluster.len()
            } else {
                cluster.chars().count()
            };
            let glyph = if cluster.chars().count() == 1 {
                Glyph::Scalar(cluster.chars().next().unwrap())
            } else {
                Glyph::Cluster(std::sync::Arc::from(cluster))
            };
            atoms.push(Atom {
                bytes: source..source + bytes,
                columns: width..end,
                glyph,
            });
            source += bytes;
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
                left_cells: 0,
                atom_end: self.source.end,
                right_cells: 0,
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
            atom_end: atom.bytes.end,
            left_cells: if inside {
                column.0 - atom.columns.start
            } else {
                0
            },
            right_cells: if inside {
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
        if let Some(atom) = self.atoms.iter().find(|a| {
            matches!(a.glyph, Glyph::Cluster(_))
                || a.glyph != Glyph::Scalar('\t') && a.columns.len() != 1
        }) {
            return Err(RectangleError::UnsupportedScalar(atom.glyph.first()));
        }
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
                    if atom.glyph == Glyph::Scalar('\t') {
                        if markers {
                            if column == atom.columns.start {
                                '>'
                            } else {
                                '.'
                            }
                        } else {
                            ' '
                        }
                    } else if markers && atom.glyph == Glyph::Scalar(' ') {
                        '.'
                    } else {
                        atom.glyph.first()
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
            if atom.glyph == Glyph::Scalar('\t')
                || from != atom.columns.start
                || to != atom.columns.end
            {
                spaces(&mut text, to - from);
            } else {
                atom.glyph.with_str(|s| text.push_str(s));
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
        advance_text(start.0, replacement)?;
        let left = self.column(start)?;
        let right = self.column(end)?;
        if replacement.is_empty() && (start == end || start.0 >= self.width) {
            return Ok(RowReplacement {
                range: left.byte..left.byte,
                text: String::new(),
            });
        }
        let from = left.byte;
        let to = if right.inside_atom() {
            right.atom_end
        } else {
            right.byte
        };
        let mut text = String::new();
        if left.inside_atom() {
            spaces(&mut text, left.left_cells);
        } else if !replacement.is_empty() {
            spaces(&mut text, left.virtual_spaces);
        }
        text.push_str(replacement);
        if right.inside_atom() {
            spaces(&mut text, right.right_cells);
        }
        // Recompute resulting columns from metadata, including preserved TABs
        // whose phase can change after an insertion. No complete source copy.
        let mut resulting = String::new();
        for atom in self.atoms.iter().take_while(|a| a.bytes.end <= from) {
            atom.glyph.with_str(|s| resulting.push_str(s));
        }
        resulting.push_str(&text);
        for atom in self.atoms.iter().filter(|a| a.bytes.start >= to) {
            atom.glyph.with_str(|s| resulting.push_str(s));
        }
        // Re-segment actual joins: an inserted ZWJ/mark may change neighboring
        // clusters. Do not sum widths of independently segmented fragments.
        advance_text(0, &resulting)?;
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
                atom_end: 102,
                left_cells: 1,
                right_cells: 2,
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
    fn zero_width_inside_atom_preserves_left_right_cells_and_empty_insert_is_noop() {
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
        for text in ["\u{301}", "\u{200d}", "\u{ad}", "\u{7f}", "\0"] {
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
        assert_eq!(
            apply(
                "abc",
                layout("abc")
                    .replace_columns(VisualColumn(0), VisualColumn(1), "😀")
                    .unwrap()
            ),
            "😀bc"
        );
        for text in ["e\u{301}", "界", "😀", "x\u{200d}", "\u{fffd}"] {
            assert!(LineLayout::new(text.as_bytes(), TextEncoding::Utf8, 0).is_ok());
        }
    }
    #[test]
    fn line_columns_input_and_result_limits_do_not_overflow() {
        let cluster = format!("e{}", "\u{301}".repeat(512));
        assert_eq!(
            LineLayout::new(cluster.as_bytes(), TextEncoding::Utf8, 0).unwrap_err(),
            RectangleError::ClusterTooLong(cluster.len())
        );
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
