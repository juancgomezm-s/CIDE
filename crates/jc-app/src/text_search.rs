//! Shared streaming literal matcher for document and project search.
use jc_text::{
    decode_fragment, full_case_fold, is_search_word, DecodedText, TextEncoding, TextPosition,
};
use std::{
    collections::VecDeque,
    ops::Range,
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchMatch {
    pub range: Range<usize>,
    pub position: TextPosition,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn streaming_ranges_match_an_independent_flat_folded_oracle() {
        let texts = [
            "ßsS ss ﬃ ffi İi\u{307}",
            "a\r\nb\r\na",
            "Σς σ σa _σ 😀σ",
            "é e\u{301} áÁ",
        ];
        let needles = [
            "s", "ss", "sss", "ffi", "fi", "i", "i\u{307}", "σ", "a", "\r", "\n", "\r\n", "é",
        ];
        for text in texts {
            for query in needles {
                for word in [false, true] {
                    let needle: Vec<_> = query.chars().flat_map(full_case_fold).collect();
                    let chars: Vec<_> = text.char_indices().collect();
                    let mut flat = Vec::new();
                    for (i, &(start, ch)) in chars.iter().enumerate() {
                        let mapped: Vec<_> = full_case_fold(ch).collect();
                        for (j, &ch) in mapped.iter().enumerate() {
                            flat.push((ch, i, start, j == 0, j + 1 == mapped.len()));
                        }
                    }
                    let mut expected = Vec::new();
                    let mut i = 0;
                    while i + needle.len() <= flat.len() {
                        let first = flat[i];
                        let last = flat[i + needle.len() - 1];
                        let start = first.2;
                        let end = chars.get(last.1 + 1).map_or(text.len(), |p| p.0);
                        let inside = |offset: usize| {
                            offset > 0
                                && offset < text.len()
                                && text.as_bytes()[offset - 1] == b'\r'
                                && text.as_bytes()[offset] == b'\n'
                        };
                        if first.3
                            && last.4
                            && !inside(start)
                            && !inside(end)
                            && flat[i..i + needle.len()]
                                .iter()
                                .map(|p| p.0)
                                .eq(needle.iter().copied())
                            && (!word
                                || !(first.1 > 0 && is_search_word(chars[first.1 - 1].1)
                                    || chars.get(last.1 + 1).is_some_and(|p| is_search_word(p.1))))
                        {
                            expected.push(start..end);
                            i += needle.len();
                        } else {
                            i += 1;
                        }
                    }
                    for size in [4, 5, 7, 16] {
                        let pattern = Pattern::compile(query, false, word).unwrap();
                        let mut scanner = Scanner::new(pattern, TextEncoding::Utf8, text.len());
                        let mut actual = Vec::new();
                        let mut steps = 0;
                        loop {
                            let step = scanner.step(
                                SearchLimits {
                                    step_bytes: size,
                                    ..SearchLimits::default()
                                },
                                |range| text.as_bytes()[range].to_vec(),
                            );
                            actual.extend(step.matches.into_iter().map(|m| m.range));
                            steps += 1;
                            assert!(steps < 100);
                            if step.status != SearchStatus::Running {
                                assert_eq!(step.status, SearchStatus::Complete);
                                break;
                            }
                        }
                        assert_eq!(
                            actual, expected,
                            "text={text:?} query={query:?} word={word} chunk={size}"
                        );
                    }
                }
            }
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchStatus {
    #[default]
    Idle,
    Running,
    Complete,
    Limited,
    Cancelled,
    TimedOut,
    Unavailable,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchLimits {
    pub step_bytes: usize,
    pub max_scan_bytes: usize,
    pub max_results: usize,
    pub step_time: Duration,
    pub total_time: Duration,
}
impl Default for SearchLimits {
    fn default() -> Self {
        Self {
            step_bytes: 16 * 1024,
            max_scan_bytes: 16 * 1024 * 1024,
            max_results: 10_000,
            step_time: Duration::from_millis(2),
            total_time: Duration::from_secs(5),
        }
    }
}
impl SearchLimits {
    pub(crate) fn valid(self) -> bool {
        (4..=64 * 1024).contains(&self.step_bytes)
            && (1..=64 * 1024 * 1024).contains(&self.max_scan_bytes)
            && (1..=10_000).contains(&self.max_results)
            && !self.step_time.is_zero()
            && self.step_time <= Duration::from_millis(10)
            && !self.total_time.is_zero()
            && self.total_time <= Duration::from_secs(30)
    }
}
#[derive(Debug)]
pub(crate) struct Pattern {
    chars: Vec<char>,
    prefix: Vec<usize>,
    fold: bool,
    word: bool,
}
impl Pattern {
    pub(crate) fn compile(query: &str, case: bool, word: bool) -> Result<Arc<Self>, &'static str> {
        if query.is_empty() || query.len() > 4096 {
            return Err("Search text must contain 1..4096 UTF-8 bytes");
        }
        let chars: Vec<char> = if case {
            query.chars().collect()
        } else {
            query.chars().flat_map(full_case_fold).collect()
        };
        let mut prefix = vec![0; chars.len()];
        let mut j = 0;
        for i in 1..chars.len() {
            while j > 0 && chars[i] != chars[j] {
                j = prefix[j - 1];
            }
            if chars[i] == chars[j] {
                j += 1;
            }
            prefix[i] = j;
        }
        Ok(Arc::new(Self {
            chars,
            prefix,
            fold: !case,
            word,
        }))
    }
}
#[derive(Debug, Clone, Copy)]
struct Origin {
    start: usize,
    end: usize,
    position: TextPosition,
    first: bool,
    last: bool,
    left_word: bool,
    start_boundary: bool,
}
#[derive(Debug)]
struct Pending {
    hit: SearchMatch,
    last_cr: bool,
}
#[derive(Debug)]
struct Matcher {
    pattern: Arc<Pattern>,
    origins: VecDeque<Origin>,
    matched: usize,
    previous: Option<char>,
    pending: Option<Pending>,
}
impl Matcher {
    fn feed(
        &mut self,
        ch: char,
        start: usize,
        end: usize,
        position: TextPosition,
        valid: bool,
    ) -> Option<SearchMatch> {
        let ready = self
            .pending
            .take()
            .filter(|p| {
                !(p.last_cr && ch == '\n' || self.pattern.word && valid && is_search_word(ch))
            })
            .map(|p| p.hit);
        if ready.is_some() {
            self.matched = 0;
            self.origins.clear();
        }
        if !valid {
            self.matched = 0;
            self.origins.clear();
            self.previous = None;
            return ready;
        }
        let left_word = self.pattern.word && self.previous.is_some_and(is_search_word);
        let start_boundary = !(self.previous == Some('\r') && ch == '\n');
        let mut scalar = [ch, '\0', '\0'];
        let mut count = 1;
        if self.pattern.fold {
            count = 0;
            for folded in full_case_fold(ch) {
                scalar[count] = folded;
                count += 1;
            }
        }
        for (index, &folded) in scalar[..count].iter().enumerate() {
            while self.matched > 0 && folded != self.pattern.chars[self.matched] {
                self.matched = self.pattern.prefix[self.matched - 1];
            }
            // Only a matching prefix needs a byte-origin map. Sparse queries
            // should not copy an Origin for every unrelated source scalar.
            while self.origins.len() > self.matched {
                self.origins.pop_front();
            }
            if folded == self.pattern.chars[self.matched] {
                self.origins.push_back(Origin {
                    start,
                    end,
                    position,
                    first: index == 0,
                    last: index + 1 == count,
                    left_word,
                    start_boundary,
                });
                self.matched += 1;
            }
            if self.matched == self.pattern.chars.len() {
                let first = *self.origins.front().unwrap();
                let last = *self.origins.back().unwrap();
                if first.first
                    && last.last
                    && first.start_boundary
                    && (!self.pattern.word || !first.left_word)
                {
                    self.pending = Some(Pending {
                        hit: SearchMatch {
                            range: first.start..last.end,
                            position: first.position,
                        },
                        last_cr: ch == '\r',
                    });
                }
                self.matched = self.pattern.prefix[self.matched - 1];
            }
        }
        self.previous = Some(ch);
        ready
    }
}
#[derive(Debug)]
struct Chunk {
    decoded: DecodedText,
    byte: usize,
    scalar: usize,
    count: usize,
    issue: usize,
}
#[derive(Debug)]
pub(crate) struct Scanner {
    matcher: Matcher,
    encoding: TextEncoding,
    len: usize,
    offset: usize,
    chunk: Option<Chunk>,
    position: TextPosition,
    cr: bool,
    started: Instant,
    total_hits: usize,
    status: SearchStatus,
}
pub(crate) struct Step {
    pub matches: Vec<SearchMatch>,
    pub status: SearchStatus,
    pub scanned: usize,
}
impl Scanner {
    pub(crate) fn new(pattern: Arc<Pattern>, encoding: TextEncoding, len: usize) -> Self {
        Self {
            matcher: Matcher {
                pattern,
                origins: VecDeque::new(),
                matched: 0,
                previous: None,
                pending: None,
            },
            encoding,
            len,
            offset: 0,
            chunk: None,
            position: TextPosition { line: 0, column: 0 },
            cr: false,
            started: Instant::now(),
            total_hits: 0,
            status: SearchStatus::Running,
        }
    }
    /// Exactly one bounded decoded chunk per call; no canonical buffer ownership.
    pub(crate) fn step(
        &mut self,
        limits: SearchLimits,
        mut read: impl FnMut(Range<usize>) -> Vec<u8>,
    ) -> Step {
        let started = Instant::now();
        let mut hits = Vec::new();
        if self.status != SearchStatus::Running {
            return Step {
                matches: hits,
                status: self.status,
                scanned: self.offset,
            };
        }
        if self.started.elapsed() >= limits.total_time {
            self.status = SearchStatus::TimedOut;
        }
        if self.offset >= limits.max_scan_bytes && self.offset < self.len {
            self.status = SearchStatus::Limited;
        }
        if self.status == SearchStatus::Running && self.chunk.is_none() && self.offset < self.len {
            let end = (self.offset + limits.step_bytes)
                .min(self.len)
                .min(limits.max_scan_bytes);
            let bytes = read(self.offset..(end + 4).min(self.len));
            let decoded = decode_fragment(&bytes, self.encoding, self.offset);
            let count = decoded
                .source_offsets
                .windows(2)
                .take_while(|pair| pair[1] <= end)
                .count();
            if count == 0 {
                self.offset = decoded.content_start.min(end);
                if self.offset < end {
                    self.status = SearchStatus::Limited;
                }
            } else {
                self.chunk = Some(Chunk {
                    decoded,
                    byte: 0,
                    scalar: 0,
                    count,
                    issue: 0,
                });
            }
        }
        let mut processed = 0;
        while self.status == SearchStatus::Running {
            let Some(chunk) = self.chunk.as_mut() else {
                break;
            };
            let ch = chunk.decoded.text[chunk.byte..].chars().next().unwrap();
            let start = chunk.decoded.source_offsets[chunk.scalar];
            let end = chunk.decoded.source_offsets[chunk.scalar + 1];
            while chunk.issue < chunk.decoded.issues.len()
                && chunk.decoded.issues[chunk.issue].source_range.end <= start
            {
                chunk.issue += 1;
            }
            let valid = chunk
                .decoded
                .issues
                .get(chunk.issue)
                .is_none_or(|issue| issue.source_range.start >= end);
            let hit = self.matcher.feed(ch, start, end, self.position, valid);
            match ch {
                '\r' => {
                    self.position.line += 1;
                    self.position.column = 0;
                }
                '\n' => {
                    if !self.cr {
                        self.position.line += 1;
                    }
                    self.position.column = 0;
                }
                _ => self.position.column += 1,
            };
            self.cr = ch == '\r';
            self.offset = end;
            chunk.byte += ch.len_utf8();
            chunk.scalar += 1;
            processed += 1;
            if let Some(hit) = hit {
                self.accept(hit, limits, &mut hits);
            }
            if self.chunk.as_ref().is_some_and(|c| c.scalar == c.count) {
                self.chunk = None;
                break;
            }
            if hits.len() >= 256 || processed % 128 == 0 && started.elapsed() >= limits.step_time {
                break;
            }
        }
        if self.status == SearchStatus::Running && self.offset == self.len && self.chunk.is_none() {
            if let Some(pending) = self.matcher.pending.take() {
                self.accept(pending.hit, limits, &mut hits);
            }
            if self.status == SearchStatus::Running {
                self.status = SearchStatus::Complete;
            }
        }
        Step {
            matches: hits,
            status: self.status,
            scanned: self.offset,
        }
    }
    fn accept(&mut self, hit: SearchMatch, limits: SearchLimits, hits: &mut Vec<SearchMatch>) {
        if self.total_hits >= limits.max_results {
            self.status = SearchStatus::Limited;
        } else {
            self.total_hits += 1;
            hits.push(hit);
        }
    }
}
