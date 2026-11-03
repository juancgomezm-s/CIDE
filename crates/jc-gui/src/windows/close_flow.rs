//! Closing one view delegates to F1. Discard of a last dirty view requires the
//! exact source intention still to be current after the native confirmation.
use super::{ffi::*, line_edit::EditTarget, owner, wide, State};

pub(super) fn commit(
    state: &mut State,
    target: EditTarget,
    confirmed: bool,
) -> Result<bool, String> {
    if !confirmed {
        state.status = "Cierre cancelado".into();
        return Ok(false);
    }
    if state.source_edit_target() != Some(target) {
        return Err("Cambió el destino durante la confirmación de cierre; reintenta".into());
    }
    let document = state.workbench.active_document().map(|d| d.id);
    state
        .workbench
        .close_active(true)
        .map_err(|e| e.to_string())?;
    state.status = if document.is_some_and(|id| state.workbench.document_by_id(id).is_some()) {
        "Vista cerrada · documento compartido conservado"
    } else {
        "Cerrado; no se conservan los cambios descartados"
    }
    .into();
    state.ensure_encoding_selector();
    Ok(true)
}
pub(super) unsafe fn perform(hwnd: Hwnd) {
    let target = {
        let Some(model) = owner(hwnd) else { return };
        let mut state = model.state.borrow_mut();
        let target = state.source_edit_target();
        match state.workbench.close_active(false) {
            Ok(None) => {
                state.status = "Vista/pestaña cerrada".into();
                state.ensure_encoding_selector();
                return;
            }
            Ok(Some(_)) => target,
            Err(error) => {
                state.error(error);
                return;
            }
        }
    };
    let Some(target) = target else { return };
    // All native dialog calls occur after releasing model borrows. Test mode
    // consumes only an explicit decision for its own HWND, never OS UI input.
    let smoke = owner(hwnd).is_some_and(|m| m.state.borrow().smoke);
    let confirmed = if smoke {
        owner(hwnd).is_some_and(|m| {
            m.state
                .borrow_mut()
                .smoke_close_decisions
                .pop_front()
                .unwrap_or(false)
        })
    } else {
        MessageBoxW(
            hwnd,
            wide("¿Descartar los cambios sin guardar de esta pestaña? Guarda antes para conservarlos.").as_ptr(),
            wide("CIDE").as_ptr(),
            0x134,
        ) == 6
    };
    if let Some(model) = owner(hwnd) {
        let mut state = model.state.borrow_mut();
        if let Err(error) = commit(&mut state, target, confirmed) {
            state.error(error);
        }
    }
}
