//! Read-only line annotations over the shared revision controller.
use super::{
    git_history::{label, opaque, short_oid},
    git_status::{GitStatusPresentation, GitStatusRow},
    GitDiffRow, GitPresentation, GitRowKind, TuiApp,
};
use crossterm::event::{KeyCode, KeyEvent};
use jc_app::{BottomPanel, PrimaryFocus};

const PAGE: usize = 128;
pub(super) struct BlameView {
    start: String,
    path: Vec<u8>,
    first: usize,
    pub(super) comparing: bool,
    return_active: bool,
    return_selected: usize,
}

impl TuiApp {
    fn blame_active_path(&self) -> Option<Vec<u8>> {
        let relative = self
            .workbench
            .active_document()?
            .session
            .path()
            .strip_prefix(self.project_root.as_ref()?)
            .ok()?;
        let parts = relative
            .components()
            .map(|c| match c {
                std::path::Component::Normal(p) => p.to_str(),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?;
        let bytes = parts.join("/").into_bytes();
        (!bytes.is_empty() && bytes.len() <= 4096).then_some(bytes)
    }
    fn open_git_blame(&mut self) {
        let comparison = if self.focus.primary() == PrimaryFocus::BottomPanel {
            self.git_revision
                .comparison()
                .and_then(|c| match &c.origin {
                    jc_app::git_revision::ComparisonOrigin::Historical { commit, .. } => {
                        Some((commit.clone(), c.path.clone()))
                    }
                    _ => None,
                })
        } else {
            None
        };
        let path_entry = self
            .git_history
            .path
            .as_ref()
            .and_then(|_| self.git_revision.path_history())
            .and_then(|p| {
                p.entries
                    .get(self.git_history.selected)
                    .map(|e| (e.commit.clone(), p.path.clone()))
            });
        let revision_entry = if self.git_history.files {
            self.git_revision.revision().and_then(|r| {
                r.files
                    .get(self.git_history.selected)
                    .map(|f| (r.commit.clone(), f.path.clone()))
            })
        } else {
            None
        };
        let selected_commit = if self.git_history.active && !self.git_history.files {
            self.git_status
                .history_info()
                .and_then(|i| i.commits.get(self.git_history.selected))
                .map(|c| c.id.clone())
        } else {
            None
        };
        let head = self
            .git_status
            .history_info()
            .and_then(|i| i.commits.first())
            .map(|c| c.id.clone());
        let selected_path = self
            .git_history
            .local
            .as_ref()
            .map(|(_, p)| p.clone())
            .or_else(|| {
                if self.git_history.active {
                    self.blame_active_path()
                } else {
                    self.git_status
                        .selected_path()
                        .or_else(|| self.blame_active_path())
                }
            });
        let pair = comparison
            .or(path_entry)
            .or(revision_entry)
            .or_else(|| selected_commit.or(head).zip(selected_path));
        if let (Some(root), Some((start, path))) = (self.project_root.clone(), pair) {
            match self
                .git_revision
                .request_blame(root, start.clone(), path.clone(), 0)
            {
                Ok(()) => {
                    self.git_diff.clear(&mut self.tasks);
                    self.git_history.blame = Some(BlameView {
                        start,
                        path,
                        first: 0,
                        comparing: false,
                        return_active: self.git_history.active,
                        return_selected: self.git_history.selected,
                    });
                    self.git_history.active = true;
                    self.git_history.selected = 0;
                    self.git_history.local = None;
                    self.git_history.encoding = self.git_history.encoding.or(self.default_encoding);
                    self.git_diff_scroll = 0;
                    self.panels.show_bottom(BottomPanel::Diff);
                    let _ = self.focus.focus(PrimaryFocus::Sidebar, &self.panels);
                    self.status="Anotaciones por línea · primer padre · Intro revisión · N/P páginas · E codificación".into();
                }
                Err(e) => self.status = e,
            }
        } else {
            self.status =
                "Selecciona un archivo y actualiza el repositorio; se necesita un commit".into();
        }
    }
    fn request_git_blame_page(&mut self, first: usize) {
        let Some(view) = &self.git_history.blame else {
            return;
        };
        let (start, path) = (view.start.clone(), view.path.clone());
        let Some(root) = self.project_root.clone() else {
            return;
        };
        match self.git_revision.request_blame(root, start, path, first) {
            Ok(()) => {
                let view = self.git_history.blame.as_mut().unwrap();
                view.first = first;
                view.comparing = false;
                self.git_history.selected = 0;
                self.git_diff_scroll = 0;
                self.panels.show_bottom(BottomPanel::Diff);
                let _ = self.focus.focus(PrimaryFocus::Sidebar, &self.panels);
                self.status = "Leyendo anotaciones de la revisión fijada".into();
            }
            Err(e) => self.status = e,
        }
    }
    pub(super) fn handle_git_blame_key(&mut self, key: KeyEvent) -> bool {
        if self.git_history.blame.is_none() {
            if matches!(key.code, KeyCode::Char('l' | 'L')) {
                self.open_git_blame();
                return true;
            }
            return false;
        }
        if matches!(key.code, KeyCode::Char('h' | 'H')) {
            let encoding = self.git_history.encoding;
            self.git_revision.cancel();
            let mut history = super::git_history::HistoryView::default();
            history.active = true;
            history.encoding = encoding;
            self.git_history = history;
            self.panels.hide_bottom();
            let _ = self.focus.focus(PrimaryFocus::Sidebar, &self.panels);
            self.status = "Historial Git general · Intro archivos · L anotaciones".into();
            return true;
        }
        if matches!(key.code, KeyCode::Esc | KeyCode::Char('b' | 'B')) {
            if self.git_history.blame.as_ref().unwrap().comparing {
                self.git_history.blame.as_mut().unwrap().comparing = false;
                self.git_diff_scroll = 0;
                let _ = self.focus.focus(PrimaryFocus::Sidebar, &self.panels);
                self.status =
                    "Anotaciones · Intro abre la revisión responsable · N/P páginas".into();
            } else {
                let view = self.git_history.blame.take().unwrap();
                self.git_history.active = view.return_active;
                self.git_history.selected = view.return_selected;
                self.git_history.files = false;
                let reload = view.return_active
                    && self.git_history.path.is_some()
                    && self.git_revision.path_history().is_none();
                if reload {
                    if let (Some(root), Some((start, path))) =
                        (self.project_root.clone(), self.git_history.path.clone())
                    {
                        self.git_history.selected = 0;
                        self.status = self
                            .git_revision
                            .request_path_history(root, start, path)
                            .err()
                            .unwrap_or_else(|| "Releyendo el historial fijado del archivo".into());
                    }
                } else if !view.return_active || self.git_history.path.is_none() {
                    self.git_revision.cancel();
                }
                self.panels.hide_bottom();
                let _ = self.focus.focus(PrimaryFocus::Sidebar, &self.panels);
                if !reload {
                    self.status = "Vista Git · L anotaciones por línea".into();
                }
            }
            return true;
        }
        if matches!(key.code, KeyCode::Char('e' | 'E')) {
            self.git_history.cycle_encoding();
            return true;
        }
        if matches!(key.code, KeyCode::Char('r' | 'R') | KeyCode::F(5)) {
            let first = self.git_history.blame.as_ref().unwrap().first;
            self.request_git_blame_page(first);
            return true;
        }
        if self.git_history.blame.as_ref().unwrap().comparing {
            return false;
        }
        match key.code {
            KeyCode::Char('n' | 'N') | KeyCode::PageDown => {
                if let Some(page) = self.git_revision.blame() {
                    let next = page.first.saturating_add(page.rows.len());
                    if next < page.total_lines {
                        self.request_git_blame_page(next);
                    } else {
                        self.status = "Última página de anotaciones".into();
                    }
                }
            }
            KeyCode::Char('p' | 'P') | KeyCode::PageUp => {
                let first = self
                    .git_history
                    .blame
                    .as_ref()
                    .unwrap()
                    .first
                    .saturating_sub(PAGE);
                self.request_git_blame_page(first);
            }
            KeyCode::Up | KeyCode::Down | KeyCode::Home | KeyCode::End => {
                let total = self.git_revision.blame().map_or(0, |p| p.rows.len());
                let delta = match key.code {
                    KeyCode::Up => -1,
                    KeyCode::Down => 1,
                    KeyCode::Home => isize::MIN,
                    _ => isize::MAX,
                };
                self.git_history.selected = self
                    .git_history
                    .selected
                    .saturating_add_signed(delta)
                    .min(total.saturating_sub(1));
                self.git_diff_scroll = self.git_history.selected;
            }
            KeyCode::Enter => {
                let target = self.git_revision.blame().and_then(|p| {
                    p.rows
                        .get(self.git_history.selected)
                        .and_then(|r| r.commit)
                        .and_then(|i| p.commits.get(i))
                        .map(|c| (c.commit.clone(), p.path.clone()))
                });
                if let (Some(root), Some((commit, path))) = (self.project_root.clone(), target) {
                    match self.git_revision.request_file(root, commit, path) {
                        Ok(()) => {
                            self.git_history.blame.as_mut().unwrap().comparing = true;
                            self.git_history.hunk = 0;
                            self.git_diff_scroll = 0;
                            self.panels.show_bottom(BottomPanel::Diff);
                            let _ = self.focus.focus(PrimaryFocus::BottomPanel, &self.panels);
                            self.status =
                                "Revisión responsable · Esc vuelve a las anotaciones".into();
                        }
                        Err(e) => self.status = e,
                    }
                } else {
                    self.status =
                        "Línea sin atribuir · Selecciona otra línea o actualiza la consulta".into();
                }
            }
            _ => return false,
        }
        true
    }
    pub(super) fn prepare_git_blame_list(
        &mut self,
        width: u16,
        height: u16,
    ) -> GitStatusPresentation {
        let page = self.git_revision.blame();
        let total = page.map_or(0, |p| p.rows.len());
        let count = usize::from(height).min(PAGE);
        self.git_history.selected = self.git_history.selected.min(total.saturating_sub(1));
        let scroll = self
            .git_history
            .selected
            .saturating_sub(count.saturating_sub(1))
            .min(total.saturating_sub(count));
        let rows = page
            .into_iter()
            .flat_map(|p| p.rows.iter().enumerate().map(move |(i, r)| (p, i, r)))
            .skip(scroll)
            .take(count)
            .map(|(p, ordinal, row)| {
                let attribution = row.commit.and_then(|i| p.commits.get(i));
                GitStatusRow {
                    ordinal,
                    flags: if attribution.is_some() { "A" } else { "?" }.into(),
                    path: label(
                        &format!(
                            "{} {}",
                            row.line + 1,
                            attribution.map_or("Sin atribuir".into(), |c| format!(
                                "{} {}",
                                short_oid(&c.commit),
                                opaque(&c.author, width)
                            ))
                        ),
                        width.saturating_sub(4),
                    ),
                    selected: ordinal == self.git_history.selected,
                }
            })
            .collect();
        let status = page.map_or(self.git_revision.message(), |p| p.boundary.label());
        GitStatusPresentation {
            title: label("Anotaciones por línea", width),
            message: label(&format!("{status} · Intro revisión · N/P páginas"), width),
            rows,
            total,
            selected: self.git_history.selected,
            scroll,
        }
    }
    pub(super) fn prepare_git_blame(&mut self, width: u16, rows: usize) -> GitPresentation {
        let mut out = GitPresentation {
            status: label(self.git_revision.message(), width),
            ..Default::default()
        };
        out.details.push(label(
            "Anotaciones · primer padre · ruta exacta · solo lectura",
            width,
        ));
        out.details.push(label(
            "N/P página · Intro revisión · E codificación · Esc volver",
            width,
        ));
        let Some(page) = self.git_revision.blame() else {
            return out;
        };
        out.details.push(label(
            &format!(
                "{} · {} · {}",
                opaque(&page.path, width),
                short_oid(&page.start),
                page.boundary.label()
            ),
            width,
        ));
        if page.metadata_limited {
            out.details
                .push(label("Metadatos de autor/resumen recortados", width));
        }
        let Some(codec) = self.git_history.encoding else {
            out.details.push(label(
                "Pulsa E para elegir la codificación histórica",
                width,
            ));
            return out;
        };
        let count = rows.min(PAGE);
        let first = self
            .git_diff_scroll
            .min(page.rows.len().saturating_sub(count));
        self.git_diff_scroll = first;
        for row in page.rows.iter().skip(first).take(count) {
            let Some(source) = page.source.get(row.range.clone()) else {
                continue;
            };
            let (text, eol) = super::git_diff::source_prefix(
                source,
                row.range.start,
                codec,
                width.saturating_sub(25),
            );
            let owner = row
                .commit
                .and_then(|i| page.commits.get(i))
                .map_or("Sin atribuir".into(), |c| {
                    format!("{} {}", short_oid(&c.commit), opaque(&c.author, 16))
                });
            out.rows.push(GitDiffRow {
                kind: GitRowKind::Context,
                left_line: Some(row.original_line + 1),
                right_line: Some(row.line + 1),
                text: label(&format!("{owner} │ {text}"), width),
                eol,
            });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::LaunchOptions;
    use jc_app::git_revision::GitRevisionState;
    #[test]
    fn blame_preparation_and_empty_codec_navigation_do_not_admit_work_or_modify_editor() {
        let mut app = TuiApp::new(LaunchOptions::default());
        app.workbench
            .new_file("muestra.txt", jc_text::TextEncoding::Utf8);
        app.workbench.insert_text("fuente viva\r\n").unwrap();
        let before = app.workbench.active_bytes().unwrap();
        app.git_history.active = true;
        app.git_history.blame = Some(BlameView {
            start: "a".repeat(40),
            path: b"muestra.txt".to_vec(),
            first: 0,
            comparing: false,
            return_active: false,
            return_selected: 0,
        });
        for width in [0, 18, 100, u16::MAX] {
            let a = app.prepare_git_history(width, u16::MAX);
            assert!(a.rows.len() <= PAGE);
            let b = app.prepare_historical_diff(width, usize::MAX, usize::MAX);
            assert!(b.rows.len() <= PAGE);
        }
        assert_eq!(app.git_revision.state(), GitRevisionState::Idle);
        assert!(!app.git_revision.ready());
        app.handle_git_history_key(KeyEvent::new(
            KeyCode::Char('e'),
            crossterm::event::KeyModifiers::NONE,
        ));
        assert_eq!(app.git_history.encoding, Some(jc_text::TextEncoding::Utf8));
        app.handle_git_history_key(KeyEvent::new(
            KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ));
        assert!(app.status.contains("sin atribuir"));
        assert_eq!(app.workbench.active_bytes().unwrap(), before);
        assert_eq!(app.git_revision.state(), GitRevisionState::Idle);
    }
}
