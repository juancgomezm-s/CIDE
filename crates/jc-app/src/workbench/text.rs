//! Revision-aware line metadata and bounded lazy scalar layouts.
use super::line_index::{shift, LineIndex};
use jc_core::Document;
use jc_text::{
    decode_fragment, DecodedScalars, DecodedText, PositionError, TextEncoding, TextPosition,
};
use std::collections::VecDeque;
use std::ops::Range;

const CACHE_LIMIT: usize = 896 * 1024; // +128 KiB invisible spans/checkpoints =1 MiB/document.

#[derive(Debug)]
struct Layout {
    line: usize,
    scalars: usize,
    checkpoints: Vec<(usize, usize)>, // scalar column, absolute source byte
}

impl Layout {
    fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.checkpoints.capacity() * std::mem::size_of::<(usize, usize)>()
    }
}

#[derive(Debug, Default)]
pub(super) struct TextProjection {
    revision: Option<u64>,
    encoding: Option<TextEncoding>,
    len: usize,
    lines: LineIndex,
    ending_counts: [usize; 4],
    layouts: VecDeque<Layout>,
    cache_bytes: usize,
    pub decoded_bytes: u64,
    pub scanned_bytes: u64,
}

impl TextProjection {
    pub fn ensure(&mut self, document: &Document, encoding: TextEncoding) {
        if self.revision == Some(document.revision()) && self.encoding == Some(encoding) {
            return;
        }
        let bom = encoding == TextEncoding::Utf8
            && document
                .read_range(0..document.len().min(3))
                .is_ok_and(|b| b == [0xef, 0xbb, 0xbf]);
        let content_start = if bom { 3 } else { 0 };
        let mut updated = false;
        if self.encoding == Some(encoding) && self.lines.first_start() == Some(content_start) {
            if let Some(change) = document
                .last_change()
                .filter(|c| Some(c.before_revision) == self.revision)
            {
                let low = change
                    .edits
                    .iter()
                    .map(|e| e.replaced.start)
                    .min()
                    .unwrap_or(0);
                let high = change
                    .edits
                    .iter()
                    .map(|e| e.replaced.end)
                    .max()
                    .unwrap_or(0);
                let margin = change.edits.iter().fold(0usize, |n, e| {
                    n.saturating_add(e.replaced.len())
                        .saturating_add(e.inserted_len)
                });
                let high = high.saturating_add(margin).min(self.len);
                let low_line = self.lines.upper_bound(low).saturating_sub(1);
                let high_line = self.lines.upper_bound(high).saturating_sub(1);
                let candidate = if low_line == high_line {
                    self.layouts
                        .iter()
                        .position(|layout| layout.line == low_line)
                        .map(|index| {
                            let content = self.line_range(document, low_line).unwrap().0;
                            (self.layouts.remove(index).unwrap(), content)
                        })
                } else {
                    None
                };
                let mut from = low.saturating_sub(2).max(content_start);
                let next = self.lines.upper_bound(from);
                if next < self.lines.len()
                    && self.lines.start(next) == from + 1
                    && self.lines.ending(next - 1) == 2
                {
                    from -= 1;
                }
                let mut old_end = high.saturating_add(2).min(self.len);
                let next = self.lines.upper_bound(old_end);
                if next < self.lines.len()
                    && self.lines.start(next) == old_end + 1
                    && self.lines.ending(next - 1) == 2
                {
                    old_end += 1;
                }
                let new_end = shift(old_end, self.len, document.len());
                if new_end <= document.len() && from <= new_end {
                    let first = self.lines.upper_bound(from);
                    let last = self.lines.upper_bound(old_end);
                    let replacement = self.scan(document, from..new_end);
                    let added = replacement.len() - 1;
                    let removed = last - first;
                    self.lines.shift_tail(last, self.len, document.len());
                    self.layouts.retain_mut(|layout| {
                        if layout.line < low_line {
                            return true;
                        }
                        if layout.line <= high_line {
                            return false;
                        }
                        layout.line = shift(layout.line, removed, added);
                        for (_, offset) in &mut layout.checkpoints {
                            *offset = shift(*offset, self.len, document.len());
                        }
                        true
                    });
                    for line in first - 1..last - 1 {
                        self.ending_counts[self.lines.ending(line) as usize] -= 1;
                    }
                    for entry in replacement.entries().take(replacement.len() - 1) {
                        self.ending_counts[entry.ending as usize] += 1;
                    }
                    self.lines.replace_after(first..last, replacement);
                    if let Some((layout, old_content)) = candidate {
                        if added == removed {
                            let new_content =
                                self.line_range_at(layout.line, document.len()).unwrap().0;
                            if new_content.start == old_content.start
                                && new_content.end
                                    == shift(old_content.end, self.len, document.len())
                            {
                                let layout = self.update_layout(
                                    document,
                                    layout,
                                    old_content,
                                    new_content,
                                    low,
                                    high,
                                );
                                self.layouts.push_back(layout);
                            }
                        }
                    }
                    self.cache_bytes = self.layouts.iter().map(Layout::bytes).sum();
                    self.trim_cache();
                    updated = true;
                }
            }
        }
        if !updated {
            self.lines = self.scan(document, content_start..document.len());
            self.ending_counts = [0; 4];
            for entry in self.lines.entries() {
                self.ending_counts[entry.ending as usize] += 1;
            }
            self.layouts.clear();
            self.cache_bytes = 0;
        }
        if !updated
            || self.lines.capacity() > self.lines.len().saturating_mul(2).saturating_add(1024)
        {
            self.lines.shrink_to_fit();
        }
        self.revision = Some(document.revision());
        self.encoding = Some(encoding);
        self.len = document.len();
    }

