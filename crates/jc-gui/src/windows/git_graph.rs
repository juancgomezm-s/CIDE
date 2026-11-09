use super::*;
use jc_app::{git_graph::GraphView, git_revision::GitRevisionState, Modifiers};

impl State {
    pub(in crate::windows) fn open_git_graph(&mut self) {
        let Some(root) = self.project_root.clone() else {
            return;
        };
        let _ = self.git_index.discard_review();
        self.git_workspace.menu = None;
        self.git_workspace.commit_search = None;
        self.git_workspace.path_query = None;
        self.git_workspace.blame_query = None;
        self.git_workspace.history_commit = None;
        self.git_workspace.history_file = None;
        self.git_workspace.local_kind = None;
        self.git_workspace.graph = Some(GraphView::default());
        self.git_workspace.notice = self.git_revision.request_graph(root).err();
        self.git_workspace.invalidate();
    }

    pub(super) fn add_graph_button(&self, frame: &mut Frame) {
        if self.project_root.is_none() || self.git_status.is_not_repository() {
            return;
        }
        frame.footer = format!("G grafo · {}", frame.footer);
        let p = |n| self.dpi.pixels(n);
        let area = frame.preview;
        for left in [area.right - p(280), area.left + p(12)] {
            let top = area.top + p(32);
            let r = rect(left, top, left + p(76), top + p(24));
            if r.left >= area.left
                && r.right <= area.right
                && r.bottom <= area.bottom
                && frame.buttons.iter().all(|(b, _, _)| {
                    r.left >= b.right || r.right <= b.left || r.top >= b.bottom || r.bottom <= b.top
                })
            {
                frame.buttons.push((r, "Grafo".into(), false));
                frame.hits.actions.push((r, Intent::GraphOpen));
                break;
            }
        }
    }

    pub(super) fn open_graph_commit(&mut self, index: usize) {
        let Some(id) = self
            .git_revision
            .graph()
            .and_then(|page| page.entries.get(index))
            .map(|e| e.commit.id.clone())
        else {
            return;
        };
        if let Some(view) = &mut self.git_workspace.graph {
            view.showing = false;
            view.selected = index;
        }
        self.request_git_history(id);
    }
    pub(super) fn open_graph_reference_draft(&mut self, kind: jc_app::git_commit::RefKind) {
        if self.git_index.state() == jc_app::git_index::IndexState::Applying {
            return;
        }
        let Some(view) = self
            .git_workspace
            .graph
            .as_ref()
            .filter(|view| view.showing && view.reference_selected.is_none())
        else {
            return;
        };
        let target = self
            .git_revision
            .graph()
            .and_then(|graph| graph.entries.get(view.selected))
            .map(|entry| entry.commit.id.clone());
        if let (Some(root), Some(target)) = (self.project_root.clone(), target) {
            self.git_commit.open_reference(root, kind, target);
            self.git_workspace.invalidate();
        }
    }

    fn graph_write_target(&self) -> Option<&jc_app::git_commit::ReferenceTarget> {
        let row = self.git_workspace.graph.as_ref()?.reference_selected?;
        if self.git_revision.state() != GitRevisionState::Ready
            || self.git_index.state() == jc_app::git_index::IndexState::Applying
        {
            return None;
        }
        self.git_revision.graph()?.reference_write_target(row)
    }

    pub(super) fn open_graph_reference_change(&mut self, rename: bool) {
        if let Some((root, target)) = self
            .project_root
            .clone()
            .zip(self.graph_write_target().cloned())
        {
            let _ = self.git_index.discard_review();
            self.git_commit.open_reference_change(root, target, rename);
            self.git_workspace.invalidate();
        }
    }

    fn graph_branch_target(&self) -> Option<&jc_app::git_commit::ReferenceTarget> {
        let row = self.git_workspace.graph.as_ref()?.reference_selected?;
        if self.git_revision.state() != GitRevisionState::Ready
            || self.git_index.state() == jc_app::git_index::IndexState::Applying
        {
            return None;
        }
        self.git_revision.graph()?.branch_switch_target(row)
    }

    pub(super) fn open_graph_branch_switch(&mut self) {
        if let Some((root, target)) = self
            .project_root
            .clone()
            .zip(self.graph_branch_target().cloned())
        {
            let _ = self.git_index.discard_review();
            self.git_commit.open_branch_switch(root, target);
            self.git_workspace.invalidate();
        }
    }

    pub(super) fn open_graph_merge(&mut self) {
        if let Some((root, target)) = self
            .project_root
            .clone()
            .zip(self.graph_merge_target().cloned())
        {
            let _ = self.git_index.discard_review();
            self.git_commit.open_merge(root, target);
            self.git_workspace.invalidate();
        }
    }

    fn graph_merge_target(&self) -> Option<&jc_app::git_commit::ReferenceTarget> {
        let row = self.git_workspace.graph.as_ref()?.reference_selected?;
        self.graph_branch_target()?;
        self.git_revision.graph()?.branch_merge_target(row)
    }

