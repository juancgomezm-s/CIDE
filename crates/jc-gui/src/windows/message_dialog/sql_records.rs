//! Read-only native SQL record browser. The active ODBC session belongs to its worker.
use super::{records, *};
use crate::windows::notify::Bridge;
use jc_platform::{
    sql::{
        ConnectOptions, Filter, Page, PageRequest, Relation, RelationDetails, RelationKind,
        PAGE_SIZE,
    },
    sql_worker::{Event, Query, Reader},
};
use std::{cell::RefCell, ptr::null_mut, sync::Arc};

const CLOSE: u16 = 2;
const RELATION_LABEL: u16 = 100;
const RELATION: u16 = 101;
const OPEN: u16 = 102;
const FILTER_LABEL: u16 = 103;
const FIELD: u16 = 104;
const VALUE: u16 = 106;
const APPLY: u16 = 107;
const SORT: u16 = 108;
const STATUS: u16 = 109;
const REFRESH: u16 = 110;
const DETAILS: u16 = 111;
const PREVIOUS: u16 = 112;
const NEXT: u16 = 113;
const BUTTONS: [(u16, &str); 8] = [
    (OPEN, "Abrir"),
    (APPLY, "Aplicar filtro"),
    (SORT, "Invertir orden"),
    (REFRESH, "Actualizar esquema"),
    (DETAILS, "Metadatos…"),
    (PREVIOUS, "Anterior"),
    (NEXT, "Siguiente"),
    (CLOSE, "Cerrar"),
];

const WM_INITDIALOG: u32 = 0x110;
const WM_DRAWITEM: u32 = 0x2b;
const WM_SETFONT: u32 = 0x30;
const WM_CTLCOLORLISTBOX: u32 = 0x134;
const WM_CTLCOLOREDIT: u32 = 0x133;
const WM_CTLCOLORDLG: u32 = 0x136;
const WM_CTLCOLORSTATIC: u32 = 0x138;
const DM_GETDEFID: u32 = 0x400;
const DM_SETDEFID: u32 = 0x401;
const CB_ADDSTRING: u32 = 0x143;
const CB_GETCURSEL: u32 = 0x147;
const CB_RESETCONTENT: u32 = 0x14b;
const CB_SETCURSEL: u32 = 0x14e;
const EM_SETLIMITTEXT: u32 = 0xc5;
const MAX_FILTER_BYTES: usize = 256;

#[link(name = "user32")]
extern "system" {
    fn EnableWindow(hwnd: Hwnd, enabled: i32) -> i32;
    fn IsWindowEnabled(hwnd: Hwnd) -> i32;
    fn SetWindowTextW(hwnd: Hwnd, text: *const u16) -> i32;
}

struct Browser<'a> {
    dialog: Dialog<'a>,
    options: ConnectOptions,
    bridge: Arc<Bridge>,
    state: RefCell<State>,
}

struct State {
    reader: Option<Reader>,
    pending: Option<Pending>,
    relations: Vec<Relation>,
    accepted: Option<Accepted>,
    grid: Hwnd,
    grid_bounds: Rect,
    column_width: i32,
}

impl Default for State {
    fn default() -> Self {
        Self {
            reader: None,
            pending: None,
            relations: Vec::new(),
            accepted: None,
            grid: null_mut(),
            grid_bounds: Rect::default(),
            column_width: 0,
        }
    }
}

enum Pending {
    Catalog,
    Details(Relation),
    Page(Relation, PageRequest),
}

struct Accepted {
    relation: Relation,
    request: PageRequest,
    page: Page,
}

impl Pending {
    fn query(&self) -> Query {
        match self {
            Self::Catalog => Query::Refresh,
            Self::Details(relation) => Query::Details(relation.clone()),
            Self::Page(relation, request) => Query::Page(relation.clone(), request.clone()),
        }
    }
}

