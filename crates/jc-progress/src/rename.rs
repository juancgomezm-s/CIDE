//! Closed ABL subset for document-local refactoring. Unsupported
//! statements reject the whole proposal because they can affect name binding.
use jc_languages::syntax::{scan, LexerState, SyntaxKind, SyntaxProfile};
use jc_text::TextEncoding;
use std::{collections::HashMap, ops::Range};

const SOURCE_LIMIT: usize = 256 * 1024;
const TOKEN_LIMIT: usize = 16_384;
const EDIT_LIMIT: usize = 1024;
const UNSUPPORTED: &str = "El documento usa sintaxis ABL fuera del subconjunto de renombrado";
const AMBIGUOUS: &str = "Hay una declaración duplicada o una referencia ambigua";

#[derive(Clone)]
pub(crate) struct Token<'a> {
    pub(crate) text: &'a [u8],
    kind: SyntaxKind,
    pub(crate) range: Range<usize>,
}

impl Token<'_> {
    pub(crate) fn is(&self, word: &[u8]) -> bool {
        self.text.eq_ignore_ascii_case(word)
    }
    fn name(&self) -> bool {
        self.kind == SyntaxKind::Plain && identifier(self.text) && !reserved_context_word(self.text)
    }
}

fn reserved_context_word(bytes: &[u8]) -> bool {
    // The highlighter recognizes common keywords. Also reject additional
    // words that can acquire a grammatical role in the supported statements.
    [
        b"YES".as_slice(),
        b"NO",
        b"VAR",
        b"DEF",
        b"INITIAL",
        b"COLUMN-LABEL",
        b"FORMAT",
        b"LABEL",
        b"AT",
        b"TO",
        b"SKIP",
        b"SPACE",
        b"COLOR",
        b"HELP",
        b"ALERT-BOX",
        b"EDITING",
    ]
    .iter()
    .any(|word| word.eq_ignore_ascii_case(bytes))
}

fn identifier(bytes: &[u8]) -> bool {
    bytes.len() <= 32
        && bytes
            .first()
            .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
        && !bytes.ends_with(b"-")
}

pub(crate) fn tokenize(bytes: &[u8]) -> Result<Vec<Token<'_>>, String> {
    let mut lexer = LexerState::default();
    let mut tokens: Vec<Token<'_>> = Vec::new();
    let mut covered = 0;
    let mut valid = true;
    scan(SyntaxProfile::Abl, &mut lexer, bytes, 0, true, |span| {
        let source = span.source;
        valid &= source.start == covered;
        covered = source.end;
        let text = &bytes[source.clone()];
        if span.kind == SyntaxKind::Preprocessor {
            valid = false;
        }
        if !valid
            || span.kind == SyntaxKind::Comment
            || text.iter().all(u8::is_ascii_whitespace) && span.kind != SyntaxKind::String
        {
            return;
        }
        if span.kind == SyntaxKind::String {
            if let Some(last) = tokens
                .last_mut()
                .filter(|last| last.kind == SyntaxKind::String && last.range.end == source.start)
            {
                last.range.end = source.end;
                last.text = &bytes[last.range.clone()];
                return;
            }
        }
        if tokens.len() == TOKEN_LIMIT {
            valid = false;
            return;
        }
        tokens.push(Token {
            text,
            kind: span.kind,
            range: source,
        });
    });
    if !valid || lexer.limited() || covered != bytes.len() {
        return Err("El fuente contiene preprocesador o supera los límites del renombrado".into());
    }
    Ok(tokens)
}

#[derive(Clone, Copy)]
enum Role {
    Declaration(bool),
    Reference,
    RoutineDeclaration(Routine),
    Forward,
    RoutineCall(Routine),
}

