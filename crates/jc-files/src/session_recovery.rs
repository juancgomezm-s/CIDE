//! Durable FileSession metadata. Restoring it does not touch the original path:
//! the separately validated Document payload remains the sole byte/undo model.
use crate::{ContentFingerprint, FileSession};
use jc_core::Document;
use jc_text::{EncodingSource, TextEncoding};
use std::{
    fmt,
    io::{self, Read, Write},
    path::PathBuf,
};

const MAGIC: &[u8; 8] = b"CIDEFSR1";
const END: &[u8; 8] = b"ENDFSR01";
const VERSION: u64 = 1;
const MAX_PATH_UNITS: usize = 32768;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRecoveryInfo {
    pub path: PathBuf,
    pub encoding: Option<TextEncoding>,
    pub encoding_source: EncodingSource,
    pub disk_fingerprint: Option<ContentFingerprint>,
}
#[derive(Debug, Clone, Copy)]
pub struct SessionRecoveryLimits {
    pub max_path_units: usize,
    pub max_encoded_bytes: u64,
}
impl Default for SessionRecoveryLimits {
    fn default() -> Self {
        Self {
            max_path_units: MAX_PATH_UNITS,
            max_encoded_bytes: 128 * 1024,
        }
    }
}
#[derive(Debug)]
pub enum SessionRecoveryError {
    Io(io::Error),
    InvalidFormat(&'static str),
    LimitExceeded(&'static str),
    UnsupportedVersion(u64),
    UnsupportedPathPlatform(u64),
    Allocation,
}
impl fmt::Display for SessionRecoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "No se pudo leer la sesión recuperada: {error}"),
            Self::InvalidFormat(reason) => write!(f, "Sesión recuperada inválida: {reason}"),
            Self::LimitExceeded(reason) => {
                write!(f, "La sesión recuperada supera el presupuesto: {reason}")
            }
            Self::UnsupportedVersion(version) => {
                write!(f, "Versión de sesión recuperada no admitida: {version}")
            }
            Self::UnsupportedPathPlatform(platform) => write!(
                f,
                "La ruta recuperada pertenece a una plataforma no admitida: {platform}"
            ),
            Self::Allocation => write!(
                f,
                "No se pudo reservar la memoria acotada de la sesión recuperada"
            ),
        }
    }
}
impl std::error::Error for SessionRecoveryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        if let Self::Io(error) = self {
            Some(error)
        } else {
            None
        }
    }
}
impl From<io::Error> for SessionRecoveryError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
fn invalid(reason: &'static str) -> SessionRecoveryError {
    SessionRecoveryError::InvalidFormat(reason)
}

