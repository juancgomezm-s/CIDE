//! Deterministic internal automation of this frontend's own hidden HWND. It
//! drives the production message handlers, not a second editor implementation.
use super::{command_id, ffi::*, owner, paint};
use crate::SmokeOptions;
use jc_app::{CommandId, Modifiers, PaneId};
use jc_text::{TextEncoding, TextPosition};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const SOURCE: &[u8] = b"alpha\r\nbeta\r\n";
fn ordinary(path: &Path) -> Result<bool, String> {
    use std::os::windows::fs::MetadataExt;
    Ok(fs::symlink_metadata(path)
        .map_err(|e| e.to_string())?
        .file_attributes()
        & 0x400
        == 0)
}
fn normalized_target(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err("Smoke paths must be absolute".into());
    }
    let parent = path
        .parent()
        .ok_or("Smoke artifact has no parent")?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !ordinary(path.parent().unwrap())? {
        return Err("Smoke artifact parent cannot be a reparse point".into());
    }
    Ok(parent.join(path.file_name().ok_or("Smoke artifact has no filename")?))
}
pub(super) fn validate(options: &SmokeOptions) -> Result<(), String> {
    if !options.fixture.is_absolute() || !ordinary(&options.fixture)? {
        return Err("Smoke requires an ordinary absolute owned fixture".into());
    }
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .ok_or("Workspace unavailable")?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let work = workspace
        .join("work")
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !work.starts_with(&workspace) {
        return Err("Smoke work root escaped the workspace".into());
    }
    let fixture = options.fixture.canonicalize().map_err(|e| e.to_string())?;
    if !fixture.starts_with(&work)
        || fs::metadata(&fixture).map_err(|e| e.to_string())?.len() != SOURCE.len() as u64
        || fs::read(&fixture).map_err(|e| e.to_string())? != SOURCE
    {
        return Err("Smoke fixture must be owned beneath workspace/work and contain alpha CRLF beta CRLF exactly".into());
    }
    let report = normalized_target(&options.report)?;
    if report.parent() != fixture.parent() || report.exists() {
        return Err("Smoke report must be new and beside its fixture".into());
    }
    if let Some(bitmap) = &options.bitmap {
        let bitmap = normalized_target(bitmap)?;
        if bitmap.parent() != fixture.parent() || bitmap.exists() || bitmap == report {
            return Err("Smoke bitmap must be a distinct new sibling artifact".into());
        }
    }
    Ok(())
}

unsafe fn bytes(hwnd: Hwnd) -> Result<Vec<u8>, String> {
    let owner = owner(hwnd).ok_or("Smoke window lost its model")?;
    let state = owner.state.borrow();
    let document = state
        .workbench
        .active_document()
        .ok_or("Smoke document closed")?;
    if document.session.document().len() > 128 {
        return Err("Smoke model exceeded its small-fixture budget".into());
    }
    document
        .session
        .document()
        .read_range(0..document.session.document().len())
        .map_err(|e| e.to_string())
}
pub(super) unsafe fn expected(hwnd: Hwnd, value: &[u8]) -> Result<(), String> {
    if bytes(hwnd)? != value {
        Err("Smoke model bytes differ from the expected action".into())
    } else {
        Ok(())
    }
}
pub(super) unsafe fn command(hwnd: Hwnd, command: CommandId) -> Result<(), String> {
    let id = command_id(command);
    if id == 0 {
        return Err("Smoke command is not exposed by the native menu".into());
    }
    SendMessageW(hwnd, WM_COMMAND, id, 0);
    Ok(())
}
pub(super) unsafe fn key(hwnd: Hwnd, code: usize, modifiers: Modifiers) -> Result<(), String> {
    owner(hwnd)
        .ok_or("Smoke HWND unavailable")?
        .state
        .borrow_mut()
        .key_override = Some(modifiers);
    SendMessageW(hwnd, WM_KEYDOWN, code, 0);
    if let Some(owner) = owner(hwnd) {
        owner.state.borrow_mut().key_override = None;
    }
    Ok(())
}
pub(super) unsafe fn position(hwnd: Hwnd, line: usize, column: usize) -> Result<(), String> {
    owner(hwnd)
        .ok_or("Smoke HWND unavailable")?
        .state
        .borrow_mut()
        .workbench
        .go_to_position(TextPosition { line, column })
        .map_err(|e| e.to_string())
}
unsafe fn type_query(hwnd: Hwnd, text: &str) {
    for unit in text.encode_utf16() {
        SendMessageW(hwnd, WM_CHAR, unit as usize, 0);
    }
}
unsafe fn wait_for_paths(hwnd: Hwnd) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        // Dispatch the actual worker-posted message for this HWND only. No
        // direct controller poll or synthetic keystroke advances the result.
        let mut message: Msg = std::mem::zeroed();
        while PeekMessageW(&mut message, hwnd, TASKS_MESSAGE, TASKS_MESSAGE, 1) != 0 {
            DispatchMessageW(&message);
        }
        let state = owner(hwnd)
            .ok_or("Selector smoke lost HWND")?
            .state
            .borrow();
        match state.quick_open.state() {
            jc_app::quick_open::QuickOpenState::Complete => return Ok(()),
            jc_app::quick_open::QuickOpenState::Queued
            | jc_app::quick_open::QuickOpenState::Running => {}
            state => return Err(format!("Quick Open did not complete: {state:?}")),
        }
        drop(state);
        if Instant::now() >= deadline {
            return Err("Native worker notification deadline exceeded".into());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}
