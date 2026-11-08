//! Shared draft, review and background publication for the two frontends.
use crate::{edit_field, FieldEdit, Wakeup, Workbench};
pub use jc_git::commit_write::AmendTarget;
pub use jc_git::commit_write::MESSAGE_BYTES;
use jc_git::commit_write::{apply_commit, prepare_amend, prepare_commit, CommitPlan};
pub use jc_git::ref_write::RefKind;
use jc_git::ref_write::{apply_ref, prepare_ref, RefPlan, REF_NAME_BYTES};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

const LIMITS: jc_git::Limits = jc_git::Limits {
    max_blob: 2 * 1024 * 1024,
    max_entries: 8192,
    max_path: 4096,
    max_bytes: 4 * 1024 * 1024,
    timeout: Duration::from_secs(10),
};

enum Completion {
    Review(Result<WritePlan, jc_git::Error>),
    Applied(Result<String, jc_git::Error>),
    Panic,
}
enum WritePlan {
    Commit(Box<CommitPlan>),
    Reference(Box<RefPlan>),
}

struct Job {
    result: Arc<Mutex<Option<Completion>>>,
    thread: JoinHandle<()>,
    cancelled: Arc<AtomicBool>,
    applying: bool,
}

pub struct CommitView {
    root: PathBuf,
    pub replaces: Option<AmendTarget>,
    pub reference: Option<(RefKind, String)>,
    pub message: String,
    pub cursor: usize,
    pub first: usize,
    pub error: Option<String>,
    input_token: Arc<()>,
    review: Option<WritePlan>,
}

impl CommitView {
    pub fn title(&self) -> &'static str {
        if let Some((kind, _)) = self.reference {
            match kind {
                RefKind::Branch => "Crear rama",
                RefKind::Tag => "Crear etiqueta",
            }
        } else if self.replaces.is_some() {
            "Modificar último commit"
        } else {
            "Crear commit"
        }
    }
    pub fn review(&self) -> Option<&CommitPlan> {
        match &self.review {
            Some(WritePlan::Commit(plan)) => Some(plan),
            _ => None,
        }
    }
    pub fn reference_review(&self) -> Option<&RefPlan> {
        match &self.review {
            Some(WritePlan::Reference(plan)) => Some(plan),
            _ => None,
        }
    }
    pub fn reviewing(&self) -> bool {
        self.review.is_some()
    }
    fn input_limit(&self) -> usize {
        if self.reference.is_some() {
            REF_NAME_BYTES
        } else {
            MESSAGE_BYTES - 1
        }
    }
}

pub struct CommitController {
    wake: Wakeup,
    view: Option<CommitView>,
    job: Option<Job>,
    notice: Option<(PathBuf, String)>,
}

impl CommitController {
    pub fn new(wake: Wakeup) -> Self {
        Self {
            wake,
            view: None,
            job: None,
            notice: None,
        }
    }

