//! Optional progress.abl source provider. Lexical inference only: no AVM, XREF,
//! include expansion, filesystem access or compiler certification.
use jc_app::{
    source_services::{
        CallerRoutine, FileResolution, SourceAction, SourceItem, SourceProviderDescriptor,
        ValueType,
    },
    Cancellation, TaskFailure, TaskItem, TaskOutput, TaskSnapshot,
};
use jc_languages::syntax::{scan, LexerState, SyntaxKind, SyntaxProfile};
use jc_text::decode_fragment;
use std::{
    collections::{HashMap, HashSet},
    ops::Range,
    path::{Path, PathBuf},
};
mod call_origins;
use call_origins::CallOrigins;

pub fn descriptor() -> SourceProviderDescriptor {
    SourceProviderDescriptor {
        id: "progress.abl",
        language: "abl",
        symbols: Some(symbols),
        references: Some(references),
        calls: Some(calls),
        callers: Some(callers),
        routines: Some(call_origins::relations),
        dependencies: Some(dependencies),
        completion: None,
        rename: Some(crate::rename::propose),
        file_resolution: FileResolution::OrderedProjectPath,
        snippets: super::snippets::TEMPLATES,
    }
}
struct Symbol {
    kind: &'static str,
    name: String,
    range: Range<usize>,
    value_type: Option<ValueType>,
}
struct Model {
    symbols: Vec<Symbol>,
    references: Vec<SourceItem>,
    grouped_references: HashSet<ReferenceKey>,
    local_procedures: HashMap<String, Option<Range<usize>>>,
    limited: bool,
}
#[derive(Hash, PartialEq, Eq)]
enum ReferenceKey {
    Local(String),
    File(PathBuf, Option<usize>),
    Dynamic(String),
}
const MAX_SYMBOLS: usize = 64;
const MAX_REFERENCES: usize = 64;
const LOCAL_CALL: &str = "local-or-PROPATH:";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Analysis<'a> {
    Symbols,
    References,
    Calls,
    Callers,
    Dependencies,
    LocalProcedures(&'a HashSet<String>),
}

impl Analysis<'_> {
    fn wants_calls(self) -> bool {
        matches!(self, Self::References | Self::Calls | Self::Callers)
    }

    fn wants_includes(self) -> bool {
        matches!(self, Self::References | Self::Dependencies | Self::Callers)
    }
}

fn push_symbol(model: &mut Model, symbol: Symbol) {
    if model.symbols.len() == MAX_SYMBOLS {
        model.limited = true;
    } else {
        model.symbols.push(symbol);
    }
}

fn push_reference(
    model: &mut Model,
    mut item: SourceItem,
    analysis: Analysis<'_>,
    routine: Option<&CallerRoutine>,
) {
    if matches!(analysis, Analysis::Calls | Analysis::Callers) {
        let key = match &item.action {
            SourceAction::RelativeFile(relative) => Some(ReferenceKey::File(
                Path::new(relative)
                    .components()
                    .filter(|part| *part != std::path::Component::CurDir)
                    .collect(),
                routine.map(|routine| routine.declaration),
            )),
            _ if analysis == Analysis::Callers => return,
            _ => local_call(&item)
                .map(|name| ReferenceKey::Local(name.to_ascii_lowercase()))
                .or_else(|| {
                    item.label.strip_prefix("RUN VALUE(").and_then(|name| {
                        name.strip_suffix(')')
                            .map(|name| ReferenceKey::Dynamic(name.to_ascii_lowercase()))
                    })
                }),
        };
        if key.is_some_and(|key| !model.grouped_references.insert(key)) {
            return;
        }
        if analysis == Analysis::Callers {
            let SourceAction::RelativeFile(relative) = item.action else {
                unreachable!()
            };
            item.action = SourceAction::IncomingReference {
                relative,
                routine: routine.cloned(),
            };
        }
    }
    let limit = if analysis == Analysis::Callers {
        2048
    } else {
        MAX_REFERENCES
    };
    if model.references.len() == limit {
        model.limited = true;
    } else {
        model.references.push(item);
    }
}

