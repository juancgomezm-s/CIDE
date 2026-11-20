//! Explicit, worker-owned SQL browsing over installed Unicode ODBC drivers.

mod credential;
mod metadata;
mod odbc;

use odbc::{Connection, Environment};
use std::{
    fmt::Write as _,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

pub use metadata::{Column, ForeignKeyPart, IndexPart, RelationDetails};

pub const PAGE_SIZE: usize = 20;
pub const MAX_OFFSET: usize = 2_000;
const MAX_FILTER_BYTES: usize = 256;
const MAX_SCHEMAS: usize = 16;
const MAX_RELATIONS: usize = 1_024;
const MAX_PAGE_COLUMNS: usize = 12;
const MAX_CELL_CHARS: usize = 1_024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Engine {
    PostgreSql,
    MySql,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tls {
    VerifyServer,
    Required,
    Disabled,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConnectOptions {
    pub engine: Engine,
    pub server: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    pub tls: Tls,
    pub credential: Option<String>,
    pub schemas: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RelationKind {
    Table,
    View,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Relation {
    session: u64,
    schema: String,
    name: String,
    kind: RelationKind,
    pg_only: bool,
}

impl Relation {
    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn kind(&self) -> RelationKind {
        self.kind
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Filter {
    pub column: usize,
    pub value: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PageRequest {
    pub offset: usize,
    pub descending: bool,
    pub filter: Option<Filter>,
}

#[derive(Debug, Eq, PartialEq)]
pub struct Page {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Option<String>>>,
    pub offset: usize,
    pub has_next: bool,
}

pub struct Session {
    connection: Connection,
    engine: Engine,
    schemas: Vec<String>,
    identity: u64,
}

impl Session {
    pub(crate) fn test_connection(&self, cancelled: impl Fn() -> bool) -> Result<(), String> {
        let rows = self.connection.query("SELECT 1", &[], 1, 1, &cancelled)?;
        if rows != [vec![Some("1".into())]] {
            return Err("El servidor SQL devolvió una respuesta inesperada.".into());
        }
        Ok(())
    }

    pub fn connect(options: &ConnectOptions) -> Result<Self, String> {
        Self::connect_with_ca(options, None)
    }

    /// An optional explicit PEM trust bundle. The driver uses its normal trust
    /// configuration when the bundle is omitted.
    pub fn connect_with_ca(options: &ConnectOptions, ca: Option<&Path>) -> Result<Self, String> {
        let schemas = validate(options)?;
        let env = Environment::open()?;
        let driver = env.driver(options.engine)?;
        let password = options
            .credential
            .as_deref()
            .map(credential::Password::read)
            .transpose()?;
        let attributes = connection_string(
            options,
            &driver,
            password.as_ref().map_or(&[], credential::Password::units),
            ca,
        )?;
        let connection = env.connect(&attributes.0)?;
        drop(attributes);
        drop(password);
        // psqlODBC's ReadOnly flag rejects SET. The server enforces read-only
        // transactions after setup, before the session is exposed to the browser.
        let setup: &[&str] = match options.engine {
            Engine::PostgreSql => &[
                "SET default_transaction_read_only = on",
                "SET search_path = pg_catalog",
                "SET statement_timeout = '5s'",
            ],
            Engine::MySql => &[
                "SET SESSION TRANSACTION READ ONLY",
                "SET SESSION max_execution_time = 5000",
                "SET SESSION lock_wait_timeout = 5",
            ],
        };
        for sql in setup {
            connection.execute(sql)?;
        }
        if options.engine == Engine::PostgreSql {
            let identity = connection.query(
                "SELECT current_database(), session_user",
                &[],
                2,
                1,
                &|| false,
            )?;
            if identity.as_slice()
                != [vec![
                    Some(options.database.clone()),
                    Some(options.user.clone()),
                ]]
            {
                return Err("PostgreSQL devolvió otra base de datos o usuario".into());
            }
        }
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Ok(Self {
            connection,
            engine: options.engine,
            schemas,
            identity: NEXT.fetch_add(1, Ordering::Relaxed),
        })
    }

    pub fn catalog(&self, cancelled: impl Fn() -> bool) -> Result<Vec<Relation>, String> {
        let mut relations = Vec::new();
        let catalog_sql = match self.engine {
            Engine::PostgreSql => {
                "
                SELECT x.table_schema, x.table_name, x.table_type, CAST(c.relkind AS text)
                FROM information_schema.tables x
                JOIN pg_namespace n ON n.nspname = x.table_schema
                JOIN pg_class c ON c.relnamespace = n.oid AND c.relname = x.table_name
                WHERE x.table_schema = ? AND x.table_type IN ('BASE TABLE','VIEW')
                ORDER BY x.table_name"
            }
            Engine::MySql => {
                "
                SELECT table_schema, table_name, table_type, ''
                FROM information_schema.tables
                WHERE CAST(table_schema AS BINARY) = CAST(? AS BINARY)
                  AND table_type IN ('BASE TABLE','VIEW')
                ORDER BY table_name"
            }
        };
        for schema in &self.schemas {
            let remaining = MAX_RELATIONS - relations.len();
            let sql = format!("{catalog_sql} LIMIT {}", remaining + 1);
            let rows = self
                .connection
                .query(&sql, &[schema], 4, remaining + 1, &cancelled)?;
            if rows.len() > remaining {
                return Err("El catálogo SQL supera el límite de tablas y vistas".into());
            }
            for row in rows {
                let [observed, name, kind, pg_kind] = metadata::required(row)?;
                if &observed != schema {
                    return Err("El catálogo SQL devolvió un esquema inesperado".into());
                }
                let kind = match kind.as_str() {
                    "BASE TABLE" => RelationKind::Table,
                    "VIEW" => RelationKind::View,
                    _ => {
                        return Err("El catálogo SQL devolvió un tipo de objeto desconocido".into())
                    }
                };
                relations.push(Relation {
                    session: self.identity,
                    schema: observed,
                    name,
                    kind,
                    // Ordinary inheritance does not extend the parent's PK
                    // constraint to its children. Partitioned tables do.
                    pg_only: self.engine == Engine::PostgreSql && pg_kind == "r",
                });
            }
        }
        Ok(relations)
    }

    pub fn details(
        &self,
        relation: &Relation,
        cancelled: impl Fn() -> bool,
    ) -> Result<RelationDetails, String> {
        self.check_relation(relation)?;
        Ok(RelationDetails {
            columns: self.columns(relation, &cancelled)?,
            indexes: self.indexes(relation, &cancelled)?,
            foreign_keys: self.foreign_keys(relation, &cancelled)?,
        })
    }

    pub fn page(
        &self,
        relation: &Relation,
        request: &PageRequest,
        cancelled: impl Fn() -> bool,
    ) -> Result<Page, String> {
        self.check_relation(relation)?;
        let columns = self.columns(relation, &cancelled)?;
        let key = self.primary_key(relation, &cancelled)?;
        let (query, filter) = page_sql(self.engine, relation, &columns, &key, request)?;
        let params: Vec<_> = filter.into_iter().collect();
        let visible = columns.len().min(MAX_PAGE_COLUMNS);
        let mut rows =
            self.connection
                .query(&query, &params, visible, PAGE_SIZE + 1, &cancelled)?;
        let has_next =
            rows.len() > PAGE_SIZE && !key.is_empty() && request.offset + PAGE_SIZE <= MAX_OFFSET;
        rows.truncate(PAGE_SIZE);
        for cell in rows.iter_mut().flatten().flatten() {
            if let Some((position, _)) = cell.char_indices().nth(MAX_CELL_CHARS) {
                cell.truncate(position);
                cell.push('…');
            }
        }
        Ok(Page {
            columns: columns
                .into_iter()
                .take(visible)
                .map(|column| column.name)
                .collect(),
            rows,
            offset: request.offset,
            has_next,
        })
    }

    fn check_relation(&self, relation: &Relation) -> Result<(), String> {
        if relation.session == self.identity {
            Ok(())
        } else {
            Err("La tabla o vista pertenece a otro catálogo SQL".into())
        }
    }
}

fn validate(options: &ConnectOptions) -> Result<Vec<String>, String> {
    let server = &options.server;
    let ipv6 = server
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(server);
    let valid_host = server.len() <= 256
        && !server.is_empty()
        && (server
            .bytes()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'.' | b'-' | b'_'))
            || ipv6.parse::<std::net::Ipv6Addr>().is_ok());
    if !valid_host || options.port == 0 {
        return Err("Servidor o puerto SQL inválido".into());
    }
    for value in [&options.database, &options.user] {
        if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
            return Err("Base de datos o usuario SQL inválido".into());
        }
    }
    if options.engine == Engine::MySql {
        return Ok(vec![options.database.clone()]);
    }
    if options.schemas.is_empty() || options.schemas.len() > MAX_SCHEMAS {
        return Err(format!(
            "Indica entre 1 y {MAX_SCHEMAS} esquemas PostgreSQL"
        ));
    }
    let mut schemas = options.schemas.clone();
    if schemas.iter().any(|schema| {
        schema.is_empty() || schema.len() > 256 || schema.chars().any(char::is_control)
    }) {
        return Err("Nombre de esquema PostgreSQL inválido".into());
    }
    schemas.sort();
    schemas.dedup();
    Ok(schemas)
}

struct SecretText(Vec<u16>);

impl Drop for SecretText {
    fn drop(&mut self) {
        credential::wipe(&mut self.0);
    }
}

fn connection_string(
    options: &ConnectOptions,
    driver: &str,
    password: &[u16],
    ca: Option<&Path>,
) -> Result<SecretText, String> {
    let mut connection = SecretText(Vec::with_capacity(8192));
    let server = options.server.trim_start_matches('[').trim_end_matches(']');
    let port = options.port.to_string();
    let ca = ca.map(ca_path).transpose()?;
    let (server_key, tls) = match (options.engine, options.tls) {
        (Engine::PostgreSql, Tls::VerifyServer) => ("SERVERNAME", "verify-full"),
        (Engine::PostgreSql, Tls::Required) => ("SERVERNAME", "require"),
        (Engine::PostgreSql, Tls::Disabled) => ("SERVERNAME", "disable"),
        (Engine::MySql, Tls::VerifyServer) => ("SERVER", "VERIFY_IDENTITY"),
        (Engine::MySql, Tls::Required) => ("SERVER", "REQUIRED"),
        (Engine::MySql, Tls::Disabled) => ("SERVER", "DISABLED"),
    };
    attribute(&mut connection.0, "DRIVER", driver.encode_utf16())?;
    match options.engine {
        Engine::PostgreSql => {
            // psqlODBC retains braces on ordinary attributes, including
            // SSLmode, where an unknown first character disables TLS.
            // https://github.com/postgresql-interfaces/psqlodbc/blob/main/dlg_specific.c
            connection
                .0
                .extend(format!(
                    "{server_key}={server};PORT={port};SSLMODE={tls};ReadOnly=0;Debug=0;CommLog=0;UseDeclareFetch=0;LFConversion=0;"
                ).encode_utf16());
            attribute(&mut connection.0, "PWD", password.iter().copied())?;
            let mut pq = "connect_timeout=5 gssencmode=disable".to_owned();
            libpq_option(&mut pq, "dbname", &options.database);
            libpq_option(&mut pq, "user", &options.user);
            if let Some(path) = ca {
                libpq_option(&mut pq, "sslrootcert", path);
            }
            attribute(&mut connection.0, "Pqopt", pq.encode_utf16())?;
        }
        Engine::MySql => {
            // Connector/ODBC 8.0.29 and earlier cannot safely parse a
            // closing brace inside a braced value. Its registered driver
            // name provides the major/minor version only.
            // https://dev.mysql.com/doc/relnotes/connector-odbc/en/news-8-0-30.html
            if driver.starts_with("MySQL ODBC 8.0 ")
                && (options.database.contains('}')
                    || options.user.contains('}')
                    || password.contains(&(b'}' as u16))
                    || ca.is_some_and(|path| path.contains('}')))
            {
                return Err("El controlador MySQL no admite «}» en los datos de conexión".into());
            }
            // PORT is numeric in the MySQL connection parser.
            connection.0.extend(format!("PORT={port};").encode_utf16());
            for (key, value) in [
                (server_key, server),
                ("DATABASE", &options.database),
                ("UID", &options.user),
                ("SSLMODE", tls),
            ] {
                attribute(&mut connection.0, key, value.encode_utf16())?;
            }
            attribute(&mut connection.0, "PWD", password.iter().copied())?;
            connection.0.extend(
                "ENABLE_LOCAL_INFILE=0;MULTI_STATEMENTS=0;LOG_QUERY=0;GET_SERVER_PUBLIC_KEY=0;READTIMEOUT=7;WRITETIMEOUT=7;"
                    .encode_utf16(),
            );
            if let Some(path) = ca {
                attribute(&mut connection.0, "SSLCA", path.encode_utf16())?;
            }
        }
    }
    connection.0.push(0);
    Ok(connection)
}

fn libpq_option(options: &mut String, key: &str, value: &str) {
    write!(options, " {key}='").unwrap();
    for ch in value.chars() {
        if matches!(ch, '\\' | '\'') {
            options.push('\\');
        }
        options.push(ch);
    }
    options.push('\'');
}

fn ca_path(path: &Path) -> Result<&str, String> {
    path.to_str()
        .filter(|text| {
            path.is_absolute() && text.len() <= 1024 && !text.chars().any(char::is_control)
        })
        .ok_or_else(|| "Usa una ruta absoluta válida para el certificado CA".into())
}

fn attribute(
    connection: &mut Vec<u16>,
    key: &str,
    value: impl Iterator<Item = u16>,
) -> Result<(), String> {
    connection.extend(key.encode_utf16());
    connection.extend([b'=' as u16, b'{' as u16]);
    for unit in value {
        if unit == 0 {
            return Err("Un dato de conexión SQL contiene un carácter nulo".into());
        }
        if unit == b'}' as u16 {
            connection.push(unit);
        }
        connection.push(unit);
    }
    connection.extend([b'}' as u16, b';' as u16]);
    Ok(())
}

fn identifier(engine: Engine, name: &str) -> Result<String, String> {
    if name.is_empty() || name.contains('\0') {
        return Err("El catálogo SQL contiene un identificador inválido".into());
    }
    let quote = match engine {
        Engine::PostgreSql => '"',
        Engine::MySql => '`',
    };
    let mut result = String::with_capacity(name.len() + 2);
    result.push(quote);
    for ch in name.chars() {
        if ch == quote {
            result.push(ch);
        }
        result.push(ch);
    }
    result.push(quote);
    Ok(result)
}

fn page_sql<'a>(
    engine: Engine,
    relation: &Relation,
    columns: &[Column],
    key: &[String],
    request: &'a PageRequest,
) -> Result<(String, Option<&'a str>), String> {
    if request.offset > MAX_OFFSET
        || (key.is_empty() && (request.offset != 0 || request.descending))
    {
        return Err(
            "La tabla o vista requiere una clave primaria para recorrer páginas u ordenar".into(),
        );
    }
    let visible = &columns[..columns.len().min(MAX_PAGE_COLUMNS)];
    if visible.is_empty() {
        return Err("La tabla o vista SQL no tiene columnas visibles".into());
    }
    let payload = visible
        .iter()
        .map(|column| {
            let name = identifier(engine, &column.name)?;
            let expression = match (engine, column.binary()) {
                (Engine::PostgreSql, true) => format!(
                    "encode(substring({name} FROM 1 FOR {}), 'hex')",
                    MAX_CELL_CHARS / 2 + 1
                ),
                (Engine::PostgreSql, false) => {
                    format!("LEFT(CAST({name} AS text), {})", MAX_CELL_CHARS + 1)
                }
                (Engine::MySql, true) => format!("HEX(LEFT({name}, {}))", MAX_CELL_CHARS / 2 + 1),
                (Engine::MySql, false) => format!(
                    "LEFT(CAST({name} AS CHAR CHARACTER SET utf8mb4), {})",
                    MAX_CELL_CHARS + 1
                ),
            };
            Ok(expression)
        })
        .collect::<Result<Vec<_>, String>>()?
        .join(", ");
    let schema = identifier(engine, relation.schema())?;
    let name = identifier(engine, relation.name())?;
    let only = if relation.pg_only { "ONLY " } else { "" };
    let mut sql = format!("SELECT {payload} FROM {only}{schema}.{name}");
    let mut value = None;
    if let Some(filter) = &request.filter {
        let column = visible
            .get(filter.column)
            .ok_or("Columna de filtro SQL inválida")?;
        let input = &filter.value;
        if input.len() > MAX_FILTER_BYTES || input.contains('\0') {
            return Err("El filtro SQL contiene NUL o supera 256 bytes".into());
        }
        let cast = match (engine, column.integer(), column.text()) {
            (Engine::PostgreSql, true, _) if input.parse::<i64>().is_ok() => "CAST(? AS bigint)",
            (Engine::MySql, true, _)
                if input.parse::<i64>().is_ok() || input.parse::<u64>().is_ok() =>
            {
                "CAST(? AS DECIMAL(20,0))"
            }
            (Engine::PostgreSql, false, true) => "CAST(? AS text)",
            (Engine::MySql, false, true) => "CAST(? AS CHAR CHARACTER SET utf8mb4)",
            (_, true, _) => return Err("Escribe un entero válido para el filtro SQL".into()),
            _ => return Err("El filtro exacto admite columnas de texto o enteros".into()),
        };
        let name = identifier(engine, &column.name)?;
        write!(sql, " WHERE {name} = {cast}").unwrap();
        value = Some(input.as_str());
    }
    if !key.is_empty() {
        let direction = if request.descending { "DESC" } else { "ASC" };
        let order = key
            .iter()
            .map(|name| identifier(engine, name).map(|name| format!("{name} {direction}")))
            .collect::<Result<Vec<_>, String>>()?;
        write!(sql, " ORDER BY {}", order.join(", ")).unwrap();
    }
    write!(
        sql,
        " LIMIT {} OFFSET {}",
        PAGE_SIZE + usize::from(!key.is_empty()),
        request.offset
    )
    .unwrap();
    Ok((sql, value))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(engine: Engine) -> ConnectOptions {
        ConnectOptions {
            engine,
            server: "localhost".into(),
            port: 15432,
            database: "Datos;UID=otro".into(),
            user: "L'usuario".into(),
            tls: Tls::VerifyServer,
            credential: None,
            schemas: vec!["public".into()],
        }
    }

    #[test]
    fn sql_connection_formats_preserve_tls_settings_and_escape_attributes() {
        let password: Vec<u16> = "p;SSLMODE=DISABLED".encode_utf16().collect();
        let pg = connection_string(
            &options(Engine::PostgreSql),
            "PostgreSQL Unicode",
            &password,
            None,
        )
        .unwrap();
        let pg = String::from_utf16(&pg.0).unwrap();
        assert!(pg.contains("SERVERNAME=localhost;PORT=15432;SSLMODE=verify-full;"));
        assert!(pg.contains("PWD={p;SSLMODE=DISABLED};"));
        assert!(pg.contains("dbname='Datos;UID=otro' user='L\\'usuario'"));

        let mysql = connection_string(
            &options(Engine::MySql),
            "MySQL ODBC 8.0 Unicode Driver",
            &password,
            None,
        )
        .unwrap();
        let mysql = String::from_utf16(&mysql.0).unwrap();
        assert!(mysql.contains("PORT=15432;SERVER={localhost};"));
        assert!(mysql.contains("SSLMODE={VERIFY_IDENTITY};PWD={p;SSLMODE=DISABLED};"));
    }

    #[test]
    fn sql_legacy_mysql_driver_rejects_ambiguous_braces() {
        let options = options(Engine::MySql);
        let password: Vec<u16> = "p};SSLMODE=DISABLED".encode_utf16().collect();
        assert!(
            connection_string(&options, "MySQL ODBC 8.0 Unicode Driver", &password, None).is_err()
        );
        let modern =
            connection_string(&options, "MySQL ODBC 8.4 Unicode Driver", &password, None).unwrap();
        assert!(String::from_utf16(&modern.0)
            .unwrap()
            .contains("PWD={p}};SSLMODE=DISABLED};"));
    }

    #[test]
    fn sql_generated_pages_bind_values_and_quote_catalog_identifiers() {
        let mut relation = Relation {
            session: 0,
            schema: "esquema".into(),
            name: "nom\"bre".into(),
            kind: RelationKind::Table,
            pg_only: true,
        };
        let columns = [Column {
            name: "col\"umna".into(),
            data_type: "text".into(),
            nullable: true,
            sql_type: "text".into(),
        }];
        let value = "x'; DROP TABLE tabla; --".to_owned();
        let request = PageRequest {
            offset: 20,
            descending: true,
            filter: Some(Filter {
                column: 0,
                value: value.clone(),
            }),
        };
        assert!(page_sql(Engine::PostgreSql, &relation, &columns, &[], &request).is_err());
        let (query, filter) = page_sql(
            Engine::PostgreSql,
            &relation,
            &columns,
            &["id".into()],
            &request,
        )
        .unwrap();
        assert_eq!(filter, Some(value.as_str()));
        assert!(!query.contains(&value));
        assert!(query.contains(
            "FROM ONLY \"esquema\".\"nom\"\"bre\" WHERE \"col\"\"umna\" = CAST(? AS text)"
        ));
        assert!(query.ends_with("ORDER BY \"id\" DESC LIMIT 21 OFFSET 20"));

        relation.pg_only = false;
        relation.name = ["nom", "bre"].join("`");
        let first = PageRequest::default();
        let (query, filter) = page_sql(Engine::MySql, &relation, &columns, &[], &first).unwrap();
        assert!(filter.is_none());
        assert!(query.ends_with("FROM `esquema`.`nom``bre` LIMIT 20 OFFSET 0"));
    }

    #[test]
    fn sql_native_driver_discovery_requires_no_connections() {
        let env = Environment::open().unwrap();
        for engine in [Engine::PostgreSql, Engine::MySql] {
            match env.driver(engine) {
                Ok(driver) => println!("Controlador disponible: {driver}"),
                Err(error) => {
                    println!("{error}");
                    assert!(error.starts_with("Falta el controlador"));
                }
            }
        }
    }
}
