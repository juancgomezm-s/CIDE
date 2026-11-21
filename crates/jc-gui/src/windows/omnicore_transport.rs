//! Optional client for the passive OmniCore stdio observer. The observer never
//! grants a command, opens an agent, or substitutes unrelated workspace diffs.
use super::open_diff_panel::OmniCorePreview;
use jc_app::Wakeup;
use serde_json::{json, Value};
use std::{
    collections::{HashSet, VecDeque},
    env, fs,
    io::{BufRead, BufReader, Write},
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const POLL_PERIOD: Duration = Duration::from_secs(2);
const IO_SLICE: Duration = Duration::from_millis(50);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_FRAME: usize = 1024 * 1024;
const MAX_ROWS: usize = 4096;
const VISIBLE_ROWS: usize = 256;
const MAX_OPENS: usize = 4;
const TRACKED_SESSIONS: usize = 8;
const NO_WINDOW: u32 = 0x0800_0000;
const MAX_MODELS: usize = 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Model {
    pub id: String,
    pub provider: String,
    pub input_tokens: Option<usize>,
}

pub(super) struct Catalog {
    cancelled: Arc<AtomicBool>,
    result: Receiver<Result<Vec<Model>, String>>,
}

impl Catalog {
    pub fn start(executable: PathBuf, wakeup: Wakeup) -> Result<Self, String> {
        let (send, result) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let running = cancelled.clone();
        thread::Builder::new()
            .name("cide-omnicore-modelos".into())
            .spawn(move || {
                if send.send(read_catalog(&executable, &running)).is_ok() {
                    wakeup.signal();
                }
            })
            .map_err(|_| "No se pudo iniciar la lectura de modelos de OmniCore.".to_owned())?;
        Ok(Self { cancelled, result })
    }

    pub fn poll(&self) -> Option<Result<Vec<Model>, String>> {
        match self.result.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(
                "Se interrumpió la lectura de modelos de OmniCore.".into(),
            )),
        }
    }
}

