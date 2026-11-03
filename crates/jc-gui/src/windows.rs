mod bottom_paint;
mod bottom_panel;
mod bottom_smoke;
mod clipboard;
mod clipboard_flow;
mod close_flow;
mod explorer_paint;
mod explorer_panel;
mod explorer_smoke;
mod ffi;
mod folder;
mod git_panel;
mod git_smoke;
mod git_view;
mod interaction_smoke;
mod line_edit;
mod line_edit_smoke;
mod menu_input;
mod menu_invoker_smoke;
mod native_menu;
mod navigation_paint;
mod navigation_panel;
mod navigation_smoke;
mod notify;
mod paint;
mod parity_smoke;
mod pointer_input;
mod project_paint;
mod project_panel;
mod project_results;
mod project_smoke;
mod rectangle_input;
mod rectangle_smoke;
mod replace_panel;
mod replace_smoke;
mod replace_view;
mod save_flow;
mod save_plan;
mod save_smoke;
mod search_paint;
mod search_panel;
mod selection_reopen;
mod selection_reopen_smoke;
mod selector;
mod selector_flow;
mod selector_input;
mod selector_input_smoke;
mod selector_paint;
mod smoke;
mod syntax_smoke;
use crate::GuiOptions;
use ffi::*;
use jc_app::quick_open::QuickOpenController;
use jc_app::{
    default_keymap, default_registry, CommandId, CommandOutcome, CommandRegistry, Key, KeyMap,
    KeyScope, KeyStroke, Modifiers, PrimaryFocus, Resolution, Wakeup, Workbench,
};
use jc_text::TextEncoding;
use selector::{Selector, SelectorKind};
use std::os::windows::ffi::OsStringExt;
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    ptr::null_mut,
    sync::Arc,
};

pub(super) fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
fn os_error(operation: &str) -> String {
    format!("{operation}: {}", std::io::Error::last_os_error())
}

const OPEN_SIDE: usize = 190;
const COMMANDS: &[(usize, CommandId, &str)] = &[
    (
        210,
        CommandId::ViewToggleBottomPanel,
        "Toggle &Bottom Panel",
    ),
    (211, CommandId::ViewProblems, "&Problems"),
    (212, CommandId::ViewOutput, "&Output"),
    (
        200,
        CommandId::EditRectangleStart,
        "Rectangle: &Start From Caret / Selection",
    ),
    (201, CommandId::EditRectangleCancel, "Rectangle: &Cancel"),
    (202, CommandId::EditRectangleLeft, "Rectangle: Extend &Left"),
    (
        203,
        CommandId::EditRectangleRight,
        "Rectangle: Extend &Right",
    ),
    (204, CommandId::EditRectangleUp, "Rectangle: Extend &Up"),
    (205, CommandId::EditRectangleDown, "Rectangle: Extend &Down"),
    (
        160,
        CommandId::SearchGoToPosition,
        "&Go to Line / Column...",
    ),
    (161, CommandId::NavigateBack, "Navigate &Back"),
    (162, CommandId::NavigateForward, "Navigate &Forward"),
    (
        163,
        CommandId::SearchHistoryPrevious,
        "&Previous Search Query",
    ),
    (164, CommandId::SearchHistoryNext, "&Next Search Query"),
    (138, CommandId::SearchToggleCase, "Toggle &Case Sensitive"),
    (139, CommandId::SearchToggleWholeWord, "Toggle &Whole Word"),
    (140, CommandId::SearchToggleRegex, "Regular E&xpression"),
    (101, CommandId::FileNew, "&New"),
    (102, CommandId::FileOpen, "&Open..."),
    (103, CommandId::FileSave, "&Save"),
    (104, CommandId::FileSaveAs, "Save &As..."),
    (105, CommandId::FileClose, "&Close Tab"),
    (106, CommandId::FileExit, "E&xit"),
    (107, CommandId::FileOpenFolder, "Open &Folder..."),
    (108, CommandId::FileQuickOpen, "&Quick Open..."),
    (
        109,
        CommandId::FileChooseEncoding,
        "Choose Source &Encoding...",
    ),
    (110, CommandId::EditUndo, "&Undo"),
    (111, CommandId::EditRedo, "&Redo"),
    (112, CommandId::EditCut, "Cu&t"),
    (113, CommandId::EditCopy, "&Copy"),
    (114, CommandId::EditPaste, "&Paste"),
    (115, CommandId::EditSelectAll, "Select &All"),
    (116, CommandId::EditJoinLines, "&Join Lines"),
    (117, CommandId::EditSplitLine, "&Split Line"),
    (170, CommandId::EditDuplicate, "&Duplicate Selection / Line"),
    (171, CommandId::EditDeleteLine, "De&lete Line"),
    (172, CommandId::EditMoveLineUp, "Move Line &Up"),
    (173, CommandId::EditMoveLineDown, "Move Line Do&wn"),
    (174, CommandId::EditToggleLineComment, "Toggle &Comment"),
    (
        175,
        CommandId::EditToggleBlockComment,
        "Toggle &Block Comment",
    ),
    (176, CommandId::EditIndent, "&Indent"),
    (177, CommandId::EditOutdent, "&Outdent"),
    (
        178,
        CommandId::EditorToggleScrollLink,
        "Link / Unlink &Scroll",
    ),
    (118, CommandId::FileSaveAll, "Save A&ll..."),
    (119, CommandId::FileReopenClosed, "Reopen Closed &Tab"),
    (194, CommandId::SearchSelectionNext, "Find Selection Ne&xt"),
    (
        195,
        CommandId::SearchSelectionPrevious,
        "Find Selection Pre&vious",
    ),
    (
        196,
        CommandId::SearchCancel,
        "Cancel Document S&earch / Rectangle",
    ),
    (180, CommandId::Tab1, "Tab &1"),
    (181, CommandId::Tab2, "Tab &2"),
    (182, CommandId::Tab3, "Tab &3"),
    (183, CommandId::Tab4, "Tab &4"),
    (184, CommandId::Tab5, "Tab &5"),
    (185, CommandId::Tab6, "Tab &6"),
    (186, CommandId::Tab7, "Tab &7"),
    (187, CommandId::Tab8, "Tab &8"),
    (188, CommandId::Tab9, "&Last Tab (Alt+9)"),
    (120, CommandId::EditorSplitRight, "Split &Right"),
    (121, CommandId::EditorOneGroup, "&One Group"),
    (122, CommandId::EditorFocusPrimary, "Focus Group &1"),
    (123, CommandId::EditorFocusSecondary, "Focus Group &2"),
    (124, CommandId::TabNext, "&Next Tab"),
    (125, CommandId::TabPrevious, "&Previous Tab"),
    (126, CommandId::PaletteOpen, "Command &Palette..."),
    (
        127,
        CommandId::ViewToggleInvisibles,
        "Show &Invisible Characters",
    ),
    (130, CommandId::SearchFind, "&Find Multiline..."),
    (128, CommandId::ViewExplorer, "Project E&xplorer"),
    (129, CommandId::EditorTogglePin, "&Pin / Unpin Reference"),
    (150, CommandId::ExplorerRefresh, "&Refresh Directory"),
    (
        151,
        CommandId::ExplorerCancel,
        "Cancel Loading / Return to &Editor",
    ),
    (152, CommandId::EditorOpenToSide, "Open to &Side"),
    (153, CommandId::FocusNext, "&Next Focus"),
    (154, CommandId::FocusPrevious, "&Previous Focus"),
    (131, CommandId::SearchReplace, "&Replace Multiline..."),
    (132, CommandId::SearchFindNext, "Find &Next"),
    (133, CommandId::SearchFindPrevious, "Find &Previous"),
    (
        134,
        CommandId::SearchProject,
        "Search Plus: Find in &Project...",
    ),
    (
        135,
        CommandId::SearchCancelProject,
        "&Cancel Project Search",
    ),
    (136, CommandId::ViewSearchResults, "Project Search &Results"),
    (
        137,
        CommandId::SearchReplaceProject,
        "Replace in Pro&ject...",
    ),
    (
        140,
        CommandId::GitRepositoryStatus,
        "Repository &Status (Read-Only)",
    ),
    (141, CommandId::ViewGit, "Active File &Diff"),
    (142, CommandId::GitDiffHead, "Compare &HEAD Snapshot"),
    (143, CommandId::GitDiffIndex, "Compare &Index Snapshot"),
    (144, CommandId::GitDiffRefresh, "&Refresh Active Diff"),
    (145, CommandId::GitDiffCancel, "&Cancel Diff"),
    (
        146,
        CommandId::GitStatusRefresh,
        "Refresh Repository S&tatus",
    ),
    (147, CommandId::GitStatusCancel, "Cancel Repositor&y Status"),
    (
        148,
        CommandId::GitConfigureExecutable,
        "Choose Git &Executable...",
    ),
];
pub(super) fn command_id(command: CommandId) -> usize {
    COMMANDS
        .iter()
        .find(|(_, c, _)| *c == command)
        .map_or(0, |item| item.0)
}