pub(crate) fn definition_kind(keyword: &[u8]) -> Option<&'static str> {
    let options: &[(&[u8], &str)] = &[
        (b"VARIABLE", "Variable"),
        (b"VAR", "Variable"),
        (b"PARAMETER", "Parámetro"),
        (b"BUFFER", "Búfer"),
        (b"TEMP-TABLE", "Tabla temporal"),
        (b"WORK-TABLE", "Tabla de trabajo"),
        (b"DATASET", "Conjunto de datos"),
        (b"QUERY", "Consulta"),
        (b"FRAME", "Marco"),
        (b"BUTTON", "Botón"),
        (b"BROWSE", "Explorador de datos"),
        (b"RECTANGLE", "Rectángulo"),
        (b"IMAGE", "Imagen"),
        (b"MENU", "Menú"),
        (b"SUB-MENU", "Submenú"),
    ];
    options
        .iter()
        .find_map(|(name, kind)| keyword.eq_ignore_ascii_case(name).then_some(*kind))
}

pub(crate) fn declared_value_type(word: &[u8]) -> Option<ValueType> {
    use ValueType::*;
    let types: &[(&[u8], ValueType)] = &[
        (b"CHARACTER", Text),
        (b"CHAR", Text),
        (b"LONGCHAR", Text),
        (b"CLOB", Text),
        (b"INTEGER", Integer),
        (b"INT", Integer),
        (b"INT64", Integer),
        (b"RECID", Integer),
        (b"DECIMAL", Decimal),
        (b"DEC", Decimal),
        (b"LOGICAL", Boolean),
        (b"DATE", Date),
        (b"DATETIME", DateTime),
        (b"DATETIME-TZ", DateTime),
        (b"RAW", Binary),
        (b"MEMPTR", Binary),
        (b"BLOB", Binary),
        (b"CLASS", Object),
        (b"HANDLE", Object),
        (b"COM-HANDLE", Object),
        (b"ROWID", Object),
    ];
    types
        .iter()
        .find_map(|(name, value_type)| word.eq_ignore_ascii_case(name).then_some(*value_type))
}

