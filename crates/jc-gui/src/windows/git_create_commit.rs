use super::*;
use jc_app::{
    git_commit::{file_flag, CommitOperation, ResetMode, WriteDraft},
    git_merge_resolution::{MergeAction, MergePane},
    present_field, FieldEdit, Modifiers,
};
use std::sync::Arc;

pub(in crate::windows) unsafe fn paste_message(hwnd: Hwnd, token: Arc<()>) {
    let valid = |state: &State| {
        state.git_panel.is_some()
            && state
                .git_commit
                .input_token()
                .is_some_and(|current| Arc::ptr_eq(&token, &current))
    };
    let Some(owner) = super::super::owner(hwnd) else {
        return;
    };
    let (budget, smoke) = {
        let state = owner.state.borrow();
        if !valid(&state) {
            return;
        }
        (
            state.git_commit.paste_budget().unwrap(),
            state.smoke.then(|| state.smoke_clipboard.clone()),
        )
    };
    let text = match smoke {
        Some(text) => Ok(text),
        None => super::super::clipboard::read_limited(hwnd, budget),
    };
    let Some(owner) = super::super::owner(hwnd) else {
        return;
    };
    let mut state = owner.state.borrow_mut();
    if valid(&state) {
        match text {
            Ok(Some(text)) => state.git_commit.edit(FieldEdit::Insert(&text)),
            Ok(None) => {}
            Err(e) => state.error(e),
        }
        state.git_workspace.invalidate();
    }
}

