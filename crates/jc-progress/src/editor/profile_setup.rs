//! Edit the same project configuration that the ABL providers consume.
use super::{
    avm_path, decode, invoke, parse_aliases, valid_logical_name, EditorProfile, JobDir,
    ProgressProfile, PROFILE,
};
use crate::{Action, BridgeRequest, CompletionStatus, SchemaRecord};
use jc_app::Cancellation;
use jc_files::{ExternalState, FileError, FileSession};
use jc_text::TextEncoding;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

const MAX_PROFILE_BYTES: usize = 8192;
pub const OPENEDGE_VALUE_BYTES: usize = 1024;
const KEYS: [&str; 7] = [
    "executable",
    "cpinternal",
    "cpstream",
    "pf",
    "ini",
    "version",
    "aliases",
];
const PF: usize = 3;
const DB: usize = 7;
const LOGICAL: usize = 8;
pub const OPENEDGE_FIELDS: [(&str, &str); 9] = [
    (
        "Ejecutable AVM",
        "Elige _progres.exe del entorno OpenEdge de desarrollo.",
    ),
    (
        "Codificación interna",
        "Página de códigos de los datos y diagnósticos, por ejemplo iso8859-1.",
    ),
    (
        "Codificación de salida",
        "Página de códigos usada por el AVM en la salida, por ejemplo ibm850.",
    ),
    (
        "Archivo .pf existente",
        "Opcional. Usa un .pf de confianza o deja este campo vacío para generar uno.",
    ),
    (
        "Archivo .ini del AVM",
        "Opcional. Archivo de inicio de OpenEdge, distinto del perfil de CIDE.",
    ),
    (
        "Versión de OpenEdge",
        "Opcional. Valor exacto de PROVERSION, por ejemplo 11.7 o 10.0B.",
    ),
    (
        "Aliases",
        "Opcional. Usa alias:base_logica; separa los pares con comas.",
    ),
    (
        "Base local .db",
        "Opcional. Elige una copia cerrada para generar un .pf con -1 y -RO.",
    ),
    (
        "Nombre lógico de la base",
        "Opcional para la copia local. Por defecto se usa el nombre del archivo .db.",
    ),
];

pub struct ProfileSetup {
    root: PathBuf,
    session: FileSession,
    values: [String; OPENEDGE_FIELDS.len()],
    comments: String,
}

impl ProfileSetup {
    pub fn open(root: &Path) -> Result<Self, String> {
        let root = root.canonicalize().map_err(|error| error.to_string())?;
        let directory = root.join(".cide");
        ordinary_directory(&directory)?;
        let path = root.join(PROFILE);
        let mut session = match FileSession::open_in_root(
            &root,
            &path,
            Some(TextEncoding::Utf8),
            MAX_PROFILE_BYTES,
        ) {
            Ok(session) => session,
            Err(FileError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
                FileSession::new(path, TextEncoding::Utf8)
            }
            Err(error) => return Err(format!("No se pudo abrir el perfil OpenEdge: {error}")),
        };
        let bytes = session.document().to_bytes();
        let fields = parse(&bytes)?;
        let mut values: [String; OPENEDGE_FIELDS.len()] = std::array::from_fn(|_| String::new());
        let defaults = [
            std::env::var_os("DLC")
                .map(PathBuf::from)
                .map(|dlc| {
                    dlc.join(if cfg!(windows) {
                        "bin/_progres.exe"
                    } else {
                        "bin/_progres"
                    })
                })
                .filter(|path| path.is_absolute() && path.is_file())
                .and_then(|path| path.to_str().map(str::to_owned))
                .unwrap_or_default(),
            "iso8859-1".into(),
            "ibm850".into(),
        ];
        for (index, value) in defaults.into_iter().enumerate() {
            values[index] = value;
        }
        for (index, value) in fields.into_iter().enumerate() {
            if let Some(value) = value {
                values[index] = value.into();
            }
        }
        let comments = std::str::from_utf8(&bytes)
            .expect("validado al leer el perfil")
            .trim_start_matches('\u{feff}')
            .lines()
            .filter(|line| line.trim_start().starts_with('#'))
            .map(|line| format!("{line}\n"))
            .collect();
        session.document_mut().set_history_bytes_limit(0);
        Ok(Self {
            root,
            session,
            values,
            comments,
        })
    }

