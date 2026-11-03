mod clipboard;
mod clipboard_flow;
mod ffi;
mod folder;
mod notify;
mod paint;
mod save_flow;
mod save_plan;
mod save_smoke;
mod search_paint;
mod search_panel;
mod selector;
mod selector_flow;
mod selector_paint;
mod smoke;
use crate::GuiOptions;
use ffi::*;
use jc_app::quick_open::QuickOpenController;
use jc_app::{
    default_keymap, default_registry, CommandId, CommandOutcome, CommandRegistry, Key, KeyMap,
    KeyScope, KeyStroke, Modifiers, PrimaryFocus, Resolution, Wakeup, Workbench,
};
use jc_text::TextEncoding;
use selector::{Selector, SelectorKind};
use std::os::windows::ffi::OsStringExt;
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    ptr::null_mut,
    sync::Arc,
};

pub(super) fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
fn os_error(operation: &str) -> String {
    format!("{operation}: {}", std::io::Error::last_os_error())
}

const OPEN_SIDE: usize = 190;
const COMMANDS: &[(usize, CommandId, &str)] = &[
    (101, CommandId::FileNew, "&New"),
    (102, CommandId::FileOpen, "&Open..."),
    (103, CommandId::FileSave, "&Save"),
    (104, CommandId::FileSaveAs, "Save &As..."),
    (105, CommandId::FileClose, "&Close Tab"),
    (106, CommandId::FileExit, "E&xit"),
    (107, CommandId::FileOpenFolder, "Open &Folder..."),
    (108, CommandId::FileQuickOpen, "&Quick Open..."),
    (
        109,
        CommandId::FileChooseEncoding,
        "Choose Source &Encoding...",
    ),
    (110, CommandId::EditUndo, "&Undo"),
    (111, CommandId::EditRedo, "&Redo"),
    (112, CommandId::EditCut, "Cu&t"),
    (113, CommandId::EditCopy, "&Copy"),
    (114, CommandId::EditPaste, "&Paste"),
    (115, CommandId::EditSelectAll, "Select &All"),
    (116, CommandId::EditJoinLines, "&Join Lines"),
    (117, CommandId::EditSplitLine, "&Split Line"),
    (118, CommandId::FileSaveAll, "Save A&ll..."),
    (120, CommandId::EditorSplitRight, "Split &Right"),
    (121, CommandId::EditorOneGroup, "&One Group"),
    (122, CommandId::EditorFocusPrimary, "Focus Group &1"),
    (123, CommandId::EditorFocusSecondary, "Focus Group &2"),
    (124, CommandId::TabNext, "&Next Tab"),
    (125, CommandId::TabPrevious, "&Previous Tab"),
    (126, CommandId::PaletteOpen, "Command &Palette..."),
    (
        127,
        CommandId::ViewToggleInvisibles,
        "Show &Invisible Characters",
    ),
    (130, CommandId::SearchFind, "&Find Multiline..."),
    (131, CommandId::SearchReplace, "&Replace Multiline..."),
    (132, CommandId::SearchFindNext, "Find &Next"),
    (133, CommandId::SearchFindPrevious, "Find &Previous"),
];
pub(super) fn command_id(command: CommandId) -> usize {
    COMMANDS
        .iter()
        .find(|(_, c, _)| *c == command)
        .map_or(0, |item| item.0)
}

