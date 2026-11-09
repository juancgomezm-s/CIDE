//! Explicit index transactions. Plans pin the reviewed pair, HEAD,
//! and the serialized index. Applying a plan never writes worktree files.
use super::diff_model::{DiffLine, DiffSide, DiffStatus, OwnedDiffModel};
use super::{Context, Error, Identity, Limits, controlled, engine, local_pair, open, revision};
use gix::bstr::ByteSlice;
use local_pair::{LocalPair, PairKind, Source, SourceIdentity};
use std::{
    fs, io,
    path::Path,
    sync::atomic::{AtomicBool, AtomicU8},
    time::Instant,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexAction {
    Stage,
    Unstage,
}

impl IndexAction {
    fn pair_kind(self) -> PairKind {
        match self {
            Self::Stage => PairKind::Worktree,
            Self::Unstage => PairKind::Prepared,
        }
    }
}

type HeadMarker = (Option<Vec<u8>>, Option<gix::ObjectId>);

pub struct IndexPlan {
    action: IndexAction,
    pair: LocalPair,
    head: HeadMarker,
    mode: Option<u32>,
    replacement: Option<Vec<u8>>,
    limits: Limits,
}

impl IndexPlan {
    pub fn action(&self) -> IndexAction {
        self.action
    }

    pub fn pair(&self) -> &LocalPair {
        &self.pair
    }

    /// Limit a plan to the selected zero-based hunks of its exact comparison.
    /// Only an existing regular file with unchanged mode supports partial edits.
    pub fn with_hunks(mut self, diff: &OwnedDiffModel, selected: &[usize]) -> Result<Self, Error> {
        if selected.is_empty()
            || selected.len() > 4096
            || !selected.windows(2).all(|pair| pair[0] < pair[1])
            || self.mode.is_none()
            || self.mode != source_mode(&self.pair.before)
            || self.mode != source_mode(&self.pair.after)
        {
            return Err(Error::Unsupported);
        }
        if diff.status() != DiffStatus::Exact {
            return Err(Error::Limited);
        }
        let left = diff.source_bytes(DiffSide::Left);
        let right = diff.source_bytes(DiffSide::Right);
        if left != self.pair.before.bytes() || right != self.pair.after.bytes() {
            return Err(Error::Stale);
        }
        if left.contains(&0) || right.contains(&0) {
            return Err(Error::Unsupported);
        }
        let boundary = |side, line| {
            if diff.line_count(side) == Some(line) {
                Ok(diff.source_bytes(side).len())
            } else {
                diff.line_byte_range(side, DiffLine(line))
                    .map(|range| range.start)
                    .ok_or(Error::Unsupported)
            }
        };
        let (base_side, change_side) = match self.action {
            IndexAction::Stage => (DiffSide::Left, DiffSide::Right),
            IndexAction::Unstage => (DiffSide::Right, DiffSide::Left),
        };
        let base = diff.source_bytes(base_side);
        let change = diff.source_bytes(change_side);
        let mut next = selected.iter().copied().peekable();
        let mut edits = Vec::with_capacity(selected.len());
        let mut len = base.len();
        for (index, span) in diff.hunks().enumerate() {
            if next.peek() != Some(&index) {
                continue;
            }
            next.next();
            let (original, replacement) = match self.action {
                IndexAction::Stage => (&span.left, &span.right),
                IndexAction::Unstage => (&span.right, &span.left),
            };
            let from = boundary(base_side, original.start.0)?..boundary(base_side, original.end.0)?;
            let to = boundary(change_side, replacement.start.0)?
                ..boundary(change_side, replacement.end.0)?;
            len = len
                .checked_sub(from.len())
                .and_then(|n| n.checked_add(to.len()))
                .ok_or(Error::Limited)?;
            edits.push((from, to));
        }
        if next.peek().is_some() {
            return Err(Error::Unsupported);
        }
        if len > self.limits.max_blob.min(2 * 1024 * 1024) {
            return Err(Error::Limited);
        }
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(len).map_err(|_| Error::Limited)?;
        let mut cursor = 0;
        for (from, to) in edits {
            bytes.extend_from_slice(&base[cursor..from.start]);
            bytes.extend_from_slice(&change[to]);
            cursor = from.end;
        }
        bytes.extend_from_slice(&base[cursor..]);
        self.replacement = Some(bytes);
        Ok(self)
    }

    /// Exact resulting bytes for a selected-hunk plan.
    pub fn partial_index_bytes(&self) -> Option<&[u8]> {
        self.replacement.as_deref()
    }
}

fn head_marker(repo: &gix::Repository) -> Result<HeadMarker, Error> {
    let head = repo.head().map_err(engine)?;
    Ok((
        head.referent_name().map(|name| name.as_bstr().to_vec()),
        head.id().map(|id| id.detach()),
    ))
}

pub(super) fn exists(path: &Path) -> Result<bool, Error> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn ordinary(path: &Path, directory: bool) -> Result<(), Error> {
    let meta = fs::symlink_metadata(path)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return Err(Error::Unsupported);
        }
    }
    if meta.is_symlink()
        || if directory {
            !meta.is_dir()
        } else {
            !meta.is_file()
        }
    {
        return Err(Error::Unsupported);
    }
    Ok(())
}

