//! Bounded linked-worktree inventory and reviewed lifecycle operations.
//!
//! Create/move/retire use pinned previews, create-only publication and explicit
//! recovery markers. Retire preserves the full checkout and Git administration
//! in a paired quarantine; it never invokes recursive deletion.

use crate::{Context, Error, Identity, Limits, controlled, open};
use gix::{ObjectId, bstr::ByteSlice, refs::FullName};
use jc_fs::{Capture, FileToken, MissingToken};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

const PATH_BYTES: usize = 4096;
const LOCK_BYTES: usize = 4 * PATH_BYTES + 512;
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

/// Create-only checkout review. The commit and destination are re-observed at
/// confirmation; gix reserves the linked worktree and checks branch occupancy.
pub struct CreateReview {
    identity: Identity,
    common_dir: PathBuf,
    destination: PathBuf,
    destination_parent: PathBuf,
    destination_missing: MissingToken,
    commit: String,
    branch: Option<String>,
    head: gix::worktree::add::Head,
    checkout: crate::worktree_materialize::CheckoutTree,
}

impl CreateReview {
    pub fn destination(&self) -> &Path {
        &self.destination
    }
    pub fn commit(&self) -> &str {
        &self.commit
    }
    pub fn branch(&self) -> Option<&str> {
        self.branch.as_deref()
    }
    pub fn file_count(&self) -> usize {
        self.checkout.file_count()
    }
    pub fn byte_count(&self) -> usize {
        self.checkout.byte_count()
    }
}

/// Result of publishing a new linked worktree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreatedWorktree {
    pub id: String,
    pub path: PathBuf,
    pub commit: String,
    pub branch: Option<String>,
    pub files: usize,
    pub bytes: usize,
}

/// Recovery review for a worktree left locked by an interrupted CIDE create.
pub struct ResumeCreateReview {
    identity: Identity,
    common_dir: PathBuf,
    id: String,
    admin_dir: PathBuf,
    lock_path: PathBuf,
    lock_token: FileToken,
    lock_bytes: Vec<u8>,
    commit: String,
    checkout: crate::worktree_materialize::CheckoutTree,
}

/// Review for a same-volume create-only move. All user files remain in the
/// directory; only the Git administrative backlink changes after the rename.
pub struct MoveReview {
    identity: Identity,
    common_dir: PathBuf,
    id: String,
    admin_dir: PathBuf,
    source: PathBuf,
    destination: PathBuf,
    destination_parent: PathBuf,
    destination_missing: MissingToken,
    source_dotgit: FileToken,
    source_dotgit_bytes: Vec<u8>,
    admin_gitdir: FileToken,
    admin_gitdir_bytes: Vec<u8>,
}

impl MoveReview {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn source(&self) -> &Path {
        &self.source
    }
    pub fn destination(&self) -> &Path {
        &self.destination
    }
}

/// Recovery review for a move whose lock marker survived interruption.
pub struct ResumeMoveReview {
    identity: Identity,
    common_dir: PathBuf,
    id: String,
    admin_dir: PathBuf,
    source: PathBuf,
    destination: PathBuf,
    lock_path: PathBuf,
    lock_token: FileToken,
    lock_bytes: Vec<u8>,
}

impl ResumeMoveReview {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn source(&self) -> &Path {
        &self.source
    }
    pub fn destination(&self) -> &Path {
        &self.destination
    }
}

/// Reversible removal review. The complete checkout and its Git administration
/// are moved into private quarantine directories; no user file is deleted.
pub struct RetireReview {
    identity: Identity,
    common_dir: PathBuf,
    id: String,
    admin_dir: PathBuf,
    worktree: PathBuf,
    quarantine_worktree: PathBuf,
    quarantine_admin: PathBuf,
    worktree_parent: PathBuf,
    worktree_destination_missing: MissingToken,
    admin_destination_missing: MissingToken,
    dotgit: FileToken,
    dotgit_bytes: Vec<u8>,
    gitdir: FileToken,
    gitdir_bytes: Vec<u8>,
    marker: Vec<u8>,
}

