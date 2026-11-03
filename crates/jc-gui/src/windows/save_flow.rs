//! Explicit per-document Save All; no model borrow crosses a modal dialog.
use super::{
    ffi::Hwnd,
    file_dialog_named, owner,
    save_plan::{SaveCandidate, SavePlan},
};
use std::path::Path;

pub(super) unsafe fn perform(hwnd: Hwnd) {
    let Some(model) = owner(hwnd) else {
        return;
    };
    if model.state.borrow().save_all_active {
        return;
    }
    model.state.borrow_mut().save_all_active = true;
    let mut plan = {
        let state = model.state.borrow();
        match SavePlan::new(
            state
                .workbench
                .modified_documents()
                .take(1025)
                .map(|(document, revision, new_file)| SaveCandidate {
                    document,
                    revision,
                    new_file,
                })
                .collect(),
        ) {
            Ok(plan) => plan,
            Err(error) => {
                drop(state);
                let mut state = model.state.borrow_mut();
                state.save_all_active = false;
                state.error(error);
                return;
            }
        }
    };
    let result = run(hwnd, &mut plan);
    if let Some(model) = owner(hwnd) {
        let mut state = model.state.borrow_mut();
        state.save_all_active = false;
        state.status = match result {
            Ok(()) => format!(
                "Save All {}: {} saved; {} pending",
                if plan.remaining() > 0 {
                    "cancelled"
                } else {
                    "complete"
                },
                plan.saved_count(),
                plan.remaining()
            ),
            Err(error) => format!(
                "Save All stopped: {} saved; {} pending · {error}",
                plan.saved_count(),
                plan.remaining()
            )
            .chars()
            .take(240)
            .collect(),
        };
    }
}
unsafe fn run(hwnd: Hwnd, plan: &mut SavePlan) -> Result<(), String> {
    while let Some(candidate) = plan.next() {
        let (target, title) = {
            let state = owner(hwnd)
                .ok_or("Window closed during Save All")?
                .state
                .borrow();
            let document = state
                .workbench
                .document_by_id(candidate.document)
                .ok_or("Save All document was closed")?;
            if document.session.is_new() != candidate.new_file {
                return Err("Save All path assignment changed; retry".into());
            }
            let title = format!(
                "Save All — {}",
                document
                    .session
                    .path()
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("<native path>")
                    .chars()
                    .take(100)
                    .collect::<String>()
            );
            (
                state
                    .workbench
                    .capture_save_target(candidate.document, candidate.revision)
                    .map_err(|e| e.to_string())?,
                title,
            )
        };
        let destination = if candidate.new_file {
            let smoke = owner(hwnd)
                .ok_or("Save All window closed")?
                .state
                .borrow()
                .smoke;
            let chosen = if smoke {
                owner(hwnd)
                    .unwrap()
                    .state
                    .borrow_mut()
                    .smoke_save_paths
                    .pop_front()
                    .unwrap_or(None)
            } else {
                file_dialog_named(hwnd, true, Some(&title))?
            };
            let Some(path) = chosen else {
                plan.cancel();
                return Ok(());
            };
            if smoke {
                validate_smoke_destination(hwnd, &path)?;
            }
            Some(path)
        } else {
            None
        };
        let mut state = owner(hwnd)
            .ok_or("Save All window closed before commit")?
            .state
            .borrow_mut();
        state
            .workbench
            .save_target(target, destination)
            .map_err(|e| e.to_string())?;
        state.save_count = state.save_count.saturating_add(1);
        plan.saved(candidate).map_err(str::to_owned)?;
    }
    Ok(())
}
unsafe fn validate_smoke_destination(hwnd: Hwnd, path: &Path) -> Result<(), String> {
    use std::os::windows::fs::MetadataExt;
    let state = owner(hwnd)
        .ok_or("Smoke Save All window closed")?
        .state
        .borrow();
    let root = state
        .smoke_root
        .as_ref()
        .ok_or("Smoke Save All has no owned directory")?;
    if !path.is_absolute()
        || path.exists()
        || path.parent().and_then(|p| p.canonicalize().ok()).as_ref() != Some(root)
    {
        return Err("Smoke Save As needs a new owned sibling path".into());
    }
    let parent = path.parent().ok_or("Smoke save has no parent")?;
    if std::fs::symlink_metadata(parent)
        .map_err(|e| e.to_string())?
        .file_attributes()
        & 0x400
        != 0
    {
        return Err("Smoke Save As parent cannot be redirected".into());
    }
    Ok(())
}
