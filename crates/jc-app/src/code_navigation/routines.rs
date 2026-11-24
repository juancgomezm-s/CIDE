//! Direct routine relationships over one versioned editor snapshot.
use super::{rust::delimiters, tokens, Kind, Source, Token};
use crate::{
    source_services::{display_ui_label, SourceAction, SourceItem, SourceMode},
    Cancellation, TaskFailure, TaskItem, TaskOutput, TaskSnapshot,
};
use jc_languages::syntax::SyntaxProfile;
use std::{
    collections::{HashMap, HashSet},
    ops::Range,
};

const MAX_ROUTINES: usize = 512;
const HEADER_TOKENS: usize = 256;

struct Routine {
    name: String,
    anchor: Range<usize>,
    body: Range<usize>,
    extent: Range<usize>,
    owner: Option<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Scope {
    Document,
    Type(usize),
    Routine(usize),
}

struct File<'a> {
    source: Source<'a>,
    tokens: &'a [Token],
    closes: &'a [Option<usize>],
}

impl File<'_> {
    fn kind(&self, at: usize) -> Kind {
        self.tokens.get(at).map_or(Kind::Other, |token| token.kind)
    }

    fn word(&self, at: usize, name: &[u8]) -> bool {
        self.tokens
            .get(at)
            .is_some_and(|token| self.source.word(token, name))
    }

    fn key(&self, name: &str) -> String {
        if self.source.profile == SyntaxProfile::Php {
            name.to_ascii_lowercase()
        } else {
            name.to_owned()
        }
    }

    fn opener(&self, mut at: usize) -> Option<usize> {
        let limit = self.tokens.len().min(at.saturating_add(HEADER_TOKENS));
        while at < limit {
            match self.kind(at) {
                Kind::Punct(b'{') => return Some(at),
                Kind::Punct(b';' | b'}') => return None,
                Kind::Punct(b'(' | b'[') => at = self.closes[at]?,
                _ => {}
            }
            at += 1;
        }
        None
    }

    fn opaque_end(&self, at: usize) -> Option<usize> {
        if self.source.profile != SyntaxProfile::Rust {
            return None;
        }
        let group = match self.kind(at) {
            Kind::Punct(b'!') if at > 0 && self.kind(at - 1) == Kind::Word => {
                at + 1 + usize::from(self.word(at - 1, b"macro_rules"))
            }
            Kind::Punct(b'#') => at + 1 + usize::from(self.kind(at + 1) == Kind::Punct(b'!')),
            _ => return None,
        };
        if !matches!(self.kind(group), Kind::Punct(b'(' | b'[' | b'{')) {
            return None;
        }
        self.closes[group]
    }

    fn after_type_parameters(&self, start: usize) -> Option<usize> {
        if self.kind(start) != Kind::Punct(b'<') {
            return Some(start);
        }
        let mut depth = 0usize;
        for at in start..self.tokens.len().min(start + HEADER_TOKENS) {
            match self.kind(at) {
                Kind::Punct(b'<') => depth += 1,
                Kind::Punct(b'>') if at > 0 && self.kind(at - 1) != Kind::Punct(b'-') => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Some(at + 1);
                    }
                }
                Kind::Punct(b';' | b'{') => return None,
                _ => {}
            }
        }
        None
    }

    fn routine_body(&self, name: usize, owner: Option<usize>) -> Option<(usize, usize)> {
        use SyntaxProfile::*;
        let profile = self.source.profile;
        let previous = name.checked_sub(1)?;
        let ecmascript = matches!(profile, JavaScript | TypeScript);
        let keyword = profile == Rust && self.word(previous, b"fn")
            || (profile == Php || ecmascript) && self.word(previous, b"function")
            || profile == Php
                && self.kind(previous) == Kind::Punct(b'&')
                && previous
                    .checked_sub(1)
                    .is_some_and(|at| self.word(at, b"function"));
        if ecmascript
            && self.kind(name + 1) == Kind::Punct(b'=')
            && [b"const".as_slice(), b"let", b"var"]
                .into_iter()
                .any(|word| self.word(previous, word))
        {
            let mut at = name + 2;
            if self.word(at, b"async") {
                at += 1;
            }
            let arrow = match self.kind(at) {
                Kind::Punct(b'(') => self.closes[at]? + 1,
                Kind::Word => at + 1,
                _ => return None,
            };
            if self.kind(arrow) == Kind::Punct(b'=')
                && self.kind(arrow + 1) == Kind::Punct(b'>')
                && self.kind(arrow + 2) == Kind::Punct(b'{')
            {
                return Some((previous, arrow + 2));
            }
        }
        let paren = if keyword {
            self.after_type_parameters(name + 1)?
        } else {
            name + 1
        };
        if self.kind(paren) != Kind::Punct(b'(') {
            return None;
        }
        let close = self.closes[paren]?;
        if keyword {
            return self.opener(close + 1).map(|open| (previous, open));
        }
        let owner = owner?;
        let member = if matches!(profile, CSharp | Java) {
            self.source
                .member_method(self.tokens, name, &self.tokens[owner])
        } else {
            ecmascript
                && !matches!(self.kind(previous), Kind::Punct(b'=' | b'.' | b':'))
                && matches!(self.kind(close + 1), Kind::Punct(b'{' | b':'))
        };
        if member {
            self.opener(close + 1).map(|open| (name, open))
        } else {
            None
        }
    }

    fn routines(&self, cancel: &Cancellation) -> Result<Vec<Routine>, TaskFailure> {
        let mut declarations = Vec::new();
        let mut owner_openers = HashMap::new();
        let mut scopes = Vec::new();
        let mut at = 0;
        while at < self.tokens.len() {
            if cancel.is_cancelled() {
                return Err(TaskFailure::Cancelled);
            }
            while scopes.last().is_some_and(|&(close, _)| at >= close) {
                scopes.pop();
            }
            // Rust macro bodies and attribute token trees do not declare items
            // in the surrounding lexical scopes.
            if let Some(close) = self.opaque_end(at) {
                at = close + 1;
                continue;
            }
            match self.kind(at) {
                Kind::Punct(b'{') => {
                    scopes.push((
                        self.closes[at].unwrap_or(self.tokens.len()),
                        owner_openers.remove(&at),
                    ));
                }
                Kind::Word => {
                    let type_header = [
                        b"class".as_slice(),
                        b"struct",
                        b"record",
                        b"interface",
                        b"trait",
                        b"impl",
                    ]
                    .into_iter()
                    .any(|word| self.word(at, word));
                    if type_header {
                        if let Some(open) = self.opener(at + 1) {
                            owner_openers.insert(open, (at + 1).min(open));
                        }
                    }
                    let owner = scopes.last().and_then(|&(_, owner)| owner);
                    if !control_word(self, at) {
                        if let Some((start, open)) = self.routine_body(at, owner) {
                            let range = self.tokens[at].range.clone();
                            if let Some(name) = self.source.text(range.clone()) {
                                if !cancel.complete_source() && declarations.len() == MAX_ROUTINES {
                                    return Err(TaskFailure::LimitExceeded);
                                }
                                let close = self.closes[open].unwrap_or(self.tokens.len());
                                let end = self
                                    .tokens
                                    .get(close)
                                    .map_or(self.source.bytes.len(), |t| t.range.end);
                                declarations.push(Routine {
                                    name,
                                    anchor: range,
                                    body: open + 1..close,
                                    extent: self.tokens[start].range.start..end,
                                    owner,
                                });
                            }
                        }
                    }
                }
                _ => {}
            }
            at += 1;
        }
        Ok(declarations)
    }

    fn receiver(&self, at: usize) -> (Option<usize>, bool) {
        let before = at.saturating_sub(1);
        let receiver = match self.kind(before) {
            Kind::Punct(b'.') => at.checked_sub(2),
            Kind::Punct(b':' | b'>')
                if matches!(
                    self.kind(before.saturating_sub(1)),
                    Kind::Punct(b':' | b'-')
                ) =>
            {
                at.checked_sub(3)
            }
            _ => return (None, false),
        };
        let own = receiver.is_some_and(|index| {
            [b"this".as_slice(), b"$this", b"self", b"Self"]
                .into_iter()
                .any(|word| self.word(index, word))
                && !matches!(
                    index.checked_sub(1).map(|previous| self.kind(previous)),
                    Some(Kind::Punct(b'.' | b':'))
                )
        });
        (receiver, !own)
    }
}

