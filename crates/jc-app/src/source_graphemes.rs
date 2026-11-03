//! Range-only UTF8 segmentation. Invalid decoder groups act as controls only in
//! temporary segmenter input; canonical bytes and legal source boundaries remain.
use jc_core::Document;
use jc_text::{
    grapheme::{GraphemeCursor, GraphemeIncomplete},
    DecodedScalars, TextEncoding,
};
use std::ops::Range;
const FRAGMENT: usize = 512;
const MAX_SCAN: usize = 16 * 1024 * 1024;
struct Chunk {
    range: Range<usize>,
    text: String,
    invalid: Vec<Range<usize>>,
}
impl Chunk {
    fn text(&self) -> &str {
        &self.text
    }
}
pub(crate) struct SourceGraphemes<'a> {
    doc: &'a Document,
    content: Range<usize>,
    cursor: GraphemeCursor,
    chunk: Option<Chunk>,
    pub scanned: usize,
}
impl<'a> SourceGraphemes<'a> {
    pub fn new(doc: &'a Document, content: Range<usize>, byte: usize) -> Option<Self> {
        if byte < content.start || byte > content.end || content.end > doc.len() {
            return None;
        }
        Some(Self {
            doc,
            cursor: GraphemeCursor::new(byte - content.start, usize::MAX / 2, true),
            content,
            chunk: None,
            scanned: 0,
        })
    }
    pub fn from_checkpoint(
        doc: &'a Document,
        content: Range<usize>,
        cursor: GraphemeCursor,
    ) -> Self {
        Self {
            doc,
            content,
            cursor,
            chunk: None,
            scanned: 0,
        }
    }
    pub fn checkpoint(&self) -> GraphemeCursor {
        self.cursor.clone()
    }
    fn load(&mut self, focus: usize) -> Result<Chunk, String> {
        let start = focus.saturating_sub(FRAGMENT / 2).max(self.content.start);
        let end = focus.saturating_add(FRAGMENT / 2).min(self.content.end);
        let raw_start = start.saturating_sub(4).max(self.content.start);
        let raw_end = end.saturating_add(4).min(self.content.end);
        self.scanned = self
            .scanned
            .checked_add(raw_end - raw_start)
            .filter(|&n| n <= MAX_SCAN)
            .ok_or("Grapheme scan exceeds16 MiB work budget")?;
        let raw = self
            .doc
            .read_range(raw_start..raw_end)
            .map_err(|e| e.to_string())?;
        let mut actual_start = None;
        let mut actual_end = start;
        let mut invalid = Vec::new();
        for scalar in DecodedScalars::new(&raw, TextEncoding::Utf8, raw_start) {
            if scalar.source_range.start < start {
                continue;
            }
            if scalar.source_range.start >= end {
                break;
            }
            actual_start.get_or_insert(scalar.source_range.start);
            actual_end = scalar.source_range.end;
            if !scalar.valid {
                invalid.push(scalar.source_range);
            }
        }
        let actual_start = actual_start.unwrap_or(start);
        if actual_start > actual_end {
            return Err("Empty grapheme context".into());
        }
        let mut bytes = raw[actual_start - raw_start..actual_end - raw_start].to_vec();
        for bad in &invalid {
            bytes[bad.start - actual_start..bad.end - actual_start].fill(0);
        }
        // Same byte length, strict UTF8, no lossy source conversion.
        // A synthetic hard newline makes line EOF a definite boundary while
        // checkpoint cursor length stays fixed across suffix insert/delete.
        if actual_end == self.content.end {
            bytes.push(b'\n');
        }
        let text = String::from_utf8(bytes).map_err(|_| "Invalid grapheme context")?;
        Ok(Chunk {
            range: actual_start..actual_end,
            text,
            invalid,
        })
    }
    fn legal(&mut self, byte: usize) -> bool {
        if byte == self.content.start || byte == self.content.end {
            return true;
        }
        if let Some(chunk) = &self.chunk {
            if byte >= chunk.range.start && byte <= chunk.range.end {
                return !chunk.invalid.iter().any(|r| byte > r.start && byte < r.end);
            }
        }
        let from = byte.saturating_sub(4).max(self.content.start);
        let end = byte.saturating_add(4).min(self.content.end);
        self.scanned = self.scanned.saturating_add(end - from);
        let Ok(raw) = self.doc.read_range(from..end) else {
            return false;
        };
        DecodedScalars::new(&raw, TextEncoding::Utf8, from)
            .any(|s| s.source_range.start == byte || s.source_range.end == byte)
    }
    pub fn adjacent(&mut self, forward: bool) -> Result<Option<usize>, String> {
        if self.content.is_empty()
            || forward && self.cursor.cur_cursor() >= self.content.len()
            || !forward && self.cursor.cur_cursor() == 0
        {
            return Ok(None);
        }
        loop {
            if self.chunk.is_none() {
                self.chunk = Some(self.load(self.content.start + self.cursor.cur_cursor())?);
            }
            let chunk = self.chunk.as_ref().unwrap();
            let result = if forward {
                self.cursor
                    .next_boundary(chunk.text(), chunk.range.start - self.content.start)
            } else {
                self.cursor
                    .prev_boundary(chunk.text(), chunk.range.start - self.content.start)
            };
            match result {
                Ok(Some(relative)) => {
                    if relative > self.content.len() {
                        return Ok(None);
                    }
                    let byte = self.content.start + relative;
                    if self.legal(byte) {
                        return Ok(Some(byte));
                    }
                }
                Ok(None) => return Ok(None),
                Err(GraphemeIncomplete::PreContext(relative)) => {
                    let byte = self.content.start + relative;
                    let context = self.load(byte.saturating_sub(1).max(self.content.start))?;
                    let end = byte
                        .checked_sub(context.range.start)
                        .filter(|&n| n <= context.text.len())
                        .ok_or("Grapheme pre-context is incomplete")?;
                    let text = context
                        .text()
                        .get(..end)
                        .ok_or("Grapheme context splits scalar")?;
                    self.cursor
                        .provide_context(text, context.range.start - self.content.start);
                }
                Err(GraphemeIncomplete::NextChunk) => {
                    let next = self.chunk.as_ref().unwrap().range.end;
                    // Retain preceding codepoints in a resumed chunk. At an
                    // exact chunk_start, RI pre-context would recount a prefix
                    // already held by the cursor; consistent overlap avoids it.
                    let chunk = self.load(
                        next.saturating_add(FRAGMENT / 2)
                            .saturating_sub(8)
                            .min(self.content.end),
                    )?;
                    if chunk.range.end <= next {
                        return Err("No forward grapheme progress".into());
                    }
                    self.chunk = Some(chunk);
                }
                Err(GraphemeIncomplete::PrevChunk) => {
                    let old = self.chunk.as_ref().unwrap().range.start;
                    let chunk =
                        self.load(old.saturating_sub(FRAGMENT / 2).max(self.content.start))?;
                    if chunk.range.start >= old {
                        return Err("No backward grapheme progress".into());
                    }
                    self.chunk = Some(chunk);
                }
                Err(GraphemeIncomplete::InvalidOffset) => {
                    return Err("Invalid grapheme source offset".into())
                }
            }
        }
    }
    pub fn visit_text<R>(
        &mut self,
        range: Range<usize>,
        visit: impl FnOnce(&[u8], bool) -> R,
    ) -> Result<R, String> {
        if let Some(chunk) = &self.chunk {
            if range.start >= chunk.range.start && range.end <= chunk.range.end {
                let valid = !chunk
                    .invalid
                    .iter()
                    .any(|r| r.start < range.end && r.end > range.start);
                if valid {
                    return Ok(visit(
                        &chunk.text.as_bytes()
                            [range.start - chunk.range.start..range.end - chunk.range.start],
                        true,
                    ));
                }
            }
        }
        self.scanned = self
            .scanned
            .checked_add(range.len())
            .filter(|&n| n <= MAX_SCAN)
            .ok_or("Grapheme scan exceeds16 MiB work budget")?;
        let raw = self.doc.read_range(range).map_err(|e| e.to_string())?;
        Ok(visit(&raw, std::str::from_utf8(&raw).is_ok()))
    }
}
