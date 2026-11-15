//! Bounded record reads through an isolated, explicitly read-only ABL session.
use super::{Catalog, SchemaController};
use crate::{
    editor::{avm_path, bounded_read, decode, load_profile, wait, JobDir},
    valid_avm_version, SchemaRecord,
};
use jc_app::{Cancellation, Wakeup};
use jc_text::{encode, TextEncoding};
use std::{
    fmt::Write as _,
    fs::{self, File},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{atomic::Ordering, mpsc, Arc},
    thread,
    time::Duration,
};

pub const BROWSE_PAGE_SIZE: usize = 20;
pub(in crate::editor) mod edit;
pub const BROWSE_MAX_FILTER_BYTES: usize = 256;
const MAX_COLUMNS: usize = 12;
const MAX_OFFSET: usize = 2_000;
const MAX_CELL_BYTES: usize = 1_024;
const MAX_RESPONSE: u64 = 1024 * 1024;
const HELPER: &[u8] = include_bytes!("../../../../../progress/abl/jc-browse.p");
const HEADER: &str = "JCIDE-BROWSE/1";

#[derive(Clone)]
pub struct BrowseTable {
    pub database: String,
    pub name: String,
    pub columns: Vec<String>,
    catalog: Arc<Catalog>,
    target: Arc<Target>,
}

struct Target {
    database: String,
    name: String,
    columns: Vec<(String, ColumnKind)>,
    key: Option<Key>,
}

struct Key {
    index: String,
    columns: Vec<(String, bool)>,
}

#[derive(Clone, Copy)]
enum ColumnKind {
    Character,
    Integer,
    Scalar,
}

#[derive(Clone)]
pub struct BrowseFilter {
    pub column: usize,
    pub value: String,
}

#[derive(Clone)]
pub struct BrowseRequest {
    pub table: BrowseTable,
    pub offset: usize,
    pub filter: Option<BrowseFilter>,
    pub descending: bool,
}

#[derive(Debug)]
pub struct BrowsePage {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Option<String>>>,
    pub offset: usize,
    pub has_next: bool,
}

#[derive(Default)]
pub struct BrowseController {
    pending: Option<Pending>,
}

struct Pending {
    catalog: Arc<Catalog>,
    cancel: Cancellation,
    result: mpsc::Receiver<Result<BrowsePage, String>>,
}

impl SchemaController {
    pub fn browse_tables(&self) -> Vec<BrowseTable> {
        let Some(catalog) = &self.catalog else {
            return Vec::new();
        };
        let mut tables = Vec::new();
        let mut remaining = catalog.records.as_slice();
        while let Some((record, tail)) = remaining.split_first() {
            remaining = tail;
            let SchemaRecord::Table { database, table } = record else {
                continue;
            };
            let end = tail
                .iter()
                .position(|entry| {
                    matches!(
                        entry,
                        SchemaRecord::Table { .. } | SchemaRecord::Database { .. }
                    )
                })
                .unwrap_or(tail.len());
            remaining = &tail[end..];
            let Some(database) = name(database.as_bytes()) else {
                continue;
            };
            let Some(table) = name(table.as_bytes()) else {
                continue;
            };
            let (columns, key) = table_shape(&tail[..end]);
            tables.push(BrowseTable {
                database: database.clone(),
                name: table.clone(),
                columns: columns.iter().map(|(name, _)| name.clone()).collect(),
                catalog: Arc::clone(catalog),
                target: Arc::new(Target {
                    database,
                    name: table,
                    columns,
                    key,
                }),
            });
        }
        tables
    }
}

fn name(bytes: &[u8]) -> Option<String> {
    (bytes.len() <= 64
        && bytes.first().is_some_and(u8::is_ascii_alphabetic)
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(b)))
    .then(|| String::from_utf8(bytes.to_vec()).expect("validated ASCII identifier"))
}

