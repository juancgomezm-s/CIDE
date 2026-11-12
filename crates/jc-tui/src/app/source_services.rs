//! Frontend intentions only; providers and versioned source results belong to
//! the shared host. File opening/application happens only on explicit accept.
use super::TuiApp;
use crate::state::{PromptKind, UiOverlay};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use jc_app::{
    source_services::{
        display_label, display_ui_label, SourceMode, SourcePresentation, SourceRow, SourceState,
    },
    CommandId, PrimaryFocus, SidebarView,
};
pub(crate) fn is_command(command: CommandId) -> bool {
    matches!(
        command,
        CommandId::ViewSymbols
            | CommandId::SourceReferences
            | CommandId::SourceDefinition
            | CommandId::SourceLspReferences
            | CommandId::SourceXref
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
impl TuiApp {
    pub(crate) fn keep_source_for(command: CommandId) -> bool {
        is_command(command)
            || matches!(
                command,
                CommandId::PaletteOpen
                    | CommandId::ViewToggleInvisibles
                    | CommandId::MenuFile
                    | CommandId::MenuEdit
                    | CommandId::MenuSearch
                    | CommandId::MenuProject
                    | CommandId::MenuBuild
                    | CommandId::MenuTools
                    | CommandId::MenuView
                    | CommandId::MenuHelp
            )
    }
    pub(crate) fn showing_source_popup(&self) -> bool {
        self.source_popup && matches!(self.overlay, UiOverlay::None)
    }
    pub(crate) fn source_context(&self, context: &mut jc_app::CommandContext) {
        let language = context.active_language.as_deref().unwrap_or("");
        context.source_symbols_available = context.has_document
            && self
                .source_registry
                .resolve(language, SourceMode::Symbols)
                .is_some();
        context.source_references_available = context.has_document
            && self
                .source_registry
                .resolve(language, SourceMode::References)
                .is_some();
        context.source_completion_available = context.has_document
            && self
                .source_registry
                .resolve(language, SourceMode::Completion)
                .is_some();
        context.source_xref_available = self
            .progress_schema
            .supports_xref(&self.workbench, self.project_root.as_deref());
        context.source_abl_definition_available = self
            .progress_schema
            .supports_definition(&self.workbench, self.project_root.as_deref());
        context.source_schema_completion_available = self
            .progress_schema
            .supports_completion(&self.workbench, self.project_root.as_deref());
        context.source_rename_available = self.workbench.active_encoding().is_some()
            && self
                .source_registry
                .supports_document(&self.workbench, SourceMode::Rename);
        context.source_active = self.source.mode().is_some()
            && !matches!(
                self.source.state(),
                SourceState::Idle | SourceState::Cancelled
            );
        context.source_calls_available = context.has_document
            && self
                .source_registry
                .resolve(language, SourceMode::Calls)
                .is_some();
        context.source_dependencies_available = context.has_document
            && self
                .source_registry
                .resolve(language, SourceMode::Dependencies)
                .is_some();
        context.source_ready = self
            .source
            .ready(&self.workbench, self.project_root.as_deref());
        context.source_incoming_available = self
            .source_registry
            .supports_incoming(&self.workbench, self.project_root.as_deref());
        context.source_lsp_available = self
            .source_registry
            .supports_lsp(&self.workbench, self.project_root.as_deref());
        context.source_side_ready = self
            .source
            .side_ready(&self.workbench, self.project_root.as_deref());
        if self.source_popup {
            context.editor_input = false;
        }
    }
    fn request_source(&mut self, mode: SourceMode) {
        let catalog = if matches!(mode, SourceMode::Symbols | SourceMode::CatalogXref) {
            self.progress_schema
                .outline(&self.workbench, self.project_root.as_deref())
        } else if mode.is_catalog() {
            let Some(provider) = self.progress_schema.source(
                &self.workbench,
                self.project_root.as_deref(),
                mode == SourceMode::CatalogCompletion,
            ) else {
                self.status = if mode == SourceMode::CatalogCompletion {
                    "Actualiza el esquema ABL del proyecto para obtener sugerencias"
                } else {
                    "Abre un archivo ABL del proyecto"
                }
                .into();
                return;
            };
            Some(provider)
        } else {
            None
        };
        self.close_overlay();
        self.source_candidates = None;
        self.source_popup = matches!(
            mode,
            SourceMode::Completion
                | SourceMode::LspCompletion
                | SourceMode::CatalogCompletion
                | SourceMode::CatalogDefinition
                | SourceMode::CatalogReferences
                | SourceMode::CatalogXref
        );
        if mode == SourceMode::LspDiagnostics {
            self.show_bottom(jc_app::BottomPanel::Problems);
        } else if !self.source_popup {
            self.panels.show_sidebar(SidebarView::Symbols);
            let _ = self.focus.focus(PrimaryFocus::Sidebar, &self.panels);
        }
        let root = self.project_root.as_deref();
        let result = if let Some(provider) = catalog {
            self.source.request_catalog(
                mode,
                provider,
                &self.source_registry,
                &self.workbench,
                root,
                &mut self.tasks,
            )
        } else {
            self.source.request(
                mode,
                &self.source_registry,
                &self.workbench,
                root,
                &mut self.tasks,
            )
        };
        self.status = match result {
            Ok(()) => self.source.message().into(),
            Err(e) => e,
        };
    }
    fn start_symbol_rename(&mut self) {
        match jc_app::snippets::SnippetTarget::capture(&self.workbench) {
            Ok(target) => {
                self.source.cancel(&mut self.tasks);
                self.source_candidates = None;
                self.source_popup = false;
                self.open_prompt(PromptKind::RenameSymbol);
                self.rename_target = Some(target);
                self.status = "Escribe el nombre nuevo y pulsa Intro para revisar".into();
            }
            Err(error) => self.status = error,
        }
    }
    pub(crate) fn preview_symbol_rename(&mut self, name: &str) {
        if !self
            .rename_target
            .as_ref()
            .is_some_and(|target| target.current(&self.workbench))
        {
            self.status = "Cambió el destino del renombrado; vuelve a abrir el comando".into();
            return;
        }
        match self.source.request_rename(
            name,
            &self.source_registry,
            &self.workbench,
            self.project_root.as_deref(),
            &mut self.tasks,
        ) {
            Ok(()) => {
                self.close_overlay();
                self.source_popup = true;
                self.status = self.source.message().into();
            }
            Err(error) => self.status = error,
        }
    }
    pub(crate) fn source_command(&mut self, command: CommandId) {
        match command {
            CommandId::ViewSymbols => self.request_source(SourceMode::Symbols),
            CommandId::SourceReferences => self.request_source(SourceMode::References),
            CommandId::SourceDefinition | CommandId::SourceLspReferences => {
                let abl = self
                    .progress_schema
                    .supports_definition(&self.workbench, self.project_root.as_deref());
                let mode = match (command, abl) {
                    (CommandId::SourceDefinition, true) => SourceMode::CatalogDefinition,
                    (CommandId::SourceDefinition, false) => SourceMode::Definition,
                    (_, true) => SourceMode::CatalogReferences,
                    (_, false) => SourceMode::LspReferences,
                };
                self.request_source(mode);
            }
            CommandId::SourceXref => self.request_source(SourceMode::CatalogXref),
            CommandId::SourceLspComplete => self.request_source(SourceMode::LspCompletion),
            CommandId::SourceSchemaComplete => self.request_source(SourceMode::CatalogCompletion),
            CommandId::SourceLspDiagnostics => self.request_source(SourceMode::LspDiagnostics),
            CommandId::SourceCalls => self.request_source(SourceMode::Calls),
            CommandId::SourceIncoming => self.request_source(SourceMode::Incoming),
            CommandId::SourceDependencies => self.request_source(SourceMode::Dependencies),
            CommandId::SourceComplete => self.request_source(SourceMode::Completion),
            CommandId::SourceRename => self.start_symbol_rename(),
            CommandId::SourceRefresh => {
                if let Some(mode) = self.source.mode() {
                    if mode == SourceMode::Rename {
                        self.start_symbol_rename();
                    } else {
                        self.request_source(mode);
                    }
                }
            }
            CommandId::SourceCancel => {
                self.source.cancel(&mut self.tasks);
                self.source_popup = false;
                self.source_candidates = None;
                self.close_overlay();
                let _ = self.focus.focus(PrimaryFocus::Editor, &self.panels);
                self.status = self.source.message().into();
            }
            CommandId::SourceAccept | CommandId::SourceOpenToSide => {
                let side = command == CommandId::SourceOpenToSide;
                let result = if let Some((paths, index)) = &self.source_candidates {
                    paths
                        .get(*index)
                        .ok_or_else(|| "No hay un destino de referencia seleccionado".into())
                        .and_then(|path| {
                            self.source.activate_candidate(
                                &mut self.workbench,
                                self.project_root.as_deref(),
                                path,
                                side,
                            )
                        })
                        .map(|()| None)
                } else {
                    self.source
                        .activate(&mut self.workbench, self.project_root.as_deref(), side)
                };
                match result {
                    Ok(Some(paths)) => {
                        self.source_candidates = Some((paths, 0));
                        self.source_popup = true;
                        self.close_overlay();
                        self.status =
                            "Referencia ambigua: elige explícitamente un destino dentro del proyecto".into();
                    }
                    Ok(None) => {
                        self.source_popup = false;
                        self.source_candidates = None;
                        self.close_overlay();
                        let _ = self.focus.focus(PrimaryFocus::Editor, &self.panels);
                        self.ensure_encoding_choice();
                        self.status = if self.source.mode() == Some(SourceMode::Rename) {
                            "Símbolo renombrado · un paso para deshacer".into()
                        } else if self.source.mode() == Some(SourceMode::LspCompletion) {
                            "Sugerencia semántica aplicada · un paso para deshacer".into()
                        } else if self.source.mode() == Some(SourceMode::CatalogCompletion) {
                            "Sugerencia del esquema aplicada · un paso para deshacer".into()
                        } else if self.source.mode() == Some(SourceMode::CatalogDefinition) {
                            "Definición ABL abierta".into()
                        } else if self.source.mode() == Some(SourceMode::CatalogReferences) {
                            "Referencia ABL abierta".into()
                        } else if self.source.mode() == Some(SourceMode::CatalogXref) {
                            "Referencia XREF abierta".into()
                        } else if self.source.mode().is_some_and(SourceMode::is_lsp) {
                            "Ubicación LSP abierta".into()
                        } else {
                            "Elemento del documento aceptado · solo evidencia léxica".into()
                        };
                    }
                    Err(e) => self.status = e,
                }
            }
            _ => {}
        }
    }
    pub(crate) fn source_key(&mut self, key: KeyEvent) {
        if self.source.mode() == Some(SourceMode::Rename) {
            if key.code == KeyCode::F(9)
                || key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::CONTROL)
            {
                self.dispatch(CommandId::SourceAccept);
                return;
            }
            if key.code == KeyCode::Enter {
                return;
            }
        }
        match key.code {
            KeyCode::Esc => {
                self.dispatch(CommandId::SourceCancel);
                return;
            }
            KeyCode::F(5) => {
                self.dispatch(CommandId::SourceRefresh);
                return;
            }
            KeyCode::Enter => {
                self.dispatch(if key.modifiers.contains(KeyModifiers::CONTROL) {
                    CommandId::SourceOpenToSide
                } else {
                    CommandId::SourceAccept
                });
                return;
            }
            _ => {}
        }
        let delta = match key.code {
            KeyCode::Up => -1,
            KeyCode::Down => 1,
            KeyCode::PageUp => -10,
            KeyCode::PageDown => 10,
            _ => 0,
        };
        if let Some((paths, index)) = &mut self.source_candidates {
            *index = if key.code == KeyCode::Home {
                0
            } else if key.code == KeyCode::End {
                paths.len().saturating_sub(1)
            } else {
                index
                    .saturating_add_signed(delta)
                    .min(paths.len().saturating_sub(1))
            };
        } else {
            match key.code {
                KeyCode::Home => self.source.first(),
                KeyCode::End => self.source.last(),
                _ => self.source.select_relative(delta),
            }
        }
    }
    pub(crate) fn validate_source(&mut self) -> bool {
        let changed = self.source.validate_context(
            &self.workbench,
            self.project_root.as_deref(),
            &mut self.tasks,
        );
        if changed || self.source.state() == SourceState::Stale {
            self.source_candidates = None;
            self.source_popup = false;
        }
        changed
    }
    pub(crate) fn source_presentation(&self, rows: usize, columns: usize) -> SourcePresentation {
        if let Some((paths, index)) = &self.source_candidates {
            return SourcePresentation {
                title: display_ui_label("Destinos de la referencia", columns),
                message: display_ui_label(
                    "Intro abre la ruta elegida; Ctrl+Intro al lado; Esc cancela",
                    columns,
                ),
                rows: paths
                    .iter()
                    .enumerate()
                    .take(rows.min(64))
                    .map(|(i, p)| SourceRow {
                        index: i,
                        label: display_label(
                            &self
                                .project_root
                                .as_deref()
                                .and_then(|root| p.strip_prefix(root).ok())
                                .unwrap_or(p)
                                .display()
                                .to_string(),
                            columns,
                        ),
                        selected: i == *index,
                    })
                    .collect(),
            };
        }
        self.source.present(rows, columns)
    }
}
