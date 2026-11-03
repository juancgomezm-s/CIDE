use super::TuiApp;
use crate::state::PromptKind;
use jc_app::{BottomPanel, PrimaryFocus, ProjectSearchOptions};

impl TuiApp {
    pub(crate) fn submit_project_prompt(&mut self, kind: PromptKind, input: String) {
        if input.len() > 4096 {
            self.status = "La consulta/filtro del proyecto supera 4096 bytes".into();
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
                            "Revisa la copia de búsqueda; se comprobarán versiones antes de cada archivo"
                                .into();
                    }
                    Err(error) => self.status = error,
                }
            }
            PromptKind::ProjectQuery => {
                if input.is_empty() {
                    self.status = "El texto de búsqueda no puede estar vacío".into();
                    return;
                }
                let mut draft = ProjectSearchOptions::literal(input);
                draft.workers = self.project_search_workers;
                draft.regex = self.workbench.search().regex;
                self.project_draft = Some(draft);
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
                    self.status = "Abre primero una carpeta de proyecto".into();
                    return;
                };
                match self.project_search.start(&self.workbench, root, draft) {
                    Ok(()) => {
                        self.close_overlay();
                        self.project_results = true;
                        self.bottom_selected = 0;
                        self.panels.show_bottom(BottomPanel::SearchResults);
                        let _ = self.focus.focus(PrimaryFocus::BottomPanel, &self.panels);
                        self.status = "Buscando en el proyecto; Esc cancela".into();
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
                "Reemplazo detenido: {} archivos cambiados, {} conflictos; restantes sin cambios",
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
                    .push_output(format!("CONFLICTO {}: {error}", outcome.path.display()));
            } else {
                if outcome.matches > 0 {
                    self.replace_files += 1;
                }
                self.workbench.push_output(format!(
                    "{} {}: {} ediciones",
                    if outcome.matches == 0 {
                        "Sin cambios"
                    } else if outcome.saved_to_disk {
                        "Guardado"
                    } else {
                        "Documento abierto editado (sin guardar)"
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
                "Reemplazo terminado: {} archivos cambiados, {} conflictos",
                self.replace_files, self.replace_conflicts
            );
            self.workbench.push_output(self.status.clone());
        } else {
            self.status = format!(
                "Reemplazando: quedan {} archivos; Esc detiene después del actual",
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
                self.status = "Resultado del proyecto abierto".into();
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

#[cfg(test)]
mod worker_settings_tests {
    use super::*;
    use crate::LaunchOptions;

    #[test]
    fn launch_budget_reaches_every_new_project_query_without_starting_work() {
        for workers in [1, 2, 4] {
            let mut app = TuiApp::with_search_workers(LaunchOptions::default(), workers).unwrap();
            for query in ["first", "second"] {
                app.submit_project_prompt(PromptKind::ProjectQuery, query.into());
                let draft = app.project_draft.as_ref().unwrap();
                assert_eq!(draft.query, query);
                assert_eq!(draft.workers, workers);
                assert_eq!(app.project_search.state, jc_app::ProjectSearchState::Idle);
                assert!(!app.project_search.ready());
            }
        }
    }

    #[test]
    fn invalid_cpu_budget_is_rejected_before_entering_a_terminal() {
        for workers in [0, 5] {
            assert!(TuiApp::with_search_workers(LaunchOptions::default(), workers).is_err());
            let error =
                crate::run_with_search_workers(LaunchOptions::default(), workers).unwrap_err();
            assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        }
    }
}
