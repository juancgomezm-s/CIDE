//! Small bounded multiline input shared by frontends; offsets are UTF-8 bytes,
//! displayed cells are ASCII spans. CRLF is one editable boundary unit.
use crate::workbench::invisible_layout::escape_character;
use std::ops::Range;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FieldSelection {
    pub cursor: usize,
    pub anchor: Option<usize>,
}
impl FieldSelection {
    /// UTF-8 byte coordinates. Editing/presentation normalize these coordinates
    /// against the current text before slicing; CRLF is one boundary unit.
    pub fn range(&self) -> Option<Range<usize>> {
        let anchor = self.anchor?;
        (anchor != self.cursor).then(|| anchor.min(self.cursor)..anchor.max(self.cursor))
    }
    pub fn select_all(&mut self, text: &str) {
        self.cursor = text.len();
        self.anchor = (!text.is_empty()).then_some(0);
    }
}

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

/// Selection-aware field edits reuse the ordinary field's movement and scalar
/// boundaries. A rejected insertion preserves text AND the caller's intention;
/// no partial deletion, cursor normalization or anchor change is published.
/// The caller retains its existing byte budget and input policy (for example,
/// a selector may reject controls even though a multiline search allows them).
pub fn edit_selected_field(
    text: &mut String,
    state: &mut FieldSelection,
    edit: FieldEdit<'_>,
    selecting: bool,
    limit: usize,
) -> Result<bool, &'static str> {
    let mut next_state = FieldSelection {
        cursor: boundary(text, state.cursor),
        anchor: state.anchor.map(|anchor| boundary(text, anchor)),
    };
    let range = next_state.range();
    let changed = match edit {
        FieldEdit::Insert(value) => {
            let replaced = range.as_ref().map_or(0, |range| range.len());
            let result_len = text.len().saturating_sub(replaced).checked_add(value.len());
            if result_len.is_none_or(|length| length > limit) {
                return Err("Field exceeds its byte budget; insertion rejected");
            }
            if let Some(range) = range {
                let changed = &text[range.clone()] != value;
                let end = range.start + value.len();
                if changed {
                    text.replace_range(range, value);
                }
                next_state.cursor = end;
                // Match ordinary Insert when the new text forms a CRLF across
                // its right boundary; the cursor belongs after the whole pair.
                if !value.is_empty()
                    && end > 0
                    && text.as_bytes().get(end - 1) == Some(&b'\r')
                    && text.as_bytes().get(end) == Some(&b'\n')
                {
                    next_state.cursor += 1;
                }
                next_state.cursor = boundary(text, next_state.cursor);
                changed
            } else {
                edit_field(text, &mut next_state.cursor, edit, limit)?
            }
        }
        FieldEdit::Backspace | FieldEdit::Delete => {
            let changed = if let Some(range) = range {
                next_state.cursor = range.start;
                text.replace_range(range, "");
                true
            } else {
                edit_field(text, &mut next_state.cursor, edit, limit)?
            };
            next_state.cursor = boundary(text, next_state.cursor);
            changed
        }
        FieldEdit::Left
        | FieldEdit::Right
        | FieldEdit::Up
        | FieldEdit::Down
        | FieldEdit::Home
        | FieldEdit::End => {
            let collapse = if !selecting && matches!(edit, FieldEdit::Left | FieldEdit::Right) {
                range
            } else {
                None
            };
            if let Some(range) = collapse {
                next_state.cursor = if matches!(edit, FieldEdit::Left) {
                    range.start
                } else {
                    range.end
                };
                false
            } else {
                if selecting && next_state.anchor.is_none() {
                    next_state.anchor = Some(next_state.cursor);
                }
                edit_field(text, &mut next_state.cursor, edit, limit)?
            }
        }
        _ => {
            let changed = edit_field(text, &mut next_state.cursor, edit, limit)?;
            next_state.cursor = boundary(text, next_state.cursor);
            changed
        }
    };
    if !selecting
        || matches!(
            edit,
            FieldEdit::Insert(_) | FieldEdit::Backspace | FieldEdit::Delete | FieldEdit::Clear
        )
    {
        next_state.anchor = None;
    }
    *state = next_state;
    Ok(changed)
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
pub struct SelectedFieldPresentation {
    pub field: FieldPresentation,
    /// Visible-row ASCII column ranges. Every fragment of a selected source
    /// scalar's escape, including wrapped fragments and CRLF, is highlighted.
    pub selections: Vec<Vec<Range<usize>>>,
}

