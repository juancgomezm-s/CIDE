use super::TuiApp;
use crate::state::{PromptKind, UiOverlay};
use jc_app::Overlay;
use jc_text::TextEncoding;
use std::path::PathBuf;

impl TuiApp {
    pub(crate) fn open_prompt(&mut self, kind: PromptKind) {
        self.open_focus_overlay(Overlay::Dialog);
        let input = self.start_file_browser(kind).unwrap_or_default();
        self.overlay = UiOverlay::Prompt { kind, input };
    }

    pub(crate) fn submit_prompt(&mut self, kind: PromptKind, input: String) {
        let limit = super::overlays::prompt_limit(kind);
        if input.len() > limit {
            self.status = format!("La entrada supera {limit} bytes UTF-8");
            return;
        }
        if kind == PromptKind::RenameSymbol {
            self.preview_symbol_rename(&input);
            return;
        }
        if kind == PromptKind::GitExecutable {
            let path = PathBuf::from(input.trim());
            if input.len() > 4096 || !path.is_absolute() {
                self.status = "Usa una ruta absoluta al ejecutable Git (máximo 4096 bytes)".into();
                return;
            }
            self.git_diff.clear(&mut self.tasks);
            self.git_status.clear();
            let target = self.git_configuration_target.take();
            self.git_executable = Some(path);
            self.close_overlay();
            match target {
                Some(super::git_repository::GitConfigurationTarget::Diff) => {
                    self.request_git_diff(self.git_source)
                }
                Some(super::git_repository::GitConfigurationTarget::Status) => {
                    self.request_git_repository_status()
                }
                None => self.status = "Ejecutable Git configurado".into(),
            }
            return;
        }
        if kind == PromptKind::GoToPosition {
            if let Some(position) = jc_app::parse_text_position(&input) {
                match self.workbench.go_to_position(position) {
                    Ok(()) => {
                        self.close_overlay();
                        let _ = self.focus.focus(jc_app::PrimaryFocus::Editor, &self.panels);
                        self.status = "Posición seleccionada".into();
                    }
                    Err(error) => self.status = error.to_string(),
                }
            } else {
                self.status = "Usa línea[:columna], coordenadas escalares desde uno".into();
            }
            return;
        }
        if matches!(
            kind,
            PromptKind::ProjectQuery
                | PromptKind::ProjectInclude
                | PromptKind::ProjectExclude
                | PromptKind::ProjectReplacement
        ) {
            self.submit_project_prompt(kind, input);
            return;
        }
        let value = input.trim();
        if value.is_empty() {
            self.status = "La ruta no puede estar vacía".to_owned();
            return;
        }
        let path = PathBuf::from(value);
        if matches!(kind, PromptKind::Open) {
            match self.workbench.open_file(path, self.default_encoding) {
                Ok(_) => {
                    self.close_overlay();
                    self.status = "Listo".to_owned();
                    self.ensure_encoding_choice();
                }
                Err(error) => self.status = error.to_string(),
            }
            return;
        }
        let result: Result<(), String> = match kind {
            PromptKind::GoToPosition | PromptKind::GitExecutable | PromptKind::RenameSymbol => {
                unreachable!()
            }
            PromptKind::Open => self.open_path(path),
            PromptKind::New => {
                let encoding = self.default_encoding.unwrap_or(TextEncoding::Utf8);
                self.workbench.new_file(path, encoding);
                Ok(())
            }
            PromptKind::SaveAs => self
                .workbench
                .save_active_as(path)
                .map_err(|e| e.to_string()),
            PromptKind::OpenFolder => self.set_explorer_root(path),
            PromptKind::ProjectQuery
            | PromptKind::ProjectInclude
            | PromptKind::ProjectExclude
            | PromptKind::ProjectReplacement => {
                unreachable!()
            }
        };
        match result {
            Ok(()) => {
                self.close_overlay();
                self.status = "Listo".to_owned();
            }
            Err(error) => self.status = error,
        }
    }

    pub(crate) fn open_path(&mut self, path: PathBuf) -> Result<(), String> {
        self.workbench
            .open_file(path, self.default_encoding)
            .map_err(|error| error.to_string())?;
        self.ensure_encoding_choice();
        Ok(())
    }

    pub(crate) fn ensure_encoding_choice(&mut self) {
        if self.workbench.active_document().is_some() && self.workbench.active_encoding().is_none()
        {
            self.open_focus_overlay(Overlay::Dialog);
            self.overlay = UiOverlay::EncodingChoice;
            self.status = "Debes elegir explícitamente la codificación heredada".to_owned();
        }
    }

    pub(crate) fn choose_encoding(&mut self, encoding: TextEncoding) {
        match self.workbench.set_encoding(encoding) {
            Ok(()) => {
                self.status = format!("Codificación: {}", encoding.name());
                self.close_overlay();
            }
            Err(error) => self.status = error.to_string(),
        }
    }
}
