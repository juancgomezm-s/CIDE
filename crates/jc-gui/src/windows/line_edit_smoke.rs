//! Own-HWND routing for existing Workbench edits. All new documents are memory
//! sessions discarded explicitly; no profile source is compiled or saved.
use super::{
    ffi::*,
    owner, paint,
    project_smoke::snapshot,
    smoke::{command, expected, key, paste, position},
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
const EDITS: [CommandId; 6] = [
    CommandId::EditDuplicate,
    CommandId::EditDeleteLine,
    CommandId::EditMoveLineUp,
    CommandId::EditMoveLineDown,
    CommandId::EditToggleLineComment,
    CommandId::EditToggleBlockComment,
];
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
unsafe fn memory(
    hwnd: Hwnd,
    options: &SmokeOptions,
    name: &str,
    encoding: TextEncoding,
    text: &str,
) -> Result<(), String> {
    let path = options
        .fixture
        .parent()
        .ok_or("Line-edit fixture has no parent")?
        .join(name);
    {
        let mut s = owner(hwnd)
            .ok_or("Line-edit smoke lost HWND")?
            .state
            .borrow_mut();
        s.close_selector();
        s.explorer_visible = false;
        s.explorer_focused = false;
        s.explorer_hits = None;
        s.workbench.new_file(path, encoding);
    }
    paste(hwnd, text)?;
    position(hwnd, 0, 0)?;
    expected(hwnd, &encode(text, encoding).map_err(|e| e.to_string())?)
}
unsafe fn discard(hwnd: Hwnd) -> Result<(), String> {
    owner(hwnd)
        .ok_or("Line-edit smoke lost HWND")?
        .state
        .borrow_mut()
        .workbench
        .close_active(true)
        .map_err(|e| e.to_string())?;
    expected(hwnd, SOURCE)
}
unsafe fn cycle(hwnd: Hwnd, original: &[u8], changed: &[u8], before: u64) -> Result<(), String> {
    expected(hwnd, changed)?;
    if revision(hwnd) == before {
        return Err("Line edit did not publish a changed revision".into());
    }
    key(hwnd, 0x5a, Modifiers::CTRL)?;
    expected(hwnd, original)?;
    key(hwnd, 0x59, Modifiers::CTRL)?;
    expected(hwnd, changed)?;
    key(hwnd, 0x5a, Modifiers::CTRL)?;
    expected(hwnd, original)
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
fn encoded(text: &str, encoding: TextEncoding) -> Result<Vec<u8>, String> {
    encode(text, encoding).map_err(|e| e.to_string())
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
    let baseline = {
        let s = owner(hwnd).unwrap().state.borrow();
        let d = s.workbench.active_document().unwrap();
        if d.session.document().is_modified() {
            return Err("Line-edit smoke requires its original clean source fixture".into());
        }
        (
            d.id,
            s.workbench
                .panes()
                .view(s.workbench.panes().active())
                .and_then(|v| v.view_id()),
        )
    };
    let mut checks = Vec::new();
    let text = "\u{feff}αx\r\nlast";
    memory(
        hwnd,
        options,
        "line-edit-duplicate.txt",
        TextEncoding::Utf8,
        text,
    )?;
    let original = text.as_bytes();
    key(hwnd, 0x27, Modifiers::SHIFT)?;
    let before = revision(hwnd);
    key(hwnd, 0x44, Modifiers::CTRL)?;
    if owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .active_editor()
        .unwrap()
        .selection()
        != Some(5..7)
    {
        return Err("Duplicate selection did not select its copied raw scalar bytes".into());
    }
    cycle(hwnd, original, "\u{feff}ααx\r\nlast".as_bytes(), before)?;
    position(hwnd, 1, 2)?;
    let before = revision(hwnd);
    command(hwnd, CommandId::EditDuplicate)?;
    if owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .active_position()
        != Some(TextPosition { line: 2, column: 2 })
    {
        return Err("Duplicate EOF line lost caret scalar column".into());
    }
    cycle(
        hwnd,
        original,
        "\u{feff}αx\r\nlast\r\nlast".as_bytes(),
        before,
    )?;
    discard(hwnd)?;
    checks.push("line_edit_duplicate_selection_line_bom_one_undo");

    let text = "one\r\ntwo\nlast";
    memory(
        hwnd,
        options,
        "line-edit-move.txt",
        TextEncoding::Utf8,
        text,
    )?;
    position(hwnd, 2, 1)?;
    let before = revision(hwnd);
    key(hwnd, 0x44, Modifiers::CTRL.union(Modifiers::SHIFT))?;
    cycle(hwnd, text.as_bytes(), b"one\r\ntwo", before)?;
    position(hwnd, 0, 1)?;
    key(hwnd, 0x28, Modifiers::SHIFT)?;
    let before = revision(hwnd);
    palette(
        hwnd,
        jc_app::default_registry()
            .get(CommandId::EditMoveLineDown)
            .unwrap()
            .label,
    )?;
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
        return Err("Move Line Down retained a multiline anchor".into());
    }
    cycle(hwnd, text.as_bytes(), b"one\r\nlast\ntwo", before)?;
    position(hwnd, 0, 1)?;
    key(hwnd, 0x28, Modifiers::SHIFT)?;
    let before = revision(hwnd);
    key(hwnd, 0x26, Modifiers::ALT)?;
    cycle(hwnd, text.as_bytes(), b"two\none\r\nlast", before)?;
    position(hwnd, 0, 0)?;
    let before = revision(hwnd);
    key(hwnd, 0x26, Modifiers::ALT)?;
    position(hwnd, 2, 0)?;
    key(hwnd, 0x28, Modifiers::ALT)?;
    if revision(hwnd) != before {
        return Err("Move Line at a nonterminated document edge added an undo edit".into());
    }
    expected(hwnd, text.as_bytes())?;
    discard(hwnd)?;
    memory(
        hwnd,
        options,
        "line-edit-eof.txt",
        TextEncoding::Utf8,
        "one\r\n",
    )?;
    position(hwnd, 0, 1)?;
    let before = revision(hwnd);
    command(hwnd, CommandId::EditMoveLineDown)?;
    // The existing engine counts the synthetic empty EOF record. The separator
    // moves between the empty and real content; this is not a no-op.
    cycle(hwnd, b"one\r\n", b"\r\none", before)?;
    discard(hwnd)?;
    checks.push("line_edit_delete_move_caret_selection_eof_contract");

    for encoding in ENCODINGS {
        let bom = if encoding == TextEncoding::Utf8 {
            "\u{feff}"
        } else {
            ""
        };
        for (name, commented) in [
            ("line-edit-profile.p", "  /* niño */\r\nfin\nlast\r"),
            ("line-edit-profile.cs", "  // niño\r\nfin\nlast\r"),
            ("line-edit-profile.py", "  # niño\r\nfin\nlast\r"),
        ] {
            let text = format!("{bom}  niño\r\nfin\nlast\r");
            let original = encoded(&text, encoding)?;
            memory(hwnd, options, name, encoding, &text)?;
            position(hwnd, 0, 2)?;
            let before = revision(hwnd);
            key(hwnd, 0xbf, Modifiers::CTRL)?;
            if owner(hwnd)
                .unwrap()
                .state
                .borrow()
                .workbench
                .active_encoding()
                != Some(encoding)
            {
                return Err("Profile comment changed explicit source encoding".into());
            }
            cycle(
                hwnd,
                &original,
                &encoded(&format!("{bom}{commented}"), encoding)?,
                before,
            )?;
            discard(hwnd)?;
        }
    }
    checks.push("line_edit_profile_comments_four_encodings_exact_eol");

    let text = "\u{feff}  niño\r\nfin\nlast\r";
    memory(hwnd, options, "line-edit-block.p", TextEncoding::Utf8, text)?;
    key(hwnd, 0x41, Modifiers::CTRL)?;
    let before = revision(hwnd);
    key(hwnd, 0xbf, Modifiers::CTRL.union(Modifiers::SHIFT))?;
    if owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .active_editor()
        .unwrap()
        .selection()
        .is_none()
    {
        return Err("Block comment lost the selected block".into());
    }
    cycle(
        hwnd,
        text.as_bytes(),
        "\u{feff}  /*niño\r\nfin\nlast*/\r".as_bytes(),
        before,
    )?;
    discard(hwnd)?;
    memory(
        hwnd,
        options,
        "line-edit-selected.cs",
        TextEncoding::Utf8,
        "one\r\n  two\n",
    )?;
    key(hwnd, 0x41, Modifiers::CTRL)?;
    let before = revision(hwnd);
    palette(
        hwnd,
        jc_app::default_registry()
            .get(CommandId::EditToggleLineComment)
            .unwrap()
            .label,
    )?;
    cycle(hwnd, b"one\r\n  two\n", b"// one\r\n  // two\n", before)?;
    discard(hwnd)?;
    for (name, commands) in [
        ("line-edit-disabled.py", &EDITS[5..6]),
        ("line-edit-disabled.txt", &EDITS[4..6]),
    ] {
        memory(hwnd, options, name, TextEncoding::Utf8, "plain\r\n")?;
        let before = snapshot(hwnd)?;
        for command_id in commands {
            command(hwnd, *command_id)?;
            if snapshot(hwnd)? != before {
                return Err("Unavailable profile comment changed source/history".into());
            }
        }
        expected(hwnd, b"plain\r\n")?;
        discard(hwnd)?;
    }
    checks.push("line_edit_selected_block_and_profile_disabled_comments");

    let text = "one\r\ntwo\nlast";
    memory(
        hwnd,
        options,
        "line-edit-shared.p",
        TextEncoding::Utf8,
        text,
    )?;
    let document = snapshot(hwnd)?.0;
    let tabs = snapshot(hwnd)?.3;
    command(hwnd, CommandId::EditorSplitRight)?;
    command(hwnd, CommandId::EditorFocusSecondary)?;
    position(hwnd, 2, 1)?;
    key(hwnd, 0x27, Modifiers::SHIFT)?;
    command(hwnd, CommandId::EditorTogglePin)?;
    command(hwnd, CommandId::EditorFocusPrimary)?;
    position(hwnd, 0, 0)?;
    let before = revision(hwnd);
    key(hwnd, 0x44, Modifiers::CTRL)?;
    expected(hwnd, b"one\r\none\r\ntwo\nlast")?;
    {
        let s = owner(hwnd).unwrap().state.borrow();
        if revision(hwnd) == before
            || s.workbench.position_for_pane(PaneId::Secondary)
                != Some(TextPosition { line: 3, column: 2 })
            || s.workbench.selection_for_pane(PaneId::Secondary) != Some(15..16)
            || !s
                .workbench
                .panes()
                .view(PaneId::Secondary)
                .unwrap()
                .is_pinned()
            || s.workbench.tabs().tabs().len() != tabs
            || s.workbench.active_document().unwrap().id != document
        {
            return Err(
                "Shared line edit lost inactive caret/selection/pin or duplicated a document"
                    .into(),
            );
        }
    }
    {
        let mut s = owner(hwnd).unwrap().state.borrow_mut();
        paint::prepare(&mut s);
        paint::prepare(&mut s);
    }
    let before_paint = snapshot(hwnd)?;
    let metrics = {
        let s = owner(hwnd).unwrap().state.borrow();
        (s.jobs.snapshot_bytes(), s.jobs.artifact_bytes())
    };
    let frame = {
        let mut s = owner(hwnd).unwrap().state.borrow_mut();
        paint::prepare(&mut s)
    };
    if let Some(bitmap) = &options.bitmap {
        let output = bitmap.with_file_name("native-line-edits.bmp");
        if output.exists() || output == options.fixture || output == options.report {
            return Err("Line-edit bitmap must be a new sibling".into());
        }
        paint::bitmap(hwnd, &frame, &output)?;
    }
    {
        let s = owner(hwnd).unwrap().state.borrow();
        if snapshot(hwnd)? != before_paint
            || (s.jobs.snapshot_bytes(), s.jobs.artifact_bytes()) != metrics
        {
            return Err("Line-edit composition changed source/views or admitted snapshots".into());
        }
    }
    key(hwnd, 0x5a, Modifiers::CTRL)?;
    expected(hwnd, text.as_bytes())?;
    if owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .selection_for_pane(PaneId::Secondary)
        != Some(10..11)
    {
        return Err("Undo failed to map the inactive selection back".into());
    }
    command(hwnd, CommandId::EditorFocusSecondary)?;
    command(hwnd, CommandId::EditorTogglePin)?;
    command(hwnd, CommandId::EditorFocusPrimary)?;
    command(hwnd, CommandId::EditorOneGroup)?;
    discard(hwnd)?;
    checks.push("line_edit_split_pin_inactive_selection_rebase_readonly");

    memory(hwnd, options, "line-edit-focus.p", TextEncoding::Utf8, text)?;
    command(hwnd, CommandId::SearchFind)?;
    key(hwnd, 0x55, Modifiers::CTRL)?;
    paste(hwnd, "field draft")?;
    let before = snapshot(hwnd)?;
    for id in EDITS {
        command(hwnd, id)?;
        if snapshot(hwnd)? != before
            || owner(hwnd)
                .unwrap()
                .state
                .borrow()
                .search_panel
                .as_ref()
                .map(|p| p.query.as_str())
                != Some("field draft")
        {
            return Err("Edit menu modified a hidden source or closed Find".into());
        }
    }
    palette_open(
        hwnd,
        jc_app::default_registry()
            .get(CommandId::EditDuplicate)
            .unwrap()
            .label,
    )?;
    key(hwnd, 0x0d, Modifiers::NONE)?;
    if snapshot(hwnd)? != before {
        return Err("Palette invoked from Find edited hidden source".into());
    }
    key(hwnd, 0x1b, Modifiers::NONE)?;
    if owner(hwnd).unwrap().state.borrow().search_panel.is_some() {
        key(hwnd, 0x1b, Modifiers::NONE)?;
    }
    command(hwnd, CommandId::SearchGoToPosition)?;
    key(hwnd, 0x55, Modifiers::CTRL)?;
    paste(hwnd, "1:1")?;
    let before = snapshot(hwnd)?;
    for id in EDITS {
        command(hwnd, id)?;
        if snapshot(hwnd)? != before
            || owner(hwnd)
                .unwrap()
                .state
                .borrow()
                .navigation_panel
                .is_none()
        {
            return Err("Edit menu modified hidden source or closed GoTo".into());
        }
    }
    key(hwnd, 0x1b, Modifiers::NONE)?;
    command(hwnd, CommandId::ViewExplorer)?;
    if !owner(hwnd).unwrap().state.borrow().explorer_focused {
        return Err("Line-edit focus fixture could not focus Explorer".into());
    }
    let before = snapshot(hwnd)?;
    for id in EDITS {
        command(hwnd, id)?;
        if snapshot(hwnd)? != before {
            return Err("Edit menu modified source behind Explorer".into());
        }
    }
    key(hwnd, 0x1b, Modifiers::NONE)?;
    {
        let mut s = owner(hwnd).unwrap().state.borrow_mut();
        s.explorer_visible = false;
        s.explorer_focused = false;
        s.explorer_hits = None;
    }
    for mutation in 0..4 {
        position(hwnd, 0, 0)?;
        palette_open(
            hwnd,
            jc_app::default_registry()
                .get(CommandId::EditToggleLineComment)
                .unwrap()
                .label,
        )?;
        {
            let mut s = owner(hwnd).unwrap().state.borrow_mut();
            match mutation {
                0 => s.workbench.insert_text("X").map_err(|e| e.to_string())?,
                1 => {
                    s.workbench.split_editor_right();
                    s.workbench.focus_editor_pane(PaneId::Secondary);
                }
                2 => s
                    .workbench
                    .set_encoding(TextEncoding::Windows1252)
                    .map_err(|e| e.to_string())?,
                _ => s.workbench.move_right(true).map_err(|e| e.to_string())?,
            }
        }
        let before = snapshot(hwnd)?;
        let bytes = owner(hwnd)
            .unwrap()
            .state
            .borrow()
            .workbench
            .active_bytes()
            .unwrap();
        key(hwnd, 0x0d, Modifiers::NONE)?;
        if snapshot(hwnd)? != before {
            return Err("Stale palette editing target changed source or view intention".into());
        }
        expected(hwnd, &bytes)?;
        key(hwnd, 0x1b, Modifiers::NONE)?;
        match mutation {
            0 => {
                key(hwnd, 0x5a, Modifiers::CTRL)?;
            }
            1 => {
                command(hwnd, CommandId::EditorFocusPrimary)?;
                command(hwnd, CommandId::EditorOneGroup)?;
            }
            2 => owner(hwnd)
                .unwrap()
                .state
                .borrow_mut()
                .workbench
                .set_encoding(TextEncoding::Utf8)
                .map_err(|e| e.to_string())?,
            _ => {}
        }
        expected(hwnd, text.as_bytes())?;
    }
    discard(hwnd)?;
    checks.push("line_edit_modal_explorer_and_stale_palette_no_hidden_edit");
    expected(hwnd, SOURCE)?;
    position(hwnd, 0, 0)?;
    key(hwnd, 0x44, Modifiers::CTRL)?;
    expected(hwnd, b"alpha\r\nalpha\r\nbeta\r\n")?;
    {
        let s = owner(hwnd).unwrap().state.borrow();
        let d = s.workbench.active_document().unwrap();
        if !d.session.document().is_modified()
            || d.id != baseline.0
            || s.workbench
                .panes()
                .view(s.workbench.panes().active())
                .and_then(|v| v.view_id())
                != baseline.1
        {
            return Err("Source clean-to-dirty line edit changed document/view ownership".into());
        }
    }
    if fs::read(&options.fixture).map_err(|e| e.to_string())? != SOURCE {
        return Err("Source line edit wrote disk without Save".into());
    }
    key(hwnd, 0x5a, Modifiers::CTRL)?;
    expected(hwnd, SOURCE)?;
    {
        let s = owner(hwnd).unwrap().state.borrow();
        let d = s.workbench.active_document().unwrap();
        if d.session.document().is_modified()
            || d.id != baseline.0
            || s.workbench
                .panes()
                .view(s.workbench.panes().active())
                .and_then(|v| v.view_id())
                != baseline.1
        {
            return Err(
                "One source Undo did not restore clean bytes and original ownership".into(),
            );
        }
    }
    if fs::read(&options.fixture).map_err(|e| e.to_string())? != SOURCE {
        return Err("Line-edit smoke changed original disk fixture".into());
    }
    checks.push("line_edit_owned_memory_cleanup_original_source_disk_preserved");
    Ok(checks)
}
