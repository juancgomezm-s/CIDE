//! Reviewed index writes for GUI and TUI. The controller owns at most one
//! request, one review, and the last publication outcome. Workers see Git
//! plans and bounded metadata, never a Workbench or editable buffers.
use crate::{Wakeup, Workbench};
use jc_git::index_write::{apply_index_change, prepare_index_change};
pub use jc_git::index_write::{IndexAction, IndexPlan};
use std::{
    fmt,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender, TrySendError},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

const PATH_LIMIT: usize = 4096;
const LIMITS: jc_git::Limits = jc_git::Limits {
    max_blob: 2 * 1024 * 1024,
    max_entries: 4096,
    max_path: PATH_LIMIT,
    max_bytes: 4 * 1024 * 1024,
    timeout: Duration::from_secs(5),
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndexState {
    Idle,
    Preparing,
    Reviewing,
    Applying,
    Published,
    Failed,
}

#[derive(Debug)]
pub enum IndexRequestError {
    Busy,
    ReviewPending,
    NoReview,
    StaleReview,
    DirtyBuffers,
    InvalidTarget,
    Exhausted,
    WorkerUnavailable,
}

impl fmt::Display for IndexRequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Busy => "Hay una operación del índice Git en curso",
            Self::ReviewPending => "Confirma o descarta primero el plan del índice Git",
            Self::NoReview => "No hay un plan del índice Git para confirmar",
            Self::StaleReview => "La confirmación no corresponde al plan o proyecto actual",
            Self::DirtyBuffers => {
                "Guarda los documentos con cambios antes de operar sobre el índice Git"
            }
            Self::InvalidTarget => {
                "La operación requiere una ruta Git y una raíz de proyecto válidas"
            }
            Self::Exhausted => "Se agotaron los identificadores de operación del índice Git",
            Self::WorkerUnavailable => "No se pudo iniciar la tarea del índice Git",
        })
    }
}

impl std::error::Error for IndexRequestError {}

#[derive(Debug)]
pub enum IndexFailure {
    Git(jc_git::Error),
    WorkerPanicked,
    WorkerDisconnected,
}

impl fmt::Display for IndexFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Git(error) => fmt::Display::fmt(error, f),
            Self::WorkerPanicked => {
                f.write_str("Falló la tarea del índice Git. Actualiza el estado del repositorio")
            }
            Self::WorkerDisconnected => {
                f.write_str("La tarea no entregó un resultado. Actualiza el estado del repositorio")
            }
        }
    }
}

impl std::error::Error for IndexFailure {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexTarget {
    pub root: PathBuf,
    pub path: Vec<u8>,
    pub action: IndexAction,
}

pub struct IndexReview {
    pub ticket: u64,
    pub target: IndexTarget,
    plan: Box<IndexPlan>,
}

impl IndexReview {
    pub fn plan(&self) -> &IndexPlan {
        &self.plan
    }
}

#[derive(Debug)]
pub struct IndexPreparationFailure {
    pub ticket: u64,
    pub target: IndexTarget,
    pub error: IndexFailure,
}

#[derive(Debug)]
pub struct IndexPublication {
    pub ticket: u64,
    pub target: IndexTarget,
    pub result: Result<(), IndexFailure>,
}

enum Phase {
    Prepare(Arc<AtomicBool>),
    Apply,
}

struct Active {
    ticket: u64,
    target: IndexTarget,
    phase: Phase,
}

enum Request {
    Prepare(IndexTarget, Arc<AtomicBool>),
    Apply(Box<IndexPlan>),
}

enum Completion {
    Prepared(Result<Box<IndexPlan>, IndexFailure>),
    Applied(Result<(), IndexFailure>),
}

struct Worker {
    requests: SyncSender<Request>,
    completion: Arc<Mutex<Option<Completion>>>,
    thread: JoinHandle<()>,
}

impl Worker {
    fn spawn(wake: Wakeup) -> Result<Self, IndexRequestError> {
        let (requests, receiver) = mpsc::sync_channel(1);
        let completion = Arc::new(Mutex::new(None));
        let slot = completion.clone();
        let thread = thread::Builder::new()
            .name("cide-git-index".into())
            .spawn(move || {
                while let Ok(request) = receiver.recv() {
                    let result = match request {
                        Request::Prepare(target, cancel) => Completion::Prepared(
                            run_worker(|| {
                                prepare_index_change(
                                    &target.root,
                                    &target.path,
                                    target.action,
                                    LIMITS,
                                    || cancel.load(Ordering::Acquire),
                                )
                            })
                            .map(Box::new),
                        ),
                        Request::Apply(plan) => {
                            Completion::Applied(run_worker(|| apply_index_change(*plan, || false)))
                        }
                    };
                    *slot.lock().unwrap_or_else(|error| error.into_inner()) = Some(result);
                    wake.signal();
                }
            })
            .map_err(|_| IndexRequestError::WorkerUnavailable)?;
        Ok(Self {
            requests,
            completion,
            thread,
        })
    }

