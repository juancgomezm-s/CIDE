//! On-demand embedded Rust Git snapshots. No executable, index mutation or UI.
use std::{
    fmt, io,
    path::{Component, Path, PathBuf},
    time::Duration,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitBlobSource {
    Head,
    Index,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitObjectId(String);
impl GitObjectId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
    /// Heap storage retained by this identifier, for owned result accounting.
    pub fn memory_bytes(&self) -> usize {
        self.0.capacity()
    }
    fn parse(bytes: &[u8]) -> Result<Self, GitReadError> {
        if ![40, 64].contains(&bytes.len())
            || !bytes.iter().all(u8::is_ascii_hexdigit)
            || bytes.iter().all(|&b| b == b'0')
        {
            return Err(GitReadError::Protocol);
        }
        Ok(Self(
            std::str::from_utf8(bytes)
                .map_err(|_| GitReadError::Protocol)?
                .to_ascii_lowercase(),
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitFileMode {
    Regular,
    Executable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRepositoryIdentity {
    pub root: PathBuf,
    pub git_dir: PathBuf,
}

/// Identity of the selected object, not a promise that mutable HEAD/index still
/// select it after delivery. Index identity is path/blob/mode, not a whole-index CAS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitBlobIdentity {
    pub repository: GitRepositoryIdentity,
    pub path: String,
    pub source: GitBlobSource,
    pub commit: Option<GitObjectId>,
    pub blob: GitObjectId,
    pub mode: GitFileMode,
}

pub struct GitBlobSnapshot {
    identity: GitBlobIdentity,
    bytes: Vec<u8>,
}
impl GitBlobSnapshot {
    pub fn into_parts(self) -> (GitBlobIdentity, Vec<u8>) {
        (self.identity, self.bytes)
    }
    pub fn identity(&self) -> &GitBlobIdentity {
        &self.identity
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}
impl fmt::Debug for GitBlobSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GitBlobSnapshot")
            .field("identity", &self.identity)
            .field("bytes", &self.bytes.len())
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GitReadLimits {
    pub max_blob_bytes: usize,
    pub max_metadata_bytes: usize,
    pub timeout: Duration,
}
impl Default for GitReadLimits {
    fn default() -> Self {
        Self {
            max_blob_bytes: 2 * 1024 * 1024,
            max_metadata_bytes: 64 * 1024,
            timeout: Duration::from_secs(5),
        }
    }
}
impl GitReadLimits {
    pub(crate) fn valid(self) -> bool {
        self.max_blob_bytes <= 16 * 1024 * 1024
            && (1..=1024 * 1024).contains(&self.max_metadata_bytes)
            && !self.timeout.is_zero()
            && self.timeout <= Duration::from_secs(30)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitOutputLimit {
    Metadata,
    Blob,
}
impl GitOutputLimit {
    pub fn label(self) -> &'static str {
        match self {
            Self::Metadata => "metadatos",
            Self::Blob => "blob",
        }
    }
}

#[derive(Debug)]
pub enum GitReadError {
    InvalidLimits,
    InvalidPath,
    UnsupportedPathEncoding,
    NotRepositoryRoot,
    NotRepository,
    Missing,
    Unmerged,
    UnsupportedMode,
    /// Repository clean/process drivers need a separate execution policy.
    ConfiguredFilter,
    Protocol,
    Cancelled,
    TimedOut,
    OutputLimit(GitOutputLimit),
    Io(io::Error),
}
impl fmt::Display for GitReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "Lectura Git, entrada/salida: {error}"),
            Self::InvalidLimits => f.write_str("Lectura Git: límites inválidos"),
            Self::InvalidPath => f.write_str("Lectura Git: ruta inválida"),
            Self::UnsupportedPathEncoding => {
                f.write_str("Lectura Git: codificación de ruta no admitida")
            }
            Self::NotRepositoryRoot => {
                f.write_str("Lectura Git: el directorio no es la raíz del repositorio")
            }
            Self::NotRepository => f.write_str("La carpeta abierta no contiene un repositorio Git"),
            Self::Missing => f.write_str("Lectura Git: objeto o entrada no encontrado"),
            Self::Unmerged => f.write_str("Lectura Git: entrada con conflicto sin resolver"),
            Self::UnsupportedMode => f.write_str("Lectura Git: formato o capacidad no admitidos"),
            Self::ConfiguredFilter => f.write_str("Lectura Git: filtro configurado no admitido"),
            Self::Protocol => f.write_str("Lectura Git: protocolo inválido"),
            Self::Cancelled => f.write_str("Lectura Git: cancelada"),
            Self::TimedOut => f.write_str("Lectura Git: tiempo agotado"),
            Self::OutputLimit(limit) => {
                write!(f, "Lectura Git: cuota excedida ({})", limit.label())
            }
        }
    }
}
impl std::error::Error for GitReadError {}
impl From<io::Error> for GitReadError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Embedded Rust engine. There is no executable or process transport.
#[derive(Debug, Default)]
pub struct GitReader;
impl GitReader {
    pub fn new() -> Result<Self, GitReadError> {
        Ok(Self)
    }
    pub fn read_blob(
        &self,
        repository_root: &Path,
        source: GitBlobSource,
        path: &Path,
        limits: GitReadLimits,
        cancelled: impl FnMut() -> bool + Send,
    ) -> Result<GitBlobSnapshot, GitReadError> {
        if !limits.valid() {
            return Err(GitReadError::InvalidLimits);
        }
        let path = validate_path(path)?;
        let blob = jc_git::read_blob(
            repository_root,
            source == GitBlobSource::Index,
            &path,
            engine_limits(limits),
            cancelled,
        )
        .map_err(|e| engine_error(e, GitOutputLimit::Blob))?;
        Ok(GitBlobSnapshot {
            identity: GitBlobIdentity {
                repository: GitRepositoryIdentity {
                    root: blob.repository.root,
                    git_dir: blob.repository.git_dir,
                },
                path,
                source,
                commit: blob
                    .commit
                    .map(|id| GitObjectId::parse(id.as_bytes()))
                    .transpose()?,
                blob: GitObjectId::parse(blob.oid.as_bytes())?,
                mode: if blob.mode == 0o100755 {
                    GitFileMode::Executable
                } else {
                    GitFileMode::Regular
                },
            },
            bytes: blob.bytes,
        })
    }
    pub(crate) fn read_status(
        &self,
        root: &Path,
        limits: crate::git_status::GitStatusReadOptions,
        cancelled: impl FnMut() -> bool + Send,
    ) -> Result<jc_git::Status, GitReadError> {
        let mut options = engine_limits(limits.git);
        options.max_entries = limits.parse.max_entries;
        options.max_path = limits.parse.max_path_bytes;
        options.max_bytes = limits
            .parse
            .max_owned_bytes
            .min(limits.parse.max_input_bytes);
        jc_git::read_status(root, options, cancelled)
            .map_err(|e| engine_error(e, GitOutputLimit::Metadata))
    }
}
fn engine_limits(limits: GitReadLimits) -> jc_git::Limits {
    jc_git::Limits {
        max_blob: limits.max_blob_bytes,
        max_entries: 4096,
        max_path: 4096,
        max_bytes: limits.max_metadata_bytes,
        timeout: limits.timeout,
    }
}
fn engine_error(e: jc_git::Error, limit: GitOutputLimit) -> GitReadError {
    match e {
        jc_git::Error::Io(e) => GitReadError::Io(e),
        jc_git::Error::InvalidRoot => GitReadError::NotRepositoryRoot,
        jc_git::Error::NotRepository => GitReadError::NotRepository,
        jc_git::Error::Missing => GitReadError::Missing,
        jc_git::Error::Unmerged => GitReadError::Unmerged,
        jc_git::Error::Unsupported => GitReadError::UnsupportedMode,
        jc_git::Error::Filter => GitReadError::ConfiguredFilter,
        jc_git::Error::Limited => GitReadError::OutputLimit(limit),
        jc_git::Error::Cancelled => GitReadError::Cancelled,
        jc_git::Error::TimedOut => GitReadError::TimedOut,
        jc_git::Error::Stale => {
            GitReadError::Io(io::Error::other("Las fuentes Git cambiaron; actualiza"))
        }
        jc_git::Error::Engine(e) => GitReadError::Io(io::Error::other(e)),
    }
}

fn validate_path(path: &Path) -> Result<String, GitReadError> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err(GitReadError::InvalidPath);
    }
    let raw = path.to_str().ok_or(GitReadError::UnsupportedPathEncoding)?;
    if raw
        .split(['/', '\\'])
        .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(GitReadError::InvalidPath);
    }
    let mut parts = Vec::new();
    for component in path.components() {
        let Component::Normal(value) = component else {
            return Err(GitReadError::InvalidPath);
        };
        let value = value
            .to_str()
            .ok_or(GitReadError::UnsupportedPathEncoding)?;
        let device = value.split('.').next().unwrap_or("").to_ascii_uppercase();
        let reserved = matches!(device.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || device
                .strip_prefix("COM")
                .or_else(|| device.strip_prefix("LPT"))
                .is_some_and(|n| matches!(n, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9"));
        if value.is_empty()
            || value.eq_ignore_ascii_case(".git")
            || reserved
            || value.contains(['\0', '\r', '\n', ':', '<', '>', '"', '|', '?', '*'])
            || value.ends_with([' ', '.'])
        {
            return Err(GitReadError::InvalidPath);
        }
        parts.push(value);
    }
    let result = parts.join("/");
    if result.is_empty() || result.len() > 4096 {
        return Err(GitReadError::InvalidPath);
    }
    Ok(result)
}

#[cfg(test)]
mod absent_repository_tests {
    use super::*;
    #[test]
    fn absent_repository_is_distinct_from_invalid_root_corruption_and_missing_blob() {
        assert!(matches!(
            engine_error(jc_git::Error::NotRepository, GitOutputLimit::Metadata),
            GitReadError::NotRepository
        ));
        assert!(matches!(
            engine_error(jc_git::Error::InvalidRoot, GitOutputLimit::Metadata),
            GitReadError::NotRepositoryRoot
        ));
        assert!(matches!(
            engine_error(jc_git::Error::Missing, GitOutputLimit::Blob),
            GitReadError::Missing
        ));
        assert!(matches!(
            engine_error(
                jc_git::Error::Engine("metadatos dañados".into()),
                GitOutputLimit::Metadata
            ),
            GitReadError::Io(_)
        ));
        assert_eq!(
            GitReadError::NotRepository.to_string(),
            "La carpeta abierta no contiene un repositorio Git"
        );
    }
}
