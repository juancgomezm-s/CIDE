use super::TuiApp;
use crate::{input::is_plain_text, state::UiOverlay};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use jc_app::{data_connections::Draft, edit_field, FieldEdit, Overlay};
use jc_platform::{
    sql::{Engine, Filter, Page, PageRequest, Relation, RelationDetails, PAGE_SIZE},
    sql_worker::{self, Event, Query, Reader},
};
use std::path::PathBuf;

pub(crate) struct SqlBrowser {
    root: PathBuf,
    reader: Reader,
    pub(crate) profile: String,
    pub(crate) tables: Vec<Relation>,
    pub(crate) table: usize,
    pub(crate) page: Option<Page>,
    query: PageRequest,
    pending: Option<PageRequest>,
    pub(crate) column: usize,
    pub(crate) row: usize,
    pub(crate) details: Option<RelationDetails>,
    pub(crate) detail_row: usize,
    pub(crate) filter: Option<(String, usize)>,
    pub(crate) message: String,
}

impl SqlBrowser {
    pub(crate) fn busy(&self) -> bool {
        self.reader.busy()
    }

    fn request(&mut self, query: Query) -> bool {
        match self.reader.request(query) {
            Ok(()) => {
                self.message = "Consultando…".into();
                true
            }
            Err(error) => {
                self.message = error;
                false
            }
        }
    }

    fn page(&mut self, query: PageRequest) {
        let Some(relation) = self.tables.get(self.table).cloned() else {
            return;
        };
        if self.request(Query::Page(relation, query.clone())) {
            self.pending = Some(query);
        }
    }
}

impl TuiApp {
    pub(super) fn open_sql_browser(&mut self, draft: &Draft) {
        let Some(root) = self.project_root.clone() else {
            return;
        };
        let engine = match draft.engine() {
            jc_app::data_connections::Engine::PostgreSql => Engine::PostgreSql,
            jc_app::data_connections::Engine::MySql => Engine::MySql,
            jc_app::data_connections::Engine::OpenEdge => return,
        };
        let values = std::array::from_fn(|index| draft.value(index));
        let wakeup = self.syntax_wakeup.clone();
        let reader = match sql_worker::options(engine, values)
            .and_then(|options| Reader::open(options, move || wakeup.signal()))
        {
            Ok(reader) => reader,
            Err(error) => {
                self.status = error;
                return;
            }
        };
        self.close_overlay();
        self.open_focus_overlay(Overlay::Dialog);
        self.overlay = UiOverlay::SqlRecords;
        self.sql_browser = Some(SqlBrowser {
            root,
            reader,
            profile: format!("{} · {}", draft.engine().label(), draft.value(0)),
            tables: Vec::new(),
            table: 0,
            page: None,
            query: PageRequest::default(),
            pending: None,
            column: 0,
            row: 0,
            details: None,
            detail_row: 0,
            filter: None,
            message: "Conectando y leyendo el catálogo…".into(),
        });
    }

    pub(super) fn poll_sql_browser(&mut self) -> bool {
        let Some(browser) = &mut self.sql_browser else {
            return false;
        };
        if self.project_root.as_ref() != Some(&browser.root) {
            self.close_overlay();
            return true;
        }
        let Some(event) = browser.reader.poll() else {
            return false;
        };
        browser.message.clear();
        match event {
            Ok(Event::Catalog(tables)) => {
                browser.message = format!("Conexión de lectura · {} tablas y vistas", tables.len());
                browser.tables = tables;
                browser.table = 0;
                browser.page = None;
                browser.details = None;
            }
            Ok(Event::Details(details)) => {
                browser.details = Some(details);
                browser.detail_row = 0;
            }
            Ok(Event::Page(page)) => {
                browser.row = 0;
                browser.column = browser.column.min(page.columns.len().saturating_sub(1));
                browser.page = Some(page);
                if let Some(query) = browser.pending.take() {
                    browser.query = query;
                }
            }
            Err(error) => {
                browser.pending = None;
                browser.message = error;
            }
        }
        true
    }

