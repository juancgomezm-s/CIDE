mod commands;
mod events;
mod explorer;
mod menus;
mod overlays;
mod paths;
mod presentation;
mod project_search;
pub(crate) use presentation::Presentation;
#[cfg(test)]
mod tests;

use crate::render;
use crate::screen::Screen;
use crate::state::{LaunchOptions, UiOverlay};
use jc_app::{
    default_keymap, default_menu_model, default_registry, CommandRegistry, FocusManager, KeyMap,
    MenuModel, PanelManager, Workbench,
};
use jc_text::TextEncoding;
use std::path::PathBuf;

pub struct TuiApp {
    pub(crate) workbench: Workbench,
    pub(crate) registry: CommandRegistry,
    pub(crate) keymap: KeyMap,
    pub(crate) menus: MenuModel,
    pub(crate) focus: FocusManager,
    pub(crate) panels: PanelManager,
    pub(crate) overlay: UiOverlay,
    pub(crate) status: String,
    pub(crate) project_root: Option<PathBuf>,
    pub(crate) explorer: explorer::ExplorerState,
    pub(crate) bottom_selected: usize,
    pub(crate) tasks: jc_app::JobSystem,
    pub(crate) project_search: jc_app::ProjectSearch,
    pub(crate) project_results: bool,
    pub(crate) project_draft: Option<jc_app::ProjectSearchOptions>,
    pub(crate) replace_preview: Option<jc_app::ProjectReplacePreview>,
    pub(crate) project_replace: Option<jc_app::ProjectReplacePreview>,
    replace_files: usize,
    replace_conflicts: usize,
    should_exit: bool,
    default_encoding: Option<TextEncoding>,
}

impl TuiApp {
    pub fn new(options: LaunchOptions) -> Self {
        Self::with_wakeup(options, jc_app::Wakeup::default())
    }
    pub(crate) fn with_wakeup(options: LaunchOptions, wakeup: jc_app::Wakeup) -> Self {
        let registry = default_registry();
        let keymap = default_keymap();
        let menus = default_menu_model();
        debug_assert!(keymap.validate(&registry).is_empty());
        debug_assert!(menus.validate(&registry).is_empty());

        let mut app = Self {
            workbench: Workbench::default(),
            registry,
            keymap,
            menus,
            focus: FocusManager::default(),
            panels: PanelManager::default(),
            overlay: UiOverlay::None,
            status: "Ready".to_owned(),
            project_root: None,
            explorer: explorer::ExplorerState::default(),
            bottom_selected: 0,
            project_search: jc_app::ProjectSearch::new(wakeup.clone()),
            project_results: false,
            project_draft: None,
            replace_preview: None,
            project_replace: None,
            replace_files: 0,
            replace_conflicts: 0,
            tasks: jc_app::JobSystem::new(jc_app::JobLimits::default(), wakeup)
                .expect("valid default task quotas"),
            should_exit: false,
            default_encoding: options.encoding,
        };
        for path in options.files {
            if let Err(error) = app.open_path(path) {
                app.status = error;
            }
        }
        app
    }

    pub fn frame(&mut self, width: u16, height: u16) -> Screen {
        let presentation = self.prepare_frame(width, height);
        render::render(&presentation)
    }

    pub(crate) fn poll_tasks(&mut self) -> TaskUpdate {
        let mut update = TaskUpdate::default();
        update.recover = self.project_search.take_recovery();
        update.changed = update.recover;
        if self.workbench.poll_document_search() {
            update.changed = true;
            if !self.project_results {
                self.status = format!(
                    "Document search {:?}: {} matches",
                    self.workbench.search().status,
                    self.workbench.search().matches.len()
                );
            }
        }
        update.changed |= self.apply_project_replace_step();
        if self.project_search.poll(&self.workbench, 4) {
            update.changed = true;
            if self.project_results {
                self.status = format!(
                    "Project search {:?}: {} rows",
                    self.project_search.state,
                    self.project_search.rows().len()
                );
            }
            update.recover |= self.project_search.state == jc_app::ProjectSearchState::Failed;
        }
        for completion in self.tasks.poll(&self.workbench, 4) {
            if completion.status == jc_app::TaskStatus::Panicked {
                update.recover = true;
            }
            if !completion.applicable {
                if completion.current {
                    self.status = match completion.status {
                        jc_app::TaskStatus::TimedOut => "Background task timed out",
                        jc_app::TaskStatus::LimitExceeded => "Task result exceeded its size limit",
                        jc_app::TaskStatus::InvalidRange => {
                            "Background task returned an invalid result"
                        }
                        _ => "Background task failed",
                    }
                    .to_owned();
                    update.changed = true;
                }
                continue;
            }
            for item in completion.output.items() {
                match item {
                    jc_app::TaskItem::Diagnostic { range, message } => {
                        self.workbench.push_problem(format!(
                            "document {} bytes {}..{}: {}",
                            completion.stamp.document.0, range.start, range.end, message
                        ))
                    }
                    jc_app::TaskItem::Message(message) => {
                        self.workbench.push_output(message.clone())
                    }
                    jc_app::TaskItem::ProposedEdit { range, text } => {
                        self.workbench.push_output(format!(
                            "Suggested replacement for bytes {}..{}: {}",
                            range.start, range.end, text
                        ))
                    }
                }
                update.changed = true;
            }
        }
        update
    }

    pub fn should_exit(&self) -> bool {
        self.should_exit
    }

    pub(crate) fn command_context(&self) -> jc_app::CommandContext {
        let mut context = self.workbench.context();
        context.focus = self.focus.primary();
        context.has_project = self.project_root.is_some();
        context.project_search_running = self.project_search.state
            == jc_app::ProjectSearchState::Running
            || self.project_replace.is_some();
        context.project_replace_ready = self.project_results
            && self.project_replace.is_none()
            && self.project_search.can_replace();
        if context.focus == jc_app::PrimaryFocus::Sidebar {
            context.can_open_to_side = self.panels.sidebar() == jc_app::SidebarView::Explorer
                && self
                    .explorer
                    .entries()
                    .get(self.explorer.selected())
                    .is_some_and(|entry| !entry.is_dir);
        }
        context
    }

    pub(crate) fn request_exit(&mut self) {
        self.should_exit = true;
    }
}

#[derive(Default)]
pub(crate) struct TaskUpdate {
    pub changed: bool,
    pub recover: bool,
}
