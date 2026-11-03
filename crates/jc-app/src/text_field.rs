//! Small bounded multiline input shared by frontends; offsets are UTF-8 bytes,
//! displayed cells are ASCII spans. CRLF is one editable boundary unit.
use crate::workbench::invisible_layout::escape_character;

#[derive(Clone, Copy)]
pub enum FieldEdit<'a> {
    Insert(&'a str),
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    Backspace,
    Delete,
    Clear,
}

fn boundary(text: &str, mut at: usize) -> usize {
    at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    if at > 0
        && text.as_bytes().get(at - 1) == Some(&b'\r')
        && text.as_bytes().get(at) == Some(&b'\n')
    {
        at -= 1;
    }
    at
}
fn previous(text: &str, at: usize) -> usize {
    if at >= 2 && &text.as_bytes()[at - 2..at] == b"\r\n" {
        at - 2
    } else {
        text[..at].char_indices().next_back().map_or(0, |(i, _)| i)
    }
}
fn next(text: &str, at: usize) -> usize {
    if text[at..].starts_with("\r\n") {
        at + 2
    } else {
        at + text[at..].chars().next().map_or(0, char::len_utf8)
    }
}
fn line_start(text: &str, at: usize) -> usize {
    text[..at].rfind(['\r', '\n']).map_or(0, |i| i + 1)
}
fn line_end(text: &str, at: usize) -> usize {
    text[at..].find(['\r', '\n']).map_or(text.len(), |i| at + i)
}
/// Whole insertion is rejected on overflow; no truncation/normalization. Returns
/// whether the text changed (movement still updates cursor).
pub fn edit_field(
    text: &mut String,
    cursor: &mut usize,
    edit: FieldEdit<'_>,
    limit: usize,
) -> Result<bool, &'static str> {
    let at = boundary(text, *cursor);
    *cursor = at;
    match edit {
        FieldEdit::Clear => {
            let changed = !text.is_empty();
            text.clear();
            *cursor = 0;
            Ok(changed)
        }
        FieldEdit::Insert(value) => {
            if value.len() > limit.saturating_sub(text.len()) {
                return Err("Field exceeds its byte budget; insertion rejected");
            }
            if value.is_empty() {
                return Ok(false);
            }
            text.insert_str(at, value);
            *cursor = at + value.len();
            // Inserting CR before an existing LF must not leave an interior caret.
            if text.as_bytes().get(*cursor - 1) == Some(&b'\r')
                && text.as_bytes().get(*cursor) == Some(&b'\n')
            {
                *cursor += 1;
            }
            Ok(true)
        }
        FieldEdit::Left => {
            *cursor = previous(text, at);
            Ok(false)
        }
        FieldEdit::Right => {
            *cursor = next(text, at);
            Ok(false)
        }
        FieldEdit::Home => {
            *cursor = line_start(text, at);
            Ok(false)
        }
        FieldEdit::End => {
            *cursor = line_end(text, at);
            Ok(false)
        }
        FieldEdit::Up | FieldEdit::Down => {
            let start = line_start(text, at);
            let column = text[start..at].chars().count();
            let target = match edit {
                FieldEdit::Up if start > 0 => line_start(text, previous(text, start)),
                FieldEdit::Down if line_end(text, at) < text.len() => {
                    next(text, line_end(text, at))
                }
                _ => return Ok(false),
            };
            let end = line_end(text, target);
            *cursor = text[target..end]
                .char_indices()
                .nth(column)
                .map_or(end, |(i, _)| target + i);
            Ok(false)
        }
        FieldEdit::Backspace if at > 0 => {
            let from = previous(text, at);
            text.replace_range(from..at, "");
            *cursor = from;
            Ok(true)
        }
        FieldEdit::Delete if at < text.len() => {
            let end = next(text, at);
            text.replace_range(at..end, "");
            Ok(true)
        }
        _ => Ok(false),
    }
}

pub struct FieldPresentation {
    pub rows: Vec<String>,
    pub cursor_row: usize,
    pub cursor_column: usize,
    pub first_row: usize,
    pub total_rows: usize,
}
/// The query/replacement limit is4096 UTF8 bytes. All non-ASCII/controls are
/// explicit ASCII so field previews never emit terminal controls or invisible glyphs.
pub fn present_field(text: &str, cursor: usize, width: usize, height: usize) -> FieldPresentation {
    let width = width.clamp(1, 4096);
    let height = height.clamp(1, 32);
    let at = boundary(text, cursor);
    let mut rows = vec![String::new()];
    let mut caret = (0, 0);
    let mut offset = 0;
    let text = if text.len() <= 4096 {
        text
    } else {
        "[field exceeds4096 bytes]"
    };
    while offset < text.len() {
        if rows.last().unwrap().len() == width {
            rows.push(String::new());
        }
        if offset == at {
            caret = (rows.len() - 1, rows.last().unwrap().len());
        }
        let ch = text[offset..].chars().next().unwrap();
        let crlf = text[offset..].starts_with("\r\n");
        let token = if crlf {
            "\\r\\n".into()
        } else {
            escape_character(ch, rows.last().unwrap().len())
        };
        for ch in token.chars() {
            if rows.last().unwrap().len() == width {
                rows.push(String::new());
            }
            rows.last_mut().unwrap().push(ch);
        }
        offset += if crlf { 2 } else { ch.len_utf8() };
        if matches!(ch, '\r' | '\n') {
            rows.push(String::new());
        }
    }
    if at >= text.len() {
        if rows.last().unwrap().len() == width {
            rows.push(String::new());
        }
        caret = (rows.len() - 1, rows.last().unwrap().len());
    }
    let total_rows = rows.len();
    let first_row = caret.0.saturating_sub(height - 1);
    FieldPresentation {
        rows: rows.into_iter().skip(first_row).take(height).collect(),
        cursor_row: caret.0 - first_row,
        cursor_column: caret.1,
        first_row,
        total_rows,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn multiline_editing_preserves_eol_and_rejects_whole_overflow() {
        let mut text = String::from("é\r\nb");
        let mut cursor = 2;
        edit_field(&mut text, &mut cursor, FieldEdit::Right, 4096).unwrap();
        assert_eq!(cursor, 4);
        edit_field(&mut text, &mut cursor, FieldEdit::Backspace, 4096).unwrap();
        assert_eq!(text, "éb");
        edit_field(&mut text, &mut cursor, FieldEdit::Insert("\r\nx\n"), 4096).unwrap();
        assert_eq!(text, "é\r\nx\nb");
        let before = text.clone();
        assert!(edit_field(
            &mut text,
            &mut cursor,
            FieldEdit::Insert(&"q".repeat(4096)),
            4096
        )
        .is_err());
        assert_eq!(text, before);
    }
    #[test]
    fn field_preview_is_bounded_navigable_and_emits_no_controls() {
        let text = "a\r\n\u{200b}\0é";
        let view = present_field(text, text.len(), 8, 2);
        assert!(view.rows.len() <= 2);
        assert!(view.total_rows > 2);
        assert!(view
            .rows
            .iter()
            .all(|r| r.len() <= 8 && r.chars().all(|ch| ch.is_ascii() && !ch.is_control())));
        assert!(view.cursor_row < 2);
        assert!(view.cursor_column < 8);
    }
}
