//! Windows ODBC 3 Unicode transport. Driver diagnostics stay inside this module.
//! https://learn.microsoft.com/sql/odbc/reference/syntax/odbc-api-reference

use super::Engine;
use std::{ffi::c_void, ptr};

type Raw = *mut c_void;
type Status = i16;
const ENV: i16 = 1;
const DBC: i16 = 2;
const STMT: i16 = 3;
const SUCCESS: Status = 0;
const SUCCESS_WITH_INFO: Status = 1;
const NO_DATA: Status = 100;
const TIMEOUT: usize = 5;
const MAX_CELL_UNITS: usize = 4096;
const CANCELLED: &str = "Consulta SQL cancelada";

#[link(name = "odbc32")]
extern "system" {
    fn SQLAllocHandle(kind: i16, parent: Raw, result: *mut Raw) -> Status;
    fn SQLFreeHandle(kind: i16, handle: Raw) -> Status;
    fn SQLSetEnvAttr(env: Raw, attr: i32, value: Raw, length: i32) -> Status;
    fn SQLDriversW(
        env: Raw,
        direction: u16,
        description: *mut u16,
        description_max: i16,
        description_len: *mut i16,
        attributes: *mut u16,
        attributes_max: i16,
        attributes_len: *mut i16,
    ) -> Status;
    fn SQLSetConnectAttrW(dbc: Raw, attr: i32, value: Raw, length: i32) -> Status;
    fn SQLDriverConnectW(
        dbc: Raw,
        window: Raw,
        input: *const u16,
        input_len: i16,
        output: *mut u16,
        output_max: i16,
        output_len: *mut i16,
        completion: u16,
    ) -> Status;
    fn SQLDisconnect(dbc: Raw) -> Status;
    fn SQLSetStmtAttrW(stmt: Raw, attr: i32, value: Raw, length: i32) -> Status;
    fn SQLPrepareW(stmt: Raw, sql: *const u16, len: i32) -> Status;
    fn SQLExecDirectW(stmt: Raw, sql: *const u16, len: i32) -> Status;
    fn SQLBindParameter(
        stmt: Raw,
        ordinal: u16,
        direction: i16,
        value_type: i16,
        param_type: i16,
        column_size: usize,
        scale: i16,
        value: Raw,
        size: isize,
        indicator: *mut isize,
    ) -> Status;
    fn SQLExecute(stmt: Raw) -> Status;
    fn SQLNumResultCols(stmt: Raw, columns: *mut i16) -> Status;
    fn SQLFetch(stmt: Raw) -> Status;
    fn SQLGetData(
        stmt: Raw,
        column: u16,
        target_type: i16,
        buffer: Raw,
        length: isize,
        indicator: *mut isize,
    ) -> Status;
    fn SQLGetDiagRecW(
        kind: i16,
        handle: Raw,
        record: i16,
        state: *mut u16,
        native: *mut i32,
        message: *mut u16,
        message_max: i16,
        message_len: *mut i16,
    ) -> Status;
}

fn succeeded(status: Status) -> bool {
    matches!(status, SUCCESS | SUCCESS_WITH_INFO)
}

struct Handle {
    raw: Raw,
    kind: i16,
}

impl Handle {
    fn new(kind: i16, parent: Option<&Self>) -> Result<Self, String> {
        let mut raw = ptr::null_mut();
        let status =
            unsafe { SQLAllocHandle(kind, parent.map_or(ptr::null_mut(), |h| h.raw), &mut raw) };
        match (succeeded(status) && !raw.is_null(), parent) {
            (true, _) => Ok(Self { raw, kind }),
            (false, Some(parent)) => Err(parent.error("crear el descriptor ODBC")),
            (false, None) => Err("No se pudo inicializar ODBC".into()),
        }
    }

    fn check(&self, status: Status, action: &str) -> Result<(), String> {
        if succeeded(status) {
            Ok(())
        } else {
            Err(self.error(action))
        }
    }

    fn state(&self) -> Option<String> {
        let mut state = [0; 6];
        let mut ignored = [0; 1];
        let status = unsafe {
            SQLGetDiagRecW(
                self.kind,
                self.raw,
                1,
                state.as_mut_ptr(),
                ptr::null_mut(),
                ignored.as_mut_ptr(),
                1,
                ptr::null_mut(),
            )
        };
        if !succeeded(status) {
            return None;
        }
        let state = String::from_utf16(&state[..5]).ok()?;
        state
            .bytes()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit())
            .then_some(state)
    }

    fn error(&self, action: &str) -> String {
        match self.state().as_deref() {
            Some("28000") => format!("No se pudo {action}: autenticación rechazada (ODBC 28000)"),
            Some("HYT00" | "HYT01") => format!("Tiempo de espera agotado al {action}"),
            Some(state) => format!("No se pudo {action} (ODBC {state})"),
            None => format!("No se pudo {action}"),
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { SQLFreeHandle(self.kind, self.raw) };
    }
}

