//! Explicit bounded commit-query field; workers start on submission only.
use super::*;
use jc_app::{edit_field, git_revision::SearchField, present_field, FieldEdit, Modifiers};
use std::sync::Arc;

pub(in crate::windows) struct CommitSearchView {
    pub(in crate::windows) start: String,
    pub(in crate::windows) query: String,
    pub(in crate::windows) cursor: usize,
    pub(in crate::windows) field: SearchField,
    pub(in crate::windows) editing: bool,
    pub(in crate::windows) opened: bool,
    pub(in crate::windows) selected: usize,
    token: Arc<()>,
}
impl State {
    pub(super) fn add_git_commit_search_button(&self, f: &mut Frame) {
        if self.git_workspace.commit_search.is_some() {
            return;
        }
        f.footer = format!("Ctrl+F buscar commits · {}", f.footer);
        if f.toolbar.right < self.dpi.pixels(900) {
            return;
        }
        let r = rect(
            f.toolbar.right - self.dpi.pixels(400),
            f.toolbar.top + self.dpi.pixels(8),
            f.toolbar.right - self.dpi.pixels(270),
            f.toolbar.top + self.dpi.pixels(34),
        );
        f.buttons.push((r, "Buscar commits".into(), false));
        f.hits.actions.push((r, Intent::SearchOpen));
    }
    pub(super) fn open_git_commit_search(&mut self) {
        let start = self.git_workspace.history_commit.clone().or_else(|| {
            self.git_status
                .result()
                .filter(|_| self.git_status.state() == GitStatusTaskState::Ready)
                .and_then(|r| r.info())
                .and_then(|i| i.commits.first())
                .map(|c| c.id.clone())
        });
        let Some(start) = start else {
            self.git_workspace.notice =
                Some("Actualiza el repositorio: se necesita un commit inicial".into());
            self.git_workspace.invalidate();
            return;
        };
        self.git_revision.cancel();
        self.git_diff.clear(&mut self.jobs);
        self.git_workspace.blame_query = None;
        self.git_workspace.path_query = None;
        self.git_workspace.history_commit = None;
        self.git_workspace.history_file = None;
        self.git_workspace.local_kind = None;
        self.git_workspace.commit_search = Some(CommitSearchView {
            start,
            query: String::new(),
            cursor: 0,
            field: SearchField::All,
            editing: true,
            opened: false,
            selected: 0,
            token: Arc::new(()),
        });
        self.git_workspace.active = Column::Comparison;
        self.git_workspace.focus_diff = false;
        self.git_workspace.notice = None;
        self.git_workspace.invalidate();
    }
    pub(in crate::windows) fn git_search_input_token(&self) -> Option<Arc<()>> {
        self.git_panel?;
        self.git_workspace
            .commit_search
            .as_ref()
            .filter(|s| s.editing && !s.opened)
            .map(|s| s.token.clone())
    }
    pub(in crate::windows) fn edit_git_search_query(&mut self, edit: FieldEdit<'_>) {
        if let FieldEdit::Insert(text) = edit {
            if text.chars().any(char::is_control) {
                self.git_workspace.notice = Some("La consulta debe ocupar una sola línea".into());
                return;
            }
        }
        let Some(s) = self
            .git_workspace
            .commit_search
            .as_mut()
            .filter(|s| s.editing && !s.opened)
        else {
            return;
        };
        match edit_field(&mut s.query, &mut s.cursor, edit, 256) {
            Ok(changed) => {
                s.token = Arc::new(());
                if changed {
                    self.git_revision.cancel();
                    s.selected = 0;
                    self.git_workspace.notice = None;
                }
            }
            Err(e) => self.git_workspace.notice = Some(e.into()),
        }
        self.git_workspace.invalidate();
    }
    pub(in crate::windows) fn git_commit_search_character(&mut self, ch: char) {
        let mut b = [0; 4];
        self.edit_git_search_query(FieldEdit::Insert(ch.encode_utf8(&mut b)));
    }
    pub(super) fn focus_git_search_input(&mut self) {
        if let Some(s) = &mut self.git_workspace.commit_search {
            s.editing = true;
            s.opened = false;
            s.token = Arc::new(());
            self.git_workspace.history_commit = None;
            self.git_workspace.history_file = None;
            self.git_workspace.active = Column::Comparison;
            self.git_workspace.invalidate();
        }
    }
    pub(super) fn cycle_git_search_scope(&mut self) {
        if let Some(s) = &mut self.git_workspace.commit_search {
            s.field = match s.field {
                SearchField::All => SearchField::Message,
                SearchField::Message => SearchField::Author,
                SearchField::Author => SearchField::Commit,
                SearchField::Commit => SearchField::All,
            };
            s.token = Arc::new(());
            s.editing = true;
            s.opened = false;
            s.selected = 0;
            self.git_revision.cancel();
            self.git_workspace.history_commit = None;
            self.git_workspace.invalidate();
        }
    }
    pub(super) fn run_git_commit_search(&mut self) {
        let Some(root) = self.project_root.clone() else {
            return;
        };
        let Some(s) = &mut self.git_workspace.commit_search else {
            return;
        };
        if s.query.trim().is_empty() {
            self.git_workspace.notice = Some("Escribe un texto para buscar".into());
            self.git_workspace.invalidate();
            return;
        }
        let result = self.git_revision.request_commit_search(
            root,
            s.start.clone(),
            s.query.clone(),
            s.field,
        );
        s.opened = false;
        s.selected = 0;
        s.editing = false;
        s.token = Arc::new(());
        self.git_workspace.history_commit = None;
        self.git_workspace.history_file = None;
        self.git_workspace.history_first = 0;
        self.git_workspace.active = Column::History;
        self.git_workspace.focus_diff = false;
        self.git_workspace.notice = result.err();
        self.git_workspace.invalidate();
    }
    pub(super) fn next_git_commit_search(&mut self) {
        let Some(root) = self.project_root.clone() else {
            return;
        };
        if let Err(e) = self.git_revision.request_commit_search_next(root) {
            self.git_workspace.notice = Some(e);
            return;
        }
        if let Some(s) = &mut self.git_workspace.commit_search {
            s.selected = 0;
            s.opened = false;
            s.editing = false;
            s.token = Arc::new(());
        }
        self.git_workspace.history_commit = None;
        self.git_workspace.history_file = None;
        self.git_workspace.history_first = 0;
        self.git_workspace.active = Column::History;
        self.git_workspace.invalidate();
    }
    pub(super) fn open_git_search_result(&mut self, index: usize) {
        let Some(id) = self
            .git_revision
            .commit_search()
            .and_then(|p| p.entries.get(index))
            .map(|e| e.commit.clone())
        else {
            return;
        };
        if let Some(s) = &mut self.git_workspace.commit_search {
            s.selected = index;
            s.opened = true;
            s.editing = false;
            s.token = Arc::new(());
        }
        self.request_git_history(id);
    }
    pub(super) fn back_git_commit_search(&mut self) {
        if self.git_workspace.history_file.is_some() {
            self.git_workspace.history_file = None;
            self.git_workspace.active = Column::Files;
            self.git_workspace.focus_diff = false;
        } else {
            if let Some(s) = &mut self.git_workspace.commit_search {
                s.opened = false;
                s.editing = false;
                s.token = Arc::new(());
            }
            self.git_workspace.history_commit = None;
            self.git_workspace.active = Column::History;
            self.git_workspace.focus_diff = false;
        }
        self.git_workspace.invalidate();
    }
    pub(in crate::windows) fn git_commit_search_key(
        &mut self,
        code: usize,
        mods: Modifiers,
    ) -> Option<Action> {
        if self.git_workspace.menu.is_some() {
            return None;
        }
        if code == 0x46 && mods == Modifiers::CTRL {
            if self.git_workspace.commit_search.is_none() {
                self.open_git_commit_search();
            } else {
                self.focus_git_search_input();
            }
            return Some(Action::None);
        }
        let s = self.git_workspace.commit_search.as_ref()?;
        if s.editing && mods == Modifiers::CTRL && code == 0x56 {
            return Some(Action::PasteGitSearch(s.token.clone()));
        }
        if mods != Modifiers::NONE {
            return Some(Action::None);
        }
        let opened = s.opened;
        let editing = s.editing;
        match code {
            0x1b => {
                if opened {
                    self.back_git_commit_search();
                } else {
                    self.request_git(GitIntent::Status);
                }
            }
            0x42 if opened => self.back_git_commit_search(),
            0x71 => self.cycle_git_search_scope(),
            0x09 if !opened => {
                let s = self.git_workspace.commit_search.as_mut().unwrap();
                s.editing = !s.editing;
                s.token = Arc::new(());
                self.git_workspace.active = if s.editing {
                    Column::Comparison
                } else {
                    Column::History
                };
                self.git_workspace.invalidate();
            }
            0x0d if editing => self.run_git_commit_search(),
            0x0d if !opened => self.open_git_search_result(
                self.git_workspace.commit_search.as_ref().unwrap().selected,
            ),
            0x4e if !editing && !opened => self.next_git_commit_search(),
            0x52 | 0x74 if !editing && !opened => self.run_git_commit_search(),
            0x25 | 0x27 | 0x24 | 0x23 | 0x08 | 0x2e if editing => {
                self.edit_git_search_query(match code {
                    0x25 => FieldEdit::Left,
                    0x27 => FieldEdit::Right,
                    0x24 => FieldEdit::Home,
                    0x23 => FieldEdit::End,
                    0x08 => FieldEdit::Backspace,
                    _ => FieldEdit::Delete,
                });
            }
            0x26 | 0x28 | 0x21 | 0x22 | 0x24 | 0x23 if !opened && !editing => {
                let total = self
                    .git_revision
                    .commit_search()
                    .map_or(0, |p| p.entries.len());
                let s = self.git_workspace.commit_search.as_mut().unwrap();
                let delta = match code {
                    0x26 => -1,
                    0x28 => 1,
                    0x21 => -10,
                    0x22 => 10,
                    0x24 => isize::MIN,
                    _ => isize::MAX,
                };
                s.selected = s
                    .selected
                    .saturating_add_signed(delta)
                    .min(total.saturating_sub(1));
                self.git_workspace.invalidate();
            }
            _ if opened => return None,
            _ => {}
        }
        Some(Action::None)
    }
    pub(super) fn prepare_git_commit_search(&mut self, frame: Frame) -> Frame {
        let opened = self.git_workspace.commit_search.as_ref().unwrap().opened;
        let mut f = if opened {
            self.prepare_git_history(frame)
        } else {
            frame
        };
        let l = Layout::new(self);
        let p = |v| self.dpi.pixels(v);
        f.commits.clear();
        f.history_heading = "RESULTADOS";
        f.buttons.retain(|(r, _, _)| {
            r.top < l.toolbar.bottom || (opened && r.top >= l.preview.top + p(128))
        });
        f.hits.actions.retain(|(_, i)| {
            matches!(
                i,
                Intent::Refresh
                    | Intent::Close
                    | Intent::HistoryFile(_)
                    | Intent::Hunk(_)
                    | Intent::HistoryEncoding
            )
        });
        let s = self.git_workspace.commit_search.as_ref().unwrap();
        let mut button = |x: i32, y: i32, w: i32, name: String, intent| {
            let r = rect(
                l.preview.left + p(x),
                l.preview.top + p(y),
                l.preview.left + p(x + w),
                l.preview.top + p(y + 26),
            );
            if r.right <= l.preview.right {
                f.buttons.push((r, name, false));
                f.hits.actions.push((r, intent));
            }
        };
        if opened {
            button(8, 2, 158, "Volver a resultados".into(), Intent::SearchBack);
        } else {
            f.rows.clear();
            f.code.clear();
            f.details.clear();
            f.summary.clear();
            f.subtitle = "Buscar commits · consulta explícita".into();
            button(
                8,
                96,
                178,
                format!("{} · F2", s.field.label()),
                Intent::SearchScope,
            );
            button(190, 96, 92, "Buscar".into(), Intent::SearchRun);
            if self
                .git_revision
                .commit_search()
                .is_some_and(|p| p.next.is_some())
            {
                button(286, 96, 112, "Más antiguos".into(), Intent::SearchNext);
            }
            let r = rect(
                l.preview.left + p(12),
                l.preview.top + p(40),
                l.preview.right - p(12),
                l.preview.top + p(70),
            );
            let width = ((r.right - r.left - p(16)) / self.cell_width.max(1)).max(1) as usize;
            let presentation = present_field(&s.query, s.cursor, width, 1);
            f.query = Some(QueryRow {
                rect: r,
                text: presentation.rows.first().cloned().unwrap_or_default(),
                caret: s
                    .editing
                    .then_some(presentation.cursor_column as i32 * self.cell_width),
            });
            f.hits.actions.push((r, Intent::SearchFocus));
            f.details.push(if s.query.is_empty() {
                "Mensaje, autor/correo o prefijo del identificador".into()
            } else if s.editing {
                "Intro busca · Tab pasa a resultados · F2 cambia campo".into()
            } else {
                self.git_revision.message().into()
            });
        }
        if let Some(page) = self
            .git_revision
            .commit_search()
            .filter(|p| p.query == s.query && p.field == s.field && p.start == s.start)
        {
            let selected = s.selected.min(page.entries.len().saturating_sub(1));
            if let Some(area) = l.history {
                let capacity =
                    ((area.bottom - area.top - l.head - p(56)) / p(64)).clamp(0, 24) as usize;
                let first = selected
                    .saturating_sub(capacity.saturating_sub(1))
                    .min(page.entries.len().saturating_sub(capacity));
                for (index, e) in page.entries.iter().enumerate().skip(first).take(capacity) {
                    let top = area.top + l.head + p(56) + (index - first) as i32 * p(64);
                    let r = rect(area.left, top, area.right, (top + p(64)).min(area.bottom));
                    f.commits.push(CommitRow {
                        rect: r,
                        title: opaque(&e.summary, 160),
                        detail: format!(
                            "{} · {}",
                            &e.commit[..e.commit.len().min(8)],
                            opaque(&e.author, 80)
                        ),
                        selected: if opened {
                            self.git_workspace.history_commit.as_ref() == Some(&e.commit)
                        } else {
                            index == selected
                        },
                        merge: false,
                    });
                    f.hits.actions.push((r, Intent::SearchResult(index)));
                }
                f.hits.actions.push((
                    rect(
                        area.left,
                        area.top + l.head,
                        area.right,
                        (area.top + l.head + p(56)).min(area.bottom),
                    ),
                    Intent::SearchExit,
                ));
            }
            if opened {
                if let Some(e) = page
                    .entries
                    .iter()
                    .find(|e| self.git_workspace.history_commit.as_ref() == Some(&e.commit))
                {
                    f.summary = vec![
                        opaque(&e.summary, 160),
                        format!(
                            "{} · {}",
                            opaque(&e.author, 80),
                            &e.commit[..e.commit.len().min(8)]
                        ),
                    ];
                }
            } else {
                f.count = format!("{} resultados", page.entries.len());
                if page.entries.is_empty() {
                    f.details.push(if page.next.is_some() {
                        "Sin coincidencias en esta página; puedes continuar".into()
                    } else {
                        "No se encontraron coincidencias".into()
                    });
                }
            }
            f.footer = format!(
                "{} resultados · primer padre{}{}{} · N más · Ctrl+F consulta · Esc vuelve",
                page.entries.len(),
                if page.next.is_some() {
                    " · hay más"
                } else {
                    " · fin"
                },
                if page.shallow {
                    " · historial superficial"
                } else {
                    ""
                },
                if page.metadata_limited {
                    " · metadatos recortados"
                } else {
                    ""
                }
            );
        } else {
            f.footer = "Buscar commits · primer padre · Intro busca · F2 campo · Esc vuelve".into();
        }
        if let Some(e) = &self.git_workspace.notice {
            f.details.push(label(e, 256));
        }
        f.hits.stamp = self.git_workspace_stamp();
        f
    }
}

