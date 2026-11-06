//! Owned-HWND verification for the Git workspace layout and captured rails.
use super::super::{git_smoke, owner, project_smoke::snapshot, smoke::key};
use super::*;
use jc_app::Modifiers;
use std::path::Path;
unsafe fn publish(hwnd: Hwnd) {
    InvalidateRect(hwnd, std::ptr::null(), 0);
    SendMessageW(hwnd, WM_PAINT, 0, 0);
}
unsafe fn point(hwnd: Hwnd, predicate: impl Fn(Intent) -> bool) -> Result<(i32, i32), String> {
    publish(hwnd);
    let s = owner(hwnd)
        .ok_or("Git layout lost own window")?
        .state
        .borrow();
    s.git_workspace
        .hits
        .as_ref()
        .and_then(|h| h.actions.iter().find(|(_, a)| predicate(*a)))
        .map(|(r, _)| ((r.left + r.right) / 2, (r.top + r.bottom) / 2))
        .ok_or("No current Git control hit".into())
}
fn packed(x: i32, y: i32) -> isize {
    ((x as u16 as usize) | ((y as u16 as usize) << 16)) as isize
}
unsafe fn click(hwnd: Hwnd, predicate: impl Fn(Intent) -> bool) -> Result<(), String> {
    let (x, y) = point(hwnd, predicate)?;
    SendMessageW(hwnd, WM_LBUTTONDOWN, 1, packed(x, y));
    SendMessageW(hwnd, WM_LBUTTONUP, 0, packed(x, y));
    publish(hwnd);
    Ok(())
}
pub(in crate::windows) unsafe fn run(
    hwnd: Hwnd,
    bitmap: Option<&Path>,
    checks: &mut Vec<&'static str>,
) -> Result<(), String> {
    publish(hwnd); // Settle the first paint after the deliberately injected long fixture.
    let before = snapshot(hwnd)?;
    let prefs = owner(hwnd)
        .ok_or("No layout HWND")?
        .state
        .borrow()
        .git_workspace
        .preference_bytes();
    click(hwnd, |a| matches!(a, Intent::DiffView(_)))?;
    {
        let mut s = owner(hwnd).unwrap().state.borrow_mut();
        let f = s.prepare_git_workspace().ok_or("No parallel frame")?;
        if !s.git_workspace.preferences.parallel
            || f.code.is_empty()
            || f.code.iter().any(|r| !r.parallel)
        {
            return Err("Parallel mode did not project both sources".into());
        }
    }
    if let Some(path) = bitmap {
        git_smoke::readonly_frame(hwnd, Some(&path.with_file_name("native-git-parallel.bmp")))?;
    }
    for arrangement in Arrangement::ALL {
        click(hwnd, |a| matches!(a, Intent::DistributionMenu))?;
        click(
            hwnd,
            |a| matches!(a,Intent::Arrange(actual) if actual==arrangement),
        )?;
        let mut s = owner(hwnd).unwrap().state.borrow_mut();
        if s.git_workspace.preferences.arrangement != arrangement {
            return Err("Layout menu did not apply chosen arrangement".into());
        }
        let f = s.prepare_git_workspace().ok_or("Missing layout frame")?;
        if f.code.is_empty() {
            return Err("Layout discarded accepted comparison".into());
        }
        drop(s);
        if let Some(path) = bitmap {
            let name = match arrangement {
                Arrangement::One => "native-git-one.bmp",
                Arrangement::TwoFilesLeft => "native-git-two-left.bmp",
                Arrangement::TwoFilesRight => "native-git-two-right.bmp",
                Arrangement::Three => "native-git-three.bmp",
            };
            git_smoke::readonly_frame(hwnd, Some(&path.with_file_name(name)))?;
        }
    }
    if let Some(path) = bitmap {
        git_smoke::readonly_frame(
            hwnd,
            Some(&path.with_file_name("native-git-three-columns.bmp")),
        )?;
    }
    // Actual window capture belongs to the client while dragging; leaving the
    // rail must not release it before the matching mouse up.
    for horizontal in [false, true] {
        let (x, y) = point(
            hwnd,
            |a| matches!(a,Intent::Rail(r) if r.horizontal==horizontal),
        )?;
        SendMessageW(hwnd, WM_LBUTTONDOWN, 1, packed(x, y));
        if GetCapture() != hwnd || !owner(hwnd).unwrap().state.borrow().git_workspace.dragging() {
            return Err("Git rail did not retain native capture".into());
        }
        SendMessageW(hwnd, WM_MOUSEMOVE, 1, packed(950, 570));
        if GetCapture() != hwnd {
            return Err("Git drag lost client capture".into());
        }
        SendMessageW(hwnd, WM_LBUTTONUP, 0, packed(950, 570));
        if GetCapture() == hwnd || owner(hwnd).unwrap().state.borrow().git_workspace.dragging() {
            return Err("Git mouse-up left capture active".into());
        }
        // Bring long visible rows back before testing the other orientation.
        owner(hwnd)
            .unwrap()
            .state
            .borrow_mut()
            .git_workspace
            .diff_first = 0;
        publish(hwnd);
    }
    let before_drag = owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .git_workspace
        .preference_bytes();
    let (x, y) = point(hwnd, |a| matches!(a, Intent::Divider(Divider::First)))?;
    SendMessageW(hwnd, WM_LBUTTONDOWN, 1, packed(x, y));
    SendMessageW(hwnd, WM_MOUSEMOVE, 1, packed(x + 45, y));
    SendMessageW(hwnd, WM_LBUTTONUP, 0, packed(x + 45, y));
    let changed = owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .git_workspace
        .preference_bytes();
    if changed == before_drag {
        return Err("Native divider drag did not update saved geometry".into());
    }
    let after = snapshot(hwnd)?;
    if after != before {
        return Err(format!(
            "Git layout changed editor geometry: before={before:?}; after={after:?}"
        ));
    }
    checks.push("git_layout_native_menu_four_arrangements_same_accepted_sources");
    checks.push("git_parallel_native_projection_and_gdi_preserve_source_geometry");
    checks.push("git_rails_and_divider_native_capture_move_release");
    owner(hwnd)
        .unwrap()
        .state
        .borrow_mut()
        .git_workspace
        .restore_preferences(&prefs)?;
    key(hwnd, 0x1b, Modifiers::NONE)?;
    Ok(())
}