impl Drop for Catalog {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

pub(super) fn user_config_dir() -> Result<PathBuf, String> {
    let configured = |name| {
        env::var_os(name)
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
    };
    let path = configured("OMNICORE_CONFIG_DIR")
        .or_else(|| configured("OMNICORE_DATA_DIR").map(|root| root.join("config")))
        .or_else(|| configured("APPDATA").map(|root| root.join("OmniCore")));
    // Resolve against process settings, never a project directory.
    path.filter(|path| path.is_absolute())
        .ok_or_else(|| "No se encontró una ruta absoluta para la configuración de OmniCore.".into())
}

fn read_catalog(executable: &Path, cancelled: &AtomicBool) -> Result<Vec<Model>, String> {
    let config = user_config_dir()?;
    let private = tempfile::Builder::new()
        .prefix("cide-omnicore-modelos-")
        .tempdir()
        .map_err(|_| "No se pudo preparar la lectura de modelos de OmniCore.")?;
    // The regular host opens a journal on startup. Its local catalog uses the
    // real user config, while all startup writes stay in disposable local data.
    let mut command = Command::new(executable);
    command
        .args(["host", "--stdio"])
        .current_dir(private.path())
        .env("OMNICORE_CONFIG_DIR", config)
        .env("OMNICORE_DATA_DIR", private.path().join("datos"));
    let (mut host, hello) = Host::connect(command, cancelled)
        .map_err(|_| "No se pudo leer el catálogo. Revisa la configuración de OmniCore.")?;
    if !has_capability(&hello, "query.modelsCatalog") {
        return Err("Esta versión de OmniCore no permite consultar sus modelos.".into());
    }
    let result = host.query(
        json!({"operation":"query", "name":"models.catalog"}),
        "models.catalog",
        cancelled,
    )?;
    decode_models(&result)
}

fn has_capability(hello: &Value, name: &str) -> bool {
    hello["capabilities"]
        .as_array()
        .is_some_and(|caps| caps.iter().any(|value| value.as_str() == Some(name)))
}

fn decode_models(catalog: &Value) -> Result<Vec<Model>, String> {
    let entries = catalog["models"]
        .as_array()
        .filter(|entries| catalog["schemaVersion"] == 1 && entries.len() <= MAX_MODELS)
        .ok_or("El catálogo de OmniCore no tiene un formato admitido.")?;
    let mut models = Vec::new();
    let mut ids = HashSet::new();
    for entry in entries {
        let has_text = |name: &str| {
            entry[name]
                .as_array()
                .is_some_and(|modalities| modalities.iter().any(|value| value == "text"))
        };
        if !has_text("inputModalities") || !has_text("outputModalities") {
            continue;
        }
        let identifier = |key: &str| {
            entry[key]
                .as_str()
                .filter(|id| !id.is_empty() && id.len() <= 512 && !id.chars().any(char::is_control))
                .ok_or("El catálogo de OmniCore contiene un identificador inválido.")
        };
        let (id, provider) = (identifier("modelId")?, identifier("providerId")?);
        if !ids.insert(id) {
            return Err("El catálogo de OmniCore contiene modelos duplicados.".into());
        }
        models.push(Model {
            id: id.into(),
            provider: provider.into(),
            input_tokens: model_input_tokens(entry),
        });
    }
    models.sort_unstable_by(|a, b| (&a.provider, &a.id).cmp(&(&b.provider, &b.id)));
    Ok(models)
}

fn model_input_tokens(entry: &Value) -> Option<usize> {
    let positive = |key: &str| {
        entry[key]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|&n| n > 0)
    };
    let window = positive("contextWindow")?;
    let response = positive("maxOutputTokens").unwrap_or((window / 8).max(1));
    let input = window.saturating_sub(response);
    let usable = input.min(positive("recommendedUsableContext").unwrap_or(input));
    // Leave room for the host's instructions and message envelope.
    Some(usable.saturating_sub((usable / 8).max(1)))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ChangedFile {
    pub effect_id: String,
    pub path: PathBuf,
    pub kind: ChangeKind,
    pub diff_available: bool,
    relative: String,
    observed: bool,
}

pub(super) struct Opened {
    pub path: PathBuf,
    pub kind: ChangeKind,
    pub preview: Option<OmniCorePreview>,
    pub unavailable: Option<String>,
}

pub(super) enum Update {
    Snapshot {
        session: Option<String>,
        rows: Vec<ChangedFile>,
        opened: Vec<Opened>,
        ask_supported: bool,
    },
    Error(String),
}

struct Request {
    root: PathBuf,
    executable: PathBuf,
    wakeup: Wakeup,
    cancelled: Arc<AtomicBool>,
    updates: SyncSender<Update>,
    manual: Receiver<String>,
}

struct Mailbox {
    pending: Arc<Mutex<Option<Request>>>,
    wake: SyncSender<()>,
}

struct Active {
    cancelled: Arc<AtomicBool>,
    updates: Receiver<Update>,
    manual: SyncSender<String>,
}

#[derive(Default)]
pub(super) struct Transport {
    mailbox: Option<Mailbox>,
    active: Option<Active>,
}

impl Transport {
    pub(super) fn start(
        &mut self,
        root: PathBuf,
        executable: PathBuf,
        wakeup: Wakeup,
    ) -> Result<(), String> {
        if !root.is_absolute()
            || !executable.is_absolute()
            || !executable
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
        {
            return Err("Selecciona un ejecutable de OmniCore y un proyecto válidos".into());
        }
        self.stop();
        if self.mailbox.is_none() {
            let pending: Arc<Mutex<Option<Request>>> = Arc::new(Mutex::new(None));
            let inbox = pending.clone();
            let (wake, commands) = mpsc::sync_channel(1);
            thread::Builder::new()
                .name("cide-omnicore".into())
                .spawn(move || {
                    while commands.recv().is_ok() {
                        let request = inbox.lock().unwrap().take();
                        if let Some(request) = request {
                            if let Err(error) = monitor(&request) {
                                let _ = publish(&request, Update::Error(error));
                            }
                        }
                    }
                })
                .map_err(|error| {
                    format!("No se pudo iniciar el observador de OmniCore: {error}")
                })?;
            self.mailbox = Some(Mailbox { pending, wake });
        }
        let mailbox = self.mailbox.as_ref().unwrap();
        let (updates, receiver) = mpsc::sync_channel(2);
        let (sender, manual) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        *mailbox.pending.lock().unwrap() = Some(Request {
            root,
            executable,
            wakeup,
            cancelled: cancelled.clone(),
            updates,
            manual,
        });
        if let Err(TrySendError::Disconnected(())) = mailbox.wake.try_send(()) {
            self.mailbox = None;
            return Err("El observador de OmniCore ya no está disponible".into());
        }
        self.active = Some(Active {
            cancelled,
            updates: receiver,
            manual: sender,
        });
        Ok(())
    }

