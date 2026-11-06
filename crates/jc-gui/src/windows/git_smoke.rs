//! Own-HWND automation over an integrator-created repository beside the source
//! fixture. Only the real read-only controllers execute Git; no process starts
//! or repository writes are implemented by this smoke component.
use super::{
    ffi::*,
    git_panel::GitIntent,
    owner, paint,
    project_smoke::snapshot,
    smoke::{command, expected, key, position},
};
use crate::SmokeOptions;
use jc_app::{
    diff_model::DiffSide,
    git_diff::GitDiffState,
    git_read::GitBlobSource,
    git_status::{GitStatusChange, GitStatusState},
    git_status_tasks::GitStatusTaskState,
    CommandId, Modifiers,
};
use std::{
    fs,
    path::Path,
    time::{Duration, Instant},
};

const SOURCE: &[u8] = b"alpha\r\nbeta\r\n";
const HEAD: &[u8] = b"base\r\nbeta\r\n";
const DIRTY: &[u8] = b"Qalpha\r\nbeta\r\n";

unsafe fn one_task(hwnd: Hwnd) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let mut message: Msg = std::mem::zeroed();
        if PeekMessageW(&mut message, hwnd, TASKS_MESSAGE, TASKS_MESSAGE, 1) != 0 {
            DispatchMessageW(&message);
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("Native Git notification deadline exceeded".into());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}
unsafe fn status_ready(hwnd: Hwnd) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        {
            let state = owner(hwnd).ok_or("Git smoke lost HWND")?.state.borrow();
            match state.git_status.state() {
                GitStatusTaskState::Ready => return Ok(()),
                GitStatusTaskState::Queued | GitStatusTaskState::Running => {}
                other => return Err(format!("Native Git status did not become ready: {other:?}")),
            }
        }
        if Instant::now() >= deadline {
            return Err("Git status delivery exceeded deadline".into());
        }
        one_task(hwnd)?;
    }
}
unsafe fn diff_ready(hwnd: Hwnd) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        {
            let state = owner(hwnd).ok_or("Git smoke lost HWND")?.state.borrow();
            match state.git_diff.state() {
                GitDiffState::Ready => return Ok(()),
                GitDiffState::Queued | GitDiffState::Running => {}
                other => return Err(format!("Native Git diff did not become ready: {other:?}")),
            }
        }
        if Instant::now() >= deadline {
            return Err("Git diff delivery exceeded deadline".into());
        }
        one_task(hwnd)?;
    }
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
unsafe fn job_metrics(hwnd: Hwnd) -> Result<(usize, usize, GitDiffState, bool), String> {
    let state = owner(hwnd).ok_or("Git smoke lost HWND")?.state.borrow();
    Ok((
        state.jobs.snapshot_bytes(),
        state.jobs.artifact_bytes(),
        state.git_diff.state(),
        state.git_diff.result().is_some(),
    ))
}
unsafe fn readonly_frame(hwnd: Hwnd, output: Option<&Path>) -> Result<(), String> {
    let before = snapshot(hwnd)?;
    let metrics = job_metrics(hwnd)?;
    let frame = {
        let mut state = owner(hwnd).ok_or("Git smoke lost HWND")?.state.borrow_mut();
        paint::prepare(&mut state)
    };
    if snapshot(hwnd)? != before || job_metrics(hwnd)? != metrics {
        return Err("Git frame preparation changed source, views or task admission".into());
    }
    if let Some(path) = output {
        if path.exists() {
            return Err("Git bitmap must be a new owned sibling".into());
        }
        paint::bitmap(hwnd, &frame, path)?;
    }
    if snapshot(hwnd)? != before || job_metrics(hwnd)? != metrics {
        return Err("Git GDI composition consumed tasks or changed source".into());
    }
    Ok(())
}
unsafe fn accepted(hwnd: Hwnd, source: GitBlobSource, right: &[u8]) -> Result<String, String> {
    let state = owner(hwnd).ok_or("Git smoke lost HWND")?.state.borrow();
    let result = state
        .git_diff
        .result()
        .ok_or("Git diff accepted no result")?;
    let left = match source {
        GitBlobSource::Head => HEAD,
        GitBlobSource::Index => SOURCE,
    };
    if result.identity.source != source
        || result.model.source_bytes(DiffSide::Left) != left
        || result.model.source_bytes(DiffSide::Right) != right
    {
        return Err("Git comparison mixed snapshot identity or original source bytes".into());
    }
    if (source == GitBlobSource::Head) != result.identity.commit.is_some() {
        return Err("Git HEAD/index commit identity is incorrect".into());
    }
    Ok(result.identity.blob.as_str().to_owned())
}

