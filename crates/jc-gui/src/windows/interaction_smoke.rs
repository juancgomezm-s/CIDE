//! Own-HWND mouse/wheel/menu routing. Only memory sessions are edited. Native
//! popup choices are injected through the same HMENU/WM_INITMENU/WM_COMMAND path;
//! no physical-menu, display-latency, OS clipboard or foreign-window assertion.
use super::{
    command_id,
    ffi::*,
    native_menu::{self, NativeMenuGroup as Group},
    owner, paint,
    project_smoke::snapshot,
    selector::SelectorKind,
    smoke::{command, expected, key, paste, position},
};
use crate::SmokeOptions;
use jc_app::{CommandId, FieldSelection, Modifiers, PaneId};
use jc_text::{encode, TextEncoding, TextPosition};
use std::{fs, ops::Range};
const SOURCE: &[u8] = b"alpha\r\nbeta\r\n";
#[link(name = "user32")]
extern "system" {
    fn ClientToScreen(hwnd: Hwnd, point: *mut Point) -> i32;
    fn GetWindowRect(hwnd: Hwnd, rect: *mut Rect) -> i32;
    fn SetWindowPos(
        hwnd: Hwnd,
        after: Hwnd,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        flags: u32,
    ) -> i32;
    fn GetSubMenu(menu: Handle, index: i32) -> Handle;
    fn GetMenuItemCount(menu: Handle) -> i32;
    fn GetMenuItemID(menu: Handle, index: i32) -> u32;
    fn GetMenuState(menu: Handle, item: u32, flags: u32) -> u32;
    fn GetMenuStringW(menu: Handle, item: u32, text: *mut u16, count: i32, flags: u32) -> i32;
}
pub(super) struct Resize {
    hwnd: Hwnd,
    rect: Rect,
}
impl Resize {
    pub(super) unsafe fn new(hwnd: Hwnd) -> Result<Self, String> {
        let mut rect = Rect::default();
        if GetWindowRect(hwnd, &mut rect) == 0 {
            return Err("Interaction own-window geometry unavailable".into());
        }
        Ok(Self { hwnd, rect })
    }
    pub(super) unsafe fn set(&self, width: i32, height: i32) -> Result<(), String> {
        if SetWindowPos(self.hwnd, std::ptr::null_mut(), 0, 0, width, height, 0x16) == 0 {
            return Err("Interaction own-window resize failed".into());
        }
        Ok(())
    }
}
impl Drop for Resize {
    fn drop(&mut self) {
        unsafe {
            SetWindowPos(
                self.hwnd,
                std::ptr::null_mut(),
                0,
                0,
                self.rect.right - self.rect.left,
                self.rect.bottom - self.rect.top,
                0x16,
            );
        }
    }
}
fn packed(x: i32, y: i32) -> isize {
    (((y as u16 as usize) << 16) | x as u16 as usize) as isize
}
unsafe fn mouse(hwnd: Hwnd, message: u32, point: (i32, i32), mods: Modifiers) {
    owner(hwnd).unwrap().state.borrow_mut().key_override = Some(mods);
    SendMessageW(
        hwnd,
        message,
        if message == WM_LBUTTONUP {
            0
        } else {
            1 | if mods.contains(Modifiers::SHIFT) {
                4
            } else {
                0
            }
        },
        packed(point.0, point.1),
    );
    owner(hwnd).unwrap().state.borrow_mut().key_override = None;
}
unsafe fn pixel(
    hwnd: Hwnd,
    pane: PaneId,
    line: usize,
    column: usize,
) -> Result<(i32, i32), String> {
    let state = owner(hwnd).unwrap().state.borrow();
    paint::rectangle_pixel(&state, pane, line, column)
        .ok_or_else(|| "Interaction source point is outside visible projection".into())
}
unsafe fn current_pixel(hwnd: Hwnd, pane: PaneId, column: usize) -> Result<(i32, i32), String> {
    let first = owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .panes()
        .view(pane)
        .ok_or("Interaction pane missing")?
        .scroll_line();
    pixel(hwnd, pane, first, column)
}
unsafe fn drag(
    hwnd: Hwnd,
    pane: PaneId,
    start: (usize, usize),
    end: (usize, usize),
    mods: Modifiers,
) -> Result<(), String> {
    mouse(
        hwnd,
        WM_LBUTTONDOWN,
        pixel(hwnd, pane, start.0, start.1)?,
        mods,
    );
    mouse(hwnd, WM_MOUSEMOVE, pixel(hwnd, pane, end.0, end.1)?, mods);
    mouse(hwnd, WM_LBUTTONUP, pixel(hwnd, pane, end.0, end.1)?, mods);
    Ok(())
}
unsafe fn wheel(hwnd: Hwnd, pane: PaneId, delta: i16, mods: Modifiers) -> Result<(), String> {
    let (x, y) = current_pixel(hwnd, pane, 0)?;
    let mut screen = Point { x, y };
    if ClientToScreen(hwnd, &mut screen) == 0 {
        return Err("Interaction wheel screen conversion failed".into());
    }
    let flags = if mods.contains(Modifiers::CTRL) { 8 } else { 0 }
        | if mods.contains(Modifiers::SHIFT) {
            4
        } else {
            0
        };
    owner(hwnd).unwrap().state.borrow_mut().key_override = Some(mods);
    SendMessageW(
        hwnd,
        0x20a,
        ((delta as u16 as usize) << 16) | flags,
        packed(screen.x, screen.y),
    );
    owner(hwnd).unwrap().state.borrow_mut().key_override = None;
    Ok(())
}
unsafe fn memory(
    hwnd: Hwnd,
    options: &SmokeOptions,
    text: &str,
    encoding: TextEncoding,
) -> Result<(), String> {
    let mut state = owner(hwnd).unwrap().state.borrow_mut();
    state.close_selector();
    state.explorer_visible = false;
    state.explorer_focused = false;
    state.bottom.visible = false;
    state.bottom.focused = false;
    state.workbench.new_file(
        options.fixture.with_file_name("interaction-memory.txt"),
        encoding,
    );
    state
        .workbench
        .insert_text(text)
        .map_err(|e| e.to_string())?;
    state
        .workbench
        .go_to_position(TextPosition { line: 0, column: 0 })
        .map_err(|e| e.to_string())?;
    paint::prepare(&mut state);
    Ok(())
}
unsafe fn discard(hwnd: Hwnd) -> Result<(), String> {
    let mut state = owner(hwnd).unwrap().state.borrow_mut();
    state.close_selector();
    state.explorer_visible = false;
    state.explorer_focused = false;
    state.bottom.visible = false;
    state.bottom.focused = false;
    state
        .workbench
        .close_active(true)
        .map_err(|e| e.to_string())?;
    drop(state);
    expected(hwnd, SOURCE)
}
unsafe fn revision(hwnd: Hwnd) -> u64 {
    owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .active_document()
        .unwrap()
        .session
        .document()
        .revision()
}
unsafe fn selection(hwnd: Hwnd, pane: PaneId) -> Option<Range<usize>> {
    owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .selection_for_pane(pane)
}
unsafe fn carets(hwnd: Hwnd) -> (Option<TextPosition>, Option<TextPosition>) {
    let state = owner(hwnd).unwrap().state.borrow();
    (
        state.workbench.position_for_pane(PaneId::Primary),
        state.workbench.position_for_pane(PaneId::Secondary),
    )
}
unsafe fn scrolls(hwnd: Hwnd) -> (usize, usize) {
    let state = owner(hwnd).unwrap().state.borrow();
    (
        state
            .workbench
            .panes()
            .view(PaneId::Primary)
            .unwrap()
            .scroll_line(),
        state
            .workbench
            .panes()
            .view(PaneId::Secondary)
            .unwrap()
            .scroll_line(),
    )
}
unsafe fn no_capture(hwnd: Hwnd) -> bool {
    let state = owner(hwnd).unwrap().state.borrow();
    state.linear_drag.is_none()
        && state.rectangle_drag.is_none()
        && state.editor_scroll_drag.is_none()
        && GetCapture() != hwnd
}
unsafe fn own_bytes(hwnd: Hwnd, value: &[u8]) -> Result<(), String> {
    if value.len() > 4096 {
        return Err("Interaction own memory fixture exceeded4KiB".into());
    }
    let state = owner(hwnd).unwrap().state.borrow();
    let source = state
        .workbench
        .active_document()
        .ok_or("Interaction source closed")?
        .session
        .document();
    if source.len() != value.len()
        || source
            .read_range(0..source.len())
            .map_err(|e| e.to_string())?
            != value
    {
        return Err("Interaction own memory bytes changed unexpectedly".into());
    }
    Ok(())
}
unsafe fn cycle(hwnd: Hwnd, before: &[u8], after: &[u8], rev: u64) -> Result<(), String> {
    expected(hwnd, after)?;
    if revision(hwnd) == rev {
        return Err("Interaction replacement did not change revision".into());
    }
    key(hwnd, 0x5a, Modifiers::CTRL)?;
    expected(hwnd, before)?;
    key(hwnd, 0x59, Modifiers::CTRL)?;
    expected(hwnd, after)?;
    key(hwnd, 0x5a, Modifiers::CTRL)?;
    expected(hwnd, before)
}
unsafe fn selection_history_messages(hwnd: Hwnd, options: &SmokeOptions) -> Result<(), String> {
    memory(hwnd, options, "uno dos\r\n", TextEncoding::Utf8)?;
    command(hwnd, CommandId::EditorSplitRight)?;
    command(hwnd, CommandId::EditorFocusSecondary)?;
    let (origin, document, tabs, undo) = {
        let mut state = owner(hwnd).unwrap().state.borrow_mut();
        // A reversed source-byte selection must preserve its orientation, not
        // merely reconstruct the sorted range after applying an inverse edit.
        state
            .workbench
            .set_linear_selection_bytes(7, 4)
            .map_err(|e| e.to_string())?;
        let doc = state.workbench.active_document().unwrap();
        (
            state
                .workbench
                .panes()
                .view(PaneId::Secondary)
                .unwrap()
                .view_id(),
            doc.id,
            state.workbench.tabs().tabs().len(),
            doc.session.document().undo_steps(),
        )
    };
    SendMessageW(hwnd, WM_CHAR, 'X' as usize, 0);
    own_bytes(hwnd, b"uno X\r\n")?;
    {
        let state = owner(hwnd).unwrap().state.borrow();
        let doc = state.workbench.active_document().unwrap();
        if doc.session.document().undo_steps() != undo + 1
            || doc.session.document().redo_steps() != 0
        {
            return Err("Native replacement did not create exactly one undo transition".into());
        }
    }
    // Invoke undo in the other persistent view: restore the initiator's
    // selection while preserving the invoking group's focus and shared source.
    command(hwnd, CommandId::EditorFocusPrimary)?;
    key(hwnd, 0x5a, Modifiers::CTRL)?;
    own_bytes(hwnd, b"uno dos\r\n")?;
    {
        let state = owner(hwnd).unwrap().state.borrow();
        let view = state.workbench.panes().view(PaneId::Secondary).unwrap();
        let selection = view.state().unwrap();
        let doc = state.workbench.active_document().unwrap();
        if state.workbench.panes().active() != PaneId::Primary
            || view.view_id() != origin
            || (selection.cursor, selection.anchor) != (4, Some(7))
            || doc.id != document
            || state.workbench.tabs().tabs().len() != tabs
            || doc.session.document().undo_steps() != undo
            || doc.session.document().redo_steps() != 1
        {
            return Err("Native undo lost selection orientation, view identity, focus, or single transition".into());
        }
    }
    key(hwnd, 0x59, Modifiers::CTRL)?;
    own_bytes(hwnd, b"uno X\r\n")?;
    {
        let state = owner(hwnd).unwrap().state.borrow();
        let view = state.workbench.panes().view(PaneId::Secondary).unwrap();
        let selection = view.state().unwrap();
        let doc = state.workbench.active_document().unwrap();
        if state.workbench.panes().active() != PaneId::Primary
            || view.view_id() != origin
            || (selection.cursor, selection.anchor) != (5, None)
            || doc.id != document
            || doc.session.document().undo_steps() != undo + 1
            || doc.session.document().redo_steps() != 0
        {
            return Err(
                "Native redo did not restore final initiator caret without changing focus".into(),
            );
        }
    }
    key(hwnd, 0x5a, Modifiers::CTRL)?;
    own_bytes(hwnd, b"uno dos\r\n")?;
    command(hwnd, CommandId::EditorOneGroup)?;
    discard(hwnd)
}
unsafe fn palette(hwnd: Hwnd, label: &str) -> Result<(), String> {
    command(hwnd, CommandId::PaletteOpen)?;
    paste(hwnd, label)?;
    key(hwnd, 0x0d, Modifiers::NONE)
}
unsafe fn native_label(hwnd: Hwnd, menu: Handle, id: usize) -> Result<String, String> {
    let mut text = [0u16; 512];
    let count = GetMenuStringW(menu, id as u32, text.as_mut_ptr(), text.len() as i32, 0);
    if count <= 0 {
        return native_menu::display_label(hwnd, menu, id)
            .ok_or("Falta la etiqueta nativa y propia del menú".into());
    }
    String::from_utf16(&text[..count as usize]).map_err(|e| e.to_string())
}
unsafe fn menu_cancel_check(
    hwnd: Hwnd,
    group: Group,
    command: CommandId,
    enabled: bool,
    shortcut: Option<&str>,
) -> Result<(), String> {
    let mut validation = Ok(());
    native_menu::open_with(hwnd, group, |popup, _, _| {
        validation = (|| {
            let id = command_id(command);
            let flags = GetMenuState(popup, id as u32, 0);
            if flags == u32::MAX || (flags & 3 == 0) != enabled {
                return Err("Native menu enabled state differs from focused control".into());
            }
            let label = native_label(hwnd, popup, id)?;
            if let Some(shortcut) = shortcut {
                if !label.contains(shortcut) {
                    return Err("Native menu shortcut label is not contextual".into());
                }
            } else if label.contains('\t') {
                return Err("Unavailable native command advertises an active shortcut".into());
            }
            Ok(())
        })();
        0
    })?;
    validation
}
#[derive(PartialEq, Eq)]
struct Input {
    text: String,
    selection: FieldSelection,
    revision: u64,
    candidate: usize,
}
unsafe fn input(hwnd: Hwnd) -> Result<Input, String> {
    let state = owner(hwnd).unwrap().state.borrow();
    let selector = state
        .selector
        .as_ref()
        .ok_or("Interaction selector closed")?;
    Ok(Input {
        text: selector.input().into(),
        selection: selector.field_selection(),
        revision: selector.input_revision(),
        candidate: selector.selected(),
    })
}

