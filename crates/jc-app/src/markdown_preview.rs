//! Range-based, inert Markdown reading view. A bounded presentation subset,
//! not a DOM, HTML renderer, external process or second editable document.
use crate::{SyntaxKind, TabId, TaskStamp, Workbench};
use jc_text::{grapheme, DecodedScalars, TextEncoding};
use std::{
    path::{Component, Path, PathBuf},
    sync::Arc,
};
const LINE_BYTES: usize = 16 * 1024;
const WORK_BYTES: usize = 256 * 1024;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MarkdownInlineStyle {
    #[default]
    Plain,
    Strong,
    Emphasis,
    Strike,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownTarget {
    pub destination: String,
    pub image: bool,
}
#[derive(Debug, Clone)]
pub struct MarkdownSpan {
    pub text: String,
    pub kind: SyntaxKind,
    pub style: MarkdownInlineStyle,
    pub target: Option<Arc<MarkdownTarget>>,
}
/// Semantic block information, independent of terminal decorations and pixel fonts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkdownBlock {
    Paragraph,
    Blank,
    Heading(u8),
    Quote,
    Rule,
    Code { fence: bool },
    List { ordered: bool, task: Option<bool> },
    Table { columns: u8, separator: bool },
    Notice,
}
#[derive(Debug, Clone)]
pub struct MarkdownRow {
    pub source_line: usize,
    pub block: MarkdownBlock,
    /// True only after the first visual row of this source line.
    pub continuation: bool,
    /// Source indentation, TAB4, bounded to64 visual columns. Spans already include it.
    pub indent_columns: usize,
    /// Hanging indentation used by wrapped continuations; also present in spans.
    pub continuation_indent: usize,
    pub spans: Vec<MarkdownSpan>,
}
#[derive(Debug, Default)]
pub struct MarkdownPresentation {
    pub rows: Vec<MarkdownRow>,
    pub message: String,
    pub pending: bool,
    pub limited: bool,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Position {
    line: usize,
    wrapped: usize,
}
#[derive(Debug, Default)]
pub struct MarkdownView {
    position: Position,
    positions: Vec<Position>,
    next: Position,
    back: Vec<Position>,
    columns: usize,
}
impl MarkdownView {
    pub fn at_line(line: usize) -> Self {
        Self {
            position: Position { line, wrapped: 0 },
            ..Default::default()
        }
    }
    pub fn first(&mut self) {
        self.position = Position::default();
        self.back.clear();
        self.positions.clear();
        self.next = self.position;
    }
    pub fn last(&mut self, app: &Workbench, tab: TabId) {
        self.position = Position {
            line: app.line_count_for_tab(tab).unwrap_or(1).saturating_sub(1),
            wrapped: usize::MAX,
        };
        self.back.clear();
        self.positions.clear();
        self.next = self.position;
    }
    fn remember(&mut self, position: Position) {
        if self.back.len() >= 1024 {
            self.back.drain(..512);
        }
        self.back.push(position);
    }
    fn wrapped_count(
        &self,
        app: &Workbench,
        tab: TabId,
        line: usize,
        work: &mut usize,
    ) -> Option<usize> {
        let doc = app.document_by_tab(tab)?;
        let encoding = doc.session.encoding()?;
        let (content, _) = app.line_range_for_tab(tab, line)?;
        *work = work.checked_add(content.len().min(LINE_BYTES))?;
        if *work > 2 * 1024 * 1024 {
            return None;
        }
        let syntax = app.syntax_rows(tab, line, 1);
        if syntax.pending || syntax.limited {
            return None;
        }
        Some(
            wrap_line(
                line_spans(doc, content, encoding, &syntax, self.columns.clamp(12, 256))?,
                self.columns.clamp(12, 256),
            )
            .len()
            .max(1),
        )
    }
    /// Navigation reads only bounded lines when the last prepared positions or
    /// bounded history cannot answer. At most128 steps/2MiB source work per call.
    pub fn scroll(&mut self, delta: isize, app: &Workbench, tab: TabId) {
        let mut work = 0;
        let steps = delta.unsigned_abs().min(128);
        if delta < 0 {
            for _ in 0..steps {
                if let Some(previous) = self.back.pop() {
                    self.position = previous;
                } else if self.position.wrapped > 0 {
                    if self.position.wrapped == usize::MAX {
                        let Some(count) =
                            self.wrapped_count(app, tab, self.position.line, &mut work)
                        else {
                            break;
                        };
                        self.position.wrapped = count.saturating_sub(1);
                    }
                    self.position.wrapped = self.position.wrapped.saturating_sub(1);
                } else if self.position.line > 0 {
                    let line = self.position.line - 1;
                    let Some(count) = self.wrapped_count(app, tab, line, &mut work) else {
                        break;
                    };
                    self.position = Position {
                        line,
                        wrapped: count - 1,
                    };
                } else {
                    break;
                }
            }
        } else {
            let mut remaining = steps;
            if let Some(start) = self.positions.iter().position(|p| *p == self.position) {
                let available = self.positions.len() - start;
                let mut count = remaining.min(available);
                if start + count == self.positions.len()
                    && self.positions.last() == Some(&self.next)
                {
                    count = count.saturating_sub(1);
                }
                for index in start..start + count {
                    self.remember(self.positions[index]);
                }
                self.position = self
                    .positions
                    .get(start + count)
                    .copied()
                    .unwrap_or(self.next);
                remaining -= count;
            }
            let lines = app.line_count_for_tab(tab).unwrap_or(0);
            let mut cached = None;
            for _ in 0..remaining {
                let count = if let Some((line, count)) =
                    cached.filter(|(line, _)| *line == self.position.line)
                {
                    let _ = line;
                    count
                } else {
                    let Some(count) = self.wrapped_count(app, tab, self.position.line, &mut work)
                    else {
                        break;
                    };
                    cached = Some((self.position.line, count));
                    count
                };
                self.position.wrapped = self.position.wrapped.min(count - 1);
                let next = if self.position.wrapped + 1 < count {
                    Position {
                        wrapped: self.position.wrapped + 1,
                        ..self.position
                    }
                } else if self.position.line + 1 < lines {
                    Position {
                        line: self.position.line + 1,
                        wrapped: 0,
                    }
                } else {
                    break;
                };
                self.remember(self.position);
                self.position = next;
            }
        }
    }
    pub fn prepare(
        &mut self,
        app: &Workbench,
        tab: TabId,
        rows: usize,
        columns: usize,
    ) -> MarkdownPresentation {
        let mut p = MarkdownPresentation::default();
        let rows = rows.min(128);
        let columns = columns.clamp(12, 256);
        if self.columns != columns {
            self.back.clear();
            self.columns = columns;
        }
        self.positions.clear();
        self.next = self.position;
        let Some(doc) = app.document_by_tab(tab) else {
            p.message = "El documento se cerró".into();
            return p;
        };
        let Some(encoding) = doc.session.encoding() else {
            p.message = "Elige la codificación para la vista previa".into();
            return p;
        };
        let count = app.line_count_for_tab(tab).unwrap_or(0);
        self.position.line = self.position.line.min(count.saturating_sub(1));
        let syntax = app.syntax_rows(tab, self.position.line, rows.max(1));
        p.pending = syntax.pending;
        p.limited = syntax.limited;
        if syntax.pending {
            p.message = "Preparando Markdown por rangos…".into();
            return p;
        }
        if syntax.limited {
            p.message = "Markdown limitado por presupuesto; fuente conservada".into();
            return p;
        }
        let mut work = 0;
        for line in self.position.line..count {
            if p.rows.len() >= rows {
                break;
            }
            let Some((content, _)) = app.line_range_for_tab(tab, line) else {
                break;
            };
            work += content.len().min(LINE_BYTES);
            if work > WORK_BYTES {
                p.limited = true;
                p.message = "Límite de lectura de la vista previa; continúa con AvPág".into();
                break;
            }
            let Some(rendered) = line_spans(doc, content, encoding, &syntax, columns) else {
                break;
            };
            let indent_columns = rendered.indent_columns;
            let continuation_indent = rendered.continuation_indent.min(columns.saturating_sub(4));
            let block = rendered.block;
            let wrapped = wrap_line(rendered, columns);
            let skip = if line == self.position.line {
                self.position.wrapped.min(wrapped.len().saturating_sub(1))
            } else {
                0
            };
            if line == self.position.line {
                self.position.wrapped = skip;
            }
            for (part, spans) in wrapped.into_iter().enumerate().skip(skip) {
                if p.rows.len() >= rows {
                    self.next = Position {
                        line,
                        wrapped: part,
                    };
                    return p;
                }
                self.positions.push(Position {
                    line,
                    wrapped: part,
                });
                p.rows.push(MarkdownRow {
                    source_line: line,
                    block,
                    continuation: part > 0,
                    indent_columns,
                    continuation_indent,
                    spans,
                });
                self.next = Position {
                    line: line + 1,
                    wrapped: 0,
                };
            }
        }
        if self.next.line >= count {
            self.next = self.positions.last().copied().unwrap_or(self.position);
        }
        if p.message.is_empty() {
            p.message =
                "Vista de lectura · activa un enlace para abrirlo · fuente sin cambios".into();
        }
        p
    }
}
struct RenderedLine {
    spans: Vec<MarkdownSpan>,
    block: MarkdownBlock,
    indent_columns: usize,
    continuation_indent: usize,
}
fn line_spans(
    doc: &crate::EditorDocument,
    content: std::ops::Range<usize>,
    encoding: TextEncoding,
    syntax: &crate::SyntaxRows,
    columns: usize,
) -> Option<RenderedLine> {
    if content.len() > LINE_BYTES {
        return Some(RenderedLine {
            spans: vec![MarkdownSpan {
                text: "[Línea mayor de 16 KiB: consulta la fuente]".into(),
                kind: SyntaxKind::Comment,
                style: MarkdownInlineStyle::Plain,
                target: None,
            }],
            block: MarkdownBlock::Notice,
            indent_columns: 0,
            continuation_indent: 0,
        });
    }
    let bytes = doc.session.document().read_range(content.clone()).ok()?;
    let mut text = String::with_capacity(bytes.len());
    for scalar in DecodedScalars::new(&bytes, encoding, content.start) {
        if !scalar.valid {
            return Some(RenderedLine {
                spans: vec![MarkdownSpan {
                    text: "[Codificación inválida: consulta los bytes de fuente]".into(),
                    kind: SyntaxKind::Comment,
                    style: MarkdownInlineStyle::Plain,
                    target: None,
                }],
                block: MarkdownBlock::Notice,
                indent_columns: 0,
                continuation_indent: 0,
            });
        }
        text.push(scalar.character);
    }
    // The lexer deliberately leaves at most three leading indentation spaces
    // uncolored. Classify the first meaningful byte, not that default Plain gap.
    let indent = bytes.iter().take(3).take_while(|&&b| b == b' ').count();
    let code = syntax.kind_at(content.start + indent) == SyntaxKind::Code;
    let block = block_kind(&text, code, columns);
    let indent_columns = indentation(&text).min(64);
    let continuation_indent = match block {
        MarkdownBlock::List { ordered: false, .. } => indent_columns + 2,
        MarkdownBlock::List { ordered: true, .. } => {
            ordered_marker(text.trim_start()).map_or(indent_columns, |(marker, _)| {
                marker.chars().fold(indent_columns, |n, ch| {
                    n + if ch == '\t' { 4 - n % 4 } else { 1 }
                })
            })
        }
        MarkdownBlock::Quote => indent_columns + 2,
        MarkdownBlock::Code { .. } => indent_columns + 2,
        _ => indent_columns,
    }
    .min(64);
    Some(RenderedLine {
        block,
        indent_columns,
        continuation_indent,
        spans: if code {
            code_line(&text)
        } else {
            format_line(&text, columns)
        },
    })
}
fn block_kind(text: &str, code: bool, columns: usize) -> MarkdownBlock {
    let body = text.trim_start();
    if code {
        let (marker, count) = body.as_bytes().first().map_or((0, 0), |&m| {
            (m, body.bytes().take_while(|&b| b == m).count())
        });
        return MarkdownBlock::Code {
            fence: matches!(marker, b'`' | b'~') && count >= 3,
        };
    }
    if body.is_empty() {
        return MarkdownBlock::Blank;
    }
    if let Some((cells, separator)) = table_shape(body, columns) {
        return MarkdownBlock::Table {
            columns: cells as u8,
            separator,
        };
    }
    let heading = body.bytes().take_while(|&b| b == b'#').count();
    if (1..=6).contains(&heading)
        && body
            .as_bytes()
            .get(heading)
            .is_some_and(u8::is_ascii_whitespace)
    {
        return MarkdownBlock::Heading(heading as u8);
    }
    if body.starts_with('>') {
        return MarkdownBlock::Quote;
    }
    if body.len() >= 3
        && body
            .chars()
            .filter(|c| !c.is_whitespace())
            .all(|c| matches!(c, '-' | '_' | '*'))
    {
        return MarkdownBlock::Rule;
    }
    if ["- ", "* ", "+ "]
        .iter()
        .any(|prefix| body.starts_with(prefix))
    {
        let item = &body[2..];
        let task = if item.starts_with("[ ] ") {
            Some(false)
        } else if item.starts_with("[x] ") || item.starts_with("[X] ") {
            Some(true)
        } else {
            None
        };
        return MarkdownBlock::List {
            ordered: false,
            task,
        };
    }
    let digits = body.bytes().take_while(u8::is_ascii_digit).count();
    if (1..=9).contains(&digits)
        && matches!(body.as_bytes().get(digits), Some(b'.' | b')'))
        && body
            .as_bytes()
            .get(digits + 1)
            .is_some_and(u8::is_ascii_whitespace)
    {
        return MarkdownBlock::List {
            ordered: true,
            task: None,
        };
    }
    MarkdownBlock::Paragraph
}
fn code_line(text: &str) -> Vec<MarkdownSpan> {
    let trim = text.trim_start();
    let (marker, n) = trim.as_bytes().first().map_or((0, 0), |&m| {
        (m, trim.bytes().take_while(|&b| b == m).count())
    });
    let text = if matches!(marker, b'`' | b'~') && n >= 3 {
        format!("  Código {}", trim[n..].trim())
    } else {
        format!("  {text}")
    };
    vec![MarkdownSpan {
        text,
        kind: SyntaxKind::Code,
        style: MarkdownInlineStyle::Plain,
        target: None,
    }]
}
fn push(spans: &mut Vec<MarkdownSpan>, text: &str, kind: SyntaxKind) {
    push_style(spans, text, kind, MarkdownInlineStyle::Plain);
}
fn push_style(
    spans: &mut Vec<MarkdownSpan>,
    text: &str,
    kind: SyntaxKind,
    style: MarkdownInlineStyle,
) {
    push_target(spans, text, kind, style, None);
}
fn push_target(
    spans: &mut Vec<MarkdownSpan>,
    text: &str,
    kind: SyntaxKind,
    style: MarkdownInlineStyle,
    target: Option<&Arc<MarkdownTarget>>,
) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = spans.last_mut().filter(|s| {
        s.kind == kind
            && s.style == style
            && match (&s.target, target) {
                (None, None) => true,
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                _ => false,
            }
    }) {
        last.text.push_str(text);
    } else {
        spans.push(MarkdownSpan {
            text: text.into(),
            kind,
            style,
            target: target.cloned(),
        });
    }
}
fn format_line(text: &str, columns: usize) -> Vec<MarkdownSpan> {
    let trimmed = text.trim_start();
    let indent = &text[..text.len() - trimmed.len()];
    let mut body = trimmed;
    let mut base = SyntaxKind::Plain;
    let mut out = Vec::new();
    if let Some(table) = table_line(body, columns) {
        return table;
    }
    let heading = body.bytes().take_while(|&b| b == b'#').count();
    if (1..=6).contains(&heading)
        && body
            .as_bytes()
            .get(heading)
            .is_some_and(u8::is_ascii_whitespace)
    {
        body = body[heading..].trim_start();
        base = SyntaxKind::Heading;
    } else if let Some(rest) = body.strip_prefix('>') {
        push(&mut out, "│ ", SyntaxKind::Comment);
        body = rest.trim_start();
    } else if body.len() >= 3
        && body
            .chars()
            .filter(|c| !c.is_whitespace())
            .all(|c| matches!(c, '-' | '_' | '*'))
    {
        return vec![MarkdownSpan {
            text: "─".repeat(columns),
            kind: SyntaxKind::Comment,
            style: MarkdownInlineStyle::Plain,
            target: None,
        }];
    } else if ["- ", "* ", "+ "]
        .iter()
        .any(|prefix| body.starts_with(prefix))
    {
        push(&mut out, indent, base);
        body = &body[2..];
        if let Some(rest) = body.strip_prefix("[ ] ") {
            push(&mut out, "☐ ", SyntaxKind::Comment);
            body = rest;
        } else if let Some(rest) = body
            .strip_prefix("[x] ")
            .or_else(|| body.strip_prefix("[X] "))
        {
            push(&mut out, "☑ ", SyntaxKind::Code);
            body = rest;
        } else {
            push(&mut out, "• ", SyntaxKind::Comment);
        }
    } else {
        push(&mut out, indent, base);
        if let Some((marker, rest)) = ordered_marker(body) {
            push(&mut out, marker, SyntaxKind::Comment);
            body = rest;
        }
    }
    let mut at = 0;
    let mut work = LINE_BYTES * 4;
    while at < body.len() && out.len() < 4096 {
        let rest = &body[at..];
        if work == 0 {
            push(&mut out, rest, base);
            at = body.len();
            break;
        }
        if let Some(escaped) = rest.strip_prefix('\\') {
            if let Some(ch) = escaped.chars().next() {
                let mut buf = [0; 4];
                push(&mut out, ch.encode_utf8(&mut buf), base);
                at += 1 + ch.len_utf8();
                continue;
            }
        }
        let mut matched = false;
        for (delimiter, kind, style) in [
            ("**", SyntaxKind::Emphasis, MarkdownInlineStyle::Strong),
            ("__", SyntaxKind::Emphasis, MarkdownInlineStyle::Strong),
            ("~~", SyntaxKind::Comment, MarkdownInlineStyle::Strike),
            ("`", SyntaxKind::Code, MarkdownInlineStyle::Plain),
            ("*", SyntaxKind::Emphasis, MarkdownInlineStyle::Emphasis),
            ("_", SyntaxKind::Emphasis, MarkdownInlineStyle::Emphasis),
        ] {
            if let Some(tail) = rest.strip_prefix(delimiter) {
                if let Some(end) = find_bounded(tail, delimiter, &mut work).filter(|&end| end > 0) {
                    push_style(&mut out, &tail[..end], kind, style);
                    at += delimiter.len() * 2 + end;
                    matched = true;
                    break;
                }
            }
        }
        if matched {
            continue;
        }
        let image = rest.starts_with("![");
        let link = if image { &rest[1..] } else { rest };
        if let Some(tail) = link.strip_prefix('[') {
            if let Some(end) = find_bounded(tail, "](", &mut work) {
                if let Some((destination, consumed)) =
                    inline_destination(&tail[end + 2..], &mut work)
                {
                    if !destination.is_empty() && destination.len() <= 4096 {
                        let target = Arc::new(MarkdownTarget {
                            destination: unescape_destination(destination),
                            image,
                        });
                        let label = if tail[..end].is_empty() {
                            if image {
                                "Imagen"
                            } else {
                                "Enlace"
                            }
                        } else {
                            &tail[..end]
                        };
                        if image {
                            push(&mut out, "Imagen: ", SyntaxKind::Comment);
                        }
                        push_target(
                            &mut out,
                            label,
                            SyntaxKind::Link,
                            MarkdownInlineStyle::Plain,
                            Some(&target),
                        );
                        at += usize::from(image) + 1 + end + 2 + consumed;
                        continue;
                    }
                }
            }
        }
        let ch = rest.chars().next().unwrap();
        let mut buf = [0; 4];
        if ch.is_control() && ch != '\t' {
            push(
                &mut out,
                &format!("\\u{{{:X}}}", ch as u32),
                SyntaxKind::Comment,
            );
        } else {
            push(&mut out, ch.encode_utf8(&mut buf), base);
        }
        at += ch.len_utf8();
    }
    if at < body.len() {
        push(&mut out, &body[at..], base);
    }
    out
}

