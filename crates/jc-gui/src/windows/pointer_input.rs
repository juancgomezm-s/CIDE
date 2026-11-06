//! Pointer intentions retain identity, never a mutable document or decoded copy.
use super::{line_edit::EditTarget, paint, State};
use jc_app::{Modifiers, PaneId, PrimaryFocus};
use std::path::PathBuf;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct PointerContext {
    edit: EditTarget,
    pane: PaneId,
    path: PathBuf,
    root: Option<PathBuf>,
    invisibles: bool,
    focus: PrimaryFocus,
}

pub(super) struct LinearDrag {
    context: PointerContext,
    anchor: usize,
    last: (i32, i32),
}
pub(super) struct WheelState {
    context: PointerContext,
    remainder: i32,
}
impl State {
    pub(super) fn pointer_source_available(&self) -> bool {
        self.selector.is_none()
            && self.navigation_panel.is_none()
            && self.search_panel.is_none()
            && !self.project_visible()
            && self.replace_panel.is_none()
            && self.git_panel.is_none()
            && !self.open_diff_visible
            && !self.source_visible()
            && self.snippet_gui.is_none()
            && self.markdown_gui.is_none()
    }
    pub(super) fn pointer_context(&self, pane: PaneId) -> Option<PointerContext> {
        let view = self.workbench.panes().view(pane)?;
        let doc = self.workbench.document_by_tab(view.location()?.tab)?;
        Some(PointerContext {
            edit: self.view_edit_target(pane)?,
            pane,
            path: doc.session.path().to_owned(),
            root: self.project_root.clone(),
            invisibles: self.workbench.invisibles_visible(),
            focus: self.context().focus,
        })
    }
    pub(super) fn cancel_pointer(&mut self) {
        self.linear_drag = None;
        self.rectangle_drag = None;
        self.editor_scroll_drag = None;
        self.explorer_scroll = Default::default();
        self.wheel = None;
    }
    pub(super) fn begin_linear_drag(&mut self, x: i32, y: i32, shift: bool) -> bool {
        self.cancel_pointer();
        if !self.pointer_source_available() {
            return false;
        }
        if let Some(pane) = paint::focus_pane_at(self, x, y) {
            self.workbench.focus_editor_pane(pane);
            self.explorer_focused = false;
            self.bottom.focused = false;
        }
        let (pane, head) = match paint::linear_hit(self, x, y, None) {
            Ok(Some(hit)) => hit,
            Ok(None) => return false,
            Err(error) => {
                self.error(error);
                return false;
            }
        };
        let state = self
            .workbench
            .panes()
            .view(pane)
            .and_then(|v| v.state())
            .expect("live hit pane");
        let anchor = if shift {
            if state.rectangle.is_some() {
                state.cursor
            } else {
                state.anchor.unwrap_or(state.cursor)
            }
        } else {
            head
        };
        if let Err(error) = self.workbench.set_linear_selection_bytes(anchor, head) {
            self.error(error);
            return false;
        }
        self.linear_drag = self.pointer_context(pane).map(|context| LinearDrag {
            context,
            anchor,
            last: (x, y),
        });
        self.linear_drag.is_some()
    }
    pub(super) fn validate_linear_drag(&mut self) -> bool {
        if self.linear_drag.as_ref().is_some_and(|drag| {
            !self.pointer_source_available()
                || self.workbench.panes().active() != drag.context.pane
                || self.pointer_context(drag.context.pane).as_ref() != Some(&drag.context)
        }) {
            self.linear_drag = None;
            self.status = "Cambió el destino del arrastre lineal; selección conservada".into();
        }
        self.linear_drag.is_some()
    }
    pub(super) fn update_linear_drag(&mut self, x: i32, y: i32) {
        if !self.validate_linear_drag() {
            return;
        }
        let drag = self.linear_drag.as_ref().unwrap();
        if drag.last == (x, y) {
            return;
        }
        let pane = drag.context.pane;
        let anchor = drag.anchor;
        let head = match paint::linear_hit(self, x, y, Some(pane)) {
            Ok(Some((_, head))) => head,
            Ok(None) => return,
            Err(error) => {
                self.error(error);
                return;
            }
        };
        if let Err(error) = self.workbench.set_linear_selection_bytes(anchor, head) {
            self.error(error);
            return;
        }
        self.linear_drag = self.pointer_context(pane).map(|context| LinearDrag {
            context,
            anchor,
            last: (x, y),
        });
    }
    pub(super) fn pointer_wheel(
        &mut self,
        x: i32,
        y: i32,
        delta: i32,
        lines: u32,
        flags: usize,
    ) -> bool {
        self.linear_drag = None;
        self.rectangle_drag = None;
        self.editor_scroll_drag = None;
        self.explorer_scroll.drag = None;
        let modifiers = self.modifiers();
        if self.git_panel.is_some() && flags & 0x0c == 0 && modifiers == Modifiers::NONE {
            return self.git_workspace_wheel(x, y, delta);
        }
        if flags & 0x0c != 0
            || modifiers.contains(Modifiers::ALT)
            || modifiers.contains(Modifiers::CTRL)
            || modifiers.contains(Modifiers::SHIFT)
            || !self.pointer_source_available()
        {
            self.wheel = None;
            self.explorer_scroll.clear_wheel();
            return false;
        }
        if let Some(changed) = self.explorer_wheel(x, y, delta, lines) {
            self.wheel = None;
            return changed;
        }
        self.explorer_scroll.clear_wheel();
        if self.outline_wheel(x, y, delta, lines) {
            self.wheel = None;
            return true;
        }
        let Some((pane, rows)) = paint::source_pane_at(self, x, y) else {
            self.wheel = None;
            return false;
        };
        let Some(context) = self.pointer_context(pane) else {
            self.wheel = None;
            return false;
        };
        let previous = self
            .wheel
            .take()
            .filter(|w| w.context == context)
            .map_or(0, |w| w.remainder);
        let total = previous.saturating_add(delta);
        let notches = total / 120;
        self.wheel = Some(WheelState {
            context,
            remainder: total % 120,
        });
        let lines = if lines == u32::MAX {
            rows
        } else {
            (lines as usize).min(128)
        };
        if notches != 0 && lines != 0 {
            let origins = |app: &jc_app::Workbench| {
                [PaneId::Primary, PaneId::Secondary]
                    .map(|pane| app.panes().view(pane).map(|view| view.scroll_line()))
            };
            let before = origins(&self.workbench);
            self.workbench
                .scroll_view_vertical(pane, -(notches as isize * lines as isize), rows);
            return origins(&self.workbench) != before;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jc_text::{TextEncoding, TextPosition};

    #[test]
    fn document_boundary_keys_respect_bom_orientation_bytes_and_focused_fields() {
        let mut state = State::new(&crate::GuiOptions::default()).unwrap();
        state.workbench.new_file("limites.txt", TextEncoding::Utf8);
        let text = "\u{feff}primera\r\nsegunda\nfinal";
        state.workbench.insert_text(text).unwrap();
        let length = text.len();
        let revision = state
            .workbench
            .active_document()
            .unwrap()
            .session
            .document()
            .revision();
        let press = |state: &mut State, code, modifiers| {
            state.key_override = Some(modifiers);
            state.key(code);
        };
        let selection = |state: &State| {
            let view = state.workbench.active_editor().unwrap();
            (view.cursor, view.anchor)
        };
        press(&mut state, 0x24, Modifiers::CTRL);
        assert_eq!(selection(&state), (3, None));
        press(&mut state, 0x23, Modifiers::CTRL.union(Modifiers::SHIFT));
        assert_eq!(selection(&state), (length, Some(3)));
        assert_eq!(
            state
                .workbench
                .selection_for_pane(state.workbench.panes().active()),
            Some(3..length)
        );
        press(&mut state, 0x23, Modifiers::CTRL);
        assert_eq!(selection(&state), (length, None));
        press(&mut state, 0x24, Modifiers::CTRL.union(Modifiers::SHIFT));
        assert_eq!(selection(&state), (3, Some(length)));

        // All four routes are palette-capable, without introducing menu rows.
        for command in [
            jc_app::CommandId::EditDocumentStart,
            jc_app::CommandId::EditDocumentEnd,
            jc_app::CommandId::EditSelectDocumentStart,
            jc_app::CommandId::EditSelectDocumentEnd,
        ] {
            assert!(super::super::supported(command));
            assert_eq!(super::super::command_id(command), 0);
        }
        let before = selection(&state);
        state.open_search_panel(false);
        press(&mut state, 0x23, Modifiers::CTRL);
        press(&mut state, 0x24, Modifiers::CTRL.union(Modifiers::SHIFT));
        assert_eq!(selection(&state), before);
        state.open_selector(super::super::selector::SelectorKind::Palette);
        press(&mut state, 0x23, Modifiers::CTRL);
        press(&mut state, 0x24, Modifiers::CTRL.union(Modifiers::SHIFT));
        assert_eq!(selection(&state), before);
        state.close_selector();
        state.explorer_focused = true;
        press(&mut state, 0x23, Modifiers::CTRL);
        press(&mut state, 0x24, Modifiers::CTRL.union(Modifiers::SHIFT));
        assert_eq!(selection(&state), before);
        assert_eq!(state.workbench.active_bytes().unwrap(), text.as_bytes());
        assert_eq!(
            state
                .workbench
                .active_document()
                .unwrap()
                .session
                .document()
                .revision(),
            revision
        );
    }

    #[test]
    fn page_navigation_uses_active_visible_rows_and_preserves_source_and_selection() {
        let mut state = State::new(&crate::GuiOptions::default()).unwrap();
        state.width = 1200;
        state.height = 700;
        state.workbench.new_file("paginas.txt", TextEncoding::Utf8);
        let source = "fila de prueba\r\n".repeat(200);
        state.workbench.insert_text(&source).unwrap();
        state.workbench.split_editor_right();
        for bottom_visible in [false, true] {
            state.bottom.visible = bottom_visible;
            state.bottom.focused = false;
            for pane in [PaneId::Primary, PaneId::Secondary] {
                state.workbench.focus_editor_pane(pane);
                state
                    .workbench
                    .go_to_position(TextPosition { line: 0, column: 0 })
                    .unwrap();
                let rows = paint::text_areas(&state)
                    .into_iter()
                    .find(|area| area.pane == pane)
                    .unwrap()
                    .capacity_rows;
                assert!(rows > 0);
                state.key_override = Some(Modifiers::NONE);
                assert!(state.key(0x22).0);
                assert_eq!(state.workbench.position_for_pane(pane).unwrap().line, rows);
                state.key_override = Some(Modifiers::SHIFT);
                assert!(state.key(0x21).0);
                assert_eq!(state.workbench.position_for_pane(pane).unwrap().line, 0);
                assert!(state.workbench.selection_for_pane(pane).is_some());
                assert_eq!(state.workbench.active_bytes().unwrap(), source.as_bytes());
            }
        }
    }
}
