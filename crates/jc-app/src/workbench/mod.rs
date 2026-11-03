mod commands;
mod display;
pub(crate) mod invisible_layout;
pub use invisible_layout::DisplayCell;
mod clipboard_transfer;
pub use clipboard_transfer::{ClipboardExport, ClipboardTarget};
mod edit;
mod line_commands;
mod line_index;
pub(crate) mod rectangular;
mod save_target;
mod search;
pub use save_target::SaveTarget;
mod syntax;
mod text;
mod viewport;
use std::cell::RefCell;
pub use syntax::{SyntaxRows, SyntaxStats};
use text::TextProjection;

use crate::{CommandContext, CommandId, NavLocation, PaneId, PaneManager, TabId, TabManager};
use jc_files::{FileError, FileSession};
use jc_languages::detect;
use jc_text::{PositionError, TextEncoding, TextPosition};
use std::fmt;
use std::ops::Range;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum WorkbenchError {
    NoDocument,
    File(FileError),
    Position(PositionError),
    SearchPattern(String),
    UnicodeLayout(String),
    ClipboardEncoding,
    ClipboardLimit,
    ClipboardStale,
    Save(String),
    SearchInput,
    SearchIncomplete,
    Rectangle(String),
    LineCommand(String),
}

impl fmt::Display for WorkbenchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Save(message) | Self::UnicodeLayout(message) => f.write_str(message),
            Self::ClipboardLimit => f.write_str("clipboard transfer exceeds its byte budget"),
            Self::ClipboardStale => f.write_str("clipboard target changed; retry the action"),
            Self::Rectangle(message) => f.write_str(message),
            Self::LineCommand(message) => f.write_str(message),
            Self::NoDocument => f.write_str("no active document"),
            Self::File(error) => write!(f, "{error}"),
            Self::Position(error) => write!(f, "{error}"),
            Self::SearchPattern(message) => f.write_str(message),
            Self::ClipboardEncoding => {
                f.write_str("clipboard text cannot be represented in the target document encoding")
            }
            Self::SearchInput => {
                f.write_str("search input or replacement exceeds its configured budget")
            }
            Self::SearchIncomplete => {
                f.write_str("search is incomplete; wait, restart or refine before replacing")
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
    pub matches: Vec<crate::SearchMatch>,
    pub current: Option<usize>,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub regex: bool,
    pub status: crate::SearchStatus,
    pub target: Option<crate::TaskStamp>,
    pub generation: u64,
    pub scanned_bytes: usize,
    pub history: std::collections::VecDeque<SearchHistoryEntry>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHistoryEntry {
    pub query: String,
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
    text: RefCell<TextProjection>,
    invisible: RefCell<invisible_layout::InvisibleLayout>,
    unicode: RefCell<crate::unicode_layout::UnicodeLayout>,
    syntax: RefCell<syntax::SyntaxCache>,
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
    ShowGoToPosition,
    ShowPalette,
    OpenMenu(CommandId),
    View(CommandId),
    Progress(CommandId),
    NeedsCloseDecision(TabId),
    ExitRequested,
}

#[derive(Debug, Default)]
pub struct Workbench {
    show_invisibles: bool,
    pub(crate) tabs: TabManager,
    panes: PaneManager,
    documents: Vec<EditorDocument>,
    closed_documents: Vec<ClosedDocument>,
    next_document: u64,
    pub(crate) search: SearchState,
    search_work: Option<search::SearchWork>,
    search_pattern: Option<search::CompiledQuery>,
    search_limits: crate::SearchLimits,
    search_navigation: Option<search::PendingNavigation>,
    search_history_index: Option<usize>,
    pub(crate) clipboard: Option<rectangular::Clipboard>,
    pub(crate) task_panels: crate::task_panels::TaskPanels,
}

impl Workbench {
    pub(crate) fn open_documents(&self) -> impl Iterator<Item = &EditorDocument> {
        self.documents.iter()
    }
    pub fn document_by_id(&self, id: DocumentId) -> Option<&EditorDocument> {
        self.documents.iter().find(|document| document.id == id)
    }
    pub fn tabs(&self) -> &TabManager {
        &self.tabs
    }

    pub fn panes(&self) -> &PaneManager {
        &self.panes
    }

    pub fn set_history_budget(
        &mut self,
        id: DocumentId,
        bytes: usize,
    ) -> Result<(), WorkbenchError> {
        let document = self
            .documents
            .iter_mut()
            .find(|document| document.id == id)
            .ok_or(WorkbenchError::NoDocument)?;
        document
            .session
            .document_mut()
            .set_history_bytes_limit(bytes);
        Ok(())
    }

    pub fn document_by_tab(&self, id: TabId) -> Option<&EditorDocument> {
        self.documents.iter().find(|document| document.tab_id == id)
    }

    fn cursor_for_tab(&self, tab: TabId) -> usize {
        self.panes.remembered_cursor(tab).unwrap_or_else(|| {
            self.document_by_tab(tab)
                .map_or(0, |d| session_content_start(&d.session))
        })
    }

    fn with_text<T>(
        &self,
        tab: TabId,
        f: impl FnOnce(&mut TextProjection, &jc_core::Document) -> T,
    ) -> Option<T> {
        let document = self.document_by_tab(tab)?;
        let encoding = document.session.encoding()?;
        let source = document.session.document();
        let mut text = document.text.borrow_mut();
        text.ensure(source, encoding);
        Some(f(&mut text, source))
    }
    fn with_active_text<T>(
        &self,
        f: impl FnOnce(&mut TextProjection, &jc_core::Document) -> T,
    ) -> Result<T, WorkbenchError> {
        let document = self.active_document().ok_or(WorkbenchError::NoDocument)?;
        if document.session.encoding().is_none() {
            return Err(FileError::EncodingRequired.into());
        }
        self.with_text(document.tab_id, f)
            .ok_or(WorkbenchError::NoDocument)
    }
    pub fn position_for_tab(&self, tab: TabId, offset: usize) -> Option<TextPosition> {
        self.with_text(tab, |text, source| text.position(source, offset))?
            .ok()
    }
    pub fn position_for_pane(&self, pane: PaneId) -> Option<TextPosition> {
        let location = self.panes.view(pane)?.location()?;
        self.position_for_tab(location.tab, location.byte_offset)
    }
    pub fn line_count_for_tab(&self, tab: TabId) -> Option<usize> {
        self.with_text(tab, |text, _| text.line_count())
    }
    /// Classify a raw byte offset using line metadata, without scalar layout.
    /// This is not a conversion to a valid caret or a visual/scalar column.
    pub fn line_ordinal_for_byte(&self, tab: TabId, offset: usize) -> Option<usize> {
        self.with_text(tab, |text, _| text.line_ordinal_for_byte(offset))?
    }
    pub fn line_range_for_tab(
        &self,
        tab: TabId,
        line: usize,
    ) -> Option<(Range<usize>, Range<usize>)> {
        self.with_text(tab, |text, source| text.line_range(source, line))?
    }
    pub fn visible_line(
        &self,
        tab: TabId,
        line: usize,
        column: usize,
        width: usize,
    ) -> Option<Vec<(char, usize)>> {
        self.with_text(tab, |text, source| {
            text.visible(source, line, column, width)
        })?
    }
    /// Cumulative raw bytes scanned, bytes decoded and retained projection metadata/cache.
    pub fn text_statistics(&self, tab: TabId) -> Option<(u64, u64, usize)> {
        let document = self.document_by_tab(tab)?;
        let text = document.text.borrow();
        let (invisible_decoded, invisible_memory) = document.invisible.borrow().statistics();
        Some((
            text.scanned_bytes,
            text.decoded_bytes
                .saturating_add(invisible_decoded)
                .saturating_add(document.unicode.borrow().scanned_bytes),
            text.memory_bytes()
                .saturating_add(invisible_memory)
                .saturating_add(document.unicode.borrow().memory_bytes())
                .saturating_add(document.syntax.borrow().memory_bytes()),
        ))
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
            || (document.session.encoding().is_some()
                && self.position_for_tab(tab, offset).is_none())
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
        self.panes.view(pane)?.state()?.selection()
    }

    /// Reveal the caret in the active viewport. Other viewports move only when
    /// linked scrolling is enabled, by the same vertical delta (not caret sync).
    pub fn reveal_active_pane(&mut self, rows: usize, columns: usize) {
        self.clamp_viewport_rows(rows);
        if self
            .panes
            .view(self.panes.active())
            .and_then(|v| v.state())
            .is_some_and(|s| s.manual_scroll)
        {
            return;
        }
        let Some(position) = self.active_position() else {
            return;
        };
        let active = self.panes.active();
        let rectangle = self.rectangle_for_pane(active);
        let caret_column = if self.show_invisibles && rectangle.is_none() {
            self.display_column_for_pane(active).unwrap_or(0)
        } else {
            self.visual_column_for_pane(active)
                .unwrap_or(position.column)
        };
        let view = self.panes.view(active).expect("active view");
        let old_line = view.scroll_line();
        let mut line = old_line;
        let mut column = view.scroll_column();
        if position.line < line {
            line = position.line;
        } else if position.line >= line.saturating_add(rows.max(1)) {
            line = position.line + 1 - rows.max(1);
        }
        if rectangle.is_none() && caret_column < column {
            column = caret_column;
        } else if rectangle.is_none() && caret_column >= column.saturating_add(columns.max(1)) {
            column = caret_column + 1 - columns.max(1);
        }
        self.panes.set_scroll(active, line, column);
        if let Some(rect) = rectangle {
            let mut scroll = rect.scroll_column;
            if rect.head_column < scroll {
                scroll = rect.head_column;
            } else if rect.head_column >= scroll.saturating_add(columns.max(1)) {
                scroll = rect.head_column + 1 - columns.max(1);
            }
            if let Some(state) = self.panes.active_state_mut() {
                if let Some(selection) = &mut state.rectangle {
                    selection.scroll_column = crate::rectangle::VisualColumn(scroll);
                }
            }
        }
        self.link_scroll_delta(active, old_line, line, rows);
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
        if let Some(change) = &change {
            document.syntax.get_mut().invalidate(change, revision);
        }
        if let Some(change) = change {
            self.panes.rebase(tab, &change, false);
        }
    }

    fn normalize_views(&mut self, tab: TabId) {
        if let Some(document) = self.documents.iter().find(|d| d.tab_id == tab) {
            if let Some(encoding) = document.session.encoding() {
                let source = document.session.document();
                let mut text = document.text.borrow_mut();
                text.ensure(source, encoding);
                self.panes
                    .normalize_with(tab, |offset| text.snap(source, offset));
            }
        }
    }

    pub fn search(&self) -> &SearchState {
        &self.search
    }

    pub fn output(&self) -> &[crate::task_panels::PanelRow] {
        self.task_panels.rows(crate::task_panels::PanelKind::Output)
    }

    pub fn problems(&self) -> &[crate::task_panels::PanelRow] {
        self.task_panels
            .rows(crate::task_panels::PanelKind::Problems)
    }
    pub fn task_panels(&self) -> &crate::task_panels::TaskPanels {
        &self.task_panels
    }
    pub fn reset_task_panels(&mut self) {
        self.task_panels.reset();
    }

    pub fn push_output(&mut self, line: impl Into<String>) {
        self.task_panels
            .push(crate::task_panels::PanelKind::Output, line.into());
    }

    pub fn push_problem(&mut self, line: impl Into<String>) {
        self.task_panels
            .push(crate::task_panels::PanelKind::Problems, line.into());
    }

    pub fn clear_problems(&mut self) {
        self.task_panels
            .clear(crate::task_panels::PanelKind::Problems);
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
        self.open_file_with_scope(path, encoding, to_side, None)
    }

    /// Metadata finders open through the same catalog/session path, with a confined
    /// byte read for a new document. Reusing a dirty document never reloads it.
    pub fn open_project_file(
        &mut self,
        root: &Path,
        path: PathBuf,
        encoding: Option<TextEncoding>,
        to_side: bool,
        max_bytes: usize,
    ) -> Result<TabId, WorkbenchError> {
        self.open_file_with_scope(path, encoding, to_side, Some((root, max_bytes, None)))
    }
    pub fn open_project_file_at_version(
        &mut self,
        root: &Path,
        path: PathBuf,
        encoding: Option<TextEncoding>,
        to_side: bool,
        max_bytes: usize,
        expected: jc_files::ContentFingerprint,
    ) -> Result<TabId, WorkbenchError> {
        self.open_file_with_scope(
            path,
            encoding,
            to_side,
            Some((root, max_bytes, Some(expected))),
        )
    }

    fn open_file_with_scope(
        &mut self,
        path: PathBuf,
        encoding: Option<TextEncoding>,
        to_side: bool,
        scope: Option<(&Path, usize, Option<jc_files::ContentFingerprint>)>,
    ) -> Result<TabId, WorkbenchError> {
        if let Ok(canonical) = std::fs::canonicalize(&path) {
            if let Some((root, _, _)) = scope {
                let root = root.canonicalize().map_err(FileError::from)?;
                if !canonical.starts_with(&root) {
                    return Err(FileError::Io(std::io::Error::other(
                        "File is outside the project root",
                    ))
                    .into());
                }
            }
            if let Some(document) = self.documents.iter().find(|document| {
                std::fs::canonicalize(document.session.path()).ok().as_ref() == Some(&canonical)
            }) {
                if let Some((root, limit, Some(expected))) = scope {
                    let source = document.session.document();
                    if source.len() > limit
                        || document.session.encoding() != encoding
                        || jc_files::ContentFingerprint::of_chunks(
                            source
                                .range_chunks(0..source.len())
                                .map_err(FileError::from)?,
                        ) != expected
                    {
                        return Err(FileError::Io(std::io::Error::other("Open document differs from the expected content/encoding; refresh the result")).into());
                    }
                    // Disk-origin results describe disk even after a previous
                    // selection opened the document. Check both before focus.
                    let (_, bytes) =
                        jc_files::read_in_root(root, &path, limit).map_err(FileError::from)?;
                    if jc_files::ContentFingerprint::of(&bytes) != expected {
                        return Err(FileError::Io(std::io::Error::other(
                            "File content differs from the expected version; refresh the result",
                        ))
                        .into());
                    }
                }
                let (tab, cursor) = (document.tab_id, self.cursor_for_tab(document.tab_id));
                if to_side {
                    self.prepare_side_target();
                }
                self.select_opened_tab(tab, cursor, true);
                return Ok(tab);
            }
        }
        let session = match scope {
            Some((root, max_bytes, Some(expected))) => {
                FileSession::open_in_root_at_version(root, &path, encoding, max_bytes, expected)?
            }
            Some((root, max_bytes, None)) => {
                FileSession::open_in_root(root, &path, encoding, max_bytes)?
            }
            None => FileSession::open(&path, encoding)?,
        };
        // Read failures leave the current group, selection and history intact.
        let first_side_open = to_side && self.active_document().is_none();
        if to_side && !first_side_open {
            self.prepare_side_target();
        }
        let cursor = session_content_start(&session);
        let title = title_for_path(&path, "<file>");
        let tab_id = self.tabs.open(title);
        self.next_document += 1;
        self.documents.push(EditorDocument {
            tab_id,
            session,
            id: DocumentId(self.next_document),
            notified_revision: 0,
            text: RefCell::default(),
            invisible: RefCell::default(),
            unicode: RefCell::default(),
            syntax: RefCell::default(),
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
            text: RefCell::default(),
            invisible: RefCell::default(),
            unicode: RefCell::default(),
            syntax: RefCell::default(),
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

    pub fn active_position(&self) -> Option<TextPosition> {
        self.position_for_pane(self.panes.active())
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
                .rectangle_for_pane(self.panes.active())
                .is_some_and(|r| r.left < r.right)
                || self
                    .active_editor()
                    .and_then(|editor| editor.selection())
                    .is_some(),
            clipboard_has_text: self.clipboard.as_ref().is_some_and(|clip| clip.has_text()),
            rectangular_selection: self.rectangle_for_pane(self.panes.active()).is_some(),
            has_project: false,
            document_search_running: self.search_running(),
            search_history_available: !self.search.history.is_empty(),
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
            open_diff_available: self.can_compare_open(),
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
        self.panes.clear_rectangles(tab);
        self.normalize_views(tab);
        self.resume_caret_tracking();
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
        let document = self.active_document().ok_or(WorkbenchError::NoDocument)?;
        let target =
            self.capture_save_target(document.id, document.session.document().revision())?;
        self.save_target(target, Some(path.into()))
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
        self.try_reopen_closed().unwrap_or(false)
    }

    /// Recreate from minimal metadata, never discarded content/undo. Failures
    /// keep both closed stacks and the current document/view untouched.
    pub fn try_reopen_closed(&mut self) -> Result<bool, WorkbenchError> {
        let Some(closed) = self.closed_documents.last() else {
            return Ok(false);
        };
        if self.tabs.last_closed_id() != Some(closed.tab) {
            return Err(
                FileError::Io(std::io::Error::other("Closed-tab history is inconsistent")).into(),
            );
        }
        let canonical = std::fs::canonicalize(&closed.path).ok();
        if self.documents.iter().any(|document| {
            document.session.path() == closed.path
                || canonical.as_ref().is_some_and(|path| {
                    std::fs::canonicalize(document.session.path()).ok().as_ref() == Some(path)
                })
        }) {
            return Err(FileError::Io(std::io::Error::other(
                "Closed path is already open; select its existing tab. Closed history retained",
            ))
            .into());
        }
        let session = if closed.new_file {
            FileSession::new(&closed.path, closed.encoding.unwrap_or(TextEncoding::Utf8))
        } else {
            FileSession::open(&closed.path, closed.encoding)?
        };
        let expected = closed.tab;
        if self.tabs.reopen_last() != Some(expected) {
            unreachable!("closed stacks validated before source admission");
        }
        self.closed_documents.pop();
        let cursor = session_content_start(&session);
        self.next_document += 1;
        self.documents.push(EditorDocument {
            id: DocumentId(self.next_document),
            tab_id: expected,
            session,
            notified_revision: 0,
            text: RefCell::default(),
            invisible: RefCell::default(),
            unicode: RefCell::default(),
            syntax: RefCell::default(),
        });
        self.panes
            .navigate(NavLocation::new(expected, cursor), false);
        let _ = self.rebuild_search();
        Ok(true)
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
        let projection = document.text.get_mut();
        projection.ensure(document.session.document(), encoding);
        let mut replacements = Vec::with_capacity(edits.len());
        for edit in edits {
            for offset in [edit.range.start, edit.range.end] {
                if offset != 0 {
                    projection.position(document.session.document(), offset)?;
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
        if let Some(change) = &change {
            document
                .syntax
                .get_mut()
                .invalidate(change, document.notified_revision);
        }
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

fn session_content_start(session: &FileSession) -> usize {
    if session.encoding() == Some(TextEncoding::Utf8)
        && session
            .document()
            .read_range(0..session.document().len().min(3))
            .is_ok_and(|bytes| bytes == [0xef, 0xbb, 0xbf])
    {
        3
    } else {
        0
    }
}

fn title_for_path(path: &Path, fallback: &str) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(fallback)
        .to_owned()
}