fn indentation(text: &str) -> usize {
    text.chars()
        .take_while(|&c| matches!(c, ' ' | '\t'))
        .fold(0usize, |n, ch| {
            if ch == '\t' {
                n.saturating_add(4 - n % 4)
            } else {
                n.saturating_add(1)
            }
        })
}
fn ordered_marker(text: &str) -> Option<(&str, &str)> {
    let digits = text.bytes().take_while(u8::is_ascii_digit).count();
    if !(1..=9).contains(&digits) || !matches!(text.as_bytes().get(digits), Some(b'.' | b')')) {
        return None;
    }
    let mut end = digits + 1;
    let mut whitespace = false;
    for ch in text[end..].chars() {
        if !matches!(ch, ' ' | '\t') {
            break;
        }
        whitespace = true;
        end += ch.len_utf8();
    }
    whitespace.then_some((&text[..end], &text[end..]))
}
/// Return a bounded inline destination plus bytes through its final ')'.
/// Titles are skipped, parenthesized file names and escaped punctuation survive.
fn inline_destination<'a>(text: &'a str, work: &mut usize) -> Option<(&'a str, usize)> {
    if text.len() > *work {
        *work = 0;
        return None;
    }
    *work -= text.len();
    let text_trim = text.trim_start();
    let start = text.len() - text_trim.len();
    if let Some(inner) = text_trim.strip_prefix('<') {
        let end = inner.find('>')?;
        let destination = &inner[..end];
        let tail = &inner[end + 1..];
        let end_tail = destination_tail(tail)?;
        return Some((destination, start + 1 + end + 1 + end_tail));
    }
    let mut depth = 0usize;
    let mut escaped = false;
    for (at, ch) in text_trim.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        match ch {
            '(' => {
                depth += 1;
                if depth > 16 {
                    return None;
                }
            }
            ')' if depth == 0 => return Some((&text_trim[..at], start + at + 1)),
            ')' => depth -= 1,
            c if c.is_whitespace() && depth == 0 => {
                let tail = &text_trim[at..];
                return Some((&text_trim[..at], start + at + destination_tail(tail)?));
            }
            _ => {}
        }
    }
    None
}
fn destination_tail(text: &str) -> Option<usize> {
    let trim = text.trim_start();
    let skip = text.len() - trim.len();
    if trim.starts_with(')') {
        return Some(skip + 1);
    }
    let quote = trim.chars().next()?;
    if !matches!(quote, '\'' | '"') {
        return None;
    }
    let rest = &trim[quote.len_utf8()..];
    let mut escaped = false;
    for (at, ch) in rest.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == quote {
            let tail = &rest[at + ch.len_utf8()..];
            let ending = tail.trim_start();
            return ending.starts_with(')').then_some(
                skip + quote.len_utf8() + at + ch.len_utf8() + tail.len() - ending.len() + 1,
            );
        }
    }
    None
}
fn unescape_destination(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' && chars.peek().is_some_and(char::is_ascii_punctuation) {
            output.push(chars.next().unwrap());
        } else {
            output.push(ch);
        }
    }
    output
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkdownDestination {
    External(String),
    Local {
        path: PathBuf,
        fragment: Option<String>,
    },
    Anchor(String),
}
fn decode_destination(text: &str) -> Result<String, String> {
    let mut bytes = text.bytes();
    let mut decoded = Vec::with_capacity(text.len());
    while let Some(byte) = bytes.next() {
        decoded.push(if byte == b'%' {
            let hi = bytes
                .next()
                .and_then(|c| char::from(c).to_digit(16))
                .ok_or("Escape de enlace incompleto o inválido")?;
            let lo = bytes
                .next()
                .and_then(|c| char::from(c).to_digit(16))
                .ok_or("Escape de enlace incompleto o inválido")?;
            (hi * 16 + lo) as u8
        } else {
            byte
        });
    }
    let decoded = String::from_utf8(decoded).map_err(|_| "El destino no contiene UTF-8 válido")?;
    if decoded.chars().any(char::is_control) {
        return Err("El enlace contiene caracteres de control".into());
    }
    Ok(decoded)
}
/// Resolve an explicit user intention. No filesystem, network or launch operation
/// occurs here; hosts must revalidate document identity and confine local access.
pub fn resolve_destination(
    document_path: &Path,
    target: &str,
) -> Result<MarkdownDestination, String> {
    if target.len() > 4096 || target.chars().any(char::is_control) {
        return Err("Destino Markdown demasiado largo o inválido".into());
    }
    let target = target.trim();
    let target = target
        .strip_prefix('<')
        .and_then(|s| s.strip_suffix('>'))
        .unwrap_or(target);
    if target.is_empty() {
        return Err("El enlace no tiene destino".into());
    }
    let decoded = decode_destination(target)?;
    if let Some((scheme, authority)) = target.split_once("://") {
        if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
            return Err("Solo se admiten enlaces web http/https".into());
        }
        let host = authority.split(['/', '?', '#']).next().unwrap_or("");
        if host.is_empty()
            || target.chars().any(char::is_whitespace)
            || decoded.contains('\\')
            || target.contains(['"', '<', '>'])
        {
            return Err("Dirección web inválida".into());
        }
        return Ok(MarkdownDestination::External(format!(
            "{}://{authority}",
            if scheme.eq_ignore_ascii_case("https") {
                "https"
            } else {
                "http"
            }
        )));
    }
    let (local, fragment) = target
        .split_once('#')
        .map_or((target, None), |(p, f)| (p, Some(f)));
    let fragment = fragment.map(decode_destination).transpose()?;
    let local = decode_destination(local)?;
    if local.is_empty() {
        return Ok(MarkdownDestination::Anchor(fragment.unwrap_or_default()));
    }
    if local.starts_with(['/', '\\']) || local.contains([':', '\\', '?', '*', '"', '<', '>', '|']) {
        return Err("El enlace local debe ser una ruta relativa".into());
    }
    if !document_path.is_absolute() {
        return Err("Guarda el documento antes de abrir rutas relativas".into());
    }
    let mut path = document_path
        .parent()
        .ok_or("El documento no tiene carpeta")?
        .to_path_buf();
    for part in Path::new(&local).components() {
        match part {
            Component::Normal(name) => path.push(name),
            Component::CurDir => {}
            Component::ParentDir if path.pop() => {}
            _ => return Err("La ruta relativa sale del volumen del documento".into()),
        }
    }
    Ok(MarkdownDestination::Local { path, fragment })
}
/// Bounded heading slug used for explicit fragment navigation; duplicate headings
/// can be handled by the host in source order without a permanent index.
pub fn heading_anchor(text: &str) -> String {
    let mut result = String::new();
    let mut separator = false;
    for ch in text.chars().take(4096).flat_map(char::to_lowercase) {
        if ch.is_alphanumeric() || matches!(ch, '-' | '_') {
            if separator && !result.is_empty() && !result.ends_with('-') {
                result.push('-');
            }
            separator = false;
            if result.len() + ch.len_utf8() > 4096 {
                break;
            }
            result.push(ch);
        } else if ch.is_whitespace() {
            separator = true;
        }
    }
    result
}

