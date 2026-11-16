//! Reviewed edits and restores for explicitly enrolled local copies.
use super::{decode_cell, encoding, io_error, local_database, BrowseTable, ColumnKind};
use crate::editor::{avm_path, bounded_read, decode, load_profile, wait, JobDir};
use jc_app::{Cancellation, Wakeup};
use jc_files::{publish_new_bytes, read_in_root, ContentFingerprint};
use jc_text::encode;
use std::{
    collections::BTreeMap,
    fmt::Write,
    fs, io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub const RECORD_EDIT_MAX_VALUE_BYTES: usize = 256;
const EDIT_PROFILE: &str = ".cide/edicion-registros.local.ini";
const PROTOCOL: &str = "JCIDE-RECORD/1";
const MAX_PROTOCOL: usize = 64 * 1024;
const HELPER: &[u8] = include_bytes!("../../../../../../progress/abl/jc-record-edit.p");
mod structure;

#[derive(Default)]
pub struct RecordController {
    pending: Option<Pending>,
}

struct Pending {
    root: PathBuf,
    writable: bool,
    cancel: Cancellation,
    result: mpsc::Receiver<Result<RecordEvent, String>>,
}

pub enum RecordEvent {
    Preview(RecordPreview),
    History(RecordHistory),
    Conflict(RecordConflict),
    Applied,
}

pub struct RecordHistory {
    selection: Selection,
    database_path: PathBuf,
    entries: Vec<RecordHistoryEntry>,
}

pub struct RecordHistoryEntry {
    id: String,
    label: String,
    receipt: Option<ContentFingerprint>,
}

impl RecordHistory {
    pub fn entries(&self) -> &[RecordHistoryEntry] {
        &self.entries
    }
}

impl RecordHistoryEntry {
    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn confirmed(&self) -> bool {
        self.receipt.is_some()
    }
}

pub struct RecordConflict {
    pub column: String,
    pub expected: Option<String>,
    pub actual: Option<String>,
}

pub struct RecordPreview {
    pub column: String,
    pub before: Option<String>,
    pub after: Option<String>,
    pub restoring: bool,
    pub structural_title: Option<&'static str>,
    operation: RecordAction,
}

enum RecordAction {
    Edit(Box<RecordOperation>),
    Structure(Box<structure::Operation>),
}

struct RecordOperation {
    selection: Selection,
    snapshot: Snapshot,
    next: String,
    database_path: PathBuf,
    restore: Option<RestoreSource>,
}

struct RestoreSource {
    id: String,
    receipt: ContentFingerprint,
}

#[derive(Clone)]
struct Selection {
    table: BrowseTable,
    key: i32,
    column: usize,
}

#[derive(PartialEq, Eq)]
struct Snapshot {
    row_id: String,
    image: String,
    value: String,
}

impl BrowseTable {
    pub fn edit_key_column(&self) -> Option<usize> {
        let key = &self.target.key.as_ref()?.columns;
        let [(key, _)] = key.as_slice() else {
            return None;
        };
        self.target.columns.iter().position(|(column, kind)| {
            column.eq_ignore_ascii_case(key) && matches!(kind, ColumnKind::Integer)
        })
    }

    pub fn can_edit_column(&self, index: usize) -> bool {
        self.edit_key_column().is_some_and(|key| key != index)
            && self.target.columns.get(index).is_some_and(|(_, kind)| {
                matches!(kind, ColumnKind::Character | ColumnKind::Integer)
            })
    }
}

impl Selection {
    fn new(table: &BrowseTable, row: &[Option<String>], column: usize) -> Result<Self, String> {
        let index = table
            .edit_key_column()
            .ok_or("Se requiere una clave primaria entera de una columna")?;
        let key: i32 = row
            .get(index)
            .and_then(Option::as_deref)
            .and_then(|key| key.parse().ok())
            .ok_or("Selecciona un registro con clave válida")?;
        Ok(Self {
            table: table.clone(),
            key,
            column,
        })
    }

    fn check_catalog(&self, root: &Path) -> Result<(), String> {
        let catalog = &self.table.catalog;
        if catalog.root != root || !catalog.live.load(Ordering::Acquire) {
            return Err("Cambió el esquema o el proyecto. Vuelve a abrir los registros".into());
        }
        Ok(())
    }

    fn column_name(&self) -> &str {
        &self.table.target.columns[self.column].0
    }
}

impl RecordController {
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }

    pub fn cancel_preview(&mut self) -> bool {
        if self.pending.as_ref().is_none_or(|pending| pending.writable) {
            return false;
        }
        if let Some(pending) = self.pending.take() {
            pending.cancel.cancel();
        }
        true
    }

    pub fn request_edit(
        &mut self,
        root: &Path,
        table: &BrowseTable,
        row: &[Option<String>],
        column: usize,
        value: String,
        wakeup: Wakeup,
    ) -> Result<(), String> {
        if !table.can_edit_column(column) {
            return Err(
                "Elige una columna de texto o entero que no forme parte de la clave".into(),
            );
        }
        let selection = Selection::new(table, row, column)?;
        selection.check_catalog(root)?;
        let next = value_wire(&selection, &value)?;
        let visible = row.get(column).cloned().ok_or("Fila de edición ausente")?;
        self.start(root, wakeup, false, move |root, cancel| {
            let db = write_database(root, &selection)?;
            let snapshot = run_avm(root, &selection, &db, None, cancel)?;
            if visible != decode_cell(&snapshot.value, &selection.table.catalog.codec)? {
                return Err("Conflicto: cambió el valor seleccionado. Recarga la página".into());
            }
            prepare(selection, db, snapshot, next, None)
        })
    }

    pub fn request_restore(
        &mut self,
        root: &Path,
        table: &BrowseTable,
        row: &[Option<String>],
        wakeup: Wakeup,
    ) -> Result<(), String> {
        let mut selection = Selection::new(table, row, 0)?;
        selection.check_catalog(root)?;
        self.start(root, wakeup, false, move |root, cancel| {
            let db = write_database(root, &selection)?;
            let directory = journal_directory(root, &selection, false)?;
            let (source, backup, recorded_after) = latest_backup(root, &directory, &db)?;
            if backup.restoring {
                return Err("El último cambio de este registro ya fue restaurado".into());
            }
            selection.column = selection.table.target.columns.iter().position(|(column, _)| column == &backup.column)
                .filter(|index| selection.table.can_edit_column(*index)).ok_or("El campo del respaldo ya no es editable")?;
            let snapshot = run_avm(root, &selection, &db, None, cancel)?;
            if snapshot != recorded_after || snapshot.value != backup.next {
                return Err("Conflicto: el registro cambió después del respaldo. La restauración se canceló".into());
            }
            prepare(selection, db, snapshot, backup.before.value, Some(source))
        })
    }

    pub fn request_history(
        &mut self,
        root: &Path,
        table: &BrowseTable,
        row: &[Option<String>],
        wakeup: Wakeup,
    ) -> Result<(), String> {
        let selection = Selection::new(table, row, 0)?;
        selection.check_catalog(root)?;
        self.start(root, wakeup, false, move |root, cancel| {
            let db = write_database(root, &selection)?;
            let directory = journal_directory(root, &selection, false)?;
            let entries = read_history(
                root,
                &directory,
                &db,
                &selection.table.catalog.codec,
                cancel,
                || {
                    let Ok(selection) = structure::choose(&selection.table, selection.key) else {
                        return Observation::Unavailable;
                    };
                    match run_mode(root, &selection, &db, AvmMode::Inspect, cancel) {
                        Ok(Some(row)) => Observation::Row(row),
                        Ok(None) => Observation::Missing,
                        Err(_) => Observation::Unavailable,
                    }
                },
            )?;
            Ok(RecordEvent::History(RecordHistory {
                selection,
                database_path: db,
                entries,
            }))
        })
    }

    pub fn request_history_restore(
        &mut self,
        root: &Path,
        history: &RecordHistory,
        index: usize,
        wakeup: Wakeup,
    ) -> Result<(), String> {
        let entry = history.entries.get(index).ok_or("Elige un respaldo")?;
        let receipt = entry
            .receipt
            .ok_or("La restauración requiere un respaldo con recibo válido")?;
        history.selection.check_catalog(root)?;
        let mut selection = history.selection.clone();
        let database_path = history.database_path.clone();
        let id = entry.id.clone();
        self.start(root, wakeup, false, move |root, cancel| {
            let db = write_database(root, &selection)?;
            if db != database_path {
                return Err("Cambió la base desde la consulta del historial".into());
            }
            let directory = journal_directory(root, &selection, false)?;
            let (backup, recorded_after, current_receipt) =
                confirmed_backup(root, &directory, &db, &id)?;
            if receipt != current_receipt {
                return Err("Conflicto: el recibo cambió desde la consulta del historial".into());
            }
            selection.column = selection
                .table
                .target
                .columns
                .iter()
                .position(|(column, _)| column == &backup.column)
                .filter(|index| selection.table.can_edit_column(*index))
                .ok_or("El campo del respaldo ya no es editable")?;
            let snapshot = run_avm(root, &selection, &db, None, cancel)?;
            prepare_history_restore(
                selection,
                db,
                snapshot,
                RestoreSource { id, receipt },
                backup,
                recorded_after,
            )
        })
    }

    pub fn apply(
        &mut self,
        root: &Path,
        preview: RecordPreview,
        wakeup: Wakeup,
    ) -> Result<(), String> {
        match preview.operation {
            RecordAction::Edit(operation) => {
                operation.selection.check_catalog(root)?;
                self.start(root, wakeup, true, move |root, cancel| {
                    apply(root, &operation, cancel)
                })
            }
            RecordAction::Structure(operation) => {
                operation.selection.check_catalog(root)?;
                self.start(root, wakeup, true, move |root, cancel| {
                    structure::apply(root, &operation, cancel)
                })
            }
        }
    }

    fn start(
        &mut self,
        root: &Path,
        wakeup: Wakeup,
        writable: bool,
        action: impl FnOnce(&Path, &Cancellation) -> Result<RecordEvent, String> + Send + 'static,
    ) -> Result<(), String> {
        if self.busy() {
            return Err("Ya hay una operación de registro en curso".into());
        }
        let root = root.to_owned();
        let worker_root = root.clone();
        let cancel = Cancellation::new(Duration::from_secs(20));
        let worker_cancel = cancel.clone();
        let (send, result) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name("cide-abl-record-edit".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    action(&worker_root, &worker_cancel)
                }))
                .unwrap_or_else(|_| Err("El lector ABL terminó sin confirmar la operación".into()));
                let _ = send.send(result);
                wakeup.signal();
            })
            .map_err(io_error)?;
        self.pending = Some(Pending {
            root,
            writable,
            cancel,
            result,
        });
        Ok(())
    }

    pub fn poll(&mut self, root: &Path) -> Option<Result<RecordEvent, String>> {
        let pending = self.pending.as_ref()?;
        let result = match pending.result.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("La operación ABL terminó sin confirmación".into())
            }
        };
        let pending = self.pending.take().unwrap();
        (pending.root == root).then_some(result)
    }
}

