//! Own-HWND selected search, reopen metadata and indexed tabs. Only exact owned
//! synthetic siblings are changed on disk; the original fixture is never saved.
use super::{
    close_flow,
    ffi::*,
    owner, paint,
    project_smoke::snapshot,
    smoke::{command, expected, key, paste, position},
};
use crate::SmokeOptions;
use jc_app::{CommandId, DocumentId, Modifiers, PaneId, SearchStatus, TabId};
use jc_text::{encode, TextEncoding};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
const SOURCE: &[u8] = b"alpha\r\nbeta\r\n";
const A: &str = "selection-reopen-a.p";
const B: &str = "selection-reopen-b.txt";
const INVALID: &str = "selection-reopen-invalid.txt";
const A_CHANGED: &[u8] = b"cambi\xf3\r\nnew\n";
const B_RETRY: &[u8] = b"\xef\xbb\xbfretry\r\n";
fn ordinary(path: &Path) -> Result<bool, String> {
    use std::os::windows::fs::MetadataExt;
    Ok(fs::symlink_metadata(path)
        .map_err(|e| e.to_string())?
        .file_attributes()
        & 0x400
        == 0)
}
struct Files {
    parent: PathBuf,
    paths: Vec<PathBuf>,
}
impl Files {
    fn new(parent: &Path) -> Result<Self, String> {
        let mut own = Self {
            parent: parent.canonicalize().map_err(|e| e.to_string())?,
            paths: Vec::new(),
        };
        for (name, bytes) in [
            (A, b"ni\xf1o\r\nfirst\n".as_slice()),
            (B, b"\xef\xbb\xbfsecond\r\n".as_slice()),
            (INVALID, b"a\xffa".as_slice()),
        ] {
            let path = own.parent.join(name);
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|e| e.to_string())?;
            own.paths.push(path);
            file.write_all(bytes).map_err(|e| e.to_string())?;
        }
        Ok(own)
    }
    fn path(&self, name: &str) -> PathBuf {
        self.parent.join(name)
    }
    fn verify(&self, path: &Path) -> Result<(), String> {
        if !self.paths.iter().any(|p| p == path)
            || !ordinary(path)?
            || !fs::metadata(path).map_err(|e| e.to_string())?.is_file()
            || path.canonicalize().map_err(|e| e.to_string())?.parent()
                != Some(self.parent.as_path())
        {
            return Err("Reopen fixture ownership changed".into());
        }
        Ok(())
    }
    fn write(&self, name: &str, bytes: &[u8]) -> Result<(), String> {
        let path = self.path(name);
        self.verify(&path)?;
        fs::write(path, bytes).map_err(|e| e.to_string())
    }
    fn remove(&self, name: &str) -> Result<(), String> {
        let path = self.path(name);
        self.verify(&path)?;
        fs::remove_file(path).map_err(|e| e.to_string())
    }
    fn recreate(&self, name: &str, bytes: &[u8]) -> Result<(), String> {
        let path = self.path(name);
        if !self.paths.contains(&path) || path.parent() != Some(self.parent.as_path()) {
            return Err("Reopen fixture is not owned".into());
        }
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| e.to_string())?;
        file.write_all(bytes).map_err(|e| e.to_string())
    }
    fn cleanup(&self) -> Result<(), String> {
        if !ordinary(&self.parent)? {
            return Err("Reopen fixture parent changed".into());
        }
        for path in &self.paths {
            match fs::symlink_metadata(path) {
                Ok(_) => self.verify(path)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.to_string()),
            }
        }
        for path in &self.paths {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.to_string()),
            }
        }
        Ok(())
    }
}
impl Drop for Files {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}
unsafe fn close(hwnd: Hwnd, discard: bool) -> Result<(), String> {
    owner(hwnd)
        .ok_or("Reopen smoke lost HWND")?
        .state
        .borrow_mut()
        .smoke_close_decisions
        .push_back(discard);
    let result = command(hwnd, CommandId::FileClose);
    // Clean/shared-view closes need no confirmation. Do not let this fixture's
    // unused decision authorize any later close.
    owner(hwnd)
        .ok_or("Reopen smoke lost HWND")?
        .state
        .borrow_mut()
        .smoke_close_decisions
        .clear();
    result
}
unsafe fn memory(
    hwnd: Hwnd,
    options: &SmokeOptions,
    name: &str,
    text: &str,
    encoding: TextEncoding,
) -> Result<(), String> {
    let path = options.fixture.parent().unwrap().join(name);
    {
        let mut s = owner(hwnd).unwrap().state.borrow_mut();
        s.close_selector();
        s.explorer_visible = false;
        s.explorer_focused = false;
        s.explorer_hits = None;
        s.workbench.new_file(path, encoding);
    }
    paste(hwnd, text)?;
    position(hwnd, 0, 0)
}
unsafe fn open(
    hwnd: Hwnd,
    files: &Files,
    name: &str,
    encoding: TextEncoding,
) -> Result<(TabId, DocumentId), String> {
    let path = files.path(name);
    files.verify(&path)?;
    let mut s = owner(hwnd).unwrap().state.borrow_mut();
    s.close_selector();
    s.workbench
        .open_project_file(&files.parent, path, Some(encoding), false, 16 * 1024 * 1024)
        .map_err(|e| e.to_string())?;
    let d = s.workbench.active_document().unwrap();
    Ok((d.tab_id, d.id))
}
unsafe fn reopen(hwnd: Hwnd) -> Result<(), String> {
    key(hwnd, 0x54, Modifiers::CTRL.union(Modifiers::SHIFT))
}
unsafe fn block(hwnd: Hwnd) -> Result<(), String> {
    position(hwnd, 0, 0)?;
    key(hwnd, 0x28, Modifiers::SHIFT)?;
    for _ in 0..3 {
        key(hwnd, 0x27, Modifiers::SHIFT)?;
    }
    Ok(())
}
unsafe fn complete(hwnd: Hwnd) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let mut m: Msg = std::mem::zeroed();
        while PeekMessageW(&mut m, hwnd, TASKS_MESSAGE, TASKS_MESSAGE, 1) != 0 {
            DispatchMessageW(&m);
            if Instant::now() >= deadline {
                return Err("Selected-search task drain deadline exceeded".into());
            }
        }
        let s = owner(hwnd).unwrap().state.borrow();
        if !s.workbench.search_running() {
            return if s.workbench.search().status == SearchStatus::Complete {
                Ok(())
            } else {
                Err("Selected search did not complete".into())
            };
        }
        drop(s);
        if Instant::now() >= deadline {
            return Err("Selected-search notification deadline exceeded".into());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}
