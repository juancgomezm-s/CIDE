//! A bounded modal intention; Workbench owns scalar conversion and jump history.
use super::{Action, State};
use jc_app::{
    edit_field, present_field, FieldEdit, FieldPresentation, Modifiers, PaneId, TaskStamp,
};

pub(super) struct NavigationPanel {
    pub input: String,
    pub cursor: usize,
    pub target: TaskStamp,
    pub pane: PaneId,
    pub generation: u64,
}

pub(super) struct NavigationPresentation {
    pub field: FieldPresentation,
    pub message: String,
}
impl State {
    pub(super) fn open_navigation_panel(&mut self) {
        let Some(document) = self.workbench.active_document() else {
            self.error("No hay documento para ir a una posición");
            return;
        };
        let Some(encoding) = document.session.encoding() else {
            self.open_encoding_selector();
            self.error("Elige la codificación del fuente antes de ir a una posición");
            return;
        };
        let pane = self.workbench.panes().active();
        let view = self.workbench.panes().view(pane).and_then(|v| v.view_id());
        let Some(view) = view else { return };
        let target = TaskStamp {
            document: document.id,
            revision: document.session.document().revision(),
            encoding,
            view: Some(view),
        };
        let Some(generation) = self.navigation_generation.checked_add(1) else {
            self.error("Se agotó la generación de navegación");
            return;
        };
        self.close_selector();
        self.workbench.cancel_search_navigation();
        self.navigation_generation = generation;
        self.navigation_panel = Some(NavigationPanel {
            input: String::new(),
            cursor: 0,
            target,
            pane,
            generation,
        });
        self.status = if self.workbench.active_paged_byte().is_some() {
            "Usa @byte, desde cero, para ir a cualquier tramo del archivo"
        } else {
            "Usa línea[:columna], coordenadas escalares del fuente desde1"
        }
        .into();
    }
    pub(super) fn navigation_target_current(&self) -> bool {
        self.navigation_panel.as_ref().is_some_and(|panel| {
            panel.pane == self.workbench.panes().active()
                && self
                    .workbench
                    .panes()
                    .view(panel.pane)
                    .and_then(|v| v.view_id())
                    == panel.target.view
                && self.workbench.active_document().is_some_and(|d| {
                    d.id == panel.target.document
                        && d.session.document().revision() == panel.target.revision
                        && d.session.encoding() == Some(panel.target.encoding)
                })
        })
    }
    pub(super) fn edit_navigation_input(&mut self, edit: FieldEdit<'_>) {
        if !self.navigation_target_current() {
            self.navigation_panel = None;
            self.error("Cambió el destino; vuelve a abrir Ir a posición");
            return;
        }
        let Some(generation) = self.navigation_generation.checked_add(1) else {
            self.error("Se agotó la generación de navegación");
            return;
        };
        self.navigation_generation = generation;
        let panel = self.navigation_panel.as_mut().unwrap();
        panel.generation = generation;
        match edit_field(&mut panel.input, &mut panel.cursor, edit, 64) {
            Ok(_) => self.status = "Ir a posición · Intro acepta; Escape cancela".into(),
            Err(error) => self.error(error),
        }
    }
    fn accept_navigation(&mut self) {
        if !self.navigation_target_current() {
            self.navigation_panel = None;
            self.error("Cambió el destino; vuelve a abrir Ir a posición");
            return;
        }
        if self.workbench.active_paged_byte().is_some() {
            let panel = self.navigation_panel.as_ref().unwrap();
            match self.workbench.go_to_paged_input(&panel.input) {
                Ok(()) => {
                    self.close_selector();
                    self.bottom.focused = false;
                    self.status = "Destino solicitado".into();
                }
                Err(error) => self.error(error),
            }
            return;
        }
        let Some(position) = self
            .navigation_panel
            .as_ref()
            .and_then(|p| jc_app::parse_text_position(&p.input))
        else {
            self.error("Usa línea[:columna], coordenadas escalares del fuente desde1");
            return;
        };
        match self.workbench.go_to_position(position) {
            Ok(()) => {
                self.close_selector();
                self.bottom.focused = false;
                self.status = "Posición elegida · coordenadas escalares del fuente".into();
            }
            Err(error) => self.error(error),
        }
    }
    pub(super) fn navigation_key(&mut self, code: usize, modifiers: Modifiers) -> Action {
        if code == 0x1b {
            self.close_selector();
            self.status = "Ir a posición cancelado".into();
            return Action::None;
        }
        if !self.navigation_target_current() {
            self.navigation_panel = None;
            self.error("Cambió el destino; vuelve a abrir Ir a posición");
            return Action::None;
        }
        if modifiers == Modifiers::CTRL && code == 0x56 {
            return Action::PasteNavigation(self.navigation_panel.as_ref().unwrap().generation);
        }
        if modifiers == Modifiers::CTRL && code == 0x55 {
            self.edit_navigation_input(FieldEdit::Clear);
            return Action::None;
        }
        if modifiers != Modifiers::NONE {
            return Action::None;
        }
        if code == 0x0d {
            self.accept_navigation();
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
            self.edit_navigation_input(edit);
        }
        Action::None
    }
    pub(super) fn navigation_presentation(&self) -> Option<NavigationPresentation> {
        let panel = self.navigation_panel.as_ref()?;
        let columns = ((self.width - 80) / self.cell_width.clamp(1, 8192)).clamp(8, 64) as usize;
        Some(NavigationPresentation {
            field: present_field(&panel.input, panel.cursor, columns, 1),
            message: if self.navigation_target_current() {
                self.status.clone()
            } else {
                "Cambió el destino; vuelve a abrir Ir a posición".into()
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_menu_paste_preserves_keyboard_target_draft_and_editor() {
        let mut s = State::new(&crate::GuiOptions::default()).unwrap();
        s.workbench.insert_text("uno\r\ndos\r\n").unwrap();
        let before = s.workbench.active_bytes().unwrap();
        let panes = format!("{:?}", s.workbench.panes());
        s.open_navigation_panel();
        s.edit_navigation_input(FieldEdit::Insert("2:3"));
        let generation = s.navigation_panel.as_ref().unwrap().generation;
        assert!(
            matches!(s.navigation_key(0x56, Modifiers::CTRL), Action::PasteNavigation(g) if g == generation)
        );
        assert!(
            matches!(s.execute_menu(jc_app::CommandId::EditPaste), Action::PasteNavigation(g) if g == generation)
        );
        assert!(s.navigation_target_current());
        assert_eq!(s.navigation_panel.as_ref().unwrap().input, "2:3");
        assert_eq!(s.navigation_panel.as_ref().unwrap().generation, generation);
        assert_eq!(s.workbench.active_bytes().unwrap(), before);
        assert_eq!(format!("{:?}", s.workbench.panes()), panes);
    }
}