impl Browser<'_> {
    fn template(&self) -> Vec<u32> {
        let mut words = template_header(self.dialog.title, (6 + BUTTONS.len()) as u16);
        let bounds = [0, 0, 1, 1];
        for (id, label) in [
            (RELATION_LABEL, "Tabla o vista"),
            (FILTER_LABEL, "Filtro exacto"),
            (STATUS, ""),
        ] {
            styled_control(&mut words, id, bounds, 0x82, label, 0x80);
        }
        for id in [RELATION, FIELD] {
            styled_control(&mut words, id, bounds, 0x85, "", 0x21_0203);
        }
        styled_control(&mut words, VALUE, bounds, 0x81, "", 0x81_0080);
        for (id, label) in BUTTONS {
            control(&mut words, id, bounds, 0x80, label);
        }
        pack_template(words)
    }

    unsafe fn arrange(&self, hwnd: Hwnd) -> bool {
        let Some((dpi, _, _)) = position(hwnd, self.dialog.parent, [1040, 630]) else {
            return false;
        };
        let mut client = Rect::default();
        if GetClientRect(hwnd, &mut client) == 0
            || client.right < dpi.pixels(620)
            || client.bottom < dpi.pixels(282)
        {
            return false;
        }
        let p = |units| dpi.pixels(units);
        let width = client.right;
        let bottom = client.bottom;
        let margin = p(16);
        let footer = bottom - p(48);
        for (id, x, y, w, h) in [
            (RELATION_LABEL, margin, p(49), p(92), p(24)),
            (RELATION, p(112), p(44), width - p(230), p(238)),
            (OPEN, width - p(112), p(44), p(96), p(28)),
            (FILTER_LABEL, margin, p(84), p(92), p(24)),
            (FIELD, p(112), p(79), p(146), p(176)),
            (VALUE, p(266), p(79), width - p(518), p(27)),
            (APPLY, width - p(244), p(79), p(108), p(28)),
            (SORT, width - p(128), p(79), p(112), p(28)),
            (STATUS, margin, bottom - p(94), width - 2 * margin, p(42)),
            (REFRESH, margin, footer, p(144), p(30)),
            (DETAILS, p(168), footer, p(110), p(30)),
            (PREVIOUS, p(286), footer, p(88), p(30)),
            (NEXT, p(382), footer, p(88), p(30)),
            (CLOSE, width - p(108), footer, p(92), p(30)),
        ] {
            let control = child(hwnd, id);
            SendMessageW(control, WM_SETFONT, self.dialog.font as usize, 0);
            if SetWindowPos(control, null_mut(), x, y, w, h, 0x14) == 0 {
                return false;
            }
        }
        SendMessageW(child(hwnd, VALUE), EM_SETLIMITTEXT, MAX_FILTER_BYTES, 0);
        let mut state = self.state.borrow_mut();
        state.grid_bounds = Rect {
            left: margin,
            top: p(116),
            right: width - margin,
            bottom: bottom - p(102),
        };
        state.column_width = p(144);
        true
    }

    unsafe fn connect(&self, hwnd: Hwnd) {
        let mut state = self.state.borrow_mut();
        let notify = self.bridge.clone();
        match Reader::open(self.options.clone(), move || notify.signal()) {
            Ok(reader) => {
                state.reader = Some(reader);
                state.pending = Some(Pending::Catalog);
                status(hwnd, "Estableciendo la conexión y leyendo el catálogo…");
            }
            Err(error) => report(hwnd, &state, &error),
        }
    }

    unsafe fn controls(&self, hwnd: Hwnd) {
        let state = self.state.borrow();
        let idle =
            state.pending.is_none() && state.reader.as_ref().is_none_or(|reader| !reader.busy());
        let selected = state
            .reader
            .as_ref()
            .and_then(|_| selected_relation(hwnd, &state));
        let browsing = state
            .accepted
            .as_ref()
            .filter(|accepted| Some(&accepted.relation) == selected);
        for (id, enabled) in [
            (RELATION, idle && selected.is_some()),
            (OPEN, idle && selected.is_some()),
            (DETAILS, idle && selected.is_some()),
            (FIELD, idle && browsing.is_some()),
            (
                VALUE,
                idle && browsing.is_some()
                    && selected_index(hwnd, FIELD).is_some_and(|index| index > 0),
            ),
            (APPLY, idle && browsing.is_some()),
            (SORT, idle && browsing.is_some()),
            (REFRESH, idle),
            (
                PREVIOUS,
                idle && browsing.is_some_and(|accepted| accepted.page.offset > 0),
            ),
            (
                NEXT,
                idle && browsing.is_some_and(|accepted| accepted.page.has_next),
            ),
        ] {
            EnableWindow(child(hwnd, id), i32::from(enabled));
        }
    }

    unsafe fn command(&self, hwnd: Hwnd, id: u16) {
        if IsWindowEnabled(child(hwnd, id)) == 0 {
            return;
        }
        let mut state = self.state.borrow_mut();
        let pending = match id {
            REFRESH if state.reader.is_none() => {
                drop(state);
                self.connect(hwnd);
                self.controls(hwnd);
                return;
            }
            REFRESH => Pending::Catalog,
            OPEN | DETAILS => {
                let Some(relation) = selected_relation(hwnd, &state).cloned() else {
                    return;
                };
                if id == OPEN {
                    Pending::Page(relation, PageRequest::default())
                } else {
                    Pending::Details(relation)
                }
            }
            _ => {
                let Some(accepted) = state.accepted.as_ref() else {
                    return;
                };
                let mut request = accepted.request.clone();
                match id {
                    APPLY => match filter(hwnd, accepted.page.columns.len()) {
                        Ok(filter) => {
                            request.filter = filter;
                            request.offset = 0;
                        }
                        Err(error) => {
                            report(hwnd, &state, &error);
                            return;
                        }
                    },
                    SORT => {
                        request.descending = !request.descending;
                        request.offset = 0;
                    }
                    PREVIOUS => request.offset = request.offset.saturating_sub(PAGE_SIZE),
                    NEXT if accepted.page.has_next => request.offset += PAGE_SIZE,
                    _ => return,
                }
                Pending::Page(accepted.relation.clone(), request)
            }
        };
        let Some(reader) = state.reader.as_mut() else {
            return;
        };
        match reader.request(pending.query()) {
            Ok(()) => {
                let progress = match pending {
                    Pending::Catalog => "Actualizando el catálogo SQL…",
                    Pending::Details(_) => "Consultando metadatos…",
                    Pending::Page(_, _) => "Consultando registros…",
                };
                state.pending = Some(pending);
                report(hwnd, &state, progress);
            }
            Err(error) => {
                state.reader = None;
                report(hwnd, &state, &error);
            }
        }
        drop(state);
        self.controls(hwnd);
    }

    unsafe fn poll(&self, hwnd: Hwnd) {
        let mut state = self.state.borrow_mut();
        let Some(result) = state.reader.as_mut().and_then(Reader::poll) else {
            return;
        };
        let mut details = None;
        match (state.pending.take(), result) {
            (Some(Pending::Catalog), Ok(Event::Catalog(relations))) => {
                if !state.install_catalog(hwnd, relations) {
                    self.dialog.failed.set(true);
                    EndDialog(hwnd, 0);
                    return;
                }
            }
            (Some(Pending::Page(relation, request)), Ok(Event::Page(page))) => {
                if !self.install_page(
                    hwnd,
                    &mut state,
                    Accepted {
                        relation,
                        request,
                        page,
                    },
                ) {
                    self.dialog.failed.set(true);
                    EndDialog(hwnd, 0);
                    return;
                }
            }
            (Some(Pending::Details(relation)), Ok(Event::Details(result))) => {
                details = Some((relation, result));
                report(hwnd, &state, "Metadatos consultados");
            }
            (pending, Err(error)) => {
                if matches!(pending, Some(Pending::Catalog)) && state.relations.is_empty() {
                    state.reader = None;
                }
                report(hwnd, &state, &error);
            }
            _ => report(hwnd, &state, "La respuesta SQL no coincide con la consulta"),
        }
        drop(state);
        self.controls(hwnd);
        if let Some((relation, details)) = details {
            let message = metadata(&relation_name(&relation), &details);
            let answer = self
                .dialog
                .child(hwnd, "Metadatos SQL", &message, "Cerrar")
                .and_then(|mut dialog| {
                    dialog.scrollable = true;
                    super::show(hwnd, &mut dialog)
                });
            if let Err(error) = answer {
                report(hwnd, &self.state.borrow(), &error);
            }
        }
    }

    unsafe fn install_page(&self, hwnd: Hwnd, state: &mut State, accepted: Accepted) -> bool {
        if !self.install_page_controls(
            hwnd,
            state,
            &accepted.page,
            accepted.request.filter.as_ref(),
        ) {
            return false;
        }
        status(hwnd, &page_heading(&accepted));
        state.accepted = Some(accepted);
        true
    }

    unsafe fn install_page_controls(
        &self,
        hwnd: Hwnd,
        state: &mut State,
        page: &Page,
        filter: Option<&Filter>,
    ) -> bool {
        let columns = &page.columns;
        if state
            .accepted
            .as_ref()
            .is_none_or(|previous| previous.page.columns != *columns)
        {
            if !state.grid.is_null() {
                DestroyWindow(state.grid);
            }
            let names: Vec<_> = columns.iter().map(|name| printable(name)).collect();
            let Some(grid) = records::create_grid(
                hwnd,
                state.grid_bounds,
                self.dialog.font,
                state.column_width,
                &names,
            ) else {
                return false;
            };
            state.grid = grid;
        }
        if !records::install_rows(state.grid, columns, &page.rows, "⟨NULL⟩") {
            return false;
        }
        let names = std::iter::once("Sin filtro".to_owned())
            .chain(columns.iter().map(|name| printable(name)));
        if !set_choices(
            hwnd,
            FIELD,
            names,
            Some(filter.map_or(0, |filter| filter.column + 1)),
        ) {
            return false;
        }
        SetWindowTextW(
            child(hwnd, VALUE),
            wide(filter.map_or("", |filter| filter.value.as_str())).as_ptr(),
        );
        true
    }
}

