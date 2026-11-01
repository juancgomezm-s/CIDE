use super::TuiApp;
use crate::state::{PromptKind, ReplaceField, UiOverlay};
use jc_app::{
    BottomPanel, CommandId, CommandOutcome, Overlay, PrimaryFocus, SidebarView,
};

impl TuiApp {
    pub(crate) fn dispatch(&mut self, command: CommandId) {
        match self.workbench.execute(command) {
            Ok(outcome) => self.handle_outcome(outcome),
            Err(error) => self.status = error.to_string(),
        }
    }

    fn handle_outcome(&mut self, outcome: CommandOutcome) {
        match outcome {
            CommandOutcome::None => self.status = "Ready".to_owned(),
            CommandOutcome::PromptOpen => self.open_prompt(PromptKind::Open),
            CommandOutcome::PromptNew => self.open_prompt(PromptKind::New),
            CommandOutcome::PromptSaveAs => self.open_prompt(PromptKind::SaveAs),
            CommandOutcome::ShowFind => {
                let input = self.workbench.search().query.clone();
                self.open_focus_overlay(Overlay::Find);
                self.overlay = UiOverlay::Find { input };
            }
            CommandOutcome::ShowReplace => {
                let find = self.workbench.search().query.clone();
                self.open_focus_overlay(Overlay::Replace);
                self.overlay = UiOverlay::Replace {
                    find,
                    replacement: String::new(),
                    field: ReplaceField::Find,
                };
            }
            CommandOutcome::ShowPalette => {
                self.open_focus_overlay(Overlay::CommandPalette);
                self.overlay = UiOverlay::Palette { input: String::new(), selected: 0 };
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
            FileOpenFolder => self.open_prompt(PromptKind::OpenFolder),
            ViewExplorer => self.show_sidebar(SidebarView::Explorer),
            ViewDatabase => self.show_sidebar(SidebarView::Database),
            ViewGit => self.show_sidebar(SidebarView::Git),
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
                self.status = "Embedded shell is deferred; JC-IDE itself remains terminal-native".to_owned();
            }
            FocusNext => { self.focus.next(&self.panels); }
            FocusPrevious => { self.focus.previous(&self.panels); }
            EditDuplicate | EditDeleteLine | EditMoveLineUp | EditMoveLineDown
            | EditToggleLineComment | SearchProject | SearchReplaceProject => {
                let label = self.registry.get(command).map_or("Command", |spec| spec.label);
                self.status = format!("{label} is not implemented in this M2 checkpoint");
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
        let _ = self.focus.focus(PrimaryFocus::BottomPanel, &self.panels);
    }
}