    pub(super) fn stop(&mut self) {
        if let Some(active) = self.active.take() {
            active.cancelled.store(true, Ordering::Relaxed);
        }
    }

    pub(super) fn poll(&mut self) -> Option<Update> {
        self.active.as_ref()?.updates.try_recv().ok()
    }

    pub(super) fn request_open(&mut self, effect_id: &str) -> Result<(), String> {
        if !valid_id(effect_id) {
            return Err("Identificador de cambio de OmniCore inválido".into());
        }
        let active = self
            .active
            .as_ref()
            .ok_or("Conecta primero el panel de OmniCore")?;
        active
            .manual
            .try_send(effect_id.to_owned())
            .map_err(|error| match error {
                TrySendError::Full(_) => "OmniCore todavía está abriendo otro cambio".into(),
                TrySendError::Disconnected(_) => "El observador de OmniCore se desconectó".into(),
            })
    }
}

impl Drop for Transport {
    fn drop(&mut self) {
        self.stop();
    }
}

fn publish(request: &Request, mut update: Update) -> bool {
    while !request.cancelled.load(Ordering::Relaxed) {
        match request.updates.try_send(update) {
            Ok(()) => {
                request.wakeup.signal();
                return true;
            }
            Err(TrySendError::Full(queued)) => {
                update = queued;
                thread::sleep(IO_SLICE);
            }
            Err(TrySendError::Disconnected(_)) => break,
        }
    }
    false
}

struct Host {
    child: Child,
    requests: Option<SyncSender<Value>>,
    responses: Option<Receiver<Result<Value, String>>>,
    io: Option<JoinHandle<()>>,
    next_id: u64,
    ask_supported: bool,
}

impl Host {
    fn launch(executable: &Path, root: &Path, cancelled: &AtomicBool) -> Result<Self, String> {
        let mut command = Command::new(executable);
        command
            .args(["host", "--stdio", "--observe"])
            .current_dir(root);
        let (mut host, hello) = Self::connect(command, cancelled)?;
        if ![
            "observe.changedFiles.v1",
            "query.changedFiles",
            "query.changedFileDiff",
        ]
        .iter()
        .all(|name| has_capability(&hello, name))
        {
            return Err(
                "Esta versión de OmniCore no ofrece el observador de cambios de CIDE".into(),
            );
        }
        host.ask_supported = has_capability(&hello, "ask.stdin.v1");
        Ok(host)
    }