    pub(super) fn handle_sql_browser_key(&mut self, key: KeyEvent) {
        let Some(browser) = &mut self.sql_browser else {
            return;
        };
        if key.code == KeyCode::Esc {
            if browser.busy()
                || (browser.filter.take().is_none()
                    && browser.details.take().is_none()
                    && browser.page.take().is_none())
            {
                self.close_overlay();
            }
            return;
        }
        if browser.busy() {
            return;
        }
        if let Some((value, cursor)) = &mut browser.filter {
            let edit = match key.code {
                KeyCode::Enter => {
                    let mut query = browser.query.clone();
                    query.offset = 0;
                    query.filter = Some(Filter {
                        column: browser.column,
                        value: value.clone(),
                    });
                    browser.filter = None;
                    browser.page(query);
                    return;
                }
                KeyCode::Left => FieldEdit::Left,
                KeyCode::Right => FieldEdit::Right,
                KeyCode::Backspace => FieldEdit::Backspace,
                KeyCode::Delete => FieldEdit::Delete,
                KeyCode::Home => FieldEdit::Home,
                KeyCode::End => FieldEdit::End,
                KeyCode::Char('u') if key.modifiers == KeyModifiers::CONTROL => FieldEdit::Clear,
                KeyCode::Char(ch) if is_plain_text(key) => {
                    let mut encoded = [0; 4];
                    if let Err(error) = edit_field(
                        value,
                        cursor,
                        FieldEdit::Insert(ch.encode_utf8(&mut encoded)),
                        256,
                    ) {
                        browser.message = error.into();
                    }
                    return;
                }
                _ => return,
            };
            if let Err(error) = edit_field(value, cursor, edit, 256) {
                browser.message = error.into();
            }
            return;
        }
        if let Some(details) = &browser.details {
            let count = details.columns.len() + details.indexes.len() + details.foreign_keys.len();
            browser.detail_row = navigate(browser.detail_row, count, key.code);
            return;
        }
        if key.code == KeyCode::F(5) {
            browser.request(Query::Refresh);
            return;
        }
        if key.code == KeyCode::Char('m') && is_plain_text(key) {
            if let Some(relation) = browser.tables.get(browser.table).cloned() {
                browser.request(Query::Details(relation));
            }
            return;
        }
        if let Some(page) = &browser.page {
            let mut next = browser.query.clone();
            match key.code {
                KeyCode::Left => browser.column = browser.column.saturating_sub(1),
                KeyCode::Right => {
                    browser.column = (browser.column + 1).min(page.columns.len().saturating_sub(1))
                }
                KeyCode::PageUp if page.offset > 0 => {
                    next.offset = page.offset.saturating_sub(PAGE_SIZE);
                    browser.page(next);
                }
                KeyCode::PageDown if page.has_next => {
                    next.offset = page.offset + PAGE_SIZE;
                    browser.page(next);
                }
                KeyCode::Char('o') if is_plain_text(key) => {
                    next.offset = 0;
                    next.descending = !next.descending;
                    browser.page(next);
                }
                KeyCode::Char('x') if is_plain_text(key) && next.filter.is_some() => {
                    next.offset = 0;
                    next.filter = None;
                    browser.page(next);
                }
                KeyCode::Char('f') if is_plain_text(key) && !page.columns.is_empty() => {
                    let value = next
                        .filter
                        .filter(|filter| filter.column == browser.column)
                        .map_or_else(String::new, |filter| filter.value);
                    let cursor = value.len();
                    browser.filter = Some((value, cursor));
                }
                _ => browser.row = navigate(browser.row, page.rows.len(), key.code),
            }
        } else if key.code == KeyCode::Enter {
            browser.page(PageRequest::default());
        } else {
            browser.table = navigate(browser.table, browser.tables.len(), key.code);
        }
    }

    pub(super) fn paste_sql_filter(&mut self, text: &str) {
        let Some(browser) = &mut self.sql_browser else {
            return;
        };
        let Some((field, cursor)) = &mut browser.filter else {
            return;
        };
        if text.chars().any(char::is_control) {
            browser.message = "El filtro debe ocupar una sola línea".into();
            return;
        }
        if let Err(error) = edit_field(field, cursor, FieldEdit::Insert(text), 256) {
            browser.message = error.into();
        }
    }

    pub(super) fn sql_filter_paste_budget(&self) -> Option<usize> {
        let browser = self.sql_browser.as_ref()?;
        if browser.busy() {
            return None;
        }
        let (value, _) = browser.filter.as_ref()?;
        Some(256usize.saturating_sub(value.len()).max(1))
    }
}

fn navigate(index: usize, count: usize, key: KeyCode) -> usize {
    let last = count.saturating_sub(1);
    match key {
        KeyCode::Up => index.saturating_sub(1),
        KeyCode::Down => (index + 1).min(last),
        KeyCode::Home => 0,
        KeyCode::End => last,
        _ => index,
    }
}