impl Drop for RecordController {
    fn drop(&mut self) {
        self.cancel_preview();
    }
}

fn value_wire(selection: &Selection, value: &str) -> Result<String, String> {
    if value.len() > RECORD_EDIT_MAX_VALUE_BYTES || value.chars().any(char::is_control) {
        return Err("El valor admite hasta 256 bytes de texto en una línea".into());
    }
    let integer;
    let value = if matches!(
        selection.table.target.columns[selection.column].1,
        ColumnKind::Integer
    ) {
        integer = value
            .trim()
            .parse::<i32>()
            .map_err(|_| "Escribe un entero ABL válido")?
            .to_string();
        &integer
    } else {
        value
    };
    let codec = encoding(&selection.table.catalog.codec)?;
    Ok(hex(&encode(value, codec).map_err(|_| {
        "El valor no cabe en la página de códigos ABL"
    })?))
}

fn hex(bytes: &[u8]) -> String {
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(value, "{byte:02X}").unwrap();
    }
    value
}

fn prepare(
    selection: Selection,
    db: PathBuf,
    snapshot: Snapshot,
    next: String,
    restore: Option<RestoreSource>,
) -> Result<RecordEvent, String> {
    if snapshot.value == next {
        return Err("El valor propuesto coincide con el actual".into());
    }
    if [&snapshot.value, &next]
        .iter()
        .any(|value| value.as_bytes().chunks_exact(2).any(|b| b == b"00"))
    {
        return Err("El valor contiene bytes NUL que este editor no puede restaurar".into());
    }
    let codec = &selection.table.catalog.codec;
    Ok(RecordEvent::Preview(RecordPreview {
        column: selection.column_name().into(),
        before: decode_cell(&snapshot.value, codec)?,
        after: decode_cell(&next, codec)?,
        restoring: restore.is_some(),
        structural_title: None,
        operation: RecordAction::Edit(Box::new(RecordOperation {
            selection,
            snapshot,
            next,
            database_path: db,
            restore,
        })),
    }))
}

