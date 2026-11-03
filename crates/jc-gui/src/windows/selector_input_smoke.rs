//! Own-HWND selector input and synthetic clipboard handoffs. Never accesses the
//! OS clipboard or saves a source; Quick Open enumerates only its owned root.
use super::{
    clipboard_flow,
    ffi::*,
    owner, paint,
    project_smoke::snapshot,
    selector::SelectorKind,
    smoke::{command, expected, key, paste, position},
};
use crate::SmokeOptions;
use jc_app::{quick_open::QuickOpenState, CommandId, FieldSelection, Modifiers, PaneId};
use jc_text::TextEncoding;
use std::{
    fs,
    time::{Duration, Instant},
};
const SOURCE: &[u8] = b"alpha\r\nbeta\r\n";
const DIRTY: &[u8] = b"Qalpha\r\nbeta\r\n";
fn shared_views(app: &jc_app::Workbench, tab: jc_app::TabId) -> usize {
    [PaneId::Primary, PaneId::Secondary]
        .into_iter()
        .filter(|pane| {
            app.panes()
                .view(*pane)
                .and_then(|view| view.location())
                .is_some_and(|location| location.tab == tab)
        })
        .count()
}
#[derive(Debug, PartialEq, Eq)]
struct Input {
    text: String,
    selection: FieldSelection,
    revision: u64,
    candidate: usize,
    kind: SelectorKind,
}
unsafe fn input(hwnd: Hwnd) -> Result<Input, String> {
    let state = owner(hwnd)
        .ok_or("Selector input smoke lost HWND")?
        .state
        .borrow();
    let selector = state
        .selector
        .as_ref()
        .ok_or("Selector input unexpectedly closed")?;
    Ok(Input {
        text: selector.input().to_owned(),
        selection: selector.field_selection(),
        revision: selector.input_revision(),
        candidate: selector.selected(),
        kind: selector.kind(),
    })
}
unsafe fn text(hwnd: Hwnd, value: &str) -> Result<(), String> {
    if input(hwnd)?.text != value {
        return Err("Selector field differs from exact synthetic input".into());
    }
    Ok(())
}
unsafe fn open(hwnd: Hwnd, kind: SelectorKind) -> Result<(), String> {
    command(
        hwnd,
        match kind {
            SelectorKind::Palette => CommandId::PaletteOpen,
            SelectorKind::Files => CommandId::FileQuickOpen,
            SelectorKind::Encoding => CommandId::FileChooseEncoding,
            SelectorKind::OpenTabs => {
                return Err("La lista de documentos se abre desde su botón gráfico".into())
            }
        },
    )?;
    let state = input(hwnd)?;
    if state.kind != kind
        || !state.text.is_empty()
        || state.selection != FieldSelection::default()
        || state.candidate != 0
    {
        return Err("Fresh selector reused query, selection or candidate intention".into());
    }
    Ok(())
}
unsafe fn memory(hwnd: Hwnd, options: &SmokeOptions) -> Result<(), String> {
    let mut state = owner(hwnd).unwrap().state.borrow_mut();
    state.close_selector();
    state.explorer_visible = false;
    state.explorer_focused = false;
    state.bottom.focused = false;
    state.bottom.visible = false;
    state.workbench.new_file(
        options.fixture.with_file_name("selector-input-memory.txt"),
        TextEncoding::Utf8,
    );
    state
        .workbench
        .insert_text("abc")
        .map_err(|e| e.to_string())?;
    Ok(())
}
unsafe fn discard(hwnd: Hwnd) -> Result<(), String> {
    let mut state = owner(hwnd).unwrap().state.borrow_mut();
    state.close_selector();
    state.explorer_visible = false;
    state.explorer_focused = false;
    state.bottom.focused = false;
    state.bottom.visible = false;
    state
        .workbench
        .close_active(true)
        .map_err(|e| e.to_string())?;
    drop(state);
    expected(hwnd, SOURCE)
}
unsafe fn complete(hwnd: Hwnd) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let state = owner(hwnd).unwrap().state.borrow().quick_open.state();
        if state == QuickOpenState::Complete {
            return Ok(());
        }
        if !matches!(state, QuickOpenState::Queued | QuickOpenState::Running) {
            return Err(format!("Selector metadata request ended as {state:?}"));
        }
        if Instant::now() >= deadline {
            return Err("Selector metadata notification deadline exceeded".into());
        }
        let mut message: Msg = std::mem::zeroed();
        if PeekMessageW(&mut message, hwnd, TASKS_MESSAGE, TASKS_MESSAGE, 1) != 0 {
            DispatchMessageW(&message);
        } else {
            std::thread::yield_now();
        }
    }
}
unsafe fn own_tasks(hwnd: Hwnd) -> Result<(), String> {
    for _ in 0..128 {
        let mut message: Msg = std::mem::zeroed();
        if PeekMessageW(&mut message, hwnd, TASKS_MESSAGE, TASKS_MESSAGE, 1) == 0 {
            return Ok(());
        }
        DispatchMessageW(&message);
    }
    Err("Owned selector task pump did not quiesce within128messages".into())
}
unsafe fn file_query(hwnd: Hwnd, name: &str) -> Result<(), String> {
    open(hwnd, SelectorKind::Files)?;
    paste(hwnd, name)?;
    complete(hwnd)
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
            return Err("Selector own-window geometry unavailable".into());
        }
        Ok(Self { hwnd, rect })
    }
    unsafe fn set(&self, width: i32, height: i32) -> Result<(), String> {
        if SetWindowPos(self.hwnd, std::ptr::null_mut(), 0, 0, width, height, 0x16) == 0 {
            return Err("Selector own-window resize failed".into());
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
    let root = options
        .fixture
        .parent()
        .ok_or("Selector fixture has no parent")?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    {
        let mut state = owner(hwnd).unwrap().state.borrow_mut();
        state.close_selector();
        state.explorer_visible = false;
        state.explorer_focused = false;
        state.bottom.visible = false;
        state.bottom.focused = false;
        state.set_project_root(root.clone())?;
    }
    position(hwnd, 0, 0)?;
    let baseline = snapshot(hwnd)?;
    let source_encoding = owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .active_encoding();
    let source_tab = owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .active_document()
        .unwrap()
        .tab_id;
    let history = {
        let state = owner(hwnd).unwrap().state.borrow();
        let doc = state
            .workbench
            .active_document()
            .unwrap()
            .session
            .document();
        (doc.can_undo(), doc.can_redo())
    };
    let mut checks = Vec::new();
    for kind in [
        SelectorKind::Palette,
        SelectorKind::Files,
        SelectorKind::Encoding,
    ] {
        open(hwnd, kind)?;
        let original = "añ😀e\u{301}\"q\"";
        paste(hwnd, original)?;
        key(hwnd, 0x24, Modifiers::NONE)?;
        key(hwnd, 0x27, Modifiers::NONE)?;
        key(hwnd, 0x27, Modifiers::SHIFT)?;
        key(hwnd, 0x27, Modifiers::SHIFT)?;
        if input(hwnd)?.selection.range() != Some(1..7) {
            return Err("Selector Shift arrows split UTF8 source coordinates".into());
        }
        command(hwnd, CommandId::EditCopy)?;
        if owner(hwnd)
            .unwrap()
            .state
            .borrow()
            .smoke_clipboard
            .as_deref()
            != Some("ñ😀")
        {
            return Err("Selector menu Copy lost exact Unicode selection".into());
        }
        command(hwnd, CommandId::EditCut)?;
        text(hwnd, "ae\u{301}\"q\"")?;
        command(hwnd, CommandId::EditPaste)?;
        text(hwnd, original)?;
        command(hwnd, CommandId::EditSelectAll)?;
        paste(hwnd, "\"niño\"")?;
        text(hwnd, "\"niño\"")?;
        key(hwnd, 0x41, Modifiers::CTRL)?;
        key(hwnd, 0x43, Modifiers::CTRL)?;
        key(hwnd, 0x58, Modifiers::CTRL)?;
        text(hwnd, "")?;
        key(hwnd, 0x56, Modifiers::CTRL)?;
        text(hwnd, "\"niño\"")?;
        let before = input(hwnd)?;
        paste(hwnd, "")?;
        if input(hwnd)? != before {
            return Err("Empty selector clipboard changed input intention".into());
        }
        key(hwnd, 0x55, Modifiers::CTRL)?;
        if input(hwnd)?.selection != FieldSelection::default()
            || !input(hwnd)?.text.is_empty()
            || input(hwnd)?.candidate != 0
        {
            return Err("CtrlU failed to clear selector field and candidate".into());
        }
        paste(hwnd, "az")?;
        key(hwnd, 0x24, Modifiers::NONE)?;
        key(hwnd, 0x27, Modifiers::NONE)?;
        key(hwnd, 0x2e, Modifiers::NONE)?;
        text(hwnd, "a")?;
        key(hwnd, 0x08, Modifiers::NONE)?;
        text(hwnd, "")?;
        if kind == SelectorKind::Encoding {
            key(hwnd, 0x28, Modifiers::NONE)?;
            key(hwnd, 0x09, Modifiers::NONE)?;
            if input(hwnd)?.candidate != 2 {
                return Err("Selector results arrows/Tab did not navigate candidates".into());
            }
            key(hwnd, 0x09, Modifiers::SHIFT)?;
            if input(hwnd)?.candidate != 1 {
                return Err("ShiftTab did not reverse selector result navigation".into());
            }
            key(hwnd, 0x23, Modifiers::CTRL)?;
            if input(hwnd)?.candidate != 3 {
                return Err("CtrlEnd did not choose the last encoding result".into());
            }
            key(hwnd, 0x24, Modifiers::CTRL)?;
            if input(hwnd)?.candidate != 0 {
                return Err("CtrlHome did not choose the first encoding result".into());
            }
        }
        key(hwnd, 0x1b, Modifiers::NONE)?;
    }
    if snapshot(hwnd)? != baseline {
        return Err("Selector clipboard edited source bytes, views or revision".into());
    }
    {
        let state = owner(hwnd).unwrap().state.borrow();
        let source = state
            .workbench
            .active_document()
            .unwrap()
            .session
            .document();
        if (source.can_undo(), source.can_redo()) != history {
            return Err("Selector field edits created source undo history".into());
        }
    }
    checks.push("selector_input_field_edit_clipboard_menu_atomic");

    open(hwnd, SelectorKind::Palette)?;
    paste(hwnd, &"ñ".repeat(128))?;
    if input(hwnd)?.text.len() != 256 {
        return Err("Exact256B selector input was rejected".into());
    }
    key(hwnd, 0x41, Modifiers::CTRL)?;
    paste(hwnd, &"x".repeat(256))?;
    if input(hwnd)?.text != "x".repeat(256) {
        return Err("Exact quota replacement incorrectly added old selection bytes".into());
    }
    key(hwnd, 0x25, Modifiers::NONE)?;
    key(hwnd, 0x25, Modifiers::SHIFT)?;
    for rejected in [
        "ñ".to_owned(),
        "x".repeat(257),
        "\n".into(),
        "\r\n".into(),
        "\t".into(),
        "\0".into(),
        "a\rb".into(),
        "\u{202e}".into(),
        "\u{2066}".into(),
    ] {
        let before = input(hwnd)?;
        paste(hwnd, &rejected)?;
        if input(hwnd)? != before {
            return Err(
                "Rejected selector paste changed query/caret/anchor/revision/candidate".into(),
            );
        }
    }
    let target = owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .selector_input_target()
        .ok_or("Selector target missing")?;
    let before = input(hwnd)?;
    clipboard_flow::selector_transfer_with(hwnd, CommandId::EditPaste, target, |export| {
        if export.is_some() {
            return Err("Paste transport unexpectedly received export".into());
        }
        Ok(None)
    })?;
    if input(hwnd)? != before {
        return Err("Unavailable clipboard mutated selector selection".into());
    }
    key(hwnd, 0x1b, Modifiers::NONE)?;
    open(hwnd, SelectorKind::Encoding)?;
    key(hwnd, 0x1b, Modifiers::NONE)?;
    checks.push("selector_input_limits_unicode_controls_no_partial");

    let file_name = options
        .fixture
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("Owned selector fixture name must be Unicode")?;
    position(hwnd, 0, 0)?;
    SendMessageW(hwnd, WM_CHAR, 'Q' as usize, 0);
    expected(hwnd, DIRTY)?;
    open(hwnd, SelectorKind::Files)?;
    paste(hwnd, "no-owned-match")?;
    key(hwnd, 0x41, Modifiers::CTRL)?;
    paste(hwnd, file_name)?;
    complete(hwnd)?;
    if owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .quick_open
        .results()
        .len()
        != 1
    {
        return Err("QuickOpen did not deliver exact owned fixture candidate".into());
    }
    paste(hwnd, "-absent")?;
    let before = snapshot(hwnd)?;
    key(hwnd, 0x0d, Modifiers::NONE)?;
    if snapshot(hwnd)? != before || owner(hwnd).unwrap().state.borrow().selector.is_none() {
        return Err("QuickOpen opened stale candidate after query edit".into());
    }
    key(hwnd, 0x41, Modifiers::CTRL)?;
    paste(hwnd, file_name)?;
    complete(hwnd)?;
    key(hwnd, 0x0d, Modifiers::NONE)?;
    expected(hwnd, DIRTY)?;
    if snapshot(hwnd)?.0 != baseline.0 || snapshot(hwnd)?.3 != baseline.3 {
        return Err("QuickOpen dirty reuse created a second document/catalog entry".into());
    }
    file_query(hwnd, file_name)?;
    key(hwnd, 0x0d, Modifiers::CTRL)?;
    {
        let state = owner(hwnd).unwrap().state.borrow();
        if state.workbench.active_document().unwrap().id != baseline.0
            || shared_views(&state.workbench, source_tab) != 2
        {
            return Err(
                "QuickOpen CtrlEnter cloned source or failed to open shared side view".into(),
            );
        }
    }
    command(hwnd, CommandId::EditorTogglePin)?;
    file_query(hwnd, file_name)?;
    key(hwnd, 0x0d, Modifiers::CTRL)?;
    {
        let state = owner(hwnd).unwrap().state.borrow();
        if state.workbench.active_document().unwrap().id != baseline.0
            || !state
                .workbench
                .panes()
                .view(PaneId::Secondary)
                .unwrap()
                .is_pinned()
            || shared_views(&state.workbench, source_tab) != 2
        {
            return Err("QuickOpen side navigation overwrote pin or duplicated ownership".into());
        }
    }
    command(hwnd, CommandId::EditorOneGroup)?;
    key(hwnd, 0x5a, Modifiers::CTRL)?;
    expected(hwnd, SOURCE)?;
    if fs::read(&options.fixture).map_err(|e| e.to_string())? != SOURCE {
        return Err("QuickOpen dirty reuse wrote source disk".into());
    }
    checks.push("selector_input_quick_open_versioned_reuse_side_pin");

    memory(hwnd, options)?;
    for mutation in 0..9 {
        open(hwnd, SelectorKind::Palette)?;
        let query = if mutation == 8 { "Pestaña" } else { "seed" };
        paste(hwnd, query)?;
        key(hwnd, 0x41, Modifiers::CTRL)?;
        let target = owner(hwnd)
            .unwrap()
            .state
            .borrow()
            .selector_input_target()
            .ok_or("Reentrant selector target missing")?;
        let result = clipboard_flow::selector_transfer_with(
            hwnd,
            if mutation == 1 {
                CommandId::EditPaste
            } else {
                CommandId::EditCut
            },
            target,
            |export| {
                if mutation != 1 && export != Some(query) {
                    return Err("Cut did not export exact selected field".into());
                }
                match mutation {
                    0 => key(hwnd, 0x27, Modifiers::NONE)?,
                    1 => {
                        SendMessageW(hwnd, WM_CHAR, 'X' as usize, 0);
                    }
                    2 => command(hwnd, CommandId::FileChooseEncoding)?,
                    3 => {
                        owner(hwnd)
                            .unwrap()
                            .state
                            .borrow_mut()
                            .workbench
                            .insert_text("Q")
                            .map_err(|e| e.to_string())?;
                    }
                    4 => {
                        let mut state = owner(hwnd).unwrap().state.borrow_mut();
                        state.workbench.split_editor_right();
                        state.workbench.focus_editor_pane(PaneId::Secondary);
                    }
                    5 => {
                        owner(hwnd)
                            .unwrap()
                            .state
                            .borrow_mut()
                            .workbench
                            .set_encoding(TextEncoding::Windows1252)
                            .map_err(|e| e.to_string())?;
                    }
                    6 => {
                        owner(hwnd)
                            .unwrap()
                            .state
                            .borrow_mut()
                            .set_project_root(root.clone())?;
                    }
                    7 => {
                        key(hwnd, 0x1b, Modifiers::NONE)?;
                        open(hwnd, SelectorKind::Palette)?;
                        paste(hwnd, "seed")?;
                        key(hwnd, 0x41, Modifiers::CTRL)?;
                    }
                    _ => key(hwnd, 0x28, Modifiers::NONE)?,
                }
                Ok(Some("LATE".into()))
            },
        );
        if result.is_ok() {
            return Err(
                "Selector accepted a clipboard result after reentrant target change".into(),
            );
        }
        match mutation {
            1 => text(hwnd, "X")?,
            2 => text(hwnd, "")?,
            6 => {
                if owner(hwnd).unwrap().state.borrow().selector.is_some() {
                    return Err("Root change revived selector".into());
                }
            }
            8 => {
                text(hwnd, "Tab")?;
                if input(hwnd)?.candidate == 0 {
                    return Err("Reentrant candidate setup did not move to an existing row".into());
                }
            }
            _ => text(hwnd, "seed")?,
        }
        {
            let mut state = owner(hwnd).unwrap().state.borrow_mut();
            state.close_selector();
            match mutation {
                3 => {
                    state.workbench.undo().map_err(|e| e.to_string())?;
                }
                4 => {
                    state.workbench.close_split_editor();
                }
                5 => state
                    .workbench
                    .set_encoding(TextEncoding::Utf8)
                    .map_err(|e| e.to_string())?,
                _ => {}
            }
        }
        expected(hwnd, b"abc")?;
    }
    discard(hwnd)?;
    checks.push("selector_input_reentrant_clipboard_target_discard");

    memory(hwnd, options)?;
    command(hwnd, CommandId::SearchReplace)?;
    paste(hwnd, "abc")?;
    key(hwnd, 0x09, Modifiers::NONE)?;
    paste(hwnd, "replacement")?;
    open(hwnd, SelectorKind::Palette)?;
    paste(
        hwnd,
        jc_app::default_registry()
            .get(CommandId::EditDuplicate)
            .unwrap()
            .label,
    )?;
    let before = snapshot(hwnd)?;
    key(hwnd, 0x0d, Modifiers::NONE)?;
    if snapshot(hwnd)? != before || owner(hwnd).unwrap().state.borrow().selector.is_none() {
        return Err("Palette invoked from Replace edited hidden source".into());
    }
    key(hwnd, 0x1b, Modifiers::NONE)?;
    {
        let state = owner(hwnd).unwrap().state.borrow();
        let panel = state
            .search_panel
            .as_ref()
            .ok_or("Palette Escape discarded Replace draft")?;
        if panel.replacement != "replacement" || !panel.replacement_active {
            return Err("Palette field editing lost replacement draft/focus".into());
        }
    }
    key(hwnd, 0x1b, Modifiers::NONE)?;
    command(hwnd, CommandId::ViewExplorer)?;
    for kind in [
        SelectorKind::Files,
        SelectorKind::Encoding,
        SelectorKind::Palette,
    ] {
        open(hwnd, kind)?;
        if kind == SelectorKind::Palette {
            paste(
                hwnd,
                jc_app::default_registry()
                    .get(CommandId::EditIndent)
                    .unwrap()
                    .label,
            )?;
            key(hwnd, 0x0d, Modifiers::NONE)?;
            expected(hwnd, b"abc")?;
        }
        key(hwnd, 0x1b, Modifiers::NONE)?;
        if !owner(hwnd).unwrap().state.borrow().explorer_focused {
            return Err("Selector Escape failed to restore Explorer invoker".into());
        }
    }
    command(hwnd, CommandId::ViewExplorer)?;
    command(hwnd, CommandId::ViewOutput)?;
    open(hwnd, SelectorKind::Palette)?;
    paste(
        hwnd,
        jc_app::default_registry()
            .get(CommandId::EditIndent)
            .unwrap()
            .label,
    )?;
    let before = snapshot(hwnd)?;
    key(hwnd, 0x0d, Modifiers::NONE)?;
    if snapshot(hwnd)? != before || owner(hwnd).unwrap().state.borrow().selector.is_none() {
        return Err("Bottom-invoked Palette edited hidden source".into());
    }
    key(hwnd, 0x1b, Modifiers::NONE)?;
    if !owner(hwnd).unwrap().state.borrow().bottom.focused {
        return Err("Palette Escape lost bottom-panel focus".into());
    }
    let before = snapshot(hwnd)?;
    open(hwnd, SelectorKind::Encoding)?;
    paste(hwnd, TextEncoding::Windows1252.name())?;
    key(hwnd, 0x0d, Modifiers::NONE)?;
    {
        let state = owner(hwnd).unwrap().state.borrow();
        if state.workbench.active_encoding() != Some(TextEncoding::Windows1252)
            || state.bottom.focused
            || state.selector.is_some()
        {
            return Err(
                "Encoding acceptance failed to return to editor with explicit interpretation"
                    .into(),
            );
        }
    }
    expected(hwnd, b"abc")?;
    let after = snapshot(hwnd)?;
    if after.0 != before.0 || after.1 != before.1 || after.3 != before.3 {
        return Err("Encoding selector rewrote bytes/revision/catalog".into());
    }
    open(hwnd, SelectorKind::Palette)?;
    paste(hwnd, "ñ😀e\u{301}")?;
    text(hwnd, "ñ😀e\u{301}")?;
    expected(hwnd, b"abc")?;
    key(hwnd, 0x1b, Modifiers::NONE)?;
    if owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .active_encoding()
        != Some(TextEncoding::Windows1252)
    {
        return Err("Unicode selector input changed explicit source interpretation".into());
    }
    open(hwnd, SelectorKind::Encoding)?;
    paste(hwnd, TextEncoding::Utf8.name())?;
    key(hwnd, 0x0d, Modifiers::NONE)?;
    discard(hwnd)?;
    checks.push("selector_input_modal_origin_palette_encoding_focus");

    file_query(hwnd, file_name)?;
    key(hwnd, 0x25, Modifiers::SHIFT)?;
    key(hwnd, 0x25, Modifiers::SHIFT)?;
    let resize = Resize::new(hwnd)?;
    for (width, height) in [(360, 260), (1400, 800), (360, 260), (1400, 800)] {
        resize.set(width, height)?;
        {
            let mut state = owner(hwnd).unwrap().state.borrow_mut();
            paint::prepare(&mut state);
        }
        let before = snapshot(hwnd)?;
        let field = input(hwnd)?;
        let (target, deliveries, metrics) = {
            let state = owner(hwnd).unwrap().state.borrow();
            (
                state.selector_input_target(),
                state.deliveries,
                (state.jobs.snapshot_bytes(), state.jobs.artifact_bytes()),
            )
        };
        {
            let mut state = owner(hwnd).unwrap().state.borrow_mut();
            paint::prepare(&mut state);
            paint::prepare(&mut state);
        }
        let state = owner(hwnd).unwrap().state.borrow();
        if snapshot(hwnd)? != before
            || input(hwnd)? != field
            || target != state.selector_input_target()
            || deliveries != state.deliveries
            || metrics != (state.jobs.snapshot_bytes(), state.jobs.artifact_bytes())
        {
            return Err(
                "Selector readonly resize/compose mutated input/source or consumed jobs".into(),
            );
        }
    }
    if let Some(bitmap) = &options.bitmap {
        let output = bitmap.with_file_name("native-selector-input.bmp");
        if output.exists()
            || output == options.fixture
            || output == options.report
            || output == *bitmap
        {
            return Err("Selector bitmap must be a new owned sibling".into());
        }
        let before = snapshot(hwnd)?;
        let field = input(hwnd)?;
        let frame = {
            let mut state = owner(hwnd).unwrap().state.borrow_mut();
            paint::prepare(&mut state)
        };
        paint::bitmap(hwnd, &frame, &output)?;
        if snapshot(hwnd)? != before || input(hwnd)? != field {
            return Err("Selector GDI paint changed input/source".into());
        }
    }
    drop(resize);
    key(hwnd, 0x1b, Modifiers::NONE)?;
    owner(hwnd)
        .unwrap()
        .state
        .borrow_mut()
        .set_project_root(root)?;
    own_tasks(hwnd)?;
    {
        let mut state = owner(hwnd).unwrap().state.borrow_mut();
        if state.selector.is_some() {
            return Err("Late selector completion revived closed input".into());
        }
        state.explorer_visible = false;
        state.explorer_focused = false;
        state.explorer_hits = None;
        state.bottom.visible = false;
        state.bottom.focused = false;
        state.bottom_hits = None;
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
            != source_encoding
        || fs::read(&options.fixture).map_err(|e| e.to_string())? != SOURCE
    {
        return Err(
            "Selector input cleanup changed Source13B identity/revision/encoding/catalog/disk"
                .into(),
        );
    }
    checks.push("selector_input_readonly_resize_cleanup");
    Ok(checks)
}
