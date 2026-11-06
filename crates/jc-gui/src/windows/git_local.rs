//! Native local Git comparisons over accepted immutable sources.
use super::*;
use jc_app::{git_revision::PairKind, Modifiers};

impl State {
    pub(super) fn add_git_sources(&self, frame: &mut Frame) {
        let l = Layout::new(self);
        let p = |n| self.dpi.pixels(n);
        let available = (l.preview.right - l.preview.left - p(24)).max(0);
        let width = ((available - p(12)) / 4).clamp(p(90), p(126));
        for (i, (name, intent, selected)) in [
            (
                "HEAD → buffer",
                Intent::Source(GitBlobSource::Head),
                self.git_workspace.local_kind.is_none() && self.git_source == GitBlobSource::Head,
            ),
            (
                "Índice → buffer",
                Intent::Source(GitBlobSource::Index),
                self.git_workspace.local_kind.is_none() && self.git_source == GitBlobSource::Index,
            ),
            (
                "Preparados",
                Intent::Pair(PairKind::Prepared),
                self.git_workspace.local_kind == Some(PairKind::Prepared),
            ),
            (
                "En disco",
                Intent::Pair(PairKind::Worktree),
                self.git_workspace.local_kind == Some(PairKind::Worktree),
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let left = l.preview.left + p(12) + i as i32 * (width + p(4));
            let top = l.preview.top + p(96);
            let r = rect(left, top, left + width, top + p(24));
            if r.right <= l.preview.right - p(12) && r.bottom < l.preview.bottom {
                frame.buttons.push((r, name.into(), selected));
                frame.hits.actions.push((r, intent));
            }
        }
    }
    pub(crate) fn request_git_local(&mut self, kind: PairKind) {
        self.git_workspace.active = Column::Comparison;
        self.git_workspace.diff_column = 0;
        self.git_workspace.history_commit = None;
        self.git_workspace.history_file = None;
        self.git_workspace.commit = None;
        self.git_workspace.local_kind = Some(kind);
        self.git_workspace.focus_diff = true;
        self.git_workspace.history_encoding = self.git_workspace.history_encoding.or(self.encoding);
        self.git_workspace.history_hunk = 0;
        self.git_workspace.diff_first = 0;
        self.git_workspace.notice = None;
        self.git_workspace.invalidate();
        self.git_diff.clear(&mut self.jobs);
        self.git_revision.cancel();
        let selected = self.git_view.selected_status();
        let path = (self.git_status.state() == GitStatusTaskState::Ready)
            .then(|| self.git_status.result())
            .flatten()
            .and_then(|r| r.status().entries().get(selected))
            .map(|e| e.path().to_vec());
        match (self.project_root.clone(), path) {
            (Some(root), Some(path)) => {
                self.git_workspace.selected_path = Some(path.clone());
                if let Err(error) = self.git_revision.request_local_file(root, path, kind) {
                    self.git_workspace.notice = Some(error);
                }
            }
            _ => {
                self.git_workspace.notice =
                    Some("Selecciona un archivo del estado Git actualizado".into())
            }
        }
    }
    pub(super) fn prepare_git_local(&mut self, mut frame: Frame) -> Frame {
        let l = Layout::new(self);
        let p = |v| self.dpi.pixels(v);
        frame.code.clear();
        frame.details.clear();
        frame.buttons.retain(|(r, _, _)| r.top < l.toolbar.bottom);
        frame.hits.actions.retain(|(_, a)| {
            matches!(
                a,
                Intent::Refresh
                    | Intent::Close
                    | Intent::File(_)
                    | Intent::Commit(_)
                    | Intent::Local
            )
        });
        self.add_git_sources(&mut frame);
        frame.summary = vec![
            match self.git_workspace.local_kind {
                Some(PairKind::Prepared) => "Cambios preparados",
                _ => "Cambios sin preparar",
            }
            .into(),
            self.git_workspace.selected_path.as_ref().map_or_else(
                || "Selecciona un archivo cambiado".into(),
                |path| opaque(path, 200),
            ),
        ];
        frame.subtitle = match self.git_workspace.local_kind {
            Some(PairKind::Prepared) => "Preparados: HEAD → índice · bytes originales",
            _ => "En disco: índice → disco · bytes originales",
        }
        .into();
        frame.footer = "Solo lectura · Índice y archivo en disco".into();
        frame.details_top = p(164);
        if let Some(notice) = &self.git_workspace.notice {
            frame.details.push(label(notice, 256));
        } else if let Some(c) = self.git_revision.comparison() {
            frame.subtitle = format!(
                "{} · {}",
                c.origin.sources_label().unwrap_or_else(|| c.origin.label()),
                opaque(&c.path, 160)
            );
            let codec = rect(
                l.preview.left + p(12),
                l.preview.top + p(128),
                l.preview.left + p(200),
                l.preview.top + p(152),
            );
            if codec.right < l.preview.right {
                frame.buttons.push((
                    codec,
                    self.git_workspace
                        .history_encoding
                        .map_or("Codificación: elegir".into(), |e| e.name().to_string()),
                    false,
                ));
                frame.hits.actions.push((codec, Intent::HistoryEncoding));
            }
            for (i, name, delta) in [(0, "Anterior", -1), (1, "Siguiente", 1)] {
                let r = rect(
                    l.preview.left + p(204) + i * p(82),
                    l.preview.top + p(128),
                    l.preview.left + p(282) + i * p(82),
                    l.preview.top + p(152),
                );
                if r.right < l.preview.right {
                    frame.buttons.push((r, name.into(), false));
                    frame.hits.actions.push((r, Intent::Hunk(delta)));
                }
            }
            let total = c.model.hunks().count();
            self.git_workspace.history_hunk =
                self.git_workspace.history_hunk.min(total.saturating_sub(1));
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
                            l.preview.top + p(164),
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
                    frame.details.push("Sin diferencias de contenido; revisa la presencia y el modo indicados arriba".into());
                }
            } else {
                frame
                    .details
                    .push("Elige la codificación para interpretar los bytes".into());
            }
        } else {
            frame.details.push(self.git_revision.message().into());
        }
        frame.hits.stamp = self.git_workspace_stamp();
        frame
    }
    pub(crate) fn git_local_key(&mut self, code: usize, modifiers: Modifiers) -> Action {
        if modifiers != Modifiers::NONE {
            return Action::None;
        }
        match code {
            0x1b => {
                self.git_revision.cancel();
                self.git_workspace.local_kind = None;
                self.git_panel = None;
            }
            0x74 => {
                if let Some(kind) = self.git_workspace.local_kind {
                    self.request_git_local(kind);
                }
            }
            0x09 => self.git_workspace.focus_diff = !self.git_workspace.focus_diff,
            0x0d if !self.git_workspace.focus_diff => {
                if let Some(kind) = self.git_workspace.local_kind {
                    self.request_git_local(kind);
                }
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
                if !self.git_workspace.focus_diff {
                    self.git_view.move_status(delta);
                } else if matches!(code, 0x26 | 0x28) {
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
            }
            _ => {}
        }
        self.git_workspace.invalidate();
        Action::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_frame_uses_four_explicit_sources_without_tasks_or_document_mutation() {
        let mut s = State::new(&crate::GuiOptions::default()).unwrap();
        s.width = 1200;
        s.height = 800;
        s.git_panel = Some(GitIntent::Status);
        s.git_workspace.local_kind = Some(PairKind::Prepared);
        let before = s
            .workbench
            .active_document()
            .unwrap()
            .session
            .document()
            .revision();
        let jobs = s.jobs.outstanding();
        let f = s.prepare_git_workspace().unwrap();
        assert!(!f.historical);
        assert_eq!(
            f.hits
                .actions
                .iter()
                .filter(|(_, a)| matches!(a, Intent::Source(_) | Intent::Pair(_)))
                .count(),
            4
        );
        let source_tabs: Vec<_> = f
            .hits
            .actions
            .iter()
            .filter(|(_, a)| matches!(a, Intent::Source(_) | Intent::Pair(_)))
            .map(|(r, _)| *r)
            .collect();
        for pair in source_tabs.windows(2) {
            assert_eq!(pair[0].top, pair[1].top);
            assert!(pair[0].right < pair[1].left);
        }
        assert!(source_tabs.iter().all(|r| {
            r.top == f.preview.top + s.dpi.pixels(96)
                && r.bottom == f.preview.top + s.dpi.pixels(120)
                && r.right <= f.preview.right - s.dpi.pixels(12)
        }));
        assert_eq!(f.details_top, s.dpi.pixels(164));
        assert!(!f
            .hits
            .actions
            .iter()
            .any(|(_, a)| matches!(a, Intent::Navigate)));
        assert_eq!(s.jobs.outstanding(), jobs);
        assert_eq!(
            s.workbench
                .active_document()
                .unwrap()
                .session
                .document()
                .revision(),
            before
        );
        assert_eq!(s.git_workspace.history_encoding, None);
    }
}
