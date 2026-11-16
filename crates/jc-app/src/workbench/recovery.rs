//! A recovery generation owns documents once; views refer to their tab identity.
use super::selection_history::{SelectionHistory, SelectionRecord, SelectionState};
use super::*;
use crate::session::{MAX_PAGED_RECOVERY_DOCUMENT, MAX_RECOVERY_PAYLOAD};
use jc_core::PAGED_ADDED_LIMIT;
use jc_files::{
    read_recovery_document_in,
    recovery::{LoadedRecovery, RecoveryStore},
    SessionRecoveryInfo, SessionRecoveryLimits,
};
use std::collections::HashMap;
use std::io::{self, Cursor, Read, Write};
mod capture;
pub use capture::{CaptureChunk, CaptureError, RecoveryCapture, RecoveryKey};

const MAGIC: &[u8; 8] = b"CIDEAPP2";
const PAGED_MAGIC: &[u8; 8] = b"CIDEAPP3";
const LEGACY_MAGIC: &[u8; 8] = b"CIDEAPP1";
const MAX_SELECTION_RECORDS: usize = 1024;
const MAX_SELECTION_BYTES: usize = 256 * 1024;
const SELECTION_RECORD_BYTES: usize = 144;
const MAX_DOCUMENTS: usize = 4096;
const MAX_METADATA: usize = 16 * 1024 * 1024;
const MAX_RESIDENT_PAYLOAD: u64 = 512 * 1024 * 1024;

fn recovery_limits(paged: bool) -> jc_core::RecoveryLimits {
    let limits = jc_core::RecoveryLimits::default();
    if paged {
        jc_core::RecoveryLimits {
            max_source_bytes: usize::try_from(7 * 1024u64.pow(3) + PAGED_ADDED_LIMIT as u64)
                .unwrap_or(usize::MAX),
            max_encoded_bytes: MAX_PAGED_RECOVERY_DOCUMENT,
            ..limits
        }
    } else {
        limits
    }
}

#[derive(Default)]
struct RecoveryPayload {
    total: u64,
    resident: u64,
}

impl RecoveryPayload {
    fn add(&mut self, bytes: u64, paged: bool) -> io::Result<()> {
        self.total = self.total.checked_add(bytes).ok_or_else(invalid)?;
        if !paged {
            self.resident = self.resident.checked_add(bytes).ok_or_else(invalid)?;
        }
        if bytes > recovery_limits(paged).max_encoded_bytes
            || self.total > MAX_RECOVERY_PAYLOAD
            || self.resident > MAX_RESIDENT_PAYLOAD
        {
            return Err(invalid());
        }
        Ok(())
    }
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "La sesión de recuperación es inválida o excede sus límites",
    )
}

