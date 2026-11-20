//! Bound unbraced match expressions and single ABL statements to their separators.
use super::{abl_period, Kind, Outline, Token, HEADER_TOKENS};

pub(super) fn rust_pairs(tokens: &[Token<'_>]) -> Vec<Option<usize>> {
    let mut pairs = vec![None; tokens.len()];
    let mut stack = Vec::new();
    for (i, token) in tokens.iter().enumerate() {
        match token.text {
            "(" => stack.push((i, ")")),
            "[" => stack.push((i, "]")),
            "{" => stack.push((i, "}")),
            ")" | "]" | "}" => {
                if let Some((open, _)) = stack
                    .last()
                    .copied()
                    .filter(|&(_, close)| close == token.text)
                {
                    stack.pop();
                    pairs[open] = Some(i);
                    pairs[i] = Some(open);
                }
            }
            _ => {}
        }
    }
    pairs
}

fn arrow(tokens: &[Token<'_>], i: usize) -> bool {
    tokens.get(i..i + 2).is_some_and(|pair| {
        pair[0].text == "=" && pair[1].text == ">" && pair[0].range.end == pair[1].range.start
    })
}

fn grouped(tokens: &[Token<'_>], pairs: &[Option<usize>], i: usize, end: usize) -> Option<usize> {
    if matches!(tokens[i].text, "(" | "[" | "{") {
        return pairs[i].filter(|&close| close < end).map(|close| close + 1);
    }
    // The commas of a turbofish belong to the expression or pattern.
    if i >= 2 && tokens[i].text == "<" && tokens[i - 1].text == ":" && tokens[i - 2].text == ":" {
        let mut depth = 1usize;
        let mut j = i + 1;
        while j < end {
            match tokens[j].text {
                "<" => depth += 1,
                ">" if tokens[j - 1].text != "-" => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(j + 1);
                    }
                }
                "(" | "[" | "{" => j = pairs[j].filter(|&close| close < end)?,
                _ => {}
            }
            j += 1;
        }
        return None;
    }
    Some(i + 1)
}

fn pattern_arrow(
    tokens: &[Token<'_>],
    pairs: &[Option<usize>],
    start: usize,
    end: usize,
) -> Option<usize> {
    let mut i = start;
    while i < end && !arrow(tokens, i) {
        if tokens[i].text == "," {
            return None;
        }
        i = grouped(tokens, pairs, i, end)?;
    }
    (i + 1 < end).then_some(i)
}

fn next_arm_after_block(
    tokens: &[Token<'_>],
    pairs: &[Option<usize>],
    brace: usize,
    end: usize,
) -> Option<usize> {
    let after = pairs[brace]? + 1;
    let next = tokens.get(after)?;
    // A postfix operator or an else clause still belongs to the current
    // expression. Otherwise require a following pattern and arrow.
    if after >= end || matches!(next.text, "," | "." | "?" | "as" | "else") {
        return None;
    }
    pattern_arrow(tokens, pairs, after, end).map(|_| after)
}

pub(super) fn rust_arm(
    tokens: &[Token<'_>],
    pairs: &[Option<usize>],
    open: usize,
    end: usize,
    cursor: usize,
) -> Option<Outline> {
    let mut start = open + 1;
    'arms: while start < end && tokens[start].range.start < cursor {
        let separator = pattern_arrow(tokens, pairs, start, end)?;
        let header_end = tokens[separator + 1].range.end;
        if cursor < header_end {
            return None;
        }
        let first = separator + 2;
        let mut close = first;
        // Closure argument commas are not arm separators.
        let params = (first..end)
            .take_while(|&i| matches!(tokens[i].text, "async" | "move"))
            .last()
            .map_or(first, |i| i + 1);
        if params < end && tokens[params].text == "|" {
            let mut i = params + 1;
            while i < end && tokens[i].text != "|" {
                i = grouped(tokens, pairs, i, end)?;
            }
            if i >= end {
                return None;
            }
            close = i + 1;
        }
        while close < end && tokens[close].text != "," {
            if arrow(tokens, close) || tokens[close].text == ";" {
                return None;
            }
            if tokens[close].text == "{" {
                if let Some(after) = next_arm_after_block(tokens, pairs, close, end) {
                    if cursor <= tokens[after - 1].range.start {
                        return None;
                    }
                    start = after;
                    continue 'arms;
                }
            }
            close = grouped(tokens, pairs, close, end)?;
        }
        if cursor <= tokens[close].range.start {
            return Some(Outline {
                kind: Kind::RustExpressionArm,
                header: tokens[start].range.start..header_end,
                close: Some(tokens[close].range.start),
            });
        }
        start = close + 1;
    }
    None
}

fn abl_boundary(tokens: &[Token<'_>], at: usize) -> bool {
    abl_period(tokens, at) || tokens[at].is(":") && !super::abl_member_colon(tokens, at)
}

fn abl_then_start(tokens: &[Token<'_>], keyword: usize) -> Option<usize> {
    let mut depth = 0usize;
    for i in (0..keyword).rev().take(HEADER_TOKENS) {
        let token = &tokens[i];
        match token.text {
            ")" | "]" => depth += 1,
            "(" | "[" => depth = depth.checked_sub(1)?,
            _ => {}
        }
        if depth > 0 {
            continue;
        }
        if token.is("if") || token.is("when") {
            let statement_start = i == 0
                || abl_boundary(tokens, i - 1)
                || tokens[i - 1].is("then")
                || tokens[i - 1].is("else");
            return statement_start.then_some(i);
        }
        if abl_boundary(tokens, i) {
            break;
        }
    }
    None
}

pub(super) fn abl_statement(
    tokens: &[Token<'_>],
    keyword: usize,
    cursor: usize,
) -> Option<Outline> {
    let start = if tokens[keyword].is("then") {
        abl_then_start(tokens, keyword)?
    } else {
        if tokens[keyword].is("else")
            && !keyword
                .checked_sub(1)
                .is_some_and(|i| abl_period(tokens, i))
        {
            return None;
        }
        keyword
    };
    let mut depth = 0usize;
    for (i, token) in tokens
        .iter()
        .enumerate()
        .skip(keyword + 1)
        .take(HEADER_TOKENS)
    {
        match token.text {
            "(" | "[" => depth += 1,
            ")" | "]" => depth = depth.checked_sub(1)?,
            _ => {}
        }
        if depth != 0 {
            continue;
        }
        if abl_period(tokens, i) {
            return (cursor <= token.range.start).then(|| Outline {
                kind: Kind::AblStatementBranch,
                header: tokens[start].range.start..tokens[keyword].range.end,
                close: Some(token.range.start),
            });
        }
        if token.is("then")
            || token.is("else")
            || token.is("end")
            || token.is(":") && !super::abl_member_colon(tokens, i)
        {
            return None;
        }
    }
    None
}
