//! CIDE's embedded Git provider. No Git executable, UI or editable buffers.
use gix::bstr::ByteSlice;
use std::{
    collections::BTreeMap,
    io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicU8, Ordering},
    time::{Duration, Instant},
};

const INDEX_BYTES: u64 = 8 * 1024 * 1024;
const OBJECT_BYTES: u64 = 16 * 1024 * 1024;
const INDEX_ENTRIES: usize = 65536;
const TREE_PATH_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    InvalidRoot,
    Missing,
    Unmerged,
    Unsupported,
    Filter,
    Limited,
    Cancelled,
    TimedOut,
    Engine(String),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "Git integrado: {e}"),
            Self::Engine(e) => write!(f, "Git integrado: {e}"),
            Self::InvalidRoot => f.write_str("Git integrado: abre la raíz del repositorio"),
            Self::Missing => f.write_str("Git integrado: objeto o archivo ausente"),
            Self::Unmerged => f.write_str("Git integrado: archivo con conflicto sin resolver"),
            Self::Unsupported => f.write_str("Git integrado: formato o capacidad no admitidos"),
            Self::Filter => {
                f.write_str("Git integrado: los filtros externos requieren una capacidad adicional")
            }
            Self::Limited => f.write_str("Git integrado: se excedió la cuota de lectura"),
            Self::Cancelled => f.write_str("Git integrado: lectura cancelada"),
            Self::TimedOut => f.write_str("Git integrado: se agotó el tiempo de lectura"),
        }
    }
}
impl std::error::Error for Error {}
impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}
fn engine(e: impl std::fmt::Display) -> Error {
    Error::Engine(e.to_string().chars().take(400).collect())
}

