//! Immutable recovery generations, streamed to owned files before atomic HEAD.
//! Source document bytes/undo payloads and metadata are opaque to this layer.
use crate::{fingerprint::FingerprintingWriter, ContentFingerprint, FileError};
mod gc;
pub use gc::PruneReport;
use std::{
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, BufWriter, Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

const HEAD_MAGIC: &[u8; 8] = b"CIDEHD01";
const MANIFEST_MAGIC: &[u8; 8] = b"CIDEGN01";
const HEAD_BYTES: usize = 72;
const MANIFEST_HEADER: usize = 24;
const ENTRY_BYTES: usize = 24;
const MAX_SCAN: usize = 16_384;
const MAX_METADATA: usize = 16 * 1024 * 1024;
const MAX_DOCUMENT: u64 = 8 * 1024 * 1024 * 1024;
const MAX_TOTAL: u64 = 128 * 1024 * 1024 * 1024;
static NEXT: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy)]
pub struct RecoveryLimits {
    pub max_documents: usize,
    pub max_metadata_bytes: usize,
    pub max_document_bytes: u64,
    pub max_total_bytes: u64,
    pub max_slots: usize,
}
impl Default for RecoveryLimits {
    fn default() -> Self {
        Self {
            max_documents: 4096,
            max_metadata_bytes: MAX_METADATA,
            max_document_bytes: MAX_DOCUMENT,
            max_total_bytes: MAX_TOTAL,
            max_slots: 256,
        }
    }
}
impl RecoveryLimits {
    fn valid(self) -> bool {
        (1..=4096).contains(&self.max_documents)
            && (1..=MAX_METADATA).contains(&self.max_metadata_bytes)
            && (1..=MAX_DOCUMENT).contains(&self.max_document_bytes)
            && (1..=MAX_TOTAL).contains(&self.max_total_bytes)
            && (1..=4096).contains(&self.max_slots)
    }
    fn manifest_bytes(self) -> usize {
        MANIFEST_HEADER + ENTRY_BYTES * (self.max_documents + 1)
    }
}

#[derive(Debug)]
pub enum RecoveryError {
    Io(io::Error),
    File(FileError),
    Limit(&'static str),
    Corrupt(&'static str),
    UnsafePath,
    Unsupported,
}
impl fmt::Display for RecoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "No se pudo acceder a la recuperación: {error}"),
            Self::File(error) => write!(f, "No se pudo confirmar la recuperación: {error}"),
            Self::Limit(message) => write!(f, "Límite de recuperación: {message}"),
            Self::Corrupt(message) => write!(f, "Copia de recuperación inválida: {message}"),
            Self::UnsafePath => write!(
                f,
                "La recuperación requiere rutas ordinarias sin enlaces ni puntos de reanálisis"
            ),
            Self::Unsupported => write!(
                f,
                "El bloqueo exclusivo de recuperación no está disponible en esta plataforma"
            ),
        }
    }
}
impl std::error::Error for RecoveryError {}
impl From<io::Error> for RecoveryError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
impl From<FileError> for RecoveryError {
    fn from(error: FileError) -> Self {
        Self::File(error)
    }
}

