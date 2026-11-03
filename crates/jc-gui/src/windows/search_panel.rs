//! Native multiline literal fields, sharing bounded editing/presentation with TUI.
use super::{Action, State};
use jc_app::{edit_field, present_field, FieldEdit, FieldPresentation, Modifiers};

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
        if self.workbench.search_running() {
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
                    self.status = "Literal query updated · exact EOL".into();
                }
            }
            Ok(false) => self.status = "Field ready · exact source text; no normalization".into(),
            Err(error) => self.error(error),
        }
        self.schedule_search();
    }
    pub(super) fn search_key(&mut self, code: usize, modifiers: Modifiers) -> Action {
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
                        .map(|n| format!("Replaced {n} matches"))
                } else {
                    self.workbench
                        .replace_current(&panel.replacement)
                        .map(|yes| {
                            if yes {
                                "Replaced current match".into()
                            } else {
                                "No match to replace".into()
                            }
                        })
                }
            } else {
                self.workbench.find_next().map(|hit| {
                    if hit.is_some() {
                        "Match selected".into()
                    } else {
                        "No match yet".into()
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
                "Literal exact EOL · Case:{} Word:{} · {:?} · {} matches · {}",
                self.workbench.search().case_sensitive,
                self.workbench.search().whole_word,
                self.workbench.search().status,
                self.workbench.search().matches.len(),
                self.status
            ),
        })
    }
}
