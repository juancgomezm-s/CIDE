//! Historical Git observations never borrow or replace the editable document.
use super::*;
#[cfg(test)]
use jc_app::diff_model::OwnedDiffModel;
use jc_app::Modifiers;

impl State {
    pub(crate) fn request_git_history(&mut self, commit: String) {
        let Some(root) = self.project_root.clone() else {
            return;
        };
        self.git_workspace.active = Column::Files;
        self.git_workspace.diff_column = 0;
        self.git_workspace.local_kind = None;
        self.git_diff.clear(&mut self.jobs);
        self.git_workspace.history_commit = Some(commit.clone());
        self.git_workspace.focus_diff = false;
        self.git_workspace.history_selected = 0;
        self.git_workspace.history_file = None;
        self.git_workspace.history_hunk = 0;
        self.git_workspace.diff_first = 0;
        self.git_workspace.history_encoding = self.git_workspace.history_encoding.or(self.encoding);
        self.git_workspace.notice = None;
        self.git_workspace.invalidate();
        if let Err(error) = self.git_revision.request_revision(root, commit) {
            self.git_workspace.notice = Some(error);
        }
    }
    pub(crate) fn request_git_history_file(&mut self, index: usize) {
        let Some(root) = self.project_root.clone() else {
            return;
        };
        let Some((commit, path)) = self.git_revision.revision().and_then(|r| {
            r.files
                .get(index)
                .map(|f| (r.commit.clone(), f.path.clone()))
        }) else {
            return;
        };
        self.git_workspace.active = Column::Comparison;
        self.git_workspace.diff_column = 0;
        self.git_workspace.focus_diff = true;
        self.git_workspace.history_selected = index;
        self.git_workspace.history_file = Some(index);
        self.git_workspace.history_hunk = 0;
        self.git_workspace.diff_first = 0;
        self.git_workspace.invalidate();
        if let Err(error) = self.git_revision.request_file(root, commit, path) {
            self.git_workspace.notice = Some(error);
        }
    }
    pub(crate) fn git_history_key(&mut self, code: usize, modifiers: Modifiers) -> Action {
        if modifiers != Modifiers::NONE {
            return Action::None;
        }
        match code {
            0x1b => {
                self.git_revision.cancel();
                self.git_panel = None;
                self.git_workspace.history_commit = None;
            }
            0x74 => {
                if let Some(c) = self.git_workspace.history_commit.clone() {
                    self.request_git_history(c);
                }
            }
            0x09 => self.git_workspace.focus_diff = !self.git_workspace.focus_diff,
            0x0d if !self.git_workspace.focus_diff => {
                self.request_git_history_file(self.git_workspace.history_selected)
            }
            0x26 | 0x28 | 0x21 | 0x22 | 0x24 | 0x23 => {
                let delta = match code {
                    0x26 => -1,
                    0x28 => 1,
                    0x21 => -10,
                    0x22 => 10,
                    0x24 => isize::MIN,
                    _ => isize::MAX,
                };
                if self.git_workspace.focus_diff {
                    if matches!(code, 0x26 | 0x28) {
                        self.git_workspace.history_hunk =
                            self.git_workspace.history_hunk.saturating_add_signed(delta);
                        self.git_workspace.diff_first = 0;
                    } else {
                        self.git_workspace.diff_first = self
                            .git_workspace
                            .diff_first
                            .saturating_add_signed(delta)
                            .min(
                                self.git_workspace
                                    .diff_total
                                    .saturating_sub(self.git_workspace.diff_page),
                            );
                    }
                } else {
                    let total = self.git_revision.revision().map_or(0, |r| r.files.len());
                    self.git_workspace.history_selected = self
                        .git_workspace
                        .history_selected
                        .saturating_add_signed(delta)
                        .min(total.saturating_sub(1));
                }
            }
            _ => {}
        }
        self.git_workspace.invalidate();
        Action::None
    }
    pub(super) fn prepare_git_history(&mut self, mut frame: Frame) -> Frame {
        self.git_revision
            .invalidate_context(self.project_root.as_deref());
        let l = Layout::new(self);
        let p = |v| self.dpi.pixels(v);
        frame.historical = true;
        frame.details_top = p(112);
        frame.rows.clear();
        frame.code.clear();
        frame.message.clear();
        frame.buttons.retain(|(r, _, _)| r.top < l.toolbar.bottom);
        frame
            .hits
            .actions
            .retain(|(_, a)| matches!(a, Intent::Refresh | Intent::Close | Intent::Commit(_)));
        for (n, name, intent) in [
            (0, "Cambios locales", Intent::Local),
            (1, "Resumen", Intent::HistorySummary),
        ] {
            let r = rect(
                l.preview.left + p(12) + n * p(130),
                l.preview.top + p(40),
                l.preview.left + p(136) + n * p(130),
                l.preview.top + p(72),
            );
            if r.right < l.preview.right {
                frame.buttons.push((r, name.into(), false));
                frame.hits.actions.push((r, intent));
            }
        }
        let revision = self.git_revision.revision();
        let files = revision.map_or(&[][..], |r| r.files.as_slice());
        let capacity =
            ((l.files.bottom - l.files.top - l.head) / l.row.max(1)).clamp(0, 128) as usize;
        let selected = self
            .git_workspace
            .history_selected
            .min(files.len().saturating_sub(1));
        self.git_workspace.history_selected = selected;
        let first = selected
            .saturating_sub(capacity.saturating_sub(1))
            .min(files.len().saturating_sub(capacity));
        for (index, f) in files.iter().enumerate().skip(first).take(capacity) {
            let top = l.files.top + l.head + (index - first) as i32 * l.row;
            let r = rect(
                l.files.left,
                top,
                l.files.right,
                (top + l.row).min(l.files.bottom),
            );
            let path = opaque(&f.path, 256);
            let (tag, color, kind) = if f.before.is_none() {
                ("A", GREEN, "Agregado")
            } else if f.after.is_none() {
                ("D", RED, "Eliminado")
            } else {
                ("M", shell::ACCENT, "Modificado")
            };
            frame.rows.push(FileRow {
                rect: r,
                name: path.rsplit('/').next().unwrap_or(&path).into(),
                detail: format!("{kind} · {path}"),
                tag,
                color,
                selected: index == selected,
            });
            frame.hits.actions.push((r, Intent::HistoryFile(index)));
        }
        frame.count = format!("{} archivos", files.len());
        frame.subtitle = "Archivos de la revisión · comparación con primer padre".into();
        frame.footer="Solo lectura · Tab cambia lista/comparación · Intro compara · F5 actualiza · Esc vuelve".into();
        if let Some(r) = revision {
            frame.details = vec![
                format!("Commit: {}", r.commit),
                r.parent
                    .as_ref()
                    .map_or("Commit raíz · árbol anterior vacío".into(), |parent| {
                        format!("Primer padre: {parent}")
                    }),
                format!(
                    "{} archivos cambiados · {} padres",
                    r.files.len(),
                    r.parents.len()
                ),
            ];
            if r.limited {
                frame.details.push("Metadatos de padres limitados".into());
            }
            if files.is_empty() {
                frame.message = "La revisión no cambia archivos frente al padre elegido".into();
            }
        } else {
            frame.message = self.git_revision.message().into();
            frame.details.clear();
        }
        if self.git_workspace.history_file.is_some() {
            frame.details.clear();
            frame.subtitle = self.git_revision.message().into();
            if let Some(c) = self.git_revision.comparison() {
                frame.subtitle = format!("{} · revisión histórica", opaque(&c.path, 200));
                let r = rect(
                    l.preview.left + p(12),
                    l.preview.top + p(76),
                    l.preview.left + p(230),
                    l.preview.top + p(102),
                );
                let name = self
                    .git_workspace
                    .history_encoding
                    .map_or("Codificación: elegir".into(), |e| {
                        format!("Codificación: {}", e.name())
                    });
                frame.buttons.push((r, name, false));
                frame.hits.actions.push((r, Intent::HistoryEncoding));
                let total = c.model.hunks().count();
                self.git_workspace.history_hunk =
                    self.git_workspace.history_hunk.min(total.saturating_sub(1));
                for (i, name, delta) in [(0, "Anterior", -1), (1, "Siguiente", 1)] {
                    let r = rect(
                        l.preview.left + p(240) + i * p(86),
                        l.preview.top + p(76),
                        l.preview.left + p(322) + i * p(86),
                        l.preview.top + p(102),
                    );
                    if r.right < l.preview.right {
                        frame.buttons.push((r, name.into(), false));
                        frame.hits.actions.push((r, Intent::Hunk(delta)));
                    }
                }
                if let Some(encoding) = self.git_workspace.history_encoding {
                    let projection = controls::project(
                        &c.model,
                        controls::ProjectionView {
                            hunk: self.git_workspace.history_hunk,
                            first: self.git_workspace.diff_first,
                            encoding,
                            geometry: (
                                rect(
                                    l.preview.left,
                                    l.preview.top,
                                    l.preview.right - p(12),
                                    l.preview.bottom - p(14),
                                ),
                                l.preview.top + p(108),
                                self.cell_height,
                            ),
                            mode: self.git_workspace.diff_mode(),
                            column: self.git_workspace.diff_column,
                            cell_width: self.cell_width,
                        },
                    );
                    let total = projection.total;
                    frame.code = self.git_workspace.set_projection(projection);
                    if total == 0 {
                        frame
                            .details
                            .push("El archivo no tiene cambios de contenido".into());
                    }
                } else {
                    frame
                        .details
                        .push("Elige la codificación para interpretar los bytes históricos".into());
                }
            }
        }
        frame.hits.stamp = self.git_workspace_stamp();
        frame
    }
}

