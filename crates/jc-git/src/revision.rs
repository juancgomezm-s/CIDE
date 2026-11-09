//! Immutable, bounded first-parent revision queries. This module has no editor,
//! task scheduler or UI dependency and never opens a source file in the worktree.
use super::{Change, Context, Error, Identity, Limits, controlled, engine, object, open};
use gix::bstr::ByteSlice;
use std::{collections::BTreeMap, fmt, path::Path};

const MAX_PARENTS: usize = 8;
const MAX_DEPTH: usize = 128;
const MAX_SOURCE_BYTES: usize = 4 * 1024 * 1024;

pub struct Revision {
    pub repository: Identity,
    pub commit: String,
    pub parents: Vec<String>,
    /// The selected comparison base. A root commit has no parent.
    pub parent: Option<String>,
    pub files: Vec<RevisionFile>,
    /// True only when additional parent metadata did not fit MAX_PARENTS.
    /// File lists never silently truncate: a quota failure returns Error::Limited.
    pub limited: bool,
}
#[derive(Debug)]
pub struct RevisionFile {
    pub path: Vec<u8>,
    pub change: Change,
    pub before: Option<RevisionEntry>,
    pub after: Option<RevisionEntry>,
}
#[derive(Debug)]
pub struct RevisionEntry {
    pub oid: String,
    pub mode: u32,
}
/// One directory of the immutable tree of a selected commit.
pub struct RevisionDirectory {
    pub repository: Identity,
    pub commit: String,
    pub path: Vec<u8>,
    pub entries: Vec<DirectoryEntry>,
}
impl fmt::Debug for RevisionDirectory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RevisionDirectory")
            .field("root", &self.repository.root)
            .field("commit", &self.commit)
            .field("path", &self.path)
            .field("entries", &self.entries)
            .finish()
    }
}
#[derive(Debug)]
pub struct DirectoryEntry {
    pub name: Vec<u8>,
    pub mode: u32,
}
impl DirectoryEntry {
    pub fn is_directory(&self) -> bool {
        self.mode == 0o40000
    }
    pub fn is_comparable(&self) -> bool {
        matches!(self.mode, 0o100644 | 0o100755)
    }
}
pub struct RevisionFilePair {
    pub repository: Identity,
    pub commit: String,
    pub parent: Option<String>,
    pub path: Vec<u8>,
    pub before: Option<RevisionBlob>,
    pub after: Option<RevisionBlob>,
}
pub struct RevisionBlob {
    pub oid: String,
    pub mode: u32,
    pub bytes: Vec<u8>,
}
impl Revision {
    pub fn memory_bytes(&self) -> usize {
        self.repository.root.capacity()
            + self.repository.git_dir.capacity()
            + self.commit.capacity()
            + self.parent.as_ref().map_or(0, String::capacity)
            + self.parents.capacity() * std::mem::size_of::<String>()
            + self.parents.iter().map(String::capacity).sum::<usize>()
            + self.files.capacity() * std::mem::size_of::<RevisionFile>()
            + self
                .files
                .iter()
                .map(|file| {
                    file.path.capacity()
                        + file.before.as_ref().map_or(0, |entry| entry.oid.capacity())
                        + file.after.as_ref().map_or(0, |entry| entry.oid.capacity())
                })
                .sum::<usize>()
    }
}
impl RevisionFilePair {
    pub fn memory_bytes(&self) -> usize {
        self.repository.root.capacity()
            + self.repository.git_dir.capacity()
            + self.commit.capacity()
            + self.parent.as_ref().map_or(0, String::capacity)
            + self.path.capacity()
            + self
                .before
                .as_ref()
                .map_or(0, |blob| blob.oid.capacity() + blob.bytes.capacity())
            + self
                .after
                .as_ref()
                .map_or(0, |blob| blob.oid.capacity() + blob.bytes.capacity())
    }
}
impl fmt::Debug for Revision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Revision")
            .field("root", &self.repository.root)
            .field("commit", &self.commit)
            .field("parents", &self.parents)
            .field("parent", &self.parent)
            .field("files", &self.files)
            .field("limited", &self.limited)
            .finish()
    }
}
impl fmt::Debug for RevisionFilePair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RevisionFilePair")
            .field("root", &self.repository.root)
            .field("commit", &self.commit)
            .field("parent", &self.parent)
            .field("path", &self.path)
            .field("before", &self.before)
            .field("after", &self.after)
            .finish()
    }
}
impl fmt::Debug for RevisionBlob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RevisionBlob")
            .field("oid", &self.oid)
            .field("mode", &self.mode)
            .field("bytes", &self.bytes.len())
            .finish()
    }
}