    pub fn value(&self, index: usize) -> &str {
        &self.values[index]
    }

    pub fn set_value(&mut self, index: usize, value: &str) -> Result<(), String> {
        check_value(index, value)?;
        value.clone_into(&mut self.values[index]);
        Ok(())
    }

    pub fn save(&mut self) -> Result<(), String> {
        let mut fields: [Option<&str>; KEYS.len()] =
            std::array::from_fn(|index| nonempty(&self.values[index]));
        let (_, parameters) = validate_draft(&self.root, &fields, &self.values)?;
        if self.session.external_state().map_err(save_error)? != ExternalState::Unchanged {
            return Err(conflict());
        }
        let directory = self.root.join(".cide");
        create_directory(&directory)?;
        let generated = parameters
            .as_deref()
            .map(|parameters| publish_parameters(&self.root, parameters))
            .transpose()?;
        if let Some(path) = generated.as_deref() {
            fields[PF] = Some(path);
        }
        let mut text = self.comments.clone();
        for (key, value) in KEYS.iter().zip(fields) {
            if let Some(value) = value {
                text.push_str(&format!("{key}={value}\n"));
            }
        }
        if text.len() > MAX_PROFILE_BYTES {
            return Err("El perfil OpenEdge supera el límite de tamaño".into());
        }
        let len = self.session.document().len();
        self.session
            .replace_text(0..len, &text)
            .map_err(save_error)?;
        self.session.save().map_err(save_error)
    }

    pub fn test_request(&self) -> OpenEdgeTest {
        OpenEdgeTest {
            root: self.root.clone(),
            values: self.values.clone(),
        }
    }
}

pub struct OpenEdgeTest {
    root: PathBuf,
    values: [String; OPENEDGE_FIELDS.len()],
}

impl OpenEdgeTest {
    pub fn new(root: &Path, values: &[String]) -> Result<Self, String> {
        let values: &[String; OPENEDGE_FIELDS.len()] = values
            .try_into()
            .map_err(|_| "Campos OpenEdge incompletos")?;
        for (index, value) in values.iter().enumerate() {
            check_value(index, value)?;
        }
        Ok(Self {
            root: root.to_owned(),
            values: values.clone(),
        })
    }

