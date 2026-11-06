//! Independent modal filesystem metadata browser. No source contents or live
//! Workbench references cross this window. ProjectExplorer owns lazy enumeration;
//! only timer/input routes poll and prepare the bounded read-only paint frame.
use super::super::{dpi, ffi::*, paint, shell, wide};
use jc_app::{
    explorer::{ExplorerLimits, ExplorerLoadState, ExplorerView, NodeId, ProjectExplorer},
    Wakeup,
};
use std::{
    cell::{Cell, RefCell},
    ffi::OsString,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
    ptr::{null, null_mut},
    sync::OnceLock,
};

const PATH: i32 = 100;
const LIST: i32 = 101;
const UP: i32 = 3;
const PATH_UNITS: usize = 32767;
const LIST_CLASS: &str = "CIDE.ThemedFileList.v1";

#[repr(C)]
struct DrawItem {
    kind: u32,
    id: u32,
    item: u32,
    action: u32,
    state: u32,
    window: Hwnd,
    dc: Hdc,
    rect: Rect,
    data: usize,
}
#[repr(C)]
struct MonitorInfo {
    size: u32,
    monitor: Rect,
    work: Rect,
    flags: u32,
}
#[repr(C)]
struct MinMaxInfo {
    reserved: Point,
    max_size: Point,
    max_position: Point,
    min_track: Point,
    max_track: Point,
}

#[link(name = "user32")]
extern "system" {
    fn DialogBoxIndirectParamW(
        instance: Handle,
        template: *const u32,
        parent: Hwnd,
        proc: Option<unsafe extern "system" fn(Hwnd, u32, usize, isize) -> isize>,
        parameter: isize,
    ) -> isize;
    fn EndDialog(hwnd: Hwnd, result: isize) -> i32;
    fn GetDlgItem(hwnd: Hwnd, id: i32) -> Hwnd;
    fn GetParent(hwnd: Hwnd) -> Hwnd;
    fn GetFocus() -> Hwnd;
    fn SetWindowTextW(hwnd: Hwnd, text: *const u16) -> i32;
    fn GetWindowRect(hwnd: Hwnd, rect: *mut Rect) -> i32;
    fn SetWindowPos(hwnd: Hwnd, after: Hwnd, x: i32, y: i32, w: i32, h: i32, flags: u32) -> i32;
    fn SetTimer(hwnd: Hwnd, id: usize, ms: u32, callback: usize) -> usize;
    fn KillTimer(hwnd: Hwnd, id: usize) -> i32;
    fn MonitorFromWindow(hwnd: Hwnd, flags: u32) -> Handle;
    fn GetMonitorInfoW(monitor: Handle, info: *mut MonitorInfo) -> i32;
}

