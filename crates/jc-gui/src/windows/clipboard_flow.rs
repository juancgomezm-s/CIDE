//! Explicit native handoff. Every OS call happens after releasing model borrows.
use super::{clipboard, ffi::Hwnd, owner};
use jc_app::CommandId;
const LIMIT: usize = 2 * 1024 * 1024;
pub(super) unsafe fn paste_replacement(hwnd: Hwnd, generation: u64) {
    let root = owner(hwnd).and_then(|model| model.state.borrow().project_root.clone());
    let smoke = owner(hwnd).is_some_and(|model| model.state.borrow().smoke);
    let text = if smoke {
        Ok(owner(hwnd).and_then(|model| model.state.borrow().smoke_clipboard.clone()))
    } else {
        clipboard::read(hwnd)
    };
    if let Some(model) = owner(hwnd) {
        let mut state = model.state.borrow_mut();
        if state.project_root != root
            || state
                .replace_panel
                .as_ref()
                .filter(|p| p.stage == super::replace_panel::ReplaceStage::Draft)
                .map(|p| p.generation)
                != Some(generation)
        {
            state.error("Replacement field/root changed during clipboard read; retry");
            return;
        }
        match text {
            Ok(Some(text)) => state.edit_replace_field(jc_app::FieldEdit::Insert(&text)),
            Ok(None) => {}
            Err(error) => state.error(error),
        }
    }
}
pub(super) unsafe fn paste_project(hwnd: Hwnd, generation: u64) {
    let root = owner(hwnd).and_then(|model| model.state.borrow().project_root.clone());
    let smoke = owner(hwnd).is_some_and(|model| model.state.borrow().smoke);
    let text = if smoke {
        Ok(owner(hwnd).and_then(|model| model.state.borrow().smoke_clipboard.clone()))
    } else {
        clipboard::read(hwnd)
    };
    if let Some(model) = owner(hwnd) {
        let mut state = model.state.borrow_mut();
        if state.project_root != root
            || state
                .project_panel
                .as_ref()
                .filter(|p| p.visible && !p.results)
                .map(|p| p.generation)
                != Some(generation)
        {
            state.error("Project field/root changed during clipboard read; retry");
            return;
        }
        match text {
            Ok(Some(text)) => state.edit_project_field(jc_app::FieldEdit::Insert(&text)),
            Ok(None) => {}
            Err(error) => state.error(error),
        }
    }
}
pub(super) unsafe fn paste_search(hwnd: Hwnd, generation: u64) {
    let target = owner(hwnd).and_then(|o| {
        o.state
            .borrow()
            .workbench
            .active_document()
            .map(|d| (d.id, d.session.document().revision(), d.session.encoding()))
    });
    let smoke = owner(hwnd).is_some_and(|o| o.state.borrow().smoke);
    let text = if smoke {
        Ok(owner(hwnd).and_then(|o| o.state.borrow().smoke_clipboard.clone()))
    } else {
        clipboard::read(hwnd)
    };
    if let Some(owner) = owner(hwnd) {
        let mut state = owner.state.borrow_mut();
        let current = state
            .workbench
            .active_document()
            .map(|d| (d.id, d.session.document().revision(), d.session.encoding()));
        if current != target
            || state.search_panel.as_ref().map(|p| p.generation) != Some(generation)
        {
            state.error("Search field changed during clipboard read; retry");
            return;
        }
        match text {
            Ok(Some(text)) => state.edit_search_input(jc_app::FieldEdit::Insert(&text)),
            Ok(None) => {}
            Err(error) => state.error(error),
        }
    }
}

pub(super) unsafe fn perform(hwnd: Hwnd, command: CommandId) {
    let result = transfer(hwnd, command);
    if let Some(owner) = owner(hwnd) {
        let mut state = owner.state.borrow_mut();
        match result {
            Ok(()) => {
                state.status = "Ready · Windows text clipboard; rectangle clipboard internal".into()
            }
            Err(error) => state.error(error),
        }
    }
}
unsafe fn transfer(hwnd: Hwnd, command: CommandId) -> Result<(), String> {
    let model = owner(hwnd).ok_or("Clipboard window is closed")?;
    match command {
        CommandId::EditCopy | CommandId::EditCut => {
            let export = model
                .state
                .borrow()
                .workbench
                .export_selection(LIMIT)
                .map_err(|e| e.to_string())?;
            let Some(export) = export else {
                return Ok(());
            };
            let smoke = model.state.borrow().smoke;
            if smoke {
                // Test transport never opens the user's OS clipboard. The
                // production export/target/commit path below is unchanged.
                model.state.borrow_mut().smoke_clipboard = Some(export.text.clone());
            } else {
                clipboard::write(hwnd, &export.text)?;
            }
            if command == CommandId::EditCut {
                owner(hwnd)
                    .ok_or("Clipboard window closed during publication")?
                    .state
                    .borrow_mut()
                    .workbench
                    .delete_exported_selection(export)
                    .map_err(|e| e.to_string())?;
            } else {
                owner(hwnd)
                    .ok_or("Clipboard window closed during publication")?
                    .state
                    .borrow_mut()
                    .workbench
                    .remember_exported_copy(export)
                    .map_err(|e| e.to_string())?;
            }
        }
        CommandId::EditPaste => {
            let target = model
                .state
                .borrow()
                .workbench
                .capture_clipboard_target()
                .map_err(|e| e.to_string())?;
            let smoke = model.state.borrow().smoke;
            let text = if smoke {
                model.state.borrow().smoke_clipboard.clone()
            } else {
                clipboard::read(hwnd)?
            };
            if let Some(text) = text {
                owner(hwnd)
                    .ok_or("Clipboard window closed during read")?
                    .state
                    .borrow_mut()
                    .workbench
                    .paste_external_text(target, &text, LIMIT)
                    .map_err(|e| e.to_string())?;
            }
        }
        _ => return Err("Unsupported clipboard action".into()),
    }
    Ok(())
}
