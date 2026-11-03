use super::{Workbench, WorkbenchError};
use jc_files::FileError;
use jc_languages::detect;
use jc_text::{encode, PositionError, TextPosition};
use std::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq)]
struct LineSlice {
    content: Range<usize>,
    separator: Range<usize>,
}

struct SourceWindow {
    start: usize,
    bytes: Vec<u8>,
}
impl std::ops::Index<Range<usize>> for SourceWindow {
    type Output = [u8];
    fn index(&self, range: Range<usize>) -> &Self::Output {
        &self.bytes[range.start - self.start..range.end - self.start]
    }
}
impl SourceWindow {
    fn get(&self, offset: usize) -> Option<&u8> {
        self.bytes.get(offset.checked_sub(self.start)?)
    }
}

impl Workbench {
    fn read_window(&self, range: Range<usize>) -> Result<SourceWindow, WorkbenchError> {
        let source = self
            .active_document()
            .ok_or(WorkbenchError::NoDocument)?
            .session
            .document();
        let bytes = source.read_range(range.clone()).map_err(FileError::from)?;
        Ok(SourceWindow {
            start: range.start,
            bytes,
        })
    }
    fn preferred_ending(&self) -> Result<&'static [u8], WorkbenchError> {
        self.with_active_text(|text, _| text.preferred_ending())
    }

    pub fn insert_text(&mut self, text: &str) -> Result<(), WorkbenchError> {
        if self.active_rectangle().is_some() {
            return self.insert_rectangle_text(text);
        }
        let mut document = self.active_editor_mut().ok_or(WorkbenchError::NoDocument)?;
        let encoding = document
            .session
            .encoding()
            .ok_or(FileError::EncodingRequired)?;
        let bytes = encode(text, encoding).map_err(FileError::from)?;
        if let Some(selection) = document.selection() {
            document
                .session
                .document_mut()
                .replace(selection.clone(), &bytes)
                .map_err(FileError::from)?;
            document.cursor = selection.start + bytes.len();
            document.anchor = None;
        } else {
            let cursor = document.cursor;
            document
                .session
                .document_mut()
                .insert(cursor, &bytes)
                .map_err(FileError::from)?;
            document.cursor += bytes.len();
        }
        self.after_edit()
    }

    pub fn insert_newline(&mut self) -> Result<(), WorkbenchError> {
        self.require_linear_selection()?;
        let text = std::str::from_utf8(self.preferred_ending()?).expect("ASCII line ending");
        self.insert_text(text)
    }

    pub fn insert_tab(&mut self) -> Result<(), WorkbenchError> {
        self.insert_text("    ")
    }

    pub fn backspace(&mut self) -> Result<(), WorkbenchError> {
        if self.active_rectangle().is_some() {
            return self.delete_rectangle(true);
        }
        if self.delete_selection()? {
            return Ok(());
        }
        let Some(previous) = self.previous_boundary()? else {
            return Ok(());
        };
        let mut document = self.active_editor_mut().ok_or(WorkbenchError::NoDocument)?;
        let cursor = document.cursor;
        document
            .session
            .document_mut()
            .delete(previous..cursor)
            .map_err(FileError::from)?;
        document.cursor = previous;
        self.after_edit()
    }

    pub fn delete_forward(&mut self) -> Result<(), WorkbenchError> {
        if self.active_rectangle().is_some() {
            return self.delete_rectangle(false);
        }
        if self.delete_selection()? {
            return Ok(());
        }
        let Some(next) = self.next_boundary()? else {
            return Ok(());
        };
        let document = self.active_editor_mut().ok_or(WorkbenchError::NoDocument)?;
        let cursor = document.cursor;
        document
            .session
            .document_mut()
            .delete(cursor..next)
            .map_err(FileError::from)?;
        self.after_edit()
    }

    pub(super) fn delete_selection(&mut self) -> Result<bool, WorkbenchError> {
        let selection = self.active_editor().and_then(|editor| editor.selection());
        let Some(selection) = selection else {
            return Ok(false);
        };
        let mut document = self.active_editor_mut().ok_or(WorkbenchError::NoDocument)?;
        document
            .session
            .document_mut()
            .delete(selection.clone())
            .map_err(FileError::from)?;
        document.cursor = selection.start;
        document.anchor = None;
        self.after_edit()?;
        Ok(true)
    }

    pub(super) fn after_edit(&mut self) -> Result<(), WorkbenchError> {
        self.rebase_after_edit();
        // Undo/redo can shorten a range before the caret. Length clamping alone
        // could leave it inside a multibyte scalar (or before the UTF-8 BOM).
        if let Some(document) = self.active_document() {
            self.normalize_views(document.tab_id);
        }
        self.sync_active_tab_dirty();
        let _ = self.rebuild_search();
        Ok(())
    }

    fn previous_boundary(&self) -> Result<Option<usize>, WorkbenchError> {
        let cursor = self
            .active_editor()
            .ok_or(WorkbenchError::NoDocument)?
            .cursor;
        Ok(self.with_active_text(|text, source| text.adjacent(source, cursor, false))??)
    }
    fn next_boundary(&self) -> Result<Option<usize>, WorkbenchError> {
        let cursor = self
            .active_editor()
            .ok_or(WorkbenchError::NoDocument)?
            .cursor;
        Ok(self.with_active_text(|text, source| text.adjacent(source, cursor, true))??)
    }

    pub fn move_left(&mut self, select: bool) -> Result<(), WorkbenchError> {
        if let Some(offset) = self.previous_boundary()? {
            self.move_cursor_to(offset, select)?;
        }
        Ok(())
    }

    pub fn move_right(&mut self, select: bool) -> Result<(), WorkbenchError> {
        if let Some(offset) = self.next_boundary()? {
            self.move_cursor_to(offset, select)?;
        }
        Ok(())
    }

    pub fn move_home(&mut self, select: bool) -> Result<(), WorkbenchError> {
        let position = self.active_position().ok_or(WorkbenchError::NoDocument)?;
        self.move_to_position(
            TextPosition {
                line: position.line,
                column: 0,
            },
            select,
        )
    }

    pub fn move_end(&mut self, select: bool) -> Result<(), WorkbenchError> {
        let pos = self.active_position().ok_or(WorkbenchError::NoDocument)?;
        let column = self
            .with_active_text(|text, source| text.line_len(source, pos.line))?
            .ok_or(PositionError::LineOutOfRange(pos.line))?;
        self.move_to_position(
            TextPosition {
                line: pos.line,
                column,
            },
            select,
        )
    }
    pub fn move_vertical(&mut self, delta: isize, select: bool) -> Result<(), WorkbenchError> {
        let current = self.active_position().ok_or(WorkbenchError::NoDocument)?;
        let offset = self.with_active_text(|text, source| {
            let line = if delta < 0 {
                current.line.saturating_sub(delta.unsigned_abs())
            } else {
                current
                    .line
                    .saturating_add(delta as usize)
                    .min(text.line_count() - 1)
            };
            let column = current.column.min(text.line_len(source, line).unwrap_or(0));
            text.offset(source, TextPosition { line, column })
        })??;
        self.move_cursor_to(offset, select)
    }
    fn move_to_position(
        &mut self,
        position: TextPosition,
        select: bool,
    ) -> Result<(), WorkbenchError> {
        let offset = self.with_active_text(|text, source| text.offset(source, position))??;
        self.move_cursor_to(offset, select)
    }

    /// Ordinary caret/selection placement in scalar coordinates, without a
    /// navigation jump. Frontends convert their own layout coordinates first.
    pub fn set_caret_position(
        &mut self,
        position: TextPosition,
        select: bool,
    ) -> Result<(), WorkbenchError> {
        self.move_to_position(position, select)
    }

    fn move_cursor_to(&mut self, offset: usize, select: bool) -> Result<(), WorkbenchError> {
        let mut document = self.active_editor_mut().ok_or(WorkbenchError::NoDocument)?;
        if document.rectangle.take().is_some() {
            document.anchor = None;
        }
        if select {
            if document.anchor.is_none() {
                document.anchor = Some(document.cursor);
            }
        } else {
            document.anchor = None;
        }
        document.cursor = offset;
        Ok(())
    }

    pub fn duplicate_selection_or_line(&mut self) -> Result<(), WorkbenchError> {
        self.require_linear_selection()?;
        if let Some(selection) = self.active_editor().and_then(|editor| editor.selection()) {
            let bytes = self
                .active_editor()
                .ok_or(WorkbenchError::NoDocument)?
                .session
                .document()
                .read_range(selection.clone())
                .map_err(FileError::from)?;
            let insert_at = selection.end;
            let mut document = self.active_editor_mut().ok_or(WorkbenchError::NoDocument)?;
            document
                .session
                .document_mut()
                .insert(insert_at, &bytes)
                .map_err(FileError::from)?;
            document.anchor = Some(insert_at);
            document.cursor = insert_at + bytes.len();
            return self.after_edit();
        }

        let position = self.active_position().ok_or(WorkbenchError::NoDocument)?;
        let line = self.line_slice(position.line)?;
        let source = self.read_window(line.content.start..line.separator.end)?;
        let mut duplicate = Vec::new();
        if line.separator.is_empty() {
            let ending = self.preferred_ending()?;
            duplicate.extend_from_slice(ending);
            duplicate.extend_from_slice(&source[line.content.clone()]);
        } else {
            duplicate.extend_from_slice(&source[line.content.start..line.separator.end]);
        }
        let insert_at = line.separator.end;
        self.active_editor_mut()
            .ok_or(WorkbenchError::NoDocument)?
            .session
            .document_mut()
            .insert(insert_at, &duplicate)
            .map_err(FileError::from)?;
        self.after_edit()?;
        self.move_to_position(
            TextPosition {
                line: position.line + 1,
                column: position.column,
            },
            false,
        )
    }

    pub fn delete_line(&mut self) -> Result<(), WorkbenchError> {
        self.require_linear_selection()?;
        let position = self.active_position().ok_or(WorkbenchError::NoDocument)?;
        let line = self.line_slice(position.line)?;
        let (range, cursor) = if line.separator.is_empty() && position.line > 0 {
            let previous = self.line_slice(position.line - 1)?;
            (
                previous.separator.start..line.content.end,
                previous.content.end,
            )
        } else {
            (line.content.start..line.separator.end, line.content.start)
        };
        let mut document = self.active_editor_mut().ok_or(WorkbenchError::NoDocument)?;
        document
            .session
            .document_mut()
            .delete(range)
            .map_err(FileError::from)?;
        document.cursor = cursor.min(document.session.document().len());
        document.anchor = None;
        self.after_edit()
    }

    pub fn move_line_up(&mut self) -> Result<(), WorkbenchError> {
        self.require_linear_selection()?;
        let position = self.active_position().ok_or(WorkbenchError::NoDocument)?;
        if position.line == 0 {
            return Ok(());
        }
        self.swap_adjacent_lines(position.line - 1, position.line)?;
        self.move_to_position(
            TextPosition {
                line: position.line - 1,
                column: position.column,
            },
            false,
        )
    }

    pub fn move_line_down(&mut self) -> Result<(), WorkbenchError> {
        self.require_linear_selection()?;
        let position = self.active_position().ok_or(WorkbenchError::NoDocument)?;
        let tab = self
            .active_document()
            .ok_or(WorkbenchError::NoDocument)?
            .tab_id;
        let line_count = self
            .line_count_for_tab(tab)
            .ok_or(WorkbenchError::NoDocument)?;
        if position.line + 1 >= line_count {
            return Ok(());
        }
        self.swap_adjacent_lines(position.line, position.line + 1)?;
        self.move_to_position(
            TextPosition {
                line: position.line + 1,
                column: position.column,
            },
            false,
        )
    }

    pub fn toggle_line_comment(&mut self) -> Result<(), WorkbenchError> {
        self.require_linear_selection()?;
        let path = self.active_path().ok_or(WorkbenchError::NoDocument)?;
        let language = detect(&path.to_string_lossy());
        if language.line_comment.is_none() && language.block_comment.is_some() {
            return self.toggle_block_comment();
        }
        let Some(comment) = language.line_comment else {
            return Ok(());
        };
        if let Some(selection) = self.active_editor().and_then(|editor| editor.selection()) {
            return self.toggle_selected_line_comments(comment, selection);
        }
        let position = self.active_position().ok_or(WorkbenchError::NoDocument)?;
        let line = self.line_slice(position.line)?;
        let document = self.active_editor().ok_or(WorkbenchError::NoDocument)?;
        let encoding = document
            .session
            .encoding()
            .ok_or(FileError::EncodingRequired)?;
        let prefix = encode(comment, encoding).map_err(FileError::from)?;
        let source = self.read_window(line.content.start..line.separator.end)?;
        let content = &source[line.content.clone()];
        let indent_relative = content
            .iter()
            .position(|byte| !matches!(*byte, b' ' | b'\t'))
            .unwrap_or(content.len());
        let indent = line.content.start + indent_relative;
        let has_comment = source[indent..line.content.end].starts_with(&prefix);

        let mut document = self.active_editor_mut().ok_or(WorkbenchError::NoDocument)?;
        if has_comment {
            let mut end = indent + prefix.len();
            if source.get(end) == Some(&b' ') && end < line.content.end {
                end += 1;
            }
            document
                .session
                .document_mut()
                .delete(indent..end)
                .map_err(FileError::from)?;
            if document.cursor > indent {
                document.cursor = document
                    .cursor
                    .saturating_sub((end - indent).min(document.cursor - indent));
            }
        } else {
            let mut insertion = prefix;
            insertion.push(b' ');
            document
                .session
                .document_mut()
                .insert(indent, &insertion)
                .map_err(FileError::from)?;
            if document.cursor >= indent {
                document.cursor += insertion.len();
            }
        }
        document.anchor = None;
        self.after_edit()
    }

    /// Toggle block delimiters without converting source encoding or line endings.
    /// ABL uses /* ... */; HTML/XML/Markdown use <!-- ... -->.
    /// A selection is wrapped as one block; without a selection the current
    /// line's non-indent content is toggled.
    fn toggle_selected_line_comments(
        &mut self,
        comment: &str,
        selection: Range<usize>,
    ) -> Result<(), WorkbenchError> {
        let doc = self.active_editor().ok_or(WorkbenchError::NoDocument)?;
        let encoding = doc.session.encoding().ok_or(FileError::EncodingRequired)?;
        let prefix = encode(comment, encoding).map_err(FileError::from)?;
        let content_start = self.with_active_text(|text, _| text.content_start())?;
        let first = self.with_active_text(|text, source| {
            text.position(source, selection.start.max(content_start))
        })??;
        let last = self.with_active_text(|text, source| text.position(source, selection.end))??;
        let last_line = if last.column == 0 && last.line > first.line {
            last.line - 1
        } else {
            last.line
        };
        let slices = (first.line..=last_line)
            .map(|line| self.line_slice(line))
            .collect::<Result<Vec<_>, _>>()?;
        let source =
            self.read_window(slices[0].content.start..slices.last().unwrap().separator.end)?;
        let all_commented = slices.iter().all(|slice| {
            let body = &source[slice.content.clone()];
            let indent = body
                .iter()
                .position(|byte| !matches!(*byte, b' ' | b'\t'))
                .unwrap_or(body.len());
            body[indent..].starts_with(&prefix)
        });
        let start = slices[0].content.start;
        let end = slices.last().map_or(start, |slice| slice.separator.end);
        let mut replacement = Vec::new();
        for slice in slices {
            let body = &source[slice.content.clone()];
            let indent = body
                .iter()
                .position(|byte| !matches!(*byte, b' ' | b'\t'))
                .unwrap_or(body.len());
            replacement.extend_from_slice(&body[..indent]);
            if all_commented {
                let mut consumed = prefix.len();
                if body.get(indent + consumed) == Some(&b' ') {
                    consumed += 1;
                }
                replacement.extend_from_slice(&body[indent + consumed..]);
            } else {
                replacement.extend_from_slice(&prefix);
                replacement.push(b' ');
                replacement.extend_from_slice(&body[indent..]);
            }
            replacement.extend_from_slice(&source[slice.separator]);
        }
        let size = replacement.len();
        let mut doc = self.active_editor_mut().ok_or(WorkbenchError::NoDocument)?;
        doc.session
            .document_mut()
            .replace(start..end, &replacement)
            .map_err(FileError::from)?;
        doc.anchor = Some(start);
        doc.cursor = start + size;
        self.after_edit()
    }

    pub fn toggle_block_comment(&mut self) -> Result<(), WorkbenchError> {
        self.require_linear_selection()?;
        let path = self.active_path().ok_or(WorkbenchError::NoDocument)?;
        let language = detect(&path.to_string_lossy());
        let Some(markers) = language.block_comment else {
            return Ok(());
        };
        let start_marker = markers.start.as_bytes();
        let end_marker = markers.end.as_bytes();
        let selection = self.active_editor().and_then(|editor| editor.selection());
        let original_cursor = self
            .active_editor()
            .ok_or(WorkbenchError::NoDocument)?
            .cursor;
        let range = if let Some(mut range) = selection.clone() {
            // Preserve a UTF-8 BOM at the start of the file, even for Select All.
            let content_start = self.with_active_text(|text, _| text.content_start())?;
            range.start = range.start.max(content_start);
            range
        } else {
            let position = self.active_position().ok_or(WorkbenchError::NoDocument)?;
            let line = self.line_slice(position.line)?;
            let source = self.read_window(line.content.start..line.separator.end)?;
            let relative = source[line.content.clone()]
                .iter()
                .position(|byte| !matches!(*byte, b' ' | b'\t'))
                .unwrap_or(line.content.len());
            (line.content.start + relative)..line.content.end
        };
        let source = self.read_window(range.clone())?;
        let bytes = &source[range.clone()];
        let start = bytes
            .iter()
            .position(|byte| !matches!(*byte, b' ' | b'\t' | b'\r' | b'\n'))
            .unwrap_or(bytes.len());
        let end = bytes
            .iter()
            .rposition(|byte| !matches!(*byte, b' ' | b'\t' | b'\r' | b'\n'))
            .map_or(start, |offset| offset + 1);
        let middle = &bytes[start..end];
        let is_wrapped = middle.starts_with(start_marker)
            && middle.ends_with(end_marker)
            && middle.len() >= start_marker.len() + end_marker.len();

        let mut replacement =
            Vec::with_capacity(bytes.len() + start_marker.len() + end_marker.len() + 2);
        let cursor_prefix_delta: isize;
        if is_wrapped {
            replacement.extend_from_slice(&bytes[..start]);
            let mut inside = &middle[start_marker.len()..middle.len() - end_marker.len()];
            if selection.is_none() {
                if inside.starts_with(b" ") {
                    inside = &inside[1..];
                }
                if inside.ends_with(b" ") {
                    inside = &inside[..inside.len() - 1];
                }
            }
            replacement.extend_from_slice(inside);
            replacement.extend_from_slice(&bytes[end..]);
            cursor_prefix_delta =
                -((start_marker.len() + usize::from(selection.is_none())) as isize);
        } else {
            replacement.extend_from_slice(&bytes[..start]);
            replacement.extend_from_slice(start_marker);
            if selection.is_none() {
                replacement.push(b' ');
            }
            replacement.extend_from_slice(middle);
            if selection.is_none() {
                replacement.push(b' ');
            }
            replacement.extend_from_slice(end_marker);
            replacement.extend_from_slice(&bytes[end..]);
            cursor_prefix_delta = (start_marker.len() + usize::from(selection.is_none())) as isize;
        }
        let new_length = replacement.len();
        let mut document = self.active_editor_mut().ok_or(WorkbenchError::NoDocument)?;
        document
            .session
            .document_mut()
            .replace(range.clone(), &replacement)
            .map_err(FileError::from)?;
        if let Some(previous) = selection {
            // Keep the block selected, ready for a second toggle.
            let reversed = document.anchor == Some(previous.end);
            if reversed {
                document.anchor = Some(range.start + new_length);
                document.cursor = range.start;
            } else {
                document.anchor = Some(range.start);
                document.cursor = range.start + new_length;
            }
        } else {
            let original_relative = original_cursor.saturating_sub(range.start);
            let shifted = if cursor_prefix_delta >= 0 {
                original_relative.saturating_add(cursor_prefix_delta as usize)
            } else {
                original_relative.saturating_sub((-cursor_prefix_delta) as usize)
            };
            document.cursor = range.start + shifted.min(new_length);
            document.anchor = None;
        }
        self.after_edit()
    }

    pub fn indent_lines(&mut self) -> Result<(), WorkbenchError> {
        if self.active_rectangle().is_some() {
            return self.insert_rectangle_text("    ");
        }
        self.adjust_indentation(true)
    }

    pub fn outdent_lines(&mut self) -> Result<(), WorkbenchError> {
        self.require_linear_selection()?;
        self.adjust_indentation(false)
    }

    fn adjust_indentation(&mut self, indent: bool) -> Result<(), WorkbenchError> {
        let selected = self.active_editor().and_then(|editor| editor.selection());
        if indent && selected.is_none() {
            return self.insert_tab();
        }
        let cursor = self
            .active_editor()
            .ok_or(WorkbenchError::NoDocument)?
            .cursor;
        let content_start = self.with_active_text(|text, _| text.content_start())?;
        let (first_line, last_line) = if let Some(range) = &selected {
            let first = self.with_active_text(|text, source| {
                text.position(source, range.start.max(content_start))
            })??;
            let last = self.with_active_text(|text, source| text.position(source, range.end))??;
            (
                first.line,
                if last.column == 0 && last.line > first.line {
                    last.line - 1
                } else {
                    last.line
                },
            )
        } else {
            let pos = self.active_position().ok_or(WorkbenchError::NoDocument)?;
            (pos.line, pos.line)
        };
        let slices = (first_line..=last_line)
            .map(|line| self.line_slice(line))
            .collect::<Result<Vec<_>, _>>()?;
        let source =
            self.read_window(slices[0].content.start..slices.last().unwrap().separator.end)?;
        let begin = slices[0].content.start;
        let end = slices.last().map_or(begin, |slice| slice.separator.end);
        let mut replacement = Vec::new();
        let mut first_delta = 0isize;
        for (line_idx, slice) in slices.iter().enumerate() {
            let body = &source[slice.content.clone()];
            let consumed = if indent {
                replacement.extend_from_slice(b"    ");
                0
            } else if body.starts_with(b"\t") {
                1
            } else {
                body.iter()
                    .take(4)
                    .take_while(|byte| **byte == b' ')
                    .count()
            };
            if line_idx == 0 {
                first_delta = if indent { 4 } else { -(consumed as isize) };
            }
            replacement.extend_from_slice(&body[consumed..]);
            replacement.extend_from_slice(&source[slice.separator.clone()]);
        }
        if &source[begin..end] == replacement.as_slice() {
            return Ok(());
        }
        let new_length = replacement.len();
        let mut document = self.active_editor_mut().ok_or(WorkbenchError::NoDocument)?;
        document
            .session
            .document_mut()
            .replace(begin..end, &replacement)
            .map_err(FileError::from)?;
        if selected.is_some() {
            document.anchor = Some(begin);
            document.cursor = begin + new_length;
        } else {
            document.anchor = None;
            document.cursor = if first_delta >= 0 {
                cursor.saturating_add(first_delta as usize)
            } else {
                cursor.saturating_sub((-first_delta) as usize).max(begin)
            }
            .min(document.session.document().len());
        }
        self.after_edit()
    }

    fn swap_adjacent_lines(
        &mut self,
        first_line: usize,
        second_line: usize,
    ) -> Result<(), WorkbenchError> {
        let first = self.line_slice(first_line)?;
        let second = self.line_slice(second_line)?;
        let source = self.read_window(first.content.start..second.separator.end)?;
        let mut replacement = Vec::new();

        if second.separator.is_empty() {
            replacement.extend_from_slice(&source[second.content.clone()]);
            replacement.extend_from_slice(&source[first.separator.clone()]);
            replacement.extend_from_slice(&source[first.content.clone()]);
        } else {
            replacement.extend_from_slice(&source[second.content.clone()]);
            replacement.extend_from_slice(&source[second.separator.clone()]);
            replacement.extend_from_slice(&source[first.content.clone()]);
            replacement.extend_from_slice(&source[first.separator.clone()]);
        }

        let end = second.separator.end.max(second.content.end);
        self.active_editor_mut()
            .ok_or(WorkbenchError::NoDocument)?
            .session
            .document_mut()
            .replace(first.content.start..end, &replacement)
            .map_err(FileError::from)?;
        self.after_edit()
    }

    fn line_slice(&self, line: usize) -> Result<LineSlice, WorkbenchError> {
        let tab = self
            .active_document()
            .ok_or(WorkbenchError::NoDocument)?
            .tab_id;
        let (content, separator) = self
            .line_range_for_tab(tab, line)
            .ok_or(PositionError::LineOutOfRange(line))?;
        Ok(LineSlice { content, separator })
    }

    pub fn select_all(&mut self) -> Result<(), WorkbenchError> {
        self.cancel_rectangle();
        let mut document = self.active_editor_mut().ok_or(WorkbenchError::NoDocument)?;
        document.anchor = Some(0);
        document.cursor = document.session.document().len();
        Ok(())
    }

    pub fn copy_selection(&mut self) -> Result<(), WorkbenchError> {
        if self.active_rectangle().is_some() {
            return self.copy_rectangle();
        }
        if let Some(text) = self.selection_text_bounded(usize::MAX)? {
            self.clipboard = Some(super::rectangular::Clipboard::Text(text));
        }
        Ok(())
    }

    pub fn cut_selection(&mut self) -> Result<(), WorkbenchError> {
        if self.active_rectangle().is_some() {
            return self.cut_rectangle();
        }
        self.copy_selection()?;
        self.delete_selection()?;
        Ok(())
    }

    pub fn paste(&mut self) -> Result<(), WorkbenchError> {
        let Some(clip) = self.clipboard.clone() else {
            return Ok(());
        };
        let text = match clip {
            super::rectangular::Clipboard::Text(text) => text,
            super::rectangular::Clipboard::Rectangle(rows) => {
                return self.paste_rectangle_rows(&rows)
            }
        };
        self.insert_text(&text).map_err(|error| match error {
            WorkbenchError::File(FileError::Encode(_)) => WorkbenchError::ClipboardEncoding,
            other => other,
        })
    }

    pub fn undo(&mut self) -> Result<(), WorkbenchError> {
        self.cancel_rectangle();
        let editor = self.active_editor_mut().ok_or(WorkbenchError::NoDocument)?;
        if editor.session.document_mut().undo() {
            let change = editor
                .session
                .document()
                .last_change()
                .expect("history change");
            editor.state.cursor = change.map_offset(editor.state.cursor);
            editor.state.anchor = editor.state.anchor.map(|n| change.map_offset(n));
        }
        self.after_edit()
    }

    pub fn redo(&mut self) -> Result<(), WorkbenchError> {
        self.cancel_rectangle();
        let editor = self.active_editor_mut().ok_or(WorkbenchError::NoDocument)?;
        if editor.session.document_mut().redo() {
            let change = editor
                .session
                .document()
                .last_change()
                .expect("history change");
            editor.state.cursor = change.map_offset(editor.state.cursor);
            editor.state.anchor = editor.state.anchor.map(|n| change.map_offset(n));
        }
        self.after_edit()
    }
}
