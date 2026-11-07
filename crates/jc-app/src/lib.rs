//! UI-independent application interaction model for CIDE.
//!
//! The TUI and GUI must share this layer. Action shortcuts, command enablement,
//! focus transitions, menu contents and tab behavior are data here rather than
//! ad-hoc widget code. Raw text entry and low-level cursor movement remain the
//! responsibility of the editor surface.

mod command;
mod dialog;
pub mod diff_model;
pub mod markdown_preview;
pub mod open_diff;
pub mod open_diff_view;
pub mod snippets;
mod source_graphemes;
mod unicode_layout;
#[cfg(test)]
mod unicode_layout_tests;
pub use unicode_layout::{
    Glyph, VisualAtom, MAX_PRESENTATION_TEXT_BYTES, MAX_VISUAL_ROW_TEXT_BYTES,
};
pub mod session;
pub mod source_services;
pub mod syntax_theme;
pub mod ui_theme;
pub use jc_languages::syntax::{SyntaxKind, SyntaxProfile};
pub mod explorer;
mod navigation_input;
pub use navigation_input::parse_text_position;
mod focus;
pub mod git_commit;
pub mod git_diff;
pub mod git_index;
pub mod git_read;
pub mod git_revision;
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
pub mod task_panels;
mod text_field;
pub use text_field::{
    edit_field, edit_selected_field, present_field, present_selected_field, FieldEdit,
    FieldPresentation, FieldSelection, SelectedFieldPresentation,
};
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
    CaptureChunk, CaptureError, ClipboardExport, ClipboardTarget, CommandOutcome, DisplayCell,
    DocumentId, EditorDocument, EditorRead, RecoveryCapture, RecoveryKey, SaveTarget,
    SearchHistoryEntry, SearchState, SyntaxRows, SyntaxStats, TextReplacement, Workbench,
    WorkbenchError,
};
mod project_search;
pub use project_search::{
    ProjectReplacePreview, ProjectSearch, ProjectSearchMatch, ProjectSearchOptions,
    ProjectSearchRow, ProjectSearchState, ReplaceFileOutcome, SearchOrigin,
};
mod regex_search;
mod text_search;
pub use text_search::{SearchLimits, SearchMatch, SearchStatus};
mod document_links;
mod javascript_navigation;