unsafe fn drain(hwnd: Hwnd) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_millis(100);
    while Instant::now() < deadline {
        let mut m: Msg = std::mem::zeroed();
        if PeekMessageW(&mut m, hwnd, TASKS_MESSAGE, TASKS_MESSAGE, 1) != 0 {
            DispatchMessageW(&m);
        } else {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    Ok(())
}
unsafe fn palette_open(hwnd: Hwnd, label: &str) -> Result<(), String> {
    key(hwnd, 0x50, Modifiers::CTRL.union(Modifiers::SHIFT))?;
    for u in label.encode_utf16() {
        SendMessageW(hwnd, WM_CHAR, u as usize, 0);
    }
    Ok(())
}
unsafe fn palette(hwnd: Hwnd, label: &str) -> Result<(), String> {
    palette_open(hwnd, label)?;
    key(hwnd, 0x0d, Modifiers::NONE)
}
unsafe fn query(hwnd: Hwnd) -> String {
    owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .search()
        .query
        .clone()
}
unsafe fn closed_state(hwnd: Hwnd) -> String {
    format!("{:?}", owner(hwnd).unwrap().state.borrow().workbench.tabs())
}

pub(super) unsafe fn run(hwnd: Hwnd, options: &SmokeOptions) -> Result<Vec<&'static str>, String> {
    expected(hwnd, SOURCE)?;
    let split = owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .panes()
        .is_split();
    if split {
        command(hwnd, CommandId::EditorOneGroup)?;
        expected(hwnd, SOURCE)?;
    }
    let source_id = snapshot(hwnd)?.0;
    let files = Files::new(
        options
            .fixture
            .parent()
            .ok_or("Reopen fixture has no parent")?,
    )?;
    let mut checks = Vec::new();
    let q = "niño\r\nfin";
    for encoding in [
        TextEncoding::Utf8,
        TextEncoding::Windows1252,
        TextEncoding::Iso8859_1,
        TextEncoding::Ibm850,
    ] {
        let bom = if encoding == TextEncoding::Utf8 {
            "\u{feff}"
        } else {
            ""
        };
        let text = format!("{bom}{q}|{q}");
        memory(hwnd, options, "selection-memory.p", &text, encoding)?;
        {
            let mut s = owner(hwnd).unwrap().state.borrow_mut();
            if !s.workbench.search().case_sensitive {
                s.workbench
                    .toggle_search_case()
                    .map_err(|e| e.to_string())?;
            }
        }
        block(hwnd)?;
        let flags = {
            let s = owner(hwnd).unwrap().state.borrow();
            (
                s.workbench.search().case_sensitive,
                s.workbench.search().whole_word,
            )
        };
        key(hwnd, 0x72, Modifiers::CTRL)?;
        complete(hwnd)?;
        if encoding == TextEncoding::Utf8 {
            if let Some(bitmap) = &options.bitmap {
                let output = bitmap.with_file_name("native-selection-search.bmp");
                if output.exists() || output == options.fixture || output == options.report {
                    return Err("Selected-search bitmap must be a new owned sibling".into());
                }
                let before = snapshot(hwnd)?;
                let frame = {
                    let mut s = owner(hwnd).unwrap().state.borrow_mut();
                    paint::prepare(&mut s)
                };
                paint::bitmap(hwnd, &frame, &output)?;
                if snapshot(hwnd)? != before {
                    return Err(
                        "Selected-search GDI preparation changed source or selection".into(),
                    );
                }
            }
        }
        let len = encode(q, encoding).map_err(|e| e.to_string())?.len();
        let start = if encoding == TextEncoding::Utf8 { 3 } else { 0 };
        if query(hwnd) != q
            || owner(hwnd)
                .unwrap()
                .state
                .borrow()
                .workbench
                .active_editor()
                .unwrap()
                .selection()
                != Some(start + len + 1..start + len + 1 + len)
        {
            return Err("Selected multiline search lost decoded query or next source range".into());
        }
        key(hwnd, 0x72, Modifiers::CTRL.union(Modifiers::SHIFT))?;
        complete(hwnd)?;
        {
            let s = owner(hwnd).unwrap().state.borrow();
            if s.workbench.active_editor().unwrap().selection() != Some(start..start + len)
                || (
                    s.workbench.search().case_sensitive,
                    s.workbench.search().whole_word,
                ) != flags
            {
                return Err("Selected previous search changed flags or source coordinates".into());
            }
        }
        expected(hwnd, &encode(&text, encoding).map_err(|e| e.to_string())?)?;
        close(hwnd, true)?;
        expected(hwnd, SOURCE)?;
    }
    checks.push("selection_search_unicode_multiline_four_encodings_flags_preserved");

    memory(
        hwnd,
        options,
        "selection-budget.txt",
        &"x".repeat(4096),
        TextEncoding::Utf8,
    )?;
    key(hwnd, 0x41, Modifiers::CTRL)?;
    command(hwnd, CommandId::SearchSelectionNext)?;
    complete(hwnd)?;
    if query(hwnd).len() != 4096 {
        return Err("Exact4096 source selection was rejected".into());
    }
    close(hwnd, true)?;
    for (name, text, encoding) in [
        (
            "selection-oversize.txt",
            "é".repeat(2049),
            TextEncoding::Utf8,
        ),
        (
            "selection-decoded-budget.txt",
            "ñ".repeat(4096),
            TextEncoding::Iso8859_1,
        ),
    ] {
        memory(hwnd, options, name, &text, encoding)?;
        key(hwnd, 0x41, Modifiers::CTRL)?;
        let before = snapshot(hwnd)?;
        let old = query(hwnd);
        command(hwnd, CommandId::SearchSelectionNext)?;
        if snapshot(hwnd)? != before || query(hwnd) != old {
            return Err(
                "Selected source/decoded-query byte budget changed prior query or caret".into(),
            );
        }
        close(hwnd, true)?;
    }
    open(hwnd, &files, INVALID, TextEncoding::Utf8)?;
    key(hwnd, 0x41, Modifiers::CTRL)?;
    let before = snapshot(hwnd)?;
    let old = query(hwnd);
    command(hwnd, CommandId::SearchSelectionNext)?;
    if snapshot(hwnd)? != before || query(hwnd) != old {
        return Err("Invalid UTF8 selection replaced a strict prior query".into());
    }
    expected(hwnd, b"a\xffa")?;
    close(hwnd, false)?;
    memory(
        hwnd,
        options,
        "selection-empty-rectangle.txt",
        "abc",
        TextEncoding::Utf8,
    )?;
    let old = query(hwnd);
    let before = snapshot(hwnd)?;
    command(hwnd, CommandId::SearchSelectionNext)?;
    if snapshot(hwnd)? != before || query(hwnd) != old {
        return Err("Empty selection changed query or source".into());
    }
    {
        let mut s = owner(hwnd).unwrap().state.borrow_mut();
        s.workbench.start_rectangle().map_err(|e| e.to_string())?;
        s.workbench
            .extend_rectangle(2, 0)
            .map_err(|e| e.to_string())?;
    }
    let before = snapshot(hwnd)?;
    command(hwnd, CommandId::SearchSelectionNext)?;
    if snapshot(hwnd)? != before || query(hwnd) != old {
        return Err("Rectangular selection was accepted as linear query".into());
    }
    owner(hwnd)
        .unwrap()
        .state
        .borrow_mut()
        .workbench
        .cancel_rectangle();
    close(hwnd, true)?;
    checks.push("selection_search_invalid_bytes_range_budget_empty_rectangle_denied");

    memory(
        hwnd,
        options,
        "selection-focus.txt",
        &format!("{q}|{q}"),
        TextEncoding::Utf8,
    )?;
    block(hwnd)?;
    let old = query(hwnd);
    command(hwnd, CommandId::SearchFind)?;
    let before = snapshot(hwnd)?;
    command(hwnd, CommandId::SearchSelectionNext)?;
    if snapshot(hwnd)? != before
        || query(hwnd) != old
        || owner(hwnd).unwrap().state.borrow().search_panel.is_none()
    {
        return Err("Selection menu used hidden source from Find".into());
    }
    key(hwnd, 0x1b, Modifiers::NONE)?;
    command(hwnd, CommandId::ViewExplorer)?;
    let before = snapshot(hwnd)?;
    command(hwnd, CommandId::SearchSelectionPrevious)?;
    if snapshot(hwnd)? != before || query(hwnd) != old {
        return Err("Selection menu used hidden source from Explorer".into());
    }
    key(hwnd, 0x1b, Modifiers::NONE)?;
    {
        let mut s = owner(hwnd).unwrap().state.borrow_mut();
        s.explorer_visible = false;
        s.explorer_focused = false;
        s.explorer_hits = None;
    }
    block(hwnd)?;
    palette_open(
        hwnd,
        jc_app::default_registry()
            .get(CommandId::SearchSelectionNext)
            .unwrap()
            .label,
    )?;
    owner(hwnd)
        .unwrap()
        .state
        .borrow_mut()
        .workbench
        .move_right(true)
        .map_err(|e| e.to_string())?;
    let before = snapshot(hwnd)?;
    key(hwnd, 0x0d, Modifiers::NONE)?;
    if snapshot(hwnd)? != before || query(hwnd) != old {
        return Err("Stale palette selection became a search query".into());
    }
    key(hwnd, 0x1b, Modifiers::NONE)?;
    close(hwnd, true)?;
    checks.push("selection_search_modal_explorer_stale_palette_no_hidden_navigation");

    memory(
        hwnd,
        options,
        "selection-late.txt",
        &format!("{q}|{q}"),
        TextEncoding::Utf8,
    )?;
    block(hwnd)?;
    key(hwnd, 0x72, Modifiers::CTRL)?;
    let before = snapshot(hwnd)?;
    command(hwnd, CommandId::SearchCancel)?;
    drain(hwnd)?;
    if snapshot(hwnd)? != before
        || owner(hwnd)
            .unwrap()
            .state
            .borrow()
            .workbench
            .search()
            .status
            != SearchStatus::Cancelled
    {
        return Err("Cancelled selection search replayed navigation".into());
    }
    block(hwnd)?;
    key(hwnd, 0x72, Modifiers::CTRL)?;
    palette(
        hwnd,
        jc_app::default_registry()
            .get(CommandId::SearchCancel)
            .unwrap()
            .label,
    )?;
    drain(hwnd)?;
    if owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .search()
        .status
        != SearchStatus::Cancelled
    {
        return Err("Palette search cancellation failed".into());
    }
    block(hwnd)?;
    key(hwnd, 0x72, Modifiers::CTRL)?;
    {
        let mut s = owner(hwnd).unwrap().state.borrow_mut();
        s.workbench.split_editor_right();
        s.workbench.focus_editor_pane(PaneId::Secondary);
    }
    let before = snapshot(hwnd)?;
    complete(hwnd)?;
    if snapshot(hwnd)? != before {
        return Err("Selected search delivered navigation to a different view".into());
    }
    command(hwnd, CommandId::EditorFocusPrimary)?;
    command(hwnd, CommandId::EditorOneGroup)?;
    // A Complete identical query may select the next hit synchronously. Force
    // admission of a fresh cooperative scan for this revision-delivery case.
    owner(hwnd)
        .unwrap()
        .state
        .borrow_mut()
        .workbench
        .set_search_query("")
        .map_err(|e| e.to_string())?;
    block(hwnd)?;
    key(hwnd, 0x72, Modifiers::CTRL)?;
    {
        let s = owner(hwnd).unwrap().state.borrow();
        if !s.workbench.search_running()
            || s.workbench.search().status != SearchStatus::Running
            || s.workbench.active_editor().unwrap().selection() != Some(0..10)
        {
            return Err(
                "Revision-delivery fixture did not retain a genuinely deferred selected search"
                    .into(),
            );
        }
    }
    owner(hwnd)
        .unwrap()
        .state
        .borrow_mut()
        .workbench
        .insert_text("X")
        .map_err(|e| e.to_string())?;
    let before = snapshot(hwnd)?;
    complete(hwnd)?;
    if snapshot(hwnd)? != before
        || owner(hwnd)
            .unwrap()
            .state
            .borrow()
            .workbench
            .active_editor()
            .unwrap()
            .selection()
            .is_some()
    {
        return Err("Selected search delivered navigation after a source revision change".into());
    }
    close(hwnd, true)?;
    checks.push("selection_search_cancel_and_view_revision_delivery_not_replayed");

    let (a_tab, a_document) = open(hwnd, &files, A, TextEncoding::Windows1252)?;
    command(hwnd, CommandId::FileClose)?;
    files.write(A, A_CHANGED)?;
    reopen(hwnd)?;
    expected(hwnd, A_CHANGED)?;
    {
        let s = owner(hwnd).unwrap().state.borrow();
        let d = s.workbench.active_document().unwrap();
        if d.tab_id != a_tab
            || d.id == a_document
            || d.session.encoding() != Some(TextEncoding::Windows1252)
            || d.session.document().can_undo()
            || d.session.document().is_modified()
        {
            return Err(
                "Reopen did not use current disk/new document with original tab/encoding".into(),
            );
        }
    }
    checks.push("reopen_closed_current_disk_same_tab_new_document");

    SendMessageW(hwnd, WM_CHAR, 'Q' as usize, 0);
    let dirty = snapshot(hwnd)?.0;
    command(hwnd, CommandId::EditorSplitRight)?;
    command(hwnd, CommandId::EditorFocusSecondary)?;
    command(hwnd, CommandId::EditorTogglePin)?;
    let tabs = closed_state(hwnd);
    let count = snapshot(hwnd)?.3;
    command(hwnd, CommandId::FileClose)?;
    {
        let s = owner(hwnd).unwrap().state.borrow();
        if s.workbench.document_by_id(dirty).is_none()
            || s.workbench
                .panes()
                .view(PaneId::Primary)
                .and_then(|v| v.location())
                .map(|l| l.tab)
                != Some(a_tab)
            || s.workbench
                .panes()
                .view(PaneId::Secondary)
                .and_then(|v| v.location())
                .is_some()
            || s.workbench.tabs().tabs().len() != count
            || closed_state(hwnd) != tabs
        {
            return Err("Closing one dirty shared view recorded a closed document".into());
        }
    }
    command(hwnd, CommandId::EditorOneGroup)?;
    let before = snapshot(hwnd)?;
    close(hwnd, false)?;
    if snapshot(hwnd)? != before {
        return Err("Cancelled last dirty close changed document/history".into());
    }
    let target = owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .source_edit_target()
        .ok_or("Dirty close target unavailable")?;
    owner(hwnd)
        .unwrap()
        .state
        .borrow_mut()
        .workbench
        .move_right(true)
        .map_err(|e| e.to_string())?;
    let before = snapshot(hwnd)?;
    {
        let mut s = owner(hwnd).unwrap().state.borrow_mut();
        if close_flow::commit(&mut s, target, true).is_ok() {
            return Err("Stale close confirmation discarded a changed source intention".into());
        }
    }
    if snapshot(hwnd)? != before {
        return Err("Stale close confirmation changed source or views".into());
    }
    close(hwnd, true)?;
    reopen(hwnd)?;
    expected(hwnd, A_CHANGED)?;
    if snapshot(hwnd)?.0 == dirty {
        return Err("Dirty discarded document was silently recovered".into());
    }
    close(hwnd, false)?;
    memory(
        hwnd,
        options,
        "selection-new-blank.txt",
        "discard this draft",
        TextEncoding::Utf8,
    )?;
    let blank_tab = owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .tabs()
        .active_id()
        .unwrap();
    let blank_doc = snapshot(hwnd)?.0;
    close(hwnd, true)?;
    command(hwnd, CommandId::FileReopenClosed)?;
    expected(hwnd, b"")?;
    {
        let s = owner(hwnd).unwrap().state.borrow();
        let d = s.workbench.active_document().unwrap();
        if d.tab_id != blank_tab
            || d.id == blank_doc
            || !d.session.is_new()
            || d.session.document().can_undo()
        {
            return Err("New-file reopen retained discarded draft or history".into());
        }
    }
    close(hwnd, false)?;
    checks.push("reopen_closed_dirty_last_view_new_blank_contract");

    let (b_tab, _) = open(hwnd, &files, B, TextEncoding::Utf8)?;
    close(hwnd, false)?;
    files.remove(B)?;
    let before = snapshot(hwnd)?;
    let stack = closed_state(hwnd);
    command(hwnd, CommandId::FileReopenClosed)?;
    if snapshot(hwnd)? != before || closed_state(hwnd) != stack {
        return Err("Failed reopen consumed metadata or changed focus".into());
    }
    files.recreate(B, B_RETRY)?;
    reopen(hwnd)?;
    expected(hwnd, B_RETRY)?;
    if owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .tabs()
        .active_id()
        != Some(b_tab)
        || owner(hwnd)
            .unwrap()
            .state
            .borrow()
            .workbench
            .active_editor()
            .unwrap()
            .cursor
            != 3
    {
        return Err("Retry reopen lost TabId or BOM initial cursor".into());
    }
    close(hwnd, false)?;
    let (a_tab, _) = open(hwnd, &files, A, TextEncoding::Windows1252)?;
    close(hwnd, false)?;
    let (b_tab, _) = open(hwnd, &files, B, TextEncoding::Utf8)?;
    close(hwnd, false)?;
    reopen(hwnd)?;
    if owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .tabs()
        .active_id()
        != Some(b_tab)
    {
        return Err("Reopen did not pop newest LIFO metadata".into());
    }
    reopen(hwnd)?;
    if owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .tabs()
        .active_id()
        != Some(a_tab)
    {
        return Err("Reopen did not retain the preceding LIFO entry".into());
    }
    close(hwnd, false)?;
    close(hwnd, false)?;
    open(hwnd, &files, B, TextEncoding::Utf8)?;
    SendMessageW(hwnd, WM_CHAR, 'Q' as usize, 0);
    let before = snapshot(hwnd)?;
    let bytes = owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .active_bytes()
        .unwrap();
    let stack = closed_state(hwnd);
    reopen(hwnd)?;
    if snapshot(hwnd)? != before || closed_state(hwnd) != stack {
        return Err(
            "Reopen collision duplicated/focused a path or consumed closed metadata".into(),
        );
    }
    expected(hwnd, &bytes)?;
    close(hwnd, true)?;
    checks.push("reopen_closed_error_retry_lifo_metadata_preserved");

    expected(hwnd, SOURCE)?;
    let mut records = Vec::new();
    {
        let s = owner(hwnd).unwrap().state.borrow();
        let e = s.workbench.active_editor().unwrap();
        records.push((
            e.tab_id,
            e.document_id,
            e.cursor,
            e.selection(),
            e.session.encoding(),
        ));
    }
    for i in 0..9 {
        memory(
            hwnd,
            options,
            &format!("selection-tab-{i}.txt"),
            &format!("tab{i}\r\nniño"),
            TextEncoding::Utf8,
        )?;
        position(hwnd, 1, 2)?;
        key(hwnd, 0x27, Modifiers::SHIFT)?;
        let s = owner(hwnd).unwrap().state.borrow();
        let e = s.workbench.active_editor().unwrap();
        records.push((
            e.tab_id,
            e.document_id,
            e.cursor,
            e.selection(),
            e.session.encoding(),
        ));
    }
    let flags = {
        let s = owner(hwnd).unwrap().state.borrow();
        (
            s.workbench.search().case_sensitive,
            s.workbench.search().whole_word,
        )
    };
    for (index, id) in [
        CommandId::Tab1,
        CommandId::Tab2,
        CommandId::Tab3,
        CommandId::Tab4,
        CommandId::Tab5,
        CommandId::Tab6,
        CommandId::Tab7,
        CommandId::Tab8,
    ]
    .into_iter()
    .enumerate()
    {
        command(hwnd, id)?;
        let s = owner(hwnd).unwrap().state.borrow();
        let e = s.workbench.active_editor().unwrap();
        if (
            e.tab_id,
            e.document_id,
            e.cursor,
            e.selection(),
            e.session.encoding(),
        ) != records[index]
            || (
                s.workbench.search().case_sensitive,
                s.workbench.search().whole_word,
            ) != flags
            || s.workbench.tabs().tabs().len() != records.len()
        {
            return Err(
                "Indexed tab activation changed document ownership/caret/selection/flags".into(),
            );
        }
    }
    palette(
        hwnd,
        jc_app::default_registry()
            .get(CommandId::Tab9)
            .unwrap()
            .label,
    )?;
    if owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .tabs()
        .active_id()
        != Some(records[9].0)
    {
        return Err("Tab9 palette did not activate the last tab beyond index9".into());
    }
    key(hwnd, 0x31, Modifiers::ALT)?;
    expected(hwnd, SOURCE)?;
    key(hwnd, 0x39, Modifiers::ALT)?;
    if owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .tabs()
        .active_id()
        != Some(records[9].0)
    {
        return Err("Alt9 did not activate the last tab".into());
    }
    for _ in 0..9 {
        close(hwnd, true)?;
    }
    expected(hwnd, SOURCE)?;
    if snapshot(hwnd)?.0 != source_id
        || fs::read(&options.fixture).map_err(|e| e.to_string())? != SOURCE
    {
        return Err(
            "Selection/reopen smoke changed original source ownership or disk bytes".into(),
        );
    }
    files.cleanup()?;
    checks.push("tabs_index_last_preserve_caret_selection_flags_and_document_identity");
    Ok(checks)
}