fn encoding_tag(encoding: Option<TextEncoding>) -> u64 {
    match encoding {
        None => 0,
        Some(TextEncoding::Utf8) => 1,
        Some(TextEncoding::Windows1252) => 2,
        Some(TextEncoding::Iso8859_1) => 3,
        Some(TextEncoding::Ibm850) => 4,
    }
}
fn source_tag(source: EncodingSource) -> u64 {
    match source {
        EncodingSource::Explicit => 0,
        EncodingSource::Utf8Bom => 1,
        EncodingSource::ValidUtf8 => 2,
        EncodingSource::AsciiCompatible => 3,
        EncodingSource::UndeterminedLegacy => 4,
    }
}
fn platform() -> u64 {
    #[cfg(windows)]
    {
        1
    }
    #[cfg(unix)]
    {
        2
    }
    #[cfg(not(any(windows, unix)))]
    {
        0
    }
}
fn path_units(path: &std::path::Path) -> Result<usize, SessionRecoveryError> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        Ok(path.as_os_str().encode_wide().count())
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        Ok(path.as_os_str().as_bytes().len())
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = path;
        Err(SessionRecoveryError::UnsupportedPathPlatform(0))
    }
}
impl SessionRecoveryInfo {
    fn validate(&self) -> Result<(), SessionRecoveryError> {
        if path_units(&self.path)? > MAX_PATH_UNITS {
            return Err(SessionRecoveryError::LimitExceeded("ruta nativa"));
        }
        if self.disk_fingerprint.is_some() && self.path.as_os_str().is_empty() {
            return Err(invalid("sesión de disco sin ruta"));
        }
        let valid = match self.encoding_source {
            EncodingSource::Explicit => self.encoding.is_some(),
            EncodingSource::Utf8Bom
            | EncodingSource::ValidUtf8
            | EncodingSource::AsciiCompatible => self.encoding == Some(TextEncoding::Utf8),
            EncodingSource::UndeterminedLegacy => self.encoding.is_none(),
        };
        if !valid {
            return Err(invalid("codificación y procedencia incompatibles"));
        }
        Ok(())
    }
    /// Native paths are serialized losslessly, never through UTF-8 display text.
    /// This method writes one framed metadata record; Document follows separately.
    pub fn write_to(&self, writer: &mut dyn Write) -> io::Result<()> {
        self.validate()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        writer.write_all(MAGIC)?;
        write_integer(writer, VERSION)?;
        write_recovery_path(&self.path, writer)?;
        write_integer(writer, encoding_tag(self.encoding))?;
        write_integer(writer, source_tag(self.encoding_source))?;
        write_integer(writer, u64::from(self.disk_fingerprint.is_some()))?;
        if let Some(fingerprint) = self.disk_fingerprint {
            let (len, first, second) = fingerprint.words();
            write_integer(writer, len)?;
            write_integer(writer, first)?;
            write_integer(writer, second)?;
        }
        writer.write_all(END)
    }
    /// Only metadata is consumed. No open/stat/probe/save of `path` occurs here.
    /// A foreign platform's path representation is rejected rather than decoded
    /// lossily. Trailing bytes belong to the caller's next framed record.
    pub fn read_from(
        reader: &mut dyn Read,
        limits: SessionRecoveryLimits,
    ) -> Result<Self, SessionRecoveryError> {
        let mut decoder = Decoder {
            reader,
            limits,
            encoded: 0,
        };
        let mut magic = [0u8; 8];
        decoder.bytes(&mut magic)?;
        if &magic != MAGIC {
            return Err(invalid("encabezado"));
        }
        let version = decoder.integer()?;
        if version != VERSION {
            return Err(SessionRecoveryError::UnsupportedVersion(version));
        }
        let path = decoder.native_path(32)?;
        let encoding = match decoder.integer()? {
            0 => None,
            1 => Some(TextEncoding::Utf8),
            2 => Some(TextEncoding::Windows1252),
            3 => Some(TextEncoding::Iso8859_1),
            4 => Some(TextEncoding::Ibm850),
            _ => return Err(invalid("etiqueta de codificación")),
        };
        let encoding_source = match decoder.integer()? {
            0 => EncodingSource::Explicit,
            1 => EncodingSource::Utf8Bom,
            2 => EncodingSource::ValidUtf8,
            3 => EncodingSource::AsciiCompatible,
            4 => EncodingSource::UndeterminedLegacy,
            _ => return Err(invalid("procedencia de codificación")),
        };
        let disk_fingerprint = match decoder.integer()? {
            0 => None,
            1 => Some(ContentFingerprint::from_words(
                decoder.integer()?,
                decoder.integer()?,
                decoder.integer()?,
            )),
            _ => return Err(invalid("etiqueta de versión de disco")),
        };
        decoder.bytes(&mut magic)?;
        if &magic != END {
            return Err(invalid("cierre"));
        }
        let info = Self {
            path,
            encoding,
            encoding_source,
            disk_fingerprint,
        };
        info.validate()?;
        Ok(info)
    }
}
impl FileSession {
    pub fn matches_recovery_info(&self, info: &SessionRecoveryInfo) -> bool {
        self.path == info.path
            && self.encoding == info.encoding
            && self.encoding_source == info.encoding_source
            && self.disk_fingerprint == info.disk_fingerprint
    }
    /// Copies only path/encoding/fingerprint metadata, never document bytes.
    pub fn recovery_info(&self) -> SessionRecoveryInfo {
        SessionRecoveryInfo {
            path: self.path.clone(),
            encoding: self.encoding,
            encoding_source: self.encoding_source,
            disk_fingerprint: self.disk_fingerprint,
        }
    }
    /// Restores ownership of an already validated canonical Document. Source
    /// encoding and dirty/undo state are retained without probing the disk path.
    pub fn from_recovery(
        info: SessionRecoveryInfo,
        document: Document,
    ) -> Result<Self, SessionRecoveryError> {
        info.validate()?;
        Ok(Self {
            path: info.path,
            document,
            encoding: info.encoding,
            encoding_source: info.encoding_source,
            disk_fingerprint: info.disk_fingerprint,
        })
    }
}
/// Standalone native path record for project roots and other outer metadata.
/// Uses the identical platform tag/count/body representation as FileSession.
pub fn write_recovery_path(path: &std::path::Path, writer: &mut dyn Write) -> io::Result<()> {
    let units =
        path_units(path).map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    if units > MAX_PATH_UNITS {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "La ruta recuperable supera32768 unidades nativas",
        ));
    }
    write_integer(writer, platform())?;
    write_integer(writer, units as u64)?;
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let mut buffer = [0u8; 4096];
        let mut filled = 0;
        for unit in path.as_os_str().encode_wide() {
            buffer[filled..filled + 2].copy_from_slice(&unit.to_le_bytes());
            filled += 2;
            if filled == buffer.len() {
                writer.write_all(&buffer)?;
                filled = 0;
            }
        }
        writer.write_all(&buffer[..filled])?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        writer.write_all(path.as_os_str().as_bytes())?;
    }
    Ok(())
}
pub fn read_recovery_path(
    reader: &mut dyn Read,
    max_units: usize,
) -> Result<PathBuf, SessionRecoveryError> {
    let mut decoder = Decoder {
        reader,
        limits: SessionRecoveryLimits {
            max_path_units: max_units,
            ..Default::default()
        },
        encoded: 0,
    };
    decoder.native_path(0)
}
fn write_integer(writer: &mut dyn Write, value: u64) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}
struct Decoder<'a> {
    reader: &'a mut dyn Read,
    limits: SessionRecoveryLimits,
    encoded: u64,
}
impl Decoder<'_> {
    fn bytes(&mut self, bytes: &mut [u8]) -> Result<(), SessionRecoveryError> {
        self.encoded = self
            .encoded
            .checked_add(bytes.len() as u64)
            .ok_or(invalid("longitud codificada"))?;
        if self.encoded > self.limits.max_encoded_bytes {
            return Err(SessionRecoveryError::LimitExceeded("bytes codificados"));
        }
        self.reader.read_exact(bytes)?;
        Ok(())
    }
    fn integer(&mut self) -> Result<u64, SessionRecoveryError> {
        let mut value = [0u8; 8];
        self.bytes(&mut value)?;
        Ok(u64::from_le_bytes(value))
    }
    fn native_path(&mut self, trailing_min: u64) -> Result<PathBuf, SessionRecoveryError> {
        let native_platform = self.integer()?;
        if native_platform == 0 || native_platform != platform() {
            return Err(SessionRecoveryError::UnsupportedPathPlatform(
                native_platform,
            ));
        }
        let units = usize::try_from(self.integer()?)
            .map_err(|_| invalid("longitud de ruta no representable"))?;
        if units > self.limits.max_path_units.min(MAX_PATH_UNITS) {
            return Err(SessionRecoveryError::LimitExceeded("ruta nativa"));
        }
        let bytes = units
            .checked_mul(if native_platform == 1 { 2 } else { 1 })
            .ok_or(invalid("longitud de ruta"))?;
        if self
            .encoded
            .checked_add(bytes as u64)
            .and_then(|n| n.checked_add(trailing_min))
            .is_none_or(|end| end > self.limits.max_encoded_bytes)
        {
            return Err(SessionRecoveryError::LimitExceeded(
                "ruta y metadatos codificados",
            ));
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStringExt;
            let mut native = Vec::<u16>::new();
            native
                .try_reserve_exact(units)
                .map_err(|_| SessionRecoveryError::Allocation)?;
            let mut scratch = [0u8; 4096];
            while native.len() < units {
                let count = (units - native.len()).min(scratch.len() / 2);
                self.bytes(&mut scratch[..count * 2])?;
                for unit in scratch[..count * 2].chunks_exact(2) {
                    native.push(u16::from_le_bytes([unit[0], unit[1]]));
                }
            }
            Ok(PathBuf::from(std::ffi::OsString::from_wide(&native)))
        }
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            let mut native = Vec::new();
            native
                .try_reserve_exact(units)
                .map_err(|_| SessionRecoveryError::Allocation)?;
            native.resize(units, 0);
            self.bytes(&mut native)?;
            Ok(PathBuf::from(std::ffi::OsString::from_vec(native)))
        }
        #[cfg(not(any(windows, unix)))]
        {
            Err(SessionRecoveryError::UnsupportedPathPlatform(
                native_platform,
            ))
        }
    }
}