    fn update_layout(
        &mut self,
        document: &Document,
        mut layout: Layout,
        old: Range<usize>,
        new: Range<usize>,
        low: usize,
        high: usize,
    ) -> Layout {
        let first = layout
            .checkpoints
            .partition_point(|&(_, byte)| byte <= low.saturating_sub(3).max(old.start))
            .saturating_sub(1);
        let last = layout
            .checkpoints
            .partition_point(|&(_, byte)| byte <= high.saturating_add(3));
        let (first_column, start) = layout.checkpoints[first];
        let (last_column, end) = layout
            .checkpoints
            .get(last)
            .copied()
            .unwrap_or((layout.scalars, old.end));
        let end = shift(end, self.len, document.len()).min(new.end);
        let decoded = self.decode(document, start..end);
        let count = decoded.source_offsets.len() - 1;
        let stride = 256usize.max(new.len().saturating_mul(32).div_ceil(CACHE_LIMIT));
        let mut checkpoints: Vec<_> = decoded
            .source_offsets
            .iter()
            .copied()
            .enumerate()
            .step_by(stride)
            .filter(|&(column, _)| column < count || (count == 0 && column == 0))
            .map(|(column, byte)| (first_column + column, byte))
            .collect();
        for (column, byte) in &mut layout.checkpoints[last..] {
            *column = *column - (last_column - first_column) + count;
            *byte = shift(*byte, self.len, document.len());
        }
        // Don't duplicate a checkpoint which is also the beginning of the tail.
        if let Some(tail) = layout.checkpoints.get(last) {
            checkpoints.retain(|&(_, byte)| byte < tail.1);
        }
        layout.checkpoints.splice(first..last, checkpoints);
        layout.scalars = layout.scalars - (last_column - first_column) + count;
        if layout.checkpoints.is_empty() {
            layout.checkpoints.push((0, new.start));
        }
        while layout.bytes() > CACHE_LIMIT / 2 && layout.checkpoints.len() > 1 {
            layout.checkpoints = layout.checkpoints.into_iter().step_by(2).collect();
            layout.checkpoints.shrink_to_fit();
        }
        layout
    }

    fn trim_cache(&mut self) {
        if self.cache_memory() > CACHE_LIMIT {
            self.layouts.shrink_to_fit();
        }
        while self.cache_memory() > CACHE_LIMIT && self.layouts.len() > 1 {
            let old = self.layouts.pop_front().unwrap();
            self.cache_bytes -= old.bytes();
            if self.layouts.capacity() * std::mem::size_of::<Layout>() > CACHE_LIMIT / 2 {
                self.layouts.shrink_to_fit();
            }
        }
    }

    fn scan(&mut self, document: &Document, range: Range<usize>) -> LineIndex {
        self.scanned_bytes += range.len() as u64;
        let mut result = LineIndex::default();
        let mut start = range.start;
        let mut bytes = document
            .range_chunks(range.clone())
            .expect("validated index range")
            .flatten()
            .copied()
            .enumerate()
            .peekable();
        while let Some((index, byte)) = bytes.next() {
            let mut end = range.start + index + 1;
            let ending = if byte == b'\r' {
                if bytes.peek().is_some_and(|(_, b)| *b == b'\n') {
                    bytes.next();
                    end += 1;
                    2
                } else {
                    3
                }
            } else if byte == b'\n' {
                1
            } else {
                continue;
            };
            result.push(start, ending);
            start = end;
        }
        result.finish(start);
        result
    }

