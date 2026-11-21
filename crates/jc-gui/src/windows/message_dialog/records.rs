//! Native record pages and explicitly reviewed ABL updates.
use super::*;
use crate::windows::notify::Bridge;
use jc_app::Wakeup;
use jc_progress::editor::{
    BrowseController, BrowseFilter, BrowsePage, BrowseRequest, BrowseTable, RecordConflict,
    RecordController, RecordEvent, RecordHistory, RecordPreview, BROWSE_MAX_FILTER_BYTES,
    BROWSE_PAGE_SIZE, RECORD_EDIT_MAX_VALUE_BYTES,
};
use std::{cell::RefCell, path::Path, ptr::null_mut, sync::Arc};

const CLOSE: u16 = 2;
const FILTER_LABEL: u16 = 100;
const FIELD: u16 = 101;
const VALUE: u16 = 102;
const APPLY: u16 = 103;
const SORT: u16 = 104;
const GRID: u16 = 105;
const STATUS: u16 = 106;
const PREVIOUS: u16 = 107;
const NEXT: u16 = 108;
const EDIT: u16 = 109;
const RESTORE: u16 = 110;
const ROW_ACTIONS: u16 = 111;
const BUTTONS: [(u16, &str); 8] = [
    (APPLY, "Aplicar filtro"),
    (SORT, "Invertir orden"),
    (PREVIOUS, "Anterior"),
    (NEXT, "Siguiente"),
    (EDIT, "Editar…"),
    (RESTORE, "Historial…"),
    (ROW_ACTIONS, "Fila…"),
    (CLOSE, "Cerrar"),
];

const WM_INITDIALOG: u32 = 0x110;
const WM_DRAWITEM: u32 = 0x2b;
const WM_NOTIFY: u32 = 0x4e;
const WM_SETFONT: u32 = 0x30;
const WM_CTLCOLORLISTBOX: u32 = 0x134;
const WM_CTLCOLOREDIT: u32 = 0x133;
const WM_CTLCOLORDLG: u32 = 0x136;
const WM_CTLCOLORSTATIC: u32 = 0x138;
const DM_GETDEFID: u32 = 0x400;
const DM_SETDEFID: u32 = 0x401;
const CB_ADDSTRING: u32 = 0x143;
const CB_GETCURSEL: u32 = 0x147;
const CB_SETCURSEL: u32 = 0x14e;
const EM_SETLIMITTEXT: u32 = 0xc5;

const LVM_SETBKCOLOR: u32 = 0x1001;
const LVM_DELETEALLITEMS: u32 = 0x1009;
const LVM_GETNEXTITEM: u32 = 0x100c;
const LVM_GETHEADER: u32 = 0x101f;
const LVM_ENSUREVISIBLE: u32 = 0x1013;
const LVM_SETTEXTCOLOR: u32 = 0x1024;
const LVM_SETTEXTBKCOLOR: u32 = 0x1026;
const LVM_SETEXTENDEDLISTVIEWSTYLE: u32 = 0x1036;
const LVM_INSERTITEMW: u32 = 0x104d;
const LVM_SETITEMSTATE: u32 = 0x102b;
const LVM_INSERTCOLUMNW: u32 = 0x1061;
const LVM_SETITEMTEXTW: u32 = 0x1074;
const HDM_GETITEMW: u32 = 0x120b;
const HDM_SETITEMW: u32 = 0x120c;
const LVN_ITEMCHANGED: i32 = -101;

#[repr(C)]
struct NotifyHeader {
    window: Hwnd,
    id: usize,
    code: i32,
}

#[link(name = "user32")]
extern "system" {
    fn EnableWindow(hwnd: Hwnd, enabled: i32) -> i32;
    fn IsWindowEnabled(hwnd: Hwnd) -> i32;
    fn SetWindowTextW(hwnd: Hwnd, text: *const u16) -> i32;
}

#[repr(C)]
struct CommonControls {
    size: u32,
    classes: u32,
}

type Subclass = unsafe extern "system" fn(Hwnd, u32, usize, isize, usize, usize) -> isize;

#[link(name = "comctl32")]
extern "system" {
    fn InitCommonControlsEx(controls: *const CommonControls) -> i32;
    fn SetWindowSubclass(hwnd: Hwnd, procedure: Subclass, id: usize, reference: usize) -> i32;
    fn RemoveWindowSubclass(hwnd: Hwnd, procedure: Subclass, id: usize) -> i32;
    fn DefSubclassProc(hwnd: Hwnd, message: u32, wp: usize, lp: isize) -> isize;
}

#[repr(C)]
struct Column {
    mask: u32,
    format: i32,
    width: i32,
    text: *mut u16,
    text_max: i32,
    subitem: i32,
    image: i32,
    order: i32,
    min_width: i32,
    default_width: i32,
    ideal_width: i32,
}

#[repr(C)]
struct Item {
    mask: u32,
    index: i32,
    subitem: i32,
    state: u32,
    state_mask: u32,
    text: *mut u16,
    text_max: i32,
    image: i32,
    parameter: isize,
    indent: i32,
    group_id: i32,
    column_count: u32,
    columns: *mut u32,
    column_formats: *mut i32,
    group: i32,
}

#[repr(C)]
struct HeaderItem {
    mask: u32,
    width: i32,
    text: *mut u16,
    bitmap: Handle,
    text_max: i32,
    format: i32,
    parameter: isize,
    image: i32,
    order: i32,
    kind: u32,
    filter: *mut std::ffi::c_void,
    state: u32,
}

#[derive(Clone, Default)]
struct Query {
    filter: Option<BrowseFilter>,
    descending: bool,
}

#[derive(Default)]
struct PageState {
    worker: BrowseController,
    records: RecordController,
    accepted: Query,
    result: Option<BrowsePage>,
    offset: usize,
    has_next: bool,
    pending: Option<Query>,
    reviewing: bool,
    applying: bool,
    record_key: Option<String>,
    needs_refresh: Option<&'static str>,
}

struct Browser<'a> {
    dialog: Dialog<'a>,
    root: &'a Path,
    table: &'a BrowseTable,
    page: RefCell<PageState>,
    bridge: Arc<Bridge>,
    wakeup: Wakeup,
    #[cfg(test)]
    review_answer: Cell<Option<bool>>,
}