enum Action {
    None,
    CapturePointer,
    NativeMenu(native_menu::NativeMenuGroup),
    Clipboard(CommandId),
    PasteSearch(u64),
    PasteNavigation(u64),
    SelectorClipboard(CommandId, Box<selector_input::SelectorInputTarget>),
    PasteProject(u64),
    PasteReplacement(u64),
    ConfigureGit(git_panel::GitIntent),
    ChooseFolder,
    Open(bool),
    SaveAs,
    SaveAll,
    CloseTab,
    Exit,
}
struct State {
    pub workbench: Workbench,
    registry: CommandRegistry,
    keymap: KeyMap,
    encoding: Option<TextEncoding>,
    project_root: Option<PathBuf>,
    explorer: jc_app::explorer::ProjectExplorer,
    explorer_view: jc_app::explorer::ExplorerView,
    explorer_visible: bool,
    explorer_focused: bool,
    explorer_hits: Option<explorer_panel::ExplorerHits>,
    bottom: bottom_panel::BottomState,
    bottom_hits: Option<bottom_panel::BottomHits>,
    rectangle_drag: Option<rectangle_input::RectangleDrag>,
    linear_drag: Option<pointer_input::LinearDrag>,
    wheel: Option<pointer_input::WheelState>,
    menu_stamp: Option<Box<menu_input::MenuStamp>>,
    interaction_epoch: u64,
    smoke_wheel_lines: Option<u32>,
    smoke_menu_choices: std::collections::VecDeque<Option<usize>>,
    selector: Option<Selector>,
    selector_generation: u64,
    selector_explorer_focus: bool,
    selector_search_panel: Option<search_panel::SearchPanel>,
    selector_editor_input: bool,
    selector_edit_target: Option<line_edit::EditTarget>,
    search_panel: Option<search_panel::SearchPanel>,
    navigation_panel: Option<navigation_panel::NavigationPanel>,
    navigation_generation: u64,
    project_panel: Option<project_panel::ProjectPanel>,
    project_search: jc_app::ProjectSearch,
    project_options: Option<jc_app::ProjectSearchOptions>,
    project_workers: usize,
    replace_panel: Option<replace_panel::ReplacePanel>,
    replace_generation: u64,
    git_executable: Option<PathBuf>,
    git_context: Option<(PathBuf, PathBuf)>,
    git_last_intent: git_panel::GitIntent,
    git_source: jc_app::git_read::GitBlobSource,
    git_panel: Option<git_panel::GitIntent>,
    git_view: git_view::GitView,
    git_diff: jc_app::git_diff::GitDiffController,
    git_status: jc_app::git_status_tasks::GitStatusController,
    git_status_options: Option<jc_app::git_status_tasks::GitStatusOptions>,
    jobs: jc_app::JobSystem,
    encoding_target: Option<(jc_app::DocumentId, u64, Option<TextEncoding>)>,
    smoke_clipboard: Option<String>,
    quick_open: QuickOpenController,
    wakeup: Wakeup,
    bridge: Arc<notify::Bridge>,
    deliveries: usize,
    pub status: String,
    pub width: i32,
    pub height: i32,
    pub cell_width: i32,
    pub cell_height: i32,
    pub font: Handle,
    owned_font: bool,
    pub focused: bool,
    pending_surrogate: Option<u16>,
    next_untitled: usize,
    pub paint_count: usize,
    pub paint_errors: usize,
    syntax_pending: bool,
    syntax_targets: [Option<(jc_app::PaneId, jc_app::TabId)>; 2],
    pub save_count: usize,
    pub smoke: bool,
    smoke_root: Option<PathBuf>,
    smoke_save_paths: std::collections::VecDeque<Option<PathBuf>>,
    smoke_close_decisions: std::collections::VecDeque<bool>,
    save_all_active: bool,
    pub key_override: Option<Modifiers>,
}
impl Drop for State {
    fn drop(&mut self) {
        if self.owned_font {
            unsafe {
                DeleteObject(self.font);
            }
        }
    }
}
impl State {
    fn new(options: &GuiOptions) -> Result<Self, String> {
        let bridge = Arc::new(notify::Bridge::default());
        let callback = bridge.clone();
        let wakeup = Wakeup::with_notifier(move || callback.signal());
        let face = wide("Consolas");
        let created =
            unsafe { CreateFontW(-17, 0, 0, 0, 400, 0, 0, 0, 1, 0, 0, 5, 0x31, face.as_ptr()) };
        let font = if created.is_null() {
            unsafe { GetStockObject(16) }
        } else {
            created
        };
        if font.is_null() {
            return Err(os_error("Create native fixed font"));
        }
        let mut state = Self {
            workbench: Workbench::default(),
            registry: default_registry(),
            keymap: default_keymap(),
            encoding: options.encoding,
            project_root: None,
            explorer: jc_app::explorer::ProjectExplorer::new(wakeup.clone(), Default::default())
                .map_err(|e| e.to_string())?,
            explorer_view: Default::default(),
            explorer_visible: false,
            explorer_focused: false,
            explorer_hits: None,
            bottom: Default::default(),
            bottom_hits: None,
            rectangle_drag: None,
            linear_drag: None,
            wheel: None,
            menu_stamp: None,
            interaction_epoch: 0,
            smoke_wheel_lines: None,
            smoke_menu_choices: Default::default(),
            selector: None,
            selector_generation: 0,
            selector_explorer_focus: false,
            selector_search_panel: None,
            selector_editor_input: false,
            selector_edit_target: None,
            search_panel: None,
            navigation_panel: None,
            navigation_generation: 0,
            project_panel: None,
            project_search: jc_app::ProjectSearch::new(wakeup.clone()),
            project_options: None,
            project_workers: options.search_workers.unwrap_or(2),
            replace_panel: None,
            replace_generation: 0,
            git_executable: options.git_executable.clone(),
            git_context: None,
            git_last_intent: git_panel::GitIntent::Status,
            git_source: jc_app::git_read::GitBlobSource::Head,
            git_panel: None,
            git_view: Default::default(),
            git_diff: jc_app::git_diff::GitDiffController::new(),
            git_status: jc_app::git_status_tasks::GitStatusController::new(wakeup.clone()),
            git_status_options: None,
            jobs: jc_app::JobSystem::new(Default::default(), wakeup.clone())
                .map_err(|e| format!("Native task admission: {e:?}"))?,
            encoding_target: None,
            smoke_clipboard: None,
            quick_open: QuickOpenController::new(wakeup.clone()),
            wakeup,
            bridge,
            deliveries: 0,
            status: "Native Win32/GDI · scalar cells · Windows text clipboard".into(),
            width: 1000,
            height: 650,
            cell_width: 9,
            cell_height: 20,
            font,
            owned_font: !created.is_null(),
            focused: false,
            pending_surrogate: None,
            next_untitled: 0,
            paint_count: 0,
            paint_errors: 0,
            syntax_pending: false,
            syntax_targets: [None, None],
            save_count: 0,
            smoke: options.smoke.is_some(),
            smoke_root: options
                .smoke
                .as_ref()
                .and_then(|s| s.fixture.parent())
                .and_then(|p| p.canonicalize().ok()),
            smoke_save_paths: Default::default(),
            smoke_close_decisions: Default::default(),
            save_all_active: false,
            key_override: None,
        };
        if let Some(root) = options
            .smoke
            .as_ref()
            .and_then(|s| s.fixture.parent())
            .or(options.project_root.as_deref())
        {
            state.set_project_root(root.to_owned())?;
        }
        if let Some(smoke) = &options.smoke {
            state
                .workbench
                .open_file(&smoke.fixture, Some(TextEncoding::Utf8))
                .map_err(|e| e.to_string())?;
        } else {
            for path in &options.paths {
                state
                    .workbench
                    .open_file(path, options.encoding)
                    .map_err(|e| e.to_string())?;
            }
            if options.paths.is_empty() {
                state.new_document();
            }
        }
        state.ensure_encoding_selector();
        if options.project_root.is_some() {
            state.explorer_visible = true;
        }
        Ok(state)
    }
    fn new_document(&mut self) {
        self.bottom.focused = false;
        self.explorer_focused = false;
        self.next_untitled += 1;
        self.workbench.new_file(
            format!("untitled-{}.txt", self.next_untitled),
            self.encoding.unwrap_or(TextEncoding::Utf8),
        );
    }
    fn error(&mut self, error: impl std::fmt::Display) {
        self.status = error.to_string().chars().take(240).collect();
    }
    fn context(&self) -> jc_app::CommandContext {
        let mut context = self.workbench.context();
        let explorer_focus = self.explorer_focused
            || (self.selector_explorer_focus
                && self
                    .selector
                    .as_ref()
                    .is_some_and(|s| s.kind() == SelectorKind::Palette));
        context.focus = if self.bottom.focused {
            PrimaryFocus::BottomPanel
        } else if explorer_focus {
            PrimaryFocus::Sidebar
        } else {
            PrimaryFocus::Editor
        };
        context.explorer_focused = explorer_focus;
        context.editor_input = if self
            .selector
            .as_ref()
            .is_some_and(|s| s.kind() == SelectorKind::Palette)
        {
            self.selector_editor_input
                && self.selector_edit_target.is_some()
                && self.selector_edit_target == self.source_edit_target()
        } else {
            !explorer_focus
                && !self.bottom.focused
                && self.selector.is_none()
                && self.search_panel.is_none()
                && self.navigation_panel.is_none()
                && !self.project_visible()
                && self.replace_panel.is_none()
                && self.git_panel.is_none()
        };
        if explorer_focus {
            context.can_open_to_side = self
                .explorer_view
                .selected()
                .and_then(|id| self.explorer.node(id))
                .is_some_and(|n| !n.is_dir());
        }
        context.has_project = self.project_root.is_some();
        context.project_search_running =
            self.project_search.state == jc_app::ProjectSearchState::Running;
        context.project_replace_ready = self.project_root.as_deref() == self.project_search.root()
            && self.project_search.can_replace()
            && self
                .replace_panel
                .as_ref()
                .is_none_or(|p| p.stage != replace_panel::ReplaceStage::Applying);
        context.git_repository_available = context.has_project;
        context.git_diff_available = context.has_project
            && context.has_document
            && self
                .workbench
                .active_document()
                .is_some_and(|d| d.session.encoding().is_some());
        context.git_diff_running = self.git_diff_running();
        context.git_status_running = matches!(
            self.git_status.state(),
            jc_app::git_status_tasks::GitStatusTaskState::Queued
                | jc_app::git_status_tasks::GitStatusTaskState::Running
        );
        // Availability is an intention gate, never a read of another owner's
        // clipboard. Content is requested only by explicit Paste.
        if !context.rectangular_selection {
            context.clipboard_has_text = context.has_document;
        }
        if explorer_focus {
            context.clipboard_has_text = false;
            context.has_selection = false;
        }
        context
    }
    fn execute(&mut self, command: CommandId) -> Action {
        self.cancel_pointer();
        if !self
            .registry
            .is_enabled(command, &self.entry_context(command))
        {
            self.status = "Command unavailable in the current context".into();
            return Action::None;
        }
        if let Some(group) = native_menu::group_for_command(command) {
            return Action::NativeMenu(group);
        }
        match command {
            command if self.selector.is_some() && selector_input::field_command(command) => {
                return self.selector_field_action(command);
            }
            CommandId::ViewToggleBottomPanel | CommandId::ViewProblems | CommandId::ViewOutput => {
                self.show_bottom(command);
                return Action::None;
            }
            command if rectangle_input::is_rectangle(command) => {
                match self.workbench.execute(command) {
                    Ok(_) => self.rectangle_status(),
                    Err(error) => self.error(error),
                }
                return Action::None;
            }
            CommandId::FileReopenClosed => {
                self.reopen_tab();
                return Action::None;
            }
            CommandId::SearchSelectionNext | CommandId::SearchSelectionPrevious => {
                self.search_selection(command);
                return Action::None;
            }
            CommandId::SearchGoToPosition => {
                self.open_navigation_panel();
                return Action::None;
            }
            CommandId::SearchHistoryPrevious
            | CommandId::SearchHistoryNext
            | CommandId::SearchToggleCase
            | CommandId::SearchToggleWholeWord
            | CommandId::SearchToggleRegex => {
                self.execute_search_option(command);
                return Action::None;
            }
            CommandId::NavigateBack | CommandId::NavigateForward => {
                self.close_selector();
                self.bottom.focused = false;
                self.workbench.cancel_search_navigation();
            }
            CommandId::ViewExplorer => {
                self.show_explorer();
                return Action::None;
            }
            CommandId::ExplorerRefresh => {
                self.refresh_explorer();
                return Action::None;
            }
            CommandId::ExplorerCancel => {
                self.cancel_explorer();
                return Action::None;
            }
            CommandId::FocusNext | CommandId::FocusPrevious => {
                self.cycle_regions(command == CommandId::FocusNext);
                return Action::None;
            }
            CommandId::EditorOpenToSide if self.explorer_focused => {
                self.activate_explorer(true, false);
                return Action::None;
            }
            CommandId::GitRepositoryStatus | CommandId::GitStatusRefresh => {
                return self.request_git(git_panel::GitIntent::Status)
            }
            CommandId::ViewGit | CommandId::GitDiffRefresh => {
                return self.request_git(git_panel::GitIntent::Diff(self.git_source));
            }
            CommandId::GitDiffHead => {
                return self.request_git(git_panel::GitIntent::Diff(
                    jc_app::git_read::GitBlobSource::Head,
                ))
            }
            CommandId::GitDiffIndex => {
                return self.request_git(git_panel::GitIntent::Diff(
                    jc_app::git_read::GitBlobSource::Index,
                ))
            }
            CommandId::GitDiffCancel => {
                self.cancel_git(git_panel::GitIntent::Diff(
                    jc_app::git_read::GitBlobSource::Head,
                ));
                return Action::None;
            }
            CommandId::GitStatusCancel => {
                self.cancel_git(git_panel::GitIntent::Status);
                return Action::None;
            }
            CommandId::GitConfigureExecutable => return Action::ConfigureGit(self.git_last_intent),
            CommandId::SearchReplaceProject => {
                self.open_project_replace();
                return Action::None;
            }
            CommandId::SearchProject => {
                self.open_project_search(false);
                return Action::None;
            }
            CommandId::SearchCancelProject => {
                self.project_search.cancel();
                self.status = "Project search cancelled".into();
                return Action::None;
            }
            CommandId::ViewSearchResults => {
                self.open_project_search(true);
                return Action::None;
            }
            CommandId::SearchFind | CommandId::SearchReplace => {
                self.open_search_panel(command == CommandId::SearchReplace);
                return Action::None;
            }
            CommandId::FileChooseEncoding => {
                self.open_encoding_selector();
                return Action::None;
            }
            CommandId::EditCopy | CommandId::EditCut | CommandId::EditPaste
                if !self.context().rectangular_selection
                    && (command != CommandId::EditPaste
                        || !self.workbench.clipboard_is_rectangular()) =>
            {
                return Action::Clipboard(command)
            }
            CommandId::FileOpenFolder => {
                self.close_selector();
                return Action::ChooseFolder;
            }
            CommandId::PaletteOpen => {
                self.open_selector(SelectorKind::Palette);
                return Action::None;
            }
            CommandId::FileQuickOpen => {
                self.open_selector(SelectorKind::Files);
                return Action::None;
            }
            CommandId::FileNew => {
                self.new_document();
                return Action::None;
            }
            CommandId::EditorFocusPrimary
            | CommandId::EditorOpenToSide
            | CommandId::EditorFocusSecondary
            | CommandId::EditorSplitRight
            | CommandId::EditorOneGroup
            | CommandId::TabNext
            | CommandId::TabPrevious => {
                self.explorer_focused = false;
                self.bottom.focused = false;
            }
            CommandId::Tab1
            | CommandId::Tab2
            | CommandId::Tab3
            | CommandId::Tab4
            | CommandId::Tab5
            | CommandId::Tab6
            | CommandId::Tab7
            | CommandId::Tab8
            | CommandId::Tab9 => {
                self.explorer_focused = false;
                self.bottom.focused = false;
            }
            CommandId::FileOpen => return Action::Open(false),
            CommandId::FileSaveAs => return Action::SaveAs,
            CommandId::FileClose => return Action::CloseTab,
            CommandId::FileExit => return Action::Exit,
            CommandId::FileSaveAll => {
                if self.save_all_active {
                    self.status = "Save All is already in progress".into();
                    return Action::None;
                }
                return Action::SaveAll;
            }
            CommandId::FileSave
                if self
                    .workbench
                    .active_document()
                    .is_some_and(|document| document.session.is_new()) =>
            {
                return Action::SaveAs
            }
            _ => {}
        }
        let edit_before = self
            .workbench
            .active_document()
            .map(|d| (d.id, d.session.document().revision()));
        match self.workbench.execute(command) {
            Ok(CommandOutcome::View(CommandId::ViewToggleInvisibles)) => {
                self.status = if self.workbench.invisibles_visible() {
                    "Invisible characters: ASCII spans on"
                } else {
                    "Invisible characters hidden"
                }
                .into()
            }
            Ok(CommandOutcome::None) => {
                if command == CommandId::SearchCancel {
                    self.status = "Document search / rectangle cancelled".into();
                    return Action::None;
                }
                if line_edit::is_line_edit(command) {
                    let label = self.registry.get(command).map_or("Line edit", |s| s.label);
                    let edit_after = self
                        .workbench
                        .active_document()
                        .map(|d| (d.id, d.session.document().revision()));
                    self.status = if edit_before == edit_after {
                        format!("No change · {label}")
                    } else {
                        format!("{label} · unsaved changes")
                    };
                    return Action::None;
                }
                if command == CommandId::FileSave {
                    self.save_count += 1;
                }
                self.status = if self.context().rectangular_selection {
                    "Ready · rectangular clipboard stays internal"
                } else {
                    "Ready · Windows text clipboard · scalar-cell preview"
                }
                .into();
            }
            Ok(CommandOutcome::View(
                CommandId::EditorSplitRight
                | CommandId::EditorOneGroup
                | CommandId::EditorFocusPrimary
                | CommandId::EditorFocusSecondary
                | CommandId::EditorTogglePin
                | CommandId::EditorToggleScrollLink
                | CommandId::EditorOpenToSide
                | CommandId::NavigateBack
                | CommandId::NavigateForward,
            )) => self.status = "Ready · shared editor groups".into(),
            Ok(_) => self.status = "This command needs a frontend flow not provided here".into(),
            Err(error) => self.error(error),
        }
        Action::None
    }
    fn execute_menu(&mut self, command: CommandId) -> Action {
        if !self
            .registry
            .is_enabled(command, &self.entry_context(command))
        {
            return self.execute(command);
        }
        if self.selector.is_some() && selector_input::field_command(command) {
            return self.execute(command);
        }
        let explorer_focus = self.explorer_focused || self.selector_explorer_focus;
        if !matches!(
            command,
            CommandId::ViewToggleInvisibles
                | CommandId::SearchHistoryPrevious
                | CommandId::SearchHistoryNext
                | CommandId::SearchToggleCase
                | CommandId::SearchToggleWholeWord
                | CommandId::SearchToggleRegex
                | CommandId::PaletteOpen
        ) {
            self.close_selector();
        }
        if matches!(
            command,
            CommandId::ViewExplorer
                | CommandId::ExplorerRefresh
                | CommandId::ExplorerCancel
                | CommandId::EditorOpenToSide
                | CommandId::FocusNext
                | CommandId::FocusPrevious
                | CommandId::PaletteOpen
                | CommandId::FileQuickOpen
                | CommandId::FileChooseEncoding
        ) {
            self.explorer_focused = explorer_focus;
        }
        self.execute(command)
    }
    fn modifiers(&self) -> Modifiers {
        if let Some(value) = self.key_override {
            return value;
        }
        if self.smoke {
            return Modifiers::NONE;
        }
        let mut modifiers = Modifiers::NONE;
        for (key, flag) in [
            (0x10, Modifiers::SHIFT),
            (0x11, Modifiers::CTRL),
            (0x12, Modifiers::ALT),
        ] {
            if unsafe { GetKeyState(key) } < 0 {
                modifiers = modifiers.union(flag);
            }
        }
        modifiers
    }
    fn key(&mut self, code: usize) -> (bool, Action) {
        if !matches!(code, 0x10..=0x12) {
            self.cancel_pointer();
        }
        self.pending_surrogate = None;
        let modifiers = self.modifiers();
        if let Some(group) = native_menu::mnemonic(code, modifiers) {
            return (true, Action::NativeMenu(group));
        }
        if code == 0x79 && modifiers == Modifiers::NONE {
            return (true, Action::NativeMenu(native_menu::NativeMenuGroup::File));
        }
        let key = match code {
            0x09 => Some(Key::Tab),
            0x0d => Some(Key::Enter),
            0x1b => Some(Key::Escape),
            0x21 => Some(Key::PageUp),
            0x22 => Some(Key::PageDown),
            0x23 => Some(Key::End),
            0x24 => Some(Key::Home),
            0x25 => Some(Key::Left),
            0x26 => Some(Key::Up),
            0x27 => Some(Key::Right),
            0x28 => Some(Key::Down),
            0x30..=0x5a => char::from_u32(code as u32).map(|ch| Key::Char(ch.to_ascii_lowercase())),
            0xbf => Some(Key::Char('/')),
            0x70..=0x87 => Some(Key::Function((code - 0x70 + 1) as u8)),
            _ => None,
        };
        if let Some(key) = key {
            let global = self.keymap.resolve(
                KeyScope::Global,
                KeyStroke::new(modifiers, key),
                &self.registry,
                &self.context(),
            );
            if let Resolution::Disabled(
                CommandId::SearchGoToPosition
                | CommandId::NavigateBack
                | CommandId::NavigateForward,
            ) = global
            {
                self.status = "Navigation command unavailable in the current context".into();
                return (true, Action::None);
            }
            if let Resolution::Enabled(
                command @ (CommandId::ViewToggleInvisibles
                | CommandId::FileExit
                | CommandId::SearchReplaceProject
                | CommandId::SearchGoToPosition
                | CommandId::NavigateBack
                | CommandId::NavigateForward
                | CommandId::PaletteOpen),
            ) = global
            {
                if command != CommandId::SearchReplaceProject || self.replace_panel.is_none() {
                    return (true, self.execute(command));
                }
            }
        }
        if self.git_panel.is_some() {
            return (true, self.git_key(code, modifiers));
        }
        if self.replace_panel.is_some() {
            return (true, self.replace_key(code, modifiers));
        }
        if self.project_visible() {
            return (true, self.project_key(code, modifiers));
        }
        if self.search_panel.is_some() {
            return (true, self.search_key(code, modifiers));
        }
        if self.navigation_panel.is_some() {
            return (true, self.navigation_key(code, modifiers));
        }
        if self.selector.is_some() {
            return (true, self.selector_key(code, modifiers));
        }
        if self.bottom.focused {
            return (true, self.bottom_key(code, key, modifiers));
        }
        if self.explorer_focused {
            if let Some(key) = key {
                match self.keymap.resolve(
                    KeyScope::Sidebar,
                    KeyStroke::new(modifiers, key),
                    &self.registry,
                    &self.context(),
                ) {
                    Resolution::Enabled(command) if supported(command) => {
                        return (true, self.execute(command))
                    }
                    Resolution::Disabled(command) if supported(command) => {
                        self.status = "Command unavailable in Explorer".into();
                        return (true, Action::None);
                    }
                    _ => {}
                }
            }
            self.explorer_key(code, modifiers);
            return (true, Action::None);
        }
        if let Some(key) = key {
            let resolution = self.keymap.resolve(
                KeyScope::Editor,
                KeyStroke::new(modifiers, key),
                &self.registry,
                &self.context(),
            );
            match resolution {
                Resolution::Enabled(command) if supported(command) => {
                    return (true, self.execute(command))
                }
                Resolution::Disabled(command) if supported(command) => {
                    self.status = "Command unavailable".into();
                    return (true, Action::None);
                }
                _ => {}
            }
        }
        if modifiers.contains(Modifiers::CTRL) || modifiers.contains(Modifiers::ALT) {
            return (false, Action::None);
        }
        let select = modifiers.contains(Modifiers::SHIFT);
        let result = match code {
            0x08 => self.workbench.backspace(),
            0x2e => self.workbench.delete_forward(),
            0x25 => self.workbench.move_left(select),
            0x27 => self.workbench.move_right(select),
            0x26 => self.workbench.move_vertical(-1, select),
            0x28 => self.workbench.move_vertical(1, select),
            0x24 => self.workbench.move_home(select),
            0x23 => self.workbench.move_end(select),
            0x21 => self
                .workbench
                .move_vertical(-((self.height / self.cell_height).max(1) as isize), select),
            0x22 => self
                .workbench
                .move_vertical((self.height / self.cell_height).max(1) as isize, select),
            _ => return (false, Action::None),
        };
        if let Err(error) = result {
            self.error(error);
        }
        (true, Action::None)
    }
    fn character(&mut self, unit: u16) {
        if unit < 0x20 || unit == 0x7f {
            return;
        }
        if (0xd800..=0xdbff).contains(&unit) {
            if self.pending_surrogate.replace(unit).is_some() {
                self.status = "Unpaired UTF-16 high surrogate rejected".into();
            }
            return;
        }
        let scalar = if (0xdc00..=0xdfff).contains(&unit) {
            let Some(high) = self.pending_surrogate.take() else {
                self.status = "Unpaired UTF-16 low surrogate rejected".into();
                return;
            };
            0x10000 + ((u32::from(high) - 0xd800) << 10) + (u32::from(unit) - 0xdc00)
        } else {
            if self.pending_surrogate.take().is_some() {
                self.status = "Unpaired UTF-16 sequence rejected".into();
                return;
            }
            u32::from(unit)
        };
        if let Some(ch) = char::from_u32(scalar) {
            self.insert_character(ch);
        }
    }
    fn insert_character(&mut self, ch: char) {
        self.cancel_pointer();
        if ch.is_control() {
            return;
        }
        let mut bytes = [0u8; 4];
        if self.git_panel.is_some() {
            return;
        }
        if self.replace_panel.is_some() {
            self.edit_replace_field(jc_app::FieldEdit::Insert(ch.encode_utf8(&mut bytes)));
            return;
        }
        if self.project_visible() {
            self.edit_project_field(jc_app::FieldEdit::Insert(ch.encode_utf8(&mut bytes)));
            return;
        }
        if self.search_panel.is_some() {
            self.edit_search_input(jc_app::FieldEdit::Insert(ch.encode_utf8(&mut bytes)));
            return;
        }
        if self.navigation_panel.is_some() {
            self.edit_navigation_input(jc_app::FieldEdit::Insert(ch.encode_utf8(&mut bytes)));
            return;
        }
        if self.selector.is_some() {
            self.edit_selector_input(jc_app::FieldEdit::Insert(ch.encode_utf8(&mut bytes)), false);
            return;
        }
        if self.explorer_focused {
            return;
        }
        if self.bottom.focused {
            return;
        }
        match self.workbench.insert_text(ch.encode_utf8(&mut bytes)) {
            Ok(()) => self.status = "Ready".into(),
            Err(error) => self.error(error),
        }
    }
}
fn supported(command: CommandId) -> bool {
    native_menu::is_supported_macro(command)
        || COMMANDS.iter().any(|(_, value, _)| *value == command)
        || matches!(
            command,
            CommandId::EditIndent
                | CommandId::EditOutdent
                | CommandId::EditorOpenToSide
                | CommandId::SearchCancel
                | CommandId::EditRectangleLeft
                | CommandId::EditRectangleRight
                | CommandId::EditRectangleUp
                | CommandId::EditRectangleDown
                | CommandId::Tab1
                | CommandId::Tab2
                | CommandId::Tab3
                | CommandId::Tab4
                | CommandId::Tab5
                | CommandId::Tab6
                | CommandId::Tab7
                | CommandId::Tab8
                | CommandId::Tab9
        )
}

