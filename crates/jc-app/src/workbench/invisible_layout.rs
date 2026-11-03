//! Optional ASCII-only diagnostic layout, deliberately distinct from ordinary
//! Unicode/scalar layout. Every non-ASCII scalar is an explicit escape here.
//! Cold lines are scanned in bounded fragments; sparse checkpoints resume warm
//! queries. An edit invalidates cached lines rather than rebuilding a document.
//! 128KiB bounds retained cache capacities (including metadata), not process RAM
//! or returned visible cells. A cold scan is linear in the requested line.
use jc_core::Document;
use jc_text::{DecodedScalars, TextEncoding};
use std::ops::Range;

const CACHE_BYTES: usize = 128 * 1024;
const MAX_LINES: usize = 64;
const MAX_CHECKPOINTS: usize = 2048;
const FRAGMENT: usize = 4096;
const MAX_WIDTH: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayCell {
    pub ch: char,
    pub source: Range<usize>,
    pub marker: bool,
}
#[derive(Debug, Clone, Copy)]
struct Checkpoint {
    byte: usize,
    scalar: usize,
    visual: usize,
}
#[derive(Debug)]
struct Line {
    content: Range<usize>,
    eol: Range<usize>,
    width: usize,
    checkpoints: Vec<Checkpoint>,
}
#[derive(Debug, Default)]
pub(super) struct InvisibleLayout {
    revision: Option<u64>,
    encoding: Option<TextEncoding>,
    document: usize,
    lines: Vec<Line>,
    scanned_bytes: u64,
}
impl InvisibleLayout {
    pub fn statistics(&self) -> (u64, usize) {
        (self.scanned_bytes, self.bytes())
    }
    pub fn clear(&mut self) {
        *self = Self::default();
    }
    fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.lines.capacity() * std::mem::size_of::<Line>()
            + self
                .lines
                .iter()
                .map(|line| line.checkpoints.capacity() * std::mem::size_of::<Checkpoint>())
                .sum::<usize>()
    }
    fn line(
        &mut self,
        doc: &Document,
        encoding: TextEncoding,
        content: Range<usize>,
        eol: Range<usize>,
    ) -> Option<usize> {
        if content.start > content.end
            || content.end != eol.start
            || eol.start > eol.end
            || eol.end > doc.len()
            || eol.len() > 2
        {
            return None;
        }
        eol_token(doc, &eol)?;
        let identity = doc as *const Document as usize;
        if self.revision != Some(doc.revision())
            || self.encoding != Some(encoding)
            || self.document != identity
        {
            self.clear();
            self.revision = Some(doc.revision());
            self.encoding = Some(encoding);
            self.document = identity;
        }
        if let Some(index) = self
            .lines
            .iter()
            .position(|line| line.content == content && line.eol == eol)
        {
            let line = self.lines.remove(index);
            self.lines.push(line);
            return Some(self.lines.len() - 1);
        }
        let stride = content.len().div_ceil(MAX_CHECKPOINTS - 2).max(256);
        let capacity = (content.len() / stride + 2).min(MAX_CHECKPOINTS);
        if self.lines.capacity() < MAX_LINES {
            self.lines.reserve_exact(MAX_LINES - self.lines.len());
        }
        // Reserve the incoming metadata budget before constructing it. Old
        // checkpoint allocations are dropped before the cold scan allocates.
        while !self.lines.is_empty()
            && (self.lines.len() >= MAX_LINES
                || self
                    .bytes()
                    .checked_add(capacity * std::mem::size_of::<Checkpoint>())?
                    > CACHE_BYTES)
        {
            self.lines.remove(0);
        }
        let mut checkpoints = Vec::with_capacity(capacity);
        if self
            .bytes()
            .checked_add(checkpoints.capacity() * std::mem::size_of::<Checkpoint>())?
            > CACHE_BYTES
        {
            self.clear();
            return None;
        }
        checkpoints.push(Checkpoint {
            byte: content.start,
            scalar: 0,
            visual: 0,
        });
        let mut last = content.start;
        let end = walk(
            doc,
            encoding,
            &content,
            checkpoints[0],
            &mut self.scanned_bytes,
            |span, scalar, visual| {
                if span.source.start.saturating_sub(last) >= stride
                    && checkpoints.len() < MAX_CHECKPOINTS - 1
                {
                    checkpoints.push(Checkpoint {
                        byte: span.source.start,
                        scalar,
                        visual,
                    });
                    last = span.source.start;
                }
                true
            },
        )?;
        if checkpoints.last()?.byte != content.end {
            checkpoints.push(Checkpoint {
                byte: content.end,
                scalar: end.scalar,
                visual: end.visual,
            });
        }
        let line = Line {
            content,
            eol,
            width: end.visual,
            checkpoints,
        };
        while !self.lines.is_empty()
            && (self.lines.len() >= MAX_LINES
                || self
                    .bytes()
                    .checked_add(line.checkpoints.capacity() * std::mem::size_of::<Checkpoint>())?
                    > CACHE_BYTES)
        {
            self.lines.remove(0);
        }
        if self
            .bytes()
            .checked_add(line.checkpoints.capacity() * std::mem::size_of::<Checkpoint>())?
            > CACHE_BYTES
        {
            self.clear();
            return None;
        }
        self.lines.push(line);
        Some(self.lines.len() - 1)
    }
    pub fn row(
        &mut self,
        doc: &Document,
        encoding: TextEncoding,
        content: Range<usize>,
        eol: Range<usize>,
        scroll: usize,
        width: usize,
    ) -> Option<Vec<DisplayCell>> {
        if width > MAX_WIDTH {
            return None;
        }
        let right = scroll.checked_add(width)?;
        let index = self.line(doc, encoding, content, eol)?;
        let line = &self.lines[index];
        let mut cells = Vec::with_capacity(width);
        if width == 0 {
            return Some(cells);
        }
        if scroll < line.width {
            let checkpoint = line.checkpoints[line
                .checkpoints
                .partition_point(|point| point.visual <= scroll)
                .saturating_sub(1)];
            walk(
                doc,
                encoding,
                &line.content,
                checkpoint,
                &mut self.scanned_bytes,
                |span, _, visual| {
                    append_cells(&mut cells, &span, visual, scroll, right);
                    visual.saturating_add(span.len) < right
                },
            )?;
        }
        if line.width < right {
            let token = eol_token(doc, &line.eol)?;
            if token.len > 0 {
                append_cells(&mut cells, &token, line.width, scroll, right);
            }
        }
        Some(cells)
    }
    /// Only scalar/group boundaries are valid; CRLF's interior is rejected.
    pub fn column(
        &mut self,
        doc: &Document,
        encoding: TextEncoding,
        content: Range<usize>,
        eol: Range<usize>,
        byte: usize,
    ) -> Option<usize> {
        let index = self.line(doc, encoding, content, eol)?;
        let line = &self.lines[index];
        if byte < line.content.start || byte > line.eol.end {
            return None;
        }
        if byte == line.content.end {
            return Some(line.width);
        }
        if byte > line.content.end {
            return (byte == line.eol.end)
                .then(|| {
                    eol_token(doc, &line.eol).and_then(|token| line.width.checked_add(token.len))
                })
                .flatten();
        }
        let checkpoint = line.checkpoints[line
            .checkpoints
            .partition_point(|point| point.byte <= byte)
            .saturating_sub(1)];
        let mut result = None;
        walk(
            doc,
            encoding,
            &line.content,
            checkpoint,
            &mut self.scanned_bytes,
            |span, _, visual| {
                if span.source.start == byte {
                    result = Some(visual);
                }
                span.source.end <= byte
            },
        )?;
        result
    }
    /// Visual cells in an atomic indicator snap to its source start. Beyond
    /// content (including EOL/virtual cells) maps to content.end, never CRLF+1.
    pub fn byte_at(
        &mut self,
        doc: &Document,
        encoding: TextEncoding,
        content: Range<usize>,
        eol: Range<usize>,
        column: usize,
    ) -> Option<usize> {
        let index = self.line(doc, encoding, content, eol)?;
        let line = &self.lines[index];
        if column >= line.width {
            return Some(line.content.end);
        }
        let checkpoint = line.checkpoints[line
            .checkpoints
            .partition_point(|point| point.visual <= column)
            .saturating_sub(1)];
        let mut result = None;
        walk(
            doc,
            encoding,
            &line.content,
            checkpoint,
            &mut self.scanned_bytes,
            |span, _, visual| {
                if column < visual.saturating_add(span.len) {
                    result = Some(span.source.start);
                    false
                } else {
                    true
                }
            },
        )?;
        result
    }
}