fn definition(snapshot: &TaskSnapshot, after_define: usize) -> Option<Symbol> {
    let bytes = snapshot.bytes();
    let limit = after_define.saturating_add(1024).min(bytes.len());
    let mut at = after_define;
    let mut word = || {
        while at < limit && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        let start = at;
        while at < limit
            && (bytes[at].is_ascii_alphanumeric()
                || matches!(bytes[at], b'_' | b'-')
                || bytes[at] >= 128)
        {
            at += 1;
        }
        (start < at && (at < limit || limit == bytes.len())).then_some(start..at)
    };
    let kind = loop {
        let range = word()?;
        let token = &bytes[range];
        if let Some(kind) = definition_kind(token) {
            break kind;
        }
        let modifiers: &[&[u8]] = &[
            b"INPUT",
            b"OUTPUT",
            b"INPUT-OUTPUT",
            b"RETURN",
            b"NEW",
            b"GLOBAL",
            b"SHARED",
            b"PUBLIC",
            b"PRIVATE",
            b"PROTECTED",
            b"PACKAGE-PRIVATE",
            b"PACKAGE-PROTECTED",
            b"STATIC",
            b"SERIALIZABLE",
            b"NON-SERIALIZABLE",
        ];
        if !modifiers
            .iter()
            .any(|word| token.eq_ignore_ascii_case(word))
        {
            return None;
        }
    };
    let range = word()?;
    let name = text(snapshot, range.clone())?;
    if !name.starts_with(|ch: char| ch.is_alphabetic() || ch == '_') {
        return None;
    }
    let value_type = if kind == "Variable"
        && word().is_some_and(|token| bytes[token].eq_ignore_ascii_case(b"AS"))
    {
        word().and_then(|token| declared_value_type(&bytes[token]))
    } else {
        None
    };
    Some(Symbol {
        kind,
        name,
        range,
        value_type,
    })
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
    let dynamic_value = !include && target.eq_ignore_ascii_case("value");
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
        || include && raw.iter().all(u8::is_ascii_digit)
        || dynamic_value;
    let action = if unsafe_syntax {
        SourceAction::Unresolved("Referencia dinámica, con escapes o sin soporte léxico".into())
    } else if include
        || target
            .rsplit_once('.')
            .is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("p") || ext.eq_ignore_ascii_case("w"))
    {
        SourceAction::RelativeFile(target.clone())
    } else {
        SourceAction::Unresolved(format!("{LOCAL_CALL}{target}"))
    };
    let label_target = if dynamic_value {
        let call = &bytes[at..bytes.len().min(at.saturating_add(128))];
        let expression = call
            .strip_prefix(b"(")
            .and_then(|body| {
                body.iter()
                    .position(|&byte| byte == b')')
                    .map(|end| &body[..end])
            })
            .and_then(|raw| std::str::from_utf8(raw).ok())
            .map(str::trim)
            .filter(|name| {
                !name.is_empty()
                    && name
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            });
        expression.map_or_else(|| target.clone(), |name| format!("VALUE({name})"))
    } else {
        target
    };
    SourceItem {
        value_type: None,
        range: trigger,
        label: label(format!(
            "{} {}",
            if include { "Inclusión" } else { "RUN" },
            if label_target.is_empty() {
                "<dinámica>"
            } else {
                &label_target
            }
        )),
        action,
    }
}
fn parse(
    snapshot: &TaskSnapshot,
    cancel: &Cancellation,
    analysis: Analysis<'_>,
) -> Result<Model, TaskFailure> {
    let mut model = Model {
        symbols: Vec::new(),
        references: Vec::new(),
        grouped_references: HashSet::new(),
        local_procedures: HashMap::new(),
        limited: false,
    };
    let mut state = LexerState::default();
    let mut previous_end = false;
    let mut pending = None;
    let mut declaration_end = 0;
    let mut method_name: Option<Range<usize>> = None;
    let mut origins = CallOrigins::default();
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
                if analysis == Analysis::Callers {
                    origins.observe(snapshot, &token);
                }
                if token.kind == SyntaxKind::Preprocessor && source == b"{" {
                    if analysis.wants_includes() {
                        push_reference(
                            &mut model,
                            reference(snapshot, token.source, true),
                            analysis,
                            origins.current(),
                        );
                    } else if analysis == Analysis::Symbols {
                        let item = reference(snapshot, token.source, true);
                        if let SourceAction::RelativeFile(name) = item.action {
                            push_symbol(
                                &mut model,
                                Symbol {
                                    kind: "Inclusión",
                                    name,
                                    range: item.range,
                                    value_type: None,
                                },
                            );
                        }
                    }
                    return;
                }
                if matches!(
                    analysis,
                    Analysis::References
                        | Analysis::Calls
                        | Analysis::Callers
                        | Analysis::Dependencies
                ) {
                    if analysis.wants_calls()
                        && token.kind.is_word()
                        && source.eq_ignore_ascii_case(b"run")
                    {
                        push_reference(
                            &mut model,
                            reference(snapshot, token.source, false),
                            analysis,
                            origins.current(),
                        );
                    }
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
                                push_symbol(
                                    &mut model,
                                    Symbol {
                                        kind: "Método",
                                        name,
                                        range,
                                        value_type: None,
                                    },
                                );
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
                if !token.kind.is_word() {
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
                if analysis == Analysis::Symbols && source.eq_ignore_ascii_case(b"define") {
                    if let Some(symbol) = definition(snapshot, token.source.end) {
                        push_symbol(&mut model, symbol);
                    }
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
                if let Some(kind) = declaration.filter(|kind| {
                    !previous_end && (analysis == Analysis::Symbols || *kind == "procedure")
                }) {
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
                            if let Analysis::LocalProcedures(targets) = analysis {
                                let name = name.to_ascii_lowercase();
                                if targets.contains(&name) {
                                    model
                                        .local_procedures
                                        .entry(name)
                                        .and_modify(|location| *location = None)
                                        .or_insert(Some(range));
                                }
                            } else {
                                push_symbol(
                                    &mut model,
                                    Symbol {
                                        kind,
                                        name,
                                        range,
                                        value_type: None,
                                    },
                                );
                            }
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
    Ok(model)
}

fn local_call(item: &SourceItem) -> Option<&str> {
    match &item.action {
        SourceAction::Unresolved(reason) => reason.strip_prefix(LOCAL_CALL),
        _ => None,
    }
}

fn function_is_forward(snapshot: &TaskSnapshot, start: usize) -> bool {
    let bytes = snapshot.bytes();
    let end = (start + 4096).min(bytes.len());
    let mut lexer = LexerState::default();
    let mut forward = false;
    let mut result = None;
    scan(
        SyntaxProfile::Abl,
        &mut lexer,
        &bytes[start..end],
        start,
        end == bytes.len(),
        |span| {
            let text = &bytes[span.source.clone()];
            if result.is_some()
                || span.kind == SyntaxKind::Comment
                || text.iter().all(u8::is_ascii_whitespace)
            {
                return;
            }
            if text == b"."
                && bytes
                    .get(span.source.end)
                    .is_none_or(u8::is_ascii_whitespace)
            {
                result = Some(forward);
            } else if text == b":" {
                result = Some(false);
            } else {
                forward = span.kind.is_word() && text.eq_ignore_ascii_case(b"FORWARD");
            }
        },
    );
    result == Some(true)
}

fn symbols(
    snapshot: &TaskSnapshot,
    cancel: &Cancellation,
    output: &mut TaskOutput,
) -> Result<(), TaskFailure> {
    for symbol in parse(snapshot, cancel, Analysis::Symbols)?.symbols {
        output.push(TaskItem::Source(SourceItem {
            value_type: symbol.value_type,
            label: label(format!(
                "{} {}",
                match symbol.kind {
                    "procedure" => "Procedimiento",
                    "function" if function_is_forward(snapshot, symbol.range.end) => "Prototipo",
                    "function" => "Función",
                    "class" => "Clase",
                    kind => kind,
                },
                symbol.name
            )),
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
    emit_references(snapshot, cancel, output, Analysis::References)
}

fn calls(
    snapshot: &TaskSnapshot,
    cancel: &Cancellation,
    output: &mut TaskOutput,
) -> Result<(), TaskFailure> {
    emit_references(snapshot, cancel, output, Analysis::Calls)
}

fn follow_run(
    snapshot: &TaskSnapshot,
    cancel: &Cancellation,
    cursor: usize,
    output: &mut TaskOutput,
) -> Result<(), TaskFailure> {
    let bytes = snapshot.bytes();
    let mut state = LexerState::default();
    let mut operand = None;
    for (index, chunk) in bytes.chunks(4096).enumerate() {
        if cancel.is_cancelled() {
            return Err(TaskFailure::Cancelled);
        }
        let start = index * 4096;
        scan(
            SyntaxProfile::Abl,
            &mut state,
            chunk,
            start,
            start + chunk.len() == bytes.len(),
            |token| {
                if token.source.start <= cursor
                    && token.kind.is_word()
                    && bytes[token.source.clone()].eq_ignore_ascii_case(b"RUN")
                {
                    if let Some(target) =
                        jc_languages::syntax::abl_run_operand(bytes, token.source.end)
                            .filter(|operand| cursor < operand.end)
                    {
                        operand = Some(target);
                    }
                }
            },
        );
        if operand.is_some() || start > cursor {
            break;
        }
    }
    let Some((range, name)) =
        operand.and_then(|range| text(snapshot, range.clone()).map(|name| (range, name)))
    else {
        return output.push(TaskItem::Message(
            "Coloca el cursor sobre el destino de un RUN estático".into(),
        ));
    };
    let (range, action) = if name.contains('.') {
        (range, SourceAction::RelativeFile(name.clone()))
    } else {
        let key = name.to_ascii_lowercase();
        let locations = parse(
            snapshot,
            cancel,
            Analysis::LocalProcedures(&HashSet::from([key.clone()])),
        )?
        .local_procedures;
        let Some(Some(target)) = locations.get(&key) else {
            return output.push(TaskItem::Message(
                "El procedimiento local no se encontró o es ambiguo".into(),
            ));
        };
        (target.clone(), SourceAction::Navigate)
    };
    output.push(TaskItem::Source(SourceItem {
        value_type: None,
        label: label(name),
        range,
        action,
    }))
}

fn callers(
    snapshot: &TaskSnapshot,
    cancel: &Cancellation,
    output: &mut TaskOutput,
) -> Result<(), TaskFailure> {
    for item in parse(snapshot, cancel, Analysis::Callers)?.references {
        output.push(TaskItem::Source(item))?;
    }
    Ok(())
}

fn dependencies(
    snapshot: &TaskSnapshot,
    cancel: &Cancellation,
    output: &mut TaskOutput,
) -> Result<(), TaskFailure> {
    emit_references(snapshot, cancel, output, Analysis::Dependencies)
}

fn emit_references(
    snapshot: &TaskSnapshot,
    cancel: &Cancellation,
    output: &mut TaskOutput,
    analysis: Analysis<'_>,
) -> Result<(), TaskFailure> {
    let mut references = parse(snapshot, cancel, analysis)?.references;
    let targets: HashSet<_> = references
        .iter()
        .filter_map(local_call)
        .map(str::to_ascii_lowercase)
        .collect();
    if !targets.is_empty() {
        let locations =
            parse(snapshot, cancel, Analysis::LocalProcedures(&targets))?.local_procedures;
        for item in &mut references {
            let Some(name) = local_call(item) else {
                continue;
            };
            item.action = match locations.get(&name.to_ascii_lowercase()) {
                Some(Some(range)) => {
                    item.range = range.clone();
                    SourceAction::Navigate
                }
                Some(None) => SourceAction::Unresolved("Procedimiento local ambiguo".into()),
                None => SourceAction::Unresolved("Procedimiento local sin resolver".into()),
            };
        }
    }
    for item in references {
        output.push(TaskItem::Source(item))?;
    }
    Ok(())
}
