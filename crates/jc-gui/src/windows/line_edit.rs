//! Frontend intention metadata only. Workbench owns all six transformations.
use super::State;
use jc_app::{CommandId, DocumentId, PaneId, ViewId};
use jc_text::TextEncoding;

pub(super) fn is_line_edit(command: CommandId) -> bool {
    matches!(
        command,
        CommandId::EditDuplicate
            | CommandId::EditDeleteLine
            | CommandId::EditMoveLineUp
            | CommandId::EditMoveLineDown
            | CommandId::EditToggleLineComment
            | CommandId::EditToggleBlockComment
    )
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct EditTarget {
    document: DocumentId,
    revision: u64,
    encoding: Option<TextEncoding>,
    view: Option<ViewId>,
    pane: PaneId,
    cursor: usize,
    anchor: Option<usize>,
    rectangle: Option<(usize, usize, u64)>,
}
impl State {
    pub(super) fn source_edit_target(&self) -> Option<EditTarget> {
        let document = self.workbench.active_document()?;
        let pane = self.workbench.panes().active();
        let editor = self.workbench.active_editor()?;
        Some(EditTarget {
            document: document.id,
            revision: document.session.document().revision(),
            encoding: document.session.encoding(),
            view: self.workbench.panes().view(pane).and_then(|v| v.view_id()),
            pane,
            cursor: editor.cursor,
            anchor: editor.anchor,
            rectangle: editor
                .rectangle
                .map(|r| (r.anchor_column.0, r.head_column.0, r.revision)),
        })
    }
}
