//! Preparation of a bounded initial viewport on a worker with an owned session.
pub use super::progressive_lines::LineLookup as LargeViewportLookup;
use super::progressive_lines::ProgressiveLines;
use crate::unicode_layout::{ascii_glyph, glyph, Glyph, VisualAtom, MAX_VISUAL_ROW_TEXT_BYTES};
use jc_core::{Document, PagedReadPlan};
use jc_files::{FileError, FileSession};
use jc_text::{grapheme, DecodedScalars, TextEncoding, TextPosition};
use std::{
    io,
    ops::Range,
    path::{Path, PathBuf},
};

const VIEWPORT_BYTES: usize = 64 * 1024;
const VIEWPORT_ROWS: usize = 256;
const UTF8_LOOKAHEAD: usize = 3;
const LOOKBEHIND: usize = 4;
const MAX_VISUAL_WIDTH: usize = 4096;
type ByteVisitor<'a> = &'a mut dyn FnMut(&[u8]) -> io::Result<()>;

/// A private snapshot and the first prepared view of that document. Dropping a
/// rejected worker result also releases its private backing.
#[derive(Debug)]
pub struct PreparedLargeDocument {
    session: FileSession,
    viewport: LargeViewport,
}

impl PreparedLargeDocument {
    /// Blocking copy and viewport admission. The caller owns worker scheduling
    /// and validates the request generation before publishing the result.
    pub fn open(
        path: PathBuf,
        encoding: Option<TextEncoding>,
        depot: &Path,
        max_bytes: u64,
        progress: impl FnMut(u64, u64) -> io::Result<()>,
    ) -> Result<Self, FileError> {
        Self::prepare(FileSession::open_private_snapshot(
            path, encoding, depot, max_bytes, progress,
        )?)
    }

    /// Blocking, also suitable for an already recovered paged session.
    pub fn prepare(session: FileSession) -> Result<Self, FileError> {
        let viewport =
            LargeViewportRequest::new(session.document(), session.encoding(), 0)?.prepare()?;
        Ok(Self { session, viewport })
    }

    pub fn into_parts(self) -> (FileSession, LargeViewport) {
        (self.session, self.viewport)
    }
}

/// A bounded source read captured for one document revision.
pub struct LargeViewportRequest {
    plan: PagedReadPlan,
    encoding: Option<TextEncoding>,
    len: usize,
    start: usize,
}

impl LargeViewportRequest {
    pub fn new(
        document: &Document,
        encoding: Option<TextEncoding>,
        start: usize,
    ) -> Result<Self, FileError> {
        if !document.is_paged() || start > document.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "El tramo de vista requiere una posición válida en un archivo paginado",
            )
            .into());
        }
        let range = viewport_read_range(document.len(), encoding, start);
        let budget = range.len();
        let plan = document
            .prepare_paged_read(range, budget)
            .map_err(io::Error::other)?;
        Ok(Self {
            plan,
            encoding,
            len: document.len(),
            start,
        })
    }

    /// Run on a worker. The result carries its source identity and revision.
    pub fn prepare(self) -> io::Result<LargeViewport> {
        LargeViewport::prepare_window(
            self.plan.document_identity(),
            self.plan.revision(),
            self.len,
            self.encoding,
            self.start,
            |_, sink| self.plan.visit(sink),
        )
    }
}