impl State {
    pub(in crate::windows) fn open_git_create_commit(&mut self, amend: bool) {
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
            self.git_workspace.menu = None;
            self.git_workspace.invalidate();
        }
    }

    fn git_amend_head(&self) -> Option<(&[u8], &str)> {
        (self.git_status.state() == GitStatusTaskState::Ready
            && self.git_status.root() == self.project_root.as_deref())
        .then_some(())?;
        let info = self.git_status.result()?.info()?;
        Some((info.branch.as_deref()?, info.head.as_deref()?))
    }

    pub(in crate::windows) fn git_create_commit_key(
        &mut self,
        code: usize,
        mods: Modifiers,
    ) -> bool {
        let Some(view) = self.git_commit.view() else {
            if mods == Modifiers::CTRL.union(Modifiers::SHIFT) && matches!(code, 0x4d | 0x41) {
                self.open_git_create_commit(code == b'A' as usize);
                return true;
            }
            return false;
        };
        if code == 0x1b && mods == Modifiers::NONE {
            if !self.git_commit.request_merge_stop() && !self.git_commit.back_merge() {
                self.git_commit.cancel();
            }
        } else if view.merge_file().is_some() {
            let edit = match (code, mods) {
                (0x09, Modifiers::NONE) => {
                    self.git_commit.merge_action(MergeAction::NextPane);
                    None
                }
                (0x55, Modifiers::CTRL) => {
                    self.git_commit.merge_action(MergeAction::UseVersion);
                    None
                }
                (0x0d, Modifiers::CTRL) => {
                    self.git_commit.merge_action(MergeAction::Accept);
                    None
                }
                (0x0d, Modifiers::NONE) => {
                    self.git_commit.merge_action(MergeAction::Newline);
                    None
                }
                (0x54, Modifiers::CTRL) => Some(FieldEdit::Insert("\t")),
                (0x08, Modifiers::NONE) => Some(FieldEdit::Backspace),
                (0x2e, Modifiers::NONE) => Some(FieldEdit::Delete),
                (0x25, Modifiers::NONE) => Some(FieldEdit::Left),
                (0x27, Modifiers::NONE) => Some(FieldEdit::Right),
                (0x24, Modifiers::NONE) => Some(FieldEdit::Home),
                (0x23, Modifiers::NONE) => Some(FieldEdit::End),
                (0x26, Modifiers::NONE) => {
                    self.git_commit.scroll(-1);
                    None
                }
                (0x28, Modifiers::NONE) => {
                    self.git_commit.scroll(1);
                    None
                }
                (0x21, Modifiers::NONE) => {
                    self.git_commit.scroll(-12);
                    None
                }
                (0x22, Modifiers::NONE) => {
                    self.git_commit.scroll(12);
                    None
                }
                _ => None,
            };
            if let Some(edit) = edit {
                self.git_commit.edit(edit);
            }
        } else if view.reviewing() {
            match (code, mods) {
                (0x0d, Modifiers::CTRL) => self.submit_git_create_commit(),
                (0x0d, Modifiers::NONE) if view.choosing_mainline().is_some() => {
                    self.submit_git_create_commit();
                }
                (0x0d, Modifiers::NONE) if view.merge_conflicts().is_some() => {
                    self.git_commit.merge_action(MergeAction::Open(view.first));
                }
                (0x26, Modifiers::NONE) => self.git_commit.scroll(-1),
                (0x28, Modifiers::NONE) => self.git_commit.scroll(1),
                (0x21, Modifiers::NONE) => self.git_commit.scroll(-10),
                (0x22, Modifiers::NONE) => self.git_commit.scroll(10),
                _ => {}
            }
        } else if mods == Modifiers::NONE {
            match code {
                0x0d => self.submit_git_create_commit(),
                0x09 | 0x26 | 0x28 if matches!(view.draft, WriteDraft::Reset(..)) => {
                    self.git_commit.select_reset_mode(None);
                }
                0x08 => self.git_commit.edit(FieldEdit::Backspace),
                0x2e => self.git_commit.edit(FieldEdit::Delete),
                0x25 => self.git_commit.edit(FieldEdit::Left),
                0x27 => self.git_commit.edit(FieldEdit::Right),
                0x24 => self.git_commit.edit(FieldEdit::Home),
                0x23 => self.git_commit.edit(FieldEdit::End),
                _ => {}
            }
        }
        self.git_workspace.invalidate();
        true
    }

    pub(in crate::windows) fn submit_git_create_commit(&mut self) {
        self.git_commit
            .submit(&self.workbench, self.project_root.as_deref());
        self.git_workspace.invalidate();
    }

    pub(in crate::windows) fn poll_git_create_commit(&mut self) -> bool {
        if !self.git_commit.matches_root(self.project_root.as_deref()) {
            self.git_commit.cancel();
        }
        let (changed, outcome) = self.git_commit.poll();
        if let Some(outcome) = outcome {
            let refresh_graph = outcome.reference
                && outcome.result.is_ok()
                && self
                    .git_workspace
                    .graph
                    .as_ref()
                    .is_some_and(|view| view.showing);
            self.status = match &outcome.result {
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
                self.schedule_git_status();
            }
        }
        if changed {
            self.git_workspace.invalidate();
        }
        changed
    }

    pub(super) fn add_create_commit_button(&self, frame: &mut Frame) {
        let ready = self.git_status.state() == GitStatusTaskState::Ready;
        if ready {
            frame.footer = format!("Ctrl+Mayús+M crear commit · {}", frame.footer);
        }
        if self.git_amend_head().is_some() {
            frame.footer = format!("Ctrl+Mayús+A modificar último commit · {}", frame.footer);
        }
        if let Some(notice) = self.git_commit.notice(self.project_root.as_deref()) {
            frame.footer = format!("{notice} · {}", frame.footer);
        }
        if !ready {
            return;
        }
        let p = |n| self.dpi.pixels(n);
        for (area, left, top) in [
            (frame.toolbar, p(224), frame.toolbar.top + p(8)),
            (frame.toolbar, p(431), frame.toolbar.top + p(8)),
            (
                frame.preview,
                frame.preview.left + p(12),
                frame.preview.top + p(128),
            ),
        ] {
            let r = rect(left, top, left + p(117), top + p(24));
            if r.right <= area.right - p(8)
                && r.bottom <= area.bottom
                && frame.buttons.iter().all(|(b, _, _)| {
                    r.left >= b.right || r.right <= b.left || r.top >= b.bottom || r.bottom <= b.top
                })
                && (area.top != frame.toolbar.top || frame.toolbar.right >= p(680))
            {
                frame.buttons.push((r, "Crear commit".into(), false));
                frame.hits.actions.push((r, Intent::CreateCommit));
                break;
            }
        }
        if self.git_amend_head().is_some() {
            let area = frame.preview;
            let left = area.right - p(192);
            let top = area.top + p(32);
            let r = rect(left, top, left + p(185), top + p(25));
            if r.left >= area.left
                && r.bottom <= area.bottom
                && frame.buttons.iter().all(|(b, _, _)| {
                    r.left >= b.right || r.right <= b.left || r.top >= b.bottom || r.bottom <= b.top
                })
            {
                frame
                    .buttons
                    .push((r, "Modificar último commit".into(), false));
                frame.hits.actions.push((r, Intent::AmendCommit));
            }
        }
    }

    pub(super) fn prepare_create_commit(&self, mut f: Frame) -> Frame {
        let view = self.git_commit.view().unwrap();
        let p = |n| self.dpi.pixels(n);
        f.preview = rect(
            f.rect.left,
            f.toolbar.bottom,
            f.rect.right,
            f.rect.bottom - p(30),
        );
        f.files = rect(0, 0, 0, 0);
        f.history = None;
        f.locations = None;
        f.rows.clear();
        f.commits.clear();
        f.annotations.clear();
        f.code.clear();
        f.details.clear();
        f.rails.clear();
        f.dividers.clear();
        f.buttons.clear();
        f.hits.actions.clear();
        f.subtitle = view.title().into();
        if let (Some(plan), Some(file)) = (view.merge_conflicts(), view.merge_file()) {
            f.subtitle = "Resolver conflicto".into();
            let row_height = self.cell_height.max(p(16));
            let top = f.preview.top + p(132);
            let text_left = f.preview.left + p(12) + 48;
            let cols = ((f.preview.right - text_left - 6) / self.cell_width.max(1)).max(1);
            let rows = ((f.preview.bottom - top) / row_height).clamp(0, 64);
            let text = file.present(plan, cols as usize, rows as usize);
            f.summary = vec![opaque(file.path(plan), 256), text.note.into()];
            for (index, (line, content)) in text.rows.into_iter().take(rows as usize).enumerate() {
                let y = top + index as i32 * row_height;
                f.code.push(CodeRow {
                    rect: rect(
                        f.preview.left + p(12),
                        y,
                        f.preview.right - p(12),
                        y + row_height,
                    ),
                    old: line.to_string(),
                    new: String::new(),
                    text: content,
                    kind: 0,
                    parallel: true,
                });
            }
            if let Some((row, column)) = text.caret.filter(|(row, _)| *row < rows as usize) {
                let x = text_left + column as i32 * self.cell_width;
                let y = top + row as i32 * row_height;
                if x < f.preview.right - p(12) {
                    f.merge_caret = Some(rect(x, y + 2, x + p(2).max(1), y + row_height - 2));
                }
            }
            for (index, pane) in MergePane::ALL.into_iter().enumerate() {
                self.add_merge_button(
                    &mut f,
                    12 + 80 * index as i32,
                    76,
                    pane.label_for(plan),
                    pane == file.pane,
                    Intent::Merge(MergeAction::Pane(pane)),
                );
            }
            if file.pane != MergePane::Result {
                self.add_merge_button(
                    &mut f,
                    337,
                    108,
                    "Usar versión",
                    false,
                    Intent::Merge(MergeAction::UseVersion),
                );
            } else {
                self.add_merge_button(
                    &mut f,
                    337,
                    122,
                    "Aceptar archivo",
                    true,
                    Intent::Merge(MergeAction::Accept),
                );
            }
            self.add_merge_button(&mut f, 465, 76, "Volver", false, Intent::BackMerge);
            f.footer = view.error.clone().unwrap_or_else(|| {
                if file.pane == MergePane::Result {
                    "Tab cambia vista · Ctrl+Intro acepta archivo · Esc vuelve".into()
                } else {
                    "Tab cambia vista · Ctrl+U usa versión · Esc vuelve".into()
                }
            });
            return f;
        }
        f.footer = view.error.clone().unwrap_or_else(|| {
            if self.git_commit.applying() {
                self.git_commit
                    .merge_publication_hint()
                    .unwrap_or("Publicando operación Git…")
                    .into()
            } else if self.git_commit.busy() {
                view.pending_hint().into()
            } else if view.choosing_mainline().is_some() {
                "↑↓ elige padre · Intro muestra los cambios · Esc cancela".into()
            } else if let Some(plan) = view.merge_review() {
                if plan.reset_mode().is_some() {
                    if plan.can_apply() {
                        "Ctrl+Intro confirma · Esc vuelve a elegir modo".into()
                    } else {
                        "Esc vuelve a elegir modo".into()
                    }
                } else if plan.can_apply() {
                    if view.can_return_to_merge_choices() {
                        "Ctrl+Intro confirma · Esc vuelve a las resoluciones".into()
                    } else if view.can_return_to_mainline() {
                        "Ctrl+Intro confirma · Esc vuelve a elegir padre".into()
                    } else {
                        "↑↓ archivos · Ctrl+Intro confirma · Esc cancela".into()
                    }
                } else if plan.rows.is_empty() {
                    "Esc vuelve".into()
                } else if view.all_merge_choices_saved() {
                    "Intro abre archivo · Ctrl+Intro revisa el resultado · Esc vuelve".into()
                } else {
                    "↑↓ selecciona conflicto · Intro abre archivo · Esc vuelve".into()
                }
            } else if view.reviewing() {
                if view.review().is_some()
                    || view
                        .branch_switch_review()
                        .is_some_and(|plan| !plan.files().is_empty())
                {
                    "↑↓ recorrer archivos · Ctrl+Intro confirmar · Esc cancelar".into()
                } else {
                    "Ctrl+Intro confirmar · Esc cancelar".into()
                }
            } else {
                "Intro revisar · Esc cancelar".into()
            }
        });
        let r = rect(
            f.preview.left + p(18),
            f.preview.top + p(69),
            f.preview.right - p(18),
            f.preview.top + p(93),
        );
        let width = ((r.right - r.left - p(16)) / self.cell_width.max(1)).clamp(1, 256) as usize;
        let field = present_field(&view.message, view.cursor, width, 1);
        f.query = view.has_input().then(|| QueryRow {
            rect: r,
            text: field.rows.first().cloned().unwrap_or_default(),
            caret: (!self.git_commit.busy() && !view.reviewing())
                .then_some(field.cursor_column as i32 * self.cell_width),
        });
        let (label, offset) = if let Some(plan) = view.reference_change_review() {
            f.summary = vec![match plan.destination() {
                Some(name) => format!("{} → {name}", plan.source()),
                None => format!("Se eliminará la referencia {}", plan.source()),
            }];
            f.details_top = p(132);
            f.details = vec![format!("Objeto actual · {}", plan.target())];
            ("Confirmar cambio", 12)
        } else if let Some(plan) = view.restore_review() {
            let pair = plan.pair();
            f.summary = vec![opaque(&pair.path, 256)];
            f.details_top = p(132);
            f.details = vec![format!(
                "Índice → disco · {} bytes · {}",
                pair.before.bytes().len(),
                pair.before.identity().oid().unwrap_or_default()
            )];
            if pair.after.identity().is_absent() {
                ("Recuperar archivo", 12)
            } else {
                f.details.push(format!(
                    "Se conservarán {} bytes actuales en un respaldo dentro de .git",
                    pair.after.bytes().len()
                ));
                if let (WriteDraft::Restore(comparison), Some(encoding)) =
                    (&view.draft, self.git_workspace.history_encoding)
                {
                    f.details.push("Vista previa · − índice · + disco".into());
                    f.code = controls::project(
                        &comparison.model,
                        controls::ProjectionView {
                            hunk: self.git_workspace.history_hunk,
                            first: self.git_workspace.diff_first,
                            encoding,
                            geometry: (f.preview, f.preview.top + p(210), self.cell_height),
                            mode: self.git_workspace.diff_mode(),
                            column: self.git_workspace.diff_column,
                            cell_width: self.cell_width,
                        },
                    )
                    .cells;
                }
                ("Restaurar archivo", 12)
            }
        } else if let Some(plan) = view.choosing_mainline() {
            f.summary = vec![
                format!("Commit de fusión · {}", plan.incoming()),
                "Elige el padre que se usará como base".into(),
            ];
            let top = f.preview.top + p(138);
            let row_height = p(23).max(1);
            let count = ((f.preview.bottom - top) / row_height).max(0) as usize;
            let first = view.first.saturating_sub(count.saturating_sub(1));
            for (index, id) in plan
                .mainline_choices()
                .iter()
                .enumerate()
                .skip(first)
                .take(count)
            {
                let y = top + (index - first) as i32 * row_height;
                let r = rect(
                    f.preview.left + p(12),
                    y,
                    f.preview.right - p(12),
                    y + row_height,
                );
                f.code.push(CodeRow {
                    rect: r,
                    old: String::new(),
                    new: String::new(),
                    text: format!("Padre {} · {id}", index + 1),
                    kind: if index == view.first { 2 } else { 0 },
                    parallel: false,
                });
                if !self.git_commit.busy() {
                    f.hits.actions.push((r, Intent::SelectMainline(index)));
                }
            }
            ("Ver cambios", 12)
        } else if let Some(plan) = view.merge_abort_review() {
            f.summary = vec![
                format!("{} · {} → {}", plan.label(), plan.incoming(), plan.active()),
                if plan.reset_mode().is_some() {
                    "Se conservan los archivos de trabajo".into()
                } else {
                    format!("{} archivos que se revertirán", plan.rows.len())
                },
            ];
            f.details_top = p(132);
            if plan.restores_index() {
                f.details.push("Se restaurará el índice original".into());
            }
            if plan.restores_branch() {
                f.details
                    .push("La rama volverá a su commit anterior".into());
            }
            if let Some((number, id)) = plan.mainline() {
                f.details.push(format!("Padre principal {number} · {id}"));
            }
            let row_height = p(23).max(1);
            let top = f.preview.top + p(if plan.mainline().is_some() { 217 } else { 194 });
            let count = ((f.preview.bottom - top) / row_height).clamp(0, 128) as usize;
            let first = view.first.min(plan.rows.len().saturating_sub(count));
            for (i, row) in plan.rows.iter().skip(first).take(count).enumerate() {
                let y = top + i as i32 * row_height;
                f.code.push(CodeRow {
                    rect: rect(
                        f.preview.left + p(12),
                        y,
                        f.preview.right - p(12),
                        y + row_height,
                    ),
                    old: String::new(),
                    new: String::new(),
                    text: format!(
                        "{}  {}",
                        row.change.map_or("", file_flag),
                        opaque(&row.path, 256)
                    ),
                    kind: 0,
                    parallel: false,
                });
            }
            ("Confirmar aborto", 12)
        } else if let Some(plan) = view.merge_review() {
            f.summary = vec![
                if let Some(mode) = plan.reset_mode() {
                    format!("Reset {} · rama {}", mode.label(), plan.active())
                } else if plan.operation().is_some() {
                    format!("Rama activa · {}", plan.active())
                } else {
                    format!("{} → {}", plan.incoming(), plan.active())
                },
                plan.label().into(),
            ];
            let (active, incoming) = plan.tips();
            f.details_top = p(132);
            let origin = if plan.reset_mode().is_some() {
                "Destino"
            } else if plan.operation() == Some(CommitOperation::Revert) {
                "Commit a revertir"
            } else {
                "Entrante"
            };
            f.details = vec![
                format!("Activa · {active}"),
                format!("{origin} · {incoming}"),
            ];
            if let Some(result) = plan.result() {
                if let Some(mode) = plan.reset_mode() {
                    f.details.push(match mode {
                        ResetMode::Soft => {
                            "Diferencias entre commits · los archivos se conservan".into()
                        }
                        ResetMode::Mixed if !plan.rows.is_empty() => {
                            "Cambios en el índice · se guardará una copia del anterior".into()
                        }
                        ResetMode::Mixed => "El índice ya coincide con el destino".into(),
                    });
                } else {
                    f.details.push(format!("Resultado · {result}"));
                }
            }
            if let Some((number, id)) = plan.mainline() {
                f.details.push(format!("Padre principal {number} · {id}"));
            }
            let row_height = p(23).max(1);
            let top = f.preview.top + p(if plan.mainline().is_some() { 230 } else { 207 });
            let count = ((f.preview.bottom - top) / row_height).clamp(0, 128) as usize;
            let conflicts = view.merge_conflicts().is_some();
            let first = if conflicts {
                view.first.saturating_sub(count.saturating_sub(1))
            } else {
                view.first.min(plan.rows.len().saturating_sub(count))
            };
            for (i, row) in plan.rows.iter().skip(first).take(count).enumerate() {
                let y = top + i as i32 * row_height;
                let r = rect(
                    f.preview.left + p(12),
                    y,
                    f.preview.right - p(12),
                    y + row_height,
                );
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
                f.code.push(CodeRow {
                    rect: r,
                    old: String::new(),
                    new: String::new(),
                    text: format!(
                        "{}  {}",
                        row.change.map_or(label, file_flag),
                        opaque(&row.path, 256)
                    ),
                    kind: if conflicts && view.first == first + i {
                        2
                    } else {
                        0
                    },
                    parallel: false,
                });
                if conflicts && !self.git_commit.busy() {
                    f.hits
                        .actions
                        .push((r, Intent::Merge(MergeAction::Open(first + i))));
                }
            }
            (
                if conflicts {
                    "Revisar resultado"
                } else if plan.recovering() {
                    "Reanudar"
                } else if plan.reset_mode().is_some() {
                    "Confirmar reset"
                } else {
                    match plan.operation() {
                        Some(CommitOperation::CherryPick) => "Confirmar cherry-pick",
                        Some(CommitOperation::Revert) => "Confirmar reversión",
                        None => "Confirmar fusión",
                    }
                },
                12,
            )
        } else if let Some(plan) = view.branch_switch_review() {
            f.summary = vec![
                format!("{} → {}", plan.source(), plan.destination()),
                if plan.files().is_empty() {
                    "El índice y los archivos de trabajo se conservan".into()
                } else {
                    format!(
                        "{} archivos afectados · originales respaldados en .git",
                        plan.files().len()
                    )
                },
            ];
            f.details_top = p(132);
            f.details = vec![
                format!("Commit de origen · {}", plan.source_target()),
                format!("Commit de destino · {}", plan.target()),
            ];
            let row_height = p(23).max(1);
            let top = f.preview.top + p(194);
            let count = ((f.preview.bottom - top) / row_height).clamp(0, 128) as usize;
            let first = view.first.min(plan.files().len().saturating_sub(count));
            for (row, file) in plan.files().iter().skip(first).take(count).enumerate() {
                let y = top + row as i32 * row_height;
                f.code.push(CodeRow {
                    rect: rect(
                        f.preview.left + p(12),
                        y,
                        f.preview.right - p(12),
                        y + row_height,
                    ),
                    old: String::new(),
                    new: String::new(),
                    text: format!("{}  {}", file_flag(file.change()), opaque(file.path(), 256)),
                    kind: 0,
                    parallel: false,
                });
            }
            (
                if plan.recovering() {
                    "Reanudar cambio"
                } else {
                    "Cambiar rama"
                },
                12,
            )
        } else if let Some(plan) = view.reference_review() {
            f.summary = vec![format!("Referencia nueva · {}", plan.full_name())];
            f.details_top = p(132);
            f.details = vec![format!("Commit de destino · {}", plan.target())];
            ("Confirmar creación", 150)
        } else if let Some(plan) = view.review() {
            let (name, email) = plan.author();
            f.summary = vec![format!(
                "{} · {} archivos preparados",
                opaque(plan.branch(), 100),
                plan.files().len()
            )];
            f.details_top = p(132);
            f.details = vec![
                format!("Autor  {} <{}>", opaque(name, 128), opaque(email, 160)),
                format!(
                    "Commit  {} · {}",
                    plan.id(),
                    plan.parent()
                        .map_or_else(|| "Primer commit".into(), |p| format!("Padre {p}"))
                ),
            ];
            if let Some(old) = plan.replaced() {
                f.details[1] = format!("Reemplaza {old} → {}", plan.id());
                if view.message.is_empty() {
                    f.query.as_mut().unwrap().text = opaque(
                        plan.message().lines().next().unwrap_or_default().as_bytes(),
                        width,
                    );
                }
            }
            let row_height = p(23).max(1);
            let top = f.preview.top + p(194);
            let count = ((f.preview.bottom - top) / row_height).clamp(0, 128) as usize;
            let first = view.first.min(plan.files().len().saturating_sub(count));
            for (i, file) in plan.files().iter().skip(first).take(count).enumerate() {
                let y = top + i as i32 * row_height;
                f.code.push(CodeRow {
                    rect: rect(
                        f.preview.left + p(12),
                        y,
                        f.preview.right - p(12),
                        y + row_height,
                    ),
                    old: String::new(),
                    new: String::new(),
                    text: format!("{}  {}", file_flag(file.change), opaque(&file.path, 256)),
                    kind: match file_flag(file.change) {
                        "+" => 1,
                        "−" => -1,
                        _ => 0,
                    },
                    parallel: false,
                });
            }
            (
                if plan.replaced().is_some() {
                    "Confirmar reemplazo"
                } else {
                    "Confirmar commit"
                },
                150,
            )
        } else if let WriteDraft::CreateReference(_, target) = &view.draft {
            f.summary = vec![format!("Nombre de la referencia · commit {target}")];
            ("Revisar referencia", 12)
        } else if let WriteDraft::ChangeReference(target, _) = &view.draft {
            f.summary = vec![format!("Referencia actual · {}", target.name)];
            ("Revisar referencia", 12)
        } else if let WriteDraft::SwitchBranch(target) = &view.draft {
            f.summary = vec![format!("Rama de destino · {}", target.name)];
            ("Revisar cambio", 12)
        } else if let WriteDraft::MergeBranch(target) = &view.draft {
            f.summary = vec![format!("Rama a incorporar en la activa · {}", target.name)];
            ("Revisar fusión", 12)
        } else if let WriteDraft::Reset(mode, target) = &view.draft {
            f.summary = vec![
                format!("Commit de destino · {target}"),
                match mode {
                    ResetMode::Soft => {
                        "Soft mueve la rama y conserva el índice y los archivos".into()
                    }
                    ResetMode::Mixed => {
                        "Mixed ajusta el índice al destino y conserva los archivos".into()
                    }
                },
            ];
            if !self.git_commit.busy() {
                for (offset, choice) in [(12, ResetMode::Soft), (92, ResetMode::Mixed)] {
                    self.add_merge_button(
                        &mut f,
                        offset,
                        76,
                        choice.label(),
                        *mode == choice,
                        Intent::ResetMode(choice),
                    );
                }
            }
            ("Ver cambios", 172)
        } else if let WriteDraft::CommitOperation(_, target) = &view.draft {
            f.summary = vec![format!("Commit de origen · {target}")];
            ("Ver cambios", 12)
        } else if matches!(view.draft, WriteDraft::AbortMerge) {
            f.summary = vec!["Revisa qué se restaurará antes de abortar".into()];
            ("Revisar aborto", 12)
        } else if let WriteDraft::Restore(comparison) = &view.draft {
            f.summary = vec![opaque(&comparison.path, 256)];
            ("Revisar archivo", 12)
        } else {
            f.summary = vec![if let Some(old) = &view.replaces {
                format!(
                    "Reemplaza {} · deja vacío para conservar el mensaje",
                    old.head
                )
            } else {
                "Mensaje del commit".into()
            }];
            ("Revisar preparados", 12)
        };
        if !self.git_commit.busy()
            && (view.choosing_mainline().is_some()
                || view.all_merge_choices_saved()
                || view.merge_review().is_none_or(|plan| plan.can_apply()))
        {
            let top = f.preview.top + p(96);
            let left = f.preview.left + p(offset);
            let r = rect(left, top, left + p(135), top + p(24));
            f.buttons.push((r, label.into(), true));
            f.hits.actions.push((r, Intent::SubmitCommit));
        }
        if !self.git_commit.applying() {
            let left = f.preview.right - p(99);
            let top = f.preview.top + p(96);
            let r = rect(left, top, left + p(85), top + p(24));
            if f.buttons.iter().all(|(b, _, _)| r.left >= b.right) {
                let back = view.can_return_to_merge_choices()
                    || view.can_return_to_mainline()
                    || view
                        .merge_review()
                        .is_some_and(|plan| plan.reset_mode().is_some());
                let label = if back {
                    "Volver"
                } else if view
                    .merge_conflicts()
                    .is_some_and(|plan| plan.operation().is_some())
                {
                    "Abortar"
                } else {
                    "Cancelar"
                };
                f.buttons.push((r, label.into(), false));
                f.hits.actions.push((
                    r,
                    if back {
                        Intent::BackMerge
                    } else {
                        Intent::CancelCommit
                    },
                ));
            }
        }
        f
    }

    fn add_merge_button(
        &self,
        f: &mut Frame,
        offset: i32,
        width: i32,
        label: &str,
        active: bool,
        intent: Intent,
    ) {
        let p = |n| self.dpi.pixels(n);
        let left = f.preview.left + p(offset);
        let top = f.preview.top + p(96);
        let r = rect(left, top, left + p(width), top + p(24));
        if r.right <= f.preview.right && r.bottom <= f.preview.bottom {
            f.buttons.push((r, label.into(), active));
            f.hits.actions.push((r, intent));
        }
    }
}
