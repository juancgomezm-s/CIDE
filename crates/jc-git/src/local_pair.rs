//! Pinned, byte-exact HEAD/index and index/worktree comparisons. All filesystem
//! work happens inside the caller's worker, never while drawing or accepting UI.
use super::{Context, Error, Identity, Limits, controlled, engine, open, revision};
use gix::bstr::ByteSlice;
use std::{fmt, path::Path};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairKind {
    Prepared,
    Worktree,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceOrigin {
    Head,
    Index,
    Worktree,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AbsentReason {
    UnbornHead,
    TreeEntryMissing,
    IndexEntryMissing,
    WorktreeEntryMissing,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeadState {
    pub referent: Option<Vec<u8>>,
    pub commit: Option<String>,
    pub tree: Option<String>,
}
impl HeadState {
    pub fn memory_bytes(&self) -> usize {
        self.referent.as_ref().map_or(0, Vec::capacity)
            + self.commit.as_ref().map_or(0, String::capacity)
            + self.tree.as_ref().map_or(0, String::capacity)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum CaptureIdentity {
    Present(jc_fs::FileToken),
    Absent(jc_fs::MissingToken),
}
impl CaptureIdentity {
    fn memory_bytes(&self) -> usize {
        match self {
            Self::Present(token) => token.memory_bytes(),
            Self::Absent(token) => token.memory_bytes(),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndexChecksum {
    Verified,
    Skipped,
    Absent,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexToken {
    capture: CaptureIdentity,
    sha256: Option<[u8; 32]>,
    checksum: IndexChecksum,
}
impl IndexToken {
    pub(super) fn matches_under_lock(&self, other: &Self) -> bool {
        match (&self.capture, &other.capture) {
            (CaptureIdentity::Absent(before), CaptureIdentity::Absent(after)) => {
                before.same_location_as(after)
            }
            _ => self == other,
        }
    }

    pub fn sha256(&self) -> Option<&[u8; 32]> {
        self.sha256.as_ref()
    }
    pub fn is_absent(&self) -> bool {
        matches!(self.capture, CaptureIdentity::Absent(_))
    }
    pub fn checksum(&self) -> IndexChecksum {
        self.checksum
    }
    pub fn memory_bytes(&self) -> usize {
        self.capture.memory_bytes()
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorktreeToken {
    capture: CaptureIdentity,
    sha256: Option<[u8; 32]>,
}
impl WorktreeToken {
    pub(super) fn present(&self) -> Option<&jc_fs::FileToken> {
        match &self.capture {
            CaptureIdentity::Present(token) => Some(token),
            CaptureIdentity::Absent(_) => None,
        }
    }

    pub(super) fn missing(&self) -> Option<&jc_fs::MissingToken> {
        match &self.capture {
            CaptureIdentity::Absent(token) => Some(token),
            CaptureIdentity::Present(_) => None,
        }
    }

    pub fn memory_bytes(&self) -> usize {
        self.capture.memory_bytes()
    }
    pub fn sha256(&self) -> Option<&[u8; 32]> {
        self.sha256.as_ref()
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceIdentity {
    Present {
        origin: SourceOrigin,
        oid: Option<String>,
        mode: u32,
        worktree: Option<WorktreeToken>,
    },
    Absent {
        origin: SourceOrigin,
        reason: AbsentReason,
        worktree: Option<WorktreeToken>,
    },
}
impl SourceIdentity {
    pub fn oid(&self) -> Option<&str> {
        match self {
            Self::Present { oid, .. } => oid.as_deref(),
            Self::Absent { .. } => None,
        }
    }
    pub fn origin(&self) -> SourceOrigin {
        match self {
            Self::Present { origin, .. } | Self::Absent { origin, .. } => *origin,
        }
    }
    pub fn is_absent(&self) -> bool {
        matches!(self, Self::Absent { .. })
    }
    pub fn memory_bytes(&self) -> usize {
        match self {
            Self::Present { oid, worktree, .. } => {
                oid.as_ref().map_or(0, String::capacity)
                    + worktree.as_ref().map_or(0, WorktreeToken::memory_bytes)
            }
            Self::Absent { worktree, .. } => {
                worktree.as_ref().map_or(0, WorktreeToken::memory_bytes)
            }
        }
    }
}
pub enum Source {
    Present {
        identity: SourceIdentity,
        bytes: Vec<u8>,
    },
    Absent {
        identity: SourceIdentity,
    },
}
impl Source {
    pub fn identity(&self) -> &SourceIdentity {
        match self {
            Self::Present { identity, .. } | Self::Absent { identity } => identity,
        }
    }
    pub fn bytes(&self) -> &[u8] {
        match self {
            Self::Present { bytes, .. } => bytes,
            Self::Absent { .. } => &[],
        }
    }
    pub fn into_parts(self) -> (SourceIdentity, Vec<u8>) {
        match self {
            Self::Present { identity, bytes } => (identity, bytes),
            Self::Absent { identity } => (identity, Vec::new()),
        }
    }
    fn memory_bytes(&self) -> usize {
        self.identity().memory_bytes()
            + match self {
                Self::Present { bytes, .. } => bytes.capacity(),
                Self::Absent { .. } => 0,
            }
    }
}
impl fmt::Debug for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Source")
            .field("identity", self.identity())
            .field("bytes", &self.bytes().len())
            .finish()
    }
}
pub struct LocalPair {
    pub repository: Identity,
    pub kind: PairKind,
    pub path: Vec<u8>,
    pub index: IndexToken,
    pub head: Option<HeadState>,
    pub before: Source,
    pub after: Source,
}
impl LocalPair {
    pub fn memory_bytes(&self) -> usize {
        self.repository.root.capacity()
            + self.repository.git_dir.capacity()
            + self.path.capacity()
            + self.index.memory_bytes()
            + self.head.as_ref().map_or(0, HeadState::memory_bytes)
            + self.before.memory_bytes()
            + self.after.memory_bytes()
    }
}
impl fmt::Debug for LocalPair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalPair")
            .field("root", &self.repository.root)
            .field("kind", &self.kind)
            .field("path", &self.path)
            .field("index", &self.index)
            .field("head", &self.head)
            .field("before", &self.before)
            .field("after", &self.after)
            .finish()
    }
}
fn absent(origin: SourceOrigin, reason: AbsentReason) -> Source {
    Source::Absent {
        identity: SourceIdentity::Absent {
            origin,
            reason,
            worktree: None,
        },
    }
}
fn digest(bytes: &[u8], kind: gix::hash::Kind, ctx: &Context<'_>) -> Result<gix::ObjectId, Error> {
    let mut hash = gix::hash::hasher(kind);
    for chunk in bytes.chunks(64 * 1024) {
        ctx.check()?;
        hash.update(chunk);
    }
    ctx.check()?;
    hash.try_finalize().map_err(engine)
}
fn sha256(bytes: &[u8], ctx: &Context<'_>) -> Result<[u8; 32], Error> {
    digest(bytes, gix::hash::Kind::Sha256, ctx)?
        .as_slice()
        .try_into()
        .map_err(|_| Error::Unsupported)
}
fn capture(
    root: &Path,
    path: &Path,
    cap: usize,
    ctx: &Context<'_>,
) -> Result<jc_fs::Capture, Error> {
    ctx.check()?;
    let result = jc_fs::capture_in_root(root, path, cap, &mut || ctx.check().is_err());
    ctx.check()?;
    result.map_err(|error| match error.kind() {
        std::io::ErrorKind::FileTooLarge | std::io::ErrorKind::OutOfMemory => Error::Limited,
        _ => Error::Io(error),
    })
}
pub(super) fn index_capture(
    repo: &gix::Repository,
    root: &Path,
    ctx: &Context<'_>,
) -> Result<(IndexToken, Option<Vec<u8>>), Error> {
    let path = root.join("index");
    match capture(root, &path, super::INDEX_BYTES as usize, ctx)? {
        jc_fs::Capture::Absent { token } => Ok((
            IndexToken {
                capture: CaptureIdentity::Absent(token),
                sha256: None,
                checksum: IndexChecksum::Absent,
            },
            None,
        )),
        jc_fs::Capture::Present { bytes, token, .. } => {
            let len = repo.object_hash().len_in_bytes();
            if bytes.len() < 12 + len {
                return Err(Error::Engine("El índice Git está truncado".into()));
            }
            let expected = gix::ObjectId::from_bytes_or_panic(&bytes[bytes.len() - len..]);
            let checksum = if expected.is_null() {
                IndexChecksum::Skipped
            } else {
                if digest(&bytes[..bytes.len() - len], repo.object_hash(), ctx)? != expected {
                    return Err(Error::Engine(
                        "El índice Git no coincide con su suma de comprobación".into(),
                    ));
                }
                IndexChecksum::Verified
            };
            let sha256 = Some(sha256(&bytes, ctx)?);
            Ok((
                IndexToken {
                    capture: CaptureIdentity::Present(token),
                    sha256,
                    checksum,
                },
                Some(bytes),
            ))
        }
    }
}
pub(super) fn decode_index(
    repo: &gix::Repository,
    bytes: Option<&[u8]>,
    ctx: &Context<'_>,
) -> Result<gix::index::State, Error> {
    let Some(bytes) = bytes else {
        return Ok(gix::index::State::new(repo.object_hash()));
    };
    // Admission only: gix remains the parser and validates signature/version.
    let count =
        u32::from_be_bytes(bytes[8..12].try_into().map_err(|_| Error::Unsupported)?) as usize;
    if count > super::INDEX_ENTRIES {
        return Err(Error::Limited);
    }
    let timestamp = gix::index::State::new(repo.object_hash()).timestamp();
    let (state, _) = gix::index::State::from_bytes(
        bytes,
        timestamp,
        repo.object_hash(),
        gix::index::decode::Options {
            thread_limit: Some(1),
            alloc_limit_bytes: Some(super::INDEX_BYTES as usize),
            ..Default::default()
        },
    )
    .map_err(engine)?;
    ctx.check()?;
    state.verify_entries().map_err(engine)?;
    if state.is_sparse() || state.link().is_some() {
        return Err(Error::Unsupported);
    }
    if state.entries().len() > super::INDEX_ENTRIES
        || state.path_backing().len() > super::TREE_PATH_BYTES
    {
        return Err(Error::Limited);
    }
    Ok(state)
}

fn selected_index(
    repo: &gix::Repository,
    bytes: Option<&[u8]>,
    path: &[u8],
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<Option<revision::Entry>, Error> {
    let state = decode_index(repo, bytes, ctx)?;
    let mut selected = None;
    for entry in state.entries() {
        ctx.check()?;
        let entry_path = entry.path(&state).as_bytes();
        revision::path_valid(entry_path, limits.max_path)?;
        if entry.mode.is_submodule() {
            return Err(Error::Unsupported);
        }
        // A file cannot be the ancestor of another selected file. Do not turn
        // an inconsistent index's non-tree ancestor into an invented absence.
        if path
            .strip_prefix(entry_path)
            .is_some_and(|tail| tail.starts_with(b"/"))
            || entry_path
                .strip_prefix(path)
                .is_some_and(|tail| tail.starts_with(b"/"))
        {
            return Err(Error::Unsupported);
        }
        if entry_path == path {
            if entry.stage_raw() != 0 {
                return Err(Error::Unmerged);
            }
            if entry.flags.intersects(
                gix::index::entry::Flags::INTENT_TO_ADD | gix::index::entry::Flags::SKIP_WORKTREE,
            ) {
                return Err(Error::Unsupported);
            }
            let mode = entry.mode.bits();
            if !matches!(mode, 0o100644 | 0o100755) {
                return Err(Error::Unsupported);
            }
            if selected
                .replace(revision::Entry { id: entry.id, mode })
                .is_some()
            {
                return Err(Error::Unsupported);
            }
        }
    }
    Ok(selected)
}
fn head(
    repo: &gix::Repository,
    limits: Limits,
    budget: &mut revision::Budget,
    ctx: &Context<'_>,
) -> Result<(HeadState, Option<gix::ObjectId>), Error> {
    ctx.check()?;
    let head = repo.head().map_err(engine)?;
    let referent = head.referent_name().map(|name| name.as_bstr().to_vec());
    if referent
        .as_ref()
        .is_some_and(|name| name.len() > limits.max_path)
    {
        return Err(Error::Limited);
    }
    if head.is_unborn() {
        return Ok((
            HeadState {
                referent,
                commit: None,
                tree: None,
            },
            None,
        ));
    }
    let id = head.id().ok_or(Error::Missing)?.detach();
    let commit = revision::commit(repo, id, limits, budget, ctx)?;
    Ok((
        HeadState {
            referent,
            commit: Some(id.to_string()),
            tree: Some(commit.tree.to_string()),
        },
        Some(commit.tree),
    ))
}
fn source(
    repo: &gix::Repository,
    entry: Option<revision::Entry>,
    origin: (SourceOrigin, AbsentReason),
    limits: Limits,
    budget: (&mut revision::Budget, &mut usize),
    ctx: &Context<'_>,
) -> Result<Source, Error> {
    let (origin, reason) = origin;
    let (budget, source_bytes) = budget;
    match revision::blob(repo, entry, limits, budget, source_bytes, ctx)? {
        None => Ok(absent(origin, reason)),
        Some(blob) => Ok(Source::Present {
            identity: SourceIdentity::Present {
                origin,
                oid: Some(blob.oid),
                mode: blob.mode,
                worktree: None,
            },
            bytes: blob.bytes,
        }),
    }
}
fn disk(
    root: &Path,
    path: &[u8],
    mode: u32,
    limit: usize,
    ctx: &Context<'_>,
) -> Result<Source, Error> {
    for part in path.split(|byte| *byte == b'/') {
        gix::validate::path::component(part.as_bstr(), None, Default::default())
            .map_err(|_| Error::Unsupported)?;
    }
    let path = gix::path::from_bstr(path.as_bstr()).map_err(engine)?;
    match capture(root, &root.join(path), limit, ctx)? {
        jc_fs::Capture::Absent { token } => Ok(Source::Absent {
            identity: SourceIdentity::Absent {
                origin: SourceOrigin::Worktree,
                reason: AbsentReason::WorktreeEntryMissing,
                worktree: Some(WorktreeToken {
                    capture: CaptureIdentity::Absent(token),
                    sha256: None,
                }),
            },
        }),
        jc_fs::Capture::Present { bytes, token, .. } => {
            let hash = sha256(&bytes, ctx)?;
            Ok(Source::Present {
                identity: SourceIdentity::Present {
                    origin: SourceOrigin::Worktree,
                    oid: None,
                    mode,
                    worktree: Some(WorktreeToken {
                        capture: CaptureIdentity::Present(token),
                        sha256: Some(hash),
                    }),
                },
                bytes,
            })
        }
    }
}

/// Captures a pair and revalidates its mutable inputs before returning. This is
/// an observation of pinned bytes, not an everlasting filesystem transaction.
/// Worktree bytes are raw: no autocrlf, attributes, smudge or external filter.
pub fn read_local_pair(
    root: &Path,
    path: &[u8],
    kind: PairKind,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<LocalPair, Error> {
    limits.admit()?;
    revision::path_valid(path, limits.max_path)?;
    controlled(limits.timeout, cancelled, |ctx| {
        let (repo, repository) = open(root, ctx)?;
        let (index, index_bytes) = index_capture(&repo, &repository.git_dir, ctx)?;
        let selected = selected_index(&repo, index_bytes.as_deref(), path, limits, ctx)?;
        // The exact vector hashed above is the vector parsed above. Drop it and
        // parsed metadata before capturing sources or a final revalidation copy.
        drop(index_bytes);
        let mut budget = revision::Budget::new(limits);
        let mut source_bytes = 0;
        let (head_state, before, after) = match kind {
            PairKind::Prepared => {
                let (head_state, tree) = head(&repo, limits, &mut budget, ctx)?;
                let entry = if let Some(tree) = tree {
                    revision::tree_entry(&repo, tree, path, limits, &mut budget, ctx)?
                } else {
                    None
                };
                let reason = if tree.is_some() {
                    AbsentReason::TreeEntryMissing
                } else {
                    AbsentReason::UnbornHead
                };
                let before = source(
                    &repo,
                    entry,
                    (SourceOrigin::Head, reason),
                    limits,
                    (&mut budget, &mut source_bytes),
                    ctx,
                )?;
                let after = source(
                    &repo,
                    selected,
                    (SourceOrigin::Index, AbsentReason::IndexEntryMissing),
                    limits,
                    (&mut budget, &mut source_bytes),
                    ctx,
                )?;
                (Some(head_state), before, after)
            }
            PairKind::Worktree => {
                let before = source(
                    &repo,
                    selected,
                    (SourceOrigin::Index, AbsentReason::IndexEntryMissing),
                    limits,
                    (&mut budget, &mut source_bytes),
                    ctx,
                )?;
                let cap = limits
                    .max_blob
                    .min(2 * 1024 * 1024)
                    .min((4 * 1024 * 1024usize).saturating_sub(source_bytes));
                let after = disk(
                    &repository.root,
                    path,
                    selected.map_or(0o100644, |entry| entry.mode),
                    cap,
                    ctx,
                )?;
                (None, before, after)
            }
        };
        // Preserve both state and content identities: hashing a newly reopened
        // index must never be attached to entries parsed from the older bytes.
        let (current_index, _) = index_capture(&repo, &repository.git_dir, ctx)?;
        if current_index != index {
            return Err(Error::Stale);
        }
        if let Some(expected) = &head_state {
            let (fresh_repo, fresh_identity) = open(&repository.root, ctx)?;
            if fresh_identity.root != repository.root
                || fresh_identity.git_dir != repository.git_dir
            {
                return Err(Error::Stale);
            }
            let (current, _) = head(&fresh_repo, limits, &mut revision::Budget::new(limits), ctx)?;
            if &current != expected {
                return Err(Error::Stale);
            }
        }
        if kind == PairKind::Worktree {
            let current = disk(
                &repository.root,
                path,
                selected.map_or(0o100644, |entry| entry.mode),
                limits.max_blob.min(2 * 1024 * 1024),
                ctx,
            )?;
            if current.identity() != after.identity() {
                return Err(Error::Stale);
            }
        }
        ctx.check()?;
        let result = LocalPair {
            repository,
            kind,
            path: path.to_vec(),
            index,
            head: head_state,
            before,
            after,
        };
        if result
            .before
            .bytes()
            .len()
            .saturating_add(result.after.bytes().len())
            > 4 * 1024 * 1024
            || result.memory_bytes() > limits.max_bytes
        {
            return Err(Error::Limited);
        }
        Ok(result)
    })
}
