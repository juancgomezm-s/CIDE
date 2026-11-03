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
                .map(|spec| format!("{} is unavailable in this context", spec.label))
                .unwrap_or_else(|| "Command unavailable".to_owned());
            return;
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
        match self.workbench.execute(command) {
            Ok(outcome) => {
                self.handle_outcome(outcome);
                if command == CommandId::SearchCancel {
                    self.status = "Document search cancelled".into();
                }
                if matches!(
                    command,
                    CommandId::SearchHistoryNext | CommandId::SearchHistoryPrevious
                ) {
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
            CommandOutcome::None => self.status = "Ready".to_owned(),
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
                self.status = format!("{:?} requires a configured Progress profile (M3)", command);
            }
            CommandOutcome::NeedsCloseDecision(tab) => {
                self.open_focus_overlay(Overlay::Dialog);
                self.overlay = UiOverlay::CloseDialog { tab };
            }
            CommandOutcome::ExitRequested => {
                if self.workbench.context().dirty_documents == 0 {
                    self.request_exit();
                } else {
                    self.status = "Unsaved documents remain; save or close them first".to_owned();
                }
            }
        }
    }

    fn handle_view(&mut self, command: CommandId) {
        use CommandId::*;
        match command {
            ViewToggleInvisibles => {
                self.status = if self.workbench.invisibles_visible() {
                    "Invisible characters: ASCII spans on"
                } else {
                    "Invisible characters hidden"
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
            ViewDatabase => self.show_sidebar(SidebarView::Database),
            ViewGit => self.open_git_diff(),
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
                self.git_diff.cancel(&mut self.tasks);
                self.status = self.git_diff.message().to_owned();
            }
            GitConfigureExecutable => self.configure_git_executable(),
            GitRepositoryStatus | GitStatusRefresh => self.open_git_repository_status(),
            GitStatusCancel => {
                self.git_status.cancel();
                self.status = self.git_status.message().to_owned();
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
                    "Embedded shell is deferred; CIDE TUI uses the current terminal".to_owned();
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
                self.status = "Project search cancelled".into();
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