    fn take(&self) -> Option<Completion> {
        self.completion
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
    }

    fn ready(&self) -> bool {
        self.completion
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .is_some()
            || self.thread.is_finished()
    }

    fn stop(self) {
        let Self {
            requests, thread, ..
        } = self;
        drop(requests);
        let _ = thread.join();
    }
}

fn run_worker<T>(run: impl FnOnce() -> Result<T, jc_git::Error>) -> Result<T, IndexFailure> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(run))
        .map_err(|_| IndexFailure::WorkerPanicked)?
        .map_err(IndexFailure::Git)
}

fn require_clean(bench: &Workbench) -> Result<(), IndexRequestError> {
    // Conservative across all open documents. Aliases or hardlinks must not
    // bypass the host guard and checking it must not read files on the UI thread.
    if bench
        .open_documents()
        .any(|doc| doc.session.document().is_modified())
    {
        return Err(IndexRequestError::DirtyBuffers);
    }
    Ok(())
}

pub struct GitIndexController {
    wake: Wakeup,
    worker: Option<Worker>,
    active: Option<Active>,
    review: Option<IndexReview>,
    preparation_failure: Option<IndexPreparationFailure>,
    publication: Option<IndexPublication>,
    next_ticket: u64,
}

impl GitIndexController {
    pub fn new(wake: Wakeup) -> Self {
        Self {
            wake,
            worker: None,
            active: None,
            review: None,
            preparation_failure: None,
            publication: None,
            next_ticket: 0,
        }
    }

    pub fn state(&self) -> IndexState {
        if let Some(active) = &self.active {
            return match active.phase {
                Phase::Prepare(_) => IndexState::Preparing,
                Phase::Apply => IndexState::Applying,
            };
        }
        if self.review.is_some() {
            IndexState::Reviewing
        } else if self.preparation_failure.is_some() {
            IndexState::Failed
        } else if let Some(publication) = &self.publication {
            if publication.result.is_ok() {
                IndexState::Published
            } else {
                IndexState::Failed
            }
        } else {
            IndexState::Idle
        }
    }

    pub fn review(&self) -> Option<&IndexReview> {
        self.review.as_ref()
    }

    pub fn preparation_failure(&self) -> Option<&IndexPreparationFailure> {
        self.preparation_failure.as_ref()
    }

    pub fn publication(&self) -> Option<&IndexPublication> {
        self.publication.as_ref()
    }

    pub fn take_publication(&mut self) -> Option<IndexPublication> {
        self.publication.take()
    }

    pub fn ready(&self) -> bool {
        self.active.is_some() && self.worker.as_ref().is_some_and(Worker::ready)
    }

    /// Request a read-only plan. The returned ticket identifies the review
    /// once `poll` accepts it. Confirmation is a separate, explicit action.
    pub fn prepare(
        &mut self,
        bench: &Workbench,
        root: PathBuf,
        path: Vec<u8>,
        action: IndexAction,
    ) -> Result<u64, IndexRequestError> {
        if self.active.is_some() {
            return Err(IndexRequestError::Busy);
        }
        if self.review.is_some() {
            return Err(IndexRequestError::ReviewPending);
        }
        require_clean(bench)?;
        if !root.is_absolute()
            || root.as_os_str().len() > PATH_LIMIT
            || path.is_empty()
            || path.len() > PATH_LIMIT
        {
            return Err(IndexRequestError::InvalidTarget);
        }
        let ticket = self
            .next_ticket
            .checked_add(1)
            .ok_or(IndexRequestError::Exhausted)?;
        if self
            .worker
            .as_ref()
            .is_some_and(|worker| worker.thread.is_finished())
        {
            self.stop_worker();
        }
        if self.worker.is_none() {
            self.worker = Some(Worker::spawn(self.wake.clone())?);
        }
        let target = IndexTarget {
            root: root.into_boxed_path().into_path_buf(),
            path: path.into_boxed_slice().into_vec(),
            action,
        };
        let cancel = Arc::new(AtomicBool::new(false));
        self.worker
            .as_ref()
            .unwrap()
            .requests
            .try_send(Request::Prepare(target.clone(), cancel.clone()))
            .map_err(|_| IndexRequestError::WorkerUnavailable)?;
        self.next_ticket = ticket;
        self.preparation_failure = None;
        self.active = Some(Active {
            ticket,
            target,
            phase: Phase::Prepare(cancel),
        });
        Ok(ticket)
    }

