//! Native multiline literal fields, sharing bounded editing/presentation with TUI.
use super::{Action, State};
use jc_app::{
    edit_field, present_field, CommandId, FieldEdit, FieldPresentation, Key, KeyScope, KeyStroke,
    Modifiers, Resolution,
};

pub(super) struct SearchPanel {
    pub query: String,
    pub replacement: String,
    pub query_cursor: usize,
    pub replacement_cursor: usize,
    pub replace: bool,
    pub replacement_active: bool,
    pub generation: u64,
}

pub(super) struct SearchPresentation {
    pub find: FieldPresentation,
    pub replacement: Option<FieldPresentation>,
    pub replacement_active: bool,
    pub message: String,
}
impl SearchPanel {
    pub fn new(query: String, replace: bool) -> Self {
        Self {
            query_cursor: query.len(),
            query,
            replacement: String::new(),
            replacement_cursor: 0,
            replace,
            replacement_active: false,
            generation: 0,
        }
    }
    pub fn edit(&mut self, edit: FieldEdit<'_>) -> Result<bool, &'static str> {
        self.generation = self.generation.wrapping_add(1);
        if self.replacement_active {
            edit_field(
                &mut self.replacement,
                &mut self.replacement_cursor,
                edit,
                4096,
            )
            .map(|_| false)
        } else {
            edit_field(&mut self.query, &mut self.query_cursor, edit, 4096)
        }
    }
}
impl State {
    pub(super) fn execute_search_option(&mut self, command: CommandId) {
        if self.search_panel.is_none() {
            if let Some(panel) = self.selector_search_panel.take() {
                self.close_selector();
                self.search_panel = Some(panel);
            } else {
                self.open_search_panel(false);
            }
        }
        match self.workbench.execute(command) {
            Ok(_) => {
                let panel = self.search_panel.as_mut().unwrap();
                panel.query = self.workbench.search().query.clone();
                panel.query_cursor = panel.query.len();
                // Invalidate a clipboard handoff even when the history endpoint
                // repeats the same query. Replacement and active field stay owned.
                panel.generation = panel.generation.wrapping_add(1);
                self.status = "Consulta/opciones restauradas · fuente intacto".into();
            }
            Err(error) => self.error(error),
        }
        self.schedule_search();
    }
    pub(super) fn open_search_panel(&mut self, replace: bool) {
        self.close_selector();
        self.search_panel = Some(SearchPanel::new(
            self.workbench.search().query.clone(),
            replace,
        ));
        if let Err(error) = self
            .workbench
            .set_search_query(self.workbench.search().query.clone())
        {
            self.error(error);
        }
        self.schedule_search();
    }
    pub(super) fn schedule_search(&self) {
        if self.workbench.search_needs_poll() {
            self.bridge.signal();
        }
    }
    pub(super) fn edit_search_input(&mut self, edit: FieldEdit<'_>) {
        let Some(panel) = self.search_panel.as_mut() else {
            return;
        };
        match panel.edit(edit) {
            Ok(true) => {
                let query = panel.query.clone();
                if let Err(error) = self.workbench.set_search_query(query) {
                    self.error(error);
                } else {
                    self.status = "Consulta actualizada · EOL exacto del fuente".into();
                }
            }
            Ok(false) => {
                self.status = "Campo listo · texto exacto del fuente; sin normalización".into()
            }
            Err(error) => self.error(error),
        }
        self.schedule_search();
    }
    pub(super) fn search_key(&mut self, code: usize, modifiers: Modifiers) -> Action {
        let key = match code {
            0x26 => Some(Key::Up),
            0x28 => Some(Key::Down),
            0x43 => Some(Key::Char('c')),
            0x57 => Some(Key::Char('w')),
            0x52 => Some(Key::Char('r')),
            _ => None,
        };
        if let Some(key) = key {
            let scope = if self.search_panel.as_ref().unwrap().replace {
                KeyScope::Replace
            } else {
                KeyScope::Find
            };
            match self.keymap.resolve(
                scope,
                KeyStroke::new(modifiers, key),
                &self.registry,
                &self.context(),
            ) {
                Resolution::Enabled(
                    command @ (CommandId::SearchHistoryPrevious
                    | CommandId::SearchHistoryNext
                    | CommandId::SearchToggleCase
                    | CommandId::SearchToggleWholeWord
                    | CommandId::SearchToggleRegex),
                ) => return self.execute(command),
                Resolution::Disabled(
                    CommandId::SearchHistoryPrevious | CommandId::SearchHistoryNext,
                ) => {
                    self.status = "El historial de búsqueda está vacío".into();
                    return Action::None;
                }
                _ => {}
            }
        }
        if code == 0x55 && modifiers.contains(Modifiers::CTRL) {
            self.edit_search_input(FieldEdit::Clear);
            return Action::None;
        }
        if code == 0x1b {
            self.workbench.cancel_search();
            self.close_selector();
            return Action::None;
        }
        if modifiers.contains(Modifiers::CTRL) && code == 0x56 {
            return Action::PasteSearch(self.search_panel.as_ref().unwrap().generation);
        }
        let insert = match code {
            0x71 if modifiers == Modifiers::NONE => Some("\n"),
            0x73 if modifiers == Modifiers::NONE => Some("\r\n"),
            0x76 if modifiers == Modifiers::NONE => Some("\t"),
            0x77 if modifiers == Modifiers::NONE => Some("\r"),
            0x0d if modifiers.contains(Modifiers::SHIFT) || modifiers.contains(Modifiers::ALT) => {
                Some("\n")
            }
            _ => None,
        };
        if let Some(text) = insert {
            self.edit_search_input(FieldEdit::Insert(text));
            return Action::None;
        }
        if code == 0x09 {
            let panel = self.search_panel.as_mut().unwrap();
            if panel.replace {
                panel.replacement_active = !panel.replacement_active;
                panel.generation = panel.generation.wrapping_add(1);
            }
            return Action::None;
        }
        if code == 0x72 {
            let result = if modifiers.contains(Modifiers::SHIFT) {
                self.workbench.find_previous()
            } else {
                self.workbench.find_next()
            };
            if let Err(error) = result {
                self.error(error);
            }
            self.schedule_search();
            return Action::None;
        }
        if code == 0x0d || code == 0x75 || code == 0x78 {
            let panel = self.search_panel.as_ref().unwrap();
            let result = if panel.replace {
                if code == 0x78 || modifiers.contains(Modifiers::CTRL) {
                    self.workbench
                        .replace_all(&panel.replacement)
                        .map(|n| format!("Se reemplazaron {n} coincidencias"))
                } else {
                    self.workbench
                        .replace_current(&panel.replacement)
                        .map(|yes| {
                            if yes {
                                "Se reemplazó la coincidencia actual".into()
                            } else {
                                "No hay coincidencia para reemplazar".into()
                            }
                        })
                }
            } else {
                self.workbench.find_next().map(|hit| {
                    if hit.is_some() {
                        "Coincidencia seleccionada".into()
                    } else {
                        "Aún no hay coincidencias".into()
                    }
                })
            };
            match result {
                Ok(status) => self.status = status,
                Err(error) => self.error(error),
            }
            self.schedule_search();
            return Action::None;
        }
        let edit = match code {
            0x25 => Some(FieldEdit::Left),
            0x27 => Some(FieldEdit::Right),
            0x26 => Some(FieldEdit::Up),
            0x28 => Some(FieldEdit::Down),
            0x24 => Some(FieldEdit::Home),
            0x23 => Some(FieldEdit::End),
            0x08 => Some(FieldEdit::Backspace),
            0x2e => Some(FieldEdit::Delete),
            _ => None,
        };
        if let Some(edit) = edit {
            self.edit_search_input(edit);
        }
        Action::None
    }
    pub(super) fn search_presentation(&self) -> Option<SearchPresentation> {
        let panel = self.search_panel.as_ref()?;
        let columns = ((self.width - 80) / self.cell_width).clamp(8, 88) as usize;
        let height = ((self.height / self.cell_height - 10) / 2).clamp(1, 6) as usize;
        Some(SearchPresentation {
            find: present_field(&panel.query, panel.query_cursor, columns, height),
            replacement: panel.replace.then(|| {
                present_field(
                    &panel.replacement,
                    panel.replacement_cursor,
                    columns,
                    height,
                )
            }),
            replacement_active: panel.replacement_active,
            message: format!(
                "Regex:{} (reemplazo literal) · Mayúsculas:{} Palabra:{} · {} · {} · {}",
                if self.workbench.search().regex {
                    "sí"
                } else {
                    "no"
                },
                if self.workbench.search().case_sensitive {
                    "sí"
                } else {
                    "no"
                },
                if self.workbench.search().whole_word {
                    "sí"
                } else {
                    "no"
                },
                self.workbench.search().status.label(),
                self.workbench.search_result_summary(),
                self.status
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_menu_paste_matches_keyboard_without_closing_find_or_replace() {
        let mut s = State::new(&crate::GuiOptions::default()).unwrap();
        s.workbench.insert_text("sin guardar\r\n").unwrap();
        let before = s.workbench.active_bytes().unwrap();
        let panes = format!("{:?}", s.workbench.panes());
        for replace in [false, true] {
            s.open_search_panel(replace);
            s.edit_search_input(FieldEdit::Clear);
            s.edit_search_input(FieldEdit::Insert("consulta"));
            for replacement_active in [false, true] {
                if replacement_active && !replace {
                    continue;
                }
                s.search_panel.as_mut().unwrap().replacement_active = replacement_active;
                let generation = s.search_panel.as_ref().unwrap().generation;
                assert!(
                    matches!(s.search_key(0x56, Modifiers::CTRL), Action::PasteSearch(g) if g == generation)
                );
                assert!(
                    matches!(s.execute_menu(CommandId::EditPaste), Action::PasteSearch(g) if g == generation)
                );
                let panel = s.search_panel.as_ref().unwrap();
                assert_eq!(panel.query, "consulta");
                assert_eq!(panel.replace, replace);
                assert_eq!(panel.replacement_active, replacement_active);
                assert_eq!(panel.generation, generation);
                assert_eq!(s.workbench.active_bytes().unwrap(), before);
                assert_eq!(format!("{:?}", s.workbench.panes()), panes);
            }
        }
    }
}
