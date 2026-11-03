//! On-demand, literal project search. One bounded worker, no resident index.
mod parallel;
mod replace;
mod scan;
use crate::{Cancellation, TaskStamp, Wakeup, Workbench};
use jc_files::ContentFingerprint;
use jc_text::TextEncoding;
pub use replace::{ProjectReplacePreview, ReplaceFileOutcome};
use std::{
    collections::{BTreeMap, VecDeque},
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
    time::Duration,
};

#[derive(Debug, Clone)]
pub struct ProjectSearchOptions {
    pub query: String,
    pub regex: bool,
    /// Root-relative globs; a pattern without '/' matches the basename anywhere.
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub encoding: Option<TextEncoding>,
    pub max_results: usize,
    pub max_file_bytes: usize,
    pub max_scan_bytes: usize,
    pub max_entries: usize,
    /// Total project CPU workers, including its coordinator/scanner. The executor
    /// further caps this by available logical processors with headroom reserved.
    pub workers: usize,
}
impl ProjectSearchOptions {
    pub fn literal(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            regex: false,
            include: vec!["**".into()],
            exclude: vec![".git/**".into(), "target/**".into(), "work/**".into()],
            encoding: None,
            max_results: 1000,
            max_file_bytes: 2 * 1024 * 1024,
            max_scan_bytes: 64 * 1024 * 1024,
            max_entries: 100_000,
            workers: 2,
        }
    }
    fn validate(&self) -> Result<(), String> {
        if self.query.is_empty()
            || self.query.len() > 4096
            || self.query.contains('\0')
            || !(1..=10_000).contains(&self.max_results)
            || !(1..=2 * 1024 * 1024).contains(&self.max_file_bytes)
            || self.max_scan_bytes == 0
            || self.max_scan_bytes > 256 * 1024 * 1024
            || !(1..=100_000).contains(&self.max_entries)
            || !(1..=4).contains(&self.workers)
            || self.include.len() + self.exclude.len() > 16
            || self
                .include
                .iter()
                .chain(&self.exclude)
                .any(|s| s.len() > 128 || s.is_empty() || s.contains('\\'))
        {
            return Err(
                "Invalid project query or search budget; use forward slashes in globs".into(),
            );
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchOrigin {
    Open(TaskStamp),
    Disk(ContentFingerprint),
}
#[derive(Debug, Clone)]
pub struct ProjectSearchMatch {
    pub path: PathBuf,
    pub range: std::ops::Range<usize>,
    /// Zero-based scalar coordinates, never display columns or LSP UTF-16.
    pub position: jc_text::TextPosition,
    pub snippet: String,
    pub encoding: TextEncoding,
    pub origin: SearchOrigin,
    pub source_len: usize,
}
#[derive(Debug, Clone)]
pub enum ProjectSearchRow {
    Match(ProjectSearchMatch),
    Issue { path: PathBuf, message: String },
}
impl ProjectSearchRow {
    fn charge(&self) -> usize {
        std::mem::size_of::<Self>()
            + match self {
                Self::Match(m) => m.path.as_os_str().len() * 2 + m.snippet.capacity(),
                Self::Issue { path, message } => path.as_os_str().len() * 2 + message.capacity(),
            }
    }
    fn current(
        &self,
        app: &Workbench,
        validity: &mut std::collections::HashMap<TaskStamp, bool>,
    ) -> bool {
        match self {
            Self::Match(ProjectSearchMatch {
                origin: SearchOrigin::Open(stamp),
                ..
            }) => *validity
                .entry(*stamp)
                .or_insert_with(|| app.task_stamp_is_current(*stamp)),
            _ => true,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProjectSearchState {
    #[default]
    Idle,
    Running,
    Complete,
    Limited,
    Cancelled,
    Failed,
}
pub(super) struct OpenSource {
    bytes: Vec<u8>,
    stamp: TaskStamp,
}
struct Request {
    generation: u64,
    root: PathBuf,
    options: ProjectSearchOptions,
    pattern: Arc<crate::text_search::Pattern>,
    open: BTreeMap<PathBuf, OpenSource>,
    cancel: Cancellation,
}
enum Event {
    Rows(u64, Vec<ProjectSearchRow>),
    Done(u64, ProjectSearchState),
}
#[derive(Default)]
struct Queue {
    pending: Option<Request>,
    events: VecDeque<Event>,
    stop: bool,
}
struct Shared {
    queue: Mutex<Queue>,
    changed: Condvar,
    wake: Wakeup,
    recover: std::sync::atomic::AtomicBool,
}

pub struct ProjectSearch {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
    cancel: Option<Cancellation>,
    generation: u64,
    rows: Vec<ProjectSearchRow>,
    pub state: ProjectSearchState,
    root: Option<PathBuf>,
    stale: bool,
    query: String,
}
impl ProjectSearch {
    pub fn new(wake: Wakeup) -> Self {
        Self {
            shared: Arc::new(Shared {
                queue: Mutex::new(Queue::default()),
                changed: Condvar::new(),
                wake,
                recover: std::sync::atomic::AtomicBool::new(false),
            }),
            worker: None,
            cancel: None,
            generation: 0,
            rows: Vec::new(),
            state: ProjectSearchState::Idle,
            root: None,
            stale: false,
            query: String::new(),
        }
    }
    pub fn rows(&self) -> &[ProjectSearchRow] {
        &self.rows
    }
    pub fn is_stale(&self) -> bool {
        self.stale
    }
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }
    pub fn ready(&self) -> bool {
        self.shared
            .recover
            .load(std::sync::atomic::Ordering::Acquire)
            || !self
                .shared
                .queue
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .events
                .is_empty()
    }
    pub fn take_recovery(&self) -> bool {
        self.shared
            .recover
            .swap(false, std::sync::atomic::Ordering::AcqRel)
    }
    pub fn cancel(&mut self) {
        if let Some(token) = &self.cancel {
            token.cancel();
        }
        let mut queue = self.shared.queue.lock().unwrap_or_else(|e| e.into_inner());
        queue.pending = None;
        queue.events.clear();
        self.shared.changed.notify_all();
        if self.state == ProjectSearchState::Running {
            self.state = ProjectSearchState::Cancelled;
        }
    }
    pub fn clear(&mut self) {
        self.cancel();
        self.rows.clear();
        self.root = None;
        self.state = ProjectSearchState::Idle;
        self.stale = false;
        self.query.clear();
    }
    pub fn start(
        &mut self,
        app: &Workbench,
        root: &Path,
        options: ProjectSearchOptions,
    ) -> Result<(), String> {
        options.validate()?;
        let pattern =
            crate::text_search::Pattern::compile_mode(&options.query, true, false, options.regex)?;
        let root = root.canonicalize().map_err(|e| e.to_string())?;
        if !root.is_dir() {
            return Err("Search root must be a directory".into());
        }
        let generation = self
            .generation
            .checked_add(1)
            .ok_or("Search generation exhausted")?;
        self.cancel(); // Drop a superseded pending snapshot before capturing another.
        let mut open = BTreeMap::new();
        let mut bytes = 0;
        for doc in app.open_documents() {
            let path = scan::absolute_source_path(doc.session.path()).map_err(|e| e.to_string())?;
            if !path.starts_with(&root) {
                continue;
            }
            if !scan::path_included(&options, &root, &path) {
                continue;
            }
            if path.as_os_str().len() > 4096 {
                return Err("Open-buffer path exceeds the project path budget".into());
            }
            let len = doc.session.document().len();
            if len > options.max_file_bytes || bytes + len > 8 * 1024 * 1024 || open.len() >= 128 {
                return Err(
                    "Open-buffer snapshot budget exceeded; narrow the workspace or close files"
                        .into(),
                );
            }
            let encoding = doc
                .session
                .encoding()
                .ok_or("Select encoding for open workspace documents before project search")?;
            bytes += len;
            open.insert(
                path,
                OpenSource {
                    bytes: doc
                        .session
                        .document()
                        .read_range(0..len)
                        .map_err(|e| e.to_string())?,
                    stamp: TaskStamp {
                        document: doc.id,
                        revision: doc.session.document().revision(),
                        encoding,
                        view: None,
                    },
                },
            );
        }
        if self.worker.is_none() {
            let shared = self.shared.clone();
            self.worker = Some(
                thread::Builder::new()
                    .name("jc-project-search".into())
                    .spawn(move || worker(shared))
                    .map_err(|e| e.to_string())?,
            );
        }
        self.cancel();
        self.rows.clear();
        self.generation = generation;
        self.root = Some(root.clone());
        self.state = ProjectSearchState::Running;
        self.stale = false;
        self.query = options.query.clone();
        let cancel = Cancellation::new(Duration::from_secs(10));
        self.cancel = Some(cancel.clone());
        let mut queue = self.shared.queue.lock().unwrap_or_else(|e| e.into_inner());
        queue.pending = Some(Request {
            generation,
            root,
            options,
            pattern,
            open,
            cancel,
        });
        drop(queue);
        self.shared.changed.notify_all();
        Ok(())
    }
    /// No filesystem reads here. Open-source revision/encoding/lifetime is checked
    /// both on arrival and again for previously displayed rows after user edits.
    /// Invalidate old open-document rows without consuming worker completions.
    /// Frontends may call before preparation; acceptance remains in poll.
    pub fn validate_context(&mut self, app: &Workbench) -> bool {
        let before = self.rows.len();
        let mut validity = std::collections::HashMap::new();
        self.rows.retain(|r| r.current(app, &mut validity));
        let changed = before != self.rows.len();
        self.stale |= changed;
        changed
    }
    pub fn poll(&mut self, app: &Workbench, max_batches: usize) -> bool {
        let mut changed = self.validate_context(app);
        let mut validity = std::collections::HashMap::new();
        for _ in 0..max_batches.min(4) {
            let event = self
                .shared
                .queue
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .events
                .pop_front();
            let Some(event) = event else {
                break;
            };
            self.shared.changed.notify_all();
            match event {
                Event::Rows(g, rows)
                    if g == self.generation && self.state == ProjectSearchState::Running =>
                {
                    for row in rows {
                        if row.current(app, &mut validity) {
                            self.rows.push(row);
                        } else {
                            self.stale = true;
                        }
                    }
                    changed = true;
                }
                Event::Done(g, state)
                    if g == self.generation && self.state == ProjectSearchState::Running =>
                {
                    self.state = state;
                    changed = true;
                }
                _ => {}
            }
        }
        changed
    }
    /// Resolve a result explicitly. Disk is re-read with root/size/identity checks;
    /// it is never trusted after an external edit, nor substituted for dirty text.
    pub fn select(&self, index: usize, app: &mut Workbench) -> Result<(), String> {
        let Some(ProjectSearchRow::Match(m)) = self.rows.get(index) else {
            return Err("This row is a search notice".into());
        };
        match m.origin {
            SearchOrigin::Open(stamp) => {
                if !app.task_stamp_is_current(stamp) {
                    return Err("Search result is stale; run the query again".into());
                }
                let tab = app
                    .document_by_id(stamp.document)
                    .ok_or("Document closed")?
                    .tab_id;
                if !app.jump_to_location(tab, m.range.start) {
                    return Err("Invalid search destination".into());
                }
            }
            SearchOrigin::Disk(expected) => {
                let root = self.root.as_deref().ok_or("Project changed")?;
                let tab = app
                    .open_project_file_at_version(
                        root,
                        m.path.clone(),
                        Some(m.encoding),
                        false,
                        2 * 1024 * 1024,
                        expected,
                    )
                    .map_err(|e| e.to_string())?;
                if !app.jump_to_location(tab, m.range.start) {
                    return Err("Invalid search destination".into());
                }
            }
        }
        Ok(())
    }
}
impl Drop for ProjectSearch {
    fn drop(&mut self) {
        self.cancel();
        let mut queue = self.shared.queue.lock().unwrap_or_else(|e| e.into_inner());
        queue.stop = true;
        drop(queue);
        self.shared.changed.notify_all();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn publish(shared: &Shared, request: &Request, event: Event) -> bool {
    let mut queue = shared.queue.lock().unwrap_or_else(|e| e.into_inner());
    while queue.events.len() >= 4 && !queue.stop && !request.cancel.is_cancelled() {
        queue = shared
            .changed
            .wait_timeout(queue, Duration::from_millis(20))
            .unwrap_or_else(|e| e.into_inner())
            .0;
    }
    if queue.stop || request.cancel.is_cancelled() {
        return false;
    }
    queue.events.push_back(event);
    drop(queue);
    shared.wake.signal();
    true
}
fn worker(shared: Arc<Shared>) {
    let mut executor = parallel::Executor::new();
    loop {
        let request = {
            let queue = shared.queue.lock().unwrap_or_else(|e| e.into_inner());
            let mut queue = shared
                .changed
                .wait_while(queue, |q| q.pending.is_none() && !q.stop)
                .unwrap_or_else(|e| e.into_inner());
            if queue.stop {
                return;
            }
            Arc::new(queue.pending.take().unwrap())
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            executor.run(request.clone(), |rows| {
                publish(&shared, &request, Event::Rows(request.generation, rows))
            })
        }));
        // Deadline still needs a terminal event to wake/finish a waiting UI.
        let state = match result {
            Ok(state) => state,
            Err(_) => {
                shared
                    .recover
                    .store(true, std::sync::atomic::Ordering::Release);
                shared.wake.signal();
                ProjectSearchState::Failed
            }
        };
        let mut queue = shared.queue.lock().unwrap_or_else(|e| e.into_inner());
        while queue.events.len() >= 4 && !queue.stop && !request.cancel.is_cancelled() {
            queue = shared
                .changed
                .wait_timeout(queue, Duration::from_millis(20))
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        if !queue.stop
            && queue
                .pending
                .as_ref()
                .is_none_or(|r| r.generation == request.generation)
        {
            // A cancelled generation may leave one terminal event. Consumers
            // reject it by generation/state, and the queue stays bounded at 5.
            queue
                .events
                .push_back(Event::Done(request.generation, state));
            drop(queue);
            shared.wake.signal();
        }
    }
}