impl State {
    unsafe fn install_catalog(&mut self, hwnd: Hwnd, relations: Vec<Relation>) -> bool {
        let previous = selected_relation(hwnd, self);
        let selected = previous
            .and_then(|selected| relations.iter().position(|relation| relation == selected));
        let keep_page = self
            .accepted
            .as_ref()
            .is_some_and(|accepted| relations.contains(&accepted.relation));
        if !keep_page {
            self.accepted = None;
            if !self.grid.is_null() {
                DestroyWindow(self.grid);
                self.grid = null_mut();
            }
            SendMessageW(child(hwnd, FIELD), CB_RESETCONTENT, 0, 0);
            SetWindowTextW(child(hwnd, VALUE), wide("").as_ptr());
        }
        self.relations = relations;
        let len = self.relations.len();
        let names = self.relations.iter().map(|relation| {
            let kind = match relation.kind() {
                RelationKind::Table => "Tabla",
                RelationKind::View => "Vista",
            };
            format!("{} · {kind}", relation_name(relation))
        });
        if !set_choices(
            hwnd,
            RELATION,
            names,
            selected.or_else(|| (len > 0).then_some(0)),
        ) {
            return false;
        }
        if len == 0 {
            status(hwnd, "El perfil no tiene tablas ni vistas visibles");
        } else {
            report(
                hwnd,
                self,
                &format!("{len} tablas y vistas · Elige una y pulsa Abrir"),
            );
        }
        true
    }
}