    /// Apply the exact plan presented under `ticket` in the current project.
    /// The host guard is checked again here. Editing after confirmation does
    /// not change the disk/index pair the worker is authorized to publish.
    pub fn apply(
        &mut self,
        bench: &Workbench,
        ticket: u64,
        current_root: &Path,
    ) -> Result<(), IndexRequestError> {
        if self.active.is_some() {
            return Err(IndexRequestError::Busy);
        }
        let review = self.review.as_ref().ok_or(IndexRequestError::NoReview)?;
        if review.ticket != ticket || review.target.root != current_root {
            return Err(IndexRequestError::StaleReview);
        }
        require_clean(bench)?;
        let worker = self
            .worker
            .as_ref()
            .ok_or(IndexRequestError::WorkerUnavailable)?;
        let IndexReview {
            ticket,
            target,
            plan,
        } = self.review.take().unwrap();
        match worker.requests.try_send(Request::Apply(plan)) {
            Ok(()) => {
                self.active = Some(Active {
                    ticket,
                    target,
                    phase: Phase::Apply,
                });
                Ok(())
            }
            Err(
                TrySendError::Full(Request::Apply(plan))
                | TrySendError::Disconnected(Request::Apply(plan)),
            ) => {
                self.review = Some(IndexReview {
                    ticket,
                    target,
                    plan,
                });
                Err(IndexRequestError::WorkerUnavailable)
            }
            Err(_) => unreachable!("send returns the same request"),
        }
    }

    /// Cancel a read-only preparation or drop a review. An admitted publication
    /// is never cancelled or discarded when the panel or selection changes.
    pub fn discard_review(&mut self) -> Result<(), IndexRequestError> {
        if let Some(active) = &self.active {
            match &active.phase {
                Phase::Prepare(cancel) => cancel.store(true, Ordering::Release),
                Phase::Apply => return Err(IndexRequestError::Busy),
            }
        }
        self.review = None;
        self.preparation_failure = None;
        Ok(())
    }

    /// Sole acceptance route. No selection or document stamp participates in
    /// accepting a write outcome. The host can refresh Git for its target root.
    pub fn poll(&mut self) -> bool {
        let (Some(worker), Some(_)) = (&self.worker, &self.active) else {
            return false;
        };
        let completion = worker.take();
        if completion.is_none() && !worker.thread.is_finished() {
            return false;
        }
        let Active {
            ticket,
            target,
            phase,
        } = self.active.take().unwrap();
        match phase {
            Phase::Prepare(cancel) if !cancel.load(Ordering::Acquire) => {
                let result = match completion {
                    Some(Completion::Prepared(result)) => result,
                    _ => Err(IndexFailure::WorkerDisconnected),
                };
                match result {
                    Ok(plan) => {
                        self.review = Some(IndexReview {
                            ticket,
                            target,
                            plan,
                        })
                    }
                    Err(error) => {
                        self.preparation_failure = Some(IndexPreparationFailure {
                            ticket,
                            target,
                            error,
                        })
                    }
                }
            }
            Phase::Prepare(_) => {}
            Phase::Apply => {
                let result = match completion {
                    Some(Completion::Applied(result)) => result,
                    _ => Err(IndexFailure::WorkerDisconnected),
                };
                self.publication = Some(IndexPublication {
                    ticket,
                    target,
                    result,
                });
            }
        }
        true
    }

    fn stop_worker(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.stop();
        }
    }
}

impl Drop for GitIndexController {
    fn drop(&mut self) {
        if let Some(Active {
            phase: Phase::Prepare(cancel),
            ..
        }) = &self.active
        {
            cancel.store(true, Ordering::Release);
        }
        // A confirmed write must reach its final result before shutting down.
        self.stop_worker();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        time::{Instant, SystemTime, UNIX_EPOCH},
    };

    struct Fixture {
        parent: PathBuf,
        root: PathBuf,
    }

