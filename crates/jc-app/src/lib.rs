//! UI-independent application interaction model for CIDE.
//!
//! The TUI and GUI must share this layer. Action shortcuts, command enablement,
//! focus transitions, menu contents and tab behavior are data here rather than
//! ad-hoc widget code. Raw text entry and low-level cursor movement remain the
//! responsibility of the editor surface.

mod command;
mod dialog;
pub mod diff_model;
pub mod explorer;
mod navigation_input;
pub use navigation_input::parse_text_position;
mod focus;
pub mod git_diff;
pub mod git_read;
pub mod git_status;
pub mod git_status_tasks;
mod jobs;
mod keymap;
mod menu;
mod panels;
mod panes;
pub mod quick_open;
pub mod rectangle;
mod tabs;
mod text_field;
pub use text_field::{edit_field, present_field, FieldEdit, FieldPresentation};
mod workbench;
pub use jobs::{
    Cancellation, CompletedTask, JobId, JobKey, JobLimits, JobSystem, SubmitError, TaskFailure,
    TaskItem, TaskOutput, TaskProvider, TaskSnapshot, TaskStamp, TaskStatus, Wakeup,
};

pub use command::{
    default_registry, CommandCategory, CommandContext, CommandId, CommandRegistry, CommandSpec,
};
pub use dialog::{DialogAction, DialogButton, DialogKind, DialogSpec};
pub use focus::{EscapeOutcome, FocusManager, Overlay, PrimaryFocus};
pub use keymap::{
    default_keymap, Binding, Key, KeyMap, KeyMapError, KeyScope, KeyStroke, Modifiers, Resolution,
};
pub use menu::{default_menu_model, Menu, MenuId, MenuItem, MenuModel, MenuValidationError};
pub use panels::{BottomPanel, PanelManager, SidebarView};
pub use panes::{
    NavLocation, PaneId, PaneManager, PaneView, RectangularSelection, ViewId, ViewState,
};
pub use tabs::{CloseRequest, Tab, TabId, TabManager};

pub use workbench::rectangular::RectangleBounds;
pub use workbench::{
    ClipboardExport, ClipboardTarget, CommandOutcome, DisplayCell, DocumentId, EditorDocument,
    EditorRead, SaveTarget, SearchHistoryEntry, SearchState, TextReplacement, Workbench,
    WorkbenchError,
};
mod project_search;
pub use project_search::{
    ProjectReplacePreview, ProjectSearch, ProjectSearchMatch, ProjectSearchOptions,
    ProjectSearchRow, ProjectSearchState, ReplaceFileOutcome, SearchOrigin,
};
mod text_search;
pub use text_search::{SearchLimits, SearchMatch, SearchStatus};
