//! One generic, bounded lexical cache per document. No provider or frontend
//! owns another buffer. Unknown context is Plain until resumable scanning ends.
use super::Workbench;
use crate::TabId;
use jc_core::{ChangeSet, Document};
pub use jc_languages::syntax::SyntaxKind;
use jc_languages::syntax::{scan, LexerState, SyntaxProfile, TokenSpan};
use jc_text::TextEncoding;
use std::{
    ops::Range,
    path::{Path, PathBuf},
};

const CACHE_LIMIT: usize = 128 * 1024;
const WORK_LIMIT: usize = 64 * 1024;
const CHUNK: usize = 4096;
const MAX_ROWS: usize = 128;
const MAX_CHECKPOINTS: usize = 256;
const MAX_TOKENS: usize = 1536;
const MAX_VISIBLE_BYTES: usize = 256 * 1024;
const MAX_PATH: usize = 4096;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SyntaxStats {
    pub processed_bytes: usize,
    pub cache_bytes: usize,
    pub scanned_to: usize,
}
#[derive(Debug)]
pub struct SyntaxRow {
    pub line: usize,
    pub source: Range<usize>,
    pub spans: Vec<TokenSpan>,
    run_links: Vec<(Range<usize>, Range<usize>)>,
}
#[derive(Debug, Default)]
pub struct SyntaxRows {
    pub rows: Vec<SyntaxRow>,
    pub pending: bool,
    pub limited: bool,
    pub stats: SyntaxStats,
    pub revision: u64,
    pub profile: SyntaxProfile,
    pub encoding: Option<TextEncoding>,
}
impl SyntaxRows {
    pub fn kind_at(&self, byte: usize) -> SyntaxKind {
        if self.pending || self.limited || byte == usize::MAX {
            return SyntaxKind::Plain;
        }
        let row = self.rows.partition_point(|row| row.source.end <= byte);
        let Some(row) = self.rows.get(row).filter(|row| row.source.contains(&byte)) else {
            return SyntaxKind::Plain;
        };
        if row
            .run_links
            .iter()
            .any(|(marker, operand)| !marker.is_empty() && operand.contains(&byte))
        {
            return SyntaxKind::Link;
        }
        let token = row.spans.partition_point(|token| token.source.end <= byte);
        row.spans
            .get(token)
            .filter(|token| token.source.contains(&byte))
            .map_or(SyntaxKind::Plain, |token| token.kind)
    }
}
#[derive(Debug, Clone, Copy)]
struct Checkpoint {
    byte: usize,
    state: LexerState,
}
#[derive(Debug, Default)]
pub(super) struct SyntaxCache {
    revision: Option<u64>,
    encoding: Option<TextEncoding>,
    profile: SyntaxProfile,
    path: PathBuf,
    checkpoints: Vec<Checkpoint>,
    tokens: Vec<TokenSpan>,
    stride: usize,
    window: Option<Range<usize>>,
    cursor: usize,
    state: LexerState,
    complete: bool,
    limited: bool,
}
impl SyntaxCache {
    pub fn clear(&mut self) {
        *self = Self::default();
    }
    pub fn memory_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.path.capacity()
            + self.checkpoints.capacity() * std::mem::size_of::<Checkpoint>()
            + self.tokens.capacity() * std::mem::size_of::<TokenSpan>()
    }
    /// Inverse transactions use the same rule. Missing links/ABA notifications
    /// cannot reuse a suffix: clear when either revision does not match exactly.
    pub fn invalidate(&mut self, change: &ChangeSet, new_revision: u64) {
        if self.revision != Some(change.before_revision) || change.after_revision != new_revision {
            self.clear();
            return;
        }
        let first = change
            .edits
            .iter()
            .map(|edit| edit.replaced.start)
            .min()
            .unwrap_or(0);
        // Strictly before the edit: an EOF-finalized word/quote can change its
        // classification when text is appended at that exact byte boundary.
        self.checkpoints
            .retain(|point| point.byte == 0 || point.byte < first);
        self.tokens.clear();
        self.window = None;
        self.complete = false;
        self.limited = false;
        self.revision = Some(new_revision);
        let point = self.checkpoints.last().copied().unwrap_or(Checkpoint {
            byte: 0,
            state: LexerState::default(),
        });
        self.cursor = point.byte;
        self.state = point.state;
    }
    fn ensure(
        &mut self,
        doc: &Document,
        path: &Path,
        profile: SyntaxProfile,
        encoding: TextEncoding,
    ) -> bool {
        if self.revision == Some(doc.revision())
            && self.encoding == Some(encoding)
            && self.profile == profile
            && self.path == path
        {
            return true;
        }
        self.clear();
        if path.as_os_str().len() > MAX_PATH {
            return false;
        }
        self.revision = Some(doc.revision());
        self.encoding = Some(encoding);
        self.profile = profile;
        self.path = path.to_owned();
        self.stride = CHUNK;
        self.checkpoints = Vec::with_capacity(MAX_CHECKPOINTS);
        self.tokens = Vec::with_capacity(MAX_TOKENS);
        self.checkpoints.push(Checkpoint {
            byte: 0,
            state: LexerState::default(),
        });
        if self.memory_bytes() > CACHE_LIMIT {
            self.clear();
            return false;
        }
        true
    }
    fn checkpoint(&mut self, force: bool) {
        if self.checkpoints.last().is_some_and(|last| {
            last.byte >= self.cursor
                || (!force && self.cursor.saturating_sub(last.byte) < self.stride)
        }) {
            return;
        }
        if self.checkpoints.len() == MAX_CHECKPOINTS {
            let mut index = 0;
            self.checkpoints.retain(|_| {
                let keep = index % 2 == 0;
                index += 1;
                keep
            });
            self.stride = self.stride.saturating_mul(2);
        }
        self.checkpoints.push(Checkpoint {
            byte: self.cursor,
            state: self.state,
        });
    }
    fn advance(&mut self, doc: &Document, window: &Range<usize>, encoding: TextEncoding) -> usize {
        let mut processed = 0;
        while self.cursor < window.end && processed < WORK_LIMIT && !self.limited {
            if self.cursor == 0 && encoding == TextEncoding::Utf8 && doc.len() >= 3 {
                // BOM is layout metadata, never the beginning of an identifier.
                let Ok(prefix) = doc.read_range(0..3) else {
                    self.limited = true;
                    break;
                };
                processed += 3;
                if prefix == [0xef, 0xbb, 0xbf] {
                    self.cursor = 3;
                    self.checkpoint(false);
                    continue;
                }
            }
            let end = self
                .cursor
                .saturating_add(CHUNK.min(WORK_LIMIT - processed))
                .min(window.end);
            let start = self.cursor;
            let mut offset = start;
            let mut state = self.state;
            let Ok(parts) = doc.range_chunks(start..end) else {
                self.limited = true;
                break;
            };
            let tokens = &mut self.tokens;
            let limited = &mut self.limited;
            for bytes in parts {
                let final_chunk = offset + bytes.len() == doc.len();
                scan(
                    self.profile,
                    &mut state,
                    bytes,
                    offset,
                    final_chunk,
                    |token| {
                        if *limited || token.kind == SyntaxKind::Plain {
                            return;
                        }
                        let source =
                            token.source.start.max(window.start)..token.source.end.min(window.end);
                        if source.start >= source.end {
                            return;
                        }
                        if let Some(last) = tokens.last_mut().filter(|last| {
                            last.kind == token.kind && last.source.end == source.start
                        }) {
                            last.source.end = source.end;
                            return;
                        }
                        if tokens.len() >= MAX_TOKENS {
                            *limited = true;
                            tokens.clear();
                            return;
                        }
                        tokens.push(TokenSpan {
                            source,
                            kind: token.kind,
                        });
                    },
                );
                offset += bytes.len();
            }
            self.state = state;
            self.cursor = end;
            processed += end - start;
            if state.limited() {
                self.limited = true;
                self.tokens.clear();
            }
            self.checkpoint(self.limited);
        }
        self.complete = self.cursor >= window.end || self.limited;
        processed
    }
    fn rows(
        &mut self,
        doc: &Document,
        path: &Path,
        profile: SyntaxProfile,
        encoding: Option<TextEncoding>,
        ranges: Vec<(usize, Range<usize>)>,
    ) -> SyntaxRows {
        let mut result = SyntaxRows {
            rows: Vec::with_capacity(ranges.len()),
            pending: false,
            limited: false,
            stats: SyntaxStats::default(),
            revision: doc.revision(),
            profile,
            encoding,
        };
        let range = ranges.first().map_or(0, |(_, range)| range.start)
            ..ranges.last().map_or(0, |(_, range)| range.end);
        match encoding.filter(|_| profile != SyntaxProfile::Plain) {
            None => self.clear(),
            Some(encoding) => {
                if range.len() > MAX_VISIBLE_BYTES || !self.ensure(doc, path, profile, encoding) {
                    result.limited = true;
                } else {
                    if self.window.as_ref() != Some(&range) {
                        self.window = Some(range.clone());
                        self.tokens.clear();
                        self.complete = false;
                        self.limited = false;
                        let checkpoint = self.checkpoints[self
                            .checkpoints
                            .partition_point(|point| point.byte <= range.start)
                            .saturating_sub(1)];
                        self.cursor = checkpoint.byte;
                        self.state = checkpoint.state;
                    }
                    if !self.complete {
                        result.stats.processed_bytes = self.advance(doc, &range, encoding);
                    }
                    result.pending = !self.complete;
                    result.limited = self.limited;
                }
            }
        }
        if self.memory_bytes() > CACHE_LIMIT {
            self.clear();
            result.pending = false;
            result.limited = true;
        }
        for (line, source) in ranges {
            let mut spans = Vec::new();
            if !result.pending
                && !result.limited
                && profile != SyntaxProfile::Plain
                && encoding.is_some()
            {
                let start = self
                    .tokens
                    .partition_point(|token| token.source.end <= source.start);
                for token in self.tokens[start..]
                    .iter()
                    .take_while(|token| token.source.start < source.end)
                {
                    spans.push(TokenSpan {
                        source: token.source.start.max(source.start)
                            ..token.source.end.min(source.end),
                        kind: token.kind,
                    });
                }
            }
            let run_links = if !result.pending
                && !result.limited
                && source.len() <= CHUNK
                && !doc.is_paged()
            {
                doc.read_range(source.clone()).map_or_else(
                    |_| Vec::new(),
                    |bytes| {
                        if profile == SyntaxProfile::Abl {
                            return spans
                                .iter()
                                .filter_map(|token| {
                                    let region = token.source.start - source.start
                                        ..token.source.end - source.start;
                                    if !token.kind.is_word()
                                        || !bytes[region.clone()].eq_ignore_ascii_case(b"RUN")
                                    {
                                        return None;
                                    }
                                    let operand =
                                        jc_languages::syntax::abl_run_operand(&bytes, region.end)?;
                                    Some((
                                        token.source.clone(),
                                        source.start + operand.start..source.start + operand.end,
                                    ))
                                })
                                .take(32)
                                .collect();
                        }
                        if !matches!(
                            profile,
                            SyntaxProfile::CSharp
                                | SyntaxProfile::Java
                                | SyntaxProfile::Php
                                | SyntaxProfile::Rust
                                | SyntaxProfile::JavaScript
                                | SyntaxProfile::TypeScript
                        ) {
                            return Vec::new();
                        }
                        let mut links = Vec::new();
                        let mut at = 0;
                        while at < bytes.len() && links.len() < 32 {
                            if !(bytes[at].is_ascii_alphabetic()
                                || bytes[at] == b'_'
                                || bytes[at] >= 128)
                            {
                                at += 1;
                                continue;
                            }
                            let start = at;
                            at += 1;
                            while at < bytes.len()
                                && (bytes[at].is_ascii_alphanumeric()
                                    || matches!(bytes[at], b'_' | b'$')
                                    || bytes[at] >= 128)
                            {
                                at += 1;
                            }
                            let range = source.start + start..source.start + at;
                            if spans.iter().any(|span| {
                                span.source.start < range.end
                                    && span.source.end > range.start
                                    && !matches!(span.kind, SyntaxKind::Plain | SyntaxKind::Type)
                            }) {
                                continue;
                            }
                            let call = bytes[at..]
                                .iter()
                                .copied()
                                .find(|b| !b.is_ascii_whitespace())
                                == Some(b'(');
                            let marker = range.start..if call { range.end } else { range.start };
                            links.push((marker, range));
                        }
                        links
                    },
                )
            } else {
                Vec::new()
            };
            result.rows.push(SyntaxRow {
                line,
                source,
                spans,
                run_links,
            });
        }
        result.stats.cache_bytes = self.memory_bytes();
        result.stats.scanned_to = self.cursor;
        result
    }
}