impl RetireReview {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn worktree(&self) -> &Path {
        &self.worktree
    }
    pub fn quarantine_worktree(&self) -> &Path {
        &self.quarantine_worktree
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetiredWorktree {
    pub id: String,
    pub worktree: PathBuf,
    pub git_admin: PathBuf,
    pub preserved_files: bool,
}

/// Review for restoring a quarantined worktree. It accepts only CIDE's exact
/// retirement marker and the original file identities captured in that marker.
pub struct RestoreRetiredReview {
    identity: Identity,
    common_dir: PathBuf,
    id: String,
    original_worktree: PathBuf,
    quarantine_worktree: PathBuf,
    original_admin: PathBuf,
    quarantine_admin: PathBuf,
    current_worktree: PathBuf,
    current_admin: PathBuf,
    lock_path: PathBuf,
    lock_token: FileToken,
    lock_bytes: Vec<u8>,
    dotgit_key: [u8; 12],
    gitdir_key: [u8; 12],
}

struct RetireMarker {
    original_worktree: PathBuf,
    quarantine_worktree: PathBuf,
    id: String,
    original_admin: PathBuf,
    quarantine_admin: PathBuf,
    dotgit_key: [u8; 12],
    gitdir_key: [u8; 12],
}

struct MoveKeys {
    source: [u8; 12],
    next_source: [u8; 12],
    admin: [u8; 12],
}

impl RestoreRetiredReview {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn original_worktree(&self) -> &Path {
        &self.original_worktree
    }
}

/// Review a same-volume worktree move. Dirty files are allowed because the
/// operation moves the complete directory without interpreting or replacing it.
pub fn prepare_move(
    root: &Path,
    id: &str,
    destination: &Path,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<MoveReview, Error> {
    let id = id.to_owned();
    let destination = destination.to_owned();
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
        let source = proxy
            .base()
            .map_err(crate::engine)?
            .canonicalize()
            .map_err(Error::Io)?;
        if source == Path::new("/") || !source.is_dir() {
            return Err(Error::InvalidRoot);
        }
        match lock_capture(&common, &admin_dir.join("locked"), ctx)? {
            Capture::Absent { .. } => {}
            Capture::Present { .. } => return Err(Error::Stale),
        }
        if !destination.is_absolute() || destination.file_name().is_none() {
            return Err(Error::InvalidRoot);
        }
        let parent = destination
            .parent()
            .ok_or(Error::InvalidRoot)?
            .canonicalize()?;
        let destination = parent.join(destination.file_name().ok_or(Error::InvalidRoot)?);
        if destination.starts_with(&source)
            || source.starts_with(&destination)
            || destination == source
        {
            return Err(Error::Unsupported);
        }
        let destination_missing =
            match jc_fs::capture_in_root(&parent, &destination, 0, &mut || ctx.check().is_err())? {
                Capture::Absent { token } if token.is_leaf() => token,
                _ => return Err(Error::Stale),
            };
        let dotgit_path = source.join(".git");
        let (source_dotgit, source_dotgit_bytes) = match jc_fs::capture_in_root(
            &source,
            &dotgit_path,
            4096,
            &mut || ctx.check().is_err(),
        )? {
            Capture::Present { token, bytes, .. } if bytes.starts_with(b"gitdir: ") => {
                (token, bytes)
            }
            _ => return Err(Error::Unsupported),
        };
        let dotgit_text = std::str::from_utf8(&source_dotgit_bytes)
            .map_err(crate::engine)?
            .strip_prefix("gitdir: ")
            .ok_or(Error::Unsupported)?
            .trim_end_matches(['\r', '\n']);
        if dotgit_text.is_empty()
            || !normalized_path_eq(&link_target(&source_dotgit_bytes, &source)?, &admin_dir)?
        {
            return Err(Error::Stale);
        }
        let gitdir_path = admin_dir.join("gitdir");
        let (admin_gitdir, admin_gitdir_bytes) = match jc_fs::capture_in_root(
            &common,
            &gitdir_path,
            4096,
            &mut || ctx.check().is_err(),
        )? {
            Capture::Present { token, bytes, .. } if token.has_single_link() => (token, bytes),
            _ => return Err(Error::Unsupported),
        };
        Ok(MoveReview {
            identity,
            common_dir: common,
            id,
            admin_dir,
            source,
            destination,
            destination_parent: parent,
            destination_missing,
            source_dotgit,
            source_dotgit_bytes,
            admin_gitdir,
            admin_gitdir_bytes,
        })
    })
}

/// Move the whole checkout create-only, then atomically update its administrative
/// backlink. If the process stops after the directory rename, the Git lock keeps
/// the unfinished operation visible for recovery and no checkout bytes are lost.
pub fn apply_move(
    review: MoveReview,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<Worktree, Error> {
    controlled(Duration::from_secs(30), cancelled, |ctx| {
        let (repo, identity) = open(&review.identity.root, ctx)?;
        if identity.git_dir != review.identity.git_dir || common_dir(&repo)? != review.common_dir {
            return Err(Error::Stale);
        }
        verify_registration(&repo, &review.common_dir, &review.id, &review.admin_dir)?;
        reject_existing(&review.destination)?;
        match jc_fs::capture_in_root(
            &review.destination_parent,
            &review.destination,
            0,
            &mut || ctx.check().is_err(),
        )? {
            Capture::Absent { token } if token == review.destination_missing => {}
            _ => return Err(Error::Stale),
        }
        let source_parent = review.source.parent().ok_or(Error::InvalidRoot)?;
        let (dotgit_token, dotgit_bytes) = match jc_fs::capture_in_root(
            source_parent,
            &review.source.join(".git"),
            4096,
            &mut || ctx.check().is_err(),
        )? {
            Capture::Present { token, bytes, .. }
                if token.object_key() == review.source_dotgit.object_key()
                    && bytes == review.source_dotgit_bytes =>
            {
                (token, bytes)
            }
            _ => return Err(Error::Stale),
        };
        if !normalized_path_eq(
            &link_target(&dotgit_bytes, &review.source)?,
            &review.admin_dir,
        )? {
            return Err(Error::Stale);
        }
        match jc_fs::capture_in_root(
            &review.common_dir,
            &review.admin_dir.join("gitdir"),
            4096,
            &mut || ctx.check().is_err(),
        )? {
            Capture::Present { token, bytes, .. }
                if token == review.admin_gitdir && bytes == review.admin_gitdir_bytes => {}
            _ => return Err(Error::Stale),
        }
        let dotgit_text = std::str::from_utf8(&dotgit_bytes)
            .map_err(crate::engine)?
            .strip_prefix("gitdir: ")
            .ok_or(Error::Unsupported)?
            .trim_end_matches(['\r', '\n']);
        let mut normalized_dotgit = None;
        let next_dotgit_key = if Path::new(dotgit_text).is_absolute() {
            dotgit_token.object_key()
        } else {
            let after = gitfile_link(&review.admin_dir)?;
            let pending = jc_fs::stage_existing_file(
                source_parent,
                &dotgit_token,
                &dotgit_bytes,
                &after,
                &review.source,
                &mut || ctx.check().is_err(),
            )?;
            let key = pending.replacement_key()?;
            normalized_dotgit = Some((pending, after));
            key
        };
        let move_marker = make_move_marker(
            &review.source,
            &review.destination,
            dotgit_token.object_key(),
            next_dotgit_key,
            review.admin_gitdir.object_key(),
        )?;
        let lock_path = review.admin_dir.join("locked");
        let lock_token = match lock_capture(&review.common_dir, &lock_path, ctx)? {
            Capture::Absent { token } => {
                jc_fs::stage_missing_file(&review.common_dir, &token, &move_marker, &mut || {
                    ctx.check().is_err()
                })?
                .publish()?;
                match lock_capture(&review.common_dir, &lock_path, ctx)? {
                    Capture::Present { token, bytes, .. } if bytes == move_marker => token,
                    _ => return Err(Error::Stale),
                }
            }
            Capture::Present { .. } => return Err(Error::Stale),
        };
        ctx.check()?;
        if let Some((pending, after)) = normalized_dotgit {
            let backup = pending.publish().map_err(replace_error)?;
            cleanup_backup(
                &review.source,
                &backup,
                dotgit_token.object_key(),
                &dotgit_bytes,
                ctx,
            )?;
            let (token, bytes) =
                capture_regular(&review.source, &review.source.join(".git"), 4096, ctx)?;
            if token.object_key() != next_dotgit_key || bytes != after {
                return Err(Error::Stale);
            }
        }
        if let Err(error) =
            crate::remote::publish_directory_create_only(&review.source, &review.destination)
        {
            // A failed same-volume rename leaves the original tree intact. Clear
            // only our own lock when the source/destination observation confirms it.
            let source_exists = review.source.is_dir();
            let destination_absent = !review.destination.exists();
            if source_exists && destination_absent {
                jc_fs::remove_captured_file(&review.common_dir, &lock_token)?;
            }
            return Err(error);
        }
        let new_backlink = gitdir_link(&review.destination)?;
        let pending = jc_fs::stage_existing_file(
            &review.common_dir,
            &review.admin_gitdir,
            &review.admin_gitdir_bytes,
            &new_backlink,
            &review.common_dir.join("worktrees"),
            &mut || ctx.check().is_err(),
        )?;
        let backup = pending.publish().map_err(replace_error)?;
        cleanup_backup(
            &review.common_dir.join("worktrees"),
            &backup,
            review.admin_gitdir.object_key(),
            &review.admin_gitdir_bytes,
            ctx,
        )?;
        match lock_capture(&review.common_dir, &lock_path, ctx)? {
            Capture::Present { token, bytes, .. }
                if token == lock_token && bytes == move_marker =>
            {
                jc_fs::remove_captured_file(&review.common_dir, &lock_token)?;
            }
            _ => return Err(Error::Stale),
        }
        let linked = repo
            .worktree_proxy_by_id(review.id.as_bytes().as_bstr())
            .map_err(crate::engine)?
            .ok_or(Error::Stale)?;
        let opened = linked
            .into_repo_with_possibly_inaccessible_worktree()
            .map_err(crate::engine)?;
        Ok(Worktree {
            id: Some(review.id),
            path: review.destination,
            head: opened.head_id().ok().map(|id| id.to_string()),
            branch: opened
                .head_name()
                .ok()
                .flatten()
                .map(|name| name.to_string()),
            locked: false,
            lock_reason: None,
            prunable: false,
        })
    })
}

/// Admit recovery for a move carrying CIDE's exact journal marker. The source
/// or destination must exist exclusively; ambiguous states are left untouched.
pub fn prepare_resume_move(
    root: &Path,
    id: &str,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<ResumeMoveReview, Error> {
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
        let lock_path = admin_dir.join("locked");
        let (lock_token, lock_bytes) = match lock_capture(&common, &lock_path, ctx)? {
            Capture::Present { token, bytes, .. }
                if bytes.starts_with(b"CIDE-MOVE-2\n") || bytes.starts_with(b"CIDE-MOVE-3\n") =>
            {
                (token, bytes)
            }
            _ => return Err(Error::Unsupported),
        };
        let mut lines = std::str::from_utf8(&lock_bytes)
            .map_err(crate::engine)?
            .lines();
        let marker_version = lines.next().ok_or(Error::Unsupported)?;
        if marker_version != "CIDE-MOVE-2" && marker_version != "CIDE-MOVE-3" {
            return Err(Error::Unsupported);
        }
        let source = PathBuf::from(lines.next().ok_or(Error::Unsupported)?);
        let destination = PathBuf::from(lines.next().ok_or(Error::Unsupported)?);
        let source_dotgit_key = parse_key(lines.next().ok_or(Error::Unsupported)?)?;
        let source_dotgit_next_key = if marker_version == "CIDE-MOVE-3" {
            parse_key(lines.next().ok_or(Error::Unsupported)?)?
        } else {
            source_dotgit_key
        };
        let admin_gitdir_key = parse_key(lines.next().ok_or(Error::Unsupported)?)?;
        if lines.next().is_some() || !source.is_absolute() || !destination.is_absolute() {
            return Err(Error::Unsupported);
        }
        let source_exists = source.is_dir();
        let destination_exists = destination.is_dir();
        if source_exists == destination_exists {
            return Err(Error::Stale);
        }
        let worktree = if source_exists { &source } else { &destination };
        let (dotgit, _) = capture_regular(worktree, &worktree.join(".git"), 4096, ctx)?;
        if dotgit.object_key() != source_dotgit_key && dotgit.object_key() != source_dotgit_next_key
        {
            return Err(Error::Stale);
        }
        let (admin_link, bytes) = capture_regular(&common, &admin_dir.join("gitdir"), 4096, ctx)?;
        if admin_link.object_key() != admin_gitdir_key {
            return Err(Error::Stale);
        }
        let backlink = admin_gitdir_target(&bytes, &admin_dir)?;
        if !normalized_path_eq(&backlink, &source.join(".git"))?
            && !normalized_path_eq(&backlink, &destination.join(".git"))?
        {
            return Err(Error::Stale);
        }
        let dotgit_target = link_target(
            &capture_regular(worktree, &worktree.join(".git"), 4096, ctx)?.1,
            &source,
        )?;
        if !normalized_path_eq(&dotgit_target, &admin_dir)? {
            return Err(Error::Stale);
        }
        ctx.check()?;
        Ok(ResumeMoveReview {
            identity,
            common_dir: common,
            id,
            admin_dir,
            source,
            destination,
            lock_path,
            lock_token,
            lock_bytes,
        })
    })
}

/// Complete a journaled move. It repeats a missing rename only when the old
/// path is still present, validates both linking files, and changes only those
/// two exact metadata files before clearing the lock.
pub fn apply_resume_move(
    review: ResumeMoveReview,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<Worktree, Error> {
    controlled(Duration::from_secs(30), cancelled, |ctx| {
        let (repo, identity) = open(&review.identity.root, ctx)?;
        if identity.git_dir != review.identity.git_dir || common_dir(&repo)? != review.common_dir {
            return Err(Error::Stale);
        }
        verify_registration(&repo, &review.common_dir, &review.id, &review.admin_dir)?;
        match lock_capture(&review.common_dir, &review.lock_path, ctx)? {
            Capture::Present { token, bytes, .. }
                if token == review.lock_token && bytes == review.lock_bytes => {}
            _ => return Err(Error::Stale),
        }
        let source_exists = review.source.is_dir();
        let destination_exists = review.destination.is_dir();
        if source_exists == destination_exists {
            return Err(Error::Stale);
        }
        let current_worktree = if source_exists {
            &review.source
        } else {
            &review.destination
        };
        let (mut current_dotgit_token, mut current_dotgit_bytes) =
            capture_regular(current_worktree, &current_worktree.join(".git"), 4096, ctx)?;
        let MoveKeys {
            source: source_key,
            next_source: mut next_source_key,
            admin: admin_key,
        } = journal_keys(&review.lock_bytes)?;
        let mut lock_token = review.lock_token.clone();
        let mut lock_bytes = review.lock_bytes.clone();
        if (current_dotgit_token.object_key() != source_key
            && current_dotgit_token.object_key() != next_source_key)
            || !normalized_path_eq(
                &link_target(&current_dotgit_bytes, &review.source)?,
                &review.admin_dir,
            )?
        {
            return Err(Error::Stale);
        }
        if current_dotgit_token.object_key() == source_key && source_key != next_source_key {
            let worktree_parent = current_worktree.parent().ok_or(Error::InvalidRoot)?;
            let (scoped_token, scoped_bytes) =
                capture_regular(worktree_parent, &current_worktree.join(".git"), 4096, ctx)?;
            if scoped_token.object_key() != current_dotgit_token.object_key()
                || scoped_bytes != current_dotgit_bytes
            {
                return Err(Error::Stale);
            }
            let replacement = gitfile_link(&review.admin_dir)?;
            let pending = jc_fs::stage_existing_file(
                worktree_parent,
                &scoped_token,
                &scoped_bytes,
                &replacement,
                current_worktree,
                &mut || ctx.check().is_err(),
            )?;
            next_source_key = pending.replacement_key()?;
            let updated_marker = make_move_marker(
                &review.source,
                &review.destination,
                source_key,
                next_source_key,
                admin_key,
            )?;
            replace_file(
                &review.common_dir,
                &lock_token,
                &lock_bytes,
                &updated_marker,
                &review.common_dir.join("worktrees"),
                ctx,
            )?;
            (lock_token, lock_bytes) =
                match lock_capture(&review.common_dir, &review.lock_path, ctx)? {
                    Capture::Present { token, bytes, .. } if bytes == updated_marker => {
                        (token, bytes)
                    }
                    _ => return Err(Error::Stale),
                };
            let backup = pending.publish().map_err(replace_error)?;
            cleanup_backup(
                current_worktree,
                &backup,
                scoped_token.object_key(),
                &scoped_bytes,
                ctx,
            )?;
            (current_dotgit_token, current_dotgit_bytes) =
                capture_regular(current_worktree, &current_worktree.join(".git"), 4096, ctx)?;
            if current_dotgit_token.object_key() != next_source_key
                || !normalized_path_eq(
                    &link_target(&current_dotgit_bytes, &review.source)?,
                    &review.admin_dir,
                )?
            {
                return Err(Error::Stale);
            }
        }
        let (current_admin_token, current_admin_bytes) = capture_regular(
            &review.common_dir,
            &review.admin_dir.join("gitdir"),
            4096,
            ctx,
        )?;
        let current_backlink = admin_gitdir_target(&current_admin_bytes, &review.admin_dir)?;
        let expected_destination_backlink = gitdir_link(&review.destination)?;
        let admin_is_already_updated = current_admin_bytes == expected_destination_backlink
            && !source_exists
            && normalized_path_eq(&current_backlink, &review.destination.join(".git"))?;
        if !admin_is_already_updated
            && (current_admin_token.object_key() != admin_key
                || !normalized_path_eq(&current_backlink, &review.source.join(".git"))?)
        {
            return Err(Error::Stale);
        }
        if source_exists {
            crate::remote::publish_directory_create_only(&review.source, &review.destination)?;
        }
        let dotgit = review.destination.join(".git");
        let (dotgit_token, dotgit_bytes) =
            capture_regular(&review.destination, &dotgit, 4096, ctx)?;
        let expected_admin = link_target(&dotgit_bytes, &review.destination)?;
        if dotgit_token.object_key() != next_source_key || expected_admin != review.admin_dir {
            return Err(Error::Stale);
        }
        let admin_gitdir = review.admin_dir.join("gitdir");
        let (admin_token, admin_bytes) =
            capture_regular(&review.common_dir, &admin_gitdir, 4096, ctx)?;
        let expected_dotgit = admin_gitdir_target(&admin_bytes, &review.admin_dir)?;
        if admin_token.object_key() != admin_key {
            return Err(Error::Stale);
        }
        if normalized_path_eq(&expected_dotgit, &review.source.join(".git"))? {
            let replacement = gitdir_link(&review.destination)?;
            replace_file(
                &review.common_dir,
                &admin_token,
                &admin_bytes,
                &replacement,
                &review.common_dir.join("worktrees"),
                ctx,
            )?;
        } else if !normalized_path_eq(&expected_dotgit, &dotgit)? {
            return Err(Error::Stale);
        }
        match lock_capture(&review.common_dir, &review.lock_path, ctx)? {
            Capture::Present { token, bytes, .. } if token == lock_token && bytes == lock_bytes => {
                jc_fs::remove_captured_file(&review.common_dir, &lock_token)?;
            }
            _ => return Err(Error::Stale),
        }
        let linked = repo
            .worktree_proxy_by_id(review.id.as_bytes().as_bstr())
            .map_err(crate::engine)?
            .ok_or(Error::Stale)?;
        let opened = linked
            .into_repo_with_possibly_inaccessible_worktree()
            .map_err(crate::engine)?;
        Ok(Worktree {
            id: Some(review.id),
            path: review.destination,
            head: opened.head_id().ok().map(|id| id.to_string()),
            branch: opened
                .head_name()
                .ok()
                .flatten()
                .map(|name| name.to_string()),
            locked: false,
            lock_reason: None,
            prunable: false,
        })
    })
}

/// Preview retiring a linked worktree into a reversible quarantine. Both the
/// checkout and administration move create-only to sibling locations so their
/// bytes remain on their original volumes and can be restored later.
pub fn prepare_retire(
    root: &Path,
    id: &str,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<RetireReview, Error> {
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
        let worktree = proxy
            .base()
            .map_err(crate::engine)?
            .canonicalize()
            .map_err(Error::Io)?;
        let worktree_parent = worktree.parent().ok_or(Error::InvalidRoot)?.to_path_buf();
        if !worktree.is_dir() || worktree.file_name().is_none() {
            return Err(Error::InvalidRoot);
        }
        match lock_capture(&common, &admin_dir.join("locked"), ctx)? {
            Capture::Absent { .. } => {}
            Capture::Present { .. } => return Err(Error::Stale),
        }
        let (dotgit, dotgit_bytes) = capture_regular(&worktree, &worktree.join(".git"), 4096, ctx)?;
        if !normalized_path_eq(&link_target(&dotgit_bytes, &worktree)?, &admin_dir)? {
            return Err(Error::Stale);
        }
        let (gitdir, gitdir_bytes) =
            capture_regular(&common, &admin_dir.join("gitdir"), 4096, ctx)?;
        if !normalized_path_eq(
            &admin_gitdir_target(&gitdir_bytes, &admin_dir)?,
            &worktree.join(".git"),
        )? {
            return Err(Error::Stale);
        }
        let quarantine_worktree = worktree_parent.join(".cide-worktree-quarantine").join(&id);
        let quarantine_admin = common.join(".cide-worktree-quarantine").join(&id);
        let worktree_destination_missing =
            missing_location(&worktree_parent, &quarantine_worktree, ctx)?;
        let admin_destination_missing = missing_location(&common, &quarantine_admin, ctx)?;
        let marker = format!(
            "CIDE-RETIRE-1\n{}\n{}\n{}\n{}\n{}:{}\n",
            path_text(&worktree)?,
            path_text(&quarantine_worktree)?,
            path_text(&admin_dir)?,
            path_text(&quarantine_admin)?,
            key_hex(dotgit.object_key()),
            key_hex(gitdir.object_key()),
        )
        .into_bytes();
        if marker.len() > LOCK_BYTES {
            return Err(Error::Limited);
        }
        Ok(RetireReview {
            identity,
            common_dir: common,
            id,
            admin_dir,
            worktree,
            quarantine_worktree,
            quarantine_admin,
            worktree_parent,
            worktree_destination_missing,
            admin_destination_missing,
            dotgit,
            dotgit_bytes,
            gitdir,
            gitdir_bytes,
            marker,
        })
    })
}

/// Retire without deleting content. The result is no longer listed as an active
/// worktree, while the complete working tree, index, HEAD, and administration
/// stay available in the paired quarantine directories.
pub fn apply_retire(
    review: RetireReview,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<RetiredWorktree, Error> {
    controlled(Duration::from_secs(30), cancelled, |ctx| {
        let (repo, identity) = open(&review.identity.root, ctx)?;
        if identity.git_dir != review.identity.git_dir || common_dir(&repo)? != review.common_dir {
            return Err(Error::Stale);
        }
        verify_registration(&repo, &review.common_dir, &review.id, &review.admin_dir)?;
        match jc_fs::capture_in_root(
            &review.worktree_parent,
            &review.quarantine_worktree,
            0,
            &mut || ctx.check().is_err(),
        )? {
            Capture::Absent { token }
                if review.worktree_destination_missing.same_location_as(&token) => {}
            _ => return Err(Error::Stale),
        }
        match jc_fs::capture_in_root(&review.common_dir, &review.quarantine_admin, 0, &mut || {
            ctx.check().is_err()
        })? {
            Capture::Absent { token }
                if review.admin_destination_missing.same_location_as(&token) => {}
            _ => return Err(Error::Stale),
        }
        match capture_regular(&review.worktree, &review.worktree.join(".git"), 4096, ctx)? {
            (token, bytes) if token == review.dotgit && bytes == review.dotgit_bytes => {}
            _ => return Err(Error::Stale),
        }
        match capture_regular(
            &review.common_dir,
            &review.admin_dir.join("gitdir"),
            4096,
            ctx,
        )? {
            (token, bytes) if token == review.gitdir && bytes == review.gitdir_bytes => {}
            _ => return Err(Error::Stale),
        }
        let lock_path = review.admin_dir.join("locked");
        let lock_token = match lock_capture(&review.common_dir, &lock_path, ctx)? {
            Capture::Absent { token } => {
                jc_fs::stage_missing_file(&review.common_dir, &token, &review.marker, &mut || {
                    ctx.check().is_err()
                })?
                .publish()?;
                match lock_capture(&review.common_dir, &lock_path, ctx)? {
                    Capture::Present { token, bytes, .. } if bytes == review.marker => token,
                    _ => return Err(Error::Stale),
                }
            }
            Capture::Present { .. } => return Err(Error::Stale),
        };
        match capture_regular(&review.worktree, &review.worktree.join(".git"), 4096, ctx)? {
            (token, bytes)
                if token == review.dotgit
                    && bytes == review.dotgit_bytes
                    && normalized_path_eq(
                        &link_target(&bytes, &review.worktree)?,
                        &review.admin_dir,
                    )? => {}
            _ => return Err(Error::Stale),
        }
        match capture_regular(
            &review.common_dir,
            &review.admin_dir.join("gitdir"),
            4096,
            ctx,
        )? {
            (token, bytes)
                if token == review.gitdir
                    && bytes == review.gitdir_bytes
                    && normalized_path_eq(
                        &admin_gitdir_target(&bytes, &review.admin_dir)?,
                        &review.worktree.join(".git"),
                    )? => {}
            _ => return Err(Error::Stale),
        }
        create_parents_if_needed(
            &review.worktree_parent,
            &review.quarantine_worktree,
            &review.worktree_destination_missing,
            ctx,
        )?;
        create_parents_if_needed(
            &review.common_dir,
            &review.quarantine_admin,
            &review.admin_destination_missing,
            ctx,
        )?;
        crate::remote::publish_directory_create_only(
            &review.worktree,
            &review.quarantine_worktree,
        )?;
        let new_backlink = gitdir_link(&review.quarantine_worktree)?;
        replace_file(
            &review.common_dir,
            &review.gitdir,
            &review.gitdir_bytes,
            &new_backlink,
            &review.common_dir.join("worktrees"),
            ctx,
        )?;
        let new_dotgit = gitfile_link(&review.quarantine_admin)?;
        let worktree_parent = review.worktree_parent.as_path();
        let quarantine_parent = worktree_parent.join(".cide-worktree-quarantine");
        let (dotgit_token, dotgit_bytes) = capture_regular(
            worktree_parent,
            &review.quarantine_worktree.join(".git"),
            4096,
            ctx,
        )?;
        if dotgit_token.object_key() != review.dotgit.object_key()
            || dotgit_bytes != review.dotgit_bytes
        {
            return Err(Error::Stale);
        }
        replace_file(
            worktree_parent,
            &dotgit_token,
            &dotgit_bytes,
            &new_dotgit,
            &quarantine_parent,
            ctx,
        )?;
        crate::remote::publish_directory_create_only(&review.admin_dir, &review.quarantine_admin)?;
        let _ = lock_token; // The marker moves with the administration and enables explicit recovery.
        Ok(RetiredWorktree {
            id: review.id,
            worktree: review.quarantine_worktree,
            git_admin: review.quarantine_admin,
            preserved_files: true,
        })
    })
}

/// Admit restoration of a complete CIDE quarantine or of either recoverable
/// half of its interrupted move.
pub fn prepare_restore_retired(
    root: &Path,
    id: &str,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<RestoreRetiredReview, Error> {
    let id = id.to_owned();
    controlled(limits.timeout, cancelled, |ctx| {
        limits.admit()?;
        validate_id(&id)?;
        let (repo, identity) = open(root, ctx)?;
        let common = common_dir(&repo)?;
        let original_admin = common.join("worktrees").join(&id);
        let quarantine_admin = common.join(".cide-worktree-quarantine").join(&id);
        let admin_orig_exists = original_admin.is_dir();
        let admin_quarantine_exists = quarantine_admin.is_dir();
        if admin_orig_exists == admin_quarantine_exists {
            return Err(Error::Stale);
        }
        let current_admin = if admin_orig_exists {
            original_admin.clone()
        } else {
            quarantine_admin.clone()
        };
        let lock_path = current_admin.join("locked");
        let (lock_token, lock_bytes) = match lock_capture(&common, &lock_path, ctx)? {
            Capture::Present { token, bytes, .. } if bytes.starts_with(b"CIDE-RETIRE-1\n") => {
                (token, bytes)
            }
            _ => return Err(Error::Unsupported),
        };
        let marker = parse_retire_marker(&lock_bytes)?;
        if marker.id != id
            || marker.original_admin != original_admin
            || marker.quarantine_admin != quarantine_admin
        {
            return Err(Error::Stale);
        }
        let original_worktree = marker.original_worktree;
        let quarantine_worktree = marker.quarantine_worktree;
        let dotgit_key = marker.dotgit_key;
        let gitdir_key = marker.gitdir_key;
        let quarantine_worktree_path_exists = quarantine_worktree.is_dir();
        let original_worktree_exists = original_worktree.is_dir();
        if quarantine_worktree_path_exists == original_worktree_exists {
            return Err(Error::Stale);
        }
        let current_worktree = if quarantine_worktree_path_exists {
            quarantine_worktree.clone()
        } else {
            original_worktree.clone()
        };
        let (dotgit, dotgit_bytes) =
            capture_regular(&current_worktree, &current_worktree.join(".git"), 4096, ctx)?;
        let (gitdir, gitdir_bytes) =
            capture_regular(&common, &current_admin.join("gitdir"), 4096, ctx)?;
        let dotgit_target = link_target(&dotgit_bytes, &current_worktree)?;
        let gitdir_target = admin_gitdir_target(&gitdir_bytes, &current_admin)?;
        let original_dotgit = dotgit.object_key() == dotgit_key
            && normalized_path_eq(&dotgit_target, &original_admin)?;
        let published_dotgit = dotgit_bytes == gitfile_link(&quarantine_admin)?;
        let original_gitdir = gitdir.object_key() == gitdir_key
            && normalized_path_eq(&gitdir_target, &original_worktree.join(".git"))?;
        let published_gitdir = gitdir_bytes == gitdir_link(&quarantine_worktree)?;
        let already_restored =
            current_worktree == original_worktree && current_admin == original_admin;
        let restored_dotgit = already_restored && dotgit_bytes == gitfile_link(&original_admin)?;
        let restored_gitdir = already_restored && gitdir_bytes == gitdir_link(&original_worktree)?;
        if !(original_dotgit || published_dotgit || restored_dotgit)
            || !(original_gitdir || published_gitdir || restored_gitdir)
        {
            return Err(Error::Stale);
        }
        ctx.check()?;
        Ok(RestoreRetiredReview {
            identity,
            common_dir: common,
            id,
            original_worktree,
            quarantine_worktree,
            original_admin,
            quarantine_admin,
            current_worktree,
            current_admin,
            lock_path,
            lock_token,
            lock_bytes,
            dotgit_key: dotgit.object_key(),
            gitdir_key: gitdir.object_key(),
        })
    })
}

/// Restore a quarantined worktree create-only and re-register it with Git. A
/// surviving marker permits retry if interruption happens between the moves.
pub fn apply_restore_retired(
    review: RestoreRetiredReview,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<Worktree, Error> {
    controlled(Duration::from_secs(30), cancelled, |ctx| {
        let (repo, identity) = open(&review.identity.root, ctx)?;
        if identity.git_dir != review.identity.git_dir || common_dir(&repo)? != review.common_dir {
            return Err(Error::Stale);
        }
        match lock_capture(&review.common_dir, &review.lock_path, ctx)? {
            Capture::Present { token, bytes, .. }
                if token == review.lock_token && bytes == review.lock_bytes => {}
            _ => return Err(Error::Stale),
        }
        let worktree_is_quarantined = review.current_worktree == review.quarantine_worktree;
        let admin_is_quarantined = review.current_admin == review.quarantine_admin;
        if worktree_is_quarantined == review.original_worktree.is_dir()
            || admin_is_quarantined == review.original_admin.is_dir()
        {
            return Err(Error::Stale);
        }
        let (pre_dotgit, pre_dotgit_bytes) = capture_regular(
            &review.current_worktree,
            &review.current_worktree.join(".git"),
            4096,
            ctx,
        )?;
        let (pre_gitdir, pre_gitdir_bytes) = capture_regular(
            &review.common_dir,
            &review.current_admin.join("gitdir"),
            4096,
            ctx,
        )?;
        let pre_dotgit_target = link_target(&pre_dotgit_bytes, &review.current_worktree)?;
        let pre_gitdir_target = admin_gitdir_target(&pre_gitdir_bytes, &review.current_admin)?;
        if pre_dotgit.object_key() != review.dotgit_key
            || pre_gitdir.object_key() != review.gitdir_key
            || (!normalized_path_eq(&pre_dotgit_target, &review.original_admin)?
                && !normalized_path_eq(&pre_dotgit_target, &review.quarantine_admin)?)
            || (!normalized_path_eq(&pre_gitdir_target, &review.original_worktree.join(".git"))?
                && !normalized_path_eq(
                    &pre_gitdir_target,
                    &review.quarantine_worktree.join(".git"),
                )?)
            || (worktree_is_quarantined && review.original_worktree.exists())
            || (admin_is_quarantined && review.original_admin.exists())
        {
            return Err(Error::Stale);
        }
        let mut current_worktree = review.current_worktree.clone();
        let mut current_admin = review.current_admin.clone();
        if current_worktree == review.quarantine_worktree {
            if review.original_worktree.exists() {
                return Err(Error::Stale);
            }
            crate::remote::publish_directory_create_only(
                &review.quarantine_worktree,
                &review.original_worktree,
            )?;
            current_worktree = review.original_worktree.clone();
        }
        if current_admin == review.quarantine_admin {
            if review.original_admin.exists() {
                return Err(Error::Stale);
            }
            crate::remote::publish_directory_create_only(
                &review.quarantine_admin,
                &review.original_admin,
            )?;
            current_admin = review.original_admin.clone();
        }
        if current_worktree != review.original_worktree || current_admin != review.original_admin {
            return Err(Error::Stale);
        }
        let dotgit_path = current_worktree.join(".git");
        let (dotgit_token, dotgit_bytes) =
            capture_regular(&current_worktree, &dotgit_path, 4096, ctx)?;
        if dotgit_token.object_key() != review.dotgit_key {
            return Err(Error::Stale);
        }
        let dotgit_target = link_target(&dotgit_bytes, &current_worktree)?;
        if !normalized_path_eq(&dotgit_target, &review.original_admin)? {
            if !normalized_path_eq(&dotgit_target, &review.quarantine_admin)? {
                return Err(Error::Stale);
            }
            let replacement = gitfile_link(&review.original_admin)?;
            let worktree_parent = review
                .original_worktree
                .parent()
                .ok_or(Error::InvalidRoot)?;
            let (dotgit_token, dotgit_bytes) =
                capture_regular(worktree_parent, &dotgit_path, 4096, ctx)?;
            if dotgit_token.object_key() != review.dotgit_key
                || !normalized_path_eq(
                    &link_target(&dotgit_bytes, &current_worktree)?,
                    &review.quarantine_admin,
                )?
            {
                return Err(Error::Stale);
            }
            replace_file(
                worktree_parent,
                &dotgit_token,
                &dotgit_bytes,
                &replacement,
                &worktree_parent.join(".cide-worktree-quarantine"),
                ctx,
            )?;
        }
        let gitdir_path = current_admin.join("gitdir");
        let (gitdir_token, gitdir_bytes) =
            capture_regular(&review.common_dir, &gitdir_path, 4096, ctx)?;
        if gitdir_token.object_key() != review.gitdir_key {
            return Err(Error::Stale);
        }
        let gitdir_target = admin_gitdir_target(&gitdir_bytes, &current_admin)?;
        if !normalized_path_eq(&gitdir_target, &dotgit_path)? {
            if !normalized_path_eq(&gitdir_target, &review.quarantine_worktree.join(".git"))?
                && !normalized_path_eq(&gitdir_target, &review.original_worktree.join(".git"))?
            {
                return Err(Error::Stale);
            }
            let replacement = gitdir_link(&current_worktree)?;
            replace_file(
                &review.common_dir,
                &gitdir_token,
                &gitdir_bytes,
                &replacement,
                &review.common_dir.join("worktrees"),
                ctx,
            )?;
        }
        let restored_lock = current_admin.join("locked");
        match lock_capture(&review.common_dir, &restored_lock, ctx)? {
            Capture::Present { token, bytes, .. }
                if token.object_key() == review.lock_token.object_key()
                    && bytes == review.lock_bytes =>
            {
                jc_fs::remove_captured_file(&review.common_dir, &token)?;
            }
            _ => return Err(Error::Stale),
        }
        let linked = repo
            .worktree_proxy_by_id(review.id.as_bytes().as_bstr())
            .map_err(crate::engine)?
            .ok_or(Error::Stale)?;
        let opened = linked
            .into_repo_with_possibly_inaccessible_worktree()
            .map_err(crate::engine)?;
        Ok(Worktree {
            id: Some(review.id),
            path: review.original_worktree,
            head: opened.head_id().ok().map(|id| id.to_string()),
            branch: opened
                .head_name()
                .ok()
                .flatten()
                .map(|name| name.to_string()),
            locked: false,
            lock_reason: None,
            prunable: false,
        })
    })
}

impl ResumeCreateReview {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn commit(&self) -> &str {
        &self.commit
    }
    pub fn file_count(&self) -> usize {
        self.checkout.file_count()
    }
    pub fn byte_count(&self) -> usize {
        self.checkout.byte_count()
    }
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

/// Inspect and retain all bounded checkout bytes before presenting a create
/// confirmation. `target` is either a full local branch name or a full object ID.
pub fn prepare_create(
    root: &Path,
    destination: &Path,
    target: &str,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<CreateReview, Error> {
    let destination = destination.to_owned();
    let target = target.to_owned();
    controlled(limits.timeout, cancelled, |ctx| {
        limits.admit()?;
        let (repo, identity) = open(root, ctx)?;
        let parent = destination.parent().ok_or(Error::InvalidRoot)?;
        if !destination.is_absolute() || destination.file_name().is_none() {
            return Err(Error::InvalidRoot);
        }
        let parent = parent.canonicalize()?;
        if !parent.is_dir()
            || destination
                .file_name()
                .is_some_and(|n| n == "." || n == "..")
        {
            return Err(Error::InvalidRoot);
        }
        let destination = parent.join(destination.file_name().ok_or(Error::InvalidRoot)?);
        reject_existing(&destination)?;
        let destination_missing =
            match jc_fs::capture_in_root(&parent, &destination, 0, &mut || ctx.check().is_err())? {
                Capture::Absent { token } if token.is_leaf() => token,
                _ => return Err(Error::Stale),
            };

        let (commit, branch, head) = if target.starts_with("refs/heads/") {
            let name = FullName::try_from(gix::bstr::BString::from(target.clone()))
                .map_err(|_| Error::Unsupported)?;
            let mut reference = repo.find_reference(name.as_ref()).map_err(crate::engine)?;
            let commit = reference.peel_to_commit().map_err(crate::engine)?.id;
            (
                commit,
                Some(target.clone()),
                gix::worktree::add::Head::Attached(name),
            )
        } else {
            let commit = ObjectId::from_hex(target.as_bytes()).map_err(crate::engine)?;
            repo.find_commit(commit).map_err(crate::engine)?;
            (commit, None, gix::worktree::add::Head::Detached(commit))
        };
        let checkout = crate::worktree_materialize::prepare(&repo, commit, limits, ctx)?;
        ctx.check()?;
        Ok(CreateReview {
            identity,
            common_dir: common_dir(&repo)?,
            destination,
            destination_parent: parent,
            destination_missing,
            commit: commit.to_string(),
            branch,
            head,
            checkout,
        })
    })
}

/// Create the reviewed linked worktree. Registration uses gix's create-only
/// reservation and branch-occupancy checks. A visible Git lock marks an
/// interrupted checkout for recovery. Checkout bytes are written by the private
/// bounded materializer, never by a process or external filter.
pub fn apply_create(
    review: CreateReview,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<CreatedWorktree, Error> {
    controlled(Duration::from_secs(30), cancelled, |ctx| {
        let (repo, identity) = open(&review.identity.root, ctx)?;
        if identity.git_dir != review.identity.git_dir || common_dir(&repo)? != review.common_dir {
            return Err(Error::Stale);
        }
        reject_existing(&review.destination)?;
        match jc_fs::capture_in_root(
            &review.destination_parent,
            &review.destination,
            0,
            &mut || ctx.check().is_err(),
        )? {
            Capture::Absent { token } if token == review.destination_missing => {}
            _ => return Err(Error::Stale),
        }
        let current_commit = match &review.head {
            gix::worktree::add::Head::Attached(name) => {
                repo.find_reference(name.as_ref())
                    .map_err(crate::engine)?
                    .peel_to_commit()
                    .map_err(crate::engine)?
                    .id
            }
            gix::worktree::add::Head::Detached(oid) => {
                repo.find_commit(*oid).map_err(crate::engine)?;
                *oid
            }
        };
        if current_commit != ObjectId::from_hex(review.commit.as_bytes()).map_err(crate::engine)? {
            return Err(Error::Stale);
        }
        let repo = with_fallback_committer(repo, &review.identity.root)?;
        let prepared = repo
            .prepare_add_worktree(review.destination.as_path(), review.head.clone(), ctx.stop)
            .map_err(crate::engine)?;
        let linked = prepared.persist().map_err(crate::engine)?;
        let locked_path = linked.git_dir().join("locked");
        let lock_token = match lock_capture(&review.common_dir, &locked_path, ctx)? {
            Capture::Absent { token } => {
                let bytes = create_marker(&review.commit)?;
                jc_fs::stage_missing_file(&review.common_dir, &token, &bytes, &mut || {
                    ctx.check().is_err()
                })?
                .publish()?;
                match lock_capture(&review.common_dir, &locked_path, ctx)? {
                    Capture::Present {
                        token,
                        bytes: found,
                        ..
                    } if found == bytes => token,
                    _ => return Err(Error::Stale),
                }
            }
            Capture::Present { .. } => return Err(Error::Stale),
        };
        review.checkout.resume_or_verify(
            &linked.workdir().ok_or(Error::InvalidRoot)?.canonicalize()?,
            &linked.git_dir().canonicalize()?.join("index"),
            ctx,
        )?;
        ctx.check()?;
        match lock_capture(&review.common_dir, &locked_path, ctx)? {
            Capture::Present { token, bytes, .. }
                if token == lock_token && bytes == create_marker(&review.commit)? =>
            {
                jc_fs::remove_captured_file(&review.common_dir, &lock_token)?;
            }
            _ => return Err(Error::Stale),
        }
        let destination = review.destination.canonicalize()?;
        let id = linked
            .worktrees()
            .map_err(crate::engine)?
            .into_iter()
            .find_map(|proxy| {
                let base = proxy.base().ok()?.canonicalize().ok()?;
                if base == destination {
                    proxy.id().ok()?.to_str().ok().map(str::to_owned)
                } else {
                    None
                }
            })
            .ok_or(Error::Stale)?;
        Ok(CreatedWorktree {
            id,
            path: review.destination,
            commit: review.commit,
            branch: review.branch,
            files: review.checkout.file_count(),
            bytes: review.checkout.byte_count(),
        })
    })
}

/// Prepare recovery only for an exact CIDE incomplete-checkout marker. Arbitrary
/// user worktree locks cannot be reinterpreted or removed as recovery state.
pub fn prepare_resume_create(
    root: &Path,
    id: &str,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<ResumeCreateReview, Error> {
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
        let lock_path = admin_dir.join("locked");
        let (lock_token, lock_bytes) = match lock_capture(&common, &lock_path, ctx)? {
            Capture::Present { token, bytes, .. } if parse_create_marker(&bytes).is_ok() => {
                (token, bytes)
            }
            _ => return Err(Error::Unsupported),
        };
        let linked = proxy
            .into_repo_with_possibly_inaccessible_worktree()
            .map_err(crate::engine)?;
        let commit = linked.head_id().map_err(crate::engine)?.detach();
        if parse_create_marker(&lock_bytes)? != commit {
            return Err(Error::Stale);
        }
        let checkout = crate::worktree_materialize::prepare(&repo, commit, limits, ctx)?;
        ctx.check()?;
        Ok(ResumeCreateReview {
            identity,
            common_dir: common,
            id,
            admin_dir,
            lock_path,
            lock_token,
            lock_bytes,
            commit: commit.to_string(),
            checkout,
        })
    })
}

/// Finish an interrupted create after rechecking the registration, HEAD, lock
/// identity and every already-present checkout byte. Only missing entries are
/// written, then the CIDE marker is removed by captured-file identity.
pub fn apply_resume_create(
    review: ResumeCreateReview,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<CreatedWorktree, Error> {
    controlled(Duration::from_secs(30), cancelled, |ctx| {
        let (repo, identity) = open(&review.identity.root, ctx)?;
        if identity.git_dir != review.identity.git_dir || common_dir(&repo)? != review.common_dir {
            return Err(Error::Stale);
        }
        verify_registration(&repo, &review.common_dir, &review.id, &review.admin_dir)?;
        match lock_capture(&review.common_dir, &review.lock_path, ctx)? {
            Capture::Present { token, bytes, .. }
                if token == review.lock_token && bytes == review.lock_bytes => {}
            _ => return Err(Error::Stale),
        }
        let proxy = repo
            .worktree_proxy_by_id(review.id.as_bytes().as_bstr())
            .map_err(crate::engine)?
            .ok_or(Error::Stale)?;
        let linked = proxy
            .into_repo_with_possibly_inaccessible_worktree()
            .map_err(crate::engine)?;
        let commit = linked.head_id().map_err(crate::engine)?.detach();
        if parse_create_marker(&review.lock_bytes)? != commit
            || commit != ObjectId::from_hex(review.commit.as_bytes()).map_err(crate::engine)?
        {
            return Err(Error::Stale);
        }
        let workdir = linked.workdir().ok_or(Error::InvalidRoot)?.canonicalize()?;
        let admin_dir = review.admin_dir.canonicalize()?;
        review
            .checkout
            .resume_or_verify(&workdir, &admin_dir.join("index"), ctx)?;
        match lock_capture(&review.common_dir, &review.lock_path, ctx)? {
            Capture::Present { token, bytes, .. }
                if token == review.lock_token
                    && bytes == review.lock_bytes
                    && parse_create_marker(&bytes)? == commit =>
            {
                jc_fs::remove_captured_file(&review.common_dir, &review.lock_token)?;
            }
            _ => return Err(Error::Stale),
        }
        Ok(CreatedWorktree {
            id: review.id,
            path: workdir.to_path_buf(),
            commit: review.commit,
            branch: linked
                .head_name()
                .ok()
                .flatten()
                .map(|name| name.to_string()),
            files: review.checkout.file_count(),
            bytes: review.checkout.byte_count(),
        })
    })
}

fn common_dir(repo: &gix::Repository) -> Result<PathBuf, Error> {
    repo.current_dir()
        .join(repo.common_dir())
        .canonicalize()
        .map_err(Error::Io)
}

fn with_fallback_committer(repo: gix::Repository, root: &Path) -> Result<gix::Repository, Error> {
    if repo
        .committer()
        .transpose()
        .map_err(crate::engine)?
        .is_some()
    {
        return Ok(repo);
    }
    let options = repo
        .open_options()
        .clone()
        .cli_overrides(["user.name=CIDE", "user.email=cide@localhost"]);
    gix::open_opts(root, options).map_err(crate::engine)
}

fn path_text(path: &Path) -> Result<String, Error> {
    let text = path.to_str().ok_or(Error::Unsupported)?;
    if text.len() > PATH_BYTES
        || text
            .bytes()
            .any(|byte| byte == 0 || byte == b'\n' || byte == b'\r')
    {
        return Err(Error::Unsupported);
    }
    Ok(text.to_owned())
}

fn key_hex(key: [u8; 12]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(24);
    for byte in key {
        value.push(HEX[(byte >> 4) as usize] as char);
        value.push(HEX[(byte & 0x0f) as usize] as char);
    }
    value
}

fn parse_key(value: &str) -> Result<[u8; 12], Error> {
    if value.len() != 24 {
        return Err(Error::Unsupported);
    }
    let mut key = [0; 12];
    for (index, byte) in key.iter_mut().enumerate() {
        let digit = |value: u8| match value {
            b'0'..=b'9' => Some(value - b'0'),
            b'a'..=b'f' => Some(value - b'a' + 10),
            _ => None,
        };
        let high = digit(value.as_bytes()[index * 2]).ok_or(Error::Unsupported)?;
        let low = digit(value.as_bytes()[index * 2 + 1]).ok_or(Error::Unsupported)?;
        *byte = (high << 4) | low;
    }
    Ok(key)
}

fn make_move_marker(
    source: &Path,
    destination: &Path,
    source_key: [u8; 12],
    next_source_key: [u8; 12],
    admin_key: [u8; 12],
) -> Result<Vec<u8>, Error> {
    let marker = format!(
        "CIDE-MOVE-3\n{}\n{}\n{}\n{}\n{}\n",
        path_text(source)?,
        path_text(destination)?,
        key_hex(source_key),
        key_hex(next_source_key),
        key_hex(admin_key),
    )
    .into_bytes();
    if marker.len() > LOCK_BYTES {
        return Err(Error::Limited);
    }
    Ok(marker)
}

fn journal_keys(bytes: &[u8]) -> Result<MoveKeys, Error> {
    let text = std::str::from_utf8(bytes).map_err(crate::engine)?;
    let mut lines = text.lines();
    let version = lines.next().ok_or(Error::Unsupported)?;
    if version != "CIDE-MOVE-2" && version != "CIDE-MOVE-3" {
        return Err(Error::Unsupported);
    }
    let _source = lines.next().ok_or(Error::Unsupported)?;
    let _destination = lines.next().ok_or(Error::Unsupported)?;
    let source = parse_key(lines.next().ok_or(Error::Unsupported)?)?;
    let next_source = if version == "CIDE-MOVE-3" {
        parse_key(lines.next().ok_or(Error::Unsupported)?)?
    } else {
        source
    };
    let admin = parse_key(lines.next().ok_or(Error::Unsupported)?)?;
    if lines.next().is_some() {
        return Err(Error::Unsupported);
    }
    Ok(MoveKeys {
        source,
        next_source,
        admin,
    })
}

fn create_marker(commit: &str) -> Result<Vec<u8>, Error> {
    ObjectId::from_hex(commit.as_bytes()).map_err(crate::engine)?;
    if !is_canonical_oid(commit) {
        return Err(Error::Unsupported);
    }
    Ok(format!("CIDE-CREATE-1\n{commit}\n").into_bytes())
}

fn parse_create_marker(bytes: &[u8]) -> Result<ObjectId, Error> {
    let text = std::str::from_utf8(bytes).map_err(crate::engine)?;
    let mut lines = text.lines();
    if lines.next() != Some("CIDE-CREATE-1") {
        return Err(Error::Unsupported);
    }
    let value = lines.next().ok_or(Error::Unsupported)?;
    if lines.next().is_some() {
        return Err(Error::Unsupported);
    }
    let oid = ObjectId::from_hex(value.as_bytes()).map_err(crate::engine)?;
    if !is_canonical_oid(value) {
        return Err(Error::Unsupported);
    }
    Ok(oid)
}

fn is_canonical_oid(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn parse_retire_marker(bytes: &[u8]) -> Result<RetireMarker, Error> {
    let text = std::str::from_utf8(bytes).map_err(crate::engine)?;
    let mut lines = text.lines();
    if lines.next() != Some("CIDE-RETIRE-1") {
        return Err(Error::Unsupported);
    }
    let original_worktree = PathBuf::from(lines.next().ok_or(Error::Unsupported)?);
    let quarantine_worktree = PathBuf::from(lines.next().ok_or(Error::Unsupported)?);
    let original_admin = PathBuf::from(lines.next().ok_or(Error::Unsupported)?);
    let quarantine_admin = PathBuf::from(lines.next().ok_or(Error::Unsupported)?);
    let id = lines.next().ok_or(Error::Unsupported)?.to_owned();
    if lines.next().is_some()
        || !original_worktree.is_absolute()
        || !quarantine_worktree.is_absolute()
        || !original_admin.is_absolute()
        || !quarantine_admin.is_absolute()
    {
        return Err(Error::Unsupported);
    }
    let (dotgit, gitdir) = id.split_once(':').ok_or(Error::Unsupported)?;
    let id = original_admin
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(Error::Unsupported)?
        .to_owned();
    Ok(RetireMarker {
        original_worktree,
        quarantine_worktree,
        id,
        original_admin,
        quarantine_admin,
        dotgit_key: parse_key(dotgit)?,
        gitdir_key: parse_key(gitdir)?,
    })
}

fn gitdir_link(worktree: &Path) -> Result<Vec<u8>, Error> {
    let gitdir = worktree.join(".git");
    let text = git_path_text(&gitdir)?;
    Ok(format!("{text}\n").into_bytes())
}

fn gitfile_link(admin_dir: &Path) -> Result<Vec<u8>, Error> {
    let text = git_path_text(admin_dir)?;
    Ok(format!("gitdir: {text}\n").into_bytes())
}

fn git_path_text(path: &Path) -> Result<String, Error> {
    let text = path.to_str().ok_or(Error::Unsupported)?;
    #[cfg(windows)]
    let text = {
        let lower = text.to_ascii_lowercase();
        if lower.starts_with("\\\\?\\unc\\") {
            format!("\\\\{}", &text[8..])
        } else if lower.starts_with("\\\\?\\")
            && text.as_bytes().get(4).is_some_and(u8::is_ascii_alphabetic)
            && text.as_bytes().get(5) == Some(&b':')
        {
            text[4..].to_owned()
        } else {
            text.to_owned()
        }
    };
    #[cfg(not(windows))]
    let text = text.to_owned();
    Ok(text.replace('\\', "/"))
}

fn replace_error(error: jc_fs::ReplaceError) -> Error {
    match error {
        jc_fs::ReplaceError::Io(error) => Error::Io(error),
        jc_fs::ReplaceError::Preserved { error, backup } => Error::Engine(format!(
            "No se actualizó el enlace Git ({error}); el archivo anterior quedó respaldado en {}",
            backup.display()
        )),
    }
}

fn capture_regular(
    root: &Path,
    path: &Path,
    cap: usize,
    ctx: &Context<'_>,
) -> Result<(FileToken, Vec<u8>), Error> {
    match jc_fs::capture_in_root(root, path, cap, &mut || ctx.check().is_err())? {
        Capture::Present { token, bytes, .. } if token.has_single_link() => Ok((token, bytes)),
        _ => Err(Error::Stale),
    }
}

fn missing_location(root: &Path, path: &Path, ctx: &Context<'_>) -> Result<MissingToken, Error> {
    match jc_fs::capture_in_root(root, path, 0, &mut || ctx.check().is_err())? {
        Capture::Absent { token } => Ok(token),
        Capture::Present { .. } => Err(Error::Stale),
    }
}

fn reject_existing(path: &Path) -> Result<(), Error> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Err(Error::Stale),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn create_parents_if_needed(
    root: &Path,
    path: &Path,
    expected: &MissingToken,
    ctx: &Context<'_>,
) -> Result<(), Error> {
    if !expected.is_leaf() {
        jc_fs::create_missing_parents(root, expected, &mut || ctx.check().is_err())?;
    }
    let current = missing_location(root, path, ctx)?;
    if !expected.is_compatible_after_creating_parents(&current) || !current.is_leaf() {
        return Err(Error::Stale);
    }
    Ok(())
}

fn link_target(bytes: &[u8], relative_to: &Path) -> Result<PathBuf, Error> {
    let value = std::str::from_utf8(bytes)
        .map_err(crate::engine)?
        .strip_prefix("gitdir: ")
        .ok_or(Error::Unsupported)?
        .trim_end_matches(['\r', '\n']);
    if value.is_empty() || value.contains(['\n', '\r', '\0']) {
        return Err(Error::Unsupported);
    }
    let path = PathBuf::from(value);
    let path = if path.is_absolute() {
        path
    } else {
        relative_to.join(path)
    };
    normalize_absolute(&path)
}

fn admin_gitdir_target(bytes: &[u8], relative_to: &Path) -> Result<PathBuf, Error> {
    let value = std::str::from_utf8(bytes)
        .map_err(crate::engine)?
        .trim_end_matches(['\r', '\n']);
    if value.is_empty() || value.contains(['\n', '\r', '\0']) {
        return Err(Error::Unsupported);
    }
    // Older CIDE operations wrote the checkout's `gitdir: ` prefix into the
    // admin backlink; accept that form only while reading/recovering it.
    if value.starts_with("gitdir: ") {
        return link_target(bytes, relative_to);
    }
    let path = PathBuf::from(value);
    let path = if path.is_absolute() {
        path
    } else {
        relative_to.join(path)
    };
    normalize_absolute(&path)
}

fn normalize_absolute(path: &Path) -> Result<PathBuf, Error> {
    if !path.is_absolute() {
        return Err(Error::InvalidRoot);
    }
    #[cfg(windows)]
    let path = {
        let text = path.to_str().ok_or(Error::Unsupported)?.replace('/', "\\");
        let lower = text.to_ascii_lowercase();
        if lower.starts_with("\\\\?\\unc\\") {
            PathBuf::from(format!("\\\\{}", &text[8..]))
        } else if (lower.starts_with("\\\\?\\") || lower.starts_with("\\??\\"))
            && text.as_bytes().get(4).is_some_and(u8::is_ascii_alphabetic)
            && text.as_bytes().get(5) == Some(&b':')
        {
            PathBuf::from(&text[4..])
        } else {
            PathBuf::from(text)
        }
    };
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            std::path::Component::RootDir => normalized.push(component.as_os_str()),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::Normal(part) => normalized.push(part),
        }
    }
    let mut probe = normalized.clone();
    let mut suffix = Vec::new();
    loop {
        match std::fs::canonicalize(&probe) {
            Ok(mut canonical) => {
                for component in suffix.iter().rev() {
                    canonical.push(component);
                }
                return Ok(canonical);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let name = probe.file_name().ok_or(Error::InvalidRoot)?.to_os_string();
                suffix.push(name);
                if !probe.pop() {
                    return Err(Error::InvalidRoot);
                }
            }
            Err(error) => return Err(error.into()),
        }
    }
}

fn normalized_path_eq(left: &Path, right: &Path) -> Result<bool, Error> {
    Ok(normalize_absolute(left)? == normalize_absolute(right)?)
}

fn replace_file(
    root: &Path,
    token: &FileToken,
    before: &[u8],
    after: &[u8],
    backup: &Path,
    ctx: &Context<'_>,
) -> Result<(), Error> {
    let expected_key = token.object_key();
    let pending = jc_fs::stage_existing_file(root, token, before, after, backup, &mut || {
        ctx.check().is_err()
    })?;
    let backup_file = pending.publish().map_err(replace_error)?;
    cleanup_backup(backup, &backup_file, expected_key, before, ctx)
}

fn cleanup_backup(
    backup_dir: &Path,
    backup_file: &Path,
    expected_key: [u8; 12],
    expected: &[u8],
    ctx: &Context<'_>,
) -> Result<(), Error> {
    match jc_fs::capture_in_root(backup_dir, backup_file, expected.len(), &mut || {
        ctx.check().is_err()
    })? {
        Capture::Present { token, bytes, .. }
            if token.has_single_link()
                && token.object_key() == expected_key
                && bytes == expected =>
        {
            jc_fs::remove_captured_file(backup_dir, &token)?;
            Ok(())
        }
        _ => Err(Error::Stale),
    }
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
                let reason = if bytes.starts_with(b"CIDE-MOVE-")
                    || bytes.starts_with(b"CIDE-RETIRE-")
                    || bytes.starts_with(b"CIDE-CREATE-")
                {
                    "Operación de worktree interrumpida; requiere recuperación".to_owned()
                } else {
                    if bytes.len() > REASON_BYTES + 1 {
                        return Err(Error::Unsupported);
                    }
                    std::str::from_utf8(&bytes)
                        .map_err(|_| Error::Unsupported)?
                        .trim_end_matches(['\r', '\n'])
                        .to_owned()
                };
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

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    struct Fixture {
        root: PathBuf,
        parent: PathBuf,
        extra: Vec<PathBuf>,
    }

    impl Fixture {
        fn new() -> Self {
            let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .nth(2)
                .unwrap()
                .canonicalize()
                .unwrap();
            let test_root = workspace.join("work/test-temp");
            fs::create_dir_all(&test_root).unwrap();
            let test_root = test_root.canonicalize().unwrap();
            assert!(test_root.starts_with(&workspace));
            let parent = test_root.join(format!(
                "git-worktrees-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir(&parent).unwrap();
            let parent = parent.canonicalize().unwrap();
            assert!(parent.starts_with(&test_root));
            let root = parent.join("repository");
            fs::create_dir(&root).unwrap();
            Self {
                root,
                parent,
                extra: Vec::new(),
            }
        }

        fn own(&mut self, path: PathBuf) {
            self.extra.push(path);
        }

        fn create_commit(&self) -> ObjectId {
            let repo = gix::init(&self.root).unwrap();
            let blob = repo.write_blob(b"fixture contents\n").unwrap().detach();
            let tree = repo
                .write_object(gix::objs::Tree {
                    entries: vec![gix::objs::tree::Entry {
                        filename: b"fixture.txt".to_vec().into(),
                        mode: 0o100644u32.try_into().unwrap(),
                        oid: blob,
                    }],
                })
                .unwrap()
                .detach();
            let signature = gix::actor::SignatureRef {
                name: b"CIDE fixture".as_bstr(),
                email: b"fixture@example.invalid".as_bstr(),
                time: "1 +0000",
            };
            repo.commit_as(
                signature,
                signature,
                "HEAD",
                "fixture",
                tree,
                std::iter::empty::<ObjectId>(),
            )
            .unwrap()
            .detach()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            assert_eq!(self.root.parent(), Some(self.parent.as_path()));
            assert_eq!(
                self.parent.parent().unwrap().file_name().unwrap(),
                "test-temp"
            );
            assert_eq!(self.root.canonicalize().unwrap(), self.root);
            fn remove_owned(path: &Path) {
                if !path.exists() {
                    return;
                }
                let meta = fs::symlink_metadata(path).unwrap();
                assert!(!meta.file_type().is_symlink());
                if meta.is_dir() {
                    for entry in fs::read_dir(path).unwrap() {
                        remove_owned(&entry.unwrap().path());
                    }
                    fs::remove_dir(path).unwrap();
                } else {
                    fs::remove_file(path).unwrap();
                }
            }
            for path in self.extra.iter().rev() {
                assert!(path.starts_with(&self.parent));
                remove_owned(path);
            }
            remove_owned(&self.root);
            fs::remove_dir(&self.parent).unwrap();
        }
    }

    fn limits() -> Limits {
        Limits {
            max_blob: 1024 * 1024,
            max_entries: 128,
            max_path: 4096,
            max_bytes: 2 * 1024 * 1024,
            timeout: Duration::from_secs(10),
        }
    }

    #[test]
    fn create_move_retire_restore_preserves_checkout_bytes() {
        let mut fixture = Fixture::new();
        let commit = fixture.create_commit();
        let first = fixture.parent.join(format!("wt-a-{}", std::process::id()));
        fixture.own(first.clone());
        fixture.own(fixture.parent.join(".cide-worktree-quarantine"));
        let review = prepare_create(&fixture.root, &first, &commit.to_string(), limits(), || {
            false
        })
        .unwrap();
        assert_eq!(review.file_count(), 1);
        let created = apply_create(review, || false).unwrap();
        assert_eq!(
            fs::read(first.join("fixture.txt")).unwrap(),
            b"fixture contents\n"
        );

        let moved = fixture.parent.join(format!("wt-b-{}", std::process::id()));
        fixture.own(moved.clone());
        let moved_worktree = apply_move(
            prepare_move(&fixture.root, &created.id, &moved, limits(), || false).unwrap(),
            || false,
        )
        .unwrap();
        assert_eq!(moved_worktree.path, moved);
        assert!(!first.exists());
        assert_eq!(
            fs::read(moved.join("fixture.txt")).unwrap(),
            b"fixture contents\n"
        );

        let retired = apply_retire(
            prepare_retire(&fixture.root, &created.id, limits(), || false).unwrap(),
            || false,
        )
        .unwrap();
        assert!(retired.preserved_files);
        assert!(fs::read(retired.worktree.join("fixture.txt")).is_ok());
        assert!(
            list(&fixture.root, limits(), || false)
                .unwrap()
                .worktrees
                .iter()
                .all(|item| item.id.as_deref() != Some(created.id.as_str()))
        );

        let restored = apply_restore_retired(
            prepare_restore_retired(&fixture.root, &created.id, limits(), || false).unwrap(),
            || false,
        )
        .unwrap();
        assert_eq!(restored.path, moved);
        assert_eq!(
            fs::read(moved.join("fixture.txt")).unwrap(),
            b"fixture contents\n"
        );
        assert!(
            list(&fixture.root, limits(), || false)
                .unwrap()
                .worktrees
                .iter()
                .any(|item| item.id.as_deref() == Some(created.id.as_str()))
        );
    }

    #[test]
    fn create_and_move_reject_stale_destinations_without_touching_source() {
        let mut fixture = Fixture::new();
        let commit = fixture.create_commit();
        let create_path = fixture
            .parent
            .join(format!("wt-stale-create-{}", std::process::id()));
        fixture.own(create_path.clone());
        let review = prepare_create(
            &fixture.root,
            &create_path,
            &commit.to_string(),
            limits(),
            || false,
        )
        .unwrap();
        fs::create_dir(&create_path).unwrap();
        assert!(matches!(apply_create(review, || false), Err(Error::Stale)));
        assert!(fs::read_dir(&create_path).unwrap().next().is_none());
        fs::remove_dir(&create_path).unwrap();

        let source = fixture
            .parent
            .join(format!("wt-source-{}", std::process::id()));
        fixture.own(source.clone());
        let created = apply_create(
            prepare_create(
                &fixture.root,
                &source,
                &commit.to_string(),
                limits(),
                || false,
            )
            .unwrap(),
            || false,
        )
        .unwrap();
        let destination = fixture
            .parent
            .join(format!("wt-stale-move-{}", std::process::id()));
        fixture.own(destination.clone());
        let review =
            prepare_move(&fixture.root, &created.id, &destination, limits(), || false).unwrap();
        fs::create_dir(&destination).unwrap();
        fs::write(destination.join("user.txt"), b"keep").unwrap();
        assert!(matches!(apply_move(review, || false), Err(Error::Stale)));
        assert_eq!(
            fs::read(source.join("fixture.txt")).unwrap(),
            b"fixture contents\n"
        );
        assert_eq!(fs::read(destination.join("user.txt")).unwrap(), b"keep");
    }

    #[test]
    fn interrupted_attached_create_rejects_a_branch_that_moved() {
        use gix::refs::Target;
        use gix::refs::transaction::{PreviousValue, RefEdit};
        use std::sync::atomic::AtomicBool;

        let mut fixture = Fixture::new();
        let initial = fixture.create_commit();
        let feature =
            gix::refs::FullName::try_from(gix::bstr::BString::from("refs/heads/cide-feature"))
                .unwrap();
        let mut repo = gix::open(&fixture.root).unwrap();
        repo.refs.write_reflog = gix::refs::store::WriteReflog::Disable;
        repo.edit_references_as(
            [RefEdit::update(
                feature.clone(),
                Target::Object(initial),
                PreviousValue::MustNotExist,
                String::new(),
            )],
            None,
        )
        .unwrap();
        let worktree = fixture
            .parent
            .join(format!("wt-incomplete-{}", std::process::id()));
        fixture.own(worktree.clone());
        let mut repo = with_fallback_committer(repo, &fixture.root).unwrap();
        repo.refs.write_reflog = gix::refs::store::WriteReflog::Disable;
        let prepared = repo
            .prepare_add_worktree(
                &worktree,
                gix::worktree::add::Head::Attached(feature.clone()),
                &AtomicBool::new(false),
            )
            .unwrap();
        let linked = prepared.persist().unwrap();
        fs::write(
            linked.git_dir().join("locked"),
            create_marker(&initial.to_string()).unwrap(),
        )
        .unwrap();
        let tree = repo
            .write_object(gix::objs::Tree {
                entries: Vec::new(),
            })
            .unwrap()
            .detach();
        let signature = gix::actor::SignatureRef {
            name: b"CIDE fixture".as_bstr(),
            email: b"fixture@example.invalid".as_bstr(),
            time: "2 +0000",
        };
        let next = repo
            .write_object(gix::objs::Commit {
                tree,
                parents: vec![initial].into(),
                author: signature.into(),
                committer: signature.into(),
                message: b"next".into(),
                encoding: None,
                extra_headers: Vec::new(),
            })
            .unwrap()
            .detach();
        repo.edit_references_as(
            [RefEdit::update(
                feature,
                Target::Object(next),
                PreviousValue::MustExistAndMatch(Target::Object(initial)),
                String::new(),
            )],
            None,
        )
        .unwrap();
        let id = linked
            .git_dir()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        assert!(matches!(
            prepare_resume_create(&fixture.root, &id, limits(), || false),
            Err(Error::Stale)
        ));
        assert!(!worktree.join("fixture.txt").exists());
    }
}
