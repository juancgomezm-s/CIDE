//! Wizard over the generic bounded snippet engine. No source buffer or ABL
//! condition belongs to the frontend; the registry contributes all template data.
use super::TuiApp;
use crate::{input::is_plain_text, state::UiOverlay};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use jc_app::{
    edit_field, present_field,
    snippets::{SnippetSession, SnippetTarget, SnippetTemplate},
    source_services::display_ui_label as display_label,
    CommandId, FieldEdit, FieldPresentation, PrimaryFocus, Workbench,
};
pub(crate) enum Stage {
    Choose,
    Parameter(usize),
    Preview,
}
pub(crate) struct SnippetUi {
    target: SnippetTarget,
    catalog: Vec<&'static SnippetTemplate>,
    selected: usize,
    query: String,
    cursor: usize,
    stage: Stage,
    session: Option<SnippetSession>,
    scroll: usize,
    page_rows: usize,
}
pub(crate) struct SnippetPresentation {
    pub title: String,
    pub message: String,
    pub footer: &'static str,
    pub field: Option<FieldPresentation>,
    pub rows: Vec<(String, bool)>,
}
impl SnippetUi {
    fn choices(&self) -> Vec<usize> {
        let query = self.query.to_lowercase();
        self.catalog
            .iter()
            .enumerate()
            .filter(|(_, t)| t.name.to_lowercase().contains(&query))
            .map(|(i, _)| i)
            .collect()
    }
    fn current(&self, app: &Workbench) -> bool {
        self.target.current(app)
    }
    fn ready(&self, app: &Workbench) -> bool {
        self.current(app)
            && matches!(self.stage, Stage::Preview)
            && self.session.as_ref().is_some_and(|s| s.plan().is_some())
    }
    fn next(&mut self, app: &Workbench) -> Result<(), String> {
        if !self.current(app) {
            return Err("Cambió el destino de la plantilla; cancela y vuelve a abrir".into());
        }
        match self.stage {
            Stage::Choose => {
                let indices = self.choices();
                let template = *self
                    .catalog
                    .get(
                        *indices
                            .get(self.selected)
                            .ok_or("No hay una plantilla seleccionada")?,
                    )
                    .ok_or("No hay una plantilla seleccionada")?;
                let mut session = if self
                    .session
                    .as_ref()
                    .is_some_and(|session| session.template().id == template.id)
                {
                    self.session.take().unwrap()
                } else {
                    SnippetSession::new(self.target.clone(), template)?
                };
                if template.parameters.is_empty() {
                    session.prepare(app)?;
                    self.stage = Stage::Preview;
                } else {
                    self.stage = Stage::Parameter(0);
                }
                self.session = Some(session);
                self.scroll = 0;
            }
            Stage::Parameter(index) => {
                let session = self
                    .session
                    .as_mut()
                    .ok_or("No hay una sesión de plantilla")?;
                if !session.valid_parameter(index) {
                    return Err("Parámetro inválido; escribe un valor válido de una línea".into());
                }
                if index + 1 < session.template().parameters.len() {
                    self.stage = Stage::Parameter(index + 1);
                } else {
                    session.prepare(app)?;
                    self.stage = Stage::Preview;
                    self.scroll = 0;
                }
            }
            Stage::Preview => {}
        }
        Ok(())
    }
    fn previous(&mut self) {
        match self.stage {
            Stage::Choose => {}
            Stage::Parameter(0) => self.stage = Stage::Choose,
            Stage::Parameter(i) => self.stage = Stage::Parameter(i - 1),
            Stage::Preview => {
                let count = self
                    .session
                    .as_ref()
                    .map_or(0, |s| s.template().parameters.len());
                self.stage = if count == 0 {
                    Stage::Choose
                } else {
                    Stage::Parameter(count - 1)
                };
            }
        }
    }
    fn edit(&mut self, edit: FieldEdit<'_>, app: &Workbench) -> Result<(), String> {
        if !self.current(app) {
            return Err("Cambió el destino de la plantilla; cancela y vuelve a abrir".into());
        }
        match self.stage {
            Stage::Choose => {
                edit_field(&mut self.query, &mut self.cursor, edit, 256).map_err(str::to_owned)?;
                self.selected = 0;
            }
            Stage::Parameter(i) => self
                .session
                .as_mut()
                .ok_or("No hay una sesión de plantilla")?
                .edit_parameter(i, edit)?,
            Stage::Preview => {}
        }
        Ok(())
    }
}
pub(crate) fn is_command(command: CommandId) -> bool {
    matches!(
        command,
        CommandId::EditInsertSnippet
            | CommandId::EditSurroundWith
            | CommandId::SnippetNext
            | CommandId::SnippetPrevious
            | CommandId::SnippetAccept
            | CommandId::SnippetCancel
    )
}
impl TuiApp {
    pub(super) fn snippet_clipboard_budget(&self) -> Option<usize> {
        let ui = self.snippet_ui.as_ref()?;
        if !ui.current(&self.workbench) {
            return None;
        }
        match ui.stage {
            Stage::Choose => Some(256usize.saturating_sub(ui.query.len()).max(1)),
            Stage::Parameter(index) => ui
                .session
                .as_ref()?
                .parameter(index)
                .map(|(_, value, _)| 256usize.saturating_sub(value.len()).max(1)),
            Stage::Preview => None,
        }
    }
    pub(crate) fn keep_snippet_for(command: CommandId) -> bool {
        is_command(command)
            || matches!(
                command,
                CommandId::PaletteOpen
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
    pub(crate) fn showing_snippet(&self) -> bool {
        self.snippet_ui.is_some() && matches!(self.overlay, UiOverlay::None)
    }
    pub(crate) fn snippet_context(&self, ctx: &mut jc_app::CommandContext) {
        let catalog = self
            .source_registry
            .snippets(ctx.active_language.as_deref().unwrap_or(""));
        ctx.snippet_insert_available =
            self.workbench.active_encoding().is_some() && catalog.iter().any(|t| !t.surround);
        ctx.snippet_surround_available =
            self.workbench.active_encoding().is_some() && catalog.iter().any(|t| t.surround);
        ctx.snippet_active = self.snippet_ui.is_some();
        ctx.snippet_next = self
            .snippet_ui
            .as_ref()
            .is_some_and(|ui| ui.current(&self.workbench) && !matches!(ui.stage, Stage::Preview));
        ctx.snippet_previous = self
            .snippet_ui
            .as_ref()
            .is_some_and(|ui| ui.current(&self.workbench) && !matches!(ui.stage, Stage::Choose));
        ctx.snippet_ready = self
            .snippet_ui
            .as_ref()
            .is_some_and(|ui| ui.ready(&self.workbench));
        if self.snippet_ui.is_some() {
            ctx.editor_input = false;
        }
    }
    pub(crate) fn snippet_command(&mut self, command: CommandId) {
        match command {
            CommandId::EditInsertSnippet | CommandId::EditSurroundWith => {
                let surround = command == CommandId::EditSurroundWith;
                let result = (|| {
                    let target = SnippetTarget::capture(&self.workbench)?;
                    if target.has_selection() != surround {
                        return Err(
                            "Para insertar coloca el cursor; para envolver selecciona un rango de texto"
                                .into(),
                        );
                    }
                    let language = self.workbench.context().active_language.unwrap_or_default();
                    let catalog = self
                        .source_registry
                        .snippets(&language)
                        .into_iter()
                        .filter(|t| t.surround == surround)
                        .collect::<Vec<_>>();
                    if catalog.is_empty() {
                        return Err("No hay plantillas disponibles para este perfil".into());
                    }
                    Ok(SnippetUi {
                        target,
                        catalog,
                        selected: 0,
                        query: String::new(),
                        cursor: 0,
                        stage: Stage::Choose,
                        session: None,
                        scroll: 0,
                        page_rows: 1,
                    })
                })();
                match result {
                    Ok(ui) => {
                        self.close_overlay();
                        self.source_popup = false;
                        self.source_candidates = None;
                        self.open_diff_visible = false;
                        self.snippet_ui = Some(ui);
                        self.status =
                            "Elige una plantilla; la fuente cambia solo al confirmar la vista previa".into();
                    }
                    Err(e) => self.status = e,
                }
            }
            CommandId::SnippetNext => {
                if let Some(ui) = &mut self.snippet_ui {
                    self.status = ui
                        .next(&self.workbench)
                        .map(|()| "Paso de plantilla listo; fuente sin cambios".into())
                        .unwrap_or_else(|e| e);
                }
            }
            CommandId::SnippetPrevious => {
                if let Some(ui) = &mut self.snippet_ui {
                    ui.previous();
                    self.status = "Paso anterior de plantilla; fuente sin cambios".into();
                }
            }
            CommandId::SnippetCancel => {
                self.snippet_ui = None;
                self.close_overlay();
                self.status = "Plantilla cancelada; fuente sin cambios".into();
            }
            CommandId::SnippetAccept => {
                let result = self
                    .snippet_ui
                    .as_mut()
                    .and_then(|ui| ui.session.as_mut())
                    .ok_or_else(|| "No hay una plantilla revisada".to_owned())
                    .and_then(|session| session.apply(&mut self.workbench));
                match result {
                    Ok(()) => {
                        self.snippet_ui = None;
                        self.close_overlay();
                        let _ = self.focus.focus(PrimaryFocus::Editor, &self.panels);
                        self.status =
                            "Plantilla aplicada en un paso de deshacer; aún sin guardar".into();
                    }
                    Err(e) => self.status = e,
                }
            }
            _ => {}
        }
    }
    pub(crate) fn snippet_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Esc {
            self.dispatch(CommandId::SnippetCancel);
            return;
        }
        let preview = self
            .snippet_ui
            .as_ref()
            .is_some_and(|ui| matches!(ui.stage, Stage::Preview));
        if preview {
            if key.code == KeyCode::F(9)
                || key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::CONTROL)
            {
                self.dispatch(CommandId::SnippetAccept);
                return;
            }
            if key.code == KeyCode::F(2) || key.code == KeyCode::BackTab {
                self.dispatch(CommandId::SnippetPrevious);
                return;
            }
            if let Some(ui) = &mut self.snippet_ui {
                let total = ui
                    .session
                    .as_ref()
                    .and_then(|s| s.plan())
                    .map_or(0, |p| p.total_rows());
                let max = total.saturating_sub(ui.page_rows);
                ui.scroll = match key.code {
                    KeyCode::Up => ui.scroll.saturating_sub(1),
                    KeyCode::Down => ui.scroll.saturating_add(1).min(max),
                    KeyCode::PageUp => ui.scroll.saturating_sub(10),
                    KeyCode::PageDown => ui.scroll.saturating_add(10).min(max),
                    KeyCode::Home => 0,
                    KeyCode::End => max,
                    _ => ui.scroll,
                };
            }
            return;
        }
        if key.code == KeyCode::Enter || key.code == KeyCode::Tab {
            self.dispatch(CommandId::SnippetNext);
            return;
        }
        if key.code == KeyCode::BackTab {
            self.dispatch(CommandId::SnippetPrevious);
            return;
        }
        let choose = self
            .snippet_ui
            .as_ref()
            .is_some_and(|ui| matches!(ui.stage, Stage::Choose));
        if choose
            && matches!(
                key.code,
                KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown
            )
        {
            if let Some(ui) = &mut self.snippet_ui {
                let count = ui.choices().len();
                let delta = match key.code {
                    KeyCode::Up => -1,
                    KeyCode::Down => 1,
                    KeyCode::PageUp => -10,
                    _ => 10,
                };
                ui.selected = ui
                    .selected
                    .saturating_add_signed(delta)
                    .min(count.saturating_sub(1));
            }
            return;
        }
        let edit = match key.code {
            KeyCode::Left => Some(FieldEdit::Left),
            KeyCode::Right => Some(FieldEdit::Right),
            KeyCode::Home => Some(FieldEdit::Home),
            KeyCode::End => Some(FieldEdit::End),
            KeyCode::Backspace => Some(FieldEdit::Backspace),
            KeyCode::Delete => Some(FieldEdit::Delete),
            KeyCode::Char('u') if key.modifiers == KeyModifiers::CONTROL => Some(FieldEdit::Clear),
            _ => None,
        };
        if let Some(edit) = edit {
            self.edit_snippet(edit);
        } else if let KeyCode::Char(ch) = key.code {
            if is_plain_text(key) {
                self.edit_snippet(FieldEdit::Insert(&ch.to_string()));
            }
        }
    }
    pub(crate) fn edit_snippet(&mut self, edit: FieldEdit<'_>) {
        if let Some(ui) = &mut self.snippet_ui {
            if let Err(error) = ui.edit(edit, &self.workbench) {
                self.status = error;
            }
        }
    }
    pub(crate) fn prepare_snippet(
        &mut self,
        rows: usize,
        columns: usize,
    ) -> Option<SnippetPresentation> {
        let ui = self.snippet_ui.as_mut()?;
        let columns = columns.min(96);
        let rows = if matches!(ui.stage, Stage::Preview) {
            rows.min(24)
        } else {
            rows.saturating_sub(1).clamp(1, 24)
        };
        ui.page_rows = rows;
        let mut p = SnippetPresentation {
            title: "Plantilla · fuente sin cambios hasta confirmar".into(),
            message: display_label(&self.status, columns),
            footer: "Intro/Tab siguiente | Mayús+Tab atrás | Flechas editar | Ctrl+U vaciar | Esc",
            field: None,
            rows: Vec::new(),
        };
        if !ui.current(&self.workbench) {
            p.message = display_label(
                "La fuente cambió; no se puede aplicar. Esc cancela.",
                columns,
            );
            return Some(p);
        }
        match ui.stage {
            Stage::Choose => {
                p.title = display_label("Elegir plantilla / Envolver selección", columns);
                p.footer =
                    "Intro elegir | Flechas lista | Escribe para filtrar | Ctrl+U vaciar | Esc";
                p.field = Some(present_field(&ui.query, ui.cursor, columns, 1));
                let choices = ui.choices();
                let first = ui.selected.saturating_sub(rows.saturating_sub(1));
                p.rows = choices
                    .into_iter()
                    .enumerate()
                    .skip(first)
                    .take(rows)
                    .map(|(index, i)| {
                        (
                            display_label(ui.catalog[i].name, columns),
                            index == ui.selected,
                        )
                    })
                    .collect();
            }
            Stage::Parameter(index) => {
                if let Some(session) = &ui.session {
                    if let Some((parameter, value, cursor)) = session.parameter(index) {
                        p.title = display_label(
                            &format!(
                                "Parámetro {}/{}: {}",
                                index + 1,
                                session.template().parameters.len(),
                                parameter.label
                            ),
                            columns,
                        );
                        p.field = Some(present_field(value, cursor, columns, 1));
                    }
                }
            }
            Stage::Preview => {
                p.title = display_label(
                    "Revisar plantilla: F9 / Ctrl+Intro aplica; F2 edita",
                    columns,
                );
                p.footer = "F9/Ctrl+Intro aplicar | F2 editar | Pág/Inicio/Fin vista previa | Esc cancelar";
                if let Some(plan) = ui.session.as_ref().and_then(|s| s.plan()) {
                    ui.scroll = ui.scroll.min(plan.total_rows().saturating_sub(rows));
                    p.rows = plan
                        .rows(ui.scroll, rows, columns)
                        .into_iter()
                        .map(|label| (label, false))
                        .collect();
                }
            }
        }
        Some(p)
    }
}