    pub fn content_start(&self) -> usize {
        self.lines.start(0)
    }
    pub fn preferred_ending(&self) -> &'static [u8] {
        let [_, lf, crlf, cr] = self.ending_counts;
        if crlf >= lf && crlf >= cr && crlf > 0 {
            b"\r\n"
        } else if lf >= cr && lf > 0 {
            b"\n"
        } else if cr > 0 {
            b"\r"
        } else {
            b"\r\n"
        }
    }
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Raw byte classification only; this does not validate scalar/CRLF boundaries.
    /// The BOM, when present, belongs to line zero but is never a text caret.
    pub fn line_ordinal_for_byte(&self, offset: usize) -> Option<usize> {
        (offset <= self.len).then(|| {
            self.lines
                .upper_bound(offset.max(self.content_start()))
                .saturating_sub(1)
        })
    }

    pub fn line_range(
        &self,
        _document: &Document,
        line: usize,
    ) -> Option<(Range<usize>, Range<usize>)> {
        self.line_range_at(line, self.len)
    }

    fn line_range_at(&self, line: usize, len: usize) -> Option<(Range<usize>, Range<usize>)> {
        let start = self.lines.get(line)?.start;
        let end = self.lines.get(line + 1).map_or(len, |entry| entry.start);
        let ending = match self.lines.ending(line) {
            0 => 0,
            2 => 2,
            _ => 1,
        };
        Some((start..end - ending, end - ending..end))
    }

    fn decode(&mut self, document: &Document, range: Range<usize>) -> DecodedText {
        let encoding = self.encoding.expect("initialized projection");
        if range.is_empty() {
            return decode_fragment(&[], encoding, range.start);
        }
        // Lookahead preserves the decoder's invalid-sequence grouping at a range edge.
        let end = range.end.saturating_add(3).min(document.len());
        self.decoded_bytes += (end - range.start) as u64;
        let mut decoded = decode_fragment(
            &document
                .read_range(range.start..end)
                .expect("indexed source range"),
            encoding,
            range.start,
        );
        let count = decoded
            .source_offsets
            .binary_search(&range.end)
            .expect("text boundary at range end");
        let text_end = decoded
            .text
            .char_indices()
            .nth(count)
            .map_or(decoded.text.len(), |(offset, _)| offset);
        decoded.text.truncate(text_end);
        decoded.source_offsets.truncate(count + 1);
        decoded
            .issues
            .retain(|issue| issue.source_range.end <= range.end);
        decoded
    }

    fn layout(&mut self, document: &Document, line: usize) -> Option<&Layout> {
        if let Some(index) = self.layouts.iter().position(|l| l.line == line) {
            let layout = self.layouts.remove(index).unwrap();
            self.layouts.push_back(layout);
            return self.layouts.back();
        }
        let range = self.line_range(document, line)?.0;
        // Build sparse checkpoints directly from bounded source ranges. A cold
        // long line must not materialize a String and one offset per scalar.
        let stride = 256usize.max(range.len().saturating_mul(32).div_ceil(CACHE_LIMIT));
        let mut checkpoints = Vec::with_capacity(range.len() / stride + 1);
        let mut bytes = Vec::with_capacity(range.len().min(64 * 1024) + 4);
        let mut cursor = range.start;
        let mut scalars = 0;
        while cursor < range.end {
            let end = (cursor + 64 * 1024).min(range.end);
            let context_end = (end + 4).min(document.len());
            bytes.clear();
            for chunk in document.range_chunks(cursor..context_end).ok()? {
                bytes.extend_from_slice(chunk);
            }
            self.decoded_bytes += bytes.len() as u64;
            for scalar in DecodedScalars::new(&bytes, self.encoding?, cursor)
                .take_while(|s| s.source_range.end <= end)
            {
                if scalars % stride == 0 {
                    checkpoints.push((scalars, scalar.source_range.start));
                }
                scalars += 1;
                cursor = scalar.source_range.end;
            }
        }
        if scalars % stride == 0 {
            checkpoints.push((scalars, range.end));
        }
        let layout = Layout {
            line,
            scalars,
            checkpoints,
        };
        let bytes = layout.bytes();
        while self.cache_bytes + bytes > CACHE_LIMIT {
            let Some(old) = self.layouts.pop_front() else {
                break;
            };
            self.cache_bytes -= old.bytes();
        }
        self.cache_bytes += bytes;
        self.layouts.push_back(layout);
        self.trim_cache();
        self.layouts.back()
    }

    pub fn line_len(&mut self, document: &Document, line: usize) -> Option<usize> {
        Some(self.layout(document, line)?.scalars)
    }

    pub fn position(
        &mut self,
        document: &Document,
        offset: usize,
    ) -> Result<TextPosition, PositionError> {
        if offset < self.lines.start(0) || offset > self.len {
            return Err(PositionError::SourceOffsetNotBoundary(offset));
        }
        let line = self.lines.upper_bound(offset).saturating_sub(1);
        let content = self.line_range(document, line).unwrap().0;
        if offset > content.end {
            return Err(PositionError::InsideLineEnding(offset));
        }
        let layout = self.layout(document, line).unwrap();
        let i = layout
            .checkpoints
            .partition_point(|&(_, byte)| byte <= offset)
            .saturating_sub(1);
        let (column, start) = layout.checkpoints[i];
        let end = layout
            .checkpoints
            .get(i + 1)
            .map_or(content.end, |&(_, byte)| byte);
        let decoded = self.decode(document, start..end);
        let relative = decoded
            .source_offsets
            .binary_search(&offset)
            .map_err(|_| PositionError::SourceOffsetNotBoundary(offset))?;
        Ok(TextPosition {
            line,
            column: column + relative,
        })
    }

    pub fn offset(
        &mut self,
        document: &Document,
        position: TextPosition,
    ) -> Result<usize, PositionError> {
        let content = self
            .line_range(document, position.line)
            .ok_or(PositionError::LineOutOfRange(position.line))?
            .0;
        let layout = self.layout(document, position.line).unwrap();
        if position.column > layout.scalars {
            return Err(PositionError::ColumnOutOfRange {
                line: position.line,
                column: position.column,
            });
        }
        let i = layout
            .checkpoints
            .partition_point(|&(column, _)| column <= position.column)
            .saturating_sub(1);
        let (column, start) = layout.checkpoints[i];
        let end = layout
            .checkpoints
            .get(i + 1)
            .map_or(content.end, |&(_, byte)| byte);
        let decoded = self.decode(document, start..end);
        Ok(decoded.source_offsets[position.column - column])
    }

    pub fn snap(&mut self, document: &Document, offset: usize) -> usize {
        let offset = offset.clamp(self.lines.start(0), self.len);
        let line = self.lines.upper_bound(offset).saturating_sub(1);
        let content = self.line_range(document, line).unwrap().0;
        let offset = offset.min(content.end);
        let layout = self.layout(document, line).unwrap();
        let i = layout
            .checkpoints
            .partition_point(|&(_, byte)| byte <= offset)
            .saturating_sub(1);
        let start = layout.checkpoints[i].1;
        let end = layout
            .checkpoints
            .get(i + 1)
            .map_or(content.end, |&(_, byte)| byte);
        let decoded = self.decode(document, start..end);
        decoded.source_offsets[decoded
            .source_offsets
            .partition_point(|&n| n <= offset)
            .saturating_sub(1)]
    }

    pub fn adjacent(
        &mut self,
        document: &Document,
        offset: usize,
        forward: bool,
    ) -> Result<Option<usize>, PositionError> {
        let pos = self.position(document, offset)?;
        let target = if forward {
            if pos.column < self.line_len(document, pos.line).unwrap() {
                TextPosition {
                    line: pos.line,
                    column: pos.column + 1,
                }
            } else if pos.line + 1 < self.line_count() {
                TextPosition {
                    line: pos.line + 1,
                    column: 0,
                }
            } else {
                return Ok(None);
            }
        } else if pos.column > 0 {
            TextPosition {
                line: pos.line,
                column: pos.column - 1,
            }
        } else if pos.line > 0 {
            TextPosition {
                line: pos.line - 1,
                column: self.line_len(document, pos.line - 1).unwrap(),
            }
        } else {
            return Ok(None);
        };
        self.offset(document, target).map(Some)
    }

    pub fn visible(
        &mut self,
        document: &Document,
        line: usize,
        column: usize,
        width: usize,
    ) -> Option<Vec<(char, usize)>> {
        let len = self.line_len(document, line)?;
        let start = self
            .offset(
                document,
                TextPosition {
                    line,
                    column: column.min(len),
                },
            )
            .ok()?;
        let end = self
            .offset(
                document,
                TextPosition {
                    line,
                    column: column.saturating_add(width).min(len),
                },
            )
            .ok()?;
        let decoded = self.decode(document, start..end);
        Some(decoded.text.chars().zip(decoded.source_offsets).collect())
    }

    fn cache_memory(&self) -> usize {
        self.cache_bytes
            + (self.layouts.capacity() - self.layouts.len()) * std::mem::size_of::<Layout>()
    }
    pub fn memory_bytes(&self) -> usize {
        self.lines.memory_bytes() + self.cache_memory()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jc_core::{EditBatch, Replacement};
    use jc_text::{decode, TextIndex};

    #[test]
    fn cold_sparse_layout_preserves_chunk_edge_scalars_invalid_groups_and_eof() {
        for shift in 0..4 {
            let mut bytes = vec![b'a'; 64 * 1024 - shift];
            bytes.extend_from_slice("😀é\u{feff}".as_bytes());
            bytes.extend_from_slice(b"\xe1\x80x\xf0\x9f");
            let doc = Document::from_bytes(bytes.clone());
            let reference = TextIndex::new(&decode(&bytes, TextEncoding::Utf8));
            let mut projection = TextProjection::default();
            projection.ensure(&doc, TextEncoding::Utf8);
            assert_eq!(projection.line_len(&doc, 0), reference.line_len(0));
            for offset in (64 * 1024 - 8)..=bytes.len() {
                assert_eq!(
                    projection.position(&doc, offset),
                    reference.source_byte_to_position(offset)
                );
            }
            for column in (64 * 1024 - 8)..=reference.line_len(0).unwrap() {
                let position = TextPosition { line: 0, column };
                assert_eq!(
                    projection.offset(&doc, position),
                    reference.position_to_source_byte(position)
                );
            }
        }
    }

    #[test]
    fn layouts_remain_within_quota_after_visiting_many_different_lines() {
        let doc = Document::from_bytes(b"a\n".repeat(20000));
        let mut projection = TextProjection::default();
        projection.ensure(&doc, TextEncoding::Utf8);
        for line in 0..20000 {
            assert_eq!(projection.line_len(&doc, line), Some(1));
        }
        assert!(projection.cache_memory() <= CACHE_LIMIT);
        assert!(projection.layouts.len() < 20000);
        assert_eq!(projection.line_len(&doc, 0), Some(1));
        assert!(projection.cache_memory() <= CACHE_LIMIT);
    }

    fn compare(document: &Document, projection: &mut TextProjection, encoding: TextEncoding) {
        projection.ensure(document, encoding);
        let decoded = decode(&document.to_bytes(), encoding);
        let reference = TextIndex::new(&decoded);
        assert_eq!(projection.line_count(), reference.line_count());
        assert_eq!(
            projection.preferred_ending(),
            jc_text::LineEndingInfo::inspect(&document.to_bytes()).preferred_bytes()
        );
        for offset in 0..=document.len() {
            assert_eq!(
                projection.position(document, offset),
                reference.source_byte_to_position(offset),
                "offset {offset}"
            );
        }
        for line in 0..reference.line_count() {
            assert_eq!(
                projection.line_len(document, line),
                reference.line_len(line)
            );
            for column in 0..=reference.line_len(line).unwrap() {
                let position = TextPosition { line, column };
                assert_eq!(
                    projection.offset(document, position),
                    reference.position_to_source_byte(position)
                );
            }
        }
    }

    #[test]
    fn incremental_projection_matches_full_decoder_through_byte_edits_undo_and_redo() {
        for encoding in [
            TextEncoding::Utf8,
            TextEncoding::Iso8859_1,
            TextEncoding::Windows1252,
            TextEncoding::Ibm850,
        ] {
            let mut doc =
                Document::from_bytes(b"\xef\xbb\xbfA\r\n\xf0\x9f\x98\nB\rC\n\xff\0".to_vec());
            let mut projection = TextProjection::default();
            compare(&doc, &mut projection, encoding);
            let mut seed = 7u64;
            for i in 0..160 {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                let start = seed as usize % (doc.len() + 1);
                let end = (start + (i % 5)).min(doc.len());
                let bytes = match i % 5 {
                    0 => b"\r\n".to_vec(),
                    1 => b"\xf0\x9f\x98".to_vec(),
                    2 => b"\xc3\xa9".to_vec(),
                    3 => vec![],
                    _ => vec![i as u8],
                };
                doc.apply_batch(EditBatch {
                    expected_revision: doc.revision(),
                    edits: vec![Replacement {
                        range: start..end,
                        bytes,
                    }],
                })
                .unwrap();
                compare(&doc, &mut projection, encoding);
                if i % 4 == 0 {
                    doc.undo();
                    compare(&doc, &mut projection, encoding);
                    doc.redo();
                    compare(&doc, &mut projection, encoding);
                }
            }
        }
    }

    #[test]
    fn sparse_layout_bounds_warm_reads_even_for_a_long_line_and_preserves_literal_bom() {
        let source = format!("{}\u{feff}é\r\nend", "😀".repeat(300000));
        let doc = Document::from_bytes(source.into_bytes());
        let mut projection = TextProjection::default();
        projection.ensure(&doc, TextEncoding::Utf8);
        assert_eq!(projection.line_len(&doc, 0), Some(300002));
        let before = projection.decoded_bytes;
        let offset = projection
            .offset(
                &doc,
                TextPosition {
                    line: 0,
                    column: 300000,
                },
            )
            .unwrap();
        assert_eq!(
            projection
                .visible(&doc, 0, 300000, 2)
                .unwrap()
                .iter()
                .map(|p| p.0)
                .collect::<String>(),
            "\u{feff}é"
        );
        assert_eq!(
            projection.position(&doc, offset).unwrap(),
            TextPosition {
                line: 0,
                column: 300000
            }
        );
        assert!(projection.decoded_bytes - before < 20000);
        assert!(projection.cache_memory() <= CACHE_LIMIT);
    }

    #[test]
    fn local_changes_update_only_a_source_neighborhood_and_shift_the_tail() {
        let mut doc = Document::from_bytes(b"abcdef\r\n".repeat(5000));
        let mut projection = TextProjection::default();
        projection.ensure(&doc, TextEncoding::Utf8);
        let scanned = projection.scanned_bytes;
        doc.replace(20002..20005, b"x\n\r\ny").unwrap();
        projection.ensure(&doc, TextEncoding::Utf8);
        assert!(projection.scanned_bytes - scanned < 128);
        compare(&doc, &mut projection, TextEncoding::Utf8);
        doc.undo();
        compare(&doc, &mut projection, TextEncoding::Utf8);
        doc.redo();
        compare(&doc, &mut projection, TextEncoding::Utf8);
    }

    #[test]
    fn changing_another_line_preserves_cached_layout_of_an_unchanged_long_line() {
        let mut bytes = vec![b'a'; 1_000_000];
        bytes.extend_from_slice(&b"\r\nabcdef\n".repeat(30));
        let mut doc = Document::from_bytes(bytes);
        let mut projection = TextProjection::default();
        projection.ensure(&doc, TextEncoding::Utf8);
        assert_eq!(projection.line_len(&doc, 0), Some(1_000_000));
        let before = projection.decoded_bytes;
        doc.insert(doc.len() - 3, b"x").unwrap();
        projection.ensure(&doc, TextEncoding::Utf8);
        assert_eq!(
            projection.position(&doc, 500000).unwrap(),
            TextPosition {
                line: 0,
                column: 500000
            }
        );
        assert!(projection.decoded_bytes - before < 4096);
    }

    #[test]
    fn sparse_checkpoints_update_after_invalid_utf8_changes_and_multirange_transactions() {
        let mut doc = Document::from_bytes("aé😀\u{feff}".repeat(700).into_bytes());
        let mut projection = TextProjection::default();
        compare(&doc, &mut projection, TextEncoding::Utf8);
        for offset in [257, 1001, 3072, 5003] {
            doc.replace(offset..offset + 2, b"\xffx").unwrap();
            compare(&doc, &mut projection, TextEncoding::Utf8);
            doc.undo();
            compare(&doc, &mut projection, TextEncoding::Utf8);
            doc.redo();
            compare(&doc, &mut projection, TextEncoding::Utf8);
        }
        doc.apply_batch(EditBatch {
            expected_revision: doc.revision(),
            edits: vec![
                Replacement {
                    range: 1..2,
                    bytes: vec![255],
                },
                Replacement {
                    range: 4500..4503,
                    bytes: "é".as_bytes().to_vec(),
                },
                Replacement {
                    range: 6000..6000,
                    bytes: "😀".as_bytes().to_vec(),
                },
            ],
        })
        .unwrap();
        compare(&doc, &mut projection, TextEncoding::Utf8);
        doc.undo();
        compare(&doc, &mut projection, TextEncoding::Utf8);
    }
}
