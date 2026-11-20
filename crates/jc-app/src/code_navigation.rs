//! Shared lexical navigation for C#, Java, PHP and Rust.
use crate::{
    source_services::{FileResolution, SourceAction, SourceItem, SourceProviderDescriptor},
    Cancellation, TaskFailure, TaskItem, TaskOutput, TaskSnapshot,
};
use jc_languages::syntax::{scan, LexerState, SyntaxKind, SyntaxProfile};
use jc_text::{decode_fragment, TextEncoding};
use std::ops::Range;

pub(crate) mod routines;
pub(crate) mod rust;

const CSHARP: u8 = 0;
const JAVA: u8 = 1;
const PHP: u8 = 2;

pub(crate) fn descriptors() -> [SourceProviderDescriptor; 3] {
    let descriptor =
        |id, language, symbols, dependencies, routines, file_resolution| SourceProviderDescriptor {
            id,
            language,
            symbols: Some(symbols),
            references: Some(dependencies),
            calls: None,
            callers: None,
            routines: Some(routines),
            dependencies: Some(dependencies),
            file_resolution,
            completion: None,
            rename: None,
            snippets: &[],
        };
    [
        descriptor(
            "core.csharp",
            "csharp",
            symbols::<CSHARP>,
            dependencies::<CSHARP>,
            routines::analyze::<0>,
            FileResolution::ProjectImports,
        ),
        descriptor(
            "core.java",
            "java",
            symbols::<JAVA>,
            dependencies::<JAVA>,
            routines::analyze::<1>,
            FileResolution::ProjectImports,
        ),
        descriptor(
            "core.php",
            "php",
            symbols::<PHP>,
            dependencies::<PHP>,
            routines::analyze::<2>,
            FileResolution::DocumentRelative,
        ),
    ]
}

const MAX_TOKENS: usize = 32_768;
const CHUNK: usize = 4096;
const MAX_LOOKAHEAD: usize = 256;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Word,
    Punct(u8),
    String,
    Other,
}

struct Token {
    kind: Kind,
    range: Range<usize>,
}

fn tokens(
    profile: SyntaxProfile,
    source: &[u8],
    encoding: TextEncoding,
    cancel: &Cancellation,
) -> Result<Vec<Token>, TaskFailure> {
    let mut state = LexerState::default();
    let mut tokens: Vec<Token> = Vec::new();
    let mut limited = false;
    let mut php_code = profile != SyntaxProfile::Php;
    let start =
        usize::from(encoding == TextEncoding::Utf8 && source.starts_with(b"\xef\xbb\xbf")) * 3;
    for (index, chunk) in source[start..].chunks(CHUNK).enumerate() {
        if cancel.is_cancelled() {
            return Err(TaskFailure::Cancelled);
        }
        let offset = start + index * CHUNK;
        scan(
            profile,
            &mut state,
            chunk,
            offset,
            offset + chunk.len() == source.len(),
            |span| {
                let bytes = &source[span.source.clone()];
                if profile == SyntaxProfile::Php && span.kind == SyntaxKind::Preprocessor {
                    php_code = bytes.starts_with(b"<?");
                    return;
                }
                if !php_code {
                    return;
                }
                let kind = match span.kind {
                    SyntaxKind::String => Kind::String,
                    SyntaxKind::Comment | SyntaxKind::Preprocessor => return,
                    SyntaxKind::Operator if bytes.len() == 1 => Kind::Punct(bytes[0]),
                    kind if kind.is_word()
                        && bytes.first().is_some_and(|b| {
                            b.is_ascii_alphabetic() || matches!(b, b'_' | b'$') || *b >= 128
                        }) =>
                    {
                        Kind::Word
                    }
                    SyntaxKind::Plain if bytes.iter().all(u8::is_ascii_whitespace) => return,
                    _ => Kind::Other,
                };
                if matches!(kind, Kind::String | Kind::Word) {
                    if let Some(last) = tokens
                        .last_mut()
                        .filter(|last| last.kind == kind && last.range.end == span.source.start)
                    {
                        last.range.end = span.source.end;
                        return;
                    }
                }
                if tokens.len() == MAX_TOKENS {
                    limited = true;
                } else {
                    tokens.push(Token {
                        kind,
                        range: span.source,
                    });
                }
            },
        );
        if limited || state.limited() {
            return Err(TaskFailure::LimitExceeded);
        }
    }
    Ok(tokens)
}

