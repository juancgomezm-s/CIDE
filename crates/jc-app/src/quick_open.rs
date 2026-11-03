//! On-demand path metadata search. No content reads, watcher or permanent index.
//! Paths are authority; escaped labels are display-only, never parsed back.
//!
//! Traversal order is the filesystem iterator order, not a lexical ranking.
//! Limits cover retained result capacities; transient walk state also has at
//! most max_depth ReadDir handles, bounded path metadata and a <=3*query KMP
//! pattern. Allocator reserve transients and OS directory buffers are not RAM
//! guarantees. Deadline/cancellation are checked between synchronous metadata
//! calls: a blocked filesystem call can delay cancellation and Drop's join.
use crate::{Cancellation, Wakeup};
use jc_text::full_case_fold;
use std::{
    collections::VecDeque,
    fmt, fs,
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const LABEL_COLUMNS: usize = 256;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuickOpenLimits {
    pub query_bytes: usize,
    pub path_units: usize,
    pub max_depth: usize,
    pub max_entries: usize,
    pub max_results: usize,
    pub max_owned_bytes: usize,
    pub timeout: Duration,
}
impl Default for QuickOpenLimits {
    fn default() -> Self {
        Self {
            query_bytes: 256,
            path_units: 4096,
            max_depth: 32,
            max_entries: 50_000,
            max_results: 512,
            max_owned_bytes: 512 * 1024,
            timeout: Duration::from_secs(2),
        }
    }
}
impl QuickOpenLimits {
    pub fn valid(self) -> bool {
        (1..=256).contains(&self.query_bytes)
            && (1..=4096).contains(&self.path_units)
            && (1..=32).contains(&self.max_depth)
            && (1..=50_000).contains(&self.max_entries)
            && (1..=512).contains(&self.max_results)
            && (std::mem::size_of::<Outcome>()..=512 * 1024).contains(&self.max_owned_bytes)
            && !self.timeout.is_zero()
            && self.timeout <= Duration::from_secs(10)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QuickOpenState {
    #[default]
    Idle,
    Queued,
    Running,
    Complete,
    Limited,
    Cancelled,
    Stale,
    Failed,
    TimedOut,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickOpenLimit {
    Entries,
    Depth,
    Results,
    OwnedBytes,
    NonUnicodePaths,
    UnreadablePaths,
    PathUnits,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickOpenRequestError {
    InvalidRoot,
    InvalidQuery,
    InvalidLimits,
    WorkerUnavailable,
    GenerationExhausted,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickOpenFailure {
    InvalidRoot,
    Io,
    WorkerPanic,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickOpenAcceptError {
    Stale,
    NotReady,
    NoCandidate,
    InvalidRoot,
    RootChanged,
    Link,
    OutsideRoot,
    NotFile,
    TargetChanged,
    Io,
}
macro_rules! error_display {
    ($ty:ty, $prefix:literal) => {
        impl fmt::Display for $ty {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!($prefix, ": {:?}"), self)
            }
        }
        impl std::error::Error for $ty {}
    };
}
error_display!(QuickOpenRequestError, "Quick Open request");
error_display!(QuickOpenFailure, "Quick Open");
error_display!(QuickOpenAcceptError, "Quick Open selection");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct QuickOpenCounters {
    pub entries: usize,
    pub directories: usize,
    pub matches: usize,
    pub skipped_links: usize,
    pub skipped_non_unicode: usize,
    pub skipped_unreadable: usize,
    pub skipped_paths: usize,
    pub excluded_directories: usize,
    /// Retained result capacities, not private-process RAM.
    pub owned_bytes: usize,
}
pub struct QuickOpenCandidate {
    ordinal: usize,
    path: PathBuf,
    canonical: PathBuf,
    label: String,
}
impl QuickOpenCandidate {
    pub fn ordinal(&self) -> usize {
        self.ordinal
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn label(&self) -> &str {
        &self.label
    }
    fn heap_bytes(&self) -> usize {
        self.path.capacity() + self.canonical.capacity() + self.label.capacity()
    }
}
impl fmt::Debug for QuickOpenCandidate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QuickOpenCandidate")
            .field("ordinal", &self.ordinal)
            .field("path_units", &self.path.as_os_str().len())
            .finish()
    }
}
#[derive(Clone, PartialEq, Eq)]
struct Context {
    root: PathBuf,
    query: String,
}
struct Request {
    generation: u64,
    context: Context,
    limits: QuickOpenLimits,
    cancel: Cancellation,
    deadline: Instant,
}
struct Outcome {
    state: QuickOpenState,
    root: PathBuf,
    candidates: Vec<QuickOpenCandidate>,
    counters: QuickOpenCounters,
    limitation: Option<QuickOpenLimit>,
    failure: Option<QuickOpenFailure>,
}
impl Outcome {
    fn empty(state: QuickOpenState) -> Self {
        Self {
            state,
            root: PathBuf::new(),
            candidates: Vec::new(),
            counters: QuickOpenCounters::default(),
            limitation: None,
            failure: None,
        }
    }
    fn limit(&mut self, reason: QuickOpenLimit) {
        self.state = QuickOpenState::Limited;
        self.limitation = Some(reason);
    }
    fn memory_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.root.capacity()
            + self.candidates.capacity() * std::mem::size_of::<QuickOpenCandidate>()
            + self
                .candidates
                .iter()
                .map(QuickOpenCandidate::heap_bytes)
                .sum::<usize>()
    }
}
struct Completion {
    generation: u64,
    context: Context,
    outcome: Outcome,
}
struct Active {
    generation: u64,
    cancel: Cancellation,
}
#[derive(Default)]
struct Queue {
    generation: Option<u64>,
    pending: Option<Request>,
    active: Option<Active>,
    completion: Option<Completion>,
    shutdown: bool,
}
struct Shared {
    queue: Mutex<Queue>,
    changed: Condvar,
    wake: Wakeup,
    recovery: AtomicBool,
}
type Provider = dyn Fn(&Request) -> Outcome + Send + Sync;

pub struct QuickOpenController {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
    provider: Arc<Provider>,
    limits: QuickOpenLimits,
    generation: u64,
    context: Option<Context>,
    outcome: Option<Outcome>,
    state: QuickOpenState,
}
impl QuickOpenController {
    pub fn new(wake: Wakeup) -> Self {
        Self::with_limits(wake, QuickOpenLimits::default()).expect("valid Quick Open defaults")
    }
    pub fn with_limits(
        wake: Wakeup,
        limits: QuickOpenLimits,
    ) -> Result<Self, QuickOpenRequestError> {
        if !limits.valid() {
            return Err(QuickOpenRequestError::InvalidLimits);
        }
        Ok(Self::with_provider(wake, limits, Arc::new(scan)))
    }
    fn with_provider(wake: Wakeup, limits: QuickOpenLimits, provider: Arc<Provider>) -> Self {
        Self {
            shared: Arc::new(Shared {
                queue: Mutex::new(Queue::default()),
                changed: Condvar::new(),
                wake,
                recovery: AtomicBool::new(false),
            }),
            worker: None,
            provider,
            limits,
            generation: 0,
            context: None,
            outcome: None,
            state: QuickOpenState::Idle,
        }
    }
    fn invalidate(&mut self) {
        let mut queue = self.shared.queue.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(active) = &queue.active {
            active.cancel.cancel();
        }
        if let Some(pending) = &queue.pending {
            pending.cancel.cancel();
        }
        queue.generation = None;
        queue.pending = None;
        queue.completion = None;
        drop(queue);
        self.outcome = None;
        self.shared.changed.notify_all();
    }
    /// Metadata only; all filesystem work happens in the single lazy worker.
    pub fn request(
        &mut self,
        mut root: PathBuf,
        mut query: String,
    ) -> Result<(), QuickOpenRequestError> {
        self.invalidate();
        self.context = None;
        if !root.is_absolute() || root.as_os_str().len() > self.limits.path_units {
            self.state = QuickOpenState::Failed;
            return Err(QuickOpenRequestError::InvalidRoot);
        }
        if query.len() > self.limits.query_bytes || query.chars().any(char::is_control) {
            self.state = QuickOpenState::Failed;
            return Err(QuickOpenRequestError::InvalidQuery);
        }
        let Some(generation) = self.generation.checked_add(1) else {
            self.state = QuickOpenState::Failed;
            return Err(QuickOpenRequestError::GenerationExhausted);
        };
        root.shrink_to_fit();
        query.shrink_to_fit();
        let context = Context { root, query };
        if self.worker.is_none() {
            let shared = self.shared.clone();
            let provider = self.provider.clone();
            match thread::Builder::new()
                .name("jc-quick-open".into())
                .spawn(move || worker(shared, provider))
            {
                Ok(worker) => self.worker = Some(worker),
                Err(_) => {
                    self.state = QuickOpenState::Failed;
                    return Err(QuickOpenRequestError::WorkerUnavailable);
                }
            }
        }
        self.generation = generation;
        self.context = Some(context.clone());
        self.state = QuickOpenState::Queued;
        let mut queue = self.shared.queue.lock().unwrap_or_else(|e| e.into_inner());
        queue.generation = Some(generation);
        let deadline = Instant::now() + self.limits.timeout;
        queue.pending = Some(Request {
            generation,
            context,
            limits: self.limits,
            cancel: Cancellation::new(self.limits.timeout),
            deadline,
        });
        drop(queue);
        self.shared.changed.notify_all();
        Ok(())
    }
    pub fn cancel(&mut self) {
        self.invalidate();
        self.state = QuickOpenState::Cancelled;
    }
    pub fn clear(&mut self) {
        self.invalidate();
        self.context = None;
        self.state = QuickOpenState::Idle;
    }
    pub fn ready(&self) -> bool {
        self.shared
            .queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .completion
            .is_some()
    }
    pub fn take_recovery(&self) -> bool {
        self.shared.recovery.swap(false, Ordering::AcqRel)
    }
    pub fn state(&self) -> QuickOpenState {
        self.state
    }
    pub fn results(&self) -> &[QuickOpenCandidate] {
        self.outcome
            .as_ref()
            .map_or(&[], |outcome| outcome.candidates.as_slice())
    }
    pub fn counters(&self) -> QuickOpenCounters {
        self.outcome
            .as_ref()
            .map_or_else(QuickOpenCounters::default, |outcome| outcome.counters)
    }
    pub fn limitation(&self) -> Option<QuickOpenLimit> {
        self.outcome.as_ref().and_then(|outcome| outcome.limitation)
    }
    pub fn failure(&self) -> Option<QuickOpenFailure> {
        self.outcome.as_ref().and_then(|outcome| outcome.failure)
    }
    pub fn message(&self) -> &'static str {
        match self.state {
            QuickOpenState::Idle => "Type a path substring",
            QuickOpenState::Queued => "Quick Open queued",
            QuickOpenState::Running => "Finding paths",
            QuickOpenState::Complete => "Paths complete; substring search",
            QuickOpenState::Limited => "Partial path list; refine query",
            QuickOpenState::Cancelled => "Quick Open cancelled",
            QuickOpenState::Stale => "Root or query changed; refresh",
            QuickOpenState::Failed => "Quick Open failed",
            QuickOpenState::TimedOut => "Quick Open timed out",
        }
    }
    /// Consume only a completion for the exact current root/query/generation.
    pub fn poll(&mut self, root: &Path, query: &str) -> bool {
        if self
            .context
            .as_ref()
            .is_some_and(|context| context.root != root || context.query != query)
        {
            self.invalidate();
            self.context = None;
            self.state = QuickOpenState::Stale;
            return true;
        }
        let (completion, running) = {
            let mut queue = self.shared.queue.lock().unwrap_or_else(|e| e.into_inner());
            let running = queue
                .active
                .as_ref()
                .is_some_and(|active| active.generation == self.generation);
            (queue.completion.take(), running)
        };
        self.shared.changed.notify_all();
        if let Some(completion) = completion {
            if completion.generation != self.generation
                || self.context.as_ref() != Some(&completion.context)
                || completion.context.root != root
                || completion.context.query != query
            {
                return false;
            }
            self.state = completion.outcome.state;
            self.outcome = Some(completion.outcome);
            return true;
        }
        if running && self.state == QuickOpenState::Queued {
            self.state = QuickOpenState::Running;
            return true;
        }
        false
    }
    /// Explicit acceptance may perform metadata I/O. It is a preflight, not
    /// race-free opened-handle confinement: the caller must use open_in_root.
    pub fn selected_target(
        &self,
        root: &Path,
        query: &str,
        index: usize,
    ) -> Result<PathBuf, QuickOpenAcceptError> {
        let context = self.context.as_ref().ok_or(QuickOpenAcceptError::Stale)?;
        if context.root != root || context.query != query {
            return Err(QuickOpenAcceptError::Stale);
        }
        if !matches!(
            self.state,
            QuickOpenState::Complete | QuickOpenState::Limited
        ) {
            return Err(QuickOpenAcceptError::NotReady);
        }
        let outcome = self
            .outcome
            .as_ref()
            .ok_or(QuickOpenAcceptError::NotReady)?;
        let candidate = outcome
            .candidates
            .get(index)
            .ok_or(QuickOpenAcceptError::NoCandidate)?;
        let root_meta =
            fs::symlink_metadata(root).map_err(|_| QuickOpenAcceptError::InvalidRoot)?;
        if !root_meta.is_dir() || reparse(&root_meta) {
            return Err(QuickOpenAcceptError::InvalidRoot);
        }
        let current_root = root
            .canonicalize()
            .map_err(|_| QuickOpenAcceptError::InvalidRoot)?;
        if current_root != outcome.root {
            return Err(QuickOpenAcceptError::RootChanged);
        }
        let relative = candidate
            .path
            .strip_prefix(&outcome.root)
            .map_err(|_| QuickOpenAcceptError::OutsideRoot)?;
        let mut current = current_root.clone();
        for part in relative.components() {
            let Component::Normal(part) = part else {
                return Err(QuickOpenAcceptError::OutsideRoot);
            };
            current.push(part);
            let metadata = fs::symlink_metadata(&current).map_err(|_| QuickOpenAcceptError::Io)?;
            if reparse(&metadata) {
                return Err(QuickOpenAcceptError::Link);
            }
        }
        let metadata = fs::symlink_metadata(&current).map_err(|_| QuickOpenAcceptError::Io)?;
        if !metadata.is_file() {
            return Err(QuickOpenAcceptError::NotFile);
        }
        let canonical = current
            .canonicalize()
            .map_err(|_| QuickOpenAcceptError::Io)?;
        if !canonical.starts_with(&current_root) {
            return Err(QuickOpenAcceptError::OutsideRoot);
        }
        if canonical != candidate.canonical {
            return Err(QuickOpenAcceptError::TargetChanged);
        }
        Ok(canonical)
    }
}
impl Drop for QuickOpenController {
    fn drop(&mut self) {
        self.invalidate();
        self.shared
            .queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .shutdown = true;
        self.shared.changed.notify_all();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn worker(shared: Arc<Shared>, provider: Arc<Provider>) {
    loop {
        let request = {
            let queue = shared.queue.lock().unwrap_or_else(|e| e.into_inner());
            let mut queue = shared
                .changed
                .wait_while(queue, |queue| {
                    (queue.pending.is_none() || queue.completion.is_some()) && !queue.shutdown
                })
                .unwrap_or_else(|e| e.into_inner());
            if queue.shutdown {
                return;
            }
            let request = queue.pending.take().unwrap();
            queue.active = Some(Active {
                generation: request.generation,
                cancel: request.cancel.clone(),
            });
            request
        };
        shared.wake.signal();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| provider(&request)));
        let outcome = match result {
            Ok(outcome) => interrupted(&request).map_or(outcome, Outcome::empty),
            Err(_) => {
                shared.recovery.store(true, Ordering::Release);
                shared.wake.signal();
                let mut outcome = Outcome::empty(QuickOpenState::Failed);
                outcome.failure = Some(QuickOpenFailure::WorkerPanic);
                outcome
            }
        };
        let mut queue = shared.queue.lock().unwrap_or_else(|e| e.into_inner());
        queue.active = None;
        if queue.shutdown {
            return;
        }
        if queue.generation == Some(request.generation) {
            queue.completion = Some(Completion {
                generation: request.generation,
                context: request.context,
                outcome,
            });
        }
        drop(queue);
        shared.changed.notify_all();
        shared.wake.signal();
        // Request and any discarded result are dropped before the next wait.
    }
}
fn reparse(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}
fn interrupted(request: &Request) -> Option<QuickOpenState> {
    if Instant::now() >= request.deadline {
        Some(QuickOpenState::TimedOut)
    } else if request.cancel.is_cancelled() {
        Some(QuickOpenState::Cancelled)
    } else {
        None
    }
}
struct FoldPattern {
    chars: Vec<char>,
    prefix: Vec<usize>,
}
impl FoldPattern {
    fn new(query: &str) -> Self {
        let chars: Vec<_> = query
            .chars()
            .flat_map(|ch| full_case_fold(if ch == '\\' { '/' } else { ch }))
            .collect();
        let mut prefix = vec![0; chars.len()];
        let mut matched = 0;
        for index in 1..chars.len() {
            while matched > 0 && chars[index] != chars[matched] {
                matched = prefix[matched - 1];
            }
            if chars[index] == chars[matched] {
                matched += 1;
            }
            prefix[index] = matched;
        }
        Self { chars, prefix }
    }
    fn matches(&self, text: &str, request: &Request) -> Option<bool> {
        if self.chars.is_empty() {
            return Some(true);
        }
        let mut matched = 0;
        let mut origins = VecDeque::with_capacity(self.chars.len());
        for (count, ch) in text.chars().enumerate() {
            if count % 128 == 0 && request.cancel.is_cancelled() {
                return None;
            }
            let fold = full_case_fold(if ch == '\\' { '/' } else { ch });
            let len = fold.len();
            for (index, ch) in fold.enumerate() {
                if origins.len() == self.chars.len() {
                    origins.pop_front();
                }
                origins.push_back((index == 0, index + 1 == len));
                while matched > 0 && ch != self.chars[matched] {
                    matched = self.prefix[matched - 1];
                }
                if ch == self.chars[matched] {
                    matched += 1;
                }
                if matched == self.chars.len() {
                    if origins.front().is_some_and(|flags| flags.0) && index + 1 == len {
                        return Some(true);
                    }
                    matched = self.prefix[matched - 1];
                }
            }
        }
        Some(false)
    }
}
fn fail(reason: QuickOpenFailure) -> Outcome {
    let mut outcome = Outcome::empty(QuickOpenState::Failed);
    outcome.failure = Some(reason);
    outcome
}
fn scan(request: &Request) -> Outcome {
    if let Some(state) = interrupted(request) {
        return Outcome::empty(state);
    }
    let metadata = match fs::symlink_metadata(&request.context.root) {
        Ok(metadata) => metadata,
        Err(_) => return fail(QuickOpenFailure::InvalidRoot),
    };
    if !metadata.is_dir() || reparse(&metadata) {
        return fail(QuickOpenFailure::InvalidRoot);
    }
    let root = match request.context.root.canonicalize() {
        Ok(root) if root.as_os_str().len() <= request.limits.path_units => root,
        _ => return fail(QuickOpenFailure::InvalidRoot),
    };
    let directory = match fs::read_dir(&root) {
        Ok(directory) => directory,
        Err(_) => return fail(QuickOpenFailure::Io),
    };
    let mut outcome = Outcome::empty(QuickOpenState::Complete);
    outcome.root = root;
    outcome.counters.directories = 1;
    if outcome.memory_bytes() > request.limits.max_owned_bytes {
        outcome.root = PathBuf::new();
        outcome.limit(QuickOpenLimit::OwnedBytes);
        outcome.counters.owned_bytes = outcome.memory_bytes();
        return outcome;
    }
    let pattern = FoldPattern::new(&request.context.query);
    let mut stack = vec![directory];
    let mut paths = 0;
    while !stack.is_empty() {
        if let Some(state) = interrupted(request) {
            return Outcome::empty(state);
        }
        let Some(entry) = stack.last_mut().unwrap().next() else {
            stack.pop();
            continue;
        };
        if outcome.counters.entries >= request.limits.max_entries {
            outcome.limit(QuickOpenLimit::Entries);
            break;
        }
        outcome.counters.entries += 1;
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                outcome.counters.skipped_unreadable += 1;
                outcome.limit(QuickOpenLimit::UnreadablePaths);
                continue;
            }
        };
        let path = entry.path();
        if path.as_os_str().len() > request.limits.path_units {
            outcome.counters.skipped_paths += 1;
            outcome.limit(QuickOpenLimit::PathUnits);
            continue;
        }
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => {
                outcome.counters.skipped_unreadable += 1;
                outcome.limit(QuickOpenLimit::UnreadablePaths);
                continue;
            }
        };
        if reparse(&metadata) {
            outcome.counters.skipped_links += 1;
            continue;
        }
        if metadata.is_dir()
            && entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.eq_ignore_ascii_case(".git"))
        {
            outcome.counters.excluded_directories += 1;
            continue;
        }
        let canonical = match path.canonicalize() {
            Ok(path)
                if path.starts_with(&outcome.root)
                    && path.as_os_str().len() <= request.limits.path_units =>
            {
                path
            }
            _ => {
                outcome.counters.skipped_paths += 1;
                outcome.limit(QuickOpenLimit::PathUnits);
                continue;
            }
        };
        if metadata.is_dir() {
            if stack.len() >= request.limits.max_depth {
                outcome.limit(QuickOpenLimit::Depth);
                continue;
            }
            match fs::read_dir(&canonical) {
                Ok(directory) => {
                    stack.push(directory);
                    outcome.counters.directories += 1;
                }
                Err(_) => {
                    outcome.counters.skipped_unreadable += 1;
                    outcome.limit(QuickOpenLimit::UnreadablePaths);
                }
            }
            continue;
        }
        if !metadata.is_file() {
            continue;
        }
        let relative = path
            .strip_prefix(&outcome.root)
            .expect("walked path inside root");
        let matched = if request.context.query.is_empty() {
            true
        } else if let Some(text) = relative.to_str() {
            match pattern.matches(text, request) {
                Some(matched) => matched,
                None => {
                    return Outcome::empty(
                        interrupted(request).unwrap_or(QuickOpenState::Cancelled),
                    )
                }
            }
        } else {
            outcome.counters.skipped_non_unicode += 1;
            outcome.limit(QuickOpenLimit::NonUnicodePaths);
            continue;
        };
        if !matched {
            continue;
        }
        outcome.counters.matches += 1;
        if outcome.candidates.len() >= request.limits.max_results {
            outcome.limit(QuickOpenLimit::Results);
            break;
        }
        let candidate = QuickOpenCandidate {
            ordinal: outcome.candidates.len(),
            label: display_label(relative.as_os_str().as_encoded_bytes()),
            path,
            canonical,
        };
        let owned = candidate.heap_bytes();
        let base = std::mem::size_of::<Outcome>() + outcome.root.capacity() + paths + owned;
        if outcome.candidates.len() == outcome.candidates.capacity() {
            let available = request.limits.max_owned_bytes.saturating_sub(base)
                / std::mem::size_of::<QuickOpenCandidate>();
            let target = outcome
                .candidates
                .capacity()
                .saturating_mul(2)
                .max(4)
                .min(request.limits.max_results)
                .min(available);
            if target <= outcome.candidates.len() {
                outcome.limit(QuickOpenLimit::OwnedBytes);
                break;
            }
            outcome
                .candidates
                .reserve_exact(target - outcome.candidates.len());
        }
        if base + outcome.candidates.capacity() * std::mem::size_of::<QuickOpenCandidate>()
            > request.limits.max_owned_bytes
        {
            outcome.limit(QuickOpenLimit::OwnedBytes);
            break;
        }
        paths += owned;
        outcome.candidates.push(candidate);
    }
    // reserve_exact may grant extra capacity. Never retain an over-budget
    // allocation even if the allocator granted more than the requested slots.
    if outcome.memory_bytes() > request.limits.max_owned_bytes {
        outcome.candidates = Vec::new();
        outcome.limit(QuickOpenLimit::OwnedBytes);
    }
    outcome.counters.owned_bytes = outcome.memory_bytes();
    outcome
}
fn display_label(bytes: &[u8]) -> String {
    let mut result = String::new();
    let mut offset = 0;
    let mut columns = 0;
    let mut previous = 0;
    while offset < bytes.len() {
        let byte = bytes[offset];
        let len = match byte {
            0..=0x7f => 1,
            0xc2..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf4 => 4,
            _ => 0,
        };
        let scalar = if len == 0 {
            None
        } else {
            bytes
                .get(offset..offset + len)
                .and_then(|bytes| std::str::from_utf8(bytes).ok())
                .and_then(|s| s.chars().next())
        };
        let (token, consumed) = match scalar {
            Some('\\') => ("\\\\".into(), 1),
            Some(ch)
                if ch.is_control()
                    || matches!(ch, '\u{061c}' | '\u{200b}'..='\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2060}' | '\u{2066}'..='\u{2069}' | '\u{feff}') =>
            {
                (format!("\\u{{{:X}}}", ch as u32), len)
            }
            Some(ch) => (ch.to_string(), len),
            None => (format!("\\x{byte:02X}"), 1),
        };
        let width = token.chars().count();
        if columns + width > LABEL_COLUMNS {
            break;
        }
        previous = result.len();
        result.push_str(&token);
        columns += width;
        offset += consumed;
        if columns == LABEL_COLUMNS {
            break;
        }
    }
    if offset < bytes.len() {
        if columns == LABEL_COLUMNS {
            result.truncate(previous);
        }
        result.push('…');
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stale_panic_recovery_survives_invalidation_and_provider_metadata_is_not_retained() {
        let wake = Wakeup::default();
        let root = std::env::current_dir().unwrap();
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let provider_gate = gate.clone();
        let provider = Arc::new(move |request: &Request| {
            let (lock, changed) = &*provider_gate;
            let mut started = lock.lock().unwrap();
            *started = true;
            changed.notify_all();
            drop(started);
            while !request.cancel.is_cancelled() {
                thread::yield_now();
            }
            panic!("injected stale quick-open provider panic");
        });
        let mut controller =
            QuickOpenController::with_provider(wake.clone(), QuickOpenLimits::default(), provider);
        controller.request(root.clone(), "old".into()).unwrap();
        let (lock, changed) = &*gate;
        let started = lock.lock().unwrap();
        let (started, _) = changed
            .wait_timeout_while(started, Duration::from_secs(2), |started| !*started)
            .unwrap();
        assert!(*started);
        drop(started);
        controller.clear();
        let end = Instant::now() + Duration::from_secs(2);
        loop {
            let ticket = wake.ticket();
            if controller.take_recovery() {
                break;
            }
            assert!(Instant::now() < end);
            wake.wait(ticket, Some(Duration::from_millis(10)));
        }
        assert_eq!(controller.state(), QuickOpenState::Idle);
        assert!(controller.results().is_empty());
        assert!(!controller.take_recovery());
    }
    #[test]
    fn labels_escape_opaque_bytes_and_controls_without_interpreting_as_a_path() {
        assert_eq!(display_label(b"x\xff\\n\n"), "x\\xFF\\\\n\\u{A}");
        assert_eq!(display_label("niño/界".as_bytes()), "niño/界");
        assert_eq!(display_label("a\u{202e}b".as_bytes()), "a\\u{202E}b");
        assert!(display_label(&vec![b'x'; 4096]).chars().count() <= LABEL_COLUMNS);
    }
    #[test]
    fn only_latest_pending_metadata_runs_and_completion_remains_until_poll() {
        let wake = Wakeup::default();
        let root = std::env::current_dir().unwrap();
        let gate = Arc::new((
            Mutex::new((false, false, Vec::<String>::new())),
            Condvar::new(),
        ));
        let provider_gate = gate.clone();
        let provider = Arc::new(move |request: &Request| {
            let (lock, changed) = &*provider_gate;
            let mut guard = lock.lock().unwrap();
            guard.2.push(request.context.query.clone());
            if request.context.query == "first" {
                guard.0 = true;
                changed.notify_all();
                guard = changed.wait_while(guard, |guard| !guard.1).unwrap();
            }
            drop(guard);
            Outcome::empty(QuickOpenState::Complete)
        });
        let mut controller =
            QuickOpenController::with_provider(wake.clone(), QuickOpenLimits::default(), provider);
        controller.request(root.clone(), "first".into()).unwrap();
        let (lock, changed) = &*gate;
        let guard = lock.lock().unwrap();
        let (mut guard, _) = changed
            .wait_timeout_while(guard, Duration::from_secs(2), |guard| !guard.0)
            .unwrap();
        let started = guard.0;
        if !started {
            guard.1 = true;
            changed.notify_all();
        }
        drop(guard);
        assert!(started);
        controller.request(root.clone(), "discard".into()).unwrap();
        controller.request(root.clone(), "latest".into()).unwrap();
        {
            let mut guard = lock.lock().unwrap();
            guard.1 = true;
            changed.notify_all();
        }
        let end = Instant::now() + Duration::from_secs(2);
        while !controller.ready() {
            let ticket = wake.ticket();
            assert!(Instant::now() < end);
            wake.wait(ticket, Some(Duration::from_millis(10)));
        }
        assert_eq!(
            lock.lock().unwrap().2,
            vec![String::from("first"), String::from("latest")]
        );
        assert!(controller.ready());
        assert!(controller.poll(&root, "latest"));
        assert_eq!(controller.state(), QuickOpenState::Complete);
        assert!(!controller.ready());
    }
    #[test]
    fn full_fold_expansions_do_not_allow_half_a_source_scalar_match() {
        let request = Request {
            generation: 1,
            context: Context {
                root: PathBuf::new(),
                query: String::new(),
            },
            limits: QuickOpenLimits::default(),
            cancel: Cancellation::new(Duration::from_secs(2)),
            deadline: Instant::now() + Duration::from_secs(2),
        };
        assert_eq!(
            FoldPattern::new("ss").matches("ß.txt", &request),
            Some(true)
        );
        assert_eq!(
            FoldPattern::new("s").matches("ß.txt", &request),
            Some(false)
        );
        assert_eq!(FoldPattern::new("σ").matches("ς.txt", &request), Some(true));
    }
}
