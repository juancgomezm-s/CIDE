//! Shared bounded syntax through the owned native window. Syntax colors are
//! observed in prepared cells and real GDI output, not inferred from a parser
//! profile or from physical display/IME behavior. No source fixture is saved.
use super::{
    ffi::*,
    owner, paint,
    project_smoke::snapshot,
    smoke::{command, expected, key, paste, position},
};
use crate::SmokeOptions;
use jc_app::{CommandId, Modifiers, PaneId, SearchStatus, SyntaxKind};
use jc_text::{encode, TextEncoding, TextPosition};
use std::{
    fs,
    time::{Duration, Instant},
};
const SOURCE: &[u8] = b"alpha\r\nbeta\r\n";
const ABL:&str="\u{feff}DISPLAY \"niño\".\r\n/* outer\r\n nested /* inner */\r\nstill */\r\nDISPLAY 42.\r\n&SCOPED-DEFINE NAME value\r\n";
const QUOTE: &str = "DISPLAY \"uno~\r\ndos\".\r\nDISPLAY 42.\r\n";
const RUST: &str = "fn main() {\r\n // line note\r\n /* outer /* nested */ tail */\r\n let normal = \"uno\r\ndos\";\r\n let bytes = br##\"byte raw\"##;\r\n let raw = r##\"raw\r\nbody\"##;\r\n let c = 'a'; let esc = '\\n';\r\n let x: &'a str = \"x\"; let y: &'static str = \"y\";\r\n let tail = 42;\r\n}\r\n";
/// Borrowed chunks compare our small synthetic Rust fixture without widening
/// the original <=128B smoke helper or collecting another source buffer.
unsafe fn own_expected(hwnd: Hwnd, value: &[u8]) -> Result<(), String> {
    if value.len() > 8192 {
        return Err("Syntax own Rust fixture exceeded8KiB".into());
    }
    let state = owner(hwnd).unwrap().state.borrow();
    let source = state
        .workbench
        .active_document()
        .ok_or("Syntax own Rust source closed")?
        .session
        .document();
    if source.len() != value.len() {
        return Err("Syntax own Rust length changed unexpectedly".into());
    }
    let mut offset = 0;
    for part in source
        .range_chunks(0..source.len())
        .map_err(|e| e.to_string())?
    {
        if part != &value[offset..offset + part.len()] {
            return Err("Syntax own Rust bytes changed unexpectedly".into());
        }
        offset += part.len();
    }
    Ok(())
}
fn rgb(kind: SyntaxKind) -> u32 {
    let c = jc_app::syntax_theme::foreground(kind);
    u32::from(c.r) | (u32::from(c.g) << 8) | (u32::from(c.b) << 16)
}
fn bytes(text: &str, encoding: TextEncoding) -> Result<Vec<u8>, String> {
    encode(text, encoding).map_err(|e| e.to_string())
}
fn offset(text: &str, needle: &str, encoding: TextEncoding) -> Result<usize, String> {
    let at = text.find(needle).ok_or("Syntax fixture token missing")?;
    Ok(bytes(&text[..at], encoding)?.len())
}
unsafe fn frame(hwnd: Hwnd) -> paint::Frame {
    let mut state = owner(hwnd).unwrap().state.borrow_mut();
    paint::prepare(&mut state)
}
unsafe fn memory(
    hwnd: Hwnd,
    options: &SmokeOptions,
    name: &str,
    text: &str,
    encoding: TextEncoding,
) -> Result<(), String> {
    let mut state = owner(hwnd)
        .ok_or("Syntax smoke lost HWND")?
        .state
        .borrow_mut();
    state.close_selector();
    state.explorer_visible = false;
    state.explorer_focused = false;
    state.bottom.visible = false;
    state.bottom.focused = false;
    state
        .workbench
        .new_file(options.fixture.with_file_name(name), encoding);
    state
        .workbench
        .insert_text(text)
        .map_err(|e| e.to_string())?;
    state
        .workbench
        .go_to_position(TextPosition { line: 0, column: 0 })
        .map_err(|e| e.to_string())?;
    Ok(())
}
unsafe fn discard(hwnd: Hwnd) -> Result<(), String> {
    owner(hwnd)
        .unwrap()
        .state
        .borrow_mut()
        .workbench
        .close_active(true)
        .map_err(|e| e.to_string())?;
    expected(hwnd, SOURCE)
}
unsafe fn pump(hwnd: Hwnd) -> bool {
    let mut message: Msg = std::mem::zeroed();
    if PeekMessageW(&mut message, hwnd, TASKS_MESSAGE, TASKS_MESSAGE, 1) != 0 {
        DispatchMessageW(&message);
        return true;
    }
    if PeekMessageW(&mut message, hwnd, WM_PAINT, WM_PAINT, 1) != 0 {
        DispatchMessageW(&message);
        return true;
    }
    false
}
fn budgets(frame: &paint::Frame) -> Result<(), String> {
    for pane in [PaneId::Primary, PaneId::Secondary] {
        if let Some(stats) = frame.syntax_budget(pane) {
            if stats.processed_bytes > 64 * 1024 || stats.cache_bytes > 128 * 1024 {
                return Err(
                    "GUI syntax exceeded per-call work or per-document cache capacity".into(),
                );
            }
        }
    }
    Ok(())
}
unsafe fn ready(hwnd: Hwnd) -> Result<paint::Frame, String> {
    let initial = frame(hwnd);
    budgets(&initial)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while owner(hwnd).unwrap().state.borrow().syntax_pending {
        if Instant::now() >= deadline {
            let state = owner(hwnd).unwrap().state.borrow();
            return Err(format!("GUI syntax continuation deadline: source={:?} bytes={} initial primary={:?} secondary={:?} paints={} deliveries={}",
                state.workbench.active_path(),state.workbench.active_document().map_or(0,|d|d.session.document().len()),
                initial.syntax_budget(PaneId::Primary),initial.syntax_budget(PaneId::Secondary),state.paint_count,state.deliveries));
        }
        if !pump(hwnd) {
            std::thread::yield_now();
        }
    }
    let prepared = frame(hwnd);
    budgets(&prepared)?;
    Ok(prepared)
}
unsafe fn kind(
    hwnd: Hwnd,
    frame: &paint::Frame,
    pane: PaneId,
    byte: usize,
    expected: SyntaxKind,
) -> Result<paint::SyntaxCell, String> {
    // Convert the test's byte expectation through the existing projection. The
    // production Cell keeps only its kind/style, not a duplicate source map.
    let (line, column) = {
        let state = owner(hwnd).unwrap().state.borrow();
        let view = state
            .workbench
            .panes()
            .view(pane)
            .ok_or("Syntax pane is not open")?;
        let tab = view.location().ok_or("Syntax pane has no source")?.tab;
        let position = state
            .workbench
            .position_for_tab(tab, byte)
            .ok_or("Syntax fixture byte is not a scalar boundary")?;
        let (column, scroll) = if let Some(rect) = state.workbench.rectangle_for_pane(pane) {
            (
                state
                    .workbench
                    .rectangle_position_from_byte(tab, byte)
                    .map_err(|e| e.to_string())?
                    .column
                    .0,
                rect.scroll_column,
            )
        } else if state.workbench.invisibles_visible() {
            (
                state
                    .workbench
                    .display_column_at_byte(tab, position.line, byte)
                    .ok_or("Syntax diagnostic column missing")?,
                view.scroll_column(),
            )
        } else {
            (position.column, view.scroll_column())
        };
        (
            position.line,
            column
                .checked_sub(scroll)
                .ok_or("Syntax fixture byte is left of viewport")?,
        )
    };
    let cell = frame
        .syntax_cell_at(pane, line, column)
        .ok_or("Syntax fixture token is not visible")?;
    if cell.kind != expected {
        return Err(format!(
            "GUI syntax kind at byte{byte}: {:?}, expected{expected:?}",
            cell.kind
        ));
    }
    Ok(cell)
}
unsafe fn both(hwnd: Hwnd, byte: usize, expected: SyntaxKind) -> Result<(), String> {
    let prepared = ready(hwnd)?;
    kind(hwnd, &prepared, PaneId::Primary, byte, expected)?;
    kind(hwnd, &prepared, PaneId::Secondary, byte, expected)?;
    Ok(())
}
unsafe fn select(hwnd: Hwnd, start: usize, end: usize) -> Result<(), String> {
    owner(hwnd)
        .unwrap()
        .state
        .borrow_mut()
        .workbench
        .set_linear_selection_bytes(start, end)
        .map_err(|e| e.to_string())
}
unsafe fn search_ready(hwnd: Hwnd) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let status = owner(hwnd)
            .unwrap()
            .state
            .borrow()
            .workbench
            .search()
            .status;
        if status == SearchStatus::Complete {
            return Ok(());
        }
        if status != SearchStatus::Running {
            return Err(format!("Syntax/search fixture ended as{status:?}"));
        }
        if Instant::now() >= deadline {
            return Err("Syntax/search delivery timed out".into());
        }
        if !pump(hwnd) {
            std::thread::yield_now();
        }
    }
}
#[link(name = "user32")]
extern "system" {
    fn GetWindowRect(hwnd: Hwnd, rect: *mut Rect) -> i32;
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
struct Resize {
    hwnd: Hwnd,
    rect: Rect,
}
impl Resize {
    unsafe fn new(hwnd: Hwnd) -> Result<Self, String> {
        let mut rect = Rect::default();
        if GetWindowRect(hwnd, &mut rect) == 0 {
            return Err("Syntax own-window geometry unavailable".into());
        }
        Ok(Self { hwnd, rect })
    }
    unsafe fn set(&self, w: i32, h: i32) -> Result<(), String> {
        if SetWindowPos(self.hwnd, std::ptr::null_mut(), 0, 0, w, h, 0x16) == 0 {
            return Err("Syntax own-window resize failed".into());
        }
        Ok(())
    }
}
impl Drop for Resize {
    fn drop(&mut self) {
        unsafe {
            SetWindowPos(
                self.hwnd,
                std::ptr::null_mut(),
                0,
                0,
                self.rect.right - self.rect.left,
                self.rect.bottom - self.rect.top,
                0x16,
            );
        }
    }
}

pub(super) unsafe fn run(hwnd: Hwnd, options: &SmokeOptions) -> Result<Vec<&'static str>, String> {
    expected(hwnd, SOURCE)?;
    command(hwnd, CommandId::EditorOneGroup)?;
    let resize = Resize::new(hwnd)?;
    resize.set(1600, 900)?;
    let baseline = snapshot(hwnd)?;
    let (encoding, invisible, query) = {
        let mut state = owner(hwnd).unwrap().state.borrow_mut();
        state.close_selector();
        let values = (
            state.workbench.active_encoding(),
            state.workbench.invisibles_visible(),
            state.workbench.search().query.clone(),
        );
        state
            .workbench
            .set_search_query(String::new())
            .map_err(|e| e.to_string())?;
        values
    };
    if invisible {
        command(hwnd, CommandId::ViewToggleInvisibles)?;
    }
    let mut checks = Vec::new();
    memory(hwnd, options, "syntax-multiline.p", ABL, TextEncoding::Utf8)?;
    command(hwnd, CommandId::EditorSplitRight)?;
    command(hwnd, CommandId::EditorFocusPrimary)?;
    let last = ABL.rfind("DISPLAY").ok_or("Syntax last keyword missing")?;
    both(hwnd, last, SyntaxKind::Keyword)?;
    both(hwnd, ABL.find("nested").unwrap(), SyntaxKind::Comment)?;
    {
        let prepared = ready(hwnd)?;
        kind(
            hwnd,
            &prepared,
            PaneId::Primary,
            ABL.find("42").unwrap(),
            SyntaxKind::Number,
        )?;
        kind(
            hwnd,
            &prepared,
            PaneId::Primary,
            ABL.find('&').unwrap(),
            SyntaxKind::Preprocessor,
        )?;
    }
    let end = ABL.find("still */").unwrap() + "still ".len();
    select(hwnd, end, end + 2)?;
    key(hwnd, 0x2e, Modifiers::NONE)?;
    let mut open = ABL.to_owned();
    open.replace_range(end..end + 2, "");
    expected(hwnd, open.as_bytes())?;
    both(hwnd, open.rfind("DISPLAY").unwrap(), SyntaxKind::Comment)?;
    key(hwnd, 0x5a, Modifiers::CTRL)?;
    expected(hwnd, ABL.as_bytes())?;
    both(hwnd, last, SyntaxKind::Keyword)?;
    key(hwnd, 0x59, Modifiers::CTRL)?;
    expected(hwnd, open.as_bytes())?;
    both(hwnd, open.rfind("DISPLAY").unwrap(), SyntaxKind::Comment)?;
    key(hwnd, 0x5a, Modifiers::CTRL)?;
    expected(hwnd, ABL.as_bytes())?;
    position(hwnd, 0, 0)?;
    paste(hwnd, "/*")?;
    let prefix = format!("\u{feff}/*{}", &ABL['\u{feff}'.len_utf8()..]);
    expected(hwnd, prefix.as_bytes())?;
    both(hwnd, prefix.rfind("DISPLAY").unwrap(), SyntaxKind::Comment)?;
    key(hwnd, 0x5a, Modifiers::CTRL)?;
    expected(hwnd, ABL.as_bytes())?;
    both(hwnd, last, SyntaxKind::Keyword)?;
    command(hwnd, CommandId::EditorOneGroup)?;
    discard(hwnd)?;
    memory(hwnd, options, "syntax-string.p", QUOTE, TextEncoding::Utf8)?;
    command(hwnd, CommandId::EditorSplitRight)?;
    command(hwnd, CommandId::EditorFocusPrimary)?;
    both(hwnd, QUOTE.find("dos").unwrap(), SyntaxKind::String)?;
    let tilde = QUOTE.find('~').unwrap();
    select(hwnd, tilde, tilde + 1)?;
    key(hwnd, 0x2e, Modifiers::NONE)?;
    let mut uncontinued = QUOTE.to_owned();
    uncontinued.remove(tilde);
    expected(hwnd, uncontinued.as_bytes())?;
    both(hwnd, uncontinued.find("dos").unwrap(), SyntaxKind::Plain)?;
    position(hwnd, 0, tilde)?;
    paste(hwnd, "~")?;
    expected(hwnd, QUOTE.as_bytes())?;
    both(hwnd, QUOTE.find("dos").unwrap(), SyntaxKind::String)?;
    key(hwnd, 0x5a, Modifiers::CTRL)?;
    expected(hwnd, uncontinued.as_bytes())?;
    both(hwnd, uncontinued.find("dos").unwrap(), SyntaxKind::Plain)?;
    key(hwnd, 0x5a, Modifiers::CTRL)?;
    expected(hwnd, QUOTE.as_bytes())?;
    both(hwnd, QUOTE.find("dos").unwrap(), SyntaxKind::String)?;
    command(hwnd, CommandId::EditorOneGroup)?;
    discard(hwnd)?;
    memory(hwnd, options, "syntax-rust.rs", RUST, TextEncoding::Utf8)?;
    command(hwnd, CommandId::EditorSplitRight)?;
    command(hwnd, CommandId::EditorFocusPrimary)?;
    for (needle, kind) in [
        ("fn main", SyntaxKind::Keyword),
        ("line note", SyntaxKind::Comment),
        ("nested", SyntaxKind::Comment),
        ("dos", SyntaxKind::String),
        ("byte raw", SyntaxKind::String),
        ("body", SyntaxKind::String),
        ("42", SyntaxKind::Number),
    ] {
        both(
            hwnd,
            RUST.find(needle)
                .ok_or("Rust syntax fixture token missing")?,
            kind,
        )?;
    }
    for needle in ["'a'", "'\\n'"] {
        both(hwnd, RUST.find(needle).unwrap() + 1, SyntaxKind::String)?;
    }
    for needle in ["'a str", "'static str"] {
        both(
            hwnd,
            RUST.find(needle).unwrap() + 1,
            SyntaxKind::Preprocessor,
        )?;
    }
    let closing = RUST.find("body\"##").unwrap() + "body\"#".len();
    select(hwnd, closing, closing + 1)?;
    key(hwnd, 0x2e, Modifiers::NONE)?;
    let mut missing = RUST.to_owned();
    missing.remove(closing);
    own_expected(hwnd, missing.as_bytes())?;
    both(hwnd, missing.find("let tail").unwrap(), SyntaxKind::String)?;
    key(hwnd, 0x5a, Modifiers::CTRL)?;
    own_expected(hwnd, RUST.as_bytes())?;
    both(hwnd, RUST.find("let tail").unwrap(), SyntaxKind::Keyword)?;
    key(hwnd, 0x59, Modifiers::CTRL)?;
    own_expected(hwnd, missing.as_bytes())?;
    both(hwnd, missing.find("let tail").unwrap(), SyntaxKind::String)?;
    key(hwnd, 0x5a, Modifiers::CTRL)?;
    own_expected(hwnd, RUST.as_bytes())?;
    // This insertion splits the immutable source into pieces inside the raw
    // prefix; the same streaming lexer must carry its hash state across them.
    let opening = RUST.find("r##\"raw").unwrap() + "r##".len();
    select(hwnd, opening, opening)?;
    paste(hwnd, "#")?;
    let mut extra = RUST.to_owned();
    extra.insert(opening, '#');
    own_expected(hwnd, extra.as_bytes())?;
    both(hwnd, extra.find("let tail").unwrap(), SyntaxKind::String)?;
    key(hwnd, 0x5a, Modifiers::CTRL)?;
    own_expected(hwnd, RUST.as_bytes())?;
    both(hwnd, RUST.find("let tail").unwrap(), SyntaxKind::Keyword)?;
    command(hwnd, CommandId::EditorOneGroup)?;
    discard(hwnd)?;
    checks.push("syntax_shared_abl_multiline_edit_undo");

    for source_encoding in [
        TextEncoding::Utf8,
        TextEncoding::Windows1252,
        TextEncoding::Iso8859_1,
        TextEncoding::Ibm850,
    ] {
        let bom = if source_encoding == TextEncoding::Utf8 {
            "\u{feff}"
        } else {
            ""
        };
        let text = format!("{bom}DISPLAY \"niño\".\r\n/* ñ */\r\n");
        let original = bytes(&text, source_encoding)?;
        memory(hwnd, options, "syntax-encoding.p", &text, source_encoding)?;
        let keyword = offset(&text, "DISPLAY", source_encoding)?;
        let accent = offset(&text, "ñ", source_encoding)?;
        let prepared = ready(hwnd)?;
        let normal = kind(
            hwnd,
            &prepared,
            PaneId::Primary,
            keyword,
            SyntaxKind::Keyword,
        )?;
        if normal.foreground != rgb(SyntaxKind::Keyword) || normal.selected || normal.search {
            return Err("Unselected GUI keyword did not use shared syntax theme".into());
        }
        let rev = snapshot(hwnd)?.1;
        select(hwnd, keyword, keyword + 7)?;
        let selected = kind(
            hwnd,
            &ready(hwnd)?,
            PaneId::Primary,
            keyword,
            SyntaxKind::Keyword,
        )?;
        if !selected.selected
            || selected.foreground != super::shell::TEXT
            || selected.background == normal.background
        {
            return Err("Source selection did not override syntax foreground/background".into());
        }
        position(hwnd, 0, 0)?;
        command(hwnd, CommandId::SearchFind)?;
        key(hwnd, 0x55, Modifiers::CTRL)?;
        paste(hwnd, "DISPLAY")?;
        search_ready(hwnd)?;
        let found = kind(
            hwnd,
            &ready(hwnd)?,
            PaneId::Primary,
            keyword,
            SyntaxKind::Keyword,
        )?;
        if !found.search
            || found.foreground != super::shell::TEXT
            || found.background == normal.background
        {
            return Err("Literal search highlight did not override GUI syntax color".into());
        }
        key(hwnd, 0x1b, Modifiers::NONE)?;
        owner(hwnd)
            .unwrap()
            .state
            .borrow_mut()
            .workbench
            .set_search_query(String::new())
            .map_err(|e| e.to_string())?;
        command(hwnd, CommandId::ViewToggleInvisibles)?;
        let marked = kind(
            hwnd,
            &ready(hwnd)?,
            PaneId::Primary,
            accent,
            SyntaxKind::String,
        )?;
        if !marked.marker
            || marked.foreground == rgb(SyntaxKind::String)
            || marked.foreground == rgb(SyntaxKind::Plain)
        {
            return Err("Diagnostic marker did not take priority over lexical style".into());
        }
        select(hwnd, accent, accent + bytes("ñ", source_encoding)?.len())?;
        let selected_marker = kind(
            hwnd,
            &ready(hwnd)?,
            PaneId::Primary,
            accent,
            SyntaxKind::String,
        )?;
        if !selected_marker.selected
            || !selected_marker.marker
            || selected_marker.foreground != super::shell::TEXT
            || selected_marker.background == marked.background
        {
            return Err(
                "Selected diagnostic escape did not take priority over marker dimming".into(),
            );
        }
        position(hwnd, 0, 0)?;
        command(hwnd, CommandId::ViewToggleInvisibles)?;
        expected(hwnd, &original)?;
        if snapshot(hwnd)?.1 != rev
            || owner(hwnd)
                .unwrap()
                .state
                .borrow()
                .workbench
                .active_encoding()
                != Some(source_encoding)
        {
            return Err("Syntax/diagnostic/selection changed original encoding or revision".into());
        }
        discard(hwnd)?;
    }
    memory(
        hwnd,
        options,
        "syntax-grid.p",
        "a\tDISPLAY \"ñ\".\r\nx\r\n",
        TextEncoding::Utf8,
    )?;
    command(hwnd, CommandId::ViewToggleInvisibles)?;
    {
        use jc_app::rectangle::{VisualColumn, VisualPosition};
        owner(hwnd)
            .unwrap()
            .state
            .borrow_mut()
            .workbench
            .set_rectangle(
                VisualPosition {
                    line: 0,
                    column: VisualColumn(2),
                },
                VisualPosition {
                    line: 1,
                    column: VisualColumn(3),
                },
            )
            .map_err(|e| e.to_string())?;
    }
    let prepared = ready(hwnd)?;
    let virtual_cell = prepared
        .syntax_cell_at(PaneId::Primary, 1, 2)
        .ok_or("Virtual grid syntax cell missing")?;
    let tab = prepared
        .syntax_cell_at(PaneId::Primary, 0, 2)
        .ok_or("Partial TAB syntax cell missing")?;
    if virtual_cell.kind != SyntaxKind::Plain
        || !virtual_cell.selected
        || virtual_cell.foreground != super::shell::TEXT
        || !tab.selected
        || tab.foreground != super::shell::TEXT
    {
        return Err(
            "Virtual/TAB rectangle cells acquired false source syntax or lost selection priority"
                .into(),
        );
    }
    kind(hwnd, &prepared, PaneId::Primary, 2, SyntaxKind::Keyword)?;
    command(hwnd, CommandId::EditRectangleCancel)?;
    command(hwnd, CommandId::ViewToggleInvisibles)?;
    discard(hwnd)?;
    checks.push("syntax_encoding_invisibles_selection_precedence");

    let mut deep = String::from("/* context\r\n");
    for _ in 0..4096 {
        deep.push_str("bounded lexical context padding0123456789\r\n");
    }
    deep.push_str("*/\r\nDISPLAY 9.\r\n");
    memory(hwnd, options, "syntax-deep.p", &deep, TextEncoding::Utf8)?;
    let last_line = 4098;
    position(hwnd, last_line, 0)?;
    let deliveries = owner(hwnd).unwrap().state.borrow().deliveries;
    let first = frame(hwnd);
    budgets(&first)?;
    let byte = deep.rfind("DISPLAY").unwrap();
    if !first
        .syntax_budget(PaneId::Primary)
        .is_some_and(|stats| stats.pending)
    {
        return Err("Deep syntax request did not expose bounded pending context".into());
    }
    if !first.contains_status("Preparando resaltado") {
        return Err("Pending lexical context was not explicit in GUI presentation".into());
    }
    kind(hwnd, &first, PaneId::Primary, byte, SyntaxKind::Plain)?;
    let prepared = ready(hwnd)?;
    kind(hwnd, &prepared, PaneId::Primary, byte, SyntaxKind::Keyword)?;
    let stats = prepared
        .syntax_budget(PaneId::Primary)
        .ok_or("Deep syntax stats missing")?;
    if stats.pending
        || stats.limited
        || stats.scanned_to < byte
        || owner(hwnd).unwrap().state.borrow().deliveries <= deliveries
    {
        return Err(
            "Syntax failed to continue through owned TASKS/paint without keyboard input".into(),
        );
    }
    {
        let state = owner(hwnd).unwrap().state.borrow();
        let source = state
            .workbench
            .active_document()
            .unwrap()
            .session
            .document();
        if source.len() != deep.len()
            || source
                .read_range(byte..byte + "DISPLAY 9.\r\n".len())
                .map_err(|e| e.to_string())?
                != b"DISPLAY 9.\r\n"
        {
            return Err("Deep syntax operation changed source bytes".into());
        }
    }
    // Raw prefix crosses the cache's 4096B scan-chunk boundary. Existing scalar
    // caret reveal brings it into view; no horizontal pointer feature is added.
    let mut rust_chunk = " ".repeat(4093);
    rust_chunk.push_str("r##\"content\r\nraw\"##;\r\nfn tail() {}\r\n");
    memory(
        hwnd,
        options,
        "syntax-rust-chunk.rs",
        &rust_chunk,
        TextEncoding::Utf8,
    )?;
    position(hwnd, 0, 4097)?;
    kind(
        hwnd,
        &ready(hwnd)?,
        PaneId::Primary,
        4097,
        SyntaxKind::String,
    )?;
    position(hwnd, 2, 0)?;
    kind(
        hwnd,
        &ready(hwnd)?,
        PaneId::Primary,
        rust_chunk.find("fn tail").unwrap(),
        SyntaxKind::Keyword,
    )?;
    own_expected(hwnd, rust_chunk.as_bytes())?;
    owner(hwnd)
        .unwrap()
        .state
        .borrow_mut()
        .workbench
        .close_active(true)
        .map_err(|e| e.to_string())?;
    let rust_limit = format!("r{}\"x\"\r\nfn tail() {{}}\r\n", "#".repeat(33));
    memory(
        hwnd,
        options,
        "syntax-rust-limit.rs",
        &rust_limit,
        TextEncoding::Utf8,
    )?;
    let limited = ready(hwnd)?;
    let stats = limited
        .syntax_budget(PaneId::Primary)
        .ok_or("Rust hash-limit metadata missing")?;
    if !stats.limited || stats.pending || !limited.contains_status("Texto plano: límite") {
        return Err("Rust raw-hash budget failed to terminate in explicit Plain fallback".into());
    }
    kind(
        hwnd,
        &limited,
        PaneId::Primary,
        rust_limit.find("fn tail").unwrap(),
        SyntaxKind::Plain,
    )?;
    own_expected(hwnd, rust_limit.as_bytes())?;
    owner(hwnd)
        .unwrap()
        .state
        .borrow_mut()
        .workbench
        .close_active(true)
        .map_err(|e| e.to_string())?;
    memory(
        hwnd,
        options,
        "syntax-fallback.py",
        "DISPLAY \"plain\".\r\n/* plain */\r\n",
        TextEncoding::Utf8,
    )?;
    let fallback = ready(hwnd)?;
    kind(hwnd, &fallback, PaneId::Primary, 0, SyntaxKind::Plain)?;
    if !fallback.contains_status("Resaltado: texto plano") {
        return Err(
            "Unsupported lexical profile did not present its explicit Plain fallback".into(),
        );
    }
    memory(
        hwnd,
        options,
        "syntax-subset.json",
        "{\"name\":\"ñ\",\"n\":42,\"ok\":true}\r\n",
        TextEncoding::Utf8,
    )?;
    let prepared = ready(hwnd)?;
    kind(hwnd, &prepared, PaneId::Primary, 1, SyntaxKind::String)?;
    kind(
        hwnd,
        &prepared,
        PaneId::Primary,
        "{\"name\":\"ñ\",\"n\":".len(),
        SyntaxKind::Number,
    )?;
    kind(
        hwnd,
        &prepared,
        PaneId::Primary,
        "{\"name\":\"ñ\",\"n\":42,\"ok\":".len(),
        SyntaxKind::Keyword,
    )?;
    memory(
        hwnd,
        options,
        "syntax-subset.jsonc",
        "{/* note */\"n\":42}\r\n",
        TextEncoding::Utf8,
    )?;
    kind(hwnd, &ready(hwnd)?, PaneId::Primary, 3, SyntaxKind::Comment)?;
    for (path, source, keyword_at, literal) in [
        (
            "syntax-modern.cs",
            "class Demo { string x = @\"hola\"; }\r\n",
            0,
            "hola",
        ),
        (
            "syntax-modern.java",
            "class Demo { String x = \"hola\"; }\r\n",
            0,
            "hola",
        ),
        (
            "syntax-modern.cjs",
            "const x = require(\"./hola\");\r\n",
            0,
            "hola",
        ),
        (
            "syntax-modern.tsx",
            "interface Demo { x: \"hola\" }\r\n",
            0,
            "hola",
        ),
        (
            "syntax-modern.php",
            "<?php function demo() { return \"hola\"; }\r\n",
            6,
            "hola",
        ),
    ] {
        memory(hwnd, options, path, source, TextEncoding::Utf8)?;
        let frame = ready(hwnd)?;
        kind(
            hwnd,
            &frame,
            PaneId::Primary,
            keyword_at,
            SyntaxKind::Keyword,
        )?;
        kind(
            hwnd,
            &frame,
            PaneId::Primary,
            source.find(literal).unwrap(),
            SyntaxKind::String,
        )?;
        owner(hwnd)
            .unwrap()
            .state
            .borrow_mut()
            .workbench
            .close_active(true)
            .map_err(|e| e.to_string())?;
    }
    checks.push("syntax_native_modern_languages_keyword_and_string");
    for _ in 0..4 {
        owner(hwnd)
            .unwrap()
            .state
            .borrow_mut()
            .workbench
            .close_active(true)
            .map_err(|e| e.to_string())?;
    }
    expected(hwnd, SOURCE)?;
    checks.push("syntax_scroll_switch_pending_budget");

    memory(hwnd, options, "syntax-raster.p", ABL, TextEncoding::Utf8)?;
    command(hwnd, CommandId::EditorSplitRight)?;
    command(hwnd, CommandId::EditorFocusPrimary)?;
    ready(hwnd)?;
    for (width, height) in [(360, 260), (1400, 800), (360, 260), (1400, 800)] {
        resize.set(width, height)?;
        ready(hwnd)?;
        let before = snapshot(hwnd)?;
        let metrics = {
            let state = owner(hwnd).unwrap().state.borrow();
            (
                state.jobs.snapshot_bytes(),
                state.jobs.artifact_bytes(),
                state.deliveries,
            )
        };
        let first = frame(hwnd);
        let second = frame(hwnd);
        budgets(&first)?;
        budgets(&second)?;
        let byte = ABL.find("DISPLAY").unwrap();
        let a = kind(hwnd, &first, PaneId::Primary, byte, SyntaxKind::Keyword)?;
        let b = kind(hwnd, &second, PaneId::Primary, byte, SyntaxKind::Keyword)?;
        if a != b || snapshot(hwnd)? != before {
            return Err("Repeated syntax preparation changed source/view or visible style".into());
        }
        let state = owner(hwnd).unwrap().state.borrow();
        if metrics
            != (
                state.jobs.snapshot_bytes(),
                state.jobs.artifact_bytes(),
                state.deliveries,
            )
            || !state.workbench.panes().is_split()
        {
            return Err("Syntax preparation admitted/accepted work or discarded split".into());
        }
    }
    if let Some(bitmap) = &options.bitmap {
        let output = bitmap.with_file_name("native-syntax.bmp");
        if output.exists()
            || output == options.fixture
            || output == options.report
            || output == *bitmap
        {
            return Err("Syntax bitmap must be a new owned sibling".into());
        }
        let prepared = ready(hwnd)?;
        let before = snapshot(hwnd)?;
        paint::bitmap(hwnd, &prepared, &output)?;
        if snapshot(hwnd)? != before {
            return Err("Syntax GDI draw mutated the source model".into());
        }
    }
    expected(hwnd, ABL.as_bytes())?;
    command(hwnd, CommandId::EditorOneGroup)?;
    discard(hwnd)?;
    {
        let mut state = owner(hwnd).unwrap().state.borrow_mut();
        state.close_selector();
        state
            .workbench
            .set_search_query(query)
            .map_err(|e| e.to_string())?;
        state.workbench.cancel_search();
        if state.workbench.invisibles_visible() != invisible {
            state
                .workbench
                .execute(CommandId::ViewToggleInvisibles)
                .map_err(|e| e.to_string())?;
        }
        state.explorer_visible = false;
        state.explorer_focused = false;
        state.explorer_hits = None;
        state.bottom.visible = false;
        state.bottom.focused = false;
        state.bottom_hits = None;
    }
    position(hwnd, 0, 0)?;
    expected(hwnd, SOURCE)?;
    let after = snapshot(hwnd)?;
    if after.0 != baseline.0
        || after.1 != baseline.1
        || after.3 != baseline.3
        || owner(hwnd)
            .unwrap()
            .state
            .borrow()
            .workbench
            .active_encoding()
            != encoding
        || fs::read(&options.fixture).map_err(|e| e.to_string())? != SOURCE
    {
        return Err(
            "Syntax cleanup changed Source13B identity/revision/encoding/catalog/disk".into(),
        );
    }
    drop(resize);
    checks.push("syntax_readonly_compact_cleanup");
    Ok(checks)
}