    fn connect(mut command: Command, cancelled: &AtomicBool) -> Result<(Self, Value), String> {
        let child = command
            .env("OMNI_LOCALE", "es")
            .env_remove("OMNICORE_TRUSTED_ACTION_BOOTSTRAP")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(NO_WINDOW)
            .spawn()
            .map_err(|error| format!("No se pudo iniciar OmniCore: {error}"))?;
        let mut host = Self {
            child,
            requests: None,
            responses: None,
            io: None,
            next_id: 0,
            ask_supported: false,
        };
        let mut stdin = host.child.stdin.take().ok_or("OmniCore no abrió stdin")?;
        let stdout = host.child.stdout.take().ok_or("OmniCore no abrió stdout")?;
        let (send, requests) = mpsc::sync_channel::<Value>(1);
        let (responses, receive) = mpsc::sync_channel(1);
        let io = thread::Builder::new()
            .name("cide-omnicore-stdio".into())
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                while let Ok(frame) = requests.recv() {
                    let result =
                        write_request(&mut stdin, &frame).and_then(|()| read_response(&mut reader));
                    let failed = result.is_err();
                    if responses.send(result).is_err() || failed {
                        break;
                    }
                }
            })
            .map_err(|error| format!("No se pudo abrir el canal de OmniCore: {error}"))?;
        host.requests = Some(send);
        host.responses = Some(receive);
        host.io = Some(io);
        let hello = host
            .exchange(
                "hello",
                json!({"supportedVersions":["1.0"], "client":"cide"}),
                cancelled,
            )
            .map_err(|error| {
                format!("No se pudo conectar al modo observador de OmniCore: {error}")
            })?;
        if hello["protocolVersion"] != "1.0" {
            return Err("OmniCore devolvió una versión de protocolo incompatible.".into());
        }
        Ok((host, hello))
    }

    fn exchange(
        &mut self,
        kind: &str,
        payload: Value,
        cancelled: &AtomicBool,
    ) -> Result<Value, String> {
        if cancelled.load(Ordering::Relaxed) {
            return Err("Se desconectó OmniCore".into());
        }
        self.next_id += 1;
        let id = format!("cide-{}", self.next_id);
        self.requests
            .as_ref()
            .unwrap()
            .try_send(
                json!({"version":"1.0", "messageType":kind, "messageId":id, "payload":payload}),
            )
            .map_err(|_| "El canal de OmniCore se cerró".to_owned())?;
        let deadline = Instant::now() + REQUEST_TIMEOUT;
        while !cancelled.load(Ordering::Relaxed) && Instant::now() < deadline {
            match self.responses.as_ref().unwrap().recv_timeout(IO_SLICE) {
                Ok(result) => {
                    let mut frame = result?;
                    if frame["version"] != "1.0" || frame["messageId"] != id {
                        return Err("La respuesta de OmniCore no corresponde a la consulta".into());
                    }
                    match frame["messageType"].as_str() {
                        Some("response") => return Ok(frame["payload"].take()),
                        Some("error") => return Err("OmniCore rechazó la consulta".into()),
                        _ => return Err("Tipo de respuesta de OmniCore inválido".into()),
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    return Err("El observador de OmniCore terminó".into());
                }
            }
        }
        Err("OmniCore no respondió dentro del plazo".into())
    }

    fn query(
        &mut self,
        payload: Value,
        name: &str,
        cancelled: &AtomicBool,
    ) -> Result<Value, String> {
        let response = self.exchange("command", payload, cancelled)?;
        if response["name"] != name {
            return Err("OmniCore devolvió una consulta distinta".into());
        }
        let body = response["json"]
            .as_str()
            .ok_or("La respuesta de OmniCore no tiene datos")?;
        serde_json::from_str(body)
            .map_err(|_| "La respuesta de OmniCore contiene datos inválidos".into())
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.requests.take();
        self.responses.take();
        // Only the observer launched by this worker is terminated.
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(io) = self.io.take() {
            let _ = io.join();
        }
    }
}

fn write_request(writer: &mut impl Write, value: &Value) -> Result<(), String> {
    serde_json::to_writer(&mut *writer, value)
        .map_err(|_| "No se pudo enviar la consulta a OmniCore")?;
    writer
        .write_all(b"\n")
        .and_then(|()| writer.flush())
        .map_err(|_| "El canal de OmniCore se cerró".into())
}

fn read_response(reader: &mut impl BufRead) -> Result<Value, String> {
    let mut frame = Vec::new();
    loop {
        let chunk = reader
            .fill_buf()
            .map_err(|_| "No se pudo leer la respuesta de OmniCore")?;
        if chunk.is_empty() {
            return Err("El observador de OmniCore terminó".into());
        }
        let end = chunk.iter().position(|byte| *byte == b'\n');
        let length = end.unwrap_or(chunk.len());
        if frame.len() + length > MAX_FRAME {
            return Err("La respuesta de OmniCore supera 1 MiB".into());
        }
        frame.extend_from_slice(&chunk[..length]);
        reader.consume(length + usize::from(end.is_some()));
        if end.is_some() {
            break;
        }
    }
    serde_json::from_slice(&frame).map_err(|_| "OmniCore devolvió un mensaje JSON inválido".into())
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 4096 && !id.chars().any(char::is_control)
}

