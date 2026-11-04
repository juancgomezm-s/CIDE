//! Closed ABL subset for document-local variable refactoring. Unsupported
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
struct Token<'a> {
    text: &'a [u8],
    kind: SyntaxKind,
    range: Range<usize>,
}

impl Token<'_> {
    fn is(&self, word: &[u8]) -> bool {
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

fn tokenize(bytes: &[u8]) -> Result<Vec<Token<'_>>, String> {
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
    if !expression(tokens)
        || tokens
            .windows(2)
            .any(|pair| pair[0].name() && pair[1].is(b"("))
    {
        return Err(UNSUPPORTED.into());
    }
    for (index, token) in tokens.iter().enumerate() {
        if token.name() {
            occurrences.push(Occurrence {
                token: start + index,
                scope,
                role: Role::Reference,
            });
        }
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

fn classify(tokens: &[Token<'_>]) -> Result<(Vec<Occurrence>, Vec<usize>), String> {
    let mut occurrences = Vec::new();
    let mut routine_names = Vec::new();
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
            if routine.is_some()
                || blocks != 0
                || !delimiter.is(b":")
                || !statement.get(1).is_some_and(Token::name)
            {
                return Err(UNSUPPORTED.into());
            }
            next_scope += 1;
            scope = next_scope;
            routine = Some(kind);
            routine_names.push(start + 1);
            match (kind, statement) {
                (Routine::Procedure, [_, _]) => {}
                (Routine::Function, [_, _, returns, datatype, open, params @ .., close])
                    if returns.is(b"RETURNS")
                        && scalar(datatype)
                        && open.is(b"(")
                        && close.is(b")") =>
                {
                    let mut at = 0;
                    while at < params.len() {
                        let [direction, name, as_word, datatype, ..] = &params[at..] else {
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
                        declaration(&mut occurrences, start + 6 + at, scope, false);
                        at += 4;
                        if at < params.len() {
                            if !params[at].is(b",") || at + 1 == params.len() {
                                return Err(UNSUPPORTED.into());
                            }
                            at += 1;
                        }
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
        } else if first.is(b"ASSIGN")
            || first.is(b"DISPLAY")
            || first.is(b"MESSAGE")
            || first.is(b"RETURN")
        {
            references(&mut occurrences, &statement[1..], start + 1, scope)?;
        } else if first.name() && statement.get(1).is_some_and(|word| word.is(b"=")) {
            references(&mut occurrences, statement, start, scope)?;
        } else {
            return Err(UNSUPPORTED.into());
        }
        start = end + 1;
    }
    if start != tokens.len() || routine.is_some() || blocks != 0 {
        return Err(UNSUPPORTED.into());
    }
    Ok((occurrences, routine_names))
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
    let prefix =
        usize::from(encoding == TextEncoding::Utf8 && bytes.starts_with(&[0xef, 0xbb, 0xbf])) * 3;
    let tokens = tokenize(&bytes[prefix..])?;
    let (occurrences, routine_names) = classify(&tokens)?;
    let mut bindings = HashMap::new();
    for occurrence in &occurrences {
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
        let local = (occurrence.scope, key);
        let (binding, renameable) = bindings
            .get(&local)
            .or_else(|| bindings.get(&(0, local.1)))
            .copied()
            .ok_or(AMBIGUOUS)?;
        let range = token.range.start + prefix..token.range.end + prefix;
        if range.contains(&cursor) && renameable {
            selected = Some((binding, occurrence.scope));
        }
        uses.push((binding, range));
    }
    let (selected, selected_use_scope) =
        selected.ok_or("Coloca el cursor en una variable DEFINE VARIABLE o en uno de sus usos")?;
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
    if routine_names.iter().any(|&index| tokens[index].is(name))
        || bindings
            .iter()
            .any(|((other_scope, other_name), (binding, _))| {
                *binding != selected
                    && *other_name == uppercase
                    && (scope == 0 || *other_scope == 0 || *other_scope == scope)
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