    pub fn view(&self) -> Option<&CommitView> {
        self.view.as_ref()
    }
    pub fn notice(&self, root: Option<&Path>) -> Option<&str> {
        self.notice
            .as_ref()
            .filter(|(path, _)| Some(path.as_path()) == root)
            .map(|(_, notice)| notice.as_str())
    }
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }
    pub fn applying(&self) -> bool {
        self.job.as_ref().is_some_and(|job| job.applying)
    }
    pub fn ready(&self) -> bool {
        self.job
            .as_ref()
            .is_some_and(|job| job.result.lock().unwrap().is_some())
    }

    pub fn open(&mut self, root: PathBuf, replaces: Option<AmendTarget>) {
        self.open_write(root, replaces, None);
    }
    pub fn open_reference(&mut self, root: PathBuf, kind: RefKind, target: String) {
        self.open_write(root, None, Some((kind, target)));
    }
    fn open_write(
        &mut self,
        root: PathBuf,
        replaces: Option<AmendTarget>,
        reference: Option<(RefKind, String)>,
    ) {
        if self.job.is_none() && self.view.is_none() {
            self.view = Some(CommitView {
                root,
                replaces,
                reference,
                message: String::new(),
                cursor: 0,
                first: 0,
                error: None,
                input_token: Arc::new(()),
                review: None,
            });
        }
    }

    pub fn cancel(&mut self) {
        if self.applying() {
            return;
        }
        if let Some(job) = &self.job {
            job.cancelled.store(true, Ordering::Release);
        }
        self.view = None;
    }

    pub fn matches_root(&self, root: Option<&Path>) -> bool {
        self.view
            .as_ref()
            .is_none_or(|v| Some(v.root.as_path()) == root)
    }

    pub fn input_token(&self) -> Option<Arc<()>> {
        self.view
            .as_ref()
            .filter(|v| !self.busy() && v.review.is_none())
            .map(|v| v.input_token.clone())
    }

    pub fn paste_budget(&self) -> Option<usize> {
        self.view
            .as_ref()
            .filter(|v| !self.busy() && v.review.is_none())
            .map(|v| v.input_limit().saturating_sub(v.message.len()).max(1))
    }

    pub fn edit(&mut self, edit: FieldEdit<'_>) {
        if self.busy() {
            return;
        }
        let Some(view) = self.view.as_mut().filter(|v| v.review.is_none()) else {
            return;
        };
        if let FieldEdit::Insert(text) = edit {
            if text.chars().any(char::is_control) {
                view.error = Some("El campo debe ocupar una sola línea".into());
                return;
            }
        }
        let limit = view.input_limit();
        view.error = edit_field(&mut view.message, &mut view.cursor, edit, limit)
            .err()
            .map(str::to_owned);
        view.input_token = Arc::new(());
    }

    pub fn scroll(&mut self, delta: isize) {
        let Some(view) = &mut self.view else {
            return;
        };
        if let Some(review) = view.review() {
            view.first = view
                .first
                .saturating_add_signed(delta)
                .min(review.files().len().saturating_sub(1));
        }
    }

    pub fn submit(&mut self, bench: &Workbench, root: Option<&Path>) {
        if self.busy() {
            return;
        }
        let Some(view) = &mut self.view else {
            return;
        };
        if Some(view.root.as_path()) != root {
            self.cancel();
            return;
        }
        if view.reference.is_none() && crate::git_index::require_clean(bench).is_err() {
            view.error = Some("Guarda los documentos modificados antes de continuar".into());
            return;
        }
        view.error = None;
        view.input_token = Arc::new(());
        let plan = view.review.take();
        let applying = plan.is_some();
        let root = view.root.clone();
        let message = view.message.clone();
        let replaces = view.replaces.clone();
        let reference = view.reference.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = cancelled.clone();
        let result = Arc::new(Mutex::new(None));
        let slot = result.clone();
        let wake = self.wake.clone();
        let thread = thread::Builder::new()
            .name("cide-git-commit".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let cancelled = || stop.load(Ordering::Acquire);
                    match plan {
                        Some(WritePlan::Commit(plan)) => {
                            Completion::Applied(apply_commit(*plan, cancelled).map(|publication| {
                                publication.reflog_warning.unwrap_or_else(|| {
                                    let label = if replaces.is_some() {
                                        "Commit modificado"
                                    } else {
                                        "Commit creado"
                                    };
                                    format!("{label} · {}", publication.id)
                                })
                            }))
                        }
                        Some(WritePlan::Reference(plan)) => {
                            Completion::Applied(apply_ref(*plan, cancelled).map(|publication| {
                                let label = match publication.kind {
                                    RefKind::Branch => "Rama creada",
                                    RefKind::Tag => "Etiqueta creada",
                                };
                                format!("{label} · {} · {}", publication.name, publication.target)
                            }))
                        }
                        None => Completion::Review(match (reference, replaces) {
                            (Some((kind, target)), _) => {
                                prepare_ref(&root, &target, kind, &message, LIMITS, cancelled)
                                    .map(|plan| WritePlan::Reference(Box::new(plan)))
                            }
                            (None, Some(head)) => {
                                prepare_amend(&root, &message, &head, LIMITS, cancelled)
                                    .map(|plan| WritePlan::Commit(Box::new(plan)))
                            }
                            (None, None) => prepare_commit(&root, &message, LIMITS, cancelled)
                                .map(|plan| WritePlan::Commit(Box::new(plan))),
                        }),
                    }
                }))
                .unwrap_or(Completion::Panic);
                *slot.lock().unwrap() = Some(result);
                wake.signal();
            });
        match thread {
            Ok(thread) => {
                self.job = Some(Job {
                    result,
                    thread,
                    cancelled,
                    applying,
                })
            }
            Err(e) => view.error = Some(format!("No se pudo iniciar la tarea de Git ({e})")),
        }
    }

    /// Returns the result of a confirmed write independently of the current view.
    pub fn poll(&mut self) -> (bool, Option<CommitOutcome>) {
        let Some(job) = &self.job else {
            return (false, None);
        };
        let result = job.result.lock().unwrap().take();
        let Some(result) = result else {
            return (false, None);
        };
        let job = self.job.take().unwrap();
        let _ = job.thread.join();
        let Some(view) = &mut self.view else {
            return (true, None);
        };
        match result {
            Completion::Review(Ok(plan)) => {
                view.review = Some(plan);
                view.first = 0;
            }
            Completion::Review(Err(e)) => view.error = Some(e.to_string()),
            Completion::Applied(result) => {
                let root = view.root.clone();
                let reference = view.reference.is_some();
                let result = result.map_err(|e| e.to_string());
                if let Ok(message) = &result {
                    self.notice = Some((root.clone(), message.clone()));
                    self.view = None;
                } else {
                    view.error = result.as_ref().err().cloned();
                }
                return (
                    true,
                    Some(CommitOutcome {
                        root,
                        result,
                        reference,
                    }),
                );
            }
            Completion::Panic => {
                view.error = Some(
                    if job.applying {
                        "Publicación incierta. Actualiza el repositorio antes de reintentar"
                    } else {
                        "Falló la preparación de la operación Git"
                    }
                    .into(),
                );
                if job.applying {
                    return (
                        true,
                        Some(CommitOutcome {
                            root: view.root.clone(),
                            result: Err(view.error.clone().unwrap()),
                            reference: view.reference.is_some(),
                        }),
                    );
                }
            }
        }
        (true, None)
    }
}

pub struct CommitOutcome {
    pub root: PathBuf,
    pub result: Result<String, String>,
    pub reference: bool,
}

pub fn file_flag(change: jc_git::Change) -> &'static str {
    match change {
        jc_git::Change::Added => "+",
        jc_git::Change::Deleted => "−",
        _ => "∼",
    }
}

impl Drop for CommitController {
    fn drop(&mut self) {
        if let Some(job) = self.job.take() {
            if !job.applying {
                job.cancelled.store(true, Ordering::Release);
            }
            let _ = job.thread.join();
        }
    }
}
