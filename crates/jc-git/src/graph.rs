//! Bounded reference snapshots and pinned, reverse-topological commit pages.
use super::{Context, Error, Identity, Limits, controlled, engine, open};
use crate::{
    path_history::shallow,
    revision::{Budget, verified},
};
use gix::bstr::ByteSlice;
use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet, BinaryHeap, VecDeque},
    fs, io,
    path::Path,
    sync::Arc,
};

const MAX_REFS: usize = 4096;
const MAX_REF_VISITS: usize = 8192;
const MAX_REF_DEPTH: usize = 128;
const PACKED_BYTES: u64 = 1024 * 1024;
const LOOSE_BYTES: u64 = 8 * 1024;
const MAX_LINKS: usize = 8;
const MAX_NODES: usize = 512;
const MAX_PARENTS: usize = 8;
const MAX_OBJECT_BYTES: usize = 64 * 1024;
const PAGE: usize = 24;
const STASH_LOG_BYTES: usize = 64 * 1024;
const STASH_MESSAGE_BYTES: usize = 384;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Head {
    Unborn { name: Vec<u8> },
    Attached { name: Vec<u8>, target: String },
    Detached { target: String },
}

impl Head {
    pub(super) fn target(&self) -> Option<&str> {
        match self {
            Self::Unborn { .. } => None,
            Self::Attached { target, .. } | Self::Detached { target } => Some(target),
        }
    }