fn project_path(root: &Path, text: &str) -> Option<PathBuf> {
    if text.is_empty()
        || text.len() > 4096
        || text.contains("[REDACTED]")
        || text
            .chars()
            .any(|c| c.is_control() || "<>:\"|?*".contains(c))
    {
        return None;
    }
    let mut path = root.to_owned();
    for part in text.split(['/', '\\']) {
        if part.is_empty() || part.ends_with([' ', '.']) || part.eq_ignore_ascii_case(".git") {
            return None;
        }
        path.push(part);
    }
    Some(path)
}

struct Snapshot {
    session: Option<String>,
    sequence: u64,
    rows: Vec<ChangedFile>,
}

fn parse_snapshot(root: &Path, value: &Value) -> Result<Snapshot, String> {
    if value.is_null() {
        return Ok(Snapshot {
            session: None,
            sequence: 0,
            rows: Vec::new(),
        });
    }
    if value["unavailable"] == true {
        return Err("El registro de cambios de OmniCore no está disponible".into());
    }
    let session = value["sessionId"]
        .as_str()
        .filter(|id| valid_id(id))
        .ok_or("Sesión de OmniCore inválida")?;
    let sequence = value["basedOnJournalSequence"]
        .as_u64()
        .ok_or("Secuencia de OmniCore inválida")?;
    let files = value["files"]
        .as_array()
        .filter(|rows| rows.len() <= MAX_ROWS)
        .ok_or("La lista de cambios de OmniCore no cabe en el panel")?;
    let mut identities = HashSet::with_capacity(files.len());
    let mut rows = Vec::with_capacity(files.len());
    for file in files {
        let effect = file["effectId"]
            .as_str()
            .filter(|id| valid_id(id))
            .ok_or("Identificador de cambio de OmniCore inválido")?;
        if !identities.insert(effect) {
            return Err("OmniCore devolvió un cambio duplicado".into());
        }
        let relative = file["path"]
            .as_str()
            .ok_or("Falta la ruta del cambio de OmniCore")?;
        let Some(path) = project_path(root, relative) else {
            continue;
        };
        let kind = match file["status"].as_str() {
            Some("A") => ChangeKind::Added,
            Some("M") => ChangeKind::Modified,
            Some("D") => ChangeKind::Deleted,
            _ => ChangeKind::Unknown,
        };
        rows.push(ChangedFile {
            effect_id: effect.to_owned(),
            path,
            kind,
            diff_available: file["diffAvailable"] == true,
            relative: relative.to_owned(),
            observed: file["observed"] == true,
        });
    }
    Ok(Snapshot {
        session: Some(session.to_owned()),
        sequence,
        rows,
    })
}

#[derive(Default)]
struct Seen(VecDeque<SeenSession>);

struct SeenSession {
    id: String,
    sequence: u64,
    handled: HashSet<String>,
}

fn openable(row: &ChangedFile) -> bool {
    matches!(row.kind, ChangeKind::Added | ChangeKind::Modified)
}

