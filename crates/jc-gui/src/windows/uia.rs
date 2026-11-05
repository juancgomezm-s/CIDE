//! UI Automation bridge. The COM objects contain immutable identities and byte
//! offsets. Only the window thread may read the live editor.
use super::{ffi::*, paint, State};
use jc_app::{TabId, TaskStamp, Workbench};
use jc_text::{DecodedScalars, TextEncoding, TextPosition};
use std::{
    ffi::c_void,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

mod provider;
mod range;

pub(super) const GET_OBJECT: u32 = 0x003d;
pub(super) const REQUEST: u32 = 0x8000 + 71;
pub(super) const NOTIFY: u32 = 0x8000 + 72;
const UNAVAILABLE: i32 = 0x80040201u32 as i32;
const INVALID: i32 = 0x80070057u32 as i32;
const MAX_BYTES: usize = 64 * 1024;
static NEXT: AtomicUsize = AtomicUsize::new(1);

#[link(name = "user32")]
extern "system" {
    fn SendMessageTimeoutW(
        hwnd: Hwnd,
        message: u32,
        wparam: usize,
        lparam: isize,
        flags: u32,
        timeout: u32,
        result: *mut usize,
    ) -> isize;
    fn ClientToScreen(hwnd: Hwnd, point: *mut Point) -> i32;
}

#[link(name = "uiautomationcore")]
extern "system" {
    fn UiaReturnRawElementProvider(
        hwnd: Hwnd,
        wparam: usize,
        lparam: isize,
        provider: *mut c_void,
    ) -> isize;
    fn UiaClientsAreListening() -> i32;
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Window {
    pub hwnd: usize,
    id: usize,
}

pub(super) fn identity() -> usize {
    NEXT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
        .unwrap_or(0)
}

impl Window {
    pub(super) fn new(hwnd: Hwnd, id: usize) -> Self {
        Self {
            hwnd: hwnd as usize,
            id,
        }
    }
}

#[derive(Clone, Copy)]
struct Context {
    window: Window,
    stamp: TaskStamp,
    tab: TabId,
    start: usize,
    end: usize,
}

struct Snapshot {
    context: Context,
    len: usize,
    caret: usize,
    selection: Vec<(usize, usize)>,
    visible: (usize, usize),
    focused: bool,
    name: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct EventState {
    stamp: Option<TaskStamp>,
    selection: Option<(usize, Option<usize>)>,
    focused: bool,
}

pub(super) unsafe fn notify(hwnd: Hwnd) {
    if UiaClientsAreListening() == 0 {
        return;
    }
    let Some(owner) = super::owner(hwnd) else {
        return;
    };
    let window = Window::new(hwnd, owner.uia_id);
    let (before, after) = {
        let Ok(state) = owner.state.try_borrow() else {
            return;
        };
        let app = &state.workbench;
        let stamp = app.active_document().and_then(|doc| {
            Some(TaskStamp {
                document: doc.id,
                revision: doc.session.document().revision(),
                encoding: doc.session.encoding()?,
                view: app.panes().view(app.panes().active())?.view_id(),
            })
        });
        let after = EventState {
            stamp,
            selection: app
                .active_editor()
                .map(|editor| (editor.cursor, editor.anchor)),
            focused: stamp.is_some() && editor_focused(&state),
        };
        (owner.uia_events.replace(Some(after)), after)
    };
    if after.focused
        && before.is_none_or(|prior| {
            !prior.focused
                || prior.stamp.map(|stamp| stamp.view) != after.stamp.map(|stamp| stamp.view)
        })
    {
        provider::raise(window, 20005);
    }
    if before.is_some_and(|prior| prior.stamp != after.stamp) {
        provider::raise(window, 20015);
    }
    if before.is_some_and(|prior| prior.selection != after.selection) {
        provider::raise(window, 20014);
    }
}

enum Query {
    Snapshot,
    Bounds(Context, usize, i32),
    Move(Context, usize, i32, i32),
    Text(Context, usize, usize, usize),
    Select(Context, usize, usize),
    Point(i32, i32),
    Rectangles(Context, usize, usize),
}
enum Reply {
    Snapshot(Snapshot),
    Bounds(usize, usize),
    Move(usize, i32),
    Text(Vec<u16>),
    Selected,
    Point(Context, usize),
    Rectangles(Vec<f64>),
}
type Answer = Arc<Mutex<Option<Result<Reply, i32>>>>;
static PENDING: Mutex<Vec<(usize, Window, Query, Answer)>> = Mutex::new(Vec::new());

fn call(window: Window, query: Query) -> Result<Reply, i32> {
    let ticket = identity();
    let answer = Arc::new(Mutex::new(None));
    {
        let mut pending = PENDING.lock().map_err(|_| UNAVAILABLE)?;
        if ticket == 0 || window.id == 0 || pending.len() >= 16 {
            return Err(UNAVAILABLE);
        }
        pending.push((ticket, window, query, answer.clone()));
    }
    // Send only an opaque ticket across the native boundary. On timeout the
    // window can finish a dequeued request safely using its own Arc.
    let mut result = 0;
    unsafe {
        SendMessageTimeoutW(
            window.hwnd as Hwnd,
            REQUEST,
            ticket,
            0,
            0x22,
            1000,
            &mut result,
        );
    }
    PENDING
        .lock()
        .map_err(|_| UNAVAILABLE)?
        .retain(|entry| entry.0 != ticket);
    let response = answer.lock().map_err(|_| UNAVAILABLE)?.take();
    response.unwrap_or(Err(UNAVAILABLE))
}

pub(super) fn serve(window: Window, ticket: usize, state: &std::cell::RefCell<State>) -> bool {
    let request = PENDING.lock().ok().and_then(|mut pending| {
        let index = pending
            .iter()
            .position(|entry| entry.0 == ticket && entry.1 == window)?;
        Some(pending.swap_remove(index))
    });
    if let Some((_, _, request, answer)) = request {
        let result = match request {
            Query::Select(context, start, end) => state
                .try_borrow_mut()
                .map_err(|_| UNAVAILABLE)
                .and_then(|mut state| select(window, &mut state, context, start, end)),
            request => state
                .try_borrow()
                .map_err(|_| UNAVAILABLE)
                .and_then(|state| evaluate(window, &state, request)),
        };
        let changed = matches!(&result, Ok(Reply::Selected));
        if let Ok(mut slot) = answer.lock() {
            *slot = Some(result);
        }
        return changed;
    }
    false
}

pub(super) unsafe fn get_object(window: Window, wparam: usize, lparam: isize) -> isize {
    let provider = provider::create(window);
    let result = UiaReturnRawElementProvider(window.hwnd as Hwnd, wparam, lparam, provider);
    provider::release(provider);
    result
}

fn query(window: Window) -> Result<Snapshot, i32> {
    match call(window, Query::Snapshot)? {
        Reply::Snapshot(snapshot) => Ok(snapshot),
        _ => Err(UNAVAILABLE),
    }
}

fn point(window: Window, x: f64, y: f64) -> Result<(Context, usize), i32> {
    if ![x, y]
        .iter()
        .all(|v| v.is_finite() && *v >= i32::MIN as f64 && *v <= i32::MAX as f64)
    {
        return Err(INVALID);
    }
    match call(window, Query::Point(x as i32, y as i32))? {
        Reply::Point(context, offset) => Ok((context, offset)),
        _ => Err(UNAVAILABLE),
    }
}

impl Context {
    fn rectangles(&self, start: usize, end: usize) -> Result<Vec<f64>, i32> {
        match call(self.window, Query::Rectangles(*self, start, end))? {
            Reply::Rectangles(rectangles) => Ok(rectangles),
            _ => Err(UNAVAILABLE),
        }
    }
    fn select(&self, start: usize, end: usize) -> Result<(), i32> {
        match call(self.window, Query::Select(*self, start, end))? {
            Reply::Selected => Ok(()),
            _ => Err(UNAVAILABLE),
        }
    }
    fn same_document(&self, other: &Self) -> bool {
        self.window == other.window && self.stamp == other.stamp && self.tab == other.tab
    }
    fn len(&self) -> Result<usize, i32> {
        self.unit_bounds(self.start, 6).map(|(_, end)| end)
    }
    fn unit_bounds(&self, offset: usize, unit: i32) -> Result<(usize, usize), i32> {
        match call(self.window, Query::Bounds(*self, offset, unit))? {
            Reply::Bounds(start, end) => Ok((start, end)),
            _ => Err(UNAVAILABLE),
        }
    }
    fn move_endpoint(&self, offset: usize, unit: i32, count: i32) -> Result<(usize, i32), i32> {
        match call(self.window, Query::Move(*self, offset, unit, count))? {
            Reply::Move(offset, count) => Ok((offset, count)),
            _ => Err(UNAVAILABLE),
        }
    }
    fn text(&self, start: usize, end: usize, max_utf16: usize) -> Result<Vec<u16>, i32> {
        match call(self.window, Query::Text(*self, start, end, max_utf16))? {
            Reply::Text(text) => Ok(text),
            _ => Err(UNAVAILABLE),
        }
    }
    fn provider(&self) -> Result<*mut c_void, i32> {
        if !query(self.window)?.context.same_document(self) {
            return Err(UNAVAILABLE);
        }
        Ok(provider::create(self.window))
    }
}

fn select(
    window: Window,
    state: &mut State,
    context: Context,
    start: usize,
    end: usize,
) -> Result<Reply, i32> {
    if !snapshot(window, state)?.context.same_document(&context)
        || state.selector.is_some()
        || state.markdown_gui.is_some()
        || !state.context().editor_input
    {
        return Err(UNAVAILABLE);
    }
    if start < context.start || start > end || end > context.end {
        return Err(INVALID);
    }
    state
        .workbench
        .set_linear_selection_bytes(start, end)
        .map_err(|_| INVALID)?;
    state.cancel_pointer();
    Ok(Reply::Selected)
}

fn editor_focused(state: &State) -> bool {
    state.focused
        && state.selector.is_none()
        && state.markdown_gui.is_none()
        && state.context().editor_input
}

fn snapshot(window: Window, state: &State) -> Result<Snapshot, i32> {
    let app = &state.workbench;
    let pane = app.panes().active();
    let doc = app.active_document().ok_or(UNAVAILABLE)?;
    let view = app.panes().view(pane).ok_or(UNAVAILABLE)?;
    let source = doc.session.document();
    let encoding = doc.session.encoding().ok_or(UNAVAILABLE)?;
    let start = app
        .line_range_for_tab(doc.tab_id, 0)
        .ok_or(UNAVAILABLE)?
        .0
        .start;
    let end = source.len();
    let context = Context {
        window,
        stamp: TaskStamp {
            document: doc.id,
            revision: source.revision(),
            encoding,
            view: Some(view.view_id().ok_or(UNAVAILABLE)?),
        },
        tab: doc.tab_id,
        start,
        end,
    };
    let caret = view
        .location()
        .ok_or(UNAVAILABLE)?
        .byte_offset
        .clamp(start, end);
    let selection = app
        .selection_for_pane(pane)
        .map(|range| (range.start.max(start), range.end.max(start)));
    let visible = paint::text_areas(state)
        .into_iter()
        .find(|area| area.pane == pane && area.rows > 0)
        .and_then(|area| {
            let first = app.line_range_for_tab(context.tab, area.first)?.0.start;
            let last = app
                .line_range_for_tab(context.tab, area.first + area.rows - 1)?
                .1
                .end;
            Some((first, last))
        })
        .unwrap_or((caret, caret));
    let name = doc.session.path().file_name().map_or_else(
        || "Editor CIDE".to_owned(),
        |name| jc_app::source_services::display_ui_label(&name.to_string_lossy(), 120),
    );
    Ok(Snapshot {
        context,
        len: end,
        caret,
        selection: selection.into_iter().collect(),
        visible,
        focused: editor_focused(state),
        name,
    })
}

fn evaluate(window: Window, state: &State, request: Query) -> Result<Reply, i32> {
    let context = match request {
        Query::Snapshot => return snapshot(window, state).map(Reply::Snapshot),
        Query::Point(x, y) => {
            let current = snapshot(window, state)?;
            if state.markdown_gui.is_some() || state.open_diff_visible {
                return Err(UNAVAILABLE);
            }
            let mut point = Point { x, y };
            if unsafe { ScreenToClient(window.hwnd as Hwnd, &mut point) } == 0 {
                return Err(UNAVAILABLE);
            }
            let pane = state.workbench.panes().active();
            let (_, offset) = paint::linear_hit(state, point.x, point.y, Some(pane))
                .map_err(|_| UNAVAILABLE)?
                .ok_or(UNAVAILABLE)?;
            return Ok(Reply::Point(current.context, offset));
        }
        Query::Select(..) => return Err(INVALID),
        Query::Bounds(context, ..)
        | Query::Move(context, ..)
        | Query::Text(context, ..)
        | Query::Rectangles(context, ..) => context,
    };
    let app = &state.workbench;
    if context.window != window || !app.task_stamp_is_current(context.stamp) {
        return Err(UNAVAILABLE);
    }
    match request {
        Query::Snapshot | Query::Select(..) | Query::Point(..) => unreachable!(),
        Query::Rectangles(_, start, end) => {
            rectangles(state, context, start, end).map(Reply::Rectangles)
        }
        Query::Bounds(_, offset, unit) => {
            let (a, b) = bounds(app, context, offset, unit)?;
            Ok(Reply::Bounds(a, b))
        }
        Query::Move(_, offset, unit, count) => {
            let (offset, moved) = move_endpoint(app, context, offset, unit, count)?;
            Ok(Reply::Move(offset, moved))
        }
        Query::Text(_, start, end, max) => {
            if start < context.start || end < start || end > context.end {
                return Err(INVALID);
            }
            let doc = app
                .document_by_id(context.stamp.document)
                .ok_or(UNAVAILABLE)?;
            let stop = end.min(start.saturating_add(MAX_BYTES));
            let bytes = doc
                .session
                .document()
                .read_range(start..stop)
                .map_err(|_| UNAVAILABLE)?;
            Ok(Reply::Text(encode_text(
                &bytes,
                context.stamp.encoding,
                start,
                stop < end,
                max.min(MAX_BYTES),
            )))
        }
    }
}

fn rectangles(state: &State, context: Context, start: usize, end: usize) -> Result<Vec<f64>, i32> {
    if start < context.start || start > end || end > context.end {
        return Err(INVALID);
    }
    if state.markdown_gui.is_some() || state.open_diff_visible {
        return Ok(Vec::new());
    }
    let app = &state.workbench;
    let pane = app.panes().active();
    let Some(area) = paint::text_areas(state).into_iter().find(|area| {
        area.pane == pane
            && area.tab == context.tab
            && app.panes().view(pane).and_then(|view| view.view_id()) == context.stamp.view
    }) else {
        return Ok(Vec::new());
    };
    let mut origin = Point {
        x: area.x,
        y: area.y,
    };
    if unsafe { ClientToScreen(context.window.hwnd as Hwnd, &mut origin) } == 0 {
        return Err(UNAVAILABLE);
    }
    let cw = f64::from(state.cell_width.max(1));
    let ch = f64::from(state.cell_height.max(1));
    let first = app
        .line_ordinal_for_byte(context.tab, start)
        .ok_or(INVALID)?;
    let last = app
        .line_ordinal_for_byte(context.tab, if start == end { end } else { end - 1 })
        .ok_or(INVALID)?;
    let mut result = Vec::new();
    for line in first.max(area.first)
        ..=last.min(
            area.first
                .saturating_add(area.rows.min(128))
                .saturating_sub(1),
        )
    {
        if area.rows == 0 {
            break;
        }
        let (body, eol) = app
            .line_range_for_tab(context.tab, line)
            .ok_or(UNAVAILABLE)?;
        let column = |offset| {
            if app.invisibles_visible() {
                app.display_column_at_byte(context.tab, line, offset)
            } else {
                app.visual_column_at_byte(context.tab, line, offset)
            }
        };
        let left = column(start.clamp(body.start, body.end)).ok_or(INVALID)?;
        let right = column(end.clamp(body.start, body.end))
            .ok_or(INVALID)?
            .saturating_add(usize::from(end > body.end && eol.end > body.end));
        let clipped_left = left.max(area.scroll);
        let clipped_right = right.min(area.scroll.saturating_add(area.columns));
        let caret =
            start == end && left >= area.scroll && left < area.scroll.saturating_add(area.columns);
        if caret || clipped_right > clipped_left {
            result.extend([
                f64::from(origin.x) + (clipped_left - area.scroll) as f64 * cw,
                f64::from(origin.y) + (line - area.first) as f64 * ch,
                if caret {
                    1.0
                } else {
                    (clipped_right - clipped_left) as f64 * cw
                },
                ch,
            ]);
        }
    }
    Ok(result)
}

fn encode_text(
    bytes: &[u8],
    encoding: TextEncoding,
    start: usize,
    clipped: bool,
    max: usize,
) -> Vec<u16> {
    let mut result = Vec::new();
    for scalar in DecodedScalars::new(bytes, encoding, start) {
        let mut utf16 = [0; 2];
        let escape;
        let units: &[u16] = if scalar.valid {
            scalar.character.encode_utf16(&mut utf16)
        } else {
            if clipped && scalar.source_range.end == start + bytes.len() {
                break;
            }
            escape = bytes[scalar.source_range.start - start..scalar.source_range.end - start]
                .iter()
                .flat_map(|byte| format!("\\x{byte:02X}").encode_utf16().collect::<Vec<_>>())
                .collect::<Vec<_>>();
            &escape
        };
        if units.len() > max.saturating_sub(result.len()) {
            break;
        }
        result.extend_from_slice(units);
    }
    result
}

fn bounds(
    app: &Workbench,
    context: Context,
    offset: usize,
    unit: i32,
) -> Result<(usize, usize), i32> {
    if offset < context.start || offset > context.end {
        return Err(INVALID);
    }
    match unit {
        0 if offset == context.end => Ok((offset, offset)),
        0 => {
            let position = app.position_for_tab(context.tab, offset).ok_or(INVALID)?;
            let next = app
                .byte_offset_for_tab(
                    context.tab,
                    TextPosition {
                        line: position.line,
                        column: position.column + 1,
                    },
                )
                .or_else(|| {
                    app.line_range_for_tab(context.tab, position.line)
                        .map(|(_, eol)| eol.end)
                })
                .ok_or(INVALID)?;
            Ok((offset, next))
        }
        3 => {
            let line = app
                .line_ordinal_for_byte(context.tab, offset)
                .ok_or(INVALID)?;
            let (body, eol) = app.line_range_for_tab(context.tab, line).ok_or(INVALID)?;
            Ok((body.start, eol.end))
        }
        6 => Ok((context.start, context.end)),
        _ => Err(INVALID),
    }
}

fn move_endpoint(
    app: &Workbench,
    context: Context,
    mut offset: usize,
    unit: i32,
    count: i32,
) -> Result<(usize, i32), i32> {
    let mut moved = 0;
    let step = count.signum();
    for _ in 0..count.unsigned_abs().min(256) {
        let next = if step > 0 {
            bounds(app, context, offset, unit)?.1
        } else if unit == 6 {
            context.start
        } else if unit == 3 {
            let start = bounds(app, context, offset, unit)?.0;
            if start == offset && start > context.start {
                bounds(app, context, start - 1, unit)?.0
            } else {
                start
            }
        } else if offset > context.start {
            let position = app.position_for_tab(context.tab, offset).ok_or(INVALID)?;
            if let Some(column) = position.column.checked_sub(1) {
                app.byte_offset_for_tab(
                    context.tab,
                    TextPosition {
                        line: position.line,
                        column,
                    },
                )
                .ok_or(INVALID)?
            } else {
                let line = position.line.checked_sub(1).ok_or(INVALID)?;
                app.line_range_for_tab(context.tab, line)
                    .ok_or(INVALID)?
                    .0
                    .end
            }
        } else {
            offset
        };
        if next == offset {
            break;
        }
        offset = next;
        moved += step;
    }
    Ok((offset, moved))
}
