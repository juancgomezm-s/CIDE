//! Explicit repository-status actions. No file opening from opaque Git paths.
use super::TuiApp;
use crossterm::event::{KeyCode, KeyEvent};
use jc_app::{PrimaryFocus, SidebarView};

impl TuiApp {
    pub(crate) fn prepare_local_git_status(
        &mut self,
        width: u16,
        height: u16,
    ) -> super::GitStatusPresentation {
        let mut p = self.git_status.prepare(width, height);
        if self.git_status.history_info().is_some() {
            p.message = jc_app::source_services::display_ui_label(
                &format!("H historial · A archivo · {}", p.message),
                usize::from(width).min(256),
            );
        }
        p
    }
    pub(crate) fn open_git_repository_status(&mut self) {
        self.git_history = Default::default();
        self.git_revision.cancel();
        self.panels.show_sidebar(SidebarView::GitStatus);
        let _ = self.focus.focus(PrimaryFocus::Sidebar, &self.panels);
        self.request_git_repository_status();
    }

    pub(crate) fn request_git_repository_status(&mut self) {
        let Some(root) = self.project_root.as_deref() else {
            self.git_status.clear();
            self.status = "Abre primero la carpeta raíz del repositorio".into();
            return;
        };
        self.status = match self.git_status.request(root) {
            Ok(()) => self.git_status.message().to_owned(),
            Err(error) => error,
        };
    }

    pub(crate) fn configure_git_executable(&mut self) {
        self.status = "Git está integrado en CIDE; no requiere un ejecutable externo".into();
    }

    pub(crate) fn handle_git_status_sidebar_key(&mut self, key: KeyEvent) {
        if self.handle_git_history_key(key) {
            return;
        }
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
