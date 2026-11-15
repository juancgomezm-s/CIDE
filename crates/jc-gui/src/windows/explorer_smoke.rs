//! Own-window Explorer integration over an exact, disposable metadata fixture.
//! All input messages target this HWND; paths never come from displayed labels.
use super::{
    explorer_panel,
    ffi::*,
    owner, paint,
    project_smoke::snapshot,
    smoke::{command, expected, key, position},
};
use crate::SmokeOptions;
use jc_app::{
    explorer::{ExplorerLoadState, NodeId},
    CommandId, Modifiers, PaneId,
};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    ptr::null_mut,
    time::{Duration, Instant},
};

const SOURCE: &[u8] = b"alpha\r\nbeta\r\n";
const DIRTY: &[u8] = b"Qalpha\r\nbeta\r\n";
const NESTED: &[u8] = b"nested\r\n";
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
    fn GetWindowRect(hwnd: Hwnd, rect: *mut Rect) -> i32;
    fn ClientToScreen(hwnd: Hwnd, point: *mut Point) -> i32;
}

struct OwnedOverflow {
    root: PathBuf,
    parent: PathBuf,
    files: Vec<PathBuf>,
}
impl OwnedOverflow {
    fn new(parent: &Path) -> Result<Self, String> {
        let parent = parent.canonicalize().map_err(|e| e.to_string())?;
        let root = parent.join("cide-explorer-scroll-smoke");
        fs::create_dir(&root).map_err(|e| e.to_string())?;
        let mut own = Self {
            root,
            parent,
            files: Vec::new(),
        };
        for index in 0..80 {
            let path = own.root.join(format!("archivo-{index:03}.txt"));
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|e| e.to_string())?;
            own.files.push(path);
            file.write_all(b"owned scroll fixture\r\n")
                .map_err(|e| e.to_string())?;
        }
        Ok(own)
    }
}
impl Drop for OwnedOverflow {
    fn drop(&mut self) {
        // Only exact registered files, never recursive deletion or catalog paths.
        if !ordinary(&self.root).unwrap_or(false)
            || self
                .root
                .canonicalize()
                .ok()
                .as_deref()
                .and_then(Path::parent)
                != Some(self.parent.as_path())
        {
            return;
        }
        for path in &self.files {
            if ordinary(path).unwrap_or(false)
                && path.canonicalize().ok().as_deref().and_then(Path::parent)
                    == Some(self.root.as_path())
            {
                let _ = fs::remove_file(path);
            }
        }
        let _ = fs::remove_dir(&self.root);
    }
}

