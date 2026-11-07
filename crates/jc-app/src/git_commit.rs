//! Shared draft, review and background publication for the two frontends.
use crate::{edit_field, FieldEdit, Wakeup, Workbench};
pub use jc_git::commit_write::MESSAGE_BYTES;
use jc_git::commit_write::{apply_commit, prepare_commit, CommitPlan, Publication};
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
    Review(Result<Box<CommitPlan>, jc_git::Error>),
    Applied(Result<Publication, jc_git::Error>),
    Panic,
}

struct Job {
    result: Arc<Mutex<Option<Completion>>>,
    thread: JoinHandle<()>,
    cancelled: Arc<AtomicBool>,
    applying: bool,
}

pub struct CommitView {
    root: PathBuf,
    pub message: String,
    pub cursor: usize,
    pub first: usize,
    pub error: Option<String>,
    input_token: Arc<()>,
    review: Option<Box<CommitPlan>>,
}

impl CommitView {
    pub fn review(&self) -> Option<&CommitPlan> {
        self.review.as_deref()
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

    pub fn open(&mut self, root: PathBuf) {
        if self.job.is_none() && self.view.is_none() {
            self.view = Some(CommitView {
                root,
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
            .map(|v| (MESSAGE_BYTES - 1).saturating_sub(v.message.len()).max(1))
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
                view.error = Some("El mensaje debe ocupar una sola línea".into());
                return;
            }
        }
        view.error = edit_field(&mut view.message, &mut view.cursor, edit, MESSAGE_BYTES - 1)
            .err()
            .map(str::to_owned);
        view.input_token = Arc::new(());
    }

    pub fn scroll(&mut self, delta: isize) {
        let Some(view) = &mut self.view else {
            return;
        };
        if let Some(review) = &view.review {
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
        if crate::git_index::require_clean(bench).is_err() {
            view.error = Some("Guarda los documentos modificados antes de continuar".into());
            return;
        }
        view.error = None;
        view.input_token = Arc::new(());
        let plan = view.review.take();
        let applying = plan.is_some();
        let root = view.root.clone();
        let message = view.message.clone();
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
                        Some(plan) => Completion::Applied(apply_commit(*plan, cancelled)),
                        None => Completion::Review(
                            prepare_commit(&root, &message, LIMITS, cancelled).map(Box::new),
                        ),
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
                let result = result.map_err(|e| e.to_string());
                if let Ok(publication) = &result {
                    self.notice = Some((
                        root.clone(),
                        publication
                            .reflog_warning
                            .clone()
                            .unwrap_or_else(|| format!("Commit creado · {}", publication.id)),
                    ));
                    self.view = None;
                } else {
                    view.error = result.as_ref().err().cloned();
                }
                return (true, Some(CommitOutcome { root, result }));
            }
            Completion::Panic => {
                view.error = Some(
                    if job.applying {
                        "Publicación incierta. Actualiza el repositorio antes de reintentar"
                    } else {
                        "Falló la preparación del commit"
                    }
                    .into(),
                );
                if job.applying {
                    return (
                        true,
                        Some(CommitOutcome {
                            root: view.root.clone(),
                            result: Err(view.error.clone().unwrap()),
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
    pub result: Result<Publication, String>,
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