    impl Fixture {
        fn unborn() -> Self {
            let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .nth(2)
                .unwrap()
                .canonicalize()
                .unwrap();
            let parent = workspace.join("work/test-temp");
            fs::create_dir_all(&parent).unwrap();
            let parent = parent.canonicalize().unwrap();
            assert!(parent.starts_with(&workspace));
            let root = parent.join(format!(
                "git-index-controller-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir(&root).unwrap();
            let git = root.join(".git");
            fs::create_dir_all(git.join("refs/heads")).unwrap();
            fs::create_dir_all(git.join("objects")).unwrap();
            fs::write(git.join("HEAD"), b"ref: refs/heads/main\n").unwrap();
            fs::write(
                git.join("config"),
                b"[core]\n\trepositoryformatversion = 0\n\tbare = false\n\tfilemode = false\n",
            )
            .unwrap();
            fs::write(root.join("fuente.txt"), b"fixture CIDE\r\n").unwrap();
            Self { parent, root }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            assert_eq!(self.root.parent(), Some(self.parent.as_path()));
            assert_eq!(self.root.canonicalize().unwrap(), self.root);
            fn remove_owned(path: &Path) {
                let metadata = fs::symlink_metadata(path).unwrap();
                assert!(!metadata.file_type().is_symlink());
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    assert_eq!(metadata.file_attributes() & 0x400, 0);
                }
                if metadata.is_dir() {
                    for entry in fs::read_dir(path).unwrap() {
                        remove_owned(&entry.unwrap().path());
                    }
                    fs::remove_dir(path).unwrap();
                } else {
                    #[cfg(windows)]
                    #[allow(clippy::permissions_set_readonly_false)]
                    if metadata.permissions().readonly() {
                        let mut permissions = metadata.permissions();
                        permissions.set_readonly(false);
                        fs::set_permissions(path, permissions).unwrap();
                    }
                    fs::remove_file(path).unwrap();
                }
            }
            remove_owned(&self.root);
        }
    }

    fn wait_delivery(controller: &mut GitIndexController, wake: &Wakeup) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let ticket = wake.ticket();
            if controller.poll() {
                return;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(!remaining.is_zero(), "no llegó la entrega del índice");
            wake.wait(ticket, Some(remaining));
        }
    }

    #[test]
    fn git_index_prepares_read_only_then_keeps_publication_after_discarding_a_review() {
        let fixture = Fixture::unborn();
        let wake = Wakeup::default();
        let mut controller = GitIndexController::new(wake.clone());
        let clean = Workbench::default();
        let mut dirty = Workbench::default();
        dirty.new_file("sin-guardar.txt", jc_text::TextEncoding::Utf8);
        dirty.insert_text("cambio local").unwrap();
        assert!(matches!(
            controller.prepare(
                &dirty,
                fixture.root.clone(),
                b"fuente.txt".to_vec(),
                IndexAction::Stage
            ),
            Err(IndexRequestError::DirtyBuffers)
        ));
        assert_eq!(controller.state(), IndexState::Idle);
        let ticket = controller
            .prepare(
                &clean,
                fixture.root.clone(),
                b"fuente.txt".to_vec(),
                IndexAction::Stage,
            )
            .unwrap();
        assert_eq!(controller.state(), IndexState::Preparing);
        assert!(controller.review().is_none());
        wait_delivery(&mut controller, &wake);
        assert_eq!(
            controller.state(),
            IndexState::Reviewing,
            "{:?}",
            controller.preparation_failure()
        );
        let review = controller.review().unwrap();
        assert_eq!(review.ticket, ticket);
        assert_eq!(review.plan().pair().after.bytes(), b"fixture CIDE\r\n");
        let index = fixture.root.join(".git/index");
        assert!(!index.exists());
        assert!(matches!(
            controller.apply(&clean, ticket + 1, &fixture.root),
            Err(IndexRequestError::StaleReview)
        ));
        assert!(matches!(
            controller.apply(&clean, ticket, &fixture.parent),
            Err(IndexRequestError::StaleReview)
        ));
        assert!(matches!(
            controller.apply(&dirty, ticket, &fixture.root),
            Err(IndexRequestError::DirtyBuffers)
        ));
        assert!(!index.exists() && controller.review().is_some());
        controller.apply(&clean, ticket, &fixture.root).unwrap();
        assert_eq!(controller.state(), IndexState::Applying);
        assert!(matches!(
            controller.discard_review(),
            Err(IndexRequestError::Busy)
        ));
        assert!(controller.publication().is_none());
        wait_delivery(&mut controller, &wake);
        let publication = controller.publication().unwrap();
        assert!(publication.result.is_ok(), "{:?}", publication.result);
        assert_eq!(publication.ticket, ticket);
        assert_eq!(publication.target.path, b"fuente.txt");
        assert!(fs::read(&index).unwrap().starts_with(b"DIRC"));
        assert_eq!(
            fs::read(fixture.root.join("fuente.txt")).unwrap(),
            b"fixture CIDE\r\n"
        );
        controller
            .prepare(
                &clean,
                fixture.root.clone(),
                b"fuente.txt".to_vec(),
                IndexAction::Unstage,
            )
            .unwrap();
        controller.discard_review().unwrap();
        wait_delivery(&mut controller, &wake);
        assert_eq!(controller.state(), IndexState::Published);
        assert!(controller.review().is_none());
        assert_eq!(controller.take_publication().unwrap().ticket, ticket);
        assert_eq!(controller.state(), IndexState::Idle);
    }
}
