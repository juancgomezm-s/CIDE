//! Range-based, inert Markdown reading view. A bounded presentation subset,
//! not a DOM, HTML renderer, external process or second editable document.
use crate::{SyntaxKind, TabId, Workbench};
use jc_text::{grapheme, DecodedScalars, TextEncoding};
const LINE_BYTES: usize = 16 * 1024;
const WORK_BYTES: usize = 256 * 1024;
#[derive(Debug, Clone)]
pub struct MarkdownSpan {
    pub text: String,
    pub kind: SyntaxKind,
}
#[derive(Debug, Clone)]
pub struct MarkdownRow {
    pub source_line: usize,
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
            wrap(
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
            let Some(spans) = line_spans(doc, content, encoding, &syntax, columns) else {
                break;
            };
            let wrapped = wrap(spans, columns);
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
            p.message = "Vista de lectura · enlaces e imágenes inertes · fuente sin cambios".into();
        }
        p
    }
}
fn line_spans(
    doc: &crate::EditorDocument,
    content: std::ops::Range<usize>,
    encoding: TextEncoding,
    syntax: &crate::SyntaxRows,
    columns: usize,
) -> Option<Vec<MarkdownSpan>> {
    if content.len() > LINE_BYTES {
        return Some(vec![MarkdownSpan {
            text: "[Línea mayor de 16 KiB: consulta la fuente]".into(),
            kind: SyntaxKind::Comment,
        }]);
    }
    let bytes = doc.session.document().read_range(content.clone()).ok()?;
    let mut text = String::with_capacity(bytes.len());
    for scalar in DecodedScalars::new(&bytes, encoding, content.start) {
        if !scalar.valid {
            return Some(vec![MarkdownSpan {
                text: "[Codificación inválida: consulta los bytes de fuente]".into(),
                kind: SyntaxKind::Comment,
            }]);
        }
        text.push(scalar.character);
    }
    // The lexer deliberately leaves at most three leading indentation spaces
    // uncolored. Classify the first meaningful byte, not that default Plain gap.
    let indent = bytes.iter().take(3).take_while(|&&b| b == b' ').count();
    Some(
        if syntax.kind_at(content.start + indent) == SyntaxKind::Code {
            code_line(&text)
        } else {
            format_line(&text, columns)
        },
    )
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
    }]
}
fn push(spans: &mut Vec<MarkdownSpan>, text: &str, kind: SyntaxKind) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = spans.last_mut().filter(|s| s.kind == kind) {
        last.text.push_str(text);
    } else {
        spans.push(MarkdownSpan {
            text: text.into(),
            kind,
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
        for (delimiter, kind) in [
            ("**", SyntaxKind::Emphasis),
            ("__", SyntaxKind::Emphasis),
            ("~~", SyntaxKind::Comment),
            ("`", SyntaxKind::Code),
            ("*", SyntaxKind::Emphasis),
            ("_", SyntaxKind::Emphasis),
        ] {
            if let Some(tail) = rest.strip_prefix(delimiter) {
                if let Some(end) = find_bounded(tail, delimiter, &mut work).filter(|&end| end > 0) {
                    push(&mut out, &tail[..end], kind);
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
                if let Some(close) = find_bounded(&tail[end + 2..], ")", &mut work) {
                    if image {
                        push(&mut out, "Imagen: ", SyntaxKind::Comment);
                    }
                    push(&mut out, &tail[..end], SyntaxKind::Link);
                    push(&mut out, " (", SyntaxKind::Comment);
                    push(&mut out, &tail[end + 2..end + 2 + close], SyntaxKind::Link);
                    push(&mut out, ")", SyntaxKind::Comment);
                    at += usize::from(image) + 1 + end + 2 + close + 1;
                    continue;
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
fn table_line(text: &str, columns: usize) -> Option<Vec<MarkdownSpan>> {
    let inner = text.strip_prefix('|')?.strip_suffix('|')?;
    let cells = inner.split('|').map(str::trim).take(33).collect::<Vec<_>>();
    if cells.len() < 2 || cells.len() > 8 || columns < cells.len() * 5 + 1 {
        return None;
    }
    let width = (columns - 1) / cells.len() - 1;
    let separator = cells.iter().all(|cell| {
        let body = cell.trim_matches(':');
        body.len() >= 3 && body.bytes().all(|b| b == b'-')
    });
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
fn wrap(spans: Vec<MarkdownSpan>, columns: usize) -> Vec<Vec<MarkdownSpan>> {
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
                rows.push(Vec::new());
                column = 0;
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
                push(row, &" ".repeat(width), span.kind);
                column += width;
            } else if cluster.chars().any(char::is_control) || grapheme::width(cluster) == 0 {
                push(row, "·", SyntaxKind::Comment);
                column += 1;
            } else {
                push(row, cluster, span.kind);
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
            rendered.contains("javascript:inert")
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
}