/// One source/escape/wrap algorithm for both presentations. The first traversal
/// locates the caret/window; the second retains only its <=32 visible rows.
/// Neither traversal owns the full escaped document or per-scalar metadata.
fn walk_field(
    text: &str,
    at: usize,
    width: usize,
    mut cell: impl FnMut(usize, usize, char, Range<usize>),
) -> ((usize, usize), usize) {
    let mut row = 0;
    let mut column = 0;
    let mut caret = (0, 0);
    let mut offset = 0;
    while offset < text.len() {
        if column == width {
            row += 1;
            column = 0;
        }
        if offset == at {
            caret = (row, column);
        }
        let character = text[offset..].chars().next().unwrap();
        let crlf = text[offset..].starts_with("\r\n");
        let end = offset + if crlf { 2 } else { character.len_utf8() };
        let token = if crlf {
            "\\r\\n".into()
        } else {
            escape_character(character, column)
        };
        for character in token.chars() {
            if column == width {
                row += 1;
                column = 0;
            }
            cell(row, column, character, offset..end);
            column += 1;
        }
        offset = end;
        if matches!(character, '\r' | '\n') {
            row += 1;
            column = 0;
        }
    }
    if at >= text.len() {
        if column == width {
            row += 1;
            column = 0;
        }
        caret = (row, column);
    }
    (caret, row + 1)
}