    fn memory_bytes(&self) -> usize {
        match self {
            Self::Unborn { name } => name.capacity(),
            Self::Attached { name, target } => name.capacity() + target.capacity(),
            Self::Detached { target } => target.capacity(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefKind {
    LocalBranch,
    RemoteBranch,
    Tag,
    Stash,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RefTarget {
    /// Raw object OID, including the tag object for an annotated tag.
    Direct(String),
    Symbolic(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefEntry {
    /// Full Git name, retained without character decoding.
    pub name: Vec<u8>,
    pub kind: RefKind,
    pub target: RefTarget,
}

#[derive(Debug, PartialEq, Eq)]
pub struct StashEntry {
    pub id: gix::ObjectId,
    pub message: Vec<u8>,
}

#[derive(Default)]
pub struct StashList {
    pub entries: Vec<StashEntry>,
    pub incomplete: bool,
}

impl StashList {
    fn memory_bytes(&self) -> usize {
        self.entries.capacity() * std::mem::size_of::<StashEntry>()
            + self
                .entries
                .iter()
                .map(|entry| entry.message.capacity())
                .sum::<usize>()
    }
}

pub struct RefSnapshot {
    pub repository: Identity,
    pub head: Head,
    pub refs: Vec<RefEntry>,
}

impl RefSnapshot {
    pub fn memory_bytes(&self) -> usize {
        self.repository.root.capacity()
            + self.repository.git_dir.capacity()
            + self.head.memory_bytes()
            + self.refs.capacity() * std::mem::size_of::<RefEntry>()
            + self
                .refs
                .iter()
                .map(|entry| {
                    entry.name.capacity()
                        + match &entry.target {
                            RefTarget::Direct(id) => id.capacity(),
                            RefTarget::Symbolic(name) => name.capacity(),
                        }
                })
                .sum::<usize>()
    }
}

fn ref_kind(name: &[u8]) -> RefKind {
    if name.starts_with(b"refs/heads/") {
        RefKind::LocalBranch
    } else if name.starts_with(b"refs/remotes/") {
        RefKind::RemoteBranch
    } else if name.starts_with(b"refs/tags/") {
        RefKind::Tag
    } else if name == b"refs/stash" {
        RefKind::Stash
    } else {
        RefKind::Other
    }
}

fn checked_name(name: &[u8], limits: Limits, budget: &mut Budget) -> Result<Vec<u8>, Error> {
    if name.len() > limits.max_path {
        return Err(Error::Limited);
    }
    budget.charge(name.len())?;
    Ok(name.to_vec())
}

fn id_text(id: &gix::hash::oid) -> Result<String, Error> {
    if id.is_null() {
        return Err(Error::Unsupported);
    }
    Ok(id.to_string())
}

fn ordinary(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return false;
        }
    }
    !metadata.file_type().is_symlink()
}

fn admit_file(path: &Path, maximum: u64, budget: &mut Budget) -> Result<(), Error> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && ordinary(&metadata) => {
            if metadata.len() > maximum {
                return Err(Error::Limited);
            }
            budget.charge(metadata.len() as usize)
        }
        Ok(_) => Err(Error::Unsupported),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn admit_loose(
    path: &Path,
    depth: usize,
    visits: &mut usize,
    budget: &mut Budget,
    ctx: &Context<'_>,
) -> Result<(), Error> {
    if depth > MAX_REF_DEPTH {
        return Err(Error::Limited);
    }
    for entry in fs::read_dir(path)? {
        ctx.check()?;
        *visits += 1;
        if *visits > MAX_REF_VISITS {
            return Err(Error::Limited);
        }
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if !ordinary(&metadata) {
            return Err(Error::Unsupported);
        }
        if metadata.is_dir() {
            admit_loose(&entry.path(), depth + 1, visits, budget, ctx)?;
        } else if metadata.is_file() {
            if metadata.len() > LOOSE_BYTES {
                return Err(Error::Limited);
            }
            budget.charge(metadata.len() as usize)?;
        } else {
            return Err(Error::Unsupported);
        }
    }
    Ok(())
}

fn admit_references(
    repo: &gix::Repository,
    budget: &mut Budget,
    ctx: &Context<'_>,
) -> Result<(), Error> {
    // gix reads a loose ref into a Vec and sorts directory entries. Admit the
    // observed file sizes and directory work before using its native reader.
    // This preflight is cooperative and cannot pin concurrent filesystem writes.
    admit_file(&repo.git_dir().join("HEAD"), LOOSE_BYTES, budget)?;
    let mut visits = 0;
    for base in std::iter::once(repo.common_dir())
        .chain((repo.common_dir() != repo.git_dir()).then_some(repo.git_dir()))
    {
        ctx.check()?;
        admit_file(&base.join("packed-refs"), PACKED_BYTES, budget)?;
        let refs = base.join("refs");
        match fs::symlink_metadata(&refs) {
            Ok(metadata) if ordinary(&metadata) && metadata.is_dir() => {
                admit_loose(&refs, 0, &mut visits, budget, ctx)?;
            }
            Ok(_) => return Err(Error::Unsupported),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn resolve_head(
    repo: &gix::Repository,
    refs: &[RefEntry],
    limits: Limits,
    budget: &mut Budget,
) -> Result<Head, Error> {
    use gix::refs::TargetRef;
    let head = repo.find_reference("HEAD").map_err(engine)?;
    if head.name().as_bstr().as_bytes() != b"HEAD" {
        return Err(Error::Missing);
    }
    match head.target() {
        TargetRef::Object(id) => {
            budget.charge(64)?;
            Ok(Head::Detached {
                target: id_text(id)?,
            })
        }
        TargetRef::Symbolic(name) => {
            let name = checked_name(name.as_bstr().as_bytes(), limits, budget)?;
            let mut current = name.as_slice();
            for depth in 0..MAX_LINKS {
                let entry = refs.binary_search_by(|entry| entry.name.as_slice().cmp(current));
                let Ok(entry) = entry else {
                    return if depth == 0 && name.starts_with(b"refs/heads/") {
                        Ok(Head::Unborn { name })
                    } else {
                        Err(Error::Missing)
                    };
                };
                match &refs[entry].target {
                    RefTarget::Direct(target) => {
                        budget.charge(target.len())?;
                        return Ok(Head::Attached {
                            name,
                            target: target.clone(),
                        });
                    }
                    RefTarget::Symbolic(target) => current = target,
                }
            }
            Err(Error::Limited)
        }
    }
}

pub(super) fn references(
    repo: &gix::Repository,
    limits: Limits,
    budget: &mut Budget,
    ctx: &Context<'_>,
) -> Result<(Head, Vec<RefEntry>), Error> {
    use gix::refs::TargetRef;
    admit_references(repo, budget, ctx)?;
    let platform = repo.references().map_err(engine)?;
    let mut refs = Vec::new();
    for reference in platform.all().map_err(engine)? {
        ctx.check()?;
        if refs.len() == MAX_REFS.min(limits.max_entries) {
            return Err(Error::Limited);
        }
        let reference = reference.map_err(engine)?;
        let name = checked_name(reference.name().as_bstr().as_bytes(), limits, budget)?;
        budget.charge(2 * std::mem::size_of::<RefEntry>())?;
        let kind = ref_kind(&name);
        let target = match reference.target() {
            TargetRef::Object(id) => {
                budget.charge(64)?;
                RefTarget::Direct(id_text(id)?)
            }
            TargetRef::Symbolic(name) => {
                RefTarget::Symbolic(checked_name(name.as_bstr().as_bytes(), limits, budget)?)
            }
        };
        refs.push(RefEntry { name, kind, target });
    }
    refs.sort_unstable_by(|a, b| a.name.cmp(&b.name));
    let head = resolve_head(repo, &refs, limits, budget)?;
    Ok((head, refs))
}

/// Read loose and packed refs without peeling every target or reading the index,
/// worktree or reflogs. The names and immediate targets are retained exactly.
/// Reference reads are a bounded observation of an externally mutable repository.
pub fn read_references(
    root: &Path,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<RefSnapshot, Error> {
    limits.admit()?;
    controlled(limits.timeout, cancelled, |ctx| {
        let (repo, repository) = open(root, ctx)?;
        let (head, refs) = references(&repo, limits, &mut Budget::new(limits), ctx)?;
        Ok(RefSnapshot {
            repository,
            head,
            refs,
        })
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Boundary {
    Interior,
    Root,
    Shallow,
    Window,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphNode {
    pub id: String,
    /// Parent order from the commit object. Shallow nodes retain these IDs as
    /// metadata, but those edges are not traversed across the declared boundary.
    /// The viewer window omits parents beyond its admitted range.
    pub parents: Vec<String>,
    pub summary: Vec<u8>,
    pub author: Vec<u8>,
    pub commit_time: i64,
    pub boundary: Boundary,
}

impl GraphNode {
    fn memory_bytes(&self) -> usize {
        self.id.capacity()
            + self.parents.capacity() * std::mem::size_of::<String>()
            + self.parents.iter().map(String::capacity).sum::<usize>()
            + self.summary.capacity()
            + self.author.capacity()
    }
}

struct Snapshot {
    repository: Identity,
    requested: Option<gix::ObjectId>,
    start: Option<String>,
    head: Option<Head>,
    refs: Arc<[RefEntry]>,
    stashes: Arc<StashList>,
    nodes: Vec<GraphNode>,
    shallow: bool,
    metadata_limited: bool,
    window: bool,
    window_limited: bool,
}

impl Snapshot {
    fn memory_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.repository.root.capacity()
            + self.repository.git_dir.capacity()
            + self.start.as_ref().map_or(0, String::capacity)
            + self.head.as_ref().map_or(0, Head::memory_bytes)
            + ref_bytes(&self.refs)
            + self.stashes.memory_bytes()
            + self.nodes.capacity() * std::mem::size_of::<GraphNode>()
            + self
                .nodes
                .iter()
                .map(GraphNode::memory_bytes)
                .sum::<usize>()
    }
}

/// Opaque continuation over the exact immutable graph from the initial call.
/// Clones share its data and never rediscover HEAD or ref names.
#[derive(Clone)]
pub struct GraphCursor {
    snapshot: Arc<Snapshot>,
    offset: usize,
}

impl GraphCursor {
    /// Logical size of the snapshot retained by this continuation. Shared clones
    /// refer to the same allocation and should not be added as independent data.
    pub fn memory_bytes(&self) -> usize {
        self.snapshot.memory_bytes()
    }
}

pub struct GraphPage {
    pub repository: Identity,
    /// Peeled commit pinned by the initial call. Absent for an unborn HEAD.
    pub start: Option<String>,
    /// Present only when the initial call selected HEAD implicitly.
    pub head: Option<Head>,
    pub refs: Arc<[RefEntry]>,
    pub stashes: Arc<StashList>,
    pub nodes: Vec<GraphNode>,
    pub next: Option<GraphCursor>,
    pub shallow: bool,
    pub metadata_limited: bool,
    pub window_limited: bool,
}

impl GraphPage {
    pub fn memory_bytes(&self) -> usize {
        self.repository.root.capacity()
            + self.repository.git_dir.capacity()
            + self.start.as_ref().map_or(0, String::capacity)
            + self.head.as_ref().map_or(0, Head::memory_bytes)
            + if self.next.is_none() {
                ref_bytes(&self.refs) + self.stashes.memory_bytes()
            } else {
                0
            }
            + self.nodes.capacity() * std::mem::size_of::<GraphNode>()
            + self
                .nodes
                .iter()
                .map(GraphNode::memory_bytes)
                .sum::<usize>()
            + self.next.as_ref().map_or(0, GraphCursor::memory_bytes)
    }
}

fn ref_bytes(refs: &[RefEntry]) -> usize {
    std::mem::size_of_val(refs)
        + refs
            .iter()
            .map(|entry| {
                entry.name.capacity()
                    + match &entry.target {
                        RefTarget::Direct(oid) => oid.capacity(),
                        RefTarget::Symbolic(name) => name.capacity(),
                    }
            })
            .sum::<usize>()
}

fn parse_oid(value: &str) -> Result<gix::ObjectId, Error> {
    if ![40, 64].contains(&value.len()) || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::Unsupported);
    }
    let id = gix::ObjectId::from_hex(value.as_bytes()).map_err(engine)?;
    if id.is_null() {
        return Err(Error::Unsupported);
    }
    Ok(id)
}

fn stash_list(
    repo: &gix::Repository,
    refs: &[RefEntry],
    limits: Limits,
    budget: &mut Budget,
    ctx: &Context<'_>,
) -> Result<StashList, Error> {
    let Some(reference) = refs.iter().find(|entry| entry.kind == RefKind::Stash) else {
        return Ok(StashList::default());
    };
    let RefTarget::Direct(id) = &reference.target else {
        return Ok(StashList {
            entries: Vec::new(),
            incomplete: true,
        });
    };
    let head = parse_oid(id)?;
    let fallback = || StashList {
        entries: vec![StashEntry {
            id: head,
            message: Vec::new(),
        }],
        incomplete: true,
    };
    let log = repo.common_dir().canonicalize().and_then(|root| {
        jc_fs::read_in_root(
            &root,
            &root.join("logs/refs/stash"),
            STASH_LOG_BYTES.min(limits.max_bytes),
        )
    });
    ctx.check()?;
    let Ok((_, bytes)) = log else {
        return Ok(fallback());
    };
    budget.charge(bytes.len())?;
    if !bytes.ends_with(b"\n") {
        return Ok(fallback());
    }
    let mut entries = Vec::new();
    let mut incomplete = false;
    for line in bytes[..bytes.len() - 1].rsplit(|&byte| byte == b'\n') {
        ctx.check()?;
        if entries.len() == PAGE.min(limits.max_entries) {
            incomplete = true;
            break;
        }
        let Ok(line) = gix::refs::file::log::LineRef::from_bytes(line) else {
            return Ok(fallback());
        };
        let id = line.new_oid();
        if id.is_null() || id.kind() != head.kind() || (entries.is_empty() && id != head) {
            return Ok(fallback());
        }
        let message = &line.message[..line.message.len().min(STASH_MESSAGE_BYTES)];
        budget.charge(2 * std::mem::size_of::<StashEntry>() + message.len())?;
        entries.push(StashEntry {
            id,
            message: message.to_vec(),
        });
    }
    Ok(StashList {
        entries,
        incomplete,
    })
}

fn graph_object<'a>(
    repo: &'a gix::Repository,
    id: gix::ObjectId,
    budget: &mut Budget,
    ctx: &Context<'_>,
) -> Result<gix::Object<'a>, Error> {
    ctx.check()?;
    if id.kind() != repo.object_hash() || id.is_null() {
        return Err(Error::Unsupported);
    }
    let header = repo
        .try_find_header(id)
        .map_err(engine)?
        .ok_or(Error::Missing)?;
    let size = usize::try_from(header.size()).map_err(|_| Error::Limited)?;
    if size > MAX_OBJECT_BYTES {
        return Err(Error::Limited);
    }
    budget.charge(size)?;
    verified(repo, id, MAX_OBJECT_BYTES, ctx)
}

pub(super) fn contains_ancestor(
    repo: &gix::Repository,
    start: &str,
    target: gix::ObjectId,
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<bool, Error> {
    let mut budget = Budget::new(limits);
    let boundaries = shallow(repo, limits, &mut budget, ctx)?;
    let start = parse_oid(start)?;
    let mut visited = BTreeSet::from([start]);
    let mut pending = VecDeque::from([start]);
    while let Some(id) = pending.pop_front() {
        let commit = graph_object(repo, id, &mut budget, ctx)?
            .try_into_commit()
            .map_err(engine)?;
        if id == target {
            return Ok(true);
        }
        if boundaries.contains(&id) {
            continue;
        }
        let decoded = commit.decode().map_err(engine)?;
        if decoded.parents.len() > MAX_PARENTS {
            return Err(Error::Limited);
        }
        for parent in decoded.parents() {
            if visited.insert(parent) {
                if visited.len() > MAX_NODES.min(limits.max_entries) {
                    return Err(Error::Limited);
                }
                pending.push_back(parent);
            }
        }
    }
    Ok(false)
}

fn peel_commit<'a>(
    repo: &'a gix::Repository,
    mut id: gix::ObjectId,
    budget: &mut Budget,
    ctx: &Context<'_>,
) -> Result<gix::Commit<'a>, Error> {
    for _ in 0..=MAX_LINKS {
        let value = graph_object(repo, id, budget, ctx)?;
        match value.kind {
            gix::objs::Kind::Commit => return value.try_into_commit().map_err(engine),
            gix::objs::Kind::Tag => {
                id = value
                    .try_into_tag()
                    .map_err(engine)?
                    .target_id()
                    .map_err(engine)?
                    .detach();
            }
            _ => return Err(Error::Unsupported),
        }
    }
    Err(Error::Limited)
}

struct RawNode {
    parents: Vec<gix::ObjectId>,
    output: GraphNode,
}

fn read_node(
    commit: gix::Commit<'_>,
    boundaries: &[gix::ObjectId],
    budget: &mut Budget,
    metadata_limited: &mut bool,
) -> Result<RawNode, Error> {
    let decoded = commit.decode().map_err(engine)?;
    if decoded.parents.len() > MAX_PARENTS {
        return Err(Error::Limited);
    }
    let parents = decoded.parents().collect::<Vec<_>>();
    if parents
        .iter()
        .any(|parent| parent.is_null() || parent.kind() != commit.id.kind())
    {
        return Err(Error::Unsupported);
    }
    let boundary = if boundaries.contains(&commit.id) {
        Boundary::Shallow
    } else if parents.is_empty() {
        Boundary::Root
    } else {
        Boundary::Interior
    };
    let author = decoded.author().map_err(engine)?;
    let time = decoded
        .committer()
        .map_err(engine)?
        .time()
        .map_err(engine)?
        .seconds;
    let summary = decoded
        .message
        .as_bytes()
        .split(|byte| matches!(byte, b'\r' | b'\n'))
        .next()
        .unwrap_or_default();
    let author = author.name.as_bytes();
    *metadata_limited |= summary.len() > 256 || author.len() > 128;
    let summary = &summary[..summary.len().min(256)];
    let author = &author[..author.len().min(128)];
    budget.charge(384 + parents.len() * 160 + summary.len() + author.len())?;
    Ok(RawNode {
        output: GraphNode {
            id: commit.id.to_string(),
            parents: parents.iter().map(ToString::to_string).collect(),
            summary: summary.to_vec(),
            author: author.to_vec(),
            commit_time: time,
            boundary,
        },
        parents,
    })
}

fn topological(
    mut nodes: BTreeMap<gix::ObjectId, RawNode>,
    ctx: &Context<'_>,
) -> Result<Vec<GraphNode>, Error> {
    let count = nodes.len();
    let mut children = nodes
        .keys()
        .map(|id| (*id, 0usize))
        .collect::<BTreeMap<_, _>>();
    for node in nodes
        .values()
        .filter(|node| matches!(node.output.boundary, Boundary::Interior | Boundary::Window))
    {
        ctx.check()?;
        for parent in &node.parents {
            *children.get_mut(parent).ok_or(Error::Missing)? += 1;
        }
    }
    let mut available = BinaryHeap::new();
    for (id, node) in &nodes {
        if children[id] == 0 {
            available.push((node.output.commit_time, Reverse(*id)));
        }
    }
    let mut result = Vec::with_capacity(count);
    while let Some((_, Reverse(id))) = available.pop() {
        ctx.check()?;
        let node = nodes.remove(&id).ok_or(Error::Missing)?;
        if matches!(node.output.boundary, Boundary::Interior | Boundary::Window) {
            for parent in node.parents {
                let count = children.get_mut(&parent).ok_or(Error::Missing)?;
                *count -= 1;
                if *count == 0 {
                    let parent_node = nodes.get(&parent).ok_or(Error::Missing)?;
                    available.push((parent_node.output.commit_time, Reverse(parent)));
                }
            }
        }
        result.push(node.output);
    }
    if result.len() != count {
        return Err(Error::Engine(
            "El historial no forma un grafo acíclico".into(),
        ));
    }
    Ok(result)
}

fn snapshot(
    repo: &gix::Repository,
    repository: Identity,
    requested: Option<gix::ObjectId>,
    window: bool,
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<Snapshot, Error> {
    let mut budget = Budget::new(limits);
    let (head, refs) = if requested.is_none() || window {
        let (head, refs) = references(repo, limits, &mut budget, ctx)?;
        (
            requested.is_none().then_some(head),
            if window {
                Arc::from(refs)
            } else {
                Arc::from([])
            },
        )
    } else {
        (None, Arc::from([]))
    };
    let id = requested.or(head
        .as_ref()
        .and_then(Head::target)
        .map(parse_oid)
        .transpose()?);
    let stashes = Arc::new(stash_list(repo, &refs, limits, &mut budget, ctx)?);
    let mut result = Snapshot {
        repository,
        requested,
        start: None,
        head,
        refs,
        stashes,
        nodes: Vec::new(),
        shallow: false,
        metadata_limited: false,
        window,
        window_limited: false,
    };
    let Some(id) = id else { return Ok(result) };
    let boundaries = shallow(repo, limits, &mut budget, ctx)?;
    let root = peel_commit(repo, id, &mut budget, ctx)?;
    let root_id = root.id;
    result.start = Some(root_id.to_string());
    let maximum = MAX_NODES.min(limits.max_entries);
    let mut pending = VecDeque::from([root_id]);
    let mut admitted = BTreeSet::from([root_id]);
    let mut nodes = BTreeMap::new();
    let mut root = Some(root);
    while let Some(id) = pending.pop_front() {
        ctx.check()?;
        let commit = if let Some(root) = root.take() {
            root
        } else {
            graph_object(repo, id, &mut budget, ctx)?
                .try_into_commit()
                .map_err(engine)?
        };
        let mut node = read_node(
            commit,
            &boundaries,
            &mut budget,
            &mut result.metadata_limited,
        )?;
        match node.output.boundary {
            Boundary::Shallow => result.shallow = true,
            Boundary::Root => {}
            Boundary::Interior | Boundary::Window => {
                for parent in &node.parents {
                    if !admitted.contains(parent) {
                        if admitted.len() == maximum {
                            if window {
                                node.output.boundary = Boundary::Window;
                                result.window_limited = true;
                                continue;
                            }
                            return Err(Error::Limited);
                        }
                        admitted.insert(*parent);
                        pending.push_back(*parent);
                    }
                }
                if node.output.boundary == Boundary::Window {
                    node.parents.retain(|parent| admitted.contains(parent));
                    node.output.parents = node.parents.iter().map(ToString::to_string).collect();
                }
            }
        }
        nodes.insert(id, node);
    }
    result.nodes = topological(nodes, ctx)?;
    Ok(result)
}

fn page(snapshot: Arc<Snapshot>, offset: usize, limits: Limits) -> Result<GraphPage, Error> {
    if snapshot.nodes.len() > limits.max_entries || snapshot.memory_bytes() > limits.max_bytes {
        return Err(Error::Limited);
    }
    let end = (offset + PAGE).min(snapshot.nodes.len());
    let result = GraphPage {
        repository: Identity {
            root: snapshot.repository.root.clone(),
            git_dir: snapshot.repository.git_dir.clone(),
        },
        start: snapshot.start.clone(),
        head: snapshot.head.clone(),
        refs: snapshot.refs.clone(),
        stashes: snapshot.stashes.clone(),
        nodes: snapshot.nodes[offset..end].to_vec(),
        shallow: snapshot.shallow,
        metadata_limited: snapshot.metadata_limited,
        window_limited: snapshot.window_limited,
        next: (end < snapshot.nodes.len()).then_some(GraphCursor {
            snapshot,
            offset: end,
        }),
    };
    if result.memory_bytes() > limits.max_bytes {
        return Err(Error::Limited);
    }
    Ok(result)
}

/// Return up to 24 commits in child-before-parent order. Among available nodes,
/// newer committer time wins, then ascending OID. The entire reachable subgraph
/// must fit the admitted window (up to 512 commits) before paging starts.
/// Over-budget ancestry returns `Limited`, never a falsely ordered prefix.
/// Continuations keep the same pinned start and do not read objects again.
pub fn read_graph(
    root: &Path,
    pinned_start: Option<&str>,
    continuation: Option<&GraphCursor>,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<GraphPage, Error> {
    read_graph_inner(root, pinned_start, continuation, false, limits, cancelled)
}

/// UI-sized ancestry window of HEAD. If the node cap is reached, page only
/// the admitted graph and mark nodes whose older edges leave the window.
pub fn read_graph_window(
    root: &Path,
    continuation: Option<&GraphCursor>,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<GraphPage, Error> {
    read_graph_inner(root, None, continuation, true, limits, cancelled)
}

/// Open the viewer on an observed branch or tag target. Annotated tags are peeled
/// by the same bounded commit reader as other explicit starting objects.
pub fn read_graph_window_from(
    root: &Path,
    pinned_start: Option<&str>,
    continuation: Option<&GraphCursor>,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<GraphPage, Error> {
    read_graph_inner(root, pinned_start, continuation, true, limits, cancelled)
}

fn read_graph_inner(
    root: &Path,
    pinned_start: Option<&str>,
    continuation: Option<&GraphCursor>,
    window: bool,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<GraphPage, Error> {
    limits.admit()?;
    if limits.max_entries == 0 {
        return Err(Error::Limited);
    }
    let requested = pinned_start.map(parse_oid).transpose()?;
    controlled(limits.timeout, cancelled, |ctx| {
        let (repo, repository) = open(root, ctx)?;
        if let Some(cursor) = continuation {
            let snapshot = &cursor.snapshot;
            if snapshot.repository.root != repository.root
                || snapshot.repository.git_dir != repository.git_dir
                || snapshot.window != window
                || requested.is_some_and(|id| {
                    Some(id) != snapshot.requested
                        && snapshot
                            .start
                            .as_deref()
                            .and_then(|start| parse_oid(start).ok())
                            != Some(id)
                })
            {
                return Err(Error::Stale);
            }
            return page(snapshot.clone(), cursor.offset, limits);
        }
        page(
            Arc::new(snapshot(&repo, repository, requested, window, limits, ctx)?),
            0,
            limits,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        path::PathBuf,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    struct Fixture {
        root: PathBuf,
        parent: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .nth(2)
                .unwrap()
                .canonicalize()
                .unwrap();
            let parent = workspace.join("work/test-temp");
            fs::create_dir_all(&parent).unwrap();
            let parent = parent.canonicalize().unwrap();
            assert!(parent.starts_with(&workspace));
            let root = parent.join(format!(
                "git-graph-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir(&root).unwrap();
            Self { root, parent }
        }

        fn git(&self) -> PathBuf {
            self.root.join(".git")
        }

        fn set_head(&self, id: gix::ObjectId) {
            fs::write(self.git().join("HEAD"), format!("{id}\n")).unwrap();
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            assert_eq!(self.root.parent(), Some(self.parent.as_path()));
            assert_eq!(self.root.canonicalize().unwrap(), self.root);
            fn clean(path: &Path) {
                let metadata = fs::symlink_metadata(path).unwrap();
                assert!(ordinary(&metadata));
                if metadata.is_dir() {
                    for entry in fs::read_dir(path).unwrap() {
                        clean(&entry.unwrap().path());
                    }
                    fs::remove_dir(path).unwrap();
                } else {
                    #[cfg(windows)]
                    #[allow(clippy::permissions_set_readonly_false)]
                    if metadata.permissions().readonly() {
                        let mut permissions = metadata.permissions();
                        permissions.set_readonly(false);
                        fs::set_permissions(path, permissions).unwrap();
                    }
                    fs::remove_file(path).unwrap();
                }
            }
            clean(&self.root);
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

    fn commit(
        repo: &gix::Repository,
        parents: &[gix::ObjectId],
        time: i64,
        message: &[u8],
    ) -> gix::ObjectId {
        let tree = repo
            .write_object(gix::objs::Tree {
                entries: Vec::new(),
            })
            .unwrap()
            .detach();
        let time = format!("{time} +0000");
        let signature = gix::actor::SignatureRef {
            name: b"Autor del fixture".as_bstr(),
            email: b"fixture@invalid".as_bstr(),
            time: &time,
        };
        repo.write_object(gix::objs::Commit {
            tree,
            parents: parents.iter().copied().collect(),
            author: signature.into(),
            committer: signature.into(),
            message: message.to_vec().into(),
            encoding: None,
            extra_headers: Default::default(),
        })
        .unwrap()
        .detach()
    }

    #[test]
    #[cfg(any(windows, target_os = "linux"))]
    fn graph_lists_stashes_newest_first_and_reports_a_stale_log() {
        let fixture = Fixture::new();
        let repo = gix::init(&fixture.root).unwrap();
        let base = commit(&repo, &[], 1, b"base");
        let index = commit(&repo, &[base], 2, b"index");
        let old = commit(&repo, &[base, index], 3, b"antigua");
        let new = commit(&repo, &[base, index], 4, b"nueva");
        fixture.set_head(base);
        let git = fixture.git();
        fs::write(git.join("refs/stash"), format!("{new}\n")).unwrap();
        fs::create_dir_all(git.join("logs/refs")).unwrap();
        let log = git.join("logs/refs/stash");
        let bytes = format!(
            "{} {old} Autor <fixture@invalid> 3 +0000\tantigua\n{old} {new} Autor <fixture@invalid> 4 +0000\tnueva\n",
            gix::ObjectId::null(repo.object_hash()),
        );
        fs::write(&log, &bytes).unwrap();
        let read = || read_graph_window(&fixture.root, None, limits(), || false).unwrap();
        let page = read();
        assert!(!page.stashes.incomplete);
        assert_eq!(
            page.stashes.entries,
            [
                StashEntry {
                    id: new,
                    message: b"nueva".to_vec()
                },
                StashEntry {
                    id: old,
                    message: b"antigua".to_vec()
                },
            ]
        );
        assert_eq!(fs::read_to_string(&log).unwrap(), bytes);
        fs::write(git.join("refs/stash"), format!("{old}\n")).unwrap();
        let page = read();
        assert!(page.stashes.incomplete);
        assert_eq!(
            page.stashes.entries,
            [StashEntry {
                id: old,
                message: Vec::new()
            }]
        );
    }

    #[test]
    fn graph_refs_merge_packed_and_loose_and_distinguish_unborn_head() {
        let fixture = Fixture::new();
        let repo = gix::init(&fixture.root).unwrap();
        let unborn = read_references(&fixture.root, limits(), || false).unwrap();
        assert!(matches!(unborn.head, Head::Unborn { .. }));
        assert!(unborn.refs.is_empty());
        let empty = read_graph(&fixture.root, None, None, limits(), || false).unwrap();
        assert!(empty.start.is_none() && empty.nodes.is_empty() && empty.next.is_none());

        let old = commit(&repo, &[], 1, b"base");
        let tip = commit(&repo, &[old], 2, b"actual");
        let git = fixture.git();
        fs::create_dir_all(git.join("refs/remotes/origin")).unwrap();
        fs::create_dir_all(git.join("refs/heads")).unwrap();
        fs::write(git.join("HEAD"), b"ref: refs/heads/main\n").unwrap();
        fs::write(git.join("refs/heads/main"), format!("{tip}\n")).unwrap();
        fs::write(
            git.join("refs/remotes/origin/HEAD"),
            b"ref: refs/remotes/origin/main\n",
        )
        .unwrap();
        let packed = format!(
            "# pack-refs with: sorted\n{old} refs/heads/main\n{tip} refs/remotes/origin/main\n{old} refs/tags/v1\n"
        );
        fs::write(git.join("packed-refs"), packed.as_bytes()).unwrap();
        let refs = read_references(&fixture.root, limits(), || false).unwrap();
        assert_eq!(
            refs.head,
            Head::Attached {
                name: b"refs/heads/main".to_vec(),
                target: tip.to_string()
            }
        );
        assert_eq!(
            refs.refs.iter().map(|entry| entry.kind).collect::<Vec<_>>(),
            [
                RefKind::LocalBranch,
                RefKind::RemoteBranch,
                RefKind::RemoteBranch,
                RefKind::Tag
            ]
        );
        assert_eq!(refs.refs[0].target, RefTarget::Direct(tip.to_string()));
        assert_eq!(
            refs.refs[1].target,
            RefTarget::Symbolic(b"refs/remotes/origin/main".to_vec())
        );
        assert_eq!(
            fs::read(git.join("packed-refs")).unwrap(),
            packed.as_bytes()
        );
        fixture.set_head(old);
        assert_eq!(
            read_references(&fixture.root, limits(), || false)
                .unwrap()
                .head,
            Head::Detached {
                target: old.to_string()
            }
        );
    }

    #[test]
    fn graph_pages_stay_topological_and_pinned_across_changed_head() {
        let fixture = Fixture::new();
        let repo = gix::init(&fixture.root).unwrap();
        // Clock skew deliberately makes the root newer than its descendants.
        let root = commit(&repo, &[], 9999, b"raiz");
        let side = commit(&repo, &[root], 20, b"rama");
        let mut main = root;
        for number in 1..=26 {
            main = commit(&repo, &[main], number, b"lineal");
        }
        let tip = commit(&repo, &[main, side], 1, b"fusion");
        fixture.set_head(tip);
        let page = read_graph(&fixture.root, None, None, limits(), || false).unwrap();
        assert_eq!(page.start, Some(tip.to_string()));
        assert_eq!(page.nodes.len(), PAGE);
        assert_eq!(page.nodes[0].parents, [main.to_string(), side.to_string()]);
        let cursor = page.next.as_ref().unwrap();
        fixture.set_head(root);
        let next = read_graph(&fixture.root, None, Some(cursor), limits(), || false).unwrap();
        assert_eq!(next.start, page.start);
        assert!(next.next.is_none());
        let all = page.nodes.iter().chain(&next.nodes).collect::<Vec<_>>();
        let order = all
            .iter()
            .enumerate()
            .map(|(index, node)| (node.id.as_str(), index))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(order.len(), 29);
        for (index, node) in all.iter().enumerate() {
            assert!(
                node.parents
                    .iter()
                    .all(|parent| index < order[parent.as_str()])
            );
        }
        assert_eq!(all.last().unwrap().boundary, Boundary::Root);
        assert!(matches!(
            read_graph(
                &fixture.root,
                Some(&root.to_string()),
                Some(cursor),
                limits(),
                || false
            ),
            Err(Error::Stale)
        ));
        fixture.set_head(tip);
        let tiny = Limits {
            max_entries: 4,
            ..limits()
        };
        let window = read_graph_window(&fixture.root, None, tiny, || false).unwrap();
        assert_eq!(window.nodes.len(), 4);
        assert!(window.window_limited && !window.shallow);
        assert!(
            window
                .nodes
                .iter()
                .any(|node| node.boundary == Boundary::Window)
        );
        for (i, node) in window.nodes.iter().enumerate() {
            assert!(node.parents.iter().all(|parent| {
                window.nodes[i + 1..]
                    .iter()
                    .any(|older| older.id == *parent)
            }));
        }
    }

    #[test]
    fn graph_distinguishes_shallow_boundaries_missing_objects_and_quota() {
        let fixture = Fixture::new();
        let repo = gix::init(&fixture.root).unwrap();
        let missing = gix::ObjectId::from_hex(&[b'a'; 40]).unwrap();
        let boundary = commit(&repo, &[missing], 1, b"limite");
        let tip = commit(&repo, &[boundary], 2, b"tip");
        let start = tip.to_string();
        assert!(matches!(
            read_graph(&fixture.root, Some(&start), None, limits(), || false),
            Err(Error::Missing)
        ));
        fs::write(fixture.git().join("shallow"), format!("{boundary}\n")).unwrap();
        let page = read_graph(&fixture.root, Some(&start), None, limits(), || false).unwrap();
        assert!(page.shallow && page.next.is_none());
        assert_eq!(page.nodes[1].boundary, Boundary::Shallow);
        assert_eq!(page.nodes[1].parents, [missing.to_string()]);
        let tiny = Limits {
            max_entries: 1,
            ..limits()
        };
        assert!(matches!(
            read_graph(&fixture.root, Some(&start), None, tiny, || false),
            Err(Error::Limited)
        ));
        assert!(matches!(
            read_graph(&fixture.root, Some(&start), None, limits(), || true),
            Err(Error::Cancelled)
        ));
    }

    #[test]
    fn graph_ref_reader_rejects_oversized_loose_and_packed_metadata() {
        let fixture = Fixture::new();
        let _repo = gix::init(&fixture.root).unwrap();
        let git = fixture.git();
        fs::create_dir_all(git.join("refs/heads")).unwrap();
        let loose = git.join("refs/heads/oversized");
        fs::File::create(&loose)
            .unwrap()
            .set_len(LOOSE_BYTES + 1)
            .unwrap();
        assert!(matches!(
            read_references(&fixture.root, limits(), || false),
            Err(Error::Limited)
        ));
        fs::remove_file(loose).unwrap();
        fs::File::create(git.join("packed-refs"))
            .unwrap()
            .set_len(PACKED_BYTES + 1)
            .unwrap();
        assert!(matches!(
            read_references(&fixture.root, limits(), || false),
            Err(Error::Limited)
        ));
    }
}
