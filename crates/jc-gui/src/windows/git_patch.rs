//! Explicit export from one accepted comparison. The save dialog runs without
//! a model borrow, and a single bounded job serializes and publishes the bytes.
use super::{ffi::Hwnd, file_dialog_options, owner, Action, State};
use jc_app::{
    git_revision::{ComparisonOrigin, RevisionComparison},
    Wakeup,
};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};

struct Job {
    cancel: Arc<AtomicBool>,
    result: mpsc::Receiver<Result<usize, String>>,
}

#[derive(Default)]
pub(super) struct Controller {
    job: Option<Job>,
    pub(super) notice: Option<String>,
}

impl Controller {
    pub(super) fn busy(&self) -> bool {
        self.job.is_some()
    }

    fn start(
        &mut self,
        comparison: Arc<RevisionComparison>,
        destination: PathBuf,
        wake: Wakeup,
    ) -> Result<(), String> {
        if self.busy() {
            return Err("Ya hay una exportación de parche en curso".into());
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let (sender, result) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("cide-exportar-parche".into())
            .spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(5);
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    comparison.export_patch_to(&destination, || {
                        worker_cancel.load(Ordering::Acquire) || Instant::now() >= deadline
                    })
                }))
                .unwrap_or_else(|_| Err("Falló la tarea de exportación del parche".into()));
                let _ = sender.send(result);
                wake.signal();
            })
            .map_err(|error| error.to_string())?;
        self.job = Some(Job { cancel, result });
        self.notice = Some("Exportando parche…".into());
        Ok(())
    }

    fn request_cancel(&mut self) {
        if let Some(job) = &self.job {
            job.cancel.store(true, Ordering::Release);
            self.notice = Some("Cancelando exportación…".into());
        }
    }

    fn poll(&mut self) -> Option<String> {
        let result = match self.job.as_ref()?.result.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("La tarea de exportación terminó sin resultado".into())
            }
        };
        self.job = None;
        let notice = match result {
            Ok(bytes) => format!("Parche exportado ({bytes} bytes)"),
            Err(error) => error.chars().take(240).collect(),
        };
        self.notice = Some(notice.clone());
        Some(notice)
    }
}

impl Drop for Controller {
    fn drop(&mut self) {
        if let Some(job) = &self.job {
            job.cancel.store(true, Ordering::Release);
        }
    }
}

impl State {
    pub(super) fn git_patch_available(&self) -> bool {
        self.git_patch_comparison().is_some()
    }

    fn git_patch_comparison(&self) -> Option<&RevisionComparison> {
        self.git_panel?;
        let comparison = self.git_revision.comparison()?;
        if self
            .git_workspace
            .commit_search
            .as_ref()
            .is_some_and(|search| !search.opened)
            || !comparison.can_export_patch()
        {
            return None;
        }
        let selected = match &comparison.origin {
            ComparisonOrigin::Local(local) => {
                self.git_workspace.local_kind == Some(local.kind)
                    && self.git_workspace.selected_path.as_deref()
                        == Some(comparison.path.as_slice())
            }
            ComparisonOrigin::Historical { commit, .. } => {
                self.git_workspace.history_file.is_some()
                    && self.git_workspace.history_commit.as_ref() == Some(commit)
            }
        };
        selected.then_some(comparison)
    }

    pub(super) fn git_patch_action(&mut self) -> Action {
        if self.git_patch.busy() {
            self.git_patch.request_cancel();
            self.git_workspace.invalidate();
            return Action::None;
        }
        if self.git_patch_available() {
            if let Some(comparison) = self.git_revision.share_comparison() {
                return Action::ExportGitPatch(comparison);
            }
        }
        Action::None
    }

    pub(super) fn poll_git_patch(&mut self) -> bool {
        if let Some(notice) = self.git_patch.poll() {
            self.status = notice;
            self.git_workspace.invalidate();
            true
        } else {
            false
        }
    }
}

pub(super) unsafe fn choose_destination(hwnd: Hwnd, comparison: Arc<RevisionComparison>) {
    let destination = file_dialog_options(
        hwnd,
        true,
        Some("Exportar parche Git fuera del repositorio"),
        true,
    );
    let Some(owner) = owner(hwnd) else { return };
    let mut state = owner.state.borrow_mut();
    state.validate_git_context();
    let result = match destination {
        Ok(Some(destination)) => {
            if state
                .git_patch_comparison()
                .is_some_and(|current| std::ptr::eq(current, comparison.as_ref()))
            {
                let wake = state.wakeup.clone();
                state.git_patch.start(comparison, destination, wake)
            } else {
                Err("La comparación cambió mientras elegías el destino".into())
            }
        }
        Ok(None) => {
            state.git_patch.notice = Some("Exportación de parche cancelada".into());
            Ok(())
        }
        Err(error) => Err(error),
    };
    if let Err(error) = result {
        state.git_patch.notice = Some(error.chars().take(240).collect());
        state.error(error);
    }
    state.git_workspace.invalidate();
}
