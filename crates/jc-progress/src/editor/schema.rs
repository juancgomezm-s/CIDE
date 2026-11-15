//! Project-scoped, read-only schema refresh. Only the editor poll publishes a catalog.
use super::{avm_path, decode, invoke, load_profile, DatabaseAlias, JobDir};
use crate::{Action, BridgeRequest, CompletionStatus, EncodedText, SchemaRecord};
use jc_app::{Cancellation, Wakeup, Workbench};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
        Arc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

pub(super) mod browse;
mod completion;
pub(super) use completion::reference_name_at;

const MAX_RECORDS: usize = 8192;
const PANEL_RECORDS: usize = 256;

#[derive(Default)]
pub struct SchemaController {
    pending: Option<Pending>,
    catalog: Option<Arc<Catalog>>,
}

struct Pending {
    root: PathBuf,
    cancel: Cancellation,
    cancelled: bool,
    result: Receiver<Result<Catalog, String>>,
    worker: JoinHandle<()>,
}

struct Catalog {
    root: PathBuf,
    live: AtomicBool,
    records: Vec<SchemaRecord>,
    codec: String,
    aliases: Vec<DatabaseAlias>,
    version: String,
}

pub struct SchemaUpdate {
    pub message: &'static str,
    pub problems: bool,
    pub first_output_row: Option<u64>,
}

impl SchemaController {
    pub fn has_catalog(&self) -> bool {
        self.catalog.is_some()
    }

    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }

    pub fn request(&mut self, root: Option<&Path>, wakeup: Wakeup) -> Result<(), String> {
        if self.busy() {
            return Err("Ya hay una actualización de esquema en curso".into());
        }
        let root = root
            .ok_or("Abre primero una carpeta de proyecto Progress")?
            .to_owned();
        let work_root = root.clone();
        let cancel = Cancellation::new(Duration::from_secs(30));
        let worker_cancel = cancel.clone();
        let (sender, result) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("cide-abl-schema".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    load(&work_root, &worker_cancel)
                }))
                .unwrap_or_else(|_| Err("Falló el lector de esquema ABL".into()));
                let _ = sender.send(result);
                wakeup.signal();
            })
            .map_err(|e| format!("No se pudo iniciar la lectura de esquema: {e}"))?;
        self.pending = Some(Pending {
            root,
            cancel,
            cancelled: false,
            result,
            worker,
        });
        Ok(())
    }

    pub fn cancel(&mut self) -> bool {
        let Some(pending) = &mut self.pending else {
            return false;
        };
        pending.cancelled = true;
        pending.cancel.cancel();
        true
    }

    pub fn clear(&mut self) {
        self.cancel();
        self.retire_catalog();
    }

    fn retire_catalog(&mut self) {
        if let Some(catalog) = self.catalog.take() {
            catalog.live.store(false, Ordering::Release);
        }
    }

    pub fn poll(&mut self, root: Option<&Path>, app: &mut Workbench) -> Option<SchemaUpdate> {
        let pending = self.pending.as_ref()?;
        let result = match pending.result.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => {
                Err("El lector de esquema terminó sin respuesta".into())
            }
        };
        let pending = self.pending.take().unwrap();
        let _ = pending.worker.join();
        if root != Some(pending.root.as_path()) {
            return None;
        }
        if pending.cancelled {
            app.push_output("Actualización de esquema ABL cancelada");
            return Some(SchemaUpdate {
                message: "Actualización de esquema ABL cancelada",
                problems: false,
                first_output_row: None,
            });
        }
        let update = match result {
            Ok(catalog) => {
                let first_output_row = catalog.write_output(app);
                self.retire_catalog();
                self.catalog = Some(Arc::new(catalog));
                SchemaUpdate {
                    message: "Esquema ABL actualizado · consulta Salida",
                    problems: false,
                    first_output_row,
                }
            }
            Err(error) => {
                app.push_problem(format!("Esquema ABL: {error}"));
                if self.catalog.is_some() {
                    app.push_output("Esquema ABL: se conserva el último catálogo válido");
                }
                SchemaUpdate {
                    message: "No se pudo actualizar el esquema ABL · consulta Problemas",
                    problems: true,
                    first_output_row: None,
                }
            }
        };
        Some(update)
    }

    pub fn show(&self, app: &mut Workbench) -> Option<u64> {
        self.catalog.as_ref()?.write_output(app)
    }
}

impl Drop for SchemaController {
    fn drop(&mut self) {
        self.clear();
        if let Some(pending) = self.pending.take() {
            let _ = pending.worker.join();
        }
    }
}

