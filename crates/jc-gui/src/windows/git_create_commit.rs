use super::*;
use jc_app::{git_commit::file_flag, present_field, FieldEdit, Modifiers};
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
            self.git_commit.cancel();
        } else if view.reviewing() {
            match (code, mods) {
                (0x0d, Modifiers::CTRL) => self.submit_git_create_commit(),
                (0x26, Modifiers::NONE) => self.git_commit.scroll(-1),
                (0x28, Modifiers::NONE) => self.git_commit.scroll(1),
                (0x21, Modifiers::NONE) => self.git_commit.scroll(-10),
                (0x22, Modifiers::NONE) => self.git_commit.scroll(10),
                _ => {}
            }
        } else if mods == Modifiers::NONE {
            match code {
                0x0d => self.submit_git_create_commit(),
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
            self.status = match outcome.result {
                Ok(_) => self
                    .git_commit
                    .notice(Some(&outcome.root))
                    .unwrap_or_default()
                    .into(),
                Err(error) => error,
            };
            if self.project_root.as_ref() == Some(&outcome.root) {
                if refresh_graph {
                    self.open_git_graph();
                } else if !outcome.reference {
                    self.git_revision.cancel();
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
        f.footer = view.error.clone().unwrap_or_else(|| {
            if self.git_commit.applying() {
                "Publicando operación Git…".into()
            } else if self.git_commit.busy() {
                if view.reference.is_some() {
                    "Comprobando referencia… · Esc cancela".into()
                } else {
                    "Leyendo el índice preparado… · Esc cancela".into()
                }
            } else if view.reviewing() {
                "↑↓ recorrer archivos · Ctrl+Intro confirmar · Esc cancelar".into()
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
        f.query = Some(QueryRow {
            rect: r,
            text: field.rows.first().cloned().unwrap_or_default(),
            caret: (!self.git_commit.busy() && !view.reviewing())
                .then_some(field.cursor_column as i32 * self.cell_width),
        });
        let (label, offset) = if let Some(plan) = view.reference_review() {
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
        } else if let Some((_, target)) = &view.reference {
            f.summary = vec![format!("Nombre de la referencia · commit {target}")];
            ("Revisar referencia", 12)
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
        if !self.git_commit.busy() {
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
                f.buttons.push((r, "Cancelar".into(), false));
                f.hits.actions.push((r, Intent::CancelCommit));
            }
        }
        f
    }
}
