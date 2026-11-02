//! Lazy repository-scoped status delivery. No document snapshots or live buffers.
//!
//! Like ProjectSearch, this owns a repository worker rather than forging a
//! document/revision for JobSystem. There is one running provider, one latest
//! metadata request, and one completion. A completion holds the worker until
//! poll consumes it; neither a hidden result backlog nor detached threads exist.
use crate::{
    git_read::{GitReadError, GitReader},
    git_status::{read_status, GitStatusError, GitStatusReadOptions, GitStatusSnapshot},
    Cancellation, Wakeup,
};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
    time::Duration,
};

const PATH_LIMIT: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitStatusOptions {
    /// Trusted absolute installed executable; validated/opened only by worker.
    pub git_executable: PathBuf,
    /// Absolute repository root, not an editor document's directory.
    pub root: PathBuf,
    pub limits: GitStatusReadOptions,
}
impl GitStatusOptions {
    fn valid(&self) -> bool {
        self.limits.valid()
            && [&self.git_executable, &self.root].into_iter().all(|path| {
                path.is_absolute()
                    && path.as_os_str().len() <= PATH_LIMIT
                    && path.capacity() <= PATH_LIMIT
            })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GitStatusTaskState {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitStatusRequestError {
    InvalidOptions,
    NoConfiguration,
    GenerationExhausted,
    WorkerUnavailable,
}
impl std::fmt::Display for GitStatusRequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Git status request: {self:?}")
    }
}
impl std::error::Error for GitStatusRequestError {}

#[derive(Debug)]
pub enum GitStatusFailure {
    Provider(GitStatusError),
    WorkerPanic,
}

type Provider = dyn Fn(&GitStatusOptions, &Cancellation) -> Result<GitStatusSnapshot, GitStatusError>
    + Send
    + Sync;

struct Request {
    generation: u64,
    options: GitStatusOptions,
}
struct Active {
    generation: u64,
    cancel: Cancellation,
}
struct Completion {
    generation: u64,
    options: GitStatusOptions,
    result: Result<GitStatusSnapshot, GitStatusFailure>,
}
#[derive(Default)]
struct Queue {
    generation: Option<u64>,
    active: Option<Active>,
    pending: Option<Request>,
    completion: Option<Completion>,
    stop: bool,
}
struct Shared {
    queue: Mutex<Queue>,
    changed: Condvar,
    wake: Wakeup,
}

pub struct GitStatusController {
    shared: Arc<Shared>,
    provider: Arc<Provider>,
    worker: Option<JoinHandle<()>>,
    generation: u64,
    options: Option<GitStatusOptions>,
    result: Option<GitStatusSnapshot>,
    failure: Option<GitStatusFailure>,
    state: GitStatusTaskState,
}
impl GitStatusController {
    /// Allocates controller metadata only. No thread, Git process or file I/O.
    pub fn new(wake: Wakeup) -> Self {
        Self::with_provider(wake, Arc::new(run_provider))
    }

    fn with_provider(wake: Wakeup, provider: Arc<Provider>) -> Self {
        Self {
            shared: Arc::new(Shared {
                queue: Mutex::new(Queue::default()),
                changed: Condvar::new(),
                wake,
            }),
            provider,
            worker: None,
            generation: 0,
            options: None,
            result: None,
            failure: None,
            state: GitStatusTaskState::Idle,
        }
    }

    pub fn state(&self) -> GitStatusTaskState {
        self.state
    }

    pub fn root(&self) -> Option<&Path> {
        self.options.as_ref().map(|options| options.root.as_path())
    }

    pub fn result(&self) -> Option<&GitStatusSnapshot> {
        self.result.as_ref()
    }

    pub fn failure(&self) -> Option<&GitStatusFailure> {
        self.failure.as_ref()
    }