fn viewport_read_range(len: usize, encoding: Option<TextEncoding>, start: usize) -> Range<usize> {
    let lookahead = if encoding == Some(TextEncoding::Utf8) {
        UTF8_LOOKAHEAD
    } else {
        0
    };
    start.saturating_sub(LOOKBEHIND)..len.min(start.saturating_add(VIEWPORT_BYTES + lookahead))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LargeViewportRow {
    /// Absolute source bytes held by this viewport. For an unfinished line it
    /// represents the available prefix.
    pub content: Range<usize>,
    /// The empty range at EOF denotes a complete final line without an ending.
    /// `None` means more source must be indexed to find the end of this row.
    pub ending: Option<Range<usize>>,
}

/// Owned worker output. Its byte and row accessors never open or read a source.
#[derive(Debug)]
pub struct LargeViewport {
    document_identity: u64,
    revision: u64,
    origin: usize,
    encoding: Option<TextEncoding>,
    left_fragment: bool,
    preferred_ending: &'static [u8],
    bytes: Vec<u8>,
    rows: Vec<LargeViewportRow>,
    index: ProgressiveLines,
}

impl LargeViewport {
    pub fn origin(&self) -> usize {
        self.origin
    }

    pub fn matches(&self, document: &Document, encoding: Option<TextEncoding>) -> bool {
        self.document_identity == document.recovery_identity()
            && self.revision == document.revision()
            && self.encoding == encoding
    }

    pub fn line_number(&self, row: usize) -> Option<usize> {
        (self.origin == 0 && row < self.rows.len()).then_some(row + 1)
    }

    pub(super) fn first_row_is_fragment(&self) -> bool {
        self.left_fragment
    }

    pub(super) fn preferred_ending(&self) -> &'static [u8] {
        self.preferred_ending
    }

    pub fn position(&self, byte: usize) -> Option<TextPosition> {
        let upper = self.rows.partition_point(|row| row.content.start <= byte);
        let line = upper.checked_sub(1)?;
        let row = self.rows.get(line)?;
        if byte > row.content.end {
            return None;
        }
        let encoding = self.encoding?;
        let mut column = 0;
        for scalar in DecodedScalars::new(
            self.bytes(row.content.clone())?,
            encoding,
            row.content.start,
        ) {
            if scalar.source_range.start == byte {
                return Some(TextPosition { line, column });
            }
            if scalar.source_range.end > byte {
                return None;
            }
            column += 1;
        }
        Some(TextPosition { line, column })
    }

    pub fn offset(&self, position: TextPosition) -> Option<usize> {
        let row = self.rows.get(position.line)?;
        if position.column == 0 {
            return Some(row.content.start);
        }
        DecodedScalars::new(
            self.bytes(row.content.clone())?,
            self.encoding?,
            row.content.start,
        )
        .nth(position.column - 1)
        .map(|scalar| scalar.source_range.end)
    }

    fn walk_visual(&self, line: usize, mut visit: impl FnMut(VisualAtom) -> bool) -> Option<usize> {
        let row = self.rows.get(line)?;
        let bytes = self.bytes(row.content.clone())?;
        let encoding = self.encoding?;
        let mut column = 0;
        let mut cluster = |start: usize, body: &[u8]| {
            let source = row.content.start + start;
            let (glyph, mut width, marker) =
                ascii_glyph(body).unwrap_or_else(|| glyph(body, encoding, source));
            if body == b"\t" {
                width = 4 - column % 4;
            }
            let atom = VisualAtom {
                source: source..source + body.len(),
                column,
                width,
                glyph,
                marker,
            };
            column += width;
            visit(atom)
        };
        if encoding != TextEncoding::Utf8 || bytes.is_ascii() {
            for (start, byte) in bytes.iter().enumerate() {
                if !cluster(start, std::slice::from_ref(byte)) {
                    break;
                }
            }
        } else {
            let mut start = 0;
            while start < bytes.len() {
                let (valid, invalid) = match std::str::from_utf8(&bytes[start..]) {
                    Ok(text) => (text.len(), 0),
                    Err(error) => (
                        error.valid_up_to(),
                        error
                            .error_len()
                            .unwrap_or(bytes.len() - start - error.valid_up_to()),
                    ),
                };
                let text = std::str::from_utf8(&bytes[start..start + valid]).ok()?;
                for segment in grapheme::graphemes(text) {
                    if !cluster(start, segment.as_bytes()) {
                        return Some(column);
                    }
                    start += segment.len();
                }
                if invalid > 0 {
                    if !cluster(start, &bytes[start..start + invalid]) {
                        break;
                    }
                    start += invalid;
                }
            }
        }
        Some(column)
    }

    pub fn visual_row(&self, line: usize, scroll: usize, width: usize) -> Option<Vec<VisualAtom>> {
        let right = scroll.saturating_add(width.min(MAX_VISUAL_WIDTH));
        let mut result = Vec::new();
        let mut text_bytes = 0;
        self.walk_visual(line, |mut atom| {
            if atom.column >= right {
                return false;
            }
            if atom.glyph == Glyph::Scalar('\t') {
                let start = atom.column.max(scroll);
                let end = (atom.column + atom.width).min(right);
                for column in start..end {
                    result.push(VisualAtom {
                        source: atom.source.clone(),
                        column: column - scroll,
                        width: 1,
                        glyph: Glyph::Scalar(' '),
                        marker: false,
                    });
                }
            } else if atom.column >= scroll && atom.column + atom.width <= right {
                text_bytes += atom.glyph.byte_len();
                if text_bytes > MAX_VISUAL_ROW_TEXT_BYTES {
                    return false;
                }
                atom.column -= scroll;
                result.push(atom);
            }
            true
        })?;
        Some(result)
    }

    pub fn visual_byte_at(&self, line: usize, column: usize) -> Option<usize> {
        let mut offset = self.rows.get(line)?.content.end;
        self.walk_visual(line, |atom| {
            if column < atom.column + atom.width {
                offset = atom.source.start;
                return false;
            }
            true
        })?;
        Some(offset)
    }

    pub fn visual_column_at(&self, line: usize, byte: usize) -> Option<usize> {
        let row = self.rows.get(line)?;
        if byte < row.content.start || byte > row.content.end {
            return None;
        }
        let mut column = None;
        let end = self.walk_visual(line, |atom| {
            if byte < atom.source.end {
                column = Some(atom.column);
                return false;
            }
            true
        })?;
        Some(column.unwrap_or(end))
    }

    pub fn document_identity(&self) -> u64 {
        self.document_identity
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn rows(&self) -> &[LargeViewportRow] {
        &self.rows
    }

    pub fn bytes(&self, range: Range<usize>) -> Option<&[u8]> {
        self.bytes
            .get(range.start.checked_sub(self.origin)?..range.end.checked_sub(self.origin)?)
    }

    pub fn indexed_through(&self) -> usize {
        self.index.indexed_through()
    }

    pub fn complete_lines(&self) -> usize {
        self.index.complete_lines()
    }

    pub fn total_lines(&self) -> Option<usize> {
        (self.origin == 0)
            .then(|| self.index.line_count())
            .flatten()
    }

    /// Query an indexed line in the retained bytes. An unfinished row is still
    /// available from `rows`, but its full range remains pending here.
    pub fn line_range(&self, line: usize) -> io::Result<LargeViewportLookup<LargeViewportRow>> {
        if let Some(row) = self.rows.get(line) {
            return Ok(if row.ending.is_some() {
                LargeViewportLookup::Ready(row.clone())
            } else {
                LargeViewportLookup::Pending
            });
        }
        let entry = self
            .index
            .line_range(line, |range, sink| self.visit_bytes(range, sink))?;
        Ok(match entry {
            LargeViewportLookup::Ready(line) => LargeViewportLookup::Ready(LargeViewportRow {
                content: line.content,
                ending: Some(line.ending),
            }),
            LargeViewportLookup::Pending => LargeViewportLookup::Pending,
            LargeViewportLookup::OutOfRange => LargeViewportLookup::OutOfRange,
        })
    }

    /// The raw line ordinal for an indexed byte. No scalar or editing
    /// boundary validation is implied by this lookup.
    pub fn line_at_offset(&self, offset: usize) -> io::Result<LargeViewportLookup<usize>> {
        if offset < self.origin {
            return Ok(LargeViewportLookup::Pending);
        }
        self.index
            .line_at_offset(offset, |range, sink| self.visit_bytes(range, sink))
    }

    fn visit_bytes(&self, range: Range<usize>, sink: ByteVisitor<'_>) -> io::Result<()> {
        let bytes = self.bytes(range).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "El índice solicitó bytes fuera de la vista preparada",
            )
        })?;
        sink(bytes)
    }

    #[cfg(test)]
    fn prepare_prefix(
        document_identity: u64,
        revision: u64,
        len: usize,
        encoding: Option<TextEncoding>,
        read: impl FnOnce(Range<usize>, ByteVisitor<'_>) -> io::Result<()>,
    ) -> io::Result<Self> {
        Self::prepare_window(document_identity, revision, len, encoding, 0, read)
    }

    fn prepare_window(
        document_identity: u64,
        revision: u64,
        len: usize,
        encoding: Option<TextEncoding>,
        start: usize,
        read: impl FnOnce(Range<usize>, ByteVisitor<'_>) -> io::Result<()>,
    ) -> io::Result<Self> {
        let range = viewport_read_range(len, encoding, start);
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(range.len()).map_err(|_| {
            io::Error::new(io::ErrorKind::OutOfMemory, "Vista paginada sin memoria")
        })?;
        read(range.clone(), &mut |chunk| {
            if chunk.len() > range.len() - bytes.len() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "La lectura de la vista excedió el tramo solicitado",
                ));
            }
            bytes.extend_from_slice(chunk);
            Ok(())
        })?;
        if bytes.len() != range.len() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "La lectura de la vista paginada quedó incompleta",
            ));
        }

        let mut origin = start;
        let mut end = len.min(start.saturating_add(VIEWPORT_BYTES));
        let mut content_start = start;
        if encoding == Some(TextEncoding::Utf8) {
            let scalars = DecodedScalars::new(&bytes, TextEncoding::Utf8, range.start);
            let bom_end = scalars.content_start();
            if range.start == 0 && start < bom_end {
                origin = 0;
                content_start = bom_end;
            }
            let mut aligned_end = bom_end;
            for scalar in scalars.take_while(|scalar| scalar.source_range.start < end) {
                let source = scalar.source_range;
                if source.start < origin && origin < source.end {
                    origin = source.start;
                    content_start = origin;
                }
                if source.end > end {
                    break;
                }
                aligned_end = source.end;
            }
            end = aligned_end.max(content_start);
        }
        let left = origin - range.start;
        if left > 0 && bytes[left - 1] == b'\r' && bytes.get(left) == Some(&b'\n') {
            origin -= 1;
            content_start = origin;
        }
        let left_fragment = origin > 0
            && bytes
                .get((origin - range.start).wrapping_sub(1))
                .is_none_or(|byte| !matches!(byte, b'\r' | b'\n'));
        bytes.truncate(end - range.start);
        bytes.drain(..origin - range.start);

        let mut rows = Vec::new();
        rows.try_reserve_exact(VIEWPORT_ROWS)
            .map_err(|_| io::Error::new(io::ErrorKind::OutOfMemory, "Vista inicial sin memoria"))?;
        let mut index = ProgressiveLines::new(len, content_start)?;
        let mut endings = [0usize; 3];
        index.append_with_lines(&bytes[content_start - origin..], |_, line| {
            let kind = match &bytes[line.ending.start - origin..line.ending.end - origin] {
                b"\r\n" => 0,
                b"\n" => 1,
                _ => 2,
            };
            endings[kind] += 1;
            if rows.len() < VIEWPORT_ROWS {
                rows.push(LargeViewportRow {
                    content: line.content,
                    ending: Some(line.ending),
                });
            }
        })?;
        if rows.len() < VIEWPORT_ROWS {
            if let Some((_, content)) = index.partial_line() {
                rows.push(LargeViewportRow {
                    content,
                    ending: None,
                });
            } else if let Some(count) = index.line_count() {
                let line = index.line_range(count - 1, |_, _| {
                    Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "El final indexado solicitó más bytes",
                    ))
                })?;
                if let LargeViewportLookup::Ready(line) = line {
                    rows.push(LargeViewportRow {
                        content: line.content,
                        ending: Some(line.ending),
                    });
                }
            }
        }
        let preferred_ending: &'static [u8] = if endings[1] > endings[0] && endings[1] >= endings[2]
        {
            b"\n"
        } else if endings[2] > endings[0] && endings[2] > endings[1] {
            b"\r"
        } else {
            b"\r\n"
        };
        Ok(Self {
            document_identity,
            revision,
            origin,
            encoding,
            left_fragment,
            preferred_ending,
            bytes,
            rows,
            index,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(all(windows, target_pointer_width = "64"))]
    fn prepared_private_document_keeps_its_first_view_after_the_original_changes() {
        use std::{
            fs,
            time::{SystemTime, UNIX_EPOCH},
        };
        let directory = std::env::temp_dir().join(format!(
            "cide-large-view-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        fs::create_dir(&directory).unwrap();
        let path = directory.join("origen.txt");
        fs::write(&path, b"antes\r\nfila").unwrap();
        let (session, frame) =
            PreparedLargeDocument::open(path.clone(), None, &directory, 1024, |_, _| Ok(()))
                .unwrap()
                .into_parts();
        fs::write(&path, b"cambiado fuera").unwrap();
        assert_eq!(
            session.document().recovery_identity(),
            frame.document_identity()
        );
        assert_eq!(session.document().revision(), frame.revision());
        assert_eq!(
            frame.bytes(frame.rows()[0].content.clone()),
            Some(b"antes".as_slice())
        );
        assert_eq!(session.document().read_range(0..5).unwrap(), b"antes");
        drop(session);
        fs::remove_file(path).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn first_viewport_preserves_bom_mixed_endings_and_eof_without_lookups() {
        let data = b"\xef\xbb\xbfalpha\r\nbeta\r";
        let frame = LargeViewport::prepare_prefix(
            17,
            3,
            data.len(),
            Some(TextEncoding::Utf8),
            |range, sink| {
                for chunk in data[range].chunks(3) {
                    sink(chunk)?;
                }
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(frame.document_identity(), 17);
        assert_eq!(frame.revision(), 3);
        assert_eq!(frame.indexed_through(), data.len());
        assert_eq!(frame.complete_lines(), 3);
        assert_eq!(frame.total_lines(), Some(3));
        assert_eq!(frame.bytes(0..3), Some(&data[..3]));
        assert_eq!(
            frame.bytes(frame.rows()[0].content.clone()),
            Some(&b"alpha"[..])
        );
        assert_eq!(frame.rows()[0].ending, Some(8..10));
        assert_eq!(frame.rows()[1].ending, Some(14..15));
        assert_eq!(frame.rows()[2].content, 15..15);
        assert_eq!(
            frame.line_at_offset(9).unwrap(),
            LargeViewportLookup::Ready(0)
        );
        assert_eq!(
            frame.line_at_offset(10).unwrap(),
            LargeViewportLookup::Ready(1)
        );
    }

    #[test]
    fn large_viewport_bounds_reads_and_does_not_split_a_scalar() {
        let mut sample = vec![b'a'; VIEWPORT_BYTES - 1];
        sample.extend_from_slice("😀".as_bytes());
        let frame = LargeViewport::prepare_prefix(
            1,
            0,
            usize::try_from(5u64 << 30).unwrap_or(usize::MAX - 1),
            Some(TextEncoding::Utf8),
            |range, sink| {
                assert_eq!(range, 0..VIEWPORT_BYTES + UTF8_LOOKAHEAD);
                sink(&sample)
            },
        )
        .unwrap();
        assert_eq!(frame.indexed_through(), VIEWPORT_BYTES - 1);
        assert_eq!(
            frame.rows(),
            &[LargeViewportRow {
                content: 0..VIEWPORT_BYTES - 1,
                ending: None,
            }]
        );
        assert_eq!(frame.total_lines(), None);
        assert!(frame.bytes(0..VIEWPORT_BYTES).is_none());
        assert_eq!(frame.line_range(0).unwrap(), LargeViewportLookup::Pending);
        assert_eq!(
            frame.line_at_offset(VIEWPORT_BYTES + 1).unwrap(),
            LargeViewportLookup::Pending
        );
    }

    #[test]
    fn first_viewport_bounds_dense_rows_and_reports_incomplete_reads() {
        let data = vec![b'\n'; 2 * VIEWPORT_ROWS];
        let frame =
            LargeViewport::prepare_prefix(1, 0, data.len(), None, |range, sink| sink(&data[range]))
                .unwrap();
        assert_eq!(frame.rows().len(), VIEWPORT_ROWS);
        assert_eq!(frame.total_lines(), Some(data.len() + 1));
        let extra = VIEWPORT_ROWS + 5;
        assert_eq!(
            frame.line_range(extra).unwrap(),
            LargeViewportLookup::Ready(LargeViewportRow {
                content: extra..extra,
                ending: Some(extra..extra + 1),
            })
        );
        let short = LargeViewport::prepare_prefix(1, 0, 4, None, |_, sink| sink(b"abc"));
        assert_eq!(short.unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
    }
}
