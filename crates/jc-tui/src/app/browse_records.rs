use super::TuiApp;
use crate::{input::is_plain_text, state::UiOverlay};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use jc_app::{edit_field, FieldEdit, Overlay};
use jc_progress::editor::{
    BrowseFilter, BrowsePage, BrowseRequest, BrowseTable, RecordEvent, RecordPreview,
    BROWSE_MAX_FILTER_BYTES, BROWSE_PAGE_SIZE, RECORD_EDIT_MAX_VALUE_BYTES,
};

pub(crate) struct RecordBrowser {
    pub(crate) tables: Vec<BrowseTable>,
    pub(crate) table: usize,
    pub(crate) accepted: Option<AcceptedPage>,
    pending: Option<Query>,
    pub(crate) filter: Option<ValueField>,
    pub(crate) edit: Option<ValueField>,
    pub(crate) review: Option<RecordReview>,
    stale: bool,
    pub(crate) row: usize,
    pub(crate) column: usize,
    pub(crate) detail: Option<usize>,
    pub(crate) message: Option<String>,
}

#[derive(Clone, Default)]
struct Query {
    filter: Option<BrowseFilter>,
    descending: bool,
}

pub(crate) struct AcceptedPage {
    pub(crate) page: BrowsePage,
    query: Query,
}

impl AcceptedPage {
    pub(crate) fn filter(&self) -> Option<&BrowseFilter> {
        self.query.filter.as_ref()
    }

    pub(crate) fn descending(&self) -> bool {
        self.query.descending
    }
}

pub(crate) struct ValueField {
    pub(crate) value: String,
    pub(crate) cursor: usize,
}

pub(crate) struct RecordReview {
    pub(crate) preview: RecordPreview,
    pub(crate) after_selected: bool,
    pub(crate) offset: usize,
    pub(crate) presented: bool,
}

pub(crate) const REVIEW_MIN_WIDTH: u16 = 32;
pub(crate) const REVIEW_MIN_HEIGHT: u16 = 11;

impl RecordBrowser {
    fn new(tables: Vec<BrowseTable>) -> Self {
        Self {
            tables,
            table: 0,
            accepted: None,
            pending: None,
            filter: None,
            edit: None,
            review: None,
            stale: false,
            row: 0,
            column: 0,
            detail: None,
            message: None,
        }
    }

    pub(crate) fn loading(&self) -> bool {
        self.pending.is_some()
    }

    pub(crate) fn stale(&self) -> bool {
        self.stale
    }

    pub(crate) fn selected_value(&self) -> Option<&str> {
        self.selected_row()?.get(self.column)?.as_deref()
    }

    fn selected_row(&self) -> Option<&[Option<String>]> {
        self.accepted
            .as_ref()?
            .page
            .rows
            .get(self.row)
            .map(Vec::as_slice)
    }

    pub(crate) fn can_edit(&self) -> bool {
        !self.stale
            && self.selected_row().is_some()
            && self
                .tables
                .get(self.table)
                .is_some_and(|table| table.can_edit_column(self.column))
    }

    pub(crate) fn can_restore(&self) -> bool {
        !self.stale
            && self.selected_row().is_some()
            && self.tables.get(self.table).is_some_and(|table| {
                (0..table.columns.len()).any(|column| table.can_edit_column(column))
            })
    }
}

impl TuiApp {
    pub(super) fn open_record_browser(&mut self) {
        if self.progress_records_busy() {
            self.status = "Ya hay una operación de registros ABL en curso".into();
            return;
        }
        let tables = self.progress_schema.browse_tables();
        if tables.is_empty() {
            self.status = "El esquema ABL no contiene tablas para consultar".into();
            return;
        }
        self.open_focus_overlay(Overlay::Dialog);
        self.overlay = UiOverlay::BrowseRecords;
        self.record_browser = Some(RecordBrowser::new(tables));
    }

    pub(super) fn clear_record_browser(&mut self) {
        if self.record_browser.take().is_some() {
            self.progress_browse.cancel();
        }
    }