unsafe fn wait_for_search(hwnd: Hwnd) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let mut message: Msg = std::mem::zeroed();
        while PeekMessageW(&mut message, hwnd, TASKS_MESSAGE, TASKS_MESSAGE, 1) != 0 {
            DispatchMessageW(&message);
        }
        let state = owner(hwnd).ok_or("Search smoke lost HWND")?.state.borrow();
        if !state.workbench.search_running() {
            return if state.workbench.search().status == jc_app::SearchStatus::Complete {
                Ok(())
            } else {
                Err(format!(
                    "Search did not complete: {:?}",
                    state.workbench.search().status
                ))
            };
        }
        drop(state);
        if Instant::now() >= deadline {
            return Err("Search message delivery deadline exceeded".into());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}
pub(super) unsafe fn paste(hwnd: Hwnd, text: &str) -> Result<(), String> {
    owner(hwnd)
        .ok_or("Paste fixture lost HWND")?
        .state
        .borrow_mut()
        .smoke_clipboard = Some(text.into());
    key(hwnd, 0x56, Modifiers::CTRL)
}
fn escaped(value: &str) -> String {
    value
        .chars()
        .take(240)
        .flat_map(|ch| match ch {
            '"' => "\\\"".chars().collect::<Vec<_>>(),
            '\\' => "\\\\".chars().collect(),
            ch if ch.is_control() => " ".chars().collect(),
            ch => vec![ch],
        })
        .collect()
}

