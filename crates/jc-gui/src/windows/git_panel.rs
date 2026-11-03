//! Native intentions over existing read-only Git controllers. No Git command
//! implementation, source ownership or filesystem navigation lives in this UI.
use super::{
    project_panel::ProjectPresentation,
    project_results::{ProjectResultRow, ProjectResultsPresentation},
    Action, State,
};
use jc_app::{
    git_diff::{GitDiffOptions, GitDiffState},
    git_read::GitBlobSource,
    git_status_tasks::GitStatusOptions,
    Modifiers, TaskStatus,
};
use std::path::PathBuf;

fn document_target(
    state: &State,
) -> Option<(
    jc_app::DocumentId,
    u64,
    Option<jc_text::TextEncoding>,
    Option<jc_app::ViewId>,
)> {
    let document = state.workbench.active_document()?;
    Some((
        document.id,
        document.session.document().revision(),
        document.session.encoding(),
        state
            .workbench
            .panes()
            .view(state.workbench.panes().active())
            .and_then(|view| view.view_id()),
    ))
}
pub(super) unsafe fn configure(hwnd: super::ffi::Hwnd, intent: GitIntent) {
    let captured = super::owner(hwnd).map(|model| {
        let state = model.state.borrow();
        (state.project_root.clone(), document_target(&state))
    });
    let selected = super::file_dialog_named(
        hwnd,
        false,
        Some("Choose your trusted Git executable (session only)"),
    );
    if let Some(model) = super::owner(hwnd) {
        let mut state = model.state.borrow_mut();
        match selected {
            Ok(Some(path)) => {
                let current = (state.project_root.clone(), document_target(&state));
                if captured.as_ref().is_none_or(|(root, document)| {
                    root != &current.0
                        || (matches!(intent, GitIntent::Diff(_)) && document != &current.1)
                }) {
                    state.error("Git destination context changed during configuration; retry");
                } else if let Err(error) = state.set_git_executable(path) {
                    state.error(error);
                } else {
                    state.request_git(intent);
                }
            }
            Ok(None) => state.status = "Git configuration cancelled; no read started".into(),
            Err(error) => state.error(error),
        }
    }
}

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
        self.git_diff.clear(&mut self.jobs);
        self.git_status.cancel();
        self.git_status_options = None;
        self.git_context = None;
        self.git_panel = None;
        self.git_view = Default::default();
    }
    pub(super) fn set_git_executable(&mut self, path: PathBuf) -> Result<(), String> {
        if !path.is_absolute() || path.as_os_str().len() > 4096 {
            return Err("Choose an absolute Git executable path within4096 native units".into());
        }
        self.clear_git();
        self.git_executable = Some(path);
        Ok(())
    }
    pub(super) fn request_git(&mut self, intent: GitIntent) -> Action {
        self.close_selector();
        self.git_last_intent = intent;
        let Some(root) = self.project_root.clone() else {
            self.error("Open the repository root folder first");
            return Action::None;
        };
        let Some(executable) = self.git_executable.clone() else {
            self.status = "Choose your trusted Git executable for this session".into();
            return Action::ConfigureGit(intent);
        };
        self.git_panel = Some(intent);
        self.git_context = Some((root.clone(), executable.clone()));
        match intent {
            GitIntent::Status => {
                let options = GitStatusOptions {
                    root,
                    git_executable: executable,
                    limits: Default::default(),
                };
                self.git_status_options = Some(options.clone());
                self.git_view.first_status();
                match self.git_status.request(options) {
                    Ok(()) => self.status = "Reading Git repository status; disk/index only".into(),
                    Err(error) => self.error(format!("Git status admission: {error:?}")),
                }
            }
            GitIntent::Diff(source) => {
                self.git_source = source;
                self.git_view.first_diff();
                let options = GitDiffOptions {
                    root: root.clone(),
                    cwd: root,
                    git_executable: executable,
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
        if self.git_context.as_ref().is_some_and(|(root, executable)| {
            self.project_root.as_ref() != Some(root)
                || self.git_executable.as_ref() != Some(executable)
        }) {
            self.clear_git();
            return true;
        }
        self.git_diff
            .validate_context(&self.workbench, &mut self.jobs)
    }
    /// The sole F3 poll: dispatch Git-owned completions first, other typed rows
    /// to Workbench. Called only from TASKS_MESSAGE, never prepare/draw.
    pub(super) fn poll_git(&mut self) -> bool {
        let mut changed = self.validate_git_context();
        let mut diff_changed = false;
        for mut completion in self.jobs.poll(&self.workbench, 4) {
            if completion.status == TaskStatus::Panicked {
                self.status = "Background worker recovered from failure; refresh explicitly".into();
                changed = true;
            }
            if self.git_diff.accept(&self.workbench, &mut completion) {
                diff_changed = true;
            } else {
                changed |= self.workbench.accept_task_completion(&completion);
            }
        }
        diff_changed |= self.git_diff.tick(&self.workbench, &mut self.jobs);
        changed |= diff_changed;
        let mut status_changed = false;
        if let Some(options) = &self.git_status_options {
            status_changed = self.git_status.poll(options);
            changed |= status_changed;
        }
        match self.git_panel {
            Some(GitIntent::Status) if status_changed => {
                self.status = self.git_status.message().into()
            }
            Some(GitIntent::Diff(_)) if diff_changed => {
                self.status = self.git_diff.message().into()
            }
            _ => {}
        }
        if self.git_status.take_recovery() {
            self.status = "Git status worker recovered from failure; refresh explicitly".into();
            changed = true;
        }
        if self.jobs.has_completed() || self.git_status.ready() {
            self.bridge.signal();
        }
        changed
    }
    pub(super) fn cancel_git(&mut self, intent: GitIntent) {
        match intent {
            GitIntent::Status => self.git_status.cancel(),
            GitIntent::Diff(_) => self.git_diff.cancel(&mut self.jobs),
        }
        self.status = "Git read cancelled; refresh is explicit".into();
    }
    pub(super) fn git_key(&mut self, code: usize, modifiers: Modifiers) -> Action {
        let Some(intent) = self.git_panel else {
            return Action::None;
        };
        self.validate_git_context();
        if code == 0x1b {
            self.cancel_git(intent);
            self.git_panel = None;
            return Action::None;
        }
        if code == 0x74 && modifiers == Modifiers::NONE {
            return self.request_git(intent);
        }
        match intent {
            GitIntent::Status => {
                self.git_presentation(); // Normalize view against accepted rows.
                if modifiers != Modifiers::NONE {
                    return Action::None;
                }
                match code {
                    0x26 => self.git_view.move_status(-1),
                    0x28 => self.git_view.move_status(1),
                    0x21 => self.git_view.move_status(-10),
                    0x22 => self.git_view.move_status(10),
                    0x24 => self.git_view.first_status(),
                    0x23 => self.git_view.last_status(),
                    0x0d => {
                        self.status = "Status paths are read-only; opening is not enabled".into()
                    }
                    _ => {}
                }
            }
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
                        self.git_view.first_diff();
                    }
                    0x21 => self.git_view.scroll_diff(-10),
                    0x22 => self.git_view.scroll_diff(10),
                    0x24 => self.git_view.first_diff(),
                    0x23 => self.git_view.last_diff(),
                    0x0d => {
                        let selected = self.git_diff.selected();
                        match self.git_diff.select_hunk(selected, &mut self.workbench) {
                            Ok(()) => {
                                self.git_panel = None;
                                self.bottom.focused = false;
                                self.status = "Git hunk selected; source unchanged".into();
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
    pub(super) fn git_presentation(&mut self) -> Option<GitFrame> {
        self.validate_git_context();
        let intent = self.git_panel?;
        let (columns, _) = super::project_paint::field_geometry(
            self.width,
            self.height,
            self.cell_width,
            self.cell_height,
            1,
        );
        let rows = super::project_paint::result_capacity(
            self.width,
            self.height,
            self.cell_width,
            self.cell_height,
        );
        let presentation = match intent {
            GitIntent::Status => self
                .git_view
                .prepare_status(&self.git_status, rows, columns),
            GitIntent::Diff(_) => self.git_view.prepare_diff(
                &self.git_diff,
                self.workbench
                    .active_document()
                    .and_then(|d| d.session.encoding()),
                rows,
                columns,
            ),
        };
        let footer = match intent {
            GitIntent::Status => "Arrows select | F5 refresh | Enter never opens paths | Esc cancel/close",
            GitIntent::Diff(_) => "Up/Down hunk | PgUp/PgDn body | Enter navigate | F2 HEAD F3 index | F5 refresh | Esc",
        };
        Some(GitFrame {
            panel: ProjectPresentation { fields: Vec::new(), active: 0, status: self.status.clone(), results: Some(ProjectResultsPresentation {
                title: presentation.title, message: presentation.message, total: presentation.total, selected: presentation.selected,
                rows: presentation.rows.into_iter().map(|row| ProjectResultRow {index:row.index,label:row.label,selected:row.selected,is_match:true}).collect(),
            }) },
            labels: super::project_paint::PanelLabels {draft:"Git read-only", policy: "Read-only snapshots | explicit refresh | isolated Git config | no watcher / writes", footer},
        })
    }
    pub(super) fn git_diff_running(&self) -> bool {
        matches!(
            self.git_diff.state(),
            GitDiffState::Queued | GitDiffState::Running
        )
    }
}
