//! An explicit save handoff survives a native modal dialog without holding the
//! live model. It owns only bounded metadata, never source bytes or undo.
use super::{title_for_path, DocumentId, Workbench, WorkbenchError};
use std::path::PathBuf;

#[derive(Debug)]
pub struct SaveTarget {
    document: DocumentId,
    revision: u64,
    path: PathBuf,
    new_file: bool,
}
impl Workbench {
    pub fn modified_documents(&self) -> impl Iterator<Item = (DocumentId, u64, bool)> + '_ {
        self.documents
            .iter()
            .filter(|d| d.session.document().is_modified())
            .map(|d| (d.id, d.session.document().revision(), d.session.is_new()))
    }
    pub fn capture_save_target(
        &self,
        document: DocumentId,
        expected_revision: u64,
    ) -> Result<SaveTarget, WorkbenchError> {
        let document = self
            .document_by_id(document)
            .ok_or(WorkbenchError::NoDocument)?;
        if document.session.document().revision() != expected_revision {
            return Err(WorkbenchError::Save(
                "El documento cambió antes de admitir el guardado".into(),
            ));
        }
        if document.session.path().as_os_str().len() > 4096 {
            return Err(WorkbenchError::Save(
                "La ruta de guardado supera 4096 unidades nativas".into(),
            ));
        }
        Ok(SaveTarget {
            document: document.id,
            revision: expected_revision,
            path: document.session.path().to_owned(),
            new_file: document.session.is_new(),
        })
    }
    /// Destination is mandatory for a new GUI document; passing Some is the
    /// frontend's explicit path decision. Existing-file conflict/no-clobber and
    /// atomic save guarantees remain FileSession's responsibility.
    pub fn save_target(
        &mut self,
        target: SaveTarget,
        destination: Option<PathBuf>,
    ) -> Result<(), WorkbenchError> {
        let document = self
            .documents
            .iter_mut()
            .find(|d| d.id == target.document)
            .ok_or(WorkbenchError::NoDocument)?;
        if document.session.document().revision() != target.revision
            || document.session.path() != target.path
            || document.session.is_new() != target.new_file
        {
            return Err(WorkbenchError::Save(
                "El destino cambió durante la decisión de guardado; inténtalo de nuevo".into(),
            ));
        }
        if let Some(path) = destination {
            if path.as_os_str().len() > 4096 {
                return Err(WorkbenchError::Save(
                    "El destino de guardado supera 4096 unidades nativas".into(),
                ));
            }
            document.session.save_as(path)?;
            if document.session.path() != target.path {
                self.task_panels.clear_lsp(target.document);
            }
        } else {
            if target.new_file {
                return Err(WorkbenchError::Save(
                    "Un documento nuevo requiere una ruta explícita de Guardar como".into(),
                ));
            }
            document.session.save()?;
        }
        self.tabs.set_title(
            document.tab_id,
            title_for_path(document.session.path(), "<file>"),
        );
        self.tabs
            .set_dirty(document.tab_id, document.session.document().is_modified());
        Ok(())
    }
}
