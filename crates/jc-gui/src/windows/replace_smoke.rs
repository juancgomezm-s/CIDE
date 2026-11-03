//! Own-HWND replacement automation; the original smoke fixture is never saved.
//! All closed-file edits target two exact create_new siblings owned by this run.
use super::{
    ffi::*,
    owner, paint,
    project_smoke::{complete, snapshot, start},
    replace_panel::ReplaceStage,
    smoke::{command, expected, key, paste, position},
};
use crate::SmokeOptions;
use jc_app::{CommandId, Modifiers};
use jc_text::TextEncoding;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const SOURCE: &[u8] = b"alpha\r\nbeta\r\n";
const DIRTY: &[u8] = b"Qalpha\r\nbeta\r\n";
const REPLACED: &[u8] = b"uno\nDOS\r\nZ\r\n";
const REPLACEMENT: &str = "uno\nDOS\r\nZ";

struct OwnedFile {
    path: PathBuf,
    parent: PathBuf,
}
impl OwnedFile {
    fn new(parent: &Path, name: &str, bytes: &[u8]) -> Result<Self, String> {
        let parent = parent.canonicalize().map_err(|e| e.to_string())?;
        let path = parent.join(name);
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| e.to_string())?;
        let guard = Self { path, parent };
        let written = file.write_all(bytes);
        drop(file);
        written.map_err(|e| e.to_string())?;
        Ok(guard)
    }
    fn verify(&self) -> Result<(), String> {
        use std::os::windows::fs::MetadataExt;
        let m = fs::symlink_metadata(&self.path).map_err(|e| e.to_string())?;
        if !m.is_file()
            || m.file_attributes() & 0x400 != 0
            || self
                .path
                .canonicalize()
                .map_err(|e| e.to_string())?
                .parent()
                != Some(self.parent.as_path())
        {
            return Err("Replacement fixture ownership changed".into());
        }
        Ok(())
    }
    fn write(&self, bytes: &[u8]) -> Result<(), String> {
        self.verify()?;
        fs::write(&self.path, bytes).map_err(|e| e.to_string())
    }
    fn read(&self) -> Result<Vec<u8>, String> {
        self.verify()?;
        if fs::metadata(&self.path).map_err(|e| e.to_string())?.len() > 4096 {
            return Err("Replacement fixture exceeded byte budget".into());
        }
        fs::read(&self.path).map_err(|e| e.to_string())
    }
    fn cleanup(&self) -> Result<(), String> {
        self.verify()?;
        fs::remove_file(&self.path).map_err(|e| e.to_string())
    }
    fn disk_stamp(&self) -> Result<(u64, u64, u64), String> {
        use std::os::windows::fs::MetadataExt;
        self.verify()?;
        let metadata = fs::metadata(&self.path).map_err(|e| e.to_string())?;
        Ok((
            metadata.file_size(),
            metadata.creation_time(),
            metadata.last_write_time(),
        ))
    }
}
impl Drop for OwnedFile {
    fn drop(&mut self) {
        if self.verify().is_ok() {
            let _ = fs::remove_file(&self.path);
        }
    }
}

