//! Native conversation cards following OmniCore's roles and inert Markdown.
use super::*;
use jc_app::{
    markdown_preview::{self, MarkdownInlineStyle, MarkdownSpan},
    SyntaxKind,
};
use std::ops::Range;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Role {
    User,
    Assistant,
    Activity,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Header,
    Body,
    Meta,
    Code,
    Gap,
}
pub(super) struct Row {
    role: Role,
    group: usize,
    kind: Kind,
    spans: Vec<MarkdownSpan>,
    pub action: Option<(usize, usize)>,
}
pub(super) struct Placed {
    pub area: Rect,
    pub row: Row,
    pub accent: u32,
    pub brand: u32,
}
struct CodeBlock {
    range: Range<usize>,
    fence: Range<usize>,
    language: String,
}
fn code_blocks(text: &str) -> Vec<CodeBlock> {
    let mut blocks = Vec::new();
    let mut offset = 0;
    let mut opened: Option<(u8, usize, usize, usize, String)> = None;
    for line in text.split_inclusive('\n') {
        let trim = line.trim();
        let marker = trim.as_bytes().first().copied().unwrap_or(0);
        let count = trim.bytes().take_while(|b| *b == marker).count();
        if let Some((m, n, start, body, language)) = &opened {
            if marker == *m && count >= *n && trim[count..].is_empty() {
                if offset > *body && blocks.len() < 32 {
                    blocks.push(CodeBlock {
                        range: *body..offset,
                        fence: *start..offset + line.len(),
                        language: language.clone(),
                    });
                }
                opened = None;
            }
        } else if matches!(marker, b'`' | b'~')
            && count >= 3
            && line.len() - line.trim_start().len() <= 3
        {
            let language = trim[count..]
                .split_whitespace()
                .next()
                .unwrap_or("texto")
                .chars()
                .take(32)
                .collect();
            opened = Some((marker, count, offset, offset + line.len(), language));
        }
        offset += line.len();
    }
    blocks
}
fn span(text: String, kind: SyntaxKind) -> MarkdownSpan {
    MarkdownSpan {
        text,
        kind,
        style: MarkdownInlineStyle::Plain,
        target: None,
    }
}
fn row(role: Role, group: usize, kind: Kind, text: String) -> Row {
    Row {
        role,
        group,
        kind,
        spans: vec![span(text, SyntaxKind::Plain)],
        action: None,
    }
}
fn markdown(out: &mut Vec<Row>, text: &str, group: usize, columns: usize) {
    let rendered = markdown_preview::render_message(text, columns);
    for line in rendered.rows {
        out.push(Row {
            role: Role::Assistant,
            group,
            kind: Kind::Body,
            spans: line.spans,
            action: None,
        });
    }
    if rendered.limited {
        out.push(row(
            Role::Assistant,
            group,
            Kind::Meta,
            "Presentación abreviada del mensaje extenso.".into(),
        ));
    }
}
pub(super) fn rows(chat: &chat::Chat, columns: usize, show_activity: bool) -> Vec<Row> {
    let mut out = Vec::new();
    for (turn_index, turn) in chat.turns.iter().enumerate() {
        let user = turn_index * 2;
        let assistant = user + 1;
        out.push(row(
            Role::User,
            user,
            Kind::Header,
            format!("◉ Tú · {}", turn.mode.label()),
        ));
        if let Some(title) = &turn.attachment {
            out.extend(
                display_lines(&format!("Adjunto · {title}"), columns)
                    .map(|s| row(Role::User, user, Kind::Meta, s)),
            );
        }
        out.extend(
            display_lines(&turn.question, columns)
                .take(2048)
                .map(|s| row(Role::User, user, Kind::Body, s)),
        );
        out.push(row(Role::User, user, Kind::Gap, String::new()));
        out.push(row(
            Role::Assistant,
            assistant,
            Kind::Header,
            format!(
                "◈ OMNICORE · {}",
                turn.model.as_deref().unwrap_or("Automático")
            ),
        ));
        match &turn.answer {
            Some(Ok(answer)) => {
                let blocks = code_blocks(&answer.text);
                let mut from = 0;
                for (block_index, block) in blocks.iter().enumerate() {
                    markdown(
                        &mut out,
                        &answer.text[from..block.fence.start],
                        assistant,
                        columns,
                    );
                    out.push(row(
                        Role::Assistant,
                        assistant,
                        Kind::Meta,
                        format!("Código · {}", block.language),
                    ));
                    out.extend(
                        display_lines(&answer.text[block.range.clone()], columns).map(|s| Row {
                            role: Role::Assistant,
                            group: assistant,
                            kind: Kind::Code,
                            spans: vec![span(s, SyntaxKind::Code)],
                            action: None,
                        }),
                    );
                    if answer.complete {
                        let mut action = row(Role::Assistant, assistant, Kind::Meta, String::new());
                        action.action = Some((turn_index, block_index));
                        out.push(action);
                    }
                    from = block.fence.end;
                }
                markdown(&mut out, &answer.text[from..], assistant, columns);
                if !answer.complete {
                    out.push(row(
                        Role::Assistant,
                        assistant,
                        Kind::Meta,
                        "La operación no se completó.".into(),
                    ));
                }
                if show_activity && !answer.activity.is_empty() {
                    out.push(row(
                        Role::Activity,
                        assistant,
                        Kind::Header,
                        "Actividad técnica".into(),
                    ));
                    for activity in &answer.activity {
                        out.extend(
                            display_lines(activity, columns)
                                .take(256)
                                .map(|s| row(Role::Activity, assistant, Kind::Meta, s)),
                        );
                    }
                }
            }
            Some(Err(error)) => out.extend(
                display_lines(error, columns)
                    .map(|s| row(Role::Assistant, assistant, Kind::Body, s)),
            ),
            None => out.push(row(
                Role::Assistant,
                assistant,
                Kind::Meta,
                "Preparando respuesta…".into(),
            )),
        }
        out.push(row(Role::Assistant, assistant, Kind::Gap, String::new()));
    }
    out
}