pub struct Identity {
    pub root: PathBuf,
    pub git_dir: PathBuf,
}
pub struct Blob {
    pub repository: Identity,
    pub commit: Option<String>,
    pub oid: String,
    pub mode: u32,
    pub bytes: Vec<u8>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Change {
    #[default]
    Unchanged,
    Modified,
    Added,
    Deleted,
    TypeChanged,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Tracked,
    Untracked,
    Conflict(u8),
}
pub struct Entry {
    pub path: Vec<u8>,
    pub index: Change,
    pub worktree: Change,
    pub kind: Kind,
    pub submodule: bool,
}
pub struct Status {
    pub repository: Identity,
    pub entries: Vec<Entry>,
}
#[derive(Clone, Copy)]
pub struct Limits {
    pub max_blob: usize,
    pub max_entries: usize,
    pub max_path: usize,
    pub max_bytes: usize,
    pub timeout: Duration,
}
impl Limits {
    fn admit(self) -> Result<(), Error> {
        if self.max_blob > OBJECT_BYTES as usize
            || self.max_entries > 16384
            || !(1..=4096).contains(&self.max_path)
            || !(1..=4 * 1024 * 1024).contains(&self.max_bytes)
            || self.timeout.is_zero()
            || self.timeout > Duration::from_secs(30)
        {
            return Err(Error::Limited);
        }
        Ok(())
    }
}

struct Context<'a> {
    stop: &'a AtomicBool,
    reason: &'a AtomicU8,
    deadline: Instant,
}
impl Context<'_> {
    fn check(&self) -> Result<(), Error> {
        match self.reason.load(Ordering::Acquire) {
            1 => Err(Error::Cancelled),
            2 => Err(Error::TimedOut),
            _ if Instant::now() >= self.deadline => Err(Error::TimedOut),
            _ if self.stop.load(Ordering::Acquire) => Err(Error::Limited),
            _ => Ok(()),
        }
    }
}
struct Finish<'a>(&'a AtomicBool);
impl Drop for Finish<'_> {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
fn controlled<T>(
    timeout: Duration,
    mut cancelled: impl FnMut() -> bool + Send,
    f: impl FnOnce(&Context<'_>) -> Result<T, Error>,
) -> Result<T, Error> {
    if cancelled() {
        return Err(Error::Cancelled);
    }
    let deadline = Instant::now().checked_add(timeout).ok_or(Error::Limited)?;
    let stop = AtomicBool::new(false);
    let reason = AtomicU8::new(0);
    let finish = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let _finish = Finish(&finish);
        // A sleeping cancellation monitor, not a nested computation pool.
        let monitor = std::thread::Builder::new()
            .name("CIDE Git cancelación".into())
            .spawn_scoped(scope, || {
                while !finish.load(Ordering::Acquire) {
                    let why = if cancelled() {
                        1
                    } else if Instant::now() >= deadline {
                        2
                    } else {
                        0
                    };
                    if why != 0 {
                        reason.store(why, Ordering::Release);
                        stop.store(true, Ordering::Release);
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            })
            .map_err(Error::Io)?;
        let ctx = Context {
            stop: &stop,
            reason: &reason,
            deadline,
        };
        let result = f(&ctx);
        finish.store(true, Ordering::Release);
        monitor
            .join()
            .map_err(|_| Error::Engine("Falló el monitor de cancelación".into()))?;
        ctx.check()?;
        result
    })
}

fn open(root: &Path, ctx: &Context<'_>) -> Result<(gix::Repository, Identity), Error> {
    ctx.check()?;
    if !root.is_absolute() {
        return Err(Error::InvalidRoot);
    }
    let root = root.canonicalize()?;
    preflight(&root)?;
    // No environment/global config, installation probing, includes or helpers.
    let mut options = gix::open::Options::isolated()
        .strict_config(true)
        .config_overrides([
            "core.useReplaceRefs=false",
            "core.fsmonitor=false",
            "core.untrackedCache=false",
            "core.hooksPath=NUL",
            "gc.auto=0",
            "maintenance.auto=false",
            "gitoxide.objects.allocLimit=16777216",
            "core.bigFileThreshold=16777216",
        ]);
    options.permissions.config.includes = false;
    let repo = gix::open_opts(&root, options).map_err(engine)?;
    let work = repo.workdir().ok_or(Error::InvalidRoot)?.canonicalize()?;
    if work != root {
        return Err(Error::InvalidRoot);
    }
    let git_dir = repo.git_dir().canonicalize()?;
    for file in [
        git_dir.join("index"),
        git_dir.join("config"),
        repo.common_dir().join("config"),
    ] {
        match std::fs::metadata(file) {
            Ok(meta) if meta.len() > INDEX_BYTES => return Err(Error::Limited),
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(Error::Io(e)),
            _ => {}
        }
    }
    if let Some(sections) = repo.config_snapshot().plumbing().sections_by_name("filter") {
        for section in sections {
            if section.value("clean").is_some()
                || section.value("process").is_some()
                || section.value("smudge").is_some()
            {
                return Err(Error::Filter);
            }
        }
    }
    for name in ["include", "includeIf"] {
        if repo
            .config_snapshot()
            .plumbing()
            .sections_by_name(name)
            .is_some()
        {
            return Err(Error::Unsupported);
        }
    }
    if repo
        .config_snapshot()
        .boolean("extensions.partialClone")
        .unwrap_or(false)
        || repo
            .config_snapshot()
            .string("extensions.partialClone")
            .is_some()
        || repo
            .config_snapshot()
            .string("extensions.refStorage")
            .is_some()
    {
        return Err(Error::Unsupported);
    }
    ctx.check()?;
    Ok((repo, Identity { root, git_dir }))
}

fn control_path(path: &Path, cap: u64) -> Result<Option<String>, Error> {
    use std::io::Read;
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    if file.metadata()?.len() > cap {
        return Err(Error::Limited);
    }
    let mut bytes = Vec::new();
    file.take(cap + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > cap {
        return Err(Error::Limited);
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| Error::Unsupported)
}
fn preflight(root: &Path) -> Result<(), Error> {
    let dot = root.join(".git");
    let git_dir = if dot.is_file() {
        let text = control_path(&dot, 32768)?.ok_or(Error::InvalidRoot)?;
        let value = text
            .strip_prefix("gitdir: ")
            .ok_or(Error::InvalidRoot)?
            .trim_end_matches(['\r', '\n']);
        root.join(value).canonicalize()?
    } else {
        dot.canonicalize()?
    };
    let common = if let Some(text) = control_path(&git_dir.join("commondir"), 32768)? {
        git_dir
            .join(text.trim_end_matches(['\r', '\n']))
            .canonicalize()?
    } else {
        git_dir.clone()
    };
    for file in [
        git_dir.join("index"),
        git_dir.join("config"),
        git_dir.join("config.worktree"),
        common.join("config"),
    ] {
        match std::fs::metadata(file) {
            Ok(meta) if meta.len() > INDEX_BYTES => return Err(Error::Limited),
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
    }
    Ok(())
}

fn index(repo: &gix::Repository) -> Result<gix::worktree::IndexPersistedOrInMemory, Error> {
    let index = repo.index_or_empty().map_err(engine)?;
    if index.is_sparse()
        || index.entries().len() > INDEX_ENTRIES
        || index.path_backing().len() > TREE_PATH_BYTES
    {
        return Err(Error::Limited);
    }
    Ok(gix::worktree::IndexPersistedOrInMemory::Persisted(index))
}
fn object(repo: &gix::Repository, id: gix::ObjectId, cap: u64) -> Result<gix::Object<'_>, Error> {
    let header = repo.find_header(id).map_err(engine)?;
    if header.size() > cap {
        return Err(Error::Limited);
    }
    repo.find_object(id).map_err(engine)
}

pub fn read_blob(
    root: &Path,
    from_index: bool,
    path: &str,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<Blob, Error> {
    limits.admit()?;
    if path.len() > limits.max_path
        || path.contains(['\\', '\0', '\r', '\n', ':'])
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(Error::Unsupported);
    }
    controlled(limits.timeout, cancelled, |ctx| {
        let (repo, repository) = open(root, ctx)?;
        let (oid, mode, commit) = if from_index {
            let index = index(&repo)?;
            let entries = index.entries();
            if entries
                .iter()
                .any(|e| e.path(&index).as_bytes() == path.as_bytes() && e.stage_raw() != 0)
            {
                return Err(Error::Unmerged);
            }
            let entry = index
                .entry_by_path(path.as_bytes().as_bstr())
                .ok_or(Error::Missing)?;
            if entry
                .flags
                .contains(gix::index::entry::Flags::INTENT_TO_ADD)
            {
                return Err(Error::Missing);
            }
            (entry.id, entry.mode.bits(), None)
        } else {
            let head = repo.head_id().map_err(|_| Error::Missing)?.detach();
            let commit_obj = object(&repo, head, OBJECT_BYTES)?
                .try_into_commit()
                .map_err(engine)?;
            let mut tree = object(
                &repo,
                commit_obj.tree_id().map_err(engine)?.detach(),
                INDEX_BYTES,
            )?
            .try_into_tree()
            .map_err(engine)?;
            let parts = path.split('/').collect::<Vec<_>>();
            let mut found = None;
            for (i, part) in parts.iter().enumerate() {
                ctx.check()?;
                let entry = tree.find_entry(part.as_bytes()).ok_or(Error::Missing)?;
                let id = entry.object_id();
                let mode = entry.mode().value() as u32;
                if i + 1 == parts.len() {
                    found = Some((id, mode));
                } else {
                    tree = object(&repo, id, INDEX_BYTES)?
                        .try_into_tree()
                        .map_err(engine)?;
                }
            }
            let (id, mode) = found.ok_or(Error::Missing)?;
            (id, mode, Some(head.to_string()))
        };
        if !matches!(mode, 0o100644 | 0o100755) {
            return Err(Error::Unsupported);
        }
        let mut object = object(&repo, oid, limits.max_blob as u64)?;
        if object.kind != gix::object::Kind::Blob {
            return Err(Error::Unsupported);
        }
        if gix::objs::compute_hash(oid.kind(), gix::objs::Kind::Blob, &object.data)
            .map_err(engine)?
            != oid
        {
            return Err(Error::Engine(
                "El contenido del objeto no coincide con su identificador".into(),
            ));
        }
        ctx.check()?;
        Ok(Blob {
            repository,
            commit,
            oid: oid.to_string(),
            mode,
            bytes: std::mem::take(&mut object.data),
        })
    })
}

type TreeMap = BTreeMap<Vec<u8>, (gix::ObjectId, u32)>;
fn tree_entries(repo: &gix::Repository, ctx: &Context<'_>) -> Result<TreeMap, Error> {
    let mut entries = BTreeMap::new();
    let id = repo.head_tree_id_or_empty().map_err(engine)?.detach();
    let mut todo = vec![(Vec::new(), id, 0usize)];
    let mut bytes = 0usize;
    let mut visited = 0usize;
    while let Some((prefix, id, depth)) = todo.pop() {
        ctx.check()?;
        if id.is_empty_tree() {
            continue;
        }
        let tree = object(repo, id, INDEX_BYTES)?
            .try_into_tree()
            .map_err(engine)?;
        for entry in tree.iter() {
            ctx.check()?;
            let entry = entry.map_err(engine)?;
            visited += 1;
            if visited > INDEX_ENTRIES || depth > 128 {
                return Err(Error::Limited);
            }
            let mut path = prefix.clone();
            path.extend_from_slice(entry.filename().as_bytes());
            bytes = bytes.checked_add(path.len()).ok_or(Error::Limited)?;
            if bytes > TREE_PATH_BYTES || path.len() > 4096 {
                return Err(Error::Limited);
            }
            let mode = entry.mode().value() as u32;
            if mode == 0o160000 {
                // No recursive submodule provider is admitted in this read-only block.
                return Err(Error::Unsupported);
            }
            if mode == 0o40000 {
                path.push(b'/');
                todo.push((path, entry.object_id(), depth + 1));
            } else {
                entries.insert(path, (entry.object_id(), mode));
            }
        }
    }
    Ok(entries)
}

struct Collector<'a> {
    entries: BTreeMap<Vec<u8>, Entry>,
    untracked: BTreeMap<Vec<u8>, Entry>,
    limits: Limits,
    bytes: usize,
    error: Option<Error>,
    ctx: &'a Context<'a>,
}
impl Collector<'_> {
    fn entry(&mut self, path: &[u8], untracked: bool) -> Option<&mut Entry> {
        if self.error.is_some() {
            return None;
        }
        if let Err(e) = self.ctx.check() {
            self.error = Some(e);
            return None;
        }
        let count = self.entries.len().saturating_add(self.untracked.len());
        let entries = if untracked {
            &mut self.untracked
        } else {
            &mut self.entries
        };
        if !entries.contains_key(path) {
            let bytes = self
                .bytes
                .saturating_add(path.len().saturating_mul(2))
                .saturating_add(std::mem::size_of::<Entry>() * 2 + 64);
            if count >= self.limits.max_entries
                || path.len() > self.limits.max_path
                || bytes > self.limits.max_bytes
            {
                self.error = Some(Error::Limited);
                self.ctx.stop.store(true, Ordering::Release);
                return None;
            }
            self.bytes = bytes;
            entries.insert(
                path.to_vec(),
                Entry {
                    path: path.to_vec(),
                    index: Change::Unchanged,
                    worktree: Change::Unchanged,
                    kind: Kind::Tracked,
                    submodule: false,
                },
            );
        }
        entries.get_mut(path)
    }
}
#[derive(Clone)]
struct NoSubmodules;
impl gix::status::plumbing::index_as_worktree::traits::SubmoduleStatus for NoSubmodules {
    type Output = ();
    fn status(&mut self, _: &gix::index::Entry, _: &gix::bstr::BStr) -> gix::Result<Option<()>> {
        Ok(None)
    }
}
impl<'index> gix::status::plumbing::index_as_worktree_with_renames::VisitEntry<'index>
    for Collector<'_>
{
    type ContentChange = ();
    type SubmoduleStatus = ();
    fn visit_entry(
        &mut self,
        item: gix::status::plumbing::index_as_worktree_with_renames::Entry<'index, (), ()>,
    ) {
        use gix::status::plumbing::index_as_worktree_with_renames::Summary as S;
        let Some(summary) = item.summary() else {
            return;
        };
        let Some(entry) = self.entry(
            item.destination_rela_path().as_bytes(),
            matches!(summary, S::Added),
        ) else {
            return;
        };
        match summary {
            S::Added => entry.kind = Kind::Untracked,
            S::Removed => entry.worktree = Change::Deleted,
            S::Modified => entry.worktree = Change::Modified,
            S::TypeChange => entry.worktree = Change::TypeChanged,
            S::IntentToAdd => entry.worktree = Change::Added,
            S::Conflict => {}
            S::Renamed | S::Copied => {
                self.error = Some(Error::Unsupported);
            }
        }
    }
}