struct Row {
    id: NodeId,
    label: String,
    directory: bool,
}
#[derive(Default)]
struct Frame {
    generation: u64,
    revision: u64,
    first: usize,
    total: usize,
    rows: Vec<Row>,
    capacity: usize,
    selected: Option<NodeId>,
    message: String,
}
struct Model {
    catalog: ProjectExplorer,
    view: ExplorerView,
    root: PathBuf,
    frame: Frame,
    error: String,
    path_edited: bool,
    wheel: i32,
    drag: Option<(u64, u64)>,
    painted: Option<(u64, u64, usize, i32, i32)>,
}
impl Model {
    fn new(root: PathBuf) -> Result<Self, String> {
        let mut catalog = ProjectExplorer::new(Wakeup::default(), ExplorerLimits::default())?;
        catalog.set_root(root.clone())?;
        Ok(Self {
            catalog,
            view: ExplorerView::default(),
            root,
            frame: Frame::default(),
            error: String::new(),
            path_edited: false,
            wheel: 0,
            drag: None,
            painted: None,
        })
    }
    fn navigate(&mut self, path: PathBuf) -> Result<(), String> {
        let path = path
            .canonicalize()
            .map_err(|e| format!("No se pudo abrir la carpeta: {e}"))?;
        if !path.is_dir() {
            return Err("La ruta no es una carpeta".into());
        }
        self.catalog.set_root(path.clone())?;
        self.root = path;
        self.view = ExplorerView::default();
        self.frame = Frame::default();
        self.drag = None;
        self.painted = None;
        self.wheel = 0;
        self.error.clear();
        Ok(())
    }
    fn prepare(&mut self, capacity: usize) {
        let presentation = self.view.prepare(&self.catalog, capacity, 180);
        let root = self.catalog.root_id();
        let rows = presentation
            .rows
            .into_iter()
            .map(|row| {
                let label = if Some(row.id) == root {
                    if self.root.parent().is_some() {
                        "[..]  Carpeta superior".into()
                    } else {
                        "[.]  Raíz de la unidad".into()
                    }
                } else {
                    let name = self.catalog.path(row.id).and_then(|path| {
                        path.file_name().and_then(|s| s.to_str()).map(str::to_owned)
                    });
                    let name = name.map_or_else(
                        || {
                            row.label
                                .trim_start()
                                .trim_start_matches("[+] ")
                                .trim_start_matches("[-] ")
                                .to_owned()
                        },
                        |name| jc_app::source_services::display_ui_label(&name, 180),
                    );
                    format!("{}  {name}", if row.is_dir { "▸" } else { " " })
                };
                Row {
                    id: row.id,
                    label,
                    directory: row.is_dir,
                }
            })
            .collect();
        self.frame = Frame {
            generation: presentation.generation,
            revision: presentation.revision,
            first: presentation.first,
            total: presentation.total,
            rows,
            capacity,
            selected: self.view.selected(),
            message: if self.error.is_empty() {
                presentation.message
            } else {
                self.error.clone()
            },
        };
    }
    fn selected_path(&self) -> Option<(PathBuf, bool)> {
        let id = self.view.selected()?;
        if Some(id) == self.catalog.root_id() {
            return self.root.parent().map(|path| (path.to_owned(), true));
        }
        Some((self.catalog.path(id)?, self.catalog.node(id)?.is_dir()))
    }
    fn frame_current(&self) -> bool {
        self.frame.generation == self.catalog.generation()
            && self.frame.revision == self.catalog.revision()
            && self.frame.first == self.view.scroll()
    }
}
struct Picker {
    parent: Hwnd,
    file: bool,
    model: RefCell<Model>,
    font: Cell<Handle>,
    brush: Handle,
    result: RefCell<Option<PathBuf>>,
    failed: Cell<bool>,
    #[cfg(test)]
    script: Option<TestChoice>,
    #[cfg(test)]
    test_ok: Cell<bool>,
}
impl Picker {
    fn title(&self) -> &'static str {
        if self.file {
            "Abrir archivo"
        } else {
            "Abrir carpeta"
        }
    }
    fn accept(&self) -> &'static str {
        if self.file {
            "Abrir"
        } else {
            "Elegir carpeta"
        }
    }
}
impl Drop for Picker {
    fn drop(&mut self) {
        unsafe {
            DeleteObject(self.font.get());
            DeleteObject(self.brush);
        }
    }
}
unsafe fn data<'a>(hwnd: Hwnd) -> Option<&'a Picker> {
    (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Picker).as_ref()
}
fn capacity(height: i32, scale: dpi::Dpi) -> usize {
    (height.max(0) / scale.pixels(25)).clamp(0, 128) as usize
}
unsafe fn prepare(hwnd: Hwnd, picker: &Picker) {
    let list = GetDlgItem(hwnd, LIST);
    let mut rect = Rect::default();
    GetClientRect(list, &mut rect);
    picker
        .model
        .borrow_mut()
        .prepare(capacity(rect.bottom, dpi::for_window(hwnd)));
    InvalidateRect(list, null(), 0);
    InvalidateRect(hwnd, null(), 0);
}
unsafe fn path_text(hwnd: Hwnd, picker: &Picker) {
    let text: Vec<_> = picker
        .model
        .borrow()
        .root
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    SetWindowTextW(GetDlgItem(hwnd, PATH), text.as_ptr());
    picker.model.borrow_mut().path_edited = false;
}
unsafe fn cancel_drag(hwnd: Hwnd, picker: &Picker) {
    picker.model.borrow_mut().drag = None;
    if GetCapture() == GetDlgItem(hwnd, LIST) {
        ReleaseCapture();
    }
}
unsafe fn navigate(hwnd: Hwnd, picker: &Picker, path: PathBuf) {
    cancel_drag(hwnd, picker);
    let result = picker.model.borrow_mut().navigate(path);
    if let Err(error) = result {
        picker.model.borrow_mut().error = error;
    } else {
        path_text(hwnd, picker);
    }
    SetTimer(hwnd, 1, 40, 0);
    prepare(hwnd, picker);
}
unsafe fn accept_path(hwnd: Hwnd, picker: &Picker, path: PathBuf) {
    match path
        .canonicalize()
        .and_then(|path| std::fs::metadata(&path).map(|meta| (path, meta)))
    {
        Ok((path, meta)) if picker.file && meta.is_file() => {
            *picker.result.borrow_mut() = Some(path);
            EndDialog(hwnd, 1);
        }
        Ok((path, meta)) if meta.is_dir() && !picker.file => {
            *picker.result.borrow_mut() = Some(path);
            EndDialog(hwnd, 1);
        }
        Ok((path, meta)) if meta.is_dir() => navigate(hwnd, picker, path),
        Ok(_) => {
            picker.model.borrow_mut().error = "El elemento elegido no es un archivo regular".into();
            prepare(hwnd, picker);
        }
        Err(error) => {
            picker.model.borrow_mut().error = format!("La ruta no está disponible: {error}");
            prepare(hwnd, picker);
        }
    }
}
unsafe fn activate_list(hwnd: Hwnd, picker: &Picker) {
    let selected = {
        let model = picker.model.borrow();
        if model.frame_current() {
            model.selected_path()
        } else {
            None
        }
    };
    if let Some((path, directory)) = selected {
        if directory {
            navigate(hwnd, picker, path)
        } else {
            accept_path(hwnd, picker, path)
        }
    }
}
unsafe fn accept(hwnd: Hwnd, picker: &Picker) {
    if GetFocus() == GetDlgItem(hwnd, PATH) || picker.model.borrow().path_edited {
        let mut units = vec![0u16; PATH_UNITS + 1];
        let count = GetWindowTextW(
            GetDlgItem(hwnd, PATH),
            units.as_mut_ptr(),
            units.len() as i32,
        );
        let path = PathBuf::from(OsString::from_wide(&units[..count.max(0) as usize]));
        let root = picker.model.borrow().root.clone();
        let path = if path.is_absolute() {
            path
        } else {
            root.join(path)
        };
        if path.is_dir() {
            navigate(hwnd, picker, path)
        } else {
            accept_path(hwnd, picker, path)
        }
    } else if picker.file || GetFocus() == GetDlgItem(hwnd, LIST) {
        activate_list(hwnd, picker);
    } else {
        let root = picker.model.borrow().root.clone();
        accept_path(hwnd, picker, root);
    }
}