/// Box remains Rust-owned during CreateWindowEx. Only after success does the
/// HWND acquire ownership; failed creation cannot double-free an NCCREATE pointer.
struct Owner {
    state: RefCell<State>,
    hwnd_owned: Cell<bool>,
}
// Caller bounds the returned reference to a live HWND; never retain it across
// DestroyWindow. Dialog/message calls are made only after RefCell guards end.
unsafe fn owner<'window>(hwnd: Hwnd) -> Option<&'window Owner> {
    let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Owner;
    pointer.as_ref()
}

unsafe extern "system" fn window_proc(
    hwnd: Hwnd,
    message: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        procedure(hwnd, message, wparam, lparam)
    })) {
        Ok(value) => value,
        Err(_) => {
            PostQuitMessage(1);
            0
        }
    }
}
unsafe fn procedure(hwnd: Hwnd, message: u32, wparam: usize, lparam: isize) -> isize {
    if message == WM_NCCREATE {
        let create = &*(lparam as *const CreateStruct);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.parameter as isize);
        if let Some(owner) = owner(hwnd) {
            owner.state.borrow().bridge.attach(hwnd);
        }
        // DefWindowProc establishes native non-client state, including the
        // supplied window title. Attaching our owner does not replace it.
        return DefWindowProcW(hwnd, message, wparam, lparam);
    }
    if message == WM_NCDESTROY {
        let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Owner;
        if !pointer.is_null() {
            (*pointer).state.borrow().bridge.detach();
        }
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        if !pointer.is_null() && (*pointer).hwnd_owned.get() {
            drop(Box::from_raw(pointer));
        }
        return DefWindowProcW(hwnd, message, wparam, lparam);
    }
    if message == WM_DESTROY {
        PostQuitMessage(0);
        return 0;
    }
    let Some(owner) = owner(hwnd) else {
        return DefWindowProcW(hwnd, message, wparam, lparam);
    };
    if message == WM_PAINT {
        let frame = {
            let Ok(mut state) = owner.state.try_borrow_mut() else {
                return DefWindowProcW(hwnd, message, wparam, lparam);
            };
            state.paint_count += 1;
            let frame = paint::prepare(&mut state);
            state.explorer_hits = frame.explorer_hits();
            state.bottom_hits = frame.bottom_hits();
            frame
        };
        let mut paint = NativePaint {
            hwnd,
            paint: std::mem::zeroed(),
        };
        let dc = BeginPaint(hwnd, &mut paint.paint);
        if !dc.is_null() {
            if let Err(error) = paint::draw(dc, &frame) {
                if let Ok(mut state) = owner.state.try_borrow_mut() {
                    state.paint_errors += 1;
                    state.error(error);
                }
            }
        } else if let Ok(mut state) = owner.state.try_borrow_mut() {
            state.paint_errors += 1;
            state.error("BeginPaint did not return a device context");
        }
        return 0;
    }
    // No RefCell borrow crosses dialogs, DestroyWindow or SendMessage. Modal
    // Win32 calls may reenter this callback and prepare another read-only frame.
    // Screen coordinates and OS settings are read outside the model borrow.
    let wheel = if message == WM_MOUSEWHEEL {
        let mut point = Point {
            x: (lparam as u16) as i16 as i32,
            y: ((lparam as usize >> 16) as u16) as i16 as i32,
        };
        let mut lines = 3u32;
        SystemParametersInfoW(0x68, 0, (&mut lines as *mut u32).cast(), 0);
        (ScreenToClient(hwnd, &mut point) != 0).then_some((point, lines))
    } else {
        None
    };
    let mut action = Action::None;
    let mut handled = true;
    let mut redraw = true;
    {
        let Ok(mut state) = owner.state.try_borrow_mut() else {
            return DefWindowProcW(hwnd, message, wparam, lparam);
        };
        match message {
            WM_CREATE => {
                let dc = GetDC(hwnd);
                if !dc.is_null() {
                    let old = SelectObject(dc, state.font);
                    let mut metrics: TextMetric = std::mem::zeroed();
                    if GetTextMetricsW(dc, &mut metrics) != 0 {
                        state.cell_width = metrics.average.max(6);
                        state.cell_height = (metrics.height + metrics.external).max(12);
                    }
                    SelectObject(dc, old);
                    ReleaseDC(hwnd, dc);
                }
            }
            TASKS_MESSAGE => {
                redraw = state.poll_tasks();
            }
            WM_SIZE => {
                state.cancel_pointer();
                state.interaction_epoch = state.interaction_epoch.saturating_add(1);
                state.explorer_hits = None;
                state.bottom_hits = None;
                let mut rect = Rect::default();
                if GetClientRect(hwnd, &mut rect) != 0 {
                    state.width = rect.right.max(1);
                    state.height = rect.bottom.max(1);
                    if explorer_panel::region(&state).is_none() {
                        state.explorer_focused = false;
                    }
                    if bottom_panel::region(&state).is_none() {
                        state.bottom.focused = false;
                    }
                }
            }
            WM_SETFOCUS => state.focused = true,
            WM_KILLFOCUS => {
                state.cancel_pointer();
                state.interaction_epoch = state.interaction_epoch.saturating_add(1);
                state.focused = false;
                state.pending_surrogate = None;
            }
            WM_ERASEBKGND => return 1,
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                let result = state.key(wparam);
                handled = result.0;
                action = result.1;
            }
            WM_KEYUP | WM_SYSKEYUP if wparam == 0x79 => {
                redraw = false;
            }
            WM_SYSCHAR
                if char::from_u32(wparam as u32).is_some_and(|ch| {
                    native_menu::mnemonic(ch.to_ascii_uppercase() as usize, Modifiers::ALT)
                        .is_some()
                }) =>
            {
                redraw = false;
            }
            WM_CHAR => state.character(wparam as u16),
            WM_UNICHAR => {
                if wparam == 0xffff {
                    return 1;
                }
                if let Some(ch) = char::from_u32(wparam as u32) {
                    state.insert_character(ch);
                }
            }
            WM_COMMAND if lparam == 0 => {
                if state.accept_menu_selection() {
                    if wparam & 0xffff == OPEN_SIDE {
                        if state.registry.is_enabled(
                            CommandId::FileOpen,
                            &state.entry_context(CommandId::FileOpen),
                        ) {
                            state.close_selector();
                            action = Action::Open(true);
                        }
                    } else if let Some((_, command, _)) =
                        COMMANDS.iter().find(|(id, _, _)| *id == wparam & 0xffff)
                    {
                        action = state.execute_menu(*command);
                    }
                }
            }
            WM_INITMENU => {
                state.cancel_pointer();
                state.menu_stamp = Some(Box::new(state.current_menu_stamp()));
                if let Err(error) = native_menu::refresh(hwnd, &state) {
                    state.error(error);
                }
            }
            WM_MOUSEWHEEL => {
                if let Some((point, lines)) = wheel {
                    let lines = state.smoke_wheel_lines.unwrap_or(lines);
                    state.pointer_wheel(
                        point.x,
                        point.y,
                        ((wparam >> 16) as u16) as i16 as i32,
                        lines,
                        wparam & 0xffff,
                    );
                } else {
                    state.cancel_pointer();
                }
            }
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
                let x = (lparam as u16) as i16 as i32;
                let y = ((lparam as usize >> 16) as u16) as i16 as i32;
                state.cancel_pointer();
                if state.pointer_source_available()
                    && !state.bottom_click(x, y, message == WM_LBUTTONDBLCLK)
                    && !state.explorer_click(x, y, message == WM_LBUTTONDBLCLK)
                    && message == WM_LBUTTONDOWN
                {
                    let modifiers = state.modifiers();
                    let started = if modifiers.contains(Modifiers::ALT)
                        && !modifiers.contains(Modifiers::CTRL)
                    {
                        state.begin_rectangle_drag(x, y)
                    } else {
                        state.begin_linear_drag(x, y, modifiers.contains(Modifiers::SHIFT))
                    };
                    if started {
                        action = Action::CapturePointer;
                    }
                }
            }
            WM_MOUSEMOVE | WM_LBUTTONUP => {
                redraw = state.rectangle_drag.is_some() || state.linear_drag.is_some();
                if wparam & 1 != 0 || message == WM_LBUTTONUP {
                    let x = (lparam as u16) as i16 as i32;
                    let y = ((lparam as usize >> 16) as u16) as i16 as i32;
                    if state.rectangle_drag.is_some() {
                        state.update_rectangle_drag(x, y);
                    } else {
                        state.update_linear_drag(x, y);
                    }
                }
                if wparam & 1 == 0 || message == WM_LBUTTONUP {
                    state.cancel_pointer();
                }
            }
            WM_CAPTURECHANGED | WM_CANCELMODE => {
                state.cancel_pointer();
            }
            WM_CLOSE => action = Action::Exit,
            _ => handled = false,
        }
        state.schedule_search();
        redraw |= state.validate_project_search();
        redraw |= state.validate_git_context();
        state.validate_rectangle_drag();
        state.validate_linear_drag();
    }
    if !handled {
        return DefWindowProcW(hwnd, message, wparam, lparam);
    }
    release_idle_capture(hwnd);
    perform(hwnd, action);
    if let Some(current) = self::owner(hwnd) {
        current.state.borrow().schedule_search();
    }
    release_idle_capture(hwnd);
    if redraw && IsWindow(hwnd) != 0 {
        InvalidateRect(hwnd, std::ptr::null(), 0);
    }
    if message == WM_UNICHAR {
        1
    } else {
        0
    }
}

