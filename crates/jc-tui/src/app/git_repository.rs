//! Explicit repository-status actions. No file opening from opaque Git paths.
use super::TuiApp;
use crate::state::PromptKind;
use crossterm::event::{KeyCode, KeyEvent};
use jc_app::{PrimaryFocus, SidebarView};

#[derive(Clone, Copy)]
pub(crate) enum GitConfigurationTarget {
    Diff,
    Status,
}

impl TuiApp {
    pub(crate) fn open_git_repository_status(&mut self) {
        self.panels.show_sidebar(SidebarView::GitStatus);
        let _ = self.focus.focus(PrimaryFocus::Sidebar, &self.panels);
        self.request_git_repository_status();
    }

    pub(crate) fn request_git_repository_status(&mut self) {
        let Some(root) = self.project_root.as_deref() else {
            self.git_status.clear();
            self.status = "Open the repository root folder first".into();
            return;
        };
        let Some(executable) = self.git_executable.as_deref() else {
            self.git_status.clear();
            self.git_configuration_target = Some(GitConfigurationTarget::Status);
            self.status = "Choose an absolute Git executable path".into();
            self.open_prompt(PromptKind::GitExecutable);
            return;
        };
        self.git_configuration_target = None;
        self.status = match self.git_status.request(root, executable) {
            Ok(()) => self.git_status.message().to_owned(),
            Err(error) => error,
        };
    }

    pub(crate) fn configure_git_executable(&mut self) {
        self.git_configuration_target = match self.panels.sidebar() {
            SidebarView::Git => Some(GitConfigurationTarget::Diff),
            SidebarView::GitStatus => Some(GitConfigurationTarget::Status),
            _ => None,
        };
        self.open_prompt(PromptKind::GitExecutable);
    }

    pub(crate) fn handle_git_status_sidebar_key(&mut self, key: KeyEvent) {
        if !key.modifiers.is_empty() {
            return;
        }
        match key.code {
            KeyCode::Up => {
                self.git_status.move_selection(-1);
            }
            KeyCode::Down => {
                self.git_status.move_selection(1);
            }
            KeyCode::Home => {
                self.git_status.first();
            }
            KeyCode::End => {
                self.git_status.last();
            }
            _ => {}
        }
    }
}