pub fn read_status(
    root: &Path,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<Status, Error> {
    limits.admit()?;
    controlled(limits.timeout, cancelled, |ctx| {
        let (repo, repository) = open(root, ctx)?;
        let index = index(&repo)?;
        // Returning no submodule status would falsely mark dirty gitlinks clean.
        // Reject all gitlinks, including unmerged or newly staged ones.
        if index
            .entries()
            .iter()
            .any(|entry| entry.mode.is_submodule())
        {
            return Err(Error::Unsupported);
        }
        let mut head = tree_entries(&repo, ctx)?;
        let mut collect = Collector {
            entries: BTreeMap::new(),
            untracked: BTreeMap::new(),
            limits,
            bytes: 0,
            error: None,
            ctx,
        };
        let mut conflicts = BTreeMap::<Vec<u8>, u8>::new();
        for entry in index.entries() {
            ctx.check()?;
            let path = entry.path(&index).as_bytes();
            if entry.stage_raw() != 0 {
                *conflicts.entry(path.to_vec()).or_default() |= 1 << (entry.stage_raw() - 1);
                head.remove(path);
                continue;
            }
            let prior = head.remove(path);
            let change = if entry
                .flags
                .contains(gix::index::entry::Flags::INTENT_TO_ADD)
            {
                Change::Unchanged
            } else {
                match prior {
                    None => Change::Added,
                    Some((id, mode)) if id != entry.id || mode != entry.mode.bits() => {
                        if mode & 0o170000 != entry.mode.bits() & 0o170000 {
                            Change::TypeChanged
                        } else {
                            Change::Modified
                        }
                    }
                    _ => Change::Unchanged,
                }
            };
            if change != Change::Unchanged {
                if let Some(item) = collect.entry(path, false) {
                    item.index = change;
                    item.submodule = entry.mode.is_submodule();
                }
            }
            // Bound content work before the status pipeline can normalize or hash it.
            if !entry.mode.is_submodule() {
                let path_os = gix::path::from_bstr(path.as_bstr()).map_err(engine)?;
                if let Ok(meta) = std::fs::symlink_metadata(repository.root.join(path_os)) {
                    if meta.len() > OBJECT_BYTES {
                        return Err(Error::Limited);
                    }
                }
            }
        }
        for path in head.keys() {
            if let Some(item) = collect.entry(path, false) {
                item.index = Change::Deleted;
            }
        }
        for (path, mask) in conflicts {
            if let Some(item) = collect.entry(&path, false) {
                item.kind = Kind::Conflict(mask);
            }
        }
        if let Some(error) = collect.error.take() {
            return Err(error);
        }
        let options = gix::status::index_worktree::Options {
            sorting: None,
            rewrites: None,
            thread_limit: Some(1),
            dirwalk_options: Some(
                repo.dirwalk_options()
                    .map_err(engine)?
                    .emit_untracked(gix::dir::walk::EmissionMode::Matching)
                    .emit_ignored(None),
            ),
        };
        let result = repo.index_worktree_status(
            &index,
            None::<&gix::bstr::BStr>,
            &mut collect,
            gix::status::plumbing::index_as_worktree::traits::FastEq,
            NoSubmodules,
            &mut gix::progress::Discard,
            ctx.stop,
            options,
        );
        if let Some(error) = collect.error.take() {
            return Err(error);
        }
        ctx.check()?;
        result.map_err(engine)?;
        // A staged deletion and the retained untracked workfile are distinct
        // records even when their paths match, as in Git porcelain output.
        let mut entries: Vec<_> = collect
            .entries
            .into_values()
            .chain(collect.untracked.into_values())
            .collect();
        entries.sort_unstable_by(|a, b| {
            a.path.cmp(&b.path).then_with(|| {
                matches!(a.kind, Kind::Untracked).cmp(&matches!(b.kind, Kind::Untracked))
            })
        });
        Ok(Status {
            repository,
            entries,
        })
    })
}
