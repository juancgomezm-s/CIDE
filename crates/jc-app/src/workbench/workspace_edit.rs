//! Prepare each resident document before publishing any member of a workspace edit.
use super::{prepare_text_batch, DocumentId, TextProjection, Workbench, WorkbenchError};
use crate::{SearchOrigin, TextReplacement};
use jc_core::{Document, PreparedEditBatch};
use jc_files::{ContentFingerprint, FileError, FileSession};
use jc_text::TextEncoding;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

pub(super) const MAX_WORKSPACE_GROUPS: usize = 64;
pub(super) const MAX_WORKSPACE_FILES: usize = 16;
const MAX_FILE_BYTES: usize = 2 * 1024 * 1024;
const MAX_SOURCE_BYTES: usize = 8 * 1024 * 1024;
const MAX_EDITS: usize = 4096;
const MAX_REPLACEMENT_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug)]
pub struct WorkspaceFileEdit {
    pub path: PathBuf,
    pub origin: SearchOrigin,
    pub encoding: TextEncoding,
    pub edits: Vec<TextReplacement>,
}

impl WorkspaceFileEdit {
    pub(crate) const MAX_FILES: usize = MAX_WORKSPACE_FILES;
}

#[derive(Clone, Copy, Debug)]
pub(super) struct WorkspaceMember {
    pub document: Option<DocumentId>,
    pub before: u64,
    pub after: u64,
    pub encoding: TextEncoding,
}

#[derive(Debug)]
pub(super) struct WorkspaceGroup(pub Vec<WorkspaceMember>);

enum Target {
    Open(usize),
    Closed(Box<FileSession>),
}

struct PreparedFile {
    target: Target,
    batch: PreparedEditBatch,
    member: WorkspaceMember,
}

fn error(message: &str) -> WorkbenchError {
    WorkbenchError::WorkspaceEdit(message.to_owned())
}

fn fingerprint(source: &Document) -> ContentFingerprint {
    ContentFingerprint::of_chunks(source.chunks())
}

impl WorkspaceMember {
    pub(super) fn retained(self, app: &Workbench) -> bool {
        self.document
            .and_then(|id| app.document_by_id(id))
            .is_some_and(|document| {
                document
                    .session
                    .document()
                    .history_transitions()
                    .any(|step| {
                        step.before_revision == self.before && step.after_revision == self.after
                    })
            })
    }

    fn is_current(self, source: &Document, undo: bool) -> bool {
        if undo {
            source.revision() == self.after && source.undo_revision() == Some(self.before)
        } else {
            source.revision() == self.before && source.redo_revision() == Some(self.after)
        }
    }
}