pub(super) unsafe fn draw(
    dc: Hdc,
    rows: &[Placed],
    dpi: super::super::dpi::Dpi,
) -> Result<(), String> {
    let px = |v| dpi.pixels(v);
    // One rounded surface per visible message fragment; separators belong to the role.
    let mut at = 0;
    while at < rows.len() {
        if rows[at].row.kind == Kind::Gap {
            at += 1;
            continue;
        }
        let begin = at;
        let group = rows[at].row.group;
        while at + 1 < rows.len()
            && rows[at + 1].row.group == group
            && rows[at + 1].row.kind != Kind::Gap
        {
            at += 1;
        }
        let bounds = Rect {
            bottom: rows[at].area.bottom,
            ..rows[begin].area
        };
        let background = if rows[begin].row.role == Role::User {
            shell::SURFACE
        } else {
            shell::BACKGROUND
        };
        composer::rounded(dc, bounds, px(9), background)?;
        let accent = if rows[begin].row.role == Role::User {
            rows[begin].accent
        } else {
            rows[begin].brand
        };
        composer::rounded(
            dc,
            Rect {
                left: bounds.left + px(2),
                right: bounds.left + px(5),
                top: bounds.top + px(5),
                bottom: bounds.bottom - px(5),
            },
            px(2),
            accent,
        )?;
        at += 1;
    }
    for placed in rows {
        let row = &placed.row;
        if row.kind == Kind::Gap {
            continue;
        }
        let mut area = Rect {
            left: placed.area.left + px(13),
            right: placed.area.right - px(10),
            top: placed.area.top + px(2),
            ..placed.area
        };
        if row.kind == Kind::Code {
            paint::fill(
                dc,
                Rect {
                    left: area.left - px(3),
                    ..placed.area
                },
                shell::SIDEBAR,
            )?;
        }
        for span in &row.spans {
            let color = match row.kind {
                Kind::Header => {
                    if row.role == Role::User {
                        placed.accent
                    } else {
                        placed.brand
                    }
                }
                Kind::Meta => shell::MUTED,
                _ if matches!(span.kind, SyntaxKind::Code | SyntaxKind::Heading)
                    || span.style == MarkdownInlineStyle::Strong =>
                {
                    placed.accent
                }
                _ => shell::TEXT,
            };
            let width = composer::text_width(dc, &span.text);
            if span.kind == SyntaxKind::Code && row.kind != Kind::Code {
                composer::rounded(
                    dc,
                    Rect {
                        right: (area.left + width).min(area.right),
                        ..area
                    },
                    px(3),
                    shell::RAISED,
                )?;
            }
            composer::text(dc, area, &span.text, color)?;
            area.left = (area.left + width).min(area.right);
        }
    }
    Ok(())
}

