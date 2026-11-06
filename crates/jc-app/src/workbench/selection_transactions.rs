//! Selection metadata belongs to the application, never to the byte editor.
use super::{
    selection_history::{SelectionRecord, SelectionState},
    DocumentId, Workbench, WorkbenchError,
};

impl Workbench {
    pub(super) fn selection_edit<T>(
        &mut self,
        action: impl FnOnce(&mut Self) -> Result<T, WorkbenchError>,
    ) -> Result<T, WorkbenchError> {
        match self.active_document().map(|d| d.id) {
            Some(id) => self.selection_edit_document(id, action),
            None => action(self),
        }
    }

    /// Reserve first and publish metadata after the complete logical command,
    /// including its final caret/rectangle. Nested text edits share this scope.
    pub(crate) fn selection_edit_document<T>(
        &mut self,
        id: DocumentId,
        action: impl FnOnce(&mut Self) -> Result<T, WorkbenchError>,
    ) -> Result<T, WorkbenchError> {
        if self.selection_transaction == Some(id) {
            return action(self);
        }
        let origin = self
            .panes
            .view(self.panes.active())
            .and_then(|p| p.view_id())
            .filter(|view| {
                self.panes.tab_for_view(*view) == self.document_by_id(id).map(|d| d.tab_id)
            });
        let before = origin.and_then(|view| {
            self.panes
                .state_for_view(view)
                .map(|state| (view, SelectionState::capture(state)))
        });
        let capture = if let Some(document) = self.documents.iter_mut().find(|d| d.id == id) {
            match (before, document.session.encoding()) {
                (Some((view, state)), Some(encoding)) => {
                    document
                        .selection_history
                        .reserve_for_edit(document.session.document())?;
                    Some((
                        view,
                        encoding,
                        document.session.document().revision(),
                        state,
                    ))
                }
                _ => None,
            }
        } else {
            None
        };
        let previous = self.selection_transaction.replace(id);
        // Reset nesting even if a frontend catches a command panic. No recovery
        // or source I/O runs here, and the ordinary path allocates no guard.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| action(self)));
        self.selection_transaction = previous;
        let result = match result {
            Ok(result) => result,
            Err(payload) => std::panic::resume_unwind(payload),
        };
        let after = capture.and_then(|(view, encoding, revision, state)| {
            self.panes.state_for_view(view).map(|after| {
                (
                    view,
                    encoding,
                    revision,
                    state,
                    SelectionState::capture(after),
                )
            })
        });
        if let Some(document) = self.documents.iter_mut().find(|d| d.id == id) {
            let source = document.session.document();
            if let Some((view, encoding, revision, before, after)) = after {
                if revision != source.revision() {
                    document.selection_history.record(
                        SelectionRecord {
                            before_revision: revision,
                            after_revision: source.revision(),
                            view,
                            encoding,
                            before,
                            after,
                        },
                        source,
                    );
                }
            }
            document.selection_history.prune(source);
        }
        result
    }

    pub(super) fn restore_history_selection(&mut self, before: u64, after: u64, undo: bool) {
        let Some(document) = self.active_document() else {
            return;
        };
        let Some(record) = document.selection_history.find(before, after) else {
            return;
        };
        let tab = document.tab_id;
        if document.session.encoding() != Some(record.encoding)
            || self.panes.tab_for_view(record.view) != Some(tab)
        {
            return;
        }
        let revision = document.session.document().revision();
        if let Some(state) = self.panes.state_for_view_mut(record.view) {
            if undo { record.before } else { record.after }.restore(state, revision);
        }
        self.normalize_views(tab);
    }

    pub(super) fn prune_active_selection_history(&mut self) {
        if let Some(document) = self.active_document_mut() {
            document
                .selection_history
                .prune(document.session.document());
        }
    }
}