pub(in crate::windows) unsafe fn paste_query(hwnd: Hwnd, token: Arc<()>) {
    let valid = super::super::owner(hwnd).is_some_and(|m| {
        m.state
            .borrow()
            .git_search_input_token()
            .is_some_and(|current| Arc::ptr_eq(&current, &token))
    });
    if !valid {
        return;
    }
    let text = if super::super::owner(hwnd).unwrap().state.borrow().smoke {
        Ok(super::super::owner(hwnd)
            .unwrap()
            .state
            .borrow()
            .smoke_clipboard
            .clone())
    } else {
        super::super::clipboard::read_limited(hwnd, 256)
    };
    if let Some(m) = super::super::owner(hwnd) {
        let mut s = m.state.borrow_mut();
        if s.git_search_input_token()
            .is_some_and(|current| Arc::ptr_eq(&current, &token))
        {
            match text {
                Ok(Some(text)) => s.edit_git_search_query(FieldEdit::Insert(&text)),
                Ok(None) => {}
                Err(e) => s.error(e),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn commit_search_menu_paste_preserves_the_visible_field_and_dirty_editor() {
        let mut s = State::new(&crate::GuiOptions::default()).unwrap();
        s.width = 1280;
        s.height = 800;
        s.workbench.insert_text("sin guardar\n").unwrap();
        let before = s.workbench.active_bytes().unwrap();
        let panes = format!("{:?}", s.workbench.panes());
        s.git_panel = Some(GitIntent::Status);
        s.git_workspace.history_commit = Some("a".repeat(40));
        s.open_git_commit_search();
        s.edit_git_search_query(FieldEdit::Insert("consulta"));
        let token = s.git_search_input_token().unwrap();
        let action = s.execute_menu(jc_app::CommandId::EditPaste);
        let Action::PasteGitSearch(received) = action else {
            panic!("El menú Pegar debe conservar el campo Git como destino");
        };
        assert!(Arc::ptr_eq(&token, &received));
        assert!(s.git_panel.is_some());
        assert_eq!(
            s.git_workspace.commit_search.as_ref().unwrap().query,
            "consulta"
        );
        assert_eq!(s.workbench.active_bytes().unwrap(), before);
        assert_eq!(format!("{:?}", s.workbench.panes()), panes);
    }

    #[test]
    fn commit_search_field_graphemes_budget_stale_tokens_and_prepare_are_inert() {
        let mut s = State::new(&crate::GuiOptions::default()).unwrap();
        s.width = 1280;
        s.height = 800;
        s.git_panel = Some(GitIntent::Status);
        s.git_workspace.history_commit = Some("a".repeat(40));
        s.open_git_commit_search();
        let token = s.git_search_input_token().unwrap();
        let before = s.workbench.active_bytes().unwrap();
        s.edit_git_search_query(FieldEdit::Insert("e\u{301}🦀"));
        assert!(!Arc::ptr_eq(&token, &s.git_search_input_token().unwrap()));
        s.edit_git_search_query(FieldEdit::Backspace);
        assert_eq!(
            s.git_workspace.commit_search.as_ref().unwrap().query,
            "e\u{301}"
        );
        s.edit_git_search_query(FieldEdit::Backspace);
        assert!(s
            .git_workspace
            .commit_search
            .as_ref()
            .unwrap()
            .query
            .is_empty());
        s.edit_git_search_query(FieldEdit::Insert(&"x".repeat(257)));
        assert!(s
            .git_workspace
            .commit_search
            .as_ref()
            .unwrap()
            .query
            .is_empty());
        s.edit_git_search_query(FieldEdit::Insert("dos\nlineas"));
        assert!(s
            .git_workspace
            .commit_search
            .as_ref()
            .unwrap()
            .query
            .is_empty());
        for width in [700, 900, 1280] {
            s.width = width;
            let f = s.prepare_git_workspace().unwrap();
            assert!(f.query.is_some());
            let controls: Vec<_> = f
                .buttons
                .iter()
                .filter(|(r, _, _)| r.top >= f.preview.top)
                .collect();
            for (n, (a, _, _)) in controls.iter().enumerate() {
                for (b, _, _) in &controls[n + 1..] {
                    assert!(
                        a.right <= b.left
                            || b.right <= a.left
                            || a.bottom <= b.top
                            || b.bottom <= a.top
                    );
                }
            }
        }
        assert_eq!(
            s.git_revision.state(),
            jc_app::git_revision::GitRevisionState::Cancelled
        );
        assert_eq!(s.workbench.active_bytes().unwrap(), before);
        s.git_panel = None;
        assert!(s.git_search_input_token().is_none());
    }

    #[test]
    fn commit_search_gui_submit_open_back_edit_and_scope_preserve_dirty_source() {
        use std::{
            fs,
            process::Command,
            time::{Duration, Instant, SystemTime, UNIX_EPOCH},
        };
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../work/test-temp");
        fs::create_dir_all(&base).unwrap();
        let base = base.canonicalize().unwrap();
        let root = base.join(format!(
            "gui-commit-search-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let git = |args: &[&str]| {
            let out = Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "NUL")
                .env("GIT_AUTHOR_NAME", "CIDE fixture")
                .env("GIT_AUTHOR_EMAIL", "fixture@invalid")
                .env("GIT_COMMITTER_NAME", "CIDE fixture")
                .env("GIT_COMMITTER_EMAIL", "fixture@invalid")
                .output()
                .unwrap();
            assert!(out.status.success());
        };
        git(&["init", "--quiet", "--template=", "--initial-branch=main"]);
        let file = root.join("archivo.js");
        fs::write(&file, b"let valor = 1;\r\n").unwrap();
        git(&["add", "--", "archivo.js"]);
        git(&[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            "Inicial",
            "-m",
            "consulta encontrada en cuerpo",
        ]);
        let mut s = State::new(&crate::GuiOptions {
            project_root: Some(root.clone()),
            encoding: Some(TextEncoding::Utf8),
            ..Default::default()
        })
        .unwrap();
        s.width = 1280;
        s.height = 800;
        s.workbench
            .open_project_file(&root, file.clone(), Some(TextEncoding::Utf8), false, 4096)
            .unwrap();
        s.workbench.insert_text("sin guardar\n").unwrap();
        let before = s.workbench.active_bytes().unwrap();
        let panes = format!("{:?}", s.workbench.panes());
        s.request_git(GitIntent::Status);
        let end = Instant::now() + Duration::from_secs(8);
        while s.git_status.state() != GitStatusTaskState::Ready {
            s.poll_git();
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(1));
        }
        s.git_key(0x46, Modifiers::CTRL);
        s.edit_git_search_query(FieldEdit::Insert("consulta"));
        let wait = |s: &mut State| {
            let end = Instant::now() + Duration::from_secs(8);
            while matches!(
                s.git_revision.state(),
                jc_app::git_revision::GitRevisionState::Queued
                    | jc_app::git_revision::GitRevisionState::Running
            ) {
                s.poll_git();
                assert!(Instant::now() < end);
                std::thread::sleep(Duration::from_millis(1));
            }
            assert_eq!(
                s.git_revision.state(),
                jc_app::git_revision::GitRevisionState::Ready,
                "{}",
                s.git_revision.message()
            );
        };
        s.git_key(0x0d, Modifiers::NONE);
        wait(&mut s);
        assert_eq!(s.git_revision.commit_search().unwrap().entries.len(), 1);
        assert_eq!(s.prepare_git_workspace().unwrap().commits.len(), 1);
        s.git_key(0x0d, Modifiers::NONE);
        wait(&mut s);
        assert_eq!(s.git_revision.revision().unwrap().files.len(), 1);
        assert!(s.git_workspace.commit_search.as_ref().unwrap().opened);
        s.git_key(0x1b, Modifiers::NONE);
        assert!(!s.git_workspace.commit_search.as_ref().unwrap().opened);
        assert!(s.git_revision.commit_search().is_some());
        s.git_key(0x46, Modifiers::CTRL);
        s.edit_git_search_query(FieldEdit::Insert(" nueva"));
        assert!(s.git_revision.commit_search().is_none());
        s.git_key(0x71, Modifiers::NONE);
        assert_eq!(
            s.git_workspace.commit_search.as_ref().unwrap().field,
            SearchField::Message
        );
        assert_eq!(s.workbench.active_bytes().unwrap(), before);
        assert_eq!(format!("{:?}", s.workbench.panes()), panes);
        assert_eq!(fs::read(file).unwrap(), b"let valor = 1;\r\n");
        drop(s);
        fn writable(path: &std::path::Path) {
            for e in fs::read_dir(path).unwrap() {
                let p = e.unwrap().path();
                let m = fs::symlink_metadata(&p).unwrap();
                assert!(!m.file_type().is_symlink());
                if m.is_dir() {
                    writable(&p)
                } else if m.permissions().readonly() {
                    let mut perms = m.permissions();
                    #[allow(clippy::permissions_set_readonly_false)]
                    perms.set_readonly(false);
                    fs::set_permissions(p, perms).unwrap();
                }
            }
        }
        assert_eq!(root.parent(), Some(base.as_path()));
        writable(&root);
        fs::remove_dir_all(root).unwrap();
    }
}
