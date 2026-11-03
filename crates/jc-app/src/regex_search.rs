//! Cooperative anchored DFA search over borrowed source chunks. No source copy
//! survives beyond one chunk, and retries are charged as work, not hidden scans.
use crate::text_search::{SearchLimits, SearchMatch, SearchStatus, Step};
use jc_text::{is_search_word, DecodedScalars, TextEncoding, TextPosition};
use regex_automata::{
    dfa::{dense, Automaton},
    nfa::thompson,
    util::{primitives::StateID, syntax},
    Anchored, Input,
};
use std::{ops::Range, sync::Arc, time::Instant};

pub(crate) type Dfa = dense::DFA<Vec<u32>>;
pub(crate) fn compile(query: &str, case: bool) -> Result<Arc<Dfa>, String> {
    if query.is_empty() || query.len() > 4096 {
        return Err("La expresión regular debe contener1..4096 bytes UTF8".into());
    }
    dense::Builder::new()
        .configure(
            dense::Config::new()
                .dfa_size_limit(Some(1024 * 1024))
                .determinize_size_limit(Some(2 * 1024 * 1024)),
        )
        .syntax(syntax::Config::new().case_insensitive(!case).nest_limit(64))
        .thompson(thompson::Config::new().nfa_size_limit(Some(1024 * 1024)))
        .build(query)
        .map(Arc::new)
        .map_err(|e| format!("Sintaxis/capacidad regex o cuota de compilación: {e}"))
}

#[derive(Debug, Clone, Copy)]
struct Cursor {
    byte: usize,
    position: TextPosition,
    previous: Option<char>,
}
impl Cursor {
    fn after(self, ch: char, end: usize) -> Self {
        let mut position = self.position;
        match ch {
            '\r' => {
                position.line += 1;
                position.column = 0;
            }
            '\n' => {
                position.line += usize::from(self.previous != Some('\r'));
                position.column = 0;
            }
            _ => position.column += 1,
        }
        Self {
            byte: end,
            position,
            previous: Some(ch),
        }
    }
}

