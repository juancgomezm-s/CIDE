//! Internal automation of one owned hidden HWND. No global input, clipboard or
//! document save is used; S1 completions enter through its production message.
use super::{
    ffi::*,
    owner, paint,
    smoke::{command, expected, key, paste, position},
};
use crate::SmokeOptions;
use jc_app::{CommandId, Modifiers, ProjectSearchRow, ProjectSearchState};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    time::{Duration, Instant},
};

const SOURCE: &[u8] = b"alpha\r\nbeta\r\n";

unsafe fn pump(hwnd: Hwnd) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut message: Msg = std::mem::zeroed();
    while PeekMessageW(&mut message, hwnd, TASKS_MESSAGE, TASKS_MESSAGE, 1) != 0 {
        DispatchMessageW(&message);
        if Instant::now() >= deadline {
            return Err("Project task message drain exceeded deadline".into());
        }
    }
    Ok(())
}
unsafe fn complete(hwnd: Hwnd) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        pump(hwnd)?;
        let state = owner(hwnd).ok_or("Project smoke lost HWND")?.state.borrow();
        match state.project_search.state {
            ProjectSearchState::Complete => return Ok(()),
            ProjectSearchState::Running => {}
            other => return Err(format!("Project query did not complete: {other:?}")),
        }
        drop(state);
        if Instant::now() >= deadline {
            return Err("Project native notification deadline exceeded".into());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}
unsafe fn start(hwnd: Hwnd, query: &str, include: &str) -> Result<(), String> {
    command(hwnd, CommandId::SearchProject)?;
    // Draft focus persists between invocations. Reach Query through the same
    // field-navigation handler instead of silently writing panel metadata.
    for _ in 0..3 {
        let active = owner(hwnd)
            .ok_or("Project smoke lost HWND")?
            .state
            .borrow()
            .project_panel
            .as_ref()
            .ok_or("Project draft missing")?
            .active;
        if active == 0 {
            break;
        }
        key(hwnd, 0x09, Modifiers::NONE)?;
    }
    key(hwnd, 0x55, Modifiers::CTRL)?;
    paste(hwnd, query)?;
    key(hwnd, 0x09, Modifiers::NONE)?;
    key(hwnd, 0x55, Modifiers::CTRL)?;
    paste(hwnd, include)?;
    key(hwnd, 0x09, Modifiers::NONE)?;
    key(hwnd, 0x55, Modifiers::CTRL)?;
    key(hwnd, 0x0d, Modifiers::NONE)
}
unsafe fn snapshot(hwnd: Hwnd) -> Result<(jc_app::DocumentId, u64, String, usize), String> {
    let state = owner(hwnd).ok_or("Project smoke lost HWND")?.state.borrow();
    let document = state
        .workbench
        .active_document()
        .ok_or("Project smoke lost document")?;
    Ok((
        document.id,
        document.session.document().revision(),
        format!("{:?}", state.workbench.panes()),
        state.workbench.tabs().tabs().len(),
    ))
}
unsafe fn match_count(hwnd: Hwnd) -> Result<usize, String> {
    Ok(owner(hwnd)
        .ok_or("Project smoke lost HWND")?
        .state
        .borrow()
        .project_search
        .rows()
        .iter()
        .filter(|row| matches!(row, ProjectSearchRow::Match(_)))
        .count())
}
unsafe fn visible(hwnd: Hwnd) -> Result<bool, String> {
    Ok(owner(hwnd)
        .ok_or("Project smoke lost HWND")?
        .state
        .borrow()
        .project_panel
        .as_ref()
        .is_some_and(|p| p.visible))
}

/// The guard only ever deletes the exact ordinary leaf it created with
/// create_new. Root validation already confines this parent to workspace/work.
struct SideFile {
    path: PathBuf,
    parent: PathBuf,
}
impl SideFile {
    fn new(parent: PathBuf) -> Result<Self, String> {
        let parent = parent.canonicalize().map_err(|e| e.to_string())?;
        let path = parent.join("project-search-side.txt");
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| e.to_string())?;
        let guard = Self { path, parent };
        let written = file.write_all(b"disk_only_token\r\n");
        drop(file);
        written.map_err(|e| e.to_string())?;
        Ok(guard)
    }
    fn verify(&self) -> Result<(), String> {
        use std::os::windows::fs::MetadataExt;
        let metadata = fs::symlink_metadata(&self.path).map_err(|e| e.to_string())?;
        if !metadata.is_file()
            || metadata.file_attributes() & 0x400 != 0
            || self
                .path
                .canonicalize()
                .map_err(|e| e.to_string())?
                .parent()
                != Some(self.parent.as_path())
        {
            return Err("Project side fixture ownership changed".into());
        }
        Ok(())
    }
    fn change(&self) -> Result<(), String> {
        self.verify()?;
        // This exact file was created by this invocation, never the source fixture.
        fs::write(&self.path, b"changed_disk_token\r\n").map_err(|e| e.to_string())
    }
    fn cleanup(&self) -> Result<(), String> {
        self.verify()?;
        fs::remove_file(&self.path).map_err(|e| e.to_string())
    }
}
impl Drop for SideFile {
    fn drop(&mut self) {
        if self.verify().is_ok() {
            let _ = fs::remove_file(&self.path);
        }
    }
}

