//! Explicit native handoff. Every OS call happens after releasing model borrows.
use super::{clipboard, ffi::Hwnd, owner};
use jc_app::CommandId;
const LIMIT: usize = 2 * 1024 * 1024;
pub(super) unsafe fn paste_snippet(hwnd: Hwnd, target: super::snippet_panel::SnippetInputTarget) {
    let captured = owner(hwnd).and_then(|m| {
        let s = m.state.borrow();
        (s.snippet_input_target() == Some(target))
            .then(|| s.snippet_field_budget())
            .flatten()
            .map(|budget| (s.smoke, budget))
    });
    let Some((smoke, budget)) = captured else {
        return;
    };
    // Delayed rendering can reenter the window; no model borrow crosses read.
    let text = if smoke {
        Ok(owner(hwnd).and_then(|m| m.state.borrow().smoke_clipboard.clone()))
    } else {
        clipboard::read_limited(hwnd, budget)
    };
    if let Some(m) = owner(hwnd) {
        let mut s = m.state.borrow_mut();
        match text {
            Ok(Some(text)) if !text.is_empty() => s.snippet_paste_to(target, &text),
            Ok(_) => {}
            Err(e) => s.error(e),
        }
    }
}
/// Explicit selector field handoff. The injectable transport also exercises
/// reentrant delivery using controlled synthetic text on an owned test HWND.
pub(super) unsafe fn selector_transfer_with(
    hwnd: Hwnd,
    command: CommandId,
    target: super::selector_input::SelectorInputTarget,
    transport: impl FnOnce(Option<&str>) -> Result<Option<String>, String>,
) -> Result<(), String> {
    if !matches!(
        command,
        CommandId::EditCopy | CommandId::EditCut | CommandId::EditPaste
    ) {
        return Err("Acción de portapapeles no admitida en el selector".into());
    }
    let export = {
        let model = owner(hwnd).ok_or("La ventana del selector está cerrada")?;
        let state = model.state.borrow();
        if state.selector_input_target() != Some(target.clone()) {
            return Err("Cambió el destino del selector antes de usar el portapapeles".into());
        }
        if command == CommandId::EditPaste {
            None
        } else {
            let Some(text) = state.selector.as_ref().and_then(|s| s.selected_text()) else {
                return Ok(());
            };
            Some(text.to_owned())
        }
    };
    // No RefCell/Workbench borrow crosses this call (native delayed rendering
    // can pump messages). Text remains at most256 UTF-8 bytes before retention.
    let returned = transport(export.as_deref())?;
    let model = owner(hwnd).ok_or("Se cerró la ventana del selector durante la transferencia")?;
    let mut state = model.state.borrow_mut();
    if state.selector_input_target() != Some(target) {
        return Err(
            "Cambió el campo/contexto del selector durante la transferencia; reintenta".into(),
        );
    }
    match command {
        CommandId::EditCut => {
            let changed = state
                .selector
                .as_mut()
                .ok_or("Selector cerrado")?
                .edit_input(jc_app::FieldEdit::Delete, false)?;
            if changed {
                state.request_files();
            }
        }
        CommandId::EditPaste => {
            if let Some(text) = returned.filter(|text| !text.is_empty()) {
                // The model preflights256B after selection replacement and rejects
                // controls/bidi atomically. Synthetic transport obeys this same gate.
                let changed = state
                    .selector
                    .as_mut()
                    .ok_or("Selector cerrado")?
                    .edit_input(jc_app::FieldEdit::Insert(&text), false)?;
                if changed {
                    state.request_files();
                }
            }
        }
        CommandId::EditCopy => {}
        _ => return Err("Acción de portapapeles no admitida en el selector".into()),
    }
    Ok(())
}
pub(super) unsafe fn selector_transfer(
    hwnd: Hwnd,
    command: CommandId,
    target: super::selector_input::SelectorInputTarget,
) {
    let smoke = owner(hwnd).is_some_and(|model| model.state.borrow().smoke);
    let result = selector_transfer_with(hwnd, command, target, |export| {
        if let Some(text) = export {
            if smoke {
                owner(hwnd)
                    .ok_or("Se cerró la ventana de prueba del selector")?
                    .state
                    .borrow_mut()
                    .smoke_clipboard = Some(text.to_owned());
            } else {
                clipboard::write(hwnd, text)?;
            }
            Ok(None)
        } else if smoke {
            Ok(owner(hwnd).and_then(|model| model.state.borrow().smoke_clipboard.clone()))
        } else {
            clipboard::read_limited(hwnd, 256)
        }
    });
    if let Some(model) = owner(hwnd) {
        let mut state = model.state.borrow_mut();
        match result {
            Ok(()) => state.status = "Transferencia del selector completada".into(),
            Err(error) => state.error(error),
        }
    }
}
pub(super) unsafe fn paste_navigation(hwnd: Hwnd, generation: u64) {
    // The read happens without a model borrow. A new modal generation, view,
    // encoding or revision cannot consume the returned text.
    let smoke = owner(hwnd).is_some_and(|model| model.state.borrow().smoke);
    let text = if smoke {
        Ok(owner(hwnd).and_then(|model| model.state.borrow().smoke_clipboard.clone()))
    } else {
        clipboard::read(hwnd)
    };
    if let Some(model) = owner(hwnd) {
        let mut state = model.state.borrow_mut();
        if !state.navigation_target_current()
            || state.navigation_panel.as_ref().map(|p| p.generation) != Some(generation)
        {
            state.error("Cambió el campo/destino de navegación al leer el portapapeles; reintenta");
            return;
        }
        match text {
            Ok(Some(text)) => state.edit_navigation_input(jc_app::FieldEdit::Insert(&text)),
            Ok(None) => {}
            Err(error) => state.error(error),
        }
    }
}
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
            state.error("Cambió el campo/raíz del reemplazo al leer el portapapeles; reintenta");
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
            state.error("Cambió el campo/raíz del proyecto al leer el portapapeles; reintenta");
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
            state.error("Cambió el campo de búsqueda al leer el portapapeles; reintenta");
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
            Ok(()) => state.status = "Listo · portapapeles de Windows; rectángulos internos".into(),
            Err(error) => state.error(error),
        }
    }
}
unsafe fn transfer(hwnd: Hwnd, command: CommandId) -> Result<(), String> {
    let model = owner(hwnd).ok_or("La ventana del portapapeles está cerrada")?;
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
                    .ok_or("Se cerró la ventana al publicar en el portapapeles")?
                    .state
                    .borrow_mut()
                    .workbench
                    .delete_exported_selection(export)
                    .map_err(|e| e.to_string())?;
            } else {
                owner(hwnd)
                    .ok_or("Se cerró la ventana al publicar en el portapapeles")?
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
                    .ok_or("Se cerró la ventana al leer el portapapeles")?
                    .state
                    .borrow_mut()
                    .workbench
                    .paste_external_text(target, &text, LIMIT)
                    .map_err(|e| e.to_string())?;
            }
        }
        _ => return Err("Acción de portapapeles no admitida".into()),
    }
    Ok(())
}