    pub(super) fn poll_progress_browse(&mut self) -> bool {
        // Project replacement always closes the dialog before publishing the new root.
        let Some(root) = self.project_root.as_deref() else {
            return false;
        };
        if let Some(result) = self.progress_records.poll(root) {
            let mut refresh = None;
            if let Some(browser) = &mut self.record_browser {
                match result {
                    Ok(RecordEvent::Preview(preview)) => {
                        browser.edit = None;
                        browser.review = Some(RecordReview {
                            preview,
                            after_selected: false,
                            offset: 0,
                            presented: false,
                        });
                    }
                    Ok(RecordEvent::Applied) => {
                        browser.stale = true;
                        refresh = browser
                            .accepted
                            .as_ref()
                            .map(|accepted| (accepted.page.offset, accepted.query.clone()));
                    }
                    Err(error) => browser.message = Some(error),
                }
            } else if let Err(error) = result {
                self.status = error;
            }
            if let Some((offset, query)) = refresh {
                self.request_browse_page(offset, query);
                if let Some(browser) = &mut self.record_browser {
                    browser.message = Some(match browser.message.take() {
                        Some(error) => format!("Cambio confirmado. {error}"),
                        None => "Cambio confirmado".into(),
                    });
                }
            }
            return true;
        }
        let Some(result) = self.progress_browse.poll(root) else {
            return false;
        };
        let Some(browser) = &mut self.record_browser else {
            return false;
        };
        let Some(query) = browser.pending.take() else {
            return false;
        };
        match result {
            Ok(page) => {
                browser.column = browser.column.min(page.columns.len().saturating_sub(1));
                browser.row = 0;
                browser.accepted = Some(AcceptedPage { page, query });
                browser.stale = false;
            }
            Err(error) => {
                browser.message = Some(if browser.stale {
                    format!("Cambio confirmado. Error al actualizar: {error}")
                } else {
                    error
                });
            }
        }
        true
    }

    pub(crate) fn progress_records_busy(&self) -> bool {
        self.progress_records.busy()
    }