pub(crate) struct Budget {
    used: usize,
    maximum: usize,
    visited: usize,
    tree_bytes: usize,
}
impl Budget {
    pub(crate) fn new(limits: Limits) -> Self {
        Self {
            used: 0,
            maximum: limits.max_bytes,
            visited: 0,
            tree_bytes: 0,
        }
    }
    pub(crate) fn charge(&mut self, bytes: usize) -> Result<(), Error> {
        self.used = self.used.checked_add(bytes).ok_or(Error::Limited)?;
        if self.used > self.maximum {
            return Err(Error::Limited);
        }
        Ok(())
    }
    pub(super) fn visit(&mut self) -> Result<(), Error> {
        self.visited += 1;
        if self.visited > super::INDEX_ENTRIES {
            return Err(Error::Limited);
        }
        Ok(())
    }
}
pub(super) fn verified_tree<'a>(
    repo: &'a gix::Repository,
    id: gix::ObjectId,
    limits: Limits,
    budget: &mut Budget,
    ctx: &Context<'_>,
) -> Result<Option<gix::Tree<'a>>, Error> {
    ctx.check()?;
    let header = repo.try_find_header(id).map_err(engine)?;
    // Git's canonical empty tree is available without a stored object. If an
    // object is present under that identity, verify it instead of hiding damage.
    let Some(header) = header else {
        return if id.is_empty_tree() {
            Ok(None)
        } else {
            Err(Error::Missing)
        };
    };
    let bytes = usize::try_from(header.size()).map_err(|_| Error::Limited)?;
    budget.tree_bytes = budget.tree_bytes.checked_add(bytes).ok_or(Error::Limited)?;
    if budget.tree_bytes > super::TREE_PATH_BYTES {
        return Err(Error::Limited);
    }
    verified(
        repo,
        id,
        limits.max_bytes.min(super::INDEX_BYTES as usize),
        ctx,
    )?
    .try_into_tree()
    .map(Some)
    .map_err(engine)
}
fn commit_id(commit: &str) -> Result<gix::ObjectId, Error> {
    if ![40, 64].contains(&commit.len()) || !commit.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::Unsupported);
    }
    let id = gix::ObjectId::from_hex(commit.as_bytes()).map_err(engine)?;
    if id.is_null() {
        return Err(Error::Unsupported);
    }
    Ok(id)
}
pub(crate) fn path_valid(path: &[u8], maximum: usize) -> Result<(), Error> {
    if path.len() > maximum {
        return Err(Error::Limited);
    }
    if path.is_empty()
        || path.contains(&0)
        || path
            .split(|b| *b == b'/')
            .any(|p| p.is_empty() || p == b"." || p == b"..")
    {
        return Err(Error::Unsupported);
    }
    Ok(())
}
pub(crate) fn verified<'a>(
    repo: &'a gix::Repository,
    id: gix::ObjectId,
    cap: usize,
    ctx: &Context<'_>,
) -> Result<gix::Object<'a>, Error> {
    ctx.check()?;
    let value = object(repo, id, cap as u64)?;
    if gix::objs::compute_hash(id.kind(), value.kind, &value.data).map_err(engine)? != id {
        return Err(Error::Engine(
            "El contenido del objeto no coincide con su identificador".into(),
        ));
    }
    ctx.check()?;
    Ok(value)
}
pub(crate) struct Commit {
    pub(crate) id: gix::ObjectId,
    pub(crate) tree: gix::ObjectId,
    pub(crate) parents: Vec<gix::ObjectId>,
    pub(super) limited: bool,
}
pub(crate) fn commit(
    repo: &gix::Repository,
    id: gix::ObjectId,
    limits: Limits,
    budget: &mut Budget,
    ctx: &Context<'_>,
) -> Result<Commit, Error> {
    if id.kind() != repo.object_hash() {
        return Err(Error::Unsupported);
    }
    let value = verified(
        repo,
        id,
        limits.max_bytes.min(super::OBJECT_BYTES as usize),
        ctx,
    )?
    .try_into_commit()
    .map_err(engine)?;
    let tree = value.tree_id().map_err(engine)?.detach();
    let mut parents = Vec::new();
    let mut limited = false;
    for parent in value.parent_ids() {
        ctx.check()?;
        if parents.len() == MAX_PARENTS {
            limited = true;
            break;
        }
        budget.charge(std::mem::size_of::<gix::ObjectId>() * 2 + 64)?;
        parents.push(parent.detach());
    }
    budget.charge(128)?;
    Ok(Commit {
        id,
        tree,
        parents,
        limited,
    })
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Entry {
    pub(crate) id: gix::ObjectId,
    pub(crate) mode: u32,
}
type Entries = BTreeMap<Vec<u8>, Entry>;

pub(super) fn tree_entries(
    repo: &gix::Repository,
    id: gix::ObjectId,
    limits: Limits,
    budget: &mut Budget,
    ctx: &Context<'_>,
) -> Result<Entries, Error> {
    collect_tree_entries(repo, id, limits, budget, ctx, false)
}

pub(super) fn merge_tree_entries(
    repo: &gix::Repository,
    id: gix::ObjectId,
    limits: Limits,
    budget: &mut Budget,
    ctx: &Context<'_>,
) -> Result<Entries, Error> {
    collect_tree_entries(repo, id, limits, budget, ctx, true)
}

fn collect_tree_entries(
    repo: &gix::Repository,
    id: gix::ObjectId,
    limits: Limits,
    budget: &mut Budget,
    ctx: &Context<'_>,
    reject_empty_subtrees: bool,
) -> Result<Entries, Error> {
    let mut entries = Entries::new();
    let mut pending = vec![(Vec::new(), id, 0usize)];
    while let Some((prefix, id, depth)) = pending.pop() {
        ctx.check()?;
        if depth >= MAX_DEPTH {
            return Err(Error::Limited);
        }
        if reject_empty_subtrees && depth != 0 && id.is_empty_tree() {
            return Err(Error::Unsupported);
        }
        let Some(tree) = verified_tree(repo, id, limits, budget, ctx)? else {
            continue;
        };
        for entry in tree.iter() {
            ctx.check()?;
            budget.visit()?;
            let entry = entry.map_err(engine)?;
            let name = entry.filename().as_bytes();
            if name.contains(&b'/') {
                return Err(Error::Unsupported);
            }
            let length = prefix.len().checked_add(name.len()).ok_or(Error::Limited)?;
            if length > limits.max_path {
                return Err(Error::Limited);
            }
            budget.charge(length.saturating_mul(2).saturating_add(192))?;
            let mut path = prefix.clone();
            path.extend_from_slice(name);
            path_valid(&path, limits.max_path)?;
            let mode = entry.mode().value() as u32;
            match mode {
                0o40000 => {
                    path.push(b'/');
                    pending.push((path, entry.object_id(), depth + 1));
                }
                0o100644 | 0o100755 | 0o120000 => {
                    if entries
                        .insert(
                            path,
                            Entry {
                                id: entry.object_id(),
                                mode,
                            },
                        )
                        .is_some()
                    {
                        return Err(Error::Engine("El árbol contiene rutas duplicadas".into()));
                    }
                }
                _ => return Err(Error::Unsupported),
            }
        }
    }
    Ok(entries)
}
fn metadata(entry: Entry) -> RevisionEntry {
    RevisionEntry {
        oid: entry.id.to_string(),
        mode: entry.mode,
    }
}

/// Lists exact path changes against the selected commit's first parent. Neither
/// HEAD nor an editor buffer is used as the comparison base after selection.
pub fn read_revision(
    root: &Path,
    selected: &str,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<Revision, Error> {
    read_revision_parent(root, selected, 0, limits, cancelled)
}

pub fn read_revision_parent(
    root: &Path,
    selected: &str,
    parent_index: usize,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<Revision, Error> {
    limits.admit()?;
    let id = commit_id(selected)?;
    controlled(limits.timeout, cancelled, |ctx| {
        let (repo, repository) = open(root, ctx)?;
        let mut budget = Budget::new(limits);
        let selected = commit(&repo, id, limits, &mut budget, ctx)?;
        let parent = choose_parent(&selected, parent_index)?;
        let before = if let Some(id) = parent {
            let parent = commit(&repo, id, limits, &mut budget, ctx)?;
            tree_entries(&repo, parent.tree, limits, &mut budget, ctx)?
        } else {
            Entries::new()
        };
        let mut after = tree_entries(&repo, selected.tree, limits, &mut budget, ctx)?;
        let mut files = Vec::new();
        for (path, before) in before {
            ctx.check()?;
            let after = after.remove(&path);
            if after == Some(before) {
                continue;
            }
            let change = match after {
                None => Change::Deleted,
                Some(after) if after.mode & 0o170000 != before.mode & 0o170000 => {
                    Change::TypeChanged
                }
                Some(_) => Change::Modified,
            };
            push_file(
                &mut files,
                path,
                change,
                Some(before),
                after,
                limits,
                &mut budget,
            )?;
        }
        for (path, after) in after {
            ctx.check()?;
            push_file(
                &mut files,
                path,
                Change::Added,
                None,
                Some(after),
                limits,
                &mut budget,
            )?;
        }
        files.sort_unstable_by(|a, b| a.path.cmp(&b.path));
        ctx.check()?;
        let result = Revision {
            repository,
            commit: selected.id.to_string(),
            parents: selected.parents.iter().map(ToString::to_string).collect(),
            parent: parent.map(|id| id.to_string()),
            files,
            limited: selected.limited,
        };
        if result.memory_bytes() > limits.max_bytes {
            return Err(Error::Limited);
        }
        Ok(result)
    })
}

fn choose_parent(commit: &Commit, index: usize) -> Result<Option<gix::ObjectId>, Error> {
    if commit.parents.is_empty() && index == 0 {
        return Ok(None);
    }
    commit.parents.get(index).copied().map(Some).ok_or_else(|| {
        Error::Engine("El padre elegido no pertenece a la revisión disponible".into())
    })
}

/// Lists immediate tree entries at a pinned commit. An empty path means the
/// repository root. A quota or missing directory is an error, never an empty
/// success. Only tree and commit objects are read.
pub fn read_revision_directory(
    root: &Path,
    selected: &str,
    path: &[u8],
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<RevisionDirectory, Error> {
    limits.admit()?;
    let id = commit_id(selected)?;
    if !path.is_empty() {
        path_valid(path, limits.max_path)?;
    }
    controlled(limits.timeout, cancelled, |ctx| {
        let (repo, repository) = open(root, ctx)?;
        let mut budget = Budget::new(limits);
        budget.charge(path.len().saturating_add(128))?;
        let selected = commit(&repo, id, limits, &mut budget, ctx)?;
        let mut id = selected.tree;
        if !path.is_empty() {
            let components = path.split(|byte| *byte == b'/');
            if components.clone().count() >= MAX_DEPTH {
                return Err(Error::Limited);
            }
            for component in components {
                let entry = find_tree_item(&repo, id, component, limits, &mut budget, ctx)?
                    .ok_or(Error::Missing)?;
                if entry.mode != 0o40000 {
                    return Err(Error::Unsupported);
                }
                id = entry.id;
            }
        }
        let mut entries = Vec::new();
        if let Some(tree) = verified_tree(&repo, id, limits, &mut budget, ctx)? {
            for entry in tree.iter() {
                ctx.check()?;
                budget.visit()?;
                if entries.len() >= limits.max_entries {
                    return Err(Error::Limited);
                }
                let entry = entry.map_err(engine)?;
                let name = entry.filename().as_bytes();
                if name.contains(&b'/') {
                    return Err(Error::Unsupported);
                }
                path_valid(name, limits.max_path)?;
                let length = path.len() + usize::from(!path.is_empty()) + name.len();
                if length > limits.max_path {
                    return Err(Error::Limited);
                }
                budget.charge(
                    name.len()
                        .saturating_add(2 * std::mem::size_of::<DirectoryEntry>()),
                )?;
                entries.push(DirectoryEntry {
                    name: name.to_vec(),
                    mode: entry.mode().value() as u32,
                });
            }
        }
        entries.sort_unstable_by(|a, b| a.name.cmp(&b.name));
        if entries.windows(2).any(|pair| pair[0].name == pair[1].name) {
            return Err(Error::Engine("El árbol contiene rutas duplicadas".into()));
        }
        entries.sort_unstable_by(|a, b| {
            b.is_directory()
                .cmp(&a.is_directory())
                .then_with(|| a.name.cmp(&b.name))
        });
        ctx.check()?;
        Ok(RevisionDirectory {
            repository,
            commit: selected.id.to_string(),
            path: path.to_vec(),
            entries,
        })
    })
}
fn push_file(
    files: &mut Vec<RevisionFile>,
    path: Vec<u8>,
    change: Change,
    before: Option<Entry>,
    after: Option<Entry>,
    limits: Limits,
    budget: &mut Budget,
) -> Result<(), Error> {
    if files.len() >= limits.max_entries {
        return Err(Error::Limited);
    }
    budget.charge(std::mem::size_of::<RevisionFile>() * 2 + 128)?;
    files.push(RevisionFile {
        path,
        change,
        before: before.map(metadata),
        after: after.map(metadata),
    });
    Ok(())
}

pub(crate) fn tree_entry(
    repo: &gix::Repository,
    mut id: gix::ObjectId,
    path: &[u8],
    limits: Limits,
    budget: &mut Budget,
    ctx: &Context<'_>,
) -> Result<Option<Entry>, Error> {
    let parts = path.split(|b| *b == b'/');
    let count = parts.clone().count();
    if count > MAX_DEPTH {
        return Err(Error::Limited);
    }
    for (ordinal, part) in parts.enumerate() {
        let Some(entry) = find_tree_item(repo, id, part, limits, budget, ctx)? else {
            return Ok(None);
        };
        if ordinal + 1 == count {
            return match entry.mode {
                0o100644 | 0o100755 => Ok(Some(entry)),
                _ => Err(Error::Unsupported),
            };
        }
        if entry.mode != 0o40000 {
            return Err(Error::Unsupported);
        }
        id = entry.id;
    }
    Ok(None)
}

fn find_tree_item(
    repo: &gix::Repository,
    id: gix::ObjectId,
    name: &[u8],
    limits: Limits,
    budget: &mut Budget,
    ctx: &Context<'_>,
) -> Result<Option<Entry>, Error> {
    let Some(tree) = verified_tree(repo, id, limits, budget, ctx)? else {
        return Ok(None);
    };
    let mut matched = None;
    for entry in tree.iter() {
        ctx.check()?;
        budget.visit()?;
        let entry = entry.map_err(engine)?;
        if entry.filename().as_bytes() == name {
            if matched.is_some() {
                return Err(Error::Engine("El árbol contiene rutas duplicadas".into()));
            }
            matched = Some(Entry {
                id: entry.object_id(),
                mode: entry.mode().value() as u32,
            });
        }
    }
    Ok(matched)
}
pub(crate) fn blob(
    repo: &gix::Repository,
    entry: Option<Entry>,
    limits: Limits,
    budget: &mut Budget,
    source_bytes: &mut usize,
    ctx: &Context<'_>,
) -> Result<Option<RevisionBlob>, Error> {
    let Some(entry) = entry else {
        return Ok(None);
    };
    let header = repo.find_header(entry.id).map_err(engine)?;
    let bytes = usize::try_from(header.size()).map_err(|_| Error::Limited)?;
    let total = source_bytes.checked_add(bytes).ok_or(Error::Limited)?;
    if bytes > limits.max_blob.min(2 * 1024 * 1024) || total > MAX_SOURCE_BYTES {
        return Err(Error::Limited);
    }
    budget.charge(bytes.saturating_add(96))?;
    let mut object = verified(repo, entry.id, limits.max_blob, ctx)?;
    if object.kind != gix::object::Kind::Blob {
        return Err(Error::Unsupported);
    }
    *source_bytes = total;
    Ok(Some(RevisionBlob {
        oid: entry.id.to_string(),
        mode: entry.mode,
        bytes: std::mem::take(&mut object.data),
    }))
}

/// Returns pinned parent/commit bytes for one raw Git path. None means only a
/// missing tree entry (or root parent), never an unreadable or corrupt object.
pub fn read_file_pair(
    root: &Path,
    selected: &str,
    path: &[u8],
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<RevisionFilePair, Error> {
    read_file_pair_parent(root, selected, path, 0, limits, cancelled)
}

pub fn read_file_pair_parent(
    root: &Path,
    selected: &str,
    path: &[u8],
    parent_index: usize,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<RevisionFilePair, Error> {
    read_file_pair_at(root, selected, path, path, parent_index, limits, cancelled)
}

/// Compares distinct paths at the pinned first parent and selected commit.
pub fn read_file_pair_paths(
    root: &Path,
    selected: &str,
    before_path: &[u8],
    after_path: &[u8],
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<RevisionFilePair, Error> {
    read_file_pair_at(
        root,
        selected,
        before_path,
        after_path,
        0,
        limits,
        cancelled,
    )
}

fn read_file_pair_at(
    root: &Path,
    selected: &str,
    before_path: &[u8],
    path: &[u8],
    parent_index: usize,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<RevisionFilePair, Error> {
    limits.admit()?;
    let id = commit_id(selected)?;
    path_valid(path, limits.max_path)?;
    path_valid(before_path, limits.max_path)?;
    controlled(limits.timeout, cancelled, |ctx| {
        let (repo, repository) = open(root, ctx)?;
        let mut budget = Budget::new(limits);
        budget.charge(path.len().saturating_add(before_path.len()))?;
        let selected = commit(&repo, id, limits, &mut budget, ctx)?;
        let parent = choose_parent(&selected, parent_index)?;
        let before = if let Some(id) = parent {
            let parent = commit(&repo, id, limits, &mut budget, ctx)?;
            tree_entry(&repo, parent.tree, before_path, limits, &mut budget, ctx)?
        } else {
            None
        };
        let after = tree_entry(&repo, selected.tree, path, limits, &mut budget, ctx)?;
        let mut source_bytes = 0;
        let before = blob(&repo, before, limits, &mut budget, &mut source_bytes, ctx)?;
        let after = blob(&repo, after, limits, &mut budget, &mut source_bytes, ctx)?;
        ctx.check()?;
        let result = RevisionFilePair {
            repository,
            commit: selected.id.to_string(),
            parent: parent.map(|id| id.to_string()),
            path: path.to_vec(),
            before,
            after,
        };
        if result.memory_bytes() > limits.max_bytes {
            return Err(Error::Limited);
        }
        Ok(result)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
        time::{Duration, SystemTime, UNIX_EPOCH},
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Fixture {
        root: PathBuf,
        base: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .nth(2)
                .unwrap()
                .canonicalize()
                .unwrap();
            let base = workspace.join("work/test-temp");
            fs::create_dir_all(&base).unwrap();
            let base = base.canonicalize().unwrap();
            assert!(base.starts_with(&workspace) && !base.is_symlink());
            let root = base.join(format!(
                "embedded-revision-{}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            Self { root, base }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            if self.root.parent() == Some(self.base.as_path())
                && !self.root.is_symlink()
                && self
                    .root
                    .canonicalize()
                    .is_ok_and(|p| p.starts_with(&self.base))
            {
                let _ = fs::remove_dir_all(&self.root);
            }
        }
    }
    fn limits() -> Limits {
        Limits {
            max_blob: 2 * 1024 * 1024,
            max_entries: 64,
            max_path: 4096,
            max_bytes: 4 * 1024 * 1024,
            timeout: Duration::from_secs(5),
        }
    }
    fn tree(repo: &gix::Repository, entries: &[(&[u8], u32, gix::ObjectId)]) -> gix::ObjectId {
        let mut entries = entries
            .iter()
            .map(|(name, mode, id)| gix::objs::tree::Entry {
                filename: name.to_vec().into(),
                mode: (*mode).try_into().unwrap(),
                oid: *id,
            })
            .collect::<Vec<_>>();
        entries.sort();
        repo.write_object(gix::objs::Tree { entries })
            .unwrap()
            .detach()
    }
    fn save_commit(
        repo: &gix::Repository,
        tree: gix::ObjectId,
        parents: &[gix::ObjectId],
    ) -> gix::ObjectId {
        let sig = gix::actor::SignatureRef {
            name: b"owned revision fixture".as_bstr(),
            email: b"fixture@example.invalid".as_bstr(),
            time: "1 +0000",
        };
        repo.commit_as(
            sig,
            sig,
            "HEAD",
            "owned revision",
            tree,
            parents.iter().copied(),
        )
        .unwrap()
        .detach()
    }
    fn image(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
        fn visit(root: &Path, path: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
            for entry in fs::read_dir(path).unwrap() {
                let path = entry.unwrap().path();
                let metadata = fs::symlink_metadata(&path).unwrap();
                assert!(!metadata.file_type().is_symlink());
                if metadata.is_dir() {
                    visit(root, &path, out)
                } else {
                    out.insert(
                        path.strip_prefix(root).unwrap().to_owned(),
                        fs::read(path).unwrap(),
                    );
                }
            }
        }
        let mut out = BTreeMap::new();
        visit(root, root, &mut out);
        out
    }
    #[test]
    fn revision_directory_lists_pinned_immediate_entries_without_blobs_or_disk_reads() {
        let fixture = Fixture::new();
        let repo = gix::init(&fixture.root).unwrap();
        let id = repo.write_blob(b"ignored content").unwrap().detach();
        let nested = tree(&repo, &[(b"z\xff", 0o100644, id)]);
        let original = tree(&repo, &[(b"aaa", 0o100644, id), (b"docs", 0o40000, nested)]);
        let first = save_commit(&repo, original, &[]);
        save_commit(
            &repo,
            gix::ObjectId::empty_tree(repo.object_hash()),
            &[first],
        );
        fs::write(fixture.root.join("untracked"), b"worktree").unwrap();
        let before = image(&fixture.root);

        let commit = first.to_string();
        let query =
            |path: &[u8]| read_revision_directory(&fixture.root, &commit, path, limits(), || false);
        let root = query(b"").unwrap();
        assert_eq!(root.commit, commit);
        assert_eq!(root.entries.len(), 2);
        assert_eq!(root.entries[0].name, b"docs");
        assert!(root.entries[0].is_directory());
        assert_eq!(root.entries[1].name, b"aaa");
        assert!(root.entries[1].is_comparable());
        let docs = query(b"docs").unwrap();
        assert_eq!(docs.path, b"docs");
        assert_eq!(docs.entries[0].name, b"z\xff");
        assert!(matches!(query(b"missing"), Err(Error::Missing)));
        assert!(matches!(query(b"aaa"), Err(Error::Unsupported)));
        assert!(matches!(query(b"../docs"), Err(Error::Unsupported)));
        assert!(matches!(
            read_revision_directory(
                &fixture.root,
                &commit,
                b"",
                Limits {
                    max_entries: 1,
                    ..limits()
                },
                || false
            ),
            Err(Error::Limited)
        ));
        assert_eq!(image(&fixture.root), before);
    }
    #[test]
    fn root_revision_has_typed_absence_empty_present_raw_paths_and_exact_bytes_without_writes() {
        let fixture = Fixture::new();
        let repo = gix::init(&fixture.root).unwrap();
        let raw = b"\xef\xbb\xbfuno\r\n\xffdos\rfinal";
        let blob = repo.write_blob(raw).unwrap().detach();
        let empty = repo.write_blob([]).unwrap().detach();
        let tree = tree(
            &repo,
            &[(b"\xff.txt", 0o100755, blob), (b"empty", 0o100644, empty)],
        );
        let root = save_commit(&repo, tree, &[]);
        drop(repo);
        fs::write(fixture.root.join("unrelated.txt"), b"not a Git source").unwrap();
        let before = image(&fixture.root);
        let revision = read_revision(&fixture.root, &root.to_string(), limits(), || false).unwrap();
        assert!(revision.parent.is_none() && revision.parents.is_empty() && !revision.limited);
        assert_eq!(revision.files.len(), 2);
        assert!(
            revision
                .files
                .iter()
                .all(|file| file.change == Change::Added && file.before.is_none())
        );
        let pair = read_file_pair(
            &fixture.root,
            &root.to_string(),
            b"\xff.txt",
            limits(),
            || false,
        )
        .unwrap();
        assert!(pair.before.is_none());
        assert_eq!(pair.after.unwrap().bytes, raw);
        let pair = read_file_pair(&fixture.root, &root.to_string(), b"empty", limits(), || {
            false
        })
        .unwrap();
        assert!(pair.before.is_none());
        assert!(pair.after.unwrap().bytes.is_empty());
        let pair = read_file_pair(
            &fixture.root,
            &root.to_string(),
            b"absent",
            limits(),
            || false,
        )
        .unwrap();
        assert!(pair.before.is_none() && pair.after.is_none());
        assert_eq!(image(&fixture.root), before);
    }
    #[test]
    fn historical_pairs_stay_pinned_after_head_moves_and_report_added_deleted_mode_and_modified() {
        let fixture = Fixture::new();
        let repo = gix::init(&fixture.root).unwrap();
        let old = repo.write_blob(b"old\r\n").unwrap().detach();
        let new = repo.write_blob(b"new\n").unwrap().detach();
        let first_tree = tree(
            &repo,
            &[
                (b"gone", 0o100644, old),
                (b"mode", 0o100644, old),
                (b"change", 0o100644, old),
            ],
        );
        let first = save_commit(&repo, first_tree, &[]);
        let next_tree = tree(
            &repo,
            &[
                (b"added", 0o100644, new),
                (b"mode", 0o100755, old),
                (b"change", 0o100644, new),
            ],
        );
        let second = save_commit(&repo, next_tree, &[first]);
        let third = save_commit(&repo, first_tree, &[second]);
        drop(repo);
        let before = image(&fixture.root);
        let revision =
            read_revision(&fixture.root, &second.to_string(), limits(), || false).unwrap();
        assert_eq!(revision.commit, second.to_string());
        assert_ne!(revision.commit, third.to_string());
        assert_eq!(revision.parent, Some(first.to_string()));
        assert_eq!(
            revision
                .files
                .iter()
                .map(|f| (f.path.as_slice(), f.change))
                .collect::<Vec<_>>(),
            vec![
                (b"added".as_slice(), Change::Added),
                (b"change".as_slice(), Change::Modified),
                (b"gone".as_slice(), Change::Deleted),
                (b"mode".as_slice(), Change::Modified)
            ]
        );
        let pair = read_file_pair(
            &fixture.root,
            &second.to_string(),
            b"change",
            limits(),
            || false,
        )
        .unwrap();
        assert_eq!(pair.before.unwrap().bytes, b"old\r\n");
        assert_eq!(pair.after.unwrap().bytes, b"new\n");
        let pair = read_file_pair(
            &fixture.root,
            &second.to_string(),
            b"gone",
            limits(),
            || false,
        )
        .unwrap();
        assert_eq!(pair.before.unwrap().bytes, b"old\r\n");
        assert!(pair.after.is_none());
        assert_eq!(image(&fixture.root), before);
    }
    #[test]
    fn merge_selects_each_parent_for_both_file_list_and_blob_pair() {
        let fixture = Fixture::new();
        let repo = gix::init(&fixture.root).unwrap();
        let a = repo.write_blob(b"A").unwrap().detach();
        let b = repo.write_blob(b"B").unwrap().detach();
        let ta = tree(&repo, &[(b"source", 0o100644, a)]);
        let tb = tree(&repo, &[(b"source", 0o100644, b)]);
        let first = save_commit(&repo, ta, &[]);
        let second = save_commit(&repo, tb, &[first]);
        let merge = save_commit(&repo, tb, &[second, first]);
        drop(repo);
        let revision =
            read_revision(&fixture.root, &merge.to_string(), limits(), || false).unwrap();
        assert_eq!(
            revision.parents,
            vec![second.to_string(), first.to_string()]
        );
        assert_eq!(revision.parent, Some(second.to_string()));
        assert!(revision.files.is_empty());
        let pair = read_file_pair(
            &fixture.root,
            &merge.to_string(),
            b"source",
            limits(),
            || false,
        )
        .unwrap();
        assert_eq!(pair.before.unwrap().bytes, b"B");
        assert_eq!(pair.after.unwrap().bytes, b"B");
        let revision =
            read_revision_parent(&fixture.root, &merge.to_string(), 1, limits(), || false).unwrap();
        assert_eq!(revision.parent, Some(first.to_string()));
        assert_eq!(revision.files[0].path, b"source");
        let pair = read_file_pair_parent(
            &fixture.root,
            &merge.to_string(),
            b"source",
            1,
            limits(),
            || false,
        )
        .unwrap();
        assert_eq!(pair.parent, Some(first.to_string()));
        assert_eq!(pair.before.unwrap().bytes, b"A");
        assert_eq!(pair.after.unwrap().bytes, b"B");
        assert!(
            read_revision_parent(&fixture.root, &merge.to_string(), 2, limits(), || false).is_err()
        );
    }
    #[test]
    fn invalid_ids_paths_cancellation_and_quotas_fail_without_a_partial_revision() {
        let fixture = Fixture::new();
        let repo = gix::init(&fixture.root).unwrap();
        let blob = repo.write_blob([b'x'; 256]).unwrap().detach();
        let t = tree(&repo, &[(b"source", 0o100644, blob)]);
        let id = save_commit(&repo, t, &[]);
        drop(repo);
        for bad in [
            "HEAD",
            "HEAD~1",
            "0000000000000000000000000000000000000000",
            "../branch",
        ] {
            assert!(matches!(
                read_revision(&fixture.root, bad, limits(), || false),
                Err(Error::Unsupported)
            ));
        }
        for path in [
            b"../source".as_slice(),
            b"/source",
            b"source//file",
            b"source\0",
        ] {
            assert!(matches!(
                read_file_pair(&fixture.root, &id.to_string(), path, limits(), || false),
                Err(Error::Unsupported)
            ));
        }
        assert!(matches!(
            read_revision(&fixture.root, &id.to_string(), limits(), || true),
            Err(Error::Cancelled)
        ));
        let mut cap = limits();
        cap.max_entries = 0;
        assert!(matches!(
            read_revision(&fixture.root, &id.to_string(), cap, || false),
            Err(Error::Limited)
        ));
        let mut cap = limits();
        cap.max_blob = 16;
        assert!(matches!(
            read_file_pair(&fixture.root, &id.to_string(), b"source", cap, || false),
            Err(Error::Limited)
        ));
        let mut cap = limits();
        cap.max_bytes = 1;
        assert!(matches!(
            read_revision(&fixture.root, &id.to_string(), cap, || false),
            Err(Error::Limited)
        ));
    }
    #[test]
    fn advertised_missing_or_wrong_hash_blob_is_never_reported_as_absent() {
        let fixture = Fixture::new();
        let repo = gix::init(&fixture.root).unwrap();
        let blob = repo.write_blob(b"advertised body").unwrap().detach();
        let other = repo.write_blob(b"different body").unwrap().detach();
        let t = tree(&repo, &[(b"source", 0o100644, blob)]);
        let id = save_commit(&repo, t, &[]);
        drop(repo);
        let hex = blob.to_string();
        let target = fixture
            .root
            .join(".git/objects")
            .join(&hex[..2])
            .join(&hex[2..]);
        let original = fs::read(&target).unwrap();
        #[cfg(windows)]
        #[allow(clippy::permissions_set_readonly_false)]
        {
            let mut permissions = fs::metadata(&target).unwrap().permissions();
            permissions.set_readonly(false);
            fs::set_permissions(&target, permissions).unwrap();
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
        }
        fs::remove_file(&target).unwrap();
        assert!(
            read_file_pair(&fixture.root, &id.to_string(), b"source", limits(), || {
                false
            })
            .is_err()
        );
        let hex = other.to_string();
        let other = fixture
            .root
            .join(".git/objects")
            .join(&hex[..2])
            .join(&hex[2..]);
        fs::write(&target, fs::read(other).unwrap()).unwrap();
        assert!(
            read_file_pair(&fixture.root, &id.to_string(), b"source", limits(), || {
                false
            })
            .is_err()
        );
        fs::write(target, original).unwrap();
        assert_eq!(
            read_file_pair(&fixture.root, &id.to_string(), b"source", limits(), || {
                false
            })
            .unwrap()
            .after
            .unwrap()
            .bytes,
            b"advertised body"
        );
    }
    #[test]
    fn nested_paths_are_resolved_without_worktree_reads_and_non_regular_sources_are_explicit() {
        let fixture = Fixture::new();
        let repo = gix::init(&fixture.root).unwrap();
        let blob = repo.write_blob(b"target").unwrap().detach();
        let child = tree(&repo, &[(b"file", 0o100644, blob)]);
        let base = save_commit(&repo, repo.empty_tree().id().detach(), &[]);
        let t = tree(
            &repo,
            &[
                (b"folder", 0o40000, child),
                (b"link", 0o120000, blob),
                (b"submodule", 0o160000, base),
            ],
        );
        let id = save_commit(&repo, t, &[base]);
        drop(repo);
        assert!(!fixture.root.join("folder").exists());
        let pair = read_file_pair(
            &fixture.root,
            &id.to_string(),
            b"folder/file",
            limits(),
            || false,
        )
        .unwrap();
        assert_eq!(pair.after.unwrap().bytes, b"target");
        assert!(matches!(
            read_file_pair(&fixture.root, &id.to_string(), b"link", limits(), || false),
            Err(Error::Unsupported)
        ));
        for path in [b"link/child".as_slice(), b"submodule/child"] {
            assert!(matches!(
                read_file_pair(&fixture.root, &id.to_string(), path, limits(), || false),
                Err(Error::Unsupported)
            ));
        }
    }
}
