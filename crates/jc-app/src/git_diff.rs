//! Versioned Git comparison coordinated through the existing F3 JobSystem.
//! Request/tick/accept use metadata only; worker run owns all path I/O and diff.
use crate::{
    diff_model::{DiffLimits, DiffSide, DiffStatus, OwnedDiffModel},
    git_read::{GitBlobIdentity, GitBlobSource, GitReadError, GitReadLimits, GitReader},
    Cancellation, CompletedTask, JobId, JobKey, JobSystem, SubmitError, TaskFailure, TaskOutput,
    TaskSnapshot, TaskStamp, TaskStatus, Workbench,
};
use jc_text::TextEncoding;
use std::{
    path::{Component, Path, PathBuf},
    sync::Arc,
};

/// Reserved for the one GitDiffController bound to this Workbench/JobSystem.
pub const GIT_DIFF_JOB_KEY: JobKey = JobKey(0x4a43_4744_4946_4601);
const SNAPSHOT_LIMIT: usize = 2 * 1024 * 1024;
const PATH_LIMIT: usize = 4096;
const ERROR_LIMIT: usize = 512;
const READ_TIMEOUT: &str = "Se agotó el tiempo de lectura del blob Git";
const READ_LIMIT: &str = "El blob Git excede los límites de comparación";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitDiffOptions {
    pub root: PathBuf,
    /// Absolute launch context for a FileSession whose stored path is relative.
    pub cwd: PathBuf,
    pub source: GitBlobSource,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GitDiffState {
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
impl GitDiffState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "Inactivo",
            Self::Queued => "En cola",
            Self::Running => "En curso",
            Self::Ready => "Listo",
            Self::Limited => "Limitado",
            Self::Cancelled => "Cancelado",
            Self::Stale => "Obsoleto",
            Self::Failed => "Fallido",
            Self::TimedOut => "Tiempo agotado",
        }
    }
}
#[derive(Debug)]
pub struct GitDiffResult {
    pub simplified: bool,
    pub identity: GitBlobIdentity,
    pub model: OwnedDiffModel,
}
#[derive(Debug)]
pub struct GitDiffReply {
    pub generation: u64,
    pub stamp: TaskStamp,
    pub result: Result<GitDiffResult, String>,
}
impl GitDiffReply {
    pub fn memory_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + match &self.result {
                Ok(result) => {
                    result.model.memory_bytes()
                        + result.identity.repository.root.capacity()
                        + result.identity.repository.git_dir.capacity()
                        + result.identity.path.capacity()
                        + result.identity.blob.memory_bytes()
                        + result
                            .identity
                            .commit
                            .as_ref()
                            .map_or(0, |oid| oid.memory_bytes())
                }
                Err(error) => error.capacity(),
            }
    }
}
#[derive(Debug, Clone)]
pub(crate) struct GitDiffWork {
    pub generation: u64,
    pub stamp: TaskStamp,
    pub options: GitDiffOptions,
    pub document_path: PathBuf,
}
#[derive(Debug, Clone)]
struct Intent {
    generation: u64,
    stamp: TaskStamp,
    document_path: PathBuf,
}
impl Intent {
    fn from_work(work: &GitDiffWork) -> Self {
        Self {
            generation: work.generation,
            stamp: work.stamp,
            document_path: work.document_path.clone(),
        }
    }
    fn current(&self, app: &Workbench) -> bool {
        current(self.stamp, &self.document_path, app)
    }
}
fn current(stamp: TaskStamp, path: &Path, app: &Workbench) -> bool {
    app.task_stamp_is_current(stamp)
        && app.active_document().is_some_and(|document| {
            document.id == stamp.document && document.session.path() == path
        })
        && app
            .panes()
            .view(app.panes().active())
            .and_then(|view| view.view_id())
            == stamp.view
}
#[derive(Debug)]
struct Flight {
    id: JobId,
    intent: Intent,
    discard: bool,
}

