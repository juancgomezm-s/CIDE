//! Bounded Rust item outlines over the existing editor lexer.
use super::{tokens, Kind, Source, Token};
use crate::{
    source_services::{FileResolution, SourceAction, SourceItem, SourceProviderDescriptor},
    Cancellation, TaskFailure, TaskItem, TaskOutput, TaskSnapshot,
};
use jc_languages::syntax::SyntaxProfile;
use jc_text::decode_fragment;
use std::ops::Range;

pub(crate) fn descriptor() -> SourceProviderDescriptor {
    SourceProviderDescriptor {
        id: "core.rust",
        language: "rust",
        symbols: Some(symbols),
        references: None,
        calls: None,
        callers: None,
        routines: Some(super::routines::analyze::<3>),
        dependencies: None,
        file_resolution: FileResolution::DocumentRelative,
        completion: None,
        rename: None,
        snippets: &[],
    }
}

const MAX_SCOPE_DEPTH: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    Items,
    Members,
    Fields,
    Variants,
}

struct Outline<'a> {
    source: Source<'a>,
    tokens: &'a [Token],
    closes: Vec<Option<usize>>,
    cancel: &'a Cancellation,
}

fn symbols(
    snapshot: &TaskSnapshot,
    cancel: &Cancellation,
    out: &mut TaskOutput,
) -> Result<(), TaskFailure> {
    let source = Source {
        bytes: snapshot.bytes(),
        encoding: snapshot.stamp.encoding,
        profile: SyntaxProfile::Rust,
    };
    let tokens = tokens(source.profile, source.bytes, source.encoding, cancel)?;
    let closes = delimiters(&tokens, cancel)?;
    let end = source.bytes.len();
    Outline {
        source,
        tokens: &tokens,
        closes,
        cancel,
    }
    .scope(Scope::Items, 0..tokens.len(), end, 0, out)
}

pub(super) fn delimiters(
    tokens: &[Token],
    cancel: &Cancellation,
) -> Result<Vec<Option<usize>>, TaskFailure> {
    let mut closes = vec![None; tokens.len()];
    let mut stack = Vec::new();
    for (at, token) in tokens.iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(TaskFailure::Cancelled);
        }
        match token.kind {
            Kind::Punct(b'{' | b'[' | b'(') => stack.push((at, token.kind)),
            Kind::Punct(b'}' | b']' | b')') => {
                let expected = match token.kind {
                    Kind::Punct(b'}') => Kind::Punct(b'{'),
                    Kind::Punct(b']') => Kind::Punct(b'['),
                    _ => Kind::Punct(b'('),
                };
                if stack.last().is_some_and(|&(_, kind)| kind == expected) {
                    closes[stack.pop().unwrap().0] = Some(at);
                }
            }
            _ => {}
        }
    }
    Ok(closes)
}

