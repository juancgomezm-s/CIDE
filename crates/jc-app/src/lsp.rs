//! Shared stdio client for explicit, read-only source navigation.
mod language_items;
mod transport;

use crate::{
    source_services::{SourceAction, SourceItem},
    Cancellation, TaskFailure, TaskItem, TaskOutput, TaskSnapshot,
};
use jc_text::{DecodedScalars, TextEncoding};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
    time::Duration,
};
use transport::Transport;

pub(crate) const TIMEOUT: Duration = Duration::from_secs(45);
const CONFIG_LIMIT: usize = 64 * 1024;
const FILE_LIMIT: usize = 2 * 1024 * 1024;
const READ_LIMIT: usize = 8 * 1024 * 1024;
const RESULT_LIMIT: usize = 48;

pub(crate) fn language_id(profile: &str, path: &Path) -> Option<&'static str> {
    let extension = path.extension()?.to_str()?;
    match profile {
        "rust" => Some("rust"),
        "csharp" => Some("csharp"),
        "php" => Some("php"),
        "java" => Some("java"),
        "javascript" if extension.eq_ignore_ascii_case("jsx") => Some("javascriptreact"),
        "typescript" if extension.eq_ignore_ascii_case("tsx") => Some("typescriptreact"),
        "javascript" => Some("javascript"),
        "typescript" => Some("typescript"),
        _ => None,
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Query {
    Definition,
    References,
    Completion,
    SemanticContext,
    Diagnostics,
}

#[derive(Default, Clone)]
pub(crate) struct Client(Arc<Mutex<Option<Server>>>);

pub(crate) struct Work {
    pub(crate) client: Client,
    pub(crate) root: PathBuf,
    pub(crate) path: PathBuf,
    pub(crate) profile: String,
    pub(crate) cursor: usize,
    pub(crate) query: Query,
}

#[derive(PartialEq)]
struct Config {
    command: PathBuf,
    args: Vec<String>,
    initialization: Value,
    settings: Value,
}

impl Config {
    fn load(profile: &str) -> Result<Self, String> {
        let path = std::env::var_os("CIDE_LSP_CONFIG")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("CIDE/lsp.json"))
            })
            .ok_or("Define CIDE_LSP_CONFIG con la ruta de tu configuración LSP")?;
        if !path.is_absolute() {
            return Err("La ruta de configuración LSP debe ser absoluta".into());
        }
        let mut bytes = Vec::new();
        File::open(path)
            .and_then(|file| file.take(CONFIG_LIMIT as u64 + 1).read_to_end(&mut bytes))
            .map_err(|_| {
                "Configura el servidor en %LOCALAPPDATA%/CIDE/lsp.json o CIDE_LSP_CONFIG"
            })?;
        if bytes.len() > CONFIG_LIMIT {
            return Err("La configuración LSP excede 64 KiB".into());
        }
        let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&bytes);
        let config: Value = serde_json::from_slice(bytes)
            .map_err(|error| format!("Configuración JSON de LSP: {error}"))?;
        let profiles = config
            .get("servers")
            .and_then(Value::as_object)
            .ok_or("Falta el objeto servers en la configuración LSP")?;
        let fallback = matches!(profile, "javascript" | "typescript").then_some("ecmascript");
        let entry = profiles
            .get(profile)
            .or_else(|| fallback.and_then(|key| profiles.get(key)))
            .and_then(Value::as_object)
            .ok_or_else(|| format!("Falta configurar el servidor LSP para {profile}"))?;
        let command = entry
            .get("command")
            .and_then(Value::as_str)
            .filter(|s| !s.contains('\0'))
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .ok_or("Indica una ruta absoluta en command para el servidor LSP")?;
        if cfg!(windows)
            && !command
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
        {
            return Err("En Windows, command debe apuntar a un ejecutable .exe".into());
        }
        let args = match entry.get("args") {
            None => Vec::new(),
            Some(value) => value
                .as_array()
                .filter(|args| args.len() <= 64)
                .and_then(|args| {
                    args.iter()
                        .map(|arg| {
                            arg.as_str()
                                .filter(|s| !s.contains('\0'))
                                .map(str::to_owned)
                        })
                        .collect()
                })
                .ok_or("args debe contener hasta 64 cadenas de texto")?,
        };
        Ok(Self {
            command,
            args,
            initialization: entry
                .get("initializationOptions")
                .cloned()
                .unwrap_or_default(),
            settings: entry.get("settings").cloned().unwrap_or_default(),
        })
    }
}