#[derive(Clone, Copy, Debug)]
struct Head {
    generation: u64,
    epoch: u128,
    manifest: ContentFingerprint,
}
impl Head {
    fn encode(self) -> [u8; HEAD_BYTES] {
        let mut bytes = [0; HEAD_BYTES];
        bytes[..8].copy_from_slice(HEAD_MAGIC);
        bytes[8..16].copy_from_slice(&self.generation.to_le_bytes());
        bytes[16..32].copy_from_slice(&self.epoch.to_le_bytes());
        let (len, first, second) = self.manifest.words();
        bytes[32..40].copy_from_slice(&len.to_le_bytes());
        bytes[40..48].copy_from_slice(&first.to_le_bytes());
        bytes[48..56].copy_from_slice(&second.to_le_bytes());
        let (_, first, second) = ContentFingerprint::of(&bytes[..56]).words();
        bytes[56..64].copy_from_slice(&first.to_le_bytes());
        bytes[64..72].copy_from_slice(&second.to_le_bytes());
        bytes
    }
    fn decode(bytes: &[u8], limits: RecoveryLimits) -> Result<Self, RecoveryError> {
        if bytes.len() != HEAD_BYTES || &bytes[..8] != HEAD_MAGIC {
            return Err(RecoveryError::Corrupt("encabezado o versión de HEAD"));
        }
        let (_, first, second) = ContentFingerprint::of(&bytes[..56]).words();
        if first != word(bytes, 56)? || second != word(bytes, 64)? {
            return Err(RecoveryError::Corrupt("checksum de HEAD"));
        }
        let generation = word(bytes, 8)?;
        let len = word(bytes, 32)?;
        if generation == 0
            || len < (MANIFEST_HEADER + ENTRY_BYTES) as u64
            || len > limits.manifest_bytes() as u64
        {
            return Err(RecoveryError::Corrupt("generación o tamaño de manifiesto"));
        }
        Ok(Self {
            generation,
            epoch: u128::from_le_bytes(bytes[16..32].try_into().unwrap()),
            manifest: ContentFingerprint::from_words(len, word(bytes, 40)?, word(bytes, 48)?),
        })
    }
}

pub struct LoadedRecovery {
    pub generation: u64,
    pub metadata: Vec<u8>,
    /// Verified immutable read handles, at offset zero. Never decoded as UTF-8.
    pub documents: Vec<File>,
    pub used_backup: bool,
}
pub struct RecoveryStore {
    slot: PathBuf,
    limits: RecoveryLimits,
    _lease: File,
    confirmed: Option<Head>,
    retained_bytes: Option<u64>,
}

