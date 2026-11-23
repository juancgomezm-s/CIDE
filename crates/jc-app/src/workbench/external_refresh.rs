//! Bounded refresh after an external writer publishes a file. Dirty buffers stay
//! untouched; a fresh DocumentId invalidates jobs stamped against the old bytes.
use super::{DocumentId, EditorDocument, Workbench, WorkbenchError};
use jc_files::{ContentFingerprint, FileError, FileSession};
use std::{cell::RefCell, path::Path};

impl Workbench {
    pub fn refresh_clean_project_file(
        &mut self,
        root: &Path,
        path: &Path,
        limit: usize,
    ) -> Result<bool, WorkbenchError> {
        let root = root.canonicalize().map_err(FileError::from)?;
        let path = path.canonicalize().map_err(FileError::from)?;
        if !path.starts_with(&root) {
            return Err(WorkbenchError::Save(
                "El archivo está fuera del proyecto".into(),
            ));
        }
        let Some(index) = self.documents.iter().position(|document| {
            document.session.path().canonicalize().ok().as_ref() == Some(&path)
        }) else {
            return Ok(false);
        };
        let previous = &self.documents[index];
        if previous.session.document().is_modified() || previous.session.is_new() {
            return Err(WorkbenchError::Save(
                "Se conservaron tus cambios sin guardar; la comparación muestra la edición en disco de OmniCore".into(),
            ));
        }
        let limit = limit.min(2 * 1024 * 1024);
        if previous.session.document().is_paged() || previous.session.document().len() > limit {
            return Err(WorkbenchError::Save(
                "Se conservó la vista abierta: el archivo supera el límite de actualización automática".into(),
            ));
        }
        let session = FileSession::open_in_root(&root, &path, previous.session.encoding(), limit)?;
        let hash =
            |session: &FileSession| ContentFingerprint::of_chunks(session.document().chunks());
        if hash(&session) == hash(&previous.session) {
            return Ok(false);
        }
        let old = previous.id;
        let tab = previous.tab_id;
        self.next_document += 1;
        self.documents[index] = EditorDocument {
            id: DocumentId(self.next_document),
            tab_id: tab,
            session,
            notified_revision: 0,
            large_viewport: None,
            text: RefCell::default(),
            invisible: RefCell::default(),
            unicode: RefCell::default(),
            syntax: RefCell::default(),
            selection_history: Default::default(),
        };
        self.task_panels.clear_lsp(old);
        self.tabs.set_dirty(tab, false);
        self.normalize_views(tab);
        let _ = self.rebuild_search();
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jc_text::TextEncoding;

    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "cide-external-refresh-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(self.0.join("demo.p"));
            let _ = std::fs::remove_dir(&self.0);
        }
    }

    #[test]
    fn external_refresh_updates_clean_tab_with_new_identity_and_save_baseline() {
        let root = Fixture::new();
        let path = root.path().join("demo.p");
        std::fs::write(&path, b"RETURN a + b.\n").unwrap();
        let mut app = Workbench::default();
        let tab = app.open_file(&path, Some(TextEncoding::Utf8)).unwrap();
        let id = app.active_document().unwrap().id;
        std::fs::write(&path, b"RETURN (a + b) * 2.\n").unwrap();
        assert!(app
            .refresh_clean_project_file(root.path(), &path, 1024)
            .unwrap());
        let current = app.active_document().unwrap();
        assert_eq!(current.tab_id, tab);
        assert_ne!(current.id, id);
        assert_eq!(
            current.session.document().to_bytes(),
            b"RETURN (a + b) * 2.\n"
        );
        assert!(!current.session.document().is_modified());
        assert!(app.document_by_id(id).is_none());
        app.insert_text("/* nuevo */").unwrap();
        app.save_active().unwrap();
        assert!(std::fs::read(&path).unwrap().starts_with(b"/* nuevo */"));
    }

    #[test]
    fn external_refresh_keeps_local_changes_and_their_single_undo() {
        let root = Fixture::new();
        let path = root.path().join("demo.p");
        std::fs::write(&path, b"original").unwrap();
        let mut app = Workbench::default();
        app.open_file(&path, Some(TextEncoding::Utf8)).unwrap();
        app.insert_text("local ").unwrap();
        let id = app.active_document().unwrap().id;
        std::fs::write(&path, b"externo").unwrap();
        assert!(app
            .refresh_clean_project_file(root.path(), &path, 1024)
            .is_err());
        assert_eq!(app.active_document().unwrap().id, id);
        assert_eq!(
            app.active_document().unwrap().session.document().to_bytes(),
            b"local original"
        );
        app.undo().unwrap();
        assert_eq!(
            app.active_document().unwrap().session.document().to_bytes(),
            b"original"
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"externo");
    }

    #[test]
    fn external_refresh_rejects_scope_and_oversize_without_changing_the_tab() {
        let root = Fixture::new();
        let other = Fixture::new();
        let path = root.path().join("demo.p");
        std::fs::write(&path, b"original").unwrap();
        let mut app = Workbench::default();
        app.open_file(&path, Some(TextEncoding::Utf8)).unwrap();
        let id = app.active_document().unwrap().id;
        std::fs::write(&path, vec![b'x'; 2048]).unwrap();
        assert!(app
            .refresh_clean_project_file(other.path(), &path, 1024)
            .is_err());
        assert!(app
            .refresh_clean_project_file(root.path(), &path, 1024)
            .is_err());
        assert_eq!(app.active_document().unwrap().id, id);
        assert_eq!(
            app.active_document().unwrap().session.document().to_bytes(),
            b"original"
        );
    }
}
