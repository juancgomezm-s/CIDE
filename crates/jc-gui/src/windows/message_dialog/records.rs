//! Native report control over accepted pages from the read-only ABL worker.
use super::*;
use crate::windows::notify::Bridge;
use jc_app::Wakeup;
use jc_progress::editor::{
    BrowseController, BrowseFilter, BrowsePage, BrowseRequest, BrowseTable,
    BROWSE_MAX_FILTER_BYTES, BROWSE_PAGE_SIZE,
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
const BUTTONS: [(u16, &str); 5] = [
    (APPLY, "Aplicar filtro"),
    (SORT, "Invertir orden"),
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
const CB_SETCURSEL: u32 = 0x14e;
const EM_SETLIMITTEXT: u32 = 0xc5;

const LVM_SETBKCOLOR: u32 = 0x1001;
const LVM_DELETEALLITEMS: u32 = 0x1009;
const LVM_GETHEADER: u32 = 0x101f;
const LVM_SETTEXTCOLOR: u32 = 0x1024;
const LVM_SETTEXTBKCOLOR: u32 = 0x1026;
const LVM_SETEXTENDEDLISTVIEWSTYLE: u32 = 0x1036;
const LVM_INSERTITEMW: u32 = 0x104d;
const LVM_INSERTCOLUMNW: u32 = 0x1061;
const LVM_SETITEMTEXTW: u32 = 0x1074;
const HDM_GETITEMW: u32 = 0x120b;
const HDM_SETITEMW: u32 = 0x120c;

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
    accepted: Query,
    offset: usize,
    has_next: bool,
    pending: Option<Query>,
}

struct Browser<'a> {
    dialog: Dialog<'a>,
    root: &'a Path,
    table: &'a BrowseTable,
    page: RefCell<PageState>,
    bridge: Arc<Bridge>,
    wakeup: Wakeup,
}

impl Browser<'_> {
    fn template(&self) -> Vec<u32> {
        let mut words = template_header(self.dialog.title, 9);
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
        EnableWindow(child(hwnd, PREVIOUS), 0);
        EnableWindow(child(hwnd, NEXT), 0);
        SetFocus(grid);
        true
    }

    unsafe fn submit(&self, hwnd: Hwnd, query: Query, offset: usize) {
        let mut page = self.page.borrow_mut();
        if page.worker.busy() {
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
                set_status(hwnd, "Solo lectura · Consultando registros…");
                for id in [PREVIOUS, NEXT, SORT, APPLY] {
                    EnableWindow(child(hwnd, id), 0);
                }
            }
            Err(error) => set_status(hwnd, &error),
        }
    }

    unsafe fn poll(&self, hwnd: Hwnd) {
        self.bridge.acknowledge();
        let mut page = self.page.borrow_mut();
        let Some(result) = page.worker.poll(self.root) else {
            return;
        };
        let Some(query) = page.pending.take() else {
            return;
        };
        match result {
            Ok(result) => {
                if result.columns != self.table.columns || !install_page(child(hwnd, GRID), &result)
                {
                    self.dialog.failed.set(true);
                    drop(page);
                    EndDialog(hwnd, 0);
                    return;
                }
                set_status(hwnd, &page_label(&result, query.descending));
                page.accepted = query;
                page.offset = result.offset;
                page.has_next = result.has_next;
            }
            Err(error) => set_status(hwnd, &error),
        }
        EnableWindow(child(hwnd, PREVIOUS), i32::from(page.offset > 0));
        EnableWindow(child(hwnd, NEXT), i32::from(page.has_next));
        EnableWindow(child(hwnd, SORT), 1);
        EnableWindow(child(hwnd, APPLY), 1);
    }

    unsafe fn command(&self, hwnd: Hwnd, id: u16) {
        let (mut query, offset, has_next) = {
            let page = self.page.borrow();
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

fn page_label(page: &BrowsePage, descending: bool) -> String {
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
    format!("Solo lectura · {records} · Clave primaria en orden {order}")
}

fn wide_cell(value: Option<&str>) -> Vec<u16> {
    let value = value.unwrap_or("(desconocido)");
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
    _: usize,
) -> isize {
    if message == WM_DRAWITEM {
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
                SetTextColor(item.dc, shell::TEXT);
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

unsafe fn create_grid(
    parent: Hwnd,
    bounds: Rect,
    font: Handle,
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
    if grid.is_null() || SetWindowSubclass(grid, grid_procedure, 1, 0) == 0 {
        return None;
    }
    SendMessageW(grid, WM_SETFONT, font as usize, 0);
    SendMessageW(grid, LVM_SETBKCOLOR, 0, shell::BACKGROUND as isize);
    SendMessageW(grid, LVM_SETTEXTBKCOLOR, 0, shell::BACKGROUND as isize);
    SendMessageW(grid, LVM_SETTEXTCOLOR, 0, shell::TEXT as isize);
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
    if page.rows.iter().any(|row| row.len() != page.columns.len()) {
        return false;
    }
    SendMessageW(grid, LVM_DELETEALLITEMS, 0, 0);
    for (index, row) in page.rows.iter().enumerate() {
        let mut item: Item = std::mem::zeroed();
        item.mask = 1; // LVIF_TEXT
        item.index = index as i32;
        for (column, value) in row.iter().enumerate() {
            let mut text = wide_cell(value.as_deref());
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
        browser.submit(hwnd, Query::default(), 0);
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
            (FIELD, 1) => {
                let selected = SendMessageW(child(hwnd, FIELD), CB_GETCURSEL, 0, 0);
                EnableWindow(child(hwnd, VALUE), i32::from(selected > 0));
                1
            }
            (id @ (PREVIOUS | NEXT | APPLY | SORT), 0) => {
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
            browser.page.borrow_mut().worker.cancel();
            0
        }
        TASKS_MESSAGE => {
            browser.poll(hwnd);
            1
        }
        DM_GETDEFID => {
            let focus = GetFocus();
            let id = BUTTONS
                .iter()
                .find(|(id, _)| child(hwnd, *id) == focus)
                .map_or(APPLY, |(id, _)| *id);
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
            SetTextColor(wp as Hdc, shell::TEXT);
            SetBkColor(wp as Hdc, shell::RAISED);
            browser.dialog.brush as isize
        }
        WM_DRAWITEM => {
            let item = &*(lp as *const DrawItem);
            if item.control_type != 4 {
                return 0;
            }
            let Some((_, label)) = BUTTONS
                .iter()
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

unsafe fn initialize_list_class() -> bool {
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
    };
    show_browser(&browser)
}

#[cfg(test)]
mod tests {
    use super::*;

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
            assert_eq!(
                page_label(&page, true),
                "Solo lectura · Página 2 · Registros 21–21 · Clave primaria en orden descendente"
            );
            page.rows = vec![vec![Some("22".into()), None]];
            assert!(install_page(grid, &page));
            assert_eq!(read_cell(0), "22");
            assert_eq!(read_cell(1), "(desconocido)");
            assert_ne!(DestroyWindow(owner), 0);
        }
    }
}
