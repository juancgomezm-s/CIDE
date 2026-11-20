//! Per-project connection settings shared by the native GUI and the TUI.

use jc_files::{FileError, FileSession};
use jc_text::TextEncoding;
use serde_json::{json, Map, Value};
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::Duration,
};

/// A single probe owns its immutable draft and never writes a profile.
pub struct ConnectionTest {
    cancel: crate::Cancellation,
    result: mpsc::Receiver<Result<String, String>>,
}

impl ConnectionTest {
    pub fn start(
        run: impl FnOnce(&crate::Cancellation) -> Result<String, String> + Send + 'static,
        notify: impl FnOnce() + Send + 'static,
    ) -> Result<Self, String> {
        let cancel = crate::Cancellation::new(Duration::from_secs(15));
        let worker_cancel = cancel.clone();
        let (send, result) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name("cide-probar-conexion".into())
            .spawn(move || {
                let result = run(&worker_cancel);
                let result = if worker_cancel.is_cancelled() {
                    Err("Se agotó el tiempo para probar la conexión.".into())
                } else {
                    result
                };
                let _ = send.send(result);
                notify();
            })
            .map_err(|error| format!("No se pudo iniciar la prueba de conexión: {error}"))?;
        Ok(Self { cancel, result })
    }

    pub fn poll(&self) -> Option<Result<String, String>> {
        match self.result.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err("La prueba de conexión terminó sin responder.".into()))
            }
        }
    }
}

impl Drop for ConnectionTest {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

const FILE: &str = "conexiones.local.json";
const MAX_BYTES: usize = 64 * 1024;
const MAX_PROFILES: usize = 32;
pub const MAX_VALUE_BYTES: usize = 256;
pub const SQL_TLS_FIELD: usize = 5;
pub const SQL_TLS_MODES: [(&str, &str); 3] = [
    (
        "verificar-servidor",
        "Verificar identidad del servidor (recomendado)",
    ),
    (
        "requerido",
        "Cifrar sin verificar la identidad del servidor",
    ),
    (
        "deshabilitado",
        "Sin TLS, para una conexión local de pruebas",
    ),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Engine {
    OpenEdge,
    PostgreSql,
    MySql,
}

impl Engine {
    pub const ALL: [Self; 3] = [Self::OpenEdge, Self::PostgreSql, Self::MySql];

    pub fn label(self) -> &'static str {
        match self {
            Self::OpenEdge => "OpenEdge",
            Self::PostgreSql => "PostgreSQL",
            Self::MySql => "MySQL",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::OpenEdge => "openedge",
            Self::PostgreSql => "postgresql",
            Self::MySql => "mysql",
        }
    }
}

pub struct Field {
    key: &'static str,
    label: &'static str,
    required: bool,
}

impl Field {
    const fn new(key: &'static str, label: &'static str, required: bool) -> Self {
        Self {
            key,
            label,
            required,
        }
    }

    pub fn label(&self) -> &'static str {
        self.label
    }

    pub fn guidance(&self) -> &'static str {
        match self.key {
            "nombre" => "Identifica esta conexión dentro del proyecto.",
            "servidor" => "Nombre DNS o dirección IP del servidor SQL.",
            "puerto" => "Puerto TCP del servidor, entre 1 y 65535.",
            "base" => "Nombre de la base de datos en el servidor.",
            "usuario" => "Usuario de la base de datos con permisos de consulta.",
            "tls" => "F4 elige TLS: verificar-servidor, requerido o deshabilitado.",
            "credencial" => "Opcional. Nombre genérico de Windows: cide/datadigger/nombre.",
            "esquemas" => "Esquemas PostgreSQL separados por coma. El inicial es public.",
            _ => "",
        }
    }
}