struct Server {
    root: PathBuf,
    config: Config,
    folders: Value,
    transport: Transport,
    capabilities: Value,
    document: Option<OpenDocument>,
}

struct OpenDocument {
    uri: String,
    language: &'static str,
    text: String,
    end: Position,
    version: i32,
    push_safe: bool,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.transport.shutdown();
    }
}

impl Server {
    fn start(root: PathBuf, config: Config, cancel: &Cancellation) -> Result<Self, String> {
        let uri = file_uri(&root).ok_or("No se pudo representar la raíz como URI local")?;
        let folders = json!([{"uri":uri, "name":root.file_name().and_then(|n| n.to_str()).unwrap_or("Proyecto")}]);
        let mut transport = Transport::spawn(
            Command::new(&config.command)
                .args(&config.args)
                .current_dir(&root),
        )?;
        let initialized = transport.request(
            "initialize",
            json!({
                "processId": std::process::id(), "rootUri": uri, "workspaceFolders": folders,
                "initializationOptions": config.initialization,
                "capabilities": {
                    "general":{"positionEncodings":["utf-16"]},
                    "workspace":{"configuration":true,"workspaceFolders":true},
                    "textDocument":{
                        "definition":{"linkSupport":true},
                        "completion":{"contextSupport":true,"completionItem":{"snippetSupport":false,"insertReplaceSupport":true}},
                        "publishDiagnostics":{"versionSupport":true},
                        "diagnostic":{"relatedDocumentSupport":false}
                    }
                },
            }),
            cancel,
            |method, params| answer(method, params, &config.settings, &folders),
        )?;
        let capabilities = initialized
            .get("capabilities")
            .filter(|value| value.is_object())
            .cloned()
            .ok_or("El servidor LSP no devolvió sus capacidades")?;
        if capabilities
            .get("positionEncoding")
            .is_some_and(|value| value != "utf-16")
        {
            return Err(
                "El servidor LSP eligió una codificación de posiciones distinta de UTF-16".into(),
            );
        }
        transport.notify("initialized", json!({}), cancel)?;
        if !config.settings.is_null() {
            transport.notify(
                "workspace/didChangeConfiguration",
                json!({"settings":config.settings}),
                cancel,
            )?;
        }
        Ok(Self {
            root,
            config,
            folders,
            transport,
            capabilities,
            document: None,
        })
    }