pub(super) fn open_for_write(
    root: &Path,
    ctx: &Context<'_>,
) -> Result<(gix::Repository, Identity), Error> {
    if !root.is_absolute() {
        return Err(Error::InvalidRoot);
    }
    let git = root.canonicalize()?.join(".git");
    ordinary(&git, true)?;
    for control in ["config", "HEAD", "index", "packed-refs"] {
        let path = git.join(control);
        if exists(&path)? {
            ordinary(&path, false)?;
        }
    }
    ordinary(&git.join("objects"), true)?;
    for control in [
        "commondir",
        "objects/info/alternates",
        "MERGE_HEAD",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "rebase-merge",
        "rebase-apply",
        "sequencer",
    ] {
        if exists(&git.join(control))? {
            return Err(Error::Unsupported);
        }
    }
    let (repo, identity) = open(root, ctx)?;
    if identity.git_dir != git || repo.common_dir().canonicalize()? != git {
        return Err(Error::Unsupported);
    }
    Ok((repo, identity))
}

fn source_mode(source: &Source) -> Option<u32> {
    match source.identity() {
        SourceIdentity::Present { mode, .. } => Some(*mode),
        SourceIdentity::Absent { .. } => None,
    }
}

pub(super) fn reject_conversion(repo: &gix::Repository, pair: &LocalPair) -> Result<(), Error> {
    let config = repo.config_snapshot();
    if config.string("core.attributesFile").is_some()
        || config.string("core.eol").is_some()
        || config.string("core.autocrlf").is_some_and(|value| {
            !matches!(value.as_bstr().as_bytes(), b"false" | b"no" | b"off" | b"0")
        })
        || exists(&pair.repository.git_dir.join("info/attributes"))?
    {
        return Err(Error::Filter);
    }
    let relative = gix::path::from_bstr(pair.path.as_bstr()).map_err(engine)?;
    let target = pair.repository.root.join(relative);
    let mut directory = target.parent();
    while let Some(parent) = directory.filter(|p| p.starts_with(&pair.repository.root)) {
        if exists(&parent.join(".gitattributes"))? {
            return Err(Error::Filter);
        }
        directory = parent.parent();
    }
    Ok(())
}

fn stage_mode(repo: &gix::Repository, pair: &LocalPair) -> Result<Option<u32>, Error> {
    let Some(mode) = source_mode(&pair.after) else {
        return Ok(None);
    };
    reject_conversion(repo, pair)?;
    if repo
        .config_snapshot()
        .boolean("core.filemode")
        .unwrap_or(cfg!(unix))
    {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let relative = gix::path::from_bstr(pair.path.as_bstr()).map_err(engine)?;
            let target = pair.repository.root.join(relative);
            let meta = fs::symlink_metadata(&target)?;
            if !meta.is_file() || meta.is_symlink() {
                return Err(Error::Stale);
            }
            return Ok(Some(if meta.permissions().mode() & 0o100 == 0 {
                0o100644
            } else {
                0o100755
            }));
        }
        #[cfg(not(unix))]
        return Err(Error::Unsupported);
    }
    Ok(Some(mode))
}