fn table_shape(records: &[SchemaRecord]) -> (Vec<(String, ColumnKind)>, Option<Key>) {
    let mut fields = Vec::new();
    let mut key = None;
    let mut reading_key = false;
    for record in records {
        match record {
            SchemaRecord::Field {
                name: field,
                data_type,
                extent: 0,
                ..
            } => {
                let kind = match data_type.as_bytes().to_ascii_uppercase().as_slice() {
                    b"CHARACTER" => ColumnKind::Character,
                    b"INTEGER" => ColumnKind::Integer,
                    b"DECIMAL" | b"DATE" | b"DATETIME" | b"DATETIME-TZ" | b"INT64" | b"LOGICAL" => {
                        ColumnKind::Scalar
                    }
                    _ => continue,
                };
                if let Some(field) = name(field.as_bytes()) {
                    fields.push((field, kind));
                }
            }
            SchemaRecord::Index {
                name: index,
                primary,
                unique,
                ..
            } => {
                reading_key = *primary && *unique && key.is_none();
                if reading_key {
                    key = name(index.as_bytes()).map(|index| Key {
                        index,
                        columns: Vec::new(),
                    });
                }
            }
            SchemaRecord::IndexField {
                field,
                sequence,
                ascending,
                ..
            } if reading_key => {
                let Some(k) = &mut key else { continue };
                if let Some(field) = name(field.as_bytes()).filter(|_| {
                    *sequence as usize == k.columns.len() + 1 && k.columns.len() < MAX_COLUMNS
                }) {
                    k.columns.push((field, *ascending));
                } else {
                    key = None;
                    reading_key = false;
                }
            }
            _ => {}
        }
    }
    let key = key.filter(|k| {
        !k.columns.is_empty()
            && k.columns.iter().all(|(column, _)| {
                fields
                    .iter()
                    .any(|(field, _)| field.eq_ignore_ascii_case(column))
            })
    });
    fields.truncate(MAX_COLUMNS);
    (fields, key)
}

impl BrowseController {
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }

    pub fn request(
        &mut self,
        root: &Path,
        request: BrowseRequest,
        wakeup: Wakeup,
    ) -> Result<(), String> {
        if self.busy() {
            return Err("Ya hay una página de registros en curso".into());
        }
        let catalog = Arc::clone(&request.table.catalog);
        if root != catalog.root || !catalog.live.load(Ordering::Acquire) {
            return Err("El esquema cambió. Vuelve a abrir Ver registros desde el proyecto".into());
        }
        if request.offset > MAX_OFFSET || request.offset % BROWSE_PAGE_SIZE != 0 {
            return Err("Página fuera del límite de esta consulta".into());
        }
        let input = encode_request(&request, &catalog.codec)?;
        let cancel = Cancellation::new(Duration::from_secs(15));
        let worker_cancel = cancel.clone();
        let root = root.to_owned();
        let (sender, result) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name("cide-abl-records".into())
            .spawn(move || {
                let page = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    read_page(&root, &request, input, &worker_cancel)
                }))
                .unwrap_or_else(|_| Err("Falló el lector de registros ABL".into()));
                let _ = sender.send(page);
                wakeup.signal();
            })
            .map_err(|err| format!("No se pudo iniciar Ver registros: {err}"))?;
        self.pending = Some(Pending {
            catalog,
            cancel,
            result,
        });
        Ok(())
    }

    pub fn poll(&mut self, root: &Path) -> Option<Result<BrowsePage, String>> {
        let pending = self.pending.as_ref()?;
        if root != pending.catalog.root || !pending.catalog.live.load(Ordering::Acquire) {
            self.cancel();
            return Some(Err(
                "El proyecto o el esquema cambió durante la consulta".into()
            ));
        }
        let result = match pending.result.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("El lector de registros terminó sin respuesta".into())
            }
        };
        self.pending = None;
        Some(result)
    }

    pub fn cancel(&mut self) -> bool {
        self.pending.take().is_some_and(|pending| {
            pending.cancel.cancel();
            true
        })
    }
}

impl Drop for BrowseController {
    fn drop(&mut self) {
        self.cancel();
    }
}