    fn query(
        &mut self,
        query: Query,
        document: OpenDocument,
        position: Position,
        cancel: &Cancellation,
        starting: bool,
    ) -> Result<Value, String> {
        let (method, capability) = match query {
            Query::Definition => ("textDocument/definition", "definitionProvider"),
            Query::References => ("textDocument/references", "referencesProvider"),
            Query::Completion | Query::SemanticContext => {
                ("textDocument/completion", "completionProvider")
            }
            Query::Diagnostics => ("textDocument/diagnostic", "diagnosticProvider"),
        };
        let available = &self.capabilities[capability];
        let supported = available == true || available.is_object();
        if !supported && !matches!(query, Query::Diagnostics) {
            return Err("El servidor LSP no anunció esta capacidad".into());
        }
        let mut params = json!({"textDocument":{"uri":document.uri}});
        if !matches!(query, Query::Diagnostics) {
            params["position"] = position.wire();
        }
        self.sync_document(document, cancel)?;
        match query {
            Query::References => params["context"] = json!({"includeDeclaration":true}),
            Query::Completion | Query::SemanticContext => {
                params["context"] = json!({"triggerKind":1})
            }
            Query::Diagnostics if !supported => {
                return self
                    .transport
                    .published_diagnostics(cancel, |method, params| {
                        answer(method, params, &self.config.settings, &self.folders)
                    })
            }
            _ => (),
        }
        let mut request = || {
            self.transport
                .request(method, params.clone(), cancel, |method, params| {
                    answer(method, params, &self.config.settings, &self.folders)
                })
        };
        let mut response = request()?;
        if starting && matches!(query, Query::SemanticContext) {
            // rust-analyzer can return null while loading a newly opened workspace.
            for _ in 0..6 {
                if !response.is_null() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(250));
                response = request()?;
            }
        }
        Ok(response)
    }

    fn sync_document(
        &mut self,
        mut document: OpenDocument,
        cancel: &Cancellation,
    ) -> Result<(), String> {
        let sync = &self.capabilities["textDocumentSync"];
        let change = sync.as_u64().or_else(|| sync["change"].as_u64());
        document.push_safe = self.document.is_none();
        if let Some(previous) = self.document.as_ref().filter(|previous| {
            previous.uri == document.uri && previous.language == document.language
        }) {
            if previous.text == document.text {
                return Ok(());
            }
            if let (Some(kind @ (1 | 2)), Some(version)) = (change, previous.version.checked_add(1))
            {
                let mut change = json!({"text":document.text});
                if kind == 2 {
                    change["range"] =
                        json!({"start":Position::default().wire(), "end":previous.end.wire()});
                }
                self.transport
                    .watch_diagnostics(&document.uri, version, false);
                self.transport.notify("textDocument/didChange", json!({
                    "textDocument":{"uri":document.uri,"version":version}, "contentChanges":[change]
                }), cancel)?;
                document.version = version;
                self.document = Some(document);
                return Ok(());
            }
        }
        if let Some(previous) = self.document.take() {
            self.transport.notify(
                "textDocument/didClose",
                json!({"textDocument":{"uri":previous.uri}}),
                cancel,
            )?;
        }
        self.transport
            .watch_diagnostics(&document.uri, document.version, document.push_safe);
        self.transport.notify("textDocument/didOpen", json!({"textDocument":{
            "uri":document.uri,"languageId":document.language,"version":document.version,"text":document.text
        }}), cancel)?;
        self.document = Some(document);
        Ok(())
    }

    fn reusable_diagnostics(&self, document: &OpenDocument) -> bool {
        self.capabilities["diagnosticProvider"].is_object()
            || self.capabilities["diagnosticProvider"] == true
            || self.document.as_ref().is_none_or(|previous| {
                previous.push_safe && previous.uri == document.uri && previous.text == document.text
            })
    }
}

fn answer(method: &str, params: &Value, settings: &Value, folders: &Value) -> Option<Value> {
    match method {
        "workspace/workspaceFolders" => Some(folders.clone()),
        "window/workDoneProgress/create" => Some(Value::Null),
        "workspace/configuration" => Some(Value::Array(
            params["items"]
                .as_array()?
                .iter()
                .take(64)
                .map(|item| {
                    let Some(section) = item["section"]
                        .as_str()
                        .filter(|section| !section.is_empty())
                    else {
                        return settings.clone();
                    };
                    section
                        .split('.')
                        .try_fold(settings, |value, key| value.get(key))
                        .cloned()
                        .unwrap_or_default()
                })
                .collect(),
        )),
        "workspace/applyEdit" => Some(json!({"applied":false})),
        _ => None,
    }
}

impl Work {
    pub(crate) fn run(
        &self,
        snapshot: &TaskSnapshot,
        cancel: &Cancellation,
        output: &mut TaskOutput,
    ) -> Result<(), TaskFailure> {
        match self.items(snapshot, cancel) {
            Ok((items, omitted)) => {
                for item in items {
                    output.push(item)?;
                }
                if omitted != 0 {
                    output.push(TaskItem::Message(format!(
                        "{omitted} elementos omitidos por alcance, formato o cuota"
                    )))?;
                }
                Ok(())
            }
            Err(_) if cancel.is_cancelled() => Err(TaskFailure::Cancelled),
            Err(message) => output.push(TaskItem::Problem(message)),
        }
    }