pub(super) unsafe fn run(hwnd: Hwnd, options: &SmokeOptions) -> Result<Vec<&'static str>, String> {
    expected(hwnd, SOURCE)?;
    let parent = options
        .fixture
        .parent()
        .ok_or("Git fixture has no repository parent")?
        .to_owned();
    let filename = options
        .fixture
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("Git smoke needs Unicode fixture filename")?;
    let mut checks = Vec::new();
    checks.push("git_embedded_runtime_requires_no_executable");
    {
        let mut state = owner(hwnd).unwrap().state.borrow_mut();
        state.close_selector();
        state.schedule_git_status();
    }
    one_task(hwnd)?;
    status_ready(hwnd)?;
    {
        let state = owner(hwnd).unwrap().state.borrow();
        let count = state
            .git_status
            .result()
            .ok_or("Automatic Git status omitted snapshot")?
            .status()
            .entries()
            .len();
        if state.git_panel.is_some()
            || state.git_status.ready()
            || state.git_summary() != format!("Git: {count} cambios")
        {
            return Err(
                "Git summary lost its accepted result or automatic status opened a modal".into(),
            );
        }
    }
    checks.push("git_background_status_summary_survives_completion_poll_without_modal");
    position(hwnd, 0, 0)?;
    SendMessageW(hwnd, WM_CHAR, 'Q' as usize, 0);
    expected(hwnd, DIRTY)?;
    command(hwnd, CommandId::GitRepositoryStatus)?;
    status_ready(hwnd)?; // Own worker notification, no keystroke accepts results.
    {
        let state = owner(hwnd).unwrap().state.borrow();
        if state.git_panel != Some(GitIntent::Status) {
            return Err("Git status panel was not displayed".into());
        }
        let status = state
            .git_status
            .result()
            .ok_or("Git status accepted no snapshot")?;
        let entry = status
            .status()
            .entries()
            .iter()
            .find(|entry| entry.path() == filename.as_bytes())
            .ok_or("Git status omitted tracked fixture")?;
        // HEAD differs from index, while disk equals index. Unsaved editor Q
        // must not masquerade as a Git worktree change.
        if entry.state()
            != (GitStatusState::Tracked {
                index: GitStatusChange::Modified,
                worktree: GitStatusChange::Unchanged,
            })
        {
            return Err("Git status did not preserve separate index/worktree states".into());
        }
    }
    let before = snapshot(hwnd)?;
    key(hwnd, 0x0d, Modifiers::NONE)?;
    if snapshot(hwnd)? != before {
        return Err("Git status comparison failed to reuse the dirty editor state".into());
    }
    expected(hwnd, DIRTY)?;
    let status_bitmap = options
        .bitmap
        .as_ref()
        .map(|path| path.with_file_name("native-git-status.bmp"));
    readonly_frame(hwnd, status_bitmap.as_deref())?;
    key(hwnd, 0x1b, Modifiers::NONE)?;
    if owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .git_status
        .result()
        .is_none()
    {
        return Err("Closing an accepted Git status discarded its status-bar summary".into());
    }
    checks.push("git_status_native_delivery_comparison_reuses_dirty_document");

    command(hwnd, CommandId::GitDiffHead)?;
    // Request may already admit its first snapshot. Painting must not add one
    // or consume a completion; only TASKS_MESSAGE below accepts worker output.
    readonly_frame(hwnd, None)?;
    diff_ready(hwnd)?;
    let head = accepted(hwnd, GitBlobSource::Head, DIRTY)?;
    let diff_bitmap = options
        .bitmap
        .as_ref()
        .map(|path| path.with_file_name("native-git-diff.bmp"));
    readonly_frame(hwnd, diff_bitmap.as_deref())?;
    key(hwnd, 0x72, Modifiers::NONE)?; // F3 switches the actual controller to index.
    diff_ready(hwnd)?;
    let index = accepted(hwnd, GitBlobSource::Index, DIRTY)?;
    if head == index {
        return Err("HEAD and index blob identities unexpectedly coincide".into());
    }
    checks.push("git_head_index_distinct_oid_exact_dirty_sources");
    checks.push("git_prepare_and_gdi_no_snapshot_admission_or_completion_consumption");

    let before = snapshot(hwnd)?;
    key(hwnd, 0x0d, Modifiers::NONE)?;
    let after = snapshot(hwnd)?;
    if (after.0, after.1, after.3) != (before.0, before.1, before.3)
        || owner(hwnd).unwrap().state.borrow().git_panel.is_some()
    {
        return Err("Git hunk navigation edited source or did not close panel".into());
    }
    expected(hwnd, DIRTY)?;
    checks.push("git_hunk_enter_navigation_without_edit");

    command(hwnd, CommandId::GitDiffHead)?;
    diff_ready(hwnd)?;
    // Simulate another shared editor view undoing the source while this modal
    // read model is visible. It uses the real transaction/history owner.
    owner(hwnd)
        .unwrap()
        .state
        .borrow_mut()
        .workbench
        .undo()
        .map_err(|e| e.to_string())?;
    expected(hwnd, SOURCE)?;
    {
        let mut state = owner(hwnd).unwrap().state.borrow_mut();
        state.git_presentation();
        if state.git_diff.state() != GitDiffState::Stale || state.git_diff.result().is_some() {
            return Err("Undo did not invalidate accepted Git source revision".into());
        }
    }
    let before = snapshot(hwnd)?;
    key(hwnd, 0x0d, Modifiers::NONE)?;
    if snapshot(hwnd)? != before {
        return Err("Stale Git hunk navigated or edited source".into());
    }
    key(hwnd, 0x74, Modifiers::NONE)?; // F5 explicit refresh after invalidation.
    diff_ready(hwnd)?;
    accepted(hwnd, GitBlobSource::Head, SOURCE)?;
    expected(hwnd, SOURCE)?;
    checks.push("git_shared_undo_stale_enter_denied_explicit_refresh");

    command(hwnd, CommandId::GitDiffIndex)?;
    key(hwnd, 0x1b, Modifiers::NONE)?;
    drain_cancelled(hwnd)?;
    {
        let state = owner(hwnd).unwrap().state.borrow();
        if state.git_panel.is_some()
            || state.git_diff.result().is_some()
            || state.git_diff.state() != GitDiffState::Cancelled
        {
            return Err("Cancelled Git delivery revived an accepted comparison".into());
        }
    }
    expected(hwnd, SOURCE)?;
    checks.push("git_cancel_late_notifications_no_result_revival");

    command(hwnd, CommandId::GitDiffHead)?;
    diff_ready(hwnd)?;
    accepted(hwnd, GitBlobSource::Head, SOURCE)?;
    command(hwnd, CommandId::GitRepositoryStatus)?;
    status_ready(hwnd)?;
    {
        let mut state = owner(hwnd).unwrap().state.borrow_mut();
        state.set_project_root(parent)?;
        if state.git_panel.is_some()
            || state.git_diff.result().is_some()
            || state.git_status.result().is_some()
            || state.git_status_options.is_some()
        {
            return Err("Repository root replacement retained Git read models".into());
        }
    }
    drain_cancelled(hwnd)?;
    expected(hwnd, SOURCE)?;
    if fs::read(&options.fixture).map_err(|e| e.to_string())? != SOURCE {
        return Err("Git smoke changed fixture bytes on disk".into());
    }
    checks.push("git_root_reconfiguration_clears_embedded_accepted_state");
    Ok(checks)
}
