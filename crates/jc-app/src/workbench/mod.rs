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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DocumentId(pub u64);

#[derive(Debug)]
pub struct EditorDocument {
    pub id: DocumentId,
    pub tab_id: TabId,
    pub session: FileSession,
    notified_revision: u64,
}

#[derive(Debug)]
struct ClosedDocument {
    tab: TabId,
    path: PathBuf,
    encoding: Option<TextEncoding>,
    new_file: bool,
}

#[derive(Debug, Clone)]
pub struct TextReplacement {
    pub range: Range<usize>,
    pub text: String,
}

/// Borrowed projection; selection lives only in the group's persistent view.
pub struct EditorRead<'a> {
    pub session: &'a FileSession,
    pub tab_id: TabId,
    pub document_id: DocumentId,
    state: &'a crate::ViewState,
}
impl std::ops::Deref for EditorRead<'_> {
    type Target = crate::ViewState;
    fn deref(&self) -> &Self::Target {
        self.state
    }
}
struct EditorWrite<'a> {
    session: &'a mut FileSession,
    state: &'a mut crate::ViewState,
}
impl std::ops::Deref for EditorWrite<'_> {
    type Target = crate::ViewState;
    fn deref(&self) -> &Self::Target {
        self.state
    }
}
impl std::ops::DerefMut for EditorWrite<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.state
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
    documents: Vec<EditorDocument>,
    closed_documents: Vec<ClosedDocument>,
    next_document: u64,
    pub(crate) search: SearchState,
    pub(crate) clipboard: Option<String>,
    output: Vec<String>,
    problems: Vec<String>,
}