    pub(super) fn select_graph_reference(&mut self, index: usize) {
        if let Some(view) = &mut self.git_workspace.graph {
            view.reference_selected = Some(index);
            self.git_workspace.invalidate();
        }
    }

    pub(super) fn open_selected_graph_reference(&mut self) {
        if let Some(index) = self
            .git_workspace
            .graph
            .as_ref()
            .and_then(|view| view.reference_selected)
        {
            self.choose_graph_reference(index);
        }
    }

    pub(super) fn next_graph_page(&mut self) {
        let Some(root) = self.project_root.clone() else {
            return;
        };
        if self.git_revision.graph().is_some_and(|p| p.has_next()) {
            self.git_workspace.notice = self.git_revision.request_graph_next(root).err();
            if let Some(view) = &mut self.git_workspace.graph {
                view.selected = 0;
            }
            self.git_workspace.invalidate();
        }
    }

    pub(super) fn close_graph(&mut self) {
        self.git_workspace.graph = None;
        self.git_revision.cancel();
        self.git_workspace.invalidate();
    }

    pub(super) fn toggle_graph_references(&mut self) {
        if self.git_revision.graph().is_some() {
            if let Some(view) = &mut self.git_workspace.graph {
                view.reference_selected = view.reference_selected.is_none().then_some(0);
                self.git_workspace.invalidate();
            }
        }
    }

    pub(super) fn choose_graph_reference(&mut self, index: usize) {
        let Some(root) = self.project_root.clone() else {
            return;
        };
        self.git_workspace.notice = self.git_revision.request_graph_reference(root, index).err();
        if let Some(view) = &mut self.git_workspace.graph {
            view.reference_selected = None;
            view.selected = 0;
        }
        self.git_workspace.invalidate();
    }

    pub(in crate::windows) fn git_graph_key(&mut self, code: usize, mods: Modifiers) -> bool {
        let Some(view) = self.git_workspace.graph.as_ref() else {
            if code == b'G' as usize
                && mods == Modifiers::NONE
                && self.git_workspace.commit_search.is_none()
            {
                self.open_git_graph();
                return true;
            }
            return false;
        };
        if !view.showing {
            if code == 0x1b && mods == Modifiers::NONE && self.git_revision.return_to_graph() {
                self.git_workspace.history_commit = None;
                self.git_workspace.history_file = None;
                self.git_workspace.graph.as_mut().unwrap().showing = true;
                self.git_workspace.invalidate();
                return true;
            }
            return false;
        }
        if mods != Modifiers::NONE {
            return true;
        }
        match code {
            0x1b if view.reference_selected.is_some() => self.toggle_graph_references(),
            0x1b => self.close_graph(),
            0x74 => self.open_git_graph(),
            0x52 => self.toggle_graph_references(),
            0x42 if view.reference_selected.is_none() => {
                self.open_graph_reference_draft(jc_app::git_commit::RefKind::Branch)
            }
            0x54 if view.reference_selected.is_none() => {
                self.open_graph_reference_draft(jc_app::git_commit::RefKind::Tag)
            }
            0x71 if view.reference_selected.is_some() => self.open_graph_reference_change(true),
            0x2e if view.reference_selected.is_some() => self.open_graph_reference_change(false),
            0x43 if view.reference_selected.is_some() => self.open_graph_branch_switch(),
            0x4D if view.reference_selected.is_some() => self.open_graph_merge(),
            0x0d if view.reference_selected.is_some() => {
                self.choose_graph_reference(view.selected_row())
            }
            0x0d => self.open_graph_commit(view.selected),
            0x4e if view.reference_selected.is_none() => self.next_graph_page(),
            0x26 | 0x28 | 0x21 | 0x22 | 0x24 | 0x23 => {
                let delta = match code {
                    0x26 => -1,
                    0x28 => 1,
                    0x21 => -10,
                    0x22 => 10,
                    0x24 => isize::MIN,
                    _ => isize::MAX,
                };
                self.move_git_graph(delta);
            }
            _ => {}
        }
        true
    }

    pub(super) fn move_git_graph(&mut self, delta: isize) {
        if let Some(view) = &mut self.git_workspace.graph {
            let count = self.git_revision.graph().map_or(0, |graph| {
                if view.reference_selected.is_some() {
                    graph.reference_count()
                } else {
                    graph.entries.len()
                }
            });
            view.move_selection(count, delta);
            self.git_workspace.invalidate();
        }
    }