unsafe fn arrange(hwnd: Hwnd) {
    let scale = dpi::for_window(hwnd);
    let mut rect = Rect::default();
    GetClientRect(hwnd, &mut rect);
    let width = rect.right;
    let height = rect.bottom;
    let margin = scale.pixels(14);
    let button = scale.pixels(112);
    let row = scale.pixels(30);
    for (id, x, y, w, h) in [
        (
            PATH,
            margin,
            scale.pixels(44),
            (width - margin * 3 - scale.pixels(64)).max(1),
            row,
        ),
        (
            UP,
            (width - margin - scale.pixels(64)).max(margin),
            scale.pixels(44),
            scale.pixels(64),
            row,
        ),
        (
            LIST,
            margin,
            scale.pixels(86),
            (width - margin * 2).max(1),
            (height - scale.pixels(164)).max(1),
        ),
        (
            1,
            (width - margin * 2 - button * 2).max(margin),
            (height - margin - row).max(0),
            button,
            row,
        ),
        (
            2,
            (width - margin - button).max(margin),
            (height - margin - row).max(0),
            button,
            row,
        ),
    ] {
        SetWindowPos(GetDlgItem(hwnd, id), null_mut(), x, y, w, h, 0x14);
    }
}
unsafe fn center(hwnd: Hwnd, picker: &Picker) -> bool {
    let mut parent = Rect::default();
    let mut info = MonitorInfo {
        size: std::mem::size_of::<MonitorInfo>() as u32,
        monitor: Rect::default(),
        work: Rect::default(),
        flags: 0,
    };
    if GetWindowRect(picker.parent, &mut parent) == 0
        || GetMonitorInfoW(MonitorFromWindow(picker.parent, 2), &mut info) == 0
    {
        return false;
    }
    let scale = dpi::for_window(picker.parent);
    let w = scale
        .pixels(640)
        .min((info.work.right - info.work.left).max(1));
    let h = scale
        .pixels(420)
        .min((info.work.bottom - info.work.top).max(1));
    let x = (parent.left + (parent.right - parent.left - w) / 2)
        .clamp(info.work.left, info.work.right - w);
    let y = (parent.top + (parent.bottom - parent.top - h) / 2)
        .clamp(info.work.top, info.work.bottom - h);
    SetWindowPos(hwnd, null_mut(), x, y, w, h, 0x14) != 0
}
fn item(words: &mut Vec<u16>, id: u16, class: &[u16], text: &str, style: u32) {
    if words.len() % 2 != 0 {
        words.push(0);
    }
    words.extend([style as u16, (style >> 16) as u16, 0, 0, 0, 0, 10, 10, id]);
    words.extend_from_slice(class);
    words.extend(wide(text));
    words.push(0);
}
fn template(picker: &Picker) -> Vec<u32> {
    let style = 0x8004_0000u32; // popup + resize frame, application-painted caption.
    let mut words = vec![
        style as u16,
        (style >> 16) as u16,
        0,
        0,
        5,
        0,
        0,
        450,
        300,
        0,
        0,
    ];
    words.extend(wide(picker.title()));
    item(&mut words, PATH as u16, &[0xffff, 0x81], "", 0x5001_0080); // EDIT, auto horizontal scroll.
    item(&mut words, LIST as u16, &wide(LIST_CLASS), "", 0x5001_0000);
    for (id, text) in [(1, picker.accept()), (2, "Cancelar"), (UP, "Subir")] {
        item(&mut words, id as u16, &[0xffff, 0x80], text, 0x5001_000b);
    }
    if words.len() % 2 != 0 {
        words.push(0);
    }
    words
        .chunks_exact(2)
        .map(|w| u32::from(w[0]) | (u32::from(w[1]) << 16))
        .collect()
}
unsafe fn paint_dialog(hwnd: Hwnd, picker: &Picker) -> Result<(), String> {
    let mut ps: PaintStruct = std::mem::zeroed();
    let dc = BeginPaint(hwnd, &mut ps);
    if dc.is_null() {
        return Err("No se pudo dibujar el selector".into());
    }
    let mut rect = Rect::default();
    GetClientRect(hwnd, &mut rect);
    let scale = dpi::for_window(hwnd);
    let old = SelectObject(dc, picker.font.get());
    let result = (|| {
        paint::fill(dc, rect, shell::RAISED)?;
        paint::fill(
            dc,
            Rect {
                bottom: scale.pixels(32),
                ..rect
            },
            shell::SELECTED,
        )?;
        paint::text(
            dc,
            Rect {
                left: scale.pixels(14),
                top: scale.pixels(7),
                right: rect.right - scale.pixels(14),
                bottom: scale.pixels(31),
            },
            &wide(picker.title()),
            shell::TEXT,
            shell::SELECTED,
        )?;
        let message = picker.model.borrow().frame.message.clone();
        paint::text(
            dc,
            Rect {
                left: scale.pixels(14),
                right: rect.right - scale.pixels(14),
                top: rect.bottom - scale.pixels(72),
                bottom: rect.bottom - scale.pixels(48),
            },
            &wide(&message),
            shell::MUTED,
            shell::RAISED,
        )?;
        for edge in [
            Rect { bottom: 1, ..rect },
            Rect {
                top: rect.bottom - 1,
                ..rect
            },
            Rect { right: 1, ..rect },
            Rect {
                left: rect.right - 1,
                ..rect
            },
        ] {
            paint::fill(dc, edge, shell::MUTED)?;
        }
        Ok(())
    })();
    SelectObject(dc, old);
    EndPaint(hwnd, &ps);
    result
}
unsafe fn paint_button(item: &DrawItem, picker: &Picker) -> Result<(), String> {
    let bg = if item.state & 17 != 0 {
        shell::SELECTED
    } else {
        shell::SURFACE
    };
    paint::fill(item.dc, item.rect, bg)?;
    let old = SelectObject(item.dc, picker.font.get());
    let label = match item.id {
        1 => picker.accept(),
        2 => "Cancelar",
        _ => "Subir",
    };
    let mut rect = item.rect;
    rect.left += (rect.right - rect.left) / 8;
    let result = paint::text(item.dc, rect, &wide(label), shell::TEXT, bg);
    SelectObject(item.dc, old);
    result
}