fn prepare_history_restore(
    selection: Selection,
    db: PathBuf,
    snapshot: Snapshot,
    source: RestoreSource,
    backup: Backup,
    recorded_after: Snapshot,
) -> Result<RecordEvent, String> {
    if snapshot != recorded_after {
        let codec = &selection.table.catalog.codec;
        return Ok(RecordEvent::Conflict(RecordConflict {
            column: backup.column,
            expected: decode_cell(&recorded_after.value, codec)?,
            actual: decode_cell(&snapshot.value, codec)?,
        }));
    }
    prepare(selection, db, snapshot, backup.before.value, Some(source))
}

fn write_database(root: &Path, selection: &Selection) -> Result<PathBuf, String> {
    selection.check_catalog(root)?;
    let config = load_profile(root)?;
    let (_, bytes) = read_in_root(root, &root.join(EDIT_PROFILE), 4096).map_err(|err| {
        format!("Habilita la edición de una copia dentro del proyecto en {EDIT_PROFILE}: {err}")
    })?;
    let profile =
        std::str::from_utf8(&bytes).map_err(|_| "El perfil de edición debe estar en UTF-8")?;
    let mut lines = profile
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'));
    let base = lines
        .next()
        .and_then(|line| line.strip_prefix("base="))
        .ok_or("Falta base en el perfil de edición")?;
    let path = lines
        .next()
        .and_then(|line| line.strip_prefix("archivo="))
        .ok_or("Falta archivo en el perfil de edición")?;
    if lines.next().is_some()
        || base != selection.table.target.database
        || Path::new(path).is_absolute()
    {
        return Err("El perfil de edición no autoriza esta base local".into());
    }
    let db = root.join(path).canonicalize().map_err(io_error)?;
    if !db.starts_with(root) {
        return Err("La copia editable de la base debe estar dentro del proyecto".into());
    }
    let pf = config
        .avm
        .parameter_file
        .ok_or("La copia editable requiere un .pf local")?;
    let (_, params) = read_in_root(root, &pf, 16 * 1024).map_err(io_error)?;
    if local_database(root, &params, base)? != db {
        return Err("La copia de edición y la base del esquema son distintas".into());
    }
    Ok(db)
}

fn apply(
    root: &Path,
    operation: &RecordOperation,
    cancel: &Cancellation,
) -> Result<RecordEvent, String> {
    let selection = &operation.selection;
    let db = write_database(root, selection)?;
    if db != operation.database_path {
        return Err("Cambió la base desde la vista previa".into());
    }
    let directory = journal_directory(root, selection, true)?;
    if let Some(source) = &operation.restore {
        let (backup, recorded_after, receipt) =
            confirmed_backup(root, &directory, &db, &source.id)?;
        if receipt != source.receipt
            || backup.column != selection.column_name()
            || recorded_after != operation.snapshot
            || backup.before.value != operation.next
        {
            return Err("Conflicto: el respaldo cambió desde la vista previa".into());
        }
    }
    let path_key = hex(db
        .to_str()
        .ok_or("La ruta de la copia ABL no es válida")?
        .as_bytes());
    let backup = format!(
        "{PROTOCOL}\n{path_key}\n{}\n{}\n{}\n{}",
        selection.column_name(),
        u8::from(operation.restore.is_some()),
        operation.next,
        snapshot_wire(&operation.snapshot)
    );
    let id = new_backup_id()?;
    publish_verified(
        root,
        &directory.join(format!("{id}.respaldo")),
        backup.as_bytes(),
    )?;
    let snapshot = run_avm(
        root,
        selection,
        &db,
        Some((&operation.snapshot, &operation.next)),
        cancel,
    )
    .map_err(|error| format!("{error}. Respaldo verificado {id}"))?;
    let receipt = format!("{backup}{}", snapshot_wire(&snapshot));
    publish_verified(
        root,
        &directory.join(format!("{id}.confirmado")),
        receipt.as_bytes(),
    )
    .map_err(|error| {
        format!(
            "El AVM confirmó el cambio y se conserva el respaldo {id}. El recibo falló: {error}"
        )
    })?;
    Ok(RecordEvent::Applied)
}