    pub(super) fn prepare_git_graph(&self, mut frame: Frame) -> Frame {
        let view = self.git_workspace.graph.as_ref().unwrap();
        let p = |n| self.dpi.pixels(n);
        frame.preview = rect(
            frame.rect.left,
            frame.toolbar.bottom,
            frame.rect.right,
            frame.rect.bottom - p(30),
        );
        frame.files = rect(0, 0, 0, 0);
        frame.history = None;
        frame.locations = None;
        frame.rows.clear();
        frame.commits.clear();
        frame.annotations.clear();
        frame.code.clear();
        frame.details.clear();
        frame.summary.clear();
        frame.rails.clear();
        frame.dividers.clear();
        frame.buttons.clear();
        frame.hits.actions.clear();
        frame.historical = false;
        let references = view.reference_selected.is_some();
        frame.subtitle = if references {
            "Ramas y etiquetas"
        } else {
            "Grafo de commits"
        }
        .into();
        frame.footer = if references && self.graph_merge_target().is_some() {
            "M fusionar · C cambiar · Intro abrir · F2 renombrar · Supr eliminar · Esc volver"
        } else if references && self.graph_branch_target().is_some() {
            "C cambiar rama · Intro abrir · Esc volver"
        } else if references && self.graph_write_target().is_some() {
            "Intro abrir · F2 renombrar · Supr eliminar · Esc volver"
        } else if references {
            "↑↓ seleccionar · Intro abrir referencia · Esc volver"
        } else {
            "Intro abrir · R referencias · B crear rama · T crear etiqueta · N más · Esc volver"
        }
        .into();
        let graph = self.git_revision.graph();
        if let Some(graph) = graph {
            frame.summary.push(graph.heading());
            if self.git_revision.state() != GitRevisionState::Ready {
                frame.summary.push(self.git_revision.message().into());
            }
            let row_height = p(23).max(1);
            let top = frame.preview.top + p(132);
            let count = ((frame.preview.bottom - top) / row_height).clamp(0, 24) as usize;
            let total = if references {
                graph.reference_count()
            } else {
                graph.entries.len()
            };
            let first = view.first(total, count);
            for index in first..total.min(first + count) {
                let y = top + (index - first) as i32 * row_height;
                let r = rect(
                    frame.preview.left + p(12),
                    y,
                    frame.preview.right - p(12),
                    y + row_height,
                );
                frame.code.push(CodeRow {
                    rect: r,
                    old: String::new(),
                    new: String::new(),
                    text: if references {
                        graph.reference_label(index).unwrap_or_default().to_owned()
                    } else {
                        let entry = &graph.entries[index];
                        format!(
                            "{}  {}  {}   ·   {}",
                            entry.rail,
                            &entry.commit.id[..8],
                            opaque(&entry.commit.summary, 160),
                            opaque(&entry.commit.author, 60)
                        )
                    },
                    kind: if index == view.selected_row() { 2 } else { 0 },
                    parallel: true,
                });
                frame.hits.actions.push((
                    r,
                    if references {
                        Intent::GraphReference(index)
                    } else {
                        Intent::GraphCommit(index)
                    },
                ));
            }
        } else {
            frame.summary.push(
                self.git_workspace
                    .notice
                    .clone()
                    .unwrap_or_else(|| self.git_revision.message().into()),
            );
        }
        let top = frame.preview.top + p(96);
        for (offset, label, intent) in [
            (
                12,
                "Volver",
                if references {
                    Intent::GraphReferences
                } else {
                    Intent::GraphClose
                },
            ),
            (
                119,
                if references { "Abrir" } else { "Referencias" },
                if references {
                    Intent::GraphOpenReference
                } else {
                    Intent::GraphReferences
                },
            ),
            (
                if references { 440 } else { 226 },
                if references {
                    "Renombrar"
                } else {
                    "Más antiguos"
                },
                if references {
                    Intent::GraphChangeReference(true)
                } else {
                    Intent::GraphNext
                },
            ),
            (
                if references { 547 } else { 333 },
                if references { "Eliminar" } else { "Crear rama" },
                if references {
                    Intent::GraphChangeReference(false)
                } else {
                    Intent::GraphCreateReference(jc_app::git_commit::RefKind::Branch)
                },
            ),
            (
                if references { 333 } else { 440 },
                if references {
                    "Cambiar rama"
                } else {
                    "Crear etiqueta"
                },
                if references {
                    Intent::GraphSwitchBranch
                } else {
                    Intent::GraphCreateReference(jc_app::git_commit::RefKind::Tag)
                },
            ),
            (226, "Fusionar rama", Intent::GraphMergeBranch),
        ] {
            if matches!(intent, Intent::GraphNext)
                && (references || !graph.is_some_and(|p| p.has_next()))
                || offset == 119 && graph.is_none()
                || matches!(intent, Intent::GraphChangeReference(_))
                    && self.graph_write_target().is_none()
                || matches!(intent, Intent::GraphSwitchBranch)
                    && self.graph_branch_target().is_none()
                || matches!(intent, Intent::GraphMergeBranch) && self.graph_merge_target().is_none()
                || matches!(intent, Intent::GraphCreateReference(_))
                    && (references || graph.is_none_or(|page| page.entries.is_empty()))
            {
                continue;
            }
            let left = frame.preview.left + p(offset);
            let r = rect(left, top, left + p(102), top + p(24));
            if r.right <= frame.preview.right && r.bottom <= frame.preview.bottom {
                frame.buttons.push((r, label.into(), false));
                frame.hits.actions.push((r, intent));
            }
        }
        frame
    }
}