impl Workbench {
    pub fn tabs(&self) -> &TabManager {
        &self.tabs
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

    fn cursor_for_tab(&self, tab: TabId) -> usize {
        self.panes.remembered_cursor(tab).unwrap_or_else(|| {
            self.document_by_tab(tab)
                .and_then(|d| d.session.inspection().decoded)
                .map_or(0, |d| d.content_start)
        })
    }

    pub fn position_for_pane(&self, pane: PaneId) -> Option<TextPosition> {
        let location = self.panes.view(pane)?.location()?;
        let decoded = self.decoded_for_tab(location.tab)?;
        let offset = location.byte_offset;
        TextIndex::new(&decoded)
            .source_byte_to_position(offset)
            .ok()
    }

    fn activate_location_document(&mut self, location: NavLocation) -> bool {
        if !self.tabs.select(location.tab) {
            return false;
        }
        let _ = self.rebuild_search();
        true
    }

    fn select_opened_tab(&mut self, tab: TabId, offset: usize, automatic: bool) {
        self.panes
            .navigate(NavLocation::new(tab, offset), automatic);
        self.activate_location_document(NavLocation::new(tab, offset));
    }

    pub fn split_editor_right(&mut self) -> bool {
        self.panes.split_right()
    }

    pub fn close_split_editor(&mut self) -> bool {
        let primary = self
            .panes
            .view(PaneId::Primary)
            .and_then(|pane| pane.location());
        if !self.panes.close_secondary() {
            return false;
        }
        if let Some(location) = primary {
            self.activate_location_document(location);
        } else if let Some(tab) = self.tabs.active_id() {
            self.select_opened_tab(tab, self.cursor_for_tab(tab), false);
        }
        true
    }

    pub fn focus_editor_pane(&mut self, pane: PaneId) -> bool {
        if pane == self.panes.active() {
            return self.active_document().is_some();
        }
        let Some(location) = self.panes.focus(pane) else {
            return false;
        };
        self.activate_location_document(location)
    }

    pub fn toggle_reference_pin(&mut self) -> Option<bool> {
        self.panes.toggle_pin()
    }

    pub fn toggle_scroll_link(&mut self) -> Option<bool> {
        self.panes.toggle_linked_scroll()
    }

    pub fn navigation_back(&mut self) -> bool {
        self.panes
            .go_back()
            .is_some_and(|location| self.activate_location_document(location))
    }

    pub fn navigation_forward(&mut self) -> bool {
        self.panes
            .go_forward()
            .is_some_and(|location| self.activate_location_document(location))
    }

    pub fn jump_to_location(&mut self, tab: TabId, offset: usize) -> bool {
        let Some(document) = self.document_by_tab(tab) else {
            return false;
        };
        if offset > document.session.document().len()
            || self.decoded_for_tab(tab).is_some_and(|decoded| {
                TextIndex::new(&decoded)
                    .source_byte_to_position(offset)
                    .is_err()
            })
        {
            return false;
        }
        self.panes.navigate(NavLocation::new(tab, offset), true);
        self.activate_location_document(NavLocation::new(tab, offset))
    }

    /// Open the current document beside this group; a pinned target is preserved.
    pub fn open_active_to_side(&mut self) -> bool {
        let Some(tab) = self.tabs.active_id() else {
            return false;
        };
        let offset = self.active_editor().map_or(0, |doc| doc.cursor);
        self.prepare_side_target();
        self.jump_to_location(tab, offset)
    }

    fn prepare_side_target(&mut self) {
        self.split_editor_right();
        let other = match self.panes.active() {
            PaneId::Primary => PaneId::Secondary,
            PaneId::Secondary => PaneId::Primary,
        };
        if !self.panes.view(other).is_some_and(|view| view.is_pinned()) {
            self.focus_editor_pane(other);
        }
    }

    pub fn open_file_to_side(
        &mut self,
        path: impl Into<PathBuf>,
        encoding: Option<TextEncoding>,
    ) -> Result<TabId, WorkbenchError> {
        self.open_file_with_target(path.into(), encoding, true)
    }

    pub fn selection_for_pane(&self, pane: PaneId) -> Option<Range<usize>> {
        let view = self.panes.view(pane)?;
        let cursor = view.location()?.byte_offset;
        let anchor = view.anchor()?;
        (anchor != cursor).then_some(anchor.min(cursor)..anchor.max(cursor))
    }

    /// Reveal the caret in the active viewport. Other viewports move only when
    /// linked scrolling is enabled, by the same vertical delta (not caret sync).
    pub fn reveal_active_pane(&mut self, rows: usize, columns: usize) {
        let Some(position) = self.active_position() else {
            return;
        };
        let active = self.panes.active();
        let view = self.panes.view(active).expect("active view");
        let old_line = view.scroll_line();
        let mut line = old_line;
        let mut column = view.scroll_column();
        if position.line < line {
            line = position.line;
        } else if position.line >= line.saturating_add(rows.max(1)) {
            line = position.line + 1 - rows.max(1);
        }
        if position.column < column {
            column = position.column;
        } else if position.column >= column.saturating_add(columns.max(1)) {
            column = position.column + 1 - columns.max(1);
        }
        self.panes.set_scroll(active, line, column);
        if self.panes.scroll_linked() {
            let other = if active == PaneId::Primary {
                PaneId::Secondary
            } else {
                PaneId::Primary
            };
            let view = self.panes.view(other).expect("split view");
            let other_line = if line >= old_line {
                view.scroll_line().saturating_add(line - old_line)
            } else {
                view.scroll_line().saturating_sub(old_line - line)
            };
            let max_line = view
                .location()
                .and_then(|loc| self.decoded_for_tab(loc.tab))
                .map_or(0, |decoded| {
                    TextIndex::new(&decoded).line_count().saturating_sub(1)
                });
            self.panes
                .set_scroll(other, other_line.min(max_line), view.scroll_column());
        }
    }

    pub(crate) fn rebase_after_edit(&mut self) {
        let Some(document) = self.active_document_mut() else {
            return;
        };
        let revision = document.session.document().revision();
        if revision == document.notified_revision {
            return;
        }
        document.notified_revision = revision;
        let tab = document.tab_id;
        let change = document.session.document().last_change().cloned();
        if let Some(change) = change {
            self.panes.rebase(tab, &change, false);
        }
    }

    fn normalize_views(&mut self, tab: TabId) {
        if let Some(decoded) = self.decoded_for_tab(tab) {
            let index = TextIndex::new(&decoded);
            let boundaries: Vec<_> = decoded
                .source_offsets
                .iter()
                .copied()
                .filter(|&n| index.source_byte_to_position(n).is_ok())
                .collect();
            self.panes.normalize(tab, &boundaries);
        }
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
        self.open_file_with_target(path.into(), encoding, false)
    }

    fn open_file_with_target(
        &mut self,
        path: PathBuf,
        encoding: Option<TextEncoding>,
        to_side: bool,
    ) -> Result<TabId, WorkbenchError> {
        if let Ok(canonical) = std::fs::canonicalize(&path) {
            if let Some(document) = self.documents.iter().find(|document| {
                std::fs::canonicalize(document.session.path()).ok().as_ref() == Some(&canonical)
            }) {
                let (tab, cursor) = (document.tab_id, self.cursor_for_tab(document.tab_id));
                if to_side {
                    self.prepare_side_target();
                }
                self.select_opened_tab(tab, cursor, true);
                return Ok(tab);
            }
        }
        let session = FileSession::open(&path, encoding)?;
        // Read failures leave the current group, selection and history intact.
        let first_side_open = to_side && self.active_document().is_none();
        if to_side && !first_side_open {
            self.prepare_side_target();
        }
        let cursor = session
            .inspection()
            .decoded
            .as_ref()
            .map_or(0, |decoded| decoded.content_start);
        let title = title_for_path(&path, "<file>");
        let tab_id = self.tabs.open(title);
        self.next_document += 1;
        self.documents.push(EditorDocument {
            tab_id,
            session,
            id: DocumentId(self.next_document),
            notified_revision: 0,
        });
        self.panes.navigate(NavLocation::new(tab_id, cursor), true);
        let _ = self.rebuild_search();
        if first_side_open {
            self.split_editor_right();
            self.focus_editor_pane(PaneId::Secondary);
        }
        Ok(tab_id)
    }

    pub fn new_file(&mut self, path: impl Into<PathBuf>, encoding: TextEncoding) -> TabId {
        let path = path.into();
        let session = FileSession::new(&path, encoding);
        let title = title_for_path(&path, "untitled");
        let tab_id = self.tabs.open(title);
        self.next_document += 1;
        self.documents.push(EditorDocument {
            tab_id,
            session,
            id: DocumentId(self.next_document),
            notified_revision: 0,
        });
        self.panes.navigate(NavLocation::new(tab_id, 0), true);
        let _ = self.rebuild_search();
        tab_id
    }

    pub fn active_document(&self) -> Option<&EditorDocument> {
        let id = self.panes.view(self.panes.active())?.location()?.tab;
        self.documents.iter().find(|document| document.tab_id == id)
    }

    fn active_document_mut(&mut self) -> Option<&mut EditorDocument> {
        let id = self.panes.view(self.panes.active())?.location()?.tab;
        self.documents
            .iter_mut()
            .find(|document| document.tab_id == id)
    }

    pub fn active_editor(&self) -> Option<EditorRead<'_>> {
        let view = self.panes.view(self.panes.active())?;
        let document = self.document_by_tab(view.location()?.tab)?;
        Some(EditorRead {
            session: &document.session,
            tab_id: document.tab_id,
            document_id: document.id,
            state: view.state()?,
        })
    }

