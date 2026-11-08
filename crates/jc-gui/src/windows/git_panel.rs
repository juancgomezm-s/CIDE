//! Native intentions over existing read-only Git controllers. No Git command
//! implementation, source ownership or filesystem navigation lives in this UI.
use super::{project_panel::ProjectPresentation, Action, State};
use jc_app::{
    git_diff::{GitDiffOptions, GitDiffState},
    git_read::GitBlobSource,
    git_status_tasks::GitStatusOptions,
    Modifiers, TaskStatus,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GitIntent {
    Status,
    Diff(GitBlobSource),
}
pub(super) struct GitFrame {
    pub panel: ProjectPresentation,
    pub labels: super::project_paint::PanelLabels,
}
impl State {
    pub(super) fn clear_git(&mut self) {
        let _ = self.git_index.discard_review();
        self.git_commit.cancel();
        self.git_revision.cancel();
        self.git_status_due = false;
        let preferences = self.git_workspace.preference_bytes();
        self.git_workspace = Default::default();
        let _ = self.git_workspace.restore_preferences(&preferences);
        self.git_diff.clear(&mut self.jobs);
        self.git_status.cancel();
        self.git_status_options = None;
        self.git_context = None;
        self.git_panel = None;
        self.git_view = Default::default();
    }
    pub(super) fn schedule_git_status(&mut self) {
        if self.project_root.is_some() {
            self.git_status_due = true;
            self.bridge.signal();
        }
    }
    pub(super) fn git_summary(&self) -> String {
        use jc_app::git_status_tasks::GitStatusTaskState as S;
        if self.project_root.is_none() {
            return "Git: abre carpeta".into();
        }
        if self.git_status.is_not_repository() {
            return "Git: sin repositorio".into();
        }
        match self.git_status.state() {
            S::Ready if self.git_status.root() == self.project_root.as_deref() => {
                match self.git_status.result() {
                    Some(result) if result.status().entries().is_empty() => "Git: limpio".into(),
                    Some(result) => format!("Git: {} cambios", result.status().entries().len()),
                    None => "Git: consultar".into(),
                }
            }
            S::Running | S::Queued => "Git: leyendo…".into(),
            S::Failed | S::TimedOut | S::Limited => "Git: error · consultar".into(),
            _ => "Git: consultar".into(),
        }
    }
    pub(super) fn request_git(&mut self, intent: GitIntent) -> Action {
        self.git_workspace.graph = None;
        self.git_workspace.path_query = None;
        self.git_workspace.blame_query = None;
        self.git_workspace.commit_search = None;
        self.close_selector();
        self.git_revision.cancel();
        self.git_workspace.history_commit = None;
        self.git_workspace.local_kind = None;
        self.git_workspace.invalidate();
        self.git_workspace.commit = None;
        let Some(root) = self.project_root.clone() else {
            self.error("Abre primero la carpeta raíz del repositorio");
            return Action::None;
        };
        self.git_panel = Some(intent);
        self.git_context = Some(root.clone());
        match intent {
            GitIntent::Status => {
                self.git_workspace.select_files();
                self.git_workspace.focus_diff = false;
                self.git_status_due = false;
                let options = GitStatusOptions {
                    root,
                    limits: Default::default(),
                };
                self.git_status_options = Some(options.clone());
                self.git_view.first_status();
                match self.git_status.request(options) {
                    Ok(()) => self.status = "Leyendo estado Git; solo disco/índice".into(),
                    Err(error) => self.error(format!("Admisión del estado Git: {error}")),
                }
            }
            GitIntent::Diff(source) => {
                self.git_workspace.select_comparison();
                self.git_workspace.diff_column = 0;
                self.git_workspace.focus_diff = true;
                self.git_source = source;
                self.git_workspace.diff_first = 0;
                let options = GitDiffOptions {
                    root: root.clone(),
                    cwd: root,
                    source,
                };
                match self
                    .git_diff
                    .request(&self.workbench, &mut self.jobs, options)
                {
                    Ok(()) => self.status = self.git_diff.message().into(),
                    Err(error) => self.error(error),
                }
            }
        }
        Action::None
    }
    /// Drop mismatching context and invalidate document stamps without admitting
    /// snapshots or consuming completions. Painting calls this route only.
    pub(super) fn validate_git_context(&mut self) -> bool {
        if self
            .git_context
            .as_ref()
            .is_some_and(|root| self.project_root.as_ref() != Some(root))
        {
            self.clear_git();
            return true;
        }
        self.git_diff
            .validate_context(&self.workbench, &mut self.jobs)
    }
    /// The sole F3 poll: dispatch Git-owned completions first, other typed rows
    /// to Workbench. Called only from TASKS_MESSAGE, never prepare/draw.
    pub(super) fn poll_git(&mut self) -> bool {
        let mut changed =
            self.validate_git_context() | self.validate_open_diff() | self.validate_source();
        let revision_changed = self.git_revision.poll(self.project_root.as_deref());
        if revision_changed {
            self.git_workspace.invalidate();
            self.status = self.git_revision.message().into();
        }
        changed |= revision_changed;
        if self.git_revision.take_recovery() {
            changed = true;
            self.status = "La consulta histórica se recuperó de un fallo; actualiza".into();
        }
        let mut diff_changed = false;
        for mut completion in self.jobs.poll(&self.workbench, 4) {
            if completion.status == TaskStatus::Panicked {
                self.status =
                    "La tarea de fondo se recuperó de un fallo; actualiza explícitamente".into();
                changed = true;
            }
            if self.git_diff.accept(&self.workbench, &mut completion) {
                diff_changed = true;
            } else if self.open_diff.accept(&self.workbench, &mut completion) {
                changed = true;
                if self.open_diff_visible {
                    self.status = self.open_diff.message().into();
                }
            } else if self.accept_source(&mut completion) || self.accept_outline(&mut completion) {
                changed = true;
            } else {
                changed |= self.workbench.accept_task_completion(&completion);
            }
        }
        diff_changed |= self.git_diff.tick(&self.workbench, &mut self.jobs);
        changed |= diff_changed;
        changed |= self.open_diff.tick(&self.workbench, &mut self.jobs);
        changed |= self.tick_source();
        changed |= self.tick_outline();
        let mut status_changed = false;
        if let Some(options) = &self.git_status_options {
            status_changed = self.git_status.poll(options);
            changed |= status_changed;
        }
        if self.git_status_due
            && !matches!(
                self.git_status.state(),
                jc_app::git_status_tasks::GitStatusTaskState::Queued
                    | jc_app::git_status_tasks::GitStatusTaskState::Running
            )
        {
            self.git_status_due = false;
            if let Some(root) = self.project_root.clone() {
                let options = GitStatusOptions {
                    root: root.clone(),
                    limits: Default::default(),
                };
                self.git_context = Some(root);
                self.git_status_options = Some(options.clone());
                if let Err(error) = self.git_status.request(options) {
                    self.error(error);
                }
                changed = true;
            }
        }
        match self.git_panel.filter(|_| {
            self.git_workspace.history_commit.is_none() && self.git_workspace.local_kind.is_none()
        }) {
            Some(GitIntent::Status) if status_changed => {
                self.status = self.git_status.message().into()
            }
            Some(GitIntent::Diff(_)) if diff_changed => {
                self.status = self.git_diff.message().into()
            }
            _ => {}
        }
        if status_changed {
            self.git_workspace.invalidate();
        }
        if diff_changed
            && self.git_workspace.history_commit.is_none()
            && self.git_workspace.local_kind.is_none()
        {
            self.git_workspace.diff_first = 0;
            self.git_workspace.invalidate();
        }
        if status_changed
            && !self.git_status.is_not_repository()
            && self.git_panel.is_some()
            && self.git_status.state() == jc_app::git_status_tasks::GitStatusTaskState::Failed
        {
            if let Some(jc_app::git_status_tasks::GitStatusFailure::Provider(error)) =
                self.git_status.failure()
            {
                let message = format!("Git: {error}");
                self.error(message);
            }
        }
        if self.git_status.take_recovery() {
            self.status =
                "La tarea de estado Git se recuperó de un fallo; actualiza explícitamente".into();
            changed = true;
        }
        if self.jobs.has_completed()
            || self.git_status.ready()
            || self.git_revision.ready()
            || self.git_index.ready()
            || self.git_commit.ready()
        {
            self.bridge.signal();
        }
        changed | self.poll_git_patch() | self.poll_git_index() | self.poll_git_create_commit()
    }
    pub(super) fn cancel_git(&mut self, intent: GitIntent) {
        match intent {
            GitIntent::Status => self.git_status.cancel(),
            GitIntent::Diff(_) => self.git_diff.cancel(&mut self.jobs),
        }
        self.status = "Lectura Git cancelada; actualización explícita".into();
    }
    pub(super) fn git_key(&mut self, code: usize, modifiers: Modifiers) -> Action {
        let Some(intent) = self.git_panel else {
            return Action::None;
        };
        self.validate_git_context();
        if self.git_create_commit_key(code, modifiers) {
            return Action::None;
        }
        if self.git_graph_key(code, modifiers) {
            return Action::None;
        }
        if code == b'I' as usize && modifiers == Modifiers::CTRL.union(Modifiers::SHIFT) {
            self.git_index_action(false);
            return Action::None;
        }
        if code == b'B' as usize && modifiers == Modifiers::CTRL.union(Modifiers::ALT) {
            if self
                .git_index
                .review()
                .is_some_and(|review| review.target.hunk.is_some())
            {
                self.status = "Ctrl+Intro confirma el bloque · Esc descarta".into();
            } else {
                self.git_index_action(true);
            }
            return Action::None;
        }
        if code == 0x0d
            && modifiers == Modifiers::CTRL
            && self
                .git_index
                .review()
                .is_some_and(|review| review.target.hunk.is_some())
        {
            self.git_index_action(true);
            return Action::None;
        }
        if code == 0x1b
            && modifiers == Modifiers::NONE
            && matches!(
                self.git_index.state(),
                jc_app::git_index::IndexState::Preparing | jc_app::git_index::IndexState::Reviewing
            )
        {
            let _ = self.git_index.discard_review();
            self.status = "Revisión del índice descartada".into();
            self.git_workspace.invalidate();
            return Action::None;
        }
        if code == b'E' as usize && modifiers == Modifiers::CTRL.union(Modifiers::ALT) {
            return self.git_patch_action();
        }
        if let Some(action) = self.git_commit_search_key(code, modifiers) {
            return action;
        }
        if self.git_controls_key(code, modifiers) {
            return Action::None;
        }
        if self
            .git_workspace
            .commit_search
            .as_ref()
            .is_some_and(|s| !s.opened)
        {
            return Action::None;
        }
        if self.git_workspace.commit_search.is_some() {
            return self.git_history_key(code, modifiers);
        }
        if self.git_workspace.blame_query.is_some() {
            return if self.git_workspace.history_commit.is_some() {
                self.git_history_key(code, modifiers)
            } else {
                Action::None
            };
        }
        if self.git_workspace.history_commit.is_some() {
            return self.git_history_key(code, modifiers);
        }
        if modifiers == Modifiers::NONE && matches!(code, 0x50 | 0x54) {
            self.request_git_local(if code == 0x50 {
                jc_app::git_revision::PairKind::Prepared
            } else {
                jc_app::git_revision::PairKind::Worktree
            });
            return Action::None;
        }
        if self.git_workspace.local_kind.is_some() {
            return self.git_local_key(code, modifiers);
        }
        if code == 0x09 && modifiers == Modifiers::NONE {
            self.git_workspace.focus_diff = !self.git_workspace.focus_diff;
            self.git_workspace.commit = None;
            self.git_workspace.invalidate();
            return Action::None;
        }
        if code == 0x1b {
            if intent != GitIntent::Status
                || self.git_status.state() != jc_app::git_status_tasks::GitStatusTaskState::Ready
            {
                self.cancel_git(intent);
            }
            self.git_panel = None;
            return Action::None;
        }
        if code == 0x74 && modifiers == Modifiers::NONE {
            return self.request_git(intent);
        }
        if !self.git_workspace.focus_diff {
            if modifiers == Modifiers::NONE {
                self.git_presentation();
                match code {
                    0x26 => self.git_view.move_status(-1),
                    0x28 => self.git_view.move_status(1),
                    0x21 => self.git_view.move_status(-10),
                    0x22 => self.git_view.move_status(10),
                    0x24 => self.git_view.first_status(),
                    0x23 => self.git_view.last_status(),
                    0x0d => self.compare_git_status_file(),
                    _ => {}
                }
                self.git_workspace.invalidate();
            }
            return Action::None;
        }
        match intent {
            GitIntent::Status => {}
            GitIntent::Diff(_) => {
                if modifiers != Modifiers::NONE {
                    return Action::None;
                }
                match code {
                    0x71 => return self.request_git(GitIntent::Diff(GitBlobSource::Head)),
                    0x72 => return self.request_git(GitIntent::Diff(GitBlobSource::Index)),
                    0x26 | 0x28 => {
                        self.git_diff
                            .select_relative(if code == 0x26 { -1 } else { 1 });
                        self.git_workspace.diff_first = 0;
                    }
                    0x21 => {
                        self.git_workspace.diff_first =
                            self.git_workspace.diff_first.saturating_sub(10)
                    }
                    0x22 => {
                        self.git_workspace.diff_first =
                            self.git_workspace.diff_first.saturating_add(10)
                    }
                    0x24 => self.git_workspace.diff_first = 0,
                    0x23 => {
                        self.git_workspace.diff_first = self
                            .git_workspace
                            .diff_total
                            .saturating_sub(self.git_workspace.diff_page)
                    }
                    0x0d => {
                        let selected = self.git_diff.selected();
                        match self.git_diff.select_hunk(selected, &mut self.workbench) {
                            Ok(()) => {
                                self.git_panel = None;
                                self.bottom.focused = false;
                                self.status = "Bloque Git seleccionado; fuente intacto".into();
                            }
                            Err(error) => self.error(error),
                        }
                    }
                    _ => {}
                }
            }
        }
        Action::None
    }
    pub(super) fn git_presentation(&mut self) -> Option<super::git_workspace::Frame> {
        self.prepare_git_workspace()
    }
    pub(super) fn git_diff_running(&self) -> bool {
        matches!(
            self.git_diff.state(),
            GitDiffState::Queued | GitDiffState::Running
        )
    }
}
