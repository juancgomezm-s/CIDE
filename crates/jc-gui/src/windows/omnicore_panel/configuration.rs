//! Read-only configuration review through OmniCore's own loader, off the UI thread.
use super::*;
use jc_app::Wakeup;
use serde_json::Value;
use std::{
    io::Read,
    os::windows::process::CommandExt,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

const INVALID: &str = "No se pudo leer la configuración de OmniCore. Comprueba sus archivos y el acceso de tu usuario.";
const LIMIT: usize = 128 * 1024;

#[derive(Debug)]
pub(super) struct Snapshot {
    directory: String,
    default_model: String,
    model_count: u64,
    legacy: bool,
    providers: Vec<Provider>,
}
#[derive(Debug)]
struct Provider {
    id: String,
    kind: String,
    address: String,
    authentication: String,
    billing: String,
}

pub(super) struct Request {
    cancelled: Arc<AtomicBool>,
    result: Receiver<Result<Snapshot, String>>,
}
impl Request {
    fn start(executable: PathBuf, wakeup: Wakeup) -> Result<Self, String> {
        let (send, result) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let running = cancelled.clone();
        thread::Builder::new()
            .name("cide-omnicore-ajustes".into())
            .spawn(move || {
                if send.send(read(&executable, &running)).is_ok() {
                    wakeup.signal();
                }
            })
            .map_err(|_| INVALID)?;
        Ok(Self { cancelled, result })
    }
    fn poll(&self) -> Option<Result<Snapshot, String>> {
        match self.result.try_recv() {
            Ok(value) => Some(value),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(INVALID.into())),
        }
    }
}
impl Drop for Request {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}
fn read(executable: &Path, cancelled: &AtomicBool) -> Result<Snapshot, String> {
    let bridge = omnicore_runtime::context_bridge(executable).ok_or(
        "Esta instalación necesita el componente de configuración de OmniCore actualizado.",
    )?;
    let config = super::super::omnicore_transport::user_config_dir()?;
    let mut child = Command::new(bridge)
        .arg("config")
        .env("OMNICORE_CONFIG_DIR", config)
        .creation_flags(0x0800_0000)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| INVALID)?;
    let result = (|| {
        let stdout = child.stdout.take().ok_or(INVALID)?;
        let (send, receive) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name("cide-omnicore-ajustes-lectura".into())
            .spawn(move || {
                let mut bytes = Vec::new();
                let result = stdout
                    .take((LIMIT + 1) as u64)
                    .read_to_end(&mut bytes)
                    .map(|_| bytes);
                let _ = send.send(result);
            })
            .map_err(|_| INVALID)?;
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut bytes = None;
        loop {
            if cancelled.load(Ordering::Relaxed) || Instant::now() >= deadline {
                return Err(INVALID.into());
            }
            if bytes.is_none() {
                match receive.recv_timeout(Duration::from_millis(40)) {
                    Ok(Ok(value)) if value.len() <= LIMIT => bytes = Some(value),
                    Ok(_) | Err(mpsc::RecvTimeoutError::Disconnected) => return Err(INVALID.into()),
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                }
            }
            if let Some(status) = child.try_wait().map_err(|_| INVALID)? {
                if !status.success() {
                    return Err(INVALID.into());
                }
                return decode(bytes.as_ref().unwrap());
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
fn decode(bytes: &[u8]) -> Result<Snapshot, String> {
    if bytes.len() > LIMIT {
        return Err(INVALID.into());
    }
    let root: Value = serde_json::from_slice(bytes).map_err(|_| INVALID)?;
    if root["schemaVersion"] != 1 || root.get("error").is_some() {
        return Err(INVALID.into());
    }
    let text = |value: &Value, key: &str, max: usize| -> Result<String, String> {
        value[key]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= max && !s.chars().any(char::is_control))
            .map(str::to_owned)
            .ok_or_else(|| INVALID.into())
    };
    let directory = text(&root, "configDirectory", 4096)?;
    let default_model = if root["defaultModel"].is_null() {
        "Automático".into()
    } else {
        text(&root, "defaultModel", 512)?
    };
    let model_count = root["modelCount"]
        .as_u64()
        .filter(|n| *n <= 1024)
        .ok_or(INVALID)?;
    let entries = root["providers"]
        .as_array()
        .filter(|p| p.len() <= 128)
        .ok_or(INVALID)?;
    let mut providers = Vec::new();
    for value in entries {
        providers.push(Provider {
            id: text(value, "id", 512)?,
            kind: text(value, "kind", 512)?,
            address: text(value, "address", 4096)?,
            authentication: text(value, "authentication", 512)?,
            billing: text(value, "billing", 512)?,
        });
    }
    providers.sort_by(|a, b| a.id.to_lowercase().cmp(&b.id.to_lowercase()));
    Ok(Snapshot {
        directory,
        default_model,
        model_count,
        legacy: root["legacyAuthentication"].as_bool().ok_or(INVALID)?,
        providers,
    })
}

impl State {
    pub(super) fn refresh_omnicore_configuration(&mut self) {
        if self.omnicore.configuration_request.is_some() {
            return;
        }
        let executable = self
            .omnicore
            .executable
            .clone()
            .or_else(omnicore_runtime::discover);
        let request = executable
            .ok_or_else(|| "OmniCore no está incluido en esta instalación.".to_owned())
            .and_then(|exe| Request::start(exe, self.wakeup.clone()));
        self.omnicore.configuration_first = 0;
        match request {
            Ok(request) => self.omnicore.configuration_request = Some(request),
            Err(error) => self.omnicore.configuration = Some(Err(error)),
        }
        self.omnicore.invalidate();
    }
    pub(super) fn poll_omnicore_configuration(&mut self) -> bool {
        let Some(result) = self
            .omnicore
            .configuration_request
            .as_ref()
            .and_then(Request::poll)
        else {
            return false;
        };
        self.omnicore.configuration_request = None;
        self.omnicore.configuration = Some(result);
        self.omnicore.invalidate();
        true
    }
    pub(super) fn omnicore_configuration_presentation(
        &self,
        rect: Rect,
        mut buttons: Vec<(Rect, Target)>,
        mut labels: Vec<(Rect, String, u32, u32)>,
    ) -> Frame {
        let dpi = self.dpi;
        let px = |v| dpi.pixels(v);
        let panel = &self.omnicore;
        let viewport = Rect {
            left: rect.left + px(12),
            right: rect.right - px(12),
            top: rect.top + px(121),
            bottom: rect.bottom - px(49),
        };
        let activity = Rect {
            left: rect.left + px(12),
            right: rect.right - px(12),
            top: rect.top + px(86),
            bottom: rect.top + px(114),
        };
        labels.push((
            activity,
            format!(
                "{} Mostrar herramientas y razonamiento",
                if panel.show_activity { "☑" } else { "☐" }
            ),
            shell::TEXT,
            shell::SURFACE,
        ));
        buttons.push((activity, Target::ToggleActivity));
        let mut cards = Vec::new();
        let mut sections: Vec<(String, Vec<String>)> = Vec::new();
        let selected = panel.model.as_ref().map_or("Automático", |m| m.id.as_str());
        let provider = panel
            .model
            .as_ref()
            .map_or("Según el modelo", |m| m.provider.as_str());
        let context = panel
            .model
            .as_ref()
            .and_then(|m| m.input_tokens)
            .map_or("Por consultar".into(), |n| {
                format!("{n} tokens de entrada disponibles")
            });
        sections.push((
            "Modelo de esta conversación".into(),
            vec![selected.into(), format!("Proveedor · {provider}"), context],
        ));
        if panel.configuration_request.is_some() {
            sections.push((
                "Leyendo configuración…".into(),
                vec!["El chat y tu borrador se conservan.".into()],
            ));
        }
        match &panel.configuration {
            Some(Ok(config)) => {
                sections.push((
                    "Configuración de OmniCore".into(),
                    vec![
                        format!("Modelo predeterminado · {}", config.default_model),
                        format!(
                            "{} modelos configurados · {} proveedores",
                            config.model_count,
                            config.providers.len()
                        ),
                        config.directory.clone(),
                    ],
                ));
                for provider in &config.providers {
                    sections.push((
                        provider.id.clone(),
                        vec![
                            format!("Tipo · {}", provider.kind),
                            provider.address.clone(),
                            format!("Acceso · {}", provider.authentication),
                            format!("Facturación · {}", provider.billing),
                        ],
                    ));
                }
                if config.legacy {
                    sections.push(("Configuración antigua detectada".into(),vec!["Algún proveedor usa el formato auth anterior. Las credenciales no se muestran aquí.".into()]));
                }
            }
            Some(Err(error)) => sections.push((
                "No se pudo leer la configuración".into(),
                vec![error.clone()],
            )),
            None => {}
        }
        sections.push(("Revisión de configuración".into(),vec!["Actualizar vuelve a leer los ajustes y los modelos. Cambiar modelo aplica a esta conversación.".into(),"La conexión se comprueba al enviar una consulta desde Chat.".into()]));
        let columns = ((viewport.right - viewport.left - px(24)) / px(7).max(1)).max(12) as usize;
        let wrapped: Vec<_> = sections
            .into_iter()
            .map(|(title, body)| {
                let lines: Vec<String> = body.iter().flat_map(|line| wrap(line, columns)).collect();
                (title, lines)
            })
            .collect();
        let total: i32 = wrapped
            .iter()
            .map(|(_, lines)| px(42 + lines.len() as i32 * 20))
            .sum();
        let max_offset = (total - (viewport.bottom - viewport.top)).max(0);
        let offset = px((panel.configuration_first.min(2048) as i32) * 20).min(max_offset);
        let mut y = viewport.top - offset;
        for (title, lines) in wrapped {
            let height = px(34 + lines.len() as i32 * 20);
            let card = Rect {
                top: y.max(viewport.top),
                bottom: (y + height).min(viewport.bottom),
                ..viewport
            };
            if card.bottom > card.top {
                cards.push(card);
            }
            for (index, line) in std::iter::once(&title).chain(lines.iter()).enumerate() {
                let top = y + px(9 + index as i32 * 20);
                let area = Rect {
                    left: viewport.left + px(12),
                    right: viewport.right - px(10),
                    top,
                    bottom: top + px(19),
                };
                if area.top >= viewport.top && area.bottom <= viewport.bottom {
                    labels.push((
                        area,
                        line.clone(),
                        if index == 0 {
                            shell::ACCENT
                        } else {
                            shell::TEXT
                        },
                        shell::SIDEBAR,
                    ));
                }
            }
            y += height + px(8);
        }
        let middle = (viewport.left + viewport.right) / 2;
        let refresh = Rect {
            left: viewport.left,
            right: middle - px(4),
            top: rect.bottom - px(38),
            bottom: rect.bottom - px(9),
        };
        let choose = Rect {
            left: middle + px(4),
            right: viewport.right,
            ..refresh
        };
        let busy = panel.configuration_request.is_some() || panel.catalog_request.is_some();
        labels.push((
            refresh,
            if busy {
                "Actualizando…"
            } else {
                "Actualizar"
            }
            .into(),
            shell::TEXT,
            shell::RAISED,
        ));
        if !busy {
            buttons.push((refresh, Target::RefreshConfiguration));
        }
        let available = matches!(&panel.model_catalog, Some(Ok(_)));
        labels.push((
            choose,
            "Cambiar modelo…".into(),
            if available { shell::TEXT } else { shell::MUTED },
            shell::RAISED,
        ));
        if available {
            buttons.push((choose, Target::ChooseModel));
        }
        Frame {
            hits: Hits {
                stamp: panel.stamp.clone(),
                rect,
                buttons,
                configuration_maximum: (max_offset / px(20).max(1)
                    + i32::from(max_offset % px(20).max(1) != 0))
                    as usize,
                draft: Vec::new(),
            },
            labels,
            dpi,
            composer_card: None,
            cards,
            conversation: Vec::new(),
            draft: None,
            folder_open: false,
        }
    }
}
fn wrap(text: &str, columns: usize) -> Vec<String> {
    let chars: Vec<_> = text.chars().collect();
    chars
        .chunks(columns)
        .map(|part| part.iter().collect())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn configuration_rejects_bad_schema_limits_and_untrusted_error_detail() {
        let mut value = serde_json::json!({"schemaVersion":1,"configDirectory":"C:/fixture","defaultModel":null,"modelCount":2,"legacyAuthentication":false,"providers":[]});
        assert_eq!(
            decode(value.to_string().as_bytes()).unwrap().default_model,
            "Automático"
        );
        value["error"] = "secret-value".into();
        assert!(!decode(value.to_string().as_bytes())
            .unwrap_err()
            .contains("secret-value"));
        value.as_object_mut().unwrap().remove("error");
        value["modelCount"] = 1025.into();
        assert!(decode(value.to_string().as_bytes()).is_err());
        assert!(decode(&vec![b' '; LIMIT + 1]).is_err());
    }
    #[test]
    fn configuration_only_retains_review_fields() {
        let value = serde_json::json!({"schemaVersion":1,"configDirectory":"C:/fixture","defaultModel":"demo","modelCount":1,"legacyAuthentication":true,"providers":[{"id":"local","kind":"OpenAICompatible","address":"http://localhost:8080/","authentication":"Clave mediante referencia","billing":"Local","apiKey":"secret-sentinel","authRef":"secret-ref"}]});
        let observed = format!("{:?}", decode(value.to_string().as_bytes()).unwrap());
        assert!(!observed.contains("secret-sentinel"));
        assert!(!observed.contains("secret-ref"));
        assert!(observed.contains("localhost"));
    }
}
