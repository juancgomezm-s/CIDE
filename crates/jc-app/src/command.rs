use crate::focus::PrimaryFocus;
use jc_languages::by_id;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CommandCategory {
    File,
    Edit,
    Search,
    Project,
    Build,
    View,
    Navigation,
    Tools,
    Menu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CommandId {
    FileNew,
    FileOpen,
    FileQuickOpen,
    FileOpenFolder,
    FileChooseEncoding,
    FileSave,
    FileSaveAs,
    FileSaveAll,
    FileClose,
    FileReopenClosed,
    FileExit,

    EditUndo,
    EditRedo,
    EditCut,
    EditCopy,
    EditPaste,
    EditSelectAll,
    EditDuplicate,
    EditDeleteLine,
    EditMoveLineUp,
    EditMoveLineDown,
    EditToggleLineComment,
    EditToggleBlockComment,
    EditIndent,
    EditOutdent,
    EditJoinLines,
    EditSplitLine,
    EditRectangleStart,
    EditRectangleLeft,
    EditRectangleRight,
    EditRectangleUp,
    EditRectangleDown,
    EditRectangleCancel,

    SearchFind,
    SearchReplace,
    SearchFindNext,
    SearchFindPrevious,
    SearchSelectionNext,
    SearchSelectionPrevious,
    SearchGoToPosition,
    SearchCancel,
    SearchHistoryPrevious,
    SearchHistoryNext,
    SearchProject,
    SearchCancelProject,
    SearchReplaceProject,
    SearchToggleCase,
    SearchToggleWholeWord,
    SearchToggleRegex,

    PaletteOpen,

    TabNext,
    TabPrevious,
    Tab1,
    Tab2,
    Tab3,
    Tab4,
    Tab5,
    Tab6,
    Tab7,
    Tab8,
    Tab9,

    ViewExplorer,
    ExplorerRefresh,
    ExplorerCancel,
    ViewToggleInvisibles,
    ViewDatabase,
    ViewGit,
    GitDiffHead,
    GitDiffIndex,
    GitDiffRefresh,
    GitDiffCancel,
    GitConfigureExecutable,
    GitRepositoryStatus,
    GitStatusRefresh,
    GitStatusCancel,
    ViewSymbols,
    ViewToggleBottomPanel,
    ViewTerminal,
    ViewProblems,
    ViewOutput,
    ViewSearchResults,
    EditorSplitRight,
    EditorOneGroup,
    EditorFocusPrimary,
    EditorFocusSecondary,
    EditorTogglePin,
    EditorToggleScrollLink,
    EditorOpenToSide,
    NavigateBack,
    NavigateForward,

    FocusNext,
    FocusPrevious,

    ProgressCheck,
    ProgressCompile,
    ProgressRun,
    ProgressRefreshSchema,

    MenuFile,
    MenuEdit,
    MenuSearch,
    MenuProject,
    MenuBuild,
    MenuTools,
    MenuView,
    MenuHelp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandContext {
    pub focus: PrimaryFocus,
    /// The frontend's focused control targets source editing (including an
    /// editor-invoked menu/palette), rather than a modal field or read model.
    pub editor_input: bool,
    pub has_document: bool,
    pub document_dirty: bool,
    pub dirty_documents: usize,
    pub can_undo: bool,
    pub can_redo: bool,
    pub has_selection: bool,
    pub rectangular_selection: bool,
    pub clipboard_has_text: bool,
    pub has_project: bool,
    pub explorer_focused: bool,
    pub project_search_running: bool,
    pub project_replace_ready: bool,
    pub document_search_running: bool,
    pub search_history_available: bool,
    pub active_language: Option<String>,
    pub has_progress_profile: bool,
    pub has_database_schema: bool,
    pub can_reopen_closed_tab: bool,
    pub terminal_available: bool,
    pub git_diff_available: bool,
    pub git_diff_running: bool,
    pub git_repository_available: bool,
    pub git_status_running: bool,
    // Shared pane navigation state; keymap handlers are connected by the frontend.
    pub split_editor: bool,
    pub reference_pinned: bool,
    pub nav_back_available: bool,
    pub nav_forward_available: bool,
    pub can_open_to_side: bool,
}

impl Default for CommandContext {
    fn default() -> Self {
        Self {
            focus: PrimaryFocus::Editor,
            editor_input: true,
            has_document: false,
            document_dirty: false,
            dirty_documents: 0,
            can_undo: false,
            can_redo: false,
            has_selection: false,
            rectangular_selection: false,
            clipboard_has_text: false,
            has_project: false,
            explorer_focused: false,
            project_search_running: false,
            project_replace_ready: false,
            document_search_running: false,
            search_history_available: false,
            active_language: None,
            has_progress_profile: false,
            has_database_schema: false,
            can_reopen_closed_tab: false,
            terminal_available: false,
            git_diff_available: false,
            git_diff_running: false,
            git_repository_available: false,
            git_status_running: false,
            split_editor: false,
            reference_pinned: false,
            nav_back_available: false,
            nav_forward_available: false,
            can_open_to_side: false,
        }
    }
}

impl CommandContext {
    pub fn is_abl(&self) -> bool {
        self.active_language
            .as_deref()
            .is_some_and(|id| id.eq_ignore_ascii_case("abl"))
    }
}

pub type EnablePredicate = fn(&CommandContext) -> bool;

#[derive(Clone, Copy)]
pub struct CommandSpec {
    pub id: CommandId,
    pub category: CommandCategory,
    pub label: &'static str,
    pub enabled: EnablePredicate,
}

impl std::fmt::Debug for CommandSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandSpec")
            .field("id", &self.id)
            .field("category", &self.category)
            .field("label", &self.label)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
pub struct CommandRegistry {
    specs: BTreeMap<CommandId, CommandSpec>,
}

impl CommandRegistry {
    pub fn new(specs: impl IntoIterator<Item = CommandSpec>) -> Result<Self, CommandId> {
        let mut map = BTreeMap::new();
        for spec in specs {
            if map.insert(spec.id, spec).is_some() {
                return Err(spec.id);
            }
        }
        Ok(Self { specs: map })
    }

    pub fn get(&self, id: CommandId) -> Option<&CommandSpec> {
        self.specs.get(&id)
    }

    pub fn contains(&self, id: CommandId) -> bool {
        self.specs.contains_key(&id)
    }

    pub fn is_enabled(&self, id: CommandId, context: &CommandContext) -> bool {
        self.get(id).is_some_and(|spec| (spec.enabled)(context))
    }

    pub fn iter(&self) -> impl Iterator<Item = &CommandSpec> {
        self.specs.values()
    }
}

fn always(_: &CommandContext) -> bool {
    true
}
fn document_search_running(ctx: &CommandContext) -> bool {
    ctx.document_search_running || ctx.rectangular_selection
}
fn search_history(ctx: &CommandContext) -> bool {
    ctx.has_document && ctx.search_history_available
}
fn never(_: &CommandContext) -> bool {
    false
}
fn has_document(ctx: &CommandContext) -> bool {
    ctx.has_document
}
fn split(ctx: &CommandContext) -> bool {
    ctx.has_document && ctx.split_editor
}
fn nav_back(ctx: &CommandContext) -> bool {
    ctx.nav_back_available
}
fn nav_forward(ctx: &CommandContext) -> bool {
    ctx.nav_forward_available
}
fn open_to_side(ctx: &CommandContext) -> bool {
    ctx.can_open_to_side
}
fn dirty_document(ctx: &CommandContext) -> bool {
    ctx.has_document && ctx.document_dirty
}
fn dirty_documents(ctx: &CommandContext) -> bool {
    ctx.dirty_documents > 0
}
fn can_undo(ctx: &CommandContext) -> bool {
    rectangle_editor(ctx) && ctx.can_undo
}
fn can_redo(ctx: &CommandContext) -> bool {
    rectangle_editor(ctx) && ctx.can_redo
}
fn rectangle_editor(ctx: &CommandContext) -> bool {
    ctx.has_document && ctx.focus == PrimaryFocus::Editor && ctx.editor_input
}
fn rectangle_active(ctx: &CommandContext) -> bool {
    rectangle_editor(ctx) && ctx.rectangular_selection
}
fn linear_document(ctx: &CommandContext) -> bool {
    ctx.has_document && !ctx.rectangular_selection
}
fn linear_editor(ctx: &CommandContext) -> bool {
    linear_document(ctx) && ctx.focus == PrimaryFocus::Editor && ctx.editor_input
}
fn source_selection(ctx: &CommandContext) -> bool {
    linear_editor(ctx) && ctx.has_selection
}
fn paste(ctx: &CommandContext) -> bool {
    rectangle_editor(ctx) && ctx.clipboard_has_text
}
fn source_clipboard_selection(ctx: &CommandContext) -> bool {
    rectangle_editor(ctx) && ctx.has_selection
}
fn block_comment(ctx: &CommandContext) -> bool {
    linear_editor(ctx)
        && ctx
            .active_language
            .as_deref()
            .and_then(by_id)
            .is_some_and(|profile| profile.block_comment.is_some())
}
fn line_comment(ctx: &CommandContext) -> bool {
    linear_editor(ctx)
        && ctx
            .active_language
            .as_deref()
            .and_then(by_id)
            .is_some_and(|profile| profile.preferred_comment().is_some())
}
fn project(ctx: &CommandContext) -> bool {
    ctx.has_project
}
fn explorer(ctx: &CommandContext) -> bool {
    ctx.has_project && ctx.explorer_focused
}
fn explorer_focus(ctx: &CommandContext) -> bool {
    ctx.explorer_focused
}
fn git_diff(ctx: &CommandContext) -> bool {
    ctx.git_diff_available
}
fn git_diff_running(ctx: &CommandContext) -> bool {
    ctx.git_diff_running
}
fn git_repository(ctx: &CommandContext) -> bool {
    ctx.git_repository_available
}
fn git_status_running(ctx: &CommandContext) -> bool {
    ctx.git_status_running
}
fn project_search_running(ctx: &CommandContext) -> bool {
    ctx.project_search_running
}
fn project_replace_ready(ctx: &CommandContext) -> bool {
    ctx.project_replace_ready
}
fn reopen(ctx: &CommandContext) -> bool {
    ctx.can_reopen_closed_tab
}
fn terminal(ctx: &CommandContext) -> bool {
    ctx.terminal_available
}
fn abl(ctx: &CommandContext) -> bool {
    ctx.has_document && ctx.is_abl() && ctx.has_progress_profile
}
fn abl_project(ctx: &CommandContext) -> bool {
    ctx.has_project && ctx.has_progress_profile
}

macro_rules! spec {
    ($id:ident, $cat:ident, $label:literal, $enabled:ident) => {
        CommandSpec {
            id: CommandId::$id,
            category: CommandCategory::$cat,
            label: $label,
            enabled: $enabled,
        }
    };
}

pub fn default_registry() -> CommandRegistry {
    let specs: Vec<CommandSpec> = vec![
        spec!(FileNew, File, "New", always),
        spec!(FileOpen, File, "Open…", always),
        spec!(FileQuickOpen, File, "Quick Open Project File", project),
        spec!(FileOpenFolder, File, "Open Folder…", always),
        spec!(
            FileChooseEncoding,
            File,
            "Choose Source Encoding…",
            has_document
        ),
        spec!(FileSave, File, "Save", dirty_document),
        spec!(FileSaveAs, File, "Save As…", has_document),
        spec!(FileSaveAll, File, "Save All", dirty_documents),
        spec!(FileClose, File, "Close", has_document),
        spec!(FileReopenClosed, File, "Reopen Closed Tab", reopen),
        spec!(FileExit, File, "Exit", always),
        spec!(EditUndo, Edit, "Undo", can_undo),
        spec!(EditRedo, Edit, "Redo", can_redo),
        spec!(EditCut, Edit, "Cut", source_clipboard_selection),
        spec!(EditCopy, Edit, "Copy", source_clipboard_selection),
        spec!(EditPaste, Edit, "Paste", paste),
        spec!(EditSelectAll, Edit, "Select All", rectangle_editor),
        spec!(
            EditDuplicate,
            Edit,
            "Duplicate Selection/Line",
            linear_editor
        ),
        spec!(EditDeleteLine, Edit, "Delete Line", linear_editor),
        spec!(EditMoveLineUp, Edit, "Move Line Up", linear_editor),
        spec!(EditMoveLineDown, Edit, "Move Line Down", linear_editor),
        spec!(EditToggleLineComment, Edit, "Toggle Comment", line_comment),
        spec!(
            EditToggleBlockComment,
            Edit,
            "Toggle Block Comment",
            block_comment
        ),
        spec!(EditIndent, Edit, "Indent", rectangle_editor),
        spec!(EditOutdent, Edit, "Outdent", linear_editor),
        spec!(EditJoinLines, Edit, "Join Lines", linear_editor),
        spec!(EditSplitLine, Edit, "Split Line", linear_editor),
        spec!(
            EditRectangleStart,
            Edit,
            "Rectangle: Select From Caret/Selection (single-cell text)",
            rectangle_editor
        ),
        spec!(
            EditRectangleLeft,
            Edit,
            "Rectangle: Extend Left",
            rectangle_editor
        ),
        spec!(
            EditRectangleRight,
            Edit,
            "Rectangle: Extend Right",
            rectangle_editor
        ),
        spec!(
            EditRectangleUp,
            Edit,
            "Rectangle: Extend Up",
            rectangle_editor
        ),
        spec!(
            EditRectangleDown,
            Edit,
            "Rectangle: Extend Down",
            rectangle_editor
        ),
        spec!(
            EditRectangleCancel,
            Edit,
            "Rectangle: Cancel",
            rectangle_active
        ),
        spec!(SearchFind, Search, "Find", has_document),
        spec!(SearchReplace, Search, "Replace", has_document),
        spec!(SearchFindNext, Search, "Find Next", has_document),
        spec!(SearchFindPrevious, Search, "Find Previous", has_document),
        spec!(
            SearchSelectionNext,
            Search,
            "Find Selection Next",
            source_selection
        ),
        spec!(
            SearchSelectionPrevious,
            Search,
            "Find Selection Previous",
            source_selection
        ),
        spec!(
            SearchGoToPosition,
            Search,
            "Go to Line / Column",
            has_document
        ),
        spec!(
            SearchCancel,
            Search,
            "Cancel Document Search / Rectangle",
            document_search_running
        ),
        spec!(
            SearchHistoryPrevious,
            Search,
            "Previous Search Query",
            search_history
        ),
        spec!(
            SearchHistoryNext,
            Search,
            "Next Search Query",
            search_history
        ),
        spec!(SearchProject, Search, "Find in Project", project),
        spec!(
            SearchReplaceProject,
            Search,
            "Replace in Project",
            project_replace_ready
        ),
        spec!(
            SearchCancelProject,
            Search,
            "Cancel Project Search",
            project_search_running
        ),
        spec!(SearchToggleCase, Search, "Match Case", always),
        spec!(SearchToggleWholeWord, Search, "Whole Word", always),
        spec!(SearchToggleRegex, Search, "Regular Expression", never),
        spec!(PaletteOpen, Tools, "Command Palette…", always),
        spec!(TabNext, Navigation, "Next Tab", has_document),
        spec!(TabPrevious, Navigation, "Previous Tab", has_document),
        spec!(Tab1, Navigation, "Tab 1", has_document),
        spec!(Tab2, Navigation, "Tab 2", has_document),
        spec!(Tab3, Navigation, "Tab 3", has_document),
        spec!(Tab4, Navigation, "Tab 4", has_document),
        spec!(Tab5, Navigation, "Tab 5", has_document),
        spec!(Tab6, Navigation, "Tab 6", has_document),
        spec!(Tab7, Navigation, "Tab 7", has_document),
        spec!(Tab8, Navigation, "Tab 8", has_document),
        spec!(Tab9, Navigation, "Last Tab", has_document),
        spec!(ViewExplorer, View, "Explorer", always),
        spec!(
            ExplorerRefresh,
            View,
            "Explorer: Refresh Directory",
            explorer
        ),
        spec!(
            ExplorerCancel,
            View,
            "Explorer: Cancel Loading / Return to Editor",
            explorer_focus
        ),
        spec!(
            ViewToggleInvisibles,
            View,
            "Show Invisible Characters",
            always
        ),
        spec!(ViewDatabase, View, "Database / Schema", never),
        spec!(ViewGit, View, "Git: Active File Diff", git_diff),
        spec!(GitDiffHead, View, "Git: Compare HEAD Snapshot", git_diff),
        spec!(GitDiffIndex, View, "Git: Compare Index Snapshot", git_diff),
        spec!(GitDiffRefresh, View, "Git: Refresh Snapshot", git_diff),
        spec!(GitDiffCancel, View, "Git: Cancel Diff", git_diff_running),
        spec!(
            GitConfigureExecutable,
            View,
            "Git: Configure Executable…",
            git_repository
        ),
        spec!(
            GitRepositoryStatus,
            View,
            "Git: Repository Status (Read-Only)",
            git_repository
        ),
        spec!(
            GitStatusRefresh,
            View,
            "Git: Refresh Repository Status",
            git_repository
        ),
        spec!(
            GitStatusCancel,
            View,
            "Git: Cancel Repository Status",
            git_status_running
        ),
        spec!(ViewSymbols, View, "Symbols", never),
        spec!(ViewToggleBottomPanel, View, "Toggle Bottom Panel", always),
        spec!(ViewTerminal, View, "Terminal", terminal),
        spec!(ViewProblems, View, "Problems", always),
        spec!(ViewOutput, View, "Output", always),
        spec!(ViewSearchResults, View, "Search Results", always),
        spec!(EditorSplitRight, View, "Split Right", has_document),
        spec!(EditorOneGroup, View, "One Group", split),
        spec!(
            EditorFocusPrimary,
            Navigation,
            "Focus Group 1",
            has_document
        ),
        spec!(EditorFocusSecondary, Navigation, "Focus Group 2", split),
        spec!(EditorTogglePin, View, "Pin / Unpin Reference", split),
        spec!(EditorToggleScrollLink, View, "Link / Unlink Scroll", split),
        spec!(EditorOpenToSide, Navigation, "Open to Side", open_to_side),
        spec!(NavigateBack, Navigation, "Navigate Back", nav_back),
        spec!(NavigateForward, Navigation, "Navigate Forward", nav_forward),
        spec!(FocusNext, Navigation, "Focus Next Area", always),
        spec!(FocusPrevious, Navigation, "Focus Previous Area", always),
        spec!(ProgressCheck, Build, "Check Syntax", abl),
        spec!(ProgressCompile, Build, "Compile", abl),
        spec!(ProgressRun, Build, "Run", abl),
        spec!(
            ProgressRefreshSchema,
            Build,
            "Refresh Database Schema",
            abl_project
        ),
        spec!(MenuFile, Menu, "File Menu", always),
        spec!(MenuEdit, Menu, "Edit Menu", always),
        spec!(MenuSearch, Menu, "Search Menu", always),
        spec!(MenuProject, Menu, "Project Menu", always),
        spec!(MenuBuild, Menu, "Build Menu", always),
        spec!(MenuTools, Menu, "Tools Menu", always),
        spec!(MenuView, Menu, "View Menu", always),
        spec!(MenuHelp, Menu, "Help Menu", always),
    ];
    CommandRegistry::new(specs).expect("built-in command ids are unique")
}
