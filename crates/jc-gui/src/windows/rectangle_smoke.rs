//! Own-window rectangle routes; no OS clipboard, Save, foreign HWND or glyph claim.
use super::{
    ffi::*,
    owner, paint,
    project_smoke::snapshot,
    smoke::{command, expected, key, position},
};
use crate::SmokeOptions;
use jc_app::{
    rectangle::{VisualColumn, VisualPosition},
    CommandId, Modifiers, PaneId,
};
use jc_text::{encode, TextEncoding, TextPosition};
use std::os::windows::fs::MetadataExt;
use std::{fs, io::Write};

const SOURCE: &[u8] = b"alpha\r\nbeta\r\n";
const ENCODINGS: [TextEncoding; 4] = [
    TextEncoding::Utf8,
    TextEncoding::Windows1252,
    TextEncoding::Iso8859_1,
    TextEncoding::Ibm850,
];
fn visual(line: usize, column: usize) -> VisualPosition {
    VisualPosition {
        line,
        column: VisualColumn(column),
    }
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
unsafe fn memory(
    hwnd: Hwnd,
    options: &SmokeOptions,
    text: &str,
    encoding: TextEncoding,
) -> Result<(), String> {
    let path = options
        .fixture
        .parent()
        .ok_or("Rectangle fixture has no parent")?
        .join("rectangle-memory.txt");
    {
        let mut state = owner(hwnd)
            .ok_or("Rectangle smoke lost HWND")?
            .state
            .borrow_mut();
        state.close_selector();
        state.explorer_visible = false;
        state.explorer_focused = false;
        state.explorer_hits = None;
        state.workbench.new_file(path, encoding);
        // Setup of a new memory session; all actions below use production routes.
        state
            .workbench
            .insert_text(text)
            .map_err(|e| e.to_string())?;
    }
    position(hwnd, 0, 0)
}
unsafe fn discard(hwnd: Hwnd) -> Result<(), String> {
    owner(hwnd)
        .ok_or("Rectangle smoke lost HWND")?
        .state
        .borrow_mut()
        .workbench
        .close_active(true)
        .map_err(|e| e.to_string())?;
    expected(hwnd, SOURCE)
}
unsafe fn set(hwnd: Hwnd, first: (usize, usize), last: (usize, usize)) -> Result<(), String> {
    owner(hwnd)
        .ok_or("Rectangle smoke lost HWND")?
        .state
        .borrow_mut()
        .workbench
        .set_rectangle(visual(first.0, first.1), visual(last.0, last.1))
        .map_err(|e| e.to_string())
}
unsafe fn bounds(hwnd: Hwnd) -> Option<jc_app::RectangleBounds> {
    let state = owner(hwnd).unwrap().state.borrow();
    state
        .workbench
        .rectangle_for_pane(state.workbench.panes().active())
}
unsafe fn cycle(hwnd: Hwnd, before: &[u8], after: &[u8], rev: u64) -> Result<(), String> {
    expected(hwnd, after)?;
    if revision(hwnd) == rev {
        return Err("Rectangle edit did not change revision".into());
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
unsafe fn paint_readonly(hwnd: Hwnd, options: &SmokeOptions) -> Result<(), String> {
    let before = snapshot(hwnd)?;
    let (frame, jobs, geometry) = {
        let mut state = owner(hwnd).unwrap().state.borrow_mut();
        let jobs = (state.jobs.snapshot_bytes(), state.jobs.artifact_bytes());
        let geometry = state
            .workbench
            .rectangle_for_pane(state.workbench.panes().active());
        (paint::prepare(&mut state), jobs, geometry)
    };
    if !frame.contains_source_row("ñ") {
        return Err("Rectangle grid replaced supported ñ by diagnostic escapes".into());
    }
    if let Some(bitmap) = &options.bitmap {
        let path = bitmap.with_file_name("native-rectangle.bmp");
        if path.exists() || path == options.fixture || path == options.report || path == *bitmap {
            return Err("Rectangle bitmap must be a new owned sibling".into());
        }
        paint::bitmap(hwnd, &frame, &path)?;
    }
    let second = {
        let mut state = owner(hwnd).unwrap().state.borrow_mut();
        paint::prepare(&mut state)
    };
    if !second.contains_source_row("ñ") || snapshot(hwnd)? != before {
        return Err("Rectangle preparation changed source/view state".into());
    }
    let state = owner(hwnd).unwrap().state.borrow();
    if jobs != (state.jobs.snapshot_bytes(), state.jobs.artifact_bytes())
        || geometry
            != state
                .workbench
                .rectangle_for_pane(state.workbench.panes().active())
    {
        return Err("Rectangle paint admitted work or changed geometry".into());
    }
    Ok(())
}

pub(super) unsafe fn run(hwnd: Hwnd, options: &SmokeOptions) -> Result<Vec<&'static str>, String> {
    expected(hwnd, SOURCE)?;
    command(hwnd, CommandId::EditorOneGroup)?;
    {
        let mut state = owner(hwnd)
            .ok_or("Rectangle smoke lost HWND")?
            .state
            .borrow_mut();
        state.close_selector();
        state.explorer_visible = false;
        state.explorer_focused = false;
        state.explorer_hits = None;
        state.workbench.cancel_rectangle();
        if state.workbench.invisibles_visible() {
            state
                .workbench
                .execute(CommandId::ViewToggleInvisibles)
                .map_err(|e| e.to_string())?;
        }
    }
    let initial = snapshot(hwnd)?;
    let mut checks = Vec::new();
    for encoding in ENCODINGS {
        let bom = if encoding == TextEncoding::Utf8 {
            "\u{feff}"
        } else {
            ""
        };
        let source = format!("{bom}a\tb\r\nñ\rX");
        memory(hwnd, options, &source, encoding)?;
        position(hwnd, 0, 1)?;
        command(hwnd, CommandId::EditRectangleStart)?;
        command(hwnd, CommandId::EditRectangleRight)?;
        key(hwnd, 0x27, Modifiers::ALT.union(Modifiers::SHIFT))?;
        command(hwnd, CommandId::EditRectangleDown)?;
        let rect = bounds(hwnd).ok_or("Rectangle commands did not activate")?;
        if (rect.left, rect.right, rect.last_line) != (1, 3, 1) {
            return Err("Rectangle commands used scalar rather than TAB4 columns".into());
        }
        let rev = revision(hwnd);
        SendMessageW(hwnd, WM_CHAR, 'Q' as usize, 0);
        if bounds(hwnd).is_some() {
            return Err("Rectangle edit kept a second selection model".into());
        }
        cycle(
            hwnd,
            &encode(&source, encoding).map_err(|e| e.to_string())?,
            &encode(&format!("{bom}aQ b\r\nñQ\rX"), encoding).map_err(|e| e.to_string())?,
            rev,
        )?;
        discard(hwnd)?;
    }
    checks.push("rectangle_commands_four_encodings_atomic");

    memory(hwnd, options, "a\tb\r\nñ\rX", TextEncoding::Utf8)?;
    set(hwnd, (0, 1), (1, 3))?;
    key(hwnd, 0x43, Modifiers::CTRL)?;
    if !owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .clipboard_is_rectangular()
    {
        return Err("Rectangle Copy lost typed internal clipboard".into());
    }
    command(hwnd, CommandId::EditRectangleCancel)?;
    position(hwnd, 2, 0)?;
    let rev = revision(hwnd);
    key(hwnd, 0x56, Modifiers::CTRL)?;
    if revision(hwnd) != rev {
        return Err("Rectangle paste created lines beyond EOF".into());
    }
    expected(hwnd, "a\tb\r\nñ\rX".as_bytes())?;
    position(hwnd, 0, 0)?;
    key(hwnd, 0x56, Modifiers::CTRL)?;
    cycle(
        hwnd,
        "a\tb\r\nñ\rX".as_bytes(),
        "  a\tb\r\n  ñ\rX".as_bytes(),
        rev,
    )?;
    set(hwnd, (0, 0), (2, 1))?;
    let before = snapshot(hwnd)?;
    let accepted = owner(hwnd)
        .unwrap()
        .state
        .borrow_mut()
        .workbench
        .insert_text("one\ntwo")
        .is_ok();
    if accepted {
        return Err("Rectangle accepted mismatched paste height".into());
    }
    if snapshot(hwnd)? != before {
        return Err("Rejected rectangle paste changed source/view".into());
    }
    command(hwnd, CommandId::EditRectangleCancel)?;
    set(hwnd, (0, 1), (1, 3))?;
    let rev = revision(hwnd);
    key(hwnd, 0x58, Modifiers::CTRL)?;
    cycle(
        hwnd,
        "a\tb\r\nñ\rX".as_bytes(),
        "a b\r\nñ\rX".as_bytes(),
        rev,
    )?;
    discard(hwnd)?;
    checks.push("rectangle_internal_clipboard_height_eof");

    memory(hwnd, options, "ab\r\nc", TextEncoding::Utf8)?;
    set(hwnd, (0, 1), (1, 1))?;
    let rev = revision(hwnd);
    key(hwnd, 0x2e, Modifiers::NONE)?;
    cycle(hwnd, b"ab\r\nc", b"a\r\nc", rev)?;
    set(hwnd, (0, 1), (1, 1))?;
    let rev = revision(hwnd);
    key(hwnd, 0x08, Modifiers::NONE)?;
    cycle(hwnd, b"ab\r\nc", b"b\r\n", rev)?;
    set(hwnd, (0, 8), (1, 8))?;
    let rev = revision(hwnd);
    key(hwnd, 0x2e, Modifiers::NONE)?;
    if revision(hwnd) != rev {
        return Err("Virtual Delete published an undo step".into());
    }
    expected(hwnd, b"ab\r\nc")?;
    command(hwnd, CommandId::EditRectangleCancel)?;
    set(hwnd, (0, 1), (1, 1))?;
    let rev = revision(hwnd);
    key(hwnd, 0x09, Modifiers::NONE)?;
    // Existing indentation command broadcasts four spaces, not a literal TAB.
    cycle(hwnd, b"ab\r\nc", b"a    b\r\nc    ", rev)?;
    discard(hwnd)?;
    checks.push("rectangle_delete_backspace_tab_noop");

    memory(hwnd, options, "a\tb\r\nñ\rX", TextEncoding::Utf8)?;
    command(hwnd, CommandId::ViewToggleInvisibles)?;
    set(hwnd, (0, 1), (1, 3))?;
    paint_readonly(hwnd, options)?;
    // Width changes use preparation only; no model accept/poll is permitted.
    let dimensions = {
        let state = owner(hwnd).unwrap().state.borrow();
        (state.width, state.height)
    };
    for width in [360, 1400, 360] {
        let mut state = owner(hwnd).unwrap().state.borrow_mut();
        state.width = width;
        let frame = paint::prepare(&mut state);
        if !frame.contains_source_row("ñ") {
            return Err("Resized rectangle lost one-cell Latin1".into());
        }
    }
    {
        let mut state = owner(hwnd).unwrap().state.borrow_mut();
        state.width = dimensions.0;
        state.height = dimensions.1;
    }
    command(hwnd, CommandId::EditRectangleCancel)?;
    {
        let mut state = owner(hwnd).unwrap().state.borrow_mut();
        if !state.workbench.invisibles_visible()
            || !paint::prepare(&mut state).contains_source_row("\\u{F1}")
        {
            return Err(
                "Cancelling rectangle did not restore diagnostic display preference".into(),
            );
        }
    }
    command(hwnd, CommandId::ViewToggleInvisibles)?;
    discard(hwnd)?;
    checks.push("rectangle_invisibles_readonly_resize");

    memory(hwnd, options, "a\tb\r\nñ", TextEncoding::Utf8)?;
    command(hwnd, CommandId::SearchFind)?;
    let before = snapshot(hwnd)?;
    command(hwnd, CommandId::EditRectangleStart)?;
    if bounds(hwnd).is_some()
        || snapshot(hwnd)? != before
        || owner(hwnd).unwrap().state.borrow().search_panel.is_none()
    {
        return Err("Rectangle menu edited behind Find".into());
    }
    key(hwnd, 0x1b, Modifiers::NONE)?;
    command(hwnd, CommandId::ViewExplorer)?;
    command(hwnd, CommandId::EditRectangleStart)?;
    if bounds(hwnd).is_some() {
        return Err("Rectangle activated behind Explorer".into());
    }
    command(hwnd, CommandId::ViewExplorer)?;
    position(hwnd, 0, 1)?;
    command(hwnd, CommandId::EditRectangleStart)?;
    palette_open(hwnd, "Rectangle: Extend Right")?;
    let old_cursor = owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .active_editor()
        .unwrap()
        .cursor;
    owner(hwnd)
        .unwrap()
        .state
        .borrow_mut()
        .workbench
        .extend_rectangle(1, 0)
        .map_err(|e| e.to_string())?;
    let current = bounds(hwnd);
    if owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .active_editor()
        .unwrap()
        .cursor
        != old_cursor
    {
        return Err("Stale-palette setup did not remain inside TAB".into());
    }
    key(hwnd, 0x0d, Modifiers::NONE)?;
    if bounds(hwnd) != current || owner(hwnd).unwrap().state.borrow().selector.is_none() {
        return Err("Palette accepted stale visual selection intent".into());
    }
    key(hwnd, 0x1b, Modifiers::NONE)?;
    key(hwnd, 0x1b, Modifiers::NONE)?;
    discard(hwnd)?;
    checks.push("rectangle_modal_explorer_palette_target");

    memory(hwnd, options, "a\tb\r\nñ", TextEncoding::Utf8)?;
    command(hwnd, CommandId::EditorSplitRight)?;
    command(hwnd, CommandId::EditorFocusPrimary)?;
    set(hwnd, (0, 1), (1, 3))?;
    command(hwnd, CommandId::EditorFocusSecondary)?;
    command(hwnd, CommandId::EditorTogglePin)?;
    set(hwnd, (0, 4), (0, 6))?;
    let doc = owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .active_document()
        .unwrap()
        .id;
    SendMessageW(hwnd, WM_CHAR, 'Q' as usize, 0);
    {
        let state = owner(hwnd).unwrap().state.borrow();
        if state.workbench.active_document().unwrap().id != doc
            || state
                .workbench
                .rectangle_for_pane(PaneId::Primary)
                .is_some()
            || state
                .workbench
                .rectangle_for_pane(PaneId::Secondary)
                .is_some()
        {
            return Err("Shared rectangle edit cloned document or retained stale rectangle".into());
        }
    }
    key(hwnd, 0x5a, Modifiers::CTRL)?;
    expected(hwnd, "a\tb\r\nñ".as_bytes())?;
    set(hwnd, (0, 1), (1, 3))?;
    owner(hwnd)
        .unwrap()
        .state
        .borrow_mut()
        .workbench
        .set_encoding(TextEncoding::Windows1252)
        .map_err(|e| e.to_string())?;
    if bounds(hwnd).is_some() {
        return Err("Encoding change retained rectangle".into());
    }
    command(hwnd, CommandId::EditorTogglePin)?;
    command(hwnd, CommandId::EditorOneGroup)?;
    discard(hwnd)?;
    checks.push("rectangle_split_pin_revision_encoding");

    for text in ["e\u{301}Z", "界Z", "👨‍👩‍👧‍👦Z"] {
        memory(hwnd, options, text, TextEncoding::Utf8)?;
        set(hwnd, (0, 0), (0, 1))?;
        let frame = {
            let model = owner(hwnd).unwrap();
            let mut state = model.state.borrow_mut();
            super::paint::prepare(&mut state)
        };
        // The owned read model retains complete clusters before GDI composition.
        if !frame.contains_source_row(text) {
            return Err("Unicode cluster lost during native preparation".into());
        }
        command(hwnd, CommandId::EditRectangleCancel)?;
        discard(hwnd)?;
    }
    checks.push("rectangle_unicode_native_complete_clusters");
    memory(hwnd, options, "界e\u{301}Z\r\n😀aQ", TextEncoding::Utf8)?;
    set(hwnd, (0, 0), (1, 1))?;
    let before = snapshot(hwnd)?;
    command(hwnd, CommandId::EditCut)?;
    expected(hwnd, " e\u{301}Z\r\n aQ".as_bytes())?;
    key(hwnd, 0x5a, Modifiers::CTRL)?;
    expected(hwnd, "界e\u{301}Z\r\n😀aQ".as_bytes())?;
    if snapshot(hwnd)?.0 != before.0 {
        return Err("Unicode rectangle undo changed document identity".into());
    }
    discard(hwnd)?;
    checks.push("rectangle_unicode_partial_atom_cut_one_undo");

    for text in ["\u{301}", "\u{200d}", "\u{ad}", "a\u{1b}b"] {
        memory(hwnd, options, text, TextEncoding::Utf8)?;
        let before = snapshot(hwnd)?;
        command(hwnd, CommandId::EditRectangleStart)?;
        if bounds(hwnd).is_some() || snapshot(hwnd)? != before {
            return Err("Unsupported rectangle source mutated state".into());
        }
        expected(hwnd, text.as_bytes())?;
        discard(hwnd)?;
    }
    memory(hwnd, options, "a\tb", TextEncoding::Utf8)?;
    set(hwnd, (0, 1), (0, 2))?;
    let rect = bounds(hwnd);
    let before = snapshot(hwnd)?;
    let accepted = owner(hwnd)
        .unwrap()
        .state
        .borrow_mut()
        .workbench
        .extend_rectangle(4097, 0)
        .is_ok();
    if accepted || bounds(hwnd) != rect || snapshot(hwnd)? != before {
        return Err("Rectangle column quota was not atomic".into());
    }
    discard(hwnd)?;
    memory(hwnd, options, &"a".repeat(4097), TextEncoding::Utf8)?;
    let before = snapshot(hwnd)?;
    command(hwnd, CommandId::EditRectangleStart)?;
    if bounds(hwnd).is_some() || snapshot(hwnd)? != before {
        return Err("Overwide rectangle line was not rejected atomically".into());
    }
    discard(hwnd)?;
    memory(hwnd, options, &"x\n".repeat(1024), TextEncoding::Utf8)?;
    command(hwnd, CommandId::EditRectangleStart)?;
    let before = snapshot(hwnd)?;
    let accepted = owner(hwnd)
        .unwrap()
        .state
        .borrow_mut()
        .workbench
        .extend_rectangle(1, 1024)
        .is_ok();
    if accepted || snapshot(hwnd)? != before {
        return Err("Rectangle row quota published partial geometry".into());
    }
    discard(hwnd)?;
    let invalid = options.fixture.with_file_name("rectangle-invalid-utf8.txt");
    let parent = invalid
        .parent()
        .ok_or("Rectangle invalid-byte fixture has no parent")?;
    let parent_metadata = fs::symlink_metadata(parent).map_err(|e| e.to_string())?;
    if !invalid.is_absolute()
        || !parent_metadata.is_dir()
        || parent_metadata.file_attributes() & 0x400 != 0
    {
        return Err("Rectangle fixture parent must remain an ordinary owned directory".into());
    }
    let admitted_parent = parent.canonicalize().map_err(|e| e.to_string())?;
    {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&invalid)
            .map_err(|e| e.to_string())?;
        file.write_all(b"a\xffb\r\n").map_err(|e| e.to_string())?;
    }
    let invalid_result = (|| -> Result<(), String> {
        owner(hwnd)
            .unwrap()
            .state
            .borrow_mut()
            .workbench
            .open_file(&invalid, Some(TextEncoding::Utf8))
            .map_err(|e| e.to_string())?;
        let before = snapshot(hwnd)?;
        command(hwnd, CommandId::EditRectangleStart)?;
        if bounds(hwnd).is_some() || snapshot(hwnd)? != before {
            return Err("Invalid UTF8 activated rectangle".into());
        }
        expected(hwnd, b"a\xffb\r\n")?;
        discard(hwnd)
    })();
    let metadata = fs::symlink_metadata(&invalid).map_err(|e| e.to_string())?;
    let current_parent = fs::symlink_metadata(parent).map_err(|e| e.to_string())?;
    if !metadata.is_file()
        || metadata.file_attributes() & 0x400 != 0
        || invalid.parent() != options.fixture.parent()
        || !current_parent.is_dir()
        || current_parent.file_attributes() & 0x400 != 0
        || parent.canonicalize().map_err(|e| e.to_string())? != admitted_parent
    {
        return Err("Rectangle invalid-byte cleanup target changed".into());
    }
    fs::remove_file(&invalid).map_err(|e| e.to_string())?;
    invalid_result?;
    checks.push("rectangle_repertoire_budget_atomic");

    mouse(hwnd, options)?;
    checks.push("rectangle_mouse_capture_versioned");
    {
        let mut state = owner(hwnd).unwrap().state.borrow_mut();
        state.close_selector();
        state.explorer_visible = false;
        state.explorer_focused = false;
        state.explorer_hits = None;
        state.workbench.cancel_rectangle();
        if state.workbench.invisibles_visible() {
            state
                .workbench
                .execute(CommandId::ViewToggleInvisibles)
                .map_err(|e| e.to_string())?;
        }
    }
    expected(hwnd, SOURCE)?;
    // Restore the normal text-clipboard route for subsequent owned smoke
    // sections. Copy uses the injected transport, never the OS clipboard.
    position(hwnd, 0, 0)?;
    key(hwnd, 0x27, Modifiers::SHIFT)?;
    command(hwnd, CommandId::EditCopy)?;
    key(hwnd, 0x25, Modifiers::NONE)?;
    let final_state = snapshot(hwnd)?;
    if final_state.0 != initial.0
        || final_state.1 != initial.1
        || final_state.3 != initial.3
        || fs::read(&options.fixture).map_err(|e| e.to_string())? != SOURCE
    {
        return Err(
            "Rectangle smoke changed original source identity/revision/catalog or disk".into(),
        );
    }
    Ok(checks)
}

fn packed(x: i32, y: i32) -> isize {
    (((y as u16 as usize) << 16) | x as u16 as usize) as isize
}
unsafe fn mouse_message(hwnd: Hwnd, message: u32, point: (i32, i32), alt: bool) {
    owner(hwnd).unwrap().state.borrow_mut().key_override =
        Some(if alt { Modifiers::ALT } else { Modifiers::NONE });
    SendMessageW(
        hwnd,
        message,
        if message == 0x202 { 0 } else { 1 },
        packed(point.0, point.1),
    );
    owner(hwnd).unwrap().state.borrow_mut().key_override = None;
}
unsafe fn pixel(hwnd: Hwnd, line: usize, column: usize) -> Result<(i32, i32), String> {
    let state = owner(hwnd).unwrap().state.borrow();
    paint::rectangle_pixel(&state, state.workbench.panes().active(), line, column)
        .ok_or_else(|| "Rectangle smoke point is not visible".into())
}
unsafe fn mouse(hwnd: Hwnd, options: &SmokeOptions) -> Result<(), String> {
    #[link(name = "user32")]
    extern "system" {
        fn GetCapture() -> Hwnd;
    }
    memory(hwnd, options, "a\tb\r\nñ", TextEncoding::Utf8)?;
    // First hit is an ASCII prefix before TAB, so pre-rectangle scalar and
    // rectangle coordinates coincide. Subsequent hits use the active grid.
    let start = pixel(hwnd, 0, 1)?;
    mouse_message(hwnd, WM_LBUTTONDOWN, start, true);
    if owner(hwnd).unwrap().state.borrow().rectangle_drag.is_none() || GetCapture() != hwnd {
        return Err("Alt-drag did not acquire own-window capture".into());
    }
    let end = pixel(hwnd, 1, 3)?;
    mouse_message(hwnd, 0x200, end, true);
    let rect = bounds(hwnd).ok_or("Alt-drag did not prepare a rectangle")?;
    if (rect.first_line, rect.last_line, rect.left, rect.right) != (0, 1, 1, 3) {
        return Err("Alt-drag changed TAB4/virtual coordinates".into());
    }
    mouse_message(hwnd, 0x202, end, true);
    if owner(hwnd).unwrap().state.borrow().rectangle_drag.is_some()
        || GetCapture() == hwnd
        || bounds(hwnd) != Some(rect)
    {
        return Err("Mouse release did not retain selection and end capture".into());
    }
    key(hwnd, 0x27, Modifiers::NONE)?;
    if bounds(hwnd).is_some() {
        return Err("Normal movement retained rectangle mode".into());
    }
    position(hwnd, 0, 0)?;
    mouse_message(hwnd, WM_LBUTTONDOWN, pixel(hwnd, 0, 0)?, true);
    let admitted = bounds(hwnd);
    SendMessageW(hwnd, 0x1f, 0, 0); // WM_CANCELMODE: terminate only our capture.
    if owner(hwnd).unwrap().state.borrow().rectangle_drag.is_some()
        || GetCapture() == hwnd
        || bounds(hwnd) != admitted
    {
        return Err("CancelMode did not preserve last admitted rectangle and end capture".into());
    }
    command(hwnd, CommandId::EditRectangleCancel)?;
    command(hwnd, CommandId::ViewToggleInvisibles)?;
    let start = pixel(hwnd, 0, 0)?;
    mouse_message(hwnd, WM_LBUTTONDOWN, start, true);
    let end = pixel(hwnd, 1, 4)?;
    mouse_message(hwnd, 0x200, end, true);
    if bounds(hwnd).is_none_or(|r| r.head_column != 4 || r.last_line != 1) {
        return Err("Invisible Alt-drag used diagnostic escape width".into());
    }
    mouse_message(hwnd, 0x202, end, true);
    mouse_message(hwnd, WM_LBUTTONDOWN, end, false);
    if bounds(hwnd).is_some()
        || owner(hwnd)
            .unwrap()
            .state
            .borrow()
            .workbench
            .active_position()
            != Some(TextPosition { line: 1, column: 1 })
    {
        return Err(
            "Normal click in invisible rectangle did not convert grid to source caret".into(),
        );
    }
    mouse_message(hwnd, 0x202, end, false);
    mouse_message(hwnd, WM_LBUTTONDOWN, pixel(hwnd, 0, 0)?, true);
    key(hwnd, 0x1b, Modifiers::NONE)?;
    if owner(hwnd).unwrap().state.borrow().rectangle_drag.is_some() || GetCapture() == hwnd {
        return Err("Escape did not release rectangle drag capture".into());
    }
    command(hwnd, CommandId::EditRectangleCancel)?;
    command(hwnd, CommandId::ViewToggleInvisibles)?;
    position(hwnd, 0, 0)?;
    let start = pixel(hwnd, 0, 0)?;
    mouse_message(hwnd, WM_LBUTTONDOWN, start, true);
    owner(hwnd)
        .unwrap()
        .state
        .borrow_mut()
        .workbench
        .insert_text("Q")
        .map_err(|e| e.to_string())?;
    let before = snapshot(hwnd)?;
    mouse_message(hwnd, 0x200, end, true);
    if owner(hwnd).unwrap().state.borrow().rectangle_drag.is_some()
        || bounds(hwnd).is_some()
        || snapshot(hwnd)? != before
    {
        return Err("Stale drag revived selection after source revision changed".into());
    }
    mouse_message(hwnd, 0x202, end, true);
    key(hwnd, 0x5a, Modifiers::CTRL)?;
    expected(hwnd, "a\tb\r\nñ".as_bytes())?;
    discard(hwnd)
}
