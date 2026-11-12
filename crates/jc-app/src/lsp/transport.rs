use crate::Cancellation;
use serde_json::{json, Value};
use std::{
    io::{self, BufRead, BufReader, Read, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver, SyncSender},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

const MAX_RESPONSE: usize = 4 * 1024 * 1024;
const MAX_REQUEST: usize = 16 * 1024 * 1024;
const POLL: Duration = Duration::from_millis(25);

pub(super) struct Transport {
    child: Child,
    outgoing: SyncSender<Vec<u8>>,
    written: Receiver<io::Result<()>>,
    incoming: Receiver<io::Result<Value>>,
    id: u64,
    diagnostics: Arc<Mutex<DiagnosticsInbox>>,
}

#[derive(Default)]
struct DiagnosticsInbox {
    path: Option<PathBuf>,
    version: i32,
    allow_unversioned: bool,
    report: Option<(Instant, Value)>,
}

impl DiagnosticsInbox {
    fn publish(&mut self, params: &mut Value) {
        let reported = params["uri"].as_str().and_then(super::path_from_uri);
        let matches =
            self.path
                .as_ref()
                .zip(reported.as_ref())
                .is_some_and(|(expected, reported)| {
                    super::inside_local_root(expected, reported)
                        && super::inside_local_root(reported, expected)
                });
        let version = &params["version"];
        if matches
            && (version.as_i64() == Some(i64::from(self.version))
                || version.is_null() && self.allow_unversioned)
            && params["diagnostics"].is_array()
        {
            self.report = Some((Instant::now(), params["diagnostics"].take()));
        }
    }
}

impl Transport {
    pub(super) fn spawn(command: &mut Command) -> Result<Self, String> {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("No se pudo iniciar el servidor LSP: {error}"))?;
        let mut input = child.stdin.take().expect("piped stdin");
        let mut output = BufReader::new(child.stdout.take().expect("piped stdout"));
        let (outgoing, write_queue) = mpsc::sync_channel::<Vec<u8>>(1);
        let (write_done, written) = mpsc::sync_channel(1);
        let (read_queue, incoming) = mpsc::sync_channel(2);
        let diagnostics = Arc::new(Mutex::new(DiagnosticsInbox::default()));
        let inbox = diagnostics.clone();
        let transport = Self {
            child,
            outgoing,
            written,
            incoming,
            id: 0,
            diagnostics,
        };
        thread::Builder::new()
            .name("cide-lsp-write".into())
            .spawn(move || {
                while let Ok(bytes) = write_queue.recv() {
                    let result = write!(input, "Content-Length: {}\r\n\r\n", bytes.len())
                        .and_then(|()| input.write_all(&bytes));
                    let failed = result.is_err();
                    if write_done.send(result).is_err() || failed {
                        break;
                    }
                }
            })
            .map_err(|_| "No se pudo abrir la entrada del servidor LSP")?;
        thread::Builder::new()
            .name("cide-lsp-read".into())
            .spawn(move || loop {
                let result = read_frame(&mut output);
                if result.as_ref().is_ok_and(|message| {
                    message.get("method").is_some() && message.get("id").is_none()
                }) {
                    if let Ok(mut message) = result {
                        if message["jsonrpc"] == "2.0"
                            && message["method"] == "textDocument/publishDiagnostics"
                        {
                            inbox
                                .lock()
                                .unwrap_or_else(|error| error.into_inner())
                                .publish(&mut message["params"]);
                        }
                    }
                    continue;
                }
                let failed = result.is_err();
                if read_queue.send(result).is_err() || failed {
                    break;
                }
            })
            .map_err(|_| "No se pudo abrir la salida del servidor LSP")?;
        Ok(transport)
    }

    pub(super) fn watch_diagnostics(&self, uri: &str, version: i32, allow_unversioned: bool) {
        *self
            .diagnostics
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = DiagnosticsInbox {
            path: super::path_from_uri(uri),
            version,
            allow_unversioned,
            report: None,
        };
    }

    pub(super) fn published_diagnostics(
        &self,
        cancel: &Cancellation,
        answer: impl Fn(&str, &Value) -> Option<Value>,
    ) -> Result<Value, String> {
        loop {
            if cancel.is_cancelled() {
                return Err("Se canceló la consulta LSP".into());
            }
            if let Some(report) = self
                .diagnostics
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .report
                .as_ref()
                .filter(|(when, _)| when.elapsed() >= Duration::from_millis(250))
                .map(|(_, report)| report.clone())
            {
                return Ok(report);
            }
            match self.incoming.recv_timeout(POLL) {
                Ok(Ok(message)) => self.respond(&message, cancel, &answer)?,
                Ok(Err(error)) => return Err(format!("Comunicación LSP: {error}")),
                Err(mpsc::RecvTimeoutError::Timeout) => (),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("El servidor LSP se desconectó".into())
                }
            }
        }
    }

    fn respond(
        &self,
        message: &Value,
        cancel: &Cancellation,
        answer: &impl Fn(&str, &Value) -> Option<Value>,
    ) -> Result<(), String> {
        if let (Some(method), Some(id)) = (message["method"].as_str(), message.get("id")) {
            let response = match answer(method, &message["params"]) {
                Some(result) => json!({"jsonrpc":"2.0", "id":id, "result":result}),
                None => {
                    json!({"jsonrpc":"2.0", "id":id, "error":{"code":-32601, "message":"Method not supported"}})
                }
            };
            self.send(response, cancel)?;
        }
        Ok(())
    }

    fn send(&self, value: Value, cancel: &Cancellation) -> Result<(), String> {
        let bytes =
            serde_json::to_vec(&value).map_err(|_| "No se pudo codificar el mensaje LSP")?;
        if bytes.len() > MAX_REQUEST {
            return Err("El mensaje LSP excede 16 MiB".into());
        }
        self.outgoing
            .try_send(bytes)
            .map_err(|_| "Se cerró la entrada del servidor LSP")?;
        receive(&self.written, cancel)
    }

    pub(super) fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    pub(super) fn shutdown(&mut self) {
        if !self.is_running() {
            return;
        }
        let deadline = Instant::now() + Duration::from_millis(200);
        let cancel = Cancellation::new(deadline.saturating_duration_since(Instant::now()));
        if self
            .request("shutdown", Value::Null, &cancel, |_, _| None)
            .is_ok()
            && self.notify("exit", Value::Null, &cancel).is_ok()
        {
            while Instant::now() < deadline && self.is_running() {
                thread::sleep(Duration::from_millis(5));
            }
        }
    }

    pub(super) fn notify(
        &self,
        method: &str,
        params: Value,
        cancel: &Cancellation,
    ) -> Result<(), String> {
        self.send(
            json!({"jsonrpc":"2.0", "method":method, "params":params}),
            cancel,
        )
    }

    pub(super) fn request(
        &mut self,
        method: &str,
        params: Value,
        cancel: &Cancellation,
        answer: impl Fn(&str, &Value) -> Option<Value>,
    ) -> Result<Value, String> {
        self.id = self
            .id
            .checked_add(1)
            .ok_or("Se agotaron los identificadores LSP")?;
        self.send(
            json!({"jsonrpc":"2.0", "id":self.id, "method":method, "params":params}),
            cancel,
        )?;
        for _ in 0..1024 {
            let mut message = receive(&self.incoming, cancel)?;
            if message["jsonrpc"] != "2.0" {
                return Err("El servidor no respondió con JSON-RPC 2.0".into());
            }
            if message["method"].is_string() {
                self.respond(&message, cancel, &answer)?;
            } else if message["id"].as_u64() == Some(self.id) {
                if let Some(result) = message.get_mut("result") {
                    return Ok(result.take());
                }
                let code = message["error"]["code"]
                    .as_i64()
                    .ok_or("Respuesta LSP inválida")?;
                let detail: String = message["error"]["message"]
                    .as_str()
                    .unwrap_or("")
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(100)
                    .collect();
                return Err(format!(
                    "El servidor LSP rechazó {method} ({code}): {detail}"
                ));
            }
        }
        Err("El servidor LSP excedió la cuota de mensajes por consulta".into())
    }
}

