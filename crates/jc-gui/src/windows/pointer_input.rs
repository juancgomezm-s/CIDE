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
    pub(super) fn pointer_wheel(&mut self, x: i32, y: i32, delta: i32, lines: u32, flags: usize) {
        self.linear_drag = None;
        self.rectangle_drag = None;
        let modifiers = self.modifiers();
        if flags & 0x0c != 0
            || modifiers.contains(Modifiers::ALT)
            || modifiers.contains(Modifiers::CTRL)
            || modifiers.contains(Modifiers::SHIFT)
            || !self.pointer_source_available()
        {
            self.wheel = None;
            return;
        }
        let Some((pane, rows)) = paint::source_pane_at(self, x, y) else {
            self.wheel = None;
            return;
        };
        let Some(context) = self.pointer_context(pane) else {
            self.wheel = None;
            return;
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
            self.workbench
                .scroll_view_vertical(pane, -(notches as isize * lines as isize), rows);
        }
    }
}
