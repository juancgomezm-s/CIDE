//! Sparse line metadata for byte sources supplied in order by a worker.
use std::{io, ops::Range};

const MIN_INTERVAL_BYTES: usize = 64 * 1024;
const MAX_INTERVALS: usize = 8192;

type ByteVisitor<'a> = &'a mut dyn FnMut(&[u8]) -> io::Result<()>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineLookup<T> {
    Ready(T),
    Pending,
    OutOfRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IndexedLine {
    pub content: Range<usize>,
    pub ending: Range<usize>,
}

#[derive(Clone, Copy, Debug)]
struct Checkpoint {
    offset: usize,
    complete_lines: usize,
    line_start: usize,
    trailing_cr: bool,
}

impl Checkpoint {
    fn complete(&mut self, content_end: usize, ending_end: usize) -> IndexedLine {
        let line = IndexedLine {
            content: self.line_start..content_end,
            ending: content_end..ending_end,
        };
        self.line_start = ending_end;
        self.complete_lines += 1;
        line
    }

    fn scan(&mut self, bytes: &[u8], mut line: impl FnMut(usize, IndexedLine)) {
        for &byte in bytes {
            if self.trailing_cr {
                self.trailing_cr = false;
                let ending_end = self.offset + usize::from(byte == b'\n');
                line(
                    self.complete_lines,
                    self.complete(self.offset - 1, ending_end),
                );
                if byte == b'\n' {
                    self.offset += 1;
                    continue;
                }
            }
            match byte {
                b'\r' => self.trailing_cr = true,
                b'\n' => line(
                    self.complete_lines,
                    self.complete(self.offset, self.offset + 1),
                ),
                _ => {}
            }
            self.offset += 1;
        }
    }

    fn finish(&mut self) -> Option<(usize, IndexedLine)> {
        if !self.trailing_cr {
            return None;
        }
        self.trailing_cr = false;
        Some((
            self.complete_lines,
            self.complete(self.offset - 1, self.offset),
        ))
    }
}

/// One checkpoint per interval of source bytes, independent of lines per interval.
/// A completed index is valid only for the byte revision supplied to `append`.
/// Query callbacks must read exactly the announced range from that same revision.
#[derive(Debug)]
pub(crate) struct ProgressiveLines {
    len: usize,
    stride: usize,
    state: Checkpoint,
    checkpoints: Vec<Checkpoint>,
}

impl ProgressiveLines {
    pub(crate) fn new(len: usize, content_start: usize) -> io::Result<Self> {
        if content_start > len || len == usize::MAX {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Longitud o inicio inválido para el índice de líneas",
            ));
        }
        let content_len = len - content_start;
        let stride = MIN_INTERVAL_BYTES.max(content_len.div_ceil(MAX_INTERVALS));
        let state = Checkpoint {
            offset: content_start,
            complete_lines: 0,
            line_start: content_start,
            trailing_cr: false,
        };
        let mut checkpoints = Vec::new();
        checkpoints
            .try_reserve_exact(content_len / stride + 1)
            .map_err(|_| {
                io::Error::new(io::ErrorKind::OutOfMemory, "Índice de líneas sin memoria")
            })?;
        checkpoints.push(state);
        Ok(Self {
            len,
            stride,
            state,
            checkpoints,
        })
    }

    #[cfg(test)]
    pub(crate) fn append(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.append_with_lines(bytes, |_, _| {})
    }

    pub(crate) fn append_with_lines(
        &mut self,
        mut bytes: &[u8],
        mut on_line: impl FnMut(usize, IndexedLine),
    ) -> io::Result<()> {
        if bytes.len() > self.len - self.state.offset {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "El tramo del índice excede el documento",
            ));
        }
        while !bytes.is_empty() {
            let next = self
                .checkpoints
                .last()
                .unwrap()
                .offset
                .saturating_add(self.stride);
            let take = bytes.len().min(next - self.state.offset);
            let (head, tail) = bytes.split_at(take);
            self.state.scan(head, &mut on_line);
            if self.state.offset == self.len {
                if let Some((ordinal, line)) = self.state.finish() {
                    on_line(ordinal, line);
                }
            }
            if self.state.offset == next {
                self.checkpoints.push(self.state);
            }
            bytes = tail;
        }
        Ok(())
    }

    pub(crate) fn indexed_through(&self) -> usize {
        self.state.offset
    }

    pub(crate) fn line_count(&self) -> Option<usize> {
        (self.indexed_through() == self.len).then_some(self.state.complete_lines + 1)
    }

    pub(crate) fn complete_lines(&self) -> usize {
        self.line_count().unwrap_or(self.state.complete_lines)
    }

    /// The known prefix of the unfinished line can be displayed while the
    /// full line range is pending. A trailing CR awaits its LF lookahead.
    pub(crate) fn partial_line(&self) -> Option<(usize, Range<usize>)> {
        self.line_count().is_none().then(|| {
            (
                self.state.complete_lines,
                self.state.line_start..self.state.offset - usize::from(self.state.trailing_cr),
            )
        })
    }

    /// Resolves a complete line by reading at most one checkpoint interval.
    /// A partial line stays pending until its delimiter or EOF is indexed.
    pub(crate) fn line_range(
        &self,
        line: usize,
        visit: impl FnOnce(Range<usize>, ByteVisitor<'_>) -> io::Result<()>,
    ) -> io::Result<LineLookup<IndexedLine>> {
        if line >= self.state.complete_lines {
            return Ok(if let Some(total) = self.line_count() {
                if line == total - 1 {
                    LineLookup::Ready(IndexedLine {
                        content: self.state.line_start..self.len,
                        ending: self.len..self.len,
                    })
                } else {
                    LineLookup::OutOfRange
                }
            } else {
                LineLookup::Pending
            });
        }
        let upper = self
            .checkpoints
            .partition_point(|checkpoint| checkpoint.complete_lines <= line);
        let end = self
            .checkpoints
            .get(upper)
            .map_or(self.indexed_through(), |checkpoint| checkpoint.offset);
        let mut found = None;
        self.scan_lookup(self.checkpoints[upper - 1], end, visit, |ordinal, entry| {
            if ordinal == line {
                found = Some(entry);
            }
        })?;
        found
            .map(LineLookup::Ready)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "La línea indexada cambió"))
    }

    /// Raw byte classification. A byte inside CRLF belongs to the preceding
    /// line; validating scalar or editing boundaries remains the caller's job.
    pub(crate) fn line_at_offset(
        &self,
        offset: usize,
        visit: impl FnOnce(Range<usize>, ByteVisitor<'_>) -> io::Result<()>,
    ) -> io::Result<LineLookup<usize>> {
        if offset > self.len {
            return Ok(LineLookup::OutOfRange);
        }
        if offset == self.len && self.line_count().is_some() {
            return Ok(LineLookup::Ready(self.state.complete_lines));
        }
        if offset < self.checkpoints[0].offset {
            return Ok(LineLookup::Ready(0));
        }
        if offset >= self.indexed_through() {
            return Ok(LineLookup::Pending);
        }
        let upper = self
            .checkpoints
            .partition_point(|checkpoint| checkpoint.offset <= offset);
        let start = self.checkpoints[upper - 1];
        let mut line = start.complete_lines;
        self.scan_lookup(start, offset + 1, visit, |ordinal, entry| {
            if entry.ending.end <= offset {
                line = ordinal + 1;
            }
        })?;
        Ok(LineLookup::Ready(line))
    }

    fn scan_lookup(
        &self,
        mut state: Checkpoint,
        end: usize,
        visit: impl FnOnce(Range<usize>, ByteVisitor<'_>) -> io::Result<()>,
        mut on_line: impl FnMut(usize, IndexedLine),
    ) -> io::Result<()> {
        let range = state.offset..end;
        visit(range, &mut |bytes| {
            if bytes.len() > end - state.offset {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "La lectura de líneas excedió el tramo solicitado",
                ));
            }
            state.scan(bytes, &mut on_line);
            Ok(())
        })?;
        if state.offset != end {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "La lectura de líneas quedó incompleta",
            ));
        }
        if end == self.len {
            if let Some((ordinal, entry)) = state.finish() {
                on_line(ordinal, entry);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(
        source: &[u8],
        range: Range<usize>,
        consume: &mut dyn FnMut(&[u8]) -> io::Result<()>,
    ) -> io::Result<()> {
        for chunk in source[range].chunks(13) {
            consume(chunk)?;
        }
        Ok(())
    }

    #[test]
    fn progressive_lines_follow_split_crlf_and_a_trailing_cr() {
        let source = b"\xef\xbb\xbfabc\r\nx\ry\n\r";
        let mut index = ProgressiveLines::new(source.len(), 3).unwrap();
        index.append(&source[3..7]).unwrap();
        assert_eq!(index.indexed_through(), 7);
        assert_eq!(index.complete_lines(), 0);
        assert_eq!(index.partial_line(), Some((0, 3..6)));
        assert_eq!(index.line_count(), None);
        assert_eq!(
            index.line_range(0, |_, _| unreachable!()).unwrap(),
            LineLookup::Pending
        );
        for chunk in source[7..].chunks(1) {
            index.append(chunk).unwrap();
        }
        assert_eq!(index.line_count(), Some(5));
        assert_eq!(index.partial_line(), None);
        let lines = [
            (3..6, 6..8),
            (8..9, 9..10),
            (10..11, 11..12),
            (12..12, 12..13),
            (13..13, 13..13),
        ];
        for (line, (content, ending)) in lines.into_iter().enumerate() {
            assert_eq!(
                index
                    .line_range(line, |range, sink| read(source, range, sink))
                    .unwrap(),
                LineLookup::Ready(IndexedLine { content, ending })
            );
        }
        let ordinals = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 2, 2, 3, 4];
        for (offset, ordinal) in ordinals.into_iter().enumerate() {
            assert_eq!(
                index
                    .line_at_offset(offset, |range, sink| read(source, range, sink))
                    .unwrap(),
                LineLookup::Ready(ordinal)
            );
        }
        assert_eq!(
            index.line_range(5, |_, _| unreachable!()).unwrap(),
            LineLookup::OutOfRange
        );
    }

    #[test]
    fn sparse_metadata_bounds_lookup_for_dense_and_extreme_lines() {
        let mut source = vec![b'x'; 3 * MIN_INTERVAL_BYTES];
        source[MIN_INTERVAL_BYTES - 1] = b'\n';
        source[2 * MIN_INTERVAL_BYTES - 1] = b'\r';
        source[2 * MIN_INTERVAL_BYTES] = b'\n';
        source[2 * MIN_INTERVAL_BYTES + 1..].fill(b'\n');
        let mut index = ProgressiveLines::new(source.len(), 0).unwrap();
        for bytes in source.chunks(67) {
            index.append(bytes).unwrap();
        }
        assert_eq!(index.checkpoints.len(), 4);
        assert_eq!(index.line_count(), Some(MIN_INTERVAL_BYTES + 2));
        let bounded_read = |range: Range<usize>, sink: &mut dyn FnMut(&[u8]) -> io::Result<()>| {
            assert!(range.len() <= MIN_INTERVAL_BYTES);
            read(&source, range, sink)
        };
        assert_eq!(
            index.line_range(1, bounded_read).unwrap(),
            LineLookup::Ready(IndexedLine {
                content: MIN_INTERVAL_BYTES..2 * MIN_INTERVAL_BYTES - 1,
                ending: 2 * MIN_INTERVAL_BYTES - 1..2 * MIN_INTERVAL_BYTES + 1,
            })
        );
        assert_eq!(
            index
                .line_at_offset(2 * MIN_INTERVAL_BYTES, bounded_read)
                .unwrap(),
            LineLookup::Ready(1)
        );
        assert_eq!(
            index
                .line_at_offset(2 * MIN_INTERVAL_BYTES + 1, bounded_read)
                .unwrap(),
            LineLookup::Ready(2)
        );
        assert_eq!(
            index.line_range(0, |_, _| Ok(())).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }

    #[test]
    fn empty_index_and_invalid_input_preserve_state() {
        let mut index = ProgressiveLines::new(0, 0).unwrap();
        assert!(index.append(b"x").is_err());
        assert_eq!(index.line_count(), Some(1));
        assert_eq!(
            index.line_range(0, |_, _| unreachable!()).unwrap(),
            LineLookup::Ready(IndexedLine {
                content: 0..0,
                ending: 0..0
            })
        );
        if let Ok(gigabytes) = usize::try_from(5u64 << 30) {
            let index = ProgressiveLines::new(gigabytes, 0).unwrap();
            assert!(index.stride > MIN_INTERVAL_BYTES);
            assert!(index.checkpoints.capacity() <= MAX_INTERVALS + 1);
            assert_eq!(index.checkpoints.len(), 1);

            let mut tail = ProgressiveLines::new(gigabytes + 4, gigabytes).unwrap();
            tail.append(b"a\r").unwrap();
            tail.append(b"\nb").unwrap();
            assert_eq!(
                tail.line_range(0, |range, sink| {
                    assert_eq!(range, gigabytes..gigabytes + 4);
                    sink(b"a\r\nb")
                })
                .unwrap(),
                LineLookup::Ready(IndexedLine {
                    content: gigabytes..gigabytes + 1,
                    ending: gigabytes + 1..gigabytes + 3
                })
            );
        }
    }
}