fn encoding(codec: &str) -> Result<TextEncoding, String> {
    match codec.to_ascii_lowercase().as_str() {
        "utf-8" | "utf8" => Ok(TextEncoding::Utf8),
        "iso8859-1" | "iso-8859-1" => Ok(TextEncoding::Iso8859_1),
        "1252" | "windows-1252" => Ok(TextEncoding::Windows1252),
        "ibm850" | "850" => Ok(TextEncoding::Ibm850),
        _ => Err("La página de códigos ABL aún no es compatible con Ver registros".into()),
    }
}

fn encode_request(request: &BrowseRequest, codec: &str) -> Result<String, String> {
    let target = &request.table.target;
    let key = target
        .key
        .as_ref()
        .ok_or("La tabla requiere un índice primario único para paginar")?;
    if target.columns.is_empty() {
        return Err("La tabla no tiene columnas escalares compatibles".into());
    }
    let codec = encoding(codec)?;
    let columns = target
        .columns
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>()
        .join(",");
    let ordering = key
        .columns
        .iter()
        .map(|(column, ascending)| format!("{column}:{}", usize::from(*ascending)))
        .collect::<Vec<_>>()
        .join(",");
    let (column, value) = if let Some(filter) = &request.filter {
        if filter.value.len() > BROWSE_MAX_FILTER_BYTES
            || filter.value.chars().any(char::is_control)
        {
            return Err("El filtro admite hasta 256 bytes en una línea".into());
        }
        let (_, kind) = target
            .columns
            .get(filter.column)
            .ok_or("Columna de filtro inválida")?;
        match kind {
            ColumnKind::Integer if filter.value.trim().parse::<i32>().is_err() => {
                return Err("Escribe un número entero válido para esta columna".into());
            }
            ColumnKind::Scalar => {
                return Err("En este modo se filtran columnas de texto o enteros".into())
            }
            _ => {}
        }
        let bytes = encode(&filter.value, codec)
            .map_err(|_| "El filtro no cabe en la página de códigos ABL")?;
        let mut hex = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            write!(hex, "{byte:02X}").unwrap();
        }
        (filter.column + 1, hex)
    } else {
        (0, String::new())
    };
    Ok(format!(
        "{HEADER}\n{}\n{}\n{}\n{columns}\n{ordering}\n{}\n{}\n{column}\n{value}\n",
        target.database,
        target.name,
        key.index,
        usize::from(request.descending),
        request.offset
    ))
}