fn control_word(file: &File<'_>, at: usize) -> bool {
    [
        b"if".as_slice(),
        b"for",
        b"while",
        b"switch",
        b"catch",
        b"match",
        b"return",
        b"function",
        b"fn",
        b"new",
        b"throw",
        b"typeof",
        b"sizeof",
        b"declare",
        b"foreach",
        b"synchronized",
        b"using",
        b"lock",
        b"with",
        b"do",
        b"import",
    ]
    .into_iter()
    .any(|word| file.word(at, word))
}

pub(crate) fn analyze<const LANGUAGE: u8>(
    snapshot: &TaskSnapshot,
    cancel: &Cancellation,
    cursor: usize,
    mode: SourceMode,
    output: &mut TaskOutput,
) -> Result<bool, TaskFailure> {
    let profile = [
        SyntaxProfile::CSharp,
        SyntaxProfile::Java,
        SyntaxProfile::Php,
        SyntaxProfile::Rust,
        SyntaxProfile::JavaScript,
        SyntaxProfile::TypeScript,
    ][usize::from(LANGUAGE)];
    let source = Source {
        bytes: snapshot.bytes(),
        encoding: snapshot.stamp.encoding,
        profile,
    };
    let tokens = tokens(profile, source.bytes, source.encoding, cancel)?;
    let closes = delimiters(&tokens, cancel)?;
    let file = File {
        source,
        tokens: &tokens,
        closes: &closes,
    };
    let routines = file.routines(cancel)?;
    let selection = routines
        .iter()
        .enumerate()
        .filter(|(_, routine)| routine.extent.contains(&cursor))
        .max_by_key(|(_, routine)| routine.extent.start);
    if selection.is_none() && mode != SourceMode::FollowLink {
        return Ok(false);
    }
    let selected = selection.map(|(id, _)| id);
    let mut names: HashMap<Scope, HashMap<String, Option<usize>>> = HashMap::new();
    let mut declarations = HashSet::new();
    let mut enclosing: Vec<usize> = Vec::new();
    for (id, routine) in routines.iter().enumerate() {
        while enclosing
            .last()
            .is_some_and(|&parent| routines[parent].extent.end <= routine.extent.start)
        {
            enclosing.pop();
        }
        let scope = routine.owner.map(Scope::Type).unwrap_or_else(|| {
            enclosing
                .last()
                .copied()
                .map_or(Scope::Document, Scope::Routine)
        });
        names
            .entry(scope)
            .or_default()
            .entry(file.key(&routine.name))
            .and_modify(|name| *name = None)
            .or_insert(Some(id));
        declarations.insert(routine.anchor.start);
        enclosing.push(id);
    }
    let mut ranges: Vec<_> = routines
        .iter()
        .enumerate()
        .map(|(id, routine)| (routine.body.start, id))
        .collect();
    ranges.sort_unstable();
    let mut next = 0;
    let mut callers = Vec::new();
    let mut seen_callers = HashSet::new();
    let mut seen_targets = HashSet::new();
    let mut opaque_until = 0;
    for at in 0..tokens.len() {
        if cancel.is_cancelled() {
            return Err(TaskFailure::Cancelled);
        }
        if at < opaque_until {
            continue;
        }
        if let Some(close) = file.opaque_end(at) {
            opaque_until = close + 1;
            continue;
        }
        while callers
            .last()
            .is_some_and(|&id: &usize| at >= routines[id].body.end)
        {
            callers.pop();
        }
        while ranges.get(next).is_some_and(|&(start, _)| start <= at) {
            if let Some(&(_, id)) = ranges
                .get(next)
                .filter(|&&(_, id)| at < routines[id].body.end)
            {
                callers.push(id);
            }
            next += 1;
        }
        let caller = callers.last().copied();
        if mode == SourceMode::Calls && caller != selected {
            continue;
        }
        let token = &tokens[at];
        if mode == SourceMode::FollowLink && !token.range.contains(&cursor) {
            continue;
        }
        if token.kind != Kind::Word
            || mode != SourceMode::FollowLink && file.kind(at + 1) != Kind::Punct(b'(')
            || declarations.contains(&token.range.start)
            || control_word(&file, at)
            || at.checked_sub(1).is_some_and(|previous| {
                file.word(previous, b"new")
                    || file.word(previous, b"fn")
                    || file.word(previous, b"function")
                    || file.kind(previous) == Kind::Punct(b'@')
            })
        {
            continue;
        }
        let Some(name) = file.source.text(token.range.clone()) else {
            continue;
        };
        let key = file.key(&name);
        let (receiver, external) = file.receiver(at);
        let scope_name = |scope| names.get(&scope).and_then(|names| names.get(&key).copied());
        let owner = callers.iter().rev().find_map(|&id| routines[id].owner);
        let target = if external {
            None
        } else if receiver.is_some() {
            owner.and_then(|owner| scope_name(Scope::Type(owner)))
        } else {
            callers
                .iter()
                .rev()
                .find_map(|&id| scope_name(Scope::Routine(id)))
                .or_else(|| {
                    matches!(profile, SyntaxProfile::CSharp | SyntaxProfile::Java)
                        .then_some(owner)
                        .flatten()
                        .and_then(|owner| scope_name(Scope::Type(owner)))
                })
                .or_else(|| scope_name(Scope::Document))
        }
        .flatten();
        if mode == SourceMode::FollowLink {
            let Some(target) = target.map(|id| &routines[id]) else {
                return Ok(false);
            };
            output.push(TaskItem::Source(SourceItem {
                label: display_ui_label(&target.name, 110),
                range: target.anchor.clone(),
                action: SourceAction::Navigate,
                value_type: None,
            }))?;
            return Ok(true);
        }
        let (label, range, action) = if mode == SourceMode::Incoming {
            if target != selected || !seen_callers.insert(caller) {
                continue;
            }
            let label = caller.map_or("Bloque principal", |id| routines[id].name.as_str());
            (
                label.to_owned(),
                token.range.clone(),
                SourceAction::Navigate,
            )
        } else if let Some(id) = target {
            if !seen_targets.insert(key) {
                continue;
            }
            let target = &routines[id];
            (
                target.name.clone(),
                target.anchor.clone(),
                SourceAction::Navigate,
            )
        } else {
            let name = receiver
                .and_then(|i| file.source.text(tokens[i].range.start..token.range.end))
                .unwrap_or(name);
            if !seen_targets.insert(file.key(&name)) {
                continue;
            }
            (
                format!("{name} · sin resolver"),
                token.range.clone(),
                SourceAction::Unresolved(name),
            )
        };
        output.push(TaskItem::Source(SourceItem {
            label: display_ui_label(&label, 110),
            range,
            action,
            value_type: None,
        }))?;
    }
    if mode == SourceMode::FollowLink {
        return Ok(false);
    }
    output.push(TaskItem::Message(display_ui_label(
        &selection.unwrap().1.name,
        56,
    )))?;
    Ok(true)
}