    pub fn run(self, cancel: &Cancellation) -> Result<String, String> {
        let fields = std::array::from_fn(|index| nonempty(&self.values[index]));
        let (mut profile, parameters) = validate_draft(&self.root, &fields, &self.values)?;
        let job = JobDir::create()
            .map_err(|error| format!("No se pudo preparar la prueba OpenEdge: {error}"))?;
        let directory = avm_path(&job.path);
        if let Some(parameters) = parameters {
            let path = directory.join("prueba.pf");
            fs::write(&path, parameters)
                .map_err(|error| format!("No se pudo preparar el .pf temporal: {error}"))?;
            profile.avm.parameter_file = Some(path);
        }
        let result = invoke(
            BridgeRequest {
                action: Action::Probe,
                source: None,
                output_dir: directory,
                propath: None,
            },
            &self.root,
            &profile,
            cancel,
        )?;
        let response = result.response.map_err(|_| "No se pudo conectar con OpenEdge. Revisa la base, los parámetros y que el AVM pueda iniciar." )?;
        if response.status != CompletionStatus::Ok || !result.status.success() {
            let detail = response
                .diagnostics
                .first()
                .map(|diag| decode(diag.message.as_bytes(), &response.cpinternal));
            return Err(detail.unwrap_or_else(|| "El AVM no pudo comprobar la conexión.".into()));
        }
        let databases = response
            .schema
            .iter()
            .filter_map(|record| match record {
                SchemaRecord::Database { name } => {
                    Some(decode(name.as_bytes(), &response.cpinternal))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        if databases.is_empty() {
            return Err("OpenEdge inició, pero no informó bases conectadas.".into());
        }
        let version = response.avm_version.unwrap_or_default();
        Ok(format!(
            "Conexión correcta a OpenEdge {version} · {}",
            databases.join(", ")
        ))
    }
}

fn check_value(index: usize, value: &str) -> Result<(), String> {
    if value.len() > OPENEDGE_VALUE_BYTES || value.chars().any(char::is_control) {
        return Err(format!(
            "{} admite hasta {OPENEDGE_VALUE_BYTES} bytes en una línea",
            OPENEDGE_FIELDS[index].0
        ));
    }
    Ok(())
}

fn validate_draft(
    root: &Path,
    fields: &[Option<&str>; KEYS.len()],
    values: &[String; OPENEDGE_FIELDS.len()],
) -> Result<(EditorProfile, Option<String>), String> {
    let local = nonempty(&values[DB]);
    if local.is_some() && fields[PF].is_some() {
        return Err("Elige un .pf existente o una base local .db. Vacía el otro campo.".into());
    }
    if local.is_none() && nonempty(&values[LOGICAL]).is_some() {
        return Err("El nombre lógico se usa al generar el .pf para una base local.".into());
    }
    let resolved = resolve(root, fields)?;
    crate::path_line(&resolved.avm.executable, "executable").map_err(|error| error.to_string())?;
    if !resolved.avm.executable.is_file() {
        return Err("El ejecutable AVM debe ser un archivo".into());
    }
    for (key, path) in [
        ("pf", resolved.avm.parameter_file.as_deref()),
        ("ini", resolved.avm.ini_file.as_deref()),
    ] {
        if let Some(path) = path {
            crate::path_line(path, key).map_err(|error| error.to_string())?;
            if !path.is_file() {
                return Err(format!("No se encontró el archivo .{key} indicado"));
            }
        }
    }
    for (key, page) in [
        ("cpinternal", &resolved.avm.cpinternal),
        ("cpstream", &resolved.avm.cpstream),
    ] {
        crate::code_page(page, key)
            .map_err(|_| format!("Indica una página de códigos de OpenEdge válida para {key}"))?;
    }
    let parameters = local
        .map(|path| local_parameters(root, path, nonempty(&values[LOGICAL])))
        .transpose()?;
    Ok((resolved, parameters))
}

pub(super) fn parse(bytes: &[u8]) -> Result<[Option<&str>; KEYS.len()], String> {
    let text = std::str::from_utf8(bytes).map_err(|_| format!("{PROFILE} debe estar en UTF-8"))?;
    let mut fields = [None; KEYS.len()];
    for (index, line) in text.trim_start_matches('\u{feff}').lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let invalid = || format!("{PROFILE}: entrada inválida en línea {}", index + 1);
        let (key, value) = line.split_once('=').ok_or_else(invalid)?;
        let index = KEYS
            .iter()
            .position(|known| *known == key.trim())
            .ok_or_else(invalid)?;
        let value = nonempty(value).ok_or_else(invalid)?;
        if value.chars().any(char::is_control) || fields[index].replace(value).is_some() {
            return Err(invalid());
        }
    }
    Ok(fields)
}

pub(super) fn resolve(
    root: &Path,
    fields: &[Option<&str>; KEYS.len()],
) -> Result<EditorProfile, String> {
    let [executable, internal, stream, pf, ini, version, aliases] = *fields;
    let required = |value: Option<&str>, key| {
        value
            .map(str::to_owned)
            .ok_or_else(|| format!("Falta {key} en {PROFILE}"))
    };
    if version.is_some_and(|value| !crate::valid_avm_version(value)) {
        return Err("Versión esperada de OpenEdge inválida".into());
    }
    let aliases = aliases.map(parse_aliases).transpose()?.unwrap_or_default();
    let executable = root
        .join(required(executable, "executable")?)
        .canonicalize()
        .map_err(|error| format!("Ejecutable Progress: {error}"))?;
    Ok(EditorProfile {
        avm: ProgressProfile {
            executable: avm_path(&executable),
            parameter_file: pf.map(|value| avm_path(&root.join(value))),
            ini_file: ini.map(|value| avm_path(&root.join(value))),
            cpinternal: required(internal, "cpinternal")?,
            cpstream: required(stream, "cpstream")?,
        },
        aliases,
        version: version.map(str::to_owned),
    })
}

fn nonempty(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

fn local_parameters(root: &Path, path: &str, logical: Option<&str>) -> Result<String, String> {
    let path = root.join(path);
    if !path
        .extension()
        .is_some_and(|value| value.eq_ignore_ascii_case("db"))
        || !path.is_file()
    {
        return Err("Selecciona un archivo .db de una copia local cerrada".into());
    }
    let path = avm_path(&path.canonicalize().map_err(|error| error.to_string())?);
    #[cfg(windows)]
    if !matches!(path.components().next(), Some(std::path::Component::Prefix(prefix)) if matches!(prefix.kind(), std::path::Prefix::Disk(_)))
    {
        return Err("Selecciona una base .db en un disco local".into());
    }
    let logical = logical
        .or_else(|| path.file_stem().and_then(std::ffi::OsStr::to_str))
        .unwrap_or_default();
    if !valid_logical_name(logical) {
        return Err("Indica un nombre lógico con hasta 32 letras ASCII, números, guiones o guiones bajos. Empieza con una letra.".into());
    }
    let path = path
        .to_str()
        .filter(|path| {
            path.bytes()
                .all(|ch| matches!(ch, 0x20..=0x7e) && ch != b'"')
        })
        .ok_or("El .pf generado requiere una ruta .db ASCII sin comillas")?;
    let path = if cfg!(windows) {
        path.replace('\\', "/")
    } else {
        path.to_owned()
    };
    Ok(format!("-db \"{path}\" -ld {logical} -1 -RO\n"))
}

fn publish_parameters(root: &Path, parameters: &str) -> Result<String, String> {
    // Keep previous parameter files valid for running jobs and concurrent editors.
    for index in 0..128 {
        let relative = if index == 0 {
            ".cide/openedge.local.pf".to_owned()
        } else {
            format!(".cide/openedge-{index}.local.pf")
        };
        let path = root.join(&relative);
        if let Ok((_, old)) = jc_files::read_in_root(root, &path, 4096) {
            if old == parameters.as_bytes() {
                return Ok(relative);
            }
            continue;
        }
        let mut file = FileSession::new(path, TextEncoding::Utf8);
        file.replace_text(0..0, parameters).map_err(save_error)?;
        match file.save() {
            Ok(()) => return Ok(relative),
            Err(FileError::TargetExists(_) | FileError::ExternalModification(_)) => continue,
            Err(error) => return Err(save_error(error)),
        }
    }
    Err("No queda un nombre libre para generar el .pf. Puedes seleccionar uno existente.".into())
}

fn ordinary_directory(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => {
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if metadata.file_attributes() & 0x400 != 0 {
                    return Err("La carpeta .cide debe ser un directorio normal".into());
                }
            }
            Ok(true)
        }
        Ok(_) => Err("La carpeta .cide debe ser un directorio normal".into()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.to_string()),
    }
}

fn create_directory(directory: &Path) -> Result<(), String> {
    if !ordinary_directory(directory)? {
        match fs::create_dir(directory) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.to_string()),
        }
        if !ordinary_directory(directory)? {
            return Err("La carpeta .cide desapareció antes de guardar".into());
        }
    }
    Ok(())
}

