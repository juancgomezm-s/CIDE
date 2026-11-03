//! Own-HWND navigation over a disposable in-memory document. The source fixture
//! on disk is only read for the final assertion, never edited or saved here.
use super::{
    ffi::*,
    owner, paint,
    project_smoke::snapshot,
    smoke::{command, expected, key, paste},
};
use crate::SmokeOptions;
use jc_app::{CommandId, Modifiers, PaneId, SearchStatus};
use jc_text::{TextEncoding, TextPosition};
use std::{
    fs,
    time::{Duration, Instant},
};

const SOURCE: &[u8] = b"alpha\r\nbeta\r\n";
const TEXT: &str = "\u{feff}αé\r\nβ\tZ\nlast\r";
unsafe fn palette(hwnd: Hwnd, label: &str) -> Result<(), String> {
    key(hwnd, 0x50, Modifiers::CTRL.union(Modifiers::SHIFT))?;
    for unit in label.encode_utf16() {
        SendMessageW(hwnd, WM_CHAR, unit as usize, 0);
    }
    key(hwnd, 0x0d, Modifiers::NONE)
}
unsafe fn dialog(hwnd: Hwnd, input: &str, entry: usize) -> Result<(), String> {
    match entry {
        0 => key(hwnd, 0x47, Modifiers::CTRL)?,
        1 => command(hwnd, CommandId::SearchGoToPosition)?,
        _ => palette(hwnd, "Go to Line / Column")?,
    }
    {
        let s = owner(hwnd)
            .ok_or("Navigation smoke lost HWND")?
            .state
            .borrow();
        let panel = s
            .navigation_panel
            .as_ref()
            .ok_or("Go to Position did not open")?;
        if panel.target.view.is_none() {
            return Err("Navigation target omitted its source ViewId".into());
        }
    }
    key(hwnd, 0x55, Modifiers::CTRL)?;
    paste(hwnd, input)
}
unsafe fn goto(hwnd: Hwnd, input: &str) -> Result<(), String> {
    dialog(hwnd, input, 0)?;
    key(hwnd, 0x0d, Modifiers::NONE)
}
unsafe fn position(hwnd: Hwnd, line: usize, column: usize) -> Result<(), String> {
    let s = owner(hwnd)
        .ok_or("Navigation smoke lost HWND")?
        .state
        .borrow();
    if s.workbench.active_position() != Some(TextPosition { line, column }) {
        return Err("Navigation did not use expected source scalar coordinates".into());
    }
    Ok(())
}
unsafe fn search_complete(hwnd: Hwnd) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let mut message: Msg = std::mem::zeroed();
        while PeekMessageW(&mut message, hwnd, TASKS_MESSAGE, TASKS_MESSAGE, 1) != 0 {
            DispatchMessageW(&message);
            if Instant::now() >= deadline {
                return Err("Navigation search task drain exceeded deadline".into());
            }
        }
        let s = owner(hwnd)
            .ok_or("Navigation smoke lost HWND")?
            .state
            .borrow();
        if !s.workbench.search_running() {
            return if s.workbench.search().status == SearchStatus::Complete {
                Ok(())
            } else {
                Err("Navigation history search did not complete".into())
            };
        }
        drop(s);
        if Instant::now() >= deadline {
            return Err("Navigation search delivery deadline exceeded".into());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}
unsafe fn search_flags(hwnd: Hwnd, sensitive: bool, whole: bool) -> Result<(), String> {
    let current = {
        let s = owner(hwnd)
            .ok_or("Navigation smoke lost HWND")?
            .state
            .borrow();
        (
            s.workbench.search().case_sensitive,
            s.workbench.search().whole_word,
        )
    };
    if current.0 != sensitive {
        key(hwnd, 0x43, Modifiers::ALT)?;
    }
    if current.1 != whole {
        key(hwnd, 0x57, Modifiers::ALT)?;
    }
    Ok(())
}
unsafe fn history_panel(
    hwnd: Hwnd,
    query: &str,
    sensitive: bool,
    whole: bool,
) -> Result<(), String> {
    let s = owner(hwnd)
        .ok_or("Navigation smoke lost HWND")?
        .state
        .borrow();
    let p = s
        .search_panel
        .as_ref()
        .ok_or("History lost Replace panel")?;
    if p.query != query
        || p.query_cursor != query.len()
        || p.replacement != "R\r\nΩ"
        || p.replacement_cursor != "R\r\nΩ".len()
        || !p.replace
        || !p.replacement_active
        || s.workbench.search().case_sensitive != sensitive
        || s.workbench.search().whole_word != whole
    {
        return Err("Search history lost exact query/flags/cursors or replacement draft".into());
    }
    Ok(())
}
unsafe fn stale_rejected(
    hwnd: Hwnd,
    before: (jc_app::DocumentId, u64, String, usize),
) -> Result<(), String> {
    key(hwnd, 0x0d, Modifiers::NONE)?;
    if snapshot(hwnd)? != before
        || owner(hwnd)
            .unwrap()
            .state
            .borrow()
            .navigation_panel
            .is_some()
    {
        return Err("Stale navigation target moved a caret or remained active".into());
    }
    Ok(())
}