pub(super) struct Environment(Handle);

impl Environment {
    pub(super) fn open() -> Result<Self, String> {
        let handle = Handle::new(ENV, None)?;
        let status = unsafe { SQLSetEnvAttr(handle.raw, 200, 3 as Raw, 0) };
        handle.check(status, "configurar ODBC 3")?;
        Ok(Self(handle))
    }

    pub(super) fn driver(&self, engine: Engine) -> Result<String, String> {
        let mut direction = 2; // SQL_FETCH_FIRST
        let mut selected = None::<([u32; 3], String)>;
        for _ in 0..256 {
            let mut name = [0; 256];
            let mut name_len = 0;
            let status = unsafe {
                SQLDriversW(
                    self.0.raw,
                    direction,
                    name.as_mut_ptr(),
                    name.len() as i16,
                    &mut name_len,
                    ptr::null_mut(),
                    0,
                    ptr::null_mut(),
                )
            };
            if status == NO_DATA {
                break;
            }
            self.0.check(status, "enumerar controladores ODBC")?;
            direction = 1; // SQL_FETCH_NEXT
            let Some(name) = name.get(..name_len as usize) else {
                continue;
            };
            let Ok(name) = String::from_utf16(name) else {
                continue;
            };
            let Some(rank) = driver_rank(engine, &name) else {
                continue;
            };
            if selected
                .as_ref()
                .is_none_or(|(old_rank, _)| rank > *old_rank)
            {
                selected = Some((rank, name));
            }
        }
        selected.map(|(_, name)| name).ok_or_else(|| match engine {
            Engine::PostgreSql => {
                "Falta el controlador ODBC Unicode de PostgreSQL (psqlODBC)".into()
            }
            Engine::MySql => {
                "Falta el controlador Unicode MySQL Connector/ODBC 8 o posterior".into()
            }
        })
    }

    pub(super) fn connect(self, connection: &[u16]) -> Result<Connection, String> {
        if connection.last() != Some(&0) {
            return Err("Cadena de conexión SQL incompleta".into());
        }
        let handle = Handle::new(DBC, Some(&self.0))?;
        let status = unsafe { SQLSetConnectAttrW(handle.raw, 103, TIMEOUT as Raw, 0) };
        handle.check(status, "configurar el tiempo de conexión SQL")?;
        let status = unsafe {
            SQLDriverConnectW(
                handle.raw,
                ptr::null_mut(),
                connection.as_ptr(),
                -3, // SQL_NTS
                ptr::null_mut(),
                0,
                ptr::null_mut(),
                0, // SQL_DRIVER_NOPROMPT
            )
        };
        handle.check(
            status,
            "abrir la conexión SQL con los ajustes de autenticación y TLS",
        )?;
        Ok(Connection {
            handle,
            _environment: self,
        })
    }
}

fn driver_rank(engine: Engine, name: &str) -> Option<[u32; 3]> {
    match engine {
        Engine::PostgreSql => match name {
            "PostgreSQL Unicode(x64)" => Some([2, 0, 0]),
            "PostgreSQL Unicode" => Some([1, 0, 0]),
            _ => None,
        },
        Engine::MySql => {
            let version = name
                .strip_prefix("MySQL ODBC ")?
                .strip_suffix(" Unicode Driver")?;
            let mut rank = [0; 3];
            let mut parts = version.split('.');
            rank[0] = parts.next()?.parse().ok()?;
            rank[1] = parts.next()?.parse().ok()?;
            if let Some(patch) = parts.next() {
                rank[2] = patch.parse().ok()?;
            }
            (parts.next().is_none() && rank[0] >= 8).then_some(rank)
        }
    }
}

pub(super) struct Connection {
    // Drop the connected handle before its environment.
    handle: Handle,
    _environment: Environment,
}

struct Param {
    text: Vec<u16>,
    bytes: isize,
}

impl Connection {
    pub(super) fn execute(&self, sql: &str) -> Result<(), String> {
        let statement = self.statement()?;
        let query: Vec<u16> = sql.encode_utf16().collect();
        let status = unsafe { SQLExecDirectW(statement.raw, query.as_ptr(), query.len() as i32) };
        statement.check(status, "configurar la sesión SQL de solo lectura")
    }

    fn statement(&self) -> Result<Handle, String> {
        let statement = Handle::new(STMT, Some(&self.handle))?;
        let status = unsafe { SQLSetStmtAttrW(statement.raw, 0, TIMEOUT as Raw, 0) };
        // The server additionally enforces statement limits. Some ODBC drivers
        // advertise only server-specific timeouts and report optional feature.
        if !succeeded(status) && statement.state().as_deref() != Some("HYC00") {
            return Err(statement.error("configurar el tiempo de consulta SQL"));
        }
        Ok(statement)
    }