enum Action {
    None,
    Clipboard(CommandId),
    PasteSearch(u64),
    ChooseFolder,
    Open(bool),
    SaveAs,
    SaveAll,
    CloseTab,
    Exit,
}
struct State {
    pub workbench: Workbench,
    registry: CommandRegistry,
    keymap: KeyMap,
    encoding: Option<TextEncoding>,
    project_root: Option<PathBuf>,
    selector: Option<Selector>,
    search_panel: Option<search_panel::SearchPanel>,
    encoding_target: Option<(jc_app::DocumentId, u64, Option<TextEncoding>)>,
    smoke_clipboard: Option<String>,
    quick_open: QuickOpenController,
    wakeup: Wakeup,
    bridge: Arc<notify::Bridge>,
    deliveries: usize,
    pub status: String,
    pub width: i32,
    pub height: i32,
    pub cell_width: i32,
    pub cell_height: i32,
    pub font: Handle,
    owned_font: bool,
    pub focused: bool,
    pending_surrogate: Option<u16>,
    next_untitled: usize,
    pub paint_count: usize,
    pub paint_errors: usize,
    pub save_count: usize,
    pub smoke: bool,
    smoke_root: Option<PathBuf>,
    smoke_save_paths: std::collections::VecDeque<Option<PathBuf>>,
    save_all_active: bool,
    pub key_override: Option<Modifiers>,
}
impl Drop for State {
    fn drop(&mut self) {
        if self.owned_font {
            unsafe {
                DeleteObject(self.font);
            }
        }
    }
}
impl State {
    fn new(options: &GuiOptions) -> Result<Self, String> {
        let bridge = Arc::new(notify::Bridge::default());
        let callback = bridge.clone();
        let wakeup = Wakeup::with_notifier(move || callback.signal());
        let face = wide("Consolas");
        let created =
            unsafe { CreateFontW(-17, 0, 0, 0, 400, 0, 0, 0, 1, 0, 0, 5, 0x31, face.as_ptr()) };
        let font = if created.is_null() {
            unsafe { GetStockObject(16) }
        } else {
            created
        };
        if font.is_null() {
            return Err(os_error("Create native fixed font"));
        }
        let mut state = Self {
            workbench: Workbench::default(),
            registry: default_registry(),
            keymap: default_keymap(),
            encoding: options.encoding,
            project_root: None,
            selector: None,
            search_panel: None,
            encoding_target: None,
            smoke_clipboard: None,
            quick_open: QuickOpenController::new(wakeup.clone()),
            wakeup,
            bridge,
            deliveries: 0,
            status: "Native Win32/GDI · scalar cells · Windows text clipboard".into(),
            width: 1000,
            height: 650,
            cell_width: 9,
            cell_height: 20,
            font,
            owned_font: !created.is_null(),
            focused: false,
            pending_surrogate: None,
            next_untitled: 0,
            paint_count: 0,
            paint_errors: 0,
            save_count: 0,
            smoke: options.smoke.is_some(),
            smoke_root: options
                .smoke
                .as_ref()
                .and_then(|s| s.fixture.parent())
                .and_then(|p| p.canonicalize().ok()),
            smoke_save_paths: Default::default(),
            save_all_active: false,
            key_override: None,
        };
        if let Some(root) = options
            .smoke
            .as_ref()
            .and_then(|s| s.fixture.parent())
            .or(options.project_root.as_deref())
        {
            state.set_project_root(root.to_owned())?;
        }
        if let Some(smoke) = &options.smoke {
            state
                .workbench
                .open_file(&smoke.fixture, Some(TextEncoding::Utf8))
                .map_err(|e| e.to_string())?;
        } else {
            for path in &options.paths {
                state
                    .workbench
                    .open_file(path, options.encoding)
                    .map_err(|e| e.to_string())?;
            }
            if options.paths.is_empty() {
                state.new_document();
            }
        }
        state.ensure_encoding_selector();
        Ok(state)
    }
    fn new_document(&mut self) {
        self.next_untitled += 1;
        self.workbench.new_file(
            format!("untitled-{}.txt", self.next_untitled),
            self.encoding.unwrap_or(TextEncoding::Utf8),
        );
    }
    fn error(&mut self, error: impl std::fmt::Display) {
        self.status = error.to_string().chars().take(240).collect();
    }
    fn context(&self) -> jc_app::CommandContext {
        let mut context = self.workbench.context();
        context.focus = PrimaryFocus::Editor;
        context.has_project = self.project_root.is_some();
        // Availability is an intention gate, never a read of another owner's
        // clipboard. Content is requested only by explicit Paste.
        if !context.rectangular_selection {
            context.clipboard_has_text = context.has_document;
        }
        context
    }
    fn execute(&mut self, command: CommandId) -> Action {
        if !self.registry.is_enabled(command, &self.context()) {
            self.status = "Command unavailable in the current context".into();
            return Action::None;
        }
        match command {
            CommandId::SearchFind | CommandId::SearchReplace => {
                self.open_search_panel(command == CommandId::SearchReplace);
                return Action::None;
            }
            CommandId::FileChooseEncoding => {
                self.open_encoding_selector();
                return Action::None;
            }
            CommandId::EditCopy | CommandId::EditCut | CommandId::EditPaste
                if !self.context().rectangular_selection
                    && (command != CommandId::EditPaste
                        || !self.workbench.clipboard_is_rectangular()) =>
            {
                return Action::Clipboard(command)
            }
            CommandId::FileOpenFolder => {
                self.close_selector();
                return Action::ChooseFolder;
            }
            CommandId::PaletteOpen => {
                self.open_selector(SelectorKind::Palette);
                return Action::None;
            }
            CommandId::FileQuickOpen => {
                self.open_selector(SelectorKind::Files);
                return Action::None;
            }
            CommandId::FileNew => {
                self.new_document();
                return Action::None;
            }
            CommandId::FileOpen => return Action::Open(false),
            CommandId::FileSaveAs => return Action::SaveAs,
            CommandId::FileClose => return Action::CloseTab,
            CommandId::FileExit => return Action::Exit,
            CommandId::FileSaveAll => {
                if self.save_all_active {
                    self.status = "Save All is already in progress".into();
                    return Action::None;
                }
                return Action::SaveAll;
            }
            CommandId::FileSave
                if self
                    .workbench
                    .active_document()
                    .is_some_and(|document| document.session.is_new()) =>
            {
                return Action::SaveAs
            }
            _ => {}
        }
        match self.workbench.execute(command) {
            Ok(CommandOutcome::View(CommandId::ViewToggleInvisibles)) => {
                self.status = if self.workbench.invisibles_visible() {
                    "Invisible characters: ASCII spans on"
                } else {
                    "Invisible characters hidden"
                }
                .into()
            }
            Ok(CommandOutcome::None) => {
                if command == CommandId::FileSave {
                    self.save_count += 1;
                }
                self.status = if self.context().rectangular_selection {
                    "Ready · rectangular clipboard stays internal"
                } else {
                    "Ready · Windows text clipboard · scalar-cell preview"
                }
                .into();
            }
            Ok(CommandOutcome::View(
                CommandId::EditorSplitRight
                | CommandId::EditorOneGroup
                | CommandId::EditorFocusPrimary
                | CommandId::EditorFocusSecondary
                | CommandId::EditorTogglePin
                | CommandId::EditorToggleScrollLink
                | CommandId::EditorOpenToSide
                | CommandId::NavigateBack
                | CommandId::NavigateForward,
            )) => self.status = "Ready · shared editor groups".into(),
            Ok(_) => self.status = "This command needs a frontend flow not provided here".into(),
            Err(error) => self.error(error),
        }
        Action::None
    }
    fn modifiers(&self) -> Modifiers {
        if let Some(value) = self.key_override {
            return value;
        }
        if self.smoke {
            return Modifiers::NONE;
        }
        let mut modifiers = Modifiers::NONE;
        for (key, flag) in [
            (0x10, Modifiers::SHIFT),
            (0x11, Modifiers::CTRL),
            (0x12, Modifiers::ALT),
        ] {
            if unsafe { GetKeyState(key) } < 0 {
                modifiers = modifiers.union(flag);
            }
        }
        modifiers
    }
    fn key(&mut self, code: usize) -> (bool, Action) {
        self.pending_surrogate = None;
        let modifiers = self.modifiers();
        let key = match code {
            0x09 => Some(Key::Tab),
            0x0d => Some(Key::Enter),
            0x1b => Some(Key::Escape),
            0x21 => Some(Key::PageUp),
            0x22 => Some(Key::PageDown),
            0x23 => Some(Key::End),
            0x24 => Some(Key::Home),
            0x25 => Some(Key::Left),
            0x26 => Some(Key::Up),
            0x27 => Some(Key::Right),
            0x28 => Some(Key::Down),
            0x30..=0x5a => char::from_u32(code as u32).map(|ch| Key::Char(ch.to_ascii_lowercase())),
            0x70..=0x87 => Some(Key::Function((code - 0x70 + 1) as u8)),
            _ => None,
        };
        if let Some(key) = key {
            let global = self.keymap.resolve(
                KeyScope::Global,
                KeyStroke::new(modifiers, key),
                &self.registry,
                &self.context(),
            );
            if let Resolution::Enabled(
                command @ (CommandId::ViewToggleInvisibles | CommandId::FileExit),
            ) = global
            {
                return (true, self.execute(command));
            }
        }
        if self.search_panel.is_some() {
            return (true, self.search_key(code, modifiers));
        }
        if self.selector.is_some() {
            return (true, self.selector_key(code, modifiers));
        }
        if let Some(key) = key {
            let resolution = self.keymap.resolve(
                KeyScope::Editor,
                KeyStroke::new(modifiers, key),
                &self.registry,
                &self.context(),
            );
            match resolution {
                Resolution::Enabled(command) if supported(command) => {
                    return (true, self.execute(command))
                }
                Resolution::Disabled(command) if supported(command) => {
                    self.status = "Command unavailable".into();
                    return (true, Action::None);
                }
                _ => {}
            }
        }
        if modifiers.contains(Modifiers::CTRL) || modifiers.contains(Modifiers::ALT) {
            return (false, Action::None);
        }
        let select = modifiers.contains(Modifiers::SHIFT);
        let result = match code {
            0x08 => self.workbench.backspace(),
            0x2e => self.workbench.delete_forward(),
            0x25 => self.workbench.move_left(select),
            0x27 => self.workbench.move_right(select),
            0x26 => self.workbench.move_vertical(-1, select),
            0x28 => self.workbench.move_vertical(1, select),
            0x24 => self.workbench.move_home(select),
            0x23 => self.workbench.move_end(select),
            0x21 => self
                .workbench
                .move_vertical(-((self.height / self.cell_height).max(1) as isize), select),
            0x22 => self
                .workbench
                .move_vertical((self.height / self.cell_height).max(1) as isize, select),
            _ => return (false, Action::None),
        };
        if let Err(error) = result {
            self.error(error);
        }
        (true, Action::None)
    }
    fn character(&mut self, unit: u16) {
        if unit < 0x20 || unit == 0x7f {
            return;
        }
        if (0xd800..=0xdbff).contains(&unit) {
            if self.pending_surrogate.replace(unit).is_some() {
                self.status = "Unpaired UTF-16 high surrogate rejected".into();
            }
            return;
        }
        let scalar = if (0xdc00..=0xdfff).contains(&unit) {
            let Some(high) = self.pending_surrogate.take() else {
                self.status = "Unpaired UTF-16 low surrogate rejected".into();
                return;
            };
            0x10000 + ((u32::from(high) - 0xd800) << 10) + (u32::from(unit) - 0xdc00)
        } else {
            if self.pending_surrogate.take().is_some() {
                self.status = "Unpaired UTF-16 sequence rejected".into();
                return;
            }
            u32::from(unit)
        };
        if let Some(ch) = char::from_u32(scalar) {
            self.insert_character(ch);
        }
    }
    fn insert_character(&mut self, ch: char) {
        if ch.is_control() {
            return;
        }
        let mut bytes = [0u8; 4];
        if self.search_panel.is_some() {
            self.edit_search_input(jc_app::FieldEdit::Insert(ch.encode_utf8(&mut bytes)));
            return;
        }
        if let Some(selector) = self.selector.as_mut() {
            if selector.append(ch.encode_utf8(&mut bytes)) {
                self.request_files();
            } else {
                self.status = "Selector query limit or unsupported control".into();
            }
            return;
        }
        match self.workbench.insert_text(ch.encode_utf8(&mut bytes)) {
            Ok(()) => self.status = "Ready".into(),
            Err(error) => self.error(error),
        }
    }
}
fn supported(command: CommandId) -> bool {
    COMMANDS.iter().any(|(_, value, _)| *value == command)
        || matches!(
            command,
            CommandId::EditIndent
                | CommandId::EditOutdent
                | CommandId::EditorOpenToSide
                | CommandId::SearchCancel
                | CommandId::EditRectangleLeft
                | CommandId::EditRectangleRight
                | CommandId::EditRectangleUp
                | CommandId::EditRectangleDown
                | CommandId::Tab1
                | CommandId::Tab2
                | CommandId::Tab3
                | CommandId::Tab4
                | CommandId::Tab5
                | CommandId::Tab6
                | CommandId::Tab7
                | CommandId::Tab8
                | CommandId::Tab9
        )
}