impl RecoveryStore {
    pub fn open(root: &Path, limits: RecoveryLimits) -> Result<Self, RecoveryError> {
        if !limits.valid() {
            return Err(RecoveryError::Limit("configuración inválida"));
        }
        if !cfg!(windows) {
            return Err(RecoveryError::Unsupported);
        }
        ensure_directory(root, true)?;
        let root = root.canonicalize()?;
        ensure_directory(&root, false)?;
        let mut best: Option<(PathBuf, File, Option<Head>, u128)> = None;
        let mut slots = 0;
        for (examined, entry) in fs::read_dir(&root)?.enumerate() {
            if examined >= MAX_SCAN {
                return Err(RecoveryError::Limit(
                    "demasiadas entradas en la carpeta de recuperación",
                ));
            }
            let entry = entry?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if !slot_name(&name) {
                continue;
            }
            slots += 1;
            if slots > limits.max_slots {
                return Err(RecoveryError::Limit("demasiadas sesiones de recuperación"));
            }
            let path = entry.path();
            ensure_directory(&path, false)?;
            let Some(lease) = lease(&path)? else {
                continue;
            };
            let head = read_head(&path, "HEAD", limits).ok().flatten();
            let backup = read_head(&path, "BACKUP", limits).ok().flatten();
            let candidate = head.or(backup);
            let has_record = fs::symlink_metadata(path.join("HEAD")).is_ok()
                || fs::symlink_metadata(path.join("BACKUP")).is_ok();
            if candidate.is_none() && !has_record {
                continue;
            }
            let epoch = candidate.map_or(0, |head| head.epoch);
            if best
                .as_ref()
                .is_none_or(|(_, _, _, previous)| epoch > *previous)
            {
                best = Some((path, lease, candidate, epoch));
            }
        }
        if let Some((slot, lease, _, _)) = best {
            return Ok(Self {
                slot,
                limits,
                _lease: lease,
                confirmed: None,
                retained_bytes: None,
            });
        }
        if slots >= limits.max_slots {
            return Err(RecoveryError::Limit(
                "todas las sesiones de recuperación están ocupadas",
            ));
        }
        for _ in 0..100 {
            let name = format!(
                "slot-{}-{:032x}-{}",
                std::process::id(),
                epoch(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            );
            let slot = root.join(name);
            match fs::create_dir(&slot) {
                Ok(()) => {
                    let lease = lease(&slot)?
                        .ok_or(RecoveryError::Corrupt("la nueva sesión ya está ocupada"))?;
                    return Ok(Self {
                        slot,
                        limits,
                        _lease: lease,
                        confirmed: None,
                        retained_bytes: Some(0),
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Err(RecoveryError::Limit(
            "no se pudo reservar una sesión exclusiva",
        ))
    }
    pub fn slot_path(&self) -> &Path {
        &self.slot
    }
    pub fn checkpoint(
        &mut self,
        metadata: &[u8],
        documents: usize,
        mut write_doc: impl FnMut(usize, &mut dyn Write) -> io::Result<()>,
    ) -> Result<u64, RecoveryError> {
        let result = self.write_checkpoint(metadata, documents, &mut write_doc);
        if result.is_err() {
            self.retained_bytes = None;
        }
        result
    }
    fn write_checkpoint(
        &mut self,
        metadata: &[u8],
        documents: usize,
        write_doc: &mut impl FnMut(usize, &mut dyn Write) -> io::Result<()>,
    ) -> Result<u64, RecoveryError> {
        if documents > self.limits.max_documents || metadata.len() > self.limits.max_metadata_bytes
        {
            return Err(RecoveryError::Limit(
                "documentos o metadatos exceden su cuota",
            ));
        }
        ensure_directory(&self.slot, false)?;
        // Revalidate the previous commit on every publication. A cached HEAD
        // must never overwrite a valid BACKUP after its payload became corrupt.
        // Keep the prior document handles pinned through the new commit.
        let _previous = self.load_latest()?;
        let existing = match self.retained_bytes {
            Some(bytes) => bytes,
            None => disk_charge(&self.slot, self.limits.max_total_bytes)?,
        };
        // A callback may also unwind. A later retry must recount any unpublished
        // bytes from that generation instead of trusting the old cached charge.
        self.retained_bytes = None;
        let remaining = self.limits.max_total_bytes.saturating_sub(existing);
        if metadata.len() as u64 > remaining {
            return Err(RecoveryError::Limit(
                "se agotó la cuota de copias retenidas",
            ));
        }
        let mut generation = self
            .confirmed
            .map_or(1, |head| head.generation.saturating_add(1))
            .max(u64::try_from(epoch()).unwrap_or(u64::MAX));
        let mut reserved = None;
        for _ in 0..100 {
            let path = generation_path(&self.slot, generation);
            match fs::create_dir(&path) {
                Ok(()) => {
                    reserved = Some(path);
                    break;
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    generation = generation
                        .checked_add(1)
                        .ok_or(RecoveryError::Limit("se agotó la generación"))?;
                }
                Err(error) => return Err(error.into()),
            }
        }
        let directory =
            reserved.ok_or(RecoveryError::Limit("no se pudo reservar una generación"))?;
        let mut unpublished = self.unpublished_guard(generation)?;
        let mut fingerprints = Vec::with_capacity(documents + 1);
        let mut total = metadata.len() as u64;
        write_file(&directory.join("metadata.bin"), |file| {
            file.write_all(metadata)
        })?;
        fingerprints.push(ContentFingerprint::of(metadata));
        for index in 0..documents {
            let mut file = new_file(&directory.join(document_name(index)))?;
            let fingerprint = {
                let limited = LimitedWriter {
                    file: BufWriter::with_capacity(64 * 1024, &mut file),
                    written: 0,
                    limit: self
                        .limits
                        .max_document_bytes
                        .min(remaining.saturating_sub(total)),
                    failed: false,
                };
                let mut writer = FingerprintingWriter::new(limited);
                write_doc(index, &mut writer)?;
                writer.flush()?;
                writer.fingerprint()
            };
            file.sync_all()?;
            total = total
                .checked_add(fingerprint.words().0)
                .ok_or(RecoveryError::Limit(
                    "desbordamiento del tamaño de generación",
                ))?;
            fingerprints.push(fingerprint);
        }
        let mut manifest = Vec::with_capacity(MANIFEST_HEADER + fingerprints.len() * ENTRY_BYTES);
        manifest.extend_from_slice(MANIFEST_MAGIC);
        manifest.extend_from_slice(&(documents as u32).to_le_bytes());
        manifest.extend_from_slice(&0u32.to_le_bytes());
        manifest.extend_from_slice(&total.to_le_bytes());
        for fingerprint in fingerprints {
            let (len, first, second) = fingerprint.words();
            manifest.extend_from_slice(&len.to_le_bytes());
            manifest.extend_from_slice(&first.to_le_bytes());
            manifest.extend_from_slice(&second.to_le_bytes());
        }
        if total
            .checked_add(manifest.len() as u64)
            .is_none_or(|total| total > remaining)
        {
            return Err(RecoveryError::Limit("la generación excede la cuota total"));
        }
        write_file(&directory.join("manifest.bin"), |file| {
            file.write_all(&manifest)
        })?;
        let head = Head {
            generation,
            epoch: epoch().max(
                self.confirmed
                    .map_or(0, |previous| previous.epoch.saturating_add(1)),
            ),
            manifest: ContentFingerprint::of(&manifest),
        };
        // Validate the persisted bytes through read-only handles before publishing
        // any pointer to them. A failed readback leaves the prior HEAD untouched.
        drop(load_generation(&self.slot, head, self.limits, false)?);
        if let Some(previous) = self.confirmed {
            publish(&self.slot, "BACKUP", &previous.encode())?;
        }
        publish(&self.slot, "HEAD", &head.encode())?;
        unpublished.commit();
        self.confirmed = Some(head);
        self.retained_bytes = Some(existing + total + manifest.len() as u64);
        Ok(generation)
    }
    pub fn load_latest(&mut self) -> Result<Option<LoadedRecovery>, RecoveryError> {
        self.retained_bytes = None;
        ensure_directory(&self.slot, false)?;
        let mut failure = None;
        for (name, used_backup) in [("HEAD", false), ("BACKUP", true)] {
            match read_head(&self.slot, name, self.limits) {
                Ok(Some(head)) => match load_generation(&self.slot, head, self.limits, used_backup)
                {
                    Ok(loaded) => {
                        self.confirmed = Some(head);
                        return Ok(Some(loaded));
                    }
                    Err(error) => {
                        if failure.is_none() {
                            failure = Some(error);
                        }
                    }
                },
                Ok(None) => {}
                Err(error) => {
                    if failure.is_none() {
                        failure = Some(error);
                    }
                }
            }
        }
        if let Some(error) = failure {
            return Err(error);
        }
        self.confirmed = None;
        Ok(None)
    }
    /// Retry a structurally invalid application payload from the previous commit.
    pub fn load_backup(&mut self) -> Result<Option<LoadedRecovery>, RecoveryError> {
        self.retained_bytes = None;
        ensure_directory(&self.slot, false)?;
        let Some(head) = read_head(&self.slot, "BACKUP", self.limits)? else {
            return Ok(None);
        };
        let loaded = load_generation(&self.slot, head, self.limits, true)?;
        self.confirmed = Some(head);
        Ok(Some(loaded))
    }
}

struct LimitedWriter<'a> {
    file: BufWriter<&'a mut File>,
    written: u64,
    limit: u64,
    failed: bool,
}
impl Write for LimitedWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() as u64 > self.limit.saturating_sub(self.written) {
            self.failed = true;
            return Err(io::Error::other(
                "La copia del documento supera la cuota de recuperación",
            ));
        }
        let count = match self.file.write(bytes) {
            Ok(count) => count,
            Err(error) => {
                self.failed |= error.kind() != io::ErrorKind::Interrupted;
                return Err(error);
            }
        };
        self.written += count as u64;
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        if self.failed {
            return Err(io::Error::other(
                "La copia del documento tuvo una escritura incompleta",
            ));
        }
        match self.file.flush() {
            Ok(()) => Ok(()),
            Err(error) => {
                self.failed = true;
                Err(error)
            }
        }
    }
}
fn epoch() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}
fn document_name(index: usize) -> String {
    format!("doc-{index:04}.bin")
}
fn generation_path(slot: &Path, generation: u64) -> PathBuf {
    slot.join(format!("generation-{generation:020}"))
}
fn slot_name(name: &str) -> bool {
    name.starts_with("slot-")
        && name.len() <= 100
        && name[5..]
            .chars()
            .all(|ch| ch.is_ascii_hexdigit() || ch == '-')
}
fn generation_name(name: &str) -> bool {
    name.len() == 31
        && name.starts_with("generation-")
        && name[11..].bytes().all(|byte| byte.is_ascii_digit())
}
fn word(bytes: &[u8], offset: usize) -> Result<u64, RecoveryError> {
    let value: [u8; 8] = bytes
        .get(offset..offset.saturating_add(8))
        .ok_or(RecoveryError::Corrupt("registro truncado"))?
        .try_into()
        .map_err(|_| RecoveryError::Corrupt("registro truncado"))?;
    Ok(u64::from_le_bytes(value))
}
fn fingerprint(bytes: &[u8], offset: usize) -> Result<ContentFingerprint, RecoveryError> {
    Ok(ContentFingerprint::from_words(
        word(bytes, offset)?,
        word(bytes, offset + 8)?,
        word(bytes, offset + 16)?,
    ))
}
fn reparse(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}
fn ensure_directory(path: &Path, create: bool) -> Result<(), RecoveryError> {
    if !path.is_absolute()
        || path.as_os_str().len() > 4096
        || path.components().count() > 128
        || path
            .components()
            .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
        || raw_dot_component(path)
    {
        return Err(RecoveryError::UnsafePath);
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        if matches!(component, Component::CurDir | Component::ParentDir) {
            return Err(RecoveryError::UnsafePath);
        }
        current.push(component.as_os_str());
        if matches!(component, Component::Prefix(_)) {
            continue;
        }
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if create && error.kind() == io::ErrorKind::NotFound => {
                match fs::create_dir(&current) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error.into()),
                }
                fs::symlink_metadata(&current)?
            }
            Err(error) => return Err(error.into()),
        };
        if !metadata.is_dir() || reparse(&metadata) {
            return Err(RecoveryError::UnsafePath);
        }
    }
    Ok(())
}
fn raw_dot_component(path: &Path) -> bool {
    // Verbatim Win32 Paths treat `/` inside a component differently from the
    // ordinary Win32 filesystem calls. Inspect both native separators before
    // any canonicalization/creation, including \?\ spellings.
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let mut length = 0usize;
        let mut dots = true;
        for unit in path.as_os_str().encode_wide().chain(Some(u16::from(b'\\'))) {
            if unit == u16::from(b'\\') || unit == u16::from(b'/') {
                if dots && matches!(length, 1 | 2) {
                    return true;
                }
                length = 0;
                dots = true;
            } else {
                length += 1;
                dots &= unit == u16::from(b'.');
            }
        }
        false
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        false
    }
}
fn new_file(path: &Path) -> Result<File, RecoveryError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0).custom_flags(0x0020_0000);
    }
    Ok(options.open(path)?)
}
fn write_file(
    path: &Path,
    write: impl FnOnce(&mut File) -> io::Result<()>,
) -> Result<(), RecoveryError> {
    let mut file = new_file(path)?;
    write(&mut file)?;
    file.flush()?;
    file.sync_all()?;
    Ok(())
}
fn read_file(path: &Path, root: &Path) -> Result<File, RecoveryError> {
    let before = fs::symlink_metadata(path)?;
    if !before.is_file() || reparse(&before) {
        return Err(RecoveryError::UnsafePath);
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1).custom_flags(0x0020_0000);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || reparse(&metadata) {
        return Err(RecoveryError::UnsafePath);
    }
    crate::verify_file_in_root(root, &file)?;
    Ok(file)
}
fn read_small(path: &Path, root: &Path, max: usize) -> Result<Vec<u8>, RecoveryError> {
    let mut file = read_file(path, root)?;
    let len = usize::try_from(file.metadata()?.len())
        .map_err(|_| RecoveryError::Corrupt("tamaño no representable"))?;
    if len > max {
        return Err(RecoveryError::Limit("archivo de control demasiado grande"));
    }
    let mut bytes = vec![0; len];
    file.read_exact(&mut bytes)?;
    let mut guard = [0];
    if file.read(&mut guard)? != 0 {
        return Err(RecoveryError::Corrupt(
            "el archivo de control cambió de tamaño",
        ));
    }
    Ok(bytes)
}
fn read_head(
    slot: &Path,
    name: &str,
    limits: RecoveryLimits,
) -> Result<Option<Head>, RecoveryError> {
    match read_small(&slot.join(name), slot, HEAD_BYTES) {
        Ok(bytes) => Head::decode(&bytes, limits).map(Some),
        Err(RecoveryError::Io(error)) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}
fn load_generation(
    slot: &Path,
    head: Head,
    limits: RecoveryLimits,
    used_backup: bool,
) -> Result<LoadedRecovery, RecoveryError> {
    let directory = generation_path(slot, head.generation);
    ensure_directory(&directory, false)?;
    let manifest = read_small(
        &directory.join("manifest.bin"),
        slot,
        limits.manifest_bytes(),
    )?;
    if ContentFingerprint::of(&manifest) != head.manifest
        || manifest.len() < MANIFEST_HEADER
        || &manifest[..8] != MANIFEST_MAGIC
    {
        return Err(RecoveryError::Corrupt("manifiesto o checksum"));
    }
    let count = u32::from_le_bytes(manifest[8..12].try_into().unwrap()) as usize;
    if count > limits.max_documents
        || manifest[12..16] != [0; 4]
        || manifest.len() != MANIFEST_HEADER + (count + 1) * ENTRY_BYTES
    {
        return Err(RecoveryError::Corrupt(
            "número de documentos o tamaño de manifiesto",
        ));
    }
    let declared = word(&manifest, 16)?;
    if declared > limits.max_total_bytes {
        return Err(RecoveryError::Limit("la generación supera la cuota total"));
    }
    let mut total = 0u64;
    for index in 0..=count {
        let fp = fingerprint(&manifest, MANIFEST_HEADER + index * ENTRY_BYTES)?;
        let max = if index == 0 {
            limits.max_metadata_bytes as u64
        } else {
            limits.max_document_bytes
        };
        if fp.words().0 > max {
            return Err(RecoveryError::Limit(
                "un archivo de la generación supera su cuota",
            ));
        }
        total = total
            .checked_add(fp.words().0)
            .ok_or(RecoveryError::Corrupt("desbordamiento del tamaño total"))?;
    }
    if total != declared {
        return Err(RecoveryError::Corrupt("tamaño total inconsistente"));
    }
    let metadata_fp = fingerprint(&manifest, MANIFEST_HEADER)?;
    let metadata = read_small(
        &directory.join("metadata.bin"),
        slot,
        limits.max_metadata_bytes,
    )?;
    if ContentFingerprint::of(&metadata) != metadata_fp {
        return Err(RecoveryError::Corrupt("checksum de metadatos"));
    }
    let mut documents = Vec::with_capacity(count);
    for index in 0..count {
        let expected = fingerprint(&manifest, MANIFEST_HEADER + (index + 1) * ENTRY_BYTES)?;
        let mut file = read_file(&directory.join(document_name(index)), slot)?;
        let before = file.metadata()?;
        if before.len() != expected.words().0 {
            return Err(RecoveryError::Corrupt("tamaño de documento"));
        }
        let actual = ContentFingerprint::of_reader(
            &mut Read::by_ref(&mut file).take(before.len().saturating_add(1)),
        )?;
        let after = file.metadata()?;
        if actual != expected
            || after.len() != before.len()
            || after.modified().ok() != before.modified().ok()
        {
            return Err(RecoveryError::Corrupt("checksum de documento"));
        }
        file.seek(SeekFrom::Start(0))?;
        documents.push(file);
    }
    Ok(LoadedRecovery {
        generation: head.generation,
        metadata,
        documents,
        used_backup,
    })
}
fn publish(slot: &Path, name: &str, bytes: &[u8]) -> Result<(), RecoveryError> {
    let path = slot.join(name);
    if let Ok(metadata) = fs::symlink_metadata(&path) {
        if !metadata.is_file() || reparse(&metadata) {
            return Err(RecoveryError::UnsafePath);
        }
    }
    crate::atomic_replace_with_commit(
        &path,
        false,
        |file| file.write_all(bytes),
        |temporary, target| crate::replace_expected(temporary, target, None),
    )?;
    Ok(())
}
fn disk_charge(slot: &Path, max: u64) -> Result<u64, RecoveryError> {
    let mut total = 0u64;
    let mut files = 0usize;
    for (examined, entry) in fs::read_dir(slot)?.enumerate() {
        if examined >= MAX_SCAN {
            return Err(RecoveryError::Limit("demasiadas generaciones retenidas"));
        }
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !generation_name(&name) {
            continue;
        }
        let path = entry.path();
        ensure_directory(&path, false)?;
        for entry in fs::read_dir(path)? {
            files += 1;
            if files > 1_000_000 {
                return Err(RecoveryError::Limit(
                    "demasiados archivos de recuperación retenidos",
                ));
            }
            let entry = entry?;
            let metadata = fs::symlink_metadata(entry.path())?;
            if !metadata.is_file() || reparse(&metadata) {
                return Err(RecoveryError::UnsafePath);
            }
            total = total
                .checked_add(metadata.len())
                .ok_or(RecoveryError::Limit("desbordamiento de cuota en disco"))?;
            if total > max {
                return Err(RecoveryError::Limit(
                    "se agotó la cuota de copias retenidas",
                ));
            }
        }
    }
    Ok(total)
}
fn lease(slot: &Path) -> Result<Option<File>, RecoveryError> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let path = slot.join("lease.lock");
        if let Ok(metadata) = fs::symlink_metadata(&path) {
            if !metadata.is_file() || reparse(&metadata) {
                return Err(RecoveryError::UnsafePath);
            }
        }
        match OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .share_mode(0)
            .custom_flags(0x0020_0000)
            .open(path)
        {
            Ok(file) => {
                if !file.metadata()?.is_file() || reparse(&file.metadata()?) {
                    return Err(RecoveryError::UnsafePath);
                }
                Ok(Some(file))
            }
            Err(error) if matches!(error.raw_os_error(), Some(32 | 33)) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = slot;
        Err(RecoveryError::Unsupported)
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn recovery_manifest_hostile_count_and_lengths_with_valid_checksums_fail_before_allocation() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .unwrap()
            .canonicalize()
            .unwrap();
        let mut base = workspace.clone();
        for name in ["work", "test-temp"] {
            base.push(name);
            if !base.exists() {
                fs::create_dir(&base).unwrap();
            }
            ensure_directory(&base, false).unwrap();
            base = base.canonicalize().unwrap();
            assert!(base.starts_with(&workspace));
        }
        let root = base.join(format!(
            "recovery-hostile-{}-{}-{}",
            std::process::id(),
            epoch(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let mut store = RecoveryStore::open(&root, Default::default()).unwrap();
        let generation = store.checkpoint(b"", 0, |_, _| unreachable!()).unwrap();
        let manifest_path = generation_path(store.slot_path(), generation).join("manifest.bin");
        let original = fs::read(&manifest_path).unwrap();
        let original_head = read_head(store.slot_path(), "HEAD", store.limits)
            .unwrap()
            .unwrap();
        for kind in 0..3 {
            let mut bytes = original.clone();
            match kind {
                0 => bytes[8..12].copy_from_slice(&u32::MAX.to_le_bytes()),
                1 => bytes[16..24].copy_from_slice(&u64::MAX.to_le_bytes()),
                _ => bytes[24..32].copy_from_slice(&u64::MAX.to_le_bytes()),
            }
            fs::write(&manifest_path, &bytes).unwrap();
            let head = Head {
                manifest: ContentFingerprint::of(&bytes),
                ..original_head
            };
            publish(store.slot_path(), "HEAD", &head.encode()).unwrap();
            assert!(
                store.load_latest().is_err(),
                "hostile manifest kind{kind} must not be exposed"
            );
        }
        drop(store);
        assert_eq!(root.parent(), Some(base.as_path()));
        assert_eq!(root.canonicalize().unwrap(), root);
        ensure_directory(&root, false).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
}
