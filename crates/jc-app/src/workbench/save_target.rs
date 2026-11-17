//! An explicit save handoff survives a native modal dialog without holding the
//! live model. It owns only bounded metadata, never source bytes or undo.
use super::{title_for_path, DocumentId, Workbench, WorkbenchError};
use jc_files::{FileError, PublishedSave};
use std::{io, path::PathBuf, sync::mpsc};

#[derive(Debug)]
pub struct SaveTarget {
    document: DocumentId,
    revision: u64,
    path: PathBuf,
    new_file: bool,
}

#[derive(Debug)]
pub(super) struct PendingSave {
    pub(super) document: DocumentId,
    result: mpsc::Receiver<Result<PublishedSave, FileError>>,
}

impl Workbench {
    pub fn save_pending(&self) -> bool {
        self.paged_save.is_some()
    }

    pub(super) fn require_no_pending_save(&self) -> Result<(), WorkbenchError> {
        if self.save_pending() {
            return Err(WorkbenchError::Save("Ya hay un guardado en curso".into()));
        }
        Ok(())
    }

    pub fn request_save_active(
        &mut self,
        destination: Option<PathBuf>,
    ) -> Result<bool, WorkbenchError> {
        let doc = self.active_document().ok_or(WorkbenchError::NoDocument)?;
        if destination.is_none() && !doc.session.document().is_paged() {
            self.save_active()?;
            return Ok(true);
        }
        let target = self.capture_save_target(doc.id, doc.session.document().revision())?;
        self.request_save_target(target, destination)
    }

    /// True for a completed resident save. Paged saves are confirmed by poll_save.
    pub fn request_save_target(
        &mut self,
        target: SaveTarget,
        destination: Option<PathBuf>,
    ) -> Result<bool, WorkbenchError> {
        self.require_no_pending_save()?;
        let doc = self
            .document_by_id(target.document)
            .ok_or(WorkbenchError::NoDocument)?;
        if !doc.session.document().is_paged() {
            self.save_target(target, destination)?;
            return Ok(true);
        }
        if doc.session.document().revision() != target.revision
            || doc.session.path() != target.path
            || doc.session.is_new() != target.new_file
        {
            return Err(WorkbenchError::Save(
                "El destino cambió durante la decisión de guardado; inténtalo de nuevo".into(),
            ));
        }
        if destination
            .as_ref()
            .is_some_and(|path| path.as_os_str().len() > 4096)
        {
            return Err(WorkbenchError::Save(
                "El destino de guardado supera 4096 unidades nativas".into(),
            ));
        }
        let plan = doc.session.prepare_paged_save(destination)?;
        let wakeup = self.large_open.wakeup.clone();
        let (sender, result) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("cide-guardar-paginado".into())
            .spawn(move || {
                let outcome =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| plan.publish()))
                        .unwrap_or_else(|_| {
                            Err(io::Error::other("Falló el trabajador de guardado").into())
                        });
                let _ = sender.send(outcome);
                wakeup.signal();
            })
            .map_err(FileError::from)?;
        self.paged_save = Some(PendingSave {
            document: target.document,
            result,
        });
        Ok(false)
    }

    /// Commit the disk baseline even if the editor has moved on to a newer revision.
    pub fn poll_save(&mut self) -> Option<Result<DocumentId, WorkbenchError>> {
        let pending = self.paged_save.as_ref()?;
        let outcome = match pending.result.try_recv() {
            Ok(outcome) => outcome,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err(io::Error::other("Se cerró el trabajador de guardado").into())
            }
        };
        let id = pending.document;
        self.paged_save = None;
        Some(outcome.map_err(WorkbenchError::from).and_then(|published| {
            let doc = self
                .documents
                .iter_mut()
                .find(|doc| doc.id == id)
                .ok_or(WorkbenchError::NoDocument)?;
            let previous = doc.session.path().to_owned();
            doc.session.accept_paged_save(published)?;
            if doc.session.path() != previous {
                self.task_panels.clear_lsp(id);
                self.tabs
                    .set_title(doc.tab_id, title_for_path(doc.session.path(), "<archivo>"));
            }
            self.tabs
                .set_dirty(doc.tab_id, doc.session.document().is_modified());
            Ok(id)
        }))
    }
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
        self.require_no_pending_save()?;
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