    fn active_editor_mut(&mut self) -> Option<EditorWrite<'_>> {
        let tab = self.panes.view(self.panes.active())?.location()?.tab;
        let document = self.documents.iter_mut().find(|d| d.tab_id == tab)?;
        Some(EditorWrite {
            session: &mut document.session,
            state: self.panes.active_state_mut()?,
        })
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
        let decoded = self.active_decoded()?;
        TextIndex::new(&decoded)
            .source_byte_to_position(self.active_editor()?.cursor)
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
            has_selection: self
                .active_editor()
                .and_then(|editor| editor.selection())
                .is_some(),
            clipboard_has_text: self.clipboard.as_ref().is_some_and(|text| !text.is_empty()),
            has_project: false,
            active_language: language,
            has_progress_profile: false,
            has_database_schema: false,
            can_reopen_closed_tab: self.tabs.can_reopen() && !self.closed_documents.is_empty(),
            terminal_available: false,
            split_editor: self.panes.is_split(),
            reference_pinned: self
                .panes
                .view(self.panes.active())
                .is_some_and(|pane| pane.is_pinned()),
            nav_back_available: self.panes.can_back(),
            nav_forward_available: self.panes.can_forward(),
            can_open_to_side: active.is_some(),
            ..CommandContext::default()
        }
    }

    pub fn set_encoding(&mut self, encoding: TextEncoding) -> Result<(), WorkbenchError> {
        self.active_document_mut()
            .ok_or(WorkbenchError::NoDocument)?
            .session
            .set_encoding(encoding);
        let tab = self
            .active_document()
            .ok_or(WorkbenchError::NoDocument)?
            .tab_id;
        self.normalize_views(tab);
        let _ = self.rebuild_search();
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
        let tab = self.tabs.active_id().ok_or(WorkbenchError::NoDocument)?;
        // Closing a presentation never discards a document used by another view.
        if self.panes.view_count(tab) > 1 {
            if let Some(loc) = self.panes.close_current() {
                self.activate_location_document(loc);
            } else {
                let other = if self.panes.active() == PaneId::Primary {
                    PaneId::Secondary
                } else {
                    PaneId::Primary
                };
                self.focus_editor_pane(other);
            }
            self.rebuild_search()?;
            return Ok(None);
        }
        let document = self.active_document().ok_or(WorkbenchError::NoDocument)?;
        if !discard_dirty && document.session.document().is_modified() {
            return Ok(Some(tab));
        }
        let closed = ClosedDocument {
            tab,
            path: document.session.path().to_owned(),
            encoding: document.session.encoding(),
            new_file: document.session.is_new(),
        };
        self.documents.retain(|d| d.tab_id != tab);
        self.closed_documents.push(closed);
        if self.closed_documents.len() > 32 {
            self.closed_documents.remove(0);
        }
        self.tabs.discard_and_close(tab);
        let fallback = self
            .tabs
            .active_id()
            .map(|id| NavLocation::new(id, self.cursor_for_tab(id)));
        self.panes.forget_tab(tab, fallback);
        if let Some(loc) = fallback {
            self.activate_location_document(loc);
        } else {
            self.rebuild_search()?;
        }
        Ok(None)
    }

    pub fn reopen_closed(&mut self) -> bool {
        let Some(closed) = self.closed_documents.last() else {
            return false;
        };
        let session = if closed.new_file {
            FileSession::new(&closed.path, closed.encoding.unwrap_or(TextEncoding::Utf8))
        } else {
            let Ok(session) = FileSession::open(&closed.path, closed.encoding) else {
                return false;
            };
            session
        };
        let expected = closed.tab;
        if self.tabs.reopen_last() != Some(expected) {
            return false;
        }
        self.closed_documents.pop();
        let cursor = session
            .inspection()
            .decoded
            .as_ref()
            .map_or(0, |d| d.content_start);
        self.next_document += 1;
        self.documents.push(EditorDocument {
            id: DocumentId(self.next_document),
            tab_id: expected,
            session,
            notified_revision: 0,
        });
        self.panes
            .navigate(NavLocation::new(expected, cursor), false);
        let _ = self.rebuild_search();
        true
    }

    /// Validate all text and byte boundaries before publishing a transaction.
    /// A provider can target any open document without switching focus.
    pub fn apply_text_edits(
        &mut self,
        id: DocumentId,
        expected_revision: u64,
        edits: Vec<TextReplacement>,
    ) -> Result<(), WorkbenchError> {
        let document = self
            .documents
            .iter_mut()
            .find(|d| d.id == id)
            .ok_or(WorkbenchError::NoDocument)?;
        let encoding = document
            .session
            .encoding()
            .ok_or(FileError::EncodingRequired)?;
        let decoded = decode(&document.session.document().to_bytes(), encoding);
        let index = TextIndex::new(&decoded);
        let mut replacements = Vec::with_capacity(edits.len());
        for edit in edits {
            for offset in [edit.range.start, edit.range.end] {
                if offset != 0 {
                    index.source_byte_to_position(offset)?;
                }
            }
            replacements.push(jc_core::Replacement {
                range: edit.range,
                bytes: jc_text::encode(&edit.text, encoding).map_err(FileError::from)?,
            });
        }
        document
            .session
            .document_mut()
            .apply_batch(jc_core::EditBatch {
                expected_revision,
                edits: replacements,
            })
            .map_err(FileError::from)?;
        let tab = document.tab_id;
        document.notified_revision = document.session.document().revision();
        let change = document.session.document().last_change().cloned();
        let dirty = document.session.document().is_modified();
        if let Some(change) = change {
            self.panes.rebase(tab, &change, true);
        }
        self.normalize_views(tab);
        self.tabs.set_dirty(tab, dirty);
        let _ = self.rebuild_search();
        Ok(())
    }

    pub fn select_tab(&mut self, one_based: usize) -> bool {
        if !self.tabs.select_index(one_based) {
            return false;
        }
        if let Some(tab) = self.tabs.active_id() {
            let offset = self
                .document_by_tab(tab)
                .map_or(0, |_| self.cursor_for_tab(tab));
            self.panes.navigate(NavLocation::new(tab, offset), false);
            self.activate_location_document(NavLocation::new(tab, offset));
        }
        let _ = self.rebuild_search();
        true
    }

    pub fn next_tab(&mut self) -> Option<TabId> {
        let tab = self.tabs.next_tab()?;
        let offset = self
            .document_by_tab(tab)
            .map_or(0, |_| self.cursor_for_tab(tab));
        self.panes.navigate(NavLocation::new(tab, offset), false);
        self.activate_location_document(NavLocation::new(tab, offset));
        let _ = self.rebuild_search();
        Some(tab)
    }

    pub fn previous_tab(&mut self) -> Option<TabId> {
        let tab = self.tabs.previous_tab()?;
        let offset = self
            .document_by_tab(tab)
            .map_or(0, |_| self.cursor_for_tab(tab));
        self.panes.navigate(NavLocation::new(tab, offset), false);
        self.activate_location_document(NavLocation::new(tab, offset));
        let _ = self.rebuild_search();
        Some(tab)
    }

    pub fn last_tab(&mut self) -> bool {
        if !self.tabs.select_last() {
            return false;
        }
        if let Some(tab) = self.tabs.active_id() {
            let offset = self
                .document_by_tab(tab)
                .map_or(0, |_| self.cursor_for_tab(tab));
            self.panes.navigate(NavLocation::new(tab, offset), false);
            self.activate_location_document(NavLocation::new(tab, offset));
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
