use super::{
    git_history::{label, opaque, short_oid},
    git_status::{GitStatusPresentation, GitStatusRow},
    TuiApp,
};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use jc_app::{git_graph::GraphView, BottomPanel, PrimaryFocus, SidebarView};

impl TuiApp {
    pub(super) fn graph_showing(&self) -> bool {
        self.git_history
            .graph
            .as_ref()
            .is_some_and(|graph| graph.showing)
    }

    pub(super) fn open_git_graph(&mut self) {
        let Some(root) = self.project_root.clone() else {
            return;
        };
        let _ = self.git_index.discard_review();
        let encoding = self.git_history.encoding.or(self.default_encoding);
        self.git_history = Default::default();
        self.git_history.active = true;
        self.git_history.encoding = encoding;
        self.git_history.graph = Some(GraphView::default());
        self.panels.show_sidebar(SidebarView::GitStatus);
        self.panels.hide_bottom();
        let _ = self.focus.focus(PrimaryFocus::Sidebar, &self.panels);
        self.status = self
            .git_revision
            .request_graph(root)
            .err()
            .unwrap_or_else(|| "Leyendo grafo de commits…".into());
    }

    pub(super) fn move_git_graph(&mut self, delta: isize) {
        if let Some(view) = &mut self.git_history.graph {
            let count = self.git_revision.graph().map_or(0, |page| {
                if view.reference_selected.is_some() {
                    page.reference_count()
                } else {
                    page.entries.len()
                }
            });
            view.move_selection(count, delta);
        }
    }

    fn graph_write_target(&self, row: usize) -> Option<&jc_app::git_commit::ReferenceTarget> {
        if self.git_revision.state() != jc_app::git_revision::GitRevisionState::Ready
            || self.git_index.state() == jc_app::git_index::IndexState::Applying
        {
            return None;
        }
        self.git_revision.graph()?.reference_write_target(row)
    }

    pub(super) fn handle_git_graph_key(&mut self, key: KeyEvent) -> bool {
        let Some(view) = self.git_history.graph.as_ref() else {
            return false;
        };
        if !view.showing {
            if self.focus.primary() == PrimaryFocus::Sidebar
                && key.modifiers.is_empty()
                && key.code == KeyCode::Esc
                && self.git_revision.return_to_graph()
            {
                self.git_history.graph.as_mut().unwrap().showing = true;
                self.git_history.files = false;
                self.git_history.commit = None;
                self.panels.hide_bottom();
                return true;
            }
            return false;
        }
        if !key.modifiers.is_empty() {
            return true;
        }
        match key.code {
            KeyCode::Esc if view.reference_selected.is_some() => {
                self.git_history.graph.as_mut().unwrap().reference_selected = None;
            }
            KeyCode::Esc => {
                self.git_history = Default::default();
                self.git_revision.cancel();
            }
            KeyCode::F(5) => self.open_git_graph(),
            KeyCode::Char('b' | 'B' | 't' | 'T') if view.reference_selected.is_none() => {
                if self.git_index.state() == jc_app::git_index::IndexState::Applying {
                    return true;
                }
                let target = self
                    .git_revision
                    .graph()
                    .and_then(|graph| graph.entries.get(view.selected))
                    .map(|entry| entry.commit.id.clone());
                if let (Some(root), Some(target)) = (self.project_root.clone(), target) {
                    let kind = if matches!(key.code, KeyCode::Char('b' | 'B')) {
                        jc_app::git_commit::RefKind::Branch
                    } else {
                        jc_app::git_commit::RefKind::Tag
                    };
                    self.git_commit.open_reference(root, kind, target);
                }
            }
            KeyCode::F(2) | KeyCode::Delete
                if view.reference_selected.is_some()
                    && key.kind == KeyEventKind::Press
                    && (cfg!(windows) || key.code == KeyCode::Delete) =>
            {
                let target = view
                    .reference_selected
                    .and_then(|row| self.graph_write_target(row))
                    .cloned();
                if let Some((root, target)) = self.project_root.clone().zip(target) {
                    let _ = self.git_index.discard_review();
                    self.git_commit
                        .open_reference_change(root, target, key.code == KeyCode::F(2));
                }
            }
            KeyCode::Char('r' | 'R') if self.git_revision.graph().is_some() => {
                let open = view.reference_selected.is_some();
                self.git_history.graph.as_mut().unwrap().reference_selected = (!open).then_some(0);
            }
            KeyCode::Up => self.move_git_graph(-1),
            KeyCode::Down => self.move_git_graph(1),
            KeyCode::PageUp => self.move_git_graph(-10),
            KeyCode::PageDown => self.move_git_graph(10),
            KeyCode::Home => self.move_git_graph(isize::MIN),
            KeyCode::End => self.move_git_graph(isize::MAX),
            KeyCode::Char('n' | 'N') if view.reference_selected.is_none() => {
                if let Some(root) = self
                    .project_root
                    .clone()
                    .filter(|_| self.git_revision.graph().is_some_and(|g| g.has_next()))
                {
                    if let Err(error) = self.git_revision.request_graph_next(root) {
                        self.status = error;
                    }
                    self.git_history.graph.as_mut().unwrap().selected = 0;
                }
            }
            KeyCode::Enter if view.reference_selected.is_some() => {
                if let Some(root) = self.project_root.clone() {
                    let row = view.selected_row();
                    self.status = self
                        .git_revision
                        .request_graph_reference(root, row)
                        .err()
                        .unwrap_or_else(|| "Leyendo referencia…".into());
                    let view = self.git_history.graph.as_mut().unwrap();
                    view.reference_selected = None;
                    view.selected = 0;
                }
            }
            KeyCode::Enter => {
                let commit = self
                    .git_revision
                    .graph()
                    .and_then(|page| page.entries.get(view.selected))
                    .map(|e| e.commit.id.clone());
                if let (Some(root), Some(commit)) = (self.project_root.clone(), commit) {
                    match self.git_revision.request_revision(root, commit.clone()) {
                        Ok(()) => {
                            self.git_history.graph.as_mut().unwrap().showing = false;
                            self.git_history.commit = Some(commit);
                            self.git_history.files = true;
                            self.git_history.selected = 0;
                            self.git_history.hunk = 0;
                            self.git_diff_scroll = 0;
                            self.panels.show_bottom(BottomPanel::Diff);
                            self.status =
                                "Leyendo archivos del commit · Esc vuelve al grafo".into();
                        }
                        Err(error) => self.status = error,
                    }
                }
            }
            _ => {}
        }
        true
    }

