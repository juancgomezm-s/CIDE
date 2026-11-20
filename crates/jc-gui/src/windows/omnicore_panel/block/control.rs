//! Recognize delimited control bodies without treating adjacent branches as context.
mod expression;

use super::{abl_close, abl_period, Kind, Outline, SyntaxProfile, Token, HEADER_TOKENS};

pub(super) enum Target {
    Outside,
    Unbounded,
    Segment(Outline),
}

pub(super) fn at_cursor(tokens: &[Token<'_>], profile: SyntaxProfile, cursor: usize) -> Target {
    let next = tokens.partition_point(|token| token.range.start < cursor);
    if tokens.get(next).is_some_and(|token| token.is("else"))
        && next.checked_sub(1).is_some_and(|i| {
            if profile == SyntaxProfile::Rust {
                tokens[i].text == "}"
            } else {
                abl_period(tokens, i)
            }
        })
    {
        return Target::Unbounded;
    }
    if profile == SyntaxProfile::Rust {
        rust(tokens, cursor)
    } else {
        abl(tokens, cursor)
    }
}

fn target(
    tokens: &[Token<'_>],
    start: usize,
    open: usize,
    kind: Kind,
    close: Option<usize>,
) -> Target {
    if close.is_none() || matches!(kind, Kind::RustMatch | Kind::AblCase) {
        Target::Unbounded
    } else {
        Target::Segment(Outline {
            kind,
            header: tokens[start].range.start..tokens[open].range.end,
            close,
        })
    }
}

fn rust(tokens: &[Token<'_>], cursor: usize) -> Target {
    let pairs = expression::rust_pairs(tokens);
    let mut controls = vec![None; tokens.len()];
    for (i, token) in tokens
        .iter()
        .enumerate()
        .take_while(|(_, token)| token.range.start < cursor)
    {
        if i > 0 && tokens[i - 1].text == "#" && tokens[i - 1].range.end == token.range.start {
            continue;
        }
        let kind = match token.text {
            "if" | "else" => Kind::RustBranch,
            "for" | "while" | "loop" => Kind::RustLoop,
            "match" => Kind::RustMatch,
            _ => continue,
        };
        if let Some(open) = rust_open(tokens, &pairs, i) {
            controls[open] = Some((i, kind));
        }
    }
    let mut parent = None;
    let mut selected = Target::Outside;
    for (open, token) in tokens
        .iter()
        .enumerate()
        .take_while(|(_, t)| t.range.end <= cursor)
    {
        if token.text != "{" {
            continue;
        }
        let close = pairs[open].map(|index| tokens[index].range.start);
        if close.is_some_and(|offset| offset < cursor) {
            continue;
        }
        // A direct braced arm belongs to the nearest active match body.
        if let Some(match_open) =
            parent.filter(|&i| matches!(controls[i], Some((_, Kind::RustMatch))))
        {
            if open >= match_open + 3
                && tokens[open - 2].text == "="
                && tokens[open - 1].text == ">"
            {
                let start = rust_arm_start(tokens, &pairs, match_open, open - 2);
                controls[open] = Some((start, Kind::RustArm));
            }
        }
        parent = Some(open);
        if let Some((start, kind)) = controls[open] {
            selected = if kind == Kind::RustMatch {
                pairs[open]
                    .and_then(|end| expression::rust_arm(tokens, &pairs, open, end, cursor))
                    .map_or(Target::Unbounded, Target::Segment)
            } else {
                target(tokens, start, open, kind, close)
            };
        }
    }
    selected
}

fn rust_open(tokens: &[Token<'_>], pairs: &[Option<usize>], start: usize) -> Option<usize> {
    let keyword = tokens[start].text;
    if matches!(keyword, "else" | "loop") {
        return tokens
            .get(start + 1)
            .filter(|token| token.text == "{")
            .map(|_| start + 1);
    }
    let mut depth = 0usize;
    let mut in_loop = keyword != "for";
    let mut i = start + 1;
    let limit = (i + HEADER_TOKENS).min(tokens.len());
    while i < limit {
        match tokens[i].text {
            "(" | "[" => depth += 1,
            ")" | "]" => depth = depth.checked_sub(1)?,
            "in" if depth == 0 => in_loop = true,
            ";" | "}" if depth == 0 => return None,
            "=" if depth == 0 && tokens.get(i + 1).is_some_and(|token| token.text == ">") => {
                return None
            }
            "{" if depth == 0 && i == start + 1 => i = pairs[i]?,
            "{" if depth == 0 => return in_loop.then_some(i),
            "{" => i = pairs[i]?,
            _ => {}
        }
        i += 1;
    }
    None
}

fn rust_arm_start(
    tokens: &[Token<'_>],
    pairs: &[Option<usize>],
    open: usize,
    arrow: usize,
) -> usize {
    let mut index = arrow;
    let mut depth = 0usize;
    while index > open + 1 {
        match tokens[index - 1].text {
            "," if depth == 0 => break,
            ")" | "]" => depth += 1,
            "(" | "[" => depth = depth.saturating_sub(1),
            "}" if depth == 0 => {
                // The previous arm may omit its comma if its body is braced.
                let Some(brace) = pairs[index - 1] else { break };
                if brace >= open + 3
                    && tokens[brace - 2].text == "="
                    && tokens[brace - 1].text == ">"
                {
                    break;
                }
                index = brace;
                continue;
            }
            _ => {}
        }
        index -= 1;
    }
    index
}

fn abl(tokens: &[Token<'_>], cursor: usize) -> Target {
    let mut candidates = 0;
    // Closers of completed inner bodies are excluded before considering outer scopes.
    let end = tokens.partition_point(|token| token.range.start < cursor);
    // Check a single-statement arm before considering enclosing DO or CASE scopes.
    for (i, token) in tokens[..end].iter().enumerate().rev().take(HEADER_TOKENS) {
        if abl_period(tokens, i) || token.is(":") && !abl_member_colon(tokens, i) {
            break;
        }
        if token.is("then") || token.is("else") || token.is("otherwise") {
            return expression::abl_statement(tokens, i, cursor)
                .map_or(Target::Unbounded, Target::Segment);
        }
    }
    for (i, token) in tokens[..end].iter().enumerate().rev() {
        let kind = if token.is("do") {
            let previous = i.checked_sub(1).and_then(|index| tokens.get(index));
            if previous
                .is_some_and(|token| token.is("then") || token.is("else") || token.is("otherwise"))
            {
                Kind::AblBranch
            } else if tokens.get(i + 1).is_some_and(|token| token.is(":")) {
                Kind::AblDo
            } else {
                Kind::AblLoop
            }
        } else if token.is("repeat")
            || token.is("for")
                && tokens
                    .get(i + 1)
                    .is_some_and(|token| token.is("each") || token.is("first") || token.is("last"))
        {
            Kind::AblLoop
        } else if token.is("case")
            && !i
                .checked_sub(1)
                .and_then(|j| tokens.get(j))
                .is_some_and(|token| token.is("end"))
        {
            Kind::AblCase
        } else {
            continue;
        };
        let Some(open) = abl_open(tokens, i) else {
            continue;
        };
        if tokens[open].range.end > cursor {
            continue;
        }
        candidates += 1;
        if candidates > 128 {
            return Target::Unbounded;
        }
        match abl_close(tokens, open, kind) {
            Ok(Some(close)) if close < cursor => continue,
            Ok(close) => {
                let start = if kind == Kind::AblBranch {
                    abl_branch_start(tokens, i)
                } else {
                    i
                };
                return target(tokens, start, open, kind, close);
            }
            Err(()) => return Target::Unbounded,
        }
    }
    Target::Outside
}

fn abl_branch_start(tokens: &[Token<'_>], do_token: usize) -> usize {
    let keyword = do_token - 1;
    if tokens[keyword].is("else") || tokens[keyword].is("otherwise") {
        return keyword;
    }
    for i in (0..keyword).rev().take(HEADER_TOKENS) {
        if tokens[i].is("if") || tokens[i].is("when") || tokens[i].is("otherwise") {
            return i;
        }
        if abl_period(tokens, i) {
            break;
        }
    }
    keyword
}

fn abl_open(tokens: &[Token<'_>], start: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (i, token) in tokens
        .iter()
        .enumerate()
        .skip(start + 1)
        .take(HEADER_TOKENS)
    {
        match token.text {
            "(" | "[" => depth += 1,
            ")" | "]" => depth = depth.checked_sub(1)?,
            ":" if depth == 0 && (i == start + 1 || !abl_member_colon(tokens, i)) => {
                return Some(i)
            }
            "." if depth == 0 && abl_period(tokens, i) => return None,
            _ => {}
        }
        if depth == 0 && (token.is("end") || token.is("then") || token.is("else")) {
            return None;
        }
    }
    None
}

pub(super) fn abl_member_colon(tokens: &[Token<'_>], at: usize) -> bool {
    let Some(left) = at.checked_sub(1).and_then(|i| tokens.get(i)) else {
        return false;
    };
    let Some(right) = tokens.get(at + 1) else {
        return false;
    };
    let colon = &tokens[at];
    !left.is("do")
        && !left.is("repeat")
        && left.range.end == colon.range.start
        && colon.range.end == right.range.start
        && left
            .text
            .chars()
            .next()
            .is_some_and(|ch| ch.is_alphanumeric() || ch == '_')
        && right
            .text
            .chars()
            .next()
            .is_some_and(|ch| ch.is_alphabetic() || ch == '_')
}