unsafe extern "system" fn dialog_proc(hwnd: Hwnd, message: u32, wp: usize, lp: isize) -> isize {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        dialog_message(hwnd, message, wp, lp)
    }))
    .unwrap_or_else(|_| {
        if let Some(p) = data(hwnd) {
            p.failed.set(true);
        }
        EndDialog(hwnd, 0);
        1
    })
}
unsafe fn dialog_message(hwnd: Hwnd, message: u32, wp: usize, lp: isize) -> isize {
    if message == 0x110 {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, lp);
        let picker = data(hwnd).unwrap();
        if !center(hwnd, picker) {
            picker.failed.set(true);
            EndDialog(hwnd, 0);
            return 1;
        }
        arrange(hwnd);
        for id in [PATH, LIST, 1, 2, UP] {
            SendMessageW(GetDlgItem(hwnd, id), 0x30, picker.font.get() as usize, 0);
        }
        SendMessageW(GetDlgItem(hwnd, PATH), 0xc5, PATH_UNITS, 0);
        path_text(hwnd, picker);
        prepare(hwnd, picker);
        SetTimer(hwnd, 1, 40, 0);
        SetFocus(GetDlgItem(hwnd, LIST));
        return 0;
    }
    let Some(picker) = data(hwnd) else {
        return 0;
    };
    match message {
        WM_CLOSE => {
            EndDialog(hwnd, 0);
            1
        }
        WM_DESTROY => {
            KillTimer(hwnd, 1);
            1
        }
        WM_SIZE => {
            cancel_drag(hwnd, picker);
            arrange(hwnd);
            prepare(hwnd, picker);
            1
        }
        dpi::CHANGED => {
            let font = dpi::ui_font(dpi::Dpi::from_raw(wp as u16 as u32));
            if font.is_null() {
                picker.failed.set(true);
                EndDialog(hwnd, 0);
                return 1;
            }
            let old = picker.font.replace(font);
            for id in [PATH, LIST, 1, 2, UP] {
                SendMessageW(GetDlgItem(hwnd, id), 0x30, font as usize, 0);
            }
            DeleteObject(old);
            let rect = &*(lp as *const Rect);
            SetWindowPos(
                hwnd,
                null_mut(),
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                0x14,
            );
            arrange(hwnd);
            prepare(hwnd, picker);
            1
        }
        0x113 => {
            let changed = {
                let mut model = picker.model.borrow_mut();
                let root = model.root.clone();
                let changed = model.catalog.poll(&root, 2);
                if changed {
                    model.drag = None;
                }
                changed
            };
            if changed {
                cancel_drag(hwnd, picker);
                prepare(hwnd, picker);
            }
            let terminal = {
                let m = picker.model.borrow();
                m.catalog
                    .root_id()
                    .and_then(|id| m.catalog.node(id))
                    .is_none_or(|node| {
                        !matches!(
                            node.state(),
                            ExplorerLoadState::Queued | ExplorerLoadState::Loading
                        )
                    })
            };
            if terminal {
                KillTimer(hwnd, 1);
                #[cfg(test)]
                if picker.script.is_some() {
                    test_script(hwnd, picker);
                }
            }
            1
        }
        WM_MOUSEWHEEL => {
            let list = GetDlgItem(hwnd, LIST);
            let mut point = Point {
                x: (lp as u16) as i16 as i32,
                y: ((lp as usize >> 16) as u16) as i16 as i32,
            };
            let mut rect = Rect::default();
            GetClientRect(list, &mut rect);
            if ScreenToClient(list, &mut point) != 0
                && point.x >= 0
                && point.x < rect.right
                && point.y >= 0
                && point.y < rect.bottom
            {
                SendMessageW(list, message, wp, lp);
                1
            } else {
                0
            }
        }
        0x24 => {
            let info = &mut *(lp as *mut MinMaxInfo);
            let scale = dpi::for_window(hwnd);
            info.min_track.x = scale.pixels(360);
            info.min_track.y = scale.pixels(280);
            1
        }
        WM_COMMAND => {
            match (wp & 0xffff) as i32 {
                PATH if wp >> 16 == 0x300 => {
                    picker.model.borrow_mut().path_edited = true;
                }
                1 => accept(hwnd, picker),
                2 => {
                    EndDialog(hwnd, 0);
                }
                UP => {
                    let parent = picker.model.borrow().root.parent().map(Path::to_owned);
                    if let Some(path) = parent {
                        navigate(hwnd, picker, path);
                    }
                }
                _ => return 0,
            }
            1
        }
        0x400 => {
            SetWindowLongPtrW(hwnd, 0, 1 | (0x534b << 16));
            1
        }
        0x401 => 1,
        0x133 | 0x136 | 0x138 => {
            SetTextColor(wp as Hdc, shell::TEXT);
            SetBkColor(wp as Hdc, shell::SURFACE);
            picker.brush as isize
        }
        0x2b => {
            let item = &*(lp as *const DrawItem);
            if item.kind == 4 {
                if paint_button(item, picker).is_err() {
                    picker.failed.set(true);
                }
                1
            } else {
                0
            }
        }
        WM_PAINT => {
            if paint_dialog(hwnd, picker).is_err() {
                picker.failed.set(true);
            }
            1
        }
        0x84 => {
            let mut point = Point {
                x: (lp as u16) as i16 as i32,
                y: ((lp as usize >> 16) as u16) as i16 as i32,
            };
            if ScreenToClient(hwnd, &mut point) != 0
                && point.y > 6
                && point.y < dpi::for_window(hwnd).pixels(32)
                && point.x > 6
            {
                SetWindowLongPtrW(hwnd, 0, 2);
                1
            } else {
                0
            }
        }
        _ => 0,
    }
}

