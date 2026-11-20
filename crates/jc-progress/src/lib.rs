//! Small default-dependency-free AVM boundary; optional source-services feature
//! links the internal lexical provider to the headless application API.

#![forbid(unsafe_code)]

#[cfg(feature = "source-services")]
pub mod editor;
#[cfg(feature = "source-services")]
pub mod project_rename;
#[cfg(feature = "source-services")]
pub mod rename;
#[cfg(feature = "source-services")]
pub mod snippets;
#[cfg(feature = "source-services")]
pub mod source_services;

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Check,
    Compile,
    Run,
    Schema,
    Probe,
    Xref,
}

impl Action {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Check => "check",
            Self::Compile => "compile",
            Self::Run => "run",
            Self::Schema => "schema",
            Self::Probe => "probe",
            Self::Xref => "xref",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressProfile {
    pub executable: PathBuf,
    pub parameter_file: Option<PathBuf>,
    pub ini_file: Option<PathBuf>,
    pub cpstream: String,
    pub cpinternal: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeRequest {
    pub action: Action,
    pub source: Option<PathBuf>,
    pub output_dir: PathBuf,
    pub propath: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub executable: PathBuf,
    pub args: Vec<OsString>,
}

impl CommandSpec {
    pub fn to_command(&self) -> Command {
        let mut command = Command::new(&self.executable);
        command.args(&self.args);
        command
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeError {
    pub line: Option<usize>,
    pub message: String,
}

impl fmt::Display for BridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(line) => write!(f, "bridge response line {line}: {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}
impl std::error::Error for BridgeError {}

fn config_error(message: impl Into<String>) -> BridgeError {
    BridgeError {
        line: None,
        message: message.into(),
    }
}
fn parse_error(line: usize, message: impl Into<String>) -> BridgeError {
    BridgeError {
        line: Some(line),
        message: message.into(),
    }
}

fn ascii_line<'a>(value: &'a str, field: &str) -> Result<&'a str, BridgeError> {
    if !value.is_ascii() || value.bytes().any(|b| b < 32 || b == 127) {
        return Err(config_error(format!(
            "{field} must contain printable ASCII; source file CONTENT is unrestricted"
        )));
    }
    Ok(value)
}

fn path_line<'a>(path: &'a Path, field: &str) -> Result<&'a str, BridgeError> {
    let value = path
        .to_str()
        .ok_or_else(|| config_error(format!("{field} must be an ASCII path")))?;
    ascii_line(value, field)?;
    let bytes = value.as_bytes();
    let windows_absolute = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\');
    let unc = value.starts_with("\\\\");
    if !(path.is_absolute() || windows_absolute || unc) {
        return Err(config_error(format!("{field} must be an absolute path")));
    }
    Ok(value)
}

fn code_page(value: &str, field: &str) -> Result<(), BridgeError> {
    if value.is_empty()
        || value.len() > 64
        || !value.as_bytes()[0].is_ascii_alphanumeric()
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        return Err(config_error(format!(
            "{field} must be an explicit Progress code-page name"
        )));
    }
    Ok(())
}

impl BridgeRequest {
    pub fn encode(&self) -> Result<Vec<u8>, BridgeError> {
        self.encode_request(None, None)
    }

    fn encode_request(
        &self,
        aliases: Option<&str>,
        version: Option<&str>,
    ) -> Result<Vec<u8>, BridgeError> {
        let output = path_line(&self.output_dir, "output_dir")?;
        let source = match (&self.action, &self.source) {
            (Action::Schema | Action::Probe, None) => "",
            (Action::Schema | Action::Probe, Some(_)) => {
                return Err(config_error(
                    "schema/probe requests must not specify source",
                ))
            }
            (_, Some(path)) => path_line(path, "source")?,
            (_, None) => return Err(config_error("check/compile/run requests require source")),
        };
        let propath = ascii_line(self.propath.as_deref().unwrap_or(""), "propath")?;
        let protocol = if version.is_some() {
            3
        } else if aliases.is_some() {
            2
        } else {
            1
        };
        let mut data = format!(
            "JCIDE/{protocol}\n{}\n{source}\n{output}\n{propath}\n",
            self.action.as_str()
        );
        if protocol >= 2 {
            data.push_str(ascii_line(aliases.unwrap_or_default(), "aliases")?);
            data.push('\n');
        }
        if let Some(version) = version {
            if !version.is_empty() && !valid_avm_version(version) {
                return Err(config_error("Versión esperada de OpenEdge inválida"));
            }
            data.push_str(version);
            data.push('\n');
        }
        Ok(data.into_bytes())
    }

