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
        p.title = "Estado Git · C crear commit".into();
        if self.git_status.history_info().is_some() {
            p.message = jc_app::source_services::display_ui_label(
                &format!(
                    "F buscar commits · H historial · A archivo · L anotar · {}",
                    p.message
                ),
                usize::from(width).min(256),
            );
        }
        if let Some(notice) = self.git_index_publication_notice() {
            p.message = super::git_history::label(&format!("{notice} · {}", p.message), width);
        }
        if let Some(notice) = self.git_commit.notice(self.project_root.as_deref()) {
            p.message = super::git_history::label(&format!("{notice} · {}", p.message), width);
        }
        p
    }
    pub(crate) fn open_git_repository_status(&mut self) {
        self.git_history = Default::default();
        self.git_revision.cancel();
        let _ = self.git_index.discard_review();
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
        if !self.git_history.active
            && key.modifiers.is_empty()
            && key.kind == crossterm::event::KeyEventKind::Press
            && matches!(key.code, KeyCode::Char('c' | 'C'))
        {
            self.open_git_create_commit();
            return;
        }
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
