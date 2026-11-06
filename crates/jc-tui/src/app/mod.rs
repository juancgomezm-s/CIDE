mod clipboard;
#[cfg(test)]
mod clipboard_tests;
mod commands;
mod events;
mod explorer;
#[cfg(test)]
mod explorer_integration_tests;
pub(crate) mod file_browser;
#[cfg(test)]
mod file_browser_tests;
mod git_blame;
mod git_commit_search;
mod git_diff;
mod git_history;
mod git_repository;
mod git_status;
mod markdown;
mod menus;
pub(crate) mod mouse;
#[cfg(test)]
mod mouse_tests;
mod open_diff;
#[cfg(test)]
mod open_diff_tests;
#[cfg(test)]
mod overlay_closure_tests;
mod overlays;
mod paths;
mod presentation;
mod project_search;
mod quick_open;
mod recovery;
mod search_input;
mod snippets;
#[cfg(test)]
mod snippets_tests;
mod source_services;
#[cfg(test)]
mod source_services_tests;
#[cfg(test)]
mod syntax_integration_tests;
#[cfg(test)]
mod unicode_tests;
#[cfg(test)]
mod visual_spanish_tests;
#[cfg(test)]
pub(crate) use git_status::GitStatusRow;
pub(crate) use git_status::{git_status_list_height, GitStatusPresentation};
pub(crate) use presentation::explorer_scrollbar;
pub(crate) use presentation::Presentation;
pub(crate) use presentation::{GitDiffRow, GitHunkRow, GitPresentation, GitRowKind};
#[cfg(test)]
mod edit_focus_parity_tests;
#[cfg(test)]
mod git_integration_tests;
#[cfg(test)]
mod git_status_integration_tests;
#[cfg(test)]
mod invisibles_tests;
#[cfg(test)]
mod keyboard_matrix_tests;
#[cfg(test)]
mod line_command_tests;
#[cfg(test)]
mod line_edit_integration_tests;
#[cfg(test)]
mod multiline_tests;
#[cfg(test)]
mod navigation_integration_tests;
#[cfg(test)]
mod quick_open_tests;
#[cfg(test)]
mod rectangle_parity_tests;
#[cfg(test)]
mod rectangle_tests;
#[cfg(test)]
mod selection_reopen_integration_tests;
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
    pub(crate) mouse: mouse::MouseState,
    pub(crate) file_browser: file_browser::FileBrowser,
    pub(crate) markdown_ui: Option<markdown::MarkdownUi>,
    pub(crate) find_cursor: usize,
    pub(crate) replacement_cursor: usize,
    pub(crate) workbench: Workbench,
    pub(crate) registry: CommandRegistry,
    pub(crate) keymap: KeyMap,
    pub(crate) menus: MenuModel,
    pub(crate) focus: FocusManager,
    pub(crate) panels: PanelManager,
    pub(crate) overlay: UiOverlay,
    pub(crate) last_frame_height: u16,
    clipboard_transport: Option<Box<dyn clipboard::ClipboardTransport>>,
    pub(crate) status: String,
    pub(crate) project_root: Option<PathBuf>,
    pub(crate) explorer: jc_app::explorer::ProjectExplorer,
    pub(crate) explorer_view: jc_app::explorer::ExplorerView,
    pub(crate) bottom_selected: usize,
    pub(crate) tasks: jc_app::JobSystem,
    pub(crate) syntax_pending: bool,
    pub(crate) syntax_targets: [Option<(jc_app::PaneId, jc_app::TabId)>; 2],
    pub(crate) syntax_wakeup: jc_app::Wakeup,
    pub(crate) git_diff: jc_app::git_diff::GitDiffController,
    pub(crate) git_revision: jc_app::git_revision::GitRevisionController,
    pub(crate) git_history: git_history::HistoryView,
    pub(crate) open_diff: jc_app::open_diff::OpenDiffController,
    pub(crate) open_diff_view: jc_app::open_diff_view::OpenDiffView,
    pub(crate) open_diff_visible: bool,
    pub(crate) source_registry: jc_app::source_services::SourceRegistry,
    pub(crate) source: jc_app::source_services::SourceController,
    pub(crate) source_popup: bool,
    pub(crate) rename_target: Option<jc_app::snippets::SnippetTarget>,
    pub(crate) snippet_ui: Option<snippets::SnippetUi>,
    pub(crate) source_candidates: Option<(Vec<PathBuf>, usize)>,
    pub(crate) git_status: git_status::GitStatusView,
    pub(crate) quick_open: jc_app::quick_open::QuickOpenController,
    quick_open_root: Option<PathBuf>,
    pub(crate) git_cwd: PathBuf,
    pub(crate) git_source: jc_app::git_read::GitBlobSource,
    pub(crate) git_diff_scroll: usize,
    pub(crate) project_search: jc_app::ProjectSearch,
    pub(crate) project_results: bool,
    pub(crate) project_draft: Option<jc_app::ProjectSearchOptions>,
    project_search_workers: usize,
    pub(crate) replace_preview: Option<jc_app::ProjectReplacePreview>,
    pub(crate) project_replace: Option<jc_app::ProjectReplacePreview>,
    replace_files: usize,
    replace_conflicts: usize,
    should_exit: bool,
    recovery: Option<jc_app::session::EditorRecovery>,
    pub(crate) periodic_recovery: jc_app::session::PeriodicRecovery,
    default_encoding: Option<TextEncoding>,
}

