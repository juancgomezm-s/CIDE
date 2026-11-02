use super::TuiApp;
use crate::state::PromptKind;
use jc_app::{BottomPanel, PrimaryFocus, ProjectSearchOptions};

impl TuiApp {
    pub(crate) fn submit_project_prompt(&mut self, kind: PromptKind, input: String) {
        if input.len() > 4096 {
            self.status = "Project query/filter exceeds 4096 bytes".into();
            return;
        }
        match kind {
            PromptKind::ProjectReplacement => {
                match self.project_search.preview_replace(&self.workbench, input) {
                    Ok(preview) => {
                        self.replace_preview = Some(preview);
                        self.open_focus_overlay(jc_app::Overlay::Dialog);
                        self.overlay =
                            crate::state::UiOverlay::ProjectReplacePreview { selected: 0 };
                        self.status =
                            "Review search snapshot; versions will be checked before each file"
                                .into();
                    }
                    Err(error) => self.status = error,
                }
            }
            PromptKind::ProjectQuery => {
                if input.is_empty() {
                    self.status = "Search text cannot be empty".into();
                    return;
                }
                self.project_draft = Some(ProjectSearchOptions::literal(input));
                self.open_prompt(PromptKind::ProjectInclude);
            }
            PromptKind::ProjectInclude => {
                if let Some(draft) = &mut self.project_draft {
                    if !input.trim().is_empty() {
                        draft.include = patterns(&input);
                    }
                }
                self.open_prompt(PromptKind::ProjectExclude);
            }
            PromptKind::ProjectExclude => {
                let Some(mut draft) = self.project_draft.take() else {
                    return;
                };
                draft.exclude.extend(patterns(&input));
                draft.encoding = self.default_encoding;
                let Some(root) = &self.project_root else {
                    self.status = "Open a project folder first".into();
                    return;
                };
                match self.project_search.start(&self.workbench, root, draft) {
                    Ok(()) => {
                        self.close_overlay();
                        self.project_results = true;
                        self.bottom_selected = 0;
                        self.panels.show_bottom(BottomPanel::SearchResults);
                        let _ = self.focus.focus(PrimaryFocus::BottomPanel, &self.panels);
                        self.status = "Searching project; Esc cancels".into();
                    }
                    Err(error) => {
                        self.status = error;
                        self.close_overlay();
                    }
                }
            }
            _ => {}
        }
    }
    pub(crate) fn search_result_count(&self) -> usize {
        if self.project_results {
            self.project_search.rows().len()
        } else {
            self.workbench.search().matches.len()
        }
    }
    pub(crate) fn accept_project_replace(&mut self) {
        self.project_replace = self.replace_preview.take();
        self.replace_files = 0;
        self.replace_conflicts = 0;
        self.close_overlay();
        self.panels.show_bottom(BottomPanel::Output);
        let _ = self.focus.focus(PrimaryFocus::BottomPanel, &self.panels);
    }
    pub(crate) fn cancel_project_replace(&mut self) {
        if self.project_replace.take().is_some() {
            self.workbench.push_output(format!(
                "Replacement stopped: {} files changed, {} conflicts; remaining files untouched",
                self.replace_files, self.replace_conflicts
            ));
        }
        self.replace_preview = None;
    }
    pub(crate) fn apply_project_replace_step(&mut self) -> bool {
        let Some(preview) = self.project_replace.as_mut() else {
            return false;
        };
        if let Some(outcome) = self
            .project_search
            .apply_next_replace(preview, &mut self.workbench)
        {
            if let Some(error) = outcome.error {
                self.replace_conflicts += 1;
                self.workbench
                    .push_output(format!("CONFLICT {}: {error}", outcome.path.display()));
            } else {
                if outcome.matches > 0 {
                    self.replace_files += 1;
                }
                self.workbench.push_output(format!(
                    "{} {}: {} edits",
                    if outcome.matches == 0 {
                        "Unchanged"
                    } else if outcome.saved_to_disk {
                        "Saved"
                    } else {
                        "Edited open buffer (not saved)"
                    },
                    outcome.path.display(),
                    outcome.matches
                ));
            }
        }
        if preview.files_remaining() == 0 {
            self.project_replace = None;
            self.project_results = false;
            self.status = format!(
                "Replacement finished: {} files changed, {} conflicts",
                self.replace_files, self.replace_conflicts
            );
            self.workbench.push_output(self.status.clone());
        } else {
            self.status = format!(
                "Replacing: {} files remain; Esc stops after current file",
                preview.files_remaining()
            );
        }
        true
    }
    pub(crate) fn select_project_result(&mut self) {
        match self
            .project_search
            .select(self.bottom_selected, &mut self.workbench)
        {
            Ok(()) => {
                let _ = self.focus.focus(PrimaryFocus::Editor, &self.panels);
                self.status = "Project result opened".into();
            }
            Err(error) => self.status = error,
        }
    }
}
fn patterns(text: &str) -> Vec<String> {
    text.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}
