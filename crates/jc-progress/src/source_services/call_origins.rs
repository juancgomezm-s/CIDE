//! Lightweight lexical owners for the reverse RUN and inclusion index.
use super::{local_call, reference, text};
use jc_app::{
    source_services::{display_ui_label, CallerRoutine, SourceAction, SourceItem, SourceMode},
    Cancellation, TaskFailure, TaskItem, TaskOutput, TaskSnapshot,
};
use jc_languages::syntax::{scan, LexerState, SyntaxKind, SyntaxProfile, TokenSpan};
use std::{
    collections::{HashMap, HashSet},
    ops::Range,
};

const ROUTINES: &[&[u8]] = &[
    b"PROCEDURE",
    b"FUNCTION",
    b"METHOD",
    b"CONSTRUCTOR",
    b"DESTRUCTOR",
];
const BLOCKS: &[&[u8]] = &[b"DO", b"FOR", b"REPEAT", b"CASE", b"CATCH", b"FINALLY"];

fn one_of(word: &[u8], names: &[&[u8]]) -> bool {
    names.iter().any(|name| word.eq_ignore_ascii_case(name))
}

struct Scope {
    kind: Range<usize>,
    routine: CallerRoutine,
    blocks: usize,
}

#[derive(Default)]
pub(super) struct CallOrigins {
    scope: Option<Scope>,
    statement: Statement,
}

#[derive(Default)]
struct Statement {
    first: Option<Range<usize>>,
    second: Option<Range<usize>>,
    last: Option<Range<usize>>,
    method_name: Option<Range<usize>>,
    parentheses: usize,
    embedded_block: bool,
}

impl CallOrigins {
    pub(super) fn current(&self) -> Option<&CallerRoutine> {
        self.scope.as_ref().map(|scope| &scope.routine)
    }

    pub(super) fn observe(&mut self, snapshot: &TaskSnapshot, token: &TokenSpan) {
        let bytes = snapshot.bytes();
        let word = &bytes[token.source.clone()];
        if matches!(token.kind, SyntaxKind::Comment | SyntaxKind::Preprocessor)
            || word.iter().all(u8::is_ascii_whitespace)
        {
            return;
        }
        let statement = &mut self.statement;
        let slice =
            |range: &Option<Range<usize>>| range.as_ref().map_or(&b""[..], |r| &bytes[r.clone()]);
        let first = slice(&statement.first);
        let last = slice(&statement.last);
        if token.kind == SyntaxKind::Operator {
            match word {
                b"(" => {
                    if statement.parentheses == 0
                        && statement.method_name.is_none()
                        && one_of(first, &ROUTINES[2..])
                    {
                        statement.method_name = statement.last.clone();
                    }
                    statement.parentheses += 1;
                }
                b")" => statement.parentheses = statement.parentheses.saturating_sub(1),
                b":" | b";" | b"." if statement.parentheses == 0 => {
                    let after = &bytes[token.source.end..];
                    if word != b"."
                        || after.first().is_none_or(u8::is_ascii_whitespace)
                        || after.starts_with(b"/*")
                    {
                        self.end_statement(snapshot, word);
                        return;
                    }
                }
                _ => {}
            }
        }
        if statement.parentheses == 0 && one_of(word, &BLOCKS[..3]) {
            statement.embedded_block |=
                one_of(last, &[b"THEN", b"ELSE"]) || first.eq_ignore_ascii_case(b"ON");
        }
        if statement.first.is_none() {
            statement.first = Some(token.source.clone());
        } else if statement.second.is_none() {
            statement.second = Some(token.source.clone());
        }
        statement.last = Some(token.source.clone());
    }

    fn end_statement(&mut self, snapshot: &TaskSnapshot, end: &[u8]) {
        let statement = std::mem::take(&mut self.statement);
        let Some(first) = statement.first else { return };
        let bytes = snapshot.bytes();
        let word = &bytes[first.clone()];
        if end == b":" && one_of(word, ROUTINES) {
            let name = if one_of(word, &ROUTINES[..2]) {
                statement.second
            } else {
                statement.method_name
            };
            if let Some(name) = name.and_then(|range| text(snapshot, range)) {
                self.scope = Some(Scope {
                    routine: CallerRoutine {
                        declaration: first.start,
                        name,
                    },
                    kind: first,
                    blocks: 0,
                });
            }
        } else if let Some(scope) = &mut self.scope {
            if end == b"." && word.eq_ignore_ascii_case(b"END") {
                let bare = statement.second.is_none();
                let explicit = statement.second.is_some_and(|range| {
                    bytes[range].eq_ignore_ascii_case(&bytes[scope.kind.clone()])
                });
                if explicit || bare && scope.blocks == 0 {
                    self.scope = None;
                } else {
                    scope.blocks = scope.blocks.saturating_sub(1);
                }
            } else if end == b":" && (one_of(word, BLOCKS) || statement.embedded_block) {
                scope.blocks += 1;
            }
        }
    }
}

struct Routine {
    name: CallerRoutine,
    end: usize,
    procedure: bool,
}

struct Call {
    owner: Option<usize>,
    site: SourceItem,
    run: bool,
}

