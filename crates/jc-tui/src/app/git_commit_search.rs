//! Bounded commit-query input and navigation; preparation never starts a query.
use super::{
    git_history::{label, opaque, short_oid},
    git_status::{GitStatusPresentation, GitStatusRow},
    TuiApp,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use jc_app::{
    edit_field,
    git_revision::{CommitSearchPage, SearchField},
    FieldEdit, PrimaryFocus,
};

pub(super) struct CommitSearchView {
    start: String,
    query: String,
    cursor: usize,
    field: SearchField,
    editing: bool,
    pub(super) browsing: bool,
    result_selected: usize,
    return_active: bool,
    return_selected: usize,
}

impl TuiApp {
    pub(super) fn commit_search_editing(&self) -> bool {
        self.focus.primary() == PrimaryFocus::Sidebar
            && self.panels.sidebar_visible()
            && self.panels.sidebar() == jc_app::SidebarView::GitStatus
            && self
                .git_history
                .search
                .as_ref()
                .is_some_and(|s| s.editing && !s.browsing)
    }
    pub(super) fn commit_search_paste_budget(&self) -> Option<usize> {
        self.commit_search_editing().then(|| {
            256usize
                .saturating_sub(self.git_history.search.as_ref().unwrap().query.len())
                .max(1)
        })
    }
    pub(super) fn paste_commit_search(&mut self, text: &str) -> bool {
        if !self.commit_search_editing() {
            return false;
        }
        if text.chars().any(char::is_control) {
            self.status = "La consulta de commits usa una sola línea sin controles".into();
            return true;
        }
        self.edit_commit_query(FieldEdit::Insert(text));
        true
    }
    fn edit_commit_query(&mut self, edit: FieldEdit<'_>) {
        let Some(s) = &mut self.git_history.search else {
            return;
        };
        match edit_field(&mut s.query, &mut s.cursor, edit, 256) {
            Ok(changed) => {
                if changed {
                    self.git_revision.cancel();
                    self.git_history.selected = 0;
                }
                self.status =
                    "Buscar commits · Intro busca · F2 ámbito · Tab resultados · Esc vuelve".into();
            }
            Err(e) => self.status = e.into(),
        }
    }
    fn open_commit_search(&mut self) {
        let Some(start) = self
            .git_status
            .history_info()
            .and_then(|i| i.commits.first())
            .map(|c| c.id.clone())
        else {
            self.status = "Actualiza el repositorio; la búsqueda necesita un commit inicial".into();
            return;
        };
        self.git_revision.cancel();
        self.git_diff.clear(&mut self.tasks);
        let view = CommitSearchView {
            start,
            query: String::new(),
            cursor: 0,
            field: SearchField::All,
            editing: true,
            browsing: false,
            result_selected: 0,
            return_active: self.git_history.active
                && !self.git_history.files
                && self.git_history.path.is_none()
                && self.git_history.blame.is_none(),
            return_selected: self.git_history.selected,
        };
        let encoding = self.git_history.encoding.or(self.default_encoding);
        self.git_history = Default::default();
        self.git_history.active = true;
        self.git_history.encoding = encoding;
        self.git_history.search = Some(view);
        self.panels.hide_bottom();
        let _ = self.focus.focus(PrimaryFocus::Sidebar, &self.panels);
        self.status = "Buscar commits · escribe y pulsa Intro · F2 ámbito · Esc vuelve".into();
    }
    fn run_commit_query(&mut self) {
        let Some(root) = self.project_root.clone() else {
            return;
        };
        let Some(s) = &mut self.git_history.search else {
            return;
        };
        if s.query.trim().is_empty() {
            self.status = "Escribe el texto que quieres buscar en los commits".into();
            return;
        }
        s.editing = false;
        s.browsing = false;
        s.result_selected = 0;
        self.git_history.files = false;
        self.git_history.commit = None;
        self.git_history.selected = 0;
        self.git_history.hunk = 0;
        self.panels.hide_bottom();
        let _ = self.focus.focus(PrimaryFocus::Sidebar, &self.panels);
        self.status = self
            .git_revision
            .request_commit_search(root, s.start.clone(), s.query.clone(), s.field)
            .err()
            .unwrap_or_else(|| "Buscando commits · solo lectura".into());
    }
    fn current_commit_search_page(&self) -> Option<&CommitSearchPage> {
        let s = self.git_history.search.as_ref()?;
        self.git_revision.commit_search().filter(|p| {
            self.project_root.as_ref() == Some(&p.repository.root)
                && p.start == s.start
                && p.query == s.query
                && p.field == s.field
        })
    }
    pub(super) fn prepare_commit_search(
        &mut self,
        width: u16,
        height: u16,
    ) -> GitStatusPresentation {
        let width = width.min(256);
        let count = usize::from(height).min(128);
        let Some(s) = self.git_history.search.as_ref() else {
            return GitStatusPresentation::default();
        };
        let query = s.query.clone();
        let cursor = s.cursor;
        let field = s.field;
        let editing = s.editing;
        let total = self
            .current_commit_search_page()
            .map_or(0, |p| p.entries.len());
        self.git_history.selected = self.git_history.selected.min(total.saturating_sub(1));
        let selected = self.git_history.selected;
        let scroll = selected
            .saturating_sub(count.saturating_sub(1))
            .min(total.saturating_sub(count));
        let page = self.current_commit_search_page();
        let rows = page
            .into_iter()
            .flat_map(|p| p.entries.iter())
            .enumerate()
            .skip(scroll)
            .take(count)
            .map(|(ordinal, c)| GitStatusRow {
                ordinal,
                flags: match c.matched {
                    SearchField::Message => "M",
                    SearchField::Author => "A",
                    SearchField::Commit => "#",
                    SearchField::All => "C",
                }
                .into(),
                path: label(
                    &format!(
                        "{} {} · {}",
                        short_oid(&c.commit),
                        opaque(&c.summary, width),
                        opaque(&c.author, 60)
                    ),
                    width.saturating_sub(3),
                ),
                selected: ordinal == selected,
            })
            .collect();
        let message = if editing {
            // Follow the caret through the bounded field instead of clipping
            // its right end when the query is wider than the Git sidebar.
            let field = jc_app::present_field(
                &query,
                cursor,
                usize::from(width.saturating_sub(16)).max(1),
                1,
            );
            let mut display = field.rows.into_iter().next().unwrap_or_default();
            display.insert(field.cursor_column.min(display.len()), '│');
            format!("{display} · Intro busca")
        } else if let Some(p) = page {
            format!(
                "{}{}{} · Tab editar · Intro archivos",
                if p.shallow {
                    "Historial superficial"
                } else if p.next.is_some() {
                    "N más resultados"
                } else {
                    "Fin del historial"
                },
                if p.entries.is_empty() {
                    " · sin coincidencias"
                } else {
                    ""
                },
                if p.metadata_limited {
                    " · metadatos recortados"
                } else {
                    ""
                }
            )
        } else {
            format!("{} · Tab editar", self.git_revision.message())
        };
        GitStatusPresentation {
            title: label(
                &format!("Buscar commits · {} · {query}", field.label()),
                width,
            ),
            message: label(&message, width),
            rows,
            total,
            selected,
            scroll,
        }
    }
    pub(super) fn handle_commit_search_key(&mut self, key: KeyEvent) -> bool {
        if self.git_history.search.is_none() {
            if key.modifiers.is_empty()
                && matches!(key.code, KeyCode::Char('f' | 'F'))
                && self.focus.primary() == PrimaryFocus::Sidebar
            {
                self.open_commit_search();
                return true;
            }
            return false;
        }
        let browsing = self.git_history.search.as_ref().unwrap().browsing;
        if matches!(key.code, KeyCode::Esc)
            || (!self.commit_search_editing() && matches!(key.code, KeyCode::Char('b' | 'B')))
        {
            if browsing && self.focus.primary() == PrimaryFocus::BottomPanel {
                self.panels.hide_bottom();
                let _ = self.focus.focus(PrimaryFocus::Sidebar, &self.panels);
                return true;
            }
            if browsing {
                let selected = self.git_history.search.as_ref().unwrap().result_selected;
                let s = self.git_history.search.as_mut().unwrap();
                s.browsing = false;
                s.editing = false;
                self.git_history.files = false;
                self.git_history.commit = None;
                self.git_history.selected = selected;
                self.panels.hide_bottom();
                let _ = self.focus.focus(PrimaryFocus::Sidebar, &self.panels);
                self.status = "Resultados de commits · Tab editar · Intro archivos · N más".into();
                return true;
            }
            let s = self.git_history.search.take().unwrap();
            self.git_revision.cancel();
            self.git_history.active = s.return_active;
            self.git_history.selected = s.return_selected;
            self.status = "Consulta de commits cerrada · fuente intacta".into();
            return true;
        }
        if key.modifiers.is_empty() && matches!(key.code, KeyCode::F(5)) {
            self.run_commit_query();
            return true;
        }
        if key.modifiers.is_empty() && key.code == KeyCode::F(2) {
            let s = self.git_history.search.as_mut().unwrap();
            s.field = match s.field {
                SearchField::All => SearchField::Message,
                SearchField::Message => SearchField::Author,
                SearchField::Author => SearchField::Commit,
                SearchField::Commit => SearchField::All,
            };
            s.editing = true;
            s.browsing = false;
            self.git_revision.cancel();
            self.git_history.files = false;
            self.git_history.selected = 0;
            self.panels.hide_bottom();
            let _ = self.focus.focus(PrimaryFocus::Sidebar, &self.panels);
            self.status = "Ámbito cambiado · Intro busca · Tab resultados".into();
            return true;
        }
        if self.commit_search_editing() {
            if key.modifiers == KeyModifiers::CONTROL {
                match key.code {
                    KeyCode::Char('u' | 'U') => self.edit_commit_query(FieldEdit::Clear),
                    KeyCode::Char('v' | 'V') => {
                        self.clipboard_command(jc_app::CommandId::EditPaste);
                    }
                    _ => {}
                }
                return true;
            }
            if key
                .modifiers
                .intersects(KeyModifiers::ALT | KeyModifiers::CONTROL)
            {
                return true;
            }
            match key.code {
                KeyCode::Enter => self.run_commit_query(),
                KeyCode::Tab | KeyCode::Down => {
                    self.git_history.search.as_mut().unwrap().editing = false;
                }
                KeyCode::Backspace => self.edit_commit_query(FieldEdit::Backspace),
                KeyCode::Delete => self.edit_commit_query(FieldEdit::Delete),
                KeyCode::Left => self.edit_commit_query(FieldEdit::Left),
                KeyCode::Right => self.edit_commit_query(FieldEdit::Right),
                KeyCode::Home => self.edit_commit_query(FieldEdit::Home),
                KeyCode::End => self.edit_commit_query(FieldEdit::End),
                KeyCode::Char(ch) if !ch.is_control() => {
                    let mut bytes = [0; 4];
                    self.edit_commit_query(FieldEdit::Insert(ch.encode_utf8(&mut bytes)));
                }
                _ => {}
            }
            return true;
        }
        if !key.modifiers.is_empty() {
            return true;
        }
        if matches!(key.code, KeyCode::Char('h' | 'H')) {
            self.git_revision.cancel();
            self.git_history = Default::default();
            self.git_history.active = true;
            self.panels.hide_bottom();
            let _ = self.focus.focus(PrimaryFocus::Sidebar, &self.panels);
            return true;
        }
        if browsing {
            return !matches!(
                key.code,
                KeyCode::Up
                    | KeyCode::Down
                    | KeyCode::Left
                    | KeyCode::Right
                    | KeyCode::PageUp
                    | KeyCode::PageDown
                    | KeyCode::Home
                    | KeyCode::End
                    | KeyCode::Enter
                    | KeyCode::Char('e' | 'E')
                    | KeyCode::Tab
            );
        }
        match key.code {
            KeyCode::Tab | KeyCode::Char('f' | 'F') => {
                self.git_history.search.as_mut().unwrap().editing = true;
            }
            KeyCode::Char('r' | 'R') => self.run_commit_query(),
            KeyCode::Char('n' | 'N') => {
                if self
                    .current_commit_search_page()
                    .is_some_and(|p| p.next.is_some())
                {
                    if let Some(root) = self.project_root.clone() {
                        self.status = self
                            .git_revision
                            .request_commit_search_next(root)
                            .err()
                            .unwrap_or_else(|| "Leyendo más resultados de commits".into());
                        self.git_history.selected = 0;
                    }
                } else {
                    self.status = "No hay más resultados disponibles".into();
                }
            }
            KeyCode::Enter => {
                if let (Some(root), Some(commit)) = (
                    self.project_root.clone(),
                    self.current_commit_search_page()
                        .and_then(|p| p.entries.get(self.git_history.selected))
                        .map(|e| e.commit.clone()),
                ) {
                    match self.git_revision.request_revision(root, commit.clone()) {
                        Ok(()) => {
                            let s = self.git_history.search.as_mut().unwrap();
                            s.browsing = true;
                            s.result_selected = self.git_history.selected;
                            self.git_history.files = true;
                            self.git_history.commit = Some(commit);
                            self.git_history.selected = 0;
                            self.git_history.hunk = 0;
                            self.git_diff_scroll = 0;
                            self.status =
                                "Archivos del resultado · Intro compara · Esc resultados".into();
                        }
                        Err(e) => self.status = e,
                    }
                }
            }
            KeyCode::Up
            | KeyCode::Down
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::PageUp
            | KeyCode::PageDown => {
                let total = self
                    .current_commit_search_page()
                    .map_or(0, |p| p.entries.len());
                let delta = match key.code {
                    KeyCode::Up => -1,
                    KeyCode::Down => 1,
                    KeyCode::Home => isize::MIN,
                    KeyCode::End => isize::MAX,
                    KeyCode::PageUp => -10,
                    _ => 10,
                };
                self.git_history.selected = self
                    .git_history
                    .selected
                    .saturating_add_signed(delta)
                    .min(total.saturating_sub(1));
            }
            _ => {}
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::LaunchOptions;
    use jc_app::{git_revision::GitRevisionState, SidebarView};
    #[test]
    fn commit_search_preparation_and_grapheme_input_do_not_admit_work_or_edit_source() {
        let mut app = TuiApp::new(LaunchOptions::default());
        app.workbench
            .new_file("muestra.txt", jc_text::TextEncoding::Utf8);
        app.workbench.insert_text("fuente viva\r\n").unwrap();
        let before = app.workbench.active_bytes().unwrap();
        app.panels.show_sidebar(SidebarView::GitStatus);
        let _ = app.focus.focus(PrimaryFocus::Sidebar, &app.panels);
        app.git_history.active = true;
        app.git_history.search = Some(CommitSearchView {
            start: "a".repeat(40),
            query: String::new(),
            cursor: 0,
            field: SearchField::All,
            editing: true,
            browsing: false,
            result_selected: 0,
            return_active: false,
            return_selected: 0,
        });
        for width in [0, 18, 100, u16::MAX] {
            assert!(app.prepare_git_history(width, u16::MAX).rows.len() <= 128);
        }
        assert_eq!(app.git_revision.state(), GitRevisionState::Idle);
        assert!(app.paste_commit_search("a\u{301}"));
        app.handle_git_history_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert!(app.git_history.search.as_ref().unwrap().query.is_empty());
        app.paste_commit_search(&"é".repeat(129));
        assert!(app.git_history.search.as_ref().unwrap().query.is_empty());
        app.paste_commit_search("mensaje");
        app.handle_git_history_key(KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE));
        assert_eq!(
            app.git_history.search.as_ref().unwrap().field,
            SearchField::Message
        );
        assert!(!app.git_revision.ready());
        assert!(!matches!(
            app.git_revision.state(),
            GitRevisionState::Queued | GitRevisionState::Running
        ));
        assert!(app
            .prepare_git_history(120, 10)
            .title
            .contains("Buscar commits"));
        app.paste_commit_search(&"z".repeat(100));
        assert!(app.prepare_git_history(40, 10).message.contains('│'));
        assert_eq!(app.workbench.active_bytes().unwrap(), before);
        app.handle_git_history_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.git_history.search.is_none());
    }
}