unsafe fn scroll_messages(hwnd: Hwnd, parent: &Path) -> Result<Vec<&'static str>, String> {
    let own = OwnedOverflow::new(parent)?;
    let (previous_root, previous_focus) = {
        let state = owner(hwnd)
            .ok_or("Explorer scroll lost HWND")?
            .state
            .borrow();
        (
            state
                .project_root
                .clone()
                .ok_or("Explorer scroll needs its fixture root")?,
            state.explorer_focused,
        )
    };
    let result = (|| {
        owner(hwnd)
            .unwrap()
            .state
            .borrow_mut()
            .set_project_root(own.root.canonicalize().map_err(|e| e.to_string())?)?;
        let root = owner(hwnd)
            .unwrap()
            .state
            .borrow()
            .explorer
            .root_id()
            .unwrap();
        ready(hwnd, root)?;
        {
            let mut state = owner(hwnd).unwrap().state.borrow_mut();
            state.explorer_focused = previous_focus;
            state.key_override = Some(Modifiers::NONE);
            state.smoke_wheel_lines = Some(3);
        }
        actual_paint(hwnd);
        let (rect, selected, before) = {
            let state = owner(hwnd).unwrap().state.borrow();
            let hits = state
                .explorer_hits
                .as_ref()
                .ok_or("Scroll paint published no Explorer hits")?;
            if hits.first != 0 || hits.total <= hits.capacity {
                return Err("Scroll fixture did not overflow from origin zero".into());
            }
            (hits.rect, state.explorer_view.selected(), snapshot(hwnd)?)
        };
        let mut point = Point {
            x: rect.left + 20,
            y: rect.top + 100,
        };
        if ClientToScreen(hwnd, &mut point) == 0 {
            return Err("Cannot translate own wheel point".into());
        }
        let packed = |x: i32, y: i32| ((y as u16 as usize) << 16 | x as u16 as usize) as isize;
        SendMessageW(
            hwnd,
            WM_MOUSEWHEEL,
            (-120i16 as u16 as usize) << 16,
            packed(point.x, point.y),
        );
        {
            let state = owner(hwnd).unwrap().state.borrow();
            if state.explorer_view.scroll() == 0
                || state.explorer_view.selected() != selected
                || state.explorer_focused != previous_focus
                || state.explorer_hits.is_some()
            {
                return Err(
                    "Native wheel lost selection/focus, failed scroll, or retained stale hits"
                        .into(),
                );
            }
        }
        // No paint between the new viewport and this old row coordinate. It must
        // not open a different file using a previous frame's typed identity.
        SendMessageW(
            hwnd,
            WM_LBUTTONDBLCLK,
            1,
            packed(rect.left + 70, rect.top + 70),
        );
        if snapshot(hwnd)? != before {
            return Err("Stale Explorer row opened/navigated another identity".into());
        }
        actual_paint(hwnd);
        SendMessageW(
            hwnd,
            WM_LBUTTONDOWN,
            1,
            packed(rect.right - 4, rect.bottom - 1),
        );
        if GetCapture() != hwnd
            || owner(hwnd)
                .unwrap()
                .state
                .borrow()
                .explorer_scroll
                .drag
                .is_none()
        {
            return Err("Explorer rail did not retain own HWND capture".into());
        }
        SendMessageW(
            hwnd,
            WM_MOUSEMOVE,
            1,
            packed(rect.right - 4, rect.top - 100),
        );
        {
            let state = owner(hwnd).unwrap().state.borrow();
            if state.explorer_view.scroll() != 0
                || state.explorer_view.selected() != selected
                || state.explorer_focused != previous_focus
                || GetCapture() != hwnd
            {
                return Err(
                    "Explorer drag did not reach origin with stable selection/focus/capture".into(),
                );
            }
        }
        SendMessageW(
            hwnd,
            WM_LBUTTONUP,
            0,
            packed(rect.right - 4, rect.top - 100),
        );
        if GetCapture() == hwnd
            || owner(hwnd)
                .unwrap()
                .state
                .borrow()
                .explorer_scroll
                .drag
                .is_some()
        {
            return Err("Explorer rail left mouse capture after button release".into());
        }
        if snapshot(hwnd)? != before {
            return Err("Explorer scrolling edited source or its caret".into());
        }
        Ok(vec![
            "explorer_native_wheel_preserves_focus_selection_source",
            "explorer_native_stale_viewport_click_rejected",
            "explorer_native_rail_capture_drag_release",
        ])
    })();
    // Restore the ordinary catalog before the exact owned fixture is removed.
    let restored = {
        let mut state = owner(hwnd)
            .ok_or("Explorer scroll lost HWND")?
            .state
            .borrow_mut();
        state.cancel_pointer();
        state.key_override = None;
        state.smoke_wheel_lines = None;
        let restored = state.set_project_root(previous_root);
        state.explorer_focused = previous_focus;
        restored
    };
    restored?;
    let root = owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .explorer
        .root_id()
        .unwrap();
    ready(hwnd, root)?;
    result
}
pub(super) struct OwnSize {
    hwnd: Hwnd,
    rect: Rect,
}
impl OwnSize {
    pub(super) unsafe fn new(hwnd: Hwnd) -> Result<Self, String> {
        let mut rect = Rect::default();
        if GetWindowRect(hwnd, &mut rect) == 0 {
            return Err("Cannot inspect owned smoke window size".into());
        }
        Ok(Self { hwnd, rect })
    }
    pub(super) unsafe fn resize(&self, width: i32, height: i32) -> Result<(), String> {
        if SetWindowPos(self.hwnd, null_mut(), 0, 0, width, height, 0x16) == 0 {
            return Err("Cannot resize owned smoke window".into());
        }
        Ok(())
    }
}
impl Drop for OwnSize {
    fn drop(&mut self) {
        unsafe {
            SetWindowPos(
                self.hwnd,
                null_mut(),
                0,
                0,
                self.rect.right - self.rect.left,
                self.rect.bottom - self.rect.top,
                0x16,
            );
        }
    }
}
fn ordinary(path: &Path) -> Result<bool, String> {
    use std::os::windows::fs::MetadataExt;
    Ok(fs::symlink_metadata(path)
        .map_err(|e| e.to_string())?
        .file_attributes()
        & 0x400
        == 0)
}
struct OwnedFolder {
    path: PathBuf,
    leaf: PathBuf,
    link: PathBuf,
    target: PathBuf,
    linked: bool,
    parent: PathBuf,
}
impl OwnedFolder {
    fn new(parent: &Path, target: &Path) -> Result<Self, String> {
        let parent = parent.canonicalize().map_err(|e| e.to_string())?;
        let path = parent.join("cide-explorer-smoke");
        fs::create_dir(&path).map_err(|e| e.to_string())?;
        let leaf = path.join("inside.txt");
        let link = path.join("link.txt");
        let mut guard = Self {
            path,
            leaf,
            link,
            target: target.to_owned(),
            linked: false,
            parent,
        };
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&guard.leaf)
            .map_err(|e| e.to_string())?;
        file.write_all(NESTED).map_err(|e| e.to_string())?;
        drop(file);
        match std::os::windows::fs::symlink_file(target, &guard.link) {
            Ok(()) => guard.linked = true,
            Err(error)
                if error.kind() == std::io::ErrorKind::PermissionDenied
                    || error.kind() == std::io::ErrorKind::Unsupported
                    || error.raw_os_error() == Some(1314) => {}
            Err(error) => return Err(error.to_string()),
        }
        Ok(guard)
    }
    fn cleanup(&self) -> Result<(), String> {
        if !ordinary(&self.path)?
            || self
                .path
                .canonicalize()
                .map_err(|e| e.to_string())?
                .parent()
                != Some(self.parent.as_path())
        {
            return Err("Explorer fixture directory ownership changed".into());
        }
        if self.linked {
            match fs::symlink_metadata(&self.link) {
                Ok(_) => {
                    if fs::read_link(&self.link).map_err(|e| e.to_string())? != self.target {
                        return Err("Explorer link fixture ownership changed".into());
                    }
                    fs::remove_file(&self.link).map_err(|e| e.to_string())?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.to_string()),
            }
        }
        match fs::symlink_metadata(&self.leaf) {
            Ok(metadata) => {
                if !metadata.is_file()
                    || !ordinary(&self.leaf)?
                    || self
                        .leaf
                        .canonicalize()
                        .map_err(|e| e.to_string())?
                        .parent()
                        != Some(self.path.as_path())
                {
                    return Err("Explorer fixture leaf ownership changed".into());
                }
                fs::remove_file(&self.leaf).map_err(|e| e.to_string())?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
        fs::remove_dir(&self.path).map_err(|e| e.to_string())
    }
}
impl Drop for OwnedFolder {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}
unsafe fn pump(hwnd: Hwnd) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut message: Msg = std::mem::zeroed();
    while PeekMessageW(&mut message, hwnd, TASKS_MESSAGE, TASKS_MESSAGE, 1) != 0 {
        DispatchMessageW(&message);
        if Instant::now() >= deadline {
            return Err("Explorer task drain exceeded deadline".into());
        }
    }
    Ok(())
}
unsafe fn ready(hwnd: Hwnd, id: NodeId) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        pump(hwnd)?;
        {
            let state = owner(hwnd)
                .ok_or("Explorer smoke lost HWND")?
                .state
                .borrow();
            match state
                .explorer
                .node(id)
                .ok_or("Explorer node was invalidated while waiting")?
                .state()
            {
                ExplorerLoadState::Ready => return Ok(()),
                ExplorerLoadState::Queued | ExplorerLoadState::Loading => {}
                other => return Err(format!("Explorer directory did not complete: {other:?}")),
            }
        }
        if Instant::now() >= deadline {
            return Err("Explorer native delivery deadline exceeded".into());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}