unsafe fn child(hwnd: Hwnd, id: u16) -> Hwnd {
    GetDlgItem(hwnd, i32::from(id))
}

unsafe fn status(hwnd: Hwnd, text: &str) {
    SetWindowTextW(child(hwnd, STATUS), wide(text).as_ptr());
}

unsafe fn report(hwnd: Hwnd, state: &State, message: &str) {
    let mut text = printable(message);
    if let Some(accepted) = &state.accepted {
        text.push_str("\r\n");
        text.push_str(&page_heading(accepted));
    }
    status(hwnd, &text);
}

unsafe fn set_choices(
    hwnd: Hwnd,
    id: u16,
    names: impl IntoIterator<Item = String>,
    selected: Option<usize>,
) -> bool {
    let combo = child(hwnd, id);
    SendMessageW(combo, CB_RESETCONTENT, 0, 0);
    for name in names {
        if SendMessageW(combo, CB_ADDSTRING, 0, wide(&name).as_ptr() as isize) < 0 {
            return false;
        }
    }
    if let Some(index) = selected {
        SendMessageW(combo, CB_SETCURSEL, index, 0);
    }
    true
}

unsafe fn selected_index(hwnd: Hwnd, id: u16) -> Option<usize> {
    SendMessageW(child(hwnd, id), CB_GETCURSEL, 0, 0)
        .try_into()
        .ok()
}