    pub(super) fn handle_browse_records_key(&mut self, key: KeyEvent) {
        if self.progress_records_busy() {
            return;
        }
        let Some(browser) = &mut self.record_browser else {
            return;
        };
        if key.code == KeyCode::Esc {
            if browser.filter.take().is_some()
                || browser.detail.take().is_some()
                || browser.edit.take().is_some()
                || browser.review.take().is_some()
            {
                browser.message = None;
            } else if browser.pending.take().is_some() {
                self.progress_browse.cancel();
                browser.message = Some(if browser.stale {
                    "Cambio confirmado. Actualización cancelada".into()
                } else {
                    "Consulta cancelada".into()
                });
            } else if browser.accepted.take().is_some() {
                browser.row = 0;
                browser.column = 0;
                browser.message = None;
            } else {
                self.close_overlay();
            }
            return;
        }
        if browser.loading() {
            return;
        }
        if let Some(review) = &mut browser.review {
            if key.code == KeyCode::F(9) && key.modifiers.is_empty() && review.presented {
                self.apply_record_review();
            } else {
                review.navigate(key.code);
            }
            return;
        }
        if let Some(field) = &browser.edit {
            if key.code == KeyCode::Enter {
                let value = field.value.clone();
                self.request_record_review(Some(value));
            } else {
                self.handle_browse_field_key(key);
            }
            return;
        }
        if let Some(offset) = browser.detail {
            let last = browser.selected_value().map_or(0, |value| {
                jc_text::grapheme::graphemes(value)
                    .count()
                    .saturating_sub(1)
            });
            browser.detail = Some(match key.code {
                KeyCode::PageUp => offset.saturating_sub(10),
                KeyCode::PageDown => (offset + 10).min(last),
                _ => move_selection(offset, last, key.code),
            });
            return;
        }

        let requested = if let Some(field) = browser.filter.as_ref() {
            if key.code != KeyCode::Enter {
                self.handle_browse_field_key(key);
                return;
            }
            let filter = BrowseFilter {
                column: browser.column,
                value: field.value.clone(),
            };
            browser.filter = None;
            let descending = browser
                .accepted
                .as_ref()
                .is_some_and(AcceptedPage::descending);
            Some((
                0,
                Query {
                    filter: Some(filter),
                    descending,
                },
            ))
        } else if let Some(accepted) = &browser.accepted {
            let page = &accepted.page;
            let query = &accepted.query;
            match key.code {
                KeyCode::Left => {
                    browser.column = browser.column.saturating_sub(1);
                    None
                }
                KeyCode::Right => {
                    browser.column = (browser.column + 1).min(page.columns.len().saturating_sub(1));
                    None
                }
                KeyCode::Enter if !page.rows.is_empty() => {
                    browser.detail = Some(0);
                    None
                }
                KeyCode::Char('f') if is_plain_text(key) && !page.columns.is_empty() => {
                    let value = query
                        .filter
                        .as_ref()
                        .filter(|filter| filter.column == browser.column)
                        .map_or_else(String::new, |filter| filter.value.clone());
                    browser.filter = Some(ValueField {
                        cursor: value.len(),
                        value,
                    });
                    browser.message = None;
                    None
                }
                KeyCode::Char('x') if is_plain_text(key) && query.filter.is_some() => Some((
                    0,
                    Query {
                        filter: None,
                        descending: query.descending,
                    },
                )),
                KeyCode::Char('o') if is_plain_text(key) => Some((
                    0,
                    Query {
                        filter: query.filter.clone(),
                        descending: !query.descending,
                    },
                )),
                KeyCode::Char('e') if is_plain_text(key) && browser.can_edit() => {
                    let value = browser.selected_value().filter(|value| {
                        value.len() <= RECORD_EDIT_MAX_VALUE_BYTES
                            && !value.chars().any(char::is_control)
                    });
                    let value = value.unwrap_or_default().to_owned();
                    browser.edit = Some(ValueField {
                        cursor: value.len(),
                        value,
                    });
                    browser.message = None;
                    None
                }
                KeyCode::Char('r') if is_plain_text(key) && browser.can_restore() => {
                    self.request_record_review(None);
                    return;
                }
                KeyCode::F(5) => Some((page.offset, query.clone())),
                KeyCode::PageUp if page.offset > 0 => {
                    Some((page.offset.saturating_sub(BROWSE_PAGE_SIZE), query.clone()))
                }
                KeyCode::PageDown if page.has_next => {
                    Some((page.offset + BROWSE_PAGE_SIZE, query.clone()))
                }
                _ => {
                    browser.row =
                        move_selection(browser.row, page.rows.len().saturating_sub(1), key.code);
                    None
                }
            }
        } else if key.code == KeyCode::Enter {
            Some((0, Query::default()))
        } else {
            let last = browser.tables.len().saturating_sub(1);
            browser.table = match key.code {
                KeyCode::PageUp => browser.table.saturating_sub(10),
                KeyCode::PageDown => (browser.table + 10).min(last),
                _ => move_selection(browser.table, last, key.code),
            };
            None
        };
        if let Some((offset, query)) = requested {
            self.request_browse_page(offset, query);
        }
    }

    fn request_browse_page(&mut self, offset: usize, query: Query) {
        let (Some(browser), Some(root)) = (&mut self.record_browser, self.project_root.as_deref())
        else {
            return;
        };
        let Some(table) = browser.tables.get(browser.table) else {
            return;
        };
        if self.progress_schema.busy() || self.tasks.has_job(jc_progress::editor::JOB_KEY) {
            browser.message = Some("Ya hay una tarea ABL en curso".into());
            return;
        }
        let request = BrowseRequest {
            table: table.clone(),
            offset,
            filter: query.filter.clone(),
            descending: query.descending,
        };
        match self
            .progress_browse
            .request(root, request, self.syntax_wakeup.clone())
        {
            Ok(()) => {
                browser.pending = Some(query);
                browser.message = None;
            }
            Err(error) => browser.message = Some(error),
        }
    }