struct Occurrence {
    token: usize,
    scope: usize,
    role: Role,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Routine {
    Procedure,
    Function,
}

impl Routine {
    fn from_token(token: &Token<'_>) -> Option<Self> {
        if token.is(b"PROCEDURE") {
            Some(Self::Procedure)
        } else if token.is(b"FUNCTION") {
            Some(Self::Function)
        } else {
            None
        }
    }
}

fn scalar(token: &Token<'_>) -> bool {
    [
        b"CHARACTER".as_slice(),
        b"INTEGER",
        b"INT64",
        b"DECIMAL",
        b"LOGICAL",
        b"DATE",
        b"DATETIME",
        b"DATETIME-TZ",
        b"HANDLE",
        b"RAW",
        b"LONGCHAR",
    ]
    .iter()
    .any(|word| token.is(word))
}

fn expression(tokens: &[Token<'_>]) -> bool {
    tokens.iter().all(|token| {
        if token.name() || matches!(token.kind, SyntaxKind::Number | SyntaxKind::String) {
            return true;
        }
        [
            b"(".as_slice(),
            b")",
            b"+",
            b"-",
            b"*",
            b"/",
            b"=",
            b"<",
            b">",
            b"?",
            b"AND",
            b"OR",
            b"NOT",
            b"TRUE",
            b"FALSE",
            b"YES",
            b"NO",
        ]
        .iter()
        .any(|word| token.is(word))
    })
}

fn references(
    occurrences: &mut Vec<Occurrence>,
    tokens: &[Token<'_>],
    start: usize,
    scope: usize,
) -> Result<(), String> {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate() {
        if token.is(b"(") {
            depth += 1;
        } else if token.is(b")") {
            depth = depth.checked_sub(1).ok_or(UNSUPPORTED)?;
        }
        let argument_separator = depth > 0 && token.is(b",");
        let argument_direction = depth > 0
            && index > 0
            && (tokens[index - 1].is(b"(") || tokens[index - 1].is(b","))
            && (token.is(b"INPUT") || token.is(b"OUTPUT") || token.is(b"INPUT-OUTPUT"));
        if !argument_separator && !argument_direction && !expression(std::slice::from_ref(token)) {
            return Err(UNSUPPORTED.into());
        }
        if !argument_direction && token.name() {
            let role = if tokens.get(index + 1).is_some_and(|next| next.is(b"(")) {
                Role::RoutineCall(Routine::Function)
            } else {
                Role::Reference
            };
            occurrences.push(Occurrence {
                token: start + index,
                scope,
                role,
            });
        }
    }
    if depth != 0 {
        return Err(UNSUPPORTED.into());
    }
    Ok(())
}

fn declaration(occurrences: &mut Vec<Occurrence>, token: usize, scope: usize, renameable: bool) {
    occurrences.push(Occurrence {
        token,
        scope,
        role: Role::Declaration(renameable),
    });
}

struct FunctionHeader {
    result: usize,
    parameters: Option<Vec<usize>>,
}

impl FunctionHeader {
    fn parse(tokens: &[Token<'_>], start: usize, end: usize) -> Result<Self, String> {
        let mut result = start + 2;
        if tokens.get(result).is_some_and(|word| word.is(b"RETURNS")) {
            result += 1;
        }
        if result >= end || !scalar(&tokens[result]) {
            return Err(UNSUPPORTED.into());
        }
        let parameters = match &tokens[result + 1..end] {
            [] => None,
            [open, parameters @ .., close] if open.is(b"(") && close.is(b")") => {
                let mut names = Vec::new();
                for (index, parameter) in parameters.split(|token| token.is(b",")).enumerate() {
                    if parameters.is_empty() {
                        break;
                    }
                    let [direction, name, as_word, datatype] = parameter else {
                        return Err(UNSUPPORTED.into());
                    };
                    if !(direction.is(b"INPUT")
                        || direction.is(b"OUTPUT")
                        || direction.is(b"INPUT-OUTPUT"))
                        || !name.name()
                        || !as_word.is(b"AS")
                        || !scalar(datatype)
                    {
                        return Err(UNSUPPORTED.into());
                    }
                    names.push(result + 3 + index * 5);
                }
                Some(names)
            }
            _ => return Err(UNSUPPORTED.into()),
        };
        Ok(Self { result, parameters })
    }

    fn bind(self, prototype: Option<Self>, tokens: &[Token<'_>]) -> Result<Vec<usize>, String> {
        let Some(prototype) = prototype else {
            return Ok(self.parameters.unwrap_or_default());
        };
        let expected = prototype.parameters.unwrap_or_default();
        let matches = |left: usize, right: usize| tokens[left].is(tokens[right].text);
        if !matches(self.result, prototype.result)
            || self.parameters.as_ref().is_some_and(|actual| {
                actual.len() != expected.len()
                    || actual
                        .iter()
                        .zip(&expected)
                        .any(|(&a, &b)| !matches(a - 1, b - 1) || !matches(a + 2, b + 2))
            })
        {
            return Err(
                "El prototipo FORWARD y la implementación tienen firmas incompatibles".into(),
            );
        }
        Ok(self.parameters.unwrap_or(expected))
    }
}

fn classify(tokens: &[Token<'_>]) -> Result<Vec<Occurrence>, String> {
    let mut occurrences = Vec::new();
    let mut prototypes = HashMap::new();
    let mut routine = None;
    let mut scope = 0;
    let mut next_scope = 0;
    let mut blocks = 0usize;
    let mut start = 0;
    for (end, delimiter) in tokens
        .iter()
        .enumerate()
        .filter(|(_, t)| t.is(b".") || t.is(b":"))
    {
        // A qualified name (or object member) is outside this grammar.
        if tokens
            .get(end + 1)
            .is_some_and(|next| delimiter.range.end == next.range.start && next.name())
        {
            return Err(UNSUPPORTED.into());
        }
        let statement = &tokens[start..end];
        let Some(first) = statement.first() else {
            return Err(UNSUPPORTED.into());
        };
        if let Some(kind) = Routine::from_token(first) {
            if routine.is_some() || blocks != 0 || !statement.get(1).is_some_and(Token::name) {
                return Err(UNSUPPORTED.into());
            }
            let forward = kind == Routine::Function
                && delimiter.is(b".")
                && statement.last().is_some_and(|token| token.is(b"FORWARD"));
            if forward {
                let signature = FunctionHeader::parse(tokens, start, end - 1)?;
                let name = statement[1].text.to_ascii_uppercase();
                if prototypes.insert(name, signature).is_some() {
                    return Err("Hay prototipos FORWARD duplicados".into());
                }
                occurrences.push(Occurrence {
                    token: start + 1,
                    scope: 0,
                    role: Role::Forward,
                });
                start = end + 1;
                continue;
            }
            if !delimiter.is(b":") {
                return Err(UNSUPPORTED.into());
            }
            next_scope += 1;
            scope = next_scope;
            routine = Some(kind);
            occurrences.push(Occurrence {
                token: start + 1,
                scope,
                role: Role::RoutineDeclaration(kind),
            });
            match (kind, statement) {
                (Routine::Procedure, [_, _]) => {}
                (Routine::Function, _) => {
                    let prototype = prototypes.remove(&statement[1].text.to_ascii_uppercase());
                    let signature = FunctionHeader::parse(tokens, start, end)?;
                    for name in signature.bind(prototype, tokens)? {
                        declaration(&mut occurrences, name, scope, false);
                    }
                }
                _ => return Err(UNSUPPORTED.into()),
            }
        } else if first.is(b"END") && delimiter.is(b".") {
            match statement {
                [_] if blocks > 0 => blocks -= 1,
                [_, kind]
                    if blocks == 0 && routine.is_some() && Routine::from_token(kind) == routine =>
                {
                    routine = None;
                    scope = 0;
                }
                _ => return Err(UNSUPPORTED.into()),
            }
        } else if delimiter.is(b":") {
            if first.is(b"DO") && statement.len() == 1 {
                blocks += 1;
            } else if first.is(b"IF")
                && statement.len() > 3
                && statement[statement.len() - 2].is(b"THEN")
                && statement[statement.len() - 1].is(b"DO")
            {
                references(
                    &mut occurrences,
                    &statement[1..statement.len() - 2],
                    start + 1,
                    scope,
                )?;
                blocks += 1;
            } else {
                return Err(UNSUPPORTED.into());
            }
        } else if first.is(b"DEFINE") {
            let (name_at, renameable) = match statement {
                [_, variable, ..] if variable.is(b"VARIABLE") => (2, true),
                [_, direction, parameter, ..]
                    if scope != 0
                        && parameter.is(b"PARAMETER")
                        && (direction.is(b"INPUT")
                            || direction.is(b"OUTPUT")
                            || direction.is(b"INPUT-OUTPUT")) =>
                {
                    (3, false)
                }
                _ => return Err(UNSUPPORTED.into()),
            };
            let [name, as_word, datatype, tail @ ..] = &statement[name_at..] else {
                return Err(UNSUPPORTED.into());
            };
            if !name.name() || !as_word.is(b"AS") || !scalar(datatype) {
                return Err(UNSUPPORTED.into());
            }
            let tail = if tail.last().is_some_and(|word| word.is(b"NO-UNDO")) {
                &tail[..tail.len() - 1]
            } else {
                tail
            };
            match tail {
                [] => {}
                [initial, value]
                    if initial.is(b"INITIAL")
                        && (matches!(value.kind, SyntaxKind::Number | SyntaxKind::String)
                            || [b"?".as_slice(), b"YES", b"NO", b"TRUE", b"FALSE"]
                                .iter()
                                .any(|word| value.is(word))) => {}
                _ => return Err(UNSUPPORTED.into()),
            }
            declaration(&mut occurrences, start + name_at, scope, renameable);
        } else if first.is(b"RUN") {
            let [_, name, arguments @ ..] = statement else {
                return Err(UNSUPPORTED.into());
            };
            let valid_arguments = arguments.is_empty()
                || arguments.first().is_some_and(|t| t.is(b"("))
                    && arguments.last().is_some_and(|t| t.is(b")"));
            if !name.name() || name.is(b"VALUE") || !valid_arguments {
                return Err(UNSUPPORTED.into());
            }
            occurrences.push(Occurrence {
                token: start + 1,
                scope,
                role: Role::RoutineCall(Routine::Procedure),
            });
            references(&mut occurrences, arguments, start + 2, scope)?;
        } else if first.is(b"ASSIGN")
            || first.is(b"DISPLAY")
            || first.is(b"MESSAGE")
            || first.is(b"RETURN")
        {
            references(&mut occurrences, &statement[1..], start + 1, scope)?;
        } else if first.name()
            && (statement.get(1).is_some_and(|word| word.is(b"="))
                || statement.get(1).is_some_and(|word| word.is(b"("))
                    && statement.last().is_some_and(|word| word.is(b")")))
        {
            references(&mut occurrences, statement, start, scope)?;
        } else {
            return Err(UNSUPPORTED.into());
        }
        start = end + 1;
    }
    if start != tokens.len() || routine.is_some() || blocks != 0 {
        return Err(UNSUPPORTED.into());
    }
    if !prototypes.is_empty() {
        return Err("Hay un prototipo FORWARD sin implementación posterior".into());
    }
    Ok(occurrences)
}

/// Copies a complete scalar signature into an existing FORWARD declaration.
pub(crate) fn synchronize_prototype(
    bytes: &[u8],
    encoding: TextEncoding,
    cursor: usize,
) -> Result<Vec<jc_app::TextReplacement>, String> {
    if bytes.len() > SOURCE_LIMIT {
        return Err("Actualizar prototipos admite documentos de hasta 256 KiB".into());
    }
    let prefix = bom_prefix(bytes, encoding);
    let body = &bytes[prefix..];
    let cursor = cursor
        .checked_sub(prefix)
        .ok_or("Selecciona el nombre de una función")?;
    let tokens = tokenize(body)?;
    let mut headers = Vec::new();
    let mut start = 0;
    for (end, delimiter) in tokens
        .iter()
        .enumerate()
        .filter(|(_, t)| t.is(b".") || t.is(b":"))
    {
        let words = &tokens[start..end];
        if let [keyword, name, ..] = words {
            if keyword.is(b"FUNCTION") && name.name() {
                let forward =
                    delimiter.is(b".") && words.last().is_some_and(|token| token.is(b"FORWARD"));
                if forward || delimiter.is(b":") {
                    headers.push((start, end - usize::from(forward), forward));
                }
            }
        }
        start = end + 1;
    }
    let selected = headers
        .iter()
        .map(|(start, _, _)| &tokens[*start + 1])
        .find(|token| (token.range.start..=token.range.end).contains(&cursor))
        .ok_or("Coloca el cursor en el nombre de la función o su prototipo FORWARD")?;
    let mut prototype = None;
    let mut implementation = None;
    for (start, end, forward) in headers {
        if !tokens[start + 1].is(selected.text) {
            continue;
        }
        let header = FunctionHeader::parse(&tokens, start, end)?;
        let range = start + 2..end;
        let slot = if forward {
            &mut prototype
        } else {
            &mut implementation
        };
        if slot.replace((range, header)).is_some() {
            return Err("La función tiene prototipos o implementaciones duplicados".into());
        }
    }
    let (previous, _) = prototype.ok_or("La función no tiene prototipo FORWARD")?;
    let (actual, signature) = implementation.ok_or("La función FORWARD no tiene implementación")?;
    if signature.parameters.is_none() {
        return Err(
            "La implementación debe declarar la lista de parámetros, incluso si es vacía".into(),
        );
    }
    let signature_bytes = |indices: Range<usize>| {
        let tokens = &tokens[indices];
        let range = tokens.first().unwrap().range.start..tokens.last().unwrap().range.end;
        let trivia_is_horizontal = tokens.windows(2).all(|pair| {
            body[pair[0].range.end..pair[1].range.start]
                .iter()
                .all(|byte| matches!(byte, b' ' | b'\t'))
        });
        trivia_is_horizontal
            .then_some(range)
            .ok_or("La firma debe estar en una línea y sin comentarios intercalados")
    };
    let range = signature_bytes(previous)?;
    let replacement = &body[signature_bytes(actual)?];
    if &body[range.clone()] == replacement {
        return Ok(Vec::new());
    }
    let mut candidate = body.to_vec();
    candidate.splice(range.clone(), replacement.iter().copied());
    let candidate_tokens = tokenize(&candidate)?;
    classify(&candidate_tokens)?;
    let text = std::str::from_utf8(replacement)
        .map_err(|_| "La firma ABL contiene bytes fuera del subconjunto admitido")?;
    Ok(vec![jc_app::TextReplacement {
        range: range.start + prefix..range.end + prefix,
        text: text.into(),
    }])
}

pub(crate) fn bom_prefix(bytes: &[u8], encoding: TextEncoding) -> usize {
    usize::from(encoding == TextEncoding::Utf8 && bytes.starts_with(b"\xef\xbb\xbf")) * 3
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProjectDeclaration {
    Procedure,
    Function,
    Global,
}

pub(crate) fn project_declaration(
    bytes: &[u8],
    encoding: TextEncoding,
    cursor: usize,
) -> Result<Option<(ProjectDeclaration, Range<usize>)>, String> {
    if bytes.len() > SOURCE_LIMIT {
        return Err("El renombrado admite documentos de hasta 256 KiB".into());
    }
    let prefix = bom_prefix(bytes, encoding);
    let tokens = tokenize(&bytes[prefix..])?;
    let occurrences = classify(&tokens)?;
    Ok(occurrences.iter().find_map(|occurrence| {
        let range = &tokens[occurrence.token].range;
        let range = range.start + prefix..range.end + prefix;
        if !range.contains(&cursor) {
            return None;
        }
        let kind = match occurrence.role {
            Role::RoutineDeclaration(Routine::Procedure) => ProjectDeclaration::Procedure,
            Role::RoutineDeclaration(Routine::Function) | Role::Forward => {
                ProjectDeclaration::Function
            }
            Role::Declaration(true) if occurrence.scope == 0 => ProjectDeclaration::Global,
            _ => return None,
        };
        Some((kind, range))
    }))
}

/// Returns the declaration and uses in the closed lexical subset, in source order.
/// All supported editor encodings preserve ASCII identifier bytes unchanged.
pub fn propose(
    bytes: &[u8],
    encoding: TextEncoding,
    cursor: usize,
    new_name: &str,
) -> Result<Vec<Range<usize>>, String> {
    if bytes.len() > SOURCE_LIMIT {
        return Err("El renombrado admite documentos de hasta 256 KiB".into());
    }
    let name = new_name.as_bytes();
    if !identifier(name) || !tokenize(name)?.first().is_some_and(Token::name) {
        return Err(
            "Escribe un identificador ABL ASCII válido y que no sea una palabra reservada".into(),
        );
    }
    // Keep the BOM outside the token stream and restore original byte coordinates.
    let prefix = bom_prefix(bytes, encoding);
    let tokens = tokenize(&bytes[prefix..])?;
    let occurrences = classify(&tokens)?;
    let mut bindings = HashMap::new();
    let mut routines = HashMap::new();
    for occurrence in &occurrences {
        if let Role::RoutineDeclaration(kind) = occurrence.role {
            let key = tokens[occurrence.token].text.to_ascii_uppercase();
            if routines.insert(key, (occurrence.token, kind)).is_some() {
                return Err(AMBIGUOUS.into());
            }
        }
        if let Role::Declaration(renameable) = occurrence.role {
            let key = (
                occurrence.scope,
                tokens[occurrence.token].text.to_ascii_uppercase(),
            );
            if bindings
                .insert(key, (occurrence.token, renameable))
                .is_some()
            {
                return Err(AMBIGUOUS.into());
            }
        }
    }
    let mut uses = Vec::with_capacity(occurrences.len());
    let mut selected = None;
    for occurrence in occurrences {
        let token = &tokens[occurrence.token];
        let key = token.text.to_ascii_uppercase();
        let (binding, renameable, is_routine) = match occurrence.role {
            Role::RoutineDeclaration(_) => (occurrence.token, true, true),
            Role::Forward | Role::RoutineCall(_) => {
                let kind = match occurrence.role {
                    Role::RoutineCall(kind) => kind,
                    _ => Routine::Function,
                };
                let &(binding, _) = routines
                    .get(&key)
                    .filter(|(_, declared)| *declared == kind)
                    .ok_or(
                        "Hay una llamada que no se puede vincular a una declaración del archivo",
                    )?;
                (binding, true, true)
            }
            Role::Declaration(_) | Role::Reference => {
                let local = (occurrence.scope, key);
                let (binding, renameable) = bindings
                    .get(&local)
                    .or_else(|| bindings.get(&(0, local.1)))
                    .copied()
                    .ok_or(AMBIGUOUS)?;
                (binding, renameable, false)
            }
        };
        let range = token.range.start + prefix..token.range.end + prefix;
        if range.contains(&cursor) && renameable {
            selected = Some((binding, occurrence.scope, is_routine));
        }
        uses.push((binding, range));
    }
    let (selected, selected_use_scope, is_routine) = selected.ok_or(
        "Coloca el cursor en una variable, procedimiento o función admitidos, o en uno de sus usos",
    )?;
    let original = &tokens[selected];
    if original.text == name {
        return Err("El nombre nuevo coincide con el actual".into());
    }
    let uppercase = name.to_ascii_uppercase();
    let scope = if bindings.contains_key(&(selected_use_scope, original.text.to_ascii_uppercase()))
    {
        selected_use_scope
    } else {
        0
    };
    if routines
        .get(&uppercase)
        .is_some_and(|(binding, _)| *binding != selected)
        || bindings
            .iter()
            .any(|((other_scope, other_name), (binding, _))| {
                *binding != selected
                    && *other_name == uppercase
                    && (is_routine || scope == 0 || *other_scope == 0 || *other_scope == scope)
            })
    {
        return Err("El nombre nuevo entra en conflicto con otra declaración".into());
    }
    let ranges: Vec<_> = uses
        .into_iter()
        .filter_map(|(binding, range)| (binding == selected).then_some(range))
        .collect();
    if ranges.len() > EDIT_LIMIT {
        return Err("El renombrado supera el límite de 1024 cambios".into());
    }
    Ok(ranges)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_prototype_implementation_and_calls_share_one_binding() {
        let source = "FUNCTION amount RETURNS INTEGER (INPUT n AS INTEGER) FORWARD.\r\namount(1).\r\nFUNCTION Amount RETURNS INTEGER:\r\nRETURN n.\r\nEND FUNCTION.\r\nDISPLAY AMOUNT(2).";
        let positions = ["amount RETURNS", "amount(1)", "Amount RETURNS", "AMOUNT(2)"]
            .map(|needle| source.find(needle).unwrap());
        for position in positions {
            assert_eq!(
                propose(source.as_bytes(), TextEncoding::Utf8, position, "quantity").unwrap(),
                positions.map(|start| start..start + 6),
            );
        }
    }

    #[test]
    fn rejects_incompatible_and_missing_forward_signatures() {
        let prototype = "FUNCTION amount RETURNS INTEGER (INPUT n AS INTEGER) FORWARD.\n";
        for header in [
            "DECIMAL (INPUT n AS INTEGER)",
            "INTEGER (OUTPUT n AS INTEGER)",
            "INTEGER (INPUT n AS DECIMAL)",
            "INTEGER ()",
        ] {
            let source =
                format!("{prototype}FUNCTION amount RETURNS {header}:\nRETURN 1.\nEND FUNCTION.");
            assert!(
                propose(source.as_bytes(), TextEncoding::Utf8, 10, "quantity")
                    .unwrap_err()
                    .contains("firmas incompatibles")
            );
        }
        for source in [prototype.to_owned(), prototype.repeat(2)] {
            assert!(propose(source.as_bytes(), TextEncoding::Utf8, 10, "quantity").is_err());
        }
        let source = format!("{prototype}FUNCTION amount RETURNS INTEGER (INPUT other AS INTEGER):\nRETURN other.\nEND FUNCTION.");
        assert_eq!(
            propose(source.as_bytes(), TextEncoding::Utf8, 10, "quantity")
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn renames_routine_declarations_and_static_calls_without_touching_trivia() {
        let source = b"\xef\xbb\xbfDEFINE VARIABLE total AS INTEGER NO-UNDO.\r\n\
            FUNCTION amount RETURNS INTEGER (INPUT n AS INTEGER):\r\n\
              RETURN n.\r\nEND FUNCTION.\r\n\
            PROCEDURE calculate:\r\n\
              total = amount(amount(2)).\r\nEND PROCEDURE.\r\n\
            /* RUN calculate. */\r\nMESSAGE 'amount(2)'.\r\nRUN CALCULATE.";
        let find = |word: &[u8]| {
            source
                .windows(word.len())
                .position(|part| part == word)
                .unwrap()
        };
        let procedure = propose(source, TextEncoding::Utf8, find(b"CALCULATE"), "compute").unwrap();
        let function = propose(
            source,
            TextEncoding::Utf8,
            find(b"amount(amount"),
            "value-total",
        )
        .unwrap();
        let selected = |ranges: Vec<Range<usize>>| {
            ranges
                .into_iter()
                .map(|range| &source[range])
                .collect::<Vec<_>>()
        };
        assert_eq!(
            selected(procedure),
            vec![b"calculate".as_slice(), b"CALCULATE"]
        );
        assert_eq!(selected(function), vec![b"amount".as_slice(); 3]);
        assert!(propose(
            source,
            TextEncoding::Utf8,
            find(b"amount(amount"),
            "calculate"
        )
        .is_err());
        assert!(propose(source, TextEncoding::Utf8, find(b"CALCULATE"), "total").is_err());
        let mut external = source.to_vec();
        external.extend_from_slice(b"\r\nRUN VALUE(total).");
        assert!(propose(&external, TextEncoding::Utf8, find(b"CALCULATE"), "compute").is_err());
    }

    #[test]
    fn renames_global_and_local_without_crossing_shadowed_scopes_or_trivia() {
        let source = b"DEFINE VARIABLE amount AS INTEGER NO-UNDO.\r\nASSIGN amount = 2.\r\nPROCEDURE show-local:\r\n DEFINE VARIABLE amount AS INTEGER.\r\n MESSAGE amount 'amount'. /* amount */\r\nEND PROCEDURE.\r\nFUNCTION show-global RETURNS INTEGER (INPUT increment AS INTEGER):\r\n RETURN amount + increment.\r\nEND FUNCTION.";
        let global = source
            .windows(6)
            .position(|part| part == b"amount")
            .unwrap();
        let local = source
            .windows(14)
            .position(|part| part == b"MESSAGE amount")
            .unwrap()
            + 8;
        let global_ranges = propose(source, TextEncoding::Utf8, global, "grand-total").unwrap();
        let local_ranges = propose(source, TextEncoding::Utf8, local, "local-total").unwrap();
        assert_eq!(global_ranges.len(), 3);
        assert_eq!(local_ranges.len(), 2);
        assert!(global_ranges
            .iter()
            .all(|range| !local_ranges.contains(range)));
        assert!(global_ranges
            .iter()
            .chain(&local_ranges)
            .all(|range| &source[range.clone()] == b"amount"));
    }

    #[test]
    fn rejects_unresolved_context_and_collisions() {
        let source = b"DEFINE VARIABLE amount AS INTEGER. DEFINE VARIABLE balance AS INTEGER. MESSAGE amount.";
        assert!(propose(source, TextEncoding::Utf8, 17, "balance").is_err());
        assert!(propose(source, TextEncoding::Utf8, 17, "return").is_err());
        for suffix in [
            b"{hidden.i}".as_slice(),
            b" MESSAGE customer.amount.",
            b" FIND FIRST customer.",
        ] {
            let mut bytes = source.to_vec();
            bytes.extend_from_slice(suffix);
            assert!(propose(&bytes, TextEncoding::Utf8, 17, "updated-total").is_err());
        }
    }
}
