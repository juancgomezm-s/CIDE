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
        let Some(comment) = detect(&path.to_string_lossy()).line_comment else {
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