fn conflict() -> String {
    "Otro proceso cambió el perfil OpenEdge. Cierra el asistente y vuelve a abrirlo.".into()
}

fn save_error(error: FileError) -> String {
    match error {
        FileError::ExternalModification(_) | FileError::TargetExists(_) => conflict(),
        _ => format!("No se pudo guardar el perfil OpenEdge: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::{load_profile, JobDir};

    #[test]
    fn connections_wizard_generates_a_read_only_pf_used_by_the_abl_provider() {
        let fixture = JobDir::create().unwrap();
        let avm = fixture.path.join("_progres.exe");
        fs::write(&avm, "AVM fixture").unwrap();
        let database = fixture.path.join("base demo.db");
        fs::write(&database, "database fixture").unwrap();
        let mut setup = ProfileSetup::open(&fixture.path).unwrap();
        let mut stale = ProfileSetup::open(&fixture.path).unwrap();
        for (index, value) in [
            (0, "_progres.exe"),
            (DB, "base demo.db"),
            (LOGICAL, "sports2000"),
            (5, "11.7"),
            (6, "demo:sports2000"),
        ] {
            setup.set_value(index, value).unwrap();
        }
        stale.set_value(0, "_progres.exe").unwrap();
        setup.save().unwrap();
        assert!(stale.save().unwrap_err().contains("Otro proceso"));

        let profile = load_profile(&fixture.path).unwrap();
        assert_eq!(profile.version.as_deref(), Some("11.7"));
        assert_eq!(
            profile.avm.executable,
            avm_path(&avm.canonicalize().unwrap())
        );
        assert_eq!(profile.aliases[0].database, "sports2000");
        let pf = profile.avm.parameter_file.unwrap();
        let bytes = fs::read_to_string(&pf).unwrap();
        assert!(bytes.contains("base demo.db\" -ld sports2000 -1 -RO"));
        assert_eq!(fs::read(&database).unwrap(), b"database fixture");
        let mut reopened = ProfileSetup::open(&fixture.path).unwrap();
        assert_eq!(reopened.value(PF), ".cide/openedge.local.pf");
        assert_eq!(reopened.value(1), "iso8859-1");
        assert_eq!(reopened.value(2), "ibm850");
        reopened.set_value(DB, "base demo.db").unwrap();
        assert!(reopened.save().unwrap_err().contains("Vacía el otro campo"));
        assert_eq!(fs::read_to_string(&pf).unwrap(), bytes);
    }

    #[test]
    fn connections_wizard_preserves_an_existing_pf_and_profile_comments() {
        let fixture = JobDir::create().unwrap();
        let config = fixture.path.join(".cide");
        fs::create_dir(&config).unwrap();
        fs::write(fixture.path.join("avm.exe"), "AVM").unwrap();
        let pf = config.join("dev.local.pf");
        fs::write(&pf, "# parámetros existentes\n").unwrap();
        fs::write(fixture.path.join(PROFILE), "# Configuración de desarrollo\nexecutable=avm.exe\npf=.cide/dev.local.pf\ncpinternal=iso8859-1\ncpstream=ibm850\n").unwrap();
        let mut setup = ProfileSetup::open(&fixture.path).unwrap();
        setup.set_value(2, "utf-8").unwrap();
        setup.save().unwrap();
        let text = fs::read_to_string(fixture.path.join(PROFILE)).unwrap();
        assert!(text.contains("# Configuración de desarrollo\n"));
        assert!(text.contains("cpstream=utf-8\n"));
        assert_eq!(fs::read_to_string(pf).unwrap(), "# parámetros existentes\n");
    }

    #[test]
    #[ignore = "requiere CIDE_TEST_PROGRESS_EXE y CIDE_TEST_PROGRESS_SETUP_DB con una copia cerrada de Sports2000"]
    fn connections_wizard_real_schema_from_generated_pf() {
        use crate::editor::SchemaController;
        use jc_app::{Wakeup, Workbench};
        use std::{
            thread,
            time::{Duration, Instant},
        };

        let executable = std::env::var("CIDE_TEST_PROGRESS_EXE").unwrap();
        let database = std::env::var("CIDE_TEST_PROGRESS_SETUP_DB").unwrap();
        let fixture = JobDir::create().unwrap();
        let mut setup = ProfileSetup::open(&fixture.path).unwrap();
        for (index, value) in [
            (0, executable.as_str()),
            (DB, database.as_str()),
            (LOGICAL, "sports2000"),
        ] {
            setup.set_value(index, value).unwrap();
        }
        let limit = Cancellation::new(Duration::from_secs(15));
        assert!(!fixture.path.join(".cide").exists());
        let result = setup.test_request().run(&limit).unwrap();
        assert!(
            result.starts_with("Conexión correcta a OpenEdge"),
            "{result}"
        );
        assert!(result.contains("sports2000"), "{result}");
        assert!(!fixture.path.join(".cide").exists());
        setup.set_value(6, "demo:inexistente").unwrap();
        assert!(setup
            .test_request()
            .run(&limit)
            .unwrap_err()
            .contains("inexistente"));
        setup.set_value(6, "").unwrap();
        assert!(!fixture.path.join(".cide").exists());
        setup.save().unwrap();
        let mut schema = SchemaController::default();
        let mut app = Workbench::default();
        schema
            .request(Some(&fixture.path), Wakeup::default())
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(35);
        loop {
            if let Some(update) = schema.poll(Some(&fixture.path), &mut app) {
                assert!(!update.problems, "{:?}", app.problems());
                assert!(schema.has_catalog());
                assert!(app
                    .output()
                    .iter()
                    .any(|row| row.text.to_ascii_lowercase().contains("customer")));
                break;
            }
            assert!(
                Instant::now() < deadline,
                "El AVM no terminó la consulta del esquema"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }
}