/// Box remains Rust-owned during CreateWindowEx. Only after success does the
/// HWND acquire ownership; failed creation cannot double-free an NCCREATE pointer.
struct Owner {
    state: RefCell<State>,
    hwnd_owned: Cell<bool>,
}
// Caller bounds the returned reference to a live HWND; never retain it across
// DestroyWindow. Dialog/message calls are made only after RefCell guards end.
unsafe fn owner<'window>(hwnd: Hwnd) -> Option<&'window Owner> {
    let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Owner;
    pointer.as_ref()
}

unsafe extern "system" fn window_proc(
    hwnd: Hwnd,
    message: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        procedure(hwnd, message, wparam, lparam)
    })) {
        Ok(value) => value,
        Err(_) => {
            PostQuitMessage(1);
            0
        }
    }
}
unsafe fn procedure(hwnd: Hwnd, message: u32, wparam: usize, lparam: isize) -> isize {
    if message == WM_NCCREATE {
        let create = &*(lparam as *const CreateStruct);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.parameter as isize);
        if let Some(owner) = owner(hwnd) {
            owner.state.borrow().bridge.attach(hwnd);
        }
        return 1;
    }
    if message == WM_NCDESTROY {
        let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Owner;
        if !pointer.is_null() {
            (*pointer).state.borrow().bridge.detach();
        }
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        if !pointer.is_null() && (*pointer).hwnd_owned.get() {
            drop(Box::from_raw(pointer));
        }
        return DefWindowProcW(hwnd, message, wparam, lparam);
    }
    if message == WM_DESTROY {
        PostQuitMessage(0);
        return 0;
    }
    let Some(owner) = owner(hwnd) else {
        return DefWindowProcW(hwnd, message, wparam, lparam);
    };
    if message == WM_PAINT {
        let frame = {
            let Ok(mut state) = owner.state.try_borrow_mut() else {
                return DefWindowProcW(hwnd, message, wparam, lparam);
            };
            state.paint_count += 1;
            paint::prepare(&mut state)
        };
        let mut paint = NativePaint {
            hwnd,
            paint: std::mem::zeroed(),
        };
        let dc = BeginPaint(hwnd, &mut paint.paint);
        if !dc.is_null() {
            if let Err(error) = paint::draw(dc, &frame) {
                if let Ok(mut state) = owner.state.try_borrow_mut() {
                    state.paint_errors += 1;
                    state.error(error);
                }
            }
        } else if let Ok(mut state) = owner.state.try_borrow_mut() {
            state.paint_errors += 1;
            state.error("BeginPaint did not return a device context");
        }
        return 0;
    }
    // No RefCell borrow crosses dialogs, DestroyWindow or SendMessage. Modal
    // Win32 calls may reenter this callback and prepare another read-only frame.
    let mut action = Action::None;
    let mut handled = true;
    let mut redraw = true;
    {
        let Ok(mut state) = owner.state.try_borrow_mut() else {
            return DefWindowProcW(hwnd, message, wparam, lparam);
        };
        match message {
            WM_CREATE => {
                let dc = GetDC(hwnd);
                if !dc.is_null() {
                    let old = SelectObject(dc, state.font);
                    let mut metrics: TextMetric = std::mem::zeroed();
                    if GetTextMetricsW(dc, &mut metrics) != 0 {
                        state.cell_width = metrics.average.max(6);
                        state.cell_height = (metrics.height + metrics.external).max(12);
                    }
                    SelectObject(dc, old);
                    ReleaseDC(hwnd, dc);
                }
            }
            TASKS_MESSAGE => {
                redraw = state.poll_tasks();
            }
            WM_SIZE => {
                let mut rect = Rect::default();
                if GetClientRect(hwnd, &mut rect) != 0 {
                    state.width = rect.right.max(1);
                    state.height = rect.bottom.max(1);
                }
            }
            WM_SETFOCUS => state.focused = true,
            WM_KILLFOCUS => {
                state.focused = false;
                state.pending_surrogate = None;
            }
            WM_ERASEBKGND => return 1,
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                let result = state.key(wparam);
                handled = result.0;
                action = result.1;
            }
            WM_CHAR => state.character(wparam as u16),
            WM_UNICHAR => {
                if wparam == 0xffff {
                    return 1;
                }
                if let Some(ch) = char::from_u32(wparam as u32) {
                    state.insert_character(ch);
                }
            }
            WM_COMMAND if lparam == 0 => {
                if wparam & 0xffff == OPEN_SIDE {
                    state.close_selector();
                    action = Action::Open(true);
                } else if let Some((_, command, _)) =
                    COMMANDS.iter().find(|(id, _, _)| *id == wparam & 0xffff)
                {
                    if *command != CommandId::ViewToggleInvisibles {
                        state.close_selector();
                    }
                    action = state.execute(*command);
                }
            }
            WM_INITMENU => {
                let menu = GetMenu(hwnd);
                CheckMenuItem(
                    menu,
                    command_id(CommandId::ViewToggleInvisibles) as u32,
                    if state.workbench.invisibles_visible() {
                        8
                    } else {
                        0
                    },
                );
                for (id, command, _) in COMMANDS {
                    EnableMenuItem(
                        menu,
                        *id as u32,
                        if state.registry.is_enabled(*command, &state.context()) {
                            0
                        } else {
                            1
                        },
                    );
                }
            }
            WM_LBUTTONDOWN => {
                let x = (lparam as u16) as i16 as i32;
                let y = ((lparam as usize >> 16) as u16) as i16 as i32;
                if state.selector.is_none() && state.search_panel.is_none() {
                    paint::click(&mut state, x, y);
                }
            }
            WM_CLOSE => action = Action::Exit,
            _ => handled = false,
        }
        state.schedule_search();
    }
    if !handled {
        return DefWindowProcW(hwnd, message, wparam, lparam);
    }
    perform(hwnd, action);
    if let Some(current) = self::owner(hwnd) {
        current.state.borrow().schedule_search();
    }
    if redraw && IsWindow(hwnd) != 0 {
        InvalidateRect(hwnd, std::ptr::null(), 0);
    }
    if message == WM_UNICHAR {
        1
    } else {
        0
    }
}

