//! Bounded streaming lexical subsets, operating on canonical source bytes.
//! This is presentation metadata, not an ABL/JSON/Rust parser or compiler.
//! ASCII delimiters work across pieces and legacy pages without decoding text.
use std::ops::Range;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SyntaxKind {
    #[default]
    Plain,
    Keyword,
    String,
    Comment,
    Number,
    Preprocessor,
    Operator,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenSpan {
    pub source: Range<usize>,
    pub kind: SyntaxKind,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SyntaxProfile {
    #[default]
    Plain,
    Abl,
    Json,
    Jsonc,
    Rust,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Mode {
    #[default]
    Normal,
    Word,
    Number,
    Slash,
    Block,
    Line,
    String,
    Directive,
    Include,
    RawPrefix,
    RawString,
    RawIdentifier,
    Character,
    Lifetime,
}
/// Delimiter cap for this lexical presentation subset, independent of Rust's
/// grammar/compiler limits. Exceeding it explicitly limits the entire viewport.
pub const RUST_RAW_HASH_LIMIT: u8 = 32;

/// Copyable bounded continuation, including an unfinished <=64-byte word.
/// Longer identifiers are known Plain immediately rather than retained whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LexerState {
    mode: Mode,
    start: usize,
    word: [u8; 64],
    length: u8,
    overflow: bool,
    depth: u16,
    last: u8,
    quote: u8,
    escape: bool,
    quote_end: bool,
    continuation_cr: bool,
    line_start: bool,
    limited: bool,
    raw_hashes: u8,
    raw_seen: u8,
    raw_close: bool,
    byte_character: bool,
}
impl Default for LexerState {
    fn default() -> Self {
        Self {
            mode: Mode::Normal,
            start: 0,
            word: [0; 64],
            length: 0,
            overflow: false,
            depth: 0,
            last: 0,
            quote: 0,
            escape: false,
            quote_end: false,
            continuation_cr: false,
            line_start: true,
            limited: false,
            raw_hashes: 0,
            raw_seen: 0,
            raw_close: false,
            byte_character: false,
        }
    }
}
impl LexerState {
    pub fn limited(&self) -> bool {
        self.limited
    }
    fn begin_word(&mut self, mode: Mode, start: usize, byte: u8) {
        self.mode = mode;
        self.start = start;
        self.length = 1;
        self.word[0] = byte;
        self.overflow = false;
        self.line_start = false;
    }
    fn finish_word(
        &mut self,
        profile: SyntaxProfile,
        end: usize,
        emit: &mut impl FnMut(TokenSpan),
    ) {
        if !self.overflow {
            let word = &self.word[..usize::from(self.length)];
            if self.mode == Mode::Number
                && profile == SyntaxProfile::Abl
                && word.last() == Some(&b'.')
                && number(&word[..word.len() - 1], false)
            {
                emit(TokenSpan {
                    source: self.start..end - 1,
                    kind: SyntaxKind::Number,
                });
                emit(TokenSpan {
                    source: end - 1..end,
                    kind: SyntaxKind::Operator,
                });
                self.mode = Mode::Normal;
                self.length = 0;
                self.overflow = false;
                return;
            }
            let kind = if self.mode == Mode::Number
                && (if profile == SyntaxProfile::Rust {
                    rust_number(word)
                } else {
                    number(word, profile != SyntaxProfile::Abl)
                }) {
                SyntaxKind::Number
            } else if self.mode == Mode::Word && keyword(profile, word) {
                SyntaxKind::Keyword
            } else {
                SyntaxKind::Plain
            };
            emit(TokenSpan {
                source: self.start..end,
                kind,
            });
        }
        self.mode = Mode::Normal;
        self.length = 0;
        self.overflow = false;
    }
}

/// Feed contiguous bytes exactly once, with absolute offsets. `final_chunk`
/// means actual source EOF, not a piece/line boundary. No allocations occur.
pub fn scan(
    profile: SyntaxProfile,
    state: &mut LexerState,
    bytes: &[u8],
    source_start: usize,
    final_chunk: bool,
    mut emit: impl FnMut(TokenSpan),
) {
    if profile == SyntaxProfile::Plain {
        return;
    }
    for (index, &byte) in bytes.iter().enumerate() {
        let Some(offset) = source_start.checked_add(index) else {
            state.limited = true;
            return;
        };
        let Some(end) = offset.checked_add(1) else {
            state.limited = true;
            return;
        };
        let mut again = true;
        while again {
            again = false;
            if state.limited {
                return;
            }
            match state.mode {
                Mode::Word | Mode::Number => {
                    if profile == SyntaxProfile::Rust && !state.overflow {
                        let word = &state.word[..usize::from(state.length)];
                        if state.mode == Mode::Word
                            && matches!(word, b"r" | b"br")
                            && matches!(byte, b'#' | b'"')
                        {
                            state.raw_hashes = u8::from(byte == b'#');
                            state.raw_seen = 0;
                            state.raw_close = false;
                            state.mode = if byte == b'"' {
                                Mode::RawString
                            } else {
                                Mode::RawPrefix
                            };
                            if byte == b'"' {
                                emit(TokenSpan {
                                    source: state.start..end,
                                    kind: SyntaxKind::String,
                                });
                            }
                            continue;
                        }
                        if state.mode == Mode::Word && word == b"b" && matches!(byte, b'"' | b'\'')
                        {
                            if byte == b'\'' {
                                state.mode = Mode::Character;
                                state.byte_character = true;
                                state.word[1] = byte;
                                state.length = 2;
                            } else {
                                state.mode = Mode::String;
                                state.quote = byte;
                                state.escape = false;
                                state.quote_end = false;
                                emit(TokenSpan {
                                    source: state.start..end,
                                    kind: SyntaxKind::String,
                                });
                            }
                            continue;
                        }
                        // A range/field delimiter is not part of the preceding
                        // numeric literal. Retain the dot until its next byte.
                        if state.mode == Mode::Number
                            && state.last == b'.'
                            && (byte == b'.' || byte.is_ascii_alphabetic() || byte == b'_')
                        {
                            state.length -= 1;
                            state.finish_word(profile, offset - 1, &mut emit);
                            emit(TokenSpan {
                                source: offset - 1..offset,
                                kind: SyntaxKind::Operator,
                            });
                            again = true;
                            continue;
                        }
                    }
                    let continuation = if state.mode == Mode::Word {
                        word_byte(byte, profile)
                    } else {
                        byte.is_ascii_alphanumeric()
                            || byte == b'_'
                            || byte == b'.'
                            || (matches!(byte, b'+' | b'-')
                                && matches!(state.last, b'e' | b'E')
                                && !(profile == SyntaxProfile::Rust
                                    && state.word[0] == b'0'
                                    && matches!(state.word[1], b'x' | b'o' | b'b')))
                    };
                    if continuation {
                        if state.overflow {
                            emit(TokenSpan {
                                source: offset..end,
                                kind: SyntaxKind::Plain,
                            });
                        } else if usize::from(state.length) == state.word.len() {
                            state.overflow = true;
                            emit(TokenSpan {
                                source: state.start..end,
                                kind: SyntaxKind::Plain,
                            });
                        } else {
                            state.word[usize::from(state.length)] = byte;
                            state.length += 1;
                        }
                        state.last = byte;
                    } else {
                        state.finish_word(profile, offset, &mut emit);
                        again = true;
                    }
                }
                Mode::Slash => {
                    if byte == b'*' {
                        state.mode = Mode::Block;
                        state.depth = 1;
                        state.last = 0;
                        emit(TokenSpan {
                            source: state.start..end,
                            kind: SyntaxKind::Comment,
                        });
                    } else if byte == b'/'
                        && matches!(profile, SyntaxProfile::Jsonc | SyntaxProfile::Rust)
                    {
                        state.mode = Mode::Line;
                        emit(TokenSpan {
                            source: state.start..end,
                            kind: SyntaxKind::Comment,
                        });
                    } else {
                        state.mode = Mode::Normal;
                        emit(TokenSpan {
                            source: state.start..offset,
                            kind: SyntaxKind::Operator,
                        });
                        again = true;
                    }
                }
                Mode::Block => {
                    emit(TokenSpan {
                        source: offset..end,
                        kind: SyntaxKind::Comment,
                    });
                    if state.last == b'/'
                        && byte == b'*'
                        && matches!(profile, SyntaxProfile::Abl | SyntaxProfile::Rust)
                    {
                        if state.depth == u16::MAX {
                            state.limited = true;
                            return;
                        }
                        state.depth += 1;
                        state.last = 0;
                    } else if state.last == b'*' && byte == b'/' {
                        state.depth -= 1;
                        state.last = 0;
                        if state.depth == 0 {
                            state.mode = Mode::Normal;
                        }
                    } else {
                        state.last = if matches!(byte, b'/' | b'*') { byte } else { 0 };
                    }
                    if matches!(byte, b'\r' | b'\n') {
                        state.line_start = true;
                    }
                }
                Mode::Line => {
                    if matches!(byte, b'\r' | b'\n') {
                        state.mode = Mode::Normal;
                        state.line_start = true;
                        emit(TokenSpan {
                            source: offset..end,
                            kind: SyntaxKind::Plain,
                        });
                    } else {
                        emit(TokenSpan {
                            source: offset..end,
                            kind: SyntaxKind::Comment,
                        });
                    }
                }
                Mode::String => {
                    if state.quote_end {
                        state.quote_end = false;
                        if byte == state.quote && profile == SyntaxProfile::Abl {
                            emit(TokenSpan {
                                source: offset..end,
                                kind: SyntaxKind::String,
                            });
                            continue;
                        }
                        state.mode = Mode::Normal;
                        again = true;
                        continue;
                    }
                    if state.continuation_cr {
                        state.continuation_cr = false;
                        if byte == b'\n' {
                            emit(TokenSpan {
                                source: offset..end,
                                kind: SyntaxKind::String,
                            });
                            continue;
                        }
                    }
                    if state.escape {
                        state.escape = false;
                        state.continuation_cr = byte == b'\r' && profile == SyntaxProfile::Abl;
                        emit(TokenSpan {
                            source: offset..end,
                            kind: SyntaxKind::String,
                        });
                        continue;
                    }
                    if byte
                        == if profile == SyntaxProfile::Abl {
                            b'~'
                        } else {
                            b'\\'
                        }
                    {
                        state.escape = true;
                        emit(TokenSpan {
                            source: offset..end,
                            kind: SyntaxKind::String,
                        });
                    } else if byte == state.quote {
                        emit(TokenSpan {
                            source: offset..end,
                            kind: SyntaxKind::String,
                        });
                        if profile == SyntaxProfile::Abl {
                            state.quote_end = true;
                        } else {
                            state.mode = Mode::Normal;
                        }
                    } else if matches!(byte, b'\r' | b'\n') {
                        if profile == SyntaxProfile::Rust {
                            emit(TokenSpan {
                                source: offset..end,
                                kind: SyntaxKind::String,
                            });
                        } else {
                            state.mode = Mode::Normal;
                            state.line_start = true;
                            emit(TokenSpan {
                                source: offset..end,
                                kind: SyntaxKind::Plain,
                            });
                        }
                    } else {
                        emit(TokenSpan {
                            source: offset..end,
                            kind: SyntaxKind::String,
                        });
                    }
                }
                Mode::Directive => {
                    if state.continuation_cr {
                        state.continuation_cr = false;
                        if byte == b'\n' {
                            emit(TokenSpan {
                                source: offset..end,
                                kind: SyntaxKind::Preprocessor,
                            });
                            continue;
                        }
                    }
                    if state.escape {
                        state.escape = false;
                        state.continuation_cr = byte == b'\r';
                        emit(TokenSpan {
                            source: offset..end,
                            kind: SyntaxKind::Preprocessor,
                        });
                    } else if byte == b'~' {
                        state.escape = true;
                        emit(TokenSpan {
                            source: offset..end,
                            kind: SyntaxKind::Preprocessor,
                        });
                    } else if matches!(byte, b'\r' | b'\n') {
                        state.mode = Mode::Normal;
                        state.line_start = true;
                        emit(TokenSpan {
                            source: offset..end,
                            kind: SyntaxKind::Plain,
                        });
                    } else {
                        emit(TokenSpan {
                            source: offset..end,
                            kind: SyntaxKind::Preprocessor,
                        });
                    }
                }
                Mode::Include => {
                    emit(TokenSpan {
                        source: offset..end,
                        kind: SyntaxKind::Preprocessor,
                    });
                    if state.escape {
                        state.escape = false;
                        continue;
                    }
                    if byte == b'~' {
                        state.escape = true;
                        continue;
                    }
                    if state.quote != 0 {
                        if state.quote_end {
                            state.quote_end = false;
                            if byte == state.quote {
                                continue;
                            }
                            state.quote = 0;
                        } else {
                            if byte == state.quote {
                                state.quote_end = true;
                            }
                            continue;
                        }
                    }
                    if matches!(byte, b'\'' | b'"') {
                        state.quote = byte;
                    } else if byte == b'{' {
                        if state.depth == u16::MAX {
                            state.limited = true;
                            return;
                        }
                        state.depth += 1;
                    } else if byte == b'}' {
                        state.depth -= 1;
                        if state.depth == 0 {
                            state.mode = Mode::Normal;
                        }
                    }
                }
                Mode::RawPrefix => {
                    if byte == b'#' {
                        if state.raw_hashes == RUST_RAW_HASH_LIMIT {
                            state.limited = true;
                            return;
                        }
                        state.raw_hashes += 1;
                    } else if byte == b'"' {
                        state.mode = Mode::RawString;
                        state.raw_seen = 0;
                        state.raw_close = false;
                        emit(TokenSpan {
                            source: state.start..end,
                            kind: SyntaxKind::String,
                        });
                    } else {
                        emit(TokenSpan {
                            source: state.start..offset,
                            kind: SyntaxKind::Plain,
                        });
                        if word_byte(byte, profile) {
                            state.mode = Mode::RawIdentifier;
                            emit(TokenSpan {
                                source: offset..end,
                                kind: SyntaxKind::Plain,
                            });
                        } else {
                            state.mode = Mode::Normal;
                            again = true;
                        }
                    }
                }
                Mode::RawString => {
                    emit(TokenSpan {
                        source: offset..end,
                        kind: SyntaxKind::String,
                    });
                    if state.raw_close && byte == b'#' {
                        state.raw_seen += 1;
                        if state.raw_seen == state.raw_hashes {
                            state.mode = Mode::Normal;
                            state.raw_close = false;
                        }
                    } else if byte == b'"' {
                        state.raw_seen = 0;
                        state.raw_close = true;
                        if state.raw_hashes == 0 {
                            state.mode = Mode::Normal;
                            state.raw_close = false;
                        }
                    } else {
                        state.raw_close = false;
                        state.raw_seen = 0;
                    }
                }
                Mode::RawIdentifier | Mode::Lifetime => {
                    if word_byte(byte, profile) {
                        emit(TokenSpan {
                            source: offset..end,
                            kind: if state.mode == Mode::Lifetime {
                                SyntaxKind::Preprocessor
                            } else {
                                SyntaxKind::Plain
                            },
                        });
                    } else {
                        state.mode = Mode::Normal;
                        again = true;
                    }
                }
                Mode::Character => {
                    let length = usize::from(state.length);
                    if length >= 16 {
                        state.limited = true;
                        return;
                    }
                    state.word[length] = byte;
                    state.length += 1;
                    let prefix = if state.byte_character { 2 } else { 1 };
                    let payload = &state.word[prefix..usize::from(state.length)];
                    match character_probe(payload, state.byte_character) {
                        CharProbe::Pending => {}
                        CharProbe::Complete => {
                            emit(TokenSpan {
                                source: state.start..end,
                                kind: SyntaxKind::String,
                            });
                            state.mode = Mode::Normal;
                        }
                        CharProbe::NotCharacter => {
                            let lifetime = !state.byte_character
                                && payload.first().is_some_and(|byte| identifier_start(*byte));
                            if lifetime && word_byte(byte, profile) {
                                emit(TokenSpan {
                                    source: state.start..end,
                                    kind: SyntaxKind::Preprocessor,
                                });
                                state.mode = Mode::Lifetime;
                            } else if lifetime {
                                emit(TokenSpan {
                                    source: state.start..offset,
                                    kind: SyntaxKind::Preprocessor,
                                });
                                state.mode = Mode::Normal;
                                again = true;
                            } else {
                                emit(TokenSpan {
                                    source: state.start..end,
                                    kind: SyntaxKind::Plain,
                                });
                                state.mode = Mode::Normal;
                            }
                        }
                    }
                }
                Mode::Normal => {
                    if state.continuation_cr {
                        state.continuation_cr = false;
                        if byte == b'\n' {
                            emit(TokenSpan {
                                source: offset..end,
                                kind: SyntaxKind::Plain,
                            });
                            continue;
                        }
                    }
                    if state.escape {
                        state.escape = false;
                        state.continuation_cr = byte == b'\r';
                        emit(TokenSpan {
                            source: offset..end,
                            kind: SyntaxKind::Plain,
                        });
                        continue;
                    }
                    if matches!(byte, b'\r' | b'\n') {
                        state.line_start = true;
                        emit(TokenSpan {
                            source: offset..end,
                            kind: SyntaxKind::Plain,
                        });
                        continue;
                    }
                    if matches!(byte, b' ' | b'\t') {
                        emit(TokenSpan {
                            source: offset..end,
                            kind: SyntaxKind::Plain,
                        });
                        continue;
                    }
                    if profile == SyntaxProfile::Abl && byte == b'&' && state.line_start {
                        state.mode = Mode::Directive;
                        state.line_start = false;
                        emit(TokenSpan {
                            source: offset..end,
                            kind: SyntaxKind::Preprocessor,
                        });
                        continue;
                    }
                    state.line_start = false;
                    if byte == b'~' && profile == SyntaxProfile::Abl {
                        state.escape = true;
                        emit(TokenSpan {
                            source: offset..end,
                            kind: SyntaxKind::Operator,
                        });
                    } else if byte == b'/'
                        && matches!(
                            profile,
                            SyntaxProfile::Abl | SyntaxProfile::Jsonc | SyntaxProfile::Rust
                        )
                    {
                        state.mode = Mode::Slash;
                        state.start = offset;
                    } else if byte == b'\'' && profile == SyntaxProfile::Rust {
                        state.mode = Mode::Character;
                        state.start = offset;
                        state.word[0] = byte;
                        state.length = 1;
                        state.byte_character = false;
                    } else if byte == b'"' || (byte == b'\'' && profile == SyntaxProfile::Abl) {
                        state.mode = Mode::String;
                        state.quote = byte;
                        state.escape = false;
                        state.quote_end = false;
                        emit(TokenSpan {
                            source: offset..end,
                            kind: SyntaxKind::String,
                        });
                    } else if byte == b'{' && profile == SyntaxProfile::Abl {
                        state.mode = Mode::Include;
                        state.depth = 1;
                        state.quote = 0;
                        state.escape = false;
                        state.quote_end = false;
                        emit(TokenSpan {
                            source: offset..end,
                            kind: SyntaxKind::Preprocessor,
                        });
                    } else if byte.is_ascii_digit()
                        || (byte == b'-'
                            && matches!(profile, SyntaxProfile::Json | SyntaxProfile::Jsonc))
                    {
                        state.begin_word(Mode::Number, offset, byte);
                        state.last = byte;
                    } else if byte.is_ascii_alphabetic() || byte == b'_' || byte >= 0x80 {
                        state.begin_word(Mode::Word, offset, byte);
                    } else {
                        emit(TokenSpan {
                            source: offset..end,
                            kind: if b"+-*/=<>():.,;[]{}!?|&".contains(&byte) {
                                SyntaxKind::Operator
                            } else {
                                SyntaxKind::Plain
                            },
                        });
                    }
                }
            }
        }
    }
    if final_chunk {
        let Some(end) = source_start.checked_add(bytes.len()) else {
            state.limited = true;
            return;
        };
        match state.mode {
            Mode::Word | Mode::Number => state.finish_word(profile, end, &mut emit),
            Mode::Slash => {
                emit(TokenSpan {
                    source: state.start..end,
                    kind: SyntaxKind::Operator,
                });
                state.mode = Mode::Normal;
            }
            Mode::String if state.quote_end => {
                state.quote_end = false;
                state.mode = Mode::Normal;
            }
            Mode::RawPrefix => {
                emit(TokenSpan {
                    source: state.start..end,
                    kind: SyntaxKind::Plain,
                });
                state.mode = Mode::Normal;
            }
            Mode::Character => {
                let payload = if state.byte_character {
                    &state.word[2..usize::from(state.length)]
                } else {
                    &state.word[1..usize::from(state.length)]
                };
                let lifetime = !state.byte_character
                    && payload.first().is_some_and(|byte| identifier_start(*byte));
                emit(TokenSpan {
                    source: state.start..end,
                    kind: if lifetime {
                        SyntaxKind::Preprocessor
                    } else {
                        SyntaxKind::Plain
                    },
                });
                state.mode = if lifetime {
                    Mode::Lifetime
                } else {
                    Mode::Normal
                };
            }
            _ => {}
        }
    }
}
fn word_byte(byte: u8, profile: SyntaxProfile) -> bool {
    byte.is_ascii_alphanumeric()
        || byte == b'_'
        || byte >= 0x80
        || (byte == b'-' && profile == SyntaxProfile::Abl)
}
fn identifier_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte >= 0x80
}
enum CharProbe {
    Pending,
    Complete,
    NotCharacter,
}
fn character_probe(payload: &[u8], byte_literal: bool) -> CharProbe {
    let Some(&first) = payload.first() else {
        return CharProbe::Pending;
    };
    let needed = if first == b'\\' {
        let Some(&code) = payload.get(1) else {
            return CharProbe::Pending;
        };
        match code {
            b'n' | b'r' | b't' | b'0' | b'\\' | b'\'' | b'"' => 2,
            b'x' => {
                if payload
                    .iter()
                    .take(4)
                    .skip(2)
                    .any(|byte| !byte.is_ascii_hexdigit())
                {
                    return CharProbe::NotCharacter;
                }
                if !byte_literal && payload.get(2).is_some_and(|byte| *byte > b'7') {
                    return CharProbe::NotCharacter;
                }
                4
            }
            b'u' if !byte_literal => {
                match payload.get(2) {
                    None => return CharProbe::Pending,
                    Some(b'{') => {}
                    _ => return CharProbe::NotCharacter,
                }
                let Some(close) = payload.iter().position(|byte| *byte == b'}') else {
                    return if payload.len() <= 12
                        && payload[3..]
                            .iter()
                            .all(|byte| byte.is_ascii_hexdigit() || *byte == b'_')
                    {
                        CharProbe::Pending
                    } else {
                        CharProbe::NotCharacter
                    };
                };
                let mut value = 0u32;
                let mut digits = 0;
                for &byte in &payload[3..close] {
                    if byte == b'_' {
                        continue;
                    }
                    let digit = match byte {
                        b'0'..=b'9' => byte - b'0',
                        b'a'..=b'f' => byte - b'a' + 10,
                        b'A'..=b'F' => byte - b'A' + 10,
                        _ => return CharProbe::NotCharacter,
                    };
                    digits += 1;
                    if digits > 6 {
                        return CharProbe::NotCharacter;
                    }
                    value = value * 16 + u32::from(digit);
                }
                if digits == 0 || char::from_u32(value).is_none() {
                    return CharProbe::NotCharacter;
                }
                close + 1
            }
            _ => return CharProbe::NotCharacter,
        }
    } else {
        match first {
            b'\'' | b'\r' | b'\n' | b'\t' | 0 => return CharProbe::NotCharacter,
            1..=0x7f => 1,
            0xc2..=0xdf if !byte_literal => 2,
            0xe0..=0xef if !byte_literal => 3,
            0xf0..=0xf4 if !byte_literal => 4,
            _ => return CharProbe::NotCharacter,
        }
    };
    if payload.len() <= needed {
        return CharProbe::Pending;
    }
    if payload.len() != needed + 1 || payload[needed] != b'\'' {
        return CharProbe::NotCharacter;
    }
    if first != b'\\'
        && std::str::from_utf8(&payload[..needed])
            .ok()
            .is_none_or(|text| text.chars().count() != 1)
    {
        return CharProbe::NotCharacter;
    }
    CharProbe::Complete
}
fn integer_suffix(bytes: &[u8]) -> bool {
    matches!(
        bytes,
        b"u8"
            | b"u16"
            | b"u32"
            | b"u64"
            | b"u128"
            | b"usize"
            | b"i8"
            | b"i16"
            | b"i32"
            | b"i64"
            | b"i128"
            | b"isize"
    )
}
fn rust_number(bytes: &[u8]) -> bool {
    if bytes.first() != Some(&b'0')
        || !bytes
            .get(1)
            .is_some_and(|byte| matches!(byte, b'x' | b'o' | b'b'))
    {
        let mut index = 0;
        let mut digits = 0;
        while bytes
            .get(index)
            .is_some_and(|byte| byte.is_ascii_digit() || *byte == b'_')
        {
            digits += usize::from(bytes[index].is_ascii_digit());
            index += 1;
        }
        if digits == 0 {
            return false;
        }
        let mut float = false;
        if bytes.get(index) == Some(&b'.') {
            float = true;
            index += 1;
            while bytes
                .get(index)
                .is_some_and(|byte| byte.is_ascii_digit() || *byte == b'_')
            {
                index += 1;
            }
        }
        if bytes
            .get(index)
            .is_some_and(|byte| matches!(byte, b'e' | b'E'))
        {
            float = true;
            index += 1;
            if bytes
                .get(index)
                .is_some_and(|byte| matches!(byte, b'+' | b'-'))
            {
                index += 1;
            }
            let mut digits = 0;
            while bytes
                .get(index)
                .is_some_and(|byte| byte.is_ascii_digit() || *byte == b'_')
            {
                digits += usize::from(bytes[index].is_ascii_digit());
                index += 1;
            }
            if digits == 0 {
                return false;
            }
        }
        let suffix = &bytes[index..];
        return suffix.is_empty()
            || matches!(suffix, b"f32" | b"f64")
            || (!float && integer_suffix(suffix));
    }
    let base = bytes[1];
    let mut index = 2;
    let mut digits = 0;
    while let Some(&byte) = bytes.get(index) {
        let digit = match base {
            b'x' => byte.is_ascii_hexdigit(),
            b'o' => matches!(byte, b'0'..=b'7'),
            _ => matches!(byte, b'0' | b'1'),
        };
        if !digit && byte != b'_' {
            break;
        }
        digits += usize::from(digit);
        index += 1;
    }
    digits > 0 && (index == bytes.len() || integer_suffix(&bytes[index..]))
}
fn number(bytes: &[u8], strict: bool) -> bool {
    let mut i = usize::from(bytes.first() == Some(&b'-'));
    let start = i;
    while bytes.get(i).is_some_and(u8::is_ascii_digit) {
        i += 1;
    }
    if i == start || (strict && i - start > 1 && bytes[start] == b'0') {
        return false;
    }
    if bytes.get(i) == Some(&b'.') {
        i += 1;
        let start = i;
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if i == start {
            return false;
        }
    }
    if bytes.get(i).is_some_and(|byte| matches!(byte, b'e' | b'E')) {
        i += 1;
        if bytes.get(i).is_some_and(|byte| matches!(byte, b'+' | b'-')) {
            i += 1;
        }
        let start = i;
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if i == start {
            return false;
        }
    }
    i == bytes.len()
}
fn keyword(profile: SyntaxProfile, bytes: &[u8]) -> bool {
    if profile == SyntaxProfile::Rust {
        return matches!(
            bytes,
            b"as"
                | b"async"
                | b"await"
                | b"break"
                | b"const"
                | b"continue"
                | b"crate"
                | b"dyn"
                | b"else"
                | b"enum"
                | b"extern"
                | b"false"
                | b"fn"
                | b"for"
                | b"if"
                | b"impl"
                | b"in"
                | b"let"
                | b"loop"
                | b"match"
                | b"mod"
                | b"move"
                | b"mut"
                | b"pub"
                | b"ref"
                | b"return"
                | b"self"
                | b"Self"
                | b"static"
                | b"struct"
                | b"super"
                | b"trait"
                | b"true"
                | b"type"
                | b"union"
                | b"unsafe"
                | b"use"
                | b"where"
                | b"while"
                | b"abstract"
                | b"become"
                | b"box"
                | b"do"
                | b"final"
                | b"macro"
                | b"override"
                | b"priv"
                | b"typeof"
                | b"unsized"
                | b"virtual"
                | b"yield"
                | b"try"
        );
    }
    if profile != SyntaxProfile::Abl {
        return matches!(bytes, b"true" | b"false" | b"null");
    }
    const WORDS: &[&[u8]] = &[
        b"ABSOLUTE",
        b"ACCUMULATE",
        b"AND",
        b"AS",
        b"ASC",
        b"ASSIGN",
        b"AVAILABLE",
        b"BEFORE-TABLE",
        b"BEGINS",
        b"BLOCK-LEVEL",
        b"BUFFER",
        b"BY",
        b"CATCH",
        b"CHARACTER",
        b"CLASS",
        b"CREATE",
        b"DATABASE",
        b"DATE",
        b"DATETIME",
        b"DATETIME-TZ",
        b"DECIMAL",
        b"DEFINE",
        b"DELETE",
        b"DISPLAY",
        b"DO",
        b"EACH",
        b"ELSE",
        b"EMPTY",
        b"END",
        b"ENTRY",
        b"ERROR",
        b"ERROR-STATUS",
        b"EXPORT",
        b"EXTENT",
        b"FALSE",
        b"FIND",
        b"FINALLY",
        b"FIRST",
        b"FOR",
        b"FORM",
        b"FRAME",
        b"FUNCTION",
        b"GET",
        b"GLOBAL",
        b"HANDLE",
        b"IF",
        b"IMPORT",
        b"IN",
        b"INDEX",
        b"INPUT",
        b"INPUT-OUTPUT",
        b"INTEGER",
        b"INT64",
        b"INTERNAL-ENTRIES",
        b"LEAVE",
        b"LENGTH",
        b"LIKE",
        b"LOCKED",
        b"LOGICAL",
        b"LONGCHAR",
        b"LOOKUP",
        b"MESSAGE",
        b"METHOD",
        b"NEW",
        b"NEXT",
        b"NO-ERROR",
        b"NO-LOCK",
        b"NO-UNDO",
        b"NOT",
        b"OBJECT",
        b"OF",
        b"ON",
        b"OR",
        b"OUTPUT",
        b"OVERRIDE",
        b"PARAMETER",
        b"PERSISTENT",
        b"PRIVATE",
        b"PROCEDURE",
        b"PROPERTY",
        b"PROTECTED",
        b"PUBLIC",
        b"QUERY",
        b"QUIT",
        b"RAW",
        b"READ-ONLY",
        b"RECORD",
        b"REPEAT",
        b"REPLACE",
        b"RETURN",
        b"RETURNS",
        b"RUN",
        b"SESSION",
        b"SET",
        b"SHARED",
        b"STATIC",
        b"STOP",
        b"STREAM",
        b"STRING",
        b"SUBSTRING",
        b"SUPER",
        b"TABLE",
        b"TEMP-TABLE",
        b"THEN",
        b"THIS-OBJECT",
        b"THROW",
        b"TODAY",
        b"TRANSACTION",
        b"TRUE",
        b"TRUNCATE",
        b"UNDO",
        b"UPDATE",
        b"USING",
        b"VALID-HANDLE",
        b"VARIABLE",
        b"VIEW-AS",
        b"WHEN",
        b"WHERE",
        b"WHILE",
        b"WITH",
        b"YES-NO",
    ];
    WORDS.iter().any(|word| bytes.eq_ignore_ascii_case(word))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn kinds(profile: SyntaxProfile, bytes: &[u8], chunk: usize) -> Vec<SyntaxKind> {
        let mut result = vec![SyntaxKind::Plain; bytes.len()];
        let mut state = LexerState::default();
        for (part, data) in bytes.chunks(chunk).enumerate() {
            let start = part * chunk;
            scan(
                profile,
                &mut state,
                data,
                start,
                start + data.len() == bytes.len(),
                |token| result[token.source].fill(token.kind),
            );
        }
        result
    }
    #[test]
    fn abl_nested_comments_hyphenated_words_and_literal_escapes_survive_every_chunk_size() {
        let source = b"dEfInE VARIABLE x AS CHARACTER NO-UNDO. /* outer /* inner */ end */\nMESSAGE 'a''b~'c' \"x~\r\ny\".\n// not an ABL comment\n{inc.i &x='}'}\n &SCOPED-DEFINE x value\n";
        let expected = kinds(SyntaxProfile::Abl, source, source.len());
        for chunk in 1..=17 {
            assert_eq!(kinds(SyntaxProfile::Abl, source, chunk), expected);
        }
        assert_eq!(expected[0], SyntaxKind::Keyword);
        let comment = source.windows(2).position(|bytes| bytes == b"/*").unwrap();
        assert_eq!(expected[comment], SyntaxKind::Comment);
        let legacy_slash = source.windows(2).position(|bytes| bytes == b"//").unwrap();
        assert_eq!(expected[legacy_slash], SyntaxKind::Operator);
        let include = source.iter().position(|byte| *byte == b'{').unwrap();
        assert_eq!(expected[include], SyntaxKind::Preprocessor);
    }
    #[test]
    fn json_and_jsonc_have_explicit_distinct_comment_capabilities() {
        let source = br#"{"key":true,"n":-1.2e+3,"bad":01} // note
/* rest */"#;
        let json = kinds(SyntaxProfile::Json, source, 1);
        let jsonc = kinds(SyntaxProfile::Jsonc, source, 1);
        let slash = source.windows(2).position(|bytes| bytes == b"//").unwrap();
        assert_ne!(json[slash], SyntaxKind::Comment);
        assert_eq!(jsonc[slash], SyntaxKind::Comment);
        let bad = source.windows(2).position(|bytes| bytes == b"01").unwrap();
        assert_eq!(json[bad], SyntaxKind::Plain);
        assert!(kinds(SyntaxProfile::Plain, source, 1)
            .iter()
            .all(|kind| *kind == SyntaxKind::Plain));
    }
    #[test]
    fn non_ascii_identifier_bytes_never_create_false_ascii_keywords_or_decode_source() {
        let source = b"\xffIF IF\xff DEFINE long-identifier 42";
        let result = kinds(SyntaxProfile::Abl, source, 1);
        assert!(result[..8].iter().all(|kind| *kind == SyntaxKind::Plain));
        assert_eq!(result[8], SyntaxKind::Keyword);
        assert_eq!(*result.last().unwrap(), SyntaxKind::Number);
        let long = vec![b'a'; 4096];
        assert!(kinds(SyntaxProfile::Abl, &long, 7)
            .iter()
            .all(|kind| *kind == SyntaxKind::Plain));
    }
    #[test]
    fn abl_numeric_period_terminates_statement_without_accepting_invalid_json_number() {
        let source = b"DISPLAY 42. 1.2.\r\n";
        for chunk in 1..=source.len() {
            let result = kinds(SyntaxProfile::Abl, source, chunk);
            assert!(result[8..10].iter().all(|kind| *kind == SyntaxKind::Number));
            assert_eq!(result[10], SyntaxKind::Operator);
            assert!(result[12..15]
                .iter()
                .all(|kind| *kind == SyntaxKind::Number));
            assert_eq!(result[15], SyntaxKind::Operator);
        }
        assert!(kinds(SyntaxProfile::Json, b"42.", 1)
            .iter()
            .all(|kind| *kind == SyntaxKind::Plain));
    }
    fn segment(source: &[u8], needle: &[u8]) -> Range<usize> {
        let start = source
            .windows(needle.len())
            .position(|part| part == needle)
            .expect("fixture segment");
        start..start + needle.len()
    }
    #[test]
    fn rust_keywords_nested_comments_multiline_and_raw_strings_are_piece_independent() {
        let source = b"pub fn demo<'a>(v: &'static str) { let r#type = 0xff_u8; // line\n/* outer /* inner */ end */ let s = \"first\nsecond\"; let raw = r##\"body\n\"#inside\"##; let bytes = br#\"byte raw\"#; }";
        let expected = kinds(SyntaxProfile::Rust, source, source.len());
        for chunk in 1..=37 {
            assert_eq!(
                kinds(SyntaxProfile::Rust, source, chunk),
                expected,
                "chunk {chunk}"
            );
        }
        for (needle, kind) in [
            (b"pub".as_slice(), SyntaxKind::Keyword),
            (b"fn".as_slice(), SyntaxKind::Keyword),
            (b"r#type".as_slice(), SyntaxKind::Plain),
            (b"0xff_u8".as_slice(), SyntaxKind::Number),
            (b"// line".as_slice(), SyntaxKind::Comment),
            (
                b"/* outer /* inner */ end */".as_slice(),
                SyntaxKind::Comment,
            ),
            (b"\"first\nsecond\"".as_slice(), SyntaxKind::String),
            (b"r##\"body\n\"#inside\"##".as_slice(), SyntaxKind::String),
            (b"br#\"byte raw\"#".as_slice(), SyntaxKind::String),
        ] {
            assert!(
                expected[segment(source, needle)]
                    .iter()
                    .all(|actual| *actual == kind),
                "{needle:?}"
            );
        }
    }
    #[test]
    fn rust_characters_byte_literals_and_lifetimes_do_not_invent_multiline_strings() {
        let source = "'a 'static 'label: 'x' 'é' '😀' '\\n' '\\x7F' '\\u{1F600}' b'x' b'\\xFF' b\"bytes\"\nfn tail() {}".as_bytes();
        let expected = kinds(SyntaxProfile::Rust, source, source.len());
        for chunk in 1..=19 {
            assert_eq!(kinds(SyntaxProfile::Rust, source, chunk), expected);
        }
        for needle in [
            "'x'",
            "'é'",
            "'😀'",
            "'\\n'",
            "'\\x7F'",
            "'\\u{1F600}'",
            "b'x'",
            "b'\\xFF'",
            "b\"bytes\"",
        ] {
            assert!(
                expected[segment(source, needle.as_bytes())]
                    .iter()
                    .all(|kind| *kind == SyntaxKind::String),
                "{needle}"
            );
        }
        for needle in [
            b"'a".as_slice(),
            b"'static".as_slice(),
            b"'label".as_slice(),
        ] {
            assert!(expected[segment(source, needle)]
                .iter()
                .all(|kind| *kind == SyntaxKind::Preprocessor));
        }
        assert!(expected[segment(source, b"fn tail")]
            .iter()
            .all(|kind| *kind != SyntaxKind::String));
    }
    #[test]
    fn rust_numeric_bases_suffixes_exponents_underscores_and_ranges_have_exact_spans() {
        let source = b"0xff_u8 0b1010u16 0o77 1_000usize 1.5e-3f64 42u128 1..=4 01 0b2 0xFE-1";
        let expected = kinds(SyntaxProfile::Rust, source, source.len());
        for chunk in 1..=11 {
            assert_eq!(kinds(SyntaxProfile::Rust, source, chunk), expected);
        }
        for needle in [
            b"0xff_u8".as_slice(),
            b"0b1010u16".as_slice(),
            b"0o77".as_slice(),
            b"1_000usize".as_slice(),
            b"1.5e-3f64".as_slice(),
            b"42u128".as_slice(),
            b"01".as_slice(),
            b"0xFE".as_slice(),
        ] {
            assert!(
                expected[segment(source, needle)]
                    .iter()
                    .all(|kind| *kind == SyntaxKind::Number),
                "{needle:?}"
            );
        }
        let range = segment(source, b"1..=4");
        assert_eq!(expected[range.start], SyntaxKind::Number);
        assert!(expected[range.start + 1..range.end - 1]
            .iter()
            .all(|kind| *kind == SyntaxKind::Operator));
        assert_eq!(expected[range.end - 1], SyntaxKind::Number);
        assert!(expected[segment(source, b"0b2")]
            .iter()
            .all(|kind| *kind == SyntaxKind::Plain));
    }
    #[test]
    fn rust_raw_hash_limit_and_delimiter_edits_are_explicit_and_finite() {
        let hashes = "#".repeat(usize::from(RUST_RAW_HASH_LIMIT));
        let source = format!("r{hashes}\"body\"{hashes} let tail = true;");
        let bytes = source.as_bytes();
        let expected = kinds(SyntaxProfile::Rust, bytes, 1);
        assert_eq!(expected[segment(bytes, b"let").start], SyntaxKind::Keyword);
        let closing = segment(bytes, format!("\"{hashes} let").as_bytes()).start + 1;
        let mut changed = bytes.to_vec();
        changed.remove(closing);
        let changed_kinds = kinds(SyntaxProfile::Rust, &changed, 1);
        assert_eq!(
            changed_kinds[segment(&changed, b"let").start],
            SyntaxKind::String
        );
        changed.insert(closing, b'#');
        assert_eq!(kinds(SyntaxProfile::Rust, &changed, 1), expected);
        let excessive = format!(
            "r{}\"body\"",
            "#".repeat(usize::from(RUST_RAW_HASH_LIMIT) + 1)
        );
        let mut state = LexerState::default();
        let mut spans = Vec::new();
        for (offset, byte) in excessive.as_bytes().iter().enumerate() {
            scan(
                SyntaxProfile::Rust,
                &mut state,
                std::slice::from_ref(byte),
                offset,
                offset + 1 == excessive.len(),
                |span| spans.push(span),
            );
        }
        assert!(state.limited());
        assert!(spans.iter().all(|span| span.kind == SyntaxKind::Plain));
    }
}
