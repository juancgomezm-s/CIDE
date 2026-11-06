//! Repository-scoped immutable Git reads. Workers never borrow editor documents.
//! One active operation, one latest metadata request and one completion; only
//! poll accepts results. Rendering may borrow accepted immutable read models.
use crate::{
    diff_model::{DiffLimits, DiffStatus, OwnedDiffModel},
    Cancellation, Wakeup,
};
pub use jc_git::local_pair::PairKind;
use jc_git::local_pair::{read_local_pair, HeadState, IndexToken, SourceIdentity};
use jc_git::path_history::read_path_history;
pub use jc_git::path_history::{PathHistoryCursor, PathHistoryEntry, PathHistoryPage};
use jc_git::revision::{read_file_pair, read_revision, RevisionFilePair};
pub use jc_git::revision::{Revision, RevisionEntry, RevisionFile};
pub use jc_git::{Change, CommitInfo, RepositoryInfo};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const SOURCE_BYTES: usize = 2 * 1024 * 1024;
const PAIR_BYTES: usize = 4 * 1024 * 1024;
const PATH_BYTES: usize = 4096;
const TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GitRevisionState {
    #[default]
    Idle,
    Queued,
    Running,
    Ready,
    Limited,
    Cancelled,
    Failed,
    TimedOut,
    Stale,
}
impl GitRevisionState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "Inactivo",
            Self::Queued => "En cola",
            Self::Running => "En curso",
            Self::Ready => "Listo",
            Self::Limited => "Limitado",
            Self::Cancelled => "Cancelado",
            Self::Failed => "Fallido",
            Self::TimedOut => "Tiempo agotado",
            Self::Stale => "Obsoleto",
        }
    }
}

/// The two Git sources remain immutable and independent of any editable buffer.
/// Missing entries keep None identities even though alignment consumes zero bytes.
#[derive(Debug)]
pub struct RevisionComparison {
    pub origin: ComparisonOrigin,
    pub path: Vec<u8>,
    pub before_oid: Option<String>,
    pub after_oid: Option<String>,
    pub model: OwnedDiffModel,
}

#[derive(Debug)]
pub enum ComparisonOrigin {
    Historical {
        commit: String,
        parent: Option<String>,
    },
    Local(Box<LocalComparisonOrigin>),
}
#[derive(Debug)]
pub struct LocalComparisonOrigin {
    pub kind: PairKind,
    pub index: IndexToken,
    pub head: Option<HeadState>,
    pub before: SourceIdentity,
    pub after: SourceIdentity,
}

