//! Native intentions over the shared bounded source-capability host. There is
//! no lexer, source buffer, path interpretation or completion edit engine here.
use super::{
    git_panel::GitFrame,
    project_panel::ProjectPresentation,
    project_results::{ProjectResultRow, ProjectResultsPresentation},
    Action, State,
};
use jc_app::{
    source_services::{
        display_label, display_ui_label, SourceController, SourceMode, SourcePresentation,
        SourceRegistry, SourceRow, SourceState,
    },
    CommandContext, CommandId, CompletedTask, Modifiers,
};
use std::path::PathBuf;

pub(super) struct SourceGui {
    registry: SourceRegistry,
    pub(super) controller: SourceController,
    visible: bool,
    candidates: Option<(Vec<PathBuf>, usize)>,
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
            | CommandId::SourceCalls
            | CommandId::SourceIncoming
            | CommandId::SourceDependencies
            | CommandId::SourceComplete
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
        context.source_calls_available =
            encoded && registry.resolve(language, SourceMode::Calls).is_some();
        context.source_dependencies_available = encoded
            && registry
                .resolve(language, SourceMode::Dependencies)
                .is_some();
        let controller = &self.source_gui.controller;
        context.source_incoming_available =
            registry.supports_incoming(&self.workbench, self.project_root.as_deref());
        context.source_active = controller.mode().is_some()
            && !matches!(
                controller.state(),
                SourceState::Idle | SourceState::Cancelled
            );
        context.source_ready = controller.ready(&self.workbench, self.project_root.as_deref());
        context.source_side_ready =
            controller.side_ready(&self.workbench, self.project_root.as_deref());
        if self.source_visible() {
            context.editor_input = false;
        }
    }
    fn request_source(&mut self, mode: SourceMode) {
        self.close_selector();
        self.bottom.focused = false;
        self.explorer_focused = false;
        self.source_gui.candidates = None;
        self.source_gui.visible = true;
        self.source_gui.epoch = self.source_gui.epoch.saturating_add(1);
        self.status = match self.source_gui.controller.request(
            mode,
            &self.source_gui.registry,
            &self.workbench,
            self.project_root.as_deref(),
            &mut self.jobs,
        ) {
            Ok(()) => self.source_gui.controller.message().into(),
            Err(error) => error,
        };
    }
    pub(super) fn source_command(&mut self, command: CommandId) -> Action {
        match command {
            CommandId::ViewSymbols => self.request_source(SourceMode::Symbols),
            CommandId::SourceReferences => self.request_source(SourceMode::References),
            CommandId::SourceCalls => self.request_source(SourceMode::Calls),
            CommandId::SourceIncoming => self.request_source(SourceMode::Incoming),
            CommandId::SourceDependencies => self.request_source(SourceMode::Dependencies),
            CommandId::SourceComplete => self.request_source(SourceMode::Completion),
            CommandId::SourceRefresh => {
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
                        self.status = "Elemento aceptado; solo evidencia léxica".into();
                    }
                    Err(error) => self.error(error),
                }
            }
            _ => {}
        }
        Action::None
    }
    pub(super) fn source_key(&mut self, code: usize, modifiers: Modifiers) -> Action {
        self.validate_source();
        match code {
            0x1b => return self.source_command(CommandId::SourceCancel),
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
    pub(super) fn source_character(&mut self, _ch: char) {
        // Both lists and completion candidates are read models. Typing/paste
        // here must never edit the hidden live source or imply implicit accept.
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
        let consumed = self.source_gui.controller.accept(
            &self.workbench,
            self.project_root.as_deref(),
            completion,
        );
        if consumed && self.source_visible() {
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
                policy: "Evidencia léxica | captura acotada | aceptación explícita | sin AVM/indexador",
                footer: "Flechas/Inicio/Fin eligen | Intro acepta | Ctrl+Intro al lado | F5 actualiza | Esc",
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
