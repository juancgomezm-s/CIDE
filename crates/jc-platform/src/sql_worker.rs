//! One bounded request at a time on a worker that owns the SQL connection.
use crate::sql::{
    ConnectOptions, Engine, Page, PageRequest, Relation, RelationDetails, Session, Tls,
};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
};

/// The saved CIDE profile passes its eight ordered field slots here.
/// MySQL leaves the eighth slot empty and uses its database as schema.
pub fn options(engine: Engine, values: [&str; 8]) -> Result<ConnectOptions, String> {
    let [_, server, port, database, user, tls, credential, schemas] = values.map(str::trim);
    let port = port
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
        .ok_or("Puerto SQL inválido")?;
    let tls = match tls {
        "verificar-servidor" => Tls::VerifyServer,
        "requerido" => Tls::Required,
        "deshabilitado" => Tls::Disabled,
        _ => return Err("Configuración TLS inválida".into()),
    };
    Ok(ConnectOptions {
        engine,
        server: server.into(),
        port,
        database: database.into(),
        user: user.into(),
        tls,
        credential: (!credential.is_empty()).then(|| credential.into()),
        schemas: if engine == Engine::PostgreSql {
            schemas
                .split(',')
                .map(str::trim)
                .map(str::to_owned)
                .collect()
        } else {
            Vec::new()
        },
    })
}

pub fn test_connection(
    options: &ConnectOptions,
    cancelled: impl Fn() -> bool,
) -> Result<String, String> {
    if cancelled() {
        return Err("Prueba de conexión cancelada.".into());
    }
    Session::connect(options)?.test_connection(cancelled)?;
    let engine = match options.engine {
        Engine::PostgreSql => "PostgreSQL",
        Engine::MySql => "MySQL",
    };
    Ok(format!(
        "Conexión correcta a {engine} · Base: {}",
        options.database
    ))
}

pub struct Reader {
    requests: mpsc::SyncSender<Query>,
    responses: mpsc::Receiver<Result<Event, String>>,
    cancelled: Arc<AtomicBool>,
    busy: bool,
}

pub enum Query {
    Refresh,
    Details(Relation),
    Page(Relation, PageRequest),
}

pub enum Event {
    Catalog(Vec<Relation>),
    Details(RelationDetails),
    Page(Page),
}

impl Reader {
    pub fn open(
        options: ConnectOptions,
        notify: impl Fn() + Send + Sync + 'static,
    ) -> Result<Self, String> {
        let (requests, pending) = mpsc::sync_channel(1);
        let (complete, responses) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_flag = Arc::clone(&cancelled);
        thread::Builder::new()
            .name("cide-sql-reader".into())
            .spawn(move || {
                let cancelled = || worker_flag.load(Ordering::Acquire);
                let publish = |event| {
                    if cancelled() || complete.send(event).is_err() {
                        return false;
                    }
                    notify();
                    true
                };
                if cancelled() {
                    return;
                }
                let session = match Session::connect(&options) {
                    Ok(session) => session,
                    Err(error) => {
                        publish(Err(error));
                        return;
                    }
                };
                if !publish(session.catalog(cancelled).map(Event::Catalog)) {
                    return;
                }
                while let Ok(query) = pending.recv() {
                    if cancelled() {
                        break;
                    }
                    let result = match query {
                        Query::Refresh => session.catalog(cancelled).map(Event::Catalog),
                        Query::Details(relation) => {
                            session.details(&relation, cancelled).map(Event::Details)
                        }
                        Query::Page(relation, page) => {
                            session.page(&relation, &page, cancelled).map(Event::Page)
                        }
                    };
                    if !publish(result) {
                        break;
                    }
                }
            })
            .map_err(|error| format!("No se pudo iniciar la conexión SQL: {error}"))?;
        Ok(Self {
            requests,
            responses,
            cancelled,
            busy: true,
        })
    }

    pub fn busy(&self) -> bool {
        self.busy
    }

    pub fn request(&mut self, query: Query) -> Result<(), String> {
        if self.busy {
            return Err("Ya hay una operación SQL en curso".into());
        }
        self.requests
            .try_send(query)
            .map_err(|_| "La sesión SQL terminó. Vuelve a conectar")?;
        self.busy = true;
        Ok(())
    }

    pub fn poll(&mut self) -> Option<Result<Event, String>> {
        if !self.busy {
            return None;
        }
        let response = match self.responses.try_recv() {
            Ok(response) => response,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("La sesión SQL se cerró sin responder".into())
            }
        };
        self.busy = false;
        Some(response)
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_sql_options_keep_engine_tls_and_reference_separate() {
        let fields = [
            "Demo",
            "localhost",
            "5432",
            "inventario",
            "lector",
            "verificar-servidor",
            "cide/datadigger/demo",
            "public, ventas",
        ];
        let postgres = options(Engine::PostgreSql, fields).unwrap();
        assert_eq!(postgres.schemas, ["public", "ventas"]);
        assert_eq!(postgres.tls, Tls::VerifyServer);
        assert_eq!(postgres.credential.as_deref(), Some("cide/datadigger/demo"));
        assert!(options(Engine::MySql, fields).unwrap().schemas.is_empty());
    }
}
