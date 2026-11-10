//! Bounded linked-worktree inventory and compare-and-swap locking.
//!
//! This module deliberately does not add/remove worktrees or change references.
//! Lock state is a Git administrative file, updated create-only or removed only
//! after its reviewed file identity and bytes still match.

use crate::{Context, Error, Identity, Limits, controlled, open};
use gix::bstr::ByteSlice;
use jc_fs::{Capture, FileToken, MissingToken};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

const LOCK_BYTES: usize = 1024;
const REASON_BYTES: usize = 512;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Worktree {
    /// `None` identifies the main worktree; linked worktrees have a stable Git id.
    pub id: Option<String>,
    pub path: PathBuf,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub locked: bool,
    pub lock_reason: Option<String>,
    pub prunable: bool,
}

#[derive(Clone)]
pub struct Inventory {
    pub repository: Identity,
    pub worktrees: Vec<Worktree>,
}

/// Review for setting a lock on a currently unlocked linked worktree.
/// The destination's absence and worktree registration are rechecked at apply.
pub struct LockReview {
    identity: Identity,
    common_dir: PathBuf,
    id: String,
    admin_dir: PathBuf,
    target: PathBuf,
    missing: MissingToken,
    bytes: Vec<u8>,
}

/// Review for unlocking a linked worktree. Both file identity and exact bytes
/// are checked again before deletion.
pub struct UnlockReview {
    identity: Identity,
    common_dir: PathBuf,
    id: String,
    admin_dir: PathBuf,
    target: PathBuf,
    token: FileToken,
    bytes: Vec<u8>,
}

impl LockReview {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn reason(&self) -> &str {
        self.bytes
            .strip_suffix(b"\n")
            .and_then(|v| std::str::from_utf8(v).ok())
            .unwrap_or("")
    }
}

impl UnlockReview {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn reason(&self) -> &str {
        let text = std::str::from_utf8(&self.bytes).unwrap_or("");
        text.trim_end_matches(['\r', '\n'])
    }
}

fn common_dir(repo: &gix::Repository) -> Result<PathBuf, Error> {
    repo.current_dir()
        .join(repo.common_dir())
        .canonicalize()
        .map_err(Error::Io)
}

fn bounded_worktree_dir(common: &Path, max: usize, ctx: &Context<'_>) -> Result<(), Error> {
    let path = common.join("worktrees");
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(Error::Unsupported);
    }
    let mut count = 0usize;
    for entry in std::fs::read_dir(path)? {
        ctx.check()?;
        let entry = entry?;
        let metadata = std::fs::symlink_metadata(entry.path())?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(Error::Unsupported);
        }
        count = count.checked_add(1).ok_or(Error::Limited)?;
        if count > max {
            return Err(Error::Limited);
        }
    }
    Ok(())
}

fn lock_capture(common: &Path, target: &Path, ctx: &Context<'_>) -> Result<Capture, Error> {
    jc_fs::capture_in_root(common, target, LOCK_BYTES, &mut || ctx.check().is_err())
        .map_err(Error::Io)
}

fn lock_bytes(reason: &str) -> Result<Vec<u8>, Error> {
    if reason.len() > REASON_BYTES
        || reason
            .bytes()
            .any(|b| b == 0 || b == b'\r' || b == b'\n' || b < 0x20 && b != b'\t')
    {
        return Err(Error::Unsupported);
    }
    let mut bytes = reason.as_bytes().to_vec();
    bytes.push(b'\n');
    Ok(bytes)
}

fn validate_id(id: &str) -> Result<(), Error> {
    if id.is_empty()
        || id.len() > 128
        || id == "."
        || id == ".."
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        return Err(Error::Unsupported);
    }
    Ok(())
}

fn verify_registration(
    repo: &gix::Repository,
    expected_common: &Path,
    id: &str,
    expected_admin: &Path,
) -> Result<(), Error> {
    if common_dir(repo)? != expected_common {
        return Err(Error::Stale);
    }
    let proxy = repo
        .worktree_proxy_by_id(id.as_bytes().as_bstr())
        .map_err(crate::engine)?
        .ok_or(Error::Stale)?;
    if proxy.git_dir().canonicalize().map_err(Error::Io)? != expected_admin {
        return Err(Error::Stale);
    }
    Ok(())
}

fn list_inner(root: &Path, limits: Limits, ctx: &Context<'_>) -> Result<Inventory, Error> {
    limits.admit()?;
    let (repo, identity) = open(root, ctx)?;
    let common = common_dir(&repo)?;
    bounded_worktree_dir(&common, limits.max_entries, ctx)?;
    let mut result = Vec::new();
    let mut add = |id: Option<String>,
                   path: PathBuf,
                   head: Option<String>,
                   branch: Option<String>,
                   locked: bool,
                   lock_reason: Option<String>,
                   prunable: bool|
     -> Result<(), Error> {
        ctx.check()?;
        if result.len() >= limits.max_entries.saturating_add(1) {
            return Err(Error::Limited);
        }
        result.push(Worktree {
            id,
            path,
            head,
            branch,
            locked,
            lock_reason,
            prunable,
        });
        Ok(())
    };

    let main = repo.main_repo().map_err(crate::engine)?;
    if let Some(path) = main.workdir().map(Path::to_path_buf) {
        let head = main.head_id().ok().map(|id| id.to_string());
        let branch = main.head_name().ok().flatten().map(|name| name.to_string());
        add(None, path, head, branch, false, None, false)?;
    }

    let proxies = repo.worktrees().map_err(crate::engine)?;
    if proxies.len() > limits.max_entries {
        return Err(Error::Limited);
    }
    for proxy in proxies {
        ctx.check()?;
        let id = proxy
            .id()
            .map_err(crate::engine)?
            .to_str()
            .map_err(|_| Error::Unsupported)?
            .to_owned();
        validate_id(&id)?;
        let git_dir = proxy.git_dir().canonicalize().map_err(Error::Io)?;
        let path = proxy.base().map_err(crate::engine)?;
        let (locked, lock_reason) = match lock_capture(&common, &git_dir.join("locked"), ctx)? {
            Capture::Absent { .. } => (false, None),
            Capture::Present { bytes, .. } => {
                let reason = std::str::from_utf8(&bytes)
                    .map_err(|_| Error::Unsupported)?
                    .trim_end_matches(['\r', '\n'])
                    .to_owned();
                (true, Some(reason))
            }
        };
        let opened = proxy
            .into_repo_with_possibly_inaccessible_worktree()
            .map_err(crate::engine)?;
        let head = opened.head_id().ok().map(|id| id.to_string());
        let branch = opened
            .head_name()
            .ok()
            .flatten()
            .map(|name| name.to_string());
        let prunable = !locked && !path.is_dir();
        add(Some(id), path, head, branch, locked, lock_reason, prunable)?;
    }
    Ok(Inventory {
        repository: identity,
        worktrees: result,
    })
}