    fn handle_browse_field_key(&mut self, key: KeyEvent) {
        let edit = match key.code {
            KeyCode::Left => FieldEdit::Left,
            KeyCode::Right => FieldEdit::Right,
            KeyCode::Home => FieldEdit::Home,
            KeyCode::End => FieldEdit::End,
            KeyCode::Backspace => FieldEdit::Backspace,
            KeyCode::Delete => FieldEdit::Delete,
            KeyCode::Char('u') if key.modifiers == KeyModifiers::CONTROL => FieldEdit::Clear,
            KeyCode::Char(ch) if is_plain_text(key) => {
                self.edit_browse_field(FieldEdit::Insert(&ch.to_string()));
                return;
            }
            _ => return,
        };
        self.edit_browse_field(edit);
    }

    pub(super) fn edit_browse_field(&mut self, edit: FieldEdit<'_>) {
        if self.progress_records_busy() {
            return;
        }
        let Some(browser) = &mut self.record_browser else {
            return;
        };
        let (field, limit, invalid) = if let Some(field) = &mut browser.edit {
            (
                field,
                RECORD_EDIT_MAX_VALUE_BYTES,
                "El valor debe ocupar una sola línea",
            )
        } else if let Some(field) = &mut browser.filter {
            (
                field,
                BROWSE_MAX_FILTER_BYTES,
                "El filtro debe ocupar una sola línea",
            )
        } else {
            return;
        };
        if matches!(edit, FieldEdit::Insert(text) if text.chars().any(char::is_control)) {
            browser.message = Some(invalid.into());
            return;
        }
        match edit_field(&mut field.value, &mut field.cursor, edit, limit) {
            Ok(true) => browser.message = None,
            Ok(false) => {}
            Err(error) => browser.message = Some(error.into()),
        }
    }

    pub(super) fn browse_field_paste_budget(&self) -> Option<usize> {
        if self.progress_records_busy() {
            return None;
        }
        let browser = self.record_browser.as_ref()?;
        let (field, limit) = if let Some(field) = &browser.edit {
            (field, RECORD_EDIT_MAX_VALUE_BYTES)
        } else {
            (browser.filter.as_ref()?, BROWSE_MAX_FILTER_BYTES)
        };
        Some(limit.saturating_sub(field.value.len()).max(1))
    }

    fn request_record_review(&mut self, value: Option<String>) {
        let (Some(browser), Some(root)) = (&mut self.record_browser, self.project_root.as_deref())
        else {
            return;
        };
        let (Some(table), Some(row)) = (browser.tables.get(browser.table), browser.selected_row())
        else {
            return;
        };
        if self.progress_schema.busy() || self.tasks.has_job(jc_progress::editor::JOB_KEY) {
            browser.message = Some("Ya hay una tarea ABL en curso".into());
            return;
        }
        let wakeup = self.syntax_wakeup.clone();
        let result = match value {
            Some(value) => {
                self.progress_records
                    .request_edit(root, table, row, browser.column, value, wakeup)
            }
            None => self
                .progress_records
                .request_restore(root, table, row, wakeup),
        };
        browser.message = result.err();
    }

    fn apply_record_review(&mut self) {
        let (Some(browser), Some(root)) = (&mut self.record_browser, self.project_root.as_deref())
        else {
            return;
        };
        let Some(review) = browser.review.take() else {
            return;
        };
        browser.message = self
            .progress_records
            .apply(root, review.preview, self.syntax_wakeup.clone())
            .err();
    }
}

impl RecordReview {
    fn navigate(&mut self, code: KeyCode) {
        match code {
            KeyCode::Up | KeyCode::Down | KeyCode::Tab => {
                self.after_selected = !self.after_selected;
                self.offset = 0;
            }
            KeyCode::Home => self.offset = 0,
            KeyCode::Left => self.offset = self.offset.saturating_sub(1),
            KeyCode::Right | KeyCode::End => {
                let value = if self.after_selected {
                    &self.preview.after
                } else {
                    &self.preview.before
                };
                let last = value.as_ref().map_or(0, |value| {
                    jc_text::grapheme::graphemes(value)
                        .count()
                        .saturating_sub(1)
                });
                self.offset = if code == KeyCode::End {
                    last
                } else {
                    (self.offset + 1).min(last)
                };
            }
            _ => {}
        }
    }
}

