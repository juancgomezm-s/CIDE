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
    FileOpenFolder,
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

    SearchFind,
    SearchReplace,
    SearchFindNext,
    SearchFindPrevious,
    SearchProject,
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
    ViewDatabase,
    ViewGit,
    ViewSymbols,
    ViewToggleBottomPanel,
    ViewTerminal,
    ViewProblems,
    ViewOutput,
    ViewSearchResults,

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
    pub has_document: bool,
    pub document_dirty: bool,
    pub dirty_documents: usize,
    pub can_undo: bool,
    pub can_redo: bool,
    pub has_selection: bool,
    pub clipboard_has_text: bool,
    pub has_project: bool,
    pub active_language: Option<String>,
    pub has_progress_profile: bool,
    pub has_database_schema: bool,
    pub can_reopen_closed_tab: bool,
    pub terminal_available: bool,
}

impl Default for CommandContext {
    fn default() -> Self {
        Self {
            focus: PrimaryFocus::Editor,
            has_document: false,
            document_dirty: false,
            dirty_documents: 0,
            can_undo: false,
            can_redo: false,
            has_selection: false,
            clipboard_has_text: false,
            has_project: false,
            active_language: None,
            has_progress_profile: false,
            has_database_schema: false,
            can_reopen_closed_tab: false,
            terminal_available: false,
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
fn never(_: &CommandContext) -> bool {
    false
}
fn has_document(ctx: &CommandContext) -> bool {
    ctx.has_document
}
fn dirty_document(ctx: &CommandContext) -> bool {
    ctx.has_document && ctx.document_dirty
}
fn dirty_documents(ctx: &CommandContext) -> bool {
    ctx.dirty_documents > 0
}
fn can_undo(ctx: &CommandContext) -> bool {
    ctx.has_document && ctx.can_undo
}
fn can_redo(ctx: &CommandContext) -> bool {
    ctx.has_document && ctx.can_redo
}
fn selection(ctx: &CommandContext) -> bool {
    ctx.has_document && ctx.has_selection
}
fn paste(ctx: &CommandContext) -> bool {
    ctx.has_document && ctx.clipboard_has_text
}
fn block_comment(ctx: &CommandContext) -> bool {
    ctx.has_document
        && ctx
            .active_language
            .as_deref()
            .and_then(by_id)
            .is_some_and(|profile| profile.block_comment.is_some())
}
fn line_comment(ctx: &CommandContext) -> bool {
    ctx.has_document
        && ctx
            .active_language
            .as_deref()
            .and_then(by_id)
            .is_some_and(|profile| profile.preferred_comment().is_some())
}
fn project(ctx: &CommandContext) -> bool {
    ctx.has_project
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
        spec!(FileOpenFolder, File, "Open Folder…", always),
        spec!(FileSave, File, "Save", dirty_document),
        spec!(FileSaveAs, File, "Save As…", has_document),
        spec!(FileSaveAll, File, "Save All", dirty_documents),
        spec!(FileClose, File, "Close", has_document),
        spec!(FileReopenClosed, File, "Reopen Closed Tab", reopen),
        spec!(FileExit, File, "Exit", always),
        spec!(EditUndo, Edit, "Undo", can_undo),
        spec!(EditRedo, Edit, "Redo", can_redo),
        spec!(EditCut, Edit, "Cut", selection),
        spec!(EditCopy, Edit, "Copy", selection),
        spec!(EditPaste, Edit, "Paste", paste),
        spec!(EditSelectAll, Edit, "Select All", has_document),
        spec!(
            EditDuplicate,
            Edit,
            "Duplicate Selection/Line",
            has_document
        ),
        spec!(EditDeleteLine, Edit, "Delete Line", has_document),
        spec!(EditMoveLineUp, Edit, "Move Line Up", has_document),
        spec!(EditMoveLineDown, Edit, "Move Line Down", has_document),
        spec!(
            EditToggleLineComment,
            Edit,
            "Toggle Comment",
            line_comment
        ),
        spec!(
            EditToggleBlockComment,
            Edit,
            "Toggle Block Comment",
            block_comment
        ),
        spec!(EditIndent, Edit, "Indent", has_document),
        spec!(EditOutdent, Edit, "Outdent", has_document),
        spec!(SearchFind, Search, "Find", has_document),
        spec!(SearchReplace, Search, "Replace", has_document),
        spec!(SearchFindNext, Search, "Find Next", has_document),
        spec!(SearchFindPrevious, Search, "Find Previous", has_document),
        spec!(SearchProject, Search, "Find in Project", project),
        spec!(SearchReplaceProject, Search, "Replace in Project", project),
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
        spec!(ViewDatabase, View, "Database / Schema", never),
        spec!(ViewGit, View, "Git", never),
        spec!(ViewSymbols, View, "Symbols", never),
        spec!(ViewToggleBottomPanel, View, "Toggle Bottom Panel", always),
        spec!(ViewTerminal, View, "Terminal", terminal),
        spec!(ViewProblems, View, "Problems", always),
        spec!(ViewOutput, View, "Output", always),
        spec!(ViewSearchResults, View, "Search Results", always),
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