fn read_page(
    root: &Path,
    request: &BrowseRequest,
    input: String,
    cancel: &Cancellation,
) -> Result<BrowsePage, String> {
    let config = load_profile(root)?;
    crate::code_page(&config.avm.cpstream, "cpstream")
        .map_err(|_| "La página de códigos de salida del perfil ABL es inválida")?;
    let catalog = &request.table.catalog;
    if encoding(&config.avm.cpinternal)? != encoding(&catalog.codec)? {
        return Err("Cambió la codificación del perfil. Actualiza el esquema ABL".into());
    }
    let target = &request.table.target;
    let pf = config
        .avm
        .parameter_file
        .as_deref()
        .ok_or("Configura un .pf local con -db, -ld, -1 y -RO")?;
    let (_, bytes) = jc_files::read_in_root(root, pf, 16 * 1024)
        .map_err(|error| format!("Ver registros requiere un .pf dentro del proyecto: {error}"))?;
    let db = local_database(root, &bytes, &target.database)?;
    if db.with_extension("lk").try_exists().map_err(io_error)? {
        return Err("Usa una copia cerrada de la base para la lectura local -RO".into());
    }
    let job = JobDir::create().map_err(io_error)?;
    let work = avm_path(&job.path);
    let helper = work.join("jc-browse.p");
    let request_path = work.join("records.in");
    let response = work.join("records.out");
    let log_path = work.join("avm.txt");
    fs::write(&helper, HELPER).map_err(io_error)?;
    fs::write(&request_path, input).map_err(io_error)?;
    let log = File::create(&log_path).map_err(io_error)?;
    let mut cmd = Command::new(config.avm.executable);
    cmd.arg("-db")
        .arg(avm_path(&db))
        .args(["-ld", &target.database, "-1", "-RO"])
        .args([
            "-cpinternal",
            &config.avm.cpinternal,
            "-cpstream",
            &config.avm.cpstream,
            "-b",
            "-p",
        ])
        .arg(helper)
        .arg("-param")
        .arg(request_path)
        .arg("-T")
        .arg(&work)
        .current_dir(&work)
        .env_remove("PROPATH")
        .env_remove("PROSTARTUP")
        .stdin(Stdio::null())
        .stdout(log.try_clone().map_err(io_error)?)
        .stderr(log);
    if let Some(dlc) = Path::new(cmd.get_program()).parent().and_then(Path::parent) {
        let dlc = dlc.to_owned();
        cmd.env("DLC", dlc);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    if cancel.is_cancelled() {
        return Err("Consulta de registros cancelada".into());
    }
    let mut child = cmd.spawn().map_err(io_error)?;
    let watched = [
        (log_path.as_path(), 128 * 1024),
        (response.as_path(), MAX_RESPONSE),
    ];
    let status = match wait(&mut child, &watched, cancel) {
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
            .take(4)
            .collect::<Vec<_>>()
            .join(" · ");
        return Err(format!(
            "El AVM no terminó la consulta de registros ({status}). {}",
            detail.chars().take(340).collect::<String>()
        ));
    }
    parse_page(
        &bounded_read(&response, MAX_RESPONSE)?,
        request,
        &catalog.codec,
        &catalog.version,
    )
}

fn local_database(root: &Path, bytes: &[u8], database: &str) -> Result<PathBuf, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "El .pf de lectura debe estar en UTF-8")?;
    let mut selected = None;
    let mut names = std::collections::BTreeSet::new();
    for line in text.trim_start_matches('\u{feff}').lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let invalid =
            "Para Ver registros usa una conexión por línea: -db \"ruta\" -ld nombre -1 -RO";
        let remainder = line.strip_prefix("-db ").ok_or(invalid)?.trim_start();
        let (path, rest) = if let Some(quoted) = remainder.strip_prefix('"') {
            let (path, rest) = quoted.split_once('"').ok_or(invalid)?;
            if !rest.starts_with(char::is_whitespace) {
                return Err(invalid.into());
            }
            (path, rest)
        } else {
            remainder.split_once(char::is_whitespace).ok_or(invalid)?
        };
        let options: Vec<_> = rest.split_ascii_whitespace().collect();
        let (["-ld", logical, "-1", "-RO"] | ["-ld", logical, "-RO", "-1"]) = options.as_slice()
        else {
            return Err(invalid.into());
        };
        if path.is_empty()
            || path.chars().any(char::is_control)
            || name(logical.as_bytes()).is_none()
            || !names.insert(logical.to_ascii_lowercase())
        {
            return Err(invalid.into());
        }
        if logical.eq_ignore_ascii_case(database) {
            selected = Some(path);
        }
    }
    let path = root.join(
        selected
            .ok_or("La base lógica no está declarada como local y de solo lectura en el .pf")?,
    );
    let path = match path.extension() {
        None => path.with_extension("db"),
        Some(ext) if ext.eq_ignore_ascii_case("db") => path,
        _ => return Err("La ruta de la base local debe apuntar a un archivo .db".into()),
    };
    let path = path.canonicalize().map_err(io_error)?;
    #[cfg(windows)]
    if !matches!(path.components().next(), Some(std::path::Component::Prefix(prefix))
        if matches!(prefix.kind(), std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)))
    {
        return Err("Ver registros requiere una copia en un disco local".into());
    }
    if !path.is_file() {
        return Err("No se encontró el archivo local de la base".into());
    }
    Ok(path)
}