unsafe fn preview(hwnd: Hwnd, text: &str) -> Result<(), String> {
    preview_with_capture(hwnd, text, None)
}
unsafe fn preview_with_capture(
    hwnd: Hwnd,
    text: &str,
    bitmap: Option<&std::path::Path>,
) -> Result<(), String> {
    command(hwnd, CommandId::SearchReplaceProject)?;
    key(hwnd, 0x55, Modifiers::CTRL)?;
    paste(hwnd, text)?;
    if let Some(output) = bitmap {
        if output.exists() {
            return Err("Replacement draft bitmap must be new".into());
        }
        let before = snapshot(hwnd)?;
        let frame = {
            let mut state = owner(hwnd).unwrap().state.borrow_mut();
            paint::prepare(&mut state)
        };
        paint::bitmap(hwnd, &frame, output)?;
        if snapshot(hwnd)? != before {
            return Err("Replacement draft composition mutated source/views".into());
        }
    }
    key(hwnd, 0x0d, Modifiers::NONE)?;
    let state = owner(hwnd)
        .ok_or("Replacement smoke lost HWND")?
        .state
        .borrow();
    let panel = state
        .replace_panel
        .as_ref()
        .ok_or("Replacement draft did not open")?;
    if !matches!(&panel.stage, ReplaceStage::Preview) || panel.preview.is_none() {
        return Err("Enter did not create a replacement preview".into());
    }
    Ok(())
}
/// Remove and dispatch EXACTLY one notification. In particular this must not
/// drain a self-posted continuation before the partial-cancellation gesture.
unsafe fn one_task(hwnd: Hwnd) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let mut message: Msg = std::mem::zeroed();
        if PeekMessageW(&mut message, hwnd, TASKS_MESSAGE, TASKS_MESSAGE, 1) != 0 {
            DispatchMessageW(&message);
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("Replacement task notification deadline exceeded".into());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}
unsafe fn finished(hwnd: Hwnd) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        {
            let state = owner(hwnd)
                .ok_or("Replacement smoke lost HWND")?
                .state
                .borrow();
            let panel = state
                .replace_panel
                .as_ref()
                .ok_or("Replacement progress panel disappeared")?;
            if matches!(&panel.stage, ReplaceStage::Finished) {
                return Ok(());
            }
            if !matches!(&panel.stage, ReplaceStage::Applying) {
                return Err("Replacement did not enter applying/finished stage".into());
            }
        }
        if Instant::now() >= deadline {
            return Err("Replacement application deadline exceeded".into());
        }
        one_task(hwnd)?;
    }
}
unsafe fn counts(hwnd: Hwnd) -> Result<(usize, usize, usize, usize, usize), String> {
    let state = owner(hwnd)
        .ok_or("Replacement smoke lost HWND")?
        .state
        .borrow();
    let p = state
        .replace_panel
        .as_ref()
        .ok_or("Replacement panel missing")?;
    Ok((p.changed, p.conflicts, p.unchanged, p.total, p.processed))
}
unsafe fn drain_cancelled(hwnd: Hwnd) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_millis(100);
    while Instant::now() < deadline {
        let mut message: Msg = std::mem::zeroed();
        if PeekMessageW(&mut message, hwnd, TASKS_MESSAGE, TASKS_MESSAGE, 1) != 0 {
            DispatchMessageW(&message);
        } else {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    Ok(())
}

pub(super) unsafe fn run(hwnd: Hwnd, options: &SmokeOptions) -> Result<Vec<&'static str>, String> {
    expected(hwnd, SOURCE)?;
    let parent = options
        .fixture
        .parent()
        .ok_or("Replacement fixture has no parent")?
        .to_owned();
    let filename = options
        .fixture
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("Replacement smoke needs Unicode fixture filename")?;
    let a = OwnedFile::new(&parent, "cide-replace-a.txt", DIRTY)?;
    let b = OwnedFile::new(&parent, "cide-replace-b.txt", b"unused\r\n")?;
    let mut checks = Vec::new();
    let includes = format!("{filename},cide-replace-a.txt");
    position(hwnd, 0, 0)?;
    SendMessageW(hwnd, WM_CHAR, 'Q' as usize, 0);
    expected(hwnd, DIRTY)?;
    start(hwnd, "Qalpha\r\nbeta", &includes)?;
    complete(hwnd)?;
    let draft_bitmap = options
        .bitmap
        .as_ref()
        .map(|p| p.with_file_name("native-project-replace-draft.bmp"));
    preview_with_capture(hwnd, REPLACEMENT, draft_bitmap.as_deref())?;
    let before = snapshot(hwnd)?;
    key(hwnd, 0x0d, Modifiers::NONE)?; // Preview Enter must not commit.
    if snapshot(hwnd)? != before || counts(hwnd)?.4 != 0 || a.read()? != DIRTY {
        return Err("Preview Enter committed replacement without confirmation".into());
    }
    if let Some(bitmap) = &options.bitmap {
        let output = bitmap.with_file_name("native-project-replace.bmp");
        if output.exists() || output == options.fixture || output == options.report {
            return Err("Replacement bitmap must be a new sibling".into());
        }
        let frame = {
            let mut state = owner(hwnd).unwrap().state.borrow_mut();
            paint::prepare(&mut state)
        };
        paint::bitmap(hwnd, &frame, &output)?;
        if snapshot(hwnd)? != before {
            return Err("Replacement preview paint mutated editor state".into());
        }
        checks.push("project_replace_preview_gdi_read_only");
    }
    key(hwnd, 0x1b, Modifiers::NONE)?;
    expected(hwnd, DIRTY)?;
    if a.read()? != DIRTY || owner(hwnd).unwrap().state.borrow().replace_panel.is_some() {
        return Err("Cancelled preview retained or applied a plan".into());
    }
    checks.push("project_replace_preview_enter_no_commit_cancel");

    start(hwnd, "Qalpha\r\nbeta", &includes)?;
    complete(hwnd)?;
    preview(hwnd, REPLACEMENT)?;
    let revision = snapshot(hwnd)?.1;
    key(hwnd, 0x78, Modifiers::NONE)?; // F9 only schedules the first file.
    key(hwnd, 0x78, Modifiers::NONE)?; // Repeated confirmation cannot start again.
    if counts(hwnd)?.4 != 0 {
        return Err("Replacement confirmation applied synchronously".into());
    }
    finished(hwnd)?;
    if counts(hwnd)? != (2, 0, 0, 2, 2) {
        return Err("Opened/closed replacement summary is incorrect".into());
    }
    expected(hwnd, REPLACED)?;
    if a.read()? != REPLACED || snapshot(hwnd)?.1 == revision {
        return Err("Replacement lost exact EOL or one-transaction source edit".into());
    }
    let after = snapshot(hwnd)?;
    key(hwnd, 0x78, Modifiers::NONE)?;
    if snapshot(hwnd)? != after || a.read()? != REPLACED {
        return Err("Finished replacement was applied twice".into());
    }
    key(hwnd, 0x1b, Modifiers::NONE)?;
    key(hwnd, 0x5a, Modifiers::CTRL)?;
    expected(hwnd, DIRTY)?;
    key(hwnd, 0x5a, Modifiers::CTRL)?;
    expected(hwnd, SOURCE)?;
    if fs::read(&options.fixture).map_err(|e| e.to_string())? != SOURCE {
        return Err("Open-source replacement unexpectedly saved the source fixture".into());
    }
    checks.push("project_replace_open_dirty_closed_exact_eol_one_undo_confirm_once");

    a.write(b"disk_only_token\r\n")?;
    start(hwnd, "disk_only_token", "cide-replace-a.txt")?;
    complete(hwnd)?;
    preview(hwnd, "new")?;
    a.write(b"external_edit\r\n")?;
    key(hwnd, 0x0d, Modifiers::CTRL)?;
    finished(hwnd)?;
    if counts(hwnd)? != (0, 1, 0, 1, 1) || a.read()? != b"external_edit\r\n" {
        return Err("Replacement overwrote a changed disk source".into());
    }
    expected(hwnd, SOURCE)?;
    key(hwnd, 0x1b, Modifiers::NONE)?;
    checks.push("project_replace_disk_conflict_not_overwritten");

    a.write(b"partial_token\r\n")?;
    b.write(b"partial_token\r\n")?;
    start(hwnd, "partial_token", "cide-replace-*.txt")?;
    complete(hwnd)?;
    preview(hwnd, "changed")?;
    key(hwnd, 0x78, Modifiers::NONE)?;
    if counts(hwnd)?.4 != 0 {
        return Err("Partial replacement started before its task message".into());
    }
    one_task(hwnd)?;
    if counts(hwnd)? != (1, 0, 0, 2, 1) {
        return Err("One native task did not apply exactly one file".into());
    }
    key(hwnd, 0x1b, Modifiers::NONE)?;
    drain_cancelled(hwnd)?;
    let values = [a.read()?, b.read()?];
    if values
        .iter()
        .filter(|v| v.as_slice() == b"changed\r\n")
        .count()
        != 1
        || values
            .iter()
            .filter(|v| v.as_slice() == b"partial_token\r\n")
            .count()
            != 1
    {
        return Err("Partial cancellation changed an unaccepted file".into());
    }
    expected(hwnd, SOURCE)?;
    checks.push("project_replace_one_file_per_task_partial_cancel");

    // An empty draft is a valid deletion, not a missing required value.
    start(hwnd, "alpha", filename)?;
    complete(hwnd)?;
    preview(hwnd, "")?;
    let before = snapshot(hwnd)?;
    key(hwnd, 0x78, Modifiers::NONE)?;
    finished(hwnd)?;
    // Revision IDs identify history nodes; after Undo, a fresh branch need not
    // be the old ID plus one. The single Undo below proves transaction count.
    if counts(hwnd)? != (1, 0, 0, 1, 1) || snapshot(hwnd)?.1 == before.1 {
        return Err("Empty replacement did not apply one deletion transaction".into());
    }
    expected(hwnd, b"\r\nbeta\r\n")?;
    if fs::read(&options.fixture).map_err(|e| e.to_string())? != SOURCE {
        return Err("Empty open-document replacement saved the fixture".into());
    }
    key(hwnd, 0x1b, Modifiers::NONE)?;
    key(hwnd, 0x5a, Modifiers::CTRL)?;
    expected(hwnd, SOURCE)?;
    checks.push("project_replace_empty_draft_deletes_one_undo_preserves_eol_disk");

    // Keep deletion redo available: a no-op must neither add history nor erase
    // that redo branch, and a closed identical file must not be saved again.
    a.write(SOURCE)?;
    start(hwnd, "alpha", &includes)?;
    complete(hwnd)?;
    preview(hwnd, "alpha")?;
    let before = snapshot(hwnd)?;
    let disk_stamp = a.disk_stamp()?;
    if !owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .active_document()
        .unwrap()
        .session
        .document()
        .can_redo()
    {
        return Err("Deletion undo did not retain its redo branch".into());
    }
    key(hwnd, 0x0d, Modifiers::CTRL)?;
    finished(hwnd)?;
    if counts(hwnd)? != (0, 0, 2, 2, 2)
        || snapshot(hwnd)? != before
        || a.read()? != SOURCE
        || a.disk_stamp()? != disk_stamp
    {
        return Err("Identical replacement changed source, views or closed-file metadata".into());
    }
    expected(hwnd, SOURCE)?;
    if !owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .active_document()
        .unwrap()
        .session
        .document()
        .can_redo()
    {
        return Err("Identical replacement cleared undo/redo history".into());
    }
    key(hwnd, 0x1b, Modifiers::NONE)?;
    key(hwnd, 0x59, Modifiers::CTRL)?;
    expected(hwnd, b"\r\nbeta\r\n")?;
    key(hwnd, 0x5a, Modifiers::CTRL)?;
    expected(hwnd, SOURCE)?;
    checks.push("project_replace_identical_open_closed_no_revision_history_or_save");

    start(hwnd, "no_such_replace_token", filename)?;
    complete(hwnd)?;
    if owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .project_search
        .can_replace()
    {
        return Err("No-hit project search enabled replacement".into());
    }
    command(hwnd, CommandId::SearchReplaceProject)?;
    if owner(hwnd).unwrap().state.borrow().replace_panel.is_some() {
        return Err("No-hit replacement opened a draft".into());
    }
    checks.push("project_replace_no_hit_disabled");

    start(hwnd, "alpha\r\nbeta", filename)?;
    complete(hwnd)?;
    preview(hwnd, "not accepted")?;
    let before = snapshot(hwnd)?;
    owner(hwnd)
        .unwrap()
        .state
        .borrow_mut()
        .workbench
        .set_encoding(TextEncoding::Windows1252)
        .map_err(|e| e.to_string())?;
    key(hwnd, 0x78, Modifiers::NONE)?;
    {
        let state = owner(hwnd).unwrap().state.borrow();
        let panel = state
            .replace_panel
            .as_ref()
            .ok_or("Stale replacement did not retain its draft")?;
        if !matches!(&panel.stage, ReplaceStage::Draft)
            || panel.preview.is_some()
            || panel.draft != "not accepted"
        {
            return Err("Encoding change did not drop stale preview and preserve draft".into());
        }
    }
    expected(hwnd, SOURCE)?;
    if snapshot(hwnd)?.1 != before.1 {
        return Err("Stale preview changed source revision".into());
    }
    owner(hwnd)
        .unwrap()
        .state
        .borrow_mut()
        .workbench
        .set_encoding(TextEncoding::Utf8)
        .map_err(|e| e.to_string())?;
    key(hwnd, 0x1b, Modifiers::NONE)?;
    checks.push("project_replace_encoding_stale_preview_denied");

    start(hwnd, "alpha\r\nbeta", filename)?;
    complete(hwnd)?;
    preview(hwnd, "not accepted")?;
    {
        let mut state = owner(hwnd).unwrap().state.borrow_mut();
        state.set_project_root(parent)?;
        if state.replace_panel.is_some() || !state.project_search.rows().is_empty() {
            return Err("Root change retained replacement plans".into());
        }
    }
    drain_cancelled(hwnd)?;
    expected(hwnd, SOURCE)?;
    if fs::read(&options.fixture).map_err(|e| e.to_string())? != SOURCE {
        return Err("Replacement smoke modified original fixture on disk".into());
    }
    a.cleanup()?;
    b.cleanup()?;
    checks.push("project_replace_root_change_drops_plan_source_preserved");
    Ok(checks)
}