    /// A bounded summary; provider failures remain typed, without formatting
    /// arbitrary paths, filenames, stderr or panic payload into UI-owned text.
    pub fn message(&self) -> &'static str {
        match self.state {
            GitStatusTaskState::Idle => "",
            GitStatusTaskState::Queued => "Git status queued",
            GitStatusTaskState::Running => "Reading Git status",
            GitStatusTaskState::Ready => "Git status ready",
            GitStatusTaskState::Limited => "Git status exceeded its limits",
            GitStatusTaskState::Cancelled => "Git status cancelled",
            GitStatusTaskState::Stale => "Git status configuration changed; refresh required",
            GitStatusTaskState::Failed => "Git status failed",
            GitStatusTaskState::TimedOut => "Git status timed out",
        }
    }

    /// True only for a completed delivery; Wakeup also signals worker start so
    /// callers may poll on their wake ticket to expose Queued -> Running.
    pub fn ready(&self) -> bool {
        self.shared
            .queue
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .completion
            .is_some()
    }

    fn invalidate_work(&mut self) {
        let mut queue = self
            .shared
            .queue
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(active) = &queue.active {
            active.cancel.cancel();
        }
        queue.generation = None;
        queue.pending = None;
        queue.completion = None;
        drop(queue);
        self.result = None;
        self.failure = None;
        self.shared.changed.notify_all();
    }

    /// Replaces the sole pending metadata request and cooperatively cancels a
    /// running generation. Does not wait, discover a repository or read files.
    pub fn request(&mut self, options: GitStatusOptions) -> Result<(), GitStatusRequestError> {
        self.invalidate_work();
        self.options = None;
        let Some(generation) = self.generation.checked_add(1) else {
            self.state = GitStatusTaskState::Failed;
            return Err(GitStatusRequestError::GenerationExhausted);
        };
        self.generation = generation;
        if !options.valid() {
            self.state = GitStatusTaskState::Failed;
            return Err(GitStatusRequestError::InvalidOptions);
        }
        self.options = Some(options.clone());
        if self.worker.is_none() {
            let shared = self.shared.clone();
            let provider = self.provider.clone();
            match thread::Builder::new()
                .name("jc-git-status".into())
                .spawn(move || worker(shared, provider))
            {
                Ok(worker) => self.worker = Some(worker),
                Err(_) => {
                    self.state = GitStatusTaskState::Failed;
                    return Err(GitStatusRequestError::WorkerUnavailable);
                }
            }
        }
        let mut queue = self
            .shared
            .queue
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        queue.generation = Some(generation);
        queue.pending = Some(Request {
            generation,
            options,
        });
        drop(queue);
        self.state = GitStatusTaskState::Queued;
        self.shared.changed.notify_all();
        Ok(())
    }

    /// Reuses only the configuration retained by request/cancel. A mismatching
    /// poll invalidates that configuration, so refresh cannot revive an old root.
    pub fn refresh(&mut self) -> Result<(), GitStatusRequestError> {
        let options = self
            .options
            .clone()
            .ok_or(GitStatusRequestError::NoConfiguration)?;
        self.request(options)
    }

    pub fn cancel(&mut self) {
        self.invalidate_work();
        self.state = GitStatusTaskState::Cancelled;
    }

    /// Pass the current root/executable/limits before exposing result(). Changes
    /// invalidate accepted and queued data even when no refresh was requested.
    /// Only locks bounded metadata and moves an already prepared completion.
    pub fn poll(&mut self, current: &GitStatusOptions) -> bool {
        if self
            .options
            .as_ref()
            .is_some_and(|options| options != current)
        {
            self.invalidate_work();
            self.options = None;
            self.state = GitStatusTaskState::Stale;
            return true;
        }
        let (completion, running) = {
            let mut queue = self
                .shared
                .queue
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let running = queue
                .active
                .as_ref()
                .is_some_and(|active| active.generation == self.generation);
            (queue.completion.take(), running)
        };
        self.shared.changed.notify_all();
        if let Some(completion) = completion {
            if completion.generation != self.generation
                || self.options.as_ref() != Some(&completion.options)
                || completion.options != *current
            {
                return false;
            }
            match completion.result {
                Ok(result) => {
                    self.result = Some(result);
                    self.failure = None;
                    self.state = GitStatusTaskState::Ready;
                }
                Err(failure) => {
                    self.state = failure_state(&failure);
                    self.failure = Some(failure);
                    self.result = None;
                }
            }
            return true;
        }
        if running && self.state == GitStatusTaskState::Queued {
            self.state = GitStatusTaskState::Running;
            return true;
        }
        false
    }
}