unsafe fn selected_relation(hwnd: Hwnd, state: &State) -> Option<&Relation> {
    state.relations.get(selected_index(hwnd, RELATION)?)
}

unsafe fn filter(hwnd: Hwnd, columns: usize) -> Result<Option<Filter>, String> {
    let Some(column) = selected_index(hwnd, FIELD).and_then(|index| index.checked_sub(1)) else {
        return Ok(None);
    };
    if column >= columns {
        return Err("Selecciona una columna del resultado actual".into());
    }
    let mut text = [0; MAX_FILTER_BYTES + 1];
    let len = GetWindowTextW(child(hwnd, VALUE), text.as_mut_ptr(), text.len() as i32);
    let value = String::from_utf16(&text[..len as usize])
        .map_err(|_| "El filtro contiene texto inválido")?;
    if value.len() > MAX_FILTER_BYTES {
        return Err("El filtro SQL admite hasta 256 bytes de texto".into());
    }
    Ok(Some(Filter { column, value }))
}

fn printable(value: &str) -> String {
    let mut text = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_control() {
            text.extend(ch.escape_default());
        } else {
            text.push(ch);
        }
    }
    text
}

fn relation_name(relation: &Relation) -> String {
    format!(
        "{}.{}",
        printable(relation.schema()),
        printable(relation.name())
    )
}

fn page_heading(accepted: &Accepted) -> String {
    let relation = relation_name(&accepted.relation);
    let page = accepted.page.offset / PAGE_SIZE + 1;
    let count = accepted.page.rows.len();
    if count == 0 {
        format!("Solo lectura · {relation} · Página {page} · Sin registros")
    } else {
        let start = accepted.page.offset + 1;
        let end = accepted.page.offset + count;
        format!("Solo lectura · {relation} · Página {page} · Registros {start}–{end}")
    }
}

fn metadata(name: &str, details: &RelationDetails) -> String {
    use std::fmt::Write as _;
    let mut text = format!("{name}\r\n\r\nColumnas\r\n");
    for column in &details.columns {
        let nullable = if column.nullable {
            "admite NULL"
        } else {
            "obligatoria"
        };
        write!(
            text,
            "{} · {} · {nullable}\r\n",
            printable(&column.name),
            printable(&column.data_type)
        )
        .unwrap();
    }
    text.push_str("\r\nÍndices\r\n");
    if details.indexes.is_empty() {
        text.push_str("Sin índices visibles\r\n");
    }
    for part in &details.indexes {
        let kind = if part.primary {
            " · Clave primaria"
        } else if part.unique {
            " · Único"
        } else {
            ""
        };
        write!(
            text,
            "{}{kind} · {}\r\n",
            printable(&part.index),
            printable(&part.definition)
        )
        .unwrap();
    }
    text.push_str("\r\nClaves foráneas\r\n");
    if details.foreign_keys.is_empty() {
        text.push_str("Sin claves foráneas visibles\r\n");
    }
    for part in &details.foreign_keys {
        write!(
            text,
            "{} · {} → {}.{}.{}\r\n",
            printable(&part.constraint),
            printable(&part.column),
            printable(&part.referenced_schema),
            printable(&part.referenced_table),
            printable(&part.referenced_column)
        )
        .unwrap();
    }
    text
}

