use super::TuiApp;
use crate::state::{PromptKind, UiOverlay};
use jc_app::Overlay;
use jc_text::TextEncoding;
use std::path::PathBuf;

impl TuiApp {
    pub(super) fn request_save_all(&mut self) {
        if self.workbench.save_pending() || self.save_batch.is_some() {
            self.status = "Ya hay un guardado en curso".into();
            return;
        }
        let batch: std::collections::VecDeque<_> =
            self.workbench.modified_documents().take(1025).collect();
        if batch.len() > 1024 {
            self.status = "Guardar todo excede el límite de 1024 documentos".into();
            return;
        }
        self.save_batch = Some(batch);
        self.continue_save_all();
    }

    fn continue_save_all(&mut self) {
        while let Some((id, revision, is_new)) =
            self.save_batch.as_mut().and_then(|batch| batch.pop_front())
        {
            let result = self
                .workbench
                .capture_save_target(id, revision)
                .and_then(|target| {
                    let destination = is_new.then(|| {
                        self.workbench
                            .document_by_id(id)
                            .unwrap()
                            .session
                            .path()
                            .to_owned()
                    });
                    self.workbench.request_save_target(target, destination)
                });
            match result {
                Ok(true) => continue,
                Ok(false) => {
                    self.status = "Guardar todo · guardando archivo en segundo plano…".into();
                    return;
                }
                Err(error) => {
                    self.save_batch = None;
                    self.exit_waiting_for_save = false;
                    self.periodic_recovery.cancel_final();
                    self.status = format!("Guardar todo detenido: {error}");
                    return;
                }
            }
        }
        self.save_batch = None;
        self.status = "Guardar todo completado".into();
    }

    pub(super) fn poll_save(&mut self) -> bool {
        let Some(result) = self.workbench.poll_save() else {
            return false;
        };
        let closing = self.save_closing_tab.take();
        match result {
            Ok(id) => {
                self.periodic_recovery
                    .note_activity(std::time::Instant::now());
                let dirty = self
                    .workbench
                    .document_by_id(id)
                    .is_some_and(|doc| doc.session.document().is_modified());
                self.status = if dirty {
                    "Guardado · hay cambios posteriores pendientes"
                } else {
                    "Guardado"
                }
                .into();
                if closing.is_some() && closing == self.workbench.tabs().active_id() && !dirty {
                    match self.workbench.close_active(false) {
                        Ok(None) => self.status = "Guardado y cerrado".into(),
                        Ok(Some(_)) => {}
                        Err(error) => self.status = error.to_string(),
                    }
                }
                if self.save_batch.is_some() {
                    self.continue_save_all();
                }
                if self.exit_waiting_for_save
                    && !self.workbench.save_pending()
                    && self.save_batch.is_none()
                {
                    self.request_exit();
                }
            }
            Err(error) => {
                let batch = self.save_batch.take().is_some();
                self.exit_waiting_for_save = false;
                self.periodic_recovery.cancel_final();
                self.status = if batch {
                    format!("Guardar todo detenido: {error}")
                } else {
                    error.to_string()
                };
            }
        }
        true
    }

    pub(crate) fn open_prompt(&mut self, kind: PromptKind) {
        self.open_focus_overlay(Overlay::Dialog);
        let input = self.start_file_browser(kind).unwrap_or_default();
        self.overlay = UiOverlay::Prompt { kind, input };
        if kind == PromptKind::GoToPosition && self.workbench.active_paged_byte().is_some() {
            self.status = "Usa @byte, desde cero".into();
        }
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
        if kind == PromptKind::GoToPosition {
            if self.workbench.active_paged_byte().is_some() {
                match self.workbench.go_to_paged_input(&input) {
                    Ok(()) => {
                        self.close_overlay();
                        let _ = self.focus.focus(jc_app::PrimaryFocus::Editor, &self.panels);
                        self.status = "Destino solicitado".into();
                    }
                    Err(error) => self.status = error.to_string(),
                }
                return;
            }
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
        if kind == PromptKind::GitExportPatch {
            self.submit_git_patch(path);
            return;
        }
        if matches!(kind, PromptKind::Open) {
            match self
                .workbench
                .request_file_open(path, self.default_encoding, false)
            {
                Ok(opened) => self.finish_file_open(opened.is_some(), "Listo"),
                Err(error) => self.status = error.to_string(),
            }
            return;
        }
        let result: Result<(), String> = match kind {
            PromptKind::GoToPosition
            | PromptKind::RenameSymbol
            | PromptKind::GitExportPatch
            | PromptKind::Open => {
                unreachable!()
            }
            PromptKind::New => {
                let encoding = self.default_encoding.unwrap_or(TextEncoding::Utf8);
                self.workbench.new_file(path, encoding);
                Ok(())
            }
            PromptKind::SaveAs => {
                match self.workbench.request_save_active(Some(path)) {
                    Ok(completed) => {
                        self.close_overlay();
                        self.status = if completed {
                            "Guardado"
                        } else {
                            "Guardando archivo en segundo plano…"
                        }
                        .into();
                    }
                    Err(error) => self.status = error.to_string(),
                }
                return;
            }
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

    pub(crate) fn open_startup_files(&mut self, files: Vec<PathBuf>) {
        self.workbench
            .set_large_open_wakeup(self.syntax_wakeup.clone());
        self.startup_files.extend(files);
        self.advance_startup_files();
    }

    fn advance_startup_files(&mut self) {
        while !self.workbench.large_open_pending() {
            let Some(path) = self.startup_files.pop_front() else {
                return;
            };
            if let Err(error) = self.open_path(path) {
                self.status = error;
            }
        }
    }

    pub(crate) fn open_path(&mut self, path: PathBuf) -> Result<(), String> {
        let opened = self
            .workbench
            .request_file_open(path, self.default_encoding, false)
            .map_err(|error| error.to_string())?;
        if opened.is_some() {
            self.ensure_encoding_choice();
        } else {
            self.status = "Copiando archivo grande en segundo plano…".into();
        }
        Ok(())
    }

    pub(super) fn finish_file_open(&mut self, opened: bool, message: &str) {
        self.close_overlay();
        if opened {
            self.status = message.to_owned();
            self.ensure_encoding_choice();
            if matches!(self.overlay, UiOverlay::None) {
                let _ = self.focus.focus(jc_app::PrimaryFocus::Editor, &self.panels);
            }
        } else {
            self.status = "Copiando archivo grande en segundo plano…".into();
        }
    }

    pub(super) fn poll_large_file(&mut self) -> bool {
        if !matches!(self.overlay, UiOverlay::None) {
            return false;
        }
        let Some(result) = self.workbench.poll_large_open() else {
            return false;
        };
        match result {
            Ok(tab)
                if self
                    .workbench
                    .active_document()
                    .is_some_and(|doc| doc.tab_id == tab) =>
            {
                self.finish_file_open(true, "Archivo abierto");
            }
            Ok(_) => self.status = "Archivo grande abierto en una pestaña".into(),
            Err(error) => self.status = error.to_string(),
        }
        self.advance_startup_files();
        self.periodic_recovery
            .note_activity(std::time::Instant::now());
        true
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