fn number(writer: &mut dyn Write, value: u64) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}
fn read_number(reader: &mut dyn Read) -> io::Result<u64> {
    let mut value = [0; 8];
    reader.read_exact(&mut value)?;
    Ok(u64::from_le_bytes(value))
}
fn count(reader: &mut dyn Read, limit: usize) -> io::Result<usize> {
    usize::try_from(read_number(reader)?)
        .ok()
        .filter(|n| *n <= limit)
        .ok_or_else(invalid)
}
fn write_string(writer: &mut dyn Write, value: &str) -> io::Result<()> {
    if value.len() > 4096 {
        return Err(invalid());
    }
    number(writer, value.len() as u64)?;
    writer.write_all(value.as_bytes())
}
fn read_string(reader: &mut dyn Read) -> io::Result<String> {
    let mut bytes = vec![0; count(reader, 4096)?];
    reader.read_exact(&mut bytes)?;
    String::from_utf8(bytes).map_err(|_| invalid())
}
struct Metadata(Vec<u8>);
impl Write for Metadata {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_METADATA.saturating_sub(self.0.len()) {
            return Err(invalid());
        }
        self.0.try_reserve(bytes.len()).map_err(|_| invalid())?;
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn validate_selection_state(state: SelectionState, len: usize, revision: u64) -> io::Result<()> {
    if state.cursor > len || state.anchor.is_some_and(|anchor| anchor > len) {
        return Err(invalid());
    }
    if let Some(rectangle) = state.rectangle {
        if rectangle.revision != revision
            || rectangle.anchor_column.0 > crate::rectangle::MAX_COLUMNS
            || rectangle.head_column.0 > crate::rectangle::MAX_COLUMNS
            || rectangle.scroll_column.0 > crate::rectangle::MAX_COLUMNS
        {
            return Err(invalid());
        }
    }
    Ok(())
}
fn selection_count_bytes(count: usize) -> io::Result<usize> {
    let bytes = count
        .checked_mul(SELECTION_RECORD_BYTES)
        .ok_or_else(invalid)?;
    if count > MAX_SELECTION_RECORDS
        || bytes > MAX_SELECTION_BYTES
        || count
            .checked_mul(std::mem::size_of::<SelectionRecord>())
            .ok_or_else(invalid)?
            > MAX_SELECTION_BYTES
    {
        return Err(invalid());
    }
    Ok(bytes)
}
fn validate_selection_records(
    records: &[SelectionRecord],
    document: &EditorDocument,
) -> io::Result<()> {
    selection_count_bytes(records.len())?;
    let transitions: HashMap<_, _> = document
        .session
        .document()
        .history_transitions()
        .map(|transition| (transition.after_revision, transition))
        .collect();
    let mut previous = None;
    for record in records {
        let transition = transitions
            .get(&record.after_revision)
            .ok_or_else(invalid)?;
        if record.view.0 == 0
            || record.before_revision >= record.after_revision
            || previous.is_some_and(|revision| record.after_revision <= revision)
            || record.before_revision != transition.before_revision
        {
            return Err(invalid());
        }
        validate_selection_state(record.before, transition.before_len, record.before_revision)?;
        validate_selection_state(record.after, transition.after_len, record.after_revision)?;
        previous = Some(record.after_revision);
    }
    Ok(())
}
fn write_selection_state(writer: &mut dyn Write, state: SelectionState) -> io::Result<()> {
    number(writer, state.cursor as u64)?;
    number(writer, state.anchor.unwrap_or(0) as u64)?;
    number(
        writer,
        u64::from(state.anchor.is_some()) | (u64::from(state.rectangle.is_some()) << 1),
    )?;
    let rectangle = state.rectangle;
    number(
        writer,
        rectangle.map_or(0, |rect| rect.anchor_column.0) as u64,
    )?;
    number(
        writer,
        rectangle.map_or(0, |rect| rect.head_column.0) as u64,
    )?;
    number(
        writer,
        rectangle.map_or(0, |rect| rect.scroll_column.0) as u64,
    )?;
    number(writer, rectangle.map_or(0, |rect| rect.revision))
}
fn read_selection_state(reader: &mut dyn Read) -> io::Result<SelectionState> {
    let cursor = count(reader, usize::MAX)?;
    let anchor = count(reader, usize::MAX)?;
    let flags = read_number(reader)?;
    let anchor_column = count(reader, crate::rectangle::MAX_COLUMNS)?;
    let head_column = count(reader, crate::rectangle::MAX_COLUMNS)?;
    let scroll_column = count(reader, crate::rectangle::MAX_COLUMNS)?;
    let revision = read_number(reader)?;
    if flags & !3 != 0
        || flags & 1 == 0 && anchor != 0
        || flags & 2 == 0
            && (anchor_column != 0 || head_column != 0 || scroll_column != 0 || revision != 0)
    {
        return Err(invalid());
    }
    Ok(SelectionState {
        cursor,
        anchor: (flags & 1 != 0).then_some(anchor),
        rectangle: (flags & 2 != 0).then_some(crate::panes::RectangularSelection {
            anchor_column: crate::rectangle::VisualColumn(anchor_column),
            head_column: crate::rectangle::VisualColumn(head_column),
            scroll_column: crate::rectangle::VisualColumn(scroll_column),
            revision,
        }),
    })
}
fn write_selection_record(writer: &mut dyn Write, record: SelectionRecord) -> io::Result<()> {
    number(writer, record.before_revision)?;
    number(writer, record.after_revision)?;
    number(writer, record.view.0)?;
    number(
        writer,
        match record.encoding {
            TextEncoding::Utf8 => 1,
            TextEncoding::Windows1252 => 2,
            TextEncoding::Iso8859_1 => 3,
            TextEncoding::Ibm850 => 4,
        },
    )?;
    write_selection_state(writer, record.before)?;
    write_selection_state(writer, record.after)
}
fn read_selection_record(reader: &mut dyn Read) -> io::Result<SelectionRecord> {
    let before_revision = read_number(reader)?;
    let after_revision = read_number(reader)?;
    let view = crate::ViewId(read_number(reader)?);
    let encoding = match read_number(reader)? {
        1 => TextEncoding::Utf8,
        2 => TextEncoding::Windows1252,
        3 => TextEncoding::Iso8859_1,
        4 => TextEncoding::Ibm850,
        _ => return Err(invalid()),
    };
    Ok(SelectionRecord {
        before_revision,
        after_revision,
        view,
        encoding,
        before: read_selection_state(reader)?,
        after: read_selection_state(reader)?,
    })
}

impl Workbench {
    /// Source paths are made absolute against the captured launch directory.
    /// No original file is read or written. Failure leaves the live model intact.
    fn recovery_metadata(&self, base: &Path, frontend: &[u8]) -> Result<Vec<u8>, String> {
        if !base.is_absolute()
            || self.documents.len() > MAX_DOCUMENTS
            || frontend.len() > 1024 * 1024
        {
            return Err(invalid().to_string());
        }
        let tabs = self.tabs.tabs();
        if tabs.len() != self.documents.len() {
            return Err(invalid().to_string());
        }
        let documents: HashMap<_, _> = self
            .documents
            .iter()
            .map(|document| (document.tab_id, document))
            .collect();
        if documents.len() != self.documents.len() {
            return Err(invalid().to_string());
        }
        let mut payload = RecoveryPayload::default();
        let mut has_paged = false;
        for document in &self.documents {
            let source = document.session.document();
            let paged = source.is_paged();
            has_paged |= paged;
            if paged && source.retained_added_bytes() > PAGED_ADDED_LIMIT {
                return Err("Los cambios de la instantánea superan 64 MiB de recuperación".into());
            }
            source
                .validate_recovery_limits(recovery_limits(paged))
                .map_err(|e| e.to_string())?;
            payload
                .add(
                    source.recovery_encoded_len().map_err(|e| e.to_string())?,
                    paged,
                )
                .map_err(|e| e.to_string())?;
        }
        let mut metadata = Metadata(Vec::new());
        let mut build = || -> io::Result<()> {
            metadata.write_all(if has_paged { PAGED_MAGIC } else { MAGIC })?;
            number(&mut metadata, tabs.len() as u64)?;
            for tab in tabs {
                let document = documents.get(&tab.id).copied().ok_or_else(invalid)?;
                number(&mut metadata, tab.id.0)?;
                write_string(&mut metadata, &tab.title)?;
                if has_paged {
                    metadata.write_all(&[u8::from(document.session.document().is_paged())])?;
                }
                let mut info = document.session.recovery_info();
                if info.path.is_relative() {
                    info.path = base.join(info.path);
                }
                info.write_to(&mut metadata)?;
            }
            metadata.write_all(&[u8::from(self.show_invisibles)])?;
            self.panes.write_recovery(&mut metadata)?;
            let views: HashMap<_, _> = self.panes.recovery_view_tabs().collect();
            for tab in tabs {
                let document = documents.get(&tab.id).copied().ok_or_else(invalid)?;
                selection_count_bytes(document.selection_history.records().len())?;
                if document.selection_history.records().iter().any(|record| {
                    views
                        .get(&record.view)
                        .is_some_and(|owner| *owner != tab.id)
                }) {
                    return Err(invalid());
                }
                let records: Vec<_> = document
                    .selection_history
                    .records()
                    .iter()
                    .filter(|record| views.get(&record.view) == Some(&tab.id))
                    .copied()
                    .collect();
                validate_selection_records(&records, document)?;
                number(&mut metadata, tab.id.0)?;
                number(&mut metadata, records.len() as u64)?;
                for record in records {
                    write_selection_record(&mut metadata, record)?;
                }
            }
            number(&mut metadata, frontend.len() as u64)?;
            metadata.write_all(frontend)
        };
        build().map_err(|e| e.to_string())?;
        Ok(metadata.0)
    }