struct NativePaint {
    hwnd: Hwnd,
    paint: PaintStruct,
}
impl Drop for NativePaint {
    fn drop(&mut self) {
        if !self.paint.dc.is_null() {
            unsafe {
                EndPaint(self.hwnd, &self.paint);
            }
        }
    }
}

unsafe fn file_dialog(hwnd: Hwnd, save: bool) -> Result<Option<PathBuf>, String> {
    file_dialog_named(hwnd, save, None)
}
unsafe fn file_dialog_named(
    hwnd: Hwnd,
    save: bool,
    title: Option<&str>,
) -> Result<Option<PathBuf>, String> {
    let mut file = vec![0u16; 32768];
    let filter = wide("All files\0*.*\0");
    let title = wide(title.unwrap_or(if save {
        "Save source as"
    } else {
        "Open source"
    }));
    let mut options: OpenFileName = std::mem::zeroed();
    options.size = std::mem::size_of::<OpenFileName>() as u32;
    options.owner = hwnd;
    options.filter = filter.as_ptr();
    options.file = file.as_mut_ptr();
    options.file_len = file.len() as u32;
    options.title = title.as_ptr();
    options.flags = 0x80000 | 0x800 | 0x8 | 0x4 | if save { 0x2 } else { 0x1000 };
    let accepted = if save {
        GetSaveFileNameW(&mut options)
    } else {
        GetOpenFileNameW(&mut options)
    };
    if accepted == 0 {
        let error = CommDlgExtendedError();
        return if error == 0 {
            Ok(None)
        } else {
            Err(format!("Native file dialog failed ({error:#x})"))
        };
    }
    let end = file
        .iter()
        .position(|unit| *unit == 0)
        .ok_or("Invalid file-dialog result")?;
    Ok(Some(PathBuf::from(std::ffi::OsString::from_wide(
        &file[..end],
    ))))
}
unsafe fn perform(hwnd: Hwnd, action: Action) {
    if matches!(
        action,
        Action::Open(_) | Action::SaveAs | Action::ChooseFolder
    ) && owner(hwnd).is_some_and(|owner| owner.state.borrow().smoke)
    {
        if let Some(owner) = owner(hwnd) {
            owner.state.borrow_mut().status = "Smoke prevented an unexpected native dialog".into();
        }
        return;
    }
    match action {
        Action::None => {}
        Action::Clipboard(command) => clipboard_flow::perform(hwnd, command),
        Action::PasteSearch(generation) => clipboard_flow::paste_search(hwnd, generation),
        Action::SaveAll => save_flow::perform(hwnd),
        Action::ChooseFolder => {
            let selected = folder::choose(hwnd);
            if let Some(owner) = owner(hwnd) {
                let mut state = owner.state.borrow_mut();
                match selected {
                    Ok(Some(path)) => {
                        if let Err(error) = state.set_project_root(path) {
                            state.error(error);
                        }
                    }
                    Ok(None) => {}
                    Err(error) => state.error(error),
                }
            }
        }
        Action::Open(side) => {
            let selected = file_dialog(hwnd, false);
            if let Some(owner) = owner(hwnd) {
                let mut state = owner.state.borrow_mut();
                match selected {
                    Ok(Some(path)) => {
                        let encoding = state.encoding;
                        let result = if side {
                            state.workbench.open_file_to_side(path, encoding)
                        } else {
                            state.workbench.open_file(path, encoding)
                        };
                        if let Err(error) = result {
                            state.error(error);
                        } else {
                            state.status = "Opened".into();
                            state.ensure_encoding_selector();
                        }
                    }
                    Ok(None) => {}
                    Err(error) => state.error(error),
                }
            }
        }
        Action::SaveAs => {
            let captured = owner(hwnd)
                .ok_or_else(|| "Save As window closed".to_owned())
                .and_then(|model| {
                    let state = model.state.borrow();
                    let document = state
                        .workbench
                        .active_document()
                        .ok_or("No active Save As document")?;
                    state
                        .workbench
                        .capture_save_target(document.id, document.session.document().revision())
                        .map_err(|e| e.to_string())
                });
            let target = match captured {
                Ok(target) => target,
                Err(error) => {
                    if let Some(model) = owner(hwnd) {
                        model.state.borrow_mut().error(error);
                    }
                    return;
                }
            };
            let selected = file_dialog(hwnd, true);
            if let Some(owner) = owner(hwnd) {
                let mut state = owner.state.borrow_mut();
                match selected {
                    Ok(Some(path)) => match state.workbench.save_target(target, Some(path)) {
                        Ok(()) => {
                            state.save_count += 1;
                            state.status = "Saved".into();
                        }
                        Err(error) => state.error(error),
                    },
                    Ok(None) => {}
                    Err(error) => state.error(error),
                }
            }
        }
        Action::CloseTab => {
            // F1 decides whether this closes only a view or needs a decision
            // for the last dirty document. Do not duplicate view-count logic.
            let decision = if let Some(owner) = owner(hwnd) {
                let mut state = owner.state.borrow_mut();
                match state.workbench.close_active(false) {
                    Ok(decision) => decision,
                    Err(error) => {
                        state.error(error);
                        return;
                    }
                }
            } else {
                return;
            };
            let Some(tab) = decision else {
                return;
            };
            if owner(hwnd).is_some_and(|owner| owner.state.borrow().smoke) {
                owner(hwnd).unwrap().state.borrow_mut().status =
                    "Smoke prevented an unexpected discard dialog".into();
                return;
            }
            if MessageBoxW(
                hwnd,
                wide("Discard unsaved changes in this tab? Save first to keep them.").as_ptr(),
                wide("JC-IDE").as_ptr(),
                0x134,
            ) != 6
            {
                return;
            }
            if let Some(owner) = owner(hwnd) {
                let mut state = owner.state.borrow_mut();
                if state.workbench.tabs().active_id() != Some(tab) {
                    state.status = "Active tab changed before close confirmation".into();
                    return;
                }
                if let Err(error) = state.workbench.close_active(true) {
                    state.error(error);
                }
            }
        }
        Action::Exit => {
            let dirty = owner(hwnd)
                .is_some_and(|owner| owner.state.borrow().workbench.context().dirty_documents > 0);
            if dirty
                && MessageBoxW(
                    hwnd,
                    wide(
                        "Close and discard ALL unsaved changes? Save documents first to keep them.",
                    )
                    .as_ptr(),
                    wide("JC-IDE").as_ptr(),
                    0x134,
                ) != 6
            {
                return;
            }
            DestroyWindow(hwnd);
        }
    }
}

