//! Save All over the owned fixture plus one create-new sibling. No native dialog
//! is accepted automatically and no user document is opened by automation.
use super::{
    ffi::Hwnd,
    owner,
    smoke::{command, expected, key, paste},
};
use crate::SmokeOptions;
use jc_app::CommandId;
use jc_app::Modifiers;

pub(super) unsafe fn run(hwnd: Hwnd, options: &SmokeOptions) -> Result<Vec<&'static str>, String> {
    let mut checks = Vec::new();
    // Make the existing fixture dirty while retaining its original bytes.
    key(hwnd, 0x24, Modifiers::NONE)?;
    super::ffi::SendMessageW(hwnd, super::ffi::WM_CHAR, 'X' as usize, 0);
    key(hwnd, 0x08, Modifiers::NONE)?;
    command(hwnd, CommandId::FileNew)?;
    paste(hwnd, "drafté\r\n")?;
    let generated = owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .active_document()
        .unwrap()
        .session
        .path()
        .to_owned();
    owner(hwnd)
        .unwrap()
        .state
        .borrow_mut()
        .smoke_save_paths
        .push_back(None);
    key(hwnd, 0x53, Modifiers::CTRL.union(Modifiers::ALT))?;
    {
        let state = owner(hwnd).unwrap().state.borrow();
        if !state.status.contains("cancelado")
            || !state.status.contains("1 guardados; 1 pendientes")
            || !state.workbench.active_document().unwrap().session.is_new()
            || generated.exists()
        {
            return Err(
                "Save All cancellation lost pending new document or wrote a generated name".into(),
            );
        }
    }
    if std::fs::read(&options.fixture).map_err(|e| format!("Save fixture read: {e}"))?
        != b"alpha\r\nbeta\r\n"
    {
        return Err("Partial Save All changed fixture bytes".into());
    }
    expected(hwnd, "drafté\r\n".as_bytes())?;
    checks.push("save_all_cancel_keeps_pending_new_document");
    let destination = options
        .fixture
        .parent()
        .unwrap()
        .join("save-all-assigned.txt");
    if destination.exists() {
        return Err("Save All smoke destination must be new".into());
    }
    let before = format!(
        "{:?}",
        owner(hwnd).unwrap().state.borrow().workbench.panes()
    );
    owner(hwnd)
        .unwrap()
        .state
        .borrow_mut()
        .smoke_save_paths
        .push_back(Some(destination.clone()));
    command(hwnd, CommandId::FileSaveAll)?;
    if std::fs::read(&destination).map_err(|e| {
        format!(
            "Save assigned read: {e}; status={}",
            owner(hwnd).unwrap().state.borrow().status
        )
    })? != "drafté\r\n".as_bytes()
    {
        return Err("Save All explicit new path lost UTF8/EOL bytes".into());
    }
    {
        let state = owner(hwnd).unwrap().state.borrow();
        if state.workbench.active_document().unwrap().session.is_new()
            || format!("{:?}", state.workbench.panes()) != before
            || state.workbench.context().dirty_documents != 0
        {
            return Err("Save All changed views or left the saved new document dirty".into());
        }
    }
    key(hwnd, 0x5a, Modifiers::CTRL)?;
    expected(hwnd, b"")?;
    key(hwnd, 0x59, Modifiers::CTRL)?;
    expected(hwnd, "drafté\r\n".as_bytes())?;
    checks.push("save_all_explicit_path_preserves_views_and_undo");
    super::ffi::SendMessageW(hwnd, super::ffi::WM_CHAR, '!' as usize, 0);
    let external = b"external owned disk snapshot";
    std::fs::write(&destination, external).map_err(|e| e.to_string())?;
    command(hwnd, CommandId::FileSaveAll)?;
    {
        let state = owner(hwnd).unwrap().state.borrow();
        if !state.status.contains("detenido")
            || !state.status.contains("0 guardados; 1 pendientes")
            || !state
                .workbench
                .active_document()
                .unwrap()
                .session
                .document()
                .is_modified()
        {
            return Err("Save All conflict was not reported as pending".into());
        }
    }
    if std::fs::read(&destination).map_err(|e| e.to_string())? != external {
        return Err("Save All overwrote an external modification".into());
    }
    owner(hwnd)
        .unwrap()
        .state
        .borrow_mut()
        .workbench
        .close_active(true)
        .map_err(|e| e.to_string())?;
    expected(hwnd, b"alpha\r\nbeta\r\n")?;
    checks.push("save_all_conflict_stops_without_overwriting_disk");
    Ok(checks)
}