pub(super) unsafe fn run(hwnd: Hwnd, options: &SmokeOptions) -> Result<Vec<&'static str>, String> {
    expected(hwnd, SOURCE)?;
    let split = owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .panes()
        .is_split();
    if split {
        command(hwnd, CommandId::EditorOneGroup)?;
        expected(hwnd, SOURCE)?;
    }
    let gates = {
        let s = owner(hwnd).unwrap().state.borrow();
        (
            s.workbench.search().history.is_empty(),
            !s.workbench.context().nav_back_available,
        )
    };
    if gates.0 || gates.1 {
        dialog(hwnd, "1:2", 1)?;
        let before = snapshot(hwnd)?;
        if gates.0 {
            command(hwnd, CommandId::SearchHistoryPrevious)?;
        }
        if gates.1 {
            command(hwnd, CommandId::NavigateBack)?;
            key(hwnd, 0x25, Modifiers::ALT)?;
        }
        let s = owner(hwnd).unwrap().state.borrow();
        if s.navigation_panel
            .as_ref()
            .map(|p| (p.input.as_str(), p.cursor))
            != Some(("1:2", 3))
        {
            return Err("Disabled history command closed a navigation draft".into());
        }
        drop(s);
        if snapshot(hwnd)? != before {
            return Err("Disabled history command changed caret or jump history".into());
        }
        key(hwnd, 0x1b, Modifiers::NONE)?;
    }
    command(hwnd, CommandId::FileNew)?;
    paste(hwnd, TEXT)?;
    expected(hwnd, TEXT.as_bytes())?;
    let document = snapshot(hwnd)?.0;
    let mut checks = Vec::new();
    let initial_invisibles = owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .invisibles_visible();
    if !initial_invisibles {
        command(hwnd, CommandId::ViewToggleInvisibles)?;
    }
    for entry in 0..3 {
        let before = snapshot(hwnd)?;
        dialog(hwnd, "2:3", entry)?;
        if snapshot(hwnd)? != before {
            return Err("Go to Position previewed before explicit acceptance".into());
        }
        if entry == 0 {
            if let Some(bitmap) = &options.bitmap {
                let output = bitmap.with_file_name("native-navigation.bmp");
                if output.exists() || output == options.fixture || output == options.report {
                    return Err("Navigation bitmap must be a new owned sibling".into());
                }
                let frame = {
                    let mut s = owner(hwnd).unwrap().state.borrow_mut();
                    paint::prepare(&mut s)
                };
                paint::bitmap(hwnd, &frame, &output)?;
                if snapshot(hwnd)? != before {
                    return Err("Navigation GDI preparation changed source or jump history".into());
                }
            }
        }
        key(hwnd, 0x0d, Modifiers::NONE)?;
        position(hwnd, 1, 2)?;
        let after = snapshot(hwnd)?;
        if after.0 != document
            || after.1 != before.1
            || owner(hwnd)
                .unwrap()
                .state
                .borrow()
                .workbench
                .active_editor()
                .unwrap()
                .cursor
                != 12
        {
            return Err("Go to Position mixed BOM/byte/display/scalar coordinates".into());
        }
        // A duplicate late dialog submit is not a second ordinary Editor Enter.
        owner(hwnd)
            .unwrap()
            .state
            .borrow_mut()
            .navigation_key(0x0d, Modifiers::NONE);
        if snapshot(hwnd)? != after {
            return Err("Closed Go to Position accepted the same submit twice".into());
        }
        expected(hwnd, TEXT.as_bytes())?;
    }
    checks.push("navigation_menu_palette_shortcut_source_scalars_bom_crlf_invisibles");

    for invalid in [
        "0:0",
        "1:0",
        "1:2:3",
        "99:1",
        "1:99",
        "184467440737095516160",
        "1\n2",
    ] {
        let before = snapshot(hwnd)?;
        dialog(hwnd, invalid, 0)?;
        key(hwnd, 0x0d, Modifiers::NONE)?;
        if snapshot(hwnd)? != before
            || owner(hwnd)
                .unwrap()
                .state
                .borrow()
                .navigation_panel
                .is_none()
        {
            return Err("Invalid Go to Position navigated or closed the draft".into());
        }
        key(hwnd, 0x1b, Modifiers::NONE)?;
        if snapshot(hwnd)? != before {
            return Err("Cancelled Go to Position changed navigation history".into());
        }
    }
    dialog(hwnd, "1:2", 0)?;
    let draft = {
        let s = owner(hwnd).unwrap().state.borrow();
        let p = s.navigation_panel.as_ref().unwrap();
        (p.input.clone(), p.cursor, p.generation)
    };
    paste(hwnd, &"9".repeat(65))?;
    {
        let s = owner(hwnd).unwrap().state.borrow();
        let p = s.navigation_panel.as_ref().unwrap();
        if p.input != draft.0 || p.cursor != draft.1 || p.input.len() > 64 {
            return Err("Oversized Go to Position paste was not rejected atomically".into());
        }
    }
    key(hwnd, 0x1b, Modifiers::NONE)?;
    checks.push("navigation_invalid_cancel_atomic_field_limit_no_live_preview");

    goto(hwnd, "1:2")?;
    goto(hwnd, "3:2")?;
    key(hwnd, 0x25, Modifiers::ALT)?;
    position(hwnd, 0, 1)?;
    command(hwnd, CommandId::NavigateForward)?;
    position(hwnd, 2, 1)?;
    command(hwnd, CommandId::EditorSplitRight)?;
    command(hwnd, CommandId::EditorFocusSecondary)?;
    goto(hwnd, "1:2")?;
    command(hwnd, CommandId::EditorTogglePin)?;
    goto(hwnd, "2:2")?;
    position(hwnd, 1, 1)?;
    {
        let s = owner(hwnd).unwrap().state.borrow();
        if s.workbench.panes().active() != PaneId::Primary
            || s.workbench.position_for_pane(PaneId::Secondary)
                != Some(TextPosition { line: 0, column: 1 })
        {
            return Err("GoTo from pinned reference did not route to the other group".into());
        }
    }
    key(hwnd, 0x25, Modifiers::ALT)?;
    position(hwnd, 2, 1)?;
    {
        let s = owner(hwnd).unwrap().state.borrow();
        if s.workbench.position_for_pane(PaneId::Primary)
            != Some(TextPosition { line: 2, column: 1 })
            || s.workbench.position_for_pane(PaneId::Secondary)
                != Some(TextPosition { line: 0, column: 1 })
            || !s
                .workbench
                .panes()
                .view(PaneId::Secondary)
                .unwrap()
                .is_pinned()
        {
            return Err("Navigation Back changed another group or unpinned reference".into());
        }
    }
    key(hwnd, 0x27, Modifiers::ALT)?;
    position(hwnd, 1, 1)?;
    command(hwnd, CommandId::EditorFocusSecondary)?;
    command(hwnd, CommandId::NavigateBack)?;
    position(hwnd, 2, 1)?;
    if owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .position_for_pane(PaneId::Primary)
        != Some(TextPosition { line: 1, column: 1 })
    {
        return Err("Reference history altered primary caret".into());
    }
    key(hwnd, 0x27, Modifiers::ALT)?;
    position(hwnd, 0, 1)?;
    if owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .position_for_pane(PaneId::Primary)
        != Some(TextPosition { line: 1, column: 1 })
    {
        return Err("Reference Forward altered primary caret".into());
    }
    command(hwnd, CommandId::EditorFocusSecondary)?;
    command(hwnd, CommandId::EditorTogglePin)?;
    command(hwnd, CommandId::EditorFocusPrimary)?;
    command(hwnd, CommandId::EditorOneGroup)?;
    checks.push("navigation_group_local_back_forward_preserves_pin_and_shared_document");

    goto(hwnd, "1:1")?;
    dialog(hwnd, "2:2", 0)?;
    owner(hwnd)
        .unwrap()
        .state
        .borrow_mut()
        .workbench
        .insert_text("X")
        .map_err(|e| e.to_string())?;
    stale_rejected(hwnd, snapshot(hwnd)?)?;
    owner(hwnd)
        .unwrap()
        .state
        .borrow_mut()
        .workbench
        .undo()
        .map_err(|e| e.to_string())?;
    expected(hwnd, TEXT.as_bytes())?;
    dialog(hwnd, "2:2", 0)?;
    owner(hwnd)
        .unwrap()
        .state
        .borrow_mut()
        .workbench
        .set_encoding(TextEncoding::Windows1252)
        .map_err(|e| e.to_string())?;
    stale_rejected(hwnd, snapshot(hwnd)?)?;
    owner(hwnd)
        .unwrap()
        .state
        .borrow_mut()
        .workbench
        .set_encoding(TextEncoding::Utf8)
        .map_err(|e| e.to_string())?;
    goto(hwnd, "1:1")?;
    dialog(hwnd, "3:2", 0)?;
    {
        let mut s = owner(hwnd).unwrap().state.borrow_mut();
        s.workbench.split_editor_right();
        s.workbench.focus_editor_pane(PaneId::Secondary);
    }
    stale_rejected(hwnd, snapshot(hwnd)?)?;
    command(hwnd, CommandId::EditorFocusPrimary)?;
    command(hwnd, CommandId::EditorOneGroup)?;
    checks.push("navigation_revision_encoding_view_pane_stale_targets_rejected");

    let a = "αé\r\nβ";
    let b = "last\r";
    command(hwnd, CommandId::SearchFind)?;
    key(hwnd, 0x55, Modifiers::CTRL)?;
    paste(hwnd, a)?;
    search_flags(hwnd, true, true)?;
    search_complete(hwnd)?;
    key(hwnd, 0x72, Modifiers::NONE)?;
    key(hwnd, 0x1b, Modifiers::NONE)?;
    command(hwnd, CommandId::SearchFind)?;
    key(hwnd, 0x55, Modifiers::CTRL)?;
    paste(hwnd, b)?;
    search_flags(hwnd, false, false)?;
    search_complete(hwnd)?;
    key(hwnd, 0x72, Modifiers::NONE)?;
    key(hwnd, 0x1b, Modifiers::NONE)?;
    command(hwnd, CommandId::SearchReplace)?;
    key(hwnd, 0x09, Modifiers::NONE)?;
    paste(hwnd, "R\r\nΩ")?;
    for (code, query, sensitive, word) in [
        (0x26, b, false, false),
        (0x26, a, true, true),
        (0x28, b, false, false),
    ] {
        key(hwnd, code, Modifiers::ALT)?;
        search_complete(hwnd)?;
        history_panel(hwnd, query, sensitive, word)?;
    }
    key(hwnd, 0x50, Modifiers::CTRL.union(Modifiers::SHIFT))?;
    key(hwnd, 0x1b, Modifiers::NONE)?;
    history_panel(hwnd, b, false, false)?;
    palette(hwnd, "Previous Search Query")?;
    search_complete(hwnd)?;
    history_panel(hwnd, a, true, true)?;
    command(hwnd, CommandId::SearchHistoryNext)?;
    search_complete(hwnd)?;
    history_panel(hwnd, b, false, false)?;
    key(hwnd, 0x1b, Modifiers::NONE)?;
    command(hwnd, CommandId::SearchHistoryPrevious)?;
    search_complete(hwnd)?;
    {
        let s = owner(hwnd).unwrap().state.borrow();
        let p = s
            .search_panel
            .as_ref()
            .ok_or("History menu without panel did not open Find")?;
        if p.replace || p.query != a {
            return Err("History menu opened an incorrect Find query".into());
        }
    }
    key(hwnd, 0x1b, Modifiers::NONE)?;
    expected(hwnd, TEXT.as_bytes())?;
    checks.push("navigation_multiline_history_flags_replace_draft_menu_palette_preserved");

    for accept in [false, true] {
        goto(hwnd, "1:1")?;
        command(hwnd, CommandId::SearchFind)?;
        key(hwnd, 0x55, Modifiers::CTRL)?;
        paste(hwnd, "last")?;
        search_flags(hwnd, false, false)?;
        key(hwnd, 0x72, Modifiers::NONE)?;
        dialog(hwnd, "2:2", 0)?;
        key(hwnd, if accept { 0x0d } else { 0x1b }, Modifiers::NONE)?;
        search_complete(hwnd)?;
        position(hwnd, if accept { 1 } else { 0 }, if accept { 1 } else { 0 })?;
        if owner(hwnd)
            .unwrap()
            .state
            .borrow()
            .workbench
            .active_editor()
            .unwrap()
            .selection()
            .is_some()
        {
            return Err("Late Find completion selected an obsolete navigation destination".into());
        }
    }
    checks.push("navigation_modal_cancel_accept_disarm_deferred_find_without_stopping_scan");

    key(hwnd, 0x5a, Modifiers::CTRL)?;
    expected(hwnd, b"")?;
    key(hwnd, 0x59, Modifiers::CTRL)?;
    expected(hwnd, TEXT.as_bytes())?;
    if !initial_invisibles {
        command(hwnd, CommandId::ViewToggleInvisibles)?;
    }
    dialog(hwnd, "2:2", 0)?;
    owner(hwnd)
        .unwrap()
        .state
        .borrow_mut()
        .workbench
        .close_active(true)
        .map_err(|e| e.to_string())?;
    expected(hwnd, SOURCE)?;
    stale_rejected(hwnd, snapshot(hwnd)?)?;
    if fs::read(&options.fixture).map_err(|e| e.to_string())? != SOURCE {
        return Err("Navigation smoke changed original disk fixture".into());
    }
    checks.push("navigation_close_stale_target_original_source_and_undo_preserved");
    Ok(checks)
}
