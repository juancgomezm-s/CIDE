use super::{git_history::opaque, TuiApp};
use crate::input::is_plain_text;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use jc_app::{git_commit::file_flag, present_field, FieldEdit};

pub(crate) struct CommitPresentation {
    pub title: &'static str,
    pub rows: Vec<String>,
    pub hint: String,
    pub caret: Option<u16>,
}

impl TuiApp {
    pub(super) fn open_git_create_commit(&mut self, amend: bool) {
        if matches!(
            self.git_index.state(),
            jc_app::git_index::IndexState::Applying
        ) {
            self.status = "Espera a que termine el cambio del índice".into();
            return;
        }
        if let Some(root) = self.project_root.clone() {
            let replaces = if amend {
                let Some((branch, head)) = self.git_amend_head() else {
                    self.status = "Actualiza Estado Git y selecciona una rama con commits".into();
                    return;
                };
                Some(jc_app::git_commit::AmendTarget {
                    branch: branch.to_vec(),
                    head: head.to_owned(),
                })
            } else {
                None
            };
            let _ = self.git_index.discard_review();
            self.git_commit.open(root, replaces);
        }
    }

    pub(super) fn git_amend_head(&self) -> Option<(&[u8], &str)> {
        let info = self.git_status.history_info()?;
        Some((info.branch.as_deref()?, info.head.as_deref()?))
    }

    pub(super) fn handle_git_create_commit_key(&mut self, key: KeyEvent) -> bool {
        let Some(view) = self.git_commit.view() else {
            return false;
        };
        let plain = key.modifiers.is_empty();
        if key.code == KeyCode::Esc && plain {
            self.git_commit.cancel();
        } else if view.review().is_some() {
            match key.code {
                KeyCode::F(2) if plain && key.kind == KeyEventKind::Press => self
                    .git_commit
                    .submit(&self.workbench, self.project_root.as_deref()),
                KeyCode::Up if plain => self.git_commit.scroll(-1),
                KeyCode::Down if plain => self.git_commit.scroll(1),
                KeyCode::PageUp if plain => self.git_commit.scroll(-10),
                KeyCode::PageDown if plain => self.git_commit.scroll(10),
                _ => {}
            }
        } else if key.code == KeyCode::Enter && plain && key.kind == KeyEventKind::Press {
            self.git_commit
                .submit(&self.workbench, self.project_root.as_deref());
        } else {
            let mut utf8 = [0; 4];
            let edit = match key.code {
                KeyCode::Char(c) if is_plain_text(key) => {
                    FieldEdit::Insert(c.encode_utf8(&mut utf8))
                }
                KeyCode::Backspace if plain => FieldEdit::Backspace,
                KeyCode::Delete if plain => FieldEdit::Delete,
                KeyCode::Left if plain => FieldEdit::Left,
                KeyCode::Right if plain => FieldEdit::Right,
                KeyCode::Home if plain => FieldEdit::Home,
                KeyCode::End if plain => FieldEdit::End,
                _ => return true,
            };
            self.git_commit.edit(edit);
        }
        true
    }

    pub(super) fn poll_git_create_commit(&mut self) -> bool {
        if !self.git_commit.matches_root(self.project_root.as_deref()) {
            self.git_commit.cancel();
        }
        let (changed, outcome) = self.git_commit.poll();
        if let Some(outcome) = outcome {
            let notice = match outcome.result {
                Ok(_) => self
                    .git_commit
                    .notice(Some(&outcome.root))
                    .unwrap_or_default()
                    .into(),
                Err(error) => error,
            };
            if self.project_root.as_ref() == Some(&outcome.root) {
                self.git_revision.cancel();
                let _ = self.git_status.refresh_after_index_write(&outcome.root);
            }
            self.status = notice;
        }
        changed
    }

    pub(super) fn prepare_git_create_commit(
        &self,
        width: u16,
        height: u16,
    ) -> Option<CommitPresentation> {
        let view = self.git_commit.view()?;
        let width = width.saturating_sub(4).min(256);
        let field = present_field(&view.message, view.cursor, usize::from(width), 1);
        let mut rows = vec![field
            .rows
            .first()
            .filter(|s| !s.is_empty())
            .cloned()
            .unwrap_or_else(|| "Escribe el mensaje del commit".into())];
        let mut caret = None;
        let hint = if self.git_commit.applying() {
            "Publicando commit…"
        } else if self.git_commit.busy() {
            "Leyendo el índice… · Esc cancela"
        } else if let Some(plan) = view.review() {
            let (name, email) = plan.author();
            rows.push(format!(
                "{} · {} archivos preparados",
                opaque(plan.branch(), 120),
                plan.files().len()
            ));
            rows.push(format!("{} <{}>", opaque(name, 128), opaque(email, 160)));
            rows.push(format!("Commit {}", plan.id()));
            if let Some(old) = plan.replaced() {
                if view.message.is_empty() {
                    rows[0] = opaque(
                        plan.message().lines().next().unwrap_or_default().as_bytes(),
                        width,
                    );
                }
                rows.push(format!("Reemplaza {old}"));
            }
            let count = usize::from(height.saturating_sub(4 + rows.len() as u16)).min(128);
            let first = view.first.min(plan.files().len().saturating_sub(count));
            rows.extend(plan.files().iter().skip(first).take(count).map(|file| {
                format!(
                    "{}  {}",
                    file_flag(file.change),
                    opaque(&file.path, width.saturating_sub(3))
                )
            }));
            if plan.replaced().is_some() {
                "↑↓ archivos · F2 reemplaza · Esc cancela"
            } else {
                "↑↓ archivos · F2 confirma · Esc cancela"
            }
        } else {
            if !view.message.is_empty() {
                caret = Some(field.cursor_column as u16);
            }
            if let Some(old) = &view.replaces {
                rows.push(format!("Reemplaza {}", old.head));
                "Vacío conserva mensaje · Intro revisa · Esc cancela"
            } else {
                "Intro revisa preparados · Esc cancela"
            }
        };
        Some(CommitPresentation {
            title: view.title(),
            rows,
            hint: view.error.clone().unwrap_or_else(|| hint.into()),
            caret,
        })
    }
}