unsafe fn create_menu(keymap: &KeyMap) -> Result<Handle, String> {
    let bar = CreateMenu();
    if bar.is_null() {
        return Err(os_error("CreateMenu"));
    }
    for (label, start, end) in [
        ("&File", 101, 110),
        ("&Edit", 110, 118),
        ("&View", 120, 128),
        ("&Search", 130, 134),
    ] {
        let popup = CreatePopupMenu();
        if popup.is_null() {
            DestroyMenu(bar);
            return Err(os_error("CreatePopupMenu"));
        }
        if AppendMenuW(bar, 0x10, popup as usize, wide(label).as_ptr()) == 0 {
            DestroyMenu(popup);
            DestroyMenu(bar);
            return Err(os_error("AppendMenu"));
        }
        for (id, command, label) in COMMANDS
            .iter()
            .filter(|(id, _, _)| *id >= start && *id < end)
        {
            let text = match keymap.primary_shortcut(*command) {
                Some(shortcut) => format!("{label}\t{shortcut}"),
                None => (*label).to_owned(),
            };
            if AppendMenuW(popup, 0, *id, wide(&text).as_ptr()) == 0 {
                DestroyMenu(bar);
                return Err(os_error("AppendMenu item"));
            }
        }
        if start == 101
            && AppendMenuW(
                popup,
                0,
                command_id(CommandId::FileSaveAll),
                wide(&format!(
                    "Save A&ll...\t{}",
                    keymap
                        .primary_shortcut(CommandId::FileSaveAll)
                        .expect("shared Save All shortcut")
                ))
                .as_ptr(),
            ) == 0
        {
            DestroyMenu(bar);
            return Err(os_error("AppendMenu Save All"));
        }
        if start == 101
            && AppendMenuW(popup, 0, OPEN_SIDE, wide("Open File to &Side...").as_ptr()) == 0
        {
            DestroyMenu(bar);
            return Err(os_error("AppendMenu side"));
        }
    }
    Ok(bar)
}