const NAME: Field = Field::new("nombre", "Nombre", true);
const POSTGRES: [Field; 8] = [
    NAME,
    Field::new("servidor", "Servidor", true),
    Field::new("puerto", "Puerto", true),
    Field::new("base", "Base de datos", true),
    Field::new("usuario", "Usuario", true),
    Field::new("tls", "TLS", true),
    Field::new("credencial", "Referencia de credencial", false),
    Field::new("esquemas", "Esquemas (separados por coma)", true),
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Draft {
    engine: Engine,
    values: Vec<String>,
}

impl Draft {
    pub fn new(engine: Engine) -> Self {
        let values = match engine {
            Engine::OpenEdge => vec![String::new()],
            Engine::PostgreSql => [
                "",
                "localhost",
                "5432",
                "",
                "",
                "verificar-servidor",
                "",
                "public",
            ]
            .map(str::to_owned)
            .into(),
            Engine::MySql => ["", "localhost", "3306", "", "", "verificar-servidor", ""]
                .map(str::to_owned)
                .into(),
        };
        Self { engine, values }
    }

    pub fn engine(&self) -> Engine {
        self.engine
    }

    pub fn fields(&self) -> &'static [Field] {
        match self.engine {
            Engine::OpenEdge => &[NAME],
            Engine::PostgreSql => &POSTGRES,
            Engine::MySql => &POSTGRES[..POSTGRES.len() - 1],
        }
    }

    pub fn value(&self, index: usize) -> &str {
        self.values.get(index).map_or("", String::as_str)
    }

    pub fn set_value(&mut self, index: usize, value: &str) -> Result<(), String> {
        let field = self
            .fields()
            .get(index)
            .ok_or("Campo de conexión desconocido")?;
        if value.len() > MAX_VALUE_BYTES || value.chars().any(char::is_control) {
            return Err(format!(
                "{} admite hasta {MAX_VALUE_BYTES} bytes de texto en una línea",
                field.label()
            ));
        }
        value.clone_into(&mut self.values[index]);
        Ok(())
    }

    fn validate(&mut self) -> Result<(), String> {
        for (field, value) in self.fields().iter().zip(&mut self.values) {
            *value = value.trim().to_owned();
            if field.required && value.is_empty() {
                return Err(format!("Completa el campo «{}»", field.label()));
            }
            let valid = match field.key {
                "servidor" => value
                    .bytes()
                    .all(|ch| ch.is_ascii_alphanumeric() || b".-_:[]".contains(&ch)),
                "puerto" => value.parse::<u16>().is_ok_and(|port| port != 0),
                "tls" => SQL_TLS_MODES.iter().any(|(mode, _)| value == mode),
                "credencial" => {
                    value.is_empty()
                        || value.strip_prefix("cide/datadigger/").is_some_and(|id| {
                            !id.is_empty()
                                && id
                                    .bytes()
                                    .all(|ch| ch.is_ascii_alphanumeric() || b"-_.".contains(&ch))
                        })
                }
                "esquemas" => value.split(',').all(|schema| !schema.trim().is_empty()),
                _ => true,
            };
            if !valid {
                return Err(match field.key {
                    "servidor" => "Indica un nombre de servidor o una dirección IP".into(),
                    "puerto" => "El puerto debe estar entre 1 y 65535".into(),
                    "tls" => "TLS admite verificar-servidor, requerido o deshabilitado".into(),
                    "credencial" => {
                        "Usa una referencia cide/datadigger/nombre del almacén seguro".into()
                    }
                    _ => format!("Campo inválido: {}", field.label()),
                });
            }
        }
        Ok(())
    }

    fn to_json(&self) -> Value {
        let fields: Map<String, Value> = self
            .fields()
            .iter()
            .zip(&self.values)
            .map(|(field, value)| (field.key.into(), json!(value)))
            .collect();
        json!({"motor": self.engine.key(), "campos": fields})
    }

    fn from_json(value: &Value) -> Result<Self, String> {
        let object = value
            .as_object()
            .filter(|item| item.len() == 2)
            .ok_or("Perfil de conexión inválido")?;
        let key = object
            .get("motor")
            .and_then(Value::as_str)
            .ok_or("Falta el motor de conexión")?;
        let engine = Engine::ALL
            .into_iter()
            .find(|engine| engine.key() == key)
            .ok_or("Motor de conexión desconocido")?;
        let mut draft = Self::new(engine);
        let fields = object
            .get("campos")
            .and_then(Value::as_object)
            .filter(|fields| fields.len() == draft.fields().len())
            .ok_or("Campos de conexión inválidos")?;
        for (index, field) in draft.fields().iter().enumerate() {
            let value = fields
                .get(field.key)
                .and_then(Value::as_str)
                .ok_or("Campo de conexión inválido")?;
            draft.set_value(index, value)?;
        }
        draft.validate()?;
        Ok(draft)
    }
}