impl Drop for Transport {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn receive<T>(channel: &Receiver<io::Result<T>>, cancel: &Cancellation) -> Result<T, String> {
    loop {
        if cancel.is_cancelled() {
            return Err("Se canceló la consulta LSP".into());
        }
        match channel.recv_timeout(POLL) {
            Ok(result) => return result.map_err(|error| format!("Comunicación LSP: {error}")),
            Err(mpsc::RecvTimeoutError::Timeout) => (),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("El servidor LSP se desconectó".into())
            }
        }
    }
}

fn read_frame(reader: &mut impl BufRead) -> io::Result<Value> {
    let mut header = Vec::new();
    let mut length = None;
    let mut remaining = 8192_u64;
    loop {
        header.clear();
        let read = reader.take(remaining).read_until(b'\n', &mut header)?;
        remaining -= read as u64;
        if read == 0 || !header.ends_with(b"\r\n") {
            return Err(io::Error::other(
                "cabecera LSP incompleta o demasiado grande",
            ));
        }
        if header == b"\r\n" {
            break;
        }
        let field = std::str::from_utf8(&header[..header.len() - 2])
            .ok()
            .and_then(|line| line.split_once(':'))
            .ok_or_else(|| io::Error::other("cabecera LSP inválida"))?;
        if field.0.eq_ignore_ascii_case("Content-Length") {
            let size = field
                .1
                .trim()
                .parse::<usize>()
                .ok()
                .filter(|n| (1..=MAX_RESPONSE).contains(n));
            if length.is_some() || size.is_none() {
                return Err(io::Error::other(
                    "Content-Length LSP inválido o fuera de cuota",
                ));
            }
            length = size;
        }
    }
    let mut bytes = vec![0; length.ok_or_else(|| io::Error::other("falta Content-Length LSP"))?];
    reader.read_exact(&mut bytes)?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pushed_diagnostics_ignore_other_documents_and_old_versions() {
        let uri = "file:///C:/project/file.ts";
        let mut inbox = DiagnosticsInbox {
            path: super::super::path_from_uri(uri),
            version: 2,
            ..Default::default()
        };
        inbox.publish(&mut json!({"uri":uri,"version":1,"diagnostics":[1]}));
        inbox.publish(&mut json!({"uri":uri,"diagnostics":[2]}));
        inbox.publish(
            &mut json!({"uri":"file:///C:/project/other.ts","version":2,"diagnostics":[3]}),
        );
        assert!(inbox.report.is_none());
        inbox.publish(&mut json!({"uri":uri,"version":2,"diagnostics":[4]}));
        assert_eq!(inbox.report.as_ref().unwrap().1, json!([4]));
        inbox.publish(&mut json!({"uri":uri,"version":2,"diagnostics":[]}));
        assert_eq!(inbox.report.unwrap().1, json!([]));
    }

    #[test]
    fn frames_are_byte_counted_and_bounded() {
        let mut frames = &b"Content-Length: 4\r\n\r\n\"\xc3\xb1\"Content-Length: 4\r\n\r\nnull"[..];
        assert_eq!(read_frame(&mut frames).unwrap(), "ñ");
        assert_eq!(read_frame(&mut frames).unwrap(), Value::Null);
        assert!(read_frame(&mut &b"Content-Length: 4294967295\r\n\r\n"[..]).is_err());
        assert!(read_frame(&mut &b"Content-Length: 1\r\nContent-Length: 1\r\n\r\n1"[..]).is_err());
    }
}
