//! Explicit external-text handoff. No OS handle or clipboard access belongs here.
use super::{rectangular::Clipboard, DocumentId, Workbench, WorkbenchError};
use crate::ViewId;
use jc_files::FileError;
use jc_text::{DecodedScalars, TextEncoding};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipboardTarget {
    document: DocumentId,
    view: ViewId,
    revision: u64,
    encoding: TextEncoding,
    cursor: usize,
    anchor: Option<usize>,
}
#[derive(Debug)]
pub struct ClipboardExport {
    pub target: ClipboardTarget,
    pub text: String,
}

impl Workbench {
    pub fn clipboard_is_rectangular(&self) -> bool {
        matches!(self.clipboard, Some(Clipboard::Rectangle(_)))
    }
    pub fn remember_exported_copy(
        &mut self,
        export: ClipboardExport,
    ) -> Result<(), WorkbenchError> {
        self.validate_clipboard_target(export.target)?;
        self.clipboard = Some(Clipboard::Text(export.text));
        Ok(())
    }
    /// Capture before a potentially reentrant native call. Revalidate after it;
    /// never keep a mutable model borrow while contacting another clipboard owner.
    pub fn capture_clipboard_target(&self) -> Result<ClipboardTarget, WorkbenchError> {
        self.require_linear_selection()?;
        let editor = self.active_editor().ok_or(WorkbenchError::NoDocument)?;
        Ok(ClipboardTarget {
            document: editor.document_id,
            view: self
                .panes
                .view(self.panes.active())
                .and_then(|v| v.view_id())
                .ok_or(WorkbenchError::NoDocument)?,
            revision: editor.session.document().revision(),
            encoding: editor
                .session
                .encoding()
                .ok_or(FileError::EncodingRequired)?,
            cursor: editor.cursor,
            anchor: editor.anchor,
        })
    }
    fn validate_clipboard_target(&self, target: ClipboardTarget) -> Result<(), WorkbenchError> {
        if self.capture_clipboard_target().ok() != Some(target) {
            return Err(WorkbenchError::ClipboardStale);
        }
        Ok(())
    }
    /// Read just the selected source range. Preflight scalar validity/UTF-8
    /// expansion without a decoded offset vector before allocating the string.
    pub(super) fn selection_text_bounded(
        &self,
        max_bytes: usize,
    ) -> Result<Option<String>, WorkbenchError> {
        self.require_linear_selection()?;
        let editor = self.active_editor().ok_or(WorkbenchError::NoDocument)?;
        let Some(range) = editor.selection() else {
            return Ok(None);
        };
        if range.len() > max_bytes {
            return Err(WorkbenchError::ClipboardLimit);
        }
        let encoding = editor
            .session
            .encoding()
            .ok_or(FileError::EncodingRequired)?;
        let bytes = editor
            .session
            .document()
            .read_range(range.clone())
            .map_err(FileError::from)?;
        let mut length = 0usize;
        for scalar in DecodedScalars::new(&bytes, encoding, range.start) {
            if !scalar.valid {
                return Err(WorkbenchError::ClipboardEncoding);
            }
            length = length
                .checked_add(scalar.character.len_utf8())
                .filter(|n| *n <= max_bytes)
                .ok_or(WorkbenchError::ClipboardLimit)?;
        }
        let mut text = String::new();
        text.try_reserve_exact(length)
            .map_err(|_| WorkbenchError::ClipboardLimit)?;
        text.extend(DecodedScalars::new(&bytes, encoding, range.start).map(|s| s.character));
        Ok(Some(text))
    }
    pub fn export_selection(
        &self,
        max_bytes: usize,
    ) -> Result<Option<ClipboardExport>, WorkbenchError> {
        let target = self.capture_clipboard_target()?;
        Ok(self
            .selection_text_bounded(max_bytes)?
            .map(|text| ClipboardExport { target, text }))
    }
    /// Called only after successful external publication. Stale handoffs cannot
    /// cut a different selection or document. Uses the ordinary single edit.
    pub fn delete_exported_selection(
        &mut self,
        export: ClipboardExport,
    ) -> Result<(), WorkbenchError> {
        self.selection_edit(|app| app.delete_exported_selection_inner(export))
    }

    fn delete_exported_selection_inner(
        &mut self,
        export: ClipboardExport,
    ) -> Result<(), WorkbenchError> {
        self.validate_clipboard_target(export.target)?;
        self.delete_selection()?;
        self.clipboard = Some(Clipboard::Text(export.text));
        Ok(())
    }
    /// Strict encoding and atomic undo remain insert_text's responsibility.
    pub fn paste_external_text(
        &mut self,
        target: ClipboardTarget,
        text: &str,
        max_bytes: usize,
    ) -> Result<(), WorkbenchError> {
        self.validate_clipboard_target(target)?;
        if text.len() > max_bytes {
            return Err(WorkbenchError::ClipboardLimit);
        }
        if text.is_empty() {
            return Ok(());
        }
        self.insert_text(text).map_err(|error| match error {
            WorkbenchError::File(FileError::Encode(_)) => WorkbenchError::ClipboardEncoding,
            other => other,
        })
    }
}
