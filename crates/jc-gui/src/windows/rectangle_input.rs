//! Mouse/command intentions only. All rectangular geometry and edits stay in jc-app.
use super::{line_edit::EditTarget, paint, State};
use jc_app::{rectangle::VisualPosition, CommandId, PaneId};

pub(super) fn is_rectangle(command: CommandId) -> bool {
    matches!(
        command,
        CommandId::EditRectangleStart
            | CommandId::EditRectangleCancel
            | CommandId::EditRectangleLeft
            | CommandId::EditRectangleRight
            | CommandId::EditRectangleUp
            | CommandId::EditRectangleDown
    )
}
#[derive(Clone, Copy)]
pub(super) struct RectangleDrag {
    anchor: VisualPosition,
    pane: PaneId,
    target: EditTarget,
}
impl State {
    pub(super) fn rectangle_status(&mut self) {
        self.status = match self
            .workbench
            .rectangle_for_pane(self.workbench.panes().active())
        {
            Some(rect) => format!(
                "Rectangle TAB4 · {} rows × {} cells · internal clipboard",
                rect.last_line - rect.first_line + 1,
                rect.right - rect.left
            ),
            None => "Rectangle cancelled".into(),
        };
    }
    pub(super) fn begin_rectangle_drag(&mut self, x: i32, y: i32) -> bool {
        self.rectangle_drag = None;
        let (pane, anchor) = match paint::rectangle_hit(self, x, y, None) {
            Ok(Some(hit)) => hit,
            Ok(None) => return false,
            Err(error) => {
                self.error(error);
                return false;
            }
        };
        self.explorer_focused = false;
        self.bottom.focused = false;
        if !self.workbench.focus_editor_pane(pane) {
            return false;
        }
        if !self
            .registry
            .is_enabled(CommandId::EditRectangleStart, &self.context())
        {
            return false;
        }
        if let Err(error) = self.workbench.set_rectangle(anchor, anchor) {
            self.error(error);
            return false;
        }
        let Some(target) = self.source_edit_target() else {
            return false;
        };
        self.rectangle_drag = Some(RectangleDrag {
            anchor,
            pane,
            target,
        });
        self.rectangle_status();
        true
    }
    pub(super) fn validate_rectangle_drag(&mut self) -> bool {
        if self.rectangle_drag.is_some_and(|drag| {
            self.source_edit_target() != Some(drag.target)
                || !self.context().editor_input
                || self.workbench.rectangle_for_pane(drag.pane).is_none()
        }) {
            self.rectangle_drag = None;
            self.status = "Rectangle drag target changed".into();
            return false;
        }
        self.rectangle_drag.is_some()
    }
    pub(super) fn update_rectangle_drag(&mut self, x: i32, y: i32) {
        if !self.validate_rectangle_drag() {
            return;
        }
        let drag = self.rectangle_drag.unwrap();
        let head = match paint::rectangle_hit(self, x, y, Some(drag.pane)) {
            Ok(Some((_, head))) => head,
            Ok(None) => return,
            Err(error) => {
                self.error(error);
                return;
            }
        };
        if let Err(error) = self.workbench.set_rectangle(drag.anchor, head) {
            self.error(error);
            return;
        }
        if let Some(target) = self.source_edit_target() {
            self.rectangle_drag = Some(RectangleDrag { target, ..drag });
        }
        self.rectangle_status();
    }
}