struct NativePaint {
    hwnd: Hwnd,
    paint: PaintStruct,
}
impl Drop for NativePaint {
    fn drop(&mut self) {
        if !self.paint.dc.is_null() {
            unsafe {
                EndPaint(self.hwnd, &self.paint);
            }
        }
    }
}

unsafe fn file_dialog(hwnd: Hwnd, save: bool) -> Result<Option<PathBuf>, String> {
    file_dialog_named(hwnd, save, None)
}
unsafe fn file_dialog_named(
    hwnd: Hwnd,
    save: bool,
    title: Option<&str>,
) -> Result<Option<PathBuf>, String> {
    let mut file = vec![0u16; 32768];
    let filter = wide("All files\0*.*\0");
    let title = wide(title.unwrap_or(if save {
        "Save source as"
    } else {
        "Open source"
    }));
    let mut options: OpenFileName = std::mem::zeroed();
    options.size = std::mem::size_of::<OpenFileName>() as u32;
    options.owner = hwnd;
    options.filter = filter.as_ptr();
    options.file = file.as_mut_ptr();
    options.file_len = file.len() as u32;
    options.title = title.as_ptr();
    options.flags = 0x80000 | 0x800 | 0x8 | 0x4 | if save { 0x2 } else { 0x1000 };
    let accepted = if save {
        GetSaveFileNameW(&mut options)
    } else {
        GetOpenFileNameW(&mut options)
    };
    if accepted == 0 {
        let error = CommDlgExtendedError();
        return if error == 0 {
            Ok(None)
        } else {
            Err(format!("Native file dialog failed ({error:#x})"))
        };
    }
    let end = file
        .iter()
        .position(|unit| *unit == 0)
        .ok_or("Invalid file-dialog result")?;
    Ok(Some(PathBuf::from(std::ffi::OsString::from_wide(
        &file[..end],
    ))))
}
unsafe fn release_idle_capture(hwnd: Hwnd) {
    // Release can synchronously send WM_CAPTURECHANGED; drop the borrow first.
    let idle = owner(hwnd).is_some_and(|m| {
        let state = m.state.borrow();
        state.rectangle_drag.is_none() && state.linear_drag.is_none()
    });
    if idle && GetCapture() == hwnd {
        ReleaseCapture();
    }
}