fn parse_page(
    bytes: &[u8],
    request: &BrowseRequest,
    codec: &str,
    version: &str,
) -> Result<BrowsePage, String> {
    let invalid = "El AVM devolvió una página de registros incompleta o inválida";
    let mut lines = std::str::from_utf8(bytes).map_err(|_| invalid)?.lines();
    if lines.next() != Some(HEADER) {
        return Err(invalid.into());
    }
    let actual = lines
        .next()
        .and_then(|row| row.strip_prefix("AVM|"))
        .filter(|version| valid_avm_version(version));
    if !actual.is_some_and(|value| value.eq_ignore_ascii_case(version)) {
        return Err(
            "Cambió la versión de OpenEdge. Actualiza el esquema antes de consultar".into(),
        );
    }
    let columns: Vec<_> = request
        .table
        .target
        .columns
        .iter()
        .map(|(name, _)| name.clone())
        .collect();
    let mut rows = Vec::with_capacity(BROWSE_PAGE_SIZE);
    while let Some(line) = lines.next() {
        if let Some(values) = line.strip_prefix("ROW|") {
            if rows.len() == BROWSE_PAGE_SIZE {
                return Err(invalid.into());
            }
            let values = values
                .split('|')
                .map(|value| decode_cell(value, codec))
                .collect::<Result<Vec<_>, _>>()?;
            if values.len() != columns.len() {
                return Err(invalid.into());
            }
            rows.push(values);
        } else if let Some(error) = line.strip_prefix("ERROR|") {
            return Err(match error {
                "scan" => "Se alcanzó el límite de lectura de esta tabla",
                "cell" => "Una celda supera el tamaño admitido para esta cuadrícula",
                "readonly" => "El AVM no confirmó una única conexión de solo lectura",
                "triggers" => {
                    "La cuenta debe poder desactivar los disparadores FIND para consultar registros"
                }
                "shape" => {
                    "La definición de la tabla cambió. Actualiza el esquema y vuelve a abrirla"
                }
                "filter" => "El AVM rechazó el filtro para esta columna",
                _ => "El AVM no pudo abrir o leer esta tabla de forma segura",
            }
            .into());
        } else {
            let more = match line {
                "END|0" => false,
                "END|1" if rows.len() == BROWSE_PAGE_SIZE => true,
                _ => return Err(invalid.into()),
            };
            if lines.next().is_some() {
                return Err(invalid.into());
            }
            return Ok(BrowsePage {
                columns,
                rows,
                offset: request.offset,
                has_next: more && request.offset < MAX_OFFSET,
            });
        }
    }
    Err(invalid.into())
}

fn decode_cell(value: &str, codec: &str) -> Result<Option<String>, String> {
    if value == "-" {
        return Ok(None);
    }
    let invalid = "El AVM devolvió una celda con codificación inválida";
    if value.len() % 2 != 0
        || value.len() > 2 * MAX_CELL_BYTES
        || !value.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(invalid.into());
    }
    let bytes = (0..value.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&value[i..i + 2], 16))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| invalid)?;
    if encoding(codec)? == TextEncoding::Utf8 && std::str::from_utf8(&bytes).is_err() {
        return Err(invalid.into());
    }
    // Keep each cell on a single visible line without forwarding control bytes.
    let text = decode(&bytes, codec).replace(['\n', '\r', '\t'], " ");
    Ok(Some(text))
}