fn new_backup_id() -> Result<String, String> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "La hora del equipo no permite identificar el respaldo")?;
    Ok(format!(
        "{:039}-{:010}-{:020}",
        timestamp.as_nanos(),
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

fn journal_directory(root: &Path, selection: &Selection, create: bool) -> Result<PathBuf, String> {
    let mut path = root.to_owned();
    let key = selection.key.to_string();
    for part in [
        ".cide",
        "respaldos-registros",
        &selection.table.target.database,
        &selection.table.target.name,
        &key,
    ] {
        path.push(part);
        if create {
            match fs::create_dir(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(format!("No se pudo preparar el respaldo: {error}")),
            }
        }
        let metadata = fs::symlink_metadata(&path)
            .map_err(|_| "No hay respaldo confirmado para el registro")?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err("Los respaldos requieren directorios locales ordinarios".into());
            }
        }
        if !metadata.is_dir()
            || !path
                .canonicalize()
                .is_ok_and(|canonical| canonical.starts_with(root))
        {
            return Err("El directorio de respaldos está fuera del proyecto".into());
        }
    }
    Ok(path)
}

fn publish_verified(root: &Path, path: &Path, bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > MAX_PROTOCOL {
        return Err("El respaldo excede su límite de tamaño".into());
    }
    publish_new_bytes(path, bytes)
        .map_err(|error| format!("No se pudo guardar el respaldo: {error}"))?;
    let (_, read_back) = read_in_root(root, path, MAX_PROTOCOL).map_err(io_error)?;
    if read_back != bytes {
        return Err("No se pudo verificar el respaldo escrito".into());
    }
    Ok(())
}

fn journal_entries(directory: &Path) -> Result<BTreeMap<String, JournalFiles>, String> {
    let mut entries = BTreeMap::<_, JournalFiles>::new();
    for (i, entry) in fs::read_dir(directory).map_err(io_error)?.enumerate() {
        if i == 8192 {
            return Err("El historial del registro excede el límite de revisión".into());
        }
        let name = entry.map_err(io_error)?.file_name();
        let Some((id, extension)) = name.to_str().and_then(|name| name.rsplit_once('.')) else {
            continue;
        };
        if id.len() != 71
            || !id.bytes().enumerate().all(|(position, byte)| {
                if matches!(position, 39 | 50) {
                    byte == b'-'
                } else {
                    byte.is_ascii_digit()
                }
            })
            || !matches!(extension, "respaldo" | "confirmado")
        {
            continue;
        }
        let files = entries.entry(id.to_owned()).or_default();
        if extension == "confirmado" {
            files.receipt = true;
        }
    }
    Ok(entries)
}

#[derive(Default)]
struct JournalFiles {
    receipt: bool,
}

enum Observation {
    Unavailable,
    Missing,
    Row(Snapshot),
}

impl Observation {
    fn describe(&self, before: &Snapshot) -> &'static str {
        match self {
            Self::Row(row) if row.row_id == before.row_id && row.image == before.image => {
                "Pendiente · coincide con el respaldo"
            }
            Self::Row(_) => "Pendiente · el registro difiere del respaldo",
            Self::Missing => "Pendiente · el registro está ausente",
            Self::Unavailable => "Pendiente · estado actual sin verificar",
        }
    }
}

fn read_history(
    root: &Path,
    directory: &Path,
    db: &Path,
    codec: &str,
    cancel: &Cancellation,
    observe: impl FnOnce() -> Observation,
) -> Result<Vec<RecordHistoryEntry>, String> {
    let files = journal_entries(directory)?;
    let mut entries = Vec::with_capacity(files.len());
    let mut observe = Some(observe);
    let mut observation = Observation::Unavailable;
    for (index, (id, files)) in files.into_iter().rev().enumerate() {
        if cancel.is_cancelled() {
            return Err("Se canceló la consulta del historial de respaldos".into());
        }
        let parsed = if files.receipt {
            confirmed_backup(root, directory, db, &id)
                .map(|(backup, _, receipt)| ("Confirmado", backup, Some(receipt)))
        } else {
            read_backup(root, directory, db, &id).map(|(backup, _)| {
                if let Some(observe) = observe.take() {
                    observation = observe();
                }
                (observation.describe(&backup.before), backup, None)
            })
        };
        let (label, receipt) = match parsed {
            Ok((status, backup, receipt)) => {
                let action = if backup.restoring {
                    "Restauración"
                } else {
                    "Edición"
                };
                let prior = history_value(&backup.before.value, codec);
                let next = history_value(&backup.next, codec);
                (
                    format!(
                        "{status} · {action} de {} · {prior} → {next}",
                        backup.column
                    ),
                    receipt,
                )
            }
            Err(error) => (format!("Respaldo no verificable · {error}"), None),
        };
        entries.push(RecordHistoryEntry {
            id,
            label: format!("{}. {label}", index + 1),
            receipt,
        });
    }
    Ok(entries)
}

fn history_value(wire: &str, codec: &str) -> String {
    match decode_cell(wire, codec) {
        Ok(None) => "?".into(),
        Ok(Some(value)) => {
            let mut chars = value.chars().flat_map(char::escape_debug);
            let mut abbreviation: String = chars.by_ref().take(24).collect();
            if chars.next().is_some() {
                abbreviation.push('…');
            }
            format!("«{abbreviation}»")
        }
        Err(_) => "Valor ilegible".into(),
    }
}