/// Capture an exact disk/index or HEAD/index comparison for explicit review.
/// Stage accepts one named regular path, including a deletion. The host is
/// responsible for offering only eligible status rows and guarding dirty buffers.
pub fn prepare_index_change(
    root: &Path,
    path: &[u8],
    action: IndexAction,
    limits: Limits,
    mut cancelled: impl FnMut() -> bool + Send,
) -> Result<IndexPlan, Error> {
    revision::path_valid(path, limits.max_path)?;
    for component in path.split(|byte| *byte == b'/') {
        gix::validate::path::component(component.as_bstr(), None, Default::default())
            .map_err(|_| Error::Unsupported)?;
    }
    let pair = local_pair::read_local_pair(root, path, action.pair_kind(), limits, &mut cancelled)?;
    controlled(limits.timeout, cancelled, |ctx| {
        let (repo, identity) = open_for_write(root, ctx)?;
        same_repository(&identity, &pair.repository)?;
        let mode = match action {
            IndexAction::Stage => stage_mode(&repo, &pair)?,
            IndexAction::Unstage => source_mode(&pair.before),
        };
        let unchanged = match action {
            IndexAction::Stage => mode == source_mode(&pair.before),
            IndexAction::Unstage => mode == source_mode(&pair.after),
        } && pair.before.bytes() == pair.after.bytes();
        if unchanged {
            return Err(Error::Engine(
                "El archivo no tiene cambios para esta operación".into(),
            ));
        }
        Ok(IndexPlan {
            action,
            head: head_marker(&repo)?,
            mode,
            replacement: None,
            pair,
            limits,
        })
    })
}

pub(super) fn same_repository(a: &Identity, b: &Identity) -> Result<(), Error> {
    if a.root != b.root || a.git_dir != b.git_dir {
        return Err(Error::Stale);
    }
    Ok(())
}