unsafe fn perform(hwnd: Hwnd, action: Action) {
    if matches!(
        action,
        Action::Open(_) | Action::SaveAs | Action::ChooseFolder | Action::ConfigureGit(_)
    ) && owner(hwnd).is_some_and(|owner| owner.state.borrow().smoke)
    {
        if let Some(owner) = owner(hwnd) {
            owner.state.borrow_mut().status = "Smoke prevented an unexpected native dialog".into();
        }
        return;
    }
    match action {
        Action::CapturePointer => {
            SetCapture(hwnd);
            if GetCapture() != hwnd {
                if let Some(model) = owner(hwnd) {
                    let mut state = model.state.borrow_mut();
                    state.cancel_pointer();
                    state.error("Mouse capture unavailable; selection retained");
                }
            }
        }
        Action::NativeMenu(group) => {
            if let Err(error) = native_menu::open(hwnd, group) {
                if let Some(model) = owner(hwnd) {
                    model.state.borrow_mut().error(error);
                }
            }
        }
        Action::None => {}
        Action::ConfigureGit(intent) => git_panel::configure(hwnd, intent),
        Action::Clipboard(command) => clipboard_flow::perform(hwnd, command),
        Action::PasteSearch(generation) => clipboard_flow::paste_search(hwnd, generation),
        Action::PasteNavigation(generation) => clipboard_flow::paste_navigation(hwnd, generation),
        Action::SelectorClipboard(command, target) => {
            clipboard_flow::selector_transfer(hwnd, command, *target)
        }
        Action::PasteProject(generation) => clipboard_flow::paste_project(hwnd, generation),
        Action::PasteReplacement(generation) => clipboard_flow::paste_replacement(hwnd, generation),
        Action::SaveAll => save_flow::perform(hwnd),
        Action::ChooseFolder => {
            let selected = folder::choose(hwnd);
            if let Some(owner) = owner(hwnd) {
                let mut state = owner.state.borrow_mut();
                match selected {
                    Ok(Some(path)) => {
                        if let Err(error) = state.set_project_root(path) {
                            state.error(error);
                        } else {
                            state.show_explorer();
                        }
                    }
                    Ok(None) => {}
                    Err(error) => state.error(error),
                }
            }
        }
        Action::Open(side) => {
            let selected = file_dialog(hwnd, false);
            if let Some(owner) = owner(hwnd) {
                let mut state = owner.state.borrow_mut();
                match selected {
                    Ok(Some(path)) => {
                        let encoding = state.encoding;
                        let result = if side {
                            state.workbench.open_file_to_side(path, encoding)
                        } else {
                            state.workbench.open_file(path, encoding)
                        };
                        if let Err(error) = result {
                            state.error(error);
                        } else {
                            state.status = "Opened".into();
                            state.explorer_focused = false;
                            state.bottom.focused = false;
                            state.ensure_encoding_selector();
                        }
                    }
                    Ok(None) => {}
                    Err(error) => state.error(error),
                }
            }
        }
        Action::SaveAs => {
            let captured = owner(hwnd)
                .ok_or_else(|| "Save As window closed".to_owned())
                .and_then(|model| {
                    let state = model.state.borrow();
                    let document = state
                        .workbench
                        .active_document()
                        .ok_or("No active Save As document")?;
                    state
                        .workbench
                        .capture_save_target(document.id, document.session.document().revision())
                        .map_err(|e| e.to_string())
                });
            let target = match captured {
                Ok(target) => target,
                Err(error) => {
                    if let Some(model) = owner(hwnd) {
                        model.state.borrow_mut().error(error);
                    }
                    return;
                }
            };
            let selected = file_dialog(hwnd, true);
            if let Some(owner) = owner(hwnd) {
                let mut state = owner.state.borrow_mut();
                match selected {
                    Ok(Some(path)) => match state.workbench.save_target(target, Some(path)) {
                        Ok(()) => {
                            state.save_count += 1;
                            state.status = "Saved".into();
                        }
                        Err(error) => state.error(error),
                    },
                    Ok(None) => {}
                    Err(error) => state.error(error),
                }
            }
        }
        Action::CloseTab => close_flow::perform(hwnd),
        Action::Exit => {
            let dirty = owner(hwnd)
                .is_some_and(|owner| owner.state.borrow().workbench.context().dirty_documents > 0);
            if dirty
                && MessageBoxW(
                    hwnd,
                    wide(
                        "Close and discard ALL unsaved changes? Save documents first to keep them.",
                    )
                    .as_ptr(),
                    wide("CIDE").as_ptr(),
                    0x134,
                ) != 6
            {
                return;
            }
            DestroyWindow(hwnd);
        }
    }
}