    pub fn response_path(&self) -> PathBuf {
        self.output_dir.join("response.txt")
    }
}

pub fn build_command(
    profile: &ProgressProfile,
    helper: &Path,
    request_file: &Path,
) -> Result<CommandSpec, BridgeError> {
    path_line(&profile.executable, "executable")?;
    path_line(helper, "helper")?;
    path_line(request_file, "request_file")?;
    code_page(&profile.cpstream, "cpstream")?;
    code_page(&profile.cpinternal, "cpinternal")?;
    let mut args = Vec::new();
    if let Some(path) = &profile.ini_file {
        path_line(path, "ini_file")?;
        args.extend([OsString::from("-ininame"), path.as_os_str().to_owned()]);
    }
    if let Some(path) = &profile.parameter_file {
        path_line(path, "parameter_file")?;
        args.extend([OsString::from("-pf"), path.as_os_str().to_owned()]);
    }
    args.extend([
        OsString::from("-cpstream"),
        OsString::from(&profile.cpstream),
        OsString::from("-cpinternal"),
        OsString::from(&profile.cpinternal),
        OsString::from("-b"),
        OsString::from("-p"),
        helper.as_os_str().to_owned(),
        OsString::from("-param"),
        request_file.as_os_str().to_owned(),
    ]);
    Ok(CommandSpec {
        executable: profile.executable.clone(),
        args,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedText(pub Vec<u8>);
impl EncodedText {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub row: Option<u32>,
    pub column: Option<u32>,
    pub file: Option<EncodedText>,
    pub message: EncodedText,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaRecord {
    Database {
        name: EncodedText,
    },
    Table {
        database: EncodedText,
        table: EncodedText,
    },
    Field {
        database: EncodedText,
        table: EncodedText,
        name: EncodedText,
        data_type: EncodedText,
        extent: u32,
        format: Option<EncodedText>,
        label: Option<EncodedText>,
    },
    Index {
        database: EncodedText,
        table: EncodedText,
        name: EncodedText,
        primary: bool,
        unique: bool,
    },
    IndexField {
        database: EncodedText,
        table: EncodedText,
        index: EncodedText,
        sequence: u32,
        field: EncodedText,
        ascending: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionStatus {
    Ok,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeResponse {
    pub cpinternal: String,
    pub cpstream: String,
    pub avm_version: Option<String>,
    pub diagnostics: Vec<Diagnostic>,
    pub schema: Vec<SchemaRecord>,
    pub status: CompletionStatus,
}

fn valid_avm_version(value: &str) -> bool {
    value.len() <= 64
        && value.as_bytes().first().is_some_and(u8::is_ascii_digit)
        && value.contains('.')
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}

fn nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn text(field: &str, line: usize) -> Result<EncodedText, BridgeError> {
    if field.len() % 2 != 0 || field.len() > 65534 {
        return Err(parse_error(line, "invalid hexadecimal text length"));
    }
    let bytes = field
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| match (nibble(pair[0]), nibble(pair[1])) {
            (Some(hi), Some(lo)) => Ok(hi * 16 + lo),
            _ => Err(parse_error(line, "invalid hexadecimal text")),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(EncodedText(bytes))
}

fn optional_text(field: &str, line: usize) -> Result<Option<EncodedText>, BridgeError> {
    if field == "-" {
        Ok(None)
    } else {
        text(field, line).map(Some)
    }
}

fn number(field: &str, line: usize) -> Result<u32, BridgeError> {
    if field.is_empty() || !field.bytes().all(|b| b.is_ascii_digit()) {
        return Err(parse_error(line, "invalid unsigned integer"));
    }
    field
        .parse()
        .map_err(|_| parse_error(line, "integer overflow"))
}

fn position(field: &str, line: usize) -> Result<Option<u32>, BridgeError> {
    if field == "-" {
        return Ok(None);
    }
    let value = number(field, line)?;
    if value == 0 {
        return Err(parse_error(line, "positions are one-based or '-'"));
    }
    Ok(Some(value))
}

fn flag(field: &str, line: usize) -> Result<bool, BridgeError> {
    match field {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => Err(parse_error(line, "expected flag 0 or 1")),
    }
}

pub fn parse_response(bytes: &[u8]) -> Result<BridgeResponse, BridgeError> {
    if bytes.len() > 32 * 1024 * 1024 {
        return Err(config_error("bridge response exceeds 32 MiB"));
    }
    if !bytes.is_ascii() {
        return Err(config_error(
            "bridge response must be ASCII with hex-encoded text",
        ));
    }
    let input =
        std::str::from_utf8(bytes).map_err(|_| config_error("invalid response encoding"))?;
    let mut lines = input
        .split_terminator('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line));
    let header = lines
        .next()
        .ok_or_else(|| config_error("missing response header"))?;
    if header.len() > 128 * 1024 {
        return Err(parse_error(1, "record exceeds 128 KiB"));
    }
    let parts: Vec<_> = header.splitn(6, '|').collect();
    if !matches!(
        parts.as_slice(),
        ["JCIDE", "1", _, _] | ["JCIDE", "2", _, _, _]
    ) {
        return Err(parse_error(1, "invalid JCIDE response header"));
    }
    let cpinternal = String::from_utf8(text(parts[2], 1)?.0)
        .map_err(|_| parse_error(1, "non-ASCII code-page name"))?;
    let cpstream = String::from_utf8(text(parts[3], 1)?.0)
        .map_err(|_| parse_error(1, "non-ASCII code-page name"))?;
    code_page(&cpinternal, "cpinternal")?;
    code_page(&cpstream, "cpstream")?;
    let avm_version = if let Some(field) = parts.get(4) {
        let version = String::from_utf8(text(field, 1)?.0)
            .map_err(|_| parse_error(1, "versión del AVM inválida"))?;
        if !valid_avm_version(&version) {
            return Err(parse_error(1, "versión del AVM inválida"));
        }
        Some(version)
    } else {
        None
    };
    let mut diagnostics = Vec::new();
    let mut schema = Vec::new();
    let mut status = None;
    for (offset, line) in lines.enumerate() {
        let line_number = offset + 2;
        if status.is_some() {
            return Err(parse_error(line_number, "data after END record"));
        }
        if line.len() > 128 * 1024 {
            return Err(parse_error(line_number, "record exceeds 128 KiB"));
        }
        let p: Vec<_> = line.splitn(9, '|').collect();
        match p.first().copied().unwrap_or("") {
            "DIAG" if p.len() == 6 => diagnostics.push(Diagnostic {
                severity: match p[1] {
                    "error" => Severity::Error,
                    "warning" => Severity::Warning,
                    _ => return Err(parse_error(line_number, "invalid diagnostic severity")),
                },
                row: position(p[2], line_number)?,
                column: position(p[3], line_number)?,
                file: optional_text(p[4], line_number)?,
                message: text(p[5], line_number)?,
            }),
            "DATABASE" if p.len() == 2 => schema.push(SchemaRecord::Database {
                name: text(p[1], line_number)?,
            }),
            "TABLE" if p.len() == 3 => schema.push(SchemaRecord::Table {
                database: text(p[1], line_number)?,
                table: text(p[2], line_number)?,
            }),
            "FIELD" if p.len() == 8 => schema.push(SchemaRecord::Field {
                database: text(p[1], line_number)?,
                table: text(p[2], line_number)?,
                name: text(p[3], line_number)?,
                data_type: text(p[4], line_number)?,
                extent: number(p[5], line_number)?,
                format: optional_text(p[6], line_number)?,
                label: optional_text(p[7], line_number)?,
            }),
            "INDEX" if p.len() == 6 => schema.push(SchemaRecord::Index {
                database: text(p[1], line_number)?,
                table: text(p[2], line_number)?,
                name: text(p[3], line_number)?,
                primary: flag(p[4], line_number)?,
                unique: flag(p[5], line_number)?,
            }),
            "INDEX_FIELD" if p.len() == 7 => schema.push(SchemaRecord::IndexField {
                database: text(p[1], line_number)?,
                table: text(p[2], line_number)?,
                index: text(p[3], line_number)?,
                sequence: number(p[4], line_number)?,
                field: text(p[5], line_number)?,
                ascending: flag(p[6], line_number)?,
            }),
            "END" if p.len() == 2 => {
                status = Some(match p[1] {
                    "OK" => CompletionStatus::Ok,
                    "ERROR" => CompletionStatus::Error,
                    _ => return Err(parse_error(line_number, "invalid END status")),
                })
            }
            _ => {
                return Err(parse_error(
                    line_number,
                    "unknown record or wrong field count",
                ))
            }
        }
    }
    let status = status.ok_or_else(|| config_error("incomplete response: missing END record"))?;
    if status == CompletionStatus::Ok && diagnostics.iter().any(|d| d.severity == Severity::Error) {
        return Err(config_error("END|OK contradicts error diagnostics"));
    }
    Ok(BridgeResponse {
        cpinternal,
        cpstream,
        avm_version,
        diagnostics,
        schema,
        status,
    })
}
