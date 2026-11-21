use super::TuiApp;
use crate::state::{PromptKind, ReplaceField, UiOverlay};
use jc_app::{BottomPanel, CommandId, CommandOutcome, Overlay, PrimaryFocus, SidebarView};

impl TuiApp {
    pub(crate) fn dispatch(&mut self, command: CommandId) {
        if command == CommandId::HelpAbout {
            self.overlay = UiOverlay::About;
            return;
        }
        // Every TUI entry point, including menu Enter, shares this gate.
        // Check before changing search/focus state or invoking any handler.
        if !self.registry.is_enabled(command, &self.command_context()) {
            if command == CommandId::ProjectDataConnections && self.project_root.is_none() {
                self.status = super::data_connections::PROJECT_REQUIRED.into();
                return;
            }
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
        if command == CommandId::ProjectDataConnections {
            self.open_data_connections();
            return;
        }
        if command == CommandId::FileSave && self.workbench.active_paged_byte().is_some() {
            self.status = match self.workbench.request_save_active(None) {
                Ok(_) => "Guardando archivo en segundo plano…".into(),
                Err(error) => error.to_string(),
            };
            return;
        }
        if command == CommandId::FileSaveAll {
            self.request_save_all();
            return;
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
                    if self.progress_schema.busy() {
                        self.status = "Ya hay una actualización de esquema en curso".into();
                        return;
                    }
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
                    self.status = if self.progress_schema.cancel() {
                        "Cancelando actualización de esquema ABL…"
                    } else {
                        jc_progress::editor::cancel(&mut self.tasks)
                    }
                    .into();
                } else if command == CommandId::ProgressRefreshSchema {
                    self.request_progress_schema();
                }
            }
            CommandOutcome::NeedsCloseDecision(tab) => {
                self.open_focus_overlay(Overlay::Dialog);
                self.overlay = UiOverlay::CloseDialog { tab };
            }
            CommandOutcome::ExitRequested => {
                self.request_exit();
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
            | NavigateForward
            | NavigateLastEdit => {
                let _ = self.focus.focus(PrimaryFocus::Editor, &self.panels);
            }
            FileOpenFolder => self.open_prompt(PromptKind::OpenFolder),
            ViewExplorer => self.show_sidebar(SidebarView::Explorer),
            ExplorerRefresh => self.refresh_explorer(),
            ExplorerCancel => {
                self.cancel_explorer();
                let _ = self.focus.focus(PrimaryFocus::Editor, &self.panels);
            }
            ProgressBrowseRecords => self.open_record_browser(),
            ViewDatabase => {
                if let Some(first) = self.progress_schema.show(&mut self.workbench) {
                    self.show_bottom(BottomPanel::Output);
                    self.select_schema_output(first);
                    self.status = "Esquema ABL · consulta Salida · Ver → Ver registros ABL".into();
                }
            }
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

    pub(super) fn show_bottom(&mut self, panel: BottomPanel) {
        self.panels.show_bottom(panel);
        self.bottom_output_anchor = None;
        self.bottom_selected = match panel {
            BottomPanel::SearchResults => self.workbench.search().current.unwrap_or(0),
            _ => 0,
        };
        let _ = self.focus.focus(PrimaryFocus::BottomPanel, &self.panels);
    }

    fn request_progress_schema(&mut self) {
        if self.tasks.has_job(jc_progress::editor::JOB_KEY) {
            self.status = "Ya hay una tarea Progress en curso".into();
            return;
        }
        match self
            .progress_schema
            .request(self.project_root.as_deref(), self.syntax_wakeup.clone())
        {
            Ok(()) => {
                self.show_bottom(BottomPanel::Output);
                self.status = "Leyendo esquema ABL…".into();
            }
            Err(error) => self.status = error,
        }
    }

    fn select_schema_output(&mut self, first: u64) {
        self.bottom_output_anchor = Some(first);
        self.bottom_selected = self
            .workbench
            .output()
            .partition_point(|row| row.id < first);
    }

    pub(crate) fn bottom_window(&self, count: usize, available: usize) -> (usize, usize) {
        let selected = self.bottom_selected.min(count.saturating_sub(1));
        if self.focus.primary() != PrimaryFocus::BottomPanel {
            return (count.saturating_sub(available), selected);
        }
        let mut start = selected.saturating_sub(available.saturating_sub(1));
        if let Some(id) = self
            .bottom_output_anchor
            .filter(|_| self.panels.bottom() == BottomPanel::Output)
        {
            let anchor = self.workbench.output().partition_point(|row| row.id < id);
            if anchor <= selected {
                start = start.max(anchor.min(count.saturating_sub(available)));
            }
        }
        (start, selected)
    }

    pub(super) fn poll_progress_schema(&mut self) -> bool {
        let Some(update) = self
            .progress_schema
            .poll(self.project_root.as_deref(), &mut self.workbench)
        else {
            return false;
        };
        self.status = update.message.into();
        if self.panels.bottom_visible() && self.panels.bottom() == BottomPanel::Output {
            if update.problems {
                self.panels.show_bottom(BottomPanel::Problems);
                self.bottom_selected = self.workbench.problems().len().saturating_sub(1);
            } else if let Some(first) = update.first_output_row {
                self.select_schema_output(first);
            }
        }
        true
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

#[cfg(test)]
#[test]
#[ignore = "requiere el proyecto sintético de esquema de OpenEdge"]
fn progress_real_schema_tui_refreshes_and_reopens_the_catalog() {
    let root = std::env::var_os("CIDE_TEST_PROGRESS_SCHEMA_PROJECT").unwrap();
    exercise_progress_schema_tui(root.into(), true);
}

#[cfg(test)]
#[test]
#[ignore = "requiere CIDE_TEST_PROGRESS_ERP_PROJECT con un perfil de desarrollo autorizado"]
fn progress_real_erp_tui_refreshes_and_reopens_the_catalog() {
    let root = std::env::var_os("CIDE_TEST_PROGRESS_ERP_PROJECT").unwrap();
    exercise_progress_schema_tui(root.into(), false);
}

#[cfg(test)]
fn exercise_progress_schema_tui(root: std::path::PathBuf, synthetic: bool) {
    use std::{
        thread,
        time::{Duration, Instant},
    };
    let mut app = TuiApp::new(Default::default());
    app.set_explorer_root(root).unwrap();
    app.dispatch(CommandId::ProgressRefreshSchema);
    assert!(app.progress_schema.busy(), "{}", app.status);
    let deadline = Instant::now() + Duration::from_secs(if synthetic { 8 } else { 35 });
    while app.progress_schema.busy() {
        app.poll_tasks();
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(15));
    }
    assert!(app.command_context().has_database_schema, "{}", app.status);
    assert_eq!(app.panels.bottom(), BottomPanel::Output);
    let output = app.workbench.output();
    assert!(output
        .iter()
        .any(|row| row.starts_with("Esquema ABL") && row.contains("OpenEdge AVM ")));
    assert!(output.iter().any(|row| row.starts_with("Tabla ")));
    if synthetic {
        assert!(output
            .iter()
            .any(|row| row.contains("Tabla devlogica.customer")));
    }
    let old_count = output.len();
    app.dispatch(CommandId::ViewDatabase);
    assert!(app.workbench.output().len() > old_count);
    assert!(!app.progress_schema.busy());
    let (first, selected) = app.bottom_window(app.workbench.output().len(), 7);
    assert!(first > 0);
    assert_eq!(first, selected);
    assert!(app.workbench.output()[first + 1].starts_with("Base "));
    if synthetic {
        assert!(app
            .workbench
            .output()
            .iter()
            .any(|row| row.starts_with("Base vacia")));
    }
    assert_eq!(
        app.status,
        "Esquema ABL · consulta Salida · Ver → Ver registros ABL"
    );
}