    fn items(
        &self,
        snapshot: &TaskSnapshot,
        cancel: &Cancellation,
    ) -> Result<(Vec<TaskItem>, usize), String> {
        let root = self
            .root
            .canonicalize()
            .map_err(|_| "No se pudo abrir la carpeta de proyecto LSP")?;
        let source = self
            .path
            .canonicalize()
            .map_err(|_| "No se pudo abrir la ruta del documento LSP")?;
        if !source.starts_with(&root) {
            return Err("El documento LSP debe estar dentro de la carpeta del proyecto".into());
        }
        let uri = file_uri(&source).ok_or("No se pudo representar el documento como URI local")?;
        let language = language_id(&self.profile, &source).ok_or("Lenguaje LSP no admitido")?;
        let encoding = snapshot.stamp.encoding;
        let bytes = snapshot.bytes();
        let mut text = String::with_capacity(bytes.len());
        for scalar in DecodedScalars::new(bytes, encoding, 0) {
            if !scalar.valid {
                return Err("El documento tiene bytes inválidos para su codificación".into());
            }
            text.push(scalar.character);
        }
        let mut positions = boundaries(bytes, encoding);
        let position = positions
            .find_map(|(offset, position)| (offset == self.cursor).then_some(position))
            .ok_or("El cursor no está en una posición válida para LSP")?;
        let end = positions.last().map_or(position, |(_, position)| position);
        let document = OpenDocument {
            uri,
            language,
            text,
            end,
            version: 1,
            push_safe: false,
        };
        let config = Config::load(&self.profile)?;
        let mut session = self
            .client
            .0
            .lock()
            .map_err(|_| "No se pudo recuperar la sesión LSP")?;
        if cancel.is_cancelled() {
            return Err("Se canceló la consulta LSP".into());
        }
        let mut existing = session.take();
        let reuse = existing.as_mut().is_some_and(|server| {
            server.root == root
                && server.config == config
                && server.transport.is_running()
                && (!matches!(self.query, Query::Diagnostics)
                    || server.reusable_diagnostics(&document))
        });
        let mut server = if reuse {
            existing.expect("matching server")
        } else {
            drop(existing);
            Server::start(root.clone(), config, cancel)?
        };
        let response = server.query(self.query, document, position, cancel, !reuse)?;
        *session = Some(server);
        drop(session);
        match self.query {
            Query::Completion => {
                language_items::completions(&response, bytes, encoding, self.cursor, position)
            }
            Query::SemanticContext => language_items::semantic_context(&response, self.cursor),
            Query::Diagnostics => language_items::diagnostics(&response, bytes, encoding),
            Query::Definition | Query::References => project_locations(
                &response, &root, &source, bytes, encoding, cancel,
            )
            .map(|(items, omitted)| (items.into_iter().map(TaskItem::Source).collect(), omitted)),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Position {
    line: usize,
    column: usize,
}

impl Position {
    fn wire(self) -> Value {
        json!({"line":self.line,"character":self.column})
    }
}

fn boundaries(
    bytes: &[u8],
    encoding: TextEncoding,
) -> impl Iterator<Item = (usize, Position)> + '_ {
    let mut scalars = DecodedScalars::new(bytes, encoding, 0).peekable();
    let mut position = Position::default();
    let mut eof = false;
    std::iter::from_fn(move || {
        let Some(scalar) = scalars.next() else {
            if std::mem::replace(&mut eof, true) {
                return None;
            }
            return Some((bytes.len(), position));
        };
        let current = (scalar.source_range.start, position);
        if matches!(scalar.character, '\n' | '\r') {
            if scalar.character == '\r' && scalars.peek().is_some_and(|next| next.character == '\n')
            {
                scalars.next();
            }
            position.line += 1;
            position.column = 0;
        } else {
            position.column += scalar.character.len_utf16();
        }
        Some(current)
    })
}

fn file_uri(path: &Path) -> Option<String> {
    let text = path.to_str()?;
    #[cfg(windows)]
    let text = text
        .strip_prefix(r"\\?\")
        .unwrap_or(text)
        .replace('\\', "/");
    #[cfg(windows)]
    if !text
        .as_bytes()
        .get(0..3)
        .is_some_and(|prefix| prefix[0].is_ascii_alphabetic() && prefix[1..] == *b":/")
    {
        return None;
    }
    let mut uri = String::from(if cfg!(windows) { "file:///" } else { "file://" });
    use std::fmt::Write;
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b'/' | b':') {
            uri.push(char::from(b));
        } else {
            write!(uri, "%{b:02X}").ok()?;
        }
    }
    Some(uri)
}

fn path_from_uri(uri: &str) -> Option<PathBuf> {
    if uri.len() > 4096 || uri.contains(['?', '#']) {
        return None;
    }
    let tail = uri.strip_prefix("file://")?;
    let tail = tail.strip_prefix("localhost").unwrap_or(tail);
    if !tail.starts_with('/') || tail.starts_with("//") {
        return None;
    }
    let mut decoded = Vec::with_capacity(tail.len());
    let mut bytes = tail.bytes();
    while let Some(byte) = bytes.next() {
        let byte = if byte == b'%' {
            let hi = char::from(bytes.next()?).to_digit(16)?;
            let lo = char::from(bytes.next()?).to_digit(16)?;
            let decoded = (hi * 16 + lo) as u8;
            if matches!(decoded, b'/' | b'\\') {
                return None;
            }
            decoded
        } else {
            byte
        };
        if byte.is_ascii_control() || byte == b'\\' {
            return None;
        }
        decoded.push(byte);
    }
    let text = std::str::from_utf8(&decoded).ok()?;
    #[cfg(windows)]
    let text = text.strip_prefix('/')?;
    #[cfg(windows)]
    if text.get(2..)?.contains(':') {
        return None;
    }
    let path = PathBuf::from(text);
    (path.is_absolute()
        && !path
            .components()
            .any(|part| part == std::path::Component::ParentDir))
    .then_some(path)
}

fn location(value: &Value) -> Option<(PathBuf, Position)> {
    let (uri, start) = match value.get("targetUri") {
        Some(uri) => (uri, &value["targetSelectionRange"]["start"]),
        None => (&value["uri"], &value["range"]["start"]),
    };
    Some((
        path_from_uri(uri.as_str()?)?,
        Position {
            line: usize::try_from(start["line"].as_u64()?).ok()?,
            column: usize::try_from(start["character"].as_u64()?).ok()?,
        },
    ))
}

fn project_locations(
    response: &Value,
    root: &Path,
    source: &Path,
    bytes: &[u8],
    encoding: TextEncoding,
    cancel: &Cancellation,
) -> Result<(Vec<SourceItem>, usize), String> {
    let locations = match response {
        Value::Null => &[],
        Value::Array(locations) => locations.as_slice(),
        Value::Object(_) => std::slice::from_ref(response),
        _ => return Err("Formato de ubicaciones LSP inválido".into()),
    };
    let mut omitted = locations.len().saturating_sub(RESULT_LIMIT);
    let mut unique = BTreeSet::new();
    let local_root = file_uri(root)
        .and_then(|uri| path_from_uri(&uri))
        .ok_or("Raíz LSP inválida")?;
    for raw in locations.iter().take(RESULT_LIMIT) {
        if cancel.is_cancelled() {
            return Err("Se canceló la consulta LSP".into());
        }
        let Some((path, position)) = location(raw) else {
            omitted += 1;
            continue;
        };
        if !inside_local_root(&path, &local_root)
            || !path
                .symlink_metadata()
                .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
        {
            omitted += 1;
            continue;
        }
        match path.canonicalize() {
            Ok(path) if path.starts_with(root) => {
                unique.insert((path, position));
            }
            _ => omitted += 1,
        }
    }
    let mut output = Vec::new();
    let mut read_budget = READ_LIMIT;
    let mut ordered = unique.into_iter().peekable();
    while let Some((path, first)) = ordered.next() {
        if cancel.is_cancelled() {
            return Err("Se canceló la consulta LSP".into());
        }
        let mut positions = vec![first];
        while ordered.peek().is_some_and(|(next, _)| next == &path) {
            positions.push(ordered.next().expect("peeked location").1);
        }
        let captured;
        let (target, encoding, fingerprint) = if path == source {
            (bytes, encoding, None)
        } else {
            let Ok((_, content)) = jc_files::read_in_root(root, &path, read_budget.min(FILE_LIMIT))
            else {
                omitted += positions.len();
                continue;
            };
            read_budget -= content.len();
            let Some(encoding) = jc_text::probe_encoding(&content, None).encoding else {
                omitted += positions.len();
                continue;
            };
            if std::str::from_utf8(&content).is_err() {
                omitted += positions.len();
                continue;
            }
            let version = jc_files::ContentFingerprint::of(&content);
            captured = content;
            (captured.as_slice(), encoding, Some(version))
        };
        let relative = path.strip_prefix(root).unwrap_or(&path).to_string_lossy();
        let mut offsets = boundaries(target, encoding).peekable();
        for position in positions {
            while offsets
                .peek()
                .is_some_and(|(_, current)| *current < position)
            {
                offsets.next();
            }
            let Some(&(offset, _)) = offsets.peek().filter(|(_, current)| *current == position)
            else {
                omitted += 1;
                continue;
            };
            let name: String = relative
                .chars()
                .filter(|c| !c.is_control())
                .take(110)
                .collect();
            let label = format!("{}:{}  {name}", position.line + 1, position.column + 1);
            let (action, offset) = match fingerprint {
                Some(version) => (
                    SourceAction::ProjectLocation {
                        path: path.clone(),
                        offset,
                        encoding,
                        version,
                    },
                    0,
                ),
                None => (SourceAction::Navigate, offset),
            };
            output.push(SourceItem {
                value_type: None,
                range: offset..offset,
                label,
                action,
            });
        }
    }
    Ok((output, omitted))
}

fn inside_local_root(path: &Path, root: &Path) -> bool {
    let mut parts = path.components();
    root.components().all(|expected| {
        parts.next().is_some_and(|actual| {
            actual == expected
                || cfg!(windows)
                    && actual
                        .as_os_str()
                        .to_str()
                        .zip(expected.as_os_str().to_str())
                        .is_some_and(|(actual, expected)| actual.eq_ignore_ascii_case(expected))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires an explicitly configured local stdio server and fixture project"]
    fn lsp_language_services_roundtrip() {
        use crate::{
            source_services::{SourceController, SourceMode, SourceRegistry, SourceState},
            task_panels::PanelKind,
            JobSystem, Workbench,
        };
        let root = PathBuf::from(std::env::var_os("CIDE_LSP_TEST_PROJECT").unwrap());
        let path = root.join(std::env::var_os("CIDE_LSP_TEST_FILE").unwrap());
        let source = std::fs::read_to_string(&path).unwrap();
        let mut app = Workbench::default();
        let tab = app.open_file(&path, Some(TextEncoding::Utf8)).unwrap();
        let cursor = [".calcu", "->calcu", " calcu"]
            .into_iter()
            .find_map(|prefix| source.find(prefix).map(|offset| offset + prefix.len()))
            .unwrap();
        assert!(app.jump_to_location(tab, cursor));
        let mut jobs = JobSystem::new(Default::default(), Default::default()).unwrap();
        let mut controller = SourceController::default();
        let registry = SourceRegistry::default();
        for mode in [SourceMode::LspCompletion, SourceMode::LspDiagnostics] {
            let deadline = std::time::Instant::now() + TIMEOUT + Duration::from_secs(5);
            loop {
                controller
                    .request(mode, &registry, &app, Some(&root), &mut jobs)
                    .unwrap();
                while matches!(
                    controller.state(),
                    SourceState::Queued | SourceState::Running
                ) {
                    for mut task in jobs.poll(&app, 8) {
                        controller.accept_editor(&mut app, Some(&root), &mut task);
                    }
                    controller.tick(&app, Some(&root), &mut jobs);
                    assert!(
                        std::time::Instant::now() < deadline,
                        "{mode:?}: {}",
                        controller.message()
                    );
                    std::thread::sleep(Duration::from_millis(10));
                }
                if mode == SourceMode::LspCompletion {
                    if let Some(index) = controller
                        .items()
                        .iter()
                        .position(|item| item.label.starts_with("calcular"))
                    {
                        controller
                            .activate_at_index(&mut app, Some(&root), index, false)
                            .unwrap();
                        assert!(std::str::from_utf8(&app.active_bytes().unwrap())
                            .unwrap()
                            .contains("calcular"));
                        app.undo().unwrap();
                        assert_eq!(app.active_bytes().unwrap(), source.as_bytes());
                        break;
                    }
                } else if let Some(id) = app
                    .task_panels()
                    .rows(PanelKind::Problems)
                    .iter()
                    .find(|row| row.text.contains("missingCideSymbol"))
                    .map(|row| row.id)
                {
                    app.activate_problem(id).unwrap();
                    assert_eq!(
                        app.active_editor().unwrap().cursor,
                        source.find("missingCideSymbol").unwrap()
                    );
                    app.insert_text("_").unwrap();
                    assert!(app.task_panels().rows(PanelKind::Problems).is_empty());
                    app.undo().unwrap();
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "{mode:?}: {} {:?}",
                    controller.message(),
                    controller.items()
                );
                std::thread::sleep(Duration::from_secs(2));
            }
            eprintln!("{mode:?}: {}", controller.message());
        }
        assert_eq!(std::fs::read_to_string(path).unwrap(), source);
    }

    #[test]
    fn positions_exclude_bom_crlf_and_surrogate_interiors() {
        let bytes = "\u{feff}😀a\r\nñ\rZ\n".as_bytes();
        let actual: Vec<_> = boundaries(bytes, TextEncoding::Utf8)
            .map(|(offset, p)| (offset, p.line, p.column))
            .collect();
        assert_eq!(
            actual,
            [
                (3, 0, 0),
                (7, 0, 2),
                (8, 0, 3),
                (10, 1, 0),
                (12, 1, 1),
                (13, 2, 0),
                (14, 2, 1),
                (15, 3, 0)
            ]
        );
    }

    #[test]
    fn file_locations_use_selection_range_and_reject_nonlocal_uris() {
        let root = if cfg!(windows) {
            Path::new("C:/prueba/ñ #.ts")
        } else {
            Path::new("/prueba/ñ #.ts")
        };
        let uri = file_uri(root).unwrap();
        assert!(uri.ends_with("/%C3%B1%20%23.ts"));
        let (path, position) = location(
            &json!({"targetUri":uri,"targetSelectionRange":{"start":{"line":4,"character":2}}}),
        )
        .unwrap();
        assert_eq!(path, root);
        assert_eq!(position, Position { line: 4, column: 2 });
        assert!(path_from_uri("https://example.org/file.ts").is_none());
        assert!(path_from_uri("file://host/file.ts").is_none());
        assert!(path_from_uri("file:///C:/prueba%2Ffile.ts").is_none());
    }

    #[test]
    #[ignore = "requires an explicitly configured local stdio server and fixture project"]
    fn lsp_server_roundtrip() {
        use crate::{
            source_services::{SourceController, SourceMode, SourceRegistry, SourceState},
            JobSystem, Workbench,
        };
        let root = PathBuf::from(std::env::var_os("CIDE_LSP_TEST_PROJECT").unwrap());
        let path = root.join(std::env::var_os("CIDE_LSP_TEST_FILE").unwrap());
        let dest = root
            .join(std::env::var_os("CIDE_LSP_TEST_DEST").unwrap())
            .canonicalize()
            .unwrap();
        let source = std::fs::read_to_string(&path).unwrap();
        let mut app = Workbench::default();
        let tab = app.open_file(path, Some(TextEncoding::Utf8)).unwrap();
        assert!(app.jump_to_pane_location(
            app.panes().active(),
            tab,
            source.rfind("calcular").unwrap()
        ));
        let mut jobs = JobSystem::new(Default::default(), Default::default()).unwrap();
        let mut controller = SourceController::default();
        let registry = SourceRegistry::default();
        for mode in [SourceMode::Definition, SourceMode::LspReferences] {
            let deadline = std::time::Instant::now() + TIMEOUT;
            let index = loop {
                controller
                    .request(mode, &registry, &app, Some(&root), &mut jobs)
                    .unwrap();
                while matches!(
                    controller.state(),
                    SourceState::Queued | SourceState::Running
                ) {
                    for mut task in jobs.poll(&app, 8) {
                        controller.accept(&app, Some(&root), &mut task);
                    }
                    controller.tick(&app, Some(&root), &mut jobs);
                    assert!(
                        std::time::Instant::now() < deadline,
                        "{mode:?}: {}",
                        controller.message()
                    );
                    std::thread::sleep(Duration::from_millis(10));
                }
                assert_eq!(
                    controller.state(),
                    SourceState::Ready,
                    "{mode:?}: {}",
                    controller.message()
                );
                let found = controller.items().iter().position(|item| matches!(&item.action, SourceAction::ProjectLocation { path, .. } if *path == dest));
                if mode == SourceMode::Definition && found.is_some()
                    || mode == SourceMode::LspReferences
                        && controller
                            .items()
                            .iter()
                            .any(|item| item.action == SourceAction::Navigate)
                {
                    break found;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "{mode:?}: {:?}",
                    controller.items()
                );
                std::thread::sleep(Duration::from_secs(2));
            };
            eprintln!("{mode:?}: {}", controller.message());
            if mode == SourceMode::LspReferences {
                let index = index.expect("declaration in references");
                controller
                    .activate_at_index(&mut app, Some(&root), index, true)
                    .unwrap();
                assert_eq!(app.active_path(), Some(dest.as_path()));
                assert!(app.panes().is_split());
            }
        }
    }
}