impl Seen {
    fn pending<'a>(&mut self, snapshot: &'a Snapshot) -> Result<Vec<&'a ChangedFile>, String> {
        let Some(id) = &snapshot.session else {
            return Ok(Vec::new());
        };
        let mut session = self
            .0
            .iter()
            .position(|entry| entry.id == *id)
            .and_then(|index| self.0.remove(index))
            .unwrap_or_else(|| SeenSession {
                id: id.clone(),
                sequence: snapshot.sequence,
                handled: HashSet::new(),
            });
        if snapshot.sequence < session.sequence {
            return Err("OmniCore devolvió una revisión anterior del registro".into());
        }
        if snapshot.sequence != session.sequence {
            let present: HashSet<_> = snapshot
                .rows
                .iter()
                .map(|row| row.effect_id.as_str())
                .collect();
            session
                .handled
                .retain(|effect| present.contains(effect.as_str()));
            session.sequence = snapshot.sequence;
        }
        let mut pending = Vec::new();
        for row in snapshot
            .rows
            .iter()
            .filter(|row| row.observed && openable(row))
        {
            if !session.handled.contains(&row.effect_id) {
                session.handled.insert(row.effect_id.clone());
                pending.push(row);
                if pending.len() == MAX_OPENS {
                    break;
                }
            }
        }
        self.0.push_front(session);
        self.0.truncate(TRACKED_SESSIONS);
        Ok(pending)
    }
}

fn prepare_open(
    host: &mut Host,
    root: &Path,
    session: &str,
    row: &ChangedFile,
    cancelled: &AtomicBool,
) -> Opened {
    let mut opened = Opened {
        path: row.path.clone(),
        kind: row.kind,
        preview: None,
        unavailable: None,
    };
    let valid = fs::canonicalize(&row.path)
        .map(|path| path.starts_with(root) && path.is_file())
        .unwrap_or(false);
    if !valid {
        opened.unavailable =
            Some("El archivo de OmniCore ya no está disponible dentro del proyecto".into());
    } else if row.kind == ChangeKind::Modified {
        match read_preview(host, session, row, cancelled) {
            Ok(preview) => opened.preview = Some(preview),
            Err(reason) => opened.unavailable = Some(reason),
        }
    }
    opened
}

fn read_preview(
    host: &mut Host,
    session: &str,
    row: &ChangedFile,
    cancelled: &AtomicBool,
) -> Result<OmniCorePreview, String> {
    if !row.diff_available {
        return Err("OmniCore no conserva una comparación para este cambio".into());
    }
    let diff = host.query(
        json!({
            "operation":"query", "name":"changedFileDiff",
            "sessionId":session, "effectId":row.effect_id
        }),
        "changedFileDiff",
        cancelled,
    )?;
    if diff["sessionId"] != session
        || diff["effectId"] != row.effect_id
        || diff["path"] != row.relative
    {
        return Err("La comparación de OmniCore pertenece a otro cambio".into());
    }
    if diff["available"] != true {
        return Err("OmniCore no pudo entregar una comparación para este cambio".into());
    }
    let before = diff["before"]
        .as_str()
        .ok_or("Falta la versión anterior de OmniCore")?;
    let after = diff["after"]
        .as_str()
        .ok_or("Falta la versión nueva de OmniCore")?;
    OmniCorePreview::compute(before.to_owned(), after.to_owned())
}

