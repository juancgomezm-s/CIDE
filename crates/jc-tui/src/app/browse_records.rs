use super::TuiApp;
use crate::{input::is_plain_text, state::UiOverlay};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use jc_app::{edit_field, FieldEdit, Overlay};
use jc_progress::editor::{
    BrowseFilter, BrowsePage, BrowseRequest, BrowseTable, BROWSE_MAX_FILTER_BYTES, BROWSE_PAGE_SIZE,
};

pub(crate) struct RecordBrowser {
    pub(crate) tables: Vec<BrowseTable>,
    pub(crate) table: usize,
    pub(crate) accepted: Option<AcceptedPage>,
    pending: Option<Query>,
    pub(crate) filter: Option<FilterField>,
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

pub(crate) struct FilterField {
    pub(crate) value: String,
    pub(crate) cursor: usize,
}

impl RecordBrowser {
    fn new(tables: Vec<BrowseTable>) -> Self {
        Self {
            tables,
            table: 0,
            accepted: None,
            pending: None,
            filter: None,
            row: 0,
            column: 0,
            detail: None,
            message: None,
        }
    }

    pub(crate) fn loading(&self) -> bool {
        self.pending.is_some()
    }

    pub(crate) fn selected_value(&self) -> Option<&str> {
        self.accepted
            .as_ref()?
            .page
            .rows
            .get(self.row)?
            .get(self.column)?
            .as_deref()
    }
}

impl TuiApp {
    pub(super) fn open_record_browser(&mut self) {
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
            }
            Err(error) => browser.message = Some(error),
        }
        true
    }

    pub(super) fn handle_browse_records_key(&mut self, key: KeyEvent) {
        let Some(browser) = &mut self.record_browser else {
            return;
        };
        if key.code == KeyCode::Esc {
            if browser.filter.take().is_some() || browser.detail.take().is_some() {
                browser.message = None;
            } else if browser.pending.take().is_some() {
                self.progress_browse.cancel();
                browser.message = Some("Consulta cancelada".into());
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
                self.handle_browse_filter_key(key);
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
                    browser.filter = Some(FilterField {
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

    fn handle_browse_filter_key(&mut self, key: KeyEvent) {
        let edit = match key.code {
            KeyCode::Left => FieldEdit::Left,
            KeyCode::Right => FieldEdit::Right,
            KeyCode::Home => FieldEdit::Home,
            KeyCode::End => FieldEdit::End,
            KeyCode::Backspace => FieldEdit::Backspace,
            KeyCode::Delete => FieldEdit::Delete,
            KeyCode::Char('u') if key.modifiers == KeyModifiers::CONTROL => FieldEdit::Clear,
            KeyCode::Char(ch) if is_plain_text(key) => {
                self.edit_browse_filter(FieldEdit::Insert(&ch.to_string()));
                return;
            }
            _ => return,
        };
        self.edit_browse_filter(edit);
    }

    pub(super) fn edit_browse_filter(&mut self, edit: FieldEdit<'_>) {
        let Some(browser) = &mut self.record_browser else {
            return;
        };
        let Some(field) = &mut browser.filter else {
            return;
        };
        if matches!(edit, FieldEdit::Insert(text) if text.chars().any(char::is_control)) {
            browser.message = Some("El filtro debe ocupar una sola línea".into());
            return;
        }
        match edit_field(
            &mut field.value,
            &mut field.cursor,
            edit,
            BROWSE_MAX_FILTER_BYTES,
        ) {
            Ok(true) => browser.message = None,
            Ok(false) => {}
            Err(error) => browser.message = Some(error.into()),
        }
    }

    pub(super) fn browse_filter_paste_budget(&self) -> Option<usize> {
        let field = self.record_browser.as_ref()?.filter.as_ref()?;
        Some(
            BROWSE_MAX_FILTER_BYTES
                .saturating_sub(field.value.len())
                .max(1),
        )
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
        let key = |app: &mut TuiApp, code| {
            app.handle_event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
        };
        let shows = |app: &mut TuiApp, needle: &str| {
            let frame = app.frame(80, 24);
            (0..frame.height()).any(|row| frame.row_text(row).contains(needle))
        };

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
}