/// List the main and linked worktrees without opening their file contents.
pub fn list(
    root: &Path,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<Inventory, Error> {
    controlled(limits.timeout, cancelled, |ctx| {
        list_inner(root, limits, ctx)
    })
}

/// Admit a lock operation for an unlocked linked worktree. Empty reason is a
/// valid lock; line breaks and control bytes are rejected to keep the file form
/// deterministic.
pub fn prepare_lock(
    root: &Path,
    id: &str,
    reason: &str,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<LockReview, Error> {
    let id = id.to_owned();
    let bytes = lock_bytes(reason)?;
    controlled(limits.timeout, cancelled, |ctx| {
        limits.admit()?;
        validate_id(&id)?;
        let (repo, identity) = open(root, ctx)?;
        let common = common_dir(&repo)?;
        let proxy = repo
            .worktree_proxy_by_id(id.as_bytes().as_bstr())
            .map_err(crate::engine)?
            .ok_or(Error::Missing)?;
        let admin_dir = proxy.git_dir().canonicalize().map_err(Error::Io)?;
        let path = admin_dir.join("locked");
        let missing = match lock_capture(&common, &path, ctx)? {
            Capture::Absent { token } => token,
            Capture::Present { .. } => return Err(Error::Stale),
        };
        ctx.check()?;
        Ok(LockReview {
            identity,
            common_dir: common,
            id,
            admin_dir,
            target: path,
            missing,
            bytes,
        })
    })
}

/// Apply a reviewed lock only if repository, worktree registration, and target
/// absence still match. Publication is create-only.
pub fn apply_lock(review: LockReview, cancelled: impl FnMut() -> bool + Send) -> Result<(), Error> {
    controlled(Duration::from_secs(10), cancelled, |ctx| {
        let (repo, identity) = open(&review.identity.root, ctx)?;
        if identity.git_dir != review.identity.git_dir {
            return Err(Error::Stale);
        }
        verify_registration(&repo, &review.common_dir, &review.id, &review.admin_dir)?;
        match lock_capture(&review.common_dir, &review.target, ctx)? {
            Capture::Absent { token } if token == review.missing => {}
            _ => return Err(Error::Stale),
        }
        let pending = jc_fs::stage_missing_file(
            &review.common_dir,
            &review.missing,
            &review.bytes,
            &mut || ctx.check().is_err(),
        )
        .map_err(Error::Io)?;
        ctx.check()?;
        pending.publish().map_err(Error::Io)
    })
}

/// Admit an unlock operation only for a regular, bounded lock file.
pub fn prepare_unlock(
    root: &Path,
    id: &str,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<UnlockReview, Error> {
    let id = id.to_owned();
    controlled(limits.timeout, cancelled, |ctx| {
        limits.admit()?;
        validate_id(&id)?;
        let (repo, identity) = open(root, ctx)?;
        let common = common_dir(&repo)?;
        let proxy = repo
            .worktree_proxy_by_id(id.as_bytes().as_bstr())
            .map_err(crate::engine)?
            .ok_or(Error::Missing)?;
        let admin_dir = proxy.git_dir().canonicalize().map_err(Error::Io)?;
        let path = admin_dir.join("locked");
        let (token, bytes) = match lock_capture(&common, &path, ctx)? {
            Capture::Present { token, bytes, .. } => (token, bytes),
            Capture::Absent { .. } => return Err(Error::Stale),
        };
        ctx.check()?;
        Ok(UnlockReview {
            identity,
            common_dir: common,
            id,
            admin_dir,
            target: path,
            token,
            bytes,
        })
    })
}

/// Remove a lock only when the reviewed file identity and contents are intact.
pub fn apply_unlock(
    review: UnlockReview,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<(), Error> {
    controlled(Duration::from_secs(10), cancelled, |ctx| {
        let (repo, identity) = open(&review.identity.root, ctx)?;
        if identity.git_dir != review.identity.git_dir {
            return Err(Error::Stale);
        }
        verify_registration(&repo, &review.common_dir, &review.id, &review.admin_dir)?;
        match lock_capture(&review.common_dir, &review.target, ctx)? {
            Capture::Present { token, bytes, .. }
                if token == review.token && bytes == review.bytes => {}
            _ => return Err(Error::Stale),
        }
        ctx.check()?;
        jc_fs::remove_captured_file(&review.common_dir, &review.token).map_err(Error::Io)
    })
}