impl TuiApp {
    pub fn new(options: LaunchOptions) -> Self {
        Self::with_wakeup(options, jc_app::Wakeup::default())
    }
    pub fn with_search_workers(options: LaunchOptions, workers: usize) -> std::io::Result<Self> {
        Self::with_wakeup_and_search_workers(options, jc_app::Wakeup::default(), workers)
    }
    pub(crate) fn with_wakeup(options: LaunchOptions, wakeup: jc_app::Wakeup) -> Self {
        Self::with_wakeup_and_search_workers(options, wakeup, 2)
            .expect("valid default project worker budget")
    }
    pub(crate) fn with_wakeup_and_search_workers(
        options: LaunchOptions,
        wakeup: jc_app::Wakeup,
        workers: usize,
    ) -> std::io::Result<Self> {
        crate::validate_search_workers(workers)?;
        let registry = default_registry();
        let keymap = default_keymap();
        let menus = default_menu_model();
        debug_assert!(keymap.validate(&registry).is_empty());
        debug_assert!(menus.validate(&registry).is_empty());

        let mut source_registry = jc_app::source_services::SourceRegistry::default();
        source_registry
            .register(jc_progress::source_services::descriptor())
            .expect("valid built-in provider");
        let mut app = Self {
            mouse: Default::default(),
            file_browser: file_browser::FileBrowser::new(wakeup.clone()),
            markdown_ui: None,
            workbench: Workbench::default(),
            registry,
            keymap,
            menus,
            focus: FocusManager::default(),
            panels: PanelManager::default(),
            overlay: UiOverlay::None,
            last_frame_height: 24,
            clipboard_transport: None,
            find_cursor: usize::MAX,
            replacement_cursor: usize::MAX,
            status: "Listo · F10 menús · F6 panel · Mayús+clic/arrastre · rueda".to_owned(),
            project_root: None,
            explorer: jc_app::explorer::ProjectExplorer::new(wakeup.clone(), Default::default())
                .expect("valid Explorer quotas"),
            explorer_view: Default::default(),
            bottom_selected: 0,
            syntax_pending: false,
            syntax_targets: [None, None],
            syntax_wakeup: wakeup.clone(),
            git_diff: jc_app::git_diff::GitDiffController::new(),
            git_revision: jc_app::git_revision::GitRevisionController::new(wakeup.clone()),
            git_history: Default::default(),
            open_diff: Default::default(),
            open_diff_view: Default::default(),
            open_diff_visible: false,
            source_registry,
            source: Default::default(),
            source_popup: false,
            rename_target: None,
            snippet_ui: None,
            source_candidates: None,
            git_status: git_status::GitStatusView::new(wakeup.clone()),
            quick_open: jc_app::quick_open::QuickOpenController::new(wakeup.clone()),
            quick_open_root: None,
            git_cwd: std::env::current_dir().unwrap_or_default(),
            git_source: jc_app::git_read::GitBlobSource::Head,
            git_diff_scroll: 0,
            project_search: jc_app::ProjectSearch::new(wakeup.clone()),
            project_results: false,
            project_draft: None,
            project_search_workers: workers,
            replace_preview: None,
            project_replace: None,
            replace_files: 0,
            replace_conflicts: 0,
            tasks: jc_app::JobSystem::new(jc_app::JobLimits::default(), wakeup)
                .expect("valid default task quotas"),
            should_exit: false,
            recovery: None,
            periodic_recovery: Default::default(),
            default_encoding: options.encoding,
        };
        for path in options.files {
            if let Err(error) = app.open_path(path) {
                app.status = error;
            }
        }
        Ok(app)
    }

