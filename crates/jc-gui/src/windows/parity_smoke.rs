//! Own-HWND routes for existing shared indentation, line and viewport commands.
//! Memory fixtures only; no OS clipboard, Save or physical display assertions.
use super::{
    ffi::*,
    owner, paint,
    project_smoke::snapshot,
    smoke::{command, expected, key, position},
};
use crate::SmokeOptions;
use jc_app::{CommandId, Modifiers, PaneId};
use jc_text::{encode, TextEncoding, TextPosition};
use std::fs;

const SOURCE: &[u8] = b"alpha\r\nbeta\r\n";
const ENCODINGS: [TextEncoding; 4] = [
    TextEncoding::Utf8,
    TextEncoding::Windows1252,
    TextEncoding::Iso8859_1,
    TextEncoding::Ibm850,
];
unsafe fn memory(
    hwnd: Hwnd,
    options: &SmokeOptions,
    text: &str,
    encoding: TextEncoding,
) -> Result<(), String> {
    let path = options
        .fixture
        .parent()
        .ok_or("Parity fixture has no parent")?
        .join("parity-memory.txt");
    let mut state = owner(hwnd)
        .ok_or("Parity smoke lost HWND")?
        .state
        .borrow_mut();
    state.close_selector();
    state.explorer_visible = false;
    state.explorer_focused = false;
    state.explorer_hits = None;
    state.workbench.new_file(path, encoding);
    state
        .workbench
        .insert_text(text)
        .map_err(|e| e.to_string())?;
    state
        .workbench
        .go_to_position(TextPosition { line: 0, column: 0 })
        .map_err(|e| e.to_string())
}
unsafe fn discard(hwnd: Hwnd) -> Result<(), String> {
    owner(hwnd)
        .ok_or("Parity smoke lost HWND")?
        .state
        .borrow_mut()
        .workbench
        .close_active(true)
        .map_err(|e| e.to_string())?;
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
unsafe fn cycle(hwnd: Hwnd, before: &[u8], after: &[u8], rev: u64) -> Result<(), String> {
    expected(hwnd, after)?;
    if revision(hwnd) == rev {
        return Err("Parity edit did not publish a changed revision".into());
    }
    key(hwnd, 0x5a, Modifiers::CTRL)?;
    expected(hwnd, before)?;
    key(hwnd, 0x59, Modifiers::CTRL)?;
    expected(hwnd, after)?;
    key(hwnd, 0x5a, Modifiers::CTRL)?;
    expected(hwnd, before)
}
unsafe fn palette_open(hwnd: Hwnd, label: &str) -> Result<(), String> {
    key(hwnd, 0x50, Modifiers::CTRL.union(Modifiers::SHIFT))?;
    for unit in label.encode_utf16() {
        SendMessageW(hwnd, WM_CHAR, unit as usize, 0);
    }
    Ok(())
}
unsafe fn palette(hwnd: Hwnd, label: &str) -> Result<(), String> {
    palette_open(hwnd, label)?;
    key(hwnd, 0x0d, Modifiers::NONE)
}
unsafe fn linked(hwnd: Hwnd) -> bool {
    owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .panes()
        .scroll_linked()
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
unsafe fn carets(hwnd: Hwnd) -> (Option<TextPosition>, Option<TextPosition>) {
    let state = owner(hwnd).unwrap().state.borrow();
    (
        state.workbench.position_for_pane(PaneId::Primary),
        state.workbench.position_for_pane(PaneId::Secondary),
    )
}
unsafe fn reveal(hwnd: Hwnd, line: usize) -> Result<(), String> {
    let mut state = owner(hwnd).unwrap().state.borrow_mut();
    state
        .workbench
        .go_to_position(TextPosition { line, column: 0 })
        .map_err(|e| e.to_string())?;
    state.workbench.reveal_active_pane(5, 40);
    Ok(())
}
unsafe fn owned_bytes(hwnd: Hwnd, bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > 4096 {
        return Err("Parity own fixture exceeded 4 KiB".into());
    }
    let state = owner(hwnd).unwrap().state.borrow();
    let source = state
        .workbench
        .active_document()
        .ok_or("Parity document closed")?
        .session
        .document();
    if source.len() != bytes.len()
        || source
            .read_range(0..source.len())
            .map_err(|e| e.to_string())?
            != bytes
    {
        return Err("Parity memory fixture bytes changed".into());
    }
    Ok(())
}
#[link(name = "user32")]
extern "system" {
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
}
struct Resize {
    hwnd: Hwnd,
    rect: Rect,
}
impl Resize {
    unsafe fn new(hwnd: Hwnd) -> Result<Self, String> {
        let mut rect = Rect::default();
        if GetWindowRect(hwnd, &mut rect) == 0 {
            return Err("Parity own-window rectangle unavailable".into());
        }
        Ok(Self { hwnd, rect })
    }
    unsafe fn set(&self, width: i32, height: i32) -> Result<(), String> {
        if SetWindowPos(self.hwnd, std::ptr::null_mut(), 0, 0, width, height, 0x16) == 0 {
            return Err("Parity own-window resize failed".into());
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

pub(super) unsafe fn run(hwnd: Hwnd, options: &SmokeOptions) -> Result<Vec<&'static str>, String> {
    expected(hwnd, SOURCE)?;
    command(hwnd, CommandId::EditorOneGroup)?;
    let baseline = snapshot(hwnd)?;
    let mut checks = Vec::new();
    for encoding in ENCODINGS {
        let bom = if encoding == TextEncoding::Utf8 {
            "\u{feff}"
        } else {
            ""
        };
        let original = format!("{bom}ñ\r\n x\nlast\r");
        let indented = format!("{bom}    ñ\r\n     x\n    last\r");
        let original = encode(&original, encoding).map_err(|e| e.to_string())?;
        let indented = encode(&indented, encoding).map_err(|e| e.to_string())?;
        // SelectAll excludes the initial BOM, while indent/outdent preserve it.
        memory(hwnd, options, &format!("{bom}ñ\r\n x\nlast\r"), encoding)?;
        key(hwnd, 0x41, Modifiers::CTRL)?;
        let rev = revision(hwnd);
        key(hwnd, 0x09, Modifiers::NONE)?;
        cycle(hwnd, &original, &indented, rev)?;
        key(hwnd, 0x41, Modifiers::CTRL)?;
        command(hwnd, CommandId::EditIndent)?;
        expected(hwnd, &indented)?;
        key(hwnd, 0x41, Modifiers::CTRL)?;
        let rev = revision(hwnd);
        key(hwnd, 0x09, Modifiers::SHIFT)?;
        cycle(hwnd, &indented, &original, rev)?;
        key(hwnd, 0x41, Modifiers::CTRL)?;
        let rev = revision(hwnd);
        palette(hwnd, "Outdent")?;
        cycle(hwnd, &indented, &original, rev)?;
        key(hwnd, 0x5a, Modifiers::CTRL)?;
        expected(hwnd, &original)?;
        discard(hwnd)?;
    }
    checks.push("parity_indent_outdent_four_encodings_undo");

    let edits = [
        CommandId::EditIndent,
        CommandId::EditOutdent,
        CommandId::EditJoinLines,
        CommandId::EditSplitLine,
    ];
    memory(hwnd, options, "one\r\n two", TextEncoding::Utf8)?;
    command(hwnd, CommandId::SearchReplace)?;
    let before = snapshot(hwnd)?;
    for edit in edits {
        command(hwnd, edit)?;
    }
    if snapshot(hwnd)? != before || owner(hwnd).unwrap().state.borrow().search_panel.is_none() {
        return Err("Parity menu edited or closed a non-editor field".into());
    }
    palette_open(hwnd, "Indent")?;
    key(hwnd, 0x0d, Modifiers::NONE)?;
    if snapshot(hwnd)? != before || owner(hwnd).unwrap().state.borrow().selector.is_none() {
        return Err("Palette from Replace edited hidden source".into());
    }
    key(hwnd, 0x1b, Modifiers::NONE)?;
    key(hwnd, 0x1b, Modifiers::NONE)?;
    command(hwnd, CommandId::ViewExplorer)?;
    let before = snapshot(hwnd)?;
    for edit in edits {
        command(hwnd, edit)?;
    }
    if snapshot(hwnd)? != before || !owner(hwnd).unwrap().state.borrow().explorer_focused {
        return Err("Parity menu edited behind Explorer or stole focus".into());
    }
    command(hwnd, CommandId::ViewExplorer)?;
    position(hwnd, 0, 0)?;
    palette_open(hwnd, "Indent")?;
    position(hwnd, 0, 1)?;
    let before = snapshot(hwnd)?;
    key(hwnd, 0x0d, Modifiers::NONE)?;
    if snapshot(hwnd)? != before || owner(hwnd).unwrap().state.borrow().selector.is_none() {
        return Err("Parity palette accepted stale caret intention".into());
    }
    key(hwnd, 0x1b, Modifiers::NONE)?;
    expected(hwnd, b"one\r\n two")?;
    discard(hwnd)?;
    checks.push("parity_edit_modal_explorer_stale_palette");

    memory(hwnd, options, "one\r\n two\nlast", TextEncoding::Utf8)?;
    command(hwnd, CommandId::EditRectangleStart)?;
    command(hwnd, CommandId::EditRectangleRight)?;
    command(hwnd, CommandId::EditRectangleDown)?;
    let bounds = owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .rectangle_for_pane(PaneId::Primary);
    let before = snapshot(hwnd)?;
    for edit in [
        CommandId::EditOutdent,
        CommandId::EditJoinLines,
        CommandId::EditSplitLine,
    ] {
        command(hwnd, edit)?;
    }
    if snapshot(hwnd)? != before
        || owner(hwnd)
            .unwrap()
            .state
            .borrow()
            .workbench
            .rectangle_for_pane(PaneId::Primary)
            != bounds
    {
        return Err("Linear parity command changed active rectangle".into());
    }
    let rev = revision(hwnd);
    command(hwnd, CommandId::EditIndent)?;
    cycle(hwnd, b"one\r\n two\nlast", b"    ne\r\n    two\nlast", rev)?;
    position(hwnd, 0, 0)?;
    let rev = revision(hwnd);
    command(hwnd, CommandId::EditJoinLines)?;
    cycle(hwnd, b"one\r\n two\nlast", b"one two\nlast", rev)?;
    position(hwnd, 0, 3)?;
    let rev = revision(hwnd);
    key(hwnd, 0x0d, Modifiers::NONE)?;
    cycle(hwnd, b"one\r\n two\nlast", b"one\r\n\r\n two\nlast", rev)?;
    position(hwnd, 0, 3)?;
    let rev = revision(hwnd);
    palette(hwnd, "Split Line")?;
    cycle(hwnd, b"one\r\n two\nlast", b"one\r\n\r\n two\nlast", rev)?;
    discard(hwnd)?;
    checks.push("parity_join_split_focus_and_rectangle");

    let text = "line\r\n".repeat(80);
    memory(hwnd, options, &text, TextEncoding::Utf8)?;
    let document = snapshot(hwnd)?.0;
    let rev = revision(hwnd);
    command(hwnd, CommandId::EditorSplitRight)?;
    command(hwnd, CommandId::EditorFocusSecondary)?;
    command(hwnd, CommandId::EditorTogglePin)?;
    command(hwnd, CommandId::EditorFocusPrimary)?;
    let caret_before = carets(hwnd);
    key(hwnd, 0x4c, Modifiers::CTRL.union(Modifiers::ALT))?;
    if !linked(hwnd) {
        return Err("CtrlAltL did not enable shared scroll linkage".into());
    }
    reveal(hwnd, 50)?;
    let first = scrolls(hwnd);
    if first.0 == 0 || first.0 != first.1 || carets(hwnd).1 != caret_before.1 {
        return Err("Linked viewport delta changed caret or failed to reach other group".into());
    }
    command(hwnd, CommandId::EditorToggleScrollLink)?;
    if linked(hwnd) {
        return Err("Native View menu did not unlink scroll".into());
    }
    reveal(hwnd, 60)?;
    let unlinked = scrolls(hwnd);
    if unlinked.0 <= first.0 || unlinked.1 != first.1 {
        return Err("Unlinked viewport still moved the inactive group".into());
    }
    palette(hwnd, "Link / Unlink Scroll")?;
    if !linked(hwnd) {
        return Err("Palette did not restore shared scroll linkage".into());
    }
    reveal(hwnd, 65)?;
    let relinked = scrolls(hwnd);
    if relinked.0 - unlinked.0 != relinked.1 - unlinked.1 || carets(hwnd).1 != caret_before.1 {
        return Err(
            "Relinking synchronized caret/absolute viewport instead of applying delta".into(),
        );
    }
    {
        let state = owner(hwnd).unwrap().state.borrow();
        if !state
            .workbench
            .panes()
            .view(PaneId::Secondary)
            .unwrap()
            .is_pinned()
            || state.workbench.active_document().unwrap().id != document
            || revision(hwnd) != rev
        {
            return Err("Scroll link changed pin, document identity or source revision".into());
        }
    }
    owned_bytes(hwnd, text.as_bytes())?;
    checks.push("parity_scroll_link_keyboard_menu_palette_pin");

    let resize = Resize::new(hwnd)?;
    let caret_before = carets(hwnd);
    for (width, height) in [(360, 260), (1400, 800), (360, 260), (1400, 800)] {
        resize.set(width, height)?;
        // Reveal can update viewport before composition; stabilize it first.
        {
            let mut state = owner(hwnd).unwrap().state.borrow_mut();
            paint::prepare(&mut state);
        }
        let before = snapshot(hwnd)?;
        let (frame, metrics) = {
            let mut state = owner(hwnd).unwrap().state.borrow_mut();
            let metrics = (state.jobs.snapshot_bytes(), state.jobs.artifact_bytes());
            (paint::prepare(&mut state), metrics)
        };
        if !frame.contains_status("Scroll linked") || !linked(hwnd) || carets(hwnd) != caret_before
        {
            return Err("Compact presentation dropped link or synchronized carets".into());
        }
        let state = owner(hwnd).unwrap().state.borrow();
        if snapshot(hwnd)? != before
            || metrics != (state.jobs.snapshot_bytes(), state.jobs.artifact_bytes())
            || !state.workbench.panes().is_split()
            || !state
                .workbench
                .panes()
                .view(PaneId::Secondary)
                .unwrap()
                .is_pinned()
        {
            return Err(
                "Repeated parity composition mutated split/pin or admitted background work".into(),
            );
        }
    }
    if let Some(bitmap) = &options.bitmap {
        let output = bitmap.with_file_name("native-parity.bmp");
        if output.exists()
            || output == options.fixture
            || output == options.report
            || output == *bitmap
        {
            return Err("Parity bitmap must be a new owned sibling".into());
        }
        let before = snapshot(hwnd)?;
        let frame = {
            let mut state = owner(hwnd).unwrap().state.borrow_mut();
            paint::prepare(&mut state)
        };
        paint::bitmap(hwnd, &frame, &output)?;
        if snapshot(hwnd)? != before {
            return Err("Parity GDI raster changed source/views".into());
        }
    }
    drop(resize);
    owned_bytes(hwnd, text.as_bytes())?;
    command(hwnd, CommandId::EditorOneGroup)?;
    if linked(hwnd) {
        return Err("Closing split retained a scroll link without a second group".into());
    }
    {
        let mut state = owner(hwnd).unwrap().state.borrow_mut();
        if paint::prepare(&mut state).contains_status("Scroll linked") {
            return Err("Single-group frame retained stale linked indicator".into());
        }
    }
    discard(hwnd)?;
    expected(hwnd, SOURCE)?;
    let after = snapshot(hwnd)?;
    if after.0 != baseline.0
        || after.1 != baseline.1
        || after.3 != baseline.3
        || fs::read(&options.fixture).map_err(|e| e.to_string())? != SOURCE
    {
        return Err("Parity cleanup changed original source/catalog or disk".into());
    }
    checks.push("parity_scroll_link_resize_readonly_cleanup");
    Ok(checks)
}