pub struct ProfileStore {
    root: PathBuf,
    session: FileSession,
    profiles: Vec<Draft>,
}

impl ProfileStore {
    pub fn open(root: &Path) -> Result<Self, String> {
        let root = root.canonicalize().map_err(io_error)?;
        if !root.is_dir() {
            return Err("Abre una carpeta de proyecto para configurar conexiones".into());
        }
        let directory = root.join(".cide");
        check_directory(&directory)?;
        let path = directory.join(FILE);
        let mut session =
            match FileSession::open_in_root(&root, &path, Some(TextEncoding::Utf8), MAX_BYTES) {
                Ok(session) => session,
                Err(FileError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
                    FileSession::new(path, TextEncoding::Utf8)
                }
                Err(error) => {
                    return Err(format!(
                        "No se pudo abrir la configuración de conexiones: {error}"
                    ))
                }
            };
        let profiles = if session.is_new() {
            Vec::new()
        } else {
            decode(&session.document().to_bytes())?
        };
        session.document_mut().set_history_bytes_limit(0);
        Ok(Self {
            root,
            session,
            profiles,
        })
    }

    pub fn profiles(&self) -> &[Draft] {
        &self.profiles
    }

    pub fn save(&mut self, mut draft: Draft, editing: Option<usize>) -> Result<(), String> {
        draft.validate()?;
        let name = draft.value(0).to_lowercase();
        if self.profiles.iter().enumerate().any(|(index, profile)| {
            Some(index) != editing && profile.value(0).to_lowercase() == name
        }) {
            return Err("Ya existe una conexión con ese nombre".into());
        }
        let mut next = self.profiles.clone();
        match editing {
            Some(index) => {
                *next
                    .get_mut(index)
                    .ok_or("El perfil seleccionado ya no existe")? = draft
            }
            None if next.len() < MAX_PROFILES => next.push(draft),
            None => {
                return Err(format!(
                    "El proyecto admite hasta {MAX_PROFILES} conexiones"
                ))
            }
        }
        let profiles: Vec<_> = next.iter().map(Draft::to_json).collect();
        let serialized =
            serde_json::to_string_pretty(&json!({"version": 1, "conexiones": profiles}))
                .map_err(|_| "No se pudo serializar la configuración de conexiones")?;
        if serialized.len() > MAX_BYTES {
            return Err("La configuración de conexiones supera el límite de tamaño".into());
        }

        let directory = self.root.join(".cide");
        if !check_directory(&directory)? {
            match fs::create_dir(&directory) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(io_error(error)),
            }
            if !check_directory(&directory)? {
                return Err("La carpeta .cide desapareció antes de guardar".into());
            }
        }
        let len = self.session.document().len();
        self.session
            .replace_text(0..len, &serialized)
            .map_err(|error| error.to_string())?;
        self.session.save().map_err(|error| match error {
            FileError::ExternalModification(_) | FileError::TargetExists(_) => {
                "Otro proceso cambió las conexiones. Vuelve a abrir el menú para recargar la configuración".into()
            }
            _ => format!("No se pudo guardar la configuración de conexiones: {error}"),
        })?;
        self.profiles = next;
        Ok(())
    }
}

fn decode(bytes: &[u8]) -> Result<Vec<Draft>, String> {
    let root: Value = serde_json::from_slice(bytes)
        .map_err(|_| "La configuración de conexiones contiene JSON inválido")?;
    let object = root
        .as_object()
        .filter(|object| object.len() == 2 && object.get("version") == Some(&json!(1)))
        .ok_or("Versión de configuración de conexiones desconocida")?;
    let list = object
        .get("conexiones")
        .and_then(Value::as_array)
        .filter(|list| list.len() <= MAX_PROFILES)
        .ok_or("Lista de conexiones inválida")?;
    let mut names = std::collections::HashSet::new();
    list.iter()
        .map(|value| {
            let draft = Draft::from_json(value)?;
            if !names.insert(draft.value(0).to_lowercase()) {
                return Err("La configuración contiene nombres de conexión duplicados".into());
            }
            Ok(draft)
        })
        .collect()
}

fn check_directory(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(meta) if normal_directory(&meta) => Ok(true),
        Ok(_) => {
            Err("La carpeta .cide debe estar dentro del proyecto y ser un directorio normal".into())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(io_error(error)),
    }
}

