use crate::{CommandContext, CommandId, TabId, TabManager};
use jc_files::{FileError, FileSession};
use jc_languages::detect;
use jc_text::{decode, encode, PositionError, TextEncoding, TextIndex, TextPosition};
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
            Self::SearchEncoding => f.write_str("search text is not representable in the document encoding"),
            Self::RegexUnavailable => f.write_str("regular-expression search is not implemented yet"),
            Self::ClipboardEncoding => f.write_str("clipboard text cannot be represented in the target document encoding"),
        }
    }
}

impl std::error::Error for WorkbenchError {}
impl From<FileError> for WorkbenchError {
    fn from(value: FileError) -> Self { Self::File(value) }
}
impl From<PositionError> for WorkbenchError {
    fn from(value: PositionError) -> Self { Self::Position(value) }
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
    tabs: TabManager,
    documents: Vec<EditorDocument>,
    search: SearchState,
    clipboard: Option<String>,
    output: Vec<String>,
    problems: Vec<String>,
}

impl Workbench {
    pub fn tabs(&self) -> &TabManager { &self.tabs }
    pub fn tabs_mut(&mut self) -> &mut TabManager { &mut self.tabs }
    pub fn search(&self) -> &SearchState { &self.search }
    pub fn output(&self) -> &[String] { &self.output }
    pub fn problems(&self) -> &[String] { &self.problems }

    pub fn push_output(&mut self, line: impl Into<String>) {
        self.output.push(line.into());
        if self.output.len() > 2000 { self.output.drain(..self.output.len() - 2000); }
    }

    pub fn push_problem(&mut self, line: impl Into<String>) {
        self.problems.push(line.into());
        if self.problems.len() > 1000 { self.problems.drain(..self.problems.len() - 1000); }
    }

    pub fn clear_problems(&mut self) { self.problems.clear(); }

    pub fn open_file(&mut self, path: impl Into<PathBuf>, encoding: Option<TextEncoding>) -> Result<TabId, WorkbenchError> {
        let path = path.into();
        let session = FileSession::open(&path, encoding)?;
        let title = path.file_name().and_then(|name| name.to_str()).unwrap_or("<file>").to_owned();
        let tab_id = self.tabs.open(title);
        self.documents.push(EditorDocument { tab_id, session, cursor: 0, anchor: None, scroll_line: 0 });
        Ok(tab_id)
    }

    pub fn new_file(&mut self, path: impl Into<PathBuf>, encoding: TextEncoding) -> TabId {
        let path = path.into();
        let session = FileSession::new(&path, encoding);
        let title = path.file_name().and_then(|name| name.to_str()).unwrap_or("untitled").to_owned();
        let tab_id = self.tabs.open(title);
        self.documents.push(EditorDocument { tab_id, session, cursor: 0, anchor: None, scroll_line: 0 });
        tab_id
    }

    pub fn active_document(&self) -> Option<&EditorDocument> {
        let id = self.tabs.active_id()?;
        self.documents.iter().find(|document| document.tab_id == id)
    }

    pub fn active_document_mut(&mut self) -> Option<&mut EditorDocument> {
        let id = self.tabs.active_id()?;
        self.documents.iter_mut().find(|document| document.tab_id == id)
    }

    pub fn active_path(&self) -> Option<&Path> {
        self.active_document().map(|document| document.session.path())
    }

    pub fn active_encoding(&self) -> Option<TextEncoding> {
        self.active_document().and_then(|document| document.session.encoding())
    }

    pub fn active_bytes(&self) -> Option<Vec<u8>> {
        self.active_document().map(|document| document.session.document().to_bytes())
    }

    pub fn active_decoded(&self) -> Option<jc_text::DecodedText> {
        let document = self.active_document()?;
        let encoding = document.session.encoding()?;
        Some(decode(&document.session.document().to_bytes(), encoding))
    }

    pub fn active_position(&self) -> Option<TextPosition> {
        let document = self.active_document()?;
        let decoded = self.active_decoded()?;
        TextIndex::new(&decoded).source_byte_to_position(document.cursor).ok()
    }

    pub fn context(&self) -> CommandContext {
        let active = self.active_document();
        let language = active.map(|document| detect(&document.session.path().to_string_lossy()).id.to_owned());
        CommandContext {
            has_document: active.is_some(),
            document_dirty: active.is_some_and(|document| document.session.document().is_modified()),
            dirty_documents: self.documents.iter().filter(|document| document.session.document().is_modified()).count(),
            can_undo: active.is_some_and(|document| document.session.document().can_undo()),
            can_redo: active.is_some_and(|document| document.session.document().can_redo()),
            has_selection: active.and_then(EditorDocument::selection).is_some(),
            clipboard_has_text: self.clipboard.as_ref().is_some_and(|text| !text.is_empty()),
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
        self.active_document_mut().ok_or(WorkbenchError::NoDocument)?.session.set_encoding(encoding);
        Ok(())
    }

    pub fn insert_text(&mut self, text: &str) -> Result<(), WorkbenchError> {
        let document = self.active_document_mut().ok_or(WorkbenchError::NoDocument)?;
        let encoding = document.session.encoding().ok_or(FileError::EncodingRequired)?;
        let bytes = encode(text, encoding).map_err(FileError::from)?;
        if let Some(selection) = document.selection() {
            document.session.document_mut().replace(selection.clone(), &bytes).map_err(FileError::fr