struct Backup {
    column: String,
    restoring: bool,
    next: String,
    before: Snapshot,
}

fn latest_backup(
    root: &Path,
    directory: &Path,
    db: &Path,
) -> Result<(RestoreSource, Backup, Snapshot), String> {
    let (id, _) = journal_entries(directory)?
        .into_iter()
        .rev()
        .find(|(_, files)| files.receipt)
        .ok_or("No hay respaldo confirmado para el registro")?;
    let (backup, after, receipt) = confirmed_backup(root, directory, db, &id)?;
    Ok((RestoreSource { id, receipt }, backup, after))
}

fn read_backup(
    root: &Path,
    directory: &Path,
    db: &Path,
    id: &str,
) -> Result<(Backup, Vec<u8>), String> {
    let invalid = "El respaldo o su recibo están incompletos";
    let (_, bytes) = read_in_root(
        root,
        &directory.join(format!("{id}.respaldo")),
        MAX_PROTOCOL,
    )
    .map_err(io_error)?;
    let mut lines = std::str::from_utf8(&bytes).map_err(|_| invalid)?.lines();
    let path = hex(db.to_str().ok_or(invalid)?.as_bytes());
    if lines.next() != Some(PROTOCOL) || lines.next() != Some(&path) {
        return Err("El respaldo pertenece a otra base o versión".into());
    }
    let column = lines
        .next()
        .and_then(|column| super::name(column.as_bytes()))
        .ok_or(invalid)?;
    let restoring = match lines.next() {
        Some("0") => false,
        Some("1") => true,
        _ => return Err(invalid.into()),
    };
    let next = lines
        .next()
        .filter(|value| valid_value(value))
        .ok_or(invalid)?
        .to_owned();
    let before = parse_snapshot(lines)?;
    Ok((
        Backup {
            column,
            restoring,
            next,
            before,
        },
        bytes,
    ))
}

fn confirmed_backup(
    root: &Path,
    directory: &Path,
    db: &Path,
    id: &str,
) -> Result<(Backup, Snapshot, ContentFingerprint), String> {
    let invalid = "El respaldo o su recibo están incompletos";
    let (backup, bytes) = read_backup(root, directory, db, id)?;
    let (_, receipt) = read_in_root(
        root,
        &directory.join(format!("{id}.confirmado")),
        MAX_PROTOCOL,
    )
    .map_err(io_error)?;
    let sealed = receipt
        .strip_prefix(bytes.as_slice())
        .ok_or("El respaldo no coincide con el recibo confirmado")?;
    let after = parse_snapshot(std::str::from_utf8(sealed).map_err(|_| invalid)?.lines())?;
    if after.row_id != backup.before.row_id || after.value != backup.next {
        return Err(invalid.into());
    }
    Ok((backup, after, ContentFingerprint::of(&receipt)))
}

fn snapshot_wire(snapshot: &Snapshot) -> String {
    format!(
        "ROW|{}\nIMAGE|{}\nVALUE|{}\nEND\n",
        snapshot.row_id, snapshot.image, snapshot.value
    )
}