fn rail_target(frame: &Frame, height: i32, row: i32, y: i32) -> usize {
    let limit = frame.total.saturating_sub(frame.capacity);
    if limit == 0 {
        return 0;
    }
    let thumb =
        (frame.capacity as u128 * height.max(0) as u128 / frame.total.max(1) as u128) as i32;
    let thumb = thumb.clamp(row.min(height.max(0)), height.max(0));
    let travel = (height - thumb).max(1);
    let pixel = (y - thumb / 2).clamp(0, travel);
    (pixel as u128 * limit as u128 / travel as u128) as usize
}
unsafe fn paint_list(hwnd: Hwnd, picker: &Picker) -> Result<(), String> {
    let mut ps: PaintStruct = std::mem::zeroed();
    let dc = BeginPaint(hwnd, &mut ps);
    if dc.is_null() {
        return Err("No se pudo dibujar la lista".into());
    }
    let mut rect = Rect::default();
    GetClientRect(hwnd, &mut rect);
    let scale = dpi::for_window(hwnd);
    let row_height = scale.pixels(25);
    let old = SelectObject(dc, picker.font.get());
    let result = (|| {
        paint::fill(dc, rect, shell::SIDEBAR)?;
        let model = picker.model.borrow();
        let frame = &model.frame;
        for (index, row) in frame.rows.iter().enumerate() {
            let top = index as i32 * row_height;
            if top >= rect.bottom {
                break;
            }
            let bg = if Some(row.id) == frame.selected {
                shell::SELECTED
            } else {
                shell::SIDEBAR
            };
            let area = Rect {
                left: 0,
                top,
                right: rect.right - scale.pixels(12),
                bottom: (top + row_height).min(rect.bottom),
            };
            paint::fill(dc, area, bg)?;
            paint::text(
                dc,
                Rect {
                    left: scale.pixels(10),
                    top: top + scale.pixels(4),
                    ..area
                },
                &wide(&row.label),
                if row.directory {
                    shell::TEXT
                } else {
                    shell::MUTED
                },
                bg,
            )?;
        }
        if frame.total > frame.capacity && rect.bottom > 0 {
            let size = ((frame.capacity as u128 * rect.bottom as u128 / frame.total as u128)
                as i32)
                .clamp(row_height.min(rect.bottom), rect.bottom);
            let top = (frame.first as u128 * (rect.bottom - size) as u128
                / frame.total.saturating_sub(frame.capacity).max(1) as u128)
                as i32;
            paint::fill(
                dc,
                Rect {
                    left: rect.right - scale.pixels(8),
                    right: rect.right - scale.pixels(2),
                    top,
                    bottom: top + size,
                },
                shell::MUTED,
            )?;
        }
        Ok(())
    })();
    SelectObject(dc, old);
    EndPaint(hwnd, &ps);
    if result.is_ok() {
        let mut model = picker.model.borrow_mut();
        model.painted = Some((
            model.frame.generation,
            model.frame.revision,
            model.frame.first,
            rect.right,
            rect.bottom,
        ));
    }
    result
}
unsafe extern "system" fn list_proc(hwnd: Hwnd, message: u32, wp: usize, lp: isize) -> isize {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        list_message(hwnd, message, wp, lp)
    }))
    .unwrap_or_else(|_| {
        let parent = GetParent(hwnd);
        if let Some(p) = data(parent) {
            p.failed.set(true);
        }
        EndDialog(parent, 0);
        0
    })
}
unsafe fn list_message(hwnd: Hwnd, message: u32, wp: usize, lp: isize) -> isize {
    let parent = GetParent(hwnd);
    let Some(picker) = data(parent) else {
        return DefWindowProcW(hwnd, message, wp, lp);
    };
    match message {
        0x87 => 1 | 0x80, // arrows and characters; Tab/Escape remain dialog navigation.
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            if paint_list(hwnd, picker).is_err() {
                picker.failed.set(true);
            }
            0
        }
        WM_SETFOCUS | WM_KILLFOCUS => {
            InvalidateRect(hwnd, null(), 0);
            0
        }
        WM_KEYDOWN => {
            cancel_drag(parent, picker);
            if wp == 0x0d {
                activate_list(parent, picker);
                return 0;
            }
            if wp == 0x08 || (wp == 0x26 && GetKeyState(0x12) < 0) {
                let path = picker.model.borrow().root.parent().map(Path::to_owned);
                if let Some(path) = path {
                    navigate(parent, picker, path);
                }
                return 0;
            }
            {
                let mut m = picker.model.borrow_mut();
                let rows = m.frame.capacity.max(1) as isize;
                let Model { view, catalog, .. } = &mut *m;
                match wp {
                    0x26 => view.move_selection(catalog, -1),
                    0x28 => view.move_selection(catalog, 1),
                    0x21 => view.move_selection(catalog, -rows),
                    0x22 => view.move_selection(catalog, rows),
                    0x24 => view.first(catalog),
                    0x23 => view.last(catalog),
                    _ => return DefWindowProcW(hwnd, message, wp, lp),
                }
            }
            prepare(parent, picker);
            0
        }
        WM_MOUSEWHEEL => {
            cancel_drag(parent, picker);
            {
                let mut m = picker.model.borrow_mut();
                m.wheel = m.wheel.saturating_add(((wp >> 16) as u16) as i16 as i32);
                let step = -(m.wheel / 120) as isize * 3;
                m.wheel %= 120;
                let rows = m.frame.capacity;
                if step != 0 {
                    let Model { view, catalog, .. } = &mut *m;
                    view.scroll_by(catalog, step, rows);
                }
            }
            prepare(parent, picker);
            0
        }
        WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
            SetFocus(hwnd);
            let x = (lp as u16) as i16 as i32;
            let y = ((lp as usize >> 16) as u16) as i16 as i32;
            let mut rect = Rect::default();
            GetClientRect(hwnd, &mut rect);
            let scale = dpi::for_window(hwnd);
            let rail = x >= rect.right - scale.pixels(12);
            {
                let mut m = picker.model.borrow_mut();
                if !m.frame_current()
                    || m.painted
                        != Some((
                            m.frame.generation,
                            m.frame.revision,
                            m.frame.first,
                            rect.right,
                            rect.bottom,
                        ))
                {
                    return 0;
                }
                if rail {
                    m.drag = Some((m.catalog.generation(), m.catalog.revision()));
                    let first = rail_target(&m.frame, rect.bottom, scale.pixels(25), y);
                    let rows = m.frame.capacity;
                    let Model { view, catalog, .. } = &mut *m;
                    view.scroll_to(catalog, first, rows);
                } else if let Some(row) = m.frame.rows.get((y.max(0) / scale.pixels(25)) as usize) {
                    let id = row.id;
                    m.view.select(id);
                }
            }
            if rail {
                SetCapture(hwnd);
            }
            prepare(parent, picker);
            if !rail && message == WM_LBUTTONDBLCLK {
                activate_list(parent, picker);
            }
            0
        }
        WM_MOUSEMOVE | WM_LBUTTONUP => {
            let y = ((lp as usize >> 16) as u16) as i16 as i32;
            let mut rect = Rect::default();
            GetClientRect(hwnd, &mut rect);
            let changed = {
                let mut m = picker.model.borrow_mut();
                if m.drag != Some((m.catalog.generation(), m.catalog.revision())) {
                    m.drag = None;
                    false
                } else {
                    let first =
                        rail_target(&m.frame, rect.bottom, dpi::for_window(hwnd).pixels(25), y);
                    let rows = m.frame.capacity;
                    let Model { view, catalog, .. } = &mut *m;
                    view.scroll_to(catalog, first, rows)
                }
            };
            if message == WM_LBUTTONUP {
                picker.model.borrow_mut().drag = None;
                if GetCapture() == hwnd {
                    ReleaseCapture();
                }
            }
            if changed {
                prepare(parent, picker);
            }
            0
        }
        WM_CAPTURECHANGED | WM_CANCELMODE => {
            picker.model.borrow_mut().drag = None;
            0
        }
        _ => DefWindowProcW(hwnd, message, wp, lp),
    }
}
unsafe fn register() -> Result<(), String> {
    static REGISTERED: OnceLock<Result<(), String>> = OnceLock::new();
    REGISTERED
        .get_or_init(|| {
            let class_name = wide(LIST_CLASS);
            let class = WndClass {
                style: 8,
                proc: Some(list_proc),
                class_extra: 0,
                window_extra: 0,
                instance: GetModuleHandleW(null()),
                icon: null_mut(),
                cursor: LoadCursorW(null_mut(), 32512usize as *const u16),
                background: null_mut(),
                menu_name: null(),
                class_name: class_name.as_ptr(),
            };
            if RegisterClassW(&class) == 0 {
                Err("No se pudo registrar la lista de archivos de CIDE".into())
            } else {
                Ok(())
            }
        })
        .clone()
}
fn initial_directory(initial: Option<&Path>) -> Result<PathBuf, String> {
    let initial = initial
        .map(Path::to_owned)
        .map_or_else(std::env::current_dir, Ok)
        .map_err(|e| e.to_string())?;
    let initial = if initial.is_file() {
        initial.parent().unwrap_or(&initial).to_owned()
    } else {
        initial
    };
    let path = initial
        .canonicalize()
        .map_err(|e| format!("La carpeta inicial no está disponible: {e}"))?;
    if !path.is_dir() {
        return Err("La ruta inicial no es una carpeta".into());
    }
    Ok(path)
}
unsafe fn create(parent: Hwnd, initial: Option<&Path>, file: bool) -> Result<Picker, String> {
    if parent.is_null() || IsWindow(parent) == 0 {
        return Err("El selector requiere una ventana propietaria activa".into());
    }
    register()?;
    let root = initial_directory(initial)?;
    let model = Model::new(root)?;
    let font = dpi::ui_font(dpi::for_window(parent));
    let brush = CreateSolidBrush(shell::SURFACE);
    if font.is_null() || brush.is_null() {
        if !font.is_null() {
            DeleteObject(font);
        }
        if !brush.is_null() {
            DeleteObject(brush);
        }
        return Err("No se pudo preparar el selector de CIDE".into());
    }
    Ok(Picker {
        parent,
        file,
        model: RefCell::new(model),
        font: Cell::new(font),
        brush,
        result: RefCell::new(None),
        failed: Cell::new(false),
        #[cfg(test)]
        script: None,
        #[cfg(test)]
        test_ok: Cell::new(false),
    })
}
unsafe fn show(picker: &mut Picker) -> Result<Option<PathBuf>, String> {
    let template = template(picker);
    let result = DialogBoxIndirectParamW(
        GetModuleHandleW(null()),
        template.as_ptr(),
        picker.parent,
        Some(dialog_proc),
        picker as *mut Picker as isize,
    );
    if result == -1 || picker.failed.get() {
        return Err("No se pudo mostrar o dibujar el selector de CIDE".into());
    }
    let selected = picker.result.borrow_mut().take();
    Ok(selected)
}

