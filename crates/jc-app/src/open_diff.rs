//! Versioned immutable comparison of the two open groups. No disk or Git I/O.
use crate::{
    diff_model::{DiffLimits, DiffSide, DiffStatus, OwnedDiffModel},
    Cancellation, CompletedTask, JobId, JobKey, JobSystem, PaneId, SubmitError, TaskFailure,
    TaskOutput, TaskSnapshot, TaskStamp, TaskStatus, Workbench,
};
use jc_text::TextEncoding;
use std::path::PathBuf;

pub const OPEN_DIFF_JOB_KEY: JobKey = JobKey(0x4a43_4f50_4449_4601);
const SOURCE_LIMIT: usize = 2 * 1024 * 1024;
const PATH_LIMIT: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OpenDiffState {
    #[default]
    Idle,
    Queued,
    Running,
    Ready,
    Limited,
    Cancelled,
    Stale,
    Failed,
    TimedOut,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenDiffOrigin {
    pub stamp: TaskStamp,
    pub pane: PaneId,
    pub path: PathBuf,
    pub modified: bool,
}
impl OpenDiffOrigin {
    fn capture(app: &Workbench, pane: PaneId) -> Result<Self, &'static str> {
        let view = app
            .panes()
            .view(pane)
            .ok_or("Both editor groups must be open")?;
        let location = view
            .location()
            .ok_or("Both editor groups need a document")?;
        let document = app
            .document_by_tab(location.tab)
            .ok_or("Comparison document closed")?;
        if document.session.document().len() > SOURCE_LIMIT {
            return Err("Open comparison source exceeds 2 MiB per side");
        }
        if document.session.path().as_os_str().len() > PATH_LIMIT {
            return Err("Open comparison path exceeds its metadata limit");
        }
        let encoding = document
            .session
            .encoding()
            .ok_or("Choose both document encodings before comparing")?;
        let view = view.view_id().ok_or("Comparison view is missing")?;
        Ok(Self {
            stamp: TaskStamp {
                document: document.id,
                revision: document.session.document().revision(),
                encoding,
                view: Some(view),
            },
            pane,
            path: document.session.path().to_owned(),
            modified: document.session.document().is_modified(),
        })
    }
    fn current(&self, app: &Workbench) -> bool {
        app.task_stamp_is_current(self.stamp)
            && app
                .document_by_id(self.stamp.document)
                .is_some_and(|document| {
                    document.session.path() == self.path
                        && document.session.document().is_modified() == self.modified
                        && app.panes().view(self.pane).is_some_and(|view| {
                            view.view_id() == self.stamp.view
                                && view
                                    .location()
                                    .is_some_and(|location| location.tab == document.tab_id)
                        })
                })
    }
}
#[derive(Debug)]
pub struct OpenDiffResult {
    pub left: OpenDiffOrigin,
    pub right: OpenDiffOrigin,
    pub model: OwnedDiffModel,
}
#[derive(Debug)]
pub(crate) struct OpenDiffReply {
    pub generation: u64,
    pub result: OpenDiffResult,
}
impl OpenDiffReply {
    pub(crate) fn memory_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.result.left.path.capacity()
            + self.result.right.path.capacity()
            + self.result.model.memory_bytes()
    }
}
#[derive(Debug, Clone)]
pub(crate) struct OpenDiffWork {
    pub generation: u64,
    pub left: OpenDiffOrigin,
    pub right: OpenDiffOrigin,
}
impl OpenDiffWork {
    pub(crate) fn current(&self, app: &Workbench) -> bool {
        app.panes().is_split()
            && self.left.stamp.document != self.right.stamp.document
            && self.left.current(app)
            && self.right.current(app)
    }
}
#[derive(Debug)]
struct Flight {
    id: JobId,
    work: OpenDiffWork,
    discard: bool,
}
#[derive(Debug, Default)]
pub struct OpenDiffController {
    generation: u64,
    flight: Option<Flight>,
    pending: Option<OpenDiffWork>,
    accepted: Option<OpenDiffWork>,
    result: Option<OpenDiffResult>,
    selected: usize,
    state: OpenDiffState,
    message: String,
}
impl OpenDiffController {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn state(&self) -> OpenDiffState {
        self.state
    }
    pub fn message(&self) -> &str {
        &self.message
    }
    pub fn result(&self) -> Option<&OpenDiffResult> {
        self.result.as_ref()
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    fn status(&mut self, state: OpenDiffState, message: impl Into<String>) -> bool {
        let message = message.into();
        let changed = self.state != state || self.message != message;
        self.state = state;
        self.message = message;
        changed
    }
    fn discard_result(&mut self) {
        self.result = None;
        self.accepted = None;
        self.selected = 0;
    }
    fn invalidate(&mut self, jobs: &mut JobSystem) {
        self.generation = self.generation.saturating_add(1);
        if let Some(flight) = &mut self.flight {
            flight.discard = true;
            jobs.cancel(OPEN_DIFF_JOB_KEY);
        }
        self.pending = None;
        self.discard_result();
    }
    pub fn cancel(&mut self, jobs: &mut JobSystem) {
        self.invalidate(jobs);
        self.status(OpenDiffState::Cancelled, "Open comparison cancelled");
    }
    pub fn clear(&mut self, jobs: &mut JobSystem) {
        self.invalidate(jobs);
        self.status(OpenDiffState::Idle, "");
    }
    pub fn request(&mut self, app: &Workbench, jobs: &mut JobSystem) -> Result<(), String> {
        if self.generation == u64::MAX {
            self.invalidate(jobs);
            self.status(
                OpenDiffState::Failed,
                "Open comparison generation exhausted",
            );
            return Err(self.message.clone());
        }
        self.invalidate(jobs);
        let work = (|| {
            if !app.panes().is_split() {
                return Err("Split the editor before comparing open documents");
            }
            let left = OpenDiffOrigin::capture(app, PaneId::Primary)?;
            let right = OpenDiffOrigin::capture(app, PaneId::Secondary)?;
            if left.stamp.document == right.stamp.document {
                return Err("Choose different documents in the two groups");
            }
            Ok(OpenDiffWork {
                generation: self.generation,
                left,
                right,
            })
        })();
        match work {
            Ok(work) => {
                self.pending = Some(work);
                self.status(OpenDiffState::Queued, "Open comparison queued");
                self.tick(app, jobs);
            }
            Err(message) => {
                let state = if message == "Open comparison source exceeds 2 MiB per side"
                    || message == "Open comparison path exceeds its metadata limit"
                {
                    OpenDiffState::Limited
                } else {
                    OpenDiffState::Failed
                };
                self.status(state, message);
                return Err(self.message.clone());
            }
        }
        if matches!(
            self.state,
            OpenDiffState::Failed | OpenDiffState::Limited | OpenDiffState::Stale
        ) {
            Err(self.message.clone())
        } else {
            Ok(())
        }
    }
    /// Metadata only: safe before read-only presentation, never admits snapshots.
    pub fn validate_context(&mut self, app: &Workbench, jobs: &mut JobSystem) -> bool {
        let stale = self
            .accepted
            .as_ref()
            .is_some_and(|work| !work.current(app))
            || self.pending.as_ref().is_some_and(|work| !work.current(app))
            || self
                .flight
                .as_ref()
                .is_some_and(|flight| !flight.discard && !flight.work.current(app));
        if stale {
            self.invalidate(jobs);
            return self.status(
                OpenDiffState::Stale,
                "Comparison documents/groups changed; refresh",
            );
        }
        false
    }
    /// Event route only; admits both bounded sources together, never polls jobs.
    pub fn tick(&mut self, app: &Workbench, jobs: &mut JobSystem) -> bool {
        let mut changed = self.validate_context(app, jobs);
        if self.flight.is_some() {
            return changed;
        }
        let Some(work) = self.pending.as_ref() else {
            return changed;
        };
        match jobs.submit_open_diff(app, OPEN_DIFF_JOB_KEY, work.clone()) {
            Ok(id) => {
                let work = self.pending.take().unwrap();
                self.flight = Some(Flight {
                    id,
                    work,
                    discard: false,
                });
                changed |= self.status(
                    OpenDiffState::Running,
                    "Comparing immutable open-document snapshots",
                );
            }
            Err(SubmitError::Busy) => {
                changed |= self.status(OpenDiffState::Queued, "Waiting for background capacity")
            }
            Err(error) => {
                self.pending = None;
                changed |= self.status(
                    if error == SubmitError::SnapshotLimit {
                        OpenDiffState::Limited
                    } else {
                        OpenDiffState::Failed
                    },
                    format!("Open comparison admission: {error:?}"),
                );
            }
        }
        changed
    }
    /// The frontend's sole JobSystem poll routes completions here, even discarded
    /// generations. Frontend recovery must still observe TaskStatus::Panicked.
    pub fn accept(&mut self, app: &Workbench, task: &mut CompletedTask) -> bool {
        if task.key != OPEN_DIFF_JOB_KEY {
            return false;
        }
        let reply = task.output.take_open_diff();
        if self
            .flight
            .as_ref()
            .is_none_or(|flight| flight.id != task.id)
        {
            return true;
        }
        let flight = self.flight.take().unwrap();
        if flight.discard || flight.work.generation != self.generation {
            return true;
        }
        if !flight.work.current(app) || task.stamp != flight.work.left.stamp {
            self.discard_result();
            self.status(
                OpenDiffState::Stale,
                "Open comparison changed before delivery",
            );
            return true;
        }
        if task.status != TaskStatus::Completed {
            let (state, message) = match task.status {
                TaskStatus::Cancelled => (OpenDiffState::Cancelled, "Open comparison cancelled"),
                TaskStatus::TimedOut => (OpenDiffState::TimedOut, "Open comparison timed out"),
                TaskStatus::LimitExceeded => (
                    OpenDiffState::Limited,
                    "Open comparison exceeded its budgets",
                ),
                TaskStatus::InvalidRange => (
                    OpenDiffState::Stale,
                    "Open comparison snapshot became invalid",
                ),
                TaskStatus::Panicked => (OpenDiffState::Failed, "Open comparison worker panicked"),
                _ => (OpenDiffState::Failed, "Open comparison worker failed"),
            };
            self.status(state, message);
            return true;
        }
        if !task.current || !task.applicable {
            self.status(
                OpenDiffState::Stale,
                "Open comparison delivery is no longer current",
            );
            return true;
        }
        let Some(reply) = reply.filter(|reply| {
            reply.generation == self.generation
                && reply.result.left == flight.work.left
                && reply.result.right == flight.work.right
        }) else {
            self.status(
                OpenDiffState::Failed,
                "Open comparison returned an invalid delivery",
            );
            return true;
        };
        match reply.result.model.status() {
            DiffStatus::Exact => {
                self.accepted = Some(flight.work);
                self.result = Some(reply.result);
                self.selected = 0;
                self.status(OpenDiffState::Ready, "Open documents comparison ready");
            }
            DiffStatus::Limited(limit) => {
                self.status(
                    OpenDiffState::Limited,
                    format!("Open comparison limit: {limit:?}"),
                );
            }
            DiffStatus::Cancelled => {
                self.status(OpenDiffState::Cancelled, "Open comparison cancelled");
            }
        }
        true
    }
    pub fn select_relative(&mut self, delta: isize) {
        let count = self
            .result
            .as_ref()
            .map_or(0, |result| result.model.hunks().count());
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(count.saturating_sub(1));
    }
    pub fn select_hunk(
        &mut self,
        index: usize,
        side: DiffSide,
        app: &mut Workbench,
    ) -> Result<(), String> {
        if self.state != OpenDiffState::Ready {
            return Err("No current open comparison".into());
        }
        if self
            .accepted
            .as_ref()
            .is_none_or(|work| work.generation != self.generation || !work.current(app))
        {
            self.discard_result();
            self.status(
                OpenDiffState::Stale,
                "Comparison documents/groups changed; refresh",
            );
            return Err(self.message.clone());
        }
        let result = self.result.as_ref().ok_or("No open comparison result")?;
        let hunk = result
            .model
            .hunks()
            .nth(index)
            .ok_or("No such comparison hunk")?;
        let (origin, line) = match side {
            DiffSide::Left => (&result.left, hunk.left.start),
            DiffSide::Right => (&result.right, hunk.right.start),
        };
        let bytes = result.model.source_bytes(side);
        let mut offset = result
            .model
            .line_byte_range(side, line)
            .map_or(bytes.len(), |range| range.start);
        if offset == 0
            && origin.stamp.encoding == TextEncoding::Utf8
            && bytes.starts_with(&[0xef, 0xbb, 0xbf])
        {
            offset = 3;
        }
        if offset > 0
            && offset < bytes.len()
            && bytes[offset - 1] == b'\r'
            && bytes[offset] == b'\n'
        {
            return Err("Cannot navigate inside CRLF".into());
        }
        let tab = app
            .document_by_id(origin.stamp.document)
            .ok_or("Comparison document closed")?
            .tab_id;
        if !app.jump_to_pane_location(origin.pane, tab, offset) {
            return Err("Hunk is not a valid boundary in its original group".into());
        }
        self.selected = index;
        Ok(())
    }
}

pub(crate) fn run(
    work: OpenDiffWork,
    left: &TaskSnapshot,
    right: &TaskSnapshot,
    cancel: &Cancellation,
    output: &mut TaskOutput,
) -> Result<(), TaskFailure> {
    if left.stamp != work.left.stamp
        || right.stamp != work.right.stamp
        || left.range != (0..left.bytes().len())
        || right.range != (0..right.bytes().len())
    {
        return Err(TaskFailure::InvalidRange);
    }
    if left.bytes().len() > SOURCE_LIMIT || right.bytes().len() > SOURCE_LIMIT {
        return Err(TaskFailure::LimitExceeded);
    }
    if cancel.is_cancelled() {
        return Err(TaskFailure::Cancelled);
    }
    let model = OwnedDiffModel::compute(
        left.shared_bytes(),
        right.shared_bytes(),
        DiffLimits {
            max_input_bytes: 2 * SOURCE_LIMIT,
            max_metadata_bytes: 4 * 1024 * 1024,
            ..DiffLimits::default()
        },
        || cancel.is_cancelled(),
    );
    if cancel.is_cancelled() {
        return Err(TaskFailure::Cancelled);
    }
    output.set_open_diff(OpenDiffReply {
        generation: work.generation,
        result: OpenDiffResult {
            left: work.left,
            right: work.right,
            model,
        },
    })
}
