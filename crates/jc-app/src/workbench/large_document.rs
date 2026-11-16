//! Preparation of a bounded initial viewport on a worker with an owned session.
pub use super::progressive_lines::LineLookup as LargeViewportLookup;
use super::progressive_lines::ProgressiveLines;
use jc_files::{FileError, FileSession};
use jc_text::{DecodedScalars, TextEncoding};
use std::{
    io,
    ops::Range,
    path::{Path, PathBuf},
};

const VIEWPORT_BYTES: usize = 64 * 1024;
const VIEWPORT_ROWS: usize = 256;
const UTF8_LOOKAHEAD: usize = 3;
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
        let source = session.document();
        if !source.is_paged() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "La preparación del archivo grande requiere una fuente paginada",
            )
            .into());
        }
        let viewport = LargeViewport::prepare_prefix(
            source.recovery_identity(),
            source.revision(),
            source.len(),
            session.encoding(),
            |range, sink| {
                source
                    .prepare_paged_read(range, VIEWPORT_BYTES + UTF8_LOOKAHEAD)
                    .map_err(io::Error::other)?
                    .visit(sink)
            },
        )?;
        Ok(Self { session, viewport })
    }

    pub fn into_parts(self) -> (FileSession, LargeViewport) {
        (self.session, self.viewport)
    }
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
    bytes: Vec<u8>,
    rows: Vec<LargeViewportRow>,
    index: ProgressiveLines,
}

impl LargeViewport {
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
        self.bytes.get(range)
    }

    pub fn indexed_through(&self) -> usize {
        self.index.indexed_through()
    }

    pub fn complete_lines(&self) -> usize {
        self.index.complete_lines()
    }

    pub fn total_lines(&self) -> Option<usize> {
        self.index.line_count()
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

    pub(crate) fn prepare_prefix(
        document_identity: u64,
        revision: u64,
        len: usize,
        encoding: Option<TextEncoding>,
        read: impl FnOnce(Range<usize>, ByteVisitor<'_>) -> io::Result<()>,
    ) -> io::Result<Self> {
        let limit = len.min(VIEWPORT_BYTES);
        let utf8 = encoding == Some(TextEncoding::Utf8);
        let read_end = if utf8 {
            len.min(limit + UTF8_LOOKAHEAD)
        } else {
            limit
        };
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(read_end)
            .map_err(|_| io::Error::new(io::ErrorKind::OutOfMemory, "Vista inicial sin memoria"))?;
        read(0..read_end, &mut |chunk| {
            if chunk.len() > read_end - bytes.len() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "La lectura de la vista excedió el tramo solicitado",
                ));
            }
            bytes.extend_from_slice(chunk);
            Ok(())
        })?;
        if bytes.len() != read_end {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "La lectura de la vista inicial quedó incompleta",
            ));
        }

        // Inspect up to three following bytes to distinguish a clipped UTF-8
        // sequence from an invalid source sequence. Neither is rewritten.
        let mut end = limit;
        let mut content_start = 0;
        if utf8 {
            let scalars = DecodedScalars::new(&bytes, TextEncoding::Utf8, 0);
            content_start = scalars.content_start();
            if read_end > limit {
                end = scalars
                    .take_while(|scalar| scalar.source_range.end <= limit)
                    .last()
                    .map_or(content_start, |scalar| scalar.source_range.end);
            }
        }
        bytes.truncate(end);

        let mut rows = Vec::new();
        rows.try_reserve_exact(VIEWPORT_ROWS)
            .map_err(|_| io::Error::new(io::ErrorKind::OutOfMemory, "Vista inicial sin memoria"))?;
        let mut index = ProgressiveLines::new(len, content_start)?;
        index.append_with_lines(&bytes[content_start..], |_, line| {
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
        Ok(Self {
            document_identity,
            revision,
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
