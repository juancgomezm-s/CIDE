//! Cooperative codec capture. The worker receives chunks, never live documents.
use super::*;
use jc_core::{source::ImmutableByteSource, RecoveryCursor, RecoveryPart};
use jc_files::{ContentFingerprint, SessionRecoveryInfo};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryKey {
    metadata: ContentFingerprint,
    documents: Vec<(DocumentId, u64, u64)>,
}
pub enum CaptureChunk {
    Data {
        document: usize,
        bytes: Vec<u8>,
    },
    Source {
        document: usize,
        source: std::sync::Arc<dyn ImmutableByteSource>,
    },
    End {
        document: usize,
    },
}
#[derive(Debug)]
pub enum CaptureError {
    Stale,
    Failed(String),
}
impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stale => f.write_str("Cambió el documento durante la captura de recuperación"),
            Self::Failed(message) => f.write_str(message),
        }
    }
}
impl std::error::Error for CaptureError {}
struct CapturedDocument {
    tab: TabId,
    id: DocumentId,
    info: SessionRecoveryInfo,
    cursor: RecoveryCursor,
}
pub struct RecoveryCapture {
    pub(crate) metadata: Vec<u8>,
    pub(crate) key: RecoveryKey,
    documents: Vec<CapturedDocument>,
    next: usize,
}
impl Workbench {
    pub fn begin_recovery_capture(
        &self,
        base: &Path,
        frontend: &[u8],
    ) -> Result<RecoveryCapture, String> {
        let metadata = self.recovery_metadata(base, frontend)?;
        let mut documents = Vec::with_capacity(self.tabs.tabs().len());
        let mut stamps = Vec::with_capacity(documents.capacity());
        for tab in self.tabs.tabs() {
            let doc = self
                .document_by_tab(tab.id)
                .ok_or("El documento de recuperación ya no está abierto")?;
            let source = doc.session.document();
            stamps.push((doc.id, source.recovery_identity(), source.recovery_epoch()));
            documents.push(CapturedDocument {
                tab: tab.id,
                id: doc.id,
                info: doc.session.recovery_info(),
                cursor: RecoveryCursor::new(source).map_err(|e| e.to_string())?,
            });
        }
        let key = RecoveryKey {
            metadata: ContentFingerprint::of(&metadata),
            documents: stamps,
        };
        Ok(RecoveryCapture {
            metadata,
            key,
            documents,
            next: 0,
        })
    }
}
impl RecoveryCapture {
    pub fn key(&self) -> &RecoveryKey {
        &self.key
    }
    pub fn document_count(&self) -> usize {
        self.documents.len()
    }
    /// At most 64 KiB is copied in a step. The writer reads paged sources.
    /// A changed/closed
    /// source aborts the whole unpublished generation instead of mixing versions.
    pub fn next_chunk(&mut self, bench: &Workbench) -> Result<Option<CaptureChunk>, CaptureError> {
        let Some(target) = self.documents.get_mut(self.next) else {
            return Ok(None);
        };
        let doc = bench
            .document_by_tab(target.tab)
            .filter(|d| d.id == target.id)
            .ok_or(CaptureError::Stale)?;
        if !doc.session.matches_recovery_info(&target.info) {
            return Err(CaptureError::Stale);
        }
        match doc
            .session
            .document()
            .recovery_part(&mut target.cursor, 64 * 1024)
            .map_err(|e| match e {
                jc_core::RecoveryError::StaleCapture => CaptureError::Stale,
                error => CaptureError::Failed(error.to_string()),
            })? {
            Some(RecoveryPart::Bytes(bytes)) => Ok(Some(CaptureChunk::Data {
                document: self.next,
                bytes,
            })),
            Some(RecoveryPart::Source(source)) => Ok(Some(CaptureChunk::Source {
                document: self.next,
                source,
            })),
            None => {
                let document = self.next;
                self.next += 1;
                Ok(Some(CaptureChunk::End { document }))
            }
        }
    }
}