#[cfg(test)]
fn historical_rows(
    model: &OwnedDiffModel,
    hunk: usize,
    first: usize,
    page: usize,
    encoding: TextEncoding,
    geometry: (Rect, i32, i32),
) -> (Vec<CodeRow>, usize, usize) {
    let (rows, first, total, _) = compare_rows::rows(
        model,
        hunk,
        first,
        page,
        encoding,
        geometry,
        DiffView::Unified,
        0,
    );
    (rows, first, total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jc_app::diff_model::DiffLimits;
    use std::sync::Arc;

    #[test]
    fn historical_addition_and_deletion_render_exact_owned_sources_without_document() {
        let bytes = b"uno\r\ndos\xff\n".to_vec();
        for added in [true, false] {
            let (a, b) = if added {
                (Vec::new(), bytes.clone())
            } else {
                (bytes.clone(), Vec::new())
            };
            let model =
                OwnedDiffModel::compute(Arc::new(a), Arc::new(b), DiffLimits::default(), || false);
            let (rows, first, total) = historical_rows(
                &model,
                0,
                usize::MAX,
                1,
                TextEncoding::Utf8,
                (rect(0, 0, 800, 600), 100, 18),
            );
            assert_eq!(rows.len(), 1);
            assert_eq!(first, total - 1);
            assert_eq!(rows[0].kind, if added { 1 } else { -1 });
            assert!(rows[0].text.contains("\\xFF"));
            assert_eq!(
                model.source_bytes(if added {
                    DiffSide::Right
                } else {
                    DiffSide::Left
                }),
                bytes
            );
        }
    }

    #[test]
    fn historical_frame_and_codec_choice_do_not_admit_work_or_use_editor_codec() {
        let mut state = State::new(&crate::GuiOptions::default()).unwrap();
        state.width = 1200;
        state.height = 800;
        state.git_panel = Some(GitIntent::Status);
        state.git_workspace.history_commit = Some("1".repeat(40));
        let revision = state
            .workbench
            .active_document()
            .unwrap()
            .session
            .document()
            .revision();
        let jobs = state.jobs.outstanding();
        let frame = state.prepare_git_workspace().unwrap();
        assert!(frame.historical);
        assert!(frame.code.is_empty());
        assert_eq!(state.git_workspace.history_encoding, None);
        assert_eq!(state.jobs.outstanding(), jobs);
        assert_eq!(
            state
                .workbench
                .active_document()
                .unwrap()
                .session
                .document()
                .revision(),
            revision
        );
        state.git_history_key(0x1b, Modifiers::NONE);
        assert!(state.git_panel.is_none());
        assert_eq!(
            state
                .workbench
                .active_document()
                .unwrap()
                .session
                .document()
                .revision(),
            revision
        );
    }
}