impl ComparisonOrigin {
    pub fn sources_label(&self) -> Option<String> {
        fn source(s: &SourceIdentity) -> String {
            let name = match s.origin() {
                jc_git::local_pair::SourceOrigin::Head => "HEAD",
                jc_git::local_pair::SourceOrigin::Index => "Índice",
                jc_git::local_pair::SourceOrigin::Worktree => "Disco",
            };
            match s {
                SourceIdentity::Present { mode, .. } => format!("{name}: presente ({mode:o})"),
                SourceIdentity::Absent { .. } => format!("{name}: ausente"),
            }
        }
        match self {
            Self::Local(local) => Some(format!(
                "{} → {}",
                source(&local.before),
                source(&local.after)
            )),
            _ => None,
        }
    }
    pub fn label(&self) -> String {
        match self {
            Self::Historical { commit, parent } => format!(
                "{} → {}",
                parent.as_deref().unwrap_or("árbol vacío"),
                commit
            ),
            Self::Local(local) if local.kind == PairKind::Prepared => {
                "Preparados: HEAD → índice · bytes originales".into()
            }
            Self::Local(_) => "En disco: índice → disco · bytes originales".into(),
        }
    }
}
#[derive(Clone, PartialEq, Eq)]
enum Query {
    PathHistory {
        start: String,
        path: Vec<u8>,
        cursor: Option<PathHistoryCursor>,
    },
    Revision {
        commit: String,
    },
    HistoricalFile {
        commit: String,
        path: Vec<u8>,
    },
    LocalFile {
        kind: PairKind,
        path: Vec<u8>,
    },
}
impl Query {
    fn commit(&self) -> Option<&str> {
        match self {
            Self::PathHistory { start, .. } => Some(start),
            Self::Revision { commit } | Self::HistoricalFile { commit, .. } => Some(commit),
            _ => None,
        }
    }
    fn path(&self) -> Option<&[u8]> {
        match self {
            Self::HistoricalFile { path, .. }
            | Self::LocalFile { path, .. }
            | Self::PathHistory { path, .. } => Some(path),
            _ => None,
        }
    }
}
#[derive(Clone, PartialEq, Eq)]
struct Selection {
    root: PathBuf,
    query: Query,
}
impl Selection {
    fn valid(&self) -> bool {
        let bounded = match &self.query {
            Query::PathHistory {
                start,
                path,
                cursor,
            } => {
                start.capacity() <= 64
                    && path.capacity() <= PATH_BYTES
                    && cursor
                        .as_ref()
                        .is_none_or(|c| c.memory_bytes() <= PATH_BYTES * 4)
            }
            Query::Revision { commit } => commit.capacity() <= 64,
            Query::HistoricalFile { commit, path } => {
                commit.capacity() <= 64 && path.capacity() <= PATH_BYTES
            }
            Query::LocalFile { path, .. } => path.capacity() <= PATH_BYTES,
        };
        bounded
            && self.root.is_absolute()
            && self.root.as_os_str().len() <= PATH_BYTES
            && self.root.capacity() <= PATH_BYTES
            && self.query.commit().is_none_or(valid_oid)
            && self.query.path().is_none_or(|path| {
                !path.is_empty()
                    && path.len() <= PATH_BYTES
                    && !path.contains(&0)
                    && path
                        .split(|byte| *byte == b'/')
                        .all(|part| !part.is_empty() && part != b"." && part != b"..")
            })
    }
}
fn valid_oid(oid: &str) -> bool {
    [40, 64].contains(&oid.len())
        && oid.bytes().all(|byte| byte.is_ascii_hexdigit())
        && !oid.bytes().all(|byte| byte == b'0')
}

enum Outcome {
    PathHistory(PathHistoryPage),
    Revision(Revision),
    Comparison(RevisionComparison),
}
enum Failure {
    Provider(jc_git::Error),
    Limited,
    Cancelled,
    TimedOut,
    Panic,
    Identity,
}
type Provider = dyn Fn(&Selection, &Cancellation) -> Result<Outcome, Failure> + Send + Sync;
struct Request {
    generation: u64,
    selection: Selection,
}
struct Active {
    generation: u64,
    cancel: Cancellation,
}
struct Completion {
    generation: u64,
    selection: Selection,
    result: Result<Outcome, Failure>,
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
    recover: AtomicBool,
}

