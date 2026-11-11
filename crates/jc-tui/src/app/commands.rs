use super::TuiApp;
use crate::state::{PromptKind, ReplaceField, UiOverlay};
use jc_app::{BottomPanel, CommandId, CommandOutcome, Overlay, PrimaryFocus, SidebarView};

impl TuiApp {
    pub(crate) fn dispatch(&mut self, command: CommandId) {
        // Every TUI entry point, including menu Enter, shares this gate.
        // Check before changing search/focus state or invoking any handler.
        if !self.registry.is_enabled(command, &self.command_context()) {
            self.status = self
                .registry
                .get(command)
                .map(|spec| format!("{} no está disponible en este contexto", spec.label))
                .unwrap_or_else(|| "Comando no disponible".to_owned());
            return;
        }
        if !matches!(
            command,
            CommandId::MenuFile
                | CommandId::MenuEdit
                | CommandId::MenuSearch
                | CommandId::MenuProject
                | CommandId::MenuBuild
                | CommandId::MenuTools
                | CommandId::MenuView
                | CommandId::MenuHelp
                | CommandId::PaletteOpen
                | CommandId::GitDiffHead
                | CommandId::GitDiffIndex
                | CommandId::GitDiffRefresh
                | CommandId::GitDiffCancel
                | CommandId::ViewGit
        ) {
            self.git_code.active = false;
        }
        if command == CommandId::ViewMarkdownPreview {
            self.toggle_markdown();
            return;
        }
        if self.markdown_ui.is_some()
            && !matches!(
                command,
                CommandId::MenuFile
                    | CommandId::MenuEdit
                    | CommandId::MenuSearch
                    | CommandId::MenuProject
                    | CommandId::MenuBuild
                    | CommandId::MenuTools
                    | CommandId::MenuView
                    | CommandId::MenuHelp
                    | CommandId::PaletteOpen
                    | CommandId::ViewToggleInvisibles
            )
        {
            self.markdown_ui = None;
        }
        if self.clipboard_command(command) {
            return;
        }
        if super::snippets::is_command(command) {
            self.snippet_command(command);
            return;
        }
        if !Self::keep_snippet_for(command) {
            self.snippet_ui = None;
        }
        if super::source_services::is_command(command) {
            self.open_diff_visible = false;
            self.source_command(command);
            return;
        }
        if !Self::keep_source_for(command) {
            self.source_popup = false;
            self.source_candidates = None;
        }
        if super::open_diff::is_command(command) {
            self.open_diff_command(command);
            return;
        }
        if !Self::keep_open_diff_for(command) {
            self.open_diff_visible = false;
        }
        if matches!(
            command,
            CommandId::SearchFind
                | CommandId::SearchReplace
                | CommandId::SearchFindNext
                | CommandId::SearchFindPrevious
                | CommandId::SearchSelectionNext
                | CommandId::SearchSelectionPrevious
                | CommandId::SearchHistoryNext
                | CommandId::SearchHistoryPrevious
        ) {
            self.project_results = false;
        }
        if matches!(
            command,
            CommandId::SearchProject | CommandId::SearchReplaceProject
        ) {
            self.workbench.cancel_search();
        }
        if command == CommandId::EditorOpenToSide && self.focus.primary() == PrimaryFocus::Sidebar {
            self.activate_explorer_entry_to_side();
            return;
        }
        if matches!(
            command,
            CommandId::SearchGoToPosition | CommandId::NavigateBack | CommandId::NavigateForward
        ) {
            self.workbench.cancel_search_navigation();
        }
        match self.workbench.execute(command) {
            Ok(outcome) => {
                self.handle_outcome(outcome);
                if command == CommandId::SearchCancel {
                    self.status = "Búsqueda del documento cancelada".into();
                }
                if matches!(
                    command,
                    CommandId::SearchHistoryNext | CommandId::SearchHistoryPrevious
                ) {
                    self.find_cursor = self.workbench.search().query.len();
                    match &mut self.overlay {
                        UiOverlay::Find { input } => *input = self.workbench.search().query.clone(),
                        UiOverlay::Replace { find, .. } => {
                            *find = self.workbench.search().query.clone()
                        }
                        _ => {}
                    }
                }
                if command == CommandId::FileReopenClosed {
                    self.ensure_encoding_choice();
                }
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    fn handle_outcome(&mut self, outcome: CommandOutcome) {
        match outcome {
            CommandOutcome::None => self.status = "Listo".to_owned(),
            CommandOutcome::PromptOpen => self.open_prompt(PromptKind::Open),
            CommandOutcome::PromptNew => self.open_prompt(PromptKind::New),
            CommandOutcome::PromptSaveAs => self.open_prompt(PromptKind::SaveAs),
            CommandOutcome::ShowGoToPosition => self.open_prompt(PromptKind::GoToPosition),
            CommandOutcome::ShowFind => {
                let input = self.workbench.search().query.clone();
                self.find_cursor = input.len();
                self.open_focus_overlay(Overlay::Find);
                self.overlay = UiOverlay::Find { input };
            }
            CommandOutcome::ShowReplace => {
                let find = self.workbench.search().query.clone();
                self.find_cursor = find.len();
                self.replacement_cursor = 0;
                self.open_focus_overlay(Overlay::Replace);
                self.overlay = UiOverlay::Replace {
                    find,
                    replacement: String::new(),
                    field: ReplaceField::Find,
                };
            }
            CommandOutcome::ShowPalette => {
                self.open_focus_overlay(Overlay::CommandPalette);
                self.overlay = UiOverlay::Palette {
                    input: String::new(),
                    selected: 0,
                };
            }
            CommandOutcome::OpenMenu(command) => self.open_menu(command),
            CommandOutcome::View(command) => self.handle_view(command),
            CommandOutcome::Progress(command) => {
                if let Some(action) = jc_progress::editor::action(command) {
                    match jc_progress::editor::submit(
                        action,
                        &self.workbench,
                        &mut self.tasks,
                        self.project_root.as_deref(),
                    ) {
                        Ok(()) => {
                            self.show_bottom(BottomPanel::Output);
                            self.status = "Tarea ABL en curso".into();
                        }
                        Err(error) => self.status = error,
                    }
                } else if command == CommandId::ProgressCancel {
                    self.status = jc_progress::editor::cancel(&mut self.tasks).into();
                } else {
                    self.status = "La actualización del esquema ABL queda pendiente".into();
                }
            }
            CommandOutcome::NeedsCloseDecision(tab) => {
                self.open_focus_overlay(Overlay::Dialog);
                self.overlay = UiOverlay::CloseDialog { tab };
            }
            CommandOutcome::ExitRequested => {
                if self.recovery.is_some()
                    || self.periodic_recovery.active()
                    || self.workbench.context().dirty_documents == 0
                {
                    self.request_exit();
                } else {
                    self.status =
                        "Hay documentos sin guardar; guárdalos o ciérralos primero".to_owned();
                }
            }
        }
    }

    fn handle_view(&mut self, command: CommandId) {
        use CommandId::*;
        match command {
            ViewToggleInvisibles => {
                self.status = if self.workbench.invisibles_visible() {
                    "Caracteres invisibles: representación ASCII activa"
                } else {
                    "Caracteres invisibles ocultos"
                }
                .into();
            }
            FileChooseEncoding => {
                self.open_focus_overlay(Overlay::Dialog);
                self.overlay = UiOverlay::EncodingChoice;
            }
            FileQuickOpen => self.open_quick_open(),
            EditorSplitRight
            | EditorOneGroup
            | EditorFocusPrimary
            | EditorFocusSecondary
            | EditorTogglePin
            | EditorToggleScrollLink
            | EditorOpenToSide
            | NavigateBack
            | NavigateForward => {
                let _ = self.focus.focus(PrimaryFocus::Editor, &self.panels);
            }
            FileOpenFolder => self.open_prompt(PromptKind::OpenFolder),
            ViewExplorer => self.show_sidebar(SidebarView::Explorer),
            ExplorerRefresh => self.refresh_explorer(),
            ExplorerCancel => {
                self.cancel_explorer();
                let _ = self.focus.focus(PrimaryFocus::Editor, &self.panels);
            }
            ViewDatabase => self.show_sidebar(SidebarView::Database),
            ViewGit => {
                self.open_git_diff();
                if self.git_code.active {
                    self.git_code_control(super::git_code::Control::History);
                }
            }
            GitDiffHead => {
                self.git_source = jc_app::git_read::GitBlobSource::Head;
                self.open_git_diff();
            }
            GitDiffIndex => {
                self.git_source = jc_app::git_read::GitBlobSource::Index;
                self.open_git_diff();
            }
            GitDiffRefresh => self.open_git_diff(),
            GitDiffCancel => {
                if self.git_code.active {
                    self.git_code_control(super::git_code::Control::Close);
                    return;
                }
                if self.git_history.immutable() {
                    self.git_revision.cancel();
                    self.status = self.git_revision.message().into();
                } else {
                    self.git_diff.cancel(&mut self.tasks);
                    self.status = self.git_diff.message().to_owned();
                }
            }
            GitConfigureExecutable => self.configure_git_executable(),
            GitRepositoryStatus | GitStatusRefresh => self.open_git_repository_status(),
            GitCloneWorktree => {
                if let Some(root) = self.project_root.clone() {
                    self.open_git_repository_status();
                    self.git_commit.open_worktree_clone(root);
                }
            }
            GitStatusCancel => {
                if self.git_history.immutable() {
                    self.git_revision.cancel();
                    self.status = self.git_revision.message().into();
                } else {
                    self.git_status.cancel();
                    self.status = self.git_status.message().to_owned();
                }
            }
            ViewSymbols => self.show_sidebar(SidebarView::Symbols),
            ViewToggleBottomPanel => {
                self.panels.toggle_bottom();
                if !self.panels.bottom_visible()
                    && self.focus.primary() == PrimaryFocus::BottomPanel
                {
                    let _ = self.focus.focus(PrimaryFocus::Editor, &self.panels);
                }
            }
            ViewProblems => self.show_bottom(BottomPanel::Problems),
            ViewOutput => self.show_bottom(BottomPanel::Output),
            ViewSearchResults => self.show_bottom(BottomPanel::SearchResults),
            ViewTerminal => {
                self.status =
                    "La consola integrada está pendiente; CIDE TUI usa la terminal actual"
                        .to_owned();
            }
            FocusNext => {
                self.focus.next(&self.panels);
            }
            FocusPrevious => {
                self.focus.previous(&self.panels);
            }
            SearchProject => {
                self.cancel_project_replace();
                self.open_prompt(PromptKind::ProjectQuery);
            }
            SearchCancelProject => {
                self.project_search.cancel();
                self.cancel_project_replace();
                self.status = "Búsqueda del proyecto cancelada".into();
            }
            SearchReplaceProject => {
                self.open_prompt(PromptKind::ProjectReplacement);
            }
            _ => {}
        }
    }

    fn show_sidebar(&mut self, view: SidebarView) {
        self.panels.show_sidebar(view);
        let _ = self.focus.focus(PrimaryFocus::Sidebar, &self.panels);
    }

    fn show_bottom(&mut self, panel: BottomPanel) {
        self.panels.show_bottom(panel);
        self.bottom_selected = match panel {
            BottomPanel::SearchResults => self.workbench.search().current.unwrap_or(0),
            _ => 0,
        };
        let _ = self.focus.focus(PrimaryFocus::BottomPanel, &self.panels);
    }
}

#[cfg(test)]
#[test]
#[ignore = "requiere un proyecto de prueba con un AVM local configurado"]
fn progress_real_tui_command_routes_to_output_and_problems() {
    use std::{
        path::PathBuf,
        thread,
        time::{Duration, Instant},
    };
    let root = PathBuf::from(std::env::var_os("CIDE_TEST_PROGRESS_PROJECT").unwrap());
    let mut app = TuiApp::new(crate::state::LaunchOptions {
        files: vec![root.join("valido.p")],
        ..Default::default()
    });
    app.set_explorer_root(root.clone()).unwrap();
    let poll = |app: &mut TuiApp| {
        assert!(
            app.tasks.has_job(jc_progress::editor::JOB_KEY),
            "{}",
            app.status
        );
        let deadline = Instant::now() + Duration::from_secs(8);
        while app.tasks.has_job(jc_progress::editor::JOB_KEY) {
            app.poll_tasks();
            assert!(Instant::now() < deadline, "{}", app.status);
            thread::sleep(Duration::from_millis(15));
        }
    };
    app.dispatch(CommandId::ProgressRun);
    poll(&mut app);
    assert!(app.panels.bottom_visible());
    assert_eq!(app.panels.bottom(), BottomPanel::Output);
    assert!(app.workbench.output().iter().any(|r| r.text == "CIDE-M3"));
    app.workbench
        .open_file(root.join("invalido.p"), None)
        .unwrap();
    app.dispatch(CommandId::ProgressCompile);
    poll(&mut app);
    assert_eq!(app.panels.bottom(), BottomPanel::Problems);
    assert!(app
        .workbench
        .problems()
        .iter()
        .any(|r| r.contains("ausente.i") && r.target.is_some()));
}
