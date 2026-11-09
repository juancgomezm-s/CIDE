//! Reviewed recovery of one deleted tracked file from the index.

use crate::{
    Context, Error, Limits, controlled, engine, index_write,
    local_pair::{self, AbsentReason, LocalPair, PairKind, SourceIdentity, SourceOrigin},
};
use gix::bstr::ByteSlice;
use std::{
    io,
    path::Path,
    sync::atomic::{AtomicBool, AtomicU8},
    time::Instant,
};

pub struct RestorePlan {
    pair: LocalPair,
    limits: Limits,
}
impl RestorePlan {
    pub fn pair(&self) -> &LocalPair {
        &self.pair
    }
}

fn missing_target(pair: &LocalPair) -> Result<&jc_fs::MissingToken, Error> {
    if !matches!(
        pair.before.identity(),
        SourceIdentity::Present {
            origin: SourceOrigin::Index,
            mode: 0o100644,
            ..
        }
    ) {
        return Err(Error::Unsupported);
    }
    if let SourceIdentity::Absent {
        origin: SourceOrigin::Worktree,
        reason: AbsentReason::WorktreeEntryMissing,
        worktree: Some(token),
    } = pair.after.identity()
        && let Some(token) = token.missing().filter(|token| token.is_leaf())
    {
        return Ok(token);
    }
    Err(Error::Engine(
        "La restauración requiere un archivo borrado y su carpeta existente".into(),
    ))
}

fn exact_index(
    repo: &gix::Repository,
    pair: &LocalPair,
    ctx: &Context<'_>,
) -> Result<Vec<u8>, Error> {
    let (token, bytes) = local_pair::index_capture(repo, &pair.repository.git_dir, ctx)?;
    if token != pair.index {
        return Err(Error::Stale);
    }
    bytes.ok_or(Error::Stale)
}

fn same_component(left: &[u8], right: &[u8]) -> bool {
    left.eq_ignore_ascii_case(right)
        || (!left.is_ascii() || !right.is_ascii())
            && std::str::from_utf8(left)
                .ok()
                .zip(std::str::from_utf8(right).ok())
                .is_some_and(|(left, right)| left.to_uppercase() == right.to_uppercase())
}

fn validate_index(
    repo: &gix::Repository,
    pair: &LocalPair,
    ctx: &Context<'_>,
) -> Result<(), Error> {
    let bytes = exact_index(repo, pair, ctx)?;
    let state = local_pair::decode_index(repo, Some(&bytes), ctx)?;
    let split = |byte: &u8| *byte == b'/';
    for entry in state.entries() {
        ctx.check()?;
        let path = entry.path(&state).as_bytes();
        if entry.stage_raw() != 0 {
            return Err(Error::Unmerged);
        }
        if path
            .rsplit(split)
            .next()
            .is_some_and(|part| part.eq_ignore_ascii_case(b".gitattributes"))
        {
            return Err(Error::Filter);
        }
        if path != pair.path
            && path
                .split(split)
                .zip(pair.path.split(split))
                .all(|(left, right)| same_component(left, right))
        {
            return Err(Error::Unsupported);
        }
    }
    index_write::reject_conversion(repo, pair)
}

/// Read the reviewed index and prove that the file, rather than an ancestor,
/// is missing. Only ordinary Windows files with a raw checkout are admitted.
pub fn prepare_restore(
    root: &Path,
    path: &[u8],
    limits: Limits,
    mut cancelled: impl FnMut() -> bool + Send,
) -> Result<RestorePlan, Error> {
    if !cfg!(windows) {
        return Err(Error::Unsupported);
    }
    if path.split(|&byte| byte == b'/').any(|part| {
        part.contains(&b'~')
            || part.eq_ignore_ascii_case(b".git")
            || part.eq_ignore_ascii_case(b".gitattributes")
    }) {
        return Err(Error::Unsupported);
    }
    let pair = local_pair::read_local_pair(root, path, PairKind::Worktree, limits, &mut cancelled)?;
    missing_target(&pair)?;
    controlled(limits.timeout, cancelled, |ctx| {
        let (repo, identity) = index_write::open_for_write(root, ctx)?;
        index_write::same_repository(&identity, &pair.repository)?;
        validate_index(&repo, &pair, ctx)?;
        Ok(RestorePlan { pair, limits })
    })
}

fn publish_error(error: io::Error) -> Error {
    match error.kind() {
        io::ErrorKind::AlreadyExists | io::ErrorKind::InvalidData | io::ErrorKind::NotFound => {
            Error::Stale
        }
        _ => Error::Io(error),
    }
}

/// Publish exact indexed bytes as a new file while holding Git's index lock.
/// The platform's create-only rename decides whether the name is still free.
/// A competing writer cannot have an existing destination replaced.
pub fn apply_restore(
    plan: RestorePlan,
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
    let pair = plan.pair;
    let (repo, identity) = index_write::open_for_write(&pair.repository.root, &ctx)?;
    index_write::same_repository(&identity, &pair.repository)?;
    exact_index(&repo, &pair, &ctx)?;
    let _index_lock = gix::lock::File::acquire_to_update_resource(
        identity.git_dir.join("index"),
        gix::lock::acquire::Fail::Immediately,
        None,
        0,
    )
    .map_err(engine)?;
    let pending = jc_fs::stage_missing_file(
        &identity.root,
        missing_target(&pair)?,
        pair.before.bytes(),
        &mut || ctx.check().is_err() || cancelled(),
    );
    ctx.check()?;
    if pending
        .as_ref()
        .is_err_and(|error| error.kind() == io::ErrorKind::Interrupted)
    {
        return Err(Error::Cancelled);
    }
    let pending = pending.map_err(publish_error)?;
    let (fresh, fresh_identity) = index_write::open_for_write(&identity.root, &ctx)?;
    index_write::same_repository(&fresh_identity, &identity)?;
    index_write::reject_conversion(&fresh, &pair)?;
    exact_index(&fresh, &pair, &ctx)?;
    ctx.check()?;
    if cancelled() {
        return Err(Error::Cancelled);
    }
    pending.publish().map_err(publish_error)
}