fn valid_hex(value: &str, limit: usize) -> bool {
    value.len() <= limit
        && value.len() % 2 == 0
        && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_value(value: &str) -> bool {
    value == "-" || valid_hex(value, 2048)
}

fn parse_snapshot(mut lines: std::str::Lines<'_>) -> Result<Snapshot, String> {
    let invalid = "La imagen del registro está incompleta o es inválida";
    let row_id = lines
        .next()
        .and_then(|line| line.strip_prefix("ROW|"))
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 128
                && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
        .ok_or(invalid)?
        .to_owned();
    let image = lines
        .next()
        .and_then(|line| line.strip_prefix("IMAGE|"))
        .filter(|value| !value.is_empty() && valid_hex(value, 16384))
        .ok_or(invalid)?
        .to_owned();
    let value = lines
        .next()
        .and_then(|line| line.strip_prefix("VALUE|"))
        .filter(|value| valid_value(value))
        .ok_or(invalid)?
        .to_owned();
    if lines.next() != Some("END") || lines.next().is_some() {
        return Err(invalid.into());
    }
    Ok(Snapshot {
        row_id,
        image,
        value,
    })
}

fn run_avm(
    root: &Path,
    selection: &Selection,
    db: &Path,
    apply: Option<(&Snapshot, &str)>,
    cancel: &Cancellation,
) -> Result<Snapshot, String> {
    let mode = apply.map_or(AvmMode::Preview, |(snapshot, value)| {
        AvmMode::Apply(snapshot, value)
    });
    run_mode(root, selection, db, mode, cancel)?.ok_or_else(|| "La fila ya no existe".into())
}

enum AvmMode<'a> {
    Preview,
    Apply(&'a Snapshot, &'a str),
    Inspect,
    Insert(&'a str),
    Delete(&'a Snapshot),
    Recreate(&'a Snapshot),
}

fn run_mode(
    root: &Path,
    selection: &Selection,
    db: &Path,
    mode: AvmMode<'_>,
    cancel: &Cancellation,
) -> Result<Option<Snapshot>, String> {
    let config = load_profile(root)?;
    crate::code_page(&config.avm.cpstream, "cpstream")
        .map_err(|_| "Página de códigos ABL inválida")?;
    if encoding(&config.avm.cpinternal)? != encoding(&selection.table.catalog.codec)? {
        return Err("Cambió la codificación del perfil. Actualiza el esquema".into());
    }
    if db.with_extension("lk").try_exists().map_err(io_error)? {
        return Err("Cierra la copia de la base antes de editarla".into());
    }
    let target = &selection.table.target;
    let key = target.key.as_ref().unwrap();
    let (mode, row, image, value) = match mode {
        AvmMode::Preview => ("preview", "", "", ""),
        AvmMode::Apply(before, value) => (
            "apply",
            before.row_id.as_str(),
            before.image.as_str(),
            value,
        ),
        AvmMode::Inspect => ("inspect", "", "", ""),
        AvmMode::Insert(value) => ("insert", "", "", value),
        AvmMode::Delete(before) => ("delete", before.row_id.as_str(), before.image.as_str(), ""),
        AvmMode::Recreate(before) => ("recreate", "", before.image.as_str(), ""),
    };
    let version = &selection.table.catalog.version;
    let input = format!(
        "{PROTOCOL}\n{mode}\n{version}\n{}\n{}\n{}\n{}\n{}\n{}\n{row}\n{image}\n{value}\n",
        target.database,
        target.name,
        key.index,
        key.columns[0].0,
        selection.key,
        selection.column_name()
    );
    let job = JobDir::create().map_err(io_error)?;
    let work = avm_path(&job.path);
    let helper = work.join("record.p");
    let request = work.join("record.in");
    let response = work.join("record.out");
    let log_path = work.join("avm.txt");
    fs::write(&helper, HELPER).map_err(io_error)?;
    fs::write(&request, input).map_err(io_error)?;
    let log = fs::File::create(&log_path).map_err(io_error)?;
    let mut cmd = Command::new(&config.avm.executable);
    cmd.arg("-db")
        .arg(avm_path(db))
        .args(["-ld", &target.database, "-1"]);
    if matches!(mode, "preview" | "inspect") {
        cmd.arg("-RO");
    }
    cmd.args([
        "-cpinternal",
        &config.avm.cpinternal,
        "-cpstream",
        &config.avm.cpstream,
        "-b",
        "-p",
    ])
    .arg(helper)
    .arg("-param")
    .arg(request)
    .arg("-T")
    .arg(&work)
    .current_dir(&work)
    .env_remove("PROPATH")
    .env_remove("PROSTARTUP")
    .stdin(Stdio::null())
    .stdout(log.try_clone().map_err(io_error)?)
    .stderr(log);
    if let Some(dlc) = config.avm.executable.parent().and_then(Path::parent) {
        cmd.env("DLC", dlc);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    if cancel.is_cancelled() {
        return Err("Operación ABL cancelada antes de abrir la base".into());
    }
    let mut child = cmd.spawn().map_err(io_error)?;
    let status = match wait(
        &mut child,
        &[(&log_path, 128 * 1024), (&response, MAX_PROTOCOL as u64)],
        cancel,
    ) {
        Ok(status) => status,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    if !status.success() || !response.is_file() {
        let log = bounded_read(&log_path, 128 * 1024).unwrap_or_default();
        let detail = decode(&log, &config.avm.cpstream)
            .lines()
            .take(7)
            .collect::<Vec<_>>()
            .join(" · ");
        return Err(format!(
            "OpenEdge terminó sin confirmar ({status}): {}",
            detail.chars().take(640).collect::<String>()
        ));
    }
    let bytes = bounded_read(&response, MAX_PROTOCOL as u64)?;
    let mut lines = std::str::from_utf8(&bytes)
        .map_err(|_| "Respuesta de OpenEdge inválida")?
        .lines();
    if lines.next() != Some(PROTOCOL)
        || lines.next().and_then(|line| line.strip_prefix("AVM|"))
            != Some(&selection.table.catalog.version)
    {
        return Err("Cambió la versión del AVM. Actualiza el esquema ABL".into());
    }
    let next = lines.clone().next();
    if let Some(error) = next.and_then(|line| line.strip_prefix("ERROR|")) {
        return Err(match error {
            "conflict" => "Conflicto: el registro cambió, ya no existe o está bloqueado",
            "shape" => "La clave o el tipo del registro ya no coincide con el esquema",
            "triggers" => "La cuenta no permite aislar los disparadores en la copia de prueba",
            "connection" => "OpenEdge no confirmó la conexión requerida para esta operación",
            _ => "OpenEdge rechazó la escritura y revirtió la transacción",
        }
        .into());
    }
    if next == Some("ABSENT") && matches!(mode, "inspect" | "delete") {
        lines.next();
        if lines.next() == Some("END") && lines.next().is_none() {
            return Ok(None);
        }
    }
    parse_snapshot(lines).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::{BrowseController, BrowseRequest, SchemaController};
    use std::time::Instant;

    fn wait_for<T>(wakeup: &Wakeup, mut poll: impl FnMut() -> Option<T>) -> T {
        let deadline = Instant::now() + Duration::from_secs(22);
        loop {
            let ticket = wakeup.ticket();
            if let Some(value) = poll() {
                return value;
            }
            assert!(
                Instant::now() < deadline,
                "La prueba de registros no recibió respuesta"
            );
            wakeup.wait(ticket, Some(Duration::from_millis(40)));
        }
    }

    fn finish(
        root: &Path,
        worker: &mut RecordController,
        wakeup: &Wakeup,
    ) -> Result<RecordEvent, String> {
        wait_for(wakeup, || worker.poll(root))
    }

    fn review(root: &Path, worker: &mut RecordController, wakeup: &Wakeup) -> RecordPreview {
        let Ok(RecordEvent::Preview(review)) = finish(root, worker, wakeup) else {
            panic!("No se recibió la vista previa de ABL");
        };
        review
    }

    fn failure(root: &Path, worker: &mut RecordController, wakeup: &Wakeup) -> String {
        finish(root, worker, wakeup)
            .err()
            .expect("la operación debía fallar")
    }

    #[test]
    fn record_history_verifies_receipts_and_keeps_pending_backups() {
        let fixture = JobDir::create().unwrap();
        let root = &fixture.path;
        let db = root.join("sports2000.db");
        let path = hex(db.to_str().unwrap().as_bytes());
        let before = Snapshot {
            row_id: "record".into(),
            image: "A1".into(),
            value: "41".into(),
        };
        let after = Snapshot {
            row_id: "record".into(),
            image: "B1".into(),
            value: "42".into(),
        };
        let backup = format!(
            "{PROTOCOL}\n{path}\nName\n0\n42\n{}",
            snapshot_wire(&before)
        );
        for index in 1..=3 {
            let id = format!("{index:039}-{:010}-{:020}", 0, 0);
            fs::write(root.join(format!("{id}.respaldo")), &backup).unwrap();
            if index != 2 {
                let receipt = if index == 1 {
                    format!("{backup}{}", snapshot_wire(&after))
                } else {
                    format!("{backup}{}", snapshot_wire(&before))
                };
                fs::write(root.join(format!("{id}.confirmado")), receipt).unwrap();
            }
        }
        fs::write(root.join("malformed.confirmado"), b"foreign").unwrap();
        let cancel = Cancellation::new(Duration::from_secs(10));
        let history = read_history(root, root, &db, "iso8859-1", &cancel, || {
            Observation::Row(before)
        })
        .unwrap();
        assert_eq!(history.len(), 3);
        assert!(history[0].label().contains("no verificable"));
        assert!(!history[0].confirmed());
        assert!(history[1]
            .label()
            .contains("Pendiente · coincide con el respaldo"));
        assert!(!history[1].confirmed());
        assert!(history[2]
            .label()
            .contains("Confirmado · Edición de Name · «A» → «B»"));
        assert!(history[2].confirmed());
        assert_eq!(fs::read_dir(root).unwrap().count(), 6);
    }

    #[test]
    #[ignore = "requiere una copia aislada de Sports2000 en CIDE_TEST_PROGRESS_RECORD_EDIT_PROJECT"]
    fn progress_real_record_edit_backup_restore_and_conflicts() {
        let root =
            PathBuf::from(std::env::var_os("CIDE_TEST_PROGRESS_RECORD_EDIT_PROJECT").unwrap())
                .canonicalize()
                .unwrap();
        let wakeup = Wakeup::default();
        let mut schema = SchemaController::default();
        let mut workbench = jc_app::Workbench::default();
        schema.request(Some(&root), wakeup.clone()).unwrap();
        let update = wait_for(&wakeup, || schema.poll(Some(&root), &mut workbench));
        assert!(!update.problems, "{:?}", workbench.problems());
        let table = schema
            .browse_tables()
            .into_iter()
            .find(|table| table.name.eq_ignore_ascii_case("Customer"))
            .unwrap();
        let column = table
            .columns
            .iter()
            .position(|column| column.eq_ignore_ascii_case("Name"))
            .unwrap();
        let other = table
            .columns
            .iter()
            .enumerate()
            .find(|(i, _)| {
                *i != column
                    && table.can_edit_column(*i)
                    && matches!(table.target.columns[*i].1, ColumnKind::Character)
            })
            .unwrap()
            .0;
        let mut chooser = BrowseController::default();
        chooser
            .request(
                &root,
                BrowseRequest {
                    table: table.clone(),
                    offset: 0,
                    filter: None,
                    descending: false,
                },
                wakeup.clone(),
            )
            .unwrap();
        let page = wait_for(&wakeup, || chooser.poll(&root)).unwrap();
        let mut row = page.rows[0].clone();
        let selection = Selection::new(&table, &row, column).unwrap();
        let external = Selection::new(&table, &row, other).unwrap();
        let db = write_database(&root, &selection).unwrap();
        let cancel = Cancellation::new(Duration::from_secs(75));
        let initial = run_avm(&root, &selection, &db, None, &cancel).unwrap();
        let external_before = run_avm(&root, &external, &db, None, &cancel).unwrap();
        let external_value = value_wire(&external, "CIDE concurrente").unwrap();
        let mut controller = RecordController::default();
        let next = "CIDE revisión ñ";

        // Make the journal unwritable using an ordinary file in the fixture.
        let journal = root
            .join(".cide/respaldos-registros")
            .join(&table.database)
            .join(&table.name)
            .join(selection.key.to_string());
        fs::create_dir_all(journal.parent().unwrap()).unwrap();
        fs::write(&journal, b"bloqueo de prueba").unwrap();
        controller
            .request_edit(&root, &table, &row, column, next.into(), wakeup.clone())
            .unwrap();
        let preview = review(&root, &mut controller, &wakeup);
        controller.apply(&root, preview, wakeup.clone()).unwrap();
        assert!(failure(&root, &mut controller, &wakeup).contains("respald"));
        assert!(run_avm(&root, &selection, &db, None, &cancel).unwrap() == initial);
        fs::remove_file(journal).unwrap();

        // An external change to a different column invalidates the whole image.
        controller
            .request_edit(&root, &table, &row, column, next.into(), wakeup.clone())
            .unwrap();
        let stale = review(&root, &mut controller, &wakeup);
        let changed = run_avm(
            &root,
            &external,
            &db,
            Some((&external_before, &external_value)),
            &cancel,
        )
        .unwrap();
        controller.apply(&root, stale, wakeup.clone()).unwrap();
        assert!(failure(&root, &mut controller, &wakeup).contains("Conflicto"));
        assert_eq!(
            run_avm(&root, &selection, &db, None, &cancel)
                .unwrap()
                .value,
            initial.value
        );
        run_avm(
            &root,
            &external,
            &db,
            Some((&changed, &external_before.value)),
            &cancel,
        )
        .unwrap();

        controller
            .request_edit(&root, &table, &row, column, next.into(), wakeup.clone())
            .unwrap();
        let preview = review(&root, &mut controller, &wakeup);
        assert_eq!(preview.before, row[column]);
        assert_eq!(preview.after.as_deref(), Some(next));
        controller.apply(&root, preview, wakeup.clone()).unwrap();
        assert!(matches!(
            finish(&root, &mut controller, &wakeup),
            Ok(RecordEvent::Applied)
        ));
        let directory = journal_directory(&root, &selection, false).unwrap();
        let (source, backup, receipt) = latest_backup(&root, &directory, &db).unwrap();
        assert_eq!(backup.before.value, initial.value);
        assert!(run_avm(&root, &selection, &db, None, &cancel).unwrap() == receipt);
        row[column] = Some(next.into());

        let backup_path = directory.join(format!("{}.respaldo", source.id));
        let original_backup = fs::read(&backup_path).unwrap();
        let mut altered = original_backup.clone();
        let position = altered
            .windows(6)
            .position(|value| value == b"IMAGE|")
            .unwrap()
            + 6;
        altered[position] = if altered[position] == b'1' {
            b'2'
        } else {
            b'1'
        };
        fs::write(&backup_path, altered).unwrap();
        controller
            .request_restore(&root, &table, &row, wakeup.clone())
            .unwrap();
        assert!(failure(&root, &mut controller, &wakeup).contains("recibo"));
        fs::write(backup_path, original_backup).unwrap();

        controller
            .request_restore(&root, &table, &row, wakeup.clone())
            .unwrap();
        let stale_restore = review(&root, &mut controller, &wakeup);
        let before_concurrent = run_avm(&root, &external, &db, None, &cancel).unwrap();
        let changed = run_avm(
            &root,
            &external,
            &db,
            Some((&before_concurrent, &external_value)),
            &cancel,
        )
        .unwrap();
        controller
            .apply(&root, stale_restore, wakeup.clone())
            .unwrap();
        assert!(failure(&root, &mut controller, &wakeup).contains("Conflicto"));
        controller
            .request_restore(&root, &table, &row, wakeup.clone())
            .unwrap();
        assert!(failure(&root, &mut controller, &wakeup).contains("Conflicto"));
        controller
            .request_history(&root, &table, &row, wakeup.clone())
            .unwrap();
        let Ok(RecordEvent::History(history)) = finish(&root, &mut controller, &wakeup) else {
            panic!("No se recibió el historial de respaldos");
        };
        let selected = history
            .entries
            .iter()
            .position(|entry| entry.id == source.id)
            .unwrap();
        assert!(history
            .entries
            .iter()
            .any(|entry| entry.label().contains("Pendiente")));
        controller
            .request_history_restore(&root, &history, selected, wakeup.clone())
            .unwrap();
        let Ok(RecordEvent::Conflict(conflict)) = finish(&root, &mut controller, &wakeup) else {
            panic!("La imagen alterada debía mostrarse como conflicto");
        };
        assert_eq!(conflict.expected.as_deref(), Some(next));
        assert_eq!(conflict.actual.as_deref(), Some(next));
        run_avm(
            &root,
            &external,
            &db,
            Some((&changed, &external_before.value)),
            &cancel,
        )
        .unwrap();

        // A later edition and its restoration leave the selected older postimage current.
        controller
            .request_edit(
                &root,
                &table,
                &row,
                column,
                "CIDE historial posterior".into(),
                wakeup.clone(),
            )
            .unwrap();
        let preview = review(&root, &mut controller, &wakeup);
        controller.apply(&root, preview, wakeup.clone()).unwrap();
        assert!(matches!(
            finish(&root, &mut controller, &wakeup),
            Ok(RecordEvent::Applied)
        ));
        controller
            .request_restore(&root, &table, &row, wakeup.clone())
            .unwrap();
        let preview = review(&root, &mut controller, &wakeup);
        assert_eq!(preview.after.as_deref(), Some(next));
        controller.apply(&root, preview, wakeup.clone()).unwrap();
        assert!(matches!(
            finish(&root, &mut controller, &wakeup),
            Ok(RecordEvent::Applied)
        ));

        controller
            .request_history(&root, &table, &row, wakeup.clone())
            .unwrap();
        let Ok(RecordEvent::History(history)) = finish(&root, &mut controller, &wakeup) else {
            panic!("No se recibió el historial actualizado");
        };
        let selected = history
            .entries
            .iter()
            .position(|entry| entry.id == source.id)
            .unwrap();
        assert!(selected > 0);
        let pending = history
            .entries
            .iter()
            .position(|entry| !entry.confirmed())
            .unwrap();
        assert!(controller
            .request_history_restore(&root, &history, pending, wakeup.clone())
            .is_err());
        assert!(!controller.busy());
        controller
            .request_history_restore(&root, &history, selected, wakeup.clone())
            .unwrap();
        let preview = review(&root, &mut controller, &wakeup);
        assert!(preview.restoring);
        assert_eq!(preview.before.as_deref(), Some(next));
        controller.apply(&root, preview, wakeup.clone()).unwrap();
        assert!(matches!(
            finish(&root, &mut controller, &wakeup),
            Ok(RecordEvent::Applied)
        ));
        let result = run_avm(&root, &selection, &db, None, &cancel).unwrap();
        assert!(
            result == initial,
            "la restauración debe recuperar la imagen original"
        );
    }
}
