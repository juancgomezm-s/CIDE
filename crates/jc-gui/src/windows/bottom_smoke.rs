//! Synthetic F3 producers exercise the real queue/notification/host, not AVM.
use super::{
    ffi::*,
    owner, paint,
    project_smoke::snapshot,
    smoke::{command, expected, key, position},
    State,
};
use crate::SmokeOptions;
use jc_app::task_panels::PanelKind;
use jc_app::{
    Cancellation, CommandId, JobKey, Modifiers, PaneId, TaskFailure, TaskItem, TaskOutput,
    TaskProvider, TaskSnapshot,
};
use jc_text::TextEncoding;
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
static RELEASE: AtomicBool = AtomicBool::new(true);
const SOURCE: &[u8] = b"alpha\r\nbeta\r\n";
#[link(name = "user32")]
extern "system" {
    fn SetWindowPos(
        hwnd: Hwnd,
        after: Hwnd,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        flags: u32,
    ) -> i32;
}
fn fixture(s: &TaskSnapshot, c: &Cancellation, out: &mut TaskOutput) -> Result<(), TaskFailure> {
    while !RELEASE.load(Ordering::Acquire) {
        if c.is_cancelled() {
            return Err(TaskFailure::Cancelled);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    out.push(TaskItem::Diagnostic {
        range: s.range.start..s.range.start + 1,
        message: "fixture diagnostic".into(),
    })?;
    out.push(TaskItem::Message(
        "fixture output: cmd /c echo inert\n\u{1b}".into(),
    ))?;
    out.push(TaskItem::ProposedEdit {
        range: s.range.start..s.range.start + 1,
        text: "DO NOT APPLY".into(),
    })
}
fn failure(_: &TaskSnapshot, _: &Cancellation, _: &mut TaskOutput) -> Result<(), TaskFailure> {
    Err(TaskFailure::Failed)
}
fn excessive(_: &TaskSnapshot, _: &Cancellation, out: &mut TaskOutput) -> Result<(), TaskFailure> {
    for _ in 0..65 {
        let _ = out.push(TaskItem::Message("over quota".into()));
    }
    Ok(())
}
unsafe fn submit(hwnd: Hwnd, key: u64, provider: TaskProvider) -> Result<(), String> {
    let mut s = owner(hwnd).unwrap().state.borrow_mut();
    let State {
        workbench, jobs, ..
    } = &mut *s;
    let doc = workbench
        .active_document()
        .ok_or("No F3 fixture document")?;
    let view = workbench
        .panes()
        .view(workbench.panes().active())
        .and_then(|v| v.view_id());
    jobs.submit(
        workbench,
        JobKey(key),
        doc.id,
        view,
        0..doc.session.document().len(),
        provider,
    )
    .map_err(|e| format!("F3 fixture admission {e:?}"))?;
    Ok(())
}
unsafe fn complete(hwnd: Hwnd) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let mut msg: Msg = std::mem::zeroed();
        while PeekMessageW(&mut msg, hwnd, TASKS_MESSAGE, TASKS_MESSAGE, 1) != 0 {
            DispatchMessageW(&msg);
            if Instant::now() >= deadline {
                return Err("F3 notification storm".into());
            }
        }
        if owner(hwnd).unwrap().state.borrow().jobs.outstanding() == 0 {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("F3 own-HWND delivery deadline".into());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}
unsafe fn palette(hwnd: Hwnd, label: &str) -> Result<(), String> {
    key(hwnd, 0x50, Modifiers::CTRL.union(Modifiers::SHIFT))?;
    for unit in label.encode_utf16() {
        SendMessageW(hwnd, WM_CHAR, unit as usize, 0);
    }
    key(hwnd, 0x0d, Modifiers::NONE)
}
unsafe fn reset(hwnd: Hwnd) {
    let mut s = owner(hwnd).unwrap().state.borrow_mut();
    s.close_selector();
    s.workbench.reset_task_panels();
    s.bottom = Default::default();
    s.bottom_hits = None;
    s.explorer_visible = false;
    s.explorer_focused = false;
}
pub(super) unsafe fn run(hwnd: Hwnd, options: &SmokeOptions) -> Result<Vec<&'static str>, String> {
    expected(hwnd, SOURCE)?;
    complete(hwnd)?;
    reset(hwnd);
    let baseline = snapshot(hwnd)?;
    let existing_workers = owner(hwnd).unwrap().state.borrow().jobs.worker_count();
    let mut checks = Vec::new();
    command(hwnd, CommandId::ViewProblems)?;
    {
        let mut s = owner(hwnd).unwrap().state.borrow_mut();
        let f = s
            .bottom_presentation()
            .ok_or("Missing empty bottom frame")?;
        if !f.rows.is_empty()
            || !f.message.contains("Empty")
            || s.jobs.worker_count() != existing_workers
        {
            return Err("Opening empty panel admitted a provider".into());
        }
    }
    let before = snapshot(hwnd)?;
    for id in [
        CommandId::EditIndent,
        CommandId::EditDuplicate,
        CommandId::EditSplitLine,
    ] {
        command(hwnd, id)?;
    }
    SendMessageW(hwnd, WM_CHAR, b'X' as usize, 0);
    if snapshot(hwnd)? != before {
        return Err("Bottom input edited hidden source".into());
    }
    key(hwnd, 0x1b, Modifiers::NONE)?;
    {
        let mut s = owner(hwnd).unwrap().state.borrow_mut();
        s.explorer_visible = true;
    }
    key(hwnd, 0x75, Modifiers::NONE)?;
    if !owner(hwnd).unwrap().state.borrow().explorer_focused {
        return Err("F6 missed Explorer".into());
    }
    key(hwnd, 0x75, Modifiers::NONE)?;
    if !owner(hwnd).unwrap().state.borrow().bottom.focused {
        return Err("F6 missed bottom region".into());
    }
    key(hwnd, 0x75, Modifiers::NONE)?;
    if owner(hwnd).unwrap().state.borrow().context().focus != jc_app::PrimaryFocus::Editor {
        return Err("F6 failed return Editor".into());
    }
    key(hwnd, 0x75, Modifiers::SHIFT)?;
    if !owner(hwnd).unwrap().state.borrow().bottom.focused {
        return Err("ShiftF6 did not reverse regions".into());
    }
    key(hwnd, 0x32, Modifiers::ALT)?;
    if owner(hwnd).unwrap().state.borrow().bottom.kind != PanelKind::Output {
        return Err("Bottom Alt2 did not switch Output".into());
    }
    palette(hwnd, "Problems")?;
    if owner(hwnd).unwrap().state.borrow().bottom.kind != PanelKind::Problems {
        return Err("Palette Problems missed host".into());
    }
    key(hwnd, 0x4a, Modifiers::CTRL)?;
    if owner(hwnd).unwrap().state.borrow().bottom.visible {
        return Err("CtrlJ did not hide panel".into());
    }
    checks.push("bottom_empty_menu_palette_three_region_focus");

    reset(hwnd);
    {
        let mut s = owner(hwnd).unwrap().state.borrow_mut();
        s.workbench.new_file(
            options.fixture.with_file_name("bottom-memory.txt"),
            TextEncoding::Utf8,
        );
        s.workbench
            .insert_text("one\r\ntwo")
            .map_err(|e| e.to_string())?;
    }
    position(hwnd, 1, 2)?;
    command(hwnd, CommandId::EditorSplitRight)?;
    command(hwnd, CommandId::EditorTogglePin)?;
    let before = snapshot(hwnd)?;
    submit(hwnd, 601, fixture)?;
    complete(hwnd)?;
    {
        let s = owner(hwnd).unwrap().state.borrow();
        if s.workbench.problems().len() != 1 || s.workbench.output().len() < 2 {
            return Err("F3 typed output did not reach shared logs".into());
        }
    }
    if snapshot(hwnd)? != before {
        return Err("F3 completion mutated document/views/undo".into());
    }
    command(hwnd, CommandId::ViewProblems)?;
    key(hwnd, 0x24, Modifiers::NONE)?;
    {
        let mut s = owner(hwnd).unwrap().state.borrow_mut();
        let presentation = s
            .bottom_presentation()
            .ok_or("Missing Problems provenance frame")?;
        if !presentation.rows.first().is_some_and(|r| {
            r.label.contains("bottom-memory.txt")
                && r.label.contains("UTF-8")
                && r.label.contains("0..1")
        }) {
            return Err("Problem presentation omitted typed source provenance".into());
        }
    }
    if let Some(bitmap) = &options.bitmap {
        let output = bitmap.with_file_name("native-bottom-problems.bmp");
        if output.exists() {
            return Err("Problems bitmap must be new".into());
        }
        let frame = {
            let mut s = owner(hwnd).unwrap().state.borrow_mut();
            paint::prepare(&mut s)
        };
        paint::bitmap(hwnd, &frame, &output)?;
    }
    key(hwnd, 0x0d, Modifiers::NONE)?;
    {
        let s = owner(hwnd).unwrap().state.borrow();
        if s.bottom.focused
            || s.workbench.active_position() != Some(jc_text::TextPosition { line: 0, column: 0 })
            || !s
                .workbench
                .active_document()
                .unwrap()
                .session
                .document()
                .is_modified()
            || !s
                .workbench
                .panes()
                .view(PaneId::Primary)
                .unwrap()
                .is_pinned()
        {
            return Err("Typed problem navigation lost dirty/pin or target".into());
        }
    }
    expected(hwnd, b"one\r\ntwo")?;
    command(hwnd, CommandId::ViewOutput)?;
    let before = snapshot(hwnd)?;
    key(hwnd, 0x0d, Modifiers::NONE)?;
    if snapshot(hwnd)? != before {
        return Err("Output executed a proposal or navigated".into());
    }
    checks.push("bottom_f3_delivery_navigation_dirty_split_pin");

    command(hwnd, CommandId::EditorFocusPrimary)?;
    command(hwnd, CommandId::EditorOneGroup)?;
    for mutation in 0..3 {
        reset(hwnd);
        RELEASE.store(false, Ordering::Release);
        submit(hwnd, 610 + mutation, fixture)?;
        {
            let mut s = owner(hwnd).unwrap().state.borrow_mut();
            match mutation {
                0 => s.workbench.insert_text("X").map_err(|e| e.to_string())?,
                1 => s
                    .workbench
                    .set_encoding(TextEncoding::Windows1252)
                    .map_err(|e| e.to_string())?,
                _ => s.jobs.cancel(JobKey(612)),
            }
        }
        RELEASE.store(true, Ordering::Release);
        complete(hwnd)?;
        if !owner(hwnd)
            .unwrap()
            .state
            .borrow()
            .workbench
            .problems()
            .is_empty()
        {
            return Err("Stale/cancelled F3 output accepted".into());
        }
        {
            let mut s = owner(hwnd).unwrap().state.borrow_mut();
            match mutation {
                0 => s.workbench.undo().map_err(|e| e.to_string())?,
                1 => s
                    .workbench
                    .set_encoding(TextEncoding::Utf8)
                    .map_err(|e| e.to_string())?,
                _ => {}
            }
        }
    }
    reset(hwnd);
    submit(hwnd, 620, fixture)?;
    complete(hwnd)?;
    let old = owner(hwnd).unwrap().state.borrow().workbench.problems()[0].id;
    let saved = options.fixture.with_file_name("bottom-save-as-owned.txt");
    if saved.exists() {
        return Err("Owned SaveAs target already exists".into());
    }
    {
        owner(hwnd)
            .unwrap()
            .state
            .borrow_mut()
            .workbench
            .save_active_as(&saved)
            .map_err(|e| e.to_string())?;
    }
    let before = snapshot(hwnd)?;
    if owner(hwnd)
        .unwrap()
        .state
        .borrow_mut()
        .workbench
        .activate_problem(old)
        .is_ok()
        || snapshot(hwnd)? != before
    {
        return Err("SaveAs path change revived diagnostic".into());
    }
    if std::fs::read(&saved).map_err(|e| e.to_string())? != b"one\r\ntwo" {
        return Err("Owned SaveAs bytes unexpected".into());
    }
    std::fs::remove_file(&saved).map_err(|e| e.to_string())?;
    reset(hwnd);
    RELEASE.store(false, Ordering::Release);
    submit(hwnd, 621, fixture)?;
    let root = options.fixture.parent().ok_or("No root")?.to_owned();
    owner(hwnd)
        .unwrap()
        .state
        .borrow_mut()
        .set_project_root(root)
        .map_err(|e| e.to_string())?;
    RELEASE.store(true, Ordering::Release);
    complete(hwnd)?;
    if !owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .problems()
        .is_empty()
    {
        return Err("Root invalidation revived diagnostics".into());
    }
    checks.push("bottom_stale_revision_encoding_path_root_cancel");

    reset(hwnd);
    submit(hwnd, 630, failure)?;
    complete(hwnd)?;
    {
        let s = owner(hwnd).unwrap().state.borrow();
        if s.workbench.output().is_empty() {
            return Err("Failed F3 completion has no state".into());
        }
    }
    submit(hwnd, 631, excessive)?;
    complete(hwnd)?;
    if owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .output()
        .iter()
        .any(|r| r.text == "over quota")
    {
        return Err("Partial over-limit task output published".into());
    }
    submit(hwnd, 630, fixture)?;
    complete(hwnd)?;
    if owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .problems()
        .len()
        != 1
    {
        return Err("F3 service did not restart after failure".into());
    }
    expected(hwnd, b"one\r\ntwo")?;
    {
        let mut s = owner(hwnd).unwrap().state.borrow_mut();
        for _ in 0..200 {
            s.workbench.push_output("log ".repeat(2000));
        }
    }
    command(hwnd, CommandId::ViewOutput)?;
    key(hwnd, 0x23, Modifiers::NONE)?;
    {
        let mut s = owner(hwnd).unwrap().state.borrow_mut();
        let f = s.bottom_presentation().ok_or("No bounded output frame")?;
        if f.rows.len() > 128
            || f.rows.iter().any(|r| r.label.chars().count() > 512)
            || s.workbench.task_panels().dropped(PanelKind::Output) == 0
            || s.workbench.task_panels().truncated(PanelKind::Output) == 0
        {
            return Err("Output quotas/partial accounting missing".into());
        }
    }
    checks.push("bottom_failure_quota_restart_proposals_inert");

    {
        let mut s = owner(hwnd).unwrap().state.borrow_mut();
        paint::prepare(&mut s);
    }
    let before = snapshot(hwnd)?;
    let metrics = {
        let s = owner(hwnd).unwrap().state.borrow();
        (
            s.jobs.outstanding(),
            s.jobs.snapshot_bytes(),
            s.jobs.artifact_bytes(),
        )
    };
    for (width, height) in [(360, 170), (360, 320), (1400, 800)] {
        if SetWindowPos(hwnd, std::ptr::null_mut(), 0, 0, width, height, 0x16) == 0 {
            return Err("Bottom owned resize failed".into());
        }
        let mut s = owner(hwnd).unwrap().state.borrow_mut();
        let _frame = paint::prepare(&mut s);
    }
    {
        let mut s = owner(hwnd).unwrap().state.borrow_mut();
        paint::prepare(&mut s);
    }
    let settled = snapshot(hwnd)?;
    let frame = {
        let mut s = owner(hwnd).unwrap().state.borrow_mut();
        paint::prepare(&mut s)
    };
    if let Some(bitmap) = &options.bitmap {
        let path = bitmap.with_file_name("native-bottom.bmp");
        if path.exists() {
            return Err("Bottom bitmap must be new".into());
        }
        paint::bitmap(hwnd, &frame, &path)?;
    }
    if snapshot(hwnd)? != settled {
        return Err("Bottom composition changed views".into());
    }
    {
        let s = owner(hwnd).unwrap().state.borrow();
        if metrics
            != (
                s.jobs.outstanding(),
                s.jobs.snapshot_bytes(),
                s.jobs.artifact_bytes(),
            )
            || snapshot(hwnd)?.0 != before.0
            || snapshot(hwnd)?.1 != before.1
        {
            return Err("Bottom prepare admitted/polled or edited".into());
        }
    }
    SendMessageW(hwnd, WM_PAINT, 0, 0);
    let hit = {
        let s = owner(hwnd).unwrap().state.borrow();
        s.bottom_hits
            .as_ref()
            .and_then(|h| h.rows.first())
            .map(|(_, r)| (r.left + 4, r.top + 2))
    };
    if let Some((x, y)) = hit {
        SendMessageW(hwnd, WM_LBUTTONDOWN, 1, ((y as isize) << 16) | (x as isize));
    }
    reset(hwnd);
    if let Some((x, y)) = hit {
        let before = snapshot(hwnd)?;
        let mut s = owner(hwnd).unwrap().state.borrow_mut();
        s.bottom.visible = true;
        s.bottom.focused = true;
        drop(s);
        SendMessageW(
            hwnd,
            WM_LBUTTONDBLCLK,
            1,
            ((y as isize) << 16) | (x as isize),
        );
        if snapshot(hwnd)? != before {
            return Err("Old panel hit targeted new log".into());
        }
    }
    checks.push("bottom_visible_rows_resize_hits_readonly");

    reset(hwnd);
    owner(hwnd)
        .unwrap()
        .state
        .borrow_mut()
        .workbench
        .close_active(true)
        .map_err(|e| e.to_string())?;
    if SetWindowPos(hwnd, std::ptr::null_mut(), 0, 0, 1000, 700, 0x16) == 0 {
        return Err("Bottom owned size restore failed".into());
    }
    expected(hwnd, SOURCE)?;
    let after = snapshot(hwnd)?;
    if after.0 != baseline.0
        || after.1 != baseline.1
        || after.3 != baseline.3
        || std::fs::read(&options.fixture).map_err(|e| e.to_string())? != SOURCE
    {
        return Err("Bottom cleanup changed original source/catalog".into());
    }
    checks.push("bottom_original_source_cleanup");
    Ok(checks)
}
