//! Native intentions over the shared bounded source-capability host. There is
//! no lexer, source buffer, path interpretation or completion edit engine here.
use super::{
    git_panel::GitFrame,
    project_panel::ProjectPresentation,
    project_results::{ProjectResultRow, ProjectResultsPresentation},
    Action, State,
};
use jc_app::{
    edit_field, present_field,
    snippets::SnippetTarget,
    source_services::{
        display_label, display_ui_label, SourceController, SourceMode, SourcePresentation,
        SourceRegistry, SourceRow, SourceState,
    },
    CommandContext, CommandId, CompletedTask, FieldEdit, Modifiers,
};
use std::path::PathBuf;

struct RenameDraft {
    target: SnippetTarget,
    name: String,
    cursor: usize,
    reviewing: bool,
}

pub(super) struct SourceGui {
    registry: SourceRegistry,
    pub(super) controller: SourceController,
    visible: bool,
    candidates: Option<(Vec<PathBuf>, usize)>,
    rename: Option<RenameDraft>,
    epoch: u64,
}
impl SourceGui {
    pub(super) fn new() -> Self {
        let mut registry = SourceRegistry::default();
        registry
            .register(jc_progress::source_services::descriptor())
            .expect("proveedor interno de fuente válido");
        Self {
            registry,
            controller: SourceController::default(),
            visible: false,
            candidates: None,
            rename: None,
            epoch: 0,
        }
    }
    pub(super) fn registry(&self) -> &SourceRegistry {
        &self.registry
    }
    /// Include this in native deferred menu context. The controller owns the
    /// origin stamp; this records selection and candidate/UI intention only.
    pub(super) fn stamp(&self) -> (u64, SourceState, Option<SourceMode>, usize, bool) {
        (
            self.epoch,
            self.controller.state(),
            self.controller.mode(),
            self.controller.selected(),
            self.visible,
        )
    }
}
pub(super) fn is_command(command: CommandId) -> bool {
    matches!(
        command,
        CommandId::ViewSymbols
            | CommandId::SourceReferences
            | CommandId::SourceDefinition
            | CommandId::SourceLspReferences
            | CommandId::SourceLspComplete
            | CommandId::SourceSchemaComplete
            | CommandId::SourceLspDiagnostics
            | CommandId::SourceCalls
            | CommandId::SourceIncoming
            | CommandId::SourceDependencies
            | CommandId::SourceComplete
            | CommandId::SourceRename
            | CommandId::SourceRefresh
            | CommandId::SourceCancel
            | CommandId::SourceAccept
            | CommandId::SourceOpenToSide
    )
}
impl State {
    pub(super) fn source_visible(&self) -> bool {
        self.source_gui.visible
    }
    /// Hiding a native modal must not drop the controller/flight. Its eventual
    /// acknowledgement still returns through the application's sole F3 poll.
    pub(super) fn close_source_ui(&mut self) {
        if self.source_gui.visible || self.source_gui.candidates.is_some() {
            self.source_gui.epoch = self.source_gui.epoch.saturating_add(1);
        }
        self.source_gui.visible = false;
        self.source_gui.candidates = None;
        if self.source_gui.rename.take().is_some()
            && self.source_gui.controller.mode() == Some(SourceMode::Rename)
            && self.source_gui.controller.state() != SourceState::Cancelled
        {
            self.source_gui.controller.cancel(&mut self.jobs);
        }
    }
    pub(super) fn source_context(&self, context: &mut CommandContext) {
        let language = context.active_language.as_deref().unwrap_or("");
        let encoded = self.workbench.active_encoding().is_some();
        let registry = self.source_gui.registry();
        context.source_symbols_available = context.has_document
            && encoded
            && registry.resolve(language, SourceMode::Symbols).is_some();
        context.source_references_available = context.has_document
            && encoded
            && registry.resolve(language, SourceMode::References).is_some();
        context.source_completion_available = context.has_document
            && encoded
            && registry.resolve(language, SourceMode::Completion).is_some();
        context.source_schema_completion_available = self
            .progress_schema
            .supports_completion(&self.workbench, self.project_root.as_deref());
        context.source_rename_available = encoded
            && context.editor_input
            && !context.has_selection
            && !context.rectangular_selection
            && registry.supports_document(&self.workbench, SourceMode::Rename);
        context.source_calls_available =
            encoded && registry.resolve(language, SourceMode::Calls).is_some();
        context.source_dependencies_available = encoded
            && registry
                .resolve(language, SourceMode::Dependencies)
                .is_some();
        let controller = &self.source_gui.controller;
        context.source_incoming_available =
            registry.supports_incoming(&self.workbench, self.project_root.as_deref());
        context.source_lsp_available =
            registry.supports_lsp(&self.workbench, self.project_root.as_deref());
        let rename = self.source_gui.rename.is_some();
        let can_accept = !rename && controller.mode() != Some(SourceMode::Rename);
        context.source_active = rename
            || controller.mode().is_some()
                && !matches!(
                    controller.state(),
                    SourceState::Idle | SourceState::Cancelled
                );
        context.source_ready =
            can_accept && controller.ready(&self.workbench, self.project_root.as_deref());
        context.source_side_ready =
            can_accept && controller.side_ready(&self.workbench, self.project_root.as_deref());
        if self.source_visible() {
            context.editor_input = false;
        }
    }
    fn request_source(&mut self, mode: SourceMode) {
        let catalog = if mode == SourceMode::CatalogCompletion {
            let Some(provider) = self
                .progress_schema
                .completion(&self.workbench, self.project_root.as_deref())
            else {
                self.status =
                    "Actualiza el esquema ABL del proyecto para obtener sugerencias".into();
                return;
            };
            Some(provider)
        } else {
            None
        };
        self.close_selector();
        self.bottom.focused = false;
        self.explorer_focused = false;
        self.source_gui.candidates = None;
        self.source_gui.rename = None;
        self.source_gui.visible = mode != SourceMode::LspDiagnostics;
        if mode == SourceMode::LspDiagnostics {
            self.show_bottom(CommandId::ViewProblems);
        }
        self.source_gui.epoch = self.source_gui.epoch.saturating_add(1);
        let root = self.project_root.as_deref();
        let result = if let Some(provider) = catalog {
            self.source_gui.controller.request_catalog_completion(
                provider,
                &self.source_gui.registry,
                &self.workbench,
                root,
                &mut self.jobs,
            )
        } else {
            self.source_gui.controller.request(
                mode,
                &self.source_gui.registry,
                &self.workbench,
                root,
                &mut self.jobs,
            )
        };
        self.status = match result {
            Ok(()) => self.source_gui.controller.message().into(),
            Err(error) => error,
        };
    }
    pub(super) fn source_command(&mut self, command: CommandId) -> Action {
        match command {
            CommandId::ViewSymbols => self.request_source(SourceMode::Symbols),
            CommandId::SourceReferences => self.request_source(SourceMode::References),
            CommandId::SourceDefinition => self.request_source(SourceMode::Definition),
            CommandId::SourceLspReferences => self.request_source(SourceMode::LspReferences),
            CommandId::SourceLspComplete => self.request_source(SourceMode::LspCompletion),
            CommandId::SourceSchemaComplete => self.request_source(SourceMode::CatalogCompletion),
            CommandId::SourceLspDiagnostics => self.request_source(SourceMode::LspDiagnostics),
            CommandId::SourceCalls => self.request_source(SourceMode::Calls),
            CommandId::SourceIncoming => self.request_source(SourceMode::Incoming),
            CommandId::SourceDependencies => self.request_source(SourceMode::Dependencies),
            CommandId::SourceComplete => self.request_source(SourceMode::Completion),
            CommandId::SourceRename => self.start_rename(),
            CommandId::SourceRefresh => {
                if self.source_gui.rename.is_some()
                    || self.source_gui.controller.mode() == Some(SourceMode::Rename)
                {
                    return Action::None;
                }
                if let Some(mode) = self.source_gui.controller.mode() {
                    self.request_source(mode);
                }
            }
            CommandId::SourceCancel => {
                self.source_gui.controller.cancel(&mut self.jobs);
                self.close_source_ui();
                self.status = self.source_gui.controller.message().into();
            }
            CommandId::SourceAccept | CommandId::SourceOpenToSide => {
                if self.source_gui.rename.is_some()
                    || self.source_gui.controller.mode() == Some(SourceMode::Rename)
                {
                    return Action::None;
                }
                self.validate_source();
                let side = command == CommandId::SourceOpenToSide;
                let result = if let Some((paths, selected)) = &self.source_gui.candidates {
                    paths
                        .get(*selected)
                        .ok_or_else(|| "No hay candidato de referencia seleccionado".to_owned())
                        .and_then(|path| {
                            self.source_gui.controller.activate_candidate(
                                &mut self.workbench,
                                self.project_root.as_deref(),
                                path,
                                side,
                            )
                        })
                        .map(|()| None)
                } else {
                    self.source_gui.controller.activate(
                        &mut self.workbench,
                        self.project_root.as_deref(),
                        side,
                    )
                };
                match result {
                    Ok(Some(paths)) => {
                        self.source_gui.candidates = Some((paths, 0));
                        self.source_gui.visible = true;
                        self.source_gui.epoch = self.source_gui.epoch.saturating_add(1);
                        self.status =
                            "Referencia léxica ambigua: elige un candidato dentro del proyecto"
                                .into();
                    }
                    Ok(None) => {
                        self.close_selector();
                        self.close_source_ui();
                        self.bottom.focused = false;
                        self.explorer_focused = false;
                        self.ensure_encoding_selector();
                        self.status = if self.source_gui.controller.mode()
                            == Some(SourceMode::LspCompletion)
                        {
                            "Sugerencia semántica aplicada · un paso para deshacer"
                        } else if self.source_gui.controller.mode()
                            == Some(SourceMode::CatalogCompletion)
                        {
                            "Sugerencia del esquema aplicada · un paso para deshacer"
                        } else if self
                            .source_gui
                            .controller
                            .mode()
                            .is_some_and(SourceMode::is_lsp)
                        {
                            "Ubicación LSP abierta"
                        } else {
                            "Elemento aceptado; solo evidencia léxica"
                        }
                        .into();
                    }
                    Err(error) => self.error(error),
                }
            }
            _ => {}
        }
        Action::None
    }
    fn start_rename(&mut self) {
        let target = match SnippetTarget::capture(&self.workbench) {
            Ok(target) if !target.has_selection() => target,
            _ => {
                self.status = "Coloca el cursor sobre una variable de un documento ABL con codificación elegida".into();
                return;
            }
        };
        self.close_selector();
        self.bottom.focused = false;
        self.explorer_focused = false;
        self.source_gui.controller.cancel(&mut self.jobs);
        self.source_gui.candidates = None;
        self.source_gui.rename = Some(RenameDraft {
            target,
            name: String::new(),
            cursor: 0,
            reviewing: false,
        });
        self.source_gui.visible = true;
        self.source_gui.epoch = self.source_gui.epoch.saturating_add(1);
        self.status = "Escribe el nuevo nombre y pulsa Intro para revisar".into();
    }
    fn rename_key(&mut self, code: usize, modifiers: Modifiers) -> bool {
        let Some(draft) = self.source_gui.rename.as_mut() else {
            return false;
        };
        if draft.reviewing {
            if (code == 0x0d && modifiers == Modifiers::CTRL)
                || (code == 0x78 && modifiers == Modifiers::NONE)
            {
                if draft.target.current(&self.workbench)
                    && self.source_gui.controller.mode() == Some(SourceMode::Rename)
                    && self
                        .source_gui
                        .controller
                        .ready(&self.workbench, self.project_root.as_deref())
                {
                    match self.source_gui.controller.activate(
                        &mut self.workbench,
                        self.project_root.as_deref(),
                        false,
                    ) {
                        Ok(_) => {
                            self.close_source_ui();
                            self.status = "Símbolo renombrado en una operación de deshacer".into();
                        }
                        Err(error) => self.error(error),
                    }
                } else {
                    self.status = "La vista previa aún no está lista o cambió el destino".into();
                }
                return true;
            }
            if code == 0x71 && modifiers == Modifiers::NONE {
                draft.reviewing = false;
                self.source_gui.controller.cancel(&mut self.jobs);
                self.source_gui.epoch = self.source_gui.epoch.saturating_add(1);
                self.status = "Edita el nombre y pulsa Intro para revisar".into();
                return true;
            }
            return matches!(code, 0x0d | 0x74 | 0x78);
        }
        if code == 0x0d && modifiers == Modifiers::NONE {
            if !draft.target.current(&self.workbench) {
                self.status = "Cambió el destino; cancela y vuelve a abrir".into();
                return true;
            }
            match self.source_gui.controller.request_rename(
                &draft.name,
                &self.source_gui.registry,
                &self.workbench,
                self.project_root.as_deref(),
                &mut self.jobs,
            ) {
                Ok(()) => {
                    draft.reviewing = true;
                    self.source_gui.epoch = self.source_gui.epoch.saturating_add(1);
                    self.status = self.source_gui.controller.message().into();
                }
                Err(error) => self.error(error),
            }
            return true;
        }
        let edit = match (code, modifiers) {
            (0x08, Modifiers::NONE) => Some(FieldEdit::Backspace),
            (0x2e, Modifiers::NONE) => Some(FieldEdit::Delete),
            (0x24, Modifiers::NONE) => Some(FieldEdit::Home),
            (0x23, Modifiers::NONE) => Some(FieldEdit::End),
            (0x25, Modifiers::NONE) => Some(FieldEdit::Left),
            (0x27, Modifiers::NONE) => Some(FieldEdit::Right),
            (0x55, Modifiers::CTRL) => Some(FieldEdit::Clear),
            _ => None,
        };
        if let Some(edit) = edit {
            self.edit_rename(edit);
        }
        true
    }
    fn edit_rename(&mut self, edit: FieldEdit<'_>) {
        let Some(draft) = self.source_gui.rename.as_mut() else {
            return;
        };
        if draft.reviewing || !draft.target.current(&self.workbench) {
            return;
        }
        match edit_field(&mut draft.name, &mut draft.cursor, edit, 128) {
            Ok(_) => self.source_gui.epoch = self.source_gui.epoch.saturating_add(1),
            Err(error) => self.status = error.into(),
        }
    }
    pub(super) fn source_key(&mut self, code: usize, modifiers: Modifiers) -> Action {
        self.validate_source();
        if code == 0x1b {
            return self.source_command(CommandId::SourceCancel);
        }
        if self.rename_key(code, modifiers) {
            return Action::None;
        }
        match code {
            0x74 if modifiers == Modifiers::NONE => {
                return self.source_command(CommandId::SourceRefresh)
            }
            0x0d if modifiers == Modifiers::NONE || modifiers == Modifiers::CTRL => {
                return self.source_command(if modifiers == Modifiers::CTRL {
                    CommandId::SourceOpenToSide
                } else {
                    CommandId::SourceAccept
                })
            }
            _ => {}
        }
        if modifiers != Modifiers::NONE {
            return Action::None;
        }
        let delta = match code {
            0x26 => -1,
            0x28 => 1,
            0x21 => -10,
            0x22 => 10,
            _ => 0,
        };
        if let Some((paths, index)) = &mut self.source_gui.candidates {
            *index = match code {
                0x24 => 0,
                0x23 => paths.len().saturating_sub(1),
                _ => index
                    .saturating_add_signed(delta)
                    .min(paths.len().saturating_sub(1)),
            };
        } else {
            match code {
                0x24 => self.source_gui.controller.first(),
                0x23 => self.source_gui.controller.last(),
                _ => self.source_gui.controller.select_relative(delta),
            }
        }
        if matches!(code, 0x21..=0x24 | 0x26 | 0x28) {
            self.source_gui.epoch = self.source_gui.epoch.saturating_add(1);
        }
        Action::None
    }
    pub(super) fn source_character(&mut self, ch: char) {
        if !ch.is_control() {
            self.edit_rename(FieldEdit::Insert(ch.encode_utf8(&mut [0; 4])));
        }
    }
    pub(super) fn validate_source(&mut self) -> bool {
        let changed = self.source_gui.controller.validate_context(
            &self.workbench,
            self.project_root.as_deref(),
            &mut self.jobs,
        );
        if changed {
            self.source_gui.candidates = None;
            self.source_gui.epoch = self.source_gui.epoch.saturating_add(1);
            if self.source_visible() {
                self.status = self.source_gui.controller.message().into();
            }
        }
        changed
    }
    pub(super) fn accept_source(&mut self, completion: &mut CompletedTask) -> bool {
        let consumed = self.source_gui.controller.accept_editor(
            &mut self.workbench,
            self.project_root.as_deref(),
            completion,
        );
        if consumed
            && (self.source_visible()
                || self.source_gui.controller.mode() == Some(SourceMode::LspDiagnostics))
        {
            self.status = self.source_gui.controller.message().into();
        }
        consumed
    }
    /// Called by the owner event loop after completion dispatch, never from a
    /// presentation/paint route. Busy keeps only the controller's metadata.
    pub(super) fn tick_source(&mut self) -> bool {
        let changed = self.source_gui.controller.tick(
            &self.workbench,
            self.project_root.as_deref(),
            &mut self.jobs,
        );
        if changed && self.source_visible() {
            self.status = self.source_gui.controller.message().into();
        }
        changed
    }
    pub(super) fn source_presentation(&mut self) -> Option<GitFrame> {
        self.validate_source();
        if !self.source_visible() {
            return None;
        }
        let (columns, _) = super::project_paint::field_geometry(
            self.width,
            (self.height, self.dpi.pixels(super::chrome::TOP)),
            self.cell_width,
            self.cell_height,
            1,
        );
        let rows = super::project_paint::result_capacity(
            self.width,
            (self.height, self.dpi.pixels(super::chrome::TOP)),
            self.cell_width,
            self.cell_height,
        );
        if let Some(draft) = self
            .source_gui
            .rename
            .as_ref()
            .filter(|draft| !draft.reviewing)
        {
            return Some(GitFrame {
                panel: ProjectPresentation {
                    fields: vec![(
                        "Nuevo nombre",
                        present_field(&draft.name, draft.cursor, columns, 1),
                    )],
                    active: 0,
                    status: display_ui_label(&self.status, columns),
                    results: None,
                },
                labels: super::project_paint::PanelLabels {
                    draft: "Renombrar símbolo ABL",
                    policy: "Ámbito ABL | El documento sigue intacto",
                    footer: "Intro revisa | Ctrl+U vacía | Esc cancela",
                },
            });
        }
        let reviewing = self.source_gui.rename.is_some();
        let (presentation, selected, total) =
            if let Some((paths, selected)) = &self.source_gui.candidates {
                let count = rows.min(64);
                let first = selected.saturating_sub(count.saturating_sub(1));
                (
                    SourcePresentation {
                        title: display_ui_label("Candidatos de referencia", columns),
                        message: display_ui_label(
                            "Elige explícitamente; el fuente sigue intacto",
                            columns,
                        ),
                        rows: paths
                            .iter()
                            .enumerate()
                            .skip(first)
                            .take(count)
                            .map(|(index, path)| SourceRow {
                                index,
                                label: display_label(
                                    &self
                                        .project_root
                                        .as_deref()
                                        .and_then(|root| path.strip_prefix(root).ok())
                                        .unwrap_or(path)
                                        .display()
                                        .to_string(),
                                    columns,
                                ),
                                selected: index == *selected,
                            })
                            .collect(),
                    },
                    *selected,
                    paths.len(),
                )
            } else {
                let controller = &self.source_gui.controller;
                (
                    controller.present(rows, columns),
                    controller.selected(),
                    controller.items().len(),
                )
            };
        Some(GitFrame {
            panel: ProjectPresentation {
                fields: Vec::new(),
                active: 0,
                status: display_ui_label(&self.status, columns),
                results: Some(ProjectResultsPresentation {
                    title: presentation.title,
                    message: presentation.message,
                    rows: presentation
                        .rows
                        .into_iter()
                        .map(|row| ProjectResultRow {
                            index: row.index,
                            label: row.label,
                            selected: row.selected,
                            is_match: true,
                        })
                        .collect(),
                    selected,
                    total,
                }),
            },
            labels: super::project_paint::PanelLabels {
                draft: "Servicios locales de código",
                policy:
                    "Evidencia léxica | captura acotada | aceptación explícita | sin AVM/indexador",
                footer: if reviewing {
                    "F9/Ctrl+Intro aplica todo | F2 edita | Flechas recorren | Esc cancela"
                } else {
                    "Flechas/Inicio/Fin eligen | Intro acepta | Ctrl+Intro al lado | F5 actualiza | Esc"
                },
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GuiOptions;
    use jc_text::TextEncoding;
    use std::time::{Duration, Instant};

    fn editor(path: &str, source: &str, encoding: TextEncoding) -> State {
        let mut state = State::new(&GuiOptions::default()).unwrap();
        state.workbench.new_file(path, encoding);
        state.workbench.insert_text(source).unwrap();
        state
    }
    fn finish(state: &mut State) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while matches!(
            state.source_gui.controller.state(),
            SourceState::Queued | SourceState::Running
        ) || state.jobs.outstanding() > 0
        {
            state.poll_tasks();
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
    }
    #[test]
    #[ignore = "requiere el proyecto sintético de esquema de OpenEdge"]
    fn progress_real_schema_completion_gui() {
        let root = PathBuf::from(std::env::var_os("CIDE_TEST_PROGRESS_SCHEMA_PROJECT").unwrap());
        let mut state = State::new(&GuiOptions {
            project_root: Some(root.clone()),
            ..GuiOptions::default()
        })
        .unwrap();
        state
            .workbench
            .new_file(root.join("completar-gui.p"), TextEncoding::Utf8);
        assert!(!state.context().source_schema_completion_available);
        state.execute_menu(CommandId::ProgressRefreshSchema);
        let deadline = Instant::now() + Duration::from_secs(10);
        while state.progress_schema.busy() {
            state.poll_tasks();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        state.bottom.focused = false;
        assert!(
            state.context().source_schema_completion_available,
            "{}",
            state.status
        );
        for (source, label, suffix) in [
            (
                "FOR EACH negocio.cust",
                "Tabla · devlogica.customer",
                "omer",
            ),
            ("DISPLAY negocio.customer.na", "Campo · name", "me"),
            (
                "FIND FIRST negocio.customer USE-INDEX cust",
                "Índice · cust-num",
                "-num",
            ),
        ] {
            state.workbench.new_file(
                root.join(format!("gui-{}.p", source.len())),
                TextEncoding::Utf8,
            );
            state.workbench.insert_text(source).unwrap();
            state.execute_menu(CommandId::SourceSchemaComplete);
            finish(&mut state);
            let panel = state.source_presentation().unwrap().panel.results.unwrap();
            let index = state
                .source_gui
                .controller
                .items()
                .iter()
                .position(|item| item.label.to_lowercase() == label.to_lowercase())
                .unwrap_or_else(|| panic!("{source}: {:?}", state.source_gui.controller.items()));
            assert!(panel
                .rows
                .iter()
                .any(|row| row.label.to_lowercase().contains(&label.to_lowercase())));
            state.source_gui.controller.select_relative(index as isize);
            state.execute_menu(CommandId::SourceAccept);
            assert!(!state.source_visible());
            assert_eq!(
                String::from_utf8(state.workbench.active_bytes().unwrap())
                    .unwrap()
                    .to_lowercase(),
                format!("{source}{suffix}").to_lowercase()
            );
            state.workbench.undo().unwrap();
            assert_eq!(state.workbench.active_bytes().unwrap(), source.as_bytes());
        }
        state.execute_menu(CommandId::SourceSchemaComplete);
        finish(&mut state);
        assert!(state.context().source_ready);
        let before = state.workbench.active_bytes().unwrap();
        state.progress_schema.clear();
        assert!(!state.context().source_ready);
        state.execute_menu(CommandId::SourceAccept);
        assert_eq!(state.workbench.active_bytes().unwrap(), before);
    }
    #[test]
    #[ignore = "requires a configured local LSP server and fixture project"]
    fn source_gui_lsp_language_services() {
        let root = PathBuf::from(std::env::var_os("CIDE_LSP_TEST_PROJECT").unwrap());
        let path = root.join(std::env::var_os("CIDE_LSP_TEST_FILE").unwrap());
        let text = std::fs::read_to_string(&path).unwrap();
        let mut state = State::new(&GuiOptions {
            paths: vec![path.clone()],
            project_root: Some(root),
            encoding: Some(TextEncoding::Utf8),
            ..GuiOptions::default()
        })
        .unwrap();
        let tab = state.workbench.tabs().active_id().unwrap();
        assert!(state
            .workbench
            .jump_to_location(tab, text.find(".calcu").unwrap() + 6));
        state.execute(CommandId::SourceLspComplete);
        finish(&mut state);
        assert!(state.source_visible());
        assert!(state
            .source_presentation()
            .unwrap()
            .panel
            .results
            .unwrap()
            .rows
            .iter()
            .any(|row| row.label.contains("calcular")));
        let index = state
            .source_gui
            .controller
            .items()
            .iter()
            .position(|item| item.label == "calcular")
            .unwrap();
        state.source_gui.controller.select_relative(index as isize);
        state.execute(CommandId::SourceAccept);
        assert!(String::from_utf8(state.workbench.active_bytes().unwrap())
            .unwrap()
            .contains("data.calcular"));
        state.workbench.undo().unwrap();
        assert_eq!(state.workbench.active_bytes().unwrap(), text.as_bytes());
        state.execute(CommandId::SourceLspDiagnostics);
        finish(&mut state);
        assert!(
            state.bottom.visible && state.bottom.kind == jc_app::task_panels::PanelKind::Problems
        );
        assert!(
            state
                .workbench
                .task_panels()
                .rows(state.bottom.kind)
                .iter()
                .any(|row| row.text.contains("missingCideSymbol")),
            "{}",
            state.status
        );
        assert_eq!(std::fs::read_to_string(path).unwrap(), text);
    }

    #[test]
    #[ignore = "requires a configured local LSP server and fixture project"]
    fn source_gui_lsp_navigation() {
        let root = PathBuf::from(std::env::var_os("CIDE_LSP_TEST_PROJECT").unwrap());
        let path = root.join(std::env::var_os("CIDE_LSP_TEST_FILE").unwrap());
        let on_disk = path.clone();
        let destination = root.join(std::env::var_os("CIDE_LSP_TEST_DEST").unwrap());
        let text = std::fs::read_to_string(&path).unwrap();
        let mut state = State::new(&GuiOptions {
            paths: vec![path],
            project_root: Some(root),
            encoding: Some(TextEncoding::Utf8),
            ..GuiOptions::default()
        })
        .unwrap();
        let tab = state.workbench.tabs().active_id().unwrap();
        let pane = state.workbench.panes().active();
        assert!(state
            .workbench
            .jump_to_pane_location(pane, tab, text.rfind("calcular").unwrap()));
        state.execute(CommandId::SourceLspReferences);
        finish(&mut state);
        assert!(
            state.source_gui.controller.items().len() >= 2,
            "{}",
            state.source_gui.controller.message()
        );
        assert!(
            state
                .source_presentation()
                .unwrap()
                .panel
                .results
                .unwrap()
                .rows
                .len()
                >= 2
        );
        state.workbench.insert_text("/* 😀😀😀😀😀😀 */ ").unwrap();
        state.execute(CommandId::SourceDefinition);
        finish(&mut state);
        assert_eq!(
            state.source_gui.controller.items().len(),
            1,
            "{}",
            state.source_gui.controller.message()
        );
        state.execute(CommandId::SourceOpenToSide);
        assert!(state.workbench.panes().is_split());
        assert_eq!(
            state.workbench.active_path().unwrap(),
            destination.canonicalize().unwrap()
        );
        assert_eq!(std::fs::read_to_string(on_disk).unwrap(), text);
    }
    #[test]
    fn source_gui_symbols_deliver_without_input_and_navigation_preserves_bytes() {
        let source = "/* PROCEDURE hidden: */\r\nPROCEDURE alpha:\r\nEND PROCEDURE.\r\nFUNCTION beta RETURNS INTEGER():\r\nEND FUNCTION.";
        let mut state = editor("gui-source.p", source, TextEncoding::Windows1252);
        state.source_command(CommandId::ViewSymbols);
        finish(&mut state);
        assert_eq!(state.source_gui.controller.state(), SourceState::Ready);
        assert_eq!(state.source_gui.controller.items().len(), 2);
        let before = state.workbench.active_bytes().unwrap();
        let outstanding = state.jobs.outstanding();
        for _ in 0..2 {
            let frame = state.source_presentation().unwrap();
            assert!(frame
                .panel
                .results
                .unwrap()
                .rows
                .iter()
                .any(|row| row.label.contains("alpha")));
        }
        assert_eq!(state.jobs.outstanding(), outstanding);
        state.source_key(0x28, Modifiers::NONE);
        state.source_key(0x0d, Modifiers::NONE);
        assert_eq!(
            state.workbench.active_editor().unwrap().cursor,
            source.find("beta RETURNS").unwrap()
        );
        assert_eq!(state.workbench.active_bytes().unwrap(), before);
        assert!(!state.source_visible());
    }
    #[test]
    fn source_gui_completion_blocks_typing_and_accepts_exactly_one_undo() {
        let source = "customer custom\r\ncus";
        let mut state = editor("gui-source.rs", source, TextEncoding::Utf8);
        state.source_command(CommandId::SourceComplete);
        finish(&mut state);
        assert_eq!(state.source_gui.controller.items().len(), 2);
        state.source_character('x');
        state.source_key(0x2e, Modifiers::NONE);
        assert_eq!(state.workbench.active_bytes().unwrap(), source.as_bytes());
        state.source_key(0x0d, Modifiers::NONE);
        assert_eq!(
            state.workbench.active_bytes().unwrap(),
            b"customer custom\r\ncustomer"
        );
        state.workbench.undo().unwrap();
        assert_eq!(state.workbench.active_bytes().unwrap(), source.as_bytes());
    }
    #[test]
    fn source_gui_rename_requires_explicit_confirmation_of_preview_and_one_undo() {
        let source =
            "DEFINE VARIABLE contador AS INTEGER NO-UNDO.\r\nASSIGN contador = contador + 1.";
        let mut state = editor("gui-rename.p", source, TextEncoding::Windows1252);
        state
            .workbench
            .go_to_position(jc_text::TextPosition {
                line: 0,
                column: 17,
            })
            .unwrap();
        state.source_command(CommandId::SourceRename);
        for ch in "acumulado".chars() {
            state.source_character(ch);
        }
        assert_eq!(state.source_presentation().unwrap().panel.fields.len(), 1);
        state.source_key(0x0d, Modifiers::NONE);
        finish(&mut state);
        assert_eq!(
            state.source_gui.controller.state(),
            SourceState::Ready,
            "{}",
            state.status
        );
        assert_eq!(state.source_gui.controller.items().len(), 3);
        state.source_key(0x0d, Modifiers::NONE);
        state.source_command(CommandId::SourceAccept);
        assert_eq!(state.workbench.active_bytes().unwrap(), source.as_bytes());
        state.source_key(0x0d, Modifiers::CTRL);
        assert_eq!(
            state.workbench.active_bytes().unwrap(),
            source.replace("contador", "acumulado").as_bytes()
        );
        assert!(!state.source_visible());
        state.workbench.undo().unwrap();
        assert_eq!(state.workbench.active_bytes().unwrap(), source.as_bytes());
    }
    #[test]
    fn source_gui_stale_completion_and_cancel_cannot_edit_new_caret_intention() {
        let source = "customer custom cus";
        let mut state = editor("gui-source.txt", source, TextEncoding::Utf8);
        state.source_command(CommandId::SourceComplete);
        state.workbench.move_left(false).unwrap();
        finish(&mut state);
        state.source_presentation();
        assert_eq!(state.source_gui.controller.state(), SourceState::Stale);
        state.source_command(CommandId::SourceAccept);
        assert_eq!(state.workbench.active_bytes().unwrap(), source.as_bytes());
        state.source_command(CommandId::SourceCancel);
        finish(&mut state);
        assert!(!state.source_visible());
        assert_eq!(state.source_gui.controller.state(), SourceState::Cancelled);
    }
    #[test]
    fn source_gui_registry_keeps_lexical_capability_optional_and_missing_provider_explicit() {
        let mut state = editor("gui-source.json", "{}", TextEncoding::Utf8);
        let mut context = state.workbench.context();
        state.source_context(&mut context);
        assert!(!context.source_symbols_available);
        assert!(!context.source_references_available);
        assert!(context.source_completion_available);
        assert!(state
            .source_gui
            .registry()
            .snippets("abl")
            .iter()
            .any(|t| !t.surround));
        state.source_command(CommandId::ViewSymbols);
        assert_eq!(state.source_gui.controller.state(), SourceState::Failed);
        assert_eq!(state.jobs.outstanding(), 0);
        assert_eq!(state.workbench.active_bytes().unwrap(), b"{}");
    }
}
