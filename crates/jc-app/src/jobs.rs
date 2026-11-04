//! Bounded internal providers. Workers own snapshots and never live buffers.
use crate::{DocumentId, ViewId, Workbench};
use jc_text::TextEncoding;
use std::{
    collections::{HashMap, VecDeque},
    ops::Range,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Clone, Default)]
pub struct Wakeup(Arc<WakeState>);
#[derive(Default)]
struct WakeState {
    ticket: Mutex<u64>,
    changed: Condvar,
    notifier: Option<Arc<dyn Fn() + Send + Sync>>,
    notifier_failed: AtomicBool,
}
impl Wakeup {
    /// Optional native-loop transport. It must be short and nonblocking, must
    /// never mutate a live editor, and runs after releasing the ticket mutex.
    /// A panic is latched instead of unwinding through a worker's delivery path.
    pub fn with_notifier(notifier: impl Fn() + Send + Sync + 'static) -> Self {
        Self(Arc::new(WakeState {
            notifier: Some(Arc::new(notifier)),
            ..WakeState::default()
        }))
    }
    pub fn take_notifier_failure(&self) -> bool {
        self.0.notifier_failed.swap(false, Ordering::AcqRel)
    }
    pub fn ticket(&self) -> u64 {
        *self.0.ticket.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn signal(&self) {
        let mut ticket = self.0.ticket.lock().unwrap_or_else(|e| e.into_inner());
        *ticket = ticket.wrapping_add(1);
        drop(ticket);
        self.0.changed.notify_all();
        if let Some(notifier) = &self.0.notifier {
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| notifier())).is_err() {
                self.0.notifier_failed.store(true, Ordering::Release);
            }
        }
    }
    pub fn wait(&self, ticket: u64, timeout: Option<Duration>) {
        let guard = self.0.ticket.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(timeout) = timeout {
            drop(
                self.0
                    .changed
                    .wait_timeout_while(guard, timeout, |current| *current == ticket)
                    .unwrap_or_else(|e| e.into_inner()),
            );
        } else {
            drop(
                self.0
                    .changed
                    .wait_while(guard, |current| *current == ticket)
                    .unwrap_or_else(|e| e.into_inner()),
            );
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct JobKey(pub u64);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct JobId(pub u64);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TaskStamp {
    pub document: DocumentId,
    pub revision: u64,
    pub encoding: TextEncoding,
    pub view: Option<ViewId>,
}
pub struct TaskSnapshot {
    pub stamp: TaskStamp,
    pub range: Range<usize>,
    bytes: Arc<Vec<u8>>,
}
impl TaskSnapshot {
    pub(crate) fn scan_source(stamp: TaskStamp, bytes: Vec<u8>) -> Self {
        Self {
            stamp,
            range: 0..bytes.len(),
            bytes: Arc::new(bytes),
        }
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub(crate) fn shared_bytes(&self) -> Arc<Vec<u8>> {
        self.bytes.clone()
    }
}

#[derive(Clone)]
pub struct Cancellation {
    flag: Arc<AtomicBool>,
    deadline: Instant,
}
impl Cancellation {
    pub(crate) fn new(timeout: Duration) -> Self {
        Self {
            flag: Arc::new(AtomicBool::new(false)),
            deadline: Instant::now() + timeout,
        }
    }
    pub(crate) fn cancel(&self) {
        self.flag.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::Acquire) || Instant::now() >= self.deadline
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskItem {
    Source(crate::source_services::SourceItem),
    Diagnostic {
        range: Range<usize>,
        message: String,
    },
    Message(String),
    ProposedEdit {
        range: Range<usize>,
        text: String,
    },
}
impl TaskItem {
    fn allocation(&self) -> usize {
        std::mem::size_of::<Self>()
            + match self {
                Self::Source(item) => item.allocation(),
                Self::Diagnostic { message, .. } | Self::Message(message) => message.capacity(),
                Self::ProposedEdit { text, .. } => text.capacity(),
            }
    }
    fn range(&self) -> Option<&Range<usize>> {
        match self {
            Self::Source(item) => Some(&item.range),
            Self::Diagnostic { range, .. } | Self::ProposedEdit { range, .. } => Some(range),
            Self::Message(_) => None,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskFailure {
    Cancelled,
    LimitExceeded,
    InvalidRange,
    Failed,
}
#[derive(Debug)]
pub struct TaskOutput {
    items: Vec<TaskItem>,
    bytes: usize,
    max_items: usize,
    max_bytes: usize,
    range: Range<usize>,
    failure: Option<TaskFailure>,
    stamp: TaskStamp,
    artifact_limit: usize,
    git_diff: Option<crate::git_diff::GitDiffReply>,
    open_diff: Option<crate::open_diff::OpenDiffReply>,
    second_stamp: Option<TaskStamp>,
}
impl TaskOutput {
    pub(crate) fn for_source_scan(snapshot: &TaskSnapshot) -> Self {
        Self::new(
            JobLimits::default(),
            snapshot.range.clone(),
            snapshot.stamp,
            0,
            None,
        )
    }
    fn new(
        limits: JobLimits,
        range: Range<usize>,
        stamp: TaskStamp,
        artifact_limit: usize,
        second_stamp: Option<TaskStamp>,
    ) -> Self {
        Self {
            items: Vec::new(),
            bytes: 0,
            max_items: limits.result_items,
            max_bytes: limits.result_bytes,
            range,
            failure: None,
            stamp,
            artifact_limit,
            git_diff: None,
            open_diff: None,
            second_stamp,
        }
    }
    pub fn items(&self) -> &[TaskItem] {
        &self.items
    }
    pub(crate) fn take_items(&mut self) -> Vec<TaskItem> {
        std::mem::take(&mut self.items)
    }
    pub fn set_git_diff(
        &mut self,
        reply: crate::git_diff::GitDiffReply,
    ) -> Result<(), TaskFailure> {
        let failure = if reply.stamp != self.stamp {
            Some(TaskFailure::InvalidRange)
        } else if self.git_diff.is_some()
            || self.open_diff.is_some()
            || self.items.len() >= self.max_items
            || reply.memory_bytes() > self.artifact_limit
        {
            Some(TaskFailure::LimitExceeded)
        } else {
            None
        };
        if let Some(failure) = failure {
            self.failure = Some(failure);
            return Err(failure);
        }
        self.git_diff = Some(reply);
        Ok(())
    }
    pub fn take_git_diff(&mut self) -> Option<crate::git_diff::GitDiffReply> {
        self.git_diff.take()
    }
    pub(crate) fn set_open_diff(
        &mut self,
        reply: crate::open_diff::OpenDiffReply,
    ) -> Result<(), TaskFailure> {
        let failure = if reply.result.left.stamp != self.stamp
            || Some(reply.result.right.stamp) != self.second_stamp
        {
            Some(TaskFailure::InvalidRange)
        } else if self.open_diff.is_some()
            || self.git_diff.is_some()
            || self.items.len() >= self.max_items
            || reply.memory_bytes() > self.artifact_limit
        {
            Some(TaskFailure::LimitExceeded)
        } else {
            None
        };
        if let Some(failure) = failure {
            self.failure = Some(failure);
            return Err(failure);
        }
        self.open_diff = Some(reply);
        Ok(())
    }
    pub(crate) fn take_open_diff(&mut self) -> Option<crate::open_diff::OpenDiffReply> {
        self.open_diff.take()
    }
    fn clear(&mut self) {
        self.items.clear();
        self.git_diff = None;
        self.open_diff = None;
        self.bytes = 0;
    }
    pub fn push(&mut self, item: TaskItem) -> Result<(), TaskFailure> {
        let error = if item.range().is_some_and(|r| {
            r.start > r.end || r.start < self.range.start || r.end > self.range.end
        }) {
            Some(TaskFailure::InvalidRange)
        } else if self.items.len()
            + usize::from(self.git_diff.is_some())
            + usize::from(self.open_diff.is_some())
            >= self.max_items
            || item.allocation() > self.max_bytes.saturating_sub(self.bytes)
        {
            Some(TaskFailure::LimitExceeded)
        } else {
            None
        };
        if let Some(error) = error {
            self.failure = Some(error);
            return Err(error);
        }
        if self.items.try_reserve_exact(1).is_err() {
            self.failure = Some(TaskFailure::LimitExceeded);
            return Err(TaskFailure::LimitExceeded);
        }
        self.bytes += item.allocation();
        self.items.push(item);
        Ok(())
    }
}
pub type TaskProvider =
    fn(&TaskSnapshot, &Cancellation, &mut TaskOutput) -> Result<(), TaskFailure>;

#[derive(Debug, Clone, Copy)]
pub struct JobLimits {
    pub workers: usize,
    pub outstanding: usize,
    pub snapshot_bytes: usize,
    pub total_snapshot_bytes: usize,
    pub result_items: usize,
    pub result_bytes: usize,
    pub timeout: Duration,
    /// Joint reservation for owned read models through pending/running/completed.
    pub artifact_bytes: usize,
}
impl Default for JobLimits {
    fn default() -> Self {
        Self {
            workers: 2,
            outstanding: 8,
            snapshot_bytes: 2 * 1024 * 1024,
            total_snapshot_bytes: 8 * 1024 * 1024,
            result_items: 64,
            result_bytes: 64 * 1024,
            timeout: Duration::from_secs(2),
            artifact_bytes: 12 * 1024 * 1024,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitError {
    InvalidLimits,
    Busy,
    SnapshotLimit,
    NoDocument,
    NoEncoding,
    InvalidView,
    InvalidRange,
    WorkerUnavailable,
    IdExhausted,
}
impl SubmitError {
    pub fn label(self) -> &'static str {
        match self {
            Self::InvalidLimits => "límites inválidos",
            Self::Busy => "capacidad ocupada",
            Self::SnapshotLimit => "cuota de capturas excedida",
            Self::NoDocument => "sin documento",
            Self::NoEncoding => "sin codificación",
            Self::InvalidView => "vista inválida",
            Self::InvalidRange => "rango inválido",
            Self::WorkerUnavailable => "tarea de fondo no disponible",
            Self::IdExhausted => "identificadores agotados",
        }
    }
}
impl std::fmt::Display for SubmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}
impl std::error::Error for SubmitError {}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Completed,
    Cancelled,
    TimedOut,
    Failed,
    Panicked,
    LimitExceeded,
    InvalidRange,
}
impl TaskStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Completed => "Completada",
            Self::Cancelled => "Cancelada",
            Self::TimedOut => "Tiempo agotado",
            Self::Failed => "Fallida",
            Self::Panicked => "Fallo de la tarea",
            Self::LimitExceeded => "Cuota excedida",
            Self::InvalidRange => "Rango inválido",
        }
    }
}
impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}
pub struct CompletedTask {
    pub id: JobId,
    pub key: JobKey,
    pub stamp: TaskStamp,
    /// Source path captured at admission, never inferred from delivery-time UI.
    pub path: PathBuf,
    pub status: TaskStatus,
    pub applicable: bool,
    pub current: bool,
    pub output: TaskOutput,
}
struct Job {
    id: JobId,
    snapshot: TaskSnapshot,
    second: Option<TaskSnapshot>,
    cancel: Cancellation,
    provider: Provider,
    artifact_limit: usize,
}
enum Provider {
    Plain(TaskProvider),
    Source(crate::source_services::SourceJob),
    GitDiff(crate::git_diff::GitDiffWork),
    OpenDiff(crate::open_diff::OpenDiffWork),
}
struct SecondCapture {
    stamp: TaskStamp,
    path: PathBuf,
    range: Range<usize>,
}
struct Capture {
    document: DocumentId,
    view: Option<ViewId>,
    range: Range<usize>,
    provider: Provider,
    artifact_limit: usize,
    timeout: Duration,
    expected: Option<TaskStamp>,
    second: Option<SecondCapture>,
}
struct Completion {
    id: JobId,
    status: TaskStatus,
    output: TaskOutput,
}
#[derive(Default)]
struct Queue {
    pending: VecDeque<Job>,
    completed: VecDeque<Completion>,
    stopping: bool,
}
struct Shared {
    queue: Mutex<Queue>,
    work: Condvar,
    wakeup: Wakeup,
    limits: JobLimits,
}
struct Record {
    key: JobKey,
    stamp: TaskStamp,
    path: PathBuf,
    cancel: Cancellation,
    bytes: usize,
    artifact_limit: usize,
    open_diff: Option<crate::open_diff::OpenDiffWork>,
}
pub struct JobSystem {
    shared: Arc<Shared>,
    handles: Vec<JoinHandle<()>>,
    records: HashMap<JobId, Record>,
    latest: HashMap<JobKey, JobId>,
    next: u64,
    bytes: usize,
    artifact_bytes: usize,
}

impl JobSystem {
    pub fn new(limits: JobLimits, wakeup: Wakeup) -> Result<Self, SubmitError> {
        if !(1..=8).contains(&limits.workers)
            || !(1..=64).contains(&limits.outstanding)
            || limits.snapshot_bytes == 0
            || limits.snapshot_bytes > 16 * 1024 * 1024
            || limits.total_snapshot_bytes < limits.snapshot_bytes
            || limits.total_snapshot_bytes > 64 * 1024 * 1024
            || !(1..=4096).contains(&limits.result_items)
            || limits.result_bytes == 0
            || limits.result_bytes > 1024 * 1024
            || limits.timeout.is_zero()
            || limits.timeout > Duration::from_secs(60)
            || limits.artifact_bytes > 64 * 1024 * 1024
        {
            return Err(SubmitError::InvalidLimits);
        }
        Ok(Self {
            shared: Arc::new(Shared {
                queue: Mutex::new(Queue::default()),
                work: Condvar::new(),
                wakeup,
                limits,
            }),
            handles: Vec::new(),
            records: HashMap::new(),
            latest: HashMap::new(),
            next: 1,
            bytes: 0,
            artifact_bytes: 0,
        })
    }
    pub fn outstanding(&self) -> usize {
        self.records.len()
    }
    pub fn snapshot_bytes(&self) -> usize {
        self.bytes
    }
    pub fn artifact_bytes(&self) -> usize {
        self.artifact_bytes
    }
    pub fn worker_count(&self) -> usize {
        self.handles.len()
    }
    pub fn has_completed(&self) -> bool {
        !self
            .shared
            .queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .completed
            .is_empty()
    }
    pub fn next_deadline(&self) -> Option<Duration> {
        let now = Instant::now();
        self.records
            .values()
            .filter_map(|r| r.cancel.deadline.checked_duration_since(now))
            .min()
    }
    pub fn submit(
        &mut self,
        app: &Workbench,
        key: JobKey,
        document: DocumentId,
        view: Option<ViewId>,
        range: Range<usize>,
        provider: TaskProvider,
    ) -> Result<JobId, SubmitError> {
        self.submit_internal(
            app,
            key,
            Capture {
                document,
                view,
                range,
                provider: Provider::Plain(provider),
                artifact_limit: 0,
                timeout: self.shared.limits.timeout,
                expected: None,
                second: None,
            },
        )
    }
    pub(crate) fn submit_git_diff(
        &mut self,
        app: &Workbench,
        key: JobKey,
        work: crate::git_diff::GitDiffWork,
    ) -> Result<JobId, SubmitError> {
        let doc = app
            .document_by_id(work.stamp.document)
            .ok_or(SubmitError::NoDocument)?;
        if doc.session.path() != work.document_path || !app.task_stamp_is_current(work.stamp) {
            return Err(SubmitError::InvalidView);
        }
        for path in [
            &work.document_path,
            &work.options.root,
            &work.options.cwd,
            &work.options.git_executable,
        ] {
            if path.as_os_str().len() > 4096 {
                return Err(SubmitError::SnapshotLimit);
            }
        }
        let len = doc.session.document().len();
        if len > 2 * 1024 * 1024 {
            return Err(SubmitError::SnapshotLimit);
        }
        let stamp = work.stamp;
        self.submit_internal(
            app,
            key,
            Capture {
                document: stamp.document,
                view: stamp.view,
                range: 0..len,
                expected: Some(stamp),
                provider: Provider::GitDiff(work),
                artifact_limit: 12 * 1024 * 1024,
                timeout: Duration::from_secs(10),
                second: None,
            },
        )
    }
    pub(crate) fn submit_open_diff(
        &mut self,
        app: &Workbench,
        key: JobKey,
        work: crate::open_diff::OpenDiffWork,
    ) -> Result<JobId, SubmitError> {
        if !work.current(app) {
            return Err(SubmitError::InvalidView);
        }
        let left = app
            .document_by_id(work.left.stamp.document)
            .ok_or(SubmitError::NoDocument)?;
        let right = app
            .document_by_id(work.right.stamp.document)
            .ok_or(SubmitError::NoDocument)?;
        let (left_len, right_len) = (
            left.session.document().len(),
            right.session.document().len(),
        );
        if left_len > 2 * 1024 * 1024 || right_len > 2 * 1024 * 1024 {
            return Err(SubmitError::SnapshotLimit);
        }
        let second = SecondCapture {
            stamp: work.right.stamp,
            path: work.right.path.clone(),
            range: 0..right_len,
        };
        self.submit_internal(
            app,
            key,
            Capture {
                document: work.left.stamp.document,
                view: work.left.stamp.view,
                range: 0..left_len,
                expected: Some(work.left.stamp),
                provider: Provider::OpenDiff(work),
                artifact_limit: 12 * 1024 * 1024,
                timeout: Duration::from_secs(10),
                second: Some(second),
            },
        )
    }
    fn submit_internal(
        &mut self,
        app: &Workbench,
        key: JobKey,
        capture: Capture,
    ) -> Result<JobId, SubmitError> {
        let Capture {
            document,
            view,
            range,
            provider,
            artifact_limit,
            timeout,
            expected,
            second,
        } = capture;
        let limits = self.shared.limits;
        if self.records.len() >= limits.outstanding {
            return Err(SubmitError::Busy);
        }
        if artifact_limit > limits.artifact_bytes {
            return Err(SubmitError::SnapshotLimit);
        }
        if artifact_limit > limits.artifact_bytes.saturating_sub(self.artifact_bytes) {
            return Err(SubmitError::Busy);
        }
        if range.start > range.end {
            return Err(SubmitError::InvalidRange);
        }
        if range.len() > limits.snapshot_bytes
            || second
                .as_ref()
                .is_some_and(|capture| capture.range.len() > limits.snapshot_bytes)
        {
            return Err(SubmitError::SnapshotLimit);
        }
        let total = range
            .len()
            .checked_add(second.as_ref().map_or(0, |capture| capture.range.len()))
            .ok_or(SubmitError::SnapshotLimit)?;
        if total > limits.total_snapshot_bytes {
            return Err(SubmitError::SnapshotLimit);
        }
        if total > limits.total_snapshot_bytes.saturating_sub(self.bytes) {
            // Git retains metadata while other F3 jobs own the budget; retry
            // after poll releases it, without capturing a second source.
            return Err(
                if matches!(&provider, Provider::GitDiff(_) | Provider::OpenDiff(_)) {
                    SubmitError::Busy
                } else {
                    SubmitError::SnapshotLimit
                },
            );
        }
        let doc = app
            .document_by_id(document)
            .ok_or(SubmitError::NoDocument)?;
        let stamp = TaskStamp {
            document,
            revision: doc.session.document().revision(),
            encoding: doc.session.encoding().ok_or(SubmitError::NoEncoding)?,
            view,
        };
        if doc.session.path().as_os_str().len() > 4096 {
            return Err(SubmitError::SnapshotLimit);
        }
        let path = doc.session.path().to_owned();
        if !app.task_stamp_is_current(stamp) {
            return Err(SubmitError::InvalidView);
        }
        if expected.is_some_and(|expected| expected != stamp) {
            return Err(SubmitError::InvalidView);
        }
        let second_doc = if let Some(capture) = &second {
            let doc = app
                .document_by_id(capture.stamp.document)
                .ok_or(SubmitError::NoDocument)?;
            if capture.range.start > capture.range.end
                || capture.range.end > doc.session.document().len()
            {
                return Err(SubmitError::InvalidRange);
            }
            if capture.path.as_os_str().len() > 4096 {
                return Err(SubmitError::SnapshotLimit);
            }
            if !app.task_stamp_is_current(capture.stamp) || doc.session.path() != capture.path {
                return Err(SubmitError::InvalidView);
            }
            Some(doc)
        } else {
            None
        };
        // Validate ALL source and artifact reservations before either copy.
        let open_diff = match &provider {
            Provider::OpenDiff(work) => Some(work.clone()),
            _ => None,
        };
        let source = Arc::new(
            doc.session
                .document()
                .read_range(range.clone())
                .map_err(|_| SubmitError::InvalidRange)?,
        );
        let second = match (second, second_doc) {
            (Some(capture), Some(doc)) => Some(TaskSnapshot {
                stamp: capture.stamp,
                range: capture.range.clone(),
                bytes: Arc::new(
                    doc.session
                        .document()
                        .read_range(capture.range)
                        .map_err(|_| SubmitError::InvalidRange)?,
                ),
            }),
            _ => None,
        };
        let charged = source
            .capacity()
            .checked_add(second.as_ref().map_or(0, |source| source.bytes.capacity()))
            .ok_or(SubmitError::SnapshotLimit)?;
        if charged > limits.total_snapshot_bytes {
            return Err(SubmitError::SnapshotLimit);
        }
        if charged > limits.total_snapshot_bytes.saturating_sub(self.bytes) {
            return Err(SubmitError::Busy);
        }
        if self.handles.is_empty() {
            for index in 0..limits.workers {
                let shared = self.shared.clone();
                if let Ok(handle) = thread::Builder::new()
                    .name(format!("jc-job-{index}"))
                    .spawn(move || worker(shared))
                {
                    self.handles.push(handle);
                }
            }
            if self.handles.is_empty() {
                return Err(SubmitError::WorkerUnavailable);
            }
        }
        let next = self.next.checked_add(1).ok_or(SubmitError::IdExhausted)?;
        let id = JobId(self.next);
        let cancel = Cancellation {
            flag: Arc::new(AtomicBool::new(false)),
            deadline: Instant::now() + timeout,
        };
        if let Some(previous) = self.latest.get(&key).and_then(|id| self.records.get(id)) {
            previous.cancel.flag.store(true, Ordering::Release);
        }
        self.records.insert(
            id,
            Record {
                key,
                stamp,
                path,
                cancel: cancel.clone(),
                bytes: charged,
                artifact_limit,
                open_diff,
            },
        );
        self.latest.insert(key, id);
        self.bytes += charged;
        self.artifact_bytes += artifact_limit;
        self.next = next;
        let mut queue = self.shared.queue.lock().unwrap_or_else(|e| e.into_inner());
        queue.pending.push_back(Job {
            id,
            snapshot: TaskSnapshot {
                stamp,
                range,
                bytes: source,
            },
            second,
            cancel,
            provider,
            artifact_limit,
        });
        self.shared.work.notify_one();
        Ok(id)
    }
    pub fn cancel(&mut self, key: JobKey) {
        if let Some(record) = self.latest.get(&key).and_then(|id| self.records.get(id)) {
            record.cancel.flag.store(true, Ordering::Release);
        }
    }
    /// Source capabilities retain metadata under temporary shared pressure and
    /// require the exact revision/view observed before admission.
    pub(crate) fn submit_source(
        &mut self,
        app: &Workbench,
        key: JobKey,
        stamp: TaskStamp,
        len: usize,
        work: crate::source_services::SourceJob,
    ) -> Result<JobId, SubmitError> {
        let limits = self.shared.limits;
        if len > limits.snapshot_bytes || len > limits.total_snapshot_bytes {
            return Err(SubmitError::SnapshotLimit);
        }
        if !app.task_stamp_is_current(stamp) {
            return Err(SubmitError::InvalidView);
        }
        if len > limits.total_snapshot_bytes.saturating_sub(self.bytes) {
            return Err(SubmitError::Busy);
        }
        self.submit_internal(
            app,
            key,
            Capture {
                document: stamp.document,
                view: stamp.view,
                range: 0..len,
                provider: Provider::Source(work),
                artifact_limit: 0,
                timeout: limits.timeout,
                expected: Some(stamp),
                second: None,
            },
        )
    }
    pub fn cancel_stale(&mut self, app: &Workbench) {
        for record in self.records.values() {
            if !app.task_stamp_is_current(record.stamp)
                || record
                    .open_diff
                    .as_ref()
                    .is_some_and(|work| !work.current(app))
                || app
                    .document_by_id(record.stamp.document)
                    .is_none_or(|doc| doc.session.path() != record.path)
            {
                record.cancel.flag.store(true, Ordering::Release);
            }
        }
    }
    /// Context invalidation cancels existing jobs without admitting or polling.
    pub fn cancel_all(&mut self) {
        for record in self.records.values() {
            record.cancel.flag.store(true, Ordering::Release);
        }
    }
    /// Completion count is capped; pending and completed work share admission quotas.
    pub fn poll(&mut self, app: &Workbench, max: usize) -> Vec<CompletedTask> {
        self.cancel_stale(app);
        let mut completed = Vec::new();
        for _ in 0..max.min(self.shared.limits.outstanding) {
            let event = self
                .shared
                .queue
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .completed
                .pop_front();
            let Some(mut event) = event else {
                break;
            };
            let Some(record) = self.records.remove(&event.id) else {
                continue;
            };
            self.bytes -= record.bytes;
            self.artifact_bytes -= record.artifact_limit;
            let latest = self.latest.get(&record.key) == Some(&event.id);
            if latest {
                self.latest.remove(&record.key);
            }
            // Preserve a panic even after cancellation/deadline: the default hook
            // may have written to the terminal and the frontend must recover it.
            if Instant::now() >= record.cancel.deadline && event.status != TaskStatus::Panicked {
                event.status = TaskStatus::TimedOut;
            }
            let current = latest
                && !record.cancel.flag.load(Ordering::Acquire)
                && record
                    .open_diff
                    .as_ref()
                    .is_none_or(|work| work.current(app))
                && app.task_stamp_is_current(record.stamp);
            let applicable = current
                && event.status == TaskStatus::Completed
                && !record.cancel.is_cancelled()
                && app.task_stamp_is_current(record.stamp);
            if !applicable {
                event.output.clear();
            }
            completed.push(CompletedTask {
                id: event.id,
                key: record.key,
                stamp: record.stamp,
                path: record.path,
                status: event.status,
                applicable,
                current,
                output: event.output,
            });
        }
        completed
    }
}
fn worker(shared: Arc<Shared>) {
    loop {
        let job = {
            let queue = shared.queue.lock().unwrap_or_else(|e| e.into_inner());
            let mut queue = shared
                .work
                .wait_while(queue, |q| q.pending.is_empty() && !q.stopping)
                .unwrap_or_else(|e| e.into_inner());
            if queue.stopping {
                return;
            }
            queue.pending.pop_front().unwrap()
        };
        let mut output = TaskOutput::new(
            shared.limits,
            job.snapshot.range.clone(),
            job.snapshot.stamp,
            job.artifact_limit,
            job.second.as_ref().map(|source| source.stamp),
        );
        let result = if job.cancel.is_cancelled() {
            Ok(Err(TaskFailure::Cancelled))
        } else {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match job.provider {
                Provider::Plain(provider) => provider(&job.snapshot, &job.cancel, &mut output),
                Provider::Source(work) => work.run(&job.snapshot, &job.cancel, &mut output),
                Provider::GitDiff(work) => {
                    crate::git_diff::run(work, &job.snapshot, &job.cancel, &mut output)
                }
                Provider::OpenDiff(work) => crate::open_diff::run(
                    work,
                    &job.snapshot,
                    job.second.as_ref().ok_or(TaskFailure::InvalidRange)?,
                    &job.cancel,
                    &mut output,
                ),
            }))
        };
        let status = if result.is_err() {
            TaskStatus::Panicked
        } else if Instant::now() >= job.cancel.deadline {
            TaskStatus::TimedOut
        } else if job.cancel.flag.load(Ordering::Acquire) {
            TaskStatus::Cancelled
        } else {
            match result {
                Err(_) => TaskStatus::Panicked,
                Ok(result) => match output.failure.or_else(|| result.err()) {
                    None => TaskStatus::Completed,
                    Some(TaskFailure::Cancelled) => TaskStatus::Cancelled,
                    Some(TaskFailure::LimitExceeded) => TaskStatus::LimitExceeded,
                    Some(TaskFailure::InvalidRange) => TaskStatus::InvalidRange,
                    Some(TaskFailure::Failed) => TaskStatus::Failed,
                },
            }
        };
        if status != TaskStatus::Completed {
            output.clear();
        }
        drop(job.snapshot); // source ownership ends before publication
        drop(job.second);
        let mut queue = shared.queue.lock().unwrap_or_else(|e| e.into_inner());
        if queue.stopping {
            return;
        }
        queue.completed.push_back(Completion {
            id: job.id,
            status,
            output,
        });
        drop(queue);
        shared.wakeup.signal();
    }
}
impl Drop for JobSystem {
    fn drop(&mut self) {
        for record in self.records.values() {
            record.cancel.flag.store(true, Ordering::Release);
        }
        let mut queue = self.shared.queue.lock().unwrap_or_else(|e| e.into_inner());
        queue.stopping = true;
        queue.pending.clear();
        drop(queue);
        self.shared.work.notify_all();
        for handle in self.handles.drain(..) {
            let _ = handle.join();
        }
    }
}

impl Workbench {
    pub fn task_stamp_is_current(&self, stamp: TaskStamp) -> bool {
        self.document_by_id(stamp.document).is_some_and(|doc| {
            doc.session.document().revision() == stamp.revision
                && doc.session.encoding() == Some(stamp.encoding)
                && stamp
                    .view
                    .is_none_or(|view| self.panes().tab_for_view(view) == Some(doc.tab_id))
        })
    }
}
