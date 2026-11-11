//! Explicit batch AVM jobs shared by both editors.
#[cfg(test)]
mod tests;
use crate::{
    build_command, parse_response, Action, BridgeRequest, CompletionStatus, ProgressProfile,
    Severity,
};
use jc_app::{
    Cancellation, CommandId, CompletedTask, JobKey, JobSystem, TaskFailure, TaskItem, TaskOutput,
    TaskSnapshot, TaskStatus, Workbench,
};
use jc_files::read_in_root;
use jc_text::{DecodedScalars, TextEncoding};
use std::{
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Child, ExitStatus, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::Duration,
};

pub const JOB_KEY: JobKey = JobKey(0x4a43_5052_4f47_0001);
const PROFILE: &str = ".cide/progress.local.ini";
const OUTPUT_LIMIT: u64 = 128 * 1024;
const HELPER: &[u8] = include_bytes!("../../../progress/abl/jc-bridge.p");

pub fn action(command: CommandId) -> Option<Action> {
    match command {
        CommandId::ProgressCheck => Some(Action::Check),
        CommandId::ProgressCompile => Some(Action::Compile),
        CommandId::ProgressRun => Some(Action::Run),
        _ => None,
    }
}

pub fn cancel(jobs: &mut JobSystem) -> &'static str {
    if !jobs.has_job(JOB_KEY) {
        return "No hay tarea ABL en curso";
    }
    jobs.cancel(JOB_KEY);
    "Cancelando tarea ABL…"
}

pub fn completion(task: &CompletedTask) -> (String, bool) {
    if task.status != TaskStatus::Completed {
        return (format!("ABL · {}", task.status), false);
    }
    if !task.current || !task.applicable {
        return ("ABL · resultado obsoleto".into(), false);
    }
    let problems = task
        .output
        .items()
        .iter()
        .any(|item| matches!(item, TaskItem::Problem(_) | TaskItem::Diagnostic { .. }));
    (
        if problems {
            "ABL · consulta Problemas"
        } else {
            "ABL · consulta Salida"
        }
        .into(),
        problems,
    )
}

pub fn submit(
    action: Action,
    app: &Workbench,
    jobs: &mut JobSystem,
    root: Option<&Path>,
) -> Result<(), String> {
    let root = root
        .ok_or("Abre primero una carpeta de proyecto Progress")?
        .to_owned();
    let doc = app.active_document().ok_or("Abre un archivo ABL")?;
    let source = doc.session.path().to_owned();
    let extension = source
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    if action == Action::Schema
        || !["p", "w", "cls"]
            .iter()
            .any(|ext| extension.eq_ignore_ascii_case(ext))
        || (action == Action::Run && !extension.eq_ignore_ascii_case("p"))
    {
        return Err(
            "Compilar admite .p, .w y .cls; Ejecutar requiere un procedimiento .p por lotes".into(),
        );
    }
    if doc.session.document().is_modified() || !source.is_absolute() {
        return Err("Guarda el archivo ABL antes de iniciar Progress".into());
    }
    if jobs.has_job(JOB_KEY) {
        return Err("Ya hay una tarea Progress en curso".into());
    }
    let label = match action {
        Action::Check => "Comprobación ABL",
        Action::Compile => "Compilación ABL",
        Action::Run => "Ejecución ABL",
        Action::Schema => unreachable!(),
    };
    jobs.submit_document(
        app,
        JOB_KEY,
        doc.id,
        0..doc.session.document().len(),
        Duration::from_secs(30),
        move |snapshot, cancel, output| {
            output.push(TaskItem::Message(format!("{label} · {}", source.display())))?;
            match execute(action, &root, &source, snapshot, cancel, output) {
                Ok(()) => Ok(()),
                Err(_) if cancel.is_cancelled() => Err(TaskFailure::Cancelled),
                Err(error) => output.push(TaskItem::Problem(format!("{label}: {error}"))),
            }
        },
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

fn load_profile(root: &Path) -> Result<ProgressProfile, String> {
    let (_, bytes) = read_in_root(root, &root.join(PROFILE), 8192).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            format!("Configura {PROFILE} para usar el AVM del proyecto")
        } else {
            format!("No se pudo leer {PROFILE}: {error}")
        }
    })?;
    let text = std::str::from_utf8(&bytes).map_err(|_| format!("{PROFILE} debe estar en UTF-8"))?;
    let mut executable = None;
    let mut parameter_file = None;
    let mut ini_file = None;
    let mut cpstream = None;
    let mut cpinternal = None;
    for (index, line) in text.trim_start_matches('\u{feff}').lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let invalid = || format!("{PROFILE}: entrada inválida en línea {}", index + 1);
        let (key, value) = line.split_once('=').ok_or_else(invalid)?;
        let value = value.trim();
        if value.is_empty() || value.chars().any(char::is_control) {
            return Err(invalid());
        }
        let repeated = match key.trim() {
            "executable" => executable.replace(root.join(value)).is_some(),
            "pf" => parameter_file.replace(root.join(value)).is_some(),
            "ini" => ini_file.replace(root.join(value)).is_some(),
            "cpstream" => cpstream.replace(value.to_owned()).is_some(),
            "cpinternal" => cpinternal.replace(value.to_owned()).is_some(),
            _ => return Err(invalid()),
        };
        if repeated {
            return Err(invalid());
        }
    }
    Ok(ProgressProfile {
        executable: executable.ok_or(format!("Falta executable en {PROFILE}"))?,
        parameter_file: parameter_file.map(|p| avm_path(&p)),
        ini_file: ini_file.map(|p| avm_path(&p)),
        cpstream: cpstream.ok_or(format!("Falta cpstream en {PROFILE}"))?,
        cpinternal: cpinternal.ok_or(format!("Falta cpinternal en {PROFILE}"))?,
    })
}