impl Browser<'_> {
    fn buttons(&self) -> impl Iterator<Item = (u16, &'static str)> {
        let editable = self.editable();
        BUTTONS
            .into_iter()
            .filter(move |(id, _)| editable || !matches!(*id, EDIT | RESTORE | ROW_ACTIONS))
    }

    fn template(&self) -> Vec<u32> {
        let mut words = template_header(self.dialog.title, (4 + self.buttons().count()) as u16);
        let bounds = [0, 0, 1, 1];
        styled_control(
            &mut words,
            FILTER_LABEL,
            bounds,
            0x82,
            "Filtro exacto",
            0x80,
        );
        // Dropdown list with the native keyboard and scrolling behavior.
        styled_control(&mut words, FIELD, bounds, 0x85, "", 0x21_0203);
        styled_control(&mut words, VALUE, bounds, 0x81, "", 0x81_0080);
        styled_control(&mut words, STATUS, bounds, 0x82, "", 0x80);
        for (id, label) in self.buttons() {
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
            || client.bottom < dpi.pixels(234)
        {
            return false;
        }
        let width = client.right;
        let height = client.bottom;
        let p = |units| dpi.pixels(units);
        let margin = p(16);
        let place = |id, x, y, w, h| {
            let item = child(hwnd, id);
            SendMessageW(item, WM_SETFONT, self.dialog.font as usize, 0);
            SetWindowPos(item, null_mut(), x, y, w, h, 0x14) != 0
        };
        for (id, x, y, w, h) in [
            (FILTER_LABEL, margin, p(49), p(96), p(24)),
            (FIELD, p(112), p(44), p(146), p(180)),
            (VALUE, p(266), p(44), width - p(518), p(27)),
            (APPLY, width - p(244), p(44), p(108), p(28)),
            (SORT, width - p(128), p(44), p(112), p(28)),
            (STATUS, margin, height - p(89), width - 2 * margin, p(39)),
            (PREVIOUS, margin, height - p(46), p(92), p(30)),
            (NEXT, p(116), height - p(46), p(92), p(30)),
            (CLOSE, width - p(108), height - p(46), p(92), p(30)),
        ] {
            if !place(id, x, y, w, h) {
                return false;
            }
        }
        if self.editable()
            && (!place(EDIT, p(220), height - p(46), p(78), p(30))
                || !place(RESTORE, p(306), height - p(46), p(108), p(30))
                || !place(ROW_ACTIONS, p(422), height - p(46), p(78), p(30)))
        {
            return false;
        }
        let combo = child(hwnd, FIELD);
        for label in
            std::iter::once("Sin filtro").chain(self.table.columns.iter().map(String::as_str))
        {
            if SendMessageW(combo, CB_ADDSTRING, 0, wide(label).as_ptr() as isize) < 0 {
                return false;
            }
        }
        SendMessageW(combo, CB_SETCURSEL, 0, 0);
        SendMessageW(
            child(hwnd, VALUE),
            EM_SETLIMITTEXT,
            BROWSE_MAX_FILTER_BYTES,
            0,
        );
        EnableWindow(child(hwnd, VALUE), 0);
        let grid = create_grid(
            hwnd,
            Rect {
                left: margin,
                top: p(82),
                right: width - margin,
                bottom: height - p(98),
            },
            self.dialog.font,
            self.dialog.theme,
            p(154),
            &self.table.columns,
        );
        let Some(grid) = grid else {
            return false;
        };
        // Place the report between the filter and the page controls in tab order.
        if SetWindowPos(grid, child(hwnd, SORT), 0, 0, 0, 0, 0x13) == 0 {
            return false;
        }
        self.sync_controls(hwnd);
        SetFocus(grid);
        true
    }

    unsafe fn submit(&self, hwnd: Hwnd, query: Query, offset: usize) {
        let mut page = self.page.borrow_mut();
        if page.busy() {
            return;
        }
        let request = BrowseRequest {
            table: self.table.clone(),
            offset,
            filter: query.filter.clone(),
            descending: query.descending,
        };
        match page.worker.request(self.root, request, self.wakeup.clone()) {
            Ok(()) => {
                page.pending = Some(query);
                page.status(hwnd, "Consultando registros…");
            }
            Err(error) => page.status(hwnd, &error),
        }
        drop(page);
        self.sync_controls(hwnd);
    }

    unsafe fn poll(&self, hwnd: Hwnd) {
        self.bridge.acknowledge();
        let mut page = self.page.borrow_mut();
        // A nested edit form may pump a wakeup before its accepted row has
        // been recorded. The caller polls again after leaving that form.
        if page.reviewing {
            return;
        }
        if let Some(result) = page.worker.poll(self.root) {
            if let Some(query) = page.pending.take() {
                match result {
                    Ok(result) => {
                        if result.columns != self.table.columns
                            || !install_page(child(hwnd, GRID), &result)
                        {
                            self.dialog.failed.set(true);
                            drop(page);
                            EndDialog(hwnd, 0);
                            return;
                        }
                        if let (Some(key_column), Some(key)) =
                            (self.table.edit_key_column(), page.record_key.take())
                        {
                            if let Some(index) = result
                                .rows
                                .iter()
                                .position(|row| row[key_column].as_deref() == Some(&key))
                            {
                                select_index(child(hwnd, GRID), index);
                            }
                        }
                        page.status(
                            hwnd,
                            &page_label(&result, query.descending, self.editable()),
                        );
                        page.needs_refresh = None;
                        page.accepted = query;
                        page.offset = result.offset;
                        page.has_next = result.has_next;
                        page.result = Some(result);
                    }
                    Err(error) => page.status(hwnd, &error),
                }
            }
        }
        let event = page.records.poll(self.root);
        drop(page);
        if let Some(event) = event {
            self.record_event(hwnd, event);
        }
        self.sync_controls(hwnd);
    }

    unsafe fn command(&self, hwnd: Hwnd, id: u16) {
        let (mut query, offset, has_next) = {
            let page = self.page.borrow();
            if page.busy() {
                return;
            }
            (page.accepted.clone(), page.offset, page.has_next)
        };
        let offset = match id {
            APPLY => {
                match read_filter(hwnd, self.table.columns.len()) {
                    Ok(filter) => query.filter = filter,
                    Err(error) => {
                        set_status(hwnd, &error);
                        return;
                    }
                }
                0
            }
            SORT => {
                query.descending = !query.descending;
                0
            }
            PREVIOUS if offset > 0 => offset.saturating_sub(BROWSE_PAGE_SIZE),
            NEXT if has_next => offset.saturating_add(BROWSE_PAGE_SIZE),
            _ => return,
        };
        self.submit(hwnd, query, offset);
    }

    fn editable(&self) -> bool {
        self.table.edit_key_column().is_some()
    }

    unsafe fn sync_controls(&self, hwnd: Hwnd) {
        // Installing a page can synchronously send selection notifications.
        let Ok(page) = self.page.try_borrow() else {
            return;
        };
        let ready = !page.busy();
        for (id, enabled) in [
            (PREVIOUS, ready && page.offset > 0),
            (NEXT, ready && page.has_next),
            (SORT, ready),
            (APPLY, ready),
            (CLOSE, !page.applying),
        ] {
            EnableWindow(child(hwnd, id), i32::from(enabled));
        }
        if let Some(key) = self.table.edit_key_column() {
            let enabled = ready && page.selected_record(hwnd, key).is_some();
            EnableWindow(child(hwnd, EDIT), i32::from(enabled));
            EnableWindow(child(hwnd, RESTORE), i32::from(enabled));
            let row_actions = ready
                && page.needs_refresh.is_none()
                && (0..self.table.columns.len()).any(|column| self.table.can_edit_column(column));
            EnableWindow(child(hwnd, ROW_ACTIONS), i32::from(row_actions));
        }
    }

    unsafe fn row_command(&self, hwnd: Hwnd) {
        let Some(key) = self.table.edit_key_column() else {
            return;
        };
        let row = {
            let page = self.page.borrow();
            if !self.editable() || page.busy() || page.needs_refresh.is_some() {
                return;
            }
            page.selected_record(hwnd, key).map(<[_]>::to_vec)
        };
        self.page.borrow_mut().reviewing = true;
        let result = self.choose_row_action(hwnd, key, row.as_deref());
        self.page.borrow_mut().reviewing = false;
        match result {
            Ok(true) => {
                set_status(hwnd, "Preparando la vista previa…");
                self.poll(hwnd);
            }
            Ok(false) => {}
            Err(error) => set_status(hwnd, &error),
        }
        self.sync_controls(hwnd);
    }

    unsafe fn choose_row_action(
        &self,
        hwnd: Hwnd,
        key: usize,
        row: Option<&[Option<String>]>,
    ) -> Result<bool, String> {
        let mut options = vec![
            "Alta de registro".into(),
            "Restaurar alta o borrado por clave".into(),
        ];
        if row.is_some() {
            options.push("Borrar registro seleccionado".into());
        }
        let table = format!("{}.{}", self.table.database, self.table.name);
        let chooser = self
            .dialog
            .child(hwnd, "Operaciones de fila", &table, "Continuar")?;
        match form::choose_dialog(chooser, &options)? {
            Some(0) => self.row_key_form(hwnd, key, true),
            Some(1) => self.row_key_form(hwnd, key, false),
            Some(2) => {
                let Some(row) = row else { return Ok(false) };
                let mut page = self.page.borrow_mut();
                page.records
                    .request_delete(self.root, self.table, row, self.wakeup.clone())?;
                page.record_key = row[key].clone();
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    unsafe fn row_key_form(&self, hwnd: Hwnd, key: usize, create: bool) -> Result<bool, String> {
        let mut columns = vec![key];
        let (title, details) = if create {
            columns.extend(
                (0..self.table.columns.len()).filter(|&column| self.table.can_edit_column(column)),
            );
            (
                "Alta de registro",
                "Escribe la clave. Los campos vacíos usan el valor inicial de la tabla.",
            )
        } else {
            (
                "Restaurar alta o borrado",
                "Escribe la clave del registro en el respaldo.",
            )
        };
        let fields: Vec<_> = columns
            .iter()
            .map(|&column| form::Field {
                label: &self.table.columns[column],
                value: "",
                limit: if column == key {
                    11
                } else {
                    RECORD_EDIT_MAX_VALUE_BYTES
                },
            })
            .collect();
        let dialog = self.dialog.child(hwnd, title, details, "Vista previa")?;
        form::edit_dialog(dialog, &fields, &mut |values| {
            let key: i32 = values[0]
                .trim()
                .parse()
                .map_err(|_| "Escribe una clave entera ABL válida")?;
            let mut page = self.page.borrow_mut();
            if create {
                let assignments = columns[1..]
                    .iter()
                    .copied()
                    .zip(values[1..].iter())
                    .filter(|(_, value)| !value.is_empty())
                    .map(|(column, value)| (column, value.clone()))
                    .collect();
                page.records.request_create(
                    self.root,
                    self.table,
                    key,
                    assignments,
                    self.wakeup.clone(),
                )?;
            } else {
                page.records.request_structure_restore(
                    self.root,
                    self.table,
                    key,
                    self.wakeup.clone(),
                )?;
            }
            page.record_key = Some(key.to_string());
            Ok(())
        })
    }

    unsafe fn record_command(&self, hwnd: Hwnd, id: u16) {
        let Some(key) = self.table.edit_key_column() else {
            return;
        };
        let row = {
            let page = self.page.borrow();
            if page.busy() {
                return;
            }
            let Some(row) = page.selected_record(hwnd, key) else {
                return;
            };
            row.to_vec()
        };
        self.page.borrow_mut().reviewing = true;
        let result = if id == EDIT {
            self.edit(hwnd, &row, key)
        } else {
            self.page
                .borrow_mut()
                .records
                .request_history(self.root, self.table, &row, self.wakeup.clone())
                .map(|()| true)
        };
        self.page.borrow_mut().reviewing = false;
        match result {
            Ok(true) => {
                self.page.borrow_mut().record_key = row[key].clone();
                set_status(
                    hwnd,
                    if id == EDIT {
                        "Preparando la vista previa…"
                    } else {
                        "Consultando respaldos…"
                    },
                );
                self.poll(hwnd);
            }
            Ok(false) => {}
            Err(error) => set_status(hwnd, &error),
        }
        self.sync_controls(hwnd);
    }

    unsafe fn edit(&self, hwnd: Hwnd, row: &[Option<String>], key: usize) -> Result<bool, String> {
        let target = self.record_label(key, row[key].as_deref());
        let columns: Vec<_> = (0..self.table.columns.len())
            .filter(|&index| self.table.can_edit_column(index))
            .collect();
        let names: Vec<_> = columns
            .iter()
            .map(|&index| self.table.columns[index].clone())
            .collect();
        let chooser = self
            .dialog
            .child(hwnd, "Editar columna", &target, "Continuar")?;
        let Some(selected) = form::choose_dialog(chooser, &names)? else {
            return Ok(false);
        };
        let column = columns[selected];
        let current = row[column]
            .as_deref()
            .filter(|value| {
                value.len() <= RECORD_EDIT_MAX_VALUE_BYTES && !value.chars().any(char::is_control)
            })
            .unwrap_or("");
        let fields = [form::Field {
            label: &self.table.columns[column],
            value: current,
            limit: RECORD_EDIT_MAX_VALUE_BYTES,
        }];
        let dialog = self
            .dialog
            .child(hwnd, "Editar registro", &target, "Vista previa")?;
        form::edit_dialog(dialog, &fields, &mut |values| {
            self.page.borrow_mut().records.request_edit(
                self.root,
                self.table,
                row,
                column,
                values[0].clone(),
                self.wakeup.clone(),
            )
        })
    }

    unsafe fn record_event(&self, hwnd: Hwnd, event: Result<RecordEvent, String>) {
        match event {
            Ok(RecordEvent::Preview(preview)) => self.review(hwnd, preview),
            Ok(RecordEvent::History(history)) => self.history(hwnd, history),
            Ok(RecordEvent::Conflict(conflict)) => self.restore_conflict(hwnd, conflict),
            Ok(RecordEvent::Applied) => {
                let (query, offset) = {
                    let mut page = self.page.borrow_mut();
                    page.applying = false;
                    (page.accepted.clone(), page.offset)
                };
                self.submit(hwnd, query, offset);
            }
            Err(error) => {
                let mut page = self.page.borrow_mut();
                // A failed request may have reached the database. Require a fresh
                // page before offering another edit to the same displayed row.
                if page.applying {
                    page.applying = false;
                    page.needs_refresh = Some("Vuelve a consultar antes de editar");
                } else {
                    page.record_key = None;
                }
                set_status(hwnd, &error);
            }
        }
    }

    unsafe fn history(&self, hwnd: Hwnd, history: RecordHistory) {
        let Some(target) = self.selected_record_label() else {
            return;
        };
        if history.entries().is_empty() {
            self.page.borrow_mut().record_key = None;
            set_status(hwnd, "El registro aún no tiene respaldos");
            return;
        }
        let names: Vec<_> = history
            .entries()
            .iter()
            .map(|entry| entry.label().to_owned())
            .collect();
        self.page.borrow_mut().reviewing = true;
        let choice = self
            .dialog
            .child(hwnd, "Historial de respaldos", &target, "Revisar")
            .and_then(|chooser| form::choose_dialog(chooser, &names));
        let mut page = self.page.borrow_mut();
        page.reviewing = false;
        let result = choice.and_then(|index| {
            if let Some(index) = index {
                page.records.request_history_restore(
                    self.root,
                    &history,
                    index,
                    self.wakeup.clone(),
                )?;
            }
            Ok(index)
        });
        match result {
            Ok(Some(_)) => set_status(hwnd, "Verificando el respaldo seleccionado…"),
            Ok(None) => {
                page.record_key = None;
                set_status(hwnd, "Historial cerrado");
            }
            Err(error) => {
                page.record_key = None;
                set_status(hwnd, &error);
            }
        }
    }

    unsafe fn restore_conflict(&self, hwnd: Hwnd, conflict: RecordConflict) {
        let Some(target) = self.selected_record_label() else {
            return;
        };
        let message = format!(
            "{target}\r\nColumna: {}\r\n\r\nLa imagen completa del registro difiere del respaldo.\r\n\r\nValor actual\r\n{}\r\n\r\nValor registrado después del cambio\r\n{}",
            conflict.column,
            display_value(conflict.actual.as_deref()),
            display_value(conflict.expected.as_deref())
        );
        self.page.borrow_mut().reviewing = true;
        let answer = self
            .dialog
            .child(hwnd, "Conflicto de restauración", &message, "Cerrar")
            .and_then(|mut dialog| {
                dialog.scrollable = true;
                super::show(hwnd, &mut dialog)
            });
        let mut page = self.page.borrow_mut();
        page.reviewing = false;
        page.record_key = None;
        set_status(
            hwnd,
            &answer
                .err()
                .unwrap_or_else(|| "Conflicto de restauración".into()),
        );
    }

    fn selected_record_label(&self) -> Option<String> {
        let page = self.page.borrow();
        self.table
            .edit_key_column()
            .zip(page.record_key.as_deref())
            .map(|(key, value)| self.record_label(key, Some(value)))
    }

    unsafe fn review(&self, hwnd: Hwnd, preview: RecordPreview) {
        let Some(target) = self.selected_record_label() else {
            set_status(
                hwnd,
                "No se pudo identificar el registro de la vista previa",
            );
            return;
        };
        let (title, action, completed) = if let Some(title) = preview.structural_title {
            (title, "Confirmar", "Operación de fila confirmada")
        } else if preview.restoring {
            (
                "Vista previa de restauración",
                "Restaurar",
                "Respaldo restaurado",
            )
        } else {
            (
                "Vista previa de edición",
                "Confirmar edición",
                "Edición respaldada y guardada",
            )
        };
        let mut message = if preview.structural_title.is_some() {
            structural_preview_message(&target, preview.before.as_deref(), preview.after.as_deref())
        } else {
            preview_message(
                &target,
                &preview.column,
                preview.before.as_deref(),
                preview.after.as_deref(),
            )
        };
        if preview.with_triggers {
            message.push_str("\r\n\r\nEl cambio se aplicará en la base activa con los disparadores del proyecto. El JSON respalda esta fila.");
        } else if preview.active_database {
            message.push_str("\r\n\r\nEl cambio se aplicará en la base activa. CIDE verifica el respaldo y comprueba el registro con bloqueo exclusivo antes de escribir.");
        }
        self.page.borrow_mut().reviewing = true;
        let answer = self
            .dialog
            .child(hwnd, title, &message, action)
            .and_then(|mut dialog| {
                dialog.scrollable = true;
                #[cfg(test)]
                {
                    dialog.answer = self.review_answer.take();
                }
                super::show(hwnd, &mut dialog)
            });
        let mut page = self.page.borrow_mut();
        page.reviewing = false;
        match answer {
            Ok(true) => match page.records.apply(self.root, preview, self.wakeup.clone()) {
                Ok(()) => {
                    page.applying = true;
                    page.needs_refresh = Some(completed);
                    set_status(hwnd, "Aplicando el cambio confirmado…");
                }
                Err(error) => {
                    page.record_key = None;
                    set_status(hwnd, &error);
                }
            },
            Ok(false) => {
                page.record_key = None;
                set_status(hwnd, "Cambio cancelado");
            }
            Err(error) => {
                page.record_key = None;
                set_status(hwnd, &error);
            }
        }
    }

    fn record_label(&self, key: usize, value: Option<&str>) -> String {
        format!(
            "{}.{} · {} = {}",
            self.table.database,
            self.table.name,
            self.table.columns[key],
            display_value(value)
        )
    }
}

impl PageState {
    fn busy(&self) -> bool {
        self.worker.busy() || self.records.busy() || self.reviewing || self.applying
    }

    unsafe fn selected_record(&self, hwnd: Hwnd, key: usize) -> Option<&[Option<String>]> {
        if self.needs_refresh.is_some() {
            return None;
        }
        let index = selected_index(child(hwnd, GRID))?;
        let row = self.result.as_ref()?.rows.get(index)?;
        row.get(key)?.as_deref()?;
        Some(row)
    }

    unsafe fn status(&self, hwnd: Hwnd, message: &str) {
        match self.needs_refresh {
            Some(prefix) => set_status(hwnd, &format!("{prefix} · {message}")),
            None => set_status(hwnd, message),
        }
    }
}

unsafe fn child(hwnd: Hwnd, id: u16) -> Hwnd {
    GetDlgItem(hwnd, i32::from(id))
}

unsafe fn set_status(hwnd: Hwnd, message: &str) {
    SetWindowTextW(child(hwnd, STATUS), wide(message).as_ptr());
}

unsafe fn read_filter(hwnd: Hwnd, columns: usize) -> Result<Option<BrowseFilter>, String> {
    let selected = SendMessageW(child(hwnd, FIELD), CB_GETCURSEL, 0, 0);
    if selected == 0 {
        return Ok(None);
    }
    if !(1..=columns as isize).contains(&selected) {
        return Err("Selecciona una columna para aplicar el filtro".into());
    }
    let mut text = [0; BROWSE_MAX_FILTER_BYTES + 1];
    let len = GetWindowTextW(child(hwnd, VALUE), text.as_mut_ptr(), text.len() as i32) as usize;
    let value = String::from_utf16(&text[..len])
        .map_err(|_| "El valor del filtro contiene texto inválido")?;
    Ok(Some(BrowseFilter {
        column: selected as usize - 1,
        value,
    }))
}

fn page_label(page: &BrowsePage, descending: bool, editable: bool) -> String {
    let records = if page.rows.is_empty() {
        "Sin registros para esta consulta".into()
    } else {
        format!(
            "Página {} · Registros {}–{}",
            page.offset / BROWSE_PAGE_SIZE + 1,
            page.offset + 1,
            page.offset + page.rows.len(),
        )
    };
    let order = if descending {
        "descendente"
    } else {
        "ascendente"
    };
    let mode = if editable { "" } else { "Solo lectura · " };
    format!("{mode}{records} · Clave primaria en orden {order}")
}

fn preview_message(
    target: &str,
    column: &str,
    before: Option<&str>,
    after: Option<&str>,
) -> String {
    format!(
        "{target}\r\nColumna: {column}\r\n\r\nValor actual\r\n{}\r\n\r\nValor propuesto\r\n{}",
        display_value(before),
        display_value(after)
    )
}

fn display_value(value: Option<&str>) -> String {
    // Quotes distinguish empty text and literal "(desconocido)" from ABL ?.
    value.map_or_else(|| "(desconocido)".into(), |value| format!("{value:?}"))
}

fn structural_preview_message(target: &str, before: Option<&str>, after: Option<&str>) -> String {
    format!(
        "{target}\r\n\r\nEstado actual\r\n{}\r\n\r\nEstado propuesto\r\n{}",
        before.unwrap_or("Registro ausente").replace('\n', "\r\n"),
        after.unwrap_or("Registro ausente").replace('\n', "\r\n")
    )
}

unsafe fn selected_index(grid: Hwnd) -> Option<usize> {
    // Start before the first row and return the selected row (LVNI_SELECTED).
    SendMessageW(grid, LVM_GETNEXTITEM, usize::MAX, 2)
        .try_into()
        .ok()
}

unsafe fn select_index(grid: Hwnd, index: usize) {
    let item = Item {
        state: 3, // LVIS_SELECTED | LVIS_FOCUSED
        state_mask: 3,
        ..std::mem::zeroed()
    };
    SendMessageW(
        grid,
        LVM_SETITEMSTATE,
        index,
        (&item as *const Item) as isize,
    );
    SendMessageW(grid, LVM_ENSUREVISIBLE, index, 0);
}

fn wide_cell(value: Option<&str>, null_value: &str) -> Vec<u16> {
    let value = value.unwrap_or(null_value);
    let mut text = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_control() {
            text.extend(ch.escape_default());
        } else {
            text.push(ch);
        }
    }
    wide(&text)
}

unsafe extern "system" fn grid_procedure(
    hwnd: Hwnd,
    message: u32,
    wp: usize,
    lp: isize,
    id: usize,
    theme: usize,
) -> isize {
    if message == WM_DRAWITEM {
        let theme = crate::windows::appearance::Theme::from_id(theme as u8).unwrap_or_default();
        let _colors = paint::UiColors::enter(theme);
        let item = &*(lp as *const DrawItem);
        if item.control_type == 100 {
            let mut title = [0u16; 256];
            let mut header: HeaderItem = std::mem::zeroed();
            header.mask = 0x02; // HDI_TEXT
            header.text = title.as_mut_ptr();
            header.text_max = title.len() as i32;
            if SendMessageW(
                item.window,
                HDM_GETITEMW,
                item.item_id as usize,
                (&mut header as *mut HeaderItem) as isize,
            ) != 0
            {
                let mut bounds = item.rect;
                let _ = paint::fill(item.dc, bounds, shell::RAISED);
                let _ = paint::fill(
                    item.dc,
                    Rect {
                        left: bounds.right - 1,
                        ..bounds
                    },
                    shell::BACKGROUND,
                );
                bounds.left += dpi::for_window(hwnd).pixels(6);
                bounds.right -= dpi::for_window(hwnd).pixels(4);
                let previous = SelectObject(item.dc, SendMessageW(hwnd, 0x31, 0, 0) as Handle);
                let mode = SetBkMode(item.dc, 1);
                SetTextColor(item.dc, paint::ui_ink(shell::TEXT));
                DrawTextW(item.dc, title.as_ptr(), -1, &mut bounds, 0x8824);
                SetBkMode(item.dc, mode);
                SelectObject(item.dc, previous);
                return 1;
            }
        }
    }
    if message == WM_NCDESTROY {
        RemoveWindowSubclass(hwnd, grid_procedure, id);
    }
    DefSubclassProc(hwnd, message, wp, lp)
}

pub(super) unsafe fn create_grid(
    parent: Hwnd,
    bounds: Rect,
    font: Handle,
    theme: crate::windows::appearance::Theme,
    column_width: i32,
    names: &[String],
) -> Option<Hwnd> {
    let grid = CreateWindowExW(
        0,
        wide("SysListView32").as_ptr(),
        wide("Registros").as_ptr(),
        // Child, visible, tab stop, report, single selection.
        0x5001_0005,
        bounds.left,
        bounds.top,
        bounds.right - bounds.left,
        bounds.bottom - bounds.top,
        parent,
        usize::from(GRID) as Handle,
        GetModuleHandleW(std::ptr::null()),
        null_mut(),
    );
    if grid.is_null() || SetWindowSubclass(grid, grid_procedure, 1, theme as usize) == 0 {
        return None;
    }
    SendMessageW(grid, WM_SETFONT, font as usize, 0);
    let background = theme.control_color(shell::BACKGROUND) as isize;
    SendMessageW(grid, LVM_SETBKCOLOR, 0, background);
    SendMessageW(grid, LVM_SETTEXTBKCOLOR, 0, background);
    SendMessageW(
        grid,
        LVM_SETTEXTCOLOR,
        0,
        theme.control_color(shell::TEXT) as isize,
    );
    // Full-row selection keeps the value aligned with its record.
    SendMessageW(grid, LVM_SETEXTENDEDLISTVIEWSTYLE, 0, 0x20);
    let heading = SendMessageW(grid, LVM_GETHEADER, 0, 0) as Hwnd;
    for (index, name) in names.iter().enumerate() {
        let mut text = wide(name);
        let mut column: Column = std::mem::zeroed();
        column.mask = 0x02 | 0x04;
        column.width = column_width;
        column.text = text.as_mut_ptr();
        if SendMessageW(
            grid,
            LVM_INSERTCOLUMNW,
            index,
            (&column as *const Column) as isize,
        ) < 0
        {
            return None;
        }
        let mut header: HeaderItem = std::mem::zeroed();
        header.mask = 0x04; // HDI_FORMAT
        if SendMessageW(
            heading,
            HDM_GETITEMW,
            index,
            (&mut header as *mut HeaderItem) as isize,
        ) == 0
        {
            return None;
        }
        header.format |= 0x8000; // HDF_OWNERDRAW
        if SendMessageW(
            heading,
            HDM_SETITEMW,
            index,
            (&header as *const HeaderItem) as isize,
        ) == 0
        {
            return None;
        }
    }
    Some(grid)
}

unsafe fn install_page(grid: Hwnd, page: &BrowsePage) -> bool {
    install_rows(grid, &page.columns, &page.rows, "(desconocido)")
}

pub(super) unsafe fn install_rows(
    grid: Hwnd,
    columns: &[String],
    rows: &[Vec<Option<String>>],
    null_value: &str,
) -> bool {
    if rows.iter().any(|row| row.len() != columns.len()) {
        return false;
    }
    SendMessageW(grid, LVM_DELETEALLITEMS, 0, 0);
    for (index, row) in rows.iter().enumerate() {
        let mut item: Item = std::mem::zeroed();
        item.mask = 1; // LVIF_TEXT
        item.index = index as i32;
        for (column, value) in row.iter().enumerate() {
            let mut text = wide_cell(value.as_deref(), null_value);
            item.subitem = column as i32;
            item.text = text.as_mut_ptr();
            let parameter = (&mut item as *mut Item) as isize;
            let inserted = if column == 0 {
                SendMessageW(grid, LVM_INSERTITEMW, 0, parameter) >= 0
            } else {
                SendMessageW(grid, LVM_SETITEMTEXTW, index, parameter) != 0
            };
            if !inserted {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
pub(super) unsafe fn cell_text(grid: Hwnd, row: usize, column: i32) -> String {
    const LVM_GETITEMTEXTW: u32 = 0x1073;
    let mut units = [0u16; 1024];
    let mut item: Item = std::mem::zeroed();
    item.subitem = column;
    item.text = units.as_mut_ptr();
    item.text_max = units.len() as i32;
    let len = SendMessageW(
        grid,
        LVM_GETITEMTEXTW,
        row,
        (&mut item as *mut Item) as isize,
    );
    String::from_utf16_lossy(&units[..len as usize])
}

unsafe extern "system" fn procedure(hwnd: Hwnd, message: u32, wp: usize, lp: isize) -> isize {
    if message == WM_INITDIALOG {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, lp);
        let browser = &*(lp as *const Browser<'_>);
        shell::set_caption_theme(hwnd, browser.dialog.theme);
        if !browser.arrange(hwnd) {
            browser.dialog.failed.set(true);
            EndDialog(hwnd, 0);
            return 0;
        }
        browser.bridge.attach(hwnd);
        browser.submit(hwnd, Query::default(), 0);
        return 0;
    }
    let data = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
    if data == 0 {
        return 0;
    }
    let browser = &*(data as *const Browser<'_>);
    let _colors = paint::UiColors::enter(browser.dialog.theme);
    match message {
        WM_COMMAND => match (wp as u16, (wp >> 16) as u16) {
            (CLOSE, 0) => {
                let page = browser.page.borrow();
                if !page.applying && !page.reviewing {
                    drop(page);
                    EndDialog(hwnd, 0);
                }
                1
            }
            (FIELD, 1) => {
                let selected = SendMessageW(child(hwnd, FIELD), CB_GETCURSEL, 0, 0);
                EnableWindow(child(hwnd, VALUE), i32::from(selected > 0));
                1
            }
            (id @ (PREVIOUS | NEXT | APPLY | SORT), 0) => {
                browser.command(hwnd, id);
                1
            }
            (id @ (EDIT | RESTORE), 0) => {
                browser.record_command(hwnd, id);
                1
            }
            (ROW_ACTIONS, 0) => {
                browser.row_command(hwnd);
                1
            }
            _ => 0,
        },
        WM_NOTIFY if wp == usize::from(GRID) && lp != 0 => {
            let header = &*(lp as *const NotifyHeader);
            if header.code == LVN_ITEMCHANGED {
                browser.sync_controls(hwnd);
            }
            0
        }
        WM_CLOSE => {
            let page = browser.page.borrow();
            if !page.applying && !page.reviewing {
                drop(page);
                EndDialog(hwnd, 0);
            }
            1
        }
        WM_DESTROY => {
            browser.bridge.detach();
            let mut page = browser.page.borrow_mut();
            page.worker.cancel();
            page.records.cancel_preview();
            0
        }
        TASKS_MESSAGE => {
            browser.poll(hwnd);
            1
        }
        DM_GETDEFID => {
            let focus = GetFocus();
            let id = browser
                .buttons()
                .find(|(id, _)| child(hwnd, *id) == focus)
                .map_or(APPLY, |(id, _)| id);
            let result = if IsWindowEnabled(child(hwnd, id)) == 0 {
                0
            } else {
                (id as isize) | (0x534b << 16)
            };
            SetWindowLongPtrW(hwnd, 0, result);
            1
        }
        DM_SETDEFID => 1,
        WM_CTLCOLORDLG | WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => {
            SetTextColor(wp as Hdc, paint::ui_ink(shell::TEXT));
            SetBkColor(wp as Hdc, paint::ui_color(shell::RAISED));
            browser.dialog.brush as isize
        }
        WM_DRAWITEM => {
            let item = &*(lp as *const DrawItem);
            if item.control_type != 4 {
                return 0;
            }
            let Some((_, label)) = browser
                .buttons()
                .find(|(id, _)| u32::from(*id) == item.control_id)
            else {
                return 0;
            };
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

pub(super) unsafe fn initialize_list_class() -> bool {
    let controls = CommonControls {
        size: std::mem::size_of::<CommonControls>() as u32,
        classes: 1, // ICC_LISTVIEW_CLASSES
    };
    InitCommonControlsEx(&controls) != 0
}

unsafe fn show_browser(browser: &Browser<'_>) -> Result<(), String> {
    if !initialize_list_class() {
        return Err("No se pudo preparar la cuadrícula de registros".into());
    }
    let template = browser.template();
    let result = DialogBoxIndirectParamW(
        GetModuleHandleW(std::ptr::null()),
        template.as_ptr(),
        browser.dialog.parent,
        Some(procedure),
        (browser as *const Browser<'_>) as isize,
    );
    if result == -1 || browser.dialog.failed.get() {
        Err("No se pudo mostrar la cuadrícula de registros".into())
    } else {
        Ok(())
    }
}

pub(in crate::windows) unsafe fn show(
    parent: Hwnd,
    root: &Path,
    table: &BrowseTable,
) -> Result<(), String> {
    let title = format!("Ver registros · {}.{}", table.database, table.name);
    let dialog = create(parent, &title, "", "")?;
    let bridge = Arc::new(Bridge::default());
    let notify = bridge.clone();
    let browser = Browser {
        dialog,
        root,
        table,
        page: RefCell::default(),
        bridge,
        wakeup: Wakeup::with_notifier(move || notify.signal()),
        #[cfg(test)]
        review_answer: Cell::new(None),
    };
    show_browser(&browser)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jc_progress::editor::SchemaController;
    use std::{
        path::PathBuf,
        time::{Duration, Instant},
    };

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

    struct TestWindow(Hwnd);

    impl Drop for TestWindow {
        fn drop(&mut self) {
            unsafe { DestroyWindow(self.0) };
        }
    }

    unsafe fn native_text(hwnd: Hwnd) -> String {
        let mut units = [0u16; 1024];
        let len = GetWindowTextW(hwnd, units.as_mut_ptr(), units.len() as i32);
        String::from_utf16_lossy(&units[..len as usize])
    }

    unsafe fn wait_for_window(browser: &Browser<'_>, hwnd: Hwnd) -> String {
        let deadline = Instant::now() + Duration::from_secs(65);
        while browser.page.borrow().busy() {
            let mut message = std::mem::zeroed();
            if PeekMessageW(&mut message, hwnd, TASKS_MESSAGE, TASKS_MESSAGE, 1) != 0 {
                DispatchMessageW(&message);
            } else {
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(
                Instant::now() < deadline,
                "{}",
                native_text(child(hwnd, STATUS))
            );
        }
        native_text(child(hwnd, STATUS))
    }

    #[test]
    #[ignore = "requiere CIDE_TEST_PROGRESS_RECORD_EDIT_PROJECT con una copia descartable de Sports2000"]
    fn records_real_sports2000_native_edit_and_restore() {
        let root =
            PathBuf::from(std::env::var_os("CIDE_TEST_PROGRESS_RECORD_EDIT_PROJECT").unwrap())
                .canonicalize()
                .unwrap();
        let mut schema = SchemaController::default();
        let mut workbench = jc_app::Workbench::default();
        schema.request(Some(&root), Wakeup::default()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(35);
        let update = loop {
            if let Some(update) = schema.poll(Some(&root), &mut workbench) {
                break update;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        };
        assert!(
            !update.problems,
            "{}: {:?}",
            update.message,
            workbench.problems()
        );
        let table = schema
            .browse_tables()
            .into_iter()
            .find(|table| table.name.eq_ignore_ascii_case("Customer"))
            .unwrap();
        let column = table
            .columns
            .iter()
            .position(|name| name.eq_ignore_ascii_case("Name"))
            .unwrap();
        assert!(table.can_edit_column(column));
        unsafe {
            assert!(initialize_list_class());
            let instance = GetModuleHandleW(std::ptr::null());
            let owner = TestWindow(CreateWindowExW(
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
            assert!(!owner.0.is_null());
            let bridge = Arc::new(Bridge::default());
            let notify = bridge.clone();
            let browser = Browser {
                dialog: create(owner.0, "Ver registros", "", "").unwrap(),
                root: &root,
                table: &table,
                page: RefCell::default(),
                bridge,
                wakeup: Wakeup::with_notifier(move || notify.signal()),
                review_answer: Cell::new(None),
            };
            let template = browser.template();
            let window = TestWindow(CreateDialogIndirectParamW(
                instance,
                template.as_ptr(),
                owner.0,
                Some(procedure),
                (&browser as *const Browser<'_>) as isize,
            ));
            let hwnd = window.0;
            assert!(!hwnd.is_null());
            let status = wait_for_window(&browser, hwnd);
            assert!(status.starts_with("Página 1"), "{status}");
            let grid = child(hwnd, GRID);
            let name = || {
                let mut text = [0u16; 1024];
                let mut item: Item = std::mem::zeroed();
                item.subitem = column as i32;
                item.text = text.as_mut_ptr();
                item.text_max = text.len() as i32;
                let len = SendMessageW(grid, 0x1073, 0, (&mut item as *mut Item) as isize);
                String::from_utf16_lossy(&text[..len as usize])
            };
            let original = name();
            let proposed = "CIDE GUI Sports2000";
            assert!(!original.is_empty() && original != proposed);
            assert_eq!(IsWindowEnabled(child(hwnd, EDIT)), 0);
            select_index(grid, 0);
            assert_ne!(IsWindowEnabled(child(hwnd, EDIT)), 0);
            for (answer, expected_status, expected_value) in [
                (false, "Cambio cancelado", original.as_str()),
                (true, "Edición respaldada y guardada", proposed),
            ] {
                browser.review_answer.set(Some(answer));
                form::script_record_edit(&table.columns[column], proposed);
                SendMessageW(hwnd, WM_COMMAND, usize::from(EDIT), 0);
                let status = wait_for_window(&browser, hwnd);
                assert!(status.starts_with(expected_status), "{status}");
                assert_eq!(name(), expected_value);
                assert_eq!(selected_index(grid), Some(0));
            }
            browser.review_answer.set(Some(true));
            form::script_record_history(0);
            SendMessageW(hwnd, WM_COMMAND, usize::from(RESTORE), 0);
            let status = wait_for_window(&browser, hwnd);
            assert!(status.starts_with("Respaldo restaurado"), "{status}");
            assert_eq!(name(), original);

            let key = &table.columns[table.edit_key_column().unwrap()];
            let new_key = "-12345678";
            let inserted = "CIDE fila GUI";
            for (action, fields, expected) in [
                (
                    0,
                    vec![
                        (key.as_str(), new_key),
                        (table.columns[column].as_str(), inserted),
                    ],
                    inserted,
                ),
                (1, vec![(key.as_str(), new_key)], original.as_str()),
                (1, vec![(key.as_str(), new_key)], inserted),
                (2, Vec::new(), original.as_str()),
            ] {
                browser.review_answer.set(Some(true));
                form::script_row_action(action, &fields);
                SendMessageW(hwnd, WM_COMMAND, usize::from(ROW_ACTIONS), 0);
                let status = wait_for_window(&browser, hwnd);
                assert!(
                    status.starts_with("Operación de fila confirmada"),
                    "{status}"
                );
                assert_eq!(name(), expected);
            }
        }
    }

    #[test]
    fn records_grid_replaces_pages_in_native_cells() {
        unsafe {
            assert!(initialize_list_class());
            let instance = GetModuleHandleW(std::ptr::null());
            let owner = CreateWindowExW(
                0,
                wide("STATIC").as_ptr(),
                wide("CIDE").as_ptr(),
                0x8000_0000,
                0,
                0,
                720,
                400,
                null_mut(),
                null_mut(),
                instance,
                null_mut(),
            );
            assert!(!owner.is_null());
            let dialog = create(owner, "Ver registros", "", "").unwrap();
            let mut page = BrowsePage {
                columns: vec!["CustNum".into(), "Name".into()],
                rows: vec![vec![Some("21".into()), Some("Café\tSan José".into())]],
                offset: BROWSE_PAGE_SIZE,
                has_next: true,
            };
            let grid = create_grid(
                owner,
                Rect {
                    right: 660,
                    bottom: 350,
                    ..Rect::default()
                },
                dialog.font,
                dialog.theme,
                154,
                &page.columns,
            )
            .unwrap();
            let read_cell = |column| {
                let mut text = [0u16; 128];
                let mut item: Item = std::mem::zeroed();
                item.subitem = column;
                item.text = text.as_mut_ptr();
                item.text_max = text.len() as i32;
                const LVM_GETITEMTEXTW: u32 = 0x1073;
                let len =
                    SendMessageW(grid, LVM_GETITEMTEXTW, 0, (&mut item as *mut Item) as isize);
                String::from_utf16(&text[..len as usize]).unwrap()
            };
            assert!(install_page(grid, &page));
            assert_eq!(read_cell(0), "21");
            assert_eq!(read_cell(1), r"Café\tSan José");
            assert_eq!(selected_index(grid), None);
            select_index(grid, 0);
            assert_eq!(selected_index(grid), Some(0));
            assert_eq!(
                page_label(&page, true, false),
                "Solo lectura · Página 2 · Registros 21–21 · Clave primaria en orden descendente"
            );
            page.rows = vec![vec![Some("22".into()), None]];
            assert!(install_page(grid, &page));
            assert_eq!(selected_index(grid), None);
            assert_eq!(read_cell(0), "22");
            assert_eq!(read_cell(1), "(desconocido)");
            assert_ne!(DestroyWindow(owner), 0);
        }
    }

    #[test]
    fn records_preview_distinguishes_unknown_empty_and_literal_text() {
        assert_eq!(
            preview_message("sports.Customer · CustNum = \"21\"", "Name", None, Some("")),
            concat!(
                "sports.Customer · CustNum = \"21\"\r\nColumna: Name\r\n\r\n",
                "Valor actual\r\n(desconocido)\r\n\r\nValor propuesto\r\n\"\""
            )
        );
        assert_eq!(display_value(Some("(desconocido)")), "\"(desconocido)\"");
        assert_eq!(display_value(Some("Café\tSan José")), r#""Café\tSan José""#);
    }
}