/// The query/replacement limit is4096 UTF8 bytes. All non-ASCII/controls are
/// explicit ASCII so field previews never emit terminal controls or invisible glyphs.
pub fn present_field(text: &str, cursor: usize, width: usize, height: usize) -> FieldPresentation {
    present_selected_field(
        text,
        FieldSelection {
            cursor,
            anchor: None,
        },
        width,
        height,
    )
    .field
}
pub fn present_selected_field(
    text: &str,
    state: FieldSelection,
    width: usize,
    height: usize,
) -> SelectedFieldPresentation {
    let width = width.clamp(1, 4096);
    let height = height.clamp(1, 32);
    let at = boundary(text, state.cursor);
    let range = if text.len() <= 4096 {
        FieldSelection {
            cursor: at,
            anchor: state.anchor.map(|anchor| boundary(text, anchor)),
        }
        .range()
    } else {
        None
    };
    let text = if text.len() <= 4096 {
        text
    } else {
        "[field exceeds4096 bytes]"
    };
    let (caret, total_rows) = walk_field(text, at, width, |_, _, _, _| {});
    let first_row = caret.0.saturating_sub(height - 1);
    let visible = height.min(total_rows - first_row);
    let mut rows = vec![String::new(); visible];
    let mut selections: Vec<Vec<Range<usize>>> = vec![Vec::new(); visible];
    walk_field(text, at, width, |row, column, ch, source| {
        if row < first_row || row >= first_row + visible {
            return;
        }
        let index = row - first_row;
        rows[index].push(ch);
        if range
            .as_ref()
            .is_some_and(|selected| selected.start < source.end && source.start < selected.end)
        {
            if let Some(last) = selections[index]
                .last_mut()
                .filter(|last| last.end == column)
            {
                last.end += 1;
            } else {
                selections[index].push(column..column + 1);
            }
        }
    });
    SelectedFieldPresentation {
        field: FieldPresentation {
            rows,
            cursor_row: caret.0 - first_row,
            cursor_column: caret.1,
            first_row,
            total_rows,
        },
        selections,
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
    #[test]
    fn selected_replacement_preflights_utf8_budget_and_keeps_crlf_atomic() {
        let mut text = String::from("añ\r\nb");
        let mut state = FieldSelection {
            cursor: 1,
            anchor: None,
        };
        edit_selected_field(&mut text, &mut state, FieldEdit::Right, true, 6).unwrap();
        assert_eq!(state.range(), Some(1..3));
        edit_selected_field(&mut text, &mut state, FieldEdit::Right, true, 6).unwrap();
        assert_eq!(state.range(), Some(1..5));
        assert!(
            edit_selected_field(&mut text, &mut state, FieldEdit::Insert("é"), false, 5).unwrap()
        );
        assert_eq!(text, "aéb");
        assert_eq!(
            state,
            FieldSelection {
                cursor: 3,
                anchor: None
            }
        );
        state.select_all(&text);
        let before = state;
        assert!(
            edit_selected_field(&mut text, &mut state, FieldEdit::Insert("😀😀"), false, 4)
                .is_err()
        );
        assert_eq!(text, "aéb");
        assert_eq!(state, before);
        assert!(
            edit_selected_field(&mut text, &mut state, FieldEdit::Insert(""), false, 4).unwrap()
        );
        assert!(text.is_empty());
        assert_eq!(state, FieldSelection::default());

        text = String::from("x\n");
        state = FieldSelection {
            cursor: 1,
            anchor: Some(0),
        };
        edit_selected_field(&mut text, &mut state, FieldEdit::Insert("\r"), false, 2).unwrap();
        assert_eq!(text, "\r\n");
        assert_eq!(state.cursor, 2);
        edit_selected_field(&mut text, &mut state, FieldEdit::Backspace, false, 2).unwrap();
        assert!(text.is_empty());
        assert_eq!(state, FieldSelection::default());
        text = String::from("\rX\n");
        state = FieldSelection {
            cursor: 1,
            anchor: Some(2),
        };
        edit_selected_field(&mut text, &mut state, FieldEdit::Delete, false, 3).unwrap();
        assert_eq!(text, "\r\n");
        assert_eq!(state.cursor, 0);
        // Even a malformed caller selection stays untouched on quota rejection.
        text = String::from("éz");
        state = FieldSelection {
            cursor: 1,
            anchor: Some(3),
        };
        let before = state;
        assert!(
            edit_selected_field(&mut text, &mut state, FieldEdit::Insert("abcd"), false, 3)
                .is_err()
        );
        assert_eq!(text, "éz");
        assert_eq!(state, before);
    }
    #[test]
    fn selection_movement_collapses_or_extends_using_existing_line_boundaries() {
        let mut text = String::from("ab\r\néz");
        let mut state = FieldSelection::default();
        state.select_all(&text);
        edit_selected_field(&mut text, &mut state, FieldEdit::Left, false, 32).unwrap();
        assert_eq!(state, FieldSelection::default());
        edit_selected_field(&mut text, &mut state, FieldEdit::Right, true, 32).unwrap();
        edit_selected_field(&mut text, &mut state, FieldEdit::End, true, 32).unwrap();
        assert_eq!(state.range(), Some(0..2));
        edit_selected_field(&mut text, &mut state, FieldEdit::Down, true, 32).unwrap();
        assert_eq!(state.range(), Some(0..7));
        edit_selected_field(&mut text, &mut state, FieldEdit::Left, false, 32).unwrap();
        assert_eq!(state.cursor, 0);
        assert!(state.anchor.is_none());
        state.anchor = Some(text.len());
        edit_selected_field(&mut text, &mut state, FieldEdit::Right, false, 32).unwrap();
        assert_eq!(state.cursor, 7);
        assert!(state.anchor.is_none());
        edit_selected_field(&mut text, &mut state, FieldEdit::Home, true, 32).unwrap();
        assert_eq!(state.range(), Some(4..7));
        edit_selected_field(&mut text, &mut state, FieldEdit::Right, true, 32).unwrap();
        assert_eq!(state.range(), Some(6..7));
        edit_selected_field(&mut text, &mut state, FieldEdit::Backspace, true, 32).unwrap();
        assert_eq!(text, "ab\r\né");
        assert_eq!(
            state,
            FieldSelection {
                cursor: 6,
                anchor: None
            }
        );
        edit_selected_field(&mut text, &mut state, FieldEdit::Clear, true, 32).unwrap();
        assert!(text.is_empty());
        assert_eq!(state, FieldSelection::default());
        state.select_all(&text);
        assert!(state.range().is_none());
    }
    #[test]
    fn selected_preview_highlights_complete_escape_fragments_and_retains_only_visible_rows() {
        let text = "Aé\r\nB";
        let state = FieldSelection {
            cursor: 5,
            anchor: Some(1),
        };
        let view = present_selected_field(text, state, 3, 32);
        assert_eq!(view.field.rows, ["A\\u", "{E9", "}\\r", "\\n", "B"]);
        assert_eq!(
            view.selections,
            vec![vec![1..3], vec![0..3], vec![0..3], vec![0..2], vec![]]
        );
        assert_eq!(
            (
                view.field.cursor_row,
                view.field.cursor_column,
                view.field.total_rows
            ),
            (4, 0, 5)
        );
        let clipped = present_selected_field(text, state, 3, 2);
        assert_eq!(clipped.field.rows, ["\\n", "B"]);
        assert_eq!(clipped.selections, vec![vec![0..2], vec![]]);
        assert_eq!((clipped.field.first_row, clipped.field.cursor_row), (3, 1));
        let again = present_selected_field(text, state, 3, 2);
        assert_eq!(again.field.rows, clipped.field.rows);
        assert_eq!(again.selections, clipped.selections);
        assert_eq!(text, "Aé\r\nB");
        assert_eq!(
            state,
            FieldSelection {
                cursor: 5,
                anchor: Some(1)
            }
        );

        let worst = "\0".repeat(4096);
        let mut all = FieldSelection::default();
        all.select_all(&worst);
        let tiny = present_selected_field(&worst, all, 1, usize::MAX);
        assert_eq!(tiny.field.rows.len(), 32);
        assert_eq!(tiny.selections.len(), 32);
        assert_eq!(tiny.field.total_rows, 16385);
        assert_eq!((tiny.field.cursor_row, tiny.field.cursor_column), (31, 0));
        assert!(tiny
            .field
            .rows
            .iter()
            .all(|row| row.len() <= 1 && row.chars().all(|ch| ch.is_ascii() && !ch.is_control())));
        assert!(tiny.field.rows.last().unwrap().is_empty());
        assert!(tiny.selections.last().unwrap().is_empty());
        let oversized = "x".repeat(4097);
        all.select_all(&oversized);
        let error = present_selected_field(&oversized, all, 80, 32);
        assert_eq!(error.field.rows, ["[field\\sexceeds4096\\sbytes]"]);
        assert!(error.selections.iter().all(Vec::is_empty));
    }
}
