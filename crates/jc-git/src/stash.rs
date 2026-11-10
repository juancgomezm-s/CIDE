//! Reviewed, embedded stash operations. Stashes use ordinary Git commit/tree
//! objects and the canonical `refs/stash` reflog; no external Git process is used.
use crate::{
    Change, Context, Error, Identity, Limits, controlled, engine, index_write, local_merge,
    local_pair, revision,
};
use gix::{
    bstr::{BString, ByteSlice},
    lock::{File, Marker, acquire::Fail},
    objs::{Commit, Kind, Tree, WriteTo, tree::EntryKind},
    refs::{
        FullName, Target,
        store::WriteReflog,
        transaction::{PreviousValue, RefEdit},
    },
};
use jc_fs::Capture;
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicU8},
    time::Instant,
};

const STASH_REF: &str = "refs/stash";
const STASH_LOG: &str = "logs/refs/stash";
const STASH_LOG_BYTES: usize = 4 * 1024 * 1024;
const MAX_STASHES: usize = 1024;
type Entries = BTreeMap<Vec<u8>, revision::Entry>;

struct WorktreeSnapshot {
    entries: Entries,
    blobs: Vec<(gix::ObjectId, Vec<u8>)>,
    pins: Vec<(
        Vec<u8>,
        Option<jc_fs::FileToken>,
        Option<jc_fs::MissingToken>,
    )>,
    paths: Vec<Vec<u8>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StashSelector {
    /// Zero is the newest entry (`stash@{0}`).
    pub index: usize,
    pub oid: gix::ObjectId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StashEntry {
    pub selector: StashSelector,
    pub message: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum RefPin {
    Present(jc_fs::FileToken),
    Missing(jc_fs::MissingToken),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LogPin {
    token: jc_fs::FileToken,
    bytes: Vec<u8>,
}

#[derive(Clone)]
struct Snapshot {
    repository: Identity,
    head: gix::ObjectId,
    branch: FullName,
    head_file: jc_fs::FileToken,
    index: local_pair::IndexToken,
    stash_ref: Option<gix::ObjectId>,
    ref_pin: RefPin,
    log: Option<LogPin>,
    entries: Vec<StashEntry>,
}

pub struct StashList {
    pub entries: Vec<StashEntry>,
    pub incomplete: bool,
}

pub struct StashPushPlan {
    snapshot: Snapshot,
    message: String,
    stash: gix::ObjectId,
    index_commit: Commit,
    stash_commit: Commit,
    blobs: Vec<(gix::ObjectId, Vec<u8>)>,
    trees: Vec<(gix::ObjectId, Tree)>,
    paths: Vec<Vec<u8>>,
    worktree_pins: Vec<(
        Vec<u8>,
        Option<jc_fs::FileToken>,
        Option<jc_fs::MissingToken>,
    )>,
    reset: local_merge::MergePlan,
    limits: Limits,
}

pub struct StashPushResult {
    pub oid: gix::ObjectId,
    pub paths: Vec<Vec<u8>>,
    pub workspace_clean: bool,
    pub warning: Option<String>,
    pub hard_backup: Option<PathBuf>,
}

pub struct StashApplyPlan {
    selector: StashSelector,
    repository: Identity,
    root: PathBuf,
    log_token: jc_fs::FileToken,
    merge: local_merge::MergePlan,
    limits: Limits,
}

pub struct StashDropPlan {
    selector: StashSelector,
    snapshot: Snapshot,
    remaining: Vec<u8>,
    next: Option<gix::ObjectId>,
    limits: Limits,
}

pub struct StashPopPlan {
    apply: StashApplyPlan,
}

#[derive(Clone)]
pub struct StashApplyResolution {
    selector: StashSelector,
    repository: Identity,
    root: PathBuf,
    log_token: jc_fs::FileToken,
    review: local_merge::MergeReview,
    limits: Limits,
}

#[derive(Debug)]
pub struct StashApplyResult {
    pub oid: gix::ObjectId,
    pub paths: Vec<Vec<u8>>,
    pub warning: Option<String>,
}

#[derive(Debug)]
pub struct StashPopResult {
    pub applied: StashApplyResult,
    pub dropped: bool,
    pub warning: Option<String>,
}

impl StashPushPlan {
    pub fn stash_oid(&self) -> gix::ObjectId {
        self.stash
    }
    pub fn paths(&self) -> impl Iterator<Item = &[u8]> {
        self.paths.iter().map(Vec::as_slice)
    }
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl StashApplyPlan {
    pub fn stash_oid(&self) -> gix::ObjectId {
        self.selector.oid
    }
    pub fn selector(&self) -> StashSelector {
        self.selector
    }
    pub fn paths(&self) -> impl Iterator<Item = &[u8]> {
        self.merge.rows.iter().map(|row| row.path.as_slice())
    }
    pub fn rows(&self) -> &[local_merge::MergeRow] {
        &self.merge.rows
    }
    pub fn merge_plan(&self) -> &local_merge::MergePlan {
        &self.merge
    }
    pub fn can_apply(&self) -> bool {
        self.merge.can_apply()
    }
    pub fn resolution_review(&self) -> Option<local_merge::MergeReview> {
        self.merge.resolution_review()
    }
    pub fn resolution_token(&self) -> Option<StashApplyResolution> {
        Some(StashApplyResolution {
            selector: self.selector,
            repository: self.repository.clone(),
            root: self.root.clone(),
            log_token: self.log_token.clone(),
            review: self.merge.resolution_review()?,
            limits: self.limits,
        })
    }
    pub fn with_resolutions(
        &self,
        choices: BTreeMap<Vec<u8>, local_merge::MergeResolution>,
        cancelled: impl FnMut() -> bool + Send,
    ) -> Result<Self, Error> {
        let review = self.merge.resolution_review().ok_or(Error::Stale)?;
        let merge = local_merge::prepare_resolved_merge(review, choices, cancelled)?;
        Ok(Self {
            selector: self.selector,
            repository: self.repository.clone(),
            root: self.root.clone(),
            log_token: self.log_token.clone(),
            merge,
            limits: self.limits,
        })
    }
}

impl StashDropPlan {
    pub fn stash_oid(&self) -> gix::ObjectId {
        self.selector.oid
    }
    pub fn selector(&self) -> StashSelector {
        self.selector
    }
}

impl StashPopPlan {
    pub fn stash_oid(&self) -> gix::ObjectId {
        self.apply.stash_oid()
    }
    pub fn selector(&self) -> StashSelector {
        self.apply.selector()
    }
    pub fn paths(&self) -> impl Iterator<Item = &[u8]> {
        self.apply.paths()
    }
    pub fn rows(&self) -> &[local_merge::MergeRow] {
        self.apply.rows()
    }
    pub fn can_apply(&self) -> bool {
        self.apply.can_apply()
    }
    pub fn merge_plan(&self) -> &local_merge::MergePlan {
        self.apply.merge_plan()
    }
    pub fn resolution_review(&self) -> Option<local_merge::MergeReview> {
        self.apply.resolution_review()
    }
    pub fn resolution_token(&self) -> Option<StashApplyResolution> {
        self.apply.resolution_token()
    }
    pub fn from_resolved(apply: StashApplyPlan) -> Self {
        Self { apply }
    }
}

impl StashApplyResolution {
    pub fn stash_oid(&self) -> gix::ObjectId {
        self.selector.oid
    }
    pub fn prepare(
        &self,
        choices: BTreeMap<Vec<u8>, local_merge::MergeResolution>,
        cancelled: impl FnMut() -> bool + Send,
    ) -> Result<StashApplyPlan, Error> {
        let merge = local_merge::prepare_resolved_merge(self.review.clone(), choices, cancelled)?;
        Ok(StashApplyPlan {
            selector: self.selector,
            repository: self.repository.clone(),
            root: self.root.clone(),
            log_token: self.log_token.clone(),
            merge,
            limits: self.limits,
        })
    }
}

fn capture(root: &Path, path: &Path, cap: usize, ctx: &Context<'_>) -> Result<Capture, Error> {
    local_pair::capture(root, path, cap, ctx)
}

fn capture_ref(git: &Path, ctx: &Context<'_>) -> Result<RefPin, Error> {
    match capture(git, &git.join(STASH_REF), 1024, ctx)? {
        Capture::Present { token, .. } => Ok(RefPin::Present(token)),
        Capture::Absent { token } => Ok(RefPin::Missing(token)),
    }
}

fn ref_id(repo: &gix::Repository) -> Result<Option<gix::ObjectId>, Error> {
    let Some(reference) = repo.try_find_reference(STASH_REF).map_err(engine)? else {
        return Ok(None);
    };
    match reference.target() {
        gix::refs::TargetRef::Object(id) if !id.is_null() => Ok(Some(id.to_owned())),
        _ => Err(Error::Unsupported),
    }
}

fn parse_log(
    bytes: &[u8],
    head: Option<gix::ObjectId>,
    limits: Limits,
) -> Result<Vec<StashEntry>, Error> {
    if bytes.is_empty() {
        return if head.is_none() {
            Ok(Vec::new())
        } else {
            Err(Error::Stale)
        };
    }
    if !bytes.ends_with(b"\n") || head.is_none() {
        return Err(Error::Unsupported);
    }
    let head = head.ok_or(Error::Stale)?;
    let lines = bytes[..bytes.len() - 1]
        .split(|b| *b == b'\n')
        .collect::<Vec<_>>();
    if lines.len() > MAX_STASHES.min(limits.max_entries) {
        return Err(Error::Limited);
    }
    let mut entries = Vec::with_capacity(lines.len());
    for (index, raw) in lines.iter().rev().enumerate() {
        let line = gix::refs::file::log::LineRef::from_bytes(raw).map_err(engine)?;
        let oid = line.new_oid();
        if oid.is_null() || oid.kind() != head.kind() || (index == 0 && oid != head) {
            return Err(Error::Stale);
        }
        entries.push(StashEntry {
            selector: StashSelector { index, oid },
            message: line.message.to_vec(),
        });
    }
    Ok(entries)
}

fn snapshot(
    root: &Path,
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<(gix::Repository, Snapshot), Error> {
    let (repo, repository) = crate::branch_switch::open_for_switch(root, ctx)?;
    if index_write::exists(&repository.git_dir.join("worktrees"))? {
        return Err(Error::Unsupported);
    }
    let head = repo.head().map_err(engine)?;
    let branch = head.referent_name().ok_or(Error::Unsupported)?.to_owned();
    if !branch.as_bstr().starts_with(b"refs/heads/") {
        return Err(Error::Unsupported);
    }
    let head_id = head.id().map(|id| id.detach()).ok_or(Error::Missing)?;
    let head_file = crate::branch_switch::capture_head(&repository.git_dir, &branch, ctx)?;
    let (index, _) = local_pair::index_capture(&repo, &repository.git_dir, ctx)?;
    let stash_ref = ref_id(&repo)?;
    let ref_pin = capture_ref(&repository.git_dir, ctx)?;
    let log = match capture(
        &repository.git_dir,
        &repository.git_dir.join(STASH_LOG),
        STASH_LOG_BYTES.min(limits.max_bytes),
        ctx,
    )? {
        Capture::Present { token, bytes, .. } => Some(LogPin { token, bytes }),
        Capture::Absent { .. } => None,
    };
    let entries = match (&log, stash_ref) {
        (Some(log), Some(id)) => parse_log(&log.bytes, Some(id), limits)?,
        (Some(log), None) if log.bytes.is_empty() => Vec::new(),
        (None, None) => Vec::new(),
        _ => return Err(Error::Stale),
    };
    Ok((
        repo,
        Snapshot {
            repository,
            head: head_id,
            branch,
            head_file,
            index,
            stash_ref,
            ref_pin,
            log,
            entries,
        },
    ))
}

/// Read the current stash reflog with a stable ordinal and commit ID for every row.
pub fn list(
    root: &Path,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<StashList, Error> {
    limits.admit()?;
    controlled(limits.timeout, cancelled, |ctx| {
        let (_, snapshot) = snapshot(root, limits, ctx)?;
        Ok(StashList {
            entries: snapshot.entries,
            incomplete: false,
        })
    })
}

fn select(snapshot: &Snapshot, selector: StashSelector) -> Result<(), Error> {
    if selector.index >= snapshot.entries.len()
        || snapshot.entries[selector.index].selector.oid != selector.oid
        || snapshot
            .entries
            .iter()
            .filter(|entry| entry.selector.oid == selector.oid)
            .count()
            != 1
    {
        return Err(Error::Stale);
    }
    Ok(())
}

fn object_commit(
    repo: &gix::Repository,
    commit: &Commit,
) -> Result<(gix::ObjectId, Vec<u8>), Error> {
    let mut bytes = Vec::new();
    commit.write_to(&mut bytes)?;
    let id = gix::objs::compute_hash(repo.object_hash(), Kind::Commit, &bytes).map_err(engine)?;
    Ok((id, bytes))
}

fn build_tree(
    repo: &gix::Repository,
    entries: &Entries,
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<(gix::ObjectId, Vec<(gix::ObjectId, Tree)>), Error> {
    let mut editor =
        gix::objs::tree::Editor::new(Tree::default(), &repo.objects, repo.object_hash());
    for (path, entry) in entries {
        ctx.check()?;
        let kind = match entry.mode {
            0o100644 => EntryKind::Blob,
            0o100755 => EntryKind::BlobExecutable,
            0o120000 => EntryKind::Link,
            _ => return Err(Error::Unsupported),
        };
        editor
            .upsert(
                path.split(|b| *b == b'/').map(ByteSlice::as_bstr),
                kind,
                entry.id,
            )
            .map_err(engine)?;
    }
    let mut trees = Vec::new();
    let mut budget = revision::Budget::new(limits);
    let id = editor
        .write(|tree| {
            ctx.check()?;
            let mut bytes = Vec::new();
            tree.write_to(&mut bytes)?;
            budget.charge(bytes.len())?;
            let id =
                gix::objs::compute_hash(repo.object_hash(), Kind::Tree, &bytes).map_err(engine)?;
            trees.push((id, tree.clone()));
            Ok::<_, Error>(id)
        })
        .map_err(engine)?;
    Ok((id, trees))
}

fn pin_matches(snapshot: &Snapshot, ctx: &Context<'_>) -> Result<(), Error> {
    let (repo, identity) = crate::branch_switch::open_for_switch(&snapshot.repository.root, ctx)?;
    index_write::same_repository(&snapshot.repository, &identity)?;
    let head = repo.head().map_err(engine)?;
    if head.referent_name().map(|name| name.as_bstr()) != Some(snapshot.branch.as_bstr())
        || head.id().map(|id| id.detach()) != Some(snapshot.head)
        || crate::branch_switch::capture_head(&identity.git_dir, &snapshot.branch, ctx)?
            != snapshot.head_file
    {
        return Err(Error::Stale);
    }
    if local_pair::index_capture(&repo, &identity.git_dir, ctx)?.0 != snapshot.index
        || ref_id(&repo)? != snapshot.stash_ref
        || capture_ref(&identity.git_dir, ctx)? != snapshot.ref_pin
    {
        return Err(Error::Stale);
    }
    let current_log = match capture(
        &identity.git_dir,
        &identity.git_dir.join(STASH_LOG),
        STASH_LOG_BYTES,
        ctx,
    )? {
        Capture::Present { token, bytes, .. } => Some(LogPin { token, bytes }),
        Capture::Absent { .. } => None,
    };
    if current_log.as_ref().map(|log| &log.token) != snapshot.log.as_ref().map(|log| &log.token)
        || current_log.as_ref().map(|log| &log.bytes) != snapshot.log.as_ref().map(|log| &log.bytes)
    {
        return Err(Error::Stale);
    }
    Ok(())
}

fn make_tree_commit(
    repo: &gix::Repository,
    tree: gix::ObjectId,
    parents: Vec<gix::ObjectId>,
    author: gix::actor::Signature,
    committer: gix::actor::Signature,
    message: String,
) -> Result<(gix::ObjectId, Commit), Error> {
    let commit = Commit {
        tree,
        parents: parents.into(),
        author,
        committer,
        encoding: None,
        extra_headers: Vec::new(),
        message: message.into(),
    };
    let (id, _) = object_commit(repo, &commit)?;
    Ok((id, commit))
}

fn source_for_worktree(
    snapshot: &Snapshot,
    repo: &gix::Repository,
    status: &crate::Status,
    stage: &Entries,
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<WorktreeSnapshot, Error> {
    if status.repository.git_dir != snapshot.repository.git_dir {
        return Err(Error::Stale);
    }
    let mut worktree = stage.clone();
    let mut blobs = BTreeMap::new();
    let mut pins = Vec::new();
    let mut paths = Vec::new();
    let mut budget = revision::Budget::new(limits);
    for row in &status.entries {
        ctx.check()?;
        if matches!(row.kind, crate::Kind::Untracked) {
            continue;
        }
        if matches!(row.kind, crate::Kind::Conflict(_)) {
            return Err(Error::Unmerged);
        }
        if row.submodule {
            return Err(Error::Unsupported);
        }
        revision::path_valid(&row.path, limits.max_path)?;
        paths.push(row.path.clone());
        if row.worktree == Change::Unchanged {
            continue;
        }
        if !matches!(
            row.worktree,
            Change::Modified | Change::Added | Change::Deleted
        ) {
            return Err(Error::Unsupported);
        }
        let path = gix::path::from_bstr(row.path.as_bstr()).map_err(engine)?;
        let absolute = snapshot.repository.root.join(path);
        match capture(&snapshot.repository.root, &absolute, limits.max_blob, ctx)? {
            Capture::Absent { token } if row.worktree == Change::Deleted => {
                worktree.remove(&row.path);
                pins.push((row.path.clone(), None, Some(token)));
            }
            Capture::Present { token, bytes, .. } if row.worktree != Change::Deleted => {
                if !token.has_single_link() {
                    return Err(Error::Unsupported);
                }
                budget.charge(bytes.len())?;
                let mode = stage.get(&row.path).map_or(0o100644, |entry| entry.mode);
                if !matches!(mode, 0o100644 | 0o100755) {
                    return Err(Error::Unsupported);
                }
                let id = gix::objs::compute_hash(repo.object_hash(), Kind::Blob, &bytes)
                    .map_err(engine)?;
                blobs.entry(id).or_insert(bytes);
                worktree.insert(row.path.clone(), revision::Entry { id, mode });
                pins.push((row.path.clone(), Some(token), None));
            }
            _ => return Err(Error::Stale),
        }
    }
    paths.sort();
    paths.dedup();
    Ok(WorktreeSnapshot {
        entries: worktree,
        blobs: blobs.into_iter().collect(),
        pins,
        paths,
    })
}

/// Prepare saving staged and tracked worktree changes. Untracked files remain
/// untouched, matching ordinary `git stash push` without `--include-untracked`.
pub fn prepare_push(
    root: &Path,
    message: &str,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<StashPushPlan, Error> {
    limits.admit()?;
    if message.len() > 1024 || message.chars().any(char::is_control) {
        return Err(Error::Unsupported);
    }
    controlled(limits.timeout, cancelled, |ctx| {
        let (repo, snapshot) = snapshot(root, limits, ctx)?;
        let status = crate::read_status(root, limits, || ctx.check().is_err())?;
        index_write::reject_config_conversion(&repo, &snapshot.repository)?;
        if status
            .entries
            .iter()
            .any(|entry| entry.path.rsplit(|byte| *byte == b'/').next() == Some(b".gitattributes"))
        {
            return Err(Error::Filter);
        }
        if !status.entries.iter().any(|entry| {
            !matches!(entry.kind, crate::Kind::Untracked)
                && (entry.index != Change::Unchanged || entry.worktree != Change::Unchanged)
        }) {
            return Err(Error::Engine(
                "No hay cambios rastreados que guardar".into(),
            ));
        }
        let (_, index_bytes) = local_pair::index_capture(&repo, &snapshot.repository.git_dir, ctx)?;
        let index = local_pair::decode_index(&repo, index_bytes.as_deref(), ctx)?;
        if index.entries().iter().any(|entry| entry.stage_raw() != 0) {
            return Err(Error::Unmerged);
        }
        if index.entries().iter().any(|entry| {
            entry.flags.intersects(
                gix::index::entry::Flags::INTENT_TO_ADD | gix::index::entry::Flags::SKIP_WORKTREE,
            )
        }) {
            return Err(Error::Unsupported);
        }
        let mut staged = Entries::new();
        for entry in index.entries() {
            ctx.check()?;
            if entry.mode.is_submodule() {
                return Err(Error::Unsupported);
            }
            let path = entry.path(&index).as_bytes();
            revision::path_valid(path, limits.max_path)?;
            if path.rsplit(|byte| *byte == b'/').next() == Some(b".gitattributes") {
                return Err(Error::Filter);
            }
            if !matches!(entry.mode.bits(), 0o100644 | 0o100755) {
                return Err(Error::Unsupported);
            }
            if staged
                .insert(
                    path.to_vec(),
                    revision::Entry {
                        id: entry.id,
                        mode: entry.mode.bits(),
                    },
                )
                .is_some()
            {
                return Err(Error::Unsupported);
            }
        }
        let head_commit = revision::commit(
            &repo,
            snapshot.head,
            limits,
            &mut revision::Budget::new(limits),
            ctx,
        )?;
        let head_entries = revision::tree_entries(
            &repo,
            head_commit.tree,
            limits,
            &mut revision::Budget::new(limits),
            ctx,
        )?;
        if head_entries
            .keys()
            .any(|path| path.rsplit(|byte| *byte == b'/').next() == Some(b".gitattributes"))
        {
            return Err(Error::Filter);
        }
        let worktree = source_for_worktree(&snapshot, &repo, &status, &staged, limits, ctx)?;
        let (_, index_trees) = build_tree(&repo, &staged, limits, ctx)?;
        let index_tree = index_trees
            .last()
            .map_or_else(|| repo.object_hash().null(), |(id, _)| *id);
        let (worktree_tree, mut trees) = build_tree(&repo, &worktree.entries, limits, ctx)?;
        let mut tree_map = BTreeMap::new();
        for (id, tree) in index_trees.into_iter().chain(trees.drain(..)) {
            tree_map.entry(id).or_insert(tree);
        }
        let trees = tree_map.into_iter().collect();
        let identity = crate::commit_write::local_identity(&repo)?;
        let index_message = format!(
            "index on {}\n",
            String::from_utf8_lossy(
                snapshot
                    .branch
                    .as_bstr()
                    .strip_prefix(b"refs/heads/")
                    .unwrap_or(snapshot.branch.as_bstr().as_bytes())
            )
        );
        let (index_id, index_commit) = make_tree_commit(
            &repo,
            index_tree,
            vec![snapshot.head],
            identity.clone(),
            identity.clone(),
            index_message,
        )?;
        let text = if message.is_empty() { "WIP" } else { message };
        let stash_message = format!(
            "On {}: {}\n",
            String::from_utf8_lossy(
                snapshot
                    .branch
                    .as_bstr()
                    .strip_prefix(b"refs/heads/")
                    .unwrap_or(snapshot.branch.as_bstr().as_bytes())
            ),
            text
        );
        let (stash, stash_commit) = make_tree_commit(
            &repo,
            worktree_tree,
            vec![snapshot.head, index_id],
            identity.clone(),
            identity,
            stash_message,
        )?;
        let reset = local_merge::prepare_reset(
            root,
            &snapshot.head.to_string(),
            local_merge::ResetMode::Hard,
            limits,
            || ctx.check().is_err(),
        )?;
        Ok(StashPushPlan {
            snapshot,
            message: text.to_owned(),
            stash,
            index_commit,
            stash_commit,
            blobs: worktree.blobs,
            trees,
            paths: worktree.paths,
            worktree_pins: worktree.pins,
            reset,
            limits,
        })
    })
}

fn validate_worktree_pins(
    snapshot: &Snapshot,
    pins: &[(
        Vec<u8>,
        Option<jc_fs::FileToken>,
        Option<jc_fs::MissingToken>,
    )],
    ctx: &Context<'_>,
) -> Result<(), Error> {
    for (path, file, missing) in pins {
        ctx.check()?;
        let path = gix::path::from_bstr(path.as_bstr()).map_err(engine)?;
        match (
            capture(
                &snapshot.repository.root,
                &snapshot.repository.root.join(path),
                16 * 1024 * 1024,
                ctx,
            )?,
            file,
            missing,
        ) {
            (Capture::Present { token, .. }, Some(expected), None) if &token == expected => {}
            (Capture::Absent { token }, None, Some(expected)) if &token == expected => {}
            _ => return Err(Error::Stale),
        }
    }
    Ok(())
}

/// Publish the stash first, then clean tracked files through the recoverable
/// hard-reset transaction. If cleanup fails, the complete stash remains safe.
pub fn apply_push(
    plan: StashPushPlan,
    mut cancelled: impl FnMut() -> bool,
) -> Result<StashPushResult, Error> {
    let stop = AtomicBool::new(false);
    let reason = AtomicU8::new(0);
    let ctx = Context {
        stop: &stop,
        reason: &reason,
        deadline: Instant::now() + plan.limits.timeout,
    };
    let mut check = || {
        if cancelled() {
            Err(Error::Cancelled)
        } else {
            ctx.check()
        }
    };
    check()?;
    let git = &plan.snapshot.repository.git_dir;
    let (mut repo, identity) = index_write::open_for_write(&plan.snapshot.repository.root, &ctx)?;
    index_write::same_repository(&plan.snapshot.repository, &identity)?;
    pin_matches(&plan.snapshot, &ctx)?;
    // Re-read status and exact changed source identities at confirmation.
    let status = crate::read_status(&identity.root, plan.limits, || ctx.check().is_err())?;
    if !status.entries.iter().any(|entry| {
        !matches!(entry.kind, crate::Kind::Untracked)
            && (entry.index != Change::Unchanged || entry.worktree != Change::Unchanged)
    }) {
        return Err(Error::Stale);
    }
    let current_index = local_pair::index_capture(&repo, git, &ctx)?.0;
    if current_index != plan.snapshot.index {
        return Err(Error::Stale);
    }
    validate_worktree_pins(&plan.snapshot, &plan.worktree_pins, &ctx)?;
    let dirty = status
        .entries
        .iter()
        .filter(|entry| {
            !matches!(entry.kind, crate::Kind::Untracked)
                && (entry.index != Change::Unchanged || entry.worktree != Change::Unchanged)
        })
        .map(|entry| entry.path.clone())
        .collect::<std::collections::BTreeSet<_>>();
    if dirty.into_iter().collect::<Vec<_>>() != plan.paths {
        return Err(Error::Stale);
    }
    // Store immutable content before publishing the only reference to it.
    for (id, bytes) in &plan.blobs {
        check()?;
        commit_write_store(&repo, git, *id, &gix::objs::BlobRef { data: bytes })?;
    }
    for (id, tree) in &plan.trees {
        check()?;
        commit_write_store(&repo, git, *id, tree)?;
    }
    let original = plan
        .snapshot
        .stash_ref
        .unwrap_or_else(|| repo.object_hash().null());
    let line = gix::refs::log::Line {
        previous_oid: original,
        new_oid: plan.stash,
        signature: crate::commit_write::local_identity(&repo)?,
        message: BString::from(plan.message.as_str()),
    };
    let (log, log_before) =
        crate::commit_write::stage_log_with_source(git, Path::new(STASH_LOG), &line)?;
    if log_before.as_ref() != plan.snapshot.log.as_ref().map(|log| &log.token) {
        return Err(Error::Stale);
    }
    repo.refs.write_reflog = WriteReflog::Disable;
    let name = FullName::try_from(BString::from(STASH_REF)).map_err(engine)?;
    let previous = plan
        .snapshot
        .stash_ref
        .map_or(PreviousValue::MustNotExist, |id| {
            PreviousValue::MustExistAndMatch(Target::Object(id))
        });
    let tx = repo
        .refs
        .transaction()
        .prepare(
            [RefEdit::update(
                name,
                Target::Object(plan.stash),
                previous,
                BString::default(),
            )],
            Fail::Immediately,
            Fail::Immediately,
        )
        .map_err(engine)?;
    check()?;
    for (id, commit) in [
        (plan.stash, &plan.stash_commit),
        (
            object_commit(&repo, &plan.index_commit)?.0,
            &plan.index_commit,
        ),
    ] {
        commit_write_store(&repo, git, id, commit)?;
    }
    tx.commit(None).map_err(|error| {
        Error::Engine(format!(
            "Stash {} publicado de forma incierta ({})",
            plan.stash,
            engine(error)
        ))
    })?;
    let mut warning = log.commit().err().map(|error| {
        format!(
            "Stash {} guardado; no se pudo actualizar su historial ({})",
            plan.stash,
            engine(error)
        )
    });
    let mut workspace_clean = false;
    let mut hard_backup = None;
    if warning.is_some() {
        // The reference may already be published while its reflog is not.
        // Preserve all local bytes until the stash list can be observed again.
    } else if !cancelled() {
        match local_merge::apply_merge(plan.reset, cancelled) {
            Ok(result) => {
                workspace_clean = true;
                hard_backup = result.hard_backup;
            }
            Err(error) => {
                warning = Some(format!(
                    "Stash {} guardado. El árbol no se limpió y requiere revisión: {error}",
                    plan.stash
                ))
            }
        }
    } else {
        warning = Some(format!(
            "Stash {} guardado; se canceló la limpieza del árbol",
            plan.stash
        ));
    }
    Ok(StashPushResult {
        oid: plan.stash,
        paths: plan.paths,
        workspace_clean,
        warning,
        hard_backup,
    })
}

fn commit_write_store(
    repo: &gix::Repository,
    git: &Path,
    id: gix::ObjectId,
    object: &impl WriteTo,
) -> Result<(), Error> {
    crate::commit_write::store_object(repo, git, id, object)
}

/// Build an apply review for an exact reflog ordinal and object ID.
pub fn prepare_apply(
    root: &Path,
    selector: StashSelector,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<StashApplyPlan, Error> {
    limits.admit()?;
    controlled(limits.timeout, cancelled, |ctx| {
        let (_, snapshot) = snapshot(root, limits, ctx)?;
        if index_write::exists(
            &snapshot
                .repository
                .git_dir
                .join(crate::branch_switch::JOURNAL_NAME),
        )? {
            return Err(Error::Engine(
                "No se puede retirar un stash mientras hay una operación de Git pendiente".into(),
            ));
        }
        select(&snapshot, selector)?;
        let log_token = snapshot.log.as_ref().ok_or(Error::Stale)?.token.clone();
        let repository = snapshot.repository.clone();
        let merge = local_merge::prepare_stash(
            root,
            &selector.oid.to_string(),
            selector.index,
            limits,
            || ctx.check().is_err(),
        )?;
        Ok(StashApplyPlan {
            selector,
            repository,
            root: root.to_owned(),
            log_token,
            merge,
            limits,
        })
    })
}

fn verify_log_token(
    git: &Path,
    expected: &jc_fs::FileToken,
    ctx: &Context<'_>,
) -> Result<(), Error> {
    let Capture::Present { token, .. } = capture(git, &git.join(STASH_LOG), STASH_LOG_BYTES, ctx)?
    else {
        return Err(Error::Stale);
    };
    if &token != expected {
        return Err(Error::Stale);
    }
    Ok(())
}

pub fn apply_stash(
    plan: StashApplyPlan,
    cancelled: impl FnMut() -> bool,
) -> Result<StashApplyResult, Error> {
    let stop = AtomicBool::new(false);
    let reason = AtomicU8::new(0);
    let ctx = Context {
        stop: &stop,
        reason: &reason,
        deadline: Instant::now() + plan.limits.timeout,
    };
    let git = plan.repository.git_dir.clone();
    let _log = Marker::acquire_to_hold_resource(git.join(STASH_LOG), Fail::Immediately, None, 0)
        .map_err(engine)?;
    verify_log_token(&git, &plan.log_token, &ctx)?;
    let listed = list(&plan.root, plan.limits, || ctx.check().is_err())?;
    if listed
        .entries
        .get(plan.selector.index)
        .map(|entry| entry.selector)
        != Some(plan.selector)
        || listed
            .entries
            .iter()
            .filter(|entry| entry.selector.oid == plan.selector.oid)
            .count()
            != 1
    {
        return Err(Error::Stale);
    }
    let paths = plan.merge.rows.iter().map(|row| row.path.clone()).collect();
    let oid = plan.selector.oid;
    let result = local_merge::apply_merge(plan.merge, cancelled)?;
    Ok(StashApplyResult {
        oid,
        paths,
        warning: result.reflog_warning,
    })
}

/// Prepare removal of one uniquely identified stash entry.
pub fn prepare_drop(
    root: &Path,
    selector: StashSelector,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<StashDropPlan, Error> {
    limits.admit()?;
    controlled(limits.timeout, cancelled, |ctx| {
        let (_, snapshot) = snapshot(root, limits, ctx)?;
        select(&snapshot, selector)?;
        let log = snapshot.log.as_ref().ok_or(Error::Stale)?;
        let raw = log.bytes[..log.bytes.len() - 1]
            .split(|b| *b == b'\n')
            .collect::<Vec<_>>();
        let line_index = raw
            .len()
            .checked_sub(selector.index + 1)
            .ok_or(Error::Stale)?;
        let remaining = raw
            .into_iter()
            .enumerate()
            .filter(|(index, _)| *index != line_index)
            .flat_map(|(_, line)| line.iter().copied().chain(std::iter::once(b'\n')))
            .collect::<Vec<_>>();
        let next = if selector.index == 0 {
            snapshot.entries.get(1).map(|entry| entry.selector.oid)
        } else {
            snapshot.stash_ref
        };
        Ok(StashDropPlan {
            selector,
            snapshot,
            remaining,
            next,
            limits,
        })
    })
}

#[derive(Debug)]
pub struct StashDropResult {
    pub oid: gix::ObjectId,
    pub warning: Option<String>,
}

pub fn apply_drop(
    plan: StashDropPlan,
    mut cancelled: impl FnMut() -> bool,
) -> Result<StashDropResult, Error> {
    let stop = AtomicBool::new(false);
    let reason = AtomicU8::new(0);
    let ctx = Context {
        stop: &stop,
        reason: &reason,
        deadline: Instant::now() + plan.limits.timeout,
    };
    if cancelled() {
        return Err(Error::Cancelled);
    }
    let git = &plan.snapshot.repository.git_dir;
    if index_write::exists(&git.join(crate::branch_switch::JOURNAL_NAME))? {
        return Err(Error::Stale);
    }
    let (mut repo, identity) = index_write::open_for_write(&plan.snapshot.repository.root, &ctx)?;
    index_write::same_repository(&plan.snapshot.repository, &identity)?;
    if ref_id(&repo)? != plan.snapshot.stash_ref || capture_ref(git, &ctx)? != plan.snapshot.ref_pin
    {
        return Err(Error::Stale);
    }
    let log = plan.snapshot.log.as_ref().ok_or(Error::Stale)?;
    let current = capture(git, &git.join(STASH_LOG), STASH_LOG_BYTES, &ctx)?;
    if !matches!(current, Capture::Present { token, .. } if token == log.token) {
        return Err(Error::Stale);
    }
    // gix deletes the reflog itself when deleting its final reference. Do not
    // hold a second reflog lock or republish an empty log after that transaction.
    let mut log_lock = if plan.next.is_some() {
        let mut lock =
            File::acquire_to_update_resource(git.join(STASH_LOG), Fail::Immediately, None, 0)
                .map_err(engine)?;
        let current = capture(git, &git.join(STASH_LOG), STASH_LOG_BYTES, &ctx)?;
        if !matches!(current, Capture::Present { token, .. } if token == log.token) {
            return Err(Error::Stale);
        }
        lock.write_all(&plan.remaining)?;
        lock.with_mut(|file| file.sync_all())?;
        Some(lock)
    } else {
        None
    };
    repo.refs.write_reflog = WriteReflog::Disable;
    let name = FullName::try_from(BString::from(STASH_REF)).map_err(engine)?;
    let edit = if let Some(next) = plan.next {
        RefEdit::update(
            name,
            Target::Object(next),
            PreviousValue::MustExistAndMatch(Target::Object(
                plan.snapshot.stash_ref.ok_or(Error::Stale)?,
            )),
            BString::default(),
        )
    } else {
        RefEdit::delete(
            name,
            PreviousValue::MustExistAndMatch(Target::Object(
                plan.snapshot.stash_ref.ok_or(Error::Stale)?,
            )),
        )
    };
    let tx = repo
        .refs
        .transaction()
        .prepare([edit], Fail::Immediately, Fail::Immediately)
        .map_err(engine)?;
    if cancelled() {
        return Err(Error::Cancelled);
    }
    if let Err(error) = tx.commit(None) {
        if plan.next.is_none() {
            let (fresh, fresh_identity) =
                crate::branch_switch::open_for_switch(&plan.snapshot.repository.root, &ctx)?;
            index_write::same_repository(&plan.snapshot.repository, &fresh_identity)?;
            let ref_after = ref_id(&fresh)?;
            let log_after = capture(git, &git.join(STASH_LOG), STASH_LOG_BYTES, &ctx)?;
            match (ref_after, log_after) {
                (None, Capture::Absent { .. }) => {
                    return Ok(StashDropResult {
                        oid: plan.selector.oid,
                        warning: Some(format!(
                            "El stash se retiró, pero Git informó un error al completar la limpieza ({})",
                            engine(error)
                        )),
                    });
                }
                (Some(id), Capture::Absent { token }) if Some(id) == plan.snapshot.stash_ref => {
                    let mut cancelled = || false;
                    jc_fs::stage_missing_file(git, &token, &log.bytes, &mut cancelled)?
                        .publish()?;
                    return Err(Error::Engine(format!(
                        "No se retiró el stash y se restauró su reflog original ({})",
                        engine(error)
                    )));
                }
                (_, Capture::Present { bytes, .. }) if bytes == log.bytes => {}
                _ => {
                    return Err(Error::Engine(format!(
                        "La eliminación del stash quedó incierta; conserva el repositorio y actualiza Git ({})",
                        engine(error)
                    )));
                }
            }
        }
        return Err(Error::Engine(format!(
            "No se pudo confirmar la eliminación del stash; actualiza la lista ({})",
            engine(error)
        )));
    }
    let warning = log_lock
        .take()
        .and_then(|lock| lock.commit().err())
        .map(|error| {
            format!(
                "La referencia se actualizó, pero falló el historial del stash ({})",
                engine(error)
            )
        });
    Ok(StashDropResult {
        oid: plan.selector.oid,
        warning,
    })
}

pub fn prepare_pop(
    root: &Path,
    selector: StashSelector,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<StashPopPlan, Error> {
    Ok(StashPopPlan {
        apply: prepare_apply(root, selector, limits, cancelled)?,
    })
}

impl StashPopPlan {
    pub fn with_resolutions(
        &self,
        choices: BTreeMap<Vec<u8>, local_merge::MergeResolution>,
        cancelled: impl FnMut() -> bool + Send,
    ) -> Result<Self, Error> {
        Ok(Self {
            apply: self.apply.with_resolutions(choices, cancelled)?,
        })
    }
}

pub fn apply_pop(
    plan: StashPopPlan,
    cancelled: impl FnMut() -> bool,
) -> Result<StashPopResult, Error> {
    if !plan.apply.can_apply() {
        return Err(Error::Engine(
            "Resuelve los conflictos antes de aplicar y retirar el stash".into(),
        ));
    }
    let selector = plan.apply.selector;
    let root = plan.apply.root.clone();
    let limits = plan.apply.limits;
    let applied = apply_stash(plan.apply, cancelled)?;
    match prepare_drop(&root, selector, limits, || false) {
        Ok(drop) => match apply_drop(drop, || false) {
            Ok(drop) => Ok(StashPopResult {
                applied,
                dropped: drop.warning.is_none(),
                warning: drop.warning,
            }),
            Err(error) => Ok(StashPopResult {
                applied,
                dropped: false,
                warning: Some(format!(
                    "Los cambios se aplicaron; el stash se conserva porque no se pudo retirar ({error})"
                )),
            }),
        },
        Err(error) => Ok(StashPopResult {
            applied,
            dropped: false,
            warning: Some(format!(
                "Los cambios se aplicaron; el stash se conserva porque su posición cambió ({error})"
            )),
        }),
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use gix::{
        bstr::ByteSlice,
        index::entry::{Flags, Mode},
        objs::{
            Commit, Tree,
            tree::{Entry as TreeEntry, EntryKind},
        },
    };
    use std::{
        fs,
        io::Write,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    struct Fixture {
        root: PathBuf,
        parent: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .nth(2)
                .unwrap()
                .canonicalize()
                .unwrap();
            let parent = workspace.join("work/test-temp");
            fs::create_dir_all(&parent).unwrap();
            let parent = parent.canonicalize().unwrap();
            let root = parent.join(format!(
                "git-stash-{}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            let repo = gix::init(&root).unwrap();
            let blob = repo.write_blob(b"base\n").unwrap().detach();
            let tree = repo
                .write_object(Tree {
                    entries: vec![TreeEntry {
                        mode: EntryKind::Blob.into(),
                        filename: "a.txt".into(),
                        oid: blob,
                    }],
                })
                .unwrap()
                .detach();
            let actor = gix::actor::SignatureRef {
                name: b"Stash fixture".as_bstr(),
                email: b"stash@invalid".as_bstr(),
                time: "1 +0000",
            };
            let head = repo
                .write_object(Commit {
                    tree,
                    parents: Default::default(),
                    author: actor.into(),
                    committer: actor.into(),
                    encoding: None,
                    extra_headers: Vec::new(),
                    message: b"base\n".as_slice().into(),
                })
                .unwrap()
                .detach();
            let git = root.join(".git");
            fs::write(git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
            fs::write(git.join("refs/heads/main"), format!("{head}\n")).unwrap();
            fs::OpenOptions::new().append(true).open(git.join("config")).unwrap().write_all(b"\n[core]\nlogAllRefUpdates = true\n[user]\nname = Stash Test\nemail = stash@invalid\n").unwrap();
            fs::write(root.join("a.txt"), b"worktree\n").unwrap();
            fs::write(root.join("untracked.txt"), b"keep me\n").unwrap();
            let staged = repo.write_blob(b"staged\n").unwrap().detach();
            let mut state = gix::index::State::new(repo.object_hash());
            state.dangerously_push_entry(
                Default::default(),
                staged,
                Flags::empty(),
                Mode::FILE,
                b"a.txt".as_bstr(),
            );
            let mut bytes = Vec::new();
            gix::index::File::from_state(state, git.join("index"))
                .write_to(&mut bytes, Default::default())
                .unwrap();
            fs::write(git.join("index"), bytes).unwrap();
            Self { root, parent }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            assert_eq!(self.root.parent(), Some(self.parent.as_path()));
            fn remove(path: &Path) {
                let metadata = fs::symlink_metadata(path).unwrap();
                if metadata.is_dir() {
                    for entry in fs::read_dir(path).unwrap() {
                        remove(&entry.unwrap().path());
                    }
                    fs::remove_dir(path).unwrap();
                } else {
                    fs::remove_file(path).unwrap();
                }
            }
            remove(&self.root);
        }
    }

    fn limits() -> Limits {
        Limits {
            max_blob: 2 * 1024 * 1024,
            max_entries: 4096,
            max_path: 4096,
            max_bytes: 4 * 1024 * 1024,
            timeout: Duration::from_secs(5),
        }
    }

    #[test]
    fn push_apply_pop_preserve_untracked_and_validate_reflog_selector() {
        let fixture = Fixture::new();
        let pushed = apply_push(
            prepare_push(&fixture.root, "prueba", limits(), || false).unwrap(),
            || false,
        )
        .unwrap();
        assert!(pushed.workspace_clean);
        assert_eq!(fs::read(fixture.root.join("a.txt")).unwrap(), b"base\n");
        assert_eq!(
            fs::read(fixture.root.join("untracked.txt")).unwrap(),
            b"keep me\n"
        );
        let listed = list(&fixture.root, limits(), || false).unwrap();
        assert_eq!(listed.entries.len(), 1);
        assert_eq!(listed.entries[0].selector.oid, pushed.oid);
        let apply = prepare_apply(&fixture.root, listed.entries[0].selector, limits(), || {
            false
        })
        .unwrap();
        assert!(apply.can_apply());
        let index_before_apply = fs::read(fixture.root.join(".git/index")).unwrap();
        let applied = apply_stash(apply, || false).unwrap();
        assert_eq!(applied.oid, pushed.oid);
        assert_eq!(fs::read(fixture.root.join("a.txt")).unwrap(), b"worktree\n");
        assert_eq!(
            fs::read(fixture.root.join(".git/index")).unwrap(),
            index_before_apply
        );

        // A second stash proves pop applies the observed entry before dropping it.
        fs::write(fixture.root.join("a.txt"), b"second\n").unwrap();
        let second = apply_push(
            prepare_push(&fixture.root, "segunda", limits(), || false).unwrap(),
            || false,
        )
        .unwrap();
        let current = list(&fixture.root, limits(), || false).unwrap();
        let pop = prepare_pop(&fixture.root, current.entries[0].selector, limits(), || {
            false
        })
        .unwrap();
        let index_before_pop = fs::read(fixture.root.join(".git/index")).unwrap();
        let result = apply_pop(pop, || false).unwrap();
        assert!(result.dropped);
        assert_eq!(result.applied.oid, second.oid);
        let after = list(&fixture.root, limits(), || false).unwrap();
        assert_eq!(after.entries.len(), 1);
        assert_eq!(after.entries[0].selector.oid, pushed.oid);
        assert_eq!(fs::read(fixture.root.join("a.txt")).unwrap(), b"second\n");
        assert_eq!(
            fs::read(fixture.root.join(".git/index")).unwrap(),
            index_before_pop
        );
    }

    #[test]
    fn drop_rejects_stale_position_and_removes_last_reflog_entry_cleanly() {
        let fixture = Fixture::new();
        let first = apply_push(
            prepare_push(&fixture.root, "primera", limits(), || false).unwrap(),
            || false,
        )
        .unwrap();
        let selected = list(&fixture.root, limits(), || false).unwrap().entries[0].selector;
        fs::write(fixture.root.join("a.txt"), b"second\n").unwrap();
        let second = apply_push(
            prepare_push(&fixture.root, "segunda", limits(), || false).unwrap(),
            || false,
        )
        .unwrap();
        let stale = prepare_drop(&fixture.root, selected, limits(), || false);
        assert!(stale.is_err());
        let current = list(&fixture.root, limits(), || false).unwrap();
        assert_eq!(current.entries[0].selector.oid, second.oid);
        apply_drop(
            prepare_drop(&fixture.root, current.entries[0].selector, limits(), || {
                false
            })
            .unwrap(),
            || false,
        )
        .unwrap();
        let remaining = list(&fixture.root, limits(), || false).unwrap();
        assert_eq!(remaining.entries.len(), 1);
        assert_eq!(remaining.entries[0].selector.oid, first.oid);
        apply_drop(
            prepare_drop(
                &fixture.root,
                remaining.entries[0].selector,
                limits(),
                || false,
            )
            .unwrap(),
            || false,
        )
        .unwrap();
        assert!(
            list(&fixture.root, limits(), || false)
                .unwrap()
                .entries
                .is_empty()
        );
    }
}
