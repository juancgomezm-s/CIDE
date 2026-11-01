use super::{EditorDocument, Workbench, WorkbenchError};
use jc_files::FileError;
use jc_languages::detect;
use jc_text::{decode, encode, PositionError, TextIndex, TextPosition};
use std::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq)]
struct LineSlice {
    content: Range<usize>,
    separator: Range<usize>,
}

impl Workbench {
    pub fn insert_text(&mut self, text: &str) -> Result<(), WorkbenchError> {
        let document = self
            .active_document_mut()
            .ok_or(WorkbenchError::NoDocument)?;
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
            document
                .session
                .document_mut()
                .insert(document.cursor, &bytes)
                .map_err(FileError::from)?;
            document.cursor += bytes.len();
        }
        self.after_edit()
    }

    pub fn insert_newline(&mut self) -> Result<(), WorkbenchError> {
        let ending = self
            .active_document()
            .ok_or(WorkbenchError::NoDocument)?
            .session
            .inspection()
            .line_endings
            .preferred_bytes();
        let text = String::from_utf8(ending.to_vec()).expect("line-ending bytes are valid ASCII");
        self.insert_text(&text)
    }

    pub fn insert_tab(&mut self) -> Result<(), WorkbenchError> {
        self.insert_text("    ")
    }

    pub fn backspace(&mut self) -> Result<(), WorkbenchError> {
        if self.delete_selection()? {
            return Ok(());
        }
        let Some(previous) = self.previous_boundary()? else {
            return Ok(());
        };
        let document = self
            .active_document_mut()
            .ok_or(WorkbenchError::NoDocument)?;
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
        if self.delete_selection()? {
            return Ok(());
        }
        let Some(next) = self.next_boundary()? else {
            return Ok(());
        };
        let document = self
            .active_document_mut()
            .ok_or(WorkbenchError::NoDocument)?;
        let cursor = document.cursor;
        document
            .session
            .document_mut()
            .delete(cursor..next)
            .map_err(FileError::from)?;
        self.after_edit()
    }

    fn delete_selection(&mut self) -> Result<bool, WorkbenchError> {
        let selection = self.active_document().and_then(EditorDocument::selection);
        let Some(selection) = selection else {
            return Ok(false);
        };
        let document = self
            .active_document_mut()
            .ok_or(WorkbenchError::NoDocument)?;
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

    fn after_edit(&mut self) -> Result<(), WorkbenchError> {
        self.sync_active_tab_dirty();
        self.rebuild_search()?;
        Ok(())
    }

    fn boundaries(&self) -> Result<Vec<usize>, WorkbenchError> {
        let document = self.active_document().ok_or(WorkbenchError::NoDocument)?;
        let encoding = document
            .session
            .encoding()
            .ok_or(FileError::EncodingRequired)?;
        Ok(decode(&document.session.document().to_bytes(), encoding).source_offsets)
    }

    fn previous_boundary(&self) -> Result<Option<usize>, WorkbenchError> {
        let cursor = self
            .active_document()
            .ok_or(WorkbenchError::NoDocument)?
            .cursor;
        Ok(self
            .boundaries()?
            .into_iter()
            .take_while(|offset| *offset < cursor)
            .last())
    }

    fn next_boundary(&self) -> Result<Option<usize>, WorkbenchError> {
        let cursor = self
            .active_document()
            .ok_or(WorkbenchError::NoDocument)?
            .cursor;
        Ok(self
            .boundaries()?
            .into_iter()
            .find(|offset| *offset > cursor))
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
        let document = self.active_document().ok_or(WorkbenchError::NoDocument)?;
        let encoding = document
            .session
            .encoding()
            .ok_or(FileError::EncodingRequired)?;
        let decoded = decode(&document.session.document().to_bytes(), encoding);
        let index = TextIndex::new(&decoded);
        let position = index.source_byte_to_position(document.cursor)?;
        let column = index
            .line_len(position.line)
            .ok_or(PositionError::LineOutOfRange(position.line))?;
        self.move_to_position(
            TextPosition {
                line: position.line,
                column,
            },
            select,
        )
    }

    pub fn move_vertical(&mut self, delta: isize, select: bool) -> Result<(), WorkbenchError> {
        let document = self.active_document().ok_or(WorkbenchError::NoDocument)?;
        let encoding = document
            .session
            .encoding()
            .ok_or(FileError::EncodingRequired)?;
        let decoded = decode(&document.session.document().to_bytes(), encoding);
        let index = TextIndex::new(&decoded);
        let current = index.source_byte_to_position(document.cursor)?;
        let target_line = if delta < 0 {
            current.line.saturating_sub(delta.unsigned_abs())
        } else {
            (current.line + delta as usize).min(index.line_count().saturating_sub(1))
        };
        let target_column = current.column.min(index.line_len(target_line).unwrap_or(0));
        let offset = index.position_to_source_byte(TextPosition {
            line: target_line,
            column: target_column,
        })?;
        self.move_cursor_to(offset, select)
    }

    fn move_to_position(
        &mut self,
        position: TextPosition,
        select: bool,
    ) -> Result<(), WorkbenchError> {
        let document = self.active_document().ok_or(WorkbenchError::NoDocument)?;
        let encoding = document
            .session
            .encoding()
            .ok_or(FileError::EncodingRequired)?;
        let decoded = decode(&document.session.document().to_bytes(), encoding);
        let offset = TextIndex::new(&decoded).position_to_source_byte(position)?;
        self.move_cursor_to(offset, select)
    }

    fn move_cursor_to(&mut self, offset: usize, select: bool) -> Result<(), WorkbenchError> {
        let document = self
            .active_document_mut()
            .ok_or(WorkbenchError::NoDocument)?;
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
        if let Some(selection) = self.active_document().and_then(EditorDocument::selection) {
            let bytes = self
                .active_document()
                .ok_or(WorkbenchError::NoDocument)?
                .session
                .document()
                .to_bytes()[selection.clone()]
                .to_vec();
            let insert_at = selection.end;
            let document = self
                .active_document_mut()
                .ok_or(WorkbenchError::NoDocument)?;
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
        let source = self
            .active_document()
            .ok_or(WorkbenchError::NoDocument)?
            .session
            .document()
            .to_bytes();
        let mut duplicate = Vec::new();
        if line.separator.is_empty() {
            let ending = self
                .active_document()
                .ok_or(WorkbenchError::NoDocument)?
                .session
                .inspection()
                .line_endings
                .preferred_bytes();
            duplicate.extend_from_slice(ending);
            duplicate.extend_from_slice(&source[line.content.clone()]);
        } else {
            duplicate.extend_from_slice(&source[line.content.start..line.separator.end]);
        }
        let insert_at = line.separator.end;
        self.active_document_mut()
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
        let position = self.active_position().ok_or(WorkbenchError::NoDocument)?;
        let line = self.line_slice(position.line)?;
        let (range, cursor) = if line.separator.is_empty() && position.line > 0 {
            let previous = self.line_slice(position.line - 1)?;
            (previous.separator.start..line.content.end, previous.content.end)
        } else {
            (line.content.start..line.separator.end, line.content.start)
        };
        let document = self
            .active_document_mut()
            .ok_or(WorkbenchError::NoDocument)?;
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
        let position = self.active_position().ok_or(WorkbenchError::NoDocument)?;
        let line_count = {
            let document = self.active_document().ok_or(WorkbenchError::NoDocument)?;
            let encoding = document
                .session
                .encoding()
                .ok_or(FileError::EncodingRequired)?;
            let decoded = decode(&document.session.document().to_bytes(), encoding);
            TextIndex::new(&decoded).line_count()
        };
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
        let path = self.active_path().ok_or(WorkbenchError::NoDocument)?;
        let language = detect(&path.to_string_lossy());
        if language.id == "abl" || (language.line_comment.is_none() && block_delimiters(language.id).is_some()) {
            return self.toggle_block_comment();
        }
        let Some(comment) = language.line_comment else {
            return Ok(());
        };
        let position = self.active_position().ok_or(WorkbenchError::NoDocument)?;
        let line = self.line_slice(position.line)?;
        let document = self.active_document().ok_or(WorkbenchError::NoDocument)?;
        let encoding = document
            .session
            .encoding()
            .ok_or(FileError::EncodingRequired)?;
        let prefix = encode(comment, encoding).map_err(FileError::from)?;
        let source = document.session.document().to_bytes();
        let content = &source[line.content.clone()];
        let indent_relative = content
            .iter()
            .position(|byte| !matches!(*byte, b' ' | b'\t'))
            .unwrap_or(content.len());
        let indent = line.content.start + indent_relative;
        let has_comment = source[indent..line.content.end].starts_with(&prefix);

        let document = self
            .active_document_mut()
            .ok_or(WorkbenchError::NoDocument)?;
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
                document.cursor = document.cursor.saturating_sub(
                    (end - indent).min(document.cursor - indent),
                );
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
    pub fn toggle_block_comment(&mut self) -> Result<(), WorkbenchError> {
        let path = self.active_path().ok_or(WorkbenchError::NoDocument)?;
        let language = detect(&path.to_string_lossy());
        let Some((start_marker, end_marker)) = block_delimiters(language.id) else {
            return Ok(());
        };
        let selection = self.active_document().and_then(EditorDocument::selection);
        let original_cursor = self.active_document().ok_or(WorkbenchError::NoDocument)?.cursor;
        let range = if let Some(range) = selection.clone() {
            range
        } else {
            let position = self.active_position().ok_or(WorkbenchError::NoDocument)?;
            let line = self.line_slice(position.line)?;
            let source = self.active_document()
                .ok_or(WorkbenchError::NoDocument)?
                .session.document().to_bytes();
            let relative = source[line.content.clone()].iter()
                .position(|byte| !matches!(*byte, b' ' | b'\t'))
                .unwrap_or(line.content.len());
            (line.content.start + relative)..line.content.end
        };
        let source = self.active_document()
            .ok_or(WorkbenchError::NoDocument)?
            .session.document().to_bytes();
        let bytes = &source[range.clone()];
        let start = bytes.iter().position(|byte| !matches!(*byte, b' ' | b'\t' | b'\r' | b'\n'))
            .unwrap_or(bytes.len());
        let end = bytes.iter().rposition(|byte| !matches!(*byte, b' ' | b'\t' | b'\r' | b'\n'))
            .map_or(start, |offset| offset + 1);
        let middle = &bytes[start..end];
        let is_wrapped = middle.starts_with(start_marker) && middle.ends_with(end_marker)
            && middle.len() >= start_marker.len() + end_marker.len();

        let mut replacement = Vec::with_capacity(bytes.len() + start_marker.len() + end_marker.len() + 2);
        let mut cursor_prefix_delta = 0isize;
        if is_wrapped {
            replacement.extend_from_slice(&bytes[..start]);
            let mut inside = &middle[start_marker.len()..middle.len() - end_marker.len()];
            if selection.is_none() {
                if inside.starts_with(b" ") { inside = &inside[1..]; }
                if inside.ends_with(b" ") { inside = &inside[..inside.len() - 1]; }
            }
            replacement.extend_from_slice(inside);
            replacement.extend_from_slice(&bytes[end..]);
            cursor_prefix_delta = -((start_marker.len() + usize::from(selection.is_none())) as isize);
        } else {
            replacement.extend_from_slice(&bytes[..start]);
            replacement.extend_from_slice(start_marker);
            if selection.is_none() { replacement.push(b' '); }
            replacement.extend_from_slice(middle);
            if selection.is_none() { replacement.push(b' '); }
            replacement.extend_from_slice(end_marker);
            replacement.extend_from_slice(&bytes[end..]);
            cursor_prefix_delta = (start_marker.len() + usize::from(selection.is_none())) as isize;
        }
        let new_length = replacement.len();
        let document = self.active_document_mut().ok_or(WorkbenchError::NoDocument)?;
        document.session.document_mut().replace(range.clone(), &replacement).map_err(FileError::from)?;
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
        self.adjust_indentation(true)
    }

    pub fn outdent_lines(&mut self) -> Result<(), WorkbenchError> {
        self.adjust_indentation(false)
    }

    fn adjust_indentation(&mut self, indent: bool) -> Result<(), WorkbenchError> {
        let selected = self.active_document().and_then(EditorDocument::selection);
        if indent && selected.is_none() {
            return self.insert_tab();
        }
        let document = self.active_document().ok_or(WorkbenchError::NoDocument)?;
        let encoding = document.session.encoding().ok_or(FileError::EncodingRequired)?;
        let decoded = decode(&document.session.document().to_bytes(), encoding);
        let index = TextIndex::new(&decoded);
        let cursor = document.cursor;
        let (first_line, last_line) = if let Some(range) = &selected {
            let first = index.source_byte_to_position(range.start)?;
            let last = index.source_byte_to_position(range.end)?;
            let end = if last.column == 0 && last.line > first.line {
                last.line - 1
            } else {
                last.line
            };
            (first.line, end)
        } else {
            let position = index.source_byte_to_position(cursor)?;
            (position.line, position.line)
        };
        let mut slices = Vec::new();
        for line in first_line..=last_line {
            slices.push(self.line_slice(line)?);
        }
        let source = document.session.document().to_bytes();
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
                body.iter().take(4).take_while(|byte| **byte == b' ').count()
            };
            if line_idx == 0 {
                first_delta = if indent { 4 } else { -(consumed as isize) };
            }
            replacement.extend_from_slice(&body[consumed..]);
            replacement.extend_from_slice(&source[slice.separator.clone()]);
        }
        if &source[begin..end] == replacement.as_slice() { return Ok(()); }
        let new_length = replacement.len();
        let document = self.active_document_mut().ok_or(WorkbenchError::NoDocument)?;
        document.session.document_mut().replace(begin..end, &replacement).map_err(FileError::from)?;
        if selected.is_some() {
            document.anchor = Some(begin);
            document.cursor = begin + new_length;
        } else {
            document.anchor = None;
            document.cursor = if first_delta >= 0 {
                cursor.saturating_add(first_delta as usize)
            } else {
                cursor.saturating_sub((-first_delta) as usize).max(begin)
            }.min(document.session.document().len());
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
        let source = self
            .active_document()
            .ok_or(WorkbenchError::NoDocument)?
            .session
            .document()
            .to_bytes();
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
        self.active_document_mut()
            .ok_or(WorkbenchError::NoDocument)?
            .session
            .document_mut()
            .replace(first.content.start..end, &replacement)
            .map_err(FileError::from)?;
        self.after_edit()
    }

    fn line_slice(&self, line: usize) -> Result<LineSlice, WorkbenchError> {
        let document = self.active_document().ok_or(WorkbenchError::NoDocument)?;
        let encoding = document
            .session
            .encoding()
            .ok_or(FileError::EncodingRequired)?;
        let decoded = decode(&document.session.document().to_bytes(), encoding);
        let index = TextIndex::new(&decoded);
        let length = index
            .line_len(line)
            .ok_or(PositionError::LineOutOfRange(line))?;
        let start = index.position_to_source_byte(TextPosition { line, column: 0 })?;
        let content_end = index.position_to_source_byte(TextPosition {
            line,
            column: length,
        })?;
        let separator_end = if line + 1 < index.line_count() {
            index.position_to_source_byte(TextPosition {
                line: line + 1,
                column: 0,
            })?
        } else {
            document.session.document().len()
        };
        Ok(LineSlice {
            content: start..content_end,
            separator: content_end..separator_end,
        })
    }

    pub fn select_all(&mut self) -> Result<(), WorkbenchError> {
        let document = self
            .active_document_mut()
            .ok_or(WorkbenchError::NoDocument)?;
        document.anchor = Some(0);
        document.cursor = document.session.document().len();
        Ok(())
    }

    pub fn copy_selection(&mut self) -> Result<(), WorkbenchError> {
        let document = self.active_document().ok_or(WorkbenchError::NoDocument)?;
        let Some(range) = document.selection() else {
            return Ok(());
        };
        let encoding = document
            .session
            .encoding()
            .ok_or(FileError::EncodingRequired)?;
        let bytes = document.session.document().to_bytes();
        let decoded = decode(&bytes[range], encoding);
        if !decoded.issues.is_empty() {
            return Err(WorkbenchError::ClipboardEncoding);
        }
        self.clipboard = Some(decoded.text);
        Ok(())
    }

    pub fn cut_selection(&mut self) -> Result<(), WorkbenchError> {
        self.copy_selection()?;
        self.delete_selection()?;
        Ok(())
    }

    pub fn paste(&mut self) -> Result<(), WorkbenchError> {
        let Some(text) = self.clipboard.clone() else {
            return Ok(());
        };
        self.insert_text(&text).map_err(|error| match error {
            WorkbenchError::File(FileError::Encode(_)) => WorkbenchError::ClipboardEncoding,
            other => other,
        })
    }

    pub fn undo(&mut self) -> Result<(), WorkbenchError> {
        let document = self
            .active_document_mut()
            .ok_or(WorkbenchError::NoDocument)?;
        if document.session.document_mut().undo() {
            document.cursor = document.cursor.min(document.session.document().len());
            document.anchor = None;
        }
        self.after_edit()
    }

    pub fn redo(&mut self) -> Result<(), WorkbenchError> {
        let document = self
            .active_document_mut()
            .ok_or(WorkbenchError::NoDocument)?;
        if document.session.document_mut().redo() {
            document.cursor = document.cursor.min(document.session.document().len());
            document.anchor = None;
        }
        self.after_edit()
    }
}

fn block_delimiters(language: &str) -> Option<(&'static [u8], &'static [u8])> {
    match language {
        "abl" | "csharp" | "rust" | "javascript" | "typescript" | "css" | "sql" | "jsonc" => {
            Some((b"/*", b"*/"))
        }
        "html" | "xml" | "markdown" => Some((b"<!--", b"-->")),
        _ => None,
    }
}