fn move_selection(current: usize, last: usize, code: KeyCode) -> usize {
    match code {
        KeyCode::Up => current.saturating_sub(1),
        KeyCode::Down => (current + 1).min(last),
        KeyCode::Home => 0,
        KeyCode::End => last,
        _ => current,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::LaunchOptions;
    use crossterm::event::Event;
    use jc_app::PrimaryFocus;
    use jc_text::TextEncoding;

    fn key(app: &mut TuiApp, code: KeyCode) {
        app.handle_event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
    }

    fn shows(app: &mut TuiApp, needle: &str) -> bool {
        let frame = app.frame(100, 24);
        (0..frame.height()).any(|row| frame.row_text(row).contains(needle))
    }

    #[test]
    fn browse_records_grid_contains_input_and_preserves_editor() {
        let mut app = TuiApp::new(LaunchOptions::default());
        app.workbench.new_file("customer.p", TextEncoding::Utf8);
        app.workbench.insert_text("FOR EACH Customer:").unwrap();
        let before = app.workbench.active_bytes().unwrap();
        app.open_focus_overlay(Overlay::Dialog);
        app.overlay = UiOverlay::BrowseRecords;
        let mut browser = RecordBrowser::new(Vec::new());
        browser.accepted = Some(AcceptedPage {
            page: BrowsePage {
                columns: vec!["CustNum".into(), "Name".into()],
                rows: vec![
                    vec![Some("1".into()), Some("Primero".into())],
                    vec![Some("2".into()), Some("Café\x1b[2J".into())],
                ],
                offset: 20,
                has_next: true,
            },
            query: Query::default(),
        });
        app.record_browser = Some(browser);
        assert!(shows(&mut app, "Página 2"));
        key(&mut app, KeyCode::Down);
        key(&mut app, KeyCode::Right);
        key(&mut app, KeyCode::Enter);
        assert!(shows(&mut app, r"Café\u{1b}[2J"));
        key(&mut app, KeyCode::Esc);
        key(&mut app, KeyCode::Char('f'));
        assert!(shows(&mut app, "Filtro exacto · Name"));
        app.handle_event(Event::Paste("Café Norte".into()));
        app.handle_event(Event::Paste("\r\ninvalid".into()));
        assert!(shows(&mut app, "Café Norte"));
        assert!(shows(&mut app, "El filtro debe ocupar una sola línea"));
        let _ = app.frame(13, 8);
        key(&mut app, KeyCode::Esc);
        key(&mut app, KeyCode::Esc);
        key(&mut app, KeyCode::Esc);
        assert_eq!(app.overlay, UiOverlay::None);
        assert_eq!(app.focus.primary(), PrimaryFocus::Editor);
        assert_eq!(app.workbench.active_bytes().unwrap(), before);
    }

    #[test]
    #[ignore = "requiere una copia independiente de Sports2000 habilitada para edición"]
    fn progress_real_record_edit_and_restore_tui() {
        use jc_app::CommandId;
        use std::{
            path::PathBuf,
            thread,
            time::{Duration, Instant},
        };

        fn wait(app: &mut TuiApp) {
            let deadline = Instant::now() + Duration::from_secs(45);
            while app.progress_schema.busy()
                || app.progress_records_busy()
                || app
                    .record_browser
                    .as_ref()
                    .is_some_and(RecordBrowser::loading)
            {
                app.poll_tasks();
                assert!(Instant::now() < deadline, "{}", app.status);
                thread::sleep(Duration::from_millis(10));
            }
        }

        fn selected(app: &TuiApp) -> Option<&str> {
            app.record_browser.as_ref().unwrap().selected_value()
        }

        let root =
            PathBuf::from(std::env::var_os("CIDE_TEST_PROGRESS_RECORD_EDIT_PROJECT").unwrap());
        let mut app = TuiApp::new(LaunchOptions::default());
        app.set_explorer_root(root).unwrap();
        app.workbench.new_file("customer.p", TextEncoding::Utf8);
        app.workbench.insert_text("FOR EACH Customer:").unwrap();
        let editor = app.workbench.active_bytes().unwrap();
        app.dispatch(CommandId::ProgressRefreshSchema);
        wait(&mut app);
        assert!(app.command_context().has_database_schema, "{}", app.status);
        app.dispatch(CommandId::ProgressBrowseRecords);
        let customer = app
            .record_browser
            .as_ref()
            .unwrap()
            .tables
            .iter()
            .position(|table| table.name.eq_ignore_ascii_case("customer"))
            .expect("Customer debe estar en el esquema de Sports2000");
        for _ in 0..customer {
            key(&mut app, KeyCode::Down);
        }
        key(&mut app, KeyCode::Enter);
        wait(&mut app);
        let browser = app.record_browser.as_ref().unwrap();
        let page = &browser
            .accepted
            .as_ref()
            .unwrap_or_else(|| panic!("{:?}", browser.message))
            .page;
        let name = page
            .columns
            .iter()
            .position(|column| column.eq_ignore_ascii_case("name"))
            .unwrap();
        for _ in 0..name {
            key(&mut app, KeyCode::Right);
        }
        let original = selected(&app)
            .expect("La primera fila debe tener un nombre")
            .to_owned();
        let proposed = "CIDE TUI revision";
        assert_ne!(original, proposed, "Usa una copia limpia de Sports2000");

        for cancel in [true, false] {
            key(&mut app, KeyCode::Char('e'));
            assert!(shows(&mut app, "Nuevo valor"));
            app.handle_event(Event::Key(KeyEvent::new(
                KeyCode::Char('u'),
                KeyModifiers::CONTROL,
            )));
            app.handle_event(Event::Paste(proposed.into()));
            key(&mut app, KeyCode::Enter);
            wait(&mut app);
            let browser = app.record_browser.as_ref().unwrap();
            let review = &browser
                .review
                .as_ref()
                .unwrap_or_else(|| panic!("{:?}", browser.message))
                .preview;
            assert_eq!(review.before.as_deref(), Some(original.as_str()));
            assert_eq!(review.after.as_deref(), Some(proposed));
            assert_eq!(selected(&app), Some(original.as_str()));
            let _ = app.frame(21, 11);
            key(&mut app, KeyCode::F(9));
            assert!(!app.progress_records_busy());
            assert!(shows(&mut app, "Propuesto · «CIDE TUI revision»"));
            if cancel {
                key(&mut app, KeyCode::Esc);
                assert!(app.record_browser.as_ref().unwrap().review.is_none());
                key(&mut app, KeyCode::F(5));
                wait(&mut app);
                assert_eq!(selected(&app), Some(original.as_str()));
            } else {
                key(&mut app, KeyCode::F(9));
                assert!(app.progress_records_busy());
                key(&mut app, KeyCode::Esc);
                assert!(app.record_browser.is_some());
                wait(&mut app);
                assert!(shows(&mut app, "Cambio confirmado"));
                assert_eq!(selected(&app), Some(proposed));
            }
        }

        key(&mut app, KeyCode::Char('r'));
        wait(&mut app);
        let browser = app.record_browser.as_ref().unwrap();
        let review = &browser
            .review
            .as_ref()
            .unwrap_or_else(|| panic!("{:?}", browser.message))
            .preview;
        assert!(review.restoring);
        assert_eq!(review.before.as_deref(), Some(proposed));
        assert_eq!(review.after.as_deref(), Some(original.as_str()));
        assert!(shows(&mut app, "Restaurar ·"));
        key(&mut app, KeyCode::F(9));
        assert!(app.progress_records_busy());
        wait(&mut app);
        assert!(shows(&mut app, "Cambio confirmado"));
        assert_eq!(selected(&app), Some(original.as_str()));
        assert_eq!(app.workbench.active_bytes().unwrap(), editor);
    }
}