#[derive(Debug)]
pub(crate) struct RegexScanner {
    dfa: Arc<Dfa>,
    encoding: TextEncoding,
    word: bool,
    len: usize,
    cursor: Cursor,
    start: Cursor,
    next: Option<Cursor>,
    best: Option<Cursor>,
    state: Option<StateID>,
    bytes: Vec<u8>,
    chunk_start: usize,
    work: usize,
    hits: usize,
    suppress_empty_at: Option<usize>,
    started: Instant,
    status: SearchStatus,
}
impl RegexScanner {
    pub(crate) fn new(dfa: Arc<Dfa>, encoding: TextEncoding, len: usize, word: bool) -> Self {
        let cursor = Cursor {
            byte: 0,
            position: TextPosition { line: 0, column: 0 },
            previous: None,
        };
        Self {
            dfa,
            encoding,
            word,
            len,
            cursor,
            start: cursor,
            next: None,
            best: None,
            state: None,
            bytes: Vec::new(),
            chunk_start: 0,
            work: 0,
            hits: 0,
            suppress_empty_at: None,
            started: Instant::now(),
            status: SearchStatus::Running,
        }
    }
    fn initialize(&mut self) {
        let mut context = [0; 4];
        let previous = self
            .cursor
            .previous
            .map(|ch| *ch.encode_utf8(&mut context).as_bytes().last().unwrap());
        let input_bytes = [previous.unwrap_or(0), 0];
        let input = if previous.is_some() {
            Input::new(&input_bytes).span(1..2)
        } else {
            Input::new(&input_bytes[..1])
        };
        match self.dfa.start_state_forward(&input.anchored(Anchored::Yes)) {
            Ok(state) => self.state = Some(state),
            Err(_) => self.status = SearchStatus::Unavailable,
        }
        self.start = self.cursor;
    }
    fn finish_candidate(&mut self, hits: &mut Vec<SearchMatch>, limits: SearchLimits) {
        if let Some(end) = self.best.take() {
            if end.byte == self.start.byte && self.suppress_empty_at == Some(end.byte) {
                if let Some(next) = self.next {
                    self.cursor = next;
                } else {
                    self.status = SearchStatus::Complete;
                }
                self.next = None;
                self.state = None;
                return;
            }
            if self.hits >= limits.max_results {
                self.status = SearchStatus::Limited;
                return;
            }
            hits.push(SearchMatch {
                range: self.start.byte..end.byte,
                position: self.start.position,
            });
            self.hits += 1;
            self.suppress_empty_at = (end.byte > self.start.byte).then_some(end.byte);
            if end.byte > self.start.byte {
                self.cursor = end;
            } else if let Some(next) = self.next {
                self.cursor = next;
            } else {
                self.status = SearchStatus::Complete;
            }
        } else if let Some(next) = self.next {
            self.cursor = next;
        } else {
            self.status = SearchStatus::Complete;
        }
        self.next = None;
        self.state = None;
    }
    pub(crate) fn step(
        &mut self,
        limits: SearchLimits,
        mut read: impl FnMut(Range<usize>, &mut Vec<u8>),
    ) -> Step {
        let began = Instant::now();
        let mut hits = Vec::new();
        let mut slot_work = 0;
        while self.status == SearchStatus::Running
            && hits.len() < 256
            && slot_work < limits.step_bytes
        {
            if self.started.elapsed() >= limits.total_time {
                self.status = SearchStatus::TimedOut;
                break;
            }
            if self.work >= limits.max_scan_bytes {
                self.status = SearchStatus::Limited;
                break;
            }
            if slot_work > 0 && began.elapsed() >= limits.step_time {
                break;
            }
            // The chunk is a disposable decoder input, never another document.
            if self.cursor.byte < self.len
                && (self.cursor.byte < self.chunk_start
                    || self.cursor.byte + 4 > self.chunk_start + self.bytes.len())
            {
                self.chunk_start = self.cursor.byte;
                self.bytes.clear();
                read(
                    self.cursor.byte..(self.cursor.byte + limits.step_bytes + 4).min(self.len),
                    &mut self.bytes,
                );
            }
            let local = self.cursor.byte.saturating_sub(self.chunk_start);
            let mut decoder = DecodedScalars::new(
                self.bytes.get(local..).unwrap_or(&[]),
                self.encoding,
                self.cursor.byte,
            );
            let content = decoder.content_start();
            if self.cursor.byte == 0 && content > 0 {
                self.cursor.byte = content;
            }
            let scalar = decoder.next();
            if self.state.is_none() {
                self.initialize();
            }
            if self.status != SearchStatus::Running {
                break;
            }
            let state = self.state.unwrap();
            let Some(scalar) = scalar else {
                if self.cursor.byte != self.len {
                    self.status = SearchStatus::Unavailable;
                    break;
                }
                let end = self.dfa.next_eoi_state(state);
                if self.dfa.is_match_state(end)
                    && !(self.word && self.start.previous.is_some_and(is_search_word))
                {
                    self.best = Some(self.cursor);
                }
                slot_work += 1;
                self.work += 1;
                self.finish_candidate(&mut hits, limits);
                continue;
            };
            if !scalar.valid {
                self.status = SearchStatus::Unavailable;
                break;
            }
            let ch = scalar.character;
            if self.next.is_none() {
                self.next = Some(self.cursor.after(ch, scalar.source_range.end));
            }
            // Neither candidate start nor match end may split CRLF.
            if self.cursor.byte == self.start.byte
                && ((self.cursor.previous == Some('\r') && ch == '\n')
                    || self.word && self.cursor.previous.is_some_and(is_search_word))
            {
                self.finish_candidate(&mut hits, limits);
                slot_work += 1;
                self.work += 1;
                continue;
            }
            let mut encoded = [0; 4];
            let mut state = state;
            for (i, &byte) in ch.encode_utf8(&mut encoded).as_bytes().iter().enumerate() {
                state = self.dfa.next_state(state, byte);
                slot_work += 1;
                self.work += 1;
                // DFA matches are delayed one byte. Only the first byte of the
                // next scalar denotes an original source boundary.
                if i == 0
                    && self.dfa.is_match_state(state)
                    && !(self.cursor.previous == Some('\r') && ch == '\n'
                        || self.word && is_search_word(ch))
                {
                    self.best = Some(self.cursor);
                }
            }
            self.cursor = self.cursor.after(ch, scalar.source_range.end);
            self.state = Some(state);
            if self.dfa.is_dead_state(state) {
                self.finish_candidate(&mut hits, limits);
            } else if self.dfa.is_quit_state(state) {
                self.status = SearchStatus::Unavailable;
            }
        }
        Step {
            matches: hits,
            status: self.status,
            scanned: self.cursor.byte,
        }
    }
}