    pub(super) fn prepare_git_graph(
        &self,
        width: u16,
        height: u16,
    ) -> Option<GitStatusPresentation> {
        let view = self
            .git_history
            .graph
            .as_ref()
            .filter(|view| view.showing)?;
        let Some(page) = self.git_revision.graph() else {
            return Some(GitStatusPresentation {
                title: "Grafo de commits".into(),
                message: self.git_revision.message().into(),
                ..Default::default()
            });
        };
        let references = view.reference_selected.is_some();
        let total = if references {
            page.reference_count()
        } else {
            page.entries.len()
        };
        let count = usize::from(height).min(24);
        let scroll = view.first(total, count);
        let rows = (scroll..total.min(scroll + count))
            .map(|ordinal| GitStatusRow {
                ordinal,
                flags: String::new(),
                path: if references {
                    label(
                        page.reference_label(ordinal).unwrap_or_default(),
                        width.saturating_sub(4),
                    )
                } else {
                    let entry = &page.entries[ordinal];
                    label(
                        &format!(
                            "{} {} {} · {}",
                            entry.rail,
                            short_oid(&entry.commit.id),
                            opaque(&entry.commit.summary, 160),
                            opaque(&entry.commit.author, 80)
                        ),
                        width.saturating_sub(4),
                    )
                },
                selected: ordinal == view.selected_row(),
            })
            .collect();
        Some(GitStatusPresentation {
            title: if references {
                "Ramas y etiquetas"
            } else {
                "Grafo de commits"
            }
            .into(),
            message: if references && self.graph_write_target(view.selected_row()).is_some() {
                if cfg!(windows) {
                    "Intro abrir · F2 renombrar · Supr eliminar · Esc volver"
                } else {
                    "Intro abrir · Supr eliminar · Esc volver"
                }
                .into()
            } else if self.git_revision.state() == jc_app::git_revision::GitRevisionState::Ready {
                page.heading()
            } else {
                format!("{} · {}", self.git_revision.message(), page.heading())
            },
            rows,
            total,
            selected: view.selected_row(),
            scroll,
        })
    }
}