pub(super) fn run(options: GuiOptions) -> Result<(), String> {
    if options
        .git_executable
        .as_ref()
        .is_some_and(|p| !p.is_absolute() || p.as_os_str().len() > 4096)
    {
        return Err("Choose an absolute Git executable path within4096 native units".into());
    }
    if options
        .search_workers
        .is_some_and(|n| !(1..=4).contains(&n))
    {
        return Err("Project workers require1..4".into());
    }
    if options.smoke.is_some() && !options.paths.is_empty() {
        return Err("Smoke mode does not accept startup source paths".into());
    }
    if let Some(smoke) = &options.smoke {
        smoke::validate(smoke)?;
    }
    let mut owner = Box::new(Owner {
        state: RefCell::new(State::new(&options)?),
        hwnd_owned: Cell::new(false),
    });
    let class = wide(&format!("JCIDE.Native.{}", std::process::id()));
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let descriptor = WndClass {
            style: 3 | 8,
            proc: Some(window_proc),
            class_extra: 0,
            window_extra: 0,
            instance,
            icon: LoadIconW(instance, 101usize as *const u16),
            cursor: LoadCursorW(null_mut(), 32512usize as *const u16),
            background: null_mut(),
            menu_name: std::ptr::null(),
            class_name: class.as_ptr(),
        };
        if RegisterClassW(&descriptor) == 0 {
            return Err(os_error("RegisterClassW"));
        }
        let menu = match native_menu::create(&owner.state.borrow().keymap) {
            Ok(menu) => menu,
            Err(error) => {
                UnregisterClassW(class.as_ptr(), instance);
                return Err(error);
            }
        };
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            wide("CIDE — native Win32/GDI development frontend").as_ptr(),
            0x00cf0000,
            0x80000000u32 as i32,
            0x80000000u32 as i32,
            1000,
            700,
            null_mut(),
            menu,
            instance,
            (&mut *owner as *mut Owner).cast(),
        );
        if hwnd.is_null() {
            DestroyMenu(menu);
            UnregisterClassW(class.as_ptr(), instance);
            return Err(os_error("CreateWindowExW"));
        }
        owner.hwnd_owned.set(true);
        let _ = Box::into_raw(owner);
        if options.smoke.is_none() {
            ShowWindow(hwnd, 5);
            SetFocus(hwnd);
            UpdateWindow(hwnd);
        } else if PostMessageW(hwnd, SMOKE_MESSAGE, 0, 0) == 0 {
            let error = os_error("Post owned smoke message");
            DestroyWindow(hwnd);
            UnregisterClassW(class.as_ptr(), instance);
            return Err(error);
        }
        let mut smoke_result = None;
        let mut message: Msg = std::mem::zeroed();
        let result = loop {
            // A cooperative search may post its next slot continuously. Give
            // hardware input and an invalidated paint bounded priority before
            // consuming that work, rather than starving them behind posted jobs.
            for _ in 0..32 {
                let mut input: Msg = std::mem::zeroed();
                if IsWindow(hwnd) == 0
                    || (PeekMessageW(&mut input, hwnd, WM_KEYDOWN, WM_UNICHAR, 1) == 0
                        && PeekMessageW(&mut input, hwnd, 0x200, 0x20e, 1) == 0)
                {
                    break;
                }
                TranslateMessage(&input);
                DispatchMessageW(&input);
            }
            let mut painting: Msg = std::mem::zeroed();
            if IsWindow(hwnd) != 0 && PeekMessageW(&mut painting, hwnd, WM_PAINT, WM_PAINT, 1) != 0
            {
                DispatchMessageW(&painting);
            }
            let received = GetMessageW(&mut message, null_mut(), 0, 0);
            if received < 0 {
                break Err(os_error("GetMessageW"));
            }
            if received == 0 {
                break if message.wparam == 0 {
                    Ok(())
                } else {
                    Err("Native window callback failed".into())
                };
            }
            if message.hwnd == hwnd && message.message == SMOKE_MESSAGE {
                if let Some(config) = &options.smoke {
                    smoke_result = Some(smoke::run(hwnd, config));
                }
                // Test failures must also close without a modal dirty prompt.
                if IsWindow(hwnd) != 0 {
                    DestroyWindow(hwnd);
                }
                continue;
            }
            TranslateMessage(&message);
            DispatchMessageW(&message);
        };
        if IsWindow(hwnd) != 0 {
            DestroyWindow(hwnd);
        }
        UnregisterClassW(class.as_ptr(), instance);
        result?;
        smoke_result.unwrap_or(Ok(()))
    }
}