pub(super) unsafe fn run(hwnd: Hwnd, options: &SmokeOptions) -> Result<Vec<&'static str>, String> {
    expected(hwnd, SOURCE)?;
    let parent = options
        .fixture
        .parent()
        .ok_or("Project fixture has no parent")?
        .to_owned();
    let filename = options
        .fixture
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("Project smoke requires a Unicode fixture filename")?;
    let mut checks = Vec::new();
    position(hwnd, 0, 0)?;
    let original = snapshot(hwnd)?;
    SendMessageW(hwnd, WM_CHAR, 'Q' as usize, 0);
    expected(hwnd, b"Qalpha\r\nbeta\r\n")?;
    start(hwnd, "Qalpha\r\nbeta", filename)?;
    complete(hwnd)?; // No input gesture advances worker completion.
    if match_count(hwnd)? != 1 {
        return Err("Dirty project source did not yield exactly one multiline match".into());
    }
    {
        let state = owner(hwnd).unwrap().state.borrow();
        let panel = state
            .project_panel
            .as_ref()
            .ok_or("Project results panel missing")?;
        if !panel.visible || !panel.results {
            return Err("Project query did not display results".into());
        }
        let hit = state
            .project_search
            .rows()
            .iter()
            .find_map(|row| {
                if let ProjectSearchRow::Match(hit) = row {
                    Some(hit)
                } else {
                    None
                }
            })
            .unwrap();
        if hit.range != (0..12)
            || hit.position.line != 0
            || hit.position.column != 0
            || hit.path.canonicalize().ok() != options.fixture.canonicalize().ok()
        {
            return Err("Project result lost exact source range or scalar coordinates".into());
        }
    }
    checks.push("project_dirty_multiline_native_notification");
    if let Some(bitmap) = &options.bitmap {
        let output = bitmap.with_file_name("native-project-results.bmp");
        if output.exists() || output == options.fixture || output == options.report {
            return Err("Project bitmap must be a new owned sibling".into());
        }
        let before = snapshot(hwnd)?;
        let frame = {
            let mut state = owner(hwnd).unwrap().state.borrow_mut();
            paint::prepare(&mut state)
        };
        if snapshot(hwnd)? != before {
            return Err("Project frame preparation changed source or views".into());
        }
        paint::bitmap(hwnd, &frame, &output)?;
        if snapshot(hwnd)? != before {
            return Err("Project GDI composition changed source or views".into());
        }
        checks.push("project_results_gdi_read_only");
    }
    key(hwnd, 0x0d, Modifiers::NONE)?;
    expected(hwnd, b"Qalpha\r\nbeta\r\n")?;
    if snapshot(hwnd)?.0 != original.0 || visible(hwnd)? {
        return Err("Project result did not reuse dirty document and hide panel".into());
    }
    key(hwnd, 0x5a, Modifiers::CTRL)?;
    expected(hwnd, SOURCE)?;
    checks.push("project_enter_dirty_document_no_reload_one_undo");

    command(hwnd, CommandId::ViewSearchResults)?;
    {
        let state = owner(hwnd).unwrap().state.borrow();
        if !state.project_search.is_stale() {
            return Err("Undo did not invalidate open-source project results".into());
        }
    }
    let stale = snapshot(hwnd)?;
    key(hwnd, 0x0d, Modifiers::NONE)?;
    if snapshot(hwnd)? != stale {
        return Err("Stale project row navigated or changed the document".into());
    }
    expected(hwnd, SOURCE)?;
    key(hwnd, 0x74, Modifiers::NONE)?; // F5 refreshes the captured exact query.
    complete(hwnd)?;
    if match_count(hwnd)? != 0 {
        return Err("Refreshed project query reused stale dirty source".into());
    }
    checks.push("project_stale_rejected_refresh_no_hit");

    key(hwnd, 0x71, Modifiers::NONE)?; // F2 returns to the current draft.
    key(hwnd, 0x0d, Modifiers::NONE)?;
    key(hwnd, 0x1b, Modifiers::NONE)?;
    // Exercise delivery after cancellation without polling the controller here.
    let deadline = Instant::now() + Duration::from_millis(100);
    while Instant::now() < deadline {
        pump(hwnd)?;
        std::thread::sleep(Duration::from_millis(1));
    }
    {
        let state = owner(hwnd).unwrap().state.borrow();
        if state.project_search.state != ProjectSearchState::Cancelled
            || state
                .project_search
                .rows()
                .iter()
                .any(|r| matches!(r, ProjectSearchRow::Match(_)))
        {
            return Err("Cancelled project query revived results".into());
        }
    }
    if visible(hwnd)? {
        return Err("Project cancellation did not hide the panel".into());
    }
    expected(hwnd, SOURCE)?;
    checks.push("project_cancel_no_late_result_revival");

    let side = SideFile::new(parent.clone())?;
    start(hwnd, "disk_only_token", "project-search-side.txt")?;
    complete(hwnd)?;
    if match_count(hwnd)? != 1 {
        return Err("Owned disk project fixture did not yield a match".into());
    }
    side.change()?;
    let before = snapshot(hwnd)?;
    key(hwnd, 0x0d, Modifiers::NONE)?;
    if snapshot(hwnd)? != before {
        return Err("Changed disk result opened or moved the editor".into());
    }
    expected(hwnd, SOURCE)?;
    checks.push("project_changed_disk_fingerprint_rejected");
    side.cleanup()?;
    {
        let mut state = owner(hwnd).unwrap().state.borrow_mut();
        state.set_project_root(parent)?;
        if state.project_panel.is_some() || !state.project_search.rows().is_empty() {
            return Err("Project root replacement retained panel or result rows".into());
        }
    }
    expected(hwnd, SOURCE)?;
    if fs::read(&options.fixture).map_err(|e| e.to_string())? != SOURCE {
        return Err("Project smoke changed original fixture bytes".into());
    }
    checks.push("project_root_change_clears_results_source_preserved");
    Ok(checks)
}