pub(super) unsafe fn run(hwnd: Hwnd, options: &SmokeOptions) -> Result<(), String> {
    let mut report = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&options.report)
        .map_err(|e| e.to_string())?;
    let started = Instant::now();
    let mut checks = Vec::new();
    let result = (|| {
        expected(hwnd, SOURCE)?;
        checks.push("startup_shared_open_bytes");
        let mut title = [0u16; 160];
        let length = GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32);
        if length <= 0
            || !String::from_utf16(&title[..length as usize])
                .map_err(|e| e.to_string())?
                .starts_with("CIDE")
        {
            return Err("Native HWND did not expose the CIDE title".into());
        }
        checks.push("cide_native_window_title");
        if GetClassLongPtrW(hwnd, -14) == 0 {
            return Err("CIDE orange GUI icon resource was not loaded by the HWND class".into());
        }
        checks.push("cide_native_gui_icon_resource");
        if !GetWindow(hwnd, 5).is_null() {
            return Err("Native editor unexpectedly contains a child text control".into());
        }
        checks.push("no_child_edit_control");
        if options.navigation_only
            || options.line_edit_only
            || options.selection_reopen_only
            || options.rectangle_only
            || options.parity_only
            || options.bottom_only
            || options.selector_input_only
            || options.interaction_only
            || options.menu_invoker_only
            || options.syntax_only
            || options.workflow_only
        {
            if options.workflow_only {
                checks.extend(super::workflow_smoke::run(hwnd, options)?);
            } else if options.syntax_only {
                checks.extend(super::syntax_smoke::run(hwnd, options)?);
            } else if options.menu_invoker_only {
                checks.extend(super::menu_invoker_smoke::run(hwnd, options)?);
            } else if options.interaction_only {
                checks.extend(super::interaction_smoke::run(hwnd, options)?);
            } else if options.selector_input_only {
                checks.extend(super::selector_input_smoke::run(hwnd, options)?);
            } else if options.bottom_only {
                checks.extend(super::bottom_smoke::run(hwnd, options)?);
            } else if options.parity_only {
                checks.extend(super::parity_smoke::run(hwnd, options)?);
            } else if options.rectangle_only {
                checks.extend(super::rectangle_smoke::run(hwnd, options)?);
            } else if options.selection_reopen_only {
                checks.extend(super::selection_reopen_smoke::run(hwnd, options)?);
            } else if options.line_edit_only {
                checks.extend(super::line_edit_smoke::run(hwnd, options)?);
            } else {
                checks.extend(super::navigation_smoke::run(hwnd, options)?);
            }
            SendMessageW(hwnd, WM_PAINT, 0, 0);
            let frame = {
                let mut state = owner(hwnd).unwrap().state.borrow_mut();
                paint::prepare(&mut state)
            };
            if let Some(bitmap) = &options.bitmap {
                paint::bitmap(hwnd, &frame, bitmap)?;
                checks.push("actual_gdi_offscreen_bitmap");
            }
            let state = owner(hwnd).unwrap().state.borrow();
            if state.paint_count == 0 || state.paint_errors != 0 {
                return Err("Navigation WM_PAINT missing or failed".into());
            }
            checks.push("native_paint_handler");
            checks.push("native_paint_no_errors");
            return Ok(());
        }
        let (x, y) = {
            let state = owner(hwnd).unwrap().state.borrow();
            (
                8 + 6 * state.cell_width + 1,
                2 * state.cell_height + 14 + state.cell_height / 2,
            )
        };
        SendMessageW(hwnd, WM_LBUTTONDOWN, 1, ((y as isize) << 16) | x as isize);
        {
            let state = owner(hwnd).unwrap().state.borrow();
            if state
                .workbench
                .active_editor()
                .ok_or("No mouse document")?
                .cursor
                != 1
                || state.workbench.context().nav_back_available
            {
                return Err(
                    "Mouse caret placement created jump history or missed its scalar boundary"
                        .into(),
                );
            }
        }
        checks.push("native_mouse_caret_without_jump_history");
        position(hwnd, 0, 0)?;
        // Deterministic private modifier override feeds the exact WM_KEYDOWN
        // handler, without changing global keyboard state or focusing a window.
        SendMessageW(hwnd, WM_CHAR, 0xd83d, 0);
        SendMessageW(hwnd, WM_CHAR, 0xde00, 0);
        expected(hwnd, "😀alpha\r\nbeta\r\n".as_bytes())?;
        key(hwnd, 0x5a, Modifiers::CTRL)?;
        expected(hwnd, SOURCE)?;
        checks.push("utf16_pair_one_undo");
        SendMessageW(hwnd, WM_CHAR, 0xdc00, 0);
        expected(hwnd, SOURCE)?;
        checks.push("unpaired_utf16_rejected");
        key(hwnd, 0x41, Modifiers::CTRL)?;
        if owner(hwnd)
            .unwrap()
            .state
            .borrow()
            .workbench
            .active_editor()
            .and_then(|editor| editor.selection())
            != Some(0..SOURCE.len())
        {
            return Err("Ctrl+A did not select the shared source".into());
        }
        key(hwnd, 0x43, Modifiers::CTRL)?;
        SendMessageW(hwnd, WM_CHAR, 'q' as usize, 0);
        expected(hwnd, b"q")?;
        key(hwnd, 0x56, Modifiers::CTRL)?;
        expected(hwnd, b"qalpha\r\nbeta\r\n")?;
        key(hwnd, 0x5a, Modifiers::CTRL)?;
        expected(hwnd, b"q")?;
        key(hwnd, 0x5a, Modifiers::CTRL)?;
        expected(hwnd, SOURCE)?;
        checks.push("selection_internal_clipboard_and_undo");
        position(hwnd, 0, 5)?;
        key(hwnd, 0x0d, Modifiers::NONE)?;
        expected(hwnd, b"alpha\r\n\r\nbeta\r\n")?;
        key(hwnd, 0x5a, Modifiers::CTRL)?;
        expected(hwnd, SOURCE)?;
        checks.push("enter_shared_split_one_undo");
        position(hwnd, 0, 0)?;
        key(hwnd, 0x4a, Modifiers::CTRL.union(Modifiers::SHIFT))?;
        expected(hwnd, b"alpha beta\r\n")?;
        key(hwnd, 0x5a, Modifiers::CTRL)?;
        expected(hwnd, SOURCE)?;
        checks.push("join_shared_transaction_one_undo");
        command(hwnd, CommandId::EditorSplitRight)?;
        command(hwnd, CommandId::EditorFocusSecondary)?;
        {
            let state = owner(hwnd).unwrap().state.borrow();
            if !state.workbench.panes().is_split() {
                return Err("Native split did not reach PaneManager".into());
            }
            let primary = state
                .workbench
                .panes()
                .view(PaneId::Primary)
                .and_then(|view| view.location())
                .ok_or("Missing primary")?;
            let secondary = state
                .workbench
                .panes()
                .view(PaneId::Secondary)
                .and_then(|view| view.location())
                .ok_or("Missing secondary")?;
            if primary.tab != secondary.tab {
                return Err("Split did not share its catalog document".into());
            }
        }
        position(hwnd, 0, 0)?;
        SendMessageW(hwnd, WM_CHAR, 'Z' as usize, 0);
        expected(hwnd, b"Zalpha\r\nbeta\r\n")?;
        key(hwnd, 0x5a, Modifiers::CTRL)?;
        expected(hwnd, SOURCE)?;
        checks.push("split_shared_document_undo");
        // Produce a dirty revision with the same original bytes, then exercise
        // real Save via the same Ctrl+S handler without leaving a changed fixture.
        position(hwnd, 0, 0)?;
        SendMessageW(hwnd, WM_CHAR, 'X' as usize, 0);
        key(hwnd, 0x08, Modifiers::NONE)?;
        expected(hwnd, SOURCE)?;
        let before = owner(hwnd).unwrap().state.borrow().save_count;
        key(hwnd, 0x53, Modifiers::CTRL)?;
        if owner(hwnd).unwrap().state.borrow().save_count != before + 1
            || fs::read(&options.fixture).map_err(|e| e.to_string())? != SOURCE
        {
            return Err("Native shared save failed or changed fixture bytes".into());
        }
        checks.push("shared_save_exact_fixture_bytes");
        let new_path = options
            .fixture
            .parent()
            .ok_or("Missing owned fixture directory")?
            .join("owned-new.txt");
        {
            let mut state = owner(hwnd).unwrap().state.borrow_mut();
            state.workbench.new_file(&new_path, TextEncoding::Utf8);
            state
                .workbench
                .insert_text("unsaved")
                .map_err(|error| error.to_string())?;
            if !matches!(state.execute(CommandId::FileSave), super::Action::SaveAs) {
                return Err("New native document bypassed Save As".into());
            }
            checks.push("unsaved_save_routes_to_save_as");
            if !matches!(
                state.execute(CommandId::FileSaveAll),
                super::Action::SaveAll
            ) || new_path.exists()
            {
                return Err(
                    "Native Save All wrote an unnamed document without its frontend flow".into(),
                );
            }
            checks.push("native_save_all_routes_explicit_flow");
            state
                .workbench
                .close_active(true)
                .map_err(|error| error.to_string())?;
            state.status = "Ready · internal clipboard · scalar-cell preview".into();
        }
        expected(hwnd, SOURCE)?;
        checks.extend(super::save_smoke::run(hwnd, options)?);
        checks.extend(super::project_smoke::run(hwnd, options)?);
        checks.extend(super::replace_smoke::run(hwnd, options)?);
        checks.extend(super::git_smoke::run(hwnd, options)?);
        checks.extend(super::explorer_smoke::run(hwnd, options)?);
        checks.extend(super::navigation_smoke::run(hwnd, options)?);
        checks.extend(super::line_edit_smoke::run(hwnd, options)?);
        checks.extend(super::selection_reopen_smoke::run(hwnd, options)?);
        checks.extend(super::rectangle_smoke::run(hwnd, options)?);
        checks.extend(super::parity_smoke::run(hwnd, options)?);
        checks.extend(super::bottom_smoke::run(hwnd, options)?);
        checks.extend(super::selector_input_smoke::run(hwnd, options)?);
        checks.extend(super::interaction_smoke::run(hwnd, options)?);
        checks.extend(super::menu_invoker_smoke::run(hwnd, options)?);
        checks.extend(super::syntax_smoke::run(hwnd, options)?);
        // A complete native selector flow: modal query input, real PostMessage
        // delivery, authoritative target, dirty session reuse and shared undo.
        position(hwnd, 0, 0)?;
        SendMessageW(hwnd, WM_CHAR, 'Q' as usize, 0);
        let original_document = owner(hwnd)
            .unwrap()
            .state
            .borrow()
            .workbench
            .active_document()
            .unwrap()
            .id;
        let before_delivery = owner(hwnd).unwrap().state.borrow().deliveries;
        key(hwnd, 0x50, Modifiers::CTRL)?;
        type_query(hwnd, "FIXTURE.TXT");
        expected(hwnd, b"Qalpha\r\nbeta\r\n")?;
        wait_for_paths(hwnd)?;
        {
            let state = owner(hwnd).unwrap().state.borrow();
            if state.deliveries <= before_delivery || state.quick_open.results().len() != 1 {
                return Err(
                    "Native Quick Open did not receive the latest worker completion".into(),
                );
            }
        }
        checks.push("quick_open_native_message_delivery");
        checks.push("selector_query_preserves_document");
        key(hwnd, 0x0d, Modifiers::CTRL)?;
        {
            let state = owner(hwnd).unwrap().state.borrow();
            if state.selector.is_some()
                || state.workbench.active_document().unwrap().id != original_document
                || !state.workbench.panes().is_split()
            {
                return Err("Native Quick Open did not reuse the dirty document to side".into());
            }
        }
        expected(hwnd, b"Qalpha\r\nbeta\r\n")?;
        key(hwnd, 0x5a, Modifiers::CTRL)?;
        expected(hwnd, SOURCE)?;
        checks.push("quick_open_dirty_side_shared_undo");
        position(hwnd, 0, 5)?;
        key(hwnd, 0x50, Modifiers::CTRL.union(Modifiers::SHIFT))?;
        type_query(
            hwnd,
            jc_app::default_registry()
                .get(CommandId::EditSplitLine)
                .unwrap()
                .label,
        );
        expected(hwnd, SOURCE)?;
        key(hwnd, 0x0d, Modifiers::NONE)?;
        expected(hwnd, b"alpha\r\n\r\nbeta\r\n")?;
        key(hwnd, 0x5a, Modifiers::CTRL)?;
        expected(hwnd, SOURCE)?;
        checks.push("palette_shared_command_one_undo");
        key(hwnd, 0x50, Modifiers::CTRL.union(Modifiers::SHIFT))?;
        type_query(
            hwnd,
            jc_app::default_registry()
                .get(CommandId::SearchGoToPosition)
                .unwrap()
                .label,
        );
        key(hwnd, 0x0d, Modifiers::NONE)?;
        if owner(hwnd).unwrap().state.borrow().selector.is_none() {
            return Err("Palette executed an unsupported native flow".into());
        }
        expected(hwnd, SOURCE)?;
        checks.push("palette_unavailable_command_rejected");
        key(hwnd, 0x1b, Modifiers::NONE)?;
        key(hwnd, 0x50, Modifiers::CTRL)?;
        type_query(hwnd, "fixture.txt");
        // Root replacement must invalidate/cancel the old selector before any
        // next completion is admitted. Reuse only the owned fixture directory.
        {
            let mut state = owner(hwnd).unwrap().state.borrow_mut();
            state.set_project_root(options.fixture.parent().unwrap().to_owned())?;
            if state.selector.is_some() || !state.quick_open.results().is_empty() {
                return Err("Project root replacement retained a stale selector".into());
            }
        }
        checks.push("project_change_cancels_selector");
        let (revision, undo, dirty) = {
            let state = owner(hwnd).unwrap().state.borrow();
            let d = state
                .workbench
                .active_document()
                .unwrap()
                .session
                .document();
            (d.revision(), d.undo_steps(), d.is_modified())
        };
        command(hwnd, CommandId::FileChooseEncoding)?;
        key(hwnd, 0x1b, Modifiers::NONE)?;
        command(hwnd, CommandId::FileChooseEncoding)?;
        type_query(hwnd, "Windows-1252");
        key(hwnd, 0x0d, Modifiers::NONE)?;
        if owner(hwnd)
            .unwrap()
            .state
            .borrow()
            .workbench
            .active_encoding()
            != Some(TextEncoding::Windows1252)
        {
            return Err("Native encoding choice did not reinterpret the document".into());
        }
        command(hwnd, CommandId::FileChooseEncoding)?;
        type_query(hwnd, "UTF-8");
        key(hwnd, 0x0d, Modifiers::NONE)?;
        expected(hwnd, SOURCE)?;
        {
            let state = owner(hwnd).unwrap().state.borrow();
            let d = state
                .workbench
                .active_document()
                .unwrap()
                .session
                .document();
            if (d.revision(), d.undo_steps(), d.is_modified()) != (revision, undo, dirty) {
                return Err("Encoding interpretation edited source/history/dirty state".into());
            }
        }
        checks.push("encoding_choice_and_cancel_preserve_source_history");
        key(hwnd, 0x49, Modifiers::CTRL.union(Modifiers::ALT))?;
        {
            let mut state = owner(hwnd).unwrap().state.borrow_mut();
            let frame = paint::prepare(&mut state);
            if !state.workbench.invisibles_visible() || !frame.contains_source_row("alpha\\r\\n") {
                return Err("Native invisible mode did not show atomic CRLF spans".into());
            }
        }
        expected(hwnd, SOURCE)?;
        checks.push("invisibles_native_spans_without_source_edit");
        command(hwnd, CommandId::SearchReplace)?;
        key(hwnd, 0x55, Modifiers::CTRL)?;
        paste(hwnd, "alpha\r\nbeta")?;
        wait_for_search(hwnd)?;
        key(hwnd, 0x49, Modifiers::CTRL.union(Modifiers::ALT))?;
        key(hwnd, 0x49, Modifiers::CTRL.union(Modifiers::ALT))?;
        if owner(hwnd)
            .unwrap()
            .state
            .borrow()
            .search_panel
            .as_ref()
            .map(|p| p.query.as_str())
            != Some("alpha\r\nbeta")
        {
            return Err("View toggle discarded the native multiline draft".into());
        }
        checks.push("invisibles_toggle_preserves_search_draft");
        if owner(hwnd)
            .unwrap()
            .state
            .borrow()
            .workbench
            .search()
            .matches
            .len()
            != 1
        {
            return Err("Native multiline query did not cross CRLF".into());
        }
        key(hwnd, 0x72, Modifiers::NONE)?;
        key(hwnd, 0x09, Modifiers::NONE)?;
        paste(hwnd, "é\nQ\r\nR")?;
        let before = owner(hwnd)
            .unwrap()
            .state
            .borrow()
            .workbench
            .active_document()
            .unwrap()
            .session
            .document()
            .undo_steps();
        key(hwnd, 0x78, Modifiers::NONE)?;
        let replaced = "é\nQ\r\nR\r\n".as_bytes();
        expected(hwnd, replaced)?;
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
            != before + 1
        {
            return Err("Native multiline replace-all did not publish one undo".into());
        }
        key(hwnd, 0x1b, Modifiers::NONE)?;
        command(hwnd, CommandId::FileSave)?;
        if fs::read(&options.fixture).map_err(|e| e.to_string())? != replaced {
            return Err("Native multiline save lost exact UTF8/EOL bytes".into());
        }
        key(hwnd, 0x5a, Modifiers::CTRL)?;
        expected(hwnd, SOURCE)?;
        command(hwnd, CommandId::FileSave)?;
        if fs::read(&options.fixture).map_err(|e| e.to_string())? != SOURCE {
            return Err("Native multiline undo/save did not restore original bytes".into());
        }
        checks.push("multiline_native_paste_find_replace_all_one_undo_save");
        command(hwnd, CommandId::SearchFind)?;
        key(hwnd, 0x55, Modifiers::CTRL)?;
        paste(hwnd, "missing\r\n")?;
        wait_for_search(hwnd)?;
        if !owner(hwnd)
            .unwrap()
            .state
            .borrow()
            .workbench
            .search()
            .matches
            .is_empty()
        {
            return Err("Native no-hit query returned a match".into());
        }
        key(hwnd, 0x72, Modifiers::SHIFT)?;
        expected(hwnd, SOURCE)?;
        key(hwnd, 0x55, Modifiers::CTRL)?;
        for code in [0x71, 0x73, 0x77, 0x76] {
            key(hwnd, code, Modifiers::NONE)?;
        }
        if owner(hwnd)
            .unwrap()
            .state
            .borrow()
            .search_panel
            .as_ref()
            .unwrap()
            .query
            != "\n\r\n\r\t"
        {
            return Err("Native field newline gestures did not retain exact separators".into());
        }
        let before = owner(hwnd)
            .unwrap()
            .state
            .borrow()
            .search_panel
            .as_ref()
            .unwrap()
            .query
            .clone();
        paste(hwnd, &"x".repeat(4097))?;
        if owner(hwnd)
            .unwrap()
            .state
            .borrow()
            .search_panel
            .as_ref()
            .unwrap()
            .query
            != before
        {
            return Err("Oversized native field paste was truncated instead of rejected".into());
        }
        expected(hwnd, SOURCE)?;
        checks.push("multiline_native_no_hit_eol_gestures_and_atomic_quota");
        key(hwnd, 0x1b, Modifiers::NONE)?;
        command(hwnd, CommandId::SearchReplace)?;
        key(hwnd, 0x55, Modifiers::CTRL)?;
        paste(hwnd, "alpha\r\nbeta")?;
        wait_for_search(hwnd)?;
        key(hwnd, 0x09, Modifiers::NONE)?;
        paste(hwnd, "é\nQ\r\nR")?;
        if let Some(bitmap) = &options.bitmap {
            if owner(hwnd)
                .unwrap()
                .state
                .borrow()
                .status
                .contains("excede")
            {
                return Err(
                    "A successful field edit retained the earlier rejected-paste status".into(),
                );
            }
            let frame = {
                let mut state = owner(hwnd).unwrap().state.borrow_mut();
                paint::prepare(&mut state)
            };
            paint::bitmap(hwnd, &frame, &bitmap.with_file_name("native-multiline.bmp"))?;
        }
        key(hwnd, 0x1b, Modifiers::NONE)?;
        checks.push("multiline_fields_gdi_and_invisibles_read_only");
        // Capture the real popup renderer as part of the final native canvas.
        key(hwnd, 0x50, Modifiers::CTRL.union(Modifiers::SHIFT))?;
        type_query(hwnd, "Split");
        let revision = owner(hwnd)
            .unwrap()
            .state
            .borrow()
            .workbench
            .active_document()
            .unwrap()
            .session
            .document()
            .revision();
        SendMessageW(hwnd, WM_PAINT, 0, 0);
        let frame = {
            let mut state = owner(hwnd).unwrap().state.borrow_mut();
            paint::prepare(&mut state)
        };
        if let Some(bitmap) = &options.bitmap {
            paint::bitmap(hwnd, &frame, bitmap)?;
            checks.push("actual_gdi_offscreen_bitmap");
            checks.push("gdi_partial_clip_no_error");
        }
        if owner(hwnd)
            .unwrap()
            .state
            .borrow()
            .workbench
            .active_document()
            .unwrap()
            .session
            .document()
            .revision()
            != revision
        {
            return Err("Native popup painting changed the source revision".into());
        }
        checks.push("selector_gdi_paint_read_only");
        key(hwnd, 0x1b, Modifiers::NONE)?;
        if owner(hwnd).unwrap().state.borrow().paint_count == 0 {
            return Err("WM_PAINT handler did not run".into());
        }
        checks.push("native_paint_handler");
        if owner(hwnd).unwrap().state.borrow().paint_errors != 0 {
            return Err("Native WM_PAINT recorded a drawing error".into());
        }
        checks.push("native_paint_no_errors");
        if started.elapsed() > Duration::from_secs(15) {
            return Err("Smoke exceeded its cooperative operation deadline".into());
        }
        Ok(())
    })();
    let paints = owner(hwnd).map_or(0, |owner| owner.state.borrow().paint_count);
    let paint_errors = owner(hwnd).map_or(1, |owner| owner.state.borrow().paint_errors);
    let checks = checks
        .iter()
        .map(|check| format!("\"{check}\""))
        .collect::<Vec<_>>()
        .join(",");
    let error = result
        .as_ref()
        .err()
        .map_or(String::new(), |error: &String| escaped(error));
    writeln!(report,"{{\"ok\":{},\"backend\":\"Win32/GDI\",\"window\":\"own_hidden_hwnd\",\"native_paints\":{paints},\"native_paint_errors\":{paint_errors},\"wall_ms\":{},\"checks\":[{checks}],\"error\":\"{error}\",\"physical_display_verified\":false,\"os_clipboard_verified\":false}}",result.is_ok(),started.elapsed().as_millis()).map_err(|e|e.to_string())?;
    result
}