impl Outline<'_> {
    fn kind(&self, at: usize) -> Kind {
        self.tokens.get(at).map_or(Kind::Other, |token| token.kind)
    }

    fn word(&self, at: usize, word: &[u8]) -> bool {
        self.tokens
            .get(at)
            .is_some_and(|token| self.source.word(token, word))
    }

    fn group_end(&self, at: usize, limit: usize) -> usize {
        self.closes[at]
            .filter(|&close| close < limit)
            .unwrap_or(limit)
    }

    fn tail(
        &self,
        mut at: usize,
        limit: usize,
        stops: &[u8],
        track_angles: bool,
    ) -> Result<usize, TaskFailure> {
        let mut angles = 0usize;
        while at < limit {
            if self.cancel.is_cancelled() {
                return Err(TaskFailure::Cancelled);
            }
            if let Kind::Punct(punctuation) = self.kind(at) {
                if angles == 0 && stops.contains(&punctuation) {
                    break;
                }
                match punctuation {
                    b'<' if track_angles => angles += 1,
                    b'>' if at == 0
                        || self.kind(at - 1) != Kind::Punct(b'-')
                        || self.tokens[at - 1].range.end != self.tokens[at].range.start =>
                    {
                        angles = angles.saturating_sub(1)
                    }
                    b'{' | b'[' | b'(' => at = self.group_end(at, limit),
                    _ => {}
                }
            }
            at += 1;
        }
        Ok(at.min(limit))
    }

    fn emit(
        &self,
        anchor: usize,
        name: String,
        kind: &str,
        extent: Option<Range<usize>>,
        out: &mut TaskOutput,
    ) -> Result<(), TaskFailure> {
        if !name.is_empty() {
            out.push(TaskItem::Source(SourceItem {
                range: self.tokens[anchor].range.clone(),
                label: format!("{name} · {kind}"),
                action: extent.map_or(SourceAction::Navigate, |extent| {
                    SourceAction::ScopedSymbol { extent }
                }),
                value_type: None,
            }))?;
        }
        Ok(())
    }

    fn name(&self, at: usize) -> Option<String> {
        (self.kind(at) == Kind::Word)
            .then(|| self.source.text(self.tokens[at].range.clone()))
            .flatten()
    }

    fn impl_name(&self, at: usize, tail: usize) -> String {
        let last = (at + 1..tail)
            .find(|&index| self.word(index, b"where"))
            .unwrap_or(tail);
        if last <= at + 1 {
            return String::from("impl");
        }
        let span = self.tokens[at + 1].range.start..self.tokens[last - 1].range.end;
        if span.len() > 384 {
            return String::from("impl");
        }
        let decoded = decode_fragment(
            &self.source.bytes[span.clone()],
            self.source.encoding,
            span.start,
        );
        if decoded.issues.is_empty() {
            return decoded
                .text
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
        }
        String::from("impl")
    }

    fn scope(
        &self,
        scope: Scope,
        range: Range<usize>,
        end: usize,
        depth: usize,
        out: &mut TaskOutput,
    ) -> Result<(), TaskFailure> {
        let mut at = range.start;
        let limit = range.end;
        while at < limit {
            if self.cancel.is_cancelled() {
                return Err(TaskFailure::Cancelled);
            }
            if matches!(self.kind(at), Kind::Punct(b'{' | b'[' | b'(')) {
                at = self.group_end(at, limit) + 1;
                continue;
            }
            if self.word(at, b"pub") {
                at += 1;
                if at < limit && self.kind(at) == Kind::Punct(b'(') {
                    at = self.group_end(at, limit) + 1;
                }
                continue;
            }
            if scope == Scope::Fields && self.kind(at + 1) == Kind::Punct(b':') {
                if let Some(name) = self.name(at) {
                    self.emit(at, name, "Campo", None, out)?;
                    at = self.tail(at + 2, limit, b",", true)? + 1;
                    continue;
                }
            }
            if scope == Scope::Variants {
                if let Some(name) = self.name(at) {
                    let next = at + 1;
                    if next < limit && self.kind(next) == Kind::Punct(b'{') {
                        let close = self.group_end(next, limit);
                        let content_end = self.body_end(close, limit, end);
                        let extent = self.tokens[at].range.start..content_end;
                        self.emit(at, name, "Variante", Some(extent), out)?;
                        if depth < MAX_SCOPE_DEPTH {
                            self.scope(
                                Scope::Fields,
                                next + 1..close,
                                content_end,
                                depth + 1,
                                out,
                            )?;
                        }
                        at = close + 1;
                    } else {
                        self.emit(at, name, "Variante", None, out)?;
                        at = self.tail(next, limit, b",", false)? + 1;
                    }
                    continue;
                }
            }
            if matches!(scope, Scope::Items | Scope::Members) {
                if let Some(next) = self.item(scope, at, limit, end, depth, out)? {
                    at = next;
                    continue;
                }
            }
            at += 1;
        }
        Ok(())
    }

    fn body_end(&self, close: usize, limit: usize, end: usize) -> usize {
        if close < limit {
            self.tokens[close].range.end
        } else {
            end
        }
    }

    fn item(
        &self,
        scope: Scope,
        at: usize,
        limit: usize,
        end: usize,
        depth: usize,
        out: &mut TaskOutput,
    ) -> Result<Option<usize>, TaskFailure> {
        if self.word(at, b"use") {
            return Ok(Some(self.tail(at + 1, limit, b";", false)? + 1));
        }
        let word = &self.source.bytes[self.tokens[at].range.clone()];
        if self.kind(at) != Kind::Word {
            return Ok(None);
        }
        let mut anchor = at + 1;
        let (kind, children) = match word {
            b"fn" if scope == Scope::Members => ("Método", None),
            b"fn" => ("Función", None),
            b"struct" | b"union" => ("Estructura", Some(Scope::Fields)),
            b"enum" => ("Enumeración", Some(Scope::Variants)),
            b"trait" => ("Rasgo", Some(Scope::Members)),
            b"impl" => {
                anchor = at;
                ("Implementación", Some(Scope::Members))
            }
            b"mod" => ("Módulo", Some(Scope::Items)),
            b"type" => ("Tipo", None),
            b"const" if self.kind(at + 2) == Kind::Punct(b':' | b'=') => ("Constante", None),
            b"static" => {
                anchor += usize::from(self.word(anchor, b"mut"));
                ("Variable", None)
            }
            b"macro_rules" if self.kind(anchor) == Kind::Punct(b'!') => {
                anchor += 1;
                ("Macro", None)
            }
            b"extern" => {
                let opener = anchor + usize::from(self.kind(anchor) == Kind::String);
                if opener < limit && self.kind(opener) == Kind::Punct(b'{') {
                    let close = self.group_end(opener, limit);
                    if depth < MAX_SCOPE_DEPTH {
                        self.scope(
                            Scope::Items,
                            opener + 1..close,
                            self.body_end(close, limit, end),
                            depth + 1,
                            out,
                        )?;
                    }
                    return Ok(Some(close + 1));
                }
                return Ok(None);
            }
            _ => return Ok(None),
        };
        if anchor >= limit {
            return Ok(None);
        }
        let name = if word == b"impl" {
            None
        } else if let Some(name) = self.name(anchor) {
            Some(name)
        } else {
            return Ok(None);
        };
        let tail = self.tail(
            anchor + 1,
            limit,
            b"{;",
            !matches!(word, b"const" | b"static"),
        )?;
        let name = name.unwrap_or_else(|| self.impl_name(at, tail));
        if tail < limit && self.kind(tail) == Kind::Punct(b'{') {
            let close = self.group_end(tail, limit);
            let content_end = self.body_end(close, limit, end);
            let extent = self.tokens[at].range.start..content_end;
            self.emit(anchor, name, kind, Some(extent), out)?;
            if let Some(children) = children.filter(|_| depth < MAX_SCOPE_DEPTH) {
                self.scope(children, tail + 1..close, content_end, depth + 1, out)?;
            }
            return Ok(Some(close + 1));
        }
        self.emit(anchor, name, kind, None, out)?;
        Ok(Some(tail + 1))
    }
}