pub(super) unsafe fn run(hwnd: Hwnd, options: &SmokeOptions) -> Result<Vec<&'static str>, String> {
    expected(hwnd, SOURCE)?;
    command(hwnd, CommandId::EditorOneGroup)?;
    let resize = Resize::new(hwnd)?;
    resize.set(1600, 900)?;
    let root = options
        .fixture
        .parent()
        .ok_or("Interaction fixture has no parent")?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let (initial_encoding, initial_invisibles, wheel_lines) = {
        let mut state = owner(hwnd).unwrap().state.borrow_mut();
        state.close_selector();
        state.set_project_root(root.clone())?;
        let values = (
            state.workbench.active_encoding(),
            state.workbench.invisibles_visible(),
            state.smoke_wheel_lines,
        );
        state.smoke_wheel_lines = Some(3);
        values
    };
    if initial_invisibles {
        command(hwnd, CommandId::ViewToggleInvisibles)?;
    }
    position(hwnd, 0, 0)?;
    let baseline = snapshot(hwnd)?;
    let mut checks = Vec::new();
    // A system move/size loop owns capture independently of editor drags.
    // WM_SIZE and client cleanup must not terminate that native capture.
    SetCapture(hwnd);
    SendMessageW(hwnd, WM_SIZE, 0, 0);
    let system_capture_preserved = GetCapture() == hwnd;
    ReleaseCapture();
    if !system_capture_preserved {
        return Err("Client pointer cleanup released system resize capture".into());
    }
    checks.push("interaction_system_resize_capture_survives_client_cleanup");
    selection_history_messages(hwnd, options)?;
    checks.push("interaction_native_undo_redo_restores_origin_selection_keeps_invoker_focus");
    for encoding in [
        TextEncoding::Utf8,
        TextEncoding::Windows1252,
        TextEncoding::Iso8859_1,
        TextEncoding::Ibm850,
    ] {
        let bom = if encoding == TextEncoding::Utf8 {
            "\u{feff}"
        } else {
            ""
        };
        let text = format!("{bom}año\r\nlast\nend\r");
        let original = encode(&text, encoding).map_err(|e| e.to_string())?;
        memory(hwnd, options, &text, encoding)?;
        let rev = revision(hwnd);
        let id = snapshot(hwnd)?.0;
        let undo = owner(hwnd)
            .unwrap()
            .state
            .borrow()
            .workbench
            .active_document()
            .unwrap()
            .session
            .document()
            .undo_steps();
        let from = encode(&format!("{bom}a"), encoding)
            .map_err(|e| e.to_string())?
            .len();
        let to = encode(&format!("{bom}año\r\nla"), encoding)
            .map_err(|e| e.to_string())?
            .len();
        drag(hwnd, PaneId::Primary, (0, 1), (1, 2), Modifiers::NONE)?;
        if selection(hwnd, PaneId::Primary) != Some(from..to)
            || !no_capture(hwnd)
            || revision(hwnd) != rev
            || snapshot(hwnd)?.0 != id
        {
            return Err("Linear drag lost UTF8/legacy boundaries or ownership".into());
        }
        expected(hwnd, &original)?;
        if owner(hwnd)
            .unwrap()
            .state
            .borrow()
            .workbench
            .active_document()
            .unwrap()
            .session
            .document()
            .undo_steps()
            != undo
        {
            return Err("Selection-only mouse input created source undo".into());
        }
        SendMessageW(hwnd, WM_CHAR, 'Q' as usize, 0);
        cycle(
            hwnd,
            &original,
            &encode(&format!("{bom}aQst\nend\r"), encoding).map_err(|e| e.to_string())?,
            rev,
        )?;
        let cr = encode(&format!("{bom}año"), encoding)
            .map_err(|e| e.to_string())?
            .len();
        let before = snapshot(hwnd)?;
        let rejected = owner(hwnd)
            .unwrap()
            .state
            .borrow_mut()
            .workbench
            .set_linear_selection_bytes(from, cr + 1)
            .is_err();
        if !rejected || snapshot(hwnd)? != before {
            return Err("Linear selection accepted interior CRLF or mutated on failure".into());
        }
        if encoding == TextEncoding::Utf8 {
            let rejected = owner(hwnd)
                .unwrap()
                .state
                .borrow_mut()
                .workbench
                .set_linear_selection_bytes(from, from + 1)
                .is_err();
            if !rejected || snapshot(hwnd)? != before {
                return Err("Linear selection accepted interior UTF8 scalar".into());
            }
        }
        command(hwnd, CommandId::ViewToggleInvisibles)?;
        let end = from + encode("ñ", encoding).map_err(|e| e.to_string())?.len();
        let (left, right) = {
            let state = owner(hwnd).unwrap().state.borrow();
            let tab = state.workbench.active_document().unwrap().tab_id;
            (
                state
                    .workbench
                    .display_column_at_byte(tab, 0, from)
                    .ok_or("Diagnostic ñ start unavailable")?,
                state
                    .workbench
                    .display_column_at_byte(tab, 0, end)
                    .ok_or("Diagnostic ñ end unavailable")?,
            )
        };
        drag(
            hwnd,
            PaneId::Primary,
            (0, left),
            (0, right),
            Modifiers::NONE,
        )?;
        if selection(hwnd, PaneId::Primary) != Some(from..end) {
            return Err(
                "Diagnostic linear drag selected escape columns instead of source scalar".into(),
            );
        }
        mouse(
            hwnd,
            WM_LBUTTONDOWN,
            pixel(hwnd, PaneId::Primary, 0, left + 2)?,
            Modifiers::NONE,
        );
        mouse(
            hwnd,
            WM_LBUTTONUP,
            pixel(hwnd, PaneId::Primary, 0, left + 2)?,
            Modifiers::NONE,
        );
        if owner(hwnd)
            .unwrap()
            .state
            .borrow()
            .workbench
            .active_editor()
            .unwrap()
            .cursor
            != from
        {
            return Err("Inside an escaped scalar produced an interior source caret".into());
        }
        command(hwnd, CommandId::ViewToggleInvisibles)?;
        discard(hwnd)?;
    }
    checks.push("interaction_linear_drag_encodings_invisibles_undo");

    memory(hwnd, options, "abc\r\ndef", TextEncoding::Utf8)?;
    for message in [WM_CANCELMODE, WM_CAPTURECHANGED, WM_KILLFOCUS] {
        mouse(
            hwnd,
            WM_LBUTTONDOWN,
            pixel(hwnd, PaneId::Primary, 0, 0)?,
            Modifiers::NONE,
        );
        mouse(
            hwnd,
            WM_MOUSEMOVE,
            pixel(hwnd, PaneId::Primary, 0, 2)?,
            Modifiers::NONE,
        );
        let selected = selection(hwnd, PaneId::Primary);
        SendMessageW(hwnd, message, 0, 0);
        if !no_capture(hwnd) || selection(hwnd, PaneId::Primary) != selected {
            return Err("Capture interruption rewrote admitted linear selection".into());
        }
        if message == WM_KILLFOCUS {
            SendMessageW(hwnd, WM_SETFOCUS, 0, 0);
        }
    }
    mouse(
        hwnd,
        WM_LBUTTONDOWN,
        pixel(hwnd, PaneId::Primary, 1, 1)?,
        Modifiers::SHIFT,
    );
    mouse(
        hwnd,
        WM_LBUTTONUP,
        pixel(hwnd, PaneId::Primary, 1, 1)?,
        Modifiers::SHIFT,
    );
    if selection(hwnd, PaneId::Primary) != Some(0..6) {
        return Err("Shiftclick did not preserve source selection anchor".into());
    }
    for mutation in 0..6 {
        position(hwnd, 0, 0)?;
        mouse(
            hwnd,
            WM_LBUTTONDOWN,
            pixel(hwnd, PaneId::Primary, 0, 0)?,
            Modifiers::NONE,
        );
        match mutation {
            0 => {
                owner(hwnd)
                    .unwrap()
                    .state
                    .borrow_mut()
                    .workbench
                    .insert_text("Q")
                    .map_err(|e| e.to_string())?;
            }
            1 => {
                owner(hwnd)
                    .unwrap()
                    .state
                    .borrow_mut()
                    .workbench
                    .set_encoding(TextEncoding::Windows1252)
                    .map_err(|e| e.to_string())?;
            }
            2 => {
                let mut state = owner(hwnd).unwrap().state.borrow_mut();
                state.workbench.split_editor_right();
                state.workbench.focus_editor_pane(PaneId::Secondary);
            }
            3 => {
                owner(hwnd)
                    .unwrap()
                    .state
                    .borrow_mut()
                    .set_project_root(root.clone())?;
            }
            4 => command(hwnd, CommandId::ViewToggleInvisibles)?,
            _ => command(hwnd, CommandId::SearchFind)?,
        }
        let after = snapshot(hwnd)?;
        mouse(hwnd, WM_MOUSEMOVE, (900, 600), Modifiers::NONE);
        mouse(hwnd, WM_LBUTTONUP, (900, 600), Modifiers::NONE);
        if !no_capture(hwnd) || snapshot(hwnd)? != after {
            return Err("Stale linear drag overwrote a newer source/view/field intention".into());
        }
        {
            let mut state = owner(hwnd).unwrap().state.borrow_mut();
            state.close_selector();
            match mutation {
                0 => {
                    state.workbench.undo().map_err(|e| e.to_string())?;
                }
                1 => state
                    .workbench
                    .set_encoding(TextEncoding::Utf8)
                    .map_err(|e| e.to_string())?,
                2 => {
                    state.workbench.close_split_editor();
                }
                _ => {}
            }
        }
        if mutation == 4 {
            command(hwnd, CommandId::ViewToggleInvisibles)?;
        }
        expected(hwnd, b"abc\r\ndef")?;
    }
    position(hwnd, 0, 0)?;
    mouse(
        hwnd,
        WM_LBUTTONDOWN,
        pixel(hwnd, PaneId::Primary, 0, 0)?,
        Modifiers::NONE,
    );
    let selected = selection(hwnd, PaneId::Primary);
    resize.set(1400, 800)?;
    if !no_capture(hwnd) || selection(hwnd, PaneId::Primary) != selected {
        return Err("Resize failed to stop drag without changing selection".into());
    }
    resize.set(1600, 900)?;
    discard(hwnd)?;
    memory(hwnd, options, "a\tb\r\nx", TextEncoding::Utf8)?;
    mouse(
        hwnd,
        WM_LBUTTONDOWN,
        pixel(hwnd, PaneId::Primary, 0, 1)?,
        Modifiers::ALT,
    );
    mouse(
        hwnd,
        WM_MOUSEMOVE,
        pixel(hwnd, PaneId::Primary, 1, 3)?,
        Modifiers::ALT,
    );
    let rectangle = owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .rectangle_for_pane(PaneId::Primary)
        .ok_or("Alt drag no longer starts rectangle")?;
    SendMessageW(hwnd, WM_CANCELMODE, 0, 0);
    if !no_capture(hwnd)
        || owner(hwnd)
            .unwrap()
            .state
            .borrow()
            .workbench
            .rectangle_for_pane(PaneId::Primary)
            != Some(rectangle)
    {
        return Err("Linear input integration regressed rectangular capture loss".into());
    }
    command(hwnd, CommandId::ViewToggleInvisibles)?;
    let point = pixel(hwnd, PaneId::Primary, 1, 3)?;
    mouse(hwnd, WM_LBUTTONDOWN, point, Modifiers::NONE);
    let after = snapshot(hwnd)?;
    mouse(hwnd, WM_LBUTTONUP, point, Modifiers::NONE);
    if snapshot(hwnd)? != after || !no_capture(hwnd) {
        return Err(
            "MouseUp reinterpreted old rectangle coordinates after diagnostic projection switch"
                .into(),
        );
    }
    command(hwnd, CommandId::ViewToggleInvisibles)?;
    discard(hwnd)?;
    checks.push("interaction_drag_capture_interrupt_stale");

    let rows = (0..100).fold(String::new(), |mut text, line| {
        use std::fmt::Write;
        write!(&mut text, "line{line:02} niño\r\n").expect("String formatting");
        text
    });
    memory(hwnd, options, &rows, TextEncoding::Utf8)?;
    command(hwnd, CommandId::EditorSplitRight)?;
    let (rail_top, rail_bottom) = {
        let state = owner(hwnd).unwrap().state.borrow();
        let area = paint::text_areas(&state)
            .into_iter()
            .next()
            .ok_or("Sin geometría de desplazamiento")?;
        let text = area.geometry.text;
        (
            (text.right + 5, text.top + 2),
            (text.right + 5, text.bottom - 2),
        )
    };
    let caret_before_scroll = carets(hwnd);
    mouse(hwnd, WM_LBUTTONDOWN, rail_top, Modifiers::NONE);
    mouse(hwnd, WM_MOUSEMOVE, rail_bottom, Modifiers::NONE);
    if GetCapture() != hwnd || scrolls(hwnd).0 == 0 {
        return Err("El arrastre del editor perdió la captura o no desplazó la vista".into());
    }
    mouse(hwnd, WM_LBUTTONUP, rail_top, Modifiers::NONE);
    if !no_capture(hwnd) || scrolls(hwnd).0 != 0 || carets(hwnd) != caret_before_scroll {
        return Err("El arrastre no restauró la vista o movió el cursor".into());
    }
    checks.push("interaction_scroll_drag_keeps_capture_until_release");
    command(hwnd, CommandId::EditorFocusSecondary)?;
    position(hwnd, 20, 0)?;
    command(hwnd, CommandId::EditorTogglePin)?;
    command(hwnd, CommandId::EditorFocusPrimary)?;
    let before_carets = carets(hwnd);
    let rev = revision(hwnd);
    let before = scrolls(hwnd);
    SendMessageW(hwnd, WM_PAINT, 0, 0);
    let prepared_before_fraction = owner(hwnd).unwrap().frame_preparations.get();
    wheel(hwnd, PaneId::Secondary, -60, Modifiers::NONE)?;
    if scrolls(hwnd) != before {
        return Err("Half wheel notch scrolled a full step".into());
    }
    SendMessageW(hwnd, WM_PAINT, 0, 0);
    if owner(hwnd).unwrap().frame_preparations.get() != prepared_before_fraction {
        return Err("La fracción de rueda reconstruyó una vista sin desplazamiento".into());
    }
    wheel(hwnd, PaneId::Secondary, -60, Modifiers::NONE)?;
    if scrolls(hwnd) != (before.0, before.1 + 3) {
        return Err("Signed wheel fractions failed to accumulate on inactive pane".into());
    }
    wheel(hwnd, PaneId::Secondary, -240, Modifiers::NONE)?;
    wheel(hwnd, PaneId::Secondary, 120, Modifiers::NONE)?;
    let scrolled = scrolls(hwnd);
    if scrolled != (before.0, before.1 + 6)
        || carets(hwnd) != before_carets
        || owner(hwnd)
            .unwrap()
            .state
            .borrow()
            .workbench
            .panes()
            .active()
            != PaneId::Primary
    {
        return Err("Wheel changed caret/focus or signed scrolling".into());
    }
    wheel(hwnd, PaneId::Secondary, 60, Modifiers::NONE)?;
    wheel(hwnd, PaneId::Secondary, -60, Modifiers::NONE)?;
    for modifiers in [Modifiers::CTRL, Modifiers::SHIFT] {
        wheel(hwnd, PaneId::Secondary, -120, modifiers)?;
    }
    if scrolls(hwnd) != scrolled {
        return Err("Ctrl/Shift wheel changed source viewport".into());
    }
    command(hwnd, CommandId::EditorToggleScrollLink)?;
    wheel(hwnd, PaneId::Secondary, -120, Modifiers::NONE)?;
    if scrolls(hwnd) != (scrolled.0 + 3, scrolled.1 + 3) || carets(hwnd) != before_carets {
        return Err("Linked inactive wheel synchronized caret or lost delta".into());
    }
    let manual = scrolls(hwnd);
    {
        let mut state = owner(hwnd).unwrap().state.borrow_mut();
        paint::prepare(&mut state);
    }
    if scrolls(hwnd) != manual {
        return Err("Caret reveal erased manually scrolled viewport".into());
    }
    key(hwnd, 0x27, Modifiers::NONE)?;
    {
        let mut state = owner(hwnd).unwrap().state.borrow_mut();
        paint::prepare(&mut state);
    }
    if scrolls(hwnd).0 != 0 || carets(hwnd).1 != before_carets.1 {
        return Err("Caret movement failed to resume tracking or moved inactive caret".into());
    }
    mouse(
        hwnd,
        WM_LBUTTONDOWN,
        current_pixel(hwnd, PaneId::Primary, 0)?,
        Modifiers::NONE,
    );
    mouse(
        hwnd,
        WM_MOUSEMOVE,
        current_pixel(hwnd, PaneId::Primary, 2)?,
        Modifiers::NONE,
    );
    let selected = selection(hwnd, PaneId::Primary);
    wheel(hwnd, PaneId::Secondary, -120, Modifiers::NONE)?;
    if !no_capture(hwnd) || selection(hwnd, PaneId::Primary) != selected {
        return Err("Wheel interrupted drag by rewriting source selection".into());
    }
    mouse(
        hwnd,
        WM_LBUTTONDOWN,
        current_pixel(hwnd, PaneId::Primary, 0)?,
        Modifiers::ALT,
    );
    let rect = owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .rectangle_for_pane(PaneId::Primary);
    wheel(hwnd, PaneId::Secondary, -120, Modifiers::NONE)?;
    if rect.is_none()
        || !no_capture(hwnd)
        || owner(hwnd)
            .unwrap()
            .state
            .borrow()
            .workbench
            .rectangle_for_pane(PaneId::Primary)
            != rect
    {
        return Err("Wheel failed to stop rectangle capture while keeping last geometry".into());
    }
    command(hwnd, CommandId::EditRectangleCancel)?;
    if revision(hwnd) != rev
        || !owner(hwnd)
            .unwrap()
            .state
            .borrow()
            .workbench
            .panes()
            .view(PaneId::Secondary)
            .unwrap()
            .is_pinned()
    {
        return Err("Wheel modified source revision or coupled reference pin".into());
    }
    own_bytes(hwnd, rows.as_bytes())?;
    command(hwnd, CommandId::EditorOneGroup)?;
    discard(hwnd)?;
    checks.push("interaction_wheel_fraction_link_pin_focus");

    memory(hwnd, options, "abc\r\ndef", TextEncoding::Utf8)?;
    let specs = native_menu::definitions();
    let bar = GetMenu(hwnd);
    if bar.is_null() || GetMenuItemCount(bar) != specs.len() as i32 {
        return Err("Native menu bar diverges from shared typed definitions".into());
    }
    for (index, spec) in specs.iter().enumerate() {
        let popup = GetSubMenu(bar, index as i32);
        if popup.is_null() || GetMenuItemCount(popup) != spec.items.len() as i32 {
            return Err("Native menu group lost items/separators".into());
        }
        for (position, id) in spec.items.iter().enumerate() {
            let correct = if let Some(id) = id {
                GetMenuItemID(popup, position as i32) == *id as u32
            } else {
                let flags = GetMenuState(popup, position as u32, 0x400);
                flags != u32::MAX && flags & 0x800 != 0
            };
            if !correct {
                return Err(
                    "Native menu composition has an incorrect command identity/separator".into(),
                );
            }
        }
    }
    for code in [0x41, 0x45, 0x42, 0x50, 0x48, 0x56, 0x4e, 0x47] {
        owner(hwnd)
            .unwrap()
            .state
            .borrow_mut()
            .smoke_menu_choices
            .push_back(None);
        let before = snapshot(hwnd)?;
        key(hwnd, code, Modifiers::ALT)?;
        if snapshot(hwnd)? != before
            || owner(hwnd).unwrap().state.borrow().menu_stamp.is_some()
            || !owner(hwnd)
                .unwrap()
                .state
                .borrow()
                .smoke_menu_choices
                .is_empty()
        {
            return Err(
                "Alt native-menu cancellation changed source or retained target stamp".into(),
            );
        }
    }
    owner(hwnd)
        .unwrap()
        .state
        .borrow_mut()
        .smoke_menu_choices
        .push_back(None);
    key(hwnd, 0x79, Modifiers::NONE)?;
    for command_id in [
        CommandId::MenuFile,
        CommandId::MenuEdit,
        CommandId::MenuSearch,
        CommandId::MenuProject,
        CommandId::MenuTools,
        CommandId::MenuView,
    ] {
        let label = owner(hwnd)
            .unwrap()
            .state
            .borrow()
            .registry
            .get(command_id)
            .ok_or("Native menu macro missing from registry")?
            .label;
        owner(hwnd)
            .unwrap()
            .state
            .borrow_mut()
            .smoke_menu_choices
            .push_back(None);
        palette(hwnd, label)?;
        if owner(hwnd).unwrap().state.borrow().selector.is_some()
            || owner(hwnd).unwrap().state.borrow().menu_stamp.is_some()
            || !owner(hwnd)
                .unwrap()
                .state
                .borrow()
                .smoke_menu_choices
                .is_empty()
        {
            return Err("Palette menu macro did not invoke/cancel native group".into());
        }
    }
    for command_id in [CommandId::MenuBuild, CommandId::MenuHelp] {
        if super::supported(command_id) || native_menu::group_for_command(command_id).is_some() {
            return Err("Unsupported Build/Help acquired decorative native handler".into());
        }
    }
    menu_cancel_check(hwnd, Group::Edit, CommandId::EditIndent, true, Some("Tab"))?;
    owner(hwnd)
        .unwrap()
        .state
        .borrow_mut()
        .smoke_menu_choices
        .push_back(Some(command_id(CommandId::EditIndent)));
    let rev = revision(hwnd);
    key(hwnd, 0x45, Modifiers::ALT)?;
    cycle(hwnd, b"abc\r\ndef", b"    abc\r\ndef", rev)?;
    owner(hwnd)
        .unwrap()
        .state
        .borrow_mut()
        .smoke_menu_choices
        .push_back(Some(command_id(CommandId::PaletteOpen)));
    key(hwnd, 0x48, Modifiers::ALT)?;
    if owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .selector
        .as_ref()
        .map(|selector| selector.kind())
        != Some(SelectorKind::Palette)
    {
        return Err("Tools popup did not execute actual Palette handler".into());
    }
    paste(hwnd, "field")?;
    key(hwnd, 0x41, Modifiers::CTRL)?;
    menu_cancel_check(hwnd, Group::Edit, CommandId::EditCopy, true, Some("Ctrl"))?;
    native_menu::open_with(hwnd, Group::Edit, |_, _, _| command_id(CommandId::EditCopy))?;
    if owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .smoke_clipboard
        .as_deref()
        != Some("field")
    {
        return Err("Native Edit Copy did not target selector field".into());
    }
    native_menu::open_with(hwnd, Group::Edit, |_, _, _| command_id(CommandId::EditCut))?;
    if !input(hwnd)?.text.is_empty() {
        return Err("Native Edit Cut did not target selector field".into());
    }
    native_menu::open_with(hwnd, Group::Edit, |_, _, _| {
        command_id(CommandId::EditPaste)
    })?;
    if input(hwnd)?.text != "field" {
        return Err("Native Edit Paste did not restore selector field".into());
    }
    key(hwnd, 0x1b, Modifiers::NONE)?;
    expected(hwnd, b"abc\r\ndef")?;
    command(hwnd, CommandId::SearchFind)?;
    menu_cancel_check(hwnd, Group::Edit, CommandId::EditDuplicate, false, None)?;
    if owner(hwnd).unwrap().state.borrow().search_panel.is_none() {
        return Err("Native menu cancellation closed Find draft".into());
    }
    key(hwnd, 0x1b, Modifiers::NONE)?;
    command(hwnd, CommandId::ViewOutput)?;
    menu_cancel_check(hwnd, Group::Edit, CommandId::EditDuplicate, false, None)?;
    {
        let mut state = owner(hwnd).unwrap().state.borrow_mut();
        state.bottom.visible = false;
        state.bottom.focused = false;
    }
    discard(hwnd)?;
    checks.push("interaction_native_menu_context_macros_capabilities");

    memory(hwnd, options, "abc\r\ndef", TextEncoding::Utf8)?;
    for mutation in 0..6 {
        position(hwnd, 0, 0)?;
        let mut mutation_result = Ok(());
        let mut after = None;
        native_menu::open_with(hwnd, Group::Edit, |_, _, _| {
            mutation_result = (|| -> Result<(), String> {
                match mutation {
                    0 => {
                        owner(hwnd)
                            .unwrap()
                            .state
                            .borrow_mut()
                            .workbench
                            .insert_text("Q")
                            .map_err(|e| e.to_string())?;
                    }
                    1 => position(hwnd, 0, 1)?,
                    2 => owner(hwnd)
                        .unwrap()
                        .state
                        .borrow_mut()
                        .workbench
                        .set_encoding(TextEncoding::Windows1252)
                        .map_err(|e| e.to_string())?,
                    3 => {
                        let mut state = owner(hwnd).unwrap().state.borrow_mut();
                        state.workbench.split_editor_right();
                        state.workbench.focus_editor_pane(PaneId::Secondary);
                    }
                    4 => owner(hwnd)
                        .unwrap()
                        .state
                        .borrow_mut()
                        .set_project_root(root.clone())?,
                    _ => {
                        let mut state = owner(hwnd).unwrap().state.borrow_mut();
                        state.workbench.new_file(
                            options
                                .fixture
                                .with_file_name("interaction-other-memory.txt"),
                            TextEncoding::Utf8,
                        );
                        state
                            .workbench
                            .insert_text("other")
                            .map_err(|e| e.to_string())?;
                    }
                }
                after = Some(snapshot(hwnd)?);
                Ok(())
            })();
            command_id(CommandId::EditDuplicate)
        })?;
        mutation_result?;
        if after != Some(snapshot(hwnd)?) {
            return Err("Native popup replayed a command against changed source intention".into());
        }
        {
            let mut state = owner(hwnd).unwrap().state.borrow_mut();
            state.close_selector();
            match mutation {
                0 => {
                    state.workbench.undo().map_err(|e| e.to_string())?;
                }
                2 => state
                    .workbench
                    .set_encoding(TextEncoding::Utf8)
                    .map_err(|e| e.to_string())?,
                3 => {
                    state.workbench.close_split_editor();
                }
                5 => {
                    state
                        .workbench
                        .close_active(true)
                        .map_err(|e| e.to_string())?;
                }
                _ => {}
            }
        }
        expected(hwnd, b"abc\r\ndef")?;
    }
    for mutation in 0..3 {
        command(hwnd, CommandId::PaletteOpen)?;
        paste(hwnd, "seed")?;
        key(hwnd, 0x41, Modifiers::CTRL)?;
        let mut after = None;
        let mut mutation_result = Ok(());
        native_menu::open_with(hwnd, Group::Edit, |_, _, _| {
            mutation_result = (|| -> Result<(), String> {
                match mutation {
                    0 => key(hwnd, 0x27, Modifiers::NONE)?,
                    1 => {
                        SendMessageW(hwnd, WM_CHAR, 'X' as usize, 0);
                    }
                    _ => command(hwnd, CommandId::FileChooseEncoding)?,
                }
                after = Some(input(hwnd)?);
                Ok(())
            })();
            command_id(CommandId::EditCut)
        })?;
        mutation_result?;
        if after != Some(input(hwnd)?) {
            return Err("Native popup replayed Cut against a changed field/caret/kind".into());
        }
        key(hwnd, 0x1b, Modifiers::NONE)?;
        expected(hwnd, b"abc\r\ndef")?;
    }
    if native_menu::open_with(hwnd, Group::Edit, |_, _, _| command_id(CommandId::FileOpen)).is_ok()
    {
        return Err("Native popup accepted command from another group".into());
    }
    command(hwnd, CommandId::SearchFind)?;
    let before = snapshot(hwnd)?;
    if native_menu::open_with(hwnd, Group::Edit, |_, _, _| {
        command_id(CommandId::EditDuplicate)
    })
    .is_ok()
        || snapshot(hwnd)? != before
    {
        return Err("Disabled native menu command edited hidden source".into());
    }
    key(hwnd, 0x1b, Modifiers::NONE)?;
    discard(hwnd)?;
    checks.push("interaction_menu_reentry_stale_field_source");

    memory(hwnd, options, &rows, TextEncoding::Utf8)?;
    command(hwnd, CommandId::EditorSplitRight)?;
    command(hwnd, CommandId::EditorFocusSecondary)?;
    position(hwnd, 10, 0)?;
    command(hwnd, CommandId::EditorTogglePin)?;
    command(hwnd, CommandId::EditorFocusPrimary)?;
    command(hwnd, CommandId::EditorToggleScrollLink)?;
    wheel(hwnd, PaneId::Secondary, -120, Modifiers::NONE)?;
    for (width, height) in [(360, 260), (1400, 800), (360, 260), (1400, 800)] {
        resize.set(width, height)?;
        {
            let mut state = owner(hwnd).unwrap().state.borrow_mut();
            paint::prepare(&mut state);
        }
        let before = snapshot(hwnd)?;
        let before_carets = carets(hwnd);
        let before_scroll = scrolls(hwnd);
        let metrics = {
            let state = owner(hwnd).unwrap().state.borrow();
            (
                state.jobs.snapshot_bytes(),
                state.jobs.artifact_bytes(),
                state.deliveries,
            )
        };
        {
            let mut state = owner(hwnd).unwrap().state.borrow_mut();
            paint::prepare(&mut state);
            paint::prepare(&mut state);
            let first = state
                .workbench
                .panes()
                .view(state.workbench.panes().active())
                .unwrap()
                .scroll_line();
            let point = paint::rectangle_pixel(&state, state.workbench.panes().active(), first, 0)
                .ok_or("Compact source hit point unavailable")?;
            if paint::linear_hit(&state, point.0, point.1, None)?.is_none()
                || paint::source_pane_at(&state, point.0, point.1).is_none()
            {
                return Err("Compact source lost typed hit geometry".into());
            }
        }
        let state = owner(hwnd).unwrap().state.borrow();
        if snapshot(hwnd)? != before
            || carets(hwnd) != before_carets
            || scrolls(hwnd) != before_scroll
            || metrics
                != (
                    state.jobs.snapshot_bytes(),
                    state.jobs.artifact_bytes(),
                    state.deliveries,
                )
            || !state.workbench.panes().is_split()
            || !state.workbench.panes().scroll_linked()
            || !state
                .workbench
                .panes()
                .view(PaneId::Secondary)
                .unwrap()
                .is_pinned()
        {
            return Err("Readonly compact pointer preparation changed source/views/tasks".into());
        }
    }
    if let Some(bitmap) = &options.bitmap {
        let output = bitmap.with_file_name("native-interaction.bmp");
        if output.exists()
            || output == options.fixture
            || output == options.report
            || output == *bitmap
        {
            return Err("Interaction bitmap must be a new owned sibling".into());
        }
        let before = snapshot(hwnd)?;
        let frame = {
            let mut state = owner(hwnd).unwrap().state.borrow_mut();
            paint::prepare(&mut state)
        };
        paint::bitmap(hwnd, &frame, &output)?;
        if snapshot(hwnd)? != before {
            return Err("Interaction GDI raster modified editor state".into());
        }
    }
    own_bytes(hwnd, rows.as_bytes())?;
    command(hwnd, CommandId::EditorOneGroup)?;
    discard(hwnd)?;
    {
        let mut state = owner(hwnd).unwrap().state.borrow_mut();
        state.close_selector();
        state.explorer_visible = false;
        state.explorer_focused = false;
        state.explorer_hits = None;
        state.bottom.visible = false;
        state.bottom.focused = false;
        state.bottom_hits = None;
        state.smoke_wheel_lines = wheel_lines;
        if state.workbench.invisibles_visible() != initial_invisibles {
            state
                .workbench
                .execute(CommandId::ViewToggleInvisibles)
                .map_err(|e| e.to_string())?;
        }
    }
    position(hwnd, 0, 0)?;
    expected(hwnd, SOURCE)?;
    let after = snapshot(hwnd)?;
    if after.0 != baseline.0
        || after.1 != baseline.1
        || after.3 != baseline.3
        || owner(hwnd)
            .unwrap()
            .state
            .borrow()
            .workbench
            .active_encoding()
            != initial_encoding
        || !no_capture(hwnd)
        || fs::read(&options.fixture).map_err(|e| e.to_string())? != SOURCE
    {
        return Err(
            "Interaction cleanup changed Source13B identity/revision/encoding/catalog/disk".into(),
        );
    }
    drop(resize);
    checks.push("interaction_readonly_compact_cleanup");
    Ok(checks)
}