/// Cooperative source-order anchor lookup. No index or editor mutation is retained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkdownAnchorProgress {
    Pending,
    Found(usize),
    NotFound,
    Stale,
    Limited,
}
pub struct MarkdownAnchorScan {
    stamp: TaskStamp,
    fragment: String,
    next_line: usize,
    read_bytes: usize,
    fence: Option<(u8, usize)>,
    finished: Option<MarkdownAnchorProgress>,
}
impl MarkdownAnchorScan {
    pub fn new(stamp: TaskStamp, fragment: String) -> Self {
        let invalid = fragment.len() > 4096 || fragment.chars().any(char::is_control);
        let at_top = fragment.trim().is_empty();
        let fragment = if invalid {
            String::new()
        } else {
            heading_anchor(&fragment)
        };
        let finished = if invalid {
            Some(MarkdownAnchorProgress::Limited)
        } else if at_top {
            Some(MarkdownAnchorProgress::Found(0))
        } else if fragment.is_empty() {
            Some(MarkdownAnchorProgress::NotFound)
        } else {
            None
        };
        Self {
            stamp,
            fragment,
            next_line: 0,
            read_bytes: 0,
            fence: None,
            finished,
        }
    }
    fn finish(&mut self, result: MarkdownAnchorProgress) -> MarkdownAnchorProgress {
        self.finished = Some(result);
        result
    }
    pub fn step(&mut self, app: &Workbench) -> MarkdownAnchorProgress {
        if !app.task_stamp_is_current(self.stamp) {
            return self.finish(MarkdownAnchorProgress::Stale);
        }
        if let Some(done) = self.finished {
            return done;
        }
        let Some(doc) = app.document_by_id(self.stamp.document) else {
            return self.finish(MarkdownAnchorProgress::Stale);
        };
        let Some(count) = app.line_count_for_tab(doc.tab_id) else {
            return self.finish(MarkdownAnchorProgress::Stale);
        };
        let mut step_bytes = 0;
        for _ in 0..64 {
            if self.next_line >= count {
                return self.finish(MarkdownAnchorProgress::NotFound);
            }
            if self.next_line >= 65536 {
                return self.finish(MarkdownAnchorProgress::Limited);
            }
            let line = self.next_line;
            let Some((range, _)) = app.line_range_for_tab(doc.tab_id, line) else {
                return self.finish(MarkdownAnchorProgress::Stale);
            };
            if range.len() > LINE_BYTES || self.read_bytes + range.len() > 4 * 1024 * 1024 {
                return self.finish(MarkdownAnchorProgress::Limited);
            }
            if step_bytes + range.len() > WORK_BYTES {
                return MarkdownAnchorProgress::Pending;
            }
            let Ok(bytes) = doc.session.document().read_range(range.clone()) else {
                return self.finish(MarkdownAnchorProgress::Limited);
            };
            let mut text = String::with_capacity(bytes.len());
            for scalar in DecodedScalars::new(&bytes, self.stamp.encoding, range.start) {
                if !scalar.valid {
                    return self.finish(MarkdownAnchorProgress::Limited);
                }
                text.push(scalar.character);
            }
            self.next_line += 1;
            self.read_bytes += range.len();
            step_bytes += range.len();
            let body = text.trim_start();
            let marker = body.as_bytes().first().copied().unwrap_or(0);
            let length = body.bytes().take_while(|&b| b == marker).count();
            if let Some((fence, minimum)) = self.fence {
                if marker == fence && length >= minimum && body[length..].trim().is_empty() {
                    self.fence = None;
                }
                continue;
            }
            if matches!(marker, b'`' | b'~') && length >= 3 {
                self.fence = Some((marker, length));
                continue;
            }
            if matches!(block_kind(&text, false, 256), MarkdownBlock::Heading(_)) {
                let spans = format_line(&text, 256);
                let rendered: String = spans.iter().map(|span| span.text.as_str()).collect();
                if heading_anchor(&rendered) == self.fragment {
                    return self.finish(MarkdownAnchorProgress::Found(line));
                }
            }
        }
        if self.next_line >= count {
            self.finish(MarkdownAnchorProgress::NotFound)
        } else {
            MarkdownAnchorProgress::Pending
        }
    }
}
fn table_shape(text: &str, columns: usize) -> Option<(usize, bool)> {
    let inner = text.strip_prefix('|')?.strip_suffix('|')?;
    let mut count = 0;
    let mut separator = true;
    for cell in inner.split('|').take(9) {
        count += 1;
        let body = cell.trim().trim_matches(':');
        separator &= body.len() >= 3 && body.bytes().all(|b| b == b'-');
    }
    ((2..=8).contains(&count) && columns > count * 5).then_some((count, separator))
}
fn table_line(text: &str, columns: usize) -> Option<Vec<MarkdownSpan>> {
    let (count, separator) = table_shape(text, columns)?;
    let cells = text[1..text.len() - 1]
        .split('|')
        .map(str::trim)
        .collect::<Vec<_>>();
    let width = (columns - 1) / count - 1;
    let mut result = Vec::new();
    push(
        &mut result,
        if separator { "├" } else { "│" },
        SyntaxKind::Comment,
    );
    for (i, cell) in cells.iter().enumerate() {
        if separator {
            push(&mut result, &"─".repeat(width), SyntaxKind::Comment);
        } else {
            let mut text = grapheme::clip(cell, width.saturating_sub(2));
            if text.len() < cell.len() {
                text = grapheme::clip(cell, width.saturating_sub(3));
                text.push('…');
            }
            push(&mut result, " ", SyntaxKind::Plain);
            push(&mut result, &text, SyntaxKind::Plain);
            push(
                &mut result,
                &" ".repeat(width - 1 - grapheme::width(&text)),
                SyntaxKind::Plain,
            );
        }
        push(
            &mut result,
            if separator {
                if i + 1 == cells.len() {
                    "┤"
                } else {
                    "┼"
                }
            } else {
                "│"
            },
            SyntaxKind::Comment,
        );
    }
    Some(result)
}
fn find_bounded(text: &str, needle: &str, work: &mut usize) -> Option<usize> {
    if text.len() > *work {
        *work = 0;
        return None;
    }
    *work -= text.len();
    text.find(needle)
}
fn wrap_line(line: RenderedLine, columns: usize) -> Vec<Vec<MarkdownSpan>> {
    wrap_indented(line.spans, columns, line.continuation_indent)
}
fn wrap_indented(
    spans: Vec<MarkdownSpan>,
    columns: usize,
    hanging: usize,
) -> Vec<Vec<MarkdownSpan>> {
    let mut rows = vec![Vec::new()];
    let mut column = 0;
    for span in spans {
        for cluster in grapheme::graphemes(&span.text) {
            let mut width = if cluster == "\t" {
                4 - column % 4
            } else {
                grapheme::width(cluster).max(1)
            };
            if column + width > columns && column > 0 {
                let mut continuation = Vec::new();
                column = hanging.min(columns.saturating_sub(4));
                push(&mut continuation, &" ".repeat(column), SyntaxKind::Plain);
                rows.push(continuation);
            }
            if cluster == "\t" {
                width = 4 - column % 4;
            }
            let row = rows.last_mut().unwrap();
            if cluster.len() > grapheme::MAX_CLUSTER_BYTES {
                let marker = grapheme::clip("[grafema largo]", columns - column);
                push(row, &marker, SyntaxKind::Comment);
                column += grapheme::width(&marker);
            } else if cluster == "\t" {
                push_target(
                    row,
                    &" ".repeat(width),
                    span.kind,
                    span.style,
                    span.target.as_ref(),
                );
                column += width;
            } else if cluster.chars().any(char::is_control) || grapheme::width(cluster) == 0 {
                push(row, "·", SyntaxKind::Comment);
                column += 1;
            } else {
                push_target(row, cluster, span.kind, span.style, span.target.as_ref());
                column += width;
            }
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use jc_text::TextEncoding;
    fn text(p: &MarkdownPresentation) -> String {
        p.rows
            .iter()
            .map(|r| r.spans.iter().map(|s| s.text.as_str()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }
    #[test]
    fn markdown_preview_formats_and_wraps_without_executing_links_or_mutating_bytes() {
        let mut app = Workbench::default();
        let tab = app.new_file("demo.md", TextEncoding::Utf8);
        let source="# Título\r\nTexto **fuerte** y `code` [sitio](javascript:inert)\n- [x] Listo\n> Cita\n```rust\nfn demo() {}\n```";
        app.insert_text(source).unwrap();
        let before = app.active_document().unwrap().session.document().revision();
        let mut view = MarkdownView::default();
        let p = view.prepare(&app, tab, 128, 80);
        let rendered = text(&p);
        assert!(rendered.contains("Título") && !rendered.contains("# Título"));
        assert!(rendered.contains("fuerte") && !rendered.contains("**fuerte**"));
        assert!(
            rendered.contains("sitio")
                && !rendered.contains("javascript:inert")
                && rendered.contains("☑ Listo")
                && rendered.contains("│ Cita")
                && rendered.contains("fn demo() {}")
        );
        assert_eq!(app.active_bytes().unwrap(), source.as_bytes());
        assert_eq!(
            app.active_document().unwrap().session.document().revision(),
            before
        );
        assert!(p
            .rows
            .iter()
            .flat_map(|r| &r.spans)
            .any(|s| s.kind == SyntaxKind::Heading));
        assert!(p
            .rows
            .iter()
            .flat_map(|r| &r.spans)
            .filter_map(|s| s.target.as_ref())
            .any(|t| t.destination == "javascript:inert" && !t.image));
    }
    #[test]
    fn markdown_preview_scroll_keeps_wrapped_clusters_and_returns_to_previous_page() {
        let mut app = Workbench::default();
        let tab = app.new_file("demo.md", TextEncoding::Utf8);
        app.insert_text(&"e\u{301}界😀 ".repeat(100)).unwrap();
        let mut view = MarkdownView::default();
        let first = view.prepare(&app, tab, 4, 20);
        assert_eq!(first.rows.len(), 4);
        view.scroll(4, &app, tab);
        let second = view.prepare(&app, tab, 4, 20);
        assert_eq!(second.rows.len(), 4);
        assert!(second.rows.iter().all(|r| r.source_line == 0));
        view.scroll(-4, &app, tab);
        let again = view.prepare(&app, tab, 4, 20);
        assert_eq!(text(&again), text(&first));
    }
    #[test]
    fn markdown_end_and_up_reach_last_wrapped_rows_without_prior_history() {
        let mut app = Workbench::default();
        let tab = app.new_file("wrapped.md", TextEncoding::Utf8);
        let long = format!("{}FINAL", "abcd ".repeat(40));
        app.insert_text(&format!("{long}\nnext")).unwrap();
        let mut view = MarkdownView::at_line(1);
        view.prepare(&app, tab, 3, 20);
        view.scroll(-1, &app, tab);
        let previous = view.prepare(&app, tab, 1, 20);
        assert!(text(&previous).contains("FINAL"));
        let mut single = Workbench::default();
        let tab = single.new_file("single.md", TextEncoding::Utf8);
        single.insert_text(&long).unwrap();
        let mut view = MarkdownView::default();
        view.prepare(&single, tab, 3, 20);
        view.last(&single, tab);
        let end = view.prepare(&single, tab, 3, 20);
        assert!(text(&end).contains("FINAL"));
        let before = text(&end);
        view.scroll(1, &single, tab);
        assert_eq!(text(&view.prepare(&single, tab, 3, 20)), before);
        let source = single.active_bytes().unwrap();
        view.first();
        view.prepare(&single, tab, 3, 20);
        view.scroll(1, &single, tab);
        view.scroll(1, &single, tab);
        let twice = text(&view.prepare(&single, tab, 1, 20));
        view.first();
        view.prepare(&single, tab, 3, 20);
        view.scroll(2, &single, tab);
        assert_eq!(text(&view.prepare(&single, tab, 1, 20)), twice);
        assert_eq!(single.active_bytes().unwrap(), source);
    }
    #[test]
    fn markdown_indented_fenced_code_keeps_markup_literal_and_source_bytes_exact() {
        let mut app = Workbench::default();
        let tab = app.new_file("fence.md", TextEncoding::Utf8);
        let source = "  ```md\r\n  # literal\r\n  **literal**\n  ```\n# Heading";
        app.insert_text(source).unwrap();
        let before = app.active_bytes().unwrap();
        let result = MarkdownView::default().prepare(&app, tab, 128, 80);
        let reading = text(&result);
        assert!(reading.contains("# literal") && reading.contains("**literal**"));
        assert!(result
            .rows
            .iter()
            .filter(|row| matches!(row.source_line, 1 | 2))
            .flat_map(|row| &row.spans)
            .all(|span| span.kind == SyntaxKind::Code));
        assert_eq!(app.active_bytes().unwrap(), before);
    }
    #[test]
    fn markdown_tables_keep_unicode_cells_visible_and_mark_clipping_without_source_edits() {
        let mut app = Workbench::default();
        let tab = app.new_file("tabla.md", TextEncoding::Utf8);
        let source="| Campo | Valor |\r\n|---|---|\n| ñ | Sí |\n| Nombre largo que no cabe en la celda | otro |";
        app.insert_text(source).unwrap();
        let p = MarkdownView::default().prepare(&app, tab, 128, 40);
        let value = text(&p);
        assert!(
            value.contains("│ Campo")
                && value.contains("┼")
                && value.contains("Sí")
                && value.contains('…')
        );
        assert!(p.rows.iter().all(|row| grapheme::width(
            &row.spans
                .iter()
                .map(|s| s.text.as_str())
                .collect::<String>()
        ) <= 40));
        assert_eq!(app.active_bytes().unwrap(), source.as_bytes());
    }
    #[test]
    fn markdown_semantic_blocks_preserve_structure_and_wrap_continuations_without_edits() {
        let mut app = Workbench::default();
        let tab = app.new_file("blocks.md", TextEncoding::Utf8);
        let source = "## Título\r\nUn párrafo suficientemente largo para ocupar varias filas con Unicode: café 😀.\r\n\r\n> Una cita\r\n---\r\n- Elemento\r\n- [x] Tarea\r\n1. Numerado\r\n| Campo | Valor |\r\n|---|---|\r\n```md\r\n# literal\r\n```\r\n### Final";
        app.insert_text(source).unwrap();
        let revision = app.active_document().unwrap().session.document().revision();
        let panes = format!("{:?}", app.panes());
        let p = MarkdownView::default().prepare(&app, tab, 128, 40);
        let first = |line| p.rows.iter().find(|row| row.source_line == line).unwrap();
        assert_eq!(first(0).block, MarkdownBlock::Heading(2));
        assert_eq!(first(1).block, MarkdownBlock::Paragraph);
        assert_eq!(first(2).block, MarkdownBlock::Blank);
        assert_eq!(first(3).block, MarkdownBlock::Quote);
        assert_eq!(first(4).block, MarkdownBlock::Rule);
        assert_eq!(
            first(5).block,
            MarkdownBlock::List {
                ordered: false,
                task: None
            }
        );
        assert_eq!(
            first(6).block,
            MarkdownBlock::List {
                ordered: false,
                task: Some(true)
            }
        );
        assert_eq!(
            first(7).block,
            MarkdownBlock::List {
                ordered: true,
                task: None
            }
        );
        assert_eq!(
            first(8).block,
            MarkdownBlock::Table {
                columns: 2,
                separator: false
            }
        );
        assert_eq!(
            first(9).block,
            MarkdownBlock::Table {
                columns: 2,
                separator: true
            }
        );
        assert_eq!(first(10).block, MarkdownBlock::Code { fence: true });
        assert_eq!(first(11).block, MarkdownBlock::Code { fence: false });
        assert_eq!(first(12).block, MarkdownBlock::Code { fence: true });
        assert_eq!(first(13).block, MarkdownBlock::Heading(3));
        assert!(!first(1).continuation);
        assert!(p
            .rows
            .iter()
            .filter(|row| row.source_line == 1)
            .skip(1)
            .all(|row| row.continuation));
        assert!(p.rows.iter().filter(|row| row.source_line == 1).count() > 1);
        assert_eq!(app.active_bytes().unwrap(), source.as_bytes());
        assert_eq!(
            app.active_document().unwrap().session.document().revision(),
            revision
        );
        assert_eq!(format!("{:?}", app.panes()), panes);
    }
    #[test]
    fn markdown_semantic_notice_and_scrolled_heading_keep_source_and_continuation_identity() {
        let mut app = Workbench::default();
        let tab = app.new_file("limited.md", TextEncoding::Utf8);
        app.insert_text(&format!(
            "{}\n# {}",
            "a".repeat(LINE_BYTES + 1),
            "título ".repeat(20)
        ))
        .unwrap();
        let before = app.active_bytes().unwrap();
        let mut view = MarkdownView::default();
        let notice = view.prepare(&app, tab, 1, 20);
        assert_eq!(notice.rows[0].block, MarkdownBlock::Notice);
        view.last(&app, tab);
        let end = view.prepare(&app, tab, 1, 20);
        assert_eq!(end.rows[0].block, MarkdownBlock::Heading(1));
        assert!(end.rows[0].continuation);
        view.scroll(-1, &app, tab);
        let previous = view.prepare(&app, tab, 1, 20);
        assert_eq!(previous.rows[0].block, MarkdownBlock::Heading(1));
        assert!(previous.rows[0].continuation);
        assert_eq!(app.active_bytes().unwrap(), before);
    }
    #[test]
    fn markdown_semantic_inline_styles_survive_wrapping_without_source_edits() {
        let mut app = Workbench::default();
        let tab = app.new_file("styles.md", TextEncoding::Utf8);
        let source = "Texto **negrita larga café** *cursiva larga 😀* ~~tachado largo~~ __fuerte__ _énfasis_";
        app.insert_text(source).unwrap();
        let revision = app.active_document().unwrap().session.document().revision();
        let p = MarkdownView::default().prepare(&app, tab, 128, 12);
        assert!(p.rows.len() > 1);
        let styled = |style| {
            p.rows
                .iter()
                .flat_map(|r| &r.spans)
                .filter(|s| s.style == style)
                .map(|s| s.text.as_str())
                .collect::<String>()
        };
        assert_eq!(
            styled(MarkdownInlineStyle::Strong),
            "negrita larga caféfuerte"
        );
        assert_eq!(
            styled(MarkdownInlineStyle::Emphasis),
            "cursiva larga 😀énfasis"
        );
        assert_eq!(styled(MarkdownInlineStyle::Strike), "tachado largo");
        assert_eq!(styled(MarkdownInlineStyle::Plain), "Texto     ");
        assert!(!text(&p).contains("**") && !text(&p).contains("~~"));
        assert_eq!(app.active_bytes().unwrap(), source.as_bytes());
        assert_eq!(
            app.active_document().unwrap().session.document().revision(),
            revision
        );
    }
    #[test]
    fn markdown_targets_keep_image_and_link_identity_across_nested_list_wrapping() {
        let mut app = Workbench::default();
        let tab = app.new_file("targets.md", TextEncoding::Utf8);
        let source = "- Principal\r\n  - [x] [una etiqueta larga 😀 que envuelve](../guía%20rápida.md#inicio)\r\n    - [profundo](deep.md)\r\n\t12) ![captura extensa](<assets/mi imagen(1).png> \"Vista\")\r\n```md\r\n    - [literal](oculto.md)\r\n```\r\n";
        app.insert_text(source).unwrap();
        let bytes = app.active_bytes().unwrap();
        let revision = app.active_document().unwrap().session.document().revision();
        let p = MarkdownView::default().prepare(&app, tab, 128, 20);
        let nested = p
            .rows
            .iter()
            .filter(|row| row.source_line == 1)
            .collect::<Vec<_>>();
        assert!(nested.len() > 1);
        assert!(nested
            .iter()
            .all(|r| r.indent_columns == 2 && r.continuation_indent == 4));
        assert!(nested
            .iter()
            .skip(1)
            .all(|r| r.spans.first().is_some_and(|s| s.text.starts_with("    "))));
        let links = nested
            .iter()
            .flat_map(|r| &r.spans)
            .filter_map(|s| s.target.as_ref())
            .collect::<Vec<_>>();
        assert!(
            links.len() > 1
                && links
                    .iter()
                    .all(|t| !t.image && t.destination == "../guía%20rápida.md#inicio")
        );
        assert!(links.iter().all(|t| Arc::ptr_eq(t, links[0])));
        let image = p
            .rows
            .iter()
            .filter(|r| r.source_line == 3)
            .flat_map(|r| &r.spans)
            .find_map(|s| s.target.as_ref())
            .unwrap();
        assert!(image.image);
        assert_eq!(image.destination, "assets/mi imagen(1).png");
        assert!(p
            .rows
            .iter()
            .filter(|r| r.source_line == 3)
            .all(|r| r.indent_columns == 4 && r.continuation_indent == 8));
        assert!(p
            .rows
            .iter()
            .filter(|r| r.source_line == 2)
            .all(|r| matches!(r.block, MarkdownBlock::List { .. })
                && r.indent_columns == 4
                && r.continuation_indent == 6));
        assert!(p
            .rows
            .iter()
            .filter(|r| r.source_line == 5)
            .all(|r| matches!(r.block, MarkdownBlock::Code { fence: false })
                && r.spans.iter().all(|s| s.target.is_none())));
        assert!(!text(&p).contains("../guía"));
        assert_eq!(app.active_bytes().unwrap(), bytes);
        assert_eq!(
            app.active_document().unwrap().session.document().revision(),
            revision
        );
    }
    #[test]
    fn markdown_targets_resolve_percent_paths_fragments_and_only_explicit_web_schemes() {
        let document = std::env::current_dir()
            .unwrap()
            .join("work/project/docs/readme.md");
        let expected = document
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("guía rápida.md");
        assert_eq!(
            resolve_destination(&document, "../guía%20rápida.md#introducci%C3%B3n").unwrap(),
            MarkdownDestination::Local {
                path: expected,
                fragment: Some("introducción".into())
            }
        );
        assert_eq!(
            resolve_destination(&document, "#una-secci%C3%B3n").unwrap(),
            MarkdownDestination::Anchor("una-sección".into())
        );
        assert_eq!(
            resolve_destination(&document, "HTTPS://example.com/guia#inicio").unwrap(),
            MarkdownDestination::External("https://example.com/guia#inicio".into())
        );
        assert!(matches!(
            resolve_destination(&document, "https://example.com/mi%20imagen.png").unwrap(),
            MarkdownDestination::External(_)
        ));
        for target in [
            "javascript:alert(1)",
            "data:image/png;base64,AA",
            "file:///c:/x",
            "//server/share",
            "%2Froot/x",
            "C%3a/x",
            "x%00.png",
            "x%0d%0a.txt",
            "https://",
            "https://example.com/a%0a",
            "https://example.com/ espacio",
            "x%ZZ",
            "x%FF",
        ] {
            assert!(resolve_destination(&document, target).is_err(), "{target}");
        }
        assert!(resolve_destination(Path::new("sin-titulo.md"), "imagen.png").is_err());
        assert_eq!(
            heading_anchor("## Una **Sección** con `código` 😀"),
            "una-sección-con-código"
        );
    }
    #[test]
    fn markdown_targets_parse_balanced_escaped_destinations_and_leave_code_literal() {
        let mut app = Workbench::default();
        let tab = app.new_file("destinations.md", TextEncoding::Utf8);
        let source = "[A](assets/foto(1).png) [B](assets/foto\\(2\\).png 'título')\n`[no](oculto.md)`\n```md\n[no](otro.md)\n```\n";
        app.insert_text(source).unwrap();
        let p = MarkdownView::default().prepare(&app, tab, 128, 80);
        let targets = p
            .rows
            .iter()
            .flat_map(|r| &r.spans)
            .filter_map(|s| s.target.as_ref())
            .collect::<Vec<_>>();
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].destination, "assets/foto(1).png");
        assert_eq!(targets[1].destination, "assets/foto(2).png");
        assert!(p
            .rows
            .iter()
            .filter(|r| matches!(r.source_line, 1 | 3))
            .flat_map(|r| &r.spans)
            .all(|s| s.target.is_none()));
        assert_eq!(app.active_bytes().unwrap(), source.as_bytes());
    }

    fn stamp(app: &Workbench) -> TaskStamp {
        let doc = app.active_document().unwrap();
        TaskStamp {
            document: doc.id,
            revision: doc.session.document().revision(),
            encoding: doc.session.encoding().unwrap(),
            view: app
                .panes()
                .view(app.panes().active())
                .and_then(|v| v.view_id()),
        }
    }
    #[test]
    fn markdown_anchor_scan_is_cooperative_skips_fences_and_rejects_stale_sources() {
        let mut app = Workbench::default();
        app.new_file("anchors.md", TextEncoding::Utf8);
        let source = format!(
            "```md\n# Destino **final**\n```\n{}# Destino **final**\n",
            "párrafo\n".repeat(130)
        );
        app.insert_text(&source).unwrap();
        let before = app.active_bytes().unwrap();
        let mut scan = MarkdownAnchorScan::new(stamp(&app), "destino-final".into());
        assert_eq!(scan.step(&app), MarkdownAnchorProgress::Pending);
        assert_eq!(scan.step(&app), MarkdownAnchorProgress::Pending);
        assert_eq!(scan.step(&app), MarkdownAnchorProgress::Found(133));
        assert_eq!(app.active_bytes().unwrap(), before);
        let mut stale = MarkdownAnchorScan::new(stamp(&app), "otro".into());
        app.insert_text("cambio").unwrap();
        assert_eq!(stale.step(&app), MarkdownAnchorProgress::Stale);
        let mut absent = MarkdownAnchorScan::new(stamp(&app), "inexistente".into());
        while absent.step(&app) == MarkdownAnchorProgress::Pending {}
        assert_eq!(absent.step(&app), MarkdownAnchorProgress::NotFound);
    }
    #[test]
    fn markdown_anchor_scan_reports_line_and_total_work_limits_without_editing() {
        let mut app = Workbench::default();
        app.new_file("limit.md", TextEncoding::Utf8);
        app.insert_text(&"x".repeat(LINE_BYTES + 1)).unwrap();
        let mut scan = MarkdownAnchorScan::new(stamp(&app), "destino".into());
        assert_eq!(scan.step(&app), MarkdownAnchorProgress::Limited);
        assert_eq!(
            app.active_document().unwrap().session.document().len(),
            LINE_BYTES + 1
        );
        let mut app = Workbench::default();
        app.new_file("budget.md", TextEncoding::Utf8);
        app.insert_text(&("x".repeat(LINE_BYTES) + "\n").repeat(257))
            .unwrap();
        let mut scan = MarkdownAnchorScan::new(stamp(&app), "destino".into());
        for _ in 0..15 {
            assert_eq!(scan.step(&app), MarkdownAnchorProgress::Pending);
        }
        assert_eq!(scan.step(&app), MarkdownAnchorProgress::Limited);
    }
}