    pub fn frame(&mut self, width: u16, height: u16) -> Screen {
        let presentation = self.prepare_frame(width, height);
        render::render(&presentation)
    }

    pub(crate) fn poll_tasks(&mut self) -> TaskUpdate {
        let mut update = TaskUpdate {
            changed: self.syntax_pending,
            ..TaskUpdate::default()
        };
        if self.syntax_pending {
            self.syntax_pending = false;
            let mut previous = None;
            for (pane, tab) in self.syntax_targets.into_iter().flatten() {
                if previous != Some(tab)
                    && self
                        .workbench
                        .panes()
                        .view(pane)
                        .and_then(|v| v.location())
                        .is_some_and(|loc| loc.tab == tab)
                {
                    self.syntax_pending |= self.workbench.continue_syntax(tab);
                    previous = Some(tab);
                }
            }
            if self.syntax_pending {
                self.syntax_wakeup.signal();
            }
        }
        if let Some(root) = &self.project_root {
            update.changed |= self.explorer.poll(root, 2);
        }
        let (browser_changed, browser_recover) = self.file_browser.poll();
        update.changed |= browser_changed;
        update.recover |= browser_recover;
        update.recover |= self.explorer.take_recovery();
        update.recover |= self.project_search.take_recovery();
        update.changed |= update.recover;
        if self.workbench.poll_document_search() {
            update.changed = true;
            if !self.project_results {
                self.status = format!(
                    "Búsqueda del documento {}: {} coincidencias",
                    self.workbench.search().status.label(),
                    self.workbench.search().matches.len()
                );
            }
        }
        update.changed |= self.apply_project_replace_step();
        if self.project_search.poll(&self.workbench, 4) {
            update.changed = true;
            if self.project_results {
                self.status = format!(
                    "Búsqueda del proyecto {}: {} filas",
                    self.project_search.state.label(),
                    self.project_search.rows().len()
                );
            }
            update.recover |= self.project_search.state == jc_app::ProjectSearchState::Failed;
        }
        for mut completion in self.tasks.poll(&self.workbench, 4) {
            if completion.status == jc_app::TaskStatus::Panicked {
                update.recover = true;
            }
            if self.source.accept(
                &self.workbench,
                self.project_root.as_deref(),
                &mut completion,
            ) {
                self.status = self.source.message().into();
                update.changed = true;
                continue;
            }
            if self.open_diff.accept(&self.workbench, &mut completion) {
                self.status = self.open_diff.message().into();
                update.changed = true;
                continue;
            }
            if self.git_diff.accept(&self.workbench, &mut completion) {
                self.status = self.git_diff.message().to_owned();
                update.changed = true;
                continue;
            }
            update.changed |= self.workbench.accept_task_completion(&completion);
        }
        update.changed |= self.source.tick(
            &self.workbench,
            self.project_root.as_deref(),
            &mut self.tasks,
        );
        update.changed |= self.open_diff.tick(&self.workbench, &mut self.tasks);
        update.changed |= self.git_diff.tick(&self.workbench, &mut self.tasks);
        let revision_changed = self.git_revision.poll(self.project_root.as_deref());
        update.changed |= revision_changed;
        update.recover |= self.git_revision.take_recovery();
        if revision_changed && self.git_history.immutable() {
            self.status = self.git_revision.message().into();
        }
        let status_changed = self.git_status.poll(self.project_root.as_deref());
        update.changed |= status_changed;
        update.recover |= self.git_status.take_recovery();
        if status_changed && !self.git_history.immutable() {
            update.recover |= self.git_status.worker_panicked();
            if self.panels.sidebar_visible()
                && self.panels.sidebar() == jc_app::SidebarView::GitStatus
            {
                self.status = format!(
                    "{} · F buscar commits · P preparados · T disco · H historial · A archivo · L anotar",
                    self.git_status.message()
                );
            }
        }
        update.changed |= self.poll_quick_open();
        update.recover |= self.quick_open.take_recovery();
        update.changed |= update.recover;
        if update.changed {
            self.mouse.frame_changed();
        }
        update
    }