unsafe fn focus(hwnd: Hwnd) -> Result<(), String> {
    let needs = {
        let s = owner(hwnd)
            .ok_or("Explorer smoke lost HWND")?
            .state
            .borrow();
        !s.explorer_visible || !s.explorer_focused
    };
    if needs {
        command(hwnd, CommandId::ViewExplorer)?;
    }
    Ok(())
}
/// Locate a displayed fixture row using keyboard navigation, then retain its
/// typed ID. No displayed text is parsed or converted into a filesystem path.
unsafe fn select_label(hwnd: Hwnd, suffix: &str, is_dir: bool) -> Result<NodeId, String> {
    focus(hwnd)?;
    key(hwnd, 0x24, Modifiers::NONE)?;
    for _ in 0..4096 {
        let found = {
            let mut s = owner(hwnd)
                .ok_or("Explorer smoke lost HWND")?
                .state
                .borrow_mut();
            s.explorer_presentation().and_then(|f| {
                f.presentation
                    .rows
                    .into_iter()
                    .find(|row| row.selected && row.is_dir == is_dir && row.label.ends_with(suffix))
                    .map(|row| row.id)
            })
        };
        if let Some(id) = found {
            return Ok(id);
        }
        key(hwnd, 0x28, Modifiers::NONE)?;
    }
    Err("Explorer fixture row is not reachable in its visible tree".into())
}
unsafe fn actual_paint(hwnd: Hwnd) {
    InvalidateRect(hwnd, std::ptr::null(), 0);
    SendMessageW(hwnd, WM_PAINT, 0, 0);
}
unsafe fn hit(hwnd: Hwnd, id: NodeId, disclosure: bool) -> Result<(i32, i32), String> {
    actual_paint(hwnd);
    let state = owner(hwnd)
        .ok_or("Explorer smoke lost HWND")?
        .state
        .borrow();
    let hits = state
        .explorer_hits
        .as_ref()
        .ok_or("Actual paint did not publish Explorer hits")?;
    let (_, depth, rect) = hits
        .rows
        .iter()
        .find(|(node, _, _)| *node == id)
        .ok_or("Painted Explorer fixture row has no hit target")?;
    let x = if disclosure {
        (rect.left + 8 + (*depth as i32 * 2) * state.cell_width + state.cell_width / 2)
            .min(rect.right - 1)
    } else {
        rect.right - 4
    };
    Ok((x, rect.top + (rect.bottom - rect.top) / 2))
}
unsafe fn mouse(hwnd: Hwnd, x: i32, y: i32, double: bool) -> Result<(), String> {
    owner(hwnd)
        .ok_or("Explorer smoke lost HWND")?
        .state
        .borrow_mut()
        .key_override = Some(Modifiers::NONE);
    SendMessageW(
        hwnd,
        if double {
            WM_LBUTTONDBLCLK
        } else {
            WM_LBUTTONDOWN
        },
        1,
        ((y as isize) << 16) | x as isize,
    );
    if let Some(owner) = owner(hwnd) {
        owner.state.borrow_mut().key_override = None;
    }
    Ok(())
}
type ExplorerMetrics = (u64, u64, usize, usize, usize, usize, String);
unsafe fn metrics(hwnd: Hwnd) -> Result<ExplorerMetrics, String> {
    let s = owner(hwnd)
        .ok_or("Explorer smoke lost HWND")?
        .state
        .borrow();
    let hits = s.explorer_hits.as_ref().map(|h| {
        (
            h.generation,
            h.revision,
            h.rows
                .iter()
                .map(|(id, depth, r)| (*id, *depth, (r.left, r.top, r.right, r.bottom)))
                .collect::<Vec<_>>(),
        )
    });
    Ok((
        s.explorer.generation(),
        s.explorer.revision(),
        s.explorer.nodes().len(),
        s.explorer.owned_bytes(),
        s.jobs.snapshot_bytes(),
        s.jobs.artifact_bytes(),
        format!("{hits:?}"),
    ))
}
unsafe fn readonly_frame(hwnd: Hwnd, bitmap: Option<&Path>) -> Result<(), String> {
    // First settle viewport with the normal owner preparation; neither call
    // below publishes mouse hits, which belong only to actual WM_PAINT.
    {
        let mut s = owner(hwnd).unwrap().state.borrow_mut();
        paint::prepare(&mut s);
        paint::prepare(&mut s);
    }
    let before = snapshot(hwnd)?;
    let counters = metrics(hwnd)?;
    let frame = {
        let mut s = owner(hwnd).unwrap().state.borrow_mut();
        paint::prepare(&mut s)
    };
    if let Some(path) = bitmap {
        if path.exists() {
            return Err("Explorer bitmap must be a new owned sibling".into());
        }
        paint::bitmap(hwnd, &frame, path)?;
    }
    if snapshot(hwnd)? != before || metrics(hwnd)? != counters {
        return Err(
            "Explorer composition changed source/catalog/tasks or published mouse hits".into(),
        );
    }
    Ok(())
}
unsafe fn source_bytes(hwnd: Hwnd, document: jc_app::DocumentId) -> Result<Vec<u8>, String> {
    let s = owner(hwnd)
        .ok_or("Explorer smoke lost HWND")?
        .state
        .borrow();
    let d = s
        .workbench
        .document_by_id(document)
        .ok_or("Explorer lost source document")?;
    if d.session.document().len() > 128 {
        return Err("Explorer source fixture exceeded budget".into());
    }
    d.session
        .document()
        .read_range(0..d.session.document().len())
        .map_err(|e| e.to_string())
}
unsafe fn cancelled_messages(hwnd: Hwnd) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_millis(100);
    while Instant::now() < deadline {
        pump(hwnd)?;
        std::thread::sleep(Duration::from_millis(1));
    }
    Ok(())
}

