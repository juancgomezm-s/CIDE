use super::{git_diff::model_rows, git_history::opaque, presentation::GitRowKind, TuiApp};
use crate::input::is_plain_text;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use jc_app::{
    git_commit::{
        file_flag, rebase_step_label, rebase_todo_label, CommitOperation, RebaseAction,
        RebaseTodoEdit, ResetMode, WriteDraft,
    },
    git_merge_resolution::{MergeAction, MergePane},
    FieldEdit,
};

pub(crate) struct CommitPresentation {
    pub title: &'static str,
    pub rows: Vec<String>,
    pub hint: String,
    pub caret: Option<(u16, u16)>,
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
            if !self.git_commit.request_merge_stop() && !self.git_commit.back_merge() {
                self.git_commit.cancel();
            }
        } else if view.repository_menu().is_some() && plain {
            match key.code {
                KeyCode::Enter if key.kind == KeyEventKind::Press => self
                    .git_commit
                    .submit(&self.workbench, self.project_root.as_deref()),
                KeyCode::Up => self.git_commit.scroll(-1),
                KeyCode::Down => self.git_commit.scroll(1),
                KeyCode::PageUp => self.git_commit.scroll(-10),
                KeyCode::PageDown => self.git_commit.scroll(10),
                _ => {}
            }
        } else if view.merge_file().is_some() {
            let mut utf8 = [0; 4];
            let edit = match key.code {
                KeyCode::Tab if plain && key.kind == KeyEventKind::Press => {
                    self.git_commit.merge_action(MergeAction::NextPane);
                    None
                }
                KeyCode::Char('u' | 'U') if key.modifiers == KeyModifiers::CONTROL => {
                    self.git_commit.merge_action(MergeAction::UseVersion);
                    None
                }
                KeyCode::F(2) if plain && key.kind == KeyEventKind::Press => {
                    self.git_commit.merge_action(MergeAction::Accept);
                    None
                }
                KeyCode::Enter if plain => {
                    self.git_commit.merge_action(MergeAction::Newline);
                    None
                }
                KeyCode::Char('t' | 'T') if key.modifiers == KeyModifiers::CONTROL => {
                    Some(FieldEdit::Insert("\t"))
                }
                KeyCode::Char(ch) if is_plain_text(key) => {
                    Some(FieldEdit::Insert(ch.encode_utf8(&mut utf8)))
                }
                KeyCode::Backspace if plain => Some(FieldEdit::Backspace),
                KeyCode::Delete if plain => Some(FieldEdit::Delete),
                KeyCode::Left if plain => Some(FieldEdit::Left),
                KeyCode::Right if plain => Some(FieldEdit::Right),
                KeyCode::Home if plain => Some(FieldEdit::Home),
                KeyCode::End if plain => Some(FieldEdit::End),
                KeyCode::Up if plain => {
                    self.git_commit.scroll(-1);
                    None
                }
                KeyCode::Down if plain => {
                    self.git_commit.scroll(1);
                    None
                }
                KeyCode::PageUp if plain => {
                    self.git_commit.scroll(-12);
                    None
                }
                KeyCode::PageDown if plain => {
                    self.git_commit.scroll(12);
                    None
                }
                _ => None,
            };
            if let Some(edit) = edit {
                self.git_commit.edit(edit);
            }
        } else if view.reviewing() {
            match key.code {
                KeyCode::F(2) if plain && key.kind == KeyEventKind::Press => self
                    .git_commit
                    .submit(&self.workbench, self.project_root.as_deref()),
                KeyCode::Enter
                    if plain
                        && key.kind == KeyEventKind::Press
                        && (view.choosing_mainline().is_some() || view.rebase_todo().is_some()) =>
                {
                    self.git_commit
                        .submit(&self.workbench, self.project_root.as_deref())
                }
                KeyCode::Enter
                    if plain
                        && key.kind == KeyEventKind::Press
                        && view.merge_conflicts().is_some() =>
                {
                    self.git_commit.merge_action(MergeAction::Open(view.first));
                }
                KeyCode::Up | KeyCode::Down
                    if key.modifiers == KeyModifiers::CONTROL && view.rebase_todo().is_some() =>
                {
                    self.git_commit.edit_rebase_todo(RebaseTodoEdit::Move(
                        if key.code == KeyCode::Up { -1 } else { 1 },
                    ));
                }
                KeyCode::Char('p' | 'P' | 's' | 'S' | 'd' | 'D')
                    if plain && view.rebase_todo().is_some() =>
                {
                    let action = match key.code {
                        KeyCode::Char('p' | 'P') => RebaseAction::Pick,
                        KeyCode::Char('s' | 'S') => RebaseAction::Squash,
                        _ => RebaseAction::Drop,
                    };
                    self.git_commit
                        .edit_rebase_todo(RebaseTodoEdit::Action(action));
                }
                KeyCode::Up if plain => self.git_commit.scroll(-1),
                KeyCode::Down if plain => self.git_commit.scroll(1),
                KeyCode::PageUp if plain => self.git_commit.scroll(-10),
                KeyCode::PageDown if plain => self.git_commit.scroll(10),
                _ => {}
            }
        } else if key.code == KeyCode::Enter && plain && key.kind == KeyEventKind::Press {
            self.git_commit
                .submit(&self.workbench, self.project_root.as_deref());
        } else if plain
            && key.kind == KeyEventKind::Press
            && matches!(view.draft, WriteDraft::Reset(..))
            && matches!(key.code, KeyCode::Tab | KeyCode::Up | KeyCode::Down)
        {
            self.git_commit
                .step_reset_mode(if key.code == KeyCode::Up { -1 } else { 1 });
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
        if let Some(actions) = view.repository_menu() {
            let count = usize::from(height.saturating_sub(4));
            let first = view.first.saturating_sub(count.saturating_sub(1));
            let rows = actions
                .enumerate()
                .skip(first)
                .take(count)
                .map(|(index, action)| {
                    let mark = if index == view.first { "› " } else { "  " };
                    format!("{mark}{}", action.title())
                })
                .collect();
            return Some(CommitPresentation {
                title: view.title(),
                rows,
                hint: "↑↓ seleccionar · Intro abrir · Esc cerrar".into(),
                caret: None,
            });
        }
        if let (Some(plan), Some(file)) = (view.merge_conflicts(), view.merge_file()) {
            let text = file.present(
                plan,
                usize::from(width.saturating_sub(8)),
                usize::from(height.saturating_sub(7)),
            );
            let tabs = MergePane::ALL
                .into_iter()
                .map(|pane| {
                    let label = pane.label_for(plan);
                    if pane == file.pane {
                        format!("[{label}]")
                    } else {
                        label.into()
                    }
                })
                .collect::<Vec<_>>()
                .join("  ");
            let mut rows = vec![tabs, opaque(file.path(plan), width), text.note.into()];
            rows.extend(
                text.rows
                    .into_iter()
                    .map(|(line, content)| format!("{line:>6}  {content}")),
            );
            return Some(CommitPresentation {
                title: "Resolver conflicto",
                rows,
                hint: view.error.clone().unwrap_or_else(|| {
                    if file.pane == MergePane::Result {
                        "Tab vista · F2 acepta archivo · Esc vuelve".into()
                    } else {
                        "Tab vista · Ctrl+U usa versión · Esc vuelve".into()
                    }
                }),
                caret: text
                    .caret
                    .map(|(row, col)| (col as u16 + 8, row as u16 + 3)),
            });
        }
        if let Some(todo) = view.rebase_todo() {
            let mut rows = vec![
                format!("{} commits en el plan", todo.len()),
                "Combinar conserva los mensajes en el commit anterior".into(),
            ];
            let count = usize::from(height.saturating_sub(4)).saturating_sub(rows.len());
            let first = view.first.saturating_sub(count.saturating_sub(1));
            rows.extend(
                todo.iter()
                    .enumerate()
                    .skip(first)
                    .take(count)
                    .map(|(index, item)| {
                        let selected = if index == view.first { "› " } else { "  " };
                        format!("{selected}{}", rebase_todo_label(item))
                    }),
            );
            return Some(CommitPresentation {
                title: view.title(),
                rows,
                hint: view.error.clone().unwrap_or_else(|| {
                    if self.git_commit.busy() {
                        view.pending_hint().into()
                    } else {
                        "↑↓ elige · Ctrl+↑↓ mueve · P reaplica · S combina · D omite · Intro revisa"
                            .into()
                    }
                }),
                caret: None,
            });
        }
        let field = view.present_input(usize::from(width));
        let mut rows = (view.has_input() && view.repository_review().is_none())
            .then(|| {
                field
                    .rows
                    .first()
                    .filter(|s| !s.is_empty())
                    .cloned()
                    .unwrap_or_else(|| {
                        if let Some(prompt) = view.repository_prompt() {
                            prompt.into()
                        } else if matches!(
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
            self.git_commit
                .merge_publication_hint()
                .unwrap_or("Publicando operación Git…")
        } else if self.git_commit.busy() {
            view.pending_hint()
        } else if let Some(review) = view
            .repository_review()
            .filter(|_| view.merge_conflicts().is_none())
        {
            let count = usize::from(height.saturating_sub(4));
            let first = view.first.min(review.rows.len().saturating_sub(count));
            rows.extend(review.rows.iter().skip(first).take(count).cloned());
            if review.can_apply() {
                "↑↓ recorrer · F2 confirma · Esc cancela"
            } else {
                "↑↓ recorrer · Esc vuelve"
            }
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
        } else if let Some(plan) = view.choosing_mainline() {
            rows.push(format!("Commit de fusión · {}", plan.incoming()));
            rows.push("Elige el padre que se usará como base".into());
            let count = usize::from(height.saturating_sub(4)).saturating_sub(rows.len());
            let first = view.first.saturating_sub(count.saturating_sub(1));
            rows.extend(
                plan.mainline_choices()
                    .iter()
                    .enumerate()
                    .skip(first)
                    .take(count)
                    .map(|(index, id)| {
                        let selected = if index == view.first { "› " } else { "  " };
                        format!("{selected}Padre {} · {id}", index + 1)
                    }),
            );
            "↑↓ elige padre · Intro muestra los cambios · Esc cancela"
        } else if let Some(plan) = view.merge_abort_review() {
            rows.push(format!(
                "{} · {} → {}",
                plan.label(),
                plan.incoming(),
                plan.active()
            ));
            if matches!(plan.reset_mode(), Some(ResetMode::Soft | ResetMode::Mixed)) {
                rows.push("Se conservan los archivos de trabajo".into());
            } else {
                rows.push(format!("{} archivos que se revertirán", plan.rows.len()));
            }
            if plan.restores_index() {
                rows.push("Se restaurará el índice original".into());
            }
            if plan.restores_branch() {
                rows.push("La rama volverá a su commit anterior".into());
            }
            if let Some((number, id)) = plan.mainline() {
                rows.push(format!("Padre principal {number} · {id}"));
            }
            let count = usize::from(height.saturating_sub(4))
                .saturating_sub(rows.len())
                .min(128);
            let first = view.first.min(plan.rows.len().saturating_sub(count));
            rows.extend(plan.rows.iter().skip(first).take(count).map(|row| {
                format!(
                    "{}  {}",
                    row.change.map_or("", file_flag),
                    opaque(&row.path, width.saturating_sub(3))
                )
            }));
            "F2 confirma el aborto · Esc cancela"
        } else if let Some(plan) = view
            .merge_review()
            .filter(|plan| plan.is_rebase() && view.merge_conflicts().is_none())
        {
            let (active, onto) = plan.tips();
            rows.extend([
                format!("{} sobre {}", plan.active(), plan.incoming()),
                plan.label().into(),
                format!("Activa · {active}"),
                format!("Nueva base · {onto}"),
            ]);
            if let Some(result) = plan.result() {
                rows.push(format!("Resultado · {result}"));
            }
            let count = usize::from(height.saturating_sub(4))
                .saturating_sub(rows.len())
                .min(128);
            let total = plan.rebase_steps().len() + plan.rows.len();
            let first = view.first.min(total.saturating_sub(count));
            rows.extend(
                plan.rebase_steps()
                    .iter()
                    .map(rebase_step_label)
                    .chain(plan.rows.iter().map(|row| {
                        format!(
                            "{} · {}",
                            row.change.map_or("Conflicto", file_flag),
                            opaque(&row.path, width.saturating_sub(3))
                        )
                    }))
                    .skip(first)
                    .take(count),
            );
            if plan.can_apply() {
                "↑↓ recorre la revisión · F2 confirma el rebase · Esc cancela"
            } else {
                "↑↓ recorre la revisión · Esc vuelve al grafo"
            }
        } else if let Some(plan) = view.merge_review() {
            let (active, incoming) = plan.tips();
            let origin = if plan.reset_mode().is_some() {
                "Destino"
            } else if plan.operation() == Some(CommitOperation::Revert) {
                "Commit a revertir"
            } else {
                "Entrante"
            };
            rows.extend([
                if let Some(commit) = plan.rebase_conflict_commit() {
                    format!(
                        "Rebase de {} sobre {} · Conflicto en {}",
                        plan.active(),
                        plan.incoming(),
                        commit.to_hex_with_len(8)
                    )
                } else if let Some(mode) = plan.reset_mode() {
                    format!("Reset {} · rama {}", mode.label(), plan.active())
                } else if plan.operation().is_some() {
                    format!("Rama activa · {}", plan.active())
                } else {
                    format!("{} → {}", plan.incoming(), plan.active())
                },
                plan.label().into(),
                format!("Activa · {active}"),
                format!("{origin} · {incoming}"),
            ]);
            if let Some(result) = plan.result() {
                rows.push(match plan.reset_mode() {
                    Some(ResetMode::Soft) => {
                        "Diferencias entre commits · los archivos se conservan".into()
                    }
                    Some(ResetMode::Mixed) if !plan.rows.is_empty() => {
                        "Cambios en el índice · se guardará una copia del anterior".into()
                    }
                    Some(ResetMode::Mixed) => "El índice ya coincide con el destino".into(),
                    Some(ResetMode::Hard) => {
                        "Disco e índice · se creará un respaldo antes de escribir".into()
                    }
                    None => format!("Resultado · {result}"),
                });
            }
            if let Some((number, id)) = plan.mainline() {
                rows.push(format!("Padre principal {number} · {id}"));
            }
            let count = usize::from(height.saturating_sub(4))
                .saturating_sub(rows.len())
                .min(128);
            let conflicts = view.merge_conflicts().is_some();
            let first = if conflicts {
                view.first.saturating_sub(count.saturating_sub(1))
            } else {
                view.first.min(plan.rows.len().saturating_sub(count))
            };
            rows.extend(plan.rows.iter().enumerate().skip(first).take(count).map(
                |(index, row)| {
                    let label = if view.merge_choice_saved(&row.path) {
                        "Resuelto"
                    } else if plan
                        .conflict(&row.path)
                        .is_some_and(|conflict| conflict.is_rename())
                    {
                        "Renombre"
                    } else if plan
                        .conflict(&row.path)
                        .is_some_and(|conflict| conflict.is_file_directory())
                    {
                        "Archivo/carpeta"
                    } else if plan
                        .conflict(&row.path)
                        .is_some_and(|conflict| !conflict.can_resolve())
                    {
                        "Estructural"
                    } else {
                        "Conflicto"
                    };
                    let scope = if plan.reset_mode() == Some(ResetMode::Hard) {
                        if plan.hard_changes_file(&row.path) {
                            "Disco · "
                        } else {
                            "Índice · "
                        }
                    } else {
                        ""
                    };
                    format!(
                        "{}{scope}{}  {}",
                        if conflicts && index == view.first {
                            "› "
                        } else {
                            "  "
                        },
                        row.change.map_or(label, file_flag),
                        opaque(&row.path, width.saturating_sub(3)),
                    )
                },
            ));
            if plan.recovering() {
                "↑↓ archivos · F2 reanuda · Esc cancela"
            } else if plan.reset_mode() == Some(ResetMode::Hard) && plan.can_apply() {
                "F2 confirma reset hard · Esc vuelve a elegir modo"
            } else if plan.reset_mode().is_some() {
                if plan.can_apply() {
                    "F2 confirma el reset · Esc vuelve a elegir modo"
                } else {
                    "Esc vuelve a elegir modo"
                }
            } else if plan.can_apply() {
                if view.can_return_to_merge_choices() {
                    "F2 confirma · Esc vuelve a resoluciones"
                } else if view.can_return_to_mainline() {
                    "F2 confirma · Esc vuelve a elegir padre"
                } else {
                    "↑↓ archivos · F2 confirma · Esc cancela"
                }
            } else if plan.rows.is_empty() {
                "Esc vuelve"
            } else if plan.is_rebase() {
                if view.all_merge_choices_saved() {
                    "F2 continúa el rebase · Esc aborta el borrador"
                } else {
                    "↑↓ selecciona · Intro abre conflicto · Esc aborta el borrador"
                }
            } else if view.all_merge_choices_saved() {
                "Intro abre · F2 revisa el resultado · Esc vuelve"
            } else {
                "↑↓ selecciona · Intro abre conflicto · Esc vuelve"
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
            if view.has_input() && (!view.message.is_empty() || view.presenting_secret()) {
                caret = Some((field.cursor_column as u16, 0));
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
            } else if let WriteDraft::RebaseBranch(target, _) = &view.draft {
                rows.push(format!("Reaplicar la rama activa sobre {}", target.name));
                "Intro revisa el rebase · Esc cancela"
            } else if let WriteDraft::MergeBranch(target) = &view.draft {
                rows.push(format!("Rama a incorporar en la activa · {}", target.name));
                "Intro revisa la fusión · Esc cancela"
            } else if let WriteDraft::Reset(mode, target) = &view.draft {
                rows.push(format!("Commit de destino · {target}"));
                for (choice, description) in [
                    (
                        ResetMode::Soft,
                        "mueve la rama y conserva el índice y los archivos",
                    ),
                    (
                        ResetMode::Mixed,
                        "ajusta el índice al destino y conserva los archivos",
                    ),
                    (
                        ResetMode::Hard,
                        "respalda y reemplaza los archivos indicados",
                    ),
                ] {
                    let mark = if *mode == choice { "›" } else { " " };
                    rows.push(format!("{mark} {} · {description}", choice.label()));
                }
                "↑↓ o Tab elige modo · Intro muestra los cambios · Esc cancela"
            } else if let WriteDraft::CommitOperation(_, target) = &view.draft {
                rows.push(format!("Commit de origen · {target}"));
                "Intro muestra los cambios · Esc cancela"
            } else if matches!(view.draft, WriteDraft::AbortMerge) {
                rows.push("Revisa qué se restaurará antes de abortar".into());
                "Intro revisa el aborto · Esc cancela"
            } else if let WriteDraft::Restore(comparison) = &view.draft {
                rows.push(opaque(&comparison.path, width));
                "Intro revisa recuperación · Esc cancela"
            } else if let WriteDraft::Repository(draft) = &view.draft {
                if let Some(stash) = &draft.stash {
                    rows.push(format!("Reserva stash@{{{}}} · {}", stash.index, stash.oid));
                }
                "Intro revisa operación · Esc cancela"
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
