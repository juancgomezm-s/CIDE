//! Focused validation of Palette -> Menu* -> restored Find/Replace. Only the
//! owned fixture's view metadata and multiline drafts are used. Popup choices
//! are synthetic; this is not physical TrackPopupMenuEx input evidence.
use super::{
    command_id,
    ffi::*,
    native_menu::{self, NativeMenuGroup},
    owner,
    project_smoke::snapshot,
    smoke::{command, expected, key, paste, position},
};
use crate::SmokeOptions;
use jc_app::{CommandId, DocumentId, Modifiers};
use jc_text::TextEncoding;
use std::fs;
const SOURCE: &[u8] = b"alpha\r\nbeta\r\n";
const QUERY: &str = "niño\r\nfin🙂";
const REPLACEMENT: &str = "otro🙂\r\né";
const MACROS: [CommandId; 6] = [
    CommandId::MenuFile,
    CommandId::MenuEdit,
    CommandId::MenuSearch,
    CommandId::MenuProject,
    CommandId::MenuTools,
    CommandId::MenuView,
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct Draft {
    query: String,
    replacement: String,
    query_cursor: usize,
    replacement_cursor: usize,
    replace: bool,
    replacement_active: bool,
    generation: u64,
}
impl Draft {
    fn from(panel: &super::search_panel::SearchPanel) -> Self {
        Self {
            query: panel.query.clone(),
            replacement: panel.replacement.clone(),
            query_cursor: panel.query_cursor,
            replacement_cursor: panel.replacement_cursor,
            replace: panel.replace,
            replacement_active: panel.replacement_active,
            generation: panel.generation,
        }
    }
}
unsafe fn draft(hwnd: Hwnd) -> Result<Draft, String> {
    let state = owner(hwnd).ok_or("Menu-invoker HWND lost")?.state.borrow();
    state
        .search_panel
        .as_ref()
        .map(Draft::from)
        .ok_or_else(|| "Find/Replace invoker draft is missing".into())
}
#[derive(PartialEq, Eq)]
struct SourceState {
    snapshot: (DocumentId, u64, String, usize),
    encoding: Option<TextEncoding>,
    undo_steps: usize,
    can_redo: bool,
    modified: bool,
}
unsafe fn source(hwnd: Hwnd) -> Result<SourceState, String> {
    expected(hwnd, SOURCE)?;
    let snapshot = snapshot(hwnd)?;
    let state = owner(hwnd).ok_or("Menu-invoker HWND lost")?.state.borrow();
    let document = state
        .workbench
        .active_document()
        .ok_or("Menu-invoker source closed")?;
    let bytes = document.session.document();
    Ok(SourceState {
        snapshot,
        encoding: document.session.encoding(),
        undo_steps: bytes.undo_steps(),
        can_redo: bytes.can_redo(),
        modified: bytes.is_modified(),
    })
}
unsafe fn invisible(hwnd: Hwnd) -> bool {
    owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .invisibles_visible()
}

/// This deliberately travels through the public palette input/accept path,
/// rather than calling execute(Menu*) with a handcrafted invoker context.
unsafe fn macro_from_draft(
    hwnd: Hwnd,
    macro_id: CommandId,
    choice: Option<usize>,
) -> Result<(), String> {
    let before = draft(hwnd)?;
    let source_before = source(hwnd)?;
    let label = {
        let state = owner(hwnd).unwrap().state.borrow();
        if !state.smoke_menu_choices.is_empty() {
            return Err("Menu-invoker choice queue was not empty".into());
        }
        state
            .registry
            .get(macro_id)
            .ok_or("Menu macro is not registered")?
            .label
    };
    owner(hwnd)
        .unwrap()
        .state
        .borrow_mut()
        .smoke_menu_choices
        .push_back(choice);
    command(hwnd, CommandId::PaletteOpen)?;
    {
        let state = owner(hwnd).unwrap().state.borrow();
        if state.search_panel.is_some()
            || state.selector_search_panel.as_ref().map(Draft::from) != Some(before.clone())
        {
            return Err("Opening palette did not move the exact Find invoker draft".into());
        }
    }
    paste(hwnd, label)?;
    {
        let state = owner(hwnd).unwrap().state.borrow();
        if state.selector_search_panel.as_ref().map(Draft::from) != Some(before.clone()) {
            return Err("Palette query input modified the stashed Find/Replace draft".into());
        }
    }
    key(hwnd, 0x0d, Modifiers::NONE)?;
    {
        let state = owner(hwnd).unwrap().state.borrow();
        if state.selector.is_some()
            || state.selector_search_panel.is_some()
            || !state.smoke_menu_choices.is_empty()
            || state.menu_stamp.is_some()
        {
            return Err(
                "Palette menu macro did not consume its popup choice and restore invoker ownership"
                    .into(),
            );
        }
    }
    if draft(hwnd)? != before || source(hwnd)? != source_before {
        return Err("Palette/native menu roundtrip changed draft/carets/generation or source ownership/undo/pin".into());
    }
    Ok(())
}
unsafe fn setup(hwnd: Hwnd, replace: bool) -> Result<(), String> {
    command(
        hwnd,
        if replace {
            CommandId::SearchReplace
        } else {
            CommandId::SearchFind
        },
    )?;
    key(hwnd, 0x55, Modifiers::CTRL)?;
    paste(hwnd, QUERY)?;
    key(hwnd, 0x25, Modifiers::NONE)?;
    if replace {
        key(hwnd, 0x09, Modifiers::NONE)?;
        paste(hwnd, REPLACEMENT)?;
        key(hwnd, 0x25, Modifiers::NONE)?;
        key(hwnd, 0x25, Modifiers::NONE)?;
    }
    let current = draft(hwnd)?;
    if current.query != QUERY
        || current.query_cursor != QUERY.len() - "🙂".len()
        || current.replace != replace
        || current.replacement_active != replace
    {
        return Err("Menu-invoker query setup does not follow scalar field movement".into());
    }
    if replace
        && (current.replacement != REPLACEMENT || current.replacement_cursor != "otro🙂".len())
    {
        return Err("Menu-invoker replacement caret did not step over CRLF atomically".into());
    }
    Ok(())
}
/// A real character and Backspace after returning prove keyboard focus remains
/// in the prior field. They do not use Workbench source editing or source undo.
unsafe fn continue_field(hwnd: Hwnd) -> Result<(), String> {
    let before = draft(hwnd)?;
    let source_before = source(hwnd)?;
    SendMessageW(hwnd, WM_CHAR, 'Z' as usize, 0);
    let inserted = draft(hwnd)?;
    let mut expected = before.clone();
    if before.replacement_active {
        expected.replacement.insert(before.replacement_cursor, 'Z');
        expected.replacement_cursor += 1;
    } else {
        expected.query.insert(before.query_cursor, 'Z');
        expected.query_cursor += 1;
    }
    expected.generation = inserted.generation;
    if inserted != expected || inserted.generation == before.generation {
        return Err("Keyboard input after native menu did not continue the restored field".into());
    }
    key(hwnd, 0x08, Modifiers::NONE)?;
    let mut restored = draft(hwnd)?;
    if restored.generation == inserted.generation {
        return Err("Restored field Backspace did not advance its intention generation".into());
    }
    restored.generation = before.generation;
    if restored != before || source(hwnd)? != source_before {
        return Err("Continuing the invoker field modified source or failed to restore exact multibyte/EOL draft".into());
    }
    Ok(())
}

pub(super) unsafe fn run(hwnd: Hwnd, options: &SmokeOptions) -> Result<Vec<&'static str>, String> {
    expected(hwnd, SOURCE)?;
    command(hwnd, CommandId::EditorOneGroup)?;
    {
        let mut state = owner(hwnd)
            .ok_or("Menu-invoker HWND lost")?
            .state
            .borrow_mut();
        state.close_selector();
        state.explorer_visible = false;
        state.explorer_focused = false;
        state.bottom.visible = false;
        state.bottom.focused = false;
    }
    let initial = source(hwnd)?;
    let initial_invisible = invisible(hwnd);
    let (initial_query, initial_position) = {
        let state = owner(hwnd).unwrap().state.borrow();
        (
            state.workbench.search().query.clone(),
            state.workbench.active_position(),
        )
    };
    // Pin is intentionally in the other group. Neither menus nor editing a
    // search draft should invoke automatic pinned source navigation.
    command(hwnd, CommandId::EditorSplitRight)?;
    command(hwnd, CommandId::EditorFocusSecondary)?;
    position(hwnd, 1, 2)?;
    command(hwnd, CommandId::EditorTogglePin)?;
    command(hwnd, CommandId::EditorFocusPrimary)?;
    position(hwnd, 0, 1)?;
    let pinned = source(hwnd)?;
    for phase in 0..3 {
        setup(hwnd, phase != 0)?;
        if phase == 2 {
            key(hwnd, 0x09, Modifiers::NONE)?;
        }
        for macro_id in MACROS {
            // Repeat cancellation on the same live draft, including a fresh
            // palette and native popup each time (no cached menu acceptance).
            macro_from_draft(hwnd, macro_id, None)?;
            macro_from_draft(hwnd, macro_id, None)?;
            continue_field(hwnd)?;
        }
        if source(hwnd)? != pinned {
            return Err("Menu macro/field loops changed shared split/pin/source state".into());
        }
        let before = invisible(hwnd);
        macro_from_draft(
            hwnd,
            CommandId::MenuView,
            Some(command_id(CommandId::ViewToggleInvisibles)),
        )?;
        if invisible(hwnd) == before {
            return Err("Restored Find invoker prevented a genuinely permitted View action".into());
        }
        macro_from_draft(
            hwnd,
            CommandId::MenuView,
            Some(command_id(CommandId::ViewToggleInvisibles)),
        )?;
        if invisible(hwnd) != before {
            return Err("Repeated View action did not restore prior preference".into());
        }
        macro_from_draft(
            hwnd,
            CommandId::MenuEdit,
            Some(command_id(CommandId::EditDuplicate)),
        )?;
        continue_field(hwnd)?;

        // Static smoke choices above prove the full palette-macro chain. The
        // same native popup transport below permits controlled reentrant input
        // while its restored Find/Replace invoker remains underneath it.
        let mutations = if phase == 0 { 2 } else { 3 };
        for mutation in 0..mutations {
            macro_from_draft(hwnd, CommandId::MenuView, None)?;
            let before = draft(hwnd)?;
            let source_before = source(hwnd)?;
            let visibility = invisible(hwnd);
            let mut changed = None;
            let mut mutation_result = Ok(());
            native_menu::open_with(hwnd, NativeMenuGroup::View, |_, _, _| {
                mutation_result = (|| -> Result<(), String> {
                    match mutation {
                        0 => {
                            SendMessageW(hwnd, WM_CHAR, 'R' as usize, 0);
                        }
                        1 => key(hwnd, 0x25, Modifiers::NONE)?,
                        _ => key(hwnd, 0x09, Modifiers::NONE)?,
                    }
                    changed = Some(draft(hwnd)?);
                    Ok(())
                })();
                if mutation_result.is_ok() {
                    command_id(CommandId::ViewToggleInvisibles)
                } else {
                    0
                }
            })?;
            mutation_result?;
            if changed == Some(before.clone())
                || changed != Some(draft(hwnd)?)
                || invisible(hwnd) != visibility
                || source(hwnd)? != source_before
            {
                return Err("Native popup accepted a View action after its invoker draft/caret/field changed".into());
            }
            if owner(hwnd).unwrap().state.borrow().menu_stamp.is_some() {
                return Err("Rejected popup choice retained a reusable menu stamp".into());
            }
            match mutation {
                0 => key(hwnd, 0x08, Modifiers::NONE)?,
                1 => key(hwnd, 0x27, Modifiers::NONE)?,
                _ => key(hwnd, 0x09, Modifiers::NONE)?,
            }
            let mut restored = draft(hwnd)?;
            restored.generation = before.generation;
            if restored != before {
                return Err(
                    "Reentrant field restoration lost exact original caret/EOL intent".into(),
                );
            }
            macro_from_draft(hwnd, CommandId::MenuFile, None)?;
        }
        key(hwnd, 0x1b, Modifiers::NONE)?;
        if owner(hwnd).unwrap().state.borrow().search_panel.is_some() {
            return Err("Find/Replace Escape failed after menu roundtrip".into());
        }
    }
    if invisible(hwnd) != initial_invisible || source(hwnd)? != pinned {
        return Err("Menu invoker testing changed preference or source/split/pin state".into());
    }
    command(hwnd, CommandId::EditorOneGroup)?;
    {
        let mut state = owner(hwnd).unwrap().state.borrow_mut();
        state.close_selector();
        state
            .workbench
            .set_search_query(initial_query)
            .map_err(|e| e.to_string())?;
        state.workbench.cancel_search();
        if let Some(position) = initial_position {
            state
                .workbench
                .set_caret_position(position, false)
                .map_err(|e| e.to_string())?;
        }
        state.explorer_visible = false;
        state.explorer_focused = false;
        state.explorer_hits = None;
        state.bottom.visible = false;
        state.bottom.focused = false;
        state.bottom_hits = None;
        if state.menu_stamp.is_some() || !state.smoke_menu_choices.is_empty() {
            return Err("Menu invoker cleanup retained a deferred popup intention".into());
        }
    }
    let after = source(hwnd)?;
    if after.snapshot.0 != initial.snapshot.0
        || after.snapshot.1 != initial.snapshot.1
        || after.snapshot.3 != initial.snapshot.3
        || after.encoding != initial.encoding
        || after.undo_steps != initial.undo_steps
        || after.can_redo != initial.can_redo
        || after.modified != initial.modified
        || invisible(hwnd) != initial_invisible
        || fs::read(&options.fixture).map_err(|e| e.to_string())? != SOURCE
    {
        return Err("Menu invoker cleanup changed original source identity/revision/encoding/undo/catalog/disk".into());
    }
    Ok(vec!["native_menu_palette_find_invoker_roundtrip"])
}