/// Publish a reviewed index change with Git's `index.lock` protocol. A failed
/// or cancelled publication may leave an unreachable immutable blob in the ODB.
/// Success remains success if cancellation arrives after the atomic replacement.
pub fn apply_index_change(
    plan: IndexPlan,
    mut cancelled: impl FnMut() -> bool + Send,
) -> Result<(), Error> {
    if cancelled() {
        return Err(Error::Cancelled);
    }
    let stop = AtomicBool::new(false);
    let reason = AtomicU8::new(0);
    let ctx = Context {
        stop: &stop,
        reason: &reason,
        deadline: Instant::now() + plan.limits.timeout,
    };
    let pair = &plan.pair;
    let (repo, identity) = open_for_write(&pair.repository.root, &ctx)?;
    same_repository(&identity, &pair.repository)?;
    if local_pair::index_capture(&repo, &identity.git_dir, &ctx)?.0 != pair.index {
        return Err(Error::Stale);
    }
    let index_path = identity.git_dir.join("index");
    let mut lock = gix::lock::File::acquire_to_update_resource(
        &index_path,
        gix::lock::acquire::Fail::Immediately,
        None,
        0,
    )
    .map_err(engine)?;
    let current = local_pair::read_local_pair(
        &identity.root,
        &pair.path,
        pair.kind,
        plan.limits,
        &mut cancelled,
    )?;
    same_repository(&current.repository, &identity)?;
    if !current.index.matches_under_lock(&pair.index)
        || current.head != pair.head
        || current.before.identity() != pair.before.identity()
        || current.after.identity() != pair.after.identity()
    {
        return Err(Error::Stale);
    }
    drop(current);
    let (fresh, fresh_id) = open_for_write(&identity.root, &ctx)?;
    same_repository(&fresh_id, &identity)?;
    if head_marker(&fresh)? != plan.head
        || (plan.action == IndexAction::Stage && stage_mode(&fresh, pair)? != plan.mode)
    {
        return Err(Error::Stale);
    }
    let (index, bytes) = local_pair::index_capture(&repo, &identity.git_dir, &ctx)?;
    if !index.matches_under_lock(&pair.index) {
        return Err(Error::Stale);
    }
    let mut state = local_pair::decode_index(&repo, bytes.as_deref(), &ctx)?;
    drop(bytes);
    for entry in state.entries() {
        if entry.stage_raw() != 0 || entry.mode.is_submodule() {
            return Err(Error::Unsupported);
        }
        if plan.action == IndexAction::Stage
            && plan.mode.is_some()
            && entry
                .path(&state)
                .as_bytes()
                .rsplit(|byte| *byte == b'/')
                .next()
                == Some(b".gitattributes")
        {
            return Err(Error::Filter);
        }
    }
    if state.resolve_undo().is_some() || state.fs_monitor().is_some() || state.untracked().is_some()
    {
        return Err(Error::Unsupported);
    }
    state.remove_entries(|_, path, _| path.as_bytes() == pair.path);
    if let Some(mode) = plan.mode {
        if state.entries().len() == super::INDEX_ENTRIES
            || state.path_backing().len().saturating_add(pair.path.len()) > super::TREE_PATH_BYTES
        {
            return Err(Error::Limited);
        }
        let reviewed = plan
            .replacement
            .as_deref()
            .or_else(|| (plan.action == IndexAction::Stage).then(|| pair.after.bytes()));
        let id = match reviewed {
            Some(bytes) => {
                let id = gix::objs::compute_hash(repo.object_hash(), gix::objs::Kind::Blob, bytes)
                    .map_err(engine)?;
                let hex = id.to_string();
                let fanout = identity.git_dir.join("objects").join(&hex[..2]);
                if exists(&fanout)? {
                    ordinary(&fanout, true)?;
                    let object = fanout.join(&hex[2..]);
                    if exists(&object)? {
                        ordinary(&object, false)?;
                    }
                }
                let stored = repo.write_blob(bytes).map_err(engine)?.detach();
                if stored != id
                    || revision::verified(&repo, id, plan.limits.max_blob, &ctx)?.kind
                        != gix::objs::Kind::Blob
                {
                    return Err(Error::Engine("Git no confirmó el objeto preparado".into()));
                }
                stored
            }
            None => gix::ObjectId::from_hex(
                pair.before
                    .identity()
                    .oid()
                    .ok_or(Error::Missing)?
                    .as_bytes(),
            )
            .map_err(engine)?,
        };
        state.dangerously_push_entry(
            Default::default(),
            id,
            gix::index::entry::Flags::empty(),
            gix::index::entry::Mode::from_bits(mode).ok_or(Error::Unsupported)?,
            pair.path.as_bstr(),
        );
        state.sort_entries();
    }
    state.remove_tree();
    state.verify_entries().map_err(engine)?;
    let file = gix::index::File::from_state(state, index_path);
    file.write_to(&mut lock, Default::default())
        .map_err(engine)?;
    lock.with_mut(|f| {
        if f.metadata()?.len() > super::INDEX_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::FileTooLarge,
                "Índice Git supera la cuota",
            ));
        }
        f.sync_all()
    })?;
    let (fresh, fresh_id) = open_for_write(&identity.root, &ctx)?;
    same_repository(&fresh_id, &identity)?;
    if head_marker(&fresh)? != plan.head
        || !local_pair::index_capture(&fresh, &identity.git_dir, &ctx)?
            .0
            .matches_under_lock(&pair.index)
    {
        return Err(Error::Stale);
    }
    ctx.check()?;
    if cancelled() {
        return Err(Error::Cancelled);
    }
    lock.commit().map_err(engine)?;
    Ok(())
}