struct Span {
    source: Range<usize>,
    bytes: [u8; 16],
    len: usize,
    marker: bool,
}
impl Span {
    fn token(source: Range<usize>, token: &[u8], marker: bool) -> Self {
        let mut bytes = [0; 16];
        bytes[..token.len()].copy_from_slice(token);
        Self {
            source,
            bytes,
            len: token.len(),
            marker,
        }
    }
    fn scalar(source: Range<usize>, ch: char, visual: usize) -> Option<Self> {
        Some(match ch {
            '\t' => {
                // At least four cells, then advance to the next TAB4 stop.
                let end = visual.checked_add(4)?.checked_add(3)? / 4 * 4;
                let mut token = Self::token(source, b"\\t", true);
                token.bytes[2..end - visual].fill(b'~');
                token.len = end - visual;
                token
            }
            ' ' => Self::token(source, b"\\s", true),
            '\\' => Self::token(source, b"\\\\", true),
            '\r' => Self::token(source, b"\\r", true),
            '\n' => Self::token(source, b"\\n", true),
            '\0'..='\u{1f}' | '\u{7f}' => Self::token(source, &byte_token(ch as u8), true),
            '!'..='~' => Self::token(source, &[ch as u8], false),
            _ => {
                let mut span = Self::token(source, b"\\u{", true);
                let mut value = ch as u32;
                let mut digits = [0; 6];
                let mut count = 0;
                loop {
                    digits[count] = HEX[(value & 15) as usize];
                    count += 1;
                    value >>= 4;
                    if value == 0 {
                        break;
                    }
                }
                for digit in digits[..count].iter().rev() {
                    span.bytes[span.len] = *digit;
                    span.len += 1;
                }
                span.bytes[span.len] = b'}';
                span.len += 1;
                span
            }
        })
    }
}
const HEX: &[u8; 16] = b"0123456789ABCDEF";
pub(crate) fn escape_character(ch: char, column: usize) -> String {
    let span = Span::scalar(0..0, ch, column).expect("bounded field column");
    String::from_utf8(span.bytes[..span.len].to_vec()).expect("ASCII diagnostic notation")
}
fn byte_token(byte: u8) -> [u8; 4] {
    [
        b'\\',
        b'x',
        HEX[(byte >> 4) as usize],
        HEX[(byte & 15) as usize],
    ]
}
fn append_cells(
    cells: &mut Vec<DisplayCell>,
    span: &Span,
    visual: usize,
    left: usize,
    right: usize,
) {
    for index in left.saturating_sub(visual)..span.len.min(right.saturating_sub(visual)) {
        cells.push(DisplayCell {
            ch: span.bytes[index] as char,
            source: span.source.clone(),
            marker: span.marker,
        });
    }
}
fn load(doc: &Document, range: Range<usize>, bytes: &mut Vec<u8>, scanned: &mut u64) -> Option<()> {
    bytes.clear();
    for part in doc.range_chunks(range).ok()? {
        bytes.extend_from_slice(part);
    }
    *scanned = scanned.saturating_add(bytes.len() as u64);
    Some(())
}
fn walk(
    doc: &Document,
    encoding: TextEncoding,
    content: &Range<usize>,
    mut point: Checkpoint,
    scanned: &mut u64,
    mut visit: impl FnMut(Span, usize, usize) -> bool,
) -> Option<Checkpoint> {
    let mut bytes = Vec::with_capacity(FRAGMENT + 4);
    while point.byte < content.end {
        let end = point.byte.saturating_add(FRAGMENT).min(content.end);
        let context_end = end.saturating_add(4).min(content.end);
        let start = point.byte;
        load(doc, start..context_end, &mut bytes, scanned)?;
        // DecodedScalars normally strips absolute BOM. In diagnostic mode it
        // is an explicit span, while later literal BOM scalars follow normally.
        if start == 0 && encoding == TextEncoding::Utf8 && bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
            let token = Span::scalar(0..3, '\u{feff}', point.visual)?;
            let len = token.len;
            if !visit(token, point.scalar, point.visual) {
                return Some(point);
            }
            point.byte = 3;
            point.scalar += 1;
            point.visual = point.visual.checked_add(len)?;
            continue;
        }
        for scalar in DecodedScalars::new(&bytes, encoding, start)
            .take_while(|scalar| scalar.source_range.end <= end)
        {
            let span = if scalar.valid {
                Span::scalar(scalar.source_range.clone(), scalar.character, point.visual)?
            } else {
                let mut span = Span::token(scalar.source_range.clone(), b"", true);
                for &byte in
                    &bytes[scalar.source_range.start - start..scalar.source_range.end - start]
                {
                    let token = byte_token(byte);
                    let end = span.len.checked_add(token.len())?;
                    if end > span.bytes.len() {
                        return None;
                    }
                    span.bytes[span.len..end].copy_from_slice(&token);
                    span.len = end;
                }
                span
            };
            let len = span.len;
            if !visit(span, point.scalar, point.visual) {
                return Some(point);
            }
            point.byte = scalar.source_range.end;
            point.scalar = point.scalar.checked_add(1)?;
            point.visual = point.visual.checked_add(len)?;
        }
        if point.byte == start {
            return None;
        }
    }
    Some(point)
}
fn eol_token(doc: &Document, eol: &Range<usize>) -> Option<Span> {
    if eol.len() > 2 {
        return None;
    }
    let mut bytes = [0; 2];
    let mut filled = 0;
    for part in doc.range_chunks(eol.clone()).ok()? {
        bytes[filled..filled + part.len()].copy_from_slice(part);
        filled += part.len();
    }
    let token: &[u8] = match &bytes[..filled] {
        [] => b"",
        b"\r" => b"\\r",
        b"\n" => b"\\n",
        b"\r\n" => b"\\r\\n",
        _ => return None,
    };
    Some(Span::token(eol.clone(), token, true))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn text(cells: &[DisplayCell]) -> String {
        cells.iter().map(|cell| cell.ch).collect()
    }
    #[test]
    fn ascii_indicators_tab_stops_and_crlf_are_mapped_atomically() {
        let doc = Document::from_bytes(b"a \t\\\x01\r\n".to_vec());
        let mut layout = InvisibleLayout::default();
        let cells = layout
            .row(&doc, TextEncoding::Utf8, 0..5, 5..7, 0, 100)
            .unwrap();
        assert_eq!(text(&cells), "a\\s\\t~~~\\\\\\x01\\r\\n");
        for cell in &cells[3..8] {
            assert_eq!(cell.source, 2..3);
            assert!(cell.marker);
        }
        assert_eq!(layout.column(&doc, TextEncoding::Utf8, 0..5, 5..7, 6), None);
        for column in 3..8 {
            assert_eq!(
                layout.byte_at(&doc, TextEncoding::Utf8, 0..5, 5..7, column),
                Some(2)
            );
        }
        for cell in cells.iter().rev().take(4) {
            assert_eq!(cell.source, 5..7);
        }
        assert!(!cells[0].marker);
        assert!(cells.iter().all(|cell| cell.ch.is_ascii()));
    }
    #[test]
    fn bom_unicode_legacy_and_invalid_groups_preserve_source_ranges() {
        let doc = Document::from_bytes("\u{feff}é\u{a0}\u{200b}😀".as_bytes().to_vec());
        let mut layout = InvisibleLayout::default();
        let end = doc.len();
        let cells = layout
            .row(&doc, TextEncoding::Utf8, 0..end, end..end, 0, 200)
            .unwrap();
        assert_eq!(text(&cells), "\\u{FEFF}\\u{E9}\\u{A0}\\u{200B}\\u{1F600}");
        assert_eq!(
            layout.column(&doc, TextEncoding::Utf8, 0..end, end..end, 3),
            Some(8)
        );
        assert_eq!(
            layout.column(&doc, TextEncoding::Utf8, 0..end, end..end, 4),
            None
        );
        let legacy = Document::from_bytes(vec![0xe9, 0x81]);
        let mut layout = InvisibleLayout::default();
        assert_eq!(
            text(
                &layout
                    .row(&legacy, TextEncoding::Windows1252, 0..2, 2..2, 0, 40)
                    .unwrap()
            ),
            "\\u{E9}\\x81"
        );
        let invalid = Document::from_bytes(vec![0xe1, 0x80, b'A']);
        let mut layout = InvisibleLayout::default();
        let cells = layout
            .row(&invalid, TextEncoding::Utf8, 0..3, 3..3, 0, 40)
            .unwrap();
        assert_eq!(text(&cells), "\\xE1\\x80A");
        assert!(cells[..8].iter().all(|cell| cell.source == (0..2)));
        assert_eq!(
            layout.column(&invalid, TextEncoding::Utf8, 0..3, 3..3, 1),
            None
        );
    }
    #[test]
    fn fragment_edge_scalar_and_invalid_group_match_whole_input_decoder() {
        let mut bytes = vec![b'a'; FRAGMENT - 1];
        bytes.extend_from_slice("😀".as_bytes());
        bytes.extend_from_slice(&[0xe1, 0x80, b'Z']);
        let end = bytes.len();
        let doc = Document::from_bytes(bytes);
        let mut layout = InvisibleLayout::default();
        let row = layout
            .row(&doc, TextEncoding::Utf8, 0..end, end..end, FRAGMENT - 2, 40)
            .unwrap();
        assert_eq!(text(&row), "a\\u{1F600}\\xE1\\x80Z");
        assert_eq!(
            layout.byte_at(&doc, TextEncoding::Utf8, 0..end, end..end, FRAGMENT + 2),
            Some(FRAGMENT - 1)
        );
        assert_eq!(
            layout.byte_at(&doc, TextEncoding::Utf8, 0..end, end..end, usize::MAX),
            Some(end)
        );
    }
    #[test]
    fn sparse_warm_rows_do_not_read_the_whole_long_line_and_cache_is_bounded() {
        let doc = Document::from_bytes(vec![b'x'; 2 * 1024 * 1024]);
        let mut layout = InvisibleLayout::default();
        let end = doc.len();
        let scroll = end - 100;
        assert_eq!(
            layout
                .row(&doc, TextEncoding::Utf8, 0..end, end..end, scroll, 40)
                .unwrap()
                .len(),
            40
        );
        let cold = layout.scanned_bytes;
        let row = layout
            .row(&doc, TextEncoding::Utf8, 0..end, end..end, scroll, 40)
            .unwrap();
        assert_eq!(text(&row), "x".repeat(40));
        assert!(layout.scanned_bytes - cold < 16 * 1024);
        assert!(layout.bytes() <= CACHE_BYTES);
        assert!(layout
            .row(&doc, TextEncoding::Utf8, 0..end, end..end, 0, MAX_WIDTH + 1)
            .is_none());
        layout.clear();
        assert!(layout.lines.is_empty());
        assert!(layout.bytes() <= CACHE_BYTES);
    }
    #[test]
    fn many_line_cache_and_revision_changes_keep_metadata_bounded_and_current() {
        let mut doc = Document::from_bytes(b"a\n".repeat(200));
        let mut layout = InvisibleLayout::default();
        for line in 0..200 {
            let start = line * 2;
            assert_eq!(
                text(
                    &layout
                        .row(
                            &doc,
                            TextEncoding::Utf8,
                            start..start + 1,
                            start + 1..start + 2,
                            0,
                            8
                        )
                        .unwrap()
                ),
                "a\\n"
            );
            assert!(layout.bytes() <= CACHE_BYTES);
        }
        assert!(layout.lines.len() <= MAX_LINES);
        doc.replace(0..1, b" ").unwrap();
        assert_eq!(
            text(
                &layout
                    .row(&doc, TextEncoding::Utf8, 0..1, 1..2, 0, 8)
                    .unwrap()
            ),
            "\\s\\n"
        );
        assert_eq!(layout.lines.len(), 1);
        assert_eq!(layout.revision, Some(doc.revision()));
        assert!(layout.bytes() <= CACHE_BYTES);
    }
}
