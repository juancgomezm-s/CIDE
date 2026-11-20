//! Launch a bounded read or human-approved edit through the installed companion.
use super::omnicore_transport::Model;
use jc_app::Wakeup;
use sha2::{Digest, Sha256};
use std::{
    env,
    fmt::Write as _,
    fs,
    io::{Read, Write},
    os::windows::{ffi::OsStrExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

pub(super) const LEGACY_INPUT_BYTES: usize = 8192;
const ANSWER_BYTES: usize = 64 * 1024;
const NO_WINDOW: u32 = 0x0800_0000;
const NEW_CONSOLE: u32 = 0x0000_0010;

pub(super) fn valid_executable(path: &Path) -> bool {
    path.is_absolute()
        && path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
        && path.is_file()
}

fn bundled_executable(editor: &Path) -> Option<PathBuf> {
    let path = editor.parent()?.join("omnicore").join("omni.exe");
    valid_executable(&path).then_some(path)
}

pub(super) fn discover() -> Option<PathBuf> {
    env::var_os("CIDE_OMNICORE_EXE")
        .map(PathBuf::from)
        .filter(|path| valid_executable(path))
        .or_else(|| bundled_executable(&env::current_exe().ok()?))
}

pub(super) fn edit_bridge(executable: &Path) -> Option<PathBuf> {
    context_bridge(executable).or_else(|| bridge(executable, "cide-omni-turn"))
}

pub(super) fn context_bridge(executable: &Path) -> Option<PathBuf> {
    bridge(executable, "cide-omni-turn-v2")
}

fn bridge(executable: &Path, name: &str) -> Option<PathBuf> {
    let directory = executable.parent()?;
    let bridge = directory.join(format!("{name}.exe"));
    (valid_executable(&bridge)
        && ["dll", "deps.json", "runtimeconfig.json"]
            .iter()
            .all(|suffix| directory.join(format!("{name}.{suffix}")).is_file()))
    .then_some(bridge)
}

pub(super) fn input_capacity(context_supported: bool, tokens: usize) -> usize {
    // OmniCore can count the prompt in the user message and in both the run
    // and task objectives. One UTF-8 byte per available estimated token
    // covers all three copies plus a margin over the host's text estimate.
    let capacity = tokens.saturating_sub(768).min(i32::MAX as usize);
    if context_supported {
        capacity
    } else {
        capacity.min(LEGACY_INPUT_BYTES)
    }
}

fn cide_omnicore_dir() -> Result<PathBuf, String> {
    let local = env::var_os("LOCALAPPDATA").ok_or("No se encontró la carpeta local de CIDE.")?;
    if !Path::new(&local).is_absolute() {
        return Err("La carpeta local de CIDE no es válida.".into());
    }
    Ok(PathBuf::from(local).join("CIDE").join("OmniCore"))
}

pub(super) fn workspace(project: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(project) = project {
        return Ok(project.to_owned());
    }
    prepare_workspace(&cide_omnicore_dir()?.join("Espacio de trabajo"))
}

fn prepare_workspace(path: &Path) -> Result<PathBuf, String> {
    fs::create_dir_all(path)
        .and_then(|()| path.canonicalize())
        .map_err(|error| format!("No se pudo abrir el espacio de OmniCore: {error}"))
}

fn read_workspace(
    root: &Path,
    model: Option<&Model>,
    context_supported: bool,
    turn: Turn,
) -> Result<PathBuf, String> {
    // The companion can resume an existing ACT Run. Consultations always use
    // their own PLAN workspace, separate from the project and its edit journal.
    let mut key = Sha256::new();
    match turn {
        Turn::Selection => key.update(b"CIDE selection preview\0"),
        Turn::Completion => key.update(b"CIDE inline completion\0"),
        _ => {}
    }
    if context_supported {
        // Old runs can retain objectives admitted under the larger estimate.
        key.update(b"CIDE chat reserved context\0");
    }
    if let Some(model) = model {
        key.update(model.provider.as_bytes());
        key.update([0]);
        key.update(model.id.as_bytes());
    }
    key.update([0]);
    for code in root.as_os_str().encode_wide() {
        key.update(code.to_le_bytes());
    }
    let mut folder = String::with_capacity(64);
    for byte in key.finalize() {
        write!(folder, "{byte:02x}").expect("writing to a String cannot fail");
    }
    prepare_workspace(&cide_omnicore_dir()?.join("Consultas").join(folder))
}

fn command(executable: &Path, root: &Path) -> Command {
    let mut command = Command::new(executable);
    command
        .current_dir(root)
        .env("OMNI_LOCALE", "es")
        .env_remove("OMNICORE_TRUSTED_ACTION_BOOTSTRAP");
    command
}

pub(super) fn open_session(executable: &Path, root: &Path) -> Result<(), String> {
    command(executable, root)
        .creation_flags(NEW_CONSOLE)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("No se pudo abrir la sesión de OmniCore: {error}"))
}

pub(super) struct Answer {
    pub text: String,
    pub complete: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Turn {
    Ask,
    Edit,
    Selection,
    Completion,
}

pub(super) struct Query {
    cancelled: Arc<AtomicBool>,
    result: Receiver<Result<Answer, String>>,
}

impl Query {
    pub fn start(
        executable: PathBuf,
        root: PathBuf,
        model: Option<Model>,
        question: String,
        capacity: usize,
        turn: Turn,
        wake: Wakeup,
    ) -> Result<Self, String> {
        let (send, result) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let running = cancelled.clone();
        thread::Builder::new()
            .name("cide-omnicore-consulta".into())
            .spawn(move || {
                let answer = run_turn(
                    &executable,
                    &root,
                    model.as_ref(),
                    question,
                    capacity,
                    turn,
                    &running,
                );
                if send.send(answer).is_ok() {
                    wake.signal();
                }
            })
            .map_err(|error| format!("No se pudo iniciar la consulta: {error}"))?;
        Ok(Self { cancelled, result })
    }

    pub fn poll(&self) -> Option<Result<Answer, String>> {
        match self.result.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err("Se interrumpió la consulta de OmniCore.".into()))
            }
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn cancelling(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

impl Drop for Query {
    fn drop(&mut self) {
        self.cancel();
    }
}

fn run_turn(
    executable: &Path,
    root: &Path,
    model: Option<&Model>,
    question: String,
    capacity: usize,
    turn: Turn,
    cancelled: &AtomicBool,
) -> Result<Answer, String> {
    if cancelled.load(Ordering::Relaxed) || question.len() > capacity || question.trim().is_empty()
    {
        return Err("Consulta vacía, demasiado larga o cancelada.".into());
    }
    let edit = turn == Turn::Edit;
    let isolated = matches!(turn, Turn::Selection | Turn::Completion);
    let compatible = context_bridge(executable);
    let framed = edit || compatible.is_some();
    if compatible.is_none() && question.len() > LEGACY_INPUT_BYTES {
        return Err("Actualiza el complemento de OmniCore para enviar este contexto.".into());
    }
    let turn_root = if edit {
        root.to_path_buf()
    } else {
        read_workspace(root, model, compatible.is_some(), turn)?
    };
    // Editor proposals must not resume a previous request's PLAN working state.
    // Keep the private directory alive until the child has exited.
    let scratch = if isolated {
        Some(
            tempfile::Builder::new()
                .prefix("editor-")
                .tempdir_in(&turn_root)
                .map_err(|error| format!("No se pudo preparar la consulta del editor: {error}"))?,
        )
    } else {
        None
    };
    let turn_root = scratch
        .as_ref()
        .map_or(turn_root.as_path(), tempfile::TempDir::path);
    let mut process = if let Some(bridge) = &compatible {
        let mut process = command(bridge, turn_root);
        process
            .arg(if edit { "edit" } else { "ask" })
            .arg(capacity.to_string());
        process
    } else if edit {
        let bridge = edit_bridge(executable)
            .ok_or("Esta instalación de OmniCore ya no tiene habilitada la edición.")?;
        let mut process = command(&bridge, turn_root);
        process.arg("edit");
        process
    } else {
        let mut process = command(executable, turn_root);
        process.args(["ask", "--stdin"]);
        process
    };
    if let Some(model) = model {
        process
            .env("OMNI_MODEL", &model.id)
            .env_remove("OMNI_BASE_URL")
            .env_remove("OMNI_QWEN_KEY");
    }
    let mut child = process
        .creation_flags(NO_WINDOW)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("No se pudo iniciar OmniCore: {error}"))?;
    let result = (|| {
        let mut input = child
            .stdin
            .take()
            .ok_or("Falta el canal de consulta de OmniCore.")?;
        let output = child
            .stdout
            .take()
            .ok_or("Falta el canal de respuesta de OmniCore.")?;
        let (send, receive) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name("cide-omnicore-respuesta".into())
            .spawn(move || {
                let mut bytes = Vec::new();
                let mut output = output;
                let result = (&mut output)
                    .take((ANSWER_BYTES + 1) as u64)
                    .read_to_end(&mut bytes)
                    .and_then(|size| {
                        if size > ANSWER_BYTES {
                            std::io::copy(&mut output, &mut std::io::sink())?;
                        }
                        Ok(bytes)
                    });
                let _ = send.send(result);
            })
            .map_err(|error| error.to_string())?;
        let (cancel_send, cancel_receive) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name("cide-omnicore-pregunta".into())
            .spawn(move || {
                if framed
                    && input
                        .write_all(&(question.len() as u32).to_le_bytes())
                        .is_err()
                {
                    return;
                }
                if input.write_all(question.as_bytes()).is_ok()
                    && framed
                    && cancel_receive.recv().is_ok()
                {
                    let _ = input.write_all(&[1]);
                }
            })
            .map_err(|error| error.to_string())?;
        let timeout = if turn == Turn::Completion { 30 } else { 180 };
        let deadline = Instant::now() + Duration::from_secs(timeout);
        let mut cancel_send = Some(cancel_send);
        let mut output = None;
        loop {
            let timed_out = Instant::now() >= deadline;
            if cancelled.load(Ordering::Relaxed) || timed_out {
                if !edit {
                    return Err(if timed_out {
                        "OmniCore superó el tiempo de respuesta.".into()
                    } else {
                        "Consulta cancelada.".into()
                    });
                }
                if let Some(send) = cancel_send.take() {
                    let _ = send.try_send(());
                }
            }
            if output.is_none() {
                match receive.recv_timeout(Duration::from_millis(50)) {
                    Ok(bytes) => output = Some(bytes.map_err(|error| error.to_string())?),
                    Err(RecvTimeoutError::Disconnected) => {
                        return Err("Se cerró el canal de respuesta.".into())
                    }
                    Err(RecvTimeoutError::Timeout) => continue,
                }
            }
            let bytes = output.as_ref().unwrap();
            if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
                if isolated && (bytes.len() > ANSWER_BYTES || std::str::from_utf8(bytes).is_err()) {
                    return Err(
                        "La propuesta de OmniCore está incompleta o contiene texto inválido."
                            .into(),
                    );
                }
                let output = String::from_utf8_lossy(&bytes[..bytes.len().min(ANSWER_BYTES)]);
                let edit_report =
                    edit && output.lines().any(|line| line.starts_with("[herramienta]"));
                if !status.success() && !edit_report {
                    return Err(query_failure(&output, model));
                }
                let text = answer_from_console(&output);
                if text == "ContextOverflow: el contexto no cabe en el presupuesto del modelo" {
                    return Err("OmniCore agotó el contexto disponible. Acorta la consulta o elige un modelo con más contexto.".into());
                }
                if text.is_empty() {
                    return Err("OmniCore no devolvió una respuesta. Abre su sesión para revisar la configuración.".into());
                }
                return Ok(Answer {
                    text: text.to_owned(),
                    complete: status.success() && bytes.len() <= ANSWER_BYTES,
                });
            }
            thread::sleep(Duration::from_millis(20));
        }
    })();
    if !matches!(child.try_wait(), Ok(Some(_))) {
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}

fn query_failure(output: &str, model: Option<&Model>) -> String {
    let failure = output.lines().find(|line| line.starts_with("Diagnóstico:"));
    if failure.is_some_and(|line| line.contains("ProviderUnavailable")) {
        return match model {
            Some(model) => format!(
                "No se pudo conectar con {} para usar {}. Revisa el servicio o elige otro modelo.",
                model.provider, model.id
            ),
            None => "No se pudo conectar con el proveedor predeterminado. Elige otro en Modelo o revisa el servicio.".into(),
        };
    }
    let detail = answer_from_console(output);
    if detail.is_empty() {
        "OmniCore no devolvió una respuesta. Revisa su configuración y vuelve a intentar.".into()
    } else {
        detail.chars().take(768).collect()
    }
}

fn answer_from_console(output: &str) -> &str {
    const LEGACY_AUTH_WARNING: &str =
        "aviso: el formato auth de providers.yaml está obsoleto; usa authRef.";
    let output = output.trim();
    if output == LEGACY_AUTH_WARNING {
        return "";
    }
    let answer = output
        .strip_prefix(LEGACY_AUTH_WARNING)
        .and_then(|rest| rest.trim_start_matches('\r').strip_prefix('\n'))
        .map_or(output, str::trim_start);

    // `omni ask` adds a localized turn summary, optionally followed by session usage.
    let is_summary = |line: &str| {
        line.starts_with("── ") && line.contains(" · pasos ") && line.contains(" · tokens ")
    };
    let without_usage = answer
        .rsplit_once('\n')
        .filter(|(_, last)| last.starts_with("sesión "))
        .map_or(answer, |(body, _)| body);
    let (body, summary) = without_usage
        .rsplit_once('\n')
        .unwrap_or(("", without_usage));
    if is_summary(summary) {
        body.trim_end()
    } else {
        answer
    }
}