    pub(super) fn query(
        &self,
        sql: &str,
        values: &[&str],
        columns: usize,
        rows: usize,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Vec<Vec<Option<String>>>, String> {
        if cancelled() {
            return Err(CANCELLED.into());
        }
        let mut params: Vec<Param> = values
            .iter()
            .map(|text| {
                let mut text: Vec<u16> = text.encode_utf16().collect();
                let bytes = (text.len() * 2) as isize;
                text.push(0);
                Param { text, bytes }
            })
            .collect();
        // The statement is dropped before the buffers it borrows on every exit.
        let statement = self.statement()?;
        let query: Vec<u16> = sql.encode_utf16().collect();
        let status = unsafe { SQLPrepareW(statement.raw, query.as_ptr(), query.len() as i32) };
        statement.check(status, "preparar la consulta SQL")?;
        for (index, param) in params.iter_mut().enumerate() {
            let status = unsafe {
                SQLBindParameter(
                    statement.raw,
                    (index + 1) as u16,
                    1,  // SQL_PARAM_INPUT
                    -8, // SQL_C_WCHAR
                    -9, // SQL_WVARCHAR
                    (param.text.len() - 1).max(1),
                    0,
                    param.text.as_mut_ptr().cast(),
                    (param.text.len() * 2) as isize,
                    &mut param.bytes,
                )
            };
            statement.check(status, "enlazar el parámetro SQL")?;
        }
        if cancelled() {
            return Err(CANCELLED.into());
        }
        let status = unsafe { SQLExecute(statement.raw) };
        statement.check(status, "ejecutar la consulta SQL")?;
        let mut count = 0;
        let status = unsafe { SQLNumResultCols(statement.raw, &mut count) };
        statement.check(status, "leer las columnas SQL")?;
        if count as usize != columns {
            return Err("La consulta SQL devolvió otro número de columnas".into());
        }
        let mut result = Vec::new();
        loop {
            if cancelled() {
                return Err(CANCELLED.into());
            }
            let status = unsafe { SQLFetch(statement.raw) };
            if status == NO_DATA {
                return Ok(result);
            }
            statement.check(status, "avanzar por los resultados SQL")?;
            if result.len() == rows {
                return Err("La consulta SQL superó el límite de filas".into());
            }
            let row = (1..=columns)
                .map(|column| text_cell(&statement, column as u16))
                .collect::<Result<Vec<_>, _>>()?;
            result.push(row);
        }
    }
}

fn text_cell(statement: &Handle, column: u16) -> Result<Option<String>, String> {
    let mut buffer = [0u16; MAX_CELL_UNITS + 1];
    let mut bytes = 0;
    let status = unsafe {
        SQLGetData(
            statement.raw,
            column,
            -8, // SQL_C_WCHAR
            buffer.as_mut_ptr().cast(),
            std::mem::size_of_val(&buffer) as isize,
            &mut bytes,
        )
    };
    statement.check(status, "leer una celda SQL")?;
    if bytes == -1 {
        // SQL_NULL_DATA
        return Ok(None);
    }
    if status != SUCCESS || !(0..=(MAX_CELL_UNITS * 2) as isize).contains(&bytes) || bytes % 2 != 0
    {
        return Err("La celda SQL supera el límite o no se pudo convertir a Unicode".into());
    }
    String::from_utf16(&buffer[..bytes as usize / 2])
        .map(Some)
        .map_err(|_| "La celda SQL contiene UTF-16 inválido".into())
}

impl Drop for Connection {
    fn drop(&mut self) {
        unsafe { SQLDisconnect(self.handle.raw) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn driver_selection_requires_supported_unicode_names() {
        assert_eq!(
            driver_rank(Engine::MySql, "MySQL ODBC 26.7 Unicode Driver"),
            Some([26, 7, 0])
        );
        assert_eq!(
            driver_rank(Engine::MySql, "MySQL ODBC 8.0 Unicode Driver"),
            Some([8, 0, 0])
        );
        assert_eq!(
            driver_rank(Engine::MySql, "MySQL ODBC 5.3 Unicode Driver"),
            None
        );
        assert_eq!(
            driver_rank(Engine::MySql, "MySQL ODBC 9.4 ANSI Driver"),
            None
        );
        assert_eq!(
            driver_rank(Engine::PostgreSql, "PostgreSQL ANSI(x64)"),
            None
        );
        assert_eq!(
            driver_rank(Engine::PostgreSql, "PostgreSQL Unicode(x64)"),
            Some([2, 0, 0])
        );
    }
}