pub struct GitRevisionController {
    shared: Arc<Shared>,
    provider: Arc<Provider>,
    worker: Option<JoinHandle<()>>,
    generation: u64,
    selection: Option<Selection>,
    revision: Option<Revision>,
    path_history: Option<PathHistoryPage>,
    comparison: Option<RevisionComparison>,
    state: GitRevisionState,
}
impl GitRevisionController {
    /// Metadata only: no thread, repository discovery or source reads.
    pub fn new(wake: Wakeup) -> Self {
        Self::with_provider(wake, Arc::new(run_provider))
    }
    fn with_provider(wake: Wakeup, provider: Arc<Provider>) -> Self {
        Self {
            shared: Arc::new(Shared {
                queue: Mutex::new(Queue::default()),
                changed: Condvar::new(),
                wake,
                recover: AtomicBool::new(false),
            }),
            provider,
            worker: None,
            generation: 0,
            selection: None,
            revision: None,
            path_history: None,
            comparison: None,
            state: GitRevisionState::Idle,
        }
    }
    pub fn state(&self) -> GitRevisionState {
        self.state
    }
    pub fn revision(&self) -> Option<&Revision> {
        self.revision.as_ref()
    }
    pub fn path_history(&self) -> Option<&PathHistoryPage> {
        self.path_history.as_ref()
    }
    pub fn request_path_history(
        &mut self,
        root: PathBuf,
        start: String,
        path: Vec<u8>,
    ) -> Result<(), String> {
        self.request(Selection {
            root,
            query: Query::PathHistory {
                start,
                path,
                cursor: None,
            },
        })
    }
    pub fn request_path_history_next(&mut self, root: PathBuf) -> Result<(), String> {
        let page = self
            .path_history
            .as_ref()
            .ok_or("No hay historial de archivo aceptado")?;
        let cursor = page.next.clone().ok_or("No hay otra página de historial")?;
        let start = page.start.clone();
        let path = page.path.clone();
        self.request(Selection {
            root,
            query: Query::PathHistory {
                start,
                path,
                cursor: Some(cursor),
            },
        })
    }
    pub fn comparison(&self) -> Option<&RevisionComparison> {
        self.comparison.as_ref()
    }
    pub fn message(&self) -> &'static str {
        match self.state {
            GitRevisionState::Idle => "",
            GitRevisionState::Queued => "Consulta Git en cola",
            GitRevisionState::Running => "Consultando fuentes Git",
            GitRevisionState::Ready => "Consulta Git lista",
            GitRevisionState::Limited => "La consulta Git excedió sus límites",
            GitRevisionState::Cancelled => "Consulta Git cancelada",
            GitRevisionState::Failed => "No se pudieron comparar estas fuentes Git",
            GitRevisionState::TimedOut => "Se agotó el tiempo de consulta Git",
            GitRevisionState::Stale => "Cambiaron las fuentes; actualiza la consulta Git",
        }
    }
    pub fn ready(&self) -> bool {
        self.shared
            .queue
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .completion
            .is_some()
    }
    pub fn take_recovery(&self) -> bool {
        self.shared.recover.swap(false, Ordering::AcqRel)
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
        self.shared.changed.notify_all();
        self.comparison = None;
    }
    pub fn request_revision(&mut self, root: PathBuf, commit: String) -> Result<(), String> {
        self.request(Selection {
            root,
            query: Query::Revision { commit },
        })
    }
    pub fn request_file(
        &mut self,
        root: PathBuf,
        commit: String,
        path: Vec<u8>,
    ) -> Result<(), String> {
        self.request(Selection {
            root,
            query: Query::HistoricalFile { commit, path },
        })
    }
    pub fn request_local_file(
        &mut self,
        root: PathBuf,
        path: Vec<u8>,
        kind: PairKind,
    ) -> Result<(), String> {
        self.request(Selection {
            root,
            query: Query::LocalFile { kind, path },
        })
    }
    fn request(&mut self, mut selection: Selection) -> Result<(), String> {
        self.invalidate_work();
        let retain_path_history = self
            .selection
            .as_ref()
            .is_some_and(|current| current.root == selection.root)
            && matches!(&selection.query,Query::HistoricalFile{commit,path}
                if self.path_history.as_ref().is_some_and(|page|page.path==*path &&page.entries.iter().any(|e|e.commit.eq_ignore_ascii_case(commit))));
        if !retain_path_history {
            self.path_history = None;
        }
        let retain_revision = matches!(selection.query, Query::HistoricalFile { .. })
            && self.selection.as_ref().is_some_and(|current| {
                current.root == selection.root
                    && current
                        .query
                        .commit()
                        .zip(selection.query.commit())
                        .is_some_and(|(a, b)| a.eq_ignore_ascii_case(b))
            });
        if !retain_revision {
            self.revision = None;
        }
        self.selection = None;
        let Some(generation) = self.generation.checked_add(1) else {
            self.state = GitRevisionState::Failed;
            return Err("Se agotó la generación de consulta Git".into());
        };
        self.generation = generation;
        if !selection.valid() {
            self.revision = None;
            self.state = GitRevisionState::Failed;
            return Err("La consulta necesita raíz absoluta, revisión y ruta Git válidas".into());
        }
        match &mut selection.query {
            Query::Revision { commit }
            | Query::HistoricalFile { commit, .. }
            | Query::PathHistory { start: commit, .. } => commit.make_ascii_lowercase(),
            Query::LocalFile { .. } => {}
        }
        if self.worker.is_none() {
            let shared = self.shared.clone();
            let provider = self.provider.clone();
            match thread::Builder::new()
                .name("jc-git-revision".into())
                .spawn(move || worker(shared, provider))
            {
                Ok(worker) => self.worker = Some(worker),
                Err(_) => {
                    self.revision = None;
                    self.state = GitRevisionState::Failed;
                    return Err("No se pudo iniciar la consulta Git".into());
                }
            }
        }
        self.selection = Some(selection.clone());
        let mut queue = self
            .shared
            .queue
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        queue.generation = Some(generation);
        queue.pending = Some(Request {
            generation,
            selection,
        });
        drop(queue);
        self.state = GitRevisionState::Queued;
        self.shared.changed.notify_all();
        Ok(())
    }
    /// Metadata invalidation only. Never accepts a completion or discovers Git.
    pub fn invalidate_context(&mut self, root: Option<&Path>) -> bool {
        if self
            .selection
            .as_ref()
            .is_some_and(|selection| Some(selection.root.as_path()) != root)
        {
            self.invalidate_work();
            self.selection = None;
            self.revision = None;
            self.path_history = None;
            self.state = GitRevisionState::Stale;
            return true;
        }
        false
    }
    pub fn cancel(&mut self) {
        self.invalidate_work();
        self.revision = None;
        self.path_history = None;
        self.selection = None;
        self.state = GitRevisionState::Cancelled;
    }
    /// Sole delivery acceptance route. Call from the host's task notification.
    pub fn poll(&mut self, root: Option<&Path>) -> bool {
        if self.invalidate_context(root) {
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
                || self.selection.as_ref() != Some(&completion.selection)
                || root != Some(completion.selection.root.as_path())
            {
                return false;
            }
            match completion.result {
                Ok(Outcome::PathHistory(page))
                    if matches!(&completion.selection.query,
                    Query::PathHistory{start,path,..} if page.start==*start &&page.path==*path)
                        && page.memory_bytes() <= PAIR_BYTES =>
                {
                    self.path_history = Some(page);
                    self.state = GitRevisionState::Ready;
                }
                Ok(Outcome::Revision(revision))
                    if matches!(completion.selection.query, Query::Revision { .. })
                        && Some(revision.commit.as_str())
                            == completion.selection.query.commit() =>
                {
                    self.revision = Some(revision);
                    self.state = GitRevisionState::Ready;
                }
                Ok(Outcome::Comparison(comparison))
                    if comparison_matches(&comparison, &completion.selection.query) =>
                {
                    self.comparison = Some(comparison);
                    self.state = GitRevisionState::Ready;
                }
                Ok(_) => self.state = GitRevisionState::Failed,
                Err(failure) => self.state = failure_state(failure),
            }
            return true;
        }
        if running && self.state == GitRevisionState::Queued {
            self.state = GitRevisionState::Running;
            return true;
        }
        false
    }
}
impl Drop for GitRevisionController {
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

fn failure_state(failure: Failure) -> GitRevisionState {
    match failure {
        Failure::Provider(jc_git::Error::Cancelled) | Failure::Cancelled => {
            GitRevisionState::Cancelled
        }
        Failure::Provider(jc_git::Error::TimedOut) | Failure::TimedOut => {
            GitRevisionState::TimedOut
        }
        Failure::Provider(jc_git::Error::Stale) => GitRevisionState::Stale,
        Failure::Provider(jc_git::Error::Limited) | Failure::Limited => GitRevisionState::Limited,
        _ => GitRevisionState::Failed,
    }
}
fn limits() -> jc_git::Limits {
    jc_git::Limits {
        max_blob: SOURCE_BYTES,
        max_entries: 4096,
        max_path: PATH_BYTES,
        max_bytes: PAIR_BYTES,
        timeout: TIMEOUT,
    }
}
fn run_provider(selection: &Selection, cancel: &Cancellation) -> Result<Outcome, Failure> {
    if cancel.is_cancelled() {
        return Err(Failure::Cancelled);
    }
    let started = Instant::now();
    match &selection.query {
        Query::PathHistory {
            start,
            path,
            cursor,
        } => read_path_history(
            &selection.root,
            start,
            path,
            cursor.as_ref(),
            limits(),
            || cancel.is_cancelled(),
        )
        .map(Outcome::PathHistory)
        .map_err(Failure::Provider),
        Query::Revision { commit } => {
            read_revision(&selection.root, commit, limits(), || cancel.is_cancelled())
                .map(Outcome::Revision)
                .map_err(Failure::Provider)
        }
        Query::HistoricalFile { commit, path } => {
            let pair = read_file_pair(&selection.root, commit, path, limits(), || {
                cancel.is_cancelled()
            })
            .map_err(Failure::Provider)?;
            if pair.commit != *commit || pair.path != *path {
                return Err(Failure::Identity);
            }
            comparison(pair, cancel, started).map(Outcome::Comparison)
        }
        Query::LocalFile { kind, path } => {
            let pair = read_local_pair(&selection.root, path, *kind, limits(), || {
                cancel.is_cancelled()
            })
            .map_err(Failure::Provider)?;
            if pair.kind != *kind || pair.path != *path {
                return Err(Failure::Identity);
            }
            let (before_id, before) = pair.before.into_parts();
            let (after_id, after) = pair.after.into_parts();
            let before_oid = before_id.oid().map(str::to_owned);
            let after_oid = after_id.oid().map(str::to_owned);
            let origin = ComparisonOrigin::Local(Box::new(LocalComparisonOrigin {
                kind: pair.kind,
                index: pair.index,
                head: pair.head,
                before: before_id,
                after: after_id,
            }));
            build_comparison(
                pair.path, origin, before_oid, before, after_oid, after, cancel, started,
            )
            .map(Outcome::Comparison)
        }
    }
}
fn comparison_matches(c: &RevisionComparison, query: &Query) -> bool {
    query.path() == Some(c.path.as_slice())
        && match (&c.origin, query) {
            (
                ComparisonOrigin::Historical { commit, .. },
                Query::HistoricalFile {
                    commit: expected, ..
                },
            ) => commit == expected,
            (ComparisonOrigin::Local(local), Query::LocalFile { kind: expected, .. }) => {
                local.kind == *expected
            }
            _ => false,
        }
}

fn comparison(
    pair: RevisionFilePair,
    cancel: &Cancellation,
    started: Instant,
) -> Result<RevisionComparison, Failure> {
    let (before_oid, before) = pair
        .before
        .map_or((None, Vec::new()), |blob| (Some(blob.oid), blob.bytes));
    let (after_oid, after) = pair
        .after
        .map_or((None, Vec::new()), |blob| (Some(blob.oid), blob.bytes));
    build_comparison(
        pair.path,
        ComparisonOrigin::Historical {
            commit: pair.commit,
            parent: pair.parent,
        },
        before_oid,
        before,
        after_oid,
        after,
        cancel,
        started,
    )
}
#[allow(clippy::too_many_arguments)]
fn build_comparison(
    path: Vec<u8>,
    origin: ComparisonOrigin,
    before_oid: Option<String>,
    before: Vec<u8>,
    after_oid: Option<String>,
    after: Vec<u8>,
    cancel: &Cancellation,
    started: Instant,
) -> Result<RevisionComparison, Failure> {
    if before.len() > SOURCE_BYTES
        || after.len() > SOURCE_BYTES
        || before.capacity().saturating_add(after.capacity()) > PAIR_BYTES
    {
        return Err(Failure::Limited);
    }
    let before = Arc::new(before);
    let after = if before_oid.is_some() && before_oid == after_oid && *before == after {
        before.clone()
    } else {
        Arc::new(after)
    };
    let model = OwnedDiffModel::compute(
        before,
        after,
        DiffLimits {
            max_input_bytes: PAIR_BYTES,
            max_metadata_bytes: PAIR_BYTES,
            ..DiffLimits::default()
        },
        || cancel.is_cancelled() || started.elapsed() >= TIMEOUT,
    );
    match model.status() {
        DiffStatus::Exact => Ok(RevisionComparison {
            origin,
            path,
            before_oid,
            after_oid,
            model,
        }),
        DiffStatus::Limited(_) => Err(Failure::Limited),
        DiffStatus::Cancelled if cancel.is_cancelled() => Err(Failure::Cancelled),
        DiffStatus::Cancelled => Err(Failure::TimedOut),
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
            let request = queue.pending.take().expect("queued revision request");
            let cancel = Cancellation::new(Duration::from_secs(6));
            queue.active = Some(Active {
                generation: request.generation,
                cancel: cancel.clone(),
            });
            (request, cancel)
        };
        shared.wake.signal();
        let result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            provider(&request.selection, &cancel)
        })) {
            Ok(result) => result,
            Err(_) => {
                shared.recover.store(true, Ordering::Release);
                Err(Failure::Panic)
            }
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
                selection: request.selection,
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
    use crate::diff_model::DiffSide;
    use jc_git::revision::RevisionBlob;
    use std::sync::{atomic::AtomicUsize, mpsc};
    fn history_page(selection: &Selection) -> PathHistoryPage {
        PathHistoryPage {
            repository: jc_git::Identity {
                root: selection.root.clone(),
                git_dir: selection.root.join(".git"),
            },
            start: selection.query.commit().unwrap().into(),
            path: selection.query.path().unwrap().to_vec(),
            entries: vec![PathHistoryEntry {
                commit: selection.query.commit().unwrap().into(),
                parent: None,
                summary: b"Creacion".to_vec(),
                author: b"Fixture".to_vec(),
                change: Change::Added,
            }],
            next: None,
            scanned: 1,
            shallow: false,
            metadata_limited: false,
        }
    }
    #[test]
    fn path_history_only_poll_accepts_and_selected_pair_retains_bounded_page() {
        let wake = Wakeup::default();
        let mut c = GitRevisionController::with_provider(
            wake.clone(),
            Arc::new(|s, cancel| match &s.query {
                Query::PathHistory { .. } => Ok(Outcome::PathHistory(history_page(s))),
                Query::HistoricalFile { commit, path } => comparison(
                    RevisionFilePair {
                        repository: jc_git::Identity {
                            root: s.root.clone(),
                            git_dir: s.root.join(".git"),
                        },
                        commit: commit.clone(),
                        parent: None,
                        path: path.clone(),
                        before: None,
                        after: None,
                    },
                    cancel,
                    Instant::now(),
                )
                .map(Outcome::Comparison),
                _ => Err(Failure::Identity),
            }),
        );
        c.request_path_history(root("path"), oid('a'), b"src/archivo.p".to_vec())
            .unwrap();
        wait_ready(&c, &wake);
        assert!(c.path_history().is_none());
        c.poll(Some(&root("path")));
        assert_eq!(c.path_history().unwrap().entries.len(), 1);
        assert!(c.request_path_history_next(root("path")).is_err());
        c.request_file(root("path"), oid('a'), b"src/archivo.p".to_vec())
            .unwrap();
        assert!(c.path_history().is_some());
        wait_ready(&c, &wake);
        c.poll(Some(&root("path")));
        assert!(c.comparison().is_some());
        assert!(c.path_history().is_some());
        assert!(c.invalidate_context(Some(&root("different"))));
        assert!(c.path_history().is_none());
        assert!(c.comparison().is_none());
    }
    #[test]
    fn path_history_rejects_wrong_delivered_path_and_cancel_drops_page() {
        let wake = Wakeup::default();
        let mut c = GitRevisionController::with_provider(
            wake.clone(),
            Arc::new(|s, _| {
                let mut p = history_page(s);
                if p.start == oid('b') {
                    p.path = b"wrong.p".to_vec();
                }
                Ok(Outcome::PathHistory(p))
            }),
        );
        c.request_path_history(root("path"), oid('b'), b"file.p".to_vec())
            .unwrap();
        wait_ready(&c, &wake);
        c.poll(Some(&root("path")));
        assert_eq!(c.state(), GitRevisionState::Failed);
        assert!(c.path_history().is_none());
        c.request_path_history(root("path"), oid('a'), b"file.p".to_vec())
            .unwrap();
        wait_ready(&c, &wake);
        c.cancel();
        assert!(!c.poll(Some(&root("path"))));
        assert!(c.path_history().is_none());
    }

    fn root(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join("cide-revision-delivery-fixture")
            .join(name)
    }
    fn oid(byte: char) -> String {
        byte.to_string().repeat(40)
    }
    fn revision(selection: &Selection) -> Outcome {
        Outcome::Revision(Revision {
            repository: jc_git::Identity {
                root: selection.root.clone(),
                git_dir: selection.root.join(".git"),
            },
            commit: selection.query.commit().expect("historical query").into(),
            parents: Vec::new(),
            parent: None,
            files: Vec::new(),
            limited: false,
        })
    }
    fn wait_ready(controller: &GitRevisionController, wake: &Wakeup) {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let ticket = wake.ticket();
            if controller.ready() {
                return;
            }
            wake.wait(
                ticket,
                Some(
                    deadline
                        .checked_duration_since(Instant::now())
                        .expect("revision delivery timeout"),
                ),
            );
        }
    }
    struct Release(Arc<AtomicBool>);
    impl Drop for Release {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
    #[test]
    fn construction_and_invalid_metadata_do_not_admit_a_worker_or_io() {
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = calls.clone();
        let mut controller = GitRevisionController::with_provider(
            Wakeup::default(),
            Arc::new(move |s, _| {
                counted.fetch_add(1, Ordering::SeqCst);
                Ok(revision(s))
            }),
        );
        assert!(controller
            .request_revision(PathBuf::from("relative"), oid('a'))
            .is_err());
        assert!(controller
            .request_revision(root("valid"), "HEAD".into())
            .is_err());
        assert!(controller
            .request_revision(root("valid"), oid('0'))
            .is_err());
        assert!(controller
            .request_file(root("valid"), oid('a'), b"../escape".to_vec())
            .is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(controller.worker.is_none());
        controller.generation = u64::MAX;
        assert!(controller
            .request_revision(root("valid"), oid('a'))
            .is_err());
        assert_eq!(controller.state(), GitRevisionState::Failed);
    }
    #[test]
    fn latest_pending_replaces_old_requests_and_only_poll_accepts_the_result() {
        let wake = Wakeup::default();
        let release_flag = Arc::new(AtomicBool::new(false));
        let gate = release_flag.clone();
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = calls.clone();
        let (entered, observed) = mpsc::channel();
        let mut controller = GitRevisionController::with_provider(
            wake.clone(),
            Arc::new(move |s, _| {
                if counted.fetch_add(1, Ordering::SeqCst) == 0 {
                    entered.send(()).unwrap();
                    while !gate.load(Ordering::Acquire) {
                        thread::yield_now();
                    }
                }
                Ok(revision(s))
            }),
        );
        let release = Release(release_flag);
        controller.request_revision(root("one"), oid('a')).unwrap();
        observed.recv_timeout(Duration::from_secs(3)).unwrap();
        controller.request_revision(root("one"), oid('b')).unwrap();
        controller.request_revision(root("one"), oid('c')).unwrap();
        release.0.store(true, Ordering::Release);
        wait_ready(&controller, &wake);
        assert!(controller.revision().is_none());
        assert!(controller.poll(Some(&root("one"))));
        assert_eq!(controller.revision().unwrap().commit, oid('c'));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(controller.state(), GitRevisionState::Ready);
        assert!(!controller.poll(Some(&root("one"))));
    }
    #[test]
    fn root_invalidation_before_delivery_and_cancel_do_not_revive_a_snapshot() {
        let wake = Wakeup::default();
        let mut controller =
            GitRevisionController::with_provider(wake.clone(), Arc::new(|s, _| Ok(revision(s))));
        controller.request_revision(root("one"), oid('a')).unwrap();
        wait_ready(&controller, &wake);
        assert!(controller.invalidate_context(Some(&root("two"))));
        assert!(controller.revision().is_none());
        assert!(!controller.ready());
        assert!(!controller.poll(Some(&root("one"))));
        assert_eq!(controller.state(), GitRevisionState::Stale);
        controller.request_revision(root("one"), oid('a')).unwrap();
        wait_ready(&controller, &wake);
        controller.cancel();
        assert!(!controller.ready());
        assert!(!controller.poll(Some(&root("one"))));
        assert_eq!(controller.state(), GitRevisionState::Cancelled);
        assert!(controller.revision().is_none());
    }
    #[test]
    fn missing_and_empty_sources_remain_distinct_and_raw_bytes_are_preserved() {
        let raw = b"\xef\xbb\xbf\xff\r\nraw\r".to_vec();
        let pair = RevisionFilePair {
            repository: jc_git::Identity {
                root: root("one"),
                git_dir: root("one").join(".git"),
            },
            commit: oid('a'),
            parent: None,
            path: b"raw.bin".to_vec(),
            before: None,
            after: Some(RevisionBlob {
                oid: oid('b'),
                mode: 0o100644,
                bytes: raw.clone(),
            }),
        };
        let cancel = Cancellation::new(Duration::from_secs(6));
        let result = comparison(pair, &cancel, Instant::now())
            .unwrap_or_else(|_| panic!("bounded comparison"));
        assert!(result.before_oid.is_none());
        assert_eq!(result.after_oid, Some(oid('b')));
        assert!(result.model.source_bytes(DiffSide::Left).is_empty());
        assert_eq!(result.model.source_bytes(DiffSide::Right), raw);
        let pair = RevisionFilePair {
            repository: jc_git::Identity {
                root: root("one"),
                git_dir: root("one").join(".git"),
            },
            commit: oid('a'),
            parent: Some(oid('c')),
            path: b"empty.txt".to_vec(),
            before: Some(RevisionBlob {
                oid: oid('b'),
                mode: 0o100644,
                bytes: Vec::new(),
            }),
            after: None,
        };
        let result = comparison(pair, &cancel, Instant::now())
            .unwrap_or_else(|_| panic!("empty comparison"));
        assert_eq!(result.before_oid, Some(oid('b')));
        assert!(result.after_oid.is_none());
        assert_eq!(result.model.status(), DiffStatus::Exact);
    }
    #[test]
    fn requesting_a_file_keeps_its_listing_but_another_commit_discards_it() {
        let wake = Wakeup::default();
        let mut controller = GitRevisionController::with_provider(
            wake.clone(),
            Arc::new(|s, _| {
                if s.query.path().is_none() {
                    Ok(revision(s))
                } else {
                    Err(Failure::Limited)
                }
            }),
        );
        controller.request_revision(root("one"), oid('a')).unwrap();
        wait_ready(&controller, &wake);
        controller.poll(Some(&root("one")));
        controller
            .request_file(root("one"), oid('a'), b"file".to_vec())
            .unwrap();
        assert_eq!(controller.revision().unwrap().commit, oid('a'));
        wait_ready(&controller, &wake);
        controller.poll(Some(&root("one")));
        assert_eq!(controller.state(), GitRevisionState::Limited);
        assert_eq!(controller.revision().unwrap().commit, oid('a'));
        controller
            .request_file(root("one"), oid('b'), b"file".to_vec())
            .unwrap();
        assert!(controller.revision().is_none());
    }
    #[test]
    fn provider_panic_signals_recovery_and_the_worker_can_serve_again() {
        let wake = Wakeup::default();
        let mut controller = GitRevisionController::with_provider(
            wake.clone(),
            Arc::new(|s, _| {
                if s.query.commit() == Some(oid('a').as_str()) {
                    panic!("owned fixture panic");
                }
                Ok(revision(s))
            }),
        );
        controller.request_revision(root("one"), oid('a')).unwrap();
        wait_ready(&controller, &wake);
        controller.poll(Some(&root("one")));
        assert!(controller.take_recovery());
        assert!(!controller.take_recovery());
        assert_eq!(controller.state(), GitRevisionState::Failed);
        controller.request_revision(root("one"), oid('b')).unwrap();
        wait_ready(&controller, &wake);
        controller.poll(Some(&root("one")));
        assert_eq!(controller.state(), GitRevisionState::Ready);
    }
    #[test]
    fn switching_to_local_query_drops_history_and_surfaces_changed_sources() {
        let wake = Wakeup::default();
        let mut controller = GitRevisionController::with_provider(
            wake.clone(),
            Arc::new(|s, _| match s.query {
                Query::LocalFile {
                    kind: PairKind::Prepared,
                    ..
                } => Err(Failure::Provider(jc_git::Error::Stale)),
                Query::LocalFile {
                    kind: PairKind::Worktree,
                    ..
                } => Err(Failure::Provider(jc_git::Error::Unmerged)),
                _ => Ok(revision(s)),
            }),
        );
        let root = root("local-swap");
        controller.request_revision(root.clone(), oid('a')).unwrap();
        wait_ready(&controller, &wake);
        controller.poll(Some(&root));
        assert!(controller.revision().is_some());
        controller
            .request_local_file(root.clone(), b"file.p".to_vec(), PairKind::Prepared)
            .unwrap();
        assert!(controller.revision().is_none());
        assert!(controller.comparison().is_none());
        wait_ready(&controller, &wake);
        controller.poll(Some(&root));
        assert_eq!(controller.state(), GitRevisionState::Stale);
        controller
            .request_local_file(root.clone(), b"file.p".to_vec(), PairKind::Worktree)
            .unwrap();
        wait_ready(&controller, &wake);
        controller.poll(Some(&root));
        assert_eq!(controller.state(), GitRevisionState::Failed);
        assert!(controller.comparison().is_none());
    }
}