    pub fn checkpoint_recovery(
        &self,
        store: &mut RecoveryStore,
        base: &Path,
        frontend: &[u8],
    ) -> Result<u64, String> {
        let metadata = self.recovery_metadata(base, frontend)?;
        let tabs = self.tabs.tabs();
        let documents: HashMap<_, _> = self
            .documents
            .iter()
            .map(|document| (document.tab_id, document))
            .collect();
        store
            .checkpoint(&metadata, tabs.len(), |index, writer| {
                documents
                    .get(&tabs[index].id)
                    .ok_or_else(invalid)?
                    .session
                    .document()
                    .write_recovery(writer)
            })
            .map_err(|e| e.to_string())
    }

    /// Decode off to the side. The caller replaces its workbench only on success.
    pub fn restore_recovery(
        store: &mut RecoveryStore,
    ) -> Result<Option<(Self, Vec<u8>, bool)>, String> {
        let Some(loaded) = store.load_latest().map_err(|e| e.to_string())? else {
            return Ok(None);
        };
        let was_backup = loaded.used_backup;
        let depot = store.slot_path().to_owned();
        match Self::decode_recovery(loaded, &depot) {
            Ok(value) => Ok(Some(value)),
            Err(error) if !was_backup => {
                let Some(backup) = store.load_backup().map_err(|e| e.to_string())? else {
                    return Err(error.to_string());
                };
                Self::decode_recovery(backup, &depot)
                    .map(Some)
                    .map_err(|e| e.to_string())
            }
            Err(error) => Err(error.to_string()),
        }
    }
    fn decode_recovery(loaded: LoadedRecovery, depot: &Path) -> io::Result<(Self, Vec<u8>, bool)> {
        if loaded.metadata.len() > MAX_METADATA || loaded.documents.len() > MAX_DOCUMENTS {
            return Err(invalid());
        }
        let mut reader = Cursor::new(loaded.metadata.as_slice());
        let mut magic = [0; 8];
        reader.read_exact(&mut magic)?;
        if &magic != PAGED_MAGIC && &magic != MAGIC && &magic != LEGACY_MAGIC {
            return Err(invalid());
        }
        let n = count(&mut reader, MAX_DOCUMENTS)?;
        if n != loaded.documents.len() {
            return Err(invalid());
        }
        let mut result = Self::default();
        let mut mapping = Vec::with_capacity(n);
        let mut payload = RecoveryPayload::default();
        for mut file in loaded.documents {
            let old = TabId(read_number(&mut reader)?);
            if mapping.iter().any(|(id, _, _, _)| *id == old) {
                return Err(invalid());
            }
            let title = read_string(&mut reader)?;
            let paged = if &magic == PAGED_MAGIC {
                let mut flag = [0];
                reader.read_exact(&mut flag)?;
                match flag[0] {
                    0 => false,
                    1 => true,
                    _ => return Err(invalid()),
                }
            } else {
                false
            };
            payload.add(file.metadata()?.len(), paged)?;
            let info =
                SessionRecoveryInfo::read_from(&mut reader, SessionRecoveryLimits::default())
                    .map_err(io::Error::other)?;
            if !info.path.is_absolute() {
                return Err(invalid());
            }
            let limits = recovery_limits(paged);
            let source = if paged {
                read_recovery_document_in(&mut file, limits, PAGED_ADDED_LIMIT, depot)
            } else {
                jc_core::Document::read_recovery(&mut file, limits)
            }
            .map_err(io::Error::other)?;
            if file.read(&mut [0])? != 0 {
                return Err(invalid());
            }
            let len = source.len();
            let revision = source.revision();
            let dirty = source.is_modified();
            let session = FileSession::from_recovery(info, source).map_err(io::Error::other)?;
            let (session, large_viewport) = if paged {
                let (session, viewport) = PreparedLargeDocument::prepare(session)
                    .map_err(io::Error::other)?
                    .into_parts();
                (session, Some(viewport))
            } else {
                (session, None)
            };
            let tab_id = result.tabs.open(title);
            result.tabs.set_dirty(tab_id, dirty);
            result.next_document += 1;
            result.documents.push(EditorDocument {
                id: DocumentId(result.next_document),
                tab_id,
                session,
                notified_revision: revision,
                text: RefCell::default(),
                invisible: RefCell::default(),
                unicode: RefCell::default(),
                syntax: RefCell::default(),
                selection_history: SelectionHistory::default(),
                large_viewport,
            });
            mapping.push((old, tab_id, len, revision));
        }
        let mut flag = [0];
        reader.read_exact(&mut flag)?;
        result.show_invisibles = match flag[0] {
            0 => false,
            1 => true,
            _ => return Err(invalid()),
        };
        let (panes, view_mapping) = if &magic == LEGACY_MAGIC {
            (
                PaneManager::read_recovery(&mut reader, &mapping)?,
                Vec::new(),
            )
        } else {
            PaneManager::read_recovery_with_views(&mut reader, &mapping)?
        };
        result.panes = panes;
        for document in &mut result.documents {
            let Some(viewport) = &document.large_viewport else {
                continue;
            };
            let active = result.panes.active();
            let other = if active == PaneId::Primary {
                PaneId::Secondary
            } else {
                PaneId::Primary
            };
            let location = [active, other].into_iter().find_map(|pane| {
                let location = result.panes.view(pane)?.location()?;
                (location.tab == document.tab_id).then_some((pane, location.byte_offset))
            });
            if let Some((pane, cursor)) = location {
                let encoding = document.session.encoding();
                if encoding.is_some() && viewport.position(cursor).is_none() {
                    let rebuilt = large_document::LargeViewportRequest::new(
                        document.session.document(),
                        encoding,
                        cursor.saturating_sub(96),
                    )
                    .map_err(io::Error::other)?
                    .prepare()?;
                    document.large_viewport = Some(rebuilt);
                    result.panes.set_scroll(pane, 0, 0);
                }
            }
        }
        if &magic != LEGACY_MAGIC {
            let views: HashMap<_, _> = result.panes.recovery_view_tabs().collect();
            let remap: HashMap<_, _> = view_mapping.into_iter().collect();
            let mut selection_bytes = 0usize;
            for (&(old_tab, tab, _, _), document) in mapping.iter().zip(&mut result.documents) {
                if read_number(&mut reader)? != old_tab.0 {
                    return Err(invalid());
                }
                let count = count(&mut reader, MAX_SELECTION_RECORDS)?;
                let bytes = selection_count_bytes(count)?;
                selection_bytes = selection_bytes.checked_add(bytes).ok_or_else(invalid)?;
                if bytes > MAX_SELECTION_BYTES
                    || selection_bytes > MAX_METADATA
                    || bytes
                        > loaded
                            .metadata
                            .len()
                            .saturating_sub(reader.position() as usize)
                {
                    return Err(invalid());
                }
                let mut records = Vec::new();
                records.try_reserve_exact(count).map_err(|_| invalid())?;
                for _ in 0..count {
                    let mut record = read_selection_record(&mut reader)?;
                    record.view = *remap.get(&record.view).ok_or_else(invalid)?;
                    if views.get(&record.view) != Some(&tab) {
                        return Err(invalid());
                    }
                    records.push(record);
                }
                if document.tab_id != tab {
                    return Err(invalid());
                }
                validate_selection_records(&records, document)?;
                document.selection_history = SelectionHistory::from_records(records)?;
            }
        }
        if let Some(tab) = result
            .panes
            .view(result.panes.active())
            .and_then(|p| p.location())
            .map(|l| l.tab)
        {
            result.tabs.select(tab);
        }
        let mut frontend = vec![0; count(&mut reader, 1024 * 1024)?];
        reader.read_exact(&mut frontend)?;
        if reader.position() != loaded.metadata.len() as u64 {
            return Err(invalid());
        }
        Ok((result, frontend, loaded.used_backup))
    }
}

#[cfg(all(test, windows))]
mod selection_recovery_tests {
    use super::*;
    use std::{
        fs,
        io::{Seek, SeekFrom},
        sync::atomic::{AtomicU64, Ordering},
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Fixture {
        root: PathBuf,
        files: Vec<PathBuf>,
    }
    impl Fixture {
        fn new() -> Self {
            let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .nth(2)
                .unwrap()
                .canonicalize()
                .unwrap();
            let base = workspace.join("work/test-temp").canonicalize().unwrap();
            assert!(base.starts_with(&workspace) && !base.is_symlink());
            let root = base.join(format!(
                "selection-recovery-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            Self {
                root,
                files: vec![],
            }
        }
        fn load(&mut self, bench: &Workbench, metadata: Vec<u8>) -> LoadedRecovery {
            let mut documents = vec![];
            for document in &bench.documents {
                let path = self.root.join(format!("document-{}", self.files.len()));
                let mut file = fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create_new(true)
                    .open(&path)
                    .unwrap();
                self.files.push(path);
                document
                    .session
                    .document()
                    .write_recovery(&mut file)
                    .unwrap();
                file.seek(SeekFrom::Start(0)).unwrap();
                documents.push(file);
            }
            LoadedRecovery {
                generation: 1,
                metadata,
                documents,
                used_backup: false,
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            for path in &self.files {
                if path.parent() == Some(self.root.as_path()) && !path.is_symlink() {
                    let _ = fs::remove_file(path);
                }
            }
            let _ = fs::remove_dir(&self.root);
        }
    }
    fn bench() -> Workbench {
        let mut bench = Workbench::default();
        bench.new_file("recuperado.txt", TextEncoding::Utf8);
        bench.insert_text("abc").unwrap();
        bench.set_linear_selection_bytes(0, 3).unwrap();
        bench.insert_text("x").unwrap();
        let view = bench
            .panes
            .view(bench.panes.active())
            .unwrap()
            .view_id()
            .unwrap();
        let document = &mut bench.documents[0];
        let transition = document
            .session
            .document()
            .history_transitions()
            .last()
            .unwrap();
        document.selection_history = SelectionHistory::from_records(vec![SelectionRecord {
            before_revision: transition.before_revision,
            after_revision: transition.after_revision,
            view,
            encoding: TextEncoding::Utf8,
            before: SelectionState {
                cursor: 3,
                anchor: Some(0),
                rectangle: None,
            },
            after: SelectionState {
                cursor: 1,
                anchor: None,
                rectangle: None,
            },
        }])
        .unwrap();
        bench
    }
    fn table_start(metadata: &[u8], records: usize) -> usize {
        // Fixture owns one document and three opaque frontend bytes.
        metadata.len() - 3 - 8 - 16 - SELECTION_RECORD_BYTES * records
    }
    #[test]
    fn selection_recovery_app2_remaps_views_and_preserves_linear_and_rectangular_geometry() {
        let mut fixture = Fixture::new();
        let mut bench = bench();
        let mut record = bench.documents[0].selection_history.records()[0];
        record.before.rectangle = Some(crate::panes::RectangularSelection {
            anchor_column: crate::rectangle::VisualColumn(7),
            head_column: crate::rectangle::VisualColumn(2),
            scroll_column: crate::rectangle::VisualColumn(1),
            revision: record.before_revision,
        });
        bench.documents[0].selection_history =
            SelectionHistory::from_records(vec![record]).unwrap();
        let metadata = bench.recovery_metadata(&fixture.root, b"UI!").unwrap();
        assert_eq!(&metadata[..8], MAGIC);
        let (restored, frontend, _) =
            Workbench::decode_recovery(fixture.load(&bench, metadata), &fixture.root).unwrap();
        assert_eq!(frontend, b"UI!");
        let actual = restored.documents[0].selection_history.records()[0];
        assert_ne!(record.view, actual.view);
        assert_eq!(
            restored.panes.tab_for_view(actual.view),
            Some(restored.documents[0].tab_id)
        );
        assert_eq!(actual.before.cursor, 3);
        assert_eq!(actual.before.anchor, Some(0));
        assert_eq!(actual.before.rectangle.unwrap().anchor_column.0, 7);
        assert_eq!(actual.before.rectangle.unwrap().head_column.0, 2);
        assert_eq!(actual.after.cursor, 1);
        assert!(actual.after.anchor.is_none());
        assert_eq!(restored.active_bytes().unwrap(), b"x");
    }
    #[test]
    fn selection_recovery_app1_legacy_restores_without_selection_history() {
        let mut fixture = Fixture::new();
        let bench = bench();
        let mut metadata = bench.recovery_metadata(&fixture.root, b"UI!").unwrap();
        let start = table_start(&metadata, 1);
        metadata.drain(start..metadata.len() - 11);
        metadata[..8].copy_from_slice(LEGACY_MAGIC);
        let (restored, frontend, _) =
            Workbench::decode_recovery(fixture.load(&bench, metadata), &fixture.root).unwrap();
        assert_eq!(frontend, b"UI!");
        assert!(restored.documents[0].selection_history.records().is_empty());
        assert_eq!(restored.active_bytes().unwrap(), b"x");
    }
    #[test]
    fn recovery_capture_key_fingerprints_selection_records_without_mutating_source_epoch() {
        let fixture = Fixture::new();
        let mut bench = bench();
        let before = bench.begin_recovery_capture(&fixture.root, b"UI!").unwrap();
        let source = bench.documents[0].session.document();
        let stamp = (
            source.recovery_identity(),
            source.recovery_epoch(),
            source.revision(),
        );
        let mut record = bench.documents[0].selection_history.records()[0];
        record.before.cursor = 0;
        record.before.anchor = Some(3);
        bench.documents[0].selection_history =
            SelectionHistory::from_records(vec![record]).unwrap();
        let after = bench.begin_recovery_capture(&fixture.root, b"UI!").unwrap();
        let source = bench.documents[0].session.document();
        assert_eq!(
            stamp,
            (
                source.recovery_identity(),
                source.recovery_epoch(),
                source.revision()
            )
        );
        assert_ne!(before.key(), after.key());
        // A captured generation owns its initial immutable metadata; changing
        // metadata alone must not invalidate an otherwise coherent source cursor.
        let mut before = before;
        while before.next_chunk(&bench).unwrap().is_some() {}
    }
    #[test]
    fn selection_recovery_rejects_counts_unknown_views_revisions_offsets_encoding_and_rectangles() {
        let mut fixture = Fixture::new();
        let bench = bench();
        let original = bench.recovery_metadata(&fixture.root, b"UI!").unwrap();
        let start = table_start(&original, 1);
        for (offset, value) in [
            (start + 8, 1025),           // Count is checked before reserve.
            (start + 16 + 16, u64::MAX), // Unknown originating view.
            (start + 16, u64::MAX),      // Before revision does not match the core transition.
            (start + 16 + 8, u64::MAX),  // Unknown after revision.
            (start + 16 + 24, 0),        // Unknown encoding.
            (start + 16 + 32, 4),        // Cursor exceeds the historical length three.
            (start + 16 + 48, 4),        // Unknown state flags.
            (start + 16 + 56, u64::MAX), // Oversized rectangular column.
        ] {
            let mut metadata = original.clone();
            metadata[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
            assert!(
                Workbench::decode_recovery(fixture.load(&bench, metadata), &fixture.root).is_err(),
                "offset {offset}"
            );
        }
        let mut truncated = original;
        truncated.truncate(start + 16 + SELECTION_RECORD_BYTES - 1);
        assert!(
            Workbench::decode_recovery(fixture.load(&bench, truncated), &fixture.root).is_err()
        );
    }
    #[test]
    fn selection_recovery_omits_closed_origins_and_rejects_a_view_owned_by_another_document() {
        let mut fixture = Fixture::new();
        let mut bench = bench();
        let mut record = bench.documents[0].selection_history.records()[0];
        record.view = crate::ViewId(u64::MAX);
        bench.documents[0].selection_history =
            SelectionHistory::from_records(vec![record]).unwrap();
        let metadata = bench.recovery_metadata(&fixture.root, b"UI!").unwrap();
        let start = table_start(&metadata, 0);
        assert_eq!(&metadata[start + 8..start + 16], &0u64.to_le_bytes());
        bench.new_file("otro.txt", TextEncoding::Utf8);
        record.view = bench
            .panes
            .view(bench.panes.active())
            .unwrap()
            .view_id()
            .unwrap();
        bench.documents[0].selection_history =
            SelectionHistory::from_records(vec![record]).unwrap();
        assert!(bench.recovery_metadata(&fixture.root, b"UI!").is_err());
        let foreign = record.view;
        record.view = bench
            .panes
            .recovery_view_tabs()
            .find(|(_, tab)| *tab == bench.documents[0].tab_id)
            .unwrap()
            .0;
        bench.documents[0].selection_history =
            SelectionHistory::from_records(vec![record]).unwrap();
        let mut metadata = bench.recovery_metadata(&fixture.root, b"UI!").unwrap();
        let tables = metadata.len() - 11 - 2 * 16 - SELECTION_RECORD_BYTES;
        metadata[tables + 32..tables + 40].copy_from_slice(&foreign.0.to_le_bytes());
        assert!(Workbench::decode_recovery(fixture.load(&bench, metadata), &fixture.root).is_err());
    }
}
