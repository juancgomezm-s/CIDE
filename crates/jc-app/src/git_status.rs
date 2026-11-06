//! Bounded, on-demand repository status. Paths remain opaque bytes, never OS paths.
use crate::git_read::{GitReadError, GitReadLimits, GitReader, GitRepositoryIdentity};
use std::{fmt, mem::size_of, path::Path, time::Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitStatusFormat {
    Embedded,
    PorcelainV1,
    PorcelainV2,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitStatusChange {
    Unchanged,
    Modified,
    Added,
    Deleted,
    Renamed,
    Copied,
    TypeChanged,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitConflict {
    BothDeleted,
    AddedByUs,
    DeletedByThem,
    AddedByThem,
    DeletedByUs,
    BothAdded,
    BothModified,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitStatusState {
    Tracked {
        index: GitStatusChange,
        worktree: GitStatusChange,
    },
    Conflict(GitConflict),
    Untracked,
    Ignored,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitSubmoduleState {
    Unknown,
    NotSubmodule,
    Submodule {
        commit_changed: bool,
        tracked_changes: bool,
        untracked_changes: bool,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitSimilarityKind {
    Rename,
    Copy,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GitSimilarity {
    pub kind: GitSimilarityKind,
    pub percent: u8,
}

/// Repository-relative Git bytes. This is not authority to open/navigate a file.
#[derive(PartialEq, Eq)]
struct StatusPath(Box<[u8]>);
impl fmt::Debug for StatusPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StatusPath")
            .field("bytes", &self.0.len())
            .finish()
    }
}
#[derive(Debug, PartialEq, Eq)]
pub struct GitStatusEntry {
    path: StatusPath,
    original_path: Option<StatusPath>,
    state: GitStatusState,
    submodule: GitSubmoduleState,
    similarity: Option<GitSimilarity>,
}
impl GitStatusEntry {
    pub fn path(&self) -> &[u8] {
        &self.path.0
    }
    pub fn original_path(&self) -> Option<&[u8]> {
        self.original_path.as_ref().map(|p| p.0.as_ref())
    }
    pub fn state(&self) -> GitStatusState {
        self.state
    }
    pub fn submodule(&self) -> GitSubmoduleState {
        self.submodule
    }
    pub fn similarity(&self) -> Option<GitSimilarity> {
        self.similarity
    }
    fn path_bytes(&self) -> usize {
        self.path.0.len() + self.original_path.as_ref().map_or(0, |p| p.0.len())
    }
}
#[derive(Debug, PartialEq, Eq)]
pub struct GitStatus {
    format: GitStatusFormat,
    entries: Vec<GitStatusEntry>,
}
impl GitStatus {
    pub fn format(&self) -> GitStatusFormat {
        self.format
    }
    pub fn entries(&self) -> &[GitStatusEntry] {
        &self.entries
    }
    fn heap_bytes(&self) -> usize {
        self.entries.capacity() * size_of::<GitStatusEntry>()
            + self
                .entries
                .iter()
                .map(GitStatusEntry::path_bytes)
                .sum::<usize>()
    }
    /// Logical root/entry capacities and exact boxed path bytes, not process RAM.
    pub fn memory_bytes(&self) -> usize {
        size_of::<Self>() + self.heap_bytes()
    }
}
#[derive(Debug)]
pub struct GitStatusSnapshot {
    repository: GitRepositoryIdentity,
    status: GitStatus,
    info: Option<jc_git::RepositoryInfo>,
}
impl GitStatusSnapshot {
    /// Compose an immutable observation; this does not certify a repository CAS.
    pub fn from_parts(repository: GitRepositoryIdentity, status: GitStatus) -> Self {
        Self {
            repository,
            status,
            info: None,
        }
    }
    pub fn repository(&self) -> &GitRepositoryIdentity {
        &self.repository
    }
    pub fn status(&self) -> &GitStatus {
        &self.status
    }
    pub fn info(&self) -> Option<&jc_git::RepositoryInfo> {
        self.info.as_ref()
    }
    pub fn memory_bytes(&self) -> usize {
        size_of::<Self>()
            + self.repository.root.capacity()
            + self.repository.git_dir.capacity()
            + self.status.heap_bytes()
            + self.info.as_ref().map_or(0, |info| info.memory_bytes())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GitStatusLimits {
    pub max_input_bytes: usize,
    pub max_entries: usize,
    /// NUL records, including headers and the second path of rename/copy.
    pub max_records: usize,
    pub max_path_bytes: usize,
    pub max_owned_bytes: usize,
}
impl Default for GitStatusLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 1024 * 1024,
            max_entries: 4096,
            max_records: 8256,
            max_path_bytes: 4096,
            max_owned_bytes: 2 * 1024 * 1024,
        }
    }
}
impl GitStatusLimits {
    pub fn valid(self) -> bool {
        self.max_input_bytes <= 1024 * 1024
            && self.max_entries <= 16384
            && self.max_records <= 32896
            && (1..=4096).contains(&self.max_path_bytes)
            && (size_of::<GitStatus>()..=4 * 1024 * 1024).contains(&self.max_owned_bytes)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GitStatusReadOptions {
    pub git: GitReadLimits,
    pub parse: GitStatusLimits,
}
impl Default for GitStatusReadOptions {
    fn default() -> Self {
        Self {
            git: GitReadLimits {
                max_metadata_bytes: 1024 * 1024,
                ..GitReadLimits::default()
            },
            parse: GitStatusLimits::default(),
        }
    }
}
impl GitStatusReadOptions {
    pub fn valid(self) -> bool {
        self.git.valid() && self.parse.valid()
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitStatusLimit {
    InputBytes,
    Entries,
    Records,
    PathBytes,
    OwnedBytes,
}
#[derive(Debug)]
pub enum GitStatusError {
    Read(GitReadError),
    InvalidLimits,
    Protocol,
    InvalidPath,
    Limited(GitStatusLimit),
    Cancelled,
    TimedOut,
    Allocation,
}
impl fmt::Display for GitStatusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(error) => write!(f, "Estado Git: {error}"),
            Self::InvalidLimits => f.write_str("Estado Git: límites inválidos"),
            Self::Protocol => f.write_str("Estado Git: protocolo inválido"),
            Self::InvalidPath => f.write_str("Estado Git: ruta inválida"),
            Self::Limited(limit) => write!(
                f,
                "Estado Git: cuota excedida ({})",
                match limit {
                    GitStatusLimit::InputBytes => "bytes de entrada",
                    GitStatusLimit::Entries => "entradas",
                    GitStatusLimit::Records => "registros",
                    GitStatusLimit::PathBytes => "bytes de ruta",
                    GitStatusLimit::OwnedBytes => "bytes retenidos",
                }
            ),
            Self::Cancelled => f.write_str("Estado Git: cancelado"),
            Self::TimedOut => f.write_str("Estado Git: tiempo agotado"),
            Self::Allocation => f.write_str("Estado Git: fallo al reservar memoria"),
        }
    }
}
impl std::error::Error for GitStatusError {}
impl From<GitReadError> for GitStatusError {
    fn from(error: GitReadError) -> Self {
        match error {
            GitReadError::Cancelled => Self::Cancelled,
            GitReadError::TimedOut => Self::TimedOut,
            other => Self::Read(other),
        }
    }
}

/// Synchronous worker operation. Never call from input dispatch or rendering.
/// Configured clean/process drivers are rejected by the shared transport.
pub fn read_status(
    reader: &GitReader,
    repository_root: &Path,
    limits: GitStatusReadOptions,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<GitStatusSnapshot, GitStatusError> {
    if !limits.valid() {
        return Err(GitStatusError::InvalidLimits);
    }
    let raw = reader.read_status(repository_root, limits, cancelled)?;
    let repository = GitRepositoryIdentity {
        root: raw.repository.root,
        git_dir: raw.repository.git_dir,
    };
    let entries = raw
        .entries
        .into_iter()
        .map(|entry| GitStatusEntry {
            path: StatusPath(entry.path.into_boxed_slice()),
            original_path: None,
            similarity: None,
            state: match entry.kind {
                jc_git::Kind::Untracked => GitStatusState::Untracked,
                jc_git::Kind::Tracked => GitStatusState::Tracked {
                    index: embedded_change(entry.index),
                    worktree: embedded_change(entry.worktree),
                },
                jc_git::Kind::Conflict(mask) => GitStatusState::Conflict(match mask {
                    1 => GitConflict::BothDeleted,
                    2 => GitConflict::AddedByUs,
                    3 => GitConflict::DeletedByThem,
                    4 => GitConflict::AddedByThem,
                    5 => GitConflict::DeletedByUs,
                    6 => GitConflict::BothAdded,
                    _ => GitConflict::BothModified,
                }),
            },
            submodule: if entry.submodule {
                GitSubmoduleState::Submodule {
                    commit_changed: false,
                    tracked_changes: false,
                    untracked_changes: false,
                }
            } else {
                GitSubmoduleState::NotSubmodule
            },
        })
        .collect();
    let mut snapshot = GitStatusSnapshot::from_parts(
        repository,
        GitStatus {
            format: GitStatusFormat::Embedded,
            entries,
        },
    );
    snapshot.info = Some(raw.info);
    if snapshot.memory_bytes() > limits.parse.max_owned_bytes {
        return Err(GitStatusError::Limited(GitStatusLimit::OwnedBytes));
    }
    Ok(snapshot)
}
fn embedded_change(change: jc_git::Change) -> GitStatusChange {
    match change {
        jc_git::Change::Unchanged => GitStatusChange::Unchanged,
        jc_git::Change::Modified => GitStatusChange::Modified,
        jc_git::Change::Added => GitStatusChange::Added,
        jc_git::Change::Deleted => GitStatusChange::Deleted,
        jc_git::Change::TypeChanged => GitStatusChange::TypeChanged,
    }
}
pub fn parse_porcelain_v2(
    input: &[u8],
    limits: GitStatusLimits,
    mut cancelled: impl FnMut() -> bool,
) -> Result<GitStatus, GitStatusError> {
    parse(
        input,
        GitStatusFormat::PorcelainV2,
        limits,
        &mut cancelled,
        None,
    )
}
pub fn parse_porcelain_v1(
    input: &[u8],
    limits: GitStatusLimits,
    mut cancelled: impl FnMut() -> bool,
) -> Result<GitStatus, GitStatusError> {
    parse(
        input,
        GitStatusFormat::PorcelainV1,
        limits,
        &mut cancelled,
        None,
    )
}

struct Control<'a> {
    cancelled: &'a mut dyn FnMut() -> bool,
    deadline: Option<Instant>,
}
impl Control<'_> {
    fn check(&mut self) -> Result<(), GitStatusError> {
        if (self.cancelled)() {
            Err(GitStatusError::Cancelled)
        } else if self.deadline.is_some_and(|d| Instant::now() >= d) {
            Err(GitStatusError::TimedOut)
        } else {
            Ok(())
        }
    }
}
struct Cursor<'a> {
    input: &'a [u8],
    offset: usize,
    records: usize,
    max_records: usize,
}
impl Cursor<'_> {
    fn next(
        &mut self,
        control: &mut Control<'_>,
    ) -> Result<std::ops::Range<usize>, GitStatusError> {
        control.check()?;
        if self.records >= self.max_records {
            return Err(GitStatusError::Limited(GitStatusLimit::Records));
        }
        let start = self.offset;
        while self.offset < self.input.len() {
            control.check()?;
            let end = (self.offset + 4096).min(self.input.len());
            if let Some(n) = self.input[self.offset..end].iter().position(|&b| b == 0) {
                let finish = self.offset + n;
                self.offset = finish + 1;
                self.records += 1;
                if finish == start {
                    return Err(GitStatusError::Protocol);
                }
                return Ok(start..finish);
            }
            self.offset = end;
        }
        Err(GitStatusError::Protocol)
    }
}
struct Parsed<'a> {
    path: &'a [u8],
    original: Option<&'a [u8]>,
    state: GitStatusState,
    submodule: GitSubmoduleState,
    similarity: Option<GitSimilarity>,
}
fn parse(
    input: &[u8],
    format: GitStatusFormat,
    limits: GitStatusLimits,
    cancelled: &mut dyn FnMut() -> bool,
    deadline: Option<Instant>,
) -> Result<GitStatus, GitStatusError> {
    if !limits.valid() {
        return Err(GitStatusError::InvalidLimits);
    }
    let mut control = Control {
        cancelled,
        deadline,
    };
    control.check()?;
    if input.len() > limits.max_input_bytes {
        return Err(GitStatusError::Limited(GitStatusLimit::InputBytes));
    }
    let mut cursor = Cursor {
        input,
        offset: 0,
        records: 0,
        max_records: limits.max_records,
    };
    let mut status = GitStatus {
        format,
        entries: Vec::new(),
    };
    let mut path_bytes = 0;
    while cursor.offset < input.len() {
        let record = &input[cursor.next(&mut control)?];
        if format == GitStatusFormat::PorcelainV2 && record.starts_with(b"# ") {
            continue;
        }
        let parsed = match format {
            GitStatusFormat::PorcelainV1 => parse_v1(record, &mut cursor, &mut control)?,
            GitStatusFormat::PorcelainV2 => parse_v2(record, &mut cursor, &mut control)?,
            GitStatusFormat::Embedded => return Err(GitStatusError::Protocol),
        };
        let directory = matches!(
            parsed.state,
            GitStatusState::Untracked | GitStatusState::Ignored
        );
        validate_path(parsed.path, limits, directory)?;
        if let Some(path) = parsed.original {
            validate_path(path, limits, false)?;
        }
        if status.entries.len() >= limits.max_entries {
            return Err(GitStatusError::Limited(GitStatusLimit::Entries));
        }
        let added = parsed.path.len() + parsed.original.map_or(0, <[u8]>::len);
        let base = size_of::<GitStatus>() + path_bytes + added;
        if status.entries.len() == status.entries.capacity() {
            let possible =
                limits.max_owned_bytes.saturating_sub(base) / size_of::<GitStatusEntry>();
            let target = status
                .entries
                .capacity()
                .max(4)
                .saturating_mul(2)
                .min(limits.max_entries)
                .min(possible);
            if target <= status.entries.len() {
                return Err(GitStatusError::Limited(GitStatusLimit::OwnedBytes));
            }
            status
                .entries
                .try_reserve_exact(target - status.entries.len())
                .map_err(|_| GitStatusError::Allocation)?;
        }
        if base + status.entries.capacity() * size_of::<GitStatusEntry>() > limits.max_owned_bytes {
            return Err(GitStatusError::Limited(GitStatusLimit::OwnedBytes));
        }
        control.check()?;
        status.entries.push(GitStatusEntry {
            path: owned_path(parsed.path)?,
            original_path: parsed.original.map(owned_path).transpose()?,
            state: parsed.state,
            submodule: parsed.submodule,
            similarity: parsed.similarity,
        });
        path_bytes += added;
    }
    control.check()?;
    Ok(status)
}
fn owned_path(bytes: &[u8]) -> Result<StatusPath, GitStatusError> {
    let mut owned = Vec::new();
    owned
        .try_reserve_exact(bytes.len())
        .map_err(|_| GitStatusError::Allocation)?;
    owned.extend_from_slice(bytes);
    Ok(StatusPath(owned.into_boxed_slice()))
}
fn validate_path(
    path: &[u8],
    limits: GitStatusLimits,
    directory: bool,
) -> Result<(), GitStatusError> {
    if path.len() > limits.max_path_bytes {
        return Err(GitStatusError::Limited(GitStatusLimit::PathBytes));
    }
    let components = if directory {
        path.strip_suffix(b"/").unwrap_or(path)
    } else {
        path
    };
    if components.is_empty()
        || components
            .split(|&b| b == b'/')
            .any(|p| p.is_empty() || p == b"." || p == b".." || p.eq_ignore_ascii_case(b".git"))
    {
        return Err(GitStatusError::InvalidPath);
    }
    Ok(())
}
fn fields<const N: usize>(record: &[u8]) -> Result<([&[u8]; N], &[u8]), GitStatusError> {
    let mut result: [&[u8]; N] = [b""; N];
    let mut rest = record;
    for field in &mut result {
        // Every fixed protocol token fits in 128 bytes (OID <=64). A malformed
        // token cannot hide an uninterruptible scan through a huge path body.
        let end = rest
            .iter()
            .take(128)
            .position(|&b| b == b' ')
            .ok_or(GitStatusError::Protocol)?;
        if end == 0 {
            return Err(GitStatusError::Protocol);
        }
        *field = &rest[..end];
        rest = &rest[end + 1..];
    }
    Ok((result, rest))
}
fn state(xy: &[u8], unchanged: u8) -> Result<GitStatusState, GitStatusError> {
    if xy.len() != 2 {
        return Err(GitStatusError::Protocol);
    }
    let conflict = match xy {
        b"DD" => Some(GitConflict::BothDeleted),
        b"AU" => Some(GitConflict::AddedByUs),
        b"UD" => Some(GitConflict::DeletedByThem),
        b"UA" => Some(GitConflict::AddedByThem),
        b"DU" => Some(GitConflict::DeletedByUs),
        b"AA" => Some(GitConflict::BothAdded),
        b"UU" => Some(GitConflict::BothModified),
        _ => None,
    };
    if let Some(kind) = conflict {
        return Ok(GitStatusState::Conflict(kind));
    }
    let change = |b| {
        if b == unchanged {
            Ok(GitStatusChange::Unchanged)
        } else {
            match b {
                b'M' => Ok(GitStatusChange::Modified),
                b'A' => Ok(GitStatusChange::Added),
                b'D' => Ok(GitStatusChange::Deleted),
                b'R' => Ok(GitStatusChange::Renamed),
                b'C' => Ok(GitStatusChange::Copied),
                b'T' => Ok(GitStatusChange::TypeChanged),
                _ => Err(GitStatusError::Protocol),
            }
        }
    };
    let index = change(xy[0])?;
    let worktree = change(xy[1])?;
    if index == GitStatusChange::Unchanged && worktree == GitStatusChange::Unchanged {
        return Err(GitStatusError::Protocol);
    }
    Ok(GitStatusState::Tracked { index, worktree })
}
fn renamed(state: GitStatusState) -> bool {
    matches!(
        state,
        GitStatusState::Tracked {
            index: GitStatusChange::Renamed | GitStatusChange::Copied,
            ..
        } | GitStatusState::Tracked {
            worktree: GitStatusChange::Renamed | GitStatusChange::Copied,
            ..
        }
    )
}
fn submodule(bytes: &[u8]) -> Result<GitSubmoduleState, GitStatusError> {
    if bytes == b"N..." {
        return Ok(GitSubmoduleState::NotSubmodule);
    }
    if bytes.len() != 4
        || bytes[0] != b'S'
        || ![b'.', b'C'].contains(&bytes[1])
        || ![b'.', b'M'].contains(&bytes[2])
        || ![b'.', b'U'].contains(&bytes[3])
    {
        return Err(GitStatusError::Protocol);
    }
    Ok(GitSubmoduleState::Submodule {
        commit_changed: bytes[1] == b'C',
        tracked_changes: bytes[2] == b'M',
        untracked_changes: bytes[3] == b'U',
    })
}
fn metadata(modes: &[&[u8]], oids: &[&[u8]]) -> Result<(), GitStatusError> {
    if modes
        .iter()
        .any(|m| m.len() != 6 || !m.iter().all(|b| (b'0'..=b'7').contains(b)))
        || oids
            .iter()
            .any(|id| ![40, 64].contains(&id.len()) || !id.iter().all(u8::is_ascii_hexdigit))
        || oids.windows(2).any(|ids| ids[0].len() != ids[1].len())
    {
        return Err(GitStatusError::Protocol);
    }
    Ok(())
}
fn parse_v1<'a>(
    record: &'a [u8],
    cursor: &mut Cursor<'a>,
    control: &mut Control<'_>,
) -> Result<Parsed<'a>, GitStatusError> {
    if record.len() < 4 || record[2] != b' ' {
        return Err(GitStatusError::Protocol);
    }
    let state = match &record[..2] {
        b"??" => GitStatusState::Untracked,
        b"!!" => GitStatusState::Ignored,
        xy => state(xy, b' ')?,
    };
    let original = if renamed(state) {
        let range = cursor.next(control)?;
        Some(&cursor.input[range])
    } else {
        None
    };
    Ok(Parsed {
        path: &record[3..],
        original,
        state,
        submodule: GitSubmoduleState::Unknown,
        similarity: None,
    })
}
fn parse_v2<'a>(
    record: &'a [u8],
    cursor: &mut Cursor<'a>,
    control: &mut Control<'_>,
) -> Result<Parsed<'a>, GitStatusError> {
    if record.starts_with(b"? ") || record.starts_with(b"! ") {
        return Ok(Parsed {
            path: &record[2..],
            original: None,
            state: if record[0] == b'?' {
                GitStatusState::Untracked
            } else {
                GitStatusState::Ignored
            },
            submodule: GitSubmoduleState::Unknown,
            similarity: None,
        });
    }
    match record.first() {
        Some(b'1') => {
            let (f, path) = fields::<8>(record)?;
            if f[0] != b"1" {
                return Err(GitStatusError::Protocol);
            }
            metadata(&f[3..6], &f[6..8])?;
            let state = state(f[1], b'.')?;
            if !matches!(state, GitStatusState::Tracked { .. }) || renamed(state) {
                return Err(GitStatusError::Protocol);
            }
            Ok(Parsed {
                path,
                original: None,
                state,
                submodule: submodule(f[2])?,
                similarity: None,
            })
        }
        Some(b'2') => {
            let (f, path) = fields::<9>(record)?;
            if f[0] != b"2" {
                return Err(GitStatusError::Protocol);
            }
            metadata(&f[3..6], &f[6..8])?;
            let state = state(f[1], b'.')?;
            if !renamed(state) || !(2..=4).contains(&f[8].len()) {
                return Err(GitStatusError::Protocol);
            }
            let kind = match f[8][0] {
                b'R' => GitSimilarityKind::Rename,
                b'C' => GitSimilarityKind::Copy,
                _ => return Err(GitStatusError::Protocol),
            };
            let expected = if kind == GitSimilarityKind::Rename {
                b'R'
            } else {
                b'C'
            };
            if !f[1].contains(&expected) || !f[8][1..].iter().all(u8::is_ascii_digit) {
                return Err(GitStatusError::Protocol);
            }
            let percent = f[8][1..]
                .iter()
                .fold(0u16, |n, b| n * 10 + u16::from(b - b'0'));
            if percent > 100 {
                return Err(GitStatusError::Protocol);
            }
            let range = cursor.next(control)?;
            let original = Some(&cursor.input[range]);
            Ok(Parsed {
                path,
                original,
                state,
                submodule: submodule(f[2])?,
                similarity: Some(GitSimilarity {
                    kind,
                    percent: percent as u8,
                }),
            })
        }
        Some(b'u') => {
            let (f, path) = fields::<10>(record)?;
            if f[0] != b"u" {
                return Err(GitStatusError::Protocol);
            }
            metadata(&f[3..7], &f[7..10])?;
            let state = state(f[1], b'.')?;
            if !matches!(state, GitStatusState::Conflict(_)) {
                return Err(GitStatusError::Protocol);
            }
            Ok(Parsed {
                path,
                original: None,
                state,
                submodule: submodule(f[2])?,
                similarity: None,
            })
        }
        _ => Err(GitStatusError::Protocol),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    const OID: &str = "1111111111111111111111111111111111111111";
    fn ordinary(xy: &str, path: &[u8]) -> Vec<u8> {
        let mut bytes = format!("1 {xy} N... 100644 100644 100644 {OID} {OID} ").into_bytes();
        bytes.extend_from_slice(path);
        bytes.push(0);
        bytes
    }
    fn parse2(bytes: &[u8]) -> GitStatus {
        parse_porcelain_v2(bytes, GitStatusLimits::default(), || false).unwrap()
    }
    #[test]
    fn v2_preserves_opaque_paths_states_headers_submodule_and_zero_sha256() {
        let mut input = b"# future.header ignored\0".to_vec();
        input.extend(ordinary("MM", b"dir/\xff space\tline\r\n\"\\.txt"));
        input.extend_from_slice(b"? new\xff\n.txt\0! ignored-dir/\0");
        let zero = "0".repeat(64);
        input.extend(format!("1 T. SCMU 000000 120000 100755 {zero} {zero} link\0").bytes());
        let status = parse2(&input);
        input.fill(0);
        assert_eq!(status.entries().len(), 4);
        assert_eq!(
            status.entries()[0].path(),
            b"dir/\xff space\tline\r\n\"\\.txt"
        );
        assert_eq!(
            status.entries()[0].state(),
            GitStatusState::Tracked {
                index: GitStatusChange::Modified,
                worktree: GitStatusChange::Modified
            }
        );
        assert_eq!(status.entries()[1].state(), GitStatusState::Untracked);
        assert_eq!(status.entries()[2].state(), GitStatusState::Ignored);
        assert_eq!(
            status.entries()[3].submodule(),
            GitSubmoduleState::Submodule {
                commit_changed: true,
                tracked_changes: true,
                untracked_changes: true
            }
        );
        assert!(status.memory_bytes() <= GitStatusLimits::default().max_owned_bytes);
        assert!(!format!("{status:?}").contains("ignored-dir"));
    }
    #[test]
    fn v1_and_v2_rename_copy_keep_destination_then_original_without_unquoting() {
        let v1 = parse_porcelain_v1(
            b"R  to\xff\n\0from\t\"quoted\"\0 C copy\0source\0?? untracked\0!! ignored/\0",
            GitStatusLimits::default(),
            || false,
        )
        .unwrap();
        assert_eq!(v1.entries()[0].path(), b"to\xff\n");
        assert_eq!(
            v1.entries()[0].original_path(),
            Some(&b"from\t\"quoted\""[..])
        );
        assert_eq!(v1.entries()[1].original_path(), Some(&b"source"[..]));
        assert_eq!(v1.entries()[1].similarity(), None);
        let mut input = format!("2 R. N... 100644 100644 100644 {OID} {OID} R100 ").into_bytes();
        input.extend_from_slice(b"to\xff\0from\xfe\0");
        input.extend(
            format!("2 .C N... 100644 100644 100644 {OID} {OID} C75 copy\0source\0").bytes(),
        );
        let v2 = parse2(&input);
        assert_eq!(v2.entries()[0].original_path(), Some(&b"from\xfe"[..]));
        assert_eq!(
            v2.entries()[0].similarity(),
            Some(GitSimilarity {
                kind: GitSimilarityKind::Rename,
                percent: 100
            })
        );
        assert_eq!(
            v2.entries()[1].similarity(),
            Some(GitSimilarity {
                kind: GitSimilarityKind::Copy,
                percent: 75
            })
        );
    }
    #[test]
    fn conflicts_are_not_misrepresented_as_staged_worktree_changes() {
        let cases = [
            ("DD", GitConflict::BothDeleted),
            ("AU", GitConflict::AddedByUs),
            ("UD", GitConflict::DeletedByThem),
            ("UA", GitConflict::AddedByThem),
            ("DU", GitConflict::DeletedByUs),
            ("AA", GitConflict::BothAdded),
            ("UU", GitConflict::BothModified),
        ];
        for (xy, expected) in cases {
            let bytes =
                format!("u {xy} N... 100644 100644 100644 100644 {OID} {OID} {OID} conflict\0");
            assert_eq!(
                parse2(bytes.as_bytes()).entries()[0].state(),
                GitStatusState::Conflict(expected)
            );
            let bytes = format!("{xy} conflict\0");
            assert_eq!(
                parse_porcelain_v1(bytes.as_bytes(), GitStatusLimits::default(), || false)
                    .unwrap()
                    .entries()[0]
                    .state(),
                GitStatusState::Conflict(expected)
            );
        }
    }
    #[test]
    fn malformed_or_ambiguous_protocol_never_returns_a_partial_status() {
        let mut cases = vec![
            b"? no-nul".to_vec(),
            b"\0".to_vec(),
            b"? \0".to_vec(),
            b"? ../escape\0".to_vec(),
            b"? /absolute\0".to_vec(),
            b"? .git/config\0".to_vec(),
            b"3 unknown\0".to_vec(),
            ordinary("..", b"file"),
            ordinary("UU", b"file"),
            ordinary("R.", b"file"),
        ];
        cases.push(
            format!("2 R. N... 100644 100644 100644 {OID} {OID} R101 to\0from\0").into_bytes(),
        );
        cases.push(
            format!("2 R. N... 100644 100644 100644 {OID} {OID} C90 to\0from\0").into_bytes(),
        );
        cases.push(format!("2 R. N... 100644 100644 100644 {OID} {OID} R90 to\0").into_bytes());
        cases.push(format!("1 M. N... 100648 100644 100644 {OID} {OID} path\0").into_bytes());
        cases.push(format!("1 M. N... 100644 100644 100644 {OID} nope path\0").into_bytes());
        cases.push(format!("1 M. SXYZ 100644 100644 100644 {OID} {OID} path\0").into_bytes());
        for bad in cases {
            let mut input = b"? valid\0".to_vec();
            input.extend(bad);
            assert!(parse_porcelain_v2(&input, GitStatusLimits::default(), || false).is_err());
        }
        assert!(parse_porcelain_v1(b"R  to\0", GitStatusLimits::default(), || false).is_err());
        assert!(parse_porcelain_v1(b" M file\0\0", GitStatusLimits::default(), || false).is_err());
    }
    #[test]
    fn explicit_input_record_entry_path_and_owned_quotas_are_distinct() {
        let input = b"? file\0";
        let defaults = GitStatusLimits::default();
        for (limits, expected) in [
            (
                GitStatusLimits {
                    max_input_bytes: input.len() - 1,
                    ..defaults
                },
                GitStatusLimit::InputBytes,
            ),
            (
                GitStatusLimits {
                    max_records: 0,
                    ..defaults
                },
                GitStatusLimit::Records,
            ),
            (
                GitStatusLimits {
                    max_entries: 0,
                    ..defaults
                },
                GitStatusLimit::Entries,
            ),
            (
                GitStatusLimits {
                    max_path_bytes: 3,
                    ..defaults
                },
                GitStatusLimit::PathBytes,
            ),
            (
                GitStatusLimits {
                    max_owned_bytes: size_of::<GitStatus>(),
                    ..defaults
                },
                GitStatusLimit::OwnedBytes,
            ),
        ] {
            assert!(
                matches!(parse_porcelain_v2(input, limits, || false), Err(GitStatusError::Limited(kind)) if kind == expected)
            );
        }
        let exact = GitStatusLimits {
            max_input_bytes: input.len(),
            max_entries: 1,
            max_records: 1,
            max_path_bytes: 4,
            max_owned_bytes: size_of::<GitStatus>() + size_of::<GitStatusEntry>() + 4,
        };
        assert_eq!(
            parse_porcelain_v2(input, exact, || false)
                .unwrap()
                .memory_bytes(),
            exact.max_owned_bytes
        );
        assert!(matches!(
            parse_porcelain_v2(
                b"",
                GitStatusLimits {
                    max_path_bytes: 0,
                    ..defaults
                },
                || false
            ),
            Err(GitStatusError::InvalidLimits)
        ));
        assert!(parse_porcelain_v2(
            b"",
            GitStatusLimits {
                max_input_bytes: 0,
                max_entries: 0,
                max_records: 0,
                ..defaults
            },
            || false
        )
        .unwrap()
        .entries()
        .is_empty());
        assert!(matches!(
            parse_porcelain_v2(
                b"# a\0# b\0",
                GitStatusLimits {
                    max_records: 1,
                    ..defaults
                },
                || false
            ),
            Err(GitStatusError::Limited(GitStatusLimit::Records))
        ));
        assert!(GitStatusReadOptions::default().valid());
        let options = GitStatusReadOptions {
            git: GitReadLimits {
                timeout: Duration::ZERO,
                ..GitReadLimits::default()
            },
            ..GitStatusReadOptions::default()
        };
        assert!(!options.valid());
    }
    #[test]
    fn cancellation_is_checked_inside_long_records_and_expired_deadline_is_typed() {
        assert!(matches!(
            parse_porcelain_v2(b"", GitStatusLimits::default(), || true),
            Err(GitStatusError::Cancelled)
        ));
        let mut input = b"? ".to_vec();
        input.extend(vec![b'x'; 12000]);
        input.push(0);
        let mut checks = 0;
        assert!(matches!(
            parse_porcelain_v2(&input, GitStatusLimits::default(), || {
                checks += 1;
                checks >= 4
            }),
            Err(GitStatusError::Cancelled)
        ));
        let mut cancel = || false;
        assert!(matches!(
            parse(
                b"? file\0",
                GitStatusFormat::PorcelainV2,
                GitStatusLimits::default(),
                &mut cancel,
                Some(Instant::now() - Duration::from_secs(1))
            ),
            Err(GitStatusError::TimedOut)
        ));
    }
}