fn avm_path(path: &Path) -> PathBuf {
    if let Some(path) = path.to_str() {
        if let Some(unc) = path.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{unc}"));
        }
        if let Some(disk) = path.strip_prefix(r"\\?\") {
            if disk.as_bytes().get(1) == Some(&b':') {
                return PathBuf::from(disk);
            }
        }
    }
    path.to_owned()
}

fn project_propath(root: &Path, source: &Path) -> Result<String, String> {
    let parents = jc_app::source_services::load_propath(root)?.unwrap_or_else(|| {
        let parent = source.parent().unwrap_or(root);
        if parent == root {
            vec![root.to_owned()]
        } else {
            vec![parent.to_owned(), root.to_owned()]
        }
    });
    if parents.is_empty() {
        return Err("El PROPATH explícito del proyecto está vacío".into());
    }
    parents
        .into_iter()
        .map(|path| {
            let resolved = path
                .canonicalize()
                .map_err(|error| format!("PROPATH {}: {error}", path.display()))?;
            if !resolved.starts_with(root) || !resolved.is_dir() {
                return Err(format!(
                    "Directorio PROPATH fuera del proyecto o inválido: {}",
                    path.display()
                ));
            }
            let path = avm_path(&resolved);
            match path.to_str() {
                Some(value) if !value.contains(',') => Ok(value.to_owned()),
                _ => Err(format!("Ruta incompatible con PROPATH: {}", path.display())),
            }
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|dirs| dirs.join(","))
}

struct JobDir {
    path: PathBuf,
    keep: bool,
}
impl JobDir {
    fn create() -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let parent = std::env::temp_dir().canonicalize()?;
        loop {
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = parent.join(format!("cide-abl-{}-{id}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path, keep: false }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
    }
}
impl Drop for JobDir {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

fn verify_source(root: &Path, path: &Path, expected: &[u8]) -> Result<PathBuf, String> {
    let (source, bytes) = read_in_root(root, path, expected.len()).map_err(|error| {
        format!("No se pudo verificar el archivo guardado dentro del proyecto: {error}")
    })?;
    if bytes != expected {
        return Err(
            "El archivo en disco cambió; vuelve a abrirlo o guárdalo antes de compilar".into(),
        );
    }
    Ok(source)
}

fn execute(
    action: Action,
    root: &Path,
    path: &Path,
    snapshot: &TaskSnapshot,
    cancel: &Cancellation,
    output: &mut TaskOutput,
) -> Result<(), String> {
    let source = verify_source(root, path, snapshot.bytes())?;
    let mut profile = load_profile(root)?;
    profile.executable = avm_path(
        &profile
            .executable
            .canonicalize()
            .map_err(|e| format!("Ejecutable Progress: {e}"))?,
    );
    let propath = project_propath(root, &source)?;
    let mut job = JobDir::create()
        .map_err(|e| format!("No se pudo crear el directorio de trabajo ABL: {e}"))?;
    let result = invoke(action, root, &source, &profile, propath, &job.path, cancel)?;
    verify_source(root, path, snapshot.bytes())?;
    if cancel.is_cancelled() {
        return Err("Tarea Progress cancelada".into());
    }
    let success = publish(root, &source, snapshot, result, output)
        .map_err(|_| "La salida supera el límite del panel")?;
    if success {
        let message = match action {
            Action::Check => "Sintaxis ABL comprobada".into(),
            Action::Compile => format!("Código r generado en {}", job.path.display()),
            Action::Run => "Ejecución ABL finalizada".into(),
            Action::Schema => unreachable!(),
        };
        output
            .push(TaskItem::Message(message))
            .map_err(|_| "La salida supera el límite del panel")?;
        job.keep = action == Action::Compile;
    }
    Ok(())
}

struct AvmResult {
    status: ExitStatus,
    log: Vec<u8>,
    response: Result<crate::BridgeResponse, String>,
    cpstream: String,
}

fn invoke(
    action: Action,
    root: &Path,
    source: &Path,
    profile: &ProgressProfile,
    propath: String,
    dir: &Path,
    cancel: &Cancellation,
) -> Result<AvmResult, String> {
    let dir = avm_path(dir);
    let helper = dir.join("jc-bridge.p");
    let request_path = dir.join("request.txt");
    let log = dir.join("avm.txt");
    let request = BridgeRequest {
        action,
        source: Some(avm_path(source)),
        output_dir: dir.clone(),
        propath: Some(propath),
    };
    let spec = build_command(profile, &helper, &request_path).map_err(|e| e.to_string())?;
    let data = request.encode().map_err(|e| e.to_string())?;
    let start = || -> io::Result<Child> {
        fs::write(&helper, HELPER)?;
        fs::write(&request_path, data)?;
        let log = File::create(&log)?;
        let mut command = spec.to_command();
        command
            .current_dir(avm_path(root))
            .arg("-T")
            .arg(&dir)
            .env_remove("PROPATH")
            .env_remove("PROSTARTUP")
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log);
        if let Some(dlc) = profile.executable.parent().and_then(Path::parent) {
            command.env("DLC", dlc);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        if cancel.is_cancelled() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Tarea cancelada",
            ));
        }
        command.spawn()
    };
    let mut child = start().map_err(|e| format!("No se pudo iniciar el AVM: {e}"))?;
    let response = request.response_path();
    let status = match wait(&mut child, [&log, &response], cancel) {
        Ok(status) => status,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    Ok(AvmResult {
        status,
        log: bounded_read(&log)?,
        response: bounded_read(&response).and_then(|bytes| {
            parse_response(&bytes).map_err(|error| format!("Respuesta del AVM inválida: {error}"))
        }),
        cpstream: profile.cpstream.clone(),
    })
}

fn wait(child: &mut Child, files: [&Path; 2], cancel: &Cancellation) -> Result<ExitStatus, String> {
    loop {
        if cancel.is_cancelled() {
            return Err("Tarea Progress cancelada".into());
        }
        if files
            .iter()
            .any(|path| fs::metadata(path).is_ok_and(|m| m.len() > OUTPUT_LIMIT))
        {
            return Err("El AVM superó la cuota de salida (128 KiB por archivo)".into());
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|e| format!("No se pudo esperar al AVM: {e}"))?
        {
            return Ok(status);
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn bounded_read(path: &Path) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|f| f.take(OUTPUT_LIMIT + 1).read_to_end(&mut bytes))
        .map_err(|e| {
            format!(
                "No se pudo leer {}: {e}",
                path.file_name().unwrap_or_default().to_string_lossy()
            )
        })?;
    if bytes.len() as u64 > OUTPUT_LIMIT {
        return Err("El AVM superó la cuota de salida (128 KiB por archivo)".into());
    }
    Ok(bytes)
}

fn decode(bytes: &[u8], cp: &str) -> String {
    let codec = match cp.to_ascii_lowercase().as_str() {
        "iso8859-1" | "iso-8859-1" => TextEncoding::Iso8859_1,
        "1252" | "windows-1252" => TextEncoding::Windows1252,
        "ibm850" | "850" => TextEncoding::Ibm850,
        _ => TextEncoding::Utf8,
    };
    DecodedScalars::new(bytes, codec, 0)
        .map(|s| match s.character {
            '\n' | '\r' | '\t' => s.character,
            c if c.is_control() => '\u{fffd}',
            c => c,
        })
        .collect()
}

fn publish(
    root: &Path,
    source: &Path,
    snapshot: &TaskSnapshot,
    result: AvmResult,
    output: &mut TaskOutput,
) -> Result<bool, TaskFailure> {
    let cpstream = result
        .response
        .as_ref()
        .map_or(&result.cpstream, |r| &r.cpstream);
    let log = decode(&result.log, cpstream);
    for line in log.lines().take(24) {
        output.push(TaskItem::Message(line.chars().take(256).collect()))?;
    }
    if log.lines().count() > 24 || log.lines().any(|line| line.chars().count() > 256) {
        output.push(TaskItem::Message(
            "Salida del AVM truncada en el panel".into(),
        ))?;
    }
    let response = match result.response {
        Ok(response) => response,
        Err(error) => {
            output.push(TaskItem::Problem(format!(
                "AVM ({}): {error}",
                result.status
            )))?;
            return Ok(false);
        }
    };
    if !result.status.success() {
        output.push(TaskItem::Problem(format!(
            "El AVM terminó con {}",
            result.status
        )))?;
    }
    for diag in response.diagnostics.iter().take(24) {
        let filename = diag
            .file
            .as_ref()
            .map(|p| decode(p.as_bytes(), &response.cpinternal));
        let severity = if diag.severity == Severity::Error {
            "Error"
        } else {
            "Advertencia"
        };
        let mut message = severity.to_owned();
        if let Some(file) = filename.as_deref() {
            message.push_str(" · ");
            message.extend(file.chars().take(256));
        }
        if let Some(row) = diag.row {
            message.push_str(&format!(":{row}"));
            if let Some(column) = diag.column {
                message.push_str(&format!(":{column}"));
            }
        }
        message.push_str(" · ");
        message.extend(
            decode(diag.message.as_bytes(), &response.cpinternal)
                .chars()
                .take(256),
        );
        let own_source =
            filename.is_some_and(|file| root.join(file).canonicalize().is_ok_and(|p| p == source));
        let offset = if own_source {
            diag.row.and_then(|row| line_offset(snapshot, row))
        } else {
            None
        };
        output.push(match offset {
            Some(offset) => TaskItem::Diagnostic {
                range: offset..offset,
                message,
            },
            None => TaskItem::Problem(message),
        })?;
    }
    if response.diagnostics.len() > 24 {
        output.push(TaskItem::Problem(
            "Diagnósticos ABL truncados en el panel".into(),
        ))?;
    }
    if response.status == CompletionStatus::Error && response.diagnostics.is_empty() {
        output.push(TaskItem::Problem(
            "El AVM informó un error sin diagnóstico".into(),
        ))?;
    }
    Ok(result.status.success() && response.status == CompletionStatus::Ok)
}

fn line_offset(snapshot: &TaskSnapshot, target: u32) -> Option<usize> {
    let mut offset = 0;
    let mut row = 1;
    for scalar in DecodedScalars::new(snapshot.bytes(), snapshot.stamp.encoding, 0) {
        if row == target {
            return Some(if scalar.character == '\u{feff}' && offset == 0 {
                scalar.source_range.end
            } else {
                offset
            });
        }
        if scalar.character == '\n' {
            row += 1;
            offset = scalar.source_range.end;
        }
    }
    (row == target).then_some(offset)
}