impl Workbench {
    /// Apply a reviewed set of byte ranges to the editor. Closed files become
    /// ordinary dirty tabs, and project edits never write original files.
    pub fn apply_workspace_edits(
        &mut self,
        root: &Path,
        plans: Vec<WorkspaceFileEdit>,
    ) -> Result<(), WorkbenchError> {
        let root = root.canonicalize().map_err(FileError::from)?;
        if plans.len() > MAX_WORKSPACE_FILES
            || plans
                .iter()
                .map(|file| file.edits.len())
                .fold(0, usize::saturating_add)
                > MAX_EDITS
            || plans
                .iter()
                .flat_map(|file| &file.edits)
                .map(|edit| edit.text.len())
                .fold(0, usize::saturating_add)
                > MAX_REPLACEMENT_BYTES
        {
            return Err(error(
                "Los cambios de proyecto superan el presupuesto de edición",
            ));
        }
        let mut source_bytes = 0usize;
        let mut files = Vec::with_capacity(plans.len());
        let mut paths = HashSet::new();
        for plan in plans {
            let canonical = plan.path.canonicalize().map_err(FileError::from)?;
            if !canonical.starts_with(&root) || !paths.insert(canonical.clone()) {
                return Err(error(
                    "Hay archivos duplicados o fuera de la raíz del proyecto",
                ));
            }
            let open = self.documents.iter().position(|document| {
                let path = document.session.path();
                path == canonical || path.canonicalize().is_ok_and(|path| path == canonical)
            });
            let allowance = MAX_FILE_BYTES.min(MAX_SOURCE_BYTES.saturating_sub(source_bytes));
            let mut target = match plan.origin {
                SearchOrigin::Open(stamp) => {
                    let index = open
                        .filter(|&index| self.documents[index].id == stamp.document)
                        .filter(|_| {
                            stamp.encoding == plan.encoding && self.task_stamp_is_current(stamp)
                        })
                        .ok_or_else(|| error("Cambió un documento desde la vista previa"))?;
                    Target::Open(index)
                }
                SearchOrigin::Disk(expected) => {
                    let session = FileSession::open_in_root_at_version(
                        &root,
                        &plan.path,
                        Some(plan.encoding),
                        allowance,
                        expected,
                    )?;
                    if session.path() != canonical {
                        return Err(error("Cambió la ruta de un archivo desde la vista previa"));
                    }
                    if let Some(index) = open {
                        let document = &self.documents[index];
                        let source = document.session.document();
                        if source.is_paged()
                            || source.len() > allowance
                            || document.session.encoding() != Some(plan.encoding)
                            || fingerprint(source) != expected
                        {
                            return Err(error("Cambió un documento desde la vista previa"));
                        }
                        Target::Open(index)
                    } else {
                        Target::Closed(Box::new(session))
                    }
                }
            };
            let mut local_projection = TextProjection::default();
            let (session, projection, id) = match &mut target {
                Target::Open(index) => {
                    let document = &mut self.documents[*index];
                    (
                        &mut document.session,
                        document.text.get_mut(),
                        Some(document.id),
                    )
                }
                Target::Closed(session) => (session.as_mut(), &mut local_projection, None),
            };
            let source = session.document();
            if source.is_paged() || source.len() > allowance {
                return Err(error(
                    "Los archivos de proyecto superan el presupuesto de lectura",
                ));
            }
            let content_start = super::session_content_start(session);
            if plan
                .edits
                .iter()
                .any(|edit| edit.range.start < content_start)
            {
                return Err(error(
                    "Un cambio de proyecto invade el marcador de codificación",
                ));
            }
            source_bytes += source.len();
            let batch = prepare_text_batch(session, projection, source.revision(), plan.edits)?;
            let batch = session
                .document_mut()
                .prepare_batch(batch)
                .map_err(FileError::from)?;
            let Some((before, after)) = batch.transition() else {
                continue;
            };
            if !batch.retains_undo() || !session.document().prepared_batch_is_current(&batch) {
                return Err(error(
                    "Un documento no puede conservar el cambio y su historial",
                ));
            }
            files.push(PreparedFile {
                target,
                batch,
                member: WorkspaceMember {
                    document: id,
                    before,
                    after,
                    encoding: plan.encoding,
                },
            });
        }
        if files.is_empty() {
            return Ok(());
        }
        let groups = std::mem::take(&mut self.workspace_history);
        self.workspace_history = groups
            .into_iter()
            .filter(|group| group.0.iter().any(|member| member.retained(self)))
            .collect();
        if self.workspace_history.len() == MAX_WORKSPACE_GROUPS {
            return Err(error(
                "Se alcanzó el límite del historial de cambios de proyecto",
            ));
        }
        let publish = |app: &mut Workbench| {
            let mut members = Vec::with_capacity(files.len());
            for PreparedFile {
                target,
                batch,
                mut member,
            } in files
            {
                let id = match target {
                    Target::Open(index) => {
                        let document = &mut app.documents[index];
                        document
                            .session
                            .document_mut()
                            .commit_prepared_batch(batch)
                            .expect("the preflighted owner has not changed its document");
                        document.id
                    }
                    Target::Closed(mut session) => {
                        session
                            .document_mut()
                            .commit_prepared_batch(batch)
                            .expect("the preflighted session has no other owner");
                        app.admit_open_session(*session, None, false, false);
                        DocumentId(app.next_document)
                    }
                };
                member.document = Some(id);
                members.push(member);
            }
            for member in &members {
                app.publish_batch_change(member.document.expect("new document was admitted"));
            }
            app.workspace_history.push(WorkspaceGroup(members));
            let _ = app.rebuild_search();
            Ok(())
        };
        if let Some(id) = self.active_document().map(|document| document.id) {
            self.selection_edit_document(id, publish)
        } else {
            publish(self)
        }
    }

    /// If the top transition belongs to a project edit, move every member or
    /// reject the action. Later ordinary edits can still be undone separately.
    pub(super) fn apply_workspace_history(&mut self, undo: bool) -> Result<bool, WorkbenchError> {
        let Some(active) = self.active_document() else {
            return Ok(false);
        };
        let Some(group) = self.workspace_history.iter().find(|group| {
            group.0.iter().any(|member| {
                member.document == Some(active.id)
                    && member.is_current(active.session.document(), undo)
            })
        }) else {
            return Ok(false);
        };
        let members = group.0.clone();
        let indices: Option<Vec<_>> = members
            .iter()
            .map(|member| {
                self.documents.iter().position(|document| {
                    member.document == Some(document.id)
                        && document.session.encoding() == Some(member.encoding)
                        && member.is_current(document.session.document(), undo)
                })
            })
            .collect();
        let Some(indices) = indices else {
            return Err(error("El historial de proyecto está bloqueado porque cambió o se cerró otro documento del grupo"));
        };
        self.cancel_rectangle();
        self.resume_caret_tracking();
        for (index, member) in indices.into_iter().zip(members) {
            let document = &mut self.documents[index];
            let id = document.id;
            let source = document.session.document_mut();
            let changed = if undo { source.undo() } else { source.redo() };
            debug_assert!(changed);
            self.publish_batch_change(id);
            self.restore_document_history_selection(id, member.before, member.after, undo);
            let document = &mut self.documents[index];
            document
                .selection_history
                .prune(document.session.document());
        }
        let _ = self.rebuild_search();
        Ok(true)
    }
}
