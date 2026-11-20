use super::{Kind, Outline};
use std::ops::Range;

#[derive(Clone)]
struct Token<'a> {
    text: &'a str,
    range: Range<usize>,
    literal: bool,
}

impl Token<'_> {
    fn is(&self, text: &str) -> bool {
        !self.literal && self.text.eq_ignore_ascii_case(text)
    }
}

fn tokenize(source: &str, cursor: usize) -> Option<Vec<Token<'_>>> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let start = index;
        let byte = bytes[index];
        if byte.is_ascii_whitespace() {
            index += 1;
            continue;
        }
        let mut literal = false;
        let mut comment = false;
        if bytes[index..].starts_with(b"--") || byte == b'#' {
            index = source[index..]
                .find('\n')
                .map_or(bytes.len(), |end| index + end);
            comment = true;
        } else if bytes[index..].starts_with(b"/*") {
            index += 2;
            let mut depth = 1;
            while depth > 0 {
                match bytes.get(index..index + 2)? {
                    b"/*" => {
                        depth += 1;
                        index += 2;
                    }
                    b"*/" => {
                        depth -= 1;
                        index += 2;
                    }
                    _ => index += 1,
                }
            }
            comment = true;
        } else if matches!(byte, b'\'' | b'"' | b'`') {
            index += 1;
            loop {
                let current = *bytes.get(index)?;
                index += 1;
                if current == byte {
                    if bytes.get(index) == Some(&byte) {
                        index += 1;
                    } else {
                        break;
                    }
                } else if current == b'\\' {
                    index += 1;
                }
            }
            literal = true;
        } else if byte == b'$' {
            index += 1;
            while bytes
                .get(index)
                .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
            {
                index += 1;
            }
            if bytes.get(index) == Some(&b'$') {
                index += 1;
                let delimiter = &source[start..index];
                index += source[index..].find(delimiter)? + delimiter.len();
                literal = true;
            }
        } else {
            let word = |ch: char| ch.is_alphanumeric() || matches!(ch, '_' | '$');
            let first = source[index..].chars().next()?;
            index += first.len_utf8();
            if word(first) {
                for ch in source[index..].chars().take_while(|ch| word(*ch)) {
                    index += ch.len_utf8();
                }
            }
        }
        if (literal || comment) && start < cursor && cursor < index {
            return None;
        }
        if !comment {
            tokens.push(Token {
                text: &source[start..index],
                range: start..index,
                literal,
            });
        }
    }
    Some(tokens)
}

fn statement<'a, 's>(tokens: &'a [Token<'s>], cursor: usize) -> Option<&'a [Token<'s>]> {
    let start = tokens
        .iter()
        .rposition(|token| token.is(";") && token.range.start < cursor)
        .map_or(0, |position| position + 1);
    let end = tokens
        .iter()
        .position(|token| token.is(";") && token.range.start >= cursor)?;
    let first = tokens.get(start)?;
    if first.range.end > cursor
        || !["SELECT", "WITH", "INSERT", "UPDATE", "DELETE"]
            .iter()
            .any(|kind| first.is(kind))
    {
        return None;
    }
    tokens.get(start..=end)
}

struct Case {
    start: usize,
    when: Option<usize>,
    branch: Option<Range<usize>>,
}

pub(super) fn outline(source: &str, cursor: usize) -> Option<Outline> {
    let tokens = tokenize(source, cursor)?;
    let tokens = statement(&tokens, cursor)?;
    let mut stack = Vec::<Case>::new();
    let mut enclosing = None;
    let mut inside_case = false;
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate() {
        if token.is("(") || token.is("[") {
            depth += 1;
        }
        if token.is(")") || token.is("]") {
            depth = depth.checked_sub(1)?;
        }
        if token.is("CASE") {
            stack.push(Case {
                start: token.range.start,
                when: None,
                branch: None,
            });
        } else if let Some(case) = stack.last_mut() {
            if token.is("WHEN") || token.is("ELSE") || token.is("END") {
                if let Some(header) = case.branch.take() {
                    if header.end <= cursor && cursor <= token.range.start && enclosing.is_none() {
                        enclosing = Some(Outline {
                            kind: Kind::SqlCase,
                            header,
                            close: Some(token.range.start),
                        });
                    }
                }
                case.when = None;
                if token.is("WHEN") {
                    case.when = Some(index);
                } else if token.is("ELSE") {
                    case.branch = Some(token.range.clone());
                } else {
                    inside_case |= case.start <= cursor && cursor < token.range.end;
                    stack.pop();
                }
            } else if token.is("THEN") {
                let when = case.when.take()?;
                case.branch = Some(tokens[when].range.start..token.range.end);
            }
        }
    }
    if depth != 0 || !stack.is_empty() {
        return None;
    }
    if inside_case {
        enclosing
    } else {
        Some(Outline {
            kind: Kind::SqlStatement,
            header: tokens.first()?.range.clone(),
            close: Some(tokens.last()?.range.start),
        })
    }
}

pub(super) fn balanced(text: &str, case_expression: bool) -> bool {
    let Some(tokens) = tokenize(text, 0) else {
        return false;
    };
    let mut delimiters = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if token.is(";") {
            return false;
        }
        if case_expression && delimiters.is_empty() {
            if token.is(",") {
                return false;
            }
            if token.is(":")
                && index
                    .checked_sub(1)
                    .is_some_and(|previous| tokens[previous].literal)
                && !tokens
                    .get(index + 1)
                    .is_some_and(|next| next.is(":") && next.range.start == token.range.end)
            {
                return false;
            }
        }
        if token.is("(") {
            delimiters.push(")");
        }
        if token.is("[") {
            delimiters.push("]");
        }
        if token.is("CASE") {
            delimiters.push("END");
        }
        if [")", "]", "END"].iter().any(|end| token.is(end))
            && !delimiters.pop().is_some_and(|end| token.is(end))
        {
            return false;
        }
    }
    delimiters.is_empty()
}

pub(super) fn references(source: &str, cursor: usize) -> Vec<(Option<String>, String)> {
    let Some(tokens) = tokenize(source, cursor) else {
        return Vec::new();
    };
    let Some(tokens) = statement(&tokens, cursor) else {
        return Vec::new();
    };
    let identifier = |token: &Token<'_>| {
        let raw = token.text;
        if (raw.starts_with('"') && raw.ends_with('"'))
            || (raw.starts_with('`') && raw.ends_with('`'))
        {
            let quote = &raw[..1];
            Some(raw[1..raw.len() - 1].replace(&quote.repeat(2), quote))
        } else if !token.literal
            && raw.starts_with(|ch: char| ch.is_alphabetic() || ch == '_')
            && raw
                .chars()
                .all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '$')
        {
            Some(raw.to_owned())
        } else {
            None
        }
    };
    let mut relations = Vec::new();
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate() {
        if token.is("(") {
            depth += 1;
        } else if token.is(")") {
            depth = depth.saturating_sub(1);
        }
        if depth != 0 {
            continue;
        }
        if !["FROM", "JOIN", "UPDATE", "INTO"]
            .iter()
            .any(|word| token.is(word))
        {
            continue;
        }
        let Some(first) = tokens.get(index + 1).and_then(&identifier) else {
            continue;
        };
        let relation = if tokens.get(index + 2).is_some_and(|token| token.is(".")) {
            let Some(second) = tokens.get(index + 3).and_then(&identifier) else {
                continue;
            };
            (Some(first), second)
        } else {
            (None, first)
        };
        if !relations.contains(&relation) {
            relations.push(relation);
        }
        if relations.len() == 4 {
            break;
        }
    }
    relations
}