impl State {
    pub(super) fn review_omnicore_chat_code(&mut self, turn: usize, block: usize, new_file: bool) {
        let result = (|| {
            let turn = self
                .omnicore
                .sessions
                .active
                .chat
                .turns
                .get(turn)
                .ok_or("La respuesta ya no está disponible.")?;
            let answer = turn
                .answer
                .as_ref()
                .and_then(|a| a.as_ref().ok())
                .filter(|a| a.complete)
                .ok_or("Espera una propuesta completa antes de aplicarla.")?;
            let blocks = code_blocks(&answer.text);
            let block = blocks
                .get(block)
                .ok_or("El bloque ya no está disponible.")?;
            let code = answer.text[block.range.clone()].to_owned();
            if code.trim().is_empty() || code.len() > 64 * 1024 || code.contains('\0') {
                return Err("El bloque de código no se puede aplicar.".into());
            }
            if new_file || self.workbench.active_editor().is_none() {
                let extension = match block.language.to_ascii_lowercase().as_str() {
                    "rust" | "rs" => "rs",
                    "abl" | "progress" | "openedge" => "p",
                    "csharp" | "cs" | "c#" => "cs",
                    "typescript" | "ts" => "ts",
                    "javascript" | "js" => "js",
                    "java" => "java",
                    "php" => "php",
                    "sql" => "sql",
                    "python" | "py" => "py",
                    _ => "txt",
                };
                self.workbench.new_file(
                    format!("Propuesta-OmniCore.{extension}"),
                    jc_text::TextEncoding::Utf8,
                );
                self.workbench
                    .insert_text(&code)
                    .map_err(|e| e.to_string())?;
                self.status =
                    "Propuesta abierta en un archivo nuevo sin guardar · Ctrl+Z deshacer".into();
                self.blur_omnicore();
                return Ok(());
            }
            let (proposal, preview, path) = selection::Proposal::from_chat(&self.workbench, code)?;
            self.open_omnicore_selection_diff(path, preview, proposal);
            Ok(())
        })();
        if let Err(error) = result {
            self.omnicore.chat_notice = Some(error);
            self.omnicore.invalidate();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn closed_code_only_has_exact_inert_apply_targets() {
        let text = "Ejemplo:\n```rust\nfn hola() {}\n```\nDespués\n```sql\nselect 1;\n";
        let blocks = code_blocks(text);
        assert_eq!(blocks.len(), 1);
        assert_eq!(&text[blocks[0].range.clone()], "fn hola() {}\n");
        assert_eq!(blocks[0].language, "rust");
    }
    #[test]
    fn activity_is_hidden_by_default_and_does_not_change_answer() {
        let mut chat = chat::Chat::default();
        chat.sent("Pregunta".into(), None);
        chat.finish(Ok(Answer {
            text: "**Respuesta** con `código`.".into(),
            complete: true,
            activity: vec!["[herramienta] sentinel".into()],
        }));
        let rendered = rows(&chat, 40, false);
        let text = rendered
            .iter()
            .flat_map(|r| r.spans.iter())
            .map(|s| s.text.as_str())
            .collect::<String>();
        assert!(!text.contains("sentinel"));
        assert!(!text.contains("**"));
        assert!(!text.contains('`'));
        assert!(rows(&chat, 40, true)
            .iter()
            .flat_map(|r| r.spans.iter())
            .any(|s| s.text.contains("sentinel")));
    }
}
