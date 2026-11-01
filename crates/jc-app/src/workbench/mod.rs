mod commands;
mod edit;
mod search;

use crate::{CommandContext, CommandId, TabId, TabManager};
use jc_files::{FileError, FileSession};
use jc_languages::detect;
use jc_text::{decode, PositionError, TextEncoding, TextIndex, TextPosition};
use std::fmt;
use std::ops::Range;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum WorkbenchError {
    NoDocument,
    File(FileError),
    Position(PositionError),
    SearchEncoding,
    RegexUnavailable,
    ClipboardEncoding,
}

impl fmt::Display for WorkbenchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDocument => f.write_str("no active document"),
            Self::File(error) => write!(f, "{error}"),
            Self::Position(error) => write!(f, "{error}"),
            Self::SearchEncoding => f.write_str(
                "search text is not representable in the document encoding",
            ),
            Self::RegexUnavailable => {
                f.write_str("regular-expression search is not implemented yet")
            }
            Self::ClipboardEncoding => f.write_str(
                "clipboard text cannot be represented in the target document encoding",
            ),
        }
    }
}

impl std::error::Error for WorkbenchError {}

impl From<FileError> for WorkbenchError {
    fn from(value: FileError) -> Self {
        Self::File(value)
    }
}

impl From<PositionError> for WorkbenchError {
    fn from(value: PositionError) -> Self {
        Self::Position(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchState {
    pub query: String,
    pub matches: Vec<Range<usize>>,
    pub current: Option<usize>,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub regex: bool,
}

impl Default for SearchState {
    fn default() -> Self {
        Self {
            query: String::new(),
            matches: Vec::new(),
            current: None,
            case_sensitive: false,
            whole_word: false,
            regex: false,
        }
    }
}

#[derive(Debug)]
pub struct EditorDocument {
    pub tab_id: TabId,
    pub session: FileSession,
    pub cursor: usize,
    pub anchor: Option<usize>,
    pub scroll_line: usize,
}

impl EditorDocument {
    pub fn selection(&self) -> Option<Range<usize>> {
        let anchor = self.anchor?;
        (anchor != self.cursor).then_some(anchor.min(self.cursor)..anchor.max(self.cursor))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandOutcome {
    None,
    PromptOpen,
    PromptNew,
    PromptSaveAs,
    ShowFind,
    ShowReplace,
    ShowPalette,
    OpenMenu(CommandId),
    View(CommandId),
    Progress(CommandId),
    NeedsCloseDecision(TabId),
    ExitRequested,
}

#[derive(Debug, Default)]
pub struct Workbench {
    pub(crate) tabs: TabManager,
    pub(crate) documents: Vec<EditorDocument>,
    pub(crate) search: SearchState,
    pub(crate) clipboard: Option<String>,
    output: Vec<String>,
    problems: Vec<String>,
}

impl Workbench {
    pub fn tabs(&self) -> &TabManager {
        &self.tabs
    }

    pub fn tabs_mut(&mut self) -> &mut TabManager {
        &mut self.tabs
    }

    pub fn search(&self) -> &SearchState {
        &self.search
    }

    pub fn output(&self) -> &[String] {
        &self.output
    }

    pub fn problems(&self) -> &[String] {
        &self.problems
    }

    pub fn push_output(&mut self, line: impl Into<String>) {
        self.output.push(line.into());
        if self.output.len() > 2000 {
            self.output.drain(..self.output.len() - 2000);
        }
    }

    pub fn push_problem(&mut self, line: impl Into<String>) {
        self.problems.push(line.into());
        if self.problems.len() > 1000 {
            self.problems.drain(..self.problems.len() - 1000);
        }
    }

    pub fn clear_problems(&mut self) {
        self.problems.clear();
    }

    pub fn open_file(
        &mut self,
        path: impl Into<PathBuf>,
        encoding: Option<TextEncoding>,
    ) -> Result<TabId, WorkbenchError> {
        let path = path.into();
        let session = FileSession::open(&path, encoding)?;
        let title = title_for_path(&path, "<file>");
        let tab_id = self.tabs.open(title);
        self.documents.push(EditorDocument {
            tab_id,
            session,
            cursor: 0,
            anchor: None,
            scroll_line: 0,
        });
        Ok(tab_id)
    }

    pub fn new_file(&mut self, path: impl Into<PathBuf>, encoding: TextEncoding) -> TabId {
        let path = path.into();
        let session = FileSession::new(&path, encoding);
        let title = title_for_path(&path, "untitled");
        let tab_id = self.tabs.open(title);
        self.documents.push(EditorDocument {
            tab_id,
            session,
            cursor: 0,
            anchor: None,
            scroll_line: 0,
        });
        tab_id
    }

    pub fn active_document(&self) -> Option<&EditorDocument> {
        let id = self.tabs.active_id()?;
        self.documents
            .iter()
            .find(|document| document.tab_id == id)
    }

    pub fn active_document_mut(&mut self) -> Option<&mut EditorDocument> {
        let id = self.tabs.active_id()?;
        self.documents
            .iter_mut()
            .find(|document| document.tab_id == id)
    }

    pub fn active_path(&self) -> Option<&Path> {
        self.active_document()
            .map(|document| document.session.path())
    }

    pub fn active_encoding(&self) -> Option<TextEncoding> {
        self.active_document()
            .and_then(|document| document.session.encoding())
    }

    pub fn active_bytes(&self) -> Option<Vec<u8>> {
        self.active_document()
            .map(|document| document.session.document().to_bytes())
    }

    pub fn active_decoded(&self) -> Option<jc_text::DecodedText> {
        let document = self.active_document()?;
        let encoding = document.session.encoding()?;
        Some(decode(
            &document.session.document().to_bytes(),
            encoding,
        ))
    }

    pub fn active_position(&self) -> Option<TextPosition> {
        let document = self.active_document()?;
        let decoded = self.active_decoded()?;
        TextIndex::new(&decoded)
            .source_byte_to_position(document.cursor)
            .ok()
    }

    pub fn context(&self) -> CommandContext {
        let active = self.active_document();
        let language = active.map(|document| {
            detect(&document.session.path().to_string_lossy())
                .id
                .to_owned()
        });
        CommandContext {
            has_document: active.is_some(),
            document_dirty: active
                .is_some_and(|document| document.session.document().is_modified()),
            dirty_documents: self
                .documents
                .iter()
                .filter(|document| document.session.document().is_modified())
                .count(),
            can_undo: active.is_some_and(|document| document.session.document().can_undo()),
            can_redo: active.is_some_and(|document| document.session.document().can_redo()),
            has_selection: active.and_then(EditorDocument::selection).is_some(),
            clipboard_has_text: self
                .clipboard
                .as_ref()
                .is_some_and(|text| !text.is_empty()),
            has_project: false,
            active_language: language,
            has_progress_profile: false,
            has_database_schema: false,
            can_reopen_closed_tab: self.tabs.can_reopen(),
            terminal_available: false,
            ..CommandContext::default()
        }
    }

    pub fn set_encoding(&mut self, encoding: TextEncoding) -> Result<(), WorkbenchError> {
        self.active_document_mut()
            .ok_or(WorkbenchError::NoDocument)?
            .session
            .set_encoding(encoding);
        Ok(())
    }

    pub fn save_active(&mut self) -> Result<(), WorkbenchError> {
        self.active_document_mut()
            .ok_or(WorkbenchError::NoDocument)?
            .session
            .save()?;
        self.sync_active_tab_dirty();
        Ok(())
    }

    pub fn save_active_as(
        &mut self,
        path: impl Into<PathBuf>,
    ) -> Result<(), WorkbenchError> {
        let path = path.into();
        let (tab_id, title) = {
            let document = self
                .active_document_mut()
                .ok_or(WorkbenchError::NoDocument)?;
            document.session.save_as(path)?;
            (
                document.tab_id,
                title_for_path(document.session.path(), "<file>"),
            )
        };
        self.tabs.set_title(tab_id, title);
        self.tabs.set_dirty(tab_id, false);
        Ok(())
    }

    pub fn save_all(&mut self) -> Result<(), WorkbenchError> {
        for document in &mut self.documents {
            if document.session.document().is_modified() {
                document.session.save()?;
            }
            self.tabs
                .set_dirty(document.tab_id, document.session.document().is_modified());
        }
        Ok(())
    }

    pub fn close_active(
        &mut self,
        discard_dirty: bool,
    ) -> Result<Option<TabId>, WorkbenchError> {
        let id = self.tabs.active_id().ok_or(WorkbenchError::NoDocument)?;
        if !discard_dirty
            && self
                .documents
                .iter()
                .find(|document| document.tab_id == id)
                .is_some_and(|document| document.session.document().is_modified())
        {
            return Ok(Some(id));
        }
        self.documents.retain(|document| document.tab_id != id);
        let _ = self.tabs.discard_and_close(id);
        Ok(None)
    }

    pub fn select_tab(&mut self, one_based: usize) -> bool {
        self.tabs.select_index(one_based)
    }

    pub fn next_tab(&mut self) -> Option<TabId> {
        self.tabs.next_tab()
    }

    pub fn previous_tab(&mut self) -> Option<TabId> {
        self.tabs.previous_tab()
    }

    pub fn last_tab(&mut self) -> bool {
        self.tabs.select_last()
    }

    pub(crate) fn sync_active_tab_dirty(&mut self) {
        let state = self.active_document().map(|document| {
            (
                document.tab_id,
                document.session.document().is_modified(),
            )
        });
        if let Some((id, dirty)) = state {
            self.tabs.set_dirty(id, dirty);
        }
    }
}

fn title_for_path(path: &Path, fallback: &str) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(fallback)
        .to_owned()
}