pub(super) fn run(options: GuiOptions) -> Result<(), String> {
    if options.smoke.is_some() && !options.paths.is_empty() {
        return Err("Smoke mode does not accept startup source paths".into());
    }
    if let Some(smoke) = &options.smoke {
        smoke::validate(smoke)?;
    }
    let mut owner = Box::new(Owner {
        state: RefCell::new(State::new(&options)?),
        hwnd_owned: Cell::new(false),
    });
    let class = wide(&format!("JCIDE.Native.{}", std::process::id()));
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let descriptor = WndClass {
            style: 3,
            proc: Some(window_proc),
            class_extra: 0,
            window_extra: 0,
            instance,
            icon: null_mut(),
            cursor: LoadCursorW(null_mut(), 32512usize as *const u16),
            background: null_mut(),
            menu_name: std::ptr::null(),
            class_name: class.as_ptr(),
        };
        if RegisterClassW(&descriptor) == 0 {
            return Err(os_error("RegisterClassW"));
        }
        let menu = match create_menu(&owner.state.borrow().keymap) {
            Ok(menu) => menu,
            Err(error) => {
                UnregisterClassW(class.as_ptr(), instance);
                return Err(error);
            }
        };
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            wide("JC-IDE — native Win32/GDI development frontend").as_ptr(),
            0x00cf0000,
            0x80000000u32 as i32,
            0x80000000u32 as i32,
            1000,
            700,
            null_mut(),
            menu,
            instance,
            (&mut *owner as *mut Owner).cast(),
        );
        if hwnd.is_null() {
            DestroyMenu(menu);
            UnregisterClassW(class.as_ptr(), instance);
            return Err(os_error("CreateWindowExW"));
        }
        owner.hwnd_owned.set(true);
        let _ = Box::into_raw(owner);
        if options.smoke.is_none() {
            ShowWindow(hwnd, 5);
            SetFocus(hwnd);
            UpdateWindow(hwnd);
        } else if PostMessageW(hwnd, SMOKE_MESSAGE, 0, 0) == 0 {
            let error = os_error("Post owned smoke message");
            DestroyWindow(hwnd);
            UnregisterClassW(class.as_ptr(), instance);
            return Err(error);
        }
        let mut smoke_result = None;
        let mut message: Msg = std::mem::zeroed();
        let result = loop {
            // A cooperative search may post its next slot continuously. Give
            // hardware input and an invalidated paint bounded priority before
            // consuming that work, rather than starving them behind posted jobs.
            for _ in 0..32 {
                let mut input: Msg = std::mem::zeroed();
                if IsWindow(hwnd) == 0
                    || (PeekMessageW(&mut input, hwnd, WM_KEYDOWN, WM_UNICHAR, 1) == 0
                        && PeekMessageW(&mut input, hwnd, 0x200, 0x20e, 1) == 0)
                {
                    break;
                }
                TranslateMessage(&input);
                DispatchMessageW(&input);
            }
            let mut painting: Msg = std::mem::zeroed();
            if IsWindow(hwnd) != 0 && PeekMessageW(&mut painting, hwnd, WM_PAINT, WM_PAINT, 1) != 0
            {
                DispatchMessageW(&painting);
            }
            let received = GetMessageW(&mut message, null_mut(), 0, 0);
            if received < 0 {
                break Err(os_error("GetMessageW"));
            }
            if received == 0 {
                break if message.wparam == 0 {
                    Ok(())
                } else {
                    Err("Native window callback failed".into())
                };
            }
            if message.hwnd == hwnd && message.message == SMOKE_MESSAGE {
                if let Some(config) = &options.smoke {
                    smoke_result = Some(smoke::run(hwnd, config));
                }
                // Test failures must also close without a modal dirty prompt.
                if IsWindow(hwnd) != 0 {
                    DestroyWindow(hwnd);
                }
                continue;
            }
            TranslateMessage(&message);
            DispatchMessageW(&message);
        };
        if IsWindow(hwnd) != 0 {
            DestroyWindow(hwnd);
        }
        UnregisterClassW(class.as_ptr(), instance);
        result?;
        smoke_result.unwrap_or(Ok(()))
    }
}
