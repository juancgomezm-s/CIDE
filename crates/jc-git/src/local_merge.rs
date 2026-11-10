//! Read-only three-way planning and recoverable local publication.
use crate::{
    Change, Context, Error, Identity, Limits,
    branch_checkout::{Checkout, Journal, Mainline, MergeCheckout, ResetTransition, Transition},
    branch_switch::{capture_head, direct_branch, name_text, open_for_switch},
    commit_write, controlled,
    diff_model::{DiffLimits, DiffModel, DiffSide, DiffStatus},
    engine, graph, index_write,
    local_pair::{self, IndexToken},
    ref_write::{self, RefKind},
    revision::{self, Budget, Entry},
};
use gix::{
    ObjectId,
    bstr::ByteSlice,
    lock::{File, Marker, acquire::Fail},
    objs::{Commit, Kind, Tree, WriteTo, tree::EntryKind},
    refs::{FullName, store::WriteReflog},
};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    io::Write,
    ops::Range,
    path::Path,
    sync::atomic::{AtomicBool, AtomicU8},
    time::Instant,
};

const HEADS: &str = "refs/heads/";
const MAX_PATHS: usize = 128;
mod abort;
mod rebase;
pub use abort::{MergeAbortPlan, apply_merge_abort, prepare_merge_abort};
pub use rebase::{RebaseStep, RebaseStepState};
pub const MERGE_EDIT_BYTES: usize = 64 * 1024;
type Entries = BTreeMap<Vec<u8>, Entry>;
type Blobs = BTreeMap<ObjectId, Vec<u8>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeKind {
    UpToDate,
    FastForward,
    Commit,
    Conflicts,
    ChooseMainline,
}

impl MergeKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::UpToDate => "La rama activa ya contiene esos cambios",
            Self::FastForward => "Avance directo de la rama activa",
            Self::Commit => "Commit de fusión con dos padres",
            Self::Conflicts => "Conflictos por resolver · borrador de fusión",
            Self::ChooseMainline => "Elige el padre principal",
        }
    }
}

pub struct MergeRow {
    pub path: Vec<u8>,
    /// None identifies a conflict rather than a proposed write.
    pub change: Option<Change>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MergeSide {
    Base,
    Current,
    Incoming,
}

pub struct MergeConflict {
    versions: Option<[Option<Vec<u8>>; 3]>,
    paths: Option<PathChoices>,
}

#[derive(PartialEq, Eq)]
enum PathConflict {
    FileDirectory,
    Rename,
}

struct PathChoices {
    kind: PathConflict,
    alternatives: [Entries; 3],
}

impl PathChoices {
    fn covers(&self, root: &[u8], path: &[u8]) -> bool {
        match self.kind {
            PathConflict::FileDirectory => contains_path(root, path),
            PathConflict::Rename => self
                .alternatives
                .iter()
                .any(|entries| entries.contains_key(path)),
        }
    }

    fn matches(&self, root: &[u8], stored: &Entries) -> bool {
        match self.kind {
            PathConflict::FileDirectory => self.alternatives.contains(&subtree(stored, root)),
            PathConflict::Rename => self.alternatives.iter().any(|selected| {
                self.alternatives
                    .iter()
                    .flat_map(Entries::keys)
                    .all(|path| stored.get(path) == selected.get(path))
            }),
        }
    }
}

impl MergeConflict {
    pub fn can_resolve(&self) -> bool {
        self.versions.is_some() || self.paths.is_some()
    }

    pub fn bytes(&self, side: MergeSide) -> Option<&[u8]> {
        self.versions.as_ref()?[side as usize].as_deref()
    }
    pub fn is_file_directory(&self) -> bool {
        self.paths
            .as_ref()
            .is_some_and(|paths| paths.kind == PathConflict::FileDirectory)
    }
    pub fn is_rename(&self) -> bool {
        self.paths
            .as_ref()
            .is_some_and(|paths| paths.kind == PathConflict::Rename)
    }
    pub fn requires_whole_version(&self) -> bool {
        self.paths.is_some()
    }
    pub fn version_paths(&self, side: MergeSide) -> Option<impl Iterator<Item = &[u8]>> {
        Some(
            self.paths.as_ref()?.alternatives[side as usize]
                .keys()
                .map(Vec::as_slice),
        )
    }
}

#[derive(Clone)]
pub enum MergeResolution {
    Use(MergeSide),
    Edit(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitOperation {
    CherryPick,
    Revert,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResetMode {
    Soft,
    Mixed,
    Hard,
}

impl ResetMode {
    pub const ALL: [Self; 3] = [Self::Soft, Self::Mixed, Self::Hard];

    pub fn label(self) -> &'static str {
        match self {
            Self::Soft => "soft",
            Self::Mixed => "mixed",
            Self::Hard => "hard",
        }
    }
}

impl CommitOperation {
    pub fn title(self) -> &'static str {
        match self {
            Self::CherryPick => "Cherry-pick",
            Self::Revert => "Revertir commit",
        }
    }
}

#[derive(Clone)]
enum Source {
    Branch(FullName, bool),
    Reset(ResetMode, String),
    Commit {
        operation: CommitOperation,
        id: String,
        mainline: Option<Mainline>,
    },
}

impl Source {
    fn label(&self) -> &str {
        match self {
            Self::Branch(name, _) => &name_text(name)[HEADS.len()..],
            Self::Commit { id, .. } | Self::Reset(_, id) => id,
        }
    }

    fn operation(&self) -> Option<CommitOperation> {
        match self {
            Self::Commit { operation, .. } => Some(*operation),
            Self::Branch(..) | Self::Reset(..) => None,
        }
    }

    fn mainline(&self) -> Option<Mainline> {
        match self {
            Self::Commit { mainline, .. } => *mainline,
            Self::Branch(..) | Self::Reset(..) => None,
        }
    }

    fn reset_mode(&self) -> Option<ResetMode> {
        match self {
            Self::Reset(mode, _) => Some(*mode),
            _ => None,
        }
    }