fn monitor(request: &Request) -> Result<(), String> {
    if request.cancelled.load(Ordering::Relaxed) {
        return Ok(());
    }
    let root = fs::canonicalize(&request.root)
        .map_err(|_| "El proyecto de OmniCore no está disponible")?;
    let mut host = Host::launch(&request.executable, &root, &request.cancelled)?;
    let mut seen = Seen::default();
    let mut visible = None;
    let mut manual = None;
    while !request.cancelled.load(Ordering::Relaxed) {
        let result = host.query(
            json!({"operation":"query", "name":"changedFiles"}),
            "changedFiles",
            &request.cancelled,
        )?;
        let snapshot = parse_snapshot(&root, &result)?;
        let mut pending = seen.pending(&snapshot)?;
        if let Some(effect) = manual.take() {
            if let Some(row) = snapshot
                .rows
                .iter()
                .find(|row| row.effect_id == effect && openable(row))
            {
                if !pending.contains(&row) {
                    pending.push(row);
                }
            }
        }
        let opened = if let Some(session) = &snapshot.session {
            pending
                .into_iter()
                .take_while(|_| !request.cancelled.load(Ordering::Relaxed))
                .map(|row| prepare_open(&mut host, &root, session, row, &request.cancelled))
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let rows = snapshot
            .rows
            .into_iter()
            .rev()
            .take(VISIBLE_ROWS)
            .collect::<Vec<_>>();
        let visible_next = (snapshot.session, rows);
        if (visible.as_ref() != Some(&visible_next) || !opened.is_empty())
            && !publish(
                request,
                Update::Snapshot {
                    session: visible_next.0.clone(),
                    rows: visible_next.1.clone(),
                    opened,
                    ask_supported: host.ask_supported,
                },
            )
        {
            break;
        }
        visible = Some(visible_next);
        match request.manual.recv_timeout(POLL_PERIOD) {
            Ok(effect) => manual = Some(effect),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(seq: u64, rows: &[(&str, &str, bool)]) -> Snapshot {
        let rows = rows.iter().map(|(id, status, observed)| {
            json!({"effectId":id, "status":status, "path":"src/test.rs", "diffAvailable":true, "observed":observed})
        }).collect::<Vec<_>>();
        parse_snapshot(
            Path::new(r"C:\project"),
            &json!({
                "sessionId":"session", "basedOnJournalSequence":seq, "files":rows
            }),
        )
        .unwrap()
    }

    #[test]
    fn only_newly_completed_omnicore_effects_open() {
        let mut seen = Seen::default();
        assert!(seen
            .pending(&snapshot(1, &[("old", "M", false), ("waiting", "?", true)]))
            .unwrap()
            .is_empty());
        let next = snapshot(
            2,
            &[
                ("old", "M", false),
                ("waiting", "A", true),
                ("edit", "M", true),
                ("removed", "D", true),
            ],
        );
        let opened = seen.pending(&next).unwrap();
        assert_eq!(
            opened
                .iter()
                .map(|row| row.effect_id.as_str())
                .collect::<Vec<_>>(),
            ["waiting", "edit"]
        );
        assert!(seen.pending(&next).unwrap().is_empty());
    }

    #[test]
    fn session_switch_admits_new_effects_without_reopening_history() {
        let mut seen = Seen::default();
        let first = snapshot(2, &[("first", "A", true)]);
        assert_eq!(seen.pending(&first).unwrap()[0].effect_id, "first");
        let mut other = snapshot(30, &[("history", "M", false), ("next", "M", true)]);
        other.session = Some("other".to_string());
        assert_eq!(seen.pending(&other).unwrap()[0].effect_id, "next");
        assert!(seen.pending(&first).unwrap().is_empty());
        assert!(seen.pending(&other).unwrap().is_empty());
    }

    #[test]
    fn opening_budget_carries_remaining_changes_into_the_next_poll() {
        let mut seen = Seen::default();
        assert!(seen.pending(&snapshot(1, &[])).unwrap().is_empty());
        let next = snapshot(
            2,
            &[
                ("a", "A", true),
                ("b", "M", true),
                ("c", "M", true),
                ("d", "M", true),
                ("e", "A", true),
            ],
        );
        assert_eq!(seen.pending(&next).unwrap().len(), MAX_OPENS);
        assert_eq!(seen.pending(&next).unwrap()[0].effect_id, "e");
        assert!(seen.pending(&next).unwrap().is_empty());
    }

    #[test]
    fn omnicore_paths_are_relative_regular_file_candidates() {
        let root = Path::new(r"C:\project");
        assert_eq!(
            project_path(root, "src/main.rs"),
            Some(root.join(r"src\main.rs"))
        );
        for rejected in [
            "../main.rs",
            "src\\..\\root.rs",
            r"C:\main.rs",
            r"\\share\a",
            "file.rs:stream",
            ".git/config",
            "src//main.rs",
            "src/alias.",
            "src/[REDACTED].rs",
        ] {
            assert!(project_path(root, rejected).is_none(), "{rejected}");
        }
    }

    #[test]
    fn stdio_lines_are_strict_and_bounded() {
        assert_eq!(
            read_response(&mut &b"{\"ok\":true}\r\n"[..]).unwrap()["ok"],
            true
        );
        assert!(read_response(&mut &b"{\"incomplete\":0}"[..]).is_err());
        assert!(read_response(&mut &vec![b'x'; MAX_FRAME + 1][..]).is_err());
    }
}