fn io_error(error: std::io::Error) -> String {
    format!("No se pudo leer la página ABL: {error}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn browse_cells_distinguish_unknown_empty_and_encoded_text() {
        assert_eq!(decode_cell("-", "iso8859-1").unwrap(), None);
        assert_eq!(decode_cell("", "iso8859-1").unwrap(), Some(String::new()));
        assert_eq!(
            decode_cell("6E69F16F", "iso8859-1").unwrap().as_deref(),
            Some("niño")
        );
        assert!(decode_cell("FF", "utf-8").is_err());
        assert!(decode_cell("6Z", "utf-8").is_err());
    }

    #[test]
    fn browse_rejects_writable_network_and_ambiguous_parameter_files() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        for profile in [
            "-db sports2000 -ld sports2000 -1\n",
            "-db sports2000 -ld sports2000 -1 -RO -H ejemplo\n",
            "-pf otro.pf\n",
            "-db a -ld sports2000 -1 -RO\n-db b -ld SPORTS2000 -1 -RO\n",
            "-db \"ruta con espacios\"-ld sports2000 -1 -RO\n",
        ] {
            let error = local_database(root, profile.as_bytes(), "sports2000").unwrap_err();
            assert!(error.starts_with("Para Ver registros"), "{error}");
        }
    }

    #[test]
    #[ignore = "requiere una copia cerrada de Sports2000 y un perfil AVM local en CIDE_TEST_PROGRESS_BROWSE_PROJECT"]
    fn browse_real_sports2000_pages_filter_order_and_preserves_database() {
        let root = PathBuf::from(std::env::var_os("CIDE_TEST_PROGRESS_BROWSE_PROJECT").unwrap())
            .canonicalize()
            .unwrap();
        let before: Vec<_> = ["db", "d1", "b1"]
            .into_iter()
            .map(|extension| {
                let path = root.join(format!("sports2000.{extension}"));
                let bytes = fs::read(&path).unwrap();
                (path, bytes)
            })
            .collect();
        let wakeup = Wakeup::default();
        let mut schema = SchemaController::default();
        let mut workbench = jc_app::Workbench::default();
        schema.request(Some(&root), wakeup.clone()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let ticket = wakeup.ticket();
            if let Some(update) = schema.poll(Some(&root), &mut workbench) {
                assert!(!update.problems, "{:?}", workbench.problems());
                break;
            }
            assert!(Instant::now() < deadline);
            wakeup.wait(ticket, Some(Duration::from_millis(40)));
        }
        let table = schema
            .browse_tables()
            .into_iter()
            .find(|table| table.name == "Customer")
            .unwrap();
        let key = table
            .columns
            .iter()
            .position(|column| column == "CustNum")
            .unwrap();
        let name = table
            .columns
            .iter()
            .position(|column| column == "Name")
            .unwrap();
        let mut controller = BrowseController::default();
        let mut next = |offset, filter, descending| {
            let request = BrowseRequest {
                table: table.clone(),
                offset,
                filter,
                descending,
            };
            controller.request(&root, request, wakeup.clone()).unwrap();
            let deadline = Instant::now() + Duration::from_secs(16);
            loop {
                let ticket = wakeup.ticket();
                if let Some(result) = controller.poll(&root) {
                    break result.unwrap();
                }
                assert!(Instant::now() < deadline);
                wakeup.wait(ticket, Some(Duration::from_millis(40)));
            }
        };
        let numbers = |page: &BrowsePage| {
            page.rows
                .iter()
                .map(|row| row[key].as_deref().unwrap().parse::<i32>().unwrap())
                .collect::<Vec<_>>()
        };
        let first = next(0, None, false);
        assert_eq!(first.rows.len(), BROWSE_PAGE_SIZE);
        assert!(first.has_next);
        assert_eq!(numbers(&first), (1..=20).collect::<Vec<_>>());
        let second = next(BROWSE_PAGE_SIZE, None, false);
        assert_eq!(second.offset, BROWSE_PAGE_SIZE);
        assert!(numbers(&second)[0] > numbers(&first)[19]);
        let descending = numbers(&next(0, None, true));
        assert!(descending.windows(2).all(|pair| pair[0] > pair[1]));
        assert!(descending[0] >= numbers(&second)[19]);
        let filtered = next(
            0,
            Some(BrowseFilter {
                column: key,
                value: "5".into(),
            }),
            false,
        );
        assert_eq!(numbers(&filtered), [5]);
        assert!(!filtered.has_next);
        let text = first.rows[0][name].clone().unwrap();
        let filtered = next(
            0,
            Some(BrowseFilter {
                column: name,
                value: text.clone(),
            }),
            false,
        );
        assert!(filtered
            .rows
            .iter()
            .any(|row| row[key].as_deref() == Some("1")));
        assert!(filtered
            .rows
            .iter()
            .all(|row| row[name].as_deref() == Some(&text)));
        let filtered = next(
            0,
            Some(BrowseFilter {
                column: name,
                value: "~\" OR TRUE /*".into(),
            }),
            false,
        );
        assert!(filtered.rows.is_empty());
        for (path, bytes) in before {
            assert_eq!(fs::read(path).unwrap(), bytes);
        }
    }
}