    pub fn should_exit(&self) -> bool {
        self.should_exit
    }

    pub(crate) fn command_context(&self) -> jc_app::CommandContext {
        let mut context = self.workbench.context();
        context.focus = self.focus.primary();
        context.editor_input = matches!(
            self.overlay,
            UiOverlay::None | UiOverlay::Menu { .. } | UiOverlay::Palette { .. }
        );
        context.has_project = self.project_root.is_some();
        context.explorer_focused = self.focus.primary() == jc_app::PrimaryFocus::Sidebar
            && self.panels.sidebar() == jc_app::SidebarView::Explorer;
        if context.explorer_focused {
            context.clipboard_has_text = false;
            context.has_selection = false;
        }
        context.git_diff_available = cfg!(windows)
            && context.has_project
            && context.has_document
            && self.workbench.active_encoding().is_some();
        context.git_diff_running = matches!(
            self.git_diff.state(),
            jc_app::git_diff::GitDiffState::Queued | jc_app::git_diff::GitDiffState::Running
        );
        context.git_repository_available = cfg!(windows) && context.has_project;
        context.git_status_running = matches!(
            self.git_status.state(),
            jc_app::git_status_tasks::GitStatusTaskState::Queued
                | jc_app::git_status_tasks::GitStatusTaskState::Running
        );
        context.project_search_running = self.project_search.state
            == jc_app::ProjectSearchState::Running
            || self.project_replace.is_some();
        context.project_replace_ready = self.project_results
            && self.project_replace.is_none()
            && self.project_search.can_replace();
        if context.focus == jc_app::PrimaryFocus::Sidebar {
            context.can_open_to_side = self.panels.sidebar() == jc_app::SidebarView::Explorer
                && self
                    .explorer_view
                    .selected()
                    .and_then(|id| self.explorer.node(id))
                    .is_some_and(|entry| !entry.is_dir());
        }
        self.open_diff_context(&mut context);
        self.source_context(&mut context);
        self.snippet_context(&mut context);
        if self.markdown_ui.is_some() {
            context.editor_input = false;
        }
        if self.git_history.immutable() && context.focus != jc_app::PrimaryFocus::Editor {
            context.can_open_to_side = false;
        }
        if self.clipboard_transport.is_some() {
            // Menus display commands for their underlying destination. Keep
            // transport/input routing modal: only executing the item after
            // closing the menu exposes that field to a paste operation.
            context.field_paste = self.clipboard_field_budget().is_some()
                || matches!(self.overlay, UiOverlay::Menu { .. })
                    && (self.commit_search_paste_budget().is_some()
                        || self.snippet_clipboard_budget().is_some());
            if context.field_paste
                || !context.rectangular_selection && !self.workbench.clipboard_is_rectangular()
            {
                context.clipboard_has_text = context.has_document || context.field_paste;
            }
        }
        context
    }

    pub(crate) fn request_exit(&mut self) {
        match self.checkpoint_session() {
            Ok(()) => self.should_exit = true,
            Err(error) => self.status = format!("No se cerró el editor: {error}"),
        }
    }
}

#[derive(Default)]
pub(crate) struct TaskUpdate {
    pub changed: bool,
    pub recover: bool,
}
