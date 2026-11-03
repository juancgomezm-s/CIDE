//! Optional progress.abl source provider. Lexical inference only: no AVM, XREF,
//! include expansion, filesystem access or compiler certification.
use jc_app::{
    source_services::{SourceAction, SourceItem, SourceProviderDescriptor},
    Cancellation, TaskFailure, TaskItem, TaskOutput, TaskSnapshot,
};
use jc_languages::syntax::{scan, LexerState, SyntaxKind, SyntaxProfile};
use jc_text::decode_fragment;
use std::ops::Range;
pub fn descriptor() -> SourceProviderDescriptor {
    SourceProviderDescriptor {
        id: "progress.abl",
        language: "abl",
        symbols: Some(symbols),
        references: Some(references),
        completion: None,
    }
}
struct Symbol {
    kind: &'static str,
    name: String,
    range: Range<usize>,
}
struct Model {
    symbols: Vec<Symbol>,
    references: Vec<SourceItem>,
    limited: bool,
}
fn label(mut value: String) -> String {
    if value.len() > 512 {
        let mut end = 509;
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        value.truncate(end);
        value.push_str("...");
    }
    value
}
fn text(snapshot: &TaskSnapshot, range: Range<usize>) -> Option<String> {
    if range.len() > 512 || range.end > snapshot.bytes().len() {
        return None;
    }
    let decoded = decode_fragment(
        &snapshot.bytes()[range.clone()],
        snapshot.stamp.encoding,
        range.start,
    );
    (decoded.issues.is_empty() && !decoded.text.chars().any(char::is_control))
        .then_some(decoded.text)
}
fn reference(snapshot: &TaskSnapshot, trigger: Range<usize>, include: bool) -> SourceItem {
    let bytes = snapshot.bytes();
    let start = trigger.end;
    let mut at = start;
    while at < bytes.len() && bytes[at].is_ascii_whitespace() {
        at += 1;
    }
    let quoted = bytes.get(at).copied().filter(|b| matches!(b, b'\'' | b'"'));
    if quoted.is_some() {
        at += 1;
    }
    let first = at;
    let stop = (first + 512).min(bytes.len());
    while at < stop {
        let b = bytes[at];
        if quoted == Some(b)
            || quoted.is_none() && (b.is_ascii_whitespace() || matches!(b, b'}' | b'('))
        {
            break;
        }
        at += 1;
    }
    let mut end = at;
    if quoted.is_none() && !include && bytes.get(end.wrapping_sub(1)) == Some(&b'.') {
        end -= 1;
    }
    let raw = &bytes[first..end];
    let target = text(snapshot, first..end).unwrap_or_default();
    let suffix_start = (at + usize::from(quoted.is_some())).min(bytes.len());
    let suffix = &bytes[suffix_start..(suffix_start + 512).min(bytes.len())];
    let suffix_end = suffix
        .iter()
        .position(|b| matches!(b, b'\r' | b'\n' | b'(' | b'.'))
        .unwrap_or(suffix.len());
    let foreign_context = !include
        && suffix[..suffix_end]
            .split(|b| !b.is_ascii_alphanumeric() && *b != b'-')
            .any(|word| {
                word.eq_ignore_ascii_case(b"IN")
                    || word.eq_ignore_ascii_case(b"ON")
                    || word.eq_ignore_ascii_case(b"ASYNCHRONOUS")
            });
    let unsafe_syntax = foreign_context
        || target.is_empty()
        || at == stop && stop < bytes.len()
        || raw.iter().any(|b| matches!(b, b'{' | b'}' | b'&' | b'~'))
        || quoted.is_some_and(|quote| bytes.get(at) != Some(&quote))
        || (!include && target.eq_ignore_ascii_case("value"));
    let action = if unsafe_syntax {
        SourceAction::Unresolved("dynamic, escaped or unsupported reference syntax".into())
    } else if include
        || target
            .rsplit('.')
            .next()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("p") || ext.eq_ignore_ascii_case("w"))
    {
        SourceAction::RelativeFile(target.clone())
    } else {
        SourceAction::Unresolved(format!("local-or-PROPATH:{target}"))
    };
    SourceItem {
        range: trigger,
        label: label(format!(
            "{} {} (lexical)",
            if include { "include" } else { "RUN" },
            if target.is_empty() {
                "<dynamic>"
            } else {
                &target
            }
        )),
        action,
    }
}
fn parse(
    snapshot: &TaskSnapshot,
    cancel: &Cancellation,
    want_references: bool,
) -> Result<Model, TaskFailure> {
    let mut model = Model {
        symbols: Vec::new(),
        references: Vec::new(),
        limited: false,
    };
    let mut state = LexerState::default();
    let mut previous_end = false;
    let mut pending = None;
    let mut declaration_end = 0;
    let mut method_name: Option<Range<usize>> = None;
    for (chunk, bytes) in snapshot.bytes().chunks(4096).enumerate() {
        if cancel.is_cancelled() {
            return Err(TaskFailure::Cancelled);
        }
        let start = chunk * 4096;
        scan(
            SyntaxProfile::Abl,
            &mut state,
            bytes,
            start,
            start + bytes.len() == snapshot.bytes().len(),
            |token| {
                if model.limited {
                    return;
                }
                let source = &snapshot.bytes()[token.source.clone()];
                if token.kind == SyntaxKind::Preprocessor && source == b"{" && want_references {
                    if model.references.len() >= 64 {
                        model.limited = true;
                        return;
                    }
                    model
                        .references
                        .push(reference(snapshot, token.source, true));
                    return;
                }
                if token.kind == SyntaxKind::Operator {
                    if source == b"(" && pending == Some("method") {
                        if let Some(range) = method_name.take() {
                            if !snapshot.bytes()[range.end..token.source.start]
                                .iter()
                                .all(u8::is_ascii_whitespace)
                            {
                                pending = None;
                                return;
                            }
                            if let Some(name) = text(snapshot, range.clone()) {
                                if model.symbols.len() >= 64 {
                                    model.limited = true;
                                    return;
                                }
                                model.symbols.push(Symbol {
                                    kind: "method",
                                    name,
                                    range,
                                });
                            }
                        }
                        pending = None;
                    } else if matches!(source, b"." | b":" | b";" | b"(") {
                        previous_end = false;
                        pending = None;
                        method_name = None;
                    }
                    return;
                }
                if !matches!(token.kind, SyntaxKind::Plain | SyntaxKind::Keyword) {
                    return;
                }
                // Plain spans also include whitespace and punctuation. Only code
                // words can name declarations or introduce a RUN reference.
                if !source
                    .first()
                    .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_' || *b >= 128)
                {
                    return;
                }
                if source.eq_ignore_ascii_case(b"run") && want_references {
                    if model.references.len() >= 64 {
                        model.limited = true;
                        return;
                    }
                    model
                        .references
                        .push(reference(snapshot, token.source.clone(), false));
                }
                let declaration = if source.eq_ignore_ascii_case(b"procedure") {
                    Some("procedure")
                } else if source.eq_ignore_ascii_case(b"function") {
                    Some("function")
                } else if source.eq_ignore_ascii_case(b"class") {
                    Some("class")
                } else if source.eq_ignore_ascii_case(b"method") {
                    Some("method")
                } else {
                    None
                };
                if let Some(kind) = declaration.filter(|_| !previous_end) {
                    pending = Some(kind);
                    declaration_end = token.source.end;
                    method_name = None;
                } else if let Some(kind) = pending {
                    if token.kind == SyntaxKind::Plain {
                        if kind == "method" {
                            if let Some(range) = &mut method_name {
                                if range.end == token.source.start {
                                    range.end = token.source.end;
                                } else {
                                    *range = token.source.clone();
                                }
                            } else {
                                method_name = Some(token.source.clone());
                            }
                        } else {
                            // Do not mistake a return type/body word for a name
                            // when the shared lexer skipped an oversized token.
                            if !snapshot.bytes()[declaration_end..token.source.start]
                                .iter()
                                .all(u8::is_ascii_whitespace)
                            {
                                pending = None;
                                return;
                            }
                            let mut range = token.source.clone();
                            let bytes = snapshot.bytes();
                            while range.end < bytes.len()
                                && range.len() < 512
                                && (bytes[range.end].is_ascii_alphanumeric()
                                    || matches!(bytes[range.end], b'_' | b'-' | b'.')
                                    || bytes[range.end] >= 128)
                            {
                                range.end += 1;
                            }
                            if bytes.get(range.end.wrapping_sub(1)) == Some(&b'.') {
                                range.end -= 1;
                            }
                            let Some(name) = text(snapshot, range.clone()) else {
                                pending = None;
                                return;
                            };
                            if model.symbols.len() >= 64 {
                                model.limited = true;
                                return;
                            }
                            model.symbols.push(Symbol { kind, name, range });
                            pending = None;
                        }
                    }
                }
                previous_end = source.eq_ignore_ascii_case(b"end");
            },
        );
        if state.limited() || model.limited {
            return Err(TaskFailure::LimitExceeded);
        }
    }
    if want_references {
        for item in &mut model.references {
            if let SourceAction::Unresolved(reason) = &item.action {
                if let Some(name) = reason.strip_prefix("local-or-PROPATH:") {
                    if let Some(symbol) = model.symbols.iter().find(|symbol| {
                        symbol.kind == "procedure" && symbol.name.eq_ignore_ascii_case(name)
                    }) {
                        item.range = symbol.range.clone();
                        item.action = SourceAction::Navigate;
                    }
                }
            }
        }
    }
    Ok(model)
}
fn symbols(
    snapshot: &TaskSnapshot,
    cancel: &Cancellation,
    output: &mut TaskOutput,
) -> Result<(), TaskFailure> {
    for symbol in parse(snapshot, cancel, false)?.symbols {
        output.push(TaskItem::Source(SourceItem {
            label: label(format!("{} {} (lexical)", symbol.kind, symbol.name)),
            range: symbol.range,
            action: SourceAction::Navigate,
        }))?;
    }
    Ok(())
}
fn references(
    snapshot: &TaskSnapshot,
    cancel: &Cancellation,
    output: &mut TaskOutput,
) -> Result<(), TaskFailure> {
    for item in parse(snapshot, cancel, true)?.references {
        output.push(TaskItem::Source(item))?;
    }
    Ok(())
}