pub(super) unsafe fn choose(
    parent: Hwnd,
    initial: Option<&Path>,
    file: bool,
) -> Result<Option<PathBuf>, String> {
    let mut picker = create(parent, initial, file)?;
    show(&mut picker)
}

#[cfg(test)]
#[derive(Clone, Copy)]
enum TestChoice {
    Folder,
    File,
    Cancel,
}
#[cfg(test)]
unsafe fn test_script(hwnd: Hwnd, picker: &Picker) {
    ShowWindow(hwnd, 0);
    let styled = [1, 2, UP]
        .into_iter()
        .all(|id| GetWindowLongPtrW(GetDlgItem(hwnd, id), -16) & 0xf == 0xb)
        && SendMessageW(GetDlgItem(hwnd, PATH), 0x31, 0, 0) == picker.font.get() as isize;
    SetWindowPos(hwnd, null_mut(), 0, 0, 610, 420, 0x16);
    let list = GetDlgItem(hwnd, LIST);
    InvalidateRect(list, null(), 0);
    SendMessageW(list, WM_PAINT, 0, 0);
    let bounded = picker.model.borrow().frame.rows.len() <= 128;
    picker
        .test_ok
        .set(styled && bounded && picker.model.borrow().painted.is_some());
    match picker.script.unwrap() {
        TestChoice::Cancel => {
            SendMessageW(hwnd, WM_COMMAND, 2, 0);
        }
        TestChoice::Folder => {
            SetFocus(GetDlgItem(hwnd, 1));
            SendMessageW(hwnd, WM_COMMAND, 1, 0);
        }
        TestChoice::File => {
            let id = {
                let m = picker.model.borrow();
                m.catalog
                    .nodes()
                    .iter()
                    .find(|node| !node.is_dir())
                    .map(|node| node.id())
            };
            if let Some(id) = id {
                picker.model.borrow_mut().view.select(id);
                prepare(hwnd, picker);
                SetFocus(list);
                SendMessageW(hwnd, WM_COMMAND, 1, 0);
            } else {
                picker.test_ok.set(false);
                EndDialog(hwnd, 0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };
    fn fixture() -> PathBuf {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("work/test-temp")
            .join(format!(
                "themed-picker-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        fs::create_dir_all(root.join("subcarpeta")).unwrap();
        fs::write(root.join("entrada.txt"), b"owned picker fixture\r\n").unwrap();
        root.canonicalize().unwrap()
    }
    fn ready(model: &mut Model) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            model.catalog.poll(&model.root, 2);
            if model
                .catalog
                .root_id()
                .and_then(|id| model.catalog.node(id))
                .is_some_and(|node| node.state() == ExplorerLoadState::Ready)
            {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    #[test]
    fn picker_catalog_parent_navigation_is_typed_lazy_bounded_and_revision_checked() {
        let root = fixture();
        let mut model = Model::new(root.clone()).unwrap();
        ready(&mut model);
        model.prepare(2);
        assert!(model.frame.rows.len() <= 2);
        assert_eq!(model.frame.rows[0].label, "[..]  Carpeta superior");
        let parent = model.selected_path().unwrap();
        assert!(parent.1);
        assert_eq!(parent.0, root.parent().unwrap());
        let file = model
            .catalog
            .nodes()
            .iter()
            .find(|node| !node.is_dir())
            .unwrap()
            .id();
        model.view.select(file);
        model.prepare(2);
        assert_eq!(
            model.selected_path(),
            Some((root.join("entrada.txt"), false))
        );
        let stale = model.frame.generation;
        model.navigate(root.join("subcarpeta")).unwrap();
        assert_ne!(model.catalog.generation(), stale);
        assert!(model.catalog.node(file).is_none());
        assert!(model.painted.is_none());
        ready(&mut model);
        model.prepare(128);
        assert_eq!(model.selected_path().unwrap().0, root);
    }
    #[test]
    fn picker_initial_file_resolves_to_parent_and_geometry_is_bounded_by_dpi() {
        let root = fixture();
        assert_eq!(
            initial_directory(Some(&root.join("entrada.txt"))).unwrap(),
            root
        );
        for raw in [96, 144, 192] {
            let scale = dpi::Dpi::from_raw(raw);
            // Individual row heights are rounded once to physical pixels. At
            // 150%, ten 25px design rows need 380px, while 250 design pixels
            // become 375px: only nine complete rows fit in that viewport.
            assert_eq!(capacity(scale.pixels(25) * 10, scale), 10);
            assert_eq!(capacity(scale.pixels(10000), scale), 128);
            assert_eq!(capacity(0, scale), 0);
        }
        let scale = dpi::Dpi::from_raw(144);
        assert_eq!(capacity(scale.pixels(250), scale), 9);
    }
    #[test]
    fn picker_own_native_modal_uses_theme_font_and_accepts_folder_file_or_cancel() {
        let root = fixture();
        unsafe {
            let parent = CreateWindowExW(
                0,
                wide("STATIC").as_ptr(),
                wide("CIDE selector fixture").as_ptr(),
                0xcf0000,
                0,
                0,
                900,
                600,
                null_mut(),
                null_mut(),
                GetModuleHandleW(null()),
                null_mut(),
            );
            assert!(!parent.is_null());
            for choice in [TestChoice::Folder, TestChoice::File, TestChoice::Cancel] {
                let mut picker =
                    create(parent, Some(&root), matches!(choice, TestChoice::File)).unwrap();
                picker.script = Some(choice);
                let result = show(&mut picker).unwrap();
                assert!(picker.test_ok.get());
                match choice {
                    TestChoice::Folder => assert_eq!(result, Some(root.clone())),
                    TestChoice::File => assert_eq!(result, Some(root.join("entrada.txt"))),
                    TestChoice::Cancel => assert!(result.is_none()),
                }
            }
            DestroyWindow(parent);
        }
        assert_eq!(
            fs::read(root.join("entrada.txt")).unwrap(),
            b"owned picker fixture\r\n"
        );
    }
}