fn load(root: &Path, cancel: &Cancellation) -> Result<Catalog, String> {
    let profile = load_profile(root)?;
    let job = JobDir::create().map_err(|e| format!("No se pudo crear el trabajo ABL: {e}"))?;
    let request = BridgeRequest {
        action: Action::Schema,
        source: None,
        output_dir: avm_path(&job.path),
        propath: None,
    };
    let result = invoke(request, root, &profile, cancel);
    if cancel.is_cancelled() {
        return Err("Se agotó el tiempo o se canceló la lectura de esquema".into());
    }
    let result = result?;
    let response = result.response.map_err(|error| {
        let log = decode(&result.log, &result.cpstream);
        let detail: String = log
            .lines()
            .take(3)
            .collect::<Vec<_>>()
            .join(" · ")
            .chars()
            .take(512)
            .collect();
        format!("{error} ({}) {detail}", result.status)
    })?;
    if !result.status.success() || response.status != CompletionStatus::Ok {
        let messages = response
            .diagnostics
            .iter()
            .take(4)
            .map(|d| {
                decode(d.message.as_bytes(), &response.cpinternal)
                    .chars()
                    .take(256)
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join(" · ");
        return Err(if messages.is_empty() {
            format!("El AVM terminó con {}", result.status)
        } else {
            messages
        });
    }
    validate(&response.schema)?;
    Ok(Catalog {
        root: root.to_owned(),
        live: AtomicBool::new(true),
        records: response.schema,
        codec: response.cpinternal,
        aliases: profile.aliases,
        version: response
            .avm_version
            .ok_or("Falta la versión del AVM en el catálogo")?,
    })
}

fn validate(records: &[SchemaRecord]) -> Result<(), String> {
    if records.len() > MAX_RECORDS {
        return Err("El catálogo supera 8192 registros".into());
    }
    let mut names = BTreeSet::new();
    let (mut db, mut current_table, mut current_index) = (None, None, None);
    for record in records {
        match record {
            SchemaRecord::Database { name }
                if !name.0.is_empty() && names.insert(name.as_bytes()) =>
            {
                db = Some(name);
                current_table = None;
                current_index = None;
            }
            SchemaRecord::Table { database, table }
                if db == Some(database) && !table.0.is_empty() =>
            {
                current_table = Some(table);
                current_index = None;
            }
            SchemaRecord::Field {
                database,
                table,
                name,
                ..
            } if db == Some(database) && current_table == Some(table) && !name.0.is_empty() => {}
            SchemaRecord::Index {
                database,
                table,
                name,
                ..
            } if db == Some(database) && current_table == Some(table) && !name.0.is_empty() => {
                current_index = Some(name);
            }
            SchemaRecord::IndexField {
                database,
                table,
                index,
                field,
                sequence,
                ..
            } if db == Some(database)
                && current_table == Some(table)
                && current_index == Some(index)
                && !field.0.is_empty()
                && *sequence > 0 => {}
            _ => return Err("El AVM devolvió un catálogo incompleto o incoherente".into()),
        }
    }
    if names.is_empty() {
        return Err("El AVM no devolvió bases conectadas".into());
    }
    Ok(())
}

impl Catalog {
    fn write_output(&self, app: &mut Workbench) -> Option<u64> {
        let (mut databases, mut tables, mut fields, mut indexes) = (0, 0, 0, 0);
        for record in &self.records {
            match record {
                SchemaRecord::Database { .. } => databases += 1,
                SchemaRecord::Table { .. } => tables += 1,
                SchemaRecord::Field { .. } => fields += 1,
                SchemaRecord::Index { .. } => indexes += 1,
                SchemaRecord::IndexField { .. } => {}
            }
        }
        app.push_output(format!("Esquema ABL · bases {databases} · tablas {tables} · campos {fields} · índices {indexes} · OpenEdge AVM {}", self.version));
        let first = app.output().last().map(|row| row.id);
        for record in self.records.iter().take(PANEL_RECORDS) {
            app.push_output(self.line(record));
        }
        if self.records.len() > PANEL_RECORDS {
            app.push_output(format!(
                "Vista parcial: {PANEL_RECORDS} de {} entradas del catálogo",
                self.records.len()
            ));
        }
        first
    }

    fn line(&self, record: &SchemaRecord) -> String {
        let text = |value: &EncodedText| {
            decode(value.as_bytes(), &self.codec)
                .chars()
                .map(|ch| if ch.is_control() { ' ' } else { ch })
                .take(96)
                .collect::<String>()
        };
        match record {
            SchemaRecord::Database { name } => {
                let name = text(name);
                let aliases = self
                    .aliases
                    .iter()
                    .filter(|a| a.database.eq_ignore_ascii_case(&name))
                    .map(|a| a.name.as_str())
                    .collect::<Vec<_>>();
                if aliases.is_empty() {
                    format!("Base {name}")
                } else {
                    format!("Base {name} · aliases {}", aliases.join(", "))
                }
            }
            SchemaRecord::Table { database, table } => {
                format!("Tabla {}.{}", text(database), text(table))
            }
            SchemaRecord::Field {
                name,
                data_type,
                extent,
                ..
            } => {
                let suffix = if *extent > 0 {
                    format!("[{extent}]")
                } else {
                    String::new()
                };
                format!("  Campo {} · {}{suffix}", text(name), text(data_type))
            }
            SchemaRecord::Index {
                name,
                primary,
                unique,
                ..
            } => format!(
                "  Índice {}{}{}",
                text(name),
                if *primary { " · principal" } else { "" },
                if *unique { " · único" } else { "" },
            ),
            SchemaRecord::IndexField {
                field,
                sequence,
                ascending,
                ..
            } => format!(
                "    {sequence}. {} · {}",
                text(field),
                if *ascending {
                    "ascendente"
                } else {
                    "descendente"
                }
            ),
        }
    }
}