unsafe extern "system" fn procedure(hwnd: Hwnd, message: u32, wp: usize, lp: isize) -> isize {
    if message == WM_INITDIALOG {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, lp);
        let browser = &*(lp as *const Browser<'_>);
        if !browser.arrange(hwnd) {
            browser.dialog.failed.set(true);
            EndDialog(hwnd, 0);
            return 0;
        }
        browser.bridge.attach(hwnd);
        browser.connect(hwnd);
        browser.controls(hwnd);
        SetFocus(child(hwnd, CLOSE));
        return 0;
    }
    let data = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
    if data == 0 {
        return 0;
    }
    let browser = &*(data as *const Browser<'_>);
    match message {
        WM_COMMAND => match (wp as u16, (wp >> 16) as u16) {
            (CLOSE, 0) => {
                EndDialog(hwnd, 0);
                1
            }
            (RELATION | FIELD, 1) => {
                browser.controls(hwnd);
                1
            }
            (id @ (OPEN | REFRESH | DETAILS | APPLY | SORT | PREVIOUS | NEXT), 0) => {
                browser.command(hwnd, id);
                1
            }
            _ => 0,
        },
        WM_CLOSE => {
            EndDialog(hwnd, 0);
            1
        }
        WM_DESTROY => {
            browser.bridge.detach();
            0
        }
        TASKS_MESSAGE => {
            browser.bridge.acknowledge();
            browser.poll(hwnd);
            1
        }
        DM_GETDEFID => {
            let focus = GetFocus();
            let default = if [child(hwnd, FIELD), child(hwnd, VALUE)].contains(&focus) {
                APPLY
            } else {
                OPEN
            };
            let id = BUTTONS
                .iter()
                .find(|(id, _)| child(hwnd, *id) == focus)
                .map_or(default, |(id, _)| *id);
            let result = if IsWindowEnabled(child(hwnd, id)) == 0 {
                0
            } else {
                id as isize | (0x534b << 16)
            };
            SetWindowLongPtrW(hwnd, 0, result);
            1
        }
        DM_SETDEFID => 1,
        WM_CTLCOLORDLG | WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => {
            SetTextColor(wp as Hdc, shell::TEXT);
            SetBkColor(wp as Hdc, shell::RAISED);
            browser.dialog.brush as isize
        }
        WM_DRAWITEM => {
            let item = &*(lp as *const DrawItem);
            let Some((_, label)) = BUTTONS
                .iter()
                .find(|(id, _)| u32::from(*id) == item.control_id)
            else {
                return 0;
            };
            if item.control_type != 4 {
                return 0;
            }
            if paint_button_label(item, &browser.dialog, label).is_err() {
                browser.dialog.failed.set(true);
            }
            1
        }
        WM_PAINT => {
            paint_frame(hwnd, &browser.dialog);
            1
        }
        _ => 0,
    }
}

pub(in crate::windows) unsafe fn show(
    parent: Hwnd,
    title: &str,
    options: ConnectOptions,
) -> Result<(), String> {
    if !records::initialize_list_class() {
        return Err("No se pudo preparar la cuadrícula SQL".into());
    }
    let browser = Browser {
        dialog: create(parent, title, "", "Cerrar")?,
        options,
        bridge: Arc::default(),
        state: RefCell::default(),
    };
    let template = browser.template();
    let result = DialogBoxIndirectParamW(
        GetModuleHandleW(null_mut()),
        template.as_ptr(),
        parent,
        Some(procedure),
        (&browser as *const Browser<'_>) as isize,
    );
    if result == -1 || browser.dialog.failed.get() {
        Err("No se pudo mostrar la cuadrícula SQL".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jc_platform::sql::{Engine, Tls};
    use std::time::{Duration, Instant};

    #[link(name = "user32")]
    extern "system" {
        fn CreateDialogIndirectParamW(
            instance: Handle,
            template: *const u32,
            parent: Hwnd,
            procedure: Option<unsafe extern "system" fn(Hwnd, u32, usize, isize) -> isize>,
            parameter: isize,
        ) -> Hwnd;
    }

    struct Window(Hwnd);

    impl Drop for Window {
        fn drop(&mut self) {
            unsafe { DestroyWindow(self.0) };
        }
    }

    #[test]
    fn sql_browser_native_grid_and_connection_error() {
        unsafe {
            assert!(records::initialize_list_class());
            let instance = GetModuleHandleW(null_mut());
            let parent = Window(CreateWindowExW(
                0,
                wide("STATIC").as_ptr(),
                wide("CIDE").as_ptr(),
                0x8000_0000,
                0,
                0,
                1040,
                630,
                null_mut(),
                null_mut(),
                instance,
                null_mut(),
            ));
            assert!(!parent.0.is_null());
            let browser = Browser {
                dialog: create(parent.0, "Registros SQL", "", "Cerrar").unwrap(),
                options: ConnectOptions {
                    engine: Engine::PostgreSql,
                    server: String::new(),
                    port: 5432,
                    database: "inventario".into(),
                    user: "lector".into(),
                    tls: Tls::VerifyServer,
                    credential: None,
                    schemas: vec!["public".into()],
                },
                bridge: Arc::default(),
                state: RefCell::default(),
            };
            let template = browser.template();
            let modal = Window(CreateDialogIndirectParamW(
                instance,
                template.as_ptr(),
                parent.0,
                Some(procedure),
                (&browser as *const Browser<'_>) as isize,
            ));
            assert!(!modal.0.is_null());
            assert!(!browser.dialog.failed.get());
            let deadline = Instant::now() + Duration::from_secs(5);
            while browser.state.borrow().pending.is_some() {
                browser.poll(modal.0);
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(5));
            }
            let mut units = [0; 256];
            let len = GetWindowTextW(
                child(modal.0, STATUS),
                units.as_mut_ptr(),
                units.len() as i32,
            );
            assert!(String::from_utf16_lossy(&units[..len as usize])
                .contains("Servidor o puerto SQL inválido"));
            assert_eq!(IsWindowEnabled(child(modal.0, OPEN)), 0);
            assert_ne!(IsWindowEnabled(child(modal.0, REFRESH)), 0);
            assert_ne!(IsWindowEnabled(child(modal.0, CLOSE)), 0);
            let mut state = browser.state.borrow_mut();
            let page = Page {
                columns: vec!["Producto".into(), "Comentario".into()],
                rows: vec![
                    vec![Some("Balón".into()), None],
                    vec![Some("Guante".into()), Some(String::new())],
                ],
                offset: PAGE_SIZE,
                has_next: true,
            };
            let chosen = Filter {
                column: 0,
                value: "Balón".into(),
            };
            assert!(browser.install_page_controls(modal.0, &mut state, &page, Some(&chosen)));
            let item_count = SendMessageW(state.grid, 0x1004, 0, 0);
            assert_eq!(item_count, 2);
            assert_eq!(records::cell_text(state.grid, 0, 0), "Balón");
            assert_eq!(records::cell_text(state.grid, 0, 1), "⟨NULL⟩");
            assert_eq!(records::cell_text(state.grid, 1, 1), "");
            assert_eq!(selected_index(modal.0, FIELD), Some(1));
            let len = GetWindowTextW(
                child(modal.0, VALUE),
                units.as_mut_ptr(),
                units.len() as i32,
            );
            assert_eq!(String::from_utf16_lossy(&units[..len as usize]), "Balón");
        }
    }

    #[test]
    fn sql_browser_formats_primary_keys_and_foreign_key_targets() {
        use jc_platform::sql::{ForeignKeyPart, IndexPart};
        let details = RelationDetails {
            columns: Vec::new(),
            indexes: vec![IndexPart {
                index: "producto_pk".into(),
                definition: "id".into(),
                primary: true,
                unique: true,
            }],
            foreign_keys: vec![ForeignKeyPart {
                constraint: "producto_categoria_fk".into(),
                column: "categoria_id".into(),
                referenced_schema: "ventas".into(),
                referenced_table: "categoría".into(),
                referenced_column: "id".into(),
            }],
        };
        let text = metadata("ventas.producto", &details);
        assert!(text.contains("producto_pk · Clave primaria · id"));
        assert!(text.contains("producto_categoria_fk · categoria_id → ventas.categoría.id"));
    }
}
