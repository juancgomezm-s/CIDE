use super::{EditorDocument, Workbench, WorkbenchError};
use jc_files::FileError;
use jc_text::{decode, encode, PositionError, TextIndex, TextPosition};

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
        let text = if ending == b"
" {
            "
"
        } else if ending == b"" {
            ""
        } else {
            "
"
        };
        self.insert_text(text)
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
        let document = self
            .active_document()
            .ok_or(WorkbenchError::NoDocument)?;
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
        Ok(self.boundaries()?.into_iter().find(|offset| *offset > cursor))
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
        let document = self
            .active_document()
            .ok_or(WorkbenchError::NoDocument)?;
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
        let document = self
            .active_document()
            .ok_or(WorkbenchError::NoDocument)?;
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
        let document = self
            .active_document()
            .ok_or(WorkbenchError::NoDocument)?;
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

    pub fn select_all(&mut self) -> Result<(), WorkbenchError> {
        let document = self
            .active_document_mut()
            .ok_or(WorkbenchError::NoDocument)?;
        document.anchor = Some(0);
        document.cursor = document.session.document().len();
        Ok(())
    }

    pub fn copy_selection(&mut self) -> Result<(), WorkbenchError> {
        let document = self
            .active_document()
            .ok_or(WorkbenchError::NoDocument)?;
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