fn normal_directory(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return false;
        }
    }
    meta.is_dir()
}

fn io_error(error: io::Error) -> String {
    format!("No se pudo acceder a la configuración de conexiones: {error}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../work/test-temp");
            fs::create_dir_all(&base).unwrap();
            let path = base.join(format!(
                "conexiones-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn settings(&self) -> PathBuf {
            self.0.join(".cide").join(FILE)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn profile(engine: Engine, name: &str) -> Draft {
        let mut draft = Draft::new(engine);
        draft.set_value(0, name).unwrap();
        if engine != Engine::OpenEdge {
            draft.set_value(3, "demo").unwrap();
            draft.set_value(4, "lector").unwrap();
            draft.set_value(6, "cide/datadigger/demo").unwrap();
        }
        draft
    }

    #[test]
    fn data_connections_reopen_three_engines_and_edit_one_without_touching_progress() {
        let fixture = Fixture::new();
        let mut store = ProfileStore::open(&fixture.0).unwrap();
        assert!(store.profiles().is_empty());
        assert!(!fixture.settings().exists());
        store
            .save(profile(Engine::OpenEdge, "Sports2000"), None)
            .unwrap();
        let progress = fixture.0.join(".cide/progress.local.ini");
        fs::write(&progress, b"perfil existente\n").unwrap();
        store
            .save(profile(Engine::PostgreSql, "Catálogo"), None)
            .unwrap();
        store
            .save(profile(Engine::MySql, "Inventario"), None)
            .unwrap();
        let mut store = ProfileStore::open(&fixture.0).unwrap();
        assert_eq!(
            store
                .profiles()
                .iter()
                .map(Draft::engine)
                .collect::<Vec<_>>(),
            Engine::ALL
        );
        assert_eq!(store.profiles()[0].fields().len(), 1);
        assert_eq!(store.profiles()[1].value(7), "public");
        assert_eq!(store.profiles()[2].value(2), "3306");
        store
            .save(profile(Engine::PostgreSql, "Pruebas"), Some(1))
            .unwrap();
        assert_eq!(
            ProfileStore::open(&fixture.0).unwrap().profiles()[1].value(0),
            "Pruebas"
        );
        assert_eq!(fs::read(progress).unwrap(), b"perfil existente\n");
    }

    #[test]
    fn data_connections_reject_duplicates_invalid_ports_and_plain_credentials() {
        let fixture = Fixture::new();
        let mut store = ProfileStore::open(&fixture.0).unwrap();
        let mut draft = profile(Engine::PostgreSql, "Demo");
        draft.set_value(2, "65536").unwrap();
        assert!(store
            .save(draft.clone(), None)
            .unwrap_err()
            .contains("puerto"));
        draft.set_value(2, "5432").unwrap();
        draft.set_value(6, "contraseña-sintética").unwrap();
        assert!(store
            .save(draft, None)
            .unwrap_err()
            .contains("almacén seguro"));
        assert!(!fixture.settings().exists());
        store.save(profile(Engine::OpenEdge, "DÉMO"), None).unwrap();
        assert!(store.save(profile(Engine::MySql, "dÉmo"), None).is_err());
        assert_eq!(store.profiles().len(), 1);
    }

    #[test]
    fn data_connections_reject_stale_writes_and_preserve_invalid_bytes() {
        let fixture = Fixture::new();
        let mut first = ProfileStore::open(&fixture.0).unwrap();
        let mut stale_creation = ProfileStore::open(&fixture.0).unwrap();
        first
            .save(profile(Engine::OpenEdge, "Inicial"), None)
            .unwrap();
        assert!(stale_creation
            .save(profile(Engine::OpenEdge, "Tardío"), None)
            .is_err());
        let mut stale_update = ProfileStore::open(&fixture.0).unwrap();
        let invalid = b"{\"version\": 99}";
        fs::write(fixture.settings(), invalid).unwrap();
        assert!(stale_update
            .save(profile(Engine::OpenEdge, "Cambio"), Some(0))
            .is_err());
        assert!(ProfileStore::open(&fixture.0).is_err());
        assert_eq!(fs::read(fixture.settings()).unwrap(), invalid);
    }
}
