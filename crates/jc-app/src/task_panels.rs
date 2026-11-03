//! Bounded, shared task logs. Rows own display text once and optional navigation
//! metadata; neither logs nor selections own source bytes or provider snapshots.
use crate::{
    jobs::{CompletedTask, TaskItem, TaskStamp, TaskStatus},
    Workbench,
};
use std::{
    mem::size_of,
    ops::{Deref, Range},
    path::{Path, PathBuf},
};

const MAX_BYTES: usize = 256 * 1024;
const MAX_TEXT: usize = 4096;
const TRUNCATED: &str = " [truncated]";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelKind {
    Problems,
    Output,
}
impl PanelKind {
    fn max_rows(self) -> usize {
        match self {
            Self::Problems => 1000,
            Self::Output => 2000,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelTarget {
    pub stamp: TaskStamp,
    pub path: PathBuf,
    pub range: Range<usize>,
}
#[derive(Debug)]
pub struct PanelRow {
    pub id: u64,
    pub text: String,
    /// Source provenance, not permission to navigate. Only a Problems row's
    /// diagnostic range can be activated; Output origins use the empty 0..0.
    pub target: Option<PanelTarget>,
}
impl Deref for PanelRow {
    type Target = String;
    fn deref(&self) -> &String {
        &self.text
    }
}
impl PanelRow {
    fn owned_bytes(&self) -> usize {
        self.text.capacity().saturating_add(
            self.target
                .as_ref()
                .map_or(0, |target| target.path.capacity()),
        )
    }
}
#[derive(Debug, Default)]
struct Log {
    rows: Vec<PanelRow>,
    owned: usize,
    dropped: usize,
    truncated: usize,
}
impl Log {
    fn bytes(&self) -> usize {
        self.rows
            .capacity()
            .saturating_mul(size_of::<PanelRow>())
            .saturating_add(self.owned)
    }
    fn evict(&mut self) {
        let row = self.rows.remove(0);
        self.owned = self.owned.saturating_sub(row.owned_bytes());
        self.dropped = self.dropped.saturating_add(1);
    }
    fn push(&mut self, kind: PanelKind, row: PanelRow, truncated: bool) -> bool {
        let owned = row.owned_bytes();
        if owned > MAX_BYTES.saturating_sub(size_of::<PanelRow>()) {
            self.dropped = self.dropped.saturating_add(1);
            return false;
        }
        loop {
            let needed = self.rows.len() + 1;
            if needed > kind.max_rows() || self.bytes().saturating_add(owned) > MAX_BYTES {
                if self.rows.is_empty() {
                    // Release a formerly large metadata allocation before
                    // admitting one larger row, without growing a second log.
                    self.rows = Vec::new();
                } else {
                    self.evict();
                    continue;
                }
            }
            if self.rows.len() == self.rows.capacity() {
                let available = MAX_BYTES.saturating_sub(self.owned.saturating_add(owned))
                    / size_of::<PanelRow>();
                if available < self.rows.len() + 1 {
                    if self.rows.is_empty() {
                        self.dropped = self.dropped.saturating_add(1);
                        return false;
                    }
                    self.evict();
                    continue;
                }
                let capacity = (self.rows.len() + 32).min(kind.max_rows()).min(available);
                if self
                    .rows
                    .try_reserve_exact(capacity - self.rows.len())
                    .is_err()
                {
                    self.dropped = self.dropped.saturating_add(1);
                    return false;
                }
                // Capacity, rather than the requested allocation, is charged.
                if self.bytes().saturating_add(owned) > MAX_BYTES {
                    self.rows.shrink_to_fit();
                    if self.bytes().saturating_add(owned) > MAX_BYTES {
                        if self.rows.is_empty() {
                            self.dropped = self.dropped.saturating_add(1);
                            return false;
                        }
                        self.evict();
                        continue;
                    }
                }
            }
            self.owned += owned;
            self.rows.push(row);
            self.truncated = self.truncated.saturating_add(usize::from(truncated));
            return true;
        }
    }
}

#[derive(Debug)]
pub struct TaskPanels {
    problems: Log,
    output: Log,
    next_id: Option<u64>,
}
impl Default for TaskPanels {
    fn default() -> Self {
        Self {
            problems: Log::default(),
            output: Log::default(),
            next_id: Some(1),
        }
    }
}
fn bounded(text: &str) -> (String, bool) {
    if text.len() <= MAX_TEXT {
        return (text.to_owned(), false);
    }
    let mut end = MAX_TEXT - TRUNCATED.len();
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut result = String::with_capacity(end + TRUNCATED.len());
    result.push_str(&text[..end]);
    result.push_str(TRUNCATED);
    (result, true)
}
impl TaskPanels {
    fn log(&self, kind: PanelKind) -> &Log {
        match kind {
            PanelKind::Problems => &self.problems,
            PanelKind::Output => &self.output,
        }
    }
    fn log_mut(&mut self, kind: PanelKind) -> &mut Log {
        match kind {
            PanelKind::Problems => &mut self.problems,
            PanelKind::Output => &mut self.output,
        }
    }
    pub fn rows(&self, kind: PanelKind) -> &[PanelRow] {
        &self.log(kind).rows
    }
    pub fn push(&mut self, kind: PanelKind, text: String) {
        self.push_borrowed(kind, &text, None);
    }
    /// Heap capacity charge, not private-process RAM or transient provider data.
    pub fn memory_bytes(&self, kind: PanelKind) -> usize {
        self.log(kind).bytes()
    }
    pub fn dropped(&self, kind: PanelKind) -> usize {
        self.log(kind).dropped
    }
    pub fn truncated(&self, kind: PanelKind) -> usize {
        self.log(kind).truncated
    }
    /// Clear rows and counters; stable IDs are never reused, even after reset.
    pub fn clear(&mut self, kind: PanelKind) {
        *self.log_mut(kind) = Log::default();
    }
    pub fn reset(&mut self) {
        self.clear(PanelKind::Problems);
        self.clear(PanelKind::Output);
    }
    fn push_borrowed(&mut self, kind: PanelKind, text: &str, target: Option<PanelTarget>) -> bool {
        let Some(id) = self.next_id else {
            let log = self.log_mut(kind);
            log.dropped = log.dropped.saturating_add(1);
            return false;
        };
        self.next_id = id.checked_add(1);
        let (text, truncated) = bounded(text);
        self.log_mut(kind)
            .push(kind, PanelRow { id, text, target }, truncated)
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PanelSelection {
    pub selected: Option<u64>,
    pub scroll: usize,
}
impl PanelSelection {
    pub fn selected_index(&self, log: &TaskPanels, kind: PanelKind) -> Option<usize> {
        let id = self.selected?;
        log.rows(kind).iter().position(|row| row.id == id)
    }
    pub fn normalize(&mut self, log: &TaskPanels, kind: PanelKind, visible: usize) {
        let rows = log.rows(kind);
        if rows.is_empty() {
            self.selected = None;
            self.scroll = 0;
            return;
        }
        let index = self.selected_index(log, kind).unwrap_or_else(|| {
            self.selected.map_or(0, |id| {
                rows.partition_point(|row| row.id < id).min(rows.len() - 1)
            })
        });
        self.selected = Some(rows[index].id);
        let visible = visible.clamp(1, 128);
        self.scroll = self.scroll.min(rows.len().saturating_sub(visible));
        if index < self.scroll {
            self.scroll = index;
        } else if index >= self.scroll.saturating_add(visible) {
            self.scroll = index + 1 - visible;
        }
    }
    pub fn move_by(&mut self, log: &TaskPanels, kind: PanelKind, delta: isize) {
        let rows = log.rows(kind);
        if rows.is_empty() {
            self.selected = None;
            self.scroll = 0;
            return;
        }
        let index = self
            .selected_index(log, kind)
            .unwrap_or(0)
            .saturating_add_signed(delta)
            .min(rows.len() - 1);
        self.selected = Some(rows[index].id);
    }
    pub fn first(&mut self, log: &TaskPanels, kind: PanelKind) {
        self.selected = log.rows(kind).first().map(|row| row.id);
        self.scroll = 0;
    }
    pub fn last(&mut self, log: &TaskPanels, kind: PanelKind) {
        let rows = log.rows(kind);
        self.selected = rows.last().map(|row| row.id);
        self.scroll = rows.len().saturating_sub(1);
    }
}

impl Workbench {
    /// Returns whether the visible logs changed, including explicit discard or
    /// failure status. A true return does not mean provider output was accepted.
    pub fn accept_task_completion(&mut self, completion: &CompletedTask) -> bool {
        self.accept_panel_items(
            completion.stamp,
            &completion.path,
            completion.status,
            completion.applicable,
            completion.current,
            completion.output.items(),
        )
    }
    fn accept_panel_items(
        &mut self,
        stamp: TaskStamp,
        path: &Path,
        status: TaskStatus,
        applicable: bool,
        current: bool,
        items: &[TaskItem],
    ) -> bool {
        let current = current
            && self.task_stamp_is_current(stamp)
            && self
                .document_by_id(stamp.document)
                .is_some_and(|doc| doc.session.path() == path);
        if !applicable || !current || status != TaskStatus::Completed {
            let message = match status {
                TaskStatus::Cancelled => "Task cancelled; output discarded",
                TaskStatus::TimedOut => "Task timed out; output discarded",
                TaskStatus::Failed => "Task failed; output discarded",
                TaskStatus::Panicked => "Task provider panicked; output discarded",
                TaskStatus::LimitExceeded => "Task result limit exceeded; output discarded",
                TaskStatus::InvalidRange => "Task returned an invalid range; output discarded",
                TaskStatus::Completed => "Task result stale or superseded; output discarded",
            };
            return self
                .task_panels
                .push_borrowed(PanelKind::Output, message, None);
        }
        // Every provider-origin row retains its authority metadata. Reject
        // oversized metadata before cloning paths or retaining provider text.
        if path.as_os_str().len() > MAX_BYTES - MAX_TEXT - size_of::<PanelRow>() {
            return self.task_panels.push_borrowed(
                PanelKind::Output,
                "Task output discarded: source metadata limit exceeded",
                None,
            );
        }
        let mut changed = false;
        for item in items {
            match item {
                TaskItem::Source(_) => continue, // Typed source controller owns this delivery.
                TaskItem::Diagnostic { range, message } => {
                    let valid = self.document_by_id(stamp.document).is_some_and(|doc| {
                        range.start <= range.end
                            && range.end <= doc.session.document().len()
                            && self.position_for_tab(doc.tab_id, range.start).is_some()
                            && self.position_for_tab(doc.tab_id, range.end).is_some()
                    });
                    if !valid {
                        changed |= self.task_panels.push_borrowed(
                            PanelKind::Output,
                            "Task diagnostic discarded: invalid range",
                            None,
                        );
                        continue;
                    }
                    let target = PanelTarget {
                        stamp,
                        path: path.to_path_buf(),
                        range: range.clone(),
                    };
                    changed |=
                        self.task_panels
                            .push_borrowed(PanelKind::Problems, message, Some(target));
                }
                TaskItem::Message(message) => {
                    changed |= self.task_panels.push_borrowed(
                        PanelKind::Output,
                        message,
                        Some(PanelTarget {
                            stamp,
                            path: path.to_path_buf(),
                            range: 0..0,
                        }),
                    )
                }
                TaskItem::ProposedEdit { range, text } => {
                    changed |= self.task_panels.push_borrowed(
                        PanelKind::Output,
                        &format!(
                            "Task proposed edit at bytes {}..{} ({} UTF-8 bytes); not applied",
                            range.start,
                            range.end,
                            text.len()
                        ),
                        Some(PanelTarget {
                            stamp,
                            path: path.to_path_buf(),
                            range: 0..0,
                        }),
                    );
                }
            }
        }
        changed |= self.task_panels.push_borrowed(
            PanelKind::Output,
            if items.is_empty() {
                "Task completed with no items"
            } else {
                "Task completed"
            },
            Some(PanelTarget {
                stamp,
                path: path.to_path_buf(),
                range: 0..0,
            }),
        );
        changed
    }
    /// Only a still-open, identically versioned target can navigate. No file is
    /// opened, decoded wholesale, written or silently refreshed from disk.
    pub fn activate_problem(&mut self, row_id: u64) -> Result<(), String> {
        let target = self
            .task_panels
            .rows(PanelKind::Problems)
            .iter()
            .find(|row| row.id == row_id)
            .and_then(|row| row.target.clone())
            .ok_or("Problem has no current navigation target")?;
        if !self.task_stamp_is_current(target.stamp) {
            return Err("Problem target is stale; refresh the task".into());
        }
        let doc = self
            .document_by_id(target.stamp.document)
            .ok_or("Problem document is closed")?;
        if doc.session.path() != target.path
            || target.range.start > target.range.end
            || target.range.end > doc.session.document().len()
        {
            return Err("Problem path or byte range changed".into());
        }
        let tab = doc.tab_id;
        if self.position_for_tab(tab, target.range.start).is_none()
            || self.position_for_tab(tab, target.range.end).is_none()
        {
            return Err("Problem range is not a valid source boundary".into());
        }
        if !self.jump_to_location(tab, target.range.start) {
            return Err("Problem target could not be activated".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PaneId;
    use jc_text::{TextEncoding, TextPosition};
    fn app() -> Workbench {
        let mut app = Workbench::default();
        app.new_file("task-panel-memory.txt", TextEncoding::Utf8);
        app.insert_text("\u{feff}é\r\nlast").unwrap();
        app
    }
    fn capture_stamp(app: &Workbench, view: Option<crate::ViewId>) -> TaskStamp {
        let doc = app.active_document().unwrap();
        TaskStamp {
            document: doc.id,
            revision: doc.session.document().revision(),
            encoding: doc.session.encoding().unwrap(),
            view,
        }
    }
    #[test]
    fn retained_capacity_unicode_truncation_and_ids_survive_clear_and_eviction() {
        let mut panels = TaskPanels::default();
        let mut overallocated = String::with_capacity(2 * MAX_BYTES);
        overallocated.push_str("short");
        panels.push(PanelKind::Output, overallocated);
        panels.push(PanelKind::Output, "é".repeat(MAX_TEXT));
        assert_eq!(panels.truncated(PanelKind::Output), 1);
        let row = &panels.rows(PanelKind::Output)[1];
        assert!(row.text.len() <= MAX_TEXT && row.text.ends_with(TRUNCATED));
        let old = row.id;
        for _ in 0..2100 {
            panels.push(PanelKind::Output, "line".into());
        }
        for _ in 0..1100 {
            panels.push(PanelKind::Problems, "problem".into());
        }
        for kind in [PanelKind::Problems, PanelKind::Output] {
            assert!(panels.rows(kind).len() <= kind.max_rows());
            assert!(panels.memory_bytes(kind) <= MAX_BYTES);
            assert!(panels.dropped(kind) > 0);
        }
        let source = app();
        let mut excessive_path = PathBuf::with_capacity(MAX_BYTES);
        excessive_path.push("x");
        let dropped = panels.dropped(PanelKind::Problems);
        assert!(!panels.push_borrowed(
            PanelKind::Problems,
            "metadata charge",
            Some(PanelTarget {
                stamp: capture_stamp(&source, None),
                path: excessive_path,
                range: 0..0,
            })
        ));
        assert_eq!(panels.dropped(PanelKind::Problems), dropped + 1);
        assert!(panels.memory_bytes(PanelKind::Problems) <= MAX_BYTES);
        let mut selection = PanelSelection {
            selected: Some(old),
            scroll: usize::MAX,
        };
        selection.normalize(&panels, PanelKind::Output, 3);
        assert!(selection
            .selected_index(&panels, PanelKind::Output)
            .is_some());
        selection.last(&panels, PanelKind::Output);
        selection.normalize(&panels, PanelKind::Output, 3);
        let last = selection.selected.unwrap();
        selection.move_by(&panels, PanelKind::Output, -1);
        assert!(selection.selected.unwrap() < last);
        panels.reset();
        selection.normalize(&panels, PanelKind::Output, 0);
        assert_eq!(selection, PanelSelection::default());
        assert_eq!(panels.memory_bytes(PanelKind::Output), 0);
        panels.push(PanelKind::Output, "new".into());
        assert!(panels.rows(PanelKind::Output)[0].id > last);
    }
    #[test]
    fn completed_items_have_typed_targets_and_proposals_never_apply() {
        let mut app = app();
        let stamp = capture_stamp(&app, None);
        let bytes = app.active_bytes().unwrap();
        let path = app.active_document().unwrap().session.path().to_path_buf();
        assert!(app.accept_panel_items(
            stamp,
            &path,
            TaskStatus::Completed,
            true,
            true,
            &[
                TaskItem::Diagnostic {
                    range: 3..5,
                    message: "accent".into()
                },
                TaskItem::Message("provider message".into()),
                TaskItem::ProposedEdit {
                    range: 3..5,
                    text: "replacement".into()
                },
                TaskItem::Diagnostic {
                    range: 6..7,
                    message: "CRLF interior must not navigate".into()
                },
            ]
        ));
        assert_eq!(app.task_panels.rows(PanelKind::Problems).len(), 1);
        let target = app.task_panels.rows(PanelKind::Problems)[0]
            .target
            .as_ref()
            .unwrap();
        assert_eq!(target.range, 3..5);
        assert_eq!(target.stamp, stamp);
        assert_eq!(target.path, path);
        let output = app.task_panels.rows(PanelKind::Output);
        assert!(output.iter().any(|row| row.text == "provider message"));
        assert!(output.iter().any(|row| row.text.contains("not applied")));
        assert!(output
            .iter()
            .all(|row| !row.text.contains("CRLF interior must not navigate")));
        let provider_row = output
            .iter()
            .find(|row| row.text == "provider message")
            .unwrap();
        let output_id = provider_row.id;
        for row in output.iter().filter(|row| row.target.is_some()) {
            let origin = row.target.as_ref().unwrap();
            assert_eq!(origin.stamp, stamp);
            assert_eq!(origin.path, path);
            assert_eq!(origin.range, 0..0);
        }
        assert!(provider_row.target.is_some());
        assert!(output
            .iter()
            .find(|row| row.text.contains("not applied"))
            .unwrap()
            .target
            .is_some());
        let panes = format!("{:?}", app.panes());
        assert!(app.activate_problem(output_id).is_err());
        assert_eq!(panes, format!("{:?}", app.panes()));
        assert_eq!(app.active_bytes().unwrap(), bytes);
        assert_eq!(
            app.active_document().unwrap().session.document().revision(),
            stamp.revision
        );
    }
    #[test]
    fn failure_stale_cancel_and_empty_completion_publish_only_explicit_status() {
        let mut app = app();
        let stamp = capture_stamp(&app, None);
        let path = app.active_document().unwrap().session.path().to_path_buf();
        for status in [
            TaskStatus::Cancelled,
            TaskStatus::TimedOut,
            TaskStatus::Failed,
            TaskStatus::Panicked,
            TaskStatus::LimitExceeded,
            TaskStatus::InvalidRange,
        ] {
            assert!(app.accept_panel_items(
                stamp,
                &path,
                status,
                false,
                true,
                &[TaskItem::Message("never retain".into())]
            ));
        }
        app.accept_panel_items(
            stamp,
            &path,
            TaskStatus::Completed,
            true,
            false,
            &[TaskItem::Message("never retain".into())],
        );
        app.accept_panel_items(
            stamp,
            Path::new("different-path.txt"),
            TaskStatus::Completed,
            true,
            true,
            &[TaskItem::Message("never retain".into())],
        );
        app.accept_panel_items(stamp, &path, TaskStatus::Completed, true, true, &[]);
        assert!(app.task_panels.rows(PanelKind::Problems).is_empty());
        assert_eq!(app.task_panels.rows(PanelKind::Output).len(), 9);
        assert!(app
            .task_panels
            .rows(PanelKind::Output)
            .iter()
            .all(|row| !row.text.contains("never retain")));
        assert!(app
            .task_panels
            .rows(PanelKind::Output)
            .last()
            .unwrap()
            .text
            .contains("no items"));
    }
    #[test]
    fn problem_activation_respects_pin_and_rejects_changed_revision_and_closed_view() {
        let mut app = app();
        app.split_editor_right();
        app.focus_editor_pane(PaneId::Secondary);
        app.go_to_position(TextPosition { line: 1, column: 0 })
            .unwrap();
        let view = app
            .panes()
            .view(PaneId::Secondary)
            .and_then(|view| view.view_id());
        let stamp = capture_stamp(&app, view);
        let path = app.active_document().unwrap().session.path().to_path_buf();
        app.accept_panel_items(
            stamp,
            &path,
            TaskStatus::Completed,
            true,
            true,
            &[TaskItem::Diagnostic {
                range: 3..5,
                message: "navigate".into(),
            }],
        );
        let id = app.task_panels.rows(PanelKind::Problems)[0].id;
        app.toggle_reference_pin();
        app.activate_problem(id).unwrap();
        assert_eq!(app.panes().active(), PaneId::Primary);
        assert_eq!(
            app.active_position(),
            Some(TextPosition { line: 0, column: 0 })
        );
        assert_eq!(
            app.position_for_pane(PaneId::Secondary),
            Some(TextPosition { line: 1, column: 0 })
        );
        app.close_split_editor();
        let before = format!("{:?}", app.panes());
        assert!(app.activate_problem(id).is_err());
        assert_eq!(before, format!("{:?}", app.panes()));
        let stamp = capture_stamp(&app, None);
        app.accept_panel_items(
            stamp,
            &path,
            TaskStatus::Completed,
            true,
            true,
            &[TaskItem::Diagnostic {
                range: 3..5,
                message: "later stale".into(),
            }],
        );
        let id = app.task_panels.rows(PanelKind::Problems).last().unwrap().id;
        app.insert_text("Q").unwrap();
        let before = format!("{:?}", app.panes());
        assert!(app.activate_problem(id).is_err());
        assert_eq!(before, format!("{:?}", app.panes()));
    }
}