pub(super) fn relations(
    snapshot: &TaskSnapshot,
    cancel: &Cancellation,
    cursor: usize,
    mode: SourceMode,
    output: &mut TaskOutput,
) -> Result<bool, TaskFailure> {
    let bytes = snapshot.bytes();
    let mut origins = CallOrigins::default();
    let mut lexer = LexerState::default();
    let mut routines: Vec<Routine> = Vec::new();
    let mut calls = Vec::new();
    let mut owner: Option<usize> = None;
    let mut word_before: Option<Range<usize>> = None;
    let mut qualified = false;
    let mut after_run = false;
    let mut limited = false;
    for (chunk_index, chunk) in bytes.chunks(4096).enumerate() {
        if cancel.is_cancelled() {
            return Err(TaskFailure::Cancelled);
        }
        let offset = chunk_index * 4096;
        scan(
            SyntaxProfile::Abl,
            &mut lexer,
            chunk,
            offset,
            offset + chunk.len() == bytes.len(),
            |token| {
                if limited || matches!(token.kind, SyntaxKind::Comment | SyntaxKind::Preprocessor) {
                    return;
                }
                let raw = &bytes[token.source.clone()];
                if raw.iter().all(u8::is_ascii_whitespace) {
                    return;
                }
                origins.observe(snapshot, &token);
                let current = origins.current();
                if current.map(|r| r.declaration) != owner.map(|i| routines[i].name.declaration) {
                    if let Some(i) = owner {
                        routines[i].end = token.source.end;
                    }
                    owner = current.map(|routine| {
                        let id = routines.len();
                        let procedure = origins.scope.as_ref().is_some_and(|scope| {
                            bytes[scope.kind.clone()].eq_ignore_ascii_case(b"PROCEDURE")
                        });
                        routines.push(Routine {
                            name: routine.clone(),
                            end: bytes.len(),
                            procedure,
                        });
                        id
                    });
                }
                if raw.eq_ignore_ascii_case(b"RUN") && token.kind.is_word() {
                    calls.push(Call {
                        owner,
                        site: reference(snapshot, token.source.clone(), false),
                        run: true,
                    });
                } else if raw == b"(" {
                    if let Some(range) = word_before.take() {
                        if let Some(name) = text(snapshot, range.clone()) {
                            calls.push(Call {
                                owner,
                                site: SourceItem {
                                    label: name,
                                    range,
                                    action: SourceAction::Navigate,
                                    value_type: None,
                                },
                                run: false,
                            });
                        }
                    }
                }
                let preceded_by_qualifier = qualified;
                qualified = raw == b":"
                    || raw == b"."
                        && bytes
                            .get(token.source.end)
                            .is_some_and(|byte| !byte.is_ascii_whitespace())
                        && !bytes[token.source.end..].starts_with(b"/*");
                word_before = (!preceded_by_qualifier && !after_run && token.kind.is_word())
                    .then_some(token.source);
                after_run = raw.eq_ignore_ascii_case(b"RUN");
                limited = routines.len() > 512 || calls.len() > 8192;
            },
        );
        if limited || lexer.limited() {
            return Err(TaskFailure::LimitExceeded);
        }
    }
    let Some(selected) = routines
        .iter()
        .position(|r| (r.name.declaration..r.end).contains(&cursor))
    else {
        return Ok(false);
    };
    let mut definitions = HashMap::new();
    for (i, routine) in routines.iter().enumerate() {
        definitions
            .entry((routine.name.name.to_ascii_lowercase(), routine.procedure))
            .and_modify(|location| *location = None)
            .or_insert(Some(i));
    }
    let mut seen = HashSet::new();
    for Call {
        owner,
        mut site,
        run,
    } in calls
    {
        if cancel.is_cancelled() {
            return Err(TaskFailure::Cancelled);
        }
        if mode == SourceMode::Calls && owner != Some(selected) {
            continue;
        }
        let name = if run {
            local_call(&site)
        } else {
            Some(site.label.as_str())
        };
        let target = name
            .and_then(|name| definitions.get(&(name.to_ascii_lowercase(), run)))
            .copied()
            .flatten();
        if mode == SourceMode::Incoming {
            if target != Some(selected)
                || !seen.insert(owner.map_or_else(String::new, |id| id.to_string()))
            {
                continue;
            }
            site.label = owner.map_or_else(
                || "Bloque principal".into(),
                |i| routines[i].name.name.clone(),
            );
            site.action = SourceAction::Navigate;
        } else if let Some(id) = target {
            let routine = &routines[id];
            if !seen.insert(routine.name.name.to_ascii_lowercase()) {
                continue;
            }
            site.label.clone_from(&routine.name.name);
            site.range = routine.name.declaration..routine.name.declaration;
            site.action = SourceAction::Navigate;
        } else if !run || !seen.insert(site.label.to_ascii_lowercase()) {
            continue;
        }
        site.label = display_ui_label(&site.label, 110);
        output.push(TaskItem::Source(site))?;
    }
    output.push(TaskItem::Message(display_ui_label(
        &routines[selected].name.name,
        56,
    )))?;
    Ok(true)
}
