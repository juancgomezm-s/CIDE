mod commands;
mod edit;
mod search;

use crate::{CommandContext, CommandId, NavLocation, PaneId, PaneManager, TabId, TabManager};
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
            Self::SearchEncoding => {
                f.write_str("search text is not representable in the document encoding")
            }
            Self::RegexUnavailable => {
                f.write_str("regular-expression search is not implemented yet")
            }
            Self::ClipboardEncoding => {
                f.write_str("clipboard text cannot be represented in the target document encoding")
            }
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchState {
    pub query: String,
    pub matches: Vec<Range<usize>>,
    pub current: Option<usize>,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub regex: bool,
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
    panes: PaneManager,
    pub(crate) documents: Vec<EditorDocument>,
    closed_documents: Vec<EditorDocument>,
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

    pub fn panes(&self) -> &PaneManager {
        &self.panes
    }

    pub fn document_by_tab(&self, id: TabId) -> Option<&EditorDocument> {
        self.documents.iter().find(|document| document.tab_id == id)
    }

    pub fn decoded_for_tab(&self, id: TabId) -> Option<jc_text::DecodedText> {
        let document = self.document_by_tab(id)?;
        let encoding = document.session.encoding()?;
        Some(decode(&document.session.document().to_bytes(), encoding))
    }

    pub fn position_for_pane(&self, pane: PaneId) -> Option<TextPosition> {
        let location = self.panes.view(pane)?.location()?;
        let decoded = self.decoded_for_tab(location.tab)?;
        let offset = if pane == self.panes.active() {
            self.document_by_tab(location.tab)?.cursor
        } else {
            location.byte_offset
        };
        TextIndex::new(&decoded).source_byte_to_position(offset).ok()
    }

    fn record_active_pane_cursor(&mut self) {
        if let Some(document) = self.active_document() {
            self.panes.update_cursor(NavLocation::new(document.tab_id, document.cursor));
        }
    }

    fn restore_location(&mut self, location: NavLocation) -> bool {
        if !self.tabs.select(location.tab) {
            return false;
        }
        if let Some(document) = self.active_document_mut() {
            document.cursor = location.byte_offset.min(document.session.document().len());
            document.anchor = None;
        }
        let _ = self.rebuild_search();
        true
    }

    fn select_opened_tab(&mut self, tab: TabId, offset: usize, automatic: bool) {
        self.record_active_pane_cursor();
        self.panes.navigate(NavLocation::new(tab, offset), automatic);
        let _ = self.tabs.select(tab);
        let _ = self.rebuild_search();
    }

    pub fn split_editor_right(&mut self) -> bool {
        self.record_active_pane_cursor();
        self.panes.split_right()
    }

    pub fn close_split_editor(&mut self) -> bool {
        self.record_active_pane_cursor();
        let primary = self.panes.view(PaneId::Primary).and_then(|pane| pane.location());
        if !self.panes.close_secondary() {
            return false;
        }
        if let Some(location) = primary {
            self.restore_location(location);
        }
        true
    }

    pub fn focus_editor_pane(&mut self, pane: PaneId) -> bool {
        self.record_active_pane_cursor();
        let Some(location) = self.panes.focus(pane) else {
            return false;
        };
        self.restore_location(location)
    }

    pub fn toggle_reference_pin(&mut self) -> Option<bool> {
        self.panes.toggle_pin()
    }

    pub fn toggle_scroll_link(&mut self) -> Option<bool> {
        self.panes.toggle_linked_scroll()
    }

    pub fn navigation_back(&mut self) -> bool {
        self.record_active_pane_cursor();
        self.panes.go_back().is_some_and(|location| self.restore_location(location))
    }

    pub fn navigation_forward(&mut self) -> bool {
        self.record_active_pane_cursor();
        self.panes.go_forward().is_some_and(|location| self.restore_location(location))
    }

    pub fn jump_to_location(&mut self, tab: TabId, offset: usize) -> bool {
        let Some(document) = self.document_by_tab(tab) else {
            return false;
        };
        if offset > document.session.document().len() {
            return false;
        }
        self.record_active_pane_cursor();
        self.panes.navigate(NavLocation::new(tab, offset), true);
        self.restore_location(NavLocation::new(tab, offset))
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
        self.record_active_pane_cursor();
        let cursor = session
            .inspection()
            .decoded
            .as_ref()
            .map_or(0, |decoded| decoded.content_start);
        let title = title_for_path(&path, "<file>");
        let tab_id = self.tabs.open(title);
        self.documents.push(EditorDocument {
            tab_id,
            session,
            cursor,
            anchor: None,
            scroll_line: 0,
        });
        self.panes.navigate(NavLocation::new(tab_id, cursor), true);
        let _ = self.rebuild_search();
        Ok(tab_id)
    }

    pub fn new_file(&mut self, path: impl Into<PathBuf>, encoding: TextEncoding) -> TabId {
        let path = path.into();
        let session = FileSession::new(&path, encoding);
        self.record_active_pane_cursor();
        let title = title_for_path(&path, "untitled");
        let tab_id = self.tabs.open(title);
        self.documents.push(EditorDocument {
            tab_id,
            session,
            cursor: 0,
            anchor: None,
            scroll_line: 0,
        });
        self.panes.navigate(NavLocation::new(tab_id, 0), true);
        let _ = self.rebuild_search();
        tab_id
    }

    pub fn active_document(&self) -> Option<&EditorDocument> {
        let id = self.tabs.active_id()?;
        self.documents.iter().find(|document| document.tab_id == id)
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
        Some(decode(&document.session.document().to_bytes(), encoding))
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
            clipboard_has_text: self.clipboard.as_ref().is_some_and(|text| !text.is_empty()),
            has_project: false,
            active_language: language,
            has_progress_profile: false,
            has_database_schema: false,
            can_reopen_closed_tab: self.tabs.can_reopen() && !self.closed_documents.is_empty(),
            terminal_available: false,
            split_editor: self.panes.is_split(),
            reference_pinned: self.panes.view(self.panes.active()).is_some_and(|pane| pane.is_pinned()),
            nav_back_available: self.panes.can_back(),
            nav_forward_available: self.panes.can_forward(),
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

    pub fn save_active_as(&mut self, path: impl Into<PathBuf>) -> Result<(), WorkbenchError> {
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

    pub fn close_active(&mut self, discard_dirty: bool) -> Result<Option<TabId>, WorkbenchError> {
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
        if let Some(index) = self
            .documents
            .iter()
            .position(|document| document.tab_id == id)
        {
            let document = self.documents.remove(index);
            self.closed_documents.push(document);
            if self.closed_documents.len() > 32 {
                self.closed_documents.remove(0);
            }
        }
        let _ = self.tabs.discard_and_close(id);
        let fallback = self.tabs.active_id().and_then(|tab| {
            self.document_by_tab(tab)
                .map(|document| NavLocation::new(tab, document.cursor))
        });
        self.panes.forget_tab(id, fallback);
        Ok(None)
    }

    pub fn reopen_closed(&mut self) -> bool {
        let Some(document) = self.closed_documents.pop() else {
            return false;
        };
        let expected = document.tab_id;
        if self.tabs.reopen_last() != Some(expected) {
            self.closed_documents.push(document);
            return false;
        }
        let cursor = document.cursor;
        self.documents.push(document);
        self.panes.navigate(NavLocation::new(expected, cursor), false);
        let _ = self.rebuild_search();
        true
    }

    pub fn select_tab(&mut self, one_based: usize) -> bool {
        self.record_active_pane_cursor();
        if !self.tabs.select_index(one_based) {
            return false;
        }
        if let Some(tab) = self.tabs.active_id() {
            let offset = self.document_by_tab(tab).map_or(0, |document| document.cursor);
            self.panes.navigate(NavLocation::new(tab, offset), false);
        }
        let _ = self.rebuild_search();
        true
    }

    pub fn next_tab(&mut self) -> Option<TabId> {
        self.record_active_pane_cursor();
        let tab = self.tabs.next_tab()?;
        let offset = self.document_by_tab(tab).map_or(0, |document| document.cursor);
        self.panes.navigate(NavLocation::new(tab, offset), false);
        let _ = self.rebuild_search();
        Some(tab)
    }

    pub fn previous_tab(&mut self) -> Option<TabId> {
        self.record_active_pane_cursor();
        let tab = self.tabs.previous_tab()?;
        let offset = self.document_by_tab(tab).map_or(0, |document| document.cursor);
        self.panes.navigate(NavLocation::new(tab, offset), false);
        let _ = self.rebuild_search();
        Some(tab)
    }

    pub fn last_tab(&mut self) -> bool {
        self.record_active_pane_cursor();
        if !self.tabs.select_last() {
            return false;
        }
        if let Some(tab) = self.tabs.active_id() {
            let offset = self.document_by_tab(tab).map_or(0, |document| document.cursor);
            self.panes.navigate(NavLocation::new(tab, offset), false);
        }
        let _ = self.rebuild_search();
        true
    }

    pub(crate) fn sync_active_tab_dirty(&mut self) {
        let state = self
            .active_document()
            .map(|document| (document.tab_id, document.session.document().is_modified()));
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
