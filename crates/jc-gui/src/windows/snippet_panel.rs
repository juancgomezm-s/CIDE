//! Native wizard over the shared declarative engine. Only bounded fields,
//! template references and one immutable preview are owned by this surface.
use super::{
    git_panel::GitFrame,
    project_panel::ProjectPresentation,
    project_results::{ProjectResultRow, ProjectResultsPresentation},
    Action, State,
};
use jc_app::{
    edit_field, present_field,
    snippets::{SnippetSession, SnippetTarget, SnippetTemplate},
    source_services::display_ui_label as display_label,
    CommandContext, CommandId, FieldEdit, Modifiers, Workbench,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Choose,
    Parameter(usize),
    Preview,
}
/// Exact editable-wizard intention for a deferred clipboard handoff. Source
/// identity is revalidated separately by SnippetTarget on every use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SnippetInputTarget {
    generation: u64,
    revision: u64,
}
pub(super) struct SnippetGui {
    target: SnippetTarget,
    catalog: Vec<&'static SnippetTemplate>,
    selected: usize,
    query: String,
    cursor: usize,
    stage: Stage,
    session: Option<SnippetSession>,
    scroll: usize,
    page_rows: usize,
    input: SnippetInputTarget,
}
impl SnippetGui {
    fn new(target: SnippetTarget, catalog: Vec<&'static SnippetTemplate>, generation: u64) -> Self {
        Self {
            target,
            catalog,
            selected: 0,
            query: String::new(),
            cursor: 0,
            stage: Stage::Choose,
            session: None,
            scroll: 0,
            page_rows: 1,
            input: SnippetInputTarget {
                generation,
                revision: 0,
            },
        }
    }
    fn choices(&self) -> Vec<usize> {
        let query = self.query.to_lowercase();
        self.catalog
            .iter()
            .enumerate()
            .filter(|(_, template)| template.name.to_lowercase().contains(&query))
            .map(|(index, _)| index)
            .collect()
    }
    fn current(&self, app: &Workbench) -> bool {
        self.target.current(app)
    }
    fn editable(&self, app: &Workbench) -> bool {
        self.current(app) && !matches!(self.stage, Stage::Preview)
    }
    fn ready(&self, app: &Workbench) -> bool {
        self.current(app)
            && matches!(self.stage, Stage::Preview)
            && self.session.as_ref().is_some_and(|s| s.plan().is_some())
    }
    fn advance_revision(&mut self) -> Result<(), String> {
        self.input.revision = self
            .input
            .revision
            .checked_add(1)
            .ok_or("Se agotó la generación del campo; cancela y vuelve a abrir")?;
        Ok(())
    }
    fn next(&mut self, app: &Workbench) -> Result<(), String> {
        if !self.current(app) {
            return Err("Cambió el documento de destino; cancela y vuelve a abrir".into());
        }
        self.advance_revision()?;
        match self.stage {
            Stage::Choose => {
                let choices = self.choices();
                let template = *self
                    .catalog
                    .get(
                        *choices
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
    fn previous(&mut self) -> Result<(), String> {
        self.advance_revision()?;
        self.stage = match self.stage {
            Stage::Choose => Stage::Choose,
            Stage::Parameter(0) => Stage::Choose,
            Stage::Parameter(index) => Stage::Parameter(index - 1),
            Stage::Preview => {
                let count = self
                    .session
                    .as_ref()
                    .map_or(0, |s| s.template().parameters.len());
                if count == 0 {
                    Stage::Choose
                } else {
                    Stage::Parameter(count - 1)
                }
            }
        };
        Ok(())
    }
    fn edit(&mut self, edit: FieldEdit<'_>, app: &Workbench) -> Result<(), String> {
        if !self.editable(app) {
            return Err("El campo no está disponible o cambió su destino".into());
        }
        if self.input.revision == u64::MAX {
            return Err("Se agotó la generación del campo; cancela y vuelve a abrir".into());
        }
        if let FieldEdit::Insert(value) = edit {
            if value.chars().any(char::is_control) {
                return Err("Los campos de plantilla solo aceptan texto de una línea".into());
            }
        }
        match self.stage {
            Stage::Choose => {
                edit_field(&mut self.query, &mut self.cursor, edit, 256).map_err(str::to_owned)?;
                self.selected = 0;
            }
            Stage::Parameter(index) => self
                .session
                .as_mut()
                .ok_or("No hay una sesión de plantilla")?
                .edit_parameter(index, edit)?,
            Stage::Preview => unreachable!(),
        }
        self.advance_revision()
    }
}

pub(super) fn is_command(command: CommandId) -> bool {
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
pub(super) fn keep_for(command: CommandId) -> bool {
    is_command(command)
        || super::native_menu::is_supported_macro(command)
        || command == CommandId::PaletteOpen
}
impl State {
    pub(super) fn snippet_context(&self, ctx: &mut CommandContext) {
        let catalog = self
            .source_gui
            .registry()
            .snippets(ctx.active_language.as_deref().unwrap_or(""));
        ctx.snippet_insert_available =
            self.workbench.active_encoding().is_some() && catalog.iter().any(|t| !t.surround);
        ctx.snippet_surround_available =
            self.workbench.active_encoding().is_some() && catalog.iter().any(|t| t.surround);
        ctx.snippet_active = self.snippet_gui.is_some();
        ctx.snippet_next = self
            .snippet_gui
            .as_ref()
            .is_some_and(|ui| ui.editable(&self.workbench));
        ctx.snippet_previous = self
            .snippet_gui
            .as_ref()
            .is_some_and(|ui| ui.current(&self.workbench) && !matches!(ui.stage, Stage::Choose));
        ctx.snippet_ready = self
            .snippet_gui
            .as_ref()
            .is_some_and(|ui| ui.ready(&self.workbench));
        if self.snippet_gui.is_some() {
            ctx.editor_input = false;
        }
    }
    pub(super) fn snippet_command(&mut self, command: CommandId) -> Action {
        match command {
            CommandId::EditInsertSnippet | CommandId::EditSurroundWith => {
                let surround = command == CommandId::EditSurroundWith;
                let result: Result<_, String> = (|| {
                    let target = SnippetTarget::capture(&self.workbench)?;
                    if target.has_selection() != surround {
                        return Err("Para insertar coloca el cursor; para envolver selecciona un rango de texto".into());
                    }
                    let language = self.workbench.context().active_language.unwrap_or_default();
                    let catalog = self
                        .source_gui
                        .registry()
                        .snippets(&language)
                        .into_iter()
                        .filter(|t| t.surround == surround)
                        .collect::<Vec<_>>();
                    if catalog.is_empty() {
                        return Err("No hay plantillas disponibles para este perfil".into());
                    }
                    Ok((target, catalog))
                })();
                match result {
                    Ok((target, catalog)) => {
                        self.close_selector();
                        self.close_source_ui();
                        if self.selector_generation == u64::MAX {
                            self.error(
                                "Se agotó la generación de la plantilla; vuelve a abrir la ventana",
                            );
                            return Action::None;
                        }
                        self.open_diff_visible = false;
                        self.explorer_focused = false;
                        self.bottom.focused = false;
                        self.snippet_gui =
                            Some(SnippetGui::new(target, catalog, self.selector_generation));
                        self.status = "Elige una plantilla; la fuente cambia solo al confirmar la vista previa".into();
                    }
                    Err(error) => self.error(error),
                }
            }
            CommandId::SnippetNext => {
                if let Some(ui) = &mut self.snippet_gui {
                    self.status = ui
                        .next(&self.workbench)
                        .map(|()| "Paso de plantilla listo; fuente sin cambios".into())
                        .unwrap_or_else(|error| error);
                }
            }
            CommandId::SnippetPrevious => {
                if let Some(ui) = &mut self.snippet_gui {
                    self.status = ui
                        .previous()
                        .map(|()| "Paso anterior de plantilla; fuente sin cambios".into())
                        .unwrap_or_else(|error| error);
                }
            }
            CommandId::SnippetCancel => {
                self.snippet_gui = None;
                self.close_selector();
                self.status = "Plantilla cancelada; fuente sin cambios".into();
            }
            CommandId::SnippetAccept => {
                let result = self
                    .snippet_gui
                    .as_mut()
                    .filter(|ui| matches!(ui.stage, Stage::Preview))
                    .and_then(|ui| ui.session.as_mut())
                    .ok_or_else(|| "No hay una plantilla revisada".to_owned())
                    .and_then(|session| session.apply(&mut self.workbench));
                match result {
                    Ok(()) => {
                        self.snippet_gui = None;
                        self.close_selector();
                        self.explorer_focused = false;
                        self.bottom.focused = false;
                        self.status =
                            "Plantilla aplicada en un paso de deshacer; aún sin guardar".into();
                    }
                    Err(error) => self.error(error),
                }
            }
            _ => {}
        }
        Action::None
    }
    pub(super) fn snippet_stamp(&self) -> Option<SnippetInputTarget> {
        self.snippet_gui.as_ref().map(|ui| ui.input)
    }
    pub(super) fn snippet_input_target(&self) -> Option<SnippetInputTarget> {
        self.snippet_gui
            .as_ref()
            .filter(|ui| ui.editable(&self.workbench))
            .map(|ui| ui.input)
    }
    pub(super) fn snippet_field_budget(&self) -> Option<usize> {
        let ui = self
            .snippet_gui
            .as_ref()
            .filter(|ui| ui.editable(&self.workbench))?;
        let length = match ui.stage {
            Stage::Choose => ui.query.len(),
            Stage::Parameter(index) => ui.session.as_ref()?.parameter(index)?.1.len(),
            Stage::Preview => return None,
        };
        Some(256usize.saturating_sub(length).max(1))
    }
    pub(super) fn snippet_paste_to(&mut self, target: SnippetInputTarget, text: &str) {
        if self.snippet_input_target() != Some(target) {
            self.error("El campo cambió durante el pegado; inténtalo de nuevo");
            return;
        }
        self.snippet_paste(text);
    }
    pub(super) fn snippet_paste(&mut self, text: &str) {
        if let Some(ui) = &mut self.snippet_gui {
            if let Err(error) = ui.edit(FieldEdit::Insert(text), &self.workbench) {
                self.error(error);
            }
        }
    }
    pub(super) fn snippet_character(&mut self, ch: char) {
        if !ch.is_control() {
            self.snippet_paste(&ch.to_string());
        }
    }
    pub(super) fn snippet_key(&mut self, code: usize, modifiers: Modifiers) -> Action {
        if code == 0x1b {
            return self.snippet_command(CommandId::SnippetCancel);
        }
        let preview = self
            .snippet_gui
            .as_ref()
            .is_some_and(|ui| ui.stage == Stage::Preview);
        if preview {
            if code == 0x78 && modifiers == Modifiers::NONE
                || code == 0x0d && modifiers == Modifiers::CTRL
            {
                return self.snippet_command(CommandId::SnippetAccept);
            }
            if code == 0x71 && modifiers == Modifiers::NONE
                || code == 0x09 && modifiers == Modifiers::SHIFT
            {
                return self.snippet_command(CommandId::SnippetPrevious);
            }
            if modifiers == Modifiers::NONE {
                if let Some(ui) = &mut self.snippet_gui {
                    let total = ui
                        .session
                        .as_ref()
                        .and_then(|s| s.plan())
                        .map_or(0, |p| p.total_rows());
                    let max = total.saturating_sub(ui.page_rows);
                    ui.scroll = match code {
                        0x26 => ui.scroll.saturating_sub(1),
                        0x28 => ui.scroll.saturating_add(1).min(max),
                        0x21 => ui.scroll.saturating_sub(ui.page_rows),
                        0x22 => ui.scroll.saturating_add(ui.page_rows).min(max),
                        0x24 => 0,
                        0x23 => max,
                        _ => ui.scroll,
                    };
                }
            }
            return Action::None;
        }
        if modifiers == Modifiers::CTRL {
            if code == 0x56 {
                return self
                    .snippet_input_target()
                    .map_or(Action::None, Action::PasteSnippet);
            }
            if code == 0x55 {
                if let Some(ui) = &mut self.snippet_gui {
                    if let Err(error) = ui.edit(FieldEdit::Clear, &self.workbench) {
                        self.error(error);
                    }
                }
            }
            return Action::None;
        }
        if code == 0x09 && modifiers == Modifiers::SHIFT {
            return self.snippet_command(CommandId::SnippetPrevious);
        }
        if modifiers != Modifiers::NONE {
            return Action::None;
        }
        if code == 0x0d || code == 0x09 {
            return self.snippet_command(CommandId::SnippetNext);
        }
        let Some(ui) = &mut self.snippet_gui else {
            return Action::None;
        };
        if ui.stage == Stage::Choose && matches!(code, 0x26 | 0x28 | 0x21 | 0x22) {
            if let Err(error) = ui.advance_revision() {
                self.error(error);
                return Action::None;
            }
            let count = ui.choices().len();
            let delta = match code {
                0x26 => -1,
                0x28 => 1,
                0x21 => -(ui.page_rows as isize),
                _ => ui.page_rows as isize,
            };
            ui.selected = ui
                .selected
                .saturating_add_signed(delta)
                .min(count.saturating_sub(1));
            return Action::None;
        }
        let edit = match code {
            0x25 => Some(FieldEdit::Left),
            0x27 => Some(FieldEdit::Right),
            0x24 => Some(FieldEdit::Home),
            0x23 => Some(FieldEdit::End),
            0x08 => Some(FieldEdit::Backspace),
            0x2e => Some(FieldEdit::Delete),
            _ => None,
        };
        if let Some(edit) = edit {
            if let Err(error) = ui.edit(edit, &self.workbench) {
                self.error(error);
            }
        }
        Action::None
    }
    pub(super) fn snippet_presentation(&mut self) -> Option<GitFrame> {
        let ui = self.snippet_gui.as_mut()?;
        let (columns, _) = super::project_paint::field_geometry(
            self.width,
            (self.height, self.dpi.pixels(super::chrome::TOP)),
            self.cell_width,
            self.cell_height,
            1,
        );
        let columns = columns.min(96);
        let rows = super::project_paint::result_capacity(
            self.width,
            (self.height, self.dpi.pixels(super::chrome::TOP)),
            self.cell_width,
            self.cell_height,
        )
        .min(24);
        ui.page_rows = rows.max(1);
        let mut title = "Plantilla: fuente sin cambios hasta confirmar".to_owned();
        let mut message = display_label(&self.status, columns);
        let mut footer =
            "Intro/Tab siguiente | Mayús+Tab atrás | Ctrl+U vaciar | Ctrl+V pegar | Esc";
        let mut fields = Vec::new();
        let mut result_rows = Vec::new();
        let mut selected = ui.selected;
        let mut total = 0;
        if !ui.current(&self.workbench) {
            message = display_label(
                "La fuente cambió; no se puede aplicar. Esc cancela.",
                columns,
            );
        } else {
            match ui.stage {
                Stage::Choose => {
                    title = "Elegir plantilla / Envolver selección".into();
                    footer = "Intro elegir | Flechas lista | Escribe para filtrar | Ctrl+U vaciar | Ctrl+V | Esc";
                    fields.push(("Filtro", present_field(&ui.query, ui.cursor, columns, 1)));
                    let choices = ui.choices();
                    total = choices.len();
                    ui.selected = ui.selected.min(total.saturating_sub(1));
                    selected = ui.selected;
                    let first = ui.selected.saturating_sub(rows.saturating_sub(1));
                    result_rows = choices
                        .into_iter()
                        .enumerate()
                        .skip(first)
                        .take(rows)
                        .map(|(index, choice)| ProjectResultRow {
                            index,
                            label: display_label(ui.catalog[choice].name, columns),
                            selected: index == ui.selected,
                            is_match: true,
                        })
                        .collect();
                }
                Stage::Parameter(index) => {
                    if let Some(session) = &ui.session {
                        if let Some((parameter, value, cursor)) = session.parameter(index) {
                            title = display_label(
                                &format!(
                                    "Parámetro {}/{}: {}",
                                    index + 1,
                                    session.template().parameters.len(),
                                    parameter.label
                                ),
                                columns,
                            );
                            fields.push(("Valor", present_field(value, cursor, columns, 1)));
                        }
                    }
                }
                Stage::Preview => {
                    title = "Revisar plantilla: F9 / Ctrl+Intro aplica".into();
                    footer = "F9/Ctrl+Intro aplicar | F2 editar | Pág/Inicio/Fin vista previa | Esc cancelar";
                    if let Some(plan) = ui.session.as_ref().and_then(|s| s.plan()) {
                        total = plan.total_rows();
                        ui.scroll = ui.scroll.min(total.saturating_sub(rows));
                        selected = ui.scroll;
                        result_rows = plan
                            .rows(ui.scroll, rows, columns)
                            .into_iter()
                            .enumerate()
                            .map(|(index, label)| ProjectResultRow {
                                index: ui.scroll + index,
                                label,
                                selected: false,
                                is_match: true,
                            })
                            .collect();
                    }
                }
            }
        }
        Some(GitFrame {
            panel: ProjectPresentation {
                fields,
                active: 0,
                status: String::new(),
                results: Some(ProjectResultsPresentation { title, message, total, selected, rows: result_rows }),
            },
            labels: super::project_paint::PanelLabels {
                draft: "Plantilla",
                policy: "Revisa antes de aplicar | Un deshacer | Codificación explícita | Sin cambios hasta confirmar",
                footer,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jc_app::snippets::{ParameterRule, SnippetParameter};
    use jc_text::{TextEncoding, TextPosition};
    static INSERT: SnippetTemplate = SnippetTemplate {
        id: "test.gui.insert",
        name: "Insert named block",
        surround: false,
        parameters: &[SnippetParameter {
            key: "name",
            label: "Name",
            default: "first",
            rule: ParameterRule::SingleLine,
        }],
        body: "BLOCK ${name}:\n${cursor}END.\n",
    };
    static SURROUND: SnippetTemplate = SnippetTemplate {
        id: "test.gui.surround",
        name: "Surround selected text",
        surround: true,
        parameters: &[],
        body: "DO:\n${selection}\nEND.${cursor}",
    };
    fn editor(text: &str) -> Workbench {
        let mut app = Workbench::default();
        app.new_file("snippet-gui-test.txt", TextEncoding::Utf8);
        app.insert_text(text).unwrap();
        app.go_to_position(TextPosition { line: 0, column: 0 })
            .unwrap();
        app
    }
    #[test]
    fn snippet_menu_paste_keeps_parameter_target_and_editor_unchanged() {
        let mut state = State::new(&crate::GuiOptions::default()).unwrap();
        state.workbench = editor("source\r\n");
        let target = SnippetTarget::capture(&state.workbench).unwrap();
        state.snippet_gui = Some(SnippetGui::new(target, vec![&INSERT], 7));
        state.snippet_command(CommandId::SnippetNext);
        let input = state.snippet_input_target().unwrap();
        let before = state.workbench.active_bytes().unwrap();
        let panes = format!("{:?}", state.workbench.panes());
        let generation = state.selector_generation;
        assert!(
            matches!(state.snippet_key(0x56, Modifiers::CTRL), Action::PasteSnippet(t) if t == input)
        );
        assert!(
            matches!(state.execute_menu(CommandId::EditPaste), Action::PasteSnippet(t) if t == input)
        );
        assert_eq!(state.selector_generation, generation);
        assert_eq!(state.snippet_input_target(), Some(input));
        assert_eq!(state.workbench.active_bytes().unwrap(), before);
        assert_eq!(format!("{:?}", state.workbench.panes()), panes);
        state.snippet_command(CommandId::SnippetNext);
        assert!(state.snippet_gui.as_ref().unwrap().ready(&state.workbench));
        assert!(state.field_paste_action().is_none());
        assert!(matches!(
            state.execute_menu(CommandId::EditPaste),
            Action::None
        ));
        assert!(state.snippet_gui.as_ref().unwrap().ready(&state.workbench));
        assert_eq!(state.workbench.active_bytes().unwrap(), before);
        assert_eq!(format!("{:?}", state.workbench.panes()), panes);
    }
    #[test]
    fn gui_snippet_wizard_review_cancel_and_apply_preserve_source_until_one_undo() {
        let source = "\u{feff}before\r\nafter\n";
        let mut app = editor(source);
        app.split_editor_right();
        let original = app.active_bytes().unwrap();
        let mut ui = SnippetGui::new(SnippetTarget::capture(&app).unwrap(), vec![&INSERT], 1);
        ui.next(&app).unwrap();
        ui.edit(FieldEdit::Clear, &app).unwrap();
        ui.edit(FieldEdit::Insert("界e\u{301}"), &app).unwrap();
        ui.next(&app).unwrap();
        assert!(ui.ready(&app));
        let preview = ui.session.as_ref().unwrap().plan().unwrap().rows(0, 24, 96);
        assert!(preview.iter().any(|row| row.starts_with("+++")));
        assert_eq!(app.active_bytes().unwrap(), original);
        ui.session.as_mut().unwrap().apply(&mut app).unwrap();
        assert!(app
            .active_bytes()
            .unwrap()
            .starts_with("\u{feff}BLOCK 界e\u{301}:\r\n".as_bytes()));
        app.undo().unwrap();
        assert_eq!(app.active_bytes().unwrap(), original);
    }
    #[test]
    fn gui_snippet_stale_caret_rejects_review_acceptance_without_editing() {
        let mut app = editor("source\r\n");
        let mut ui = SnippetGui::new(SnippetTarget::capture(&app).unwrap(), vec![&INSERT], 2);
        ui.next(&app).unwrap();
        ui.next(&app).unwrap();
        app.move_right(false).unwrap();
        assert!(!ui.ready(&app));
        assert!(ui.session.as_mut().unwrap().apply(&mut app).is_err());
        assert_eq!(app.active_bytes().unwrap(), b"source\r\n");
    }
    #[test]
    fn gui_snippet_fields_remove_graphemes_and_reject_oversize_paste_atomically() {
        let app = editor("source");
        let mut ui = SnippetGui::new(SnippetTarget::capture(&app).unwrap(), vec![&INSERT], 3);
        ui.edit(FieldEdit::Insert("e\u{301}👨‍👩‍👧‍👦"), &app).unwrap();
        ui.edit(FieldEdit::Backspace, &app).unwrap();
        assert_eq!(ui.query, "e\u{301}");
        let before = (ui.query.clone(), ui.cursor, ui.input);
        assert!(ui.edit(FieldEdit::Insert(&"x".repeat(257)), &app).is_err());
        assert_eq!((ui.query.clone(), ui.cursor, ui.input), before);
        assert!(ui.edit(FieldEdit::Insert("a\r\nb"), &app).is_err());
        assert_eq!(app.active_bytes().unwrap(), b"source");
    }
    #[test]
    fn gui_snippet_surround_review_preserves_selected_mixed_eol_and_shared_undo() {
        let mut app = editor("one\r\ntwo\nthree");
        app.set_linear_selection_bytes(0, 9).unwrap();
        let original = app.active_bytes().unwrap();
        let mut ui = SnippetGui::new(SnippetTarget::capture(&app).unwrap(), vec![&SURROUND], 4);
        ui.next(&app).unwrap();
        assert!(ui.ready(&app));
        assert_eq!(app.active_bytes().unwrap(), original);
        ui.session.as_mut().unwrap().apply(&mut app).unwrap();
        assert_eq!(
            app.active_bytes().unwrap(),
            b"DO:\r\none\r\ntwo\r\nEND.\nthree"
        );
        app.undo().unwrap();
        assert_eq!(app.active_bytes().unwrap(), original);
    }
}