#[derive(Debug, Default)]
pub struct GitDiffController {
    generation: u64,
    in_flight: Option<Flight>,
    pending: Option<GitDiffWork>,
    accepted: Option<Intent>,
    result: Option<GitDiffResult>,
    selected: usize,
    state: GitDiffState,
    message: String,
}
impl GitDiffController {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn state(&self) -> GitDiffState {
        self.state
    }
    pub fn message(&self) -> &str {
        &self.message
    }
    /// Valid at the latest tick/accept. select_hunk revalidates before navigation.
    pub fn result(&self) -> Option<&GitDiffResult> {
        self.result.as_ref()
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    fn status(&mut self, state: GitDiffState, message: impl Into<String>) -> bool {
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
        if let Some(flight) = &mut self.in_flight {
            flight.discard = true;
            jobs.cancel(GIT_DIFF_JOB_KEY);
        }
        self.pending = None;
        self.discard_result();
    }
    pub fn cancel(&mut self, jobs: &mut JobSystem) {
        self.invalidate(jobs);
        self.status(GitDiffState::Cancelled, "Comparación Git cancelada");
    }
    pub fn clear(&mut self, jobs: &mut JobSystem) {
        self.invalidate(jobs);
        self.status(GitDiffState::Idle, "");
    }
    pub fn request(
        &mut self,
        app: &Workbench,
        jobs: &mut JobSystem,
        options: GitDiffOptions,
    ) -> Result<(), String> {
        if self.generation == u64::MAX {
            self.invalidate(jobs);
            self.status(
                GitDiffState::Failed,
                "Se agotó la generación de comparación Git",
            );
            return Err(self.message.clone());
        }
        self.invalidate(jobs);
        let request = (|| {
            let document = app.active_document().ok_or("No hay documento activo")?;
            let encoding = document
                .session
                .encoding()
                .ok_or("Elige la codificación del documento antes de comparar")?;
            let view = app
                .panes()
                .view(app.panes().active())
                .and_then(|view| view.view_id())
                .ok_or("No hay vista de editor activa")?;
            if document.session.document().len() > SNAPSHOT_LIMIT {
                return Err(READ_LIMIT);
            }
            if [
                options.root.as_path(),
                options.cwd.as_path(),
                document.session.path(),
            ]
            .iter()
            .any(|path| path.as_os_str().len() > PATH_LIMIT)
            {
                return Err("La ruta de comparación Git excede el límite de metadatos");
            }
            if !options.root.is_absolute() || !options.cwd.is_absolute() {
                return Err(
                    "La raíz del repositorio y el directorio de inicio deben ser absolutos",
                );
            }
            let stamp = TaskStamp {
                document: document.id,
                revision: document.session.document().revision(),
                encoding,
                view: Some(view),
            };
            Ok(GitDiffWork {
                generation: self.generation,
                stamp,
                options,
                document_path: document.session.path().to_owned(),
            })
        })();
        match request {
            Ok(work) => self.pending = Some(work),
            Err(message) => {
                let state = if message == READ_LIMIT
                    || message == "La ruta de comparación Git excede el límite de metadatos"
                {
                    GitDiffState::Limited
                } else {
                    GitDiffState::Failed
                };
                self.status(state, message);
                return Err(message.into());
            }
        }
        self.status(GitDiffState::Queued, "Comparación Git en cola");
        self.tick(app, jobs);
        if matches!(
            self.state,
            GitDiffState::Failed | GitDiffState::Limited | GitDiffState::Stale
        ) {
            Err(self.message.clone())
        } else {
            Ok(())
        }
    }
    /// Metadata-only invalidation; no snapshot admission or completion acceptance.
    /// Safe before preparing a read-only frame.
    pub fn validate_context(&mut self, app: &Workbench, jobs: &mut JobSystem) -> bool {
        let mut changed = false;
        if self
            .accepted
            .as_ref()
            .is_some_and(|intent| !intent.current(app))
        {
            self.discard_result();
            changed |= self.status(
                GitDiffState::Stale,
                "Cambió el documento/vista; actualiza la comparación Git",
            );
        }
        if self.in_flight.as_ref().is_some_and(|flight| {
            !flight.discard
                && flight.intent.generation == self.generation
                && !flight.intent.current(app)
        }) {
            jobs.cancel(GIT_DIFF_JOB_KEY);
            changed |= self.status(
                GitDiffState::Stale,
                "Cambió el documento/vista; actualiza la comparación Git",
            );
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|work| !current(work.stamp, &work.document_path, app))
        {
            self.pending = None;
            changed |= self.status(GitDiffState::Stale, "La comparación en cola quedó obsoleta");
        }
        changed
    }
    /// May admit one bounded snapshot; never drains provider completions.
    /// Call on the application event path, never while preparing/drawing a frame.
    pub fn tick(&mut self, app: &Workbench, jobs: &mut JobSystem) -> bool {
        let mut changed = self.validate_context(app, jobs);
        if self.in_flight.is_some() {
            return changed;
        }
        let Some(work) = self.pending.as_ref() else {
            return changed;
        };
        let intent = Intent::from_work(work);
        match jobs.submit_git_diff(app, GIT_DIFF_JOB_KEY, work.clone()) {
            Ok(id) => {
                self.pending = None;
                self.in_flight = Some(Flight {
                    id,
                    intent,
                    discard: false,
                });
                changed |= self.status(
                    GitDiffState::Running,
                    "Leyendo captura inmutable Git y comparando",
                );
            }
            Err(SubmitError::Busy) => {
                changed |= self.status(
                    GitDiffState::Queued,
                    "Esperando capacidad de tareas de fondo",
                );
            }
            Err(error) => {
                self.pending = None;
                let state = if error == SubmitError::SnapshotLimit {
                    GitDiffState::Limited
                } else {
                    GitDiffState::Failed
                };
                changed |= self.status(state, format!("Admisión de comparación Git: {error}"));
            }
        }
        changed
    }
    /// True means this completion belongs to the controller, including discarded
    /// old generations. Other job outputs remain untouched for their consumer.
    pub fn accept(&mut self, app: &Workbench, task: &mut CompletedTask) -> bool {
        if task.key != GIT_DIFF_JOB_KEY {
            return false;
        }
        let reply = task.output.take_git_diff();
        if self
            .in_flight
            .as_ref()
            .is_none_or(|flight| flight.id != task.id)
        {
            return true;
        }
        let flight = self.in_flight.take().unwrap();
        if flight.discard || flight.intent.generation != self.generation {
            return true;
        }
        if !flight.intent.current(app) || task.stamp != flight.intent.stamp {
            self.discard_result();
            self.status(
                GitDiffState::Stale,
                "El fuente de comparación Git cambió antes de su entrega",
            );
            return true;
        }
        if task.status != TaskStatus::Completed {
            let (state, message) = match task.status {
                TaskStatus::Cancelled => (GitDiffState::Cancelled, "Comparación Git cancelada"),
                TaskStatus::TimedOut => (
                    GitDiffState::TimedOut,
                    "Se agotó el tiempo de comparación Git",
                ),
                TaskStatus::LimitExceeded => (
                    GitDiffState::Limited,
                    "La comparación Git excedió sus cuotas",
                ),
                TaskStatus::InvalidRange => (
                    GitDiffState::Stale,
                    "La captura de comparación Git dejó de ser válida",
                ),
                TaskStatus::Panicked => (
                    GitDiffState::Failed,
                    "La tarea de comparación Git sufrió un fallo",
                ),
                _ => (GitDiffState::Failed, "Falló la tarea de comparación Git"),
            };
            self.status(state, message);
            return true;
        }
        if !task.current || !task.applicable {
            self.status(
                GitDiffState::Stale,
                "La entrega de comparación Git ya no está vigente",
            );
            return true;
        }
        let Some(reply) = reply.filter(|reply| {
            reply.generation == self.generation && reply.stamp == flight.intent.stamp
        }) else {
            self.status(
                GitDiffState::Failed,
                "La comparación Git devolvió una identidad de entrega inválida",
            );
            return true;
        };
        match reply.result {
            Ok(result) => match result.model.status() {
                DiffStatus::Exact => {
                    let message = if result.simplified {
                        "Comparación completa simplificada: se agrupan los cambios para limitar memoria"
                    } else {
                        "Comparación de captura Git lista"
                    };
                    self.selected = 0;
                    self.accepted = Some(flight.intent);
                    self.result = Some(result);
                    self.status(GitDiffState::Ready, message);
                }
                DiffStatus::Limited(limit) => {
                    self.status(
                        GitDiffState::Limited,
                        format!("Límite de comparación Git: {}", limit.label()),
                    );
                }
                DiffStatus::Cancelled => {
                    self.status(GitDiffState::Cancelled, "Comparación Git cancelada");
                }
            },
            Err(message) => {
                let state = if message == READ_TIMEOUT {
                    GitDiffState::TimedOut
                } else if message == READ_LIMIT {
                    GitDiffState::Limited
                } else {
                    GitDiffState::Failed
                };
                self.status(state, message);
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
    pub fn select_hunk(&mut self, index: usize, app: &mut Workbench) -> Result<(), String> {
        if self.state != GitDiffState::Ready {
            return Err("No hay comparación Git vigente".into());
        }
        let Some(intent) = self
            .accepted
            .as_ref()
            .filter(|intent| intent.generation == self.generation && intent.current(app))
        else {
            self.discard_result();
            self.status(
                GitDiffState::Stale,
                "Cambió el documento/vista; actualiza la comparación Git",
            );
            return Err(self.message.clone());
        };
        let result = self
            .result
            .as_ref()
            .ok_or("No hay resultado de comparación Git")?;
        let hunk = result
            .model
            .hunks()
            .nth(index)
            .ok_or("No existe ese bloque Git")?;
        let mut offset = result
            .model
            .line_byte_range(DiffSide::Right, hunk.right.start)
            .map_or(result.model.source_bytes(DiffSide::Right).len(), |range| {
                range.start
            });
        if offset == 0
            && intent.stamp.encoding == TextEncoding::Utf8
            && result
                .model
                .source_bytes(DiffSide::Right)
                .starts_with(&[0xef, 0xbb, 0xbf])
        {
            offset = 3;
        }
        let tab = app
            .document_by_id(intent.stamp.document)
            .ok_or("Se cerró el documento de la comparación")?
            .tab_id;
        if !app.jump_to_location(tab, offset) {
            return Err("El bloque Git no es una frontera textual válida".into());
        }
        self.selected = index;
        Ok(())
    }
}

fn bounded_error(message: impl ToString) -> String {
    let mut message = message.to_string();
    if message.len() > ERROR_LIMIT {
        let mut end = ERROR_LIMIT;
        while !message.is_char_boundary(end) {
            end -= 1;
        }
        message.truncate(end);
        message.shrink_to_fit();
    }
    message
}
fn resolve_path(work: &GitDiffWork) -> Result<(PathBuf, PathBuf), String> {
    let root = work
        .options
        .root
        .canonicalize()
        .map_err(|e| bounded_error(format!("Raíz del repositorio: {e}")))?;
    let requested = if work.document_path.is_absolute() {
        work.document_path.clone()
    } else {
        work.options.cwd.join(&work.document_path)
    };
    let resolved = match requested.canonicalize() {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let name = requested
                .file_name()
                .ok_or("La ruta del documento no tiene nombre de archivo")?;
            let parent = requested
                .parent()
                .ok_or("La ruta del documento no tiene directorio padre")?
                .canonicalize()
                .map_err(|e| bounded_error(format!("Directorio padre del documento: {e}")))?;
            parent.join(name)
        }
        Err(error) => return Err(bounded_error(format!("Ruta del documento: {error}"))),
    };
    let path = resolved
        .strip_prefix(&root)
        .map_err(|_| "El documento está fuera de la raíz del repositorio")?;
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("Ruta relativa del documento inválida".into());
    }
    Ok((root, path.to_owned()))
}
fn compute(
    work: &GitDiffWork,
    snapshot: &TaskSnapshot,
    cancel: &Cancellation,
) -> Result<GitDiffResult, String> {
    let (root, path) = resolve_path(work)?;
    if cancel.is_cancelled() {
        return Err("Comparación Git cancelada".into());
    }
    let reader = GitReader::new().map_err(|e| bounded_error(format!("Motor Git: {e}")))?;
    let blob = reader
        .read_blob(
            &root,
            work.options.source,
            &path,
            GitReadLimits::default(),
            || cancel.is_cancelled(),
        )
        .map_err(|error| match error {
            GitReadError::TimedOut => READ_TIMEOUT.into(),
            GitReadError::OutputLimit(_) => READ_LIMIT.into(),
            other => bounded_error(other),
        })?;
    let (identity, bytes) = blob.into_parts();
    let (model, simplified) = OwnedDiffModel::compute_display(
        Arc::new(bytes),
        snapshot.shared_bytes(),
        DiffLimits {
            max_input_bytes: 4 * 1024 * 1024,
            max_metadata_bytes: 4 * 1024 * 1024,
            ..DiffLimits::default()
        },
        || cancel.is_cancelled(),
    );
    Ok(GitDiffResult {
        identity,
        model,
        simplified,
    })
}
pub(crate) fn run(
    work: GitDiffWork,
    snapshot: &TaskSnapshot,
    cancel: &Cancellation,
    output: &mut TaskOutput,
) -> Result<(), TaskFailure> {
    if snapshot.stamp != work.stamp
        || snapshot.range.start != 0
        || snapshot.range.len() != snapshot.bytes().len()
    {
        return Err(TaskFailure::InvalidRange);
    }
    if snapshot.bytes().len() > SNAPSHOT_LIMIT {
        return Err(TaskFailure::LimitExceeded);
    }
    if cancel.is_cancelled() {
        return Err(TaskFailure::Cancelled);
    }
    let result = compute(&work, snapshot, cancel);
    if cancel.is_cancelled() {
        return Err(TaskFailure::Cancelled);
    }
    output.set_git_diff(GitDiffReply {
        generation: work.generation,
        stamp: work.stamp,
        result,
    })
}
