//! A recovery generation owns documents once; views refer to their tab identity.
use super::*;
use jc_files::{
    recovery::{LoadedRecovery, RecoveryStore},
    SessionRecoveryInfo, SessionRecoveryLimits,
};
use std::io::{self, Cursor, Read, Write};

const MAGIC: &[u8; 8] = b"CIDEAPP1";
const MAX_DOCUMENTS: usize = 4096;
const MAX_METADATA: usize = 16 * 1024 * 1024;
const MAX_ENCODED_DOCUMENTS: u64 = 512 * 1024 * 1024;

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

impl Workbench {
    /// Source paths are made absolute against the captured launch directory.
    /// No original file is read or written. Failure leaves the live model intact.
    pub fn checkpoint_recovery(
        &self,
        store: &mut RecoveryStore,
        base: &Path,
        frontend: &[u8],
    ) -> Result<u64, String> {
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
        let mut total = 0u64;
        for document in &self.documents {
            let source = document.session.document();
            source
                .validate_recovery_limits(jc_core::RecoveryLimits::default())
                .map_err(|e| e.to_string())?;
            total = total
                .checked_add(source.recovery_encoded_len().map_err(|e| e.to_string())?)
                .ok_or_else(|| invalid().to_string())?;
            if total > MAX_ENCODED_DOCUMENTS {
                return Err(invalid().to_string());
            }
        }
        let mut metadata = Metadata(Vec::new());
        let mut build = || -> io::Result<()> {
            metadata.write_all(MAGIC)?;
            number(&mut metadata, tabs.len() as u64)?;
            for tab in tabs {
                let document = self.document_by_tab(tab.id).ok_or_else(invalid)?;
                number(&mut metadata, tab.id.0)?;
                write_string(&mut metadata, &tab.title)?;
                let mut info = document.session.recovery_info();
                if info.path.is_relative() {
                    info.path = base.join(info.path);
                }
                info.write_to(&mut metadata)?;
            }
            metadata.write_all(&[u8::from(self.show_invisibles)])?;
            self.panes.write_recovery(&mut metadata)?;
            number(&mut metadata, frontend.len() as u64)?;
            metadata.write_all(frontend)
        };
        build().map_err(|e| e.to_string())?;
        store
            .checkpoint(&metadata.0, tabs.len(), |index, writer| {
                self.document_by_tab(tabs[index].id)
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
        match Self::decode_recovery(loaded) {
            Ok(value) => Ok(Some(value)),
            Err(error) if !was_backup => {
                let Some(backup) = store.load_backup().map_err(|e| e.to_string())? else {
                    return Err(error.to_string());
                };
                Self::decode_recovery(backup)
                    .map(Some)
                    .map_err(|e| e.to_string())
            }
            Err(error) => Err(error.to_string()),
        }
    }
    fn decode_recovery(loaded: LoadedRecovery) -> io::Result<(Self, Vec<u8>, bool)> {
        if loaded.metadata.len() > MAX_METADATA || loaded.documents.len() > MAX_DOCUMENTS {
            return Err(invalid());
        }
        let mut total = 0u64;
        for file in &loaded.documents {
            total = total
                .checked_add(file.metadata()?.len())
                .ok_or_else(invalid)?;
            if total > MAX_ENCODED_DOCUMENTS {
                return Err(invalid());
            }
        }
        let mut reader = Cursor::new(loaded.metadata.as_slice());
        let mut magic = [0; 8];
        reader.read_exact(&mut magic)?;
        if &magic != MAGIC {
            return Err(invalid());
        }
        let n = count(&mut reader, MAX_DOCUMENTS)?;
        if n != loaded.documents.len() {
            return Err(invalid());
        }
        let mut result = Self::default();
        let mut mapping = Vec::with_capacity(n);
        for mut file in loaded.documents {
            let old = TabId(read_number(&mut reader)?);
            if mapping.iter().any(|(id, _, _, _)| *id == old) {
                return Err(invalid());
            }
            let title = read_string(&mut reader)?;
            let info =
                SessionRecoveryInfo::read_from(&mut reader, SessionRecoveryLimits::default())
                    .map_err(io::Error::other)?;
            if !info.path.is_absolute() {
                return Err(invalid());
            }
            let source =
                jc_core::Document::read_recovery(&mut file, jc_core::RecoveryLimits::default())
                    .map_err(io::Error::other)?;
            if file.read(&mut [0])? != 0 {
                return Err(invalid());
            }
            let len = source.len();
            let revision = source.revision();
            let dirty = source.is_modified();
            let session = FileSession::from_recovery(info, source).map_err(io::Error::other)?;
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
        result.panes = PaneManager::read_recovery(&mut reader, &mapping)?;
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