impl Drop for GitStatusController {
    fn drop(&mut self) {
        self.invalidate_work();
        let mut queue = self
            .shared
            .queue
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        queue.stop = true;
        drop(queue);
        self.shared.changed.notify_all();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run_provider(
    options: &GitStatusOptions,
    cancel: &Cancellation,
) -> Result<GitStatusSnapshot, GitStatusError> {
    if cancel.is_cancelled() {
        return Err(GitStatusError::Cancelled);
    }
    let reader = GitReader::new(options.git_executable.clone()).map_err(GitStatusError::Read)?;
    read_status(&reader, &options.root, options.limits, || {
        cancel.is_cancelled()
    })
}

fn failure_state(failure: &GitStatusFailure) -> GitStatusTaskState {
    match failure {
        GitStatusFailure::Provider(GitStatusError::Cancelled)
        | GitStatusFailure::Provider(GitStatusError::Read(GitReadError::Cancelled)) => {
            GitStatusTaskState::Cancelled
        }
        GitStatusFailure::Provider(GitStatusError::TimedOut)
        | GitStatusFailure::Provider(GitStatusError::Read(GitReadError::TimedOut)) => {
            GitStatusTaskState::TimedOut
        }
        GitStatusFailure::Provider(GitStatusError::Limited(_))
        | GitStatusFailure::Provider(GitStatusError::Read(GitReadError::OutputLimit(_))) => {
            GitStatusTaskState::Limited
        }
        _ => GitStatusTaskState::Failed,
    }
}

fn worker(shared: Arc<Shared>, provider: Arc<Provider>) {
    loop {
        let (request, cancel) = {
            let queue = shared
                .queue
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let mut queue = shared
                .changed
                .wait_while(queue, |queue| {
                    !queue.stop && (queue.pending.is_none() || queue.completion.is_some())
                })
                .unwrap_or_else(|error| error.into_inner());
            if queue.stop {
                return;
            }
            let request = queue.pending.take().expect("queued status request");
            // Provider owns the <=30s execution/parse deadline. This extra
            // second keeps explicit cancellation separate from TimedOut; queue
            // residence consumes neither deadline because the token starts here.
            let cancel = Cancellation::new(Duration::from_secs(31));
            queue.active = Some(Active {
                generation: request.generation,
                cancel: cancel.clone(),
            });
            (request, cancel)
        };
        shared.wake.signal();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            provider(&request.options, &cancel)
        }));
        let result = match result {
            Ok(result) => result.map_err(GitStatusFailure::Provider),
            Err(_) => Err(GitStatusFailure::WorkerPanic),
        };
        let mut queue = shared
            .queue
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        queue.active = None;
        if queue.stop {
            return;
        }
        if queue.generation == Some(request.generation) {
            queue.completion = Some(Completion {
                generation: request.generation,
                options: request.options,
                result,
            });
        }
        drop(queue);
        shared.changed.notify_all();
        shared.wake.signal();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        git_read::GitRepositoryIdentity,
        git_status::{parse_porcelain_v2, GitStatusLimit, GitStatusLimits},
    };
    use std::{
        sync::{
            atomic::{AtomicUsize, Ordering},
            mpsc,
        },
        time::Instant,
    };

    fn options(name: &str) -> GitStatusOptions {
        let base = std::env::temp_dir().join("jc-status-controller-fixture");
        GitStatusOptions {
            git_executable: base.join("git.exe"),
            root: base.join(name),
            limits: GitStatusReadOptions::default(),
        }
    }

    fn snapshot(options: &GitStatusOptions) -> GitStatusSnapshot {
        GitStatusSnapshot::from_parts(
            GitRepositoryIdentity {
                root: options.root.clone(),
                git_dir: options.root.join(".git"),
            },
            parse_porcelain_v2(b"? raw-file\0", GitStatusLimits::default(), || false)
                .expect("fixture status"),
        )
    }

    fn wait_ready(controller: &GitStatusController, wake: &Wakeup) {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let ticket = wake.ticket();
            if controller.ready() {
                return;
            }
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .expect("status worker did not publish its bounded completion");
            wake.wait(ticket, Some(remaining));
        }
    }

    #[test]
    fn construction_and_invalid_metadata_do_not_start_a_provider() {
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let mut controller = GitStatusController::with_provider(
            Wakeup::default(),
            Arc::new(move |options, _| {
                observed.fetch_add(1, Ordering::SeqCst);
                Ok(snapshot(options))
            }),
        );
        assert_eq!(controller.state(), GitStatusTaskState::Idle);
        assert_eq!(
            controller.refresh(),
            Err(GitStatusRequestError::NoConfiguration)
        );
        let mut invalid = options("invalid");
        invalid.root = PathBuf::from("relative");
        assert_eq!(
            controller.request(invalid),
            Err(GitStatusRequestError::InvalidOptions)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(controller.worker.is_none());
        let mut oversized = options("excess-capacity");
        oversized.root.reserve(PATH_LIMIT * 2);
        assert_eq!(
            controller.request(oversized),
            Err(GitStatusRequestError::InvalidOptions)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(controller.worker.is_none());
    }

    #[test]
    fn active_success_after_cancel_is_discarded_and_only_latest_pending_runs() {
        let wake = Wakeup::default();
        let gate = Arc::new(Mutex::new(()));
        let provider_gate = gate.clone();
        let (started, starts) = mpsc::channel();
        let current = Arc::new(AtomicUsize::new(0));
        let active = current.clone();
        let mut controller = GitStatusController::with_provider(
            wake.clone(),
            Arc::new(move |options, cancel| {
                assert_eq!(active.fetch_add(1, Ordering::SeqCst), 0);
                started.send(options.root.clone()).unwrap();
                if options.root.ends_with("first") {
                    // Parent owns the guard before dispatch. It queues both
                    // replacements before letting the old provider return a
                    // deliberately successful but obsolete completion.
                    drop(provider_gate.lock().unwrap());
                    assert!(cancel.is_cancelled());
                }
                active.fetch_sub(1, Ordering::SeqCst);
                Ok(snapshot(options))
            }),
        );
        let held = gate.lock().unwrap();
        controller.request(options("first")).unwrap();
        assert_eq!(
            starts.recv_timeout(Duration::from_secs(3)).unwrap(),
            options("first").root
        );
        controller.request(options("second")).unwrap();
        let latest = options("third");
        controller.request(latest.clone()).unwrap();
        drop(held);
        assert_eq!(
            starts.recv_timeout(Duration::from_secs(3)).unwrap(),
            latest.root
        );
        wait_ready(&controller, &wake);
        assert!(controller.poll(&latest));
        assert_eq!(controller.state(), GitStatusTaskState::Ready);
        assert_eq!(controller.result().unwrap().repository().root, latest.root);
        assert!(starts.try_recv().is_err());
        assert_eq!(current.load(Ordering::SeqCst), 0);
        assert!(!controller.ready());
    }

    #[test]
    fn every_configuration_change_rejects_an_unpolled_or_accepted_result() {
        for change in 0..3 {
            let wake = Wakeup::default();
            let mut controller = GitStatusController::with_provider(
                wake.clone(),
                Arc::new(|options, _| Ok(snapshot(options))),
            );
            let original = options("original");
            controller.request(original.clone()).unwrap();
            wait_ready(&controller, &wake);
            if change == 1 {
                controller.poll(&original);
                assert!(controller.result().is_some());
            }
            let mut next = original.clone();
            match change {
                0 => next.root = options("other-root").root,
                1 => next.git_executable = next.git_executable.with_file_name("other-git.exe"),
                _ => next.limits.parse.max_entries -= 1,
            }
            assert!(controller.poll(&next));
            assert_eq!(controller.state(), GitStatusTaskState::Stale);
            assert!(controller.result().is_none());
            assert!(controller.failure().is_none());
            assert!(controller.root().is_none());
            assert!(!controller.ready());
            assert_eq!(
                controller.refresh(),
                Err(GitStatusRequestError::NoConfiguration)
            );
        }
    }

    #[test]
    fn cancel_clears_completion_and_refresh_reuses_configuration() {
        let wake = Wakeup::default();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let mut controller = GitStatusController::with_provider(
            wake.clone(),
            Arc::new(move |options, _| {
                observed.fetch_add(1, Ordering::SeqCst);
                Ok(snapshot(options))
            }),
        );
        let options = options("retained");
        controller.request(options.clone()).unwrap();
        wait_ready(&controller, &wake);
        controller.cancel();
        assert_eq!(controller.state(), GitStatusTaskState::Cancelled);
        assert!(!controller.poll(&options));
        assert!(controller.result().is_none());
        assert!(!controller.ready());
        controller.refresh().unwrap();
        wait_ready(&controller, &wake);
        controller.poll(&options);
        assert_eq!(controller.state(), GitStatusTaskState::Ready);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn bounded_typed_failures_and_provider_panic_have_terminal_states() {
        for (error, state) in [
            (GitStatusError::Cancelled, GitStatusTaskState::Cancelled),
            (GitStatusError::TimedOut, GitStatusTaskState::TimedOut),
            (
                GitStatusError::Limited(GitStatusLimit::Entries),
                GitStatusTaskState::Limited,
            ),
            (
                GitStatusError::Read(GitReadError::ConfiguredFilter),
                GitStatusTaskState::Failed,
            ),
        ] {
            let wake = Wakeup::default();
            let failure = Arc::new(Mutex::new(Some(error)));
            let mut controller = GitStatusController::with_provider(
                wake.clone(),
                Arc::new(move |_, _| Err(failure.lock().unwrap().take().unwrap())),
            );
            let options = options("failure");
            controller.request(options.clone()).unwrap();
            wait_ready(&controller, &wake);
            controller.poll(&options);
            assert_eq!(controller.state(), state);
            assert!(matches!(
                controller.failure(),
                Some(GitStatusFailure::Provider(_))
            ));
            assert!(controller.result().is_none());
        }

        let wake = Wakeup::default();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut controller = GitStatusController::with_provider(
            wake.clone(),
            Arc::new(move |options, _| {
                assert_ne!(
                    calls.fetch_add(1, Ordering::SeqCst),
                    0,
                    "injected provider panic"
                );
                Ok(snapshot(options))
            }),
        );
        let options = options("panic");
        controller.request(options.clone()).unwrap();
        wait_ready(&controller, &wake);
        controller.poll(&options);
        assert_eq!(controller.state(), GitStatusTaskState::Failed);
        assert!(matches!(
            controller.failure(),
            Some(GitStatusFailure::WorkerPanic)
        ));
        controller.refresh().unwrap();
        wait_ready(&controller, &wake);
        controller.poll(&options);
        assert_eq!(controller.state(), GitStatusTaskState::Ready);
    }

    #[test]
    fn drop_cancels_and_joins_the_running_provider() {
        let (started, starts) = mpsc::channel();
        let (finished, finishes) = mpsc::channel();
        let mut controller = GitStatusController::with_provider(
            Wakeup::default(),
            Arc::new(move |_, cancel| {
                started.send(()).unwrap();
                // Cooperative provider wait, not an ordering/timing assertion.
                // The predicate and channel delimit lifecycle; timeout bounds
                // a regression so a failed test cannot hang the whole suite.
                let deadline = Instant::now() + Duration::from_secs(3);
                let lock = Mutex::new(());
                let changed = Condvar::new();
                let mut guard = lock.lock().unwrap();
                while !cancel.is_cancelled() && Instant::now() < deadline {
                    guard = changed
                        .wait_timeout(guard, Duration::from_millis(5))
                        .unwrap()
                        .0;
                }
                assert!(cancel.is_cancelled());
                finished.send(()).unwrap();
                Err(GitStatusError::Cancelled)
            }),
        );
        controller.request(options("drop")).unwrap();
        starts.recv_timeout(Duration::from_secs(3)).unwrap();
        drop(controller);
        assert_eq!(finishes.try_recv(), Ok(()));
    }
}
