//! Export a pinned local or historical comparison through the shared Git core.
use super::{git_history::label, GitPresentation, TuiApp};
use crate::state::PromptKind;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use jc_app::{
    git_revision::{ComparisonOrigin, RevisionComparison},
    BottomPanel, PrimaryFocus, Wakeup,
};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

struct Draft {
    root: PathBuf,
    comparison: Arc<RevisionComparison>,
}

type Outcome = Result<usize, String>;

struct Job {
    cancel: Arc<AtomicBool>,
    result: Arc<Mutex<Option<Outcome>>>,
}

#[derive(Default)]
pub(crate) struct PatchExport {
    draft: Option<Draft>,
    job: Option<Job>,
    notice: Option<String>,
}

impl PatchExport {
    pub(super) fn clear_draft(&mut self) {
        self.draft = None;
    }

    pub(crate) fn ready(&self) -> bool {
        self.job.as_ref().is_some_and(|job| {
            job.result
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_some()
        })
    }

    fn start(
        &mut self,
        comparison: Arc<RevisionComparison>,
        destination: PathBuf,
        wake: Wakeup,
    ) -> Result<(), String> {
        if self.job.is_some() {
            return Err("Ya hay una exportación de parche en curso".into());
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let result = Arc::new(Mutex::new(None));
        let worker_cancel = cancel.clone();
        let worker_result = result.clone();
        std::thread::Builder::new()
            .name("cide-tui-exportar-parche".into())
            .spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(5);
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    comparison.export_patch_to(&destination, || {
                        worker_cancel.load(Ordering::Acquire) || Instant::now() >= deadline
                    })
                }))
                .unwrap_or_else(|_| Err("Falló la tarea de exportación del parche".into()));
                *worker_result.lock().unwrap_or_else(|e| e.into_inner()) = Some(outcome);
                wake.signal();
            })
            .map_err(|error| error.to_string())?;
        self.job = Some(Job { cancel, result });
        self.notice = None;
        Ok(())
    }

    fn poll(&mut self) -> Option<String> {
        let result = self
            .job
            .as_ref()?
            .result
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()?;
        self.job = None;
        let message = match result {
            Ok(bytes) => format!("Parche exportado ({bytes} bytes)"),
            Err(error) => error.chars().take(240).collect(),
        };
        self.notice = Some(message.clone());
        Some(message)
    }
}

impl Drop for PatchExport {
    fn drop(&mut self) {
        if let Some(job) = &self.job {
            job.cancel.store(true, Ordering::Release);
        }
    }
}

impl TuiApp {
    fn git_patch_comparison(&self) -> Option<&RevisionComparison> {
        if !self.panels.bottom_visible()
            || self.panels.bottom() != BottomPanel::Diff
            || !self.git_history.immutable()
            || self
                .git_history
                .blame
                .as_ref()
                .is_some_and(|blame| !blame.comparing)
        {
            return None;
        }
        let comparison = self.git_revision.comparison()?;
        let visible = match (&comparison.origin, self.git_history.local.as_ref()) {
            (ComparisonOrigin::Local(local), Some((kind, path))) => {
                local.kind == *kind
                    && comparison.path == *path
                    && self.git_status.selected_path_ref() == Some(path.as_slice())
            }
            (ComparisonOrigin::Historical { .. }, None) => self.git_history.active,
            _ => false,
        };
        (visible && comparison.can_export_patch()).then_some(comparison)
    }

    pub(super) fn handle_git_patch_key(&mut self, key: KeyEvent) -> bool {
        if key.kind != KeyEventKind::Press
            || !key.modifiers.is_empty()
            || !matches!(key.code, KeyCode::Char('x' | 'X'))
            || self.focus.primary() != PrimaryFocus::BottomPanel
        {
            return false;
        }
        if let Some(job) = &self.git_patch.job {
            job.cancel.store(true, Ordering::Release);
            self.status = "Cancelando exportación de parche…".into();
            return true;
        }
        self.git_revision
            .invalidate_context(self.project_root.as_deref());
        if self.git_patch_comparison().is_none() {
            return false;
        }
        let Some((root, comparison)) = self
            .project_root
            .clone()
            .zip(self.git_revision.share_comparison())
        else {
            return false;
        };
        self.open_prompt(PromptKind::GitExportPatch);
        self.git_patch.draft = Some(Draft { root, comparison });
        self.status =
            "Escribe una ruta absoluta .patch fuera del repositorio · Intro exporta · Esc cancela"
                .into();
        true
    }

    pub(super) fn submit_git_patch(&mut self, destination: PathBuf) {
        if !destination.is_absolute()
            || !destination
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("patch"))
        {
            self.status = "Elige una ruta absoluta con extensión .patch".into();
            return;
        }
        let Some(draft) = self.git_patch.draft.as_ref().filter(|draft| {
            self.project_root.as_ref() == Some(&draft.root)
                && self
                    .git_patch_comparison()
                    .is_some_and(|current| std::ptr::eq(current, draft.comparison.as_ref()))
        }) else {
            self.close_overlay();
            self.status = "La comparación cambió mientras elegías el destino".into();
            return;
        };
        let comparison = draft.comparison.clone();
        match self
            .git_patch
            .start(comparison, destination, self.syntax_wakeup.clone())
        {
            Ok(()) => {
                self.close_overlay();
                self.status = "Exportando parche…".into();
            }
            Err(error) => self.status = error,
        }
    }

    pub(super) fn poll_git_patch(&mut self) -> bool {
        if let Some(notice) = self.git_patch.poll() {
            self.status = notice;
            true
        } else {
            false
        }
    }

    pub(super) fn add_git_patch_status(&self, prepared: &mut GitPresentation, width: u16) {
        if !self.panels.bottom_visible()
            || self.panels.bottom() != BottomPanel::Diff
            || !self.git_history.immutable()
        {
            return;
        }
        let available = self.git_patch_comparison().is_some();
        if let Some(job) = &self.git_patch.job {
            let progress = if job.cancel.load(Ordering::Acquire) {
                "Cancelando exportación de parche…"
            } else {
                "Exportando parche… · X cancela"
            };
            prepared.status = label(progress, width);
        } else if let Some(notice) = &self.git_patch.notice {
            prepared.status = label(&format!("{notice} · {}", prepared.status), width);
        }
        if available && self.git_patch.job.is_none() {
            if self.git_index.review().is_some() {
                prepared
                    .details
                    .insert(0, label("X exportar parche", width));
            } else {
                prepared.status = label(&format!("X exportar parche · {}", prepared.status), width);
            }
        }
    }
}