    fn is_rebase(&self) -> bool {
        matches!(self, Self::Branch(_, true))
    }
}

/// Identity of a reviewed integration, independent of the frontend draft.
pub struct MergeReview {
    repository: Identity,
    limits: Limits,
    active: FullName,
    source: Source,
    from: ObjectId,
    theirs: ObjectId,
    head_file: jc_fs::FileToken,
    index: IndexToken,
    rebase_review: Option<rebase::ConflictReview>,
}

pub struct MergePlan {
    pub kind: MergeKind,
    pub rows: Vec<MergeRow>,
    conflicts: BTreeMap<Vec<u8>, MergeConflict>,
    mainline_choices: Vec<ObjectId>,
    rebase_steps: Vec<RebaseStep>,
    rebase_review: Option<rebase::ConflictReview>,
    repository: Identity,
    limits: Limits,
    active: FullName,
    source: Source,
    from: ObjectId,
    theirs: ObjectId,
    result: Option<(ObjectId, ObjectId)>,
    head_file: jc_fs::FileToken,
    index: IndexToken,
    checkout: Option<Checkout>,
    objects: Option<Objects>,
    completed: bool,
    write_reflog: bool,
}

impl MergePlan {
    pub fn active(&self) -> &str {
        &name_text(&self.active)[HEADS.len()..]
    }
    pub fn incoming(&self) -> &str {
        self.source.label()
    }
    pub fn is_cherry_pick(&self) -> bool {
        self.operation() == Some(CommitOperation::CherryPick)
    }
    pub fn operation(&self) -> Option<CommitOperation> {
        self.source.operation()
    }
    pub fn label(&self) -> &'static str {
        if self.is_rebase() {
            return match self.kind {
                MergeKind::UpToDate => "La rama activa ya está basada en el destino",
                MergeKind::Conflicts => "Resuelve los conflictos para continuar el rebase",
                _ => "Se actualizará la rama activa sobre la base elegida",
            };
        }
        if let Some(mode) = self.reset_mode() {
            return match (mode, self.can_apply()) {
                (_, false) => "Sin cambios que aplicar",
                (ResetMode::Soft, true) => "Se conserva el índice",
                (ResetMode::Mixed, true) => "El índice quedará como en el commit de destino",
                (ResetMode::Hard, true) => {
                    "Se actualizarán la rama, el índice y los archivos indicados"
                }
            };
        }
        match (self.operation(), self.kind) {
            (Some(_), MergeKind::UpToDate) => "Sin cambios que aplicar",
            (Some(_), MergeKind::Conflicts) => "Conflictos por resolver · borrador sin escrituras",
            (Some(CommitOperation::CherryPick), MergeKind::Commit) => {
                "Se creará un commit con los cambios seleccionados"
            }
            (Some(CommitOperation::Revert), MergeKind::Commit) => {
                "Se creará un commit que revierte los cambios"
            }
            _ => self.kind.label(),
        }
    }
    pub fn reset_mode(&self) -> Option<ResetMode> {
        self.source.reset_mode()
    }
    pub fn is_rebase(&self) -> bool {
        self.source.is_rebase()
    }
    pub fn rebase_steps(&self) -> &[RebaseStep] {
        &self.rebase_steps
    }
    pub fn rebase_conflict_commit(&self) -> Option<ObjectId> {
        self.rebase_review.as_ref().map(|review| review.current)
    }
    pub fn hard_changes_file(&self, path: &[u8]) -> bool {
        self.reset_mode() == Some(ResetMode::Hard)
            && self.checkout.as_ref().is_some_and(|checkout| {
                checkout
                    .files
                    .binary_search_by(|file| file.path().cmp(path))
                    .is_ok()
            })
    }
    pub fn tips(&self) -> (ObjectId, ObjectId) {
        (self.from, self.theirs)
    }
    pub fn result(&self) -> Option<ObjectId> {
        self.result.map(|(commit, _)| commit)
    }
    pub fn can_apply(&self) -> bool {
        self.checkout.is_some()
    }
    pub fn recovering(&self) -> bool {
        self.checkout.as_ref().is_some_and(|plan| plan.resuming)
    }
    pub fn conflict(&self, path: &[u8]) -> Option<&MergeConflict> {
        self.conflicts.get(path)
    }
    pub fn mainline_choices(&self) -> &[ObjectId] {
        &self.mainline_choices
    }
    pub fn mainline(&self) -> Option<(usize, ObjectId)> {
        self.source
            .mainline()
            .map(|parent| (parent.number, parent.id))
    }
    pub fn mainline_review(&self, index: usize) -> Option<MergeReview> {
        let id = *self.mainline_choices.get(index)?;
        let mut review = self.review_identity();
        if let Source::Commit { mainline, .. } = &mut review.source {
            *mainline = Some(Mainline {
                number: index + 1,
                id,
            });
        }
        Some(review)
    }
    pub fn resolution_review(&self) -> Option<MergeReview> {
        (!self.conflicts.is_empty() && self.conflicts.values().all(MergeConflict::can_resolve))
            .then(|| self.review_identity())
    }