struct Source<'a> {
    bytes: &'a [u8],
    encoding: TextEncoding,
    profile: SyntaxProfile,
}
impl Source<'_> {
    fn word(&self, token: &Token, word: &[u8]) -> bool {
        token.kind == Kind::Word
            && if self.profile == SyntaxProfile::Php {
                self.bytes[token.range.clone()].eq_ignore_ascii_case(word)
            } else {
                &self.bytes[token.range.clone()] == word
            }
    }

    fn text(&self, range: Range<usize>) -> Option<String> {
        if range.len() > 384 {
            return None;
        }
        let decoded = decode_fragment(&self.bytes[range.clone()], self.encoding, range.start);
        (decoded.issues.is_empty() && !decoded.text.chars().any(char::is_control))
            .then_some(decoded.text)
    }

    fn outline(
        &self,
        tokens: &[Token],
        cancel: &Cancellation,
        out: &mut TaskOutput,
    ) -> Result<(), TaskFailure> {
        let mut depth = 0usize;
        let mut type_bodies: Vec<(usize, &Token)> = Vec::new();
        let mut pending_type = None;
        for (i, token) in tokens.iter().enumerate() {
            if cancel.is_cancelled() {
                return Err(TaskFailure::Cancelled);
            }
            match token.kind {
                Kind::Punct(b'{') => {
                    depth += 1;
                    if let Some(name) = pending_type.take() {
                        type_bodies.push((depth, name));
                    }
                }
                Kind::Punct(b'}') => {
                    if type_bodies.last().is_some_and(|(level, _)| *level == depth) {
                        type_bodies.pop();
                    }
                    depth = depth.saturating_sub(1);
                    pending_type = None;
                }
                Kind::Punct(b';') => pending_type = None,
                Kind::Word => {
                    let previous = i.checked_sub(1).and_then(|j| tokens.get(j));
                    if previous.is_some_and(|t| matches!(t.kind, Kind::Punct(b'.' | b':' | b'\\')))
                    {
                        continue;
                    }
                    let kind = [
                        (b"class".as_slice(), "Clase"),
                        (b"interface", "Interfaz"),
                        (b"enum", "Enumeración"),
                        (b"record", "Registro"),
                        (b"struct", "Estructura"),
                        (b"trait", "Trait"),
                        (b"namespace", "Espacio de nombres"),
                        (b"package", "Paquete"),
                        (b"function", "Función"),
                    ]
                    .into_iter()
                    .find(|(word, _)| self.word(token, word));
                    if let Some((word, label)) = kind.filter(|(word, _)| match *word {
                        b"struct" => self.profile == SyntaxProfile::CSharp,
                        b"record" => self.profile != SyntaxProfile::Php,
                        b"package" => self.profile == SyntaxProfile::Java,
                        b"namespace" => self.profile != SyntaxProfile::Java,
                        b"function" | b"trait" => self.profile == SyntaxProfile::Php,
                        _ => true,
                    }) {
                        let mut next = i + 1;
                        if word == b"record"
                            && self.profile == SyntaxProfile::CSharp
                            && tokens
                                .get(next)
                                .is_some_and(|t| self.word(t, b"class") || self.word(t, b"struct"))
                        {
                            next += 1;
                        }
                        if word == b"function"
                            && tokens
                                .get(next)
                                .is_some_and(|t| t.kind == Kind::Punct(b'&'))
                        {
                            next += 1;
                        }
                        let Some(name) = tokens.get(next).filter(|t| t.kind == Kind::Word) else {
                            continue;
                        };
                        if matches!(
                            word,
                            b"class" | b"interface" | b"enum" | b"record" | b"struct" | b"trait"
                        ) {
                            pending_type = Some(name);
                        }
                        if matches!(word, b"package" | b"namespace") {
                            let end = qualified(
                                tokens,
                                next,
                                if self.profile == SyntaxProfile::Php {
                                    b'\\'
                                } else {
                                    b'.'
                                },
                            );
                            self.emit_symbol(
                                name.range.start..tokens[end - 1].range.end,
                                label,
                                out,
                            )?;
                        } else if !previous.is_some_and(|t| self.word(t, b"record")) {
                            self.emit_symbol(name.range.clone(), label, out)?;
                        }
                    } else if self.profile != SyntaxProfile::Php
                        && type_bodies.last().is_some_and(|(level, name)| {
                            *level == depth && self.member_method(tokens, i, name)
                        })
                    {
                        self.emit_symbol(token.range.clone(), "Método", out)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn member_method(&self, tokens: &[Token], i: usize, type_name: &Token) -> bool {
        let method = &tokens[i];
        if tokens
            .get(i + 1)
            .is_none_or(|t| t.kind != Kind::Punct(b'('))
        {
            return false;
        }
        if [
            b"this".as_slice(),
            b"base",
            b"super",
            b"new",
            b"if",
            b"while",
            b"for",
            b"switch",
            b"catch",
        ]
        .into_iter()
        .any(|word| self.word(method, word))
        {
            return false;
        }
        let constructor = self.bytes[method.range.clone()] == self.bytes[type_name.range.clone()];
        let prefix = tokens[..i]
            .iter()
            .rev()
            .take(64)
            .take_while(|t| !matches!(t.kind, Kind::Punct(b';' | b'{' | b'}')));
        let prior = i.checked_sub(1).and_then(|j| tokens.get(j));
        if !(constructor
            || prior
                .is_some_and(|t| matches!(t.kind, Kind::Word | Kind::Punct(b'>' | b']' | b'?'))))
            || prefix.clone().any(|t| t.kind == Kind::Punct(b'='))
            || prior.is_some_and(|t| self.word(t, b"new") || t.kind == Kind::Punct(b'@'))
        {
            return false;
        }
        let mut nesting = 1;
        let Some(end) = tokens
            .iter()
            .enumerate()
            .skip(i + 2)
            .take(MAX_LOOKAHEAD)
            .find_map(|(j, token)| {
                match token.kind {
                    Kind::Punct(b'(') => nesting += 1,
                    Kind::Punct(b')') => nesting -= 1,
                    _ => {}
                }
                (nesting == 0).then_some(j)
            })
        else {
            return false;
        };
        let Some(after) = tokens.get(end + 1) else {
            return false;
        };
        matches!(after.kind, Kind::Punct(b'{' | b';'))
            || self.profile == SyntaxProfile::CSharp
                && (self.word(after, b"where")
                    || after.kind == Kind::Punct(b'=')
                        && tokens
                            .get(end + 2)
                            .is_some_and(|t| t.kind == Kind::Punct(b'>')))
            || self.profile == SyntaxProfile::Java && self.word(after, b"throws")
    }

    fn emit_symbol(
        &self,
        range: Range<usize>,
        kind: &str,
        out: &mut TaskOutput,
    ) -> Result<(), TaskFailure> {
        if let Some(name) = self.text(range.clone()) {
            out.push(TaskItem::Source(SourceItem {
                value_type: None,
                range,
                label: format!("{kind}: {name}"),
                action: SourceAction::Navigate,
            }))?;
        }
        Ok(())
    }

    fn dependencies(
        &self,
        tokens: &[Token],
        cancel: &Cancellation,
        out: &mut TaskOutput,
    ) -> Result<(), TaskFailure> {
        for (i, token) in tokens.iter().enumerate() {
            if cancel.is_cancelled() {
                return Err(TaskFailure::Cancelled);
            }
            let result = match self.profile {
                SyntaxProfile::Php
                    if [
                        b"include".as_slice(),
                        b"include_once",
                        b"require",
                        b"require_once",
                    ]
                    .into_iter()
                    .any(|word| self.word(token, word)) =>
                {
                    self.php_include(tokens, i + 1)
                }
                SyntaxProfile::CSharp if self.word(token, b"using") => self.import(tokens, i + 1),
                SyntaxProfile::Java if self.word(token, b"import") => self.import(tokens, i + 1),
                _ => None,
            };
            if let Some(item) = result {
                out.push(TaskItem::Source(item))?;
            }
        }
        Ok(())
    }

    fn php_include(&self, tokens: &[Token], mut at: usize) -> Option<SourceItem> {
        let parens = tokens.get(at)?.kind == Kind::Punct(b'(');
        at += usize::from(parens);
        let anchored =
            self.word(tokens.get(at)?, b"__DIR__") && tokens.get(at + 1)?.kind == Kind::Punct(b'.');
        at += if anchored { 2 } else { 0 };
        let token = tokens.get(at)?;
        let next = tokens.get(at + 1)?.kind;
        if token.kind != Kind::String
            || next != Kind::Punct(if parens { b')' } else { b';' })
            || parens && tokens.get(at + 2)?.kind != Kind::Punct(b';')
        {
            return None;
        }
        let literal = &self.bytes[token.range.clone()];
        let quote = *literal.first()?;
        if literal.len() < 3 || !matches!(quote, b'\'' | b'"') || literal.last() != Some(&quote) {
            return None;
        }
        let name = self.text(token.range.start + 1..token.range.end - 1)?;
        let path = if anchored {
            name.strip_prefix('/')?
        } else {
            &name
        };
        let action = if path.is_empty()
            || path.starts_with('/')
            || path.contains(['\\', ':', '$', '?', '*', '#', '%', '\'', '"', '<', '>', '|'])
        {
            SourceAction::Unresolved("Inclusión PHP externa o calculada".into())
        } else {
            SourceAction::RelativeFile(path.into())
        };
        Some(SourceItem {
            value_type: None,
            range: token.range.clone(),
            label: format!("Inclusión: {name}"),
            action,
        })
    }

    fn import(&self, tokens: &[Token], mut at: usize) -> Option<SourceItem> {
        let csharp = self.profile == SyntaxProfile::CSharp;
        let is_static = self.word(tokens.get(at)?, b"static");
        at += usize::from(is_static);
        let alias = csharp && tokens.get(at + 1)?.kind == Kind::Punct(b'=');
        if alias && tokens.get(at)?.kind != Kind::Word {
            return None;
        }
        at += if alias { 2 } else { 0 };
        if csharp
            && self.word(tokens.get(at)?, b"global")
            && tokens.get(at + 1)?.kind == Kind::Punct(b':')
            && tokens.get(at + 2)?.kind == Kind::Punct(b':')
        {
            at += 3;
        }
        let first = tokens.get(at)?.range.start;
        let end = qualified(tokens, at, b'.');
        if end == at {
            return None;
        }
        let wildcard = !csharp
            && tokens.get(end)?.kind == Kind::Punct(b'.')
            && tokens.get(end + 1)?.kind == Kind::Punct(b'*');
        let after = end + if wildcard { 2 } else { 0 };
        if tokens.get(after)?.kind != Kind::Punct(b';') {
            return None;
        }
        let parts = tokens[at..end]
            .iter()
            .step_by(2)
            .map(|t| self.text(t.range.clone()))
            .collect::<Option<Vec<_>>>()?;
        let target_parts = if !csharp && is_static && !wildcard {
            parts.len().checked_sub(1)?
        } else {
            parts.len()
        };
        let mut label = parts.join(".");
        if wildcard {
            label.push_str(".*");
        }
        if label.len() > 384 {
            return None;
        }
        let action =
            if csharp && !(is_static || alias) || wildcard && !is_static || target_parts == 0 {
                SourceAction::Unresolved("Importación de espacio de nombres o paquete".into())
            } else {
                SourceAction::RelativeFile(format!(
                    "{}.{}",
                    parts[..target_parts].join("/"),
                    if csharp { "cs" } else { "java" }
                ))
            };
        Some(SourceItem {
            value_type: None,
            range: first..tokens[after - 1].range.end,
            label: format!("Importación: {label}"),
            action,
        })
    }
}

fn qualified(tokens: &[Token], start: usize, delimiter: u8) -> usize {
    if tokens.get(start).is_none_or(|t| t.kind != Kind::Word) {
        return start;
    }
    let mut end = start + 1;
    while end - start < 63
        && tokens
            .get(end)
            .is_some_and(|t| t.kind == Kind::Punct(delimiter))
        && tokens.get(end + 1).is_some_and(|t| t.kind == Kind::Word)
    {
        end += 2;
    }
    end
}

fn run<const LANGUAGE: u8>(
    snapshot: &TaskSnapshot,
    cancel: &Cancellation,
    output: &mut TaskOutput,
    symbols: bool,
) -> Result<(), TaskFailure> {
    let profile = match LANGUAGE {
        CSHARP => SyntaxProfile::CSharp,
        JAVA => SyntaxProfile::Java,
        PHP => SyntaxProfile::Php,
        _ => unreachable!(),
    };
    let source = Source {
        bytes: snapshot.bytes(),
        encoding: snapshot.stamp.encoding,
        profile,
    };
    let tokens = tokens(profile, source.bytes, source.encoding, cancel)?;
    if symbols {
        source.outline(&tokens, cancel, output)
    } else {
        source.dependencies(&tokens, cancel, output)
    }
}

fn symbols<const LANGUAGE: u8>(
    snapshot: &TaskSnapshot,
    cancel: &Cancellation,
    out: &mut TaskOutput,
) -> Result<(), TaskFailure> {
    run::<LANGUAGE>(snapshot, cancel, out, true)
}
fn dependencies<const LANGUAGE: u8>(
    snapshot: &TaskSnapshot,
    cancel: &Cancellation,
    out: &mut TaskOutput,
) -> Result<(), TaskFailure> {
    run::<LANGUAGE>(snapshot, cancel, out, false)
}