pub(super) unsafe fn run(hwnd: Hwnd, options: &SmokeOptions) -> Result<Vec<&'static str>, String> {
    expected(hwnd, SOURCE).map_err(|error| format!("Explorer inicial: {error}"))?;
    // Earlier smoke stages may retain two views of this same source. Establish
    // one group so this block can prove the side/pin transition from scratch.
    let split = owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .panes()
        .is_split();
    if split {
        command(hwnd, CommandId::EditorOneGroup)?;
        expected(hwnd, SOURCE)
            .map_err(|error| format!("Explorer al volver a un grupo: {error}"))?;
    }
    let parent = options
        .fixture
        .parent()
        .ok_or("Explorer fixture has no parent")?
        .to_owned();
    let filename = options
        .fixture
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| n.is_ascii())
        .ok_or("Explorer smoke needs its ASCII fixture filename")?;
    let own = OwnedFolder::new(&parent, &options.fixture)?;
    let size = OwnSize::new(hwnd)?;
    size.resize(1600, 900)?;
    let mut checks = Vec::new();
    command(hwnd, CommandId::ViewExplorer)?;
    key(hwnd, 0x24, Modifiers::NONE)?;
    key(hwnd, 0x74, Modifiers::NONE)?;
    let root = owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .explorer
        .root_id()
        .ok_or("Explorer root missing")?;
    ready(hwnd, root)?;
    {
        let mut s = owner(hwnd).unwrap().state.borrow_mut();
        let frame = s
            .explorer_presentation()
            .ok_or("Wide client omitted Explorer region")?;
        if !s.explorer_focused
            || frame
                .presentation
                .rows
                .iter()
                .any(|r| r.is_dir && r.label.ends_with(".git"))
        {
            return Err("Explorer did not focus or exposed excluded .git".into());
        }
    }
    checks.push("explorer_root_native_delivery_no_input_git_excluded");
    checks.extend(scroll_messages(hwnd, &parent)?);

    let folder = select_label(hwnd, "cide-explorer-smoke", true)?;
    if owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .explorer
        .node(folder)
        .unwrap()
        .state()
        != ExplorerLoadState::Unloaded
    {
        return Err("Explorer eagerly loaded an unexpanded directory".into());
    }
    let (x, y) = hit(hwnd, folder, true)?;
    mouse(hwnd, x, y, false)?;
    ready(hwnd, folder)?;
    {
        let s = owner(hwnd).unwrap().state.borrow();
        if s.explorer.node(folder).unwrap().children().len() != 1 {
            return Err("Explorer included a link or missed the ordinary nested leaf".into());
        }
    }
    key(hwnd, 0x27, Modifiers::NONE)?;
    let nested = owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .explorer_view
        .selected()
        .ok_or("Explorer Right did not select a child")?;
    if owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .explorer
        .node(nested)
        .unwrap()
        .parent()
        != Some(folder)
    {
        return Err("Explorer Right did not use typed child identity".into());
    }
    checks.push("explorer_lazy_disclosure_expand_right_child_native_delivery");
    if own.linked {
        checks.push("explorer_owned_reparse_leaf_excluded");
    }

    let before = snapshot(hwnd)?;
    key(hwnd, 0x09, Modifiers::NONE)?;
    SendMessageW(hwnd, WM_CHAR, '\t' as usize, 0);
    SendMessageW(hwnd, WM_CHAR, 'x' as usize, 0);
    super::smoke::paste(hwnd, "must not paste into the editor from Explorer")?;
    if snapshot(hwnd)? != before {
        return Err("Explorer Tab edited the source or moved an editor caret".into());
    }
    key(hwnd, 0x75, Modifiers::NONE)?;
    if super::outline_panel::region(&owner(hwnd).unwrap().state.borrow()).is_some() {
        if !owner(hwnd).unwrap().state.borrow().outline_focused() {
            return Err("F6 did not visit the visible right code panel".into());
        }
        key(hwnd, 0x75, Modifiers::NONE)?;
    }
    if owner(hwnd).unwrap().state.borrow().explorer_focused {
        return Err("F6 did not return focus to Editor".into());
    }
    key(hwnd, 0x75, Modifiers::NONE)?;
    if !owner(hwnd).unwrap().state.borrow().explorer_focused {
        return Err("F6 did not restore Explorer focus".into());
    }
    checks.push("explorer_focus_cycle_tab_do_not_edit_source");

    key(hwnd, 0x1b, Modifiers::NONE)?;
    position(hwnd, 0, 0)?;
    SendMessageW(hwnd, WM_CHAR, 'Q' as usize, 0);
    expected(hwnd, DIRTY)?;
    let original = snapshot(hwnd)?;
    let source = original.0;
    let file = select_label(hwnd, filename, false)?;
    let (x, y) = hit(hwnd, file, false)?;
    mouse(hwnd, x, y, false)?;
    if snapshot(hwnd)? != original {
        return Err("Explorer single click unexpectedly opened or navigated source".into());
    }
    mouse(hwnd, x, y, true)?;
    expected(hwnd, DIRTY)?;
    let after = snapshot(hwnd)?;
    if (after.0, after.1, after.3) != (original.0, original.1, original.3) {
        return Err("Explorer double click cloned or reloaded the dirty document".into());
    }
    checks.push("explorer_actual_paint_mouse_select_double_dirty_reuse");

    // WM_COMMAND must retain the invoking sidebar context even when it closes
    // other overlays before dispatching the registered action.
    command(hwnd, CommandId::ViewExplorer)?;
    {
        let state = owner(hwnd).unwrap().state.borrow();
        if !state.explorer_visible || !state.explorer_focused {
            return Err("Explorer menu did not show and focus the sidebar".into());
        }
    }
    command(hwnd, CommandId::ViewExplorer)?;
    {
        let state = owner(hwnd).unwrap().state.borrow();
        if state.explorer_visible || state.explorer_focused || state.explorer_hits.is_some() {
            return Err("Second Explorer menu invocation did not hide the focused sidebar".into());
        }
    }
    command(hwnd, CommandId::ViewExplorer)?;
    expected(hwnd, DIRTY)?;
    checks.push("explorer_menu_toggle_preserves_invoking_focus_context");

    // The menu opens a different selected file from the active dirty source.
    // The palette then opens the selected source while the nested file is
    // active. Losing sidebar context would open the wrong active file instead.
    select_label(hwnd, "inside.txt", false)?;
    command(hwnd, CommandId::EditorOpenToSide)?;
    expected(hwnd, NESTED)?;
    {
        let state = owner(hwnd).unwrap().state.borrow();
        if state.workbench.panes().active() != PaneId::Secondary
            || state.workbench.tabs().tabs().len() != original.3 + 1
        {
            return Err("Explorer menu Open to Side did not open its selected file".into());
        }
    }
    select_label(hwnd, filename, false)?;
    key(hwnd, 0x50, Modifiers::CTRL.union(Modifiers::SHIFT))?;
    for unit in "Abrir al lado".encode_utf16() {
        SendMessageW(hwnd, WM_CHAR, unit as usize, 0);
    }
    key(hwnd, 0x0d, Modifiers::NONE)?;
    expected(hwnd, DIRTY)?;
    let palette_result = snapshot(hwnd)?;
    if palette_result.0 != source
        || palette_result.1 != original.1
        || palette_result.3 != original.3 + 1
        || source_bytes(hwnd, source)? != DIRTY
    {
        return Err("Palette Open to Side lost Explorer target or duplicated the source".into());
    }
    checks.push("explorer_menu_and_palette_open_to_side_use_selected_node");

    let bitmap = options
        .bitmap
        .as_ref()
        .map(|p| p.with_file_name("native-explorer.bmp"));
    readonly_frame(hwnd, bitmap.as_deref())?;
    checks.push("explorer_gdi_readonly_no_catalog_task_or_hit_admission");

    select_label(hwnd, "inside.txt", false)?;
    key(hwnd, 0x0d, Modifiers::CTRL)?;
    expected(hwnd, NESTED)?;
    let nested_tab = owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .tabs()
        .active_id()
        .ok_or("Explorer side opened no tab")?;
    {
        let s = owner(hwnd).unwrap().state.borrow();
        if !s.workbench.panes().is_split()
            || s.workbench.panes().active() != PaneId::Secondary
            || s.workbench.tabs().tabs().len() != original.3 + 1
        {
            return Err(
                "Explorer CtrlEnter did not create only the intended side view/document".into(),
            );
        }
    }
    command(hwnd, CommandId::EditorTogglePin)?;
    select_label(hwnd, filename, false)?;
    key(hwnd, 0x0d, Modifiers::CTRL)?;
    expected(hwnd, DIRTY)?;
    {
        let s = owner(hwnd).unwrap().state.borrow();
        let reference = s
            .workbench
            .panes()
            .view(PaneId::Secondary)
            .ok_or("Pinned reference group disappeared")?;
        if !reference.is_pinned()
            || reference.location().map(|l| l.tab) != Some(nested_tab)
            || s.workbench.active_document().unwrap().id != source
            || s.workbench.tabs().tabs().len() != original.3 + 1
        {
            return Err(
                "Explorer side routing replaced the pinned reference or duplicated source".into(),
            );
        }
    }
    if source_bytes(hwnd, source)? != DIRTY {
        return Err("Explorer side changed original source bytes".into());
    }
    checks.push("explorer_ctrl_enter_side_pin_shared_document_routing");

    let panes = format!(
        "{:?}",
        owner(hwnd).unwrap().state.borrow().workbench.panes()
    );
    focus(hwnd)?;
    size.resize(500, 300)?;
    actual_paint(hwnd);
    {
        let s = owner(hwnd).unwrap().state.borrow();
        if explorer_panel::region(&s).is_some()
            || s.explorer_focused
            || s.explorer_hits.is_some()
            || !s.workbench.panes().is_split()
            || format!("{:?}", s.workbench.panes()) != panes
        {
            return Err("Compact resize lost split/view state or kept Explorer focus".into());
        }
    }
    size.resize(1600, 900)?;
    actual_paint(hwnd);
    if !owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .panes()
        .is_split()
    {
        return Err("Wide resize did not preserve the existing split".into());
    }
    checks.push("explorer_compact_resize_preserves_split_and_editor_ownership");

    // Retain authentic WM_PAINT metadata, then emulate a delayed cached input
    // against a refreshed catalog. No offscreen frame is installed as hits.
    let file = select_label(hwnd, filename, false)?;
    let (old_x, old_y) = hit(hwnd, file, false)?;
    let old = owner(hwnd)
        .unwrap()
        .state
        .borrow_mut()
        .explorer_hits
        .take()
        .ok_or("Explorer old frame unavailable")?;
    key(hwnd, 0x24, Modifiers::NONE)?;
    key(hwnd, 0x74, Modifiers::NONE)?;
    owner(hwnd).unwrap().state.borrow_mut().explorer_hits = Some(old);
    let before = snapshot(hwnd)?;
    mouse(hwnd, old_x, old_y, true)?;
    if snapshot(hwnd)? != before {
        return Err("Stale Explorer refresh hit navigated source".into());
    }
    key(hwnd, 0x1b, Modifiers::NONE)?;
    cancelled_messages(hwnd)?;
    {
        let s = owner(hwnd).unwrap().state.borrow();
        let root = s.explorer.root_id().unwrap();
        let node = s.explorer.node(root).unwrap();
        if node.state() != ExplorerLoadState::Cancelled
            || !node.children().is_empty()
            || s.explorer_focused
        {
            return Err("Cancelled root load revived expanded results or retained focus".into());
        }
    }
    checks.push("explorer_refresh_stale_hit_and_cancelled_delivery_rejected");

    let old = owner(hwnd).unwrap().state.borrow_mut().explorer_hits.take();
    let generation = owner(hwnd).unwrap().state.borrow().explorer.generation();
    {
        let mut borrow = owner(hwnd).unwrap().state.borrow_mut();
        let s = &mut *borrow;
        s.set_project_root(parent.clone())?;
        if s.explorer.generation() == generation
            || s.explorer
                .open_file(
                    file,
                    &mut s.workbench,
                    Some(jc_text::TextEncoding::Utf8),
                    false,
                    1024,
                )
                .is_ok()
        {
            return Err("Root replacement accepted an old Explorer node ID".into());
        }
        s.explorer_hits = old;
    }
    let before = snapshot(hwnd)?;
    mouse(hwnd, old_x, old_y, true)?;
    if snapshot(hwnd)? != before {
        return Err("Old Explorer root-generation mouse target navigated source".into());
    }
    let root = owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .explorer
        .root_id()
        .unwrap();
    ready(hwnd, root)?;
    checks.push("explorer_root_generation_invalidates_node_and_cached_mouse_target");

    command(hwnd, CommandId::EditorFocusSecondary)?;
    if owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .tabs()
        .active_id()
        != Some(nested_tab)
    {
        return Err("Explorer cleanup lost its owned reference tab".into());
    }
    command(hwnd, CommandId::EditorTogglePin)?;
    command(hwnd, CommandId::FileClose)?;
    command(hwnd, CommandId::EditorFocusPrimary)?;
    command(hwnd, CommandId::EditorOneGroup)?;
    {
        let mut s = owner(hwnd).unwrap().state.borrow_mut();
        s.explorer_visible = false;
        s.explorer_focused = false;
        s.explorer_hits = None;
    }
    key(hwnd, 0x5a, Modifiers::CTRL)?;
    expected(hwnd, SOURCE).map_err(|error| format!("Explorer después de deshacer: {error}"))?;
    if owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .active_document()
        .unwrap()
        .id
        != source
        || fs::read(&options.fixture).map_err(|e| e.to_string())? != SOURCE
    {
        return Err(
            "Explorer open lost one-undo ownership or changed original disk fixture".into(),
        );
    }
    own.cleanup()?;
    {
        let mut s = owner(hwnd).unwrap().state.borrow_mut();
        s.set_project_root(parent)?;
        s.explorer_visible = false;
        s.explorer_focused = false;
        s.explorer_hits = None;
    }
    checks.push("explorer_source_one_undo_disk_preserved_owned_fixture_cleanup");
    Ok(checks)
}