    fn review_identity(&self) -> MergeReview {
        MergeReview {
            repository: self.repository.clone(),
            limits: self.limits,
            active: self.active.clone(),
            source: self.source.clone(),
            from: self.from,
            theirs: self.theirs,
            head_file: self.head_file.clone(),
            index: self.index.clone(),
            rebase_review: self.rebase_review.clone(),
        }
    }
}

#[derive(Debug)]
pub struct MergePublication {
    pub id: ObjectId,
    pub reflog_warning: Option<String>,
    pub index_backup: Option<std::path::PathBuf>,
    pub hard_backup: Option<std::path::PathBuf>,
}

struct Objects {
    blobs: Blobs,
    trees: Vec<(ObjectId, Tree)>,
    commits: Vec<(ObjectId, Commit)>,
}

fn open(root: &Path, ctx: &Context<'_>) -> Result<(gix::Repository, Identity), Error> {
    let (repo, identity) = open_for_switch(root, ctx)?;
    for control in ["shallow", "info/grafts", "refs/replace", "MERGE_AUTOSTASH"] {
        if index_write::exists(&identity.git_dir.join(control))? {
            return Err(Error::Unsupported);
        }
    }
    if repo.config_snapshot().string("merge.default").is_some() {
        return Err(Error::Filter);
    }
    Ok((repo, identity))
}

fn refs(
    repo: &gix::Repository,
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<(FullName, Vec<graph::RefEntry>), Error> {
    let (head, refs) = graph::references(repo, limits, &mut Budget::new(limits), ctx)?;
    if refs
        .iter()
        .any(|entry| entry.name.starts_with(b"refs/replace/"))
    {
        return Err(Error::Unsupported);
    }
    let graph::Head::Attached { name, .. } = head else {
        return Err(Error::Engine(
            "La operación requiere una rama local activa".into(),
        ));
    };
    let short = name
        .strip_prefix(HEADS.as_bytes())
        .and_then(|name| std::str::from_utf8(name).ok())
        .ok_or(Error::Unsupported)?;
    Ok((
        ref_write::reference_name(RefKind::Branch, short, limits)?,
        refs,
    ))
}

fn base(
    repo: &gix::Repository,
    from: ObjectId,
    theirs: ObjectId,
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<ObjectId, Error> {
    if from == theirs {
        return Ok(from);
    }
    let mut budget = Budget::new(limits);
    let mut commits = BTreeMap::new();
    let mut seen = [BTreeSet::new(), BTreeSet::new()];
    let mut todo = VecDeque::from([(0, from), (1, theirs)]);
    while let Some((side, id)) = todo.pop_front() {
        ctx.check()?;
        if id == [theirs, from][side] {
            return Ok(id);
        }
        if !seen[side].insert(id) {
            continue;
        }
        if let std::collections::btree_map::Entry::Vacant(slot) = commits.entry(id) {
            // Bound the walk even if the repository lacks a commit graph.
            if seen[0].len() + seen[1].len() > limits.max_entries.min(8192) {
                return Err(Error::Limited);
            }
            let commit = revision::commit(repo, id, limits, &mut budget, ctx)?;
            if commit.limited {
                return Err(Error::Limited);
            }
            slot.insert(commit);
        }
        todo.extend(commits[&id].parents.iter().map(|parent| (side, *parent)));
    }
    let common: BTreeSet<_> = seen[0].intersection(&seen[1]).copied().collect();
    let mut best = common.clone();
    for id in common {
        ctx.check()?;
        for parent in &commits[&id].parents {
            best.remove(parent);
        }
    }
    match best.len() {
        1 => best.into_iter().next().ok_or(Error::Stale),
        0 => Err(Error::Engine("Las ramas no tienen una base común".into())),
        _ => Err(Error::Engine(
            "Las ramas tienen varias bases de fusión; se requiere resolución manual".into(),
        )),
    }
}

fn tree_id(
    repo: &gix::Repository,
    commit: ObjectId,
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<ObjectId, Error> {
    Ok(revision::commit(repo, commit, limits, &mut Budget::new(limits), ctx)?.tree)
}

fn offset(model: &DiffModel<'_>, side: DiffSide, line: crate::diff_model::DiffLine) -> usize {
    model
        .line_byte_range(side, line)
        .map_or_else(|| model.source_bytes(side).len(), |range| range.start)
}

fn edits<'a>(model: &DiffModel<'a>) -> Vec<(Range<usize>, &'a [u8])> {
    model
        .hunks()
        .map(|span| {
            let start = offset(model, DiffSide::Left, span.left.start);
            let end = offset(model, DiffSide::Left, span.left.end);
            let a = offset(model, DiffSide::Right, span.right.start);
            let b = offset(model, DiffSide::Right, span.right.end);
            (start..end, &model.source_bytes(DiffSide::Right)[a..b])
        })
        .collect()
}

fn merge_bytes(
    base: &[u8],
    ours: &[u8],
    theirs: &[u8],
    ctx: &Context<'_>,
) -> Result<Option<Vec<u8>>, Error> {
    if [base, ours, theirs]
        .into_iter()
        .any(|source| source.contains(&0))
    {
        return Ok(None);
    }
    let diff =
        |side| DiffModel::compute(base, side, DiffLimits::default(), || ctx.check().is_err());
    let left = diff(ours);
    let right = diff(theirs);
    ctx.check()?;
    if left.status() != DiffStatus::Exact || right.status() != DiffStatus::Exact {
        return Err(Error::Limited);
    }
    let mut edits = [edits(&left), edits(&right)].concat();
    edits.sort_by_key(|(range, _)| (range.start, range.end));
    edits.dedup();
    let mut out = Vec::new();
    let mut last: Option<Range<usize>> = None;
    for (range, replacement) in edits {
        ctx.check()?;
        let start = last.as_ref().map_or(0, |last| last.end);
        if range.start < start
            || last.as_ref().is_some_and(|last| {
                last.start == range.start || (range.is_empty() && range.start == last.end)
            })
        {
            return Ok(None);
        }
        out.extend_from_slice(&base[start..range.start]);
        out.extend_from_slice(replacement);
        last = Some(range);
    }
    out.extend_from_slice(&base[last.map_or(0, |range| range.end)..]);
    Ok(Some(out))
}

struct Candidate {
    entries: Entries,
    blobs: Blobs,
    conflicts: BTreeMap<Vec<u8>, MergeConflict>,
}

fn resolve_candidate(
    repo: &gix::Repository,
    candidate: &mut Candidate,
    choices: &Resolutions,
    limits: Limits,
    budget: &mut Budget,
    ctx: &Context<'_>,
) -> Result<(), Error> {
    if candidate.conflicts.is_empty() || choices.len() != candidate.conflicts.len() {
        return Err(Error::Stale);
    }
    for (path, conflict) in &candidate.conflicts {
        ctx.check()?;
        if !conflict.can_resolve() {
            return Err(Error::Unsupported);
        }
        let choice = choices.get(path).ok_or(Error::Stale)?;
        if let Some(paths) = &conflict.paths {
            let MergeResolution::Use(side) = choice else {
                return Err(Error::Unsupported);
            };
            candidate
                .entries
                .extend(paths.alternatives[*side as usize].clone());
            continue;
        }
        let bytes = match choice {
            MergeResolution::Use(side) => conflict.bytes(*side),
            MergeResolution::Edit(text) => {
                if text.len() > MERGE_EDIT_BYTES.min(limits.max_blob)
                    || text
                        .chars()
                        .any(|c| c.is_control() && !matches!(c, '\t' | '\r' | '\n'))
                {
                    return Err(Error::Unsupported);
                }
                Some(text.as_bytes())
            }
        };
        if let Some(bytes) = bytes {
            budget.charge(bytes.len())?;
            let entry = add_result_blob(repo, &mut candidate.blobs, bytes.to_vec())?;
            candidate.entries.insert(path.clone(), entry);
        }
    }
    if path_collisions(&candidate.entries).next().is_some() {
        return Err(Error::Engine(
            "El resultado contiene una colisión entre un archivo y una carpeta".into(),
        ));
    }
    Ok(())
}

fn read_version(
    repo: &gix::Repository,
    entry: Option<&Entry>,
    generated: &Blobs,
    limits: Limits,
    budget: &mut Budget,
    ctx: &Context<'_>,
) -> Result<Option<Vec<u8>>, Error> {
    let Some(entry) = entry else {
        return Ok(None);
    };
    if let Some(bytes) = generated.get(&entry.id) {
        budget.charge(bytes.len())?;
        return Ok(Some(bytes.clone()));
    }
    let mut blob = revision::verified(repo, entry.id, limits.max_blob, ctx)?;
    if blob.kind != Kind::Blob {
        return Err(Error::Unsupported);
    }
    budget.charge(blob.data.len())?;
    Ok(Some(std::mem::take(&mut blob.data)))
}

fn add_result_blob(
    repo: &gix::Repository,
    blobs: &mut Blobs,
    bytes: Vec<u8>,
) -> Result<Entry, Error> {
    let id = gix::objs::compute_hash(repo.object_hash(), Kind::Blob, &bytes).map_err(engine)?;
    blobs.entry(id).or_insert(bytes);
    Ok(Entry { id, mode: 0o100644 })
}

fn path_collisions(entries: &Entries) -> impl Iterator<Item = &[u8]> {
    entries.keys().flat_map(|path| {
        path.iter().enumerate().filter_map(|(offset, byte)| {
            (*byte == b'/' && entries.contains_key(&path[..offset])).then_some(&path[..offset])
        })
    })
}

fn contains_path(root: &[u8], path: &[u8]) -> bool {
    path == root
        || path
            .strip_prefix(root)
            .is_some_and(|tail| tail.starts_with(b"/"))
}

fn subtree(entries: &Entries, path: &[u8]) -> Entries {
    let mut subtree = Entries::new();
    if let Some(entry) = entries.get(path) {
        subtree.insert(path.to_vec(), *entry);
    }
    let prefix = [path, b"/"].concat();
    subtree.extend(
        entries
            .range(prefix.clone()..)
            .take_while(|(key, _)| key.starts_with(&prefix))
            .map(|(key, entry)| (key.clone(), *entry)),
    );
    subtree
}

type UniqueFiles<'a> = BTreeMap<ObjectId, Option<&'a [u8]>>;

fn unique_files<'a>(tree: &'a Entries, ctx: &Context<'_>) -> Result<UniqueFiles<'a>, Error> {
    let mut unique = BTreeMap::new();
    for (path, entry) in tree {
        ctx.check()?;
        unique
            .entry(entry.id)
            .and_modify(|seen| *seen = None)
            .or_insert_with(|| (entry.mode == 0o100644).then_some(path.as_slice()));
    }
    Ok(unique)
}

/// A rename is inferred only from a unique, unchanged blob and a new name.
/// Independent edits at a competing destination stay outside this choice.
fn rename_conflicts(
    trees: [&Entries; 3],
    added: [bool; 2],
    structural: &[Vec<u8>],
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<BTreeMap<Vec<u8>, MergeConflict>, Error> {
    let [base, ours, theirs] = trees;
    let [original, left, right] = trees.map(|tree| unique_files(tree, ctx));
    let (original, left, right) = (original?, left?, right?);
    let mut conflicts = BTreeMap::new();
    for (id, origin) in original {
        ctx.check()?;
        let Some(origin) = origin else { continue };
        let [current, incoming] = [(ours, &left), (theirs, &right)].map(|(side, unique)| {
            let dest = unique.get(&id).copied().flatten()?;
            (!side.contains_key(origin) && !base.contains_key(dest)).then_some(dest)
        });
        if current == incoming
            || current.is_some_and(|path| theirs.contains_key(path))
            || incoming.is_some_and(|path| ours.contains_key(path))
        {
            continue;
        }
        // A branch with other additions and no exact match might have moved
        // and edited the file. Keep the existing conservative conflict there.
        let deleted =
            |side: &Entries, has_additions: bool| !side.contains_key(origin) && !has_additions;
        if (current.is_none() && !deleted(ours, added[0]))
            || (incoming.is_none() && !deleted(theirs, added[1]))
        {
            continue;
        }
        let paths = [Some(origin), current, incoming];
        if paths.into_iter().flatten().any(|path| {
            structural
                .iter()
                .any(|root| contains_path(root, path) || contains_path(path, root))
        }) {
            continue;
        }
        let alternatives = [base, ours, theirs].map(|tree| {
            paths
                .into_iter()
                .flatten()
                .filter_map(|path| tree.get(path).map(|entry| (path.to_vec(), *entry)))
                .collect()
        });
        conflicts.insert(
            origin.to_vec(),
            MergeConflict {
                versions: None,
                paths: Some(PathChoices {
                    kind: PathConflict::Rename,
                    alternatives,
                }),
            },
        );
        if conflicts.len() > limits.max_entries.min(MAX_PATHS) {
            return Err(Error::Limited);
        }
    }
    Ok(conflicts)
}

fn combine(
    repo: &gix::Repository,
    trees: [Option<ObjectId>; 3],
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<Candidate, Error> {
    let mut budget = Budget::new(limits);
    let [base, ours, theirs] = trees.map(|id| {
        id.map(|id| revision::merge_tree_entries(repo, id, limits, &mut budget, ctx))
            .transpose()
            .map(Option::unwrap_or_default)
    });
    let (base, ours, theirs) = (base?, ours?, theirs?);
    combine_entries(
        repo,
        [&base, &ours, &theirs],
        &Blobs::new(),
        limits,
        &mut budget,
        ctx,
    )
}

fn combine_entries(
    repo: &gix::Repository,
    [base, ours, theirs]: [&Entries; 3],
    generated: &Blobs,
    limits: Limits,
    budget: &mut Budget,
    ctx: &Context<'_>,
) -> Result<Candidate, Error> {
    let added = [ours, theirs].map(|side| side.keys().any(|path| !base.contains_key(path)));
    let [left_moves, right_moves] =
        [ours, theirs].map(|side| base.keys().any(|path| !side.contains_key(path)));
    let (left_moves, right_moves) = (left_moves && added[0], right_moves && added[1]);
    let mut candidate = Candidate {
        entries: BTreeMap::new(),
        blobs: BTreeMap::new(),
        conflicts: BTreeMap::new(),
    };
    let mut structural = BTreeSet::new();
    for (files, folders) in [(ours, theirs), (theirs, ours)] {
        for path in folders.keys() {
            ctx.check()?;
            for (offset, byte) in path.iter().enumerate() {
                if *byte == b'/' && files.contains_key(&path[..offset]) {
                    structural.insert(path[..offset].to_vec());
                }
            }
        }
    }
    for path in structural {
        ctx.check()?;
        let shapes = [base, ours, theirs].map(|entries| subtree(entries, &path));
        if shapes[0] == shapes[1] || shapes[0] == shapes[2] {
            continue;
        }
        let ordinary = shapes
            .iter()
            .flatten()
            .all(|(_, entry)| entry.mode == 0o100644);
        candidate.conflicts.insert(
            path,
            MergeConflict {
                versions: None,
                paths: ordinary.then_some(PathChoices {
                    kind: PathConflict::FileDirectory,
                    alternatives: shapes,
                }),
            },
        );
        if candidate.conflicts.len() > limits.max_entries.min(MAX_PATHS) {
            return Err(Error::Limited);
        }
    }
    let structural: Vec<_> = candidate.conflicts.keys().cloned().collect();
    let renames = if left_moves || right_moves {
        rename_conflicts([base, ours, theirs], added, &structural, limits, ctx)?
    } else {
        BTreeMap::new()
    };
    let renamed_paths: BTreeSet<_> = renames
        .values()
        .filter_map(|conflict| conflict.paths.as_ref())
        .flat_map(|choices| choices.alternatives.iter().flat_map(Entries::keys))
        .cloned()
        .collect();
    candidate.conflicts.extend(renames);
    let paths: BTreeSet<_> = base
        .keys()
        .chain(ours.keys())
        .chain(theirs.keys())
        .collect();
    let different_sides = (left_moves || right_moves) && ours != theirs;
    for path in paths {
        ctx.check()?;
        if renamed_paths.contains(path) || structural.iter().any(|root| contains_path(root, path)) {
            continue;
        }
        let (before, left, right) = (base.get(path), ours.get(path), theirs.get(path));
        let conflicting_move = before.is_some()
            && left != before
            && right != before
            && ((left_moves && left.is_none()) || (right_moves && right.is_none()))
            && different_sides;
        let entry = if !conflicting_move && (left == right || right == before) {
            left.copied()
        } else if !conflicting_move && left == before {
            right.copied()
        } else if !conflicting_move
            && [before, left, right]
                .into_iter()
                .flatten()
                .all(|entry| entry.mode == 0o100644)
        {
            let versions = [
                read_version(repo, before, generated, limits, budget, ctx)?,
                read_version(repo, left, generated, limits, budget, ctx)?,
                read_version(repo, right, generated, limits, budget, ctx)?,
            ];
            let merged = match versions.each_ref().map(Option::as_deref) {
                [Some(base), Some(current), Some(incoming)] => {
                    match merge_bytes(base, current, incoming, ctx) {
                        Ok(result) => result,
                        Err(Error::Limited) => None,
                        Err(error) => return Err(error),
                    }
                }
                _ => None,
            };
            let Some(bytes) = merged else {
                candidate.conflicts.insert(
                    path.clone(),
                    MergeConflict {
                        versions: Some(versions),
                        paths: None,
                    },
                );
                continue;
            };
            if bytes.len() > limits.max_blob {
                return Err(Error::Limited);
            }
            budget.charge(bytes.len())?;
            Some(add_result_blob(repo, &mut candidate.blobs, bytes)?)
        } else {
            candidate.conflicts.insert(
                path.clone(),
                MergeConflict {
                    versions: None,
                    paths: None,
                },
            );
            continue;
        };
        if let Some(entry) = entry {
            candidate.entries.insert(path.clone(), entry);
        }
    }
    // A flat merge can otherwise accept both sides of a file/directory collision.
    for path in path_collisions(&candidate.entries) {
        ctx.check()?;
        candidate.conflicts.insert(
            path.to_vec(),
            MergeConflict {
                versions: None,
                paths: None,
            },
        );
    }
    if candidate.conflicts.len() > limits.max_entries.min(MAX_PATHS) {
        return Err(Error::Limited);
    }
    Ok(candidate)
}

fn make_trees(
    repo: &gix::Repository,
    entries: &Entries,
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<(ObjectId, Vec<(ObjectId, Tree)>), Error> {
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
    let mut budget = Budget::new(limits);
    let id = editor
        .write(|tree| {
            ctx.check()?;
            let mut data = Vec::new();
            tree.write_to(&mut data)?;
            budget.charge(data.len())?;
            let id =
                gix::objs::compute_hash(repo.object_hash(), Kind::Tree, &data).map_err(engine)?;
            trees.push((id, tree.clone()));
            Ok::<_, Error>(id)
        })
        .map_err(engine)?;
    Ok((id, trees))
}

/// Pin both tips, the active branch, the index and the affected worktree paths.
/// Conflict reviews are read-only and cannot be passed to publication.
pub fn prepare_merge(
    root: &Path,
    incoming: &str,
    expected_tip: &str,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<MergePlan, Error> {
    let source = Source::Branch(
        ref_write::reference_name(RefKind::Branch, incoming, limits)?,
        false,
    );
    let theirs = ObjectId::from_hex(expected_tip.as_bytes()).map_err(engine)?;
    prepare(root, source, theirs, limits, None, cancelled)
}

pub fn prepare_rebase(
    root: &Path,
    onto: &str,
    expected_tip: &str,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<MergePlan, Error> {
    let source = Source::Branch(
        ref_write::reference_name(RefKind::Branch, onto, limits)?,
        true,
    );
    let theirs = ObjectId::from_hex(expected_tip.as_bytes()).map_err(engine)?;
    prepare(root, source, theirs, limits, None, cancelled)
}

pub fn prepare_cherry_pick(
    root: &Path,
    commit: &str,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<MergePlan, Error> {
    prepare_commit_operation(root, commit, CommitOperation::CherryPick, limits, cancelled)
}

pub fn prepare_reset(
    root: &Path,
    commit: &str,
    mode: ResetMode,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<MergePlan, Error> {
    let target = ObjectId::from_hex(commit.as_bytes()).map_err(engine)?;
    prepare(
        root,
        Source::Reset(mode, target.to_string()),
        target,
        limits,
        None,
        cancelled,
    )
}

pub fn prepare_commit_operation(
    root: &Path,
    commit: &str,
    operation: CommitOperation,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<MergePlan, Error> {
    let theirs = ObjectId::from_hex(commit.as_bytes()).map_err(engine)?;
    prepare(
        root,
        Source::Commit {
            operation,
            id: theirs.to_string(),
            mainline: None,
        },
        theirs,
        limits,
        None,
        cancelled,
    )
}

/// Review the delta against the selected parent of a merge commit.
pub fn prepare_commit_mainline(
    review: MergeReview,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<MergePlan, Error> {
    if review.source.mainline().is_none() {
        return Err(Error::Stale);
    }
    prepare_review(&review, None, cancelled)
}

pub use prepare_commit_mainline as prepare_cherry_pick_mainline;

/// Build a fresh publication preview from explicit choices for all reviewed paths.
pub fn prepare_resolved_merge(
    review: MergeReview,
    choices: BTreeMap<Vec<u8>, MergeResolution>,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<MergePlan, Error> {
    prepare_review(&review, Some(choices), cancelled)
}

type Resolutions = BTreeMap<Vec<u8>, MergeResolution>;

fn prepare_review(
    review: &MergeReview,
    choices: Option<Resolutions>,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<MergePlan, Error> {
    prepare(
        &review.repository.root,
        review.source.clone(),
        review.theirs,
        review.limits,
        Some((review, choices)),
        cancelled,
    )
}

fn prepare(
    root: &Path,
    mut source: Source,
    theirs: ObjectId,
    limits: Limits,
    review: Option<(&MergeReview, Option<Resolutions>)>,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<MergePlan, Error> {
    if !cfg!(windows) {
        return Err(Error::Unsupported);
    }
    limits.admit()?;
    controlled(limits.timeout, cancelled, |ctx| {
        let (repo, repository) = open(root, ctx)?;
        let (active, refs) = refs(&repo, limits, ctx)?;
        if let Source::Branch(incoming, _) = &source {
            if ref_write::namespace_conflicts(active.as_bstr().as_bytes(), name_text(incoming)) {
                return Err(Error::Engine("Elige otra rama local".into()));
            }
            if direct_branch(&refs, incoming)? != theirs {
                return Err(Error::Stale);
            }
        }
        let head = direct_branch(&refs, &active)?;
        let head_file = capture_head(&repository.git_dir, &active, ctx)?;
        let pending = Journal::read(&repository, ctx)?;
        let active_short = &name_text(&active)[HEADS.len()..];
        if let Some(journal) = &pending {
            if journal.aborting {
                return Err(Error::Engine(
                    "Hay un aborto pendiente. Elige Abortar operación para continuarlo".into(),
                ));
            }
            if journal.incoming.is_none() {
                return Err(Error::Engine(
                    "Hay un cambio de rama pendiente de recuperación".into(),
                ));
            }
            if journal.source != active_short
                || journal.commit_operation != source.operation()
                || journal.rebase != source.is_rebase()
                || journal.reset != source.reset_mode()
                || journal.destination != source.label()
                || journal.incoming != Some(theirs)
                || (head != journal.from && head != journal.to)
            {
                return Err(Error::Engine(format!(
                    "Hay una operación pendiente con {}. Elige la misma operación y el mismo origen, o selecciona Abortar",
                    journal.destination
                )));
            }
            if let Source::Commit {
                mainline: parent, ..
            } = &mut source
            {
                if parent.is_some() && *parent != journal.mainline {
                    return Err(Error::Stale);
                }
                *parent = journal.mainline;
            }
            if journal.reset == Some(ResetMode::Hard) {
                return Err(Error::Engine(
                    "Hay un reset hard interrumpido. Elige Abortar antes de volver a intentarlo"
                        .into(),
                ));
            }
            if journal.rebase {
                return Err(Error::Engine(
                    "Hay un rebase interrumpido. Elige Abortar antes de volver a intentarlo".into(),
                ));
            }
        }
        let from = pending.as_ref().map_or(head, |journal| journal.from);
        let (index, index_bytes) = local_pair::index_capture(&repo, &repository.git_dir, ctx)?;
        if let Some((review, _)) = &review {
            index_write::same_repository(&review.repository, &repository)?;
            if pending.is_some()
                || active != review.active
                || from != review.from
                || head_file != review.head_file
                || index != review.index
            {
                return Err(Error::Stale);
            }
        }
        let (review, resolution) =
            review.map_or((None, None), |(review, choices)| (Some(review), choices));
        if !matches!(
            source.reset_mode(),
            Some(ResetMode::Mixed | ResetMode::Hard)
        ) {
            let state = local_pair::decode_index(&repo, index_bytes.as_deref(), ctx)?;
            if state.entries().iter().any(|entry| entry.stage_raw() != 0) {
                return Err(Error::Unmerged);
            }
        }
        let write_reflog = repo.refs.write_reflog != WriteReflog::Disable;
        let mut plan = MergePlan {
            kind: MergeKind::UpToDate,
            rows: Vec::new(),
            conflicts: BTreeMap::new(),
            mainline_choices: Vec::new(),
            rebase_steps: Vec::new(),
            rebase_review: None,
            repository,
            limits,
            active,
            source,
            from,
            theirs,
            result: None,
            head_file,
            index,
            checkout: None,
            objects: None,
            completed: false,
            write_reflog,
        };
        if let Some(mode) = plan.reset_mode() {
            let target_tree = tree_id(&repo, theirs, limits, ctx)?;
            let old_tree = if mode != ResetMode::Mixed {
                tree_id(&repo, from, limits, ctx)?
            } else {
                target_tree
            };
            if mode == ResetMode::Soft && from == theirs && pending.is_none() {
                return Ok(plan);
            }
            let (checkout, rows) = Checkout::prepare_reset(
                &repo,
                &plan.repository,
                ResetTransition {
                    mode,
                    branch: plan.active(),
                    commits: (from, theirs),
                    trees: (old_tree, target_tree),
                },
                (&plan.index, index_bytes.as_deref()),
                pending,
                limits,
                ctx,
            )?;
            if from == theirs && rows.is_empty() && !checkout.resuming {
                return Ok(plan);
            }
            plan.completed = head == theirs;
            if from != theirs && plan.completed && !checkout.index_published(&plan.index) {
                return Err(Error::Stale);
            }
            plan.rows = rows;
            plan.checkout = Some(checkout);
            plan.result = Some((theirs, target_tree));
            return Ok(plan);
        }
        if plan.is_rebase() {
            let continuation = resolution
                .map(|choices| {
                    let review = review
                        .and_then(|review| review.rebase_review.as_ref())
                        .ok_or(Error::Stale)?;
                    Ok::<_, Error>((review, choices))
                })
                .transpose()?;
            rebase::review(
                &repo,
                &mut plan,
                index_bytes.as_deref().ok_or(Error::Unsupported)?,
                continuation,
                ctx,
            )?;
            return Ok(plan);
        }
        let operation = plan.operation();
        let picked = if operation.is_some() {
            let commit = revision::commit(&repo, theirs, limits, &mut Budget::new(limits), ctx)?;
            if commit.limited {
                return Err(Error::Limited);
            }
            if commit.parents.len() > 1 && plan.source.mainline().is_none() {
                if pending.is_some() || resolution.is_some() {
                    return Err(Error::Stale);
                }
                plan.kind = MergeKind::ChooseMainline;
                plan.mainline_choices = commit.parents;
                return Ok(plan);
            }
            Some(commit)
        } else {
            None
        };
        let common = if let Some(commit) = &picked {
            if let Some(parent) = plan.source.mainline() {
                if commit.parents.len() < 2
                    || commit.parents.get(parent.number - 1) != Some(&parent.id)
                {
                    return Err(Error::Stale);
                }
                Some(parent.id)
            } else {
                commit.parents.first().copied()
            }
        } else {
            Some(base(&repo, from, theirs, limits, ctx)?)
        };
        if operation.is_none() && common == Some(theirs) {
            if pending.is_some() || resolution.is_some() {
                return Err(Error::Stale);
            }
            return Ok(plan);
        }
        let old_tree = tree_id(&repo, from, limits, ctx)?;
        let their_tree = if let Some(commit) = picked {
            commit.tree
        } else {
            tree_id(&repo, theirs, limits, ctx)?
        };
        let (result, tree, entries, blobs) = if operation.is_none() && common == Some(from) {
            if resolution.is_some() {
                return Err(Error::Stale);
            }
            plan.kind = MergeKind::FastForward;
            let entries =
                revision::tree_entries(&repo, their_tree, limits, &mut Budget::new(limits), ctx)?;
            (theirs, their_tree, entries, BTreeMap::new())
        } else {
            plan.kind = MergeKind::Commit;
            let base_tree = common
                .map(|id| tree_id(&repo, id, limits, ctx))
                .transpose()?;
            let (base_tree, target_tree) = if operation == Some(CommitOperation::Revert) {
                (Some(their_tree), base_tree)
            } else {
                (base_tree, Some(their_tree))
            };
            let mut parents = vec![from];
            if operation.is_none() {
                parents.push(theirs);
            }
            let mut candidate =
                combine(&repo, [base_tree, Some(old_tree), target_tree], limits, ctx)?;
            if let Some(journal) = &pending {
                let Candidate {
                    entries, conflicts, ..
                } = candidate;
                let commit =
                    revision::commit(&repo, journal.to, limits, &mut Budget::new(limits), ctx)?;
                if commit.parents != parents || commit.limited {
                    return Err(Error::Stale);
                }
                if conflicts.is_empty()
                    && make_trees(&repo, &entries, limits, ctx)?.0 != commit.tree
                {
                    return Err(Error::Stale);
                }
                let stored = revision::merge_tree_entries(
                    &repo,
                    commit.tree,
                    limits,
                    &mut Budget::new(limits),
                    ctx,
                )?;
                // Only paths offered by the conflict review can differ from the
                // automatic merge. The journal pins the exact resolved commit.
                if conflicts.values().any(|conflict| !conflict.can_resolve())
                    || conflicts.iter().any(|(path, conflict)| {
                        conflict
                            .paths
                            .as_ref()
                            .is_some_and(|choices| !choices.matches(path, &stored))
                    })
                    || stored.iter().any(|(path, entry)| {
                        if conflicts.contains_key(path) {
                            entry.mode != 0o100644
                        } else if conflicts.iter().any(|(root, conflict)| {
                            conflict
                                .paths
                                .as_ref()
                                .is_some_and(|choices| choices.covers(root, path))
                        }) {
                            false
                        } else {
                            entries.get(path) != Some(entry)
                        }
                    })
                    || entries.keys().any(|path| !stored.contains_key(path))
                {
                    return Err(Error::Stale);
                }
                (journal.to, commit.tree, stored, BTreeMap::new())
            } else {
                if let Some(choices) = resolution {
                    resolve_candidate(
                        &repo,
                        &mut candidate,
                        &choices,
                        limits,
                        &mut Budget::new(limits),
                        ctx,
                    )?;
                } else if !candidate.conflicts.is_empty() {
                    plan.kind = MergeKind::Conflicts;
                    plan.rows = candidate
                        .conflicts
                        .keys()
                        .cloned()
                        .map(|path| MergeRow { path, change: None })
                        .collect();
                    plan.conflicts = candidate.conflicts;
                    return Ok(plan);
                }
                let Candidate { entries, blobs, .. } = candidate;
                let (tree, trees) = make_trees(&repo, &entries, limits, ctx)?;
                if operation.is_some() && tree == old_tree {
                    plan.kind = MergeKind::UpToDate;
                    return Ok(plan);
                }
                commit_write::check_signing(&repo)?;
                let signature = commit_write::local_identity(&repo)?;
                let commit = if plan.is_cherry_pick() {
                    let original = revision::verified(
                        &repo,
                        theirs,
                        limits.max_bytes.min(crate::OBJECT_BYTES as usize),
                        ctx,
                    )?
                    .try_into_commit()
                    .map_err(engine)?
                    .decode()
                    .map_err(engine)?
                    .into_owned()
                    .map_err(engine)?;
                    Commit {
                        tree,
                        parents: parents.into_iter().collect(),
                        committer: signature,
                        extra_headers: Vec::new(),
                        ..original
                    }
                } else {
                    let message = if operation == Some(CommitOperation::Revert) {
                        let short = theirs.to_hex_with_len(12);
                        let mut message =
                            format!("Revertir {short}\n\nRevierte el commit {theirs}.\n");
                        if let Some((number, parent)) = plan.mainline() {
                            message.push_str(&format!("Padre principal {number}: {parent}.\n"));
                        }
                        message
                    } else {
                        format!(
                            "Fusionar rama '{}' en '{}'\n",
                            plan.incoming(),
                            plan.active()
                        )
                    };
                    Commit {
                        tree,
                        parents: parents.into_iter().collect(),
                        author: signature.clone(),
                        committer: signature,
                        encoding: None,
                        extra_headers: Vec::new(),
                        message: message.into(),
                    }
                };
                let mut bytes = Vec::new();
                commit.write_to(&mut bytes)?;
                let id = gix::objs::compute_hash(repo.object_hash(), Kind::Commit, &bytes)
                    .map_err(engine)?;
                plan.objects = Some(Objects {
                    blobs: blobs.clone(),
                    trees,
                    commits: vec![(id, commit)],
                });
                (id, tree, entries, blobs)
            }
        };
        if pending.as_ref().is_some_and(|journal| journal.to != result) {
            return Err(Error::Stale);
        }
        if write_reflog && head != result {
            commit_write::local_identity(&repo)?;
        }
        let checkout = Checkout::prepare(
            &repo,
            &plan.repository,
            Transition {
                branches: (plan.active(), plan.incoming()),
                commits: (from, result),
                trees: (old_tree, tree),
                merge: Some(MergeCheckout {
                    incoming: theirs,
                    commit_operation: operation,
                    rebase: false,
                    mainline: plan.source.mainline(),
                    entries: &entries,
                    blobs: &blobs,
                }),
            },
            index_bytes.as_deref().ok_or(Error::Unsupported)?,
            pending,
            limits,
            ctx,
        )?;
        plan.completed = head == result;
        if plan.completed && !checkout.index_published(&plan.index) {
            return Err(Error::Stale);
        }
        plan.rows = checkout
            .files
            .iter()
            .map(|file| MergeRow {
                path: file.path().to_vec(),
                change: Some(file.change()),
            })
            .collect();
        plan.checkout = Some(checkout);
        plan.result = Some((result, tree));
        Ok(plan)
    })
}

/// Publish a confirmed plan. The durable journal permits resuming the pinned
/// merge or reviewing its rollback after an interruption.
pub fn apply_merge(
    plan: MergePlan,
    mut cancelled: impl FnMut() -> bool,
) -> Result<MergePublication, Error> {
    let (Some(checkout), Some(result)) = (&plan.checkout, plan.result) else {
        return Err(Error::Engine("Esta revisión no admite confirmación".into()));
    };
    publish(&plan, checkout, result, &mut cancelled).map_err(|error| {
        if plan
            .repository
            .git_dir
            .join(crate::branch_checkout::JOURNAL_NAME)
            .exists()
        {
            let origin = match (plan.operation(), plan.reset_mode()) {
                (Some(CommitOperation::CherryPick), _) => "Cherry-pick en el commit",
                (Some(CommitOperation::Revert), _) => "Revertir en el commit",
                (_, Some(ResetMode::Soft)) => "Reset soft en el commit",
                (_, Some(ResetMode::Mixed)) => "Reset mixed en el commit",
                (_, Some(ResetMode::Hard)) => {
                    return Error::Engine(format!(
                        "{error}. Elige Abortar para recuperar el reset hard"
                    ));
                }
                _ if plan.is_rebase() => {
                    return Error::Engine(format!(
                        "{error}. Elige Abortar para recuperar el rebase"
                    ));
                }
                _ => "Fusionar en la rama",
            };
            Error::Engine(format!(
                "{error}. Elige Abortar o vuelve a usar {origin} {} para reanudar",
                plan.incoming()
            ))
        } else {
            error
        }
    })
}

fn publish(
    plan: &MergePlan,
    checkout: &Checkout,
    (result, result_tree): (ObjectId, ObjectId),
    cancelled: &mut impl FnMut() -> bool,
) -> Result<MergePublication, Error> {
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
    let git = &plan.repository.git_dir;
    let hold = |name: &str, boundary| {
        Marker::acquire_to_hold_resource(git.join(name), Fail::Immediately, boundary, 0)
            .map_err(engine)
    };
    let expected_head = if plan.completed { result } else { plan.from };
    let verify_refs = |repo: &gix::Repository, expected| -> Result<(), Error> {
        let (active, refs) = refs(repo, plan.limits, &ctx)?;
        if active != plan.active
            || direct_branch(&refs, &plan.active)? != expected
            || capture_head(git, &plan.active, &ctx)? != plan.head_file
        {
            return Err(Error::Stale);
        }
        if let Source::Branch(incoming, _) = &plan.source
            && direct_branch(&refs, incoming)? != plan.theirs
        {
            return Err(Error::Stale);
        }
        Ok(())
    };
    check()?;
    let (repo, identity) = open(&plan.repository.root, &ctx)?;
    index_write::same_repository(&plan.repository, &identity)?;
    if local_pair::index_capture(&repo, git, &ctx)?.0 != plan.index {
        return Err(Error::Stale);
    }
    let mut index_lock = Some(
        File::acquire_to_update_resource(git.join("index"), Fail::Immediately, None, 0)
            .map_err(engine)?,
    );
    let _head = hold("HEAD", None)?;
    let _config = hold("config", None)?;
    let _packed = hold("packed-refs", None)?;
    let active = name_text(&plan.active);
    ref_write::ordinary_path_absent(git, active, &ctx)?;
    let _incoming = if let Source::Branch(incoming, _) = &plan.source {
        let incoming = name_text(incoming);
        ref_write::ordinary_path_absent(git, incoming, &ctx)?;
        Some(hold(incoming, Some(git.join("refs")))?)
    } else {
        None
    };
    let mut branch = File::acquire_to_update_resource(
        git.join(active),
        Fail::Immediately,
        Some(git.join("refs")),
        0,
    )
    .map_err(engine)?;
    let (fresh, identity) = open(&plan.repository.root, &ctx)?;
    index_write::same_repository(&plan.repository, &identity)?;
    verify_refs(&fresh, expected_head)?;
    let (index, index_bytes) = local_pair::index_capture(&fresh, git, &ctx)?;
    if !index.matches_under_lock(&plan.index)
        || (fresh.refs.write_reflog != WriteReflog::Disable) != plan.write_reflog
    {
        return Err(Error::Stale);
    }
    let original = if plan.reset_mode() == Some(ResetMode::Soft) {
        index_bytes.as_deref().unwrap_or_default()
    } else {
        index_bytes.as_deref().ok_or(Error::Stale)?
    };
    checkout.validate(&fresh, &identity, original, plan.limits, &ctx)?;
    if let Some(objects) = &plan.objects {
        commit_write::check_signing(&fresh)?;
        let signature = commit_write::local_identity(&fresh)?;
        if objects.commits.iter().any(|(_, commit)| {
            signature.name != commit.committer.name || signature.email != commit.committer.email
        }) {
            return Err(Error::Stale);
        }
        for (id, bytes) in &objects.blobs {
            check()?;
            commit_write::store_object(&fresh, git, *id, &gix::objs::BlobRef { data: bytes })?;
        }
        for (id, tree) in &objects.trees {
            check()?;
            commit_write::store_object(&fresh, git, *id, tree)?;
        }
        for (id, commit) in &objects.commits {
            check()?;
            commit_write::store_object(&fresh, git, *id, commit)?;
        }
    }
    if tree_id(&fresh, result, plan.limits, &ctx)? != result_tree {
        return Err(Error::Stale);
    }
    revision::verified_tree(
        &fresh,
        result_tree,
        plan.limits,
        &mut Budget::new(plan.limits),
        &ctx,
    )?;
    let logs = if plan.write_reflog && !plan.completed {
        let line = gix::refs::log::Line {
            previous_oid: plan.from,
            new_oid: result,
            signature: commit_write::local_identity(&fresh)?,
            message: match (plan.operation(), plan.reset_mode()) {
                (Some(CommitOperation::CherryPick), _) => {
                    format!("cherry-pick: {}", plan.incoming())
                }
                (Some(CommitOperation::Revert), _) => format!("revert: {}", plan.incoming()),
                (_, Some(mode)) => format!("reset --{}: {}", mode.label(), plan.incoming()),
                _ if plan.is_rebase() => format!("rebase sobre {}", plan.incoming()),
                _ => format!("merge {}: {}", plan.incoming(), plan.kind.label()),
            }
            .into(),
        };
        let branch_log = Path::new("logs").join(active);
        Some(
            [branch_log, Path::new("logs/HEAD").into()]
                .map(|path| {
                    let (lock, token) = commit_write::stage_log_with_source(git, &path, &line)?;
                    Ok::<_, Error>((path, lock, token))
                })
                .into_iter()
                .collect::<Result<Vec<_>, _>>()?,
        )
    } else {
        None
    };
    if !plan.completed {
        writeln!(branch, "{result}")?;
        branch.with_mut(|file| file.sync_all())?;
    }
    if let Some(bytes) = &checkout.index_bytes {
        let lock = index_lock.as_mut().ok_or(Error::Stale)?;
        lock.write_all(bytes)?;
        lock.with_mut(|file| file.sync_all())?;
    }
    check()?;
    checkout.save_original_index(&identity, original, &ctx)?;
    let index_backup = checkout.journal.retained_reset_index(&identity);
    if index_backup.is_some() && checkout.journal.original_index(&identity, &ctx)?.is_none() {
        return Err(Error::Stale);
    }
    checkout.publish_files(&identity, plan.limits, &ctx, &mut check)?;
    let mut index_again = None;
    if checkout.index_bytes.is_some() {
        check()?;
        if local_pair::index_capture(&fresh, git, &ctx)?.0 != plan.index {
            return Err(Error::Stale);
        }
        index_lock
            .take()
            .ok_or(Error::Stale)?
            .commit()
            .map_err(engine)?;
        index_again = Some(hold("index", None)?);
    }
    if !checkout.index_published(&local_pair::index_capture(&fresh, git, &ctx)?.0) {
        return Err(Error::Stale);
    }
    check()?;
    let _branch_again;
    if !plan.completed {
        branch.commit().map_err(engine)?;
        _branch_again = hold(active, Some(git.join("refs")))?;
        let (fresh, identity) = open(&plan.repository.root, &ctx)?;
        index_write::same_repository(&plan.repository, &identity)?;
        verify_refs(&fresh, result)?;
    }
    check()?;
    let mut reflog_warning = None;
    for (path, lock, before) in logs.into_iter().flatten() {
        let write = || -> Result<(), Error> {
            let current = match commit_write::capture_log(git, &git.join(path))? {
                jc_fs::Capture::Present { token, .. } => Some(token),
                jc_fs::Capture::Absent { .. } => None,
            };
            if current != before {
                return Err(Error::Stale);
            }
            lock.commit().map_err(engine)?;
            Ok(())
        };
        if let Err(error) = write() {
            reflog_warning = Some(format!(
                "Operación publicada · {result}. Falló el reflog ({error})"
            ));
        }
    }
    checkout.finish(&identity, &ctx)?;
    drop(index_again);
    Ok(MergePublication {
        id: result,
        reflog_warning,
        index_backup,
        hard_backup: checkout.journal.hard_backup_path(&identity),
    })
}