impl Workbench {
    pub fn code_link_at(&self, tab: TabId, byte: usize) -> bool {
        self.line_ordinal_for_byte(tab, byte).is_some_and(|line| {
            self.syntax_rows(tab, line, 1).rows.iter().any(|row| {
                row.run_links
                    .iter()
                    .any(|(keyword, operand)| (keyword.start..operand.end).contains(&byte))
            })
        })
    }

    pub fn syntax_profile_for_tab(&self, tab: TabId) -> SyntaxProfile {
        self.document_by_tab(tab)
            .map_or(SyntaxProfile::Plain, |document| {
                jc_languages::detect(
                    document
                        .session
                        .path()
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or(""),
                )
                .syntax_profile()
            })
    }

    /// Owner-thread continuation of an already requested window. No frame or
    /// source snapshot is constructed, and stale metadata cannot resume work.
    pub fn continue_syntax(&self, tab: TabId) -> bool {
        let Some(document) = self.document_by_tab(tab) else {
            return false;
        };
        let mut cache = document.syntax.borrow_mut();
        let source = document.session.document();
        if cache.revision != Some(source.revision())
            || cache.encoding != document.session.encoding()
            || cache.path != document.session.path()
        {
            cache.clear();
            return false;
        }
        if cache.complete || cache.limited {
            return false;
        }
        let (Some(window), Some(encoding)) = (cache.window.clone(), cache.encoding) else {
            return false;
        };
        cache.advance(source, &window, encoding);
        !cache.complete
    }
    /// At most 128 visible lines and 64KiB of new lexical work per call. Pending
    /// returns Plain and the frontend schedules another owner-thread delivery.
    pub fn syntax_rows(&self, tab: TabId, first_line: usize, rows: usize) -> SyntaxRows {
        let Some(document) = self.document_by_tab(tab) else {
            return SyntaxRows {
                rows: Vec::new(),
                pending: false,
                limited: false,
                stats: SyntaxStats::default(),
                revision: 0,
                profile: SyntaxProfile::Plain,
                encoding: None,
            };
        };
        let profile = self.syntax_profile_for_tab(tab);
        let mut ranges = Vec::with_capacity(rows.min(MAX_ROWS));
        for line in first_line..first_line.saturating_add(rows.min(MAX_ROWS)) {
            let Some((content, eol)) = self.line_range_for_tab(tab, line) else {
                break;
            };
            ranges.push((line, content.start..eol.end));
        }
        document.syntax.borrow_mut().rows(
            document.session.document(),
            document.session.path(),
            profile,
            document.session.encoding(),
            ranges,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(cache: &mut SyntaxCache, doc: &Document, source: Range<usize>) -> SyntaxRows {
        cache.rows(
            doc,
            Path::new("memory.p"),
            SyntaxProfile::Abl,
            Some(TextEncoding::Utf8),
            vec![(0, source)],
        )
    }
    #[test]
    fn deep_context_advances_cooperatively_without_false_colors_or_global_rescan() {
        let mut bytes = b"/*\n".to_vec();
        bytes.extend_from_slice(&b"comment line\n".repeat(20_000));
        bytes.extend_from_slice(b"*/\nDISPLAY 42.\n");
        let doc = Document::from_bytes(bytes);
        let start = doc.len() - b"DISPLAY 42.\n".len();
        let mut cache = SyntaxCache::default();
        let mut last = 0;
        let mut calls = 0;
        loop {
            let result = request(&mut cache, &doc, start..doc.len());
            calls += 1;
            assert!(result.stats.processed_bytes <= WORK_LIMIT);
            assert!(result.stats.cache_bytes <= CACHE_LIMIT);
            assert!(result.stats.scanned_to >= last);
            last = result.stats.scanned_to;
            if !result.pending {
                assert_eq!(result.kind_at(start), SyntaxKind::Keyword);
                break;
            }
            assert_eq!(result.kind_at(start), SyntaxKind::Plain);
            assert!(calls < 20);
        }
        assert!(calls > 1);
        assert_eq!(
            request(&mut cache, &doc, start..doc.len())
                .stats
                .processed_bytes,
            0
        );
    }
    #[test]
    fn edit_and_undo_invalidation_reuses_only_prefix_context_and_recolors_suffix() {
        let mut doc = Document::from_bytes(b"/* note */\nDISPLAY 42.\n".to_vec());
        let mut cache = SyntaxCache::default();
        let start = b"/* note */\n".len();
        assert_eq!(
            request(&mut cache, &doc, start..doc.len()).kind_at(start),
            SyntaxKind::Keyword
        );
        doc.delete(8..10).unwrap();
        let change = doc.last_change().unwrap().clone();
        cache.invalidate(&change, doc.revision());
        assert_eq!(
            request(&mut cache, &doc, start - 2..doc.len()).kind_at(start - 2),
            SyntaxKind::Comment
        );
        assert!(doc.undo());
        let change = doc.last_change().unwrap().clone();
        cache.invalidate(&change, doc.revision());
        assert_eq!(
            request(&mut cache, &doc, start..doc.len()).kind_at(start),
            SyntaxKind::Keyword
        );
    }
    #[test]
    fn visible_caps_are_finite_plain_fallback_and_unsupported_profiles_do_no_work() {
        let doc = Document::from_bytes(b"DISPLAY x. ".repeat(40_000));
        let mut cache = SyntaxCache::default();
        let result = request(&mut cache, &doc, 0..doc.len());
        assert!(result.limited);
        assert!(!result.pending);
        assert_eq!(result.kind_at(0), SyntaxKind::Plain);
        assert_eq!(result.stats.processed_bytes, 0);
        let result = cache.rows(
            &doc,
            Path::new("memory.cs"),
            SyntaxProfile::Plain,
            Some(TextEncoding::Utf8),
            vec![(0, 0..10)],
        );
        assert!(!result.pending);
        assert!(!result.limited);
        assert_eq!(result.stats.processed_bytes, 0);
        assert_eq!(result.kind_at(0), SyntaxKind::Plain);
    }
    #[test]
    fn token_exhaustion_finishes_plain_and_encoding_or_path_changes_clear_context() {
        let doc = Document::from_bytes(b"IF x THEN y. ".repeat(1000));
        let mut cache = SyntaxCache::default();
        let result = request(&mut cache, &doc, 0..doc.len());
        assert!(result.limited);
        assert!(!result.pending);
        assert_eq!(result.kind_at(0), SyntaxKind::Plain);
        assert!(result.stats.cache_bytes <= CACHE_LIMIT);
        let doc = Document::from_bytes(b"DISPLAY 1.".to_vec());
        cache.clear();
        let result = request(&mut cache, &doc, 0..doc.len());
        assert_eq!(result.kind_at(0), SyntaxKind::Keyword);
        let result = cache.rows(
            &doc,
            Path::new("new.json"),
            SyntaxProfile::Json,
            Some(TextEncoding::Windows1252),
            vec![(0, 0..doc.len())],
        );
        assert_eq!(result.kind_at(0), SyntaxKind::Plain);
        assert_eq!(result.profile, SyntaxProfile::Json);
    }
    #[test]
    fn alternating_deep_and_limited_windows_preserve_progress() {
        let mut bytes = b"IF x THEN y. ".repeat(1000);
        let prefix = bytes.len();
        bytes.extend_from_slice(&b" \n".repeat(100_000));
        let target = bytes.len();
        bytes.extend_from_slice(b"DISPLAY 42.\n");
        let doc = Document::from_bytes(bytes);
        let mut cache = SyntaxCache::default();
        while request(&mut cache, &doc, target..doc.len()).pending {}
        assert!(request(&mut cache, &doc, 0..prefix).limited);
        let warm = request(&mut cache, &doc, target..doc.len());
        assert!(!warm.pending);
        assert_eq!(warm.kind_at(target), SyntaxKind::Keyword);
        assert!(warm.stats.processed_bytes <= 8192);
    }
}
