use super::{git_diff::model_rows, git_history::opaque, presentation::GitRowKind, TuiApp};
use crate::input::is_plain_text;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use jc_app::{
    git_commit::{file_flag, WriteDraft},
    present_field, FieldEdit,
};

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
        } else if view.reviewing() {
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
            let refresh_graph = outcome.reference && outcome.result.is_ok() && self.graph_showing();
            let notice = match &outcome.result {
                Ok(_) => self
                    .git_commit
                    .notice(Some(&outcome.root))
                    .unwrap_or_default()
                    .into(),
                Err(error) => error.clone(),
            };
            if self.project_root.as_ref() == Some(&outcome.root) {
                if refresh_graph {
                    self.open_git_graph();
                } else if !outcome.reference {
                    self.git_revision.cancel();
                }
                if outcome.refresh_files && outcome.result.is_ok() {
                    self.refresh_explorer();
                }
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
        let mut rows = view
            .has_input()
            .then(|| {
                field
                    .rows
                    .first()
                    .filter(|s| !s.is_empty())
                    .cloned()
                    .unwrap_or_else(|| {
                        if matches!(
                            view.draft,
                            WriteDraft::CreateReference(..) | WriteDraft::ChangeReference(..)
                        ) {
                            "Escribe el nombre de la referencia".into()
                        } else {
                            "Escribe el mensaje del commit".into()
                        }
                    })
            })
            .into_iter()
            .collect::<Vec<_>>();
        let mut caret = None;
        let hint = if self.git_commit.applying() {
            "Publicando operación Git…"
        } else if self.git_commit.busy() {
            view.pending_hint()
        } else if let Some(plan) = view.reference_change_review() {
            rows.push(match plan.destination() {
                Some(name) => format!("{} → {name}", plan.source()),
                None => format!("Se eliminará la referencia {}", plan.source()),
            });
            rows.push(format!("Objeto actual · {}", plan.target()));
            "F2 confirma el cambio · Esc cancela"
        } else if let Some(plan) = view.restore_review() {
            let pair = plan.pair();
            rows.push(opaque(&pair.path, width));
            rows.push(format!(
                "Índice → disco · {} bytes",
                pair.before.bytes().len()
            ));
            rows.push(format!(
                "Objeto · {}",
                pair.before.identity().oid().unwrap_or_default()
            ));
            if pair.after.identity().is_absent() {
                "F2 recupera el archivo · Esc cancela"
            } else {
                rows.push(format!(
                    "Respaldo en .git · {} bytes actuales",
                    pair.after.bytes().len()
                ));
                if let (WriteDraft::Restore(comparison), Some(encoding)) =
                    (&view.draft, self.git_history.encoding)
                {
                    rows.push("Vista previa · − índice · + disco".into());
                    let available =
                        usize::from(height.saturating_sub(4)).saturating_sub(rows.len());
                    let (preview, _) = model_rows(
                        &comparison.model,
                        self.git_history.hunk,
                        self.git_diff_scroll,
                        width.saturating_sub(3),
                        available.min(12),
                        encoding,
                    );
                    rows.extend(preview.into_iter().map(|row| {
                        let sign = match row.kind {
                            GitRowKind::Deleted => "−",
                            GitRowKind::Added => "+",
                            _ => " ",
                        };
                        format!("{sign} {}{}", row.text, row.eol)
                    }));
                }
                "F2 restaura el archivo · Esc cancela"
            }
        } else if let Some(plan) = view.merge_review() {
            let (active, incoming) = plan.tips();
            rows.extend([
                format!("{} → {}", plan.incoming(), plan.active()),
                plan.kind.label().into(),
                format!("Activa · {active}"),
                format!("Entrante · {incoming}"),
            ]);
            if let Some(result) = plan.result() {
                rows.push(format!("Resultado · {result}"));
            }
            let count = usize::from(height.saturating_sub(4))
                .saturating_sub(rows.len())
                .min(128);
            let first = view.first.min(plan.rows.len().saturating_sub(count));
            rows.extend(plan.rows.iter().skip(first).take(count).map(|row| {
                format!(
                    "{}  {}",
                    row.change.map_or("Conflicto", file_flag),
                    opaque(&row.path, width.saturating_sub(3)),
                )
            }));
            if plan.recovering() {
                "↑↓ archivos · F2 reanuda la fusión · Esc cancela"
            } else if plan.can_apply() {
                "↑↓ archivos · F2 confirma la fusión · Esc cancela"
            } else if plan.rows.is_empty() {
                "Esc vuelve"
            } else {
                "↑↓ conflictos · Esc vuelve"
            }
        } else if let Some(plan) = view.branch_switch_review() {
            rows.extend([
                format!("{} → {}", plan.source(), plan.destination()),
                format!("Commit de origen · {}", plan.source_target()),
                format!("Commit de destino · {}", plan.target()),
                if plan.files().is_empty() {
                    "El índice y los archivos de trabajo se conservan".into()
                } else {
                    format!(
                        "{} archivos afectados · respaldo en .git",
                        plan.files().len()
                    )
                },
            ]);
            let count = usize::from(height.saturating_sub(4))
                .saturating_sub(rows.len())
                .min(128);
            let first = view.first.min(plan.files().len().saturating_sub(count));
            rows.extend(plan.files().iter().skip(first).take(count).map(|file| {
                format!(
                    "{}  {}",
                    file_flag(file.change()),
                    opaque(file.path(), width.saturating_sub(3))
                )
            }));
            if plan.recovering() {
                "↑↓ archivos · F2 reanuda el cambio · Esc cancela"
            } else if plan.files().is_empty() {
                "F2 cambia de rama · Esc cancela"
            } else {
                "↑↓ archivos · F2 cambia de rama · Esc cancela"
            }
        } else if let Some(plan) = view.reference_review() {
            rows.push(format!("Referencia nueva · {}", plan.full_name()));
            rows.push(format!("Commit de destino · {}", plan.target()));
            "F2 crea la referencia · Esc cancela"
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
            if view.has_input() && !view.message.is_empty() {
                caret = Some(field.cursor_column as u16);
            }
            if let WriteDraft::CreateReference(_, target) = &view.draft {
                rows.push(format!("Commit de destino · {target}"));
                "Intro revisa · Esc cancela"
            } else if let WriteDraft::ChangeReference(target, _) = &view.draft {
                rows.push(format!("Referencia actual · {}", target.name));
                "Intro revisa · Esc cancela"
            } else if let WriteDraft::SwitchBranch(target) = &view.draft {
                rows.push(format!("Rama de destino · {}", target.name));
                "Intro revisa el cambio · Esc cancela"
            } else if let WriteDraft::MergeBranch(target) = &view.draft {
                rows.push(format!("Rama a incorporar en la activa · {}", target.name));
                "Intro revisa la fusión · Esc cancela"
            } else if let WriteDraft::Restore(comparison) = &view.draft {
                rows.push(opaque(&comparison.path, width));
                "Intro revisa recuperación · Esc cancela"
            } else if let Some(old) = &view.replaces {
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
