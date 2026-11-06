//! Bounded first-parent history of one exact Git path. No worktree, HEAD,
//! rename detection, editable document or external executable participates.
use super::{Change, Context, Error, Identity, Limits, controlled, engine, open};
use crate::revision::{Budget, commit, path_valid, tree_entry, verified};
use gix::bstr::ByteSlice;
use std::path::{Path, PathBuf};

const MATCHES: usize = 24;
const WALKED: usize = 128;
const COMMIT_BYTES: usize = 64 * 1024;
const SHALLOW_BYTES: usize = 32768;

/// Only this provider can mint a continuation. Its identity binds a verified
/// first-parent position to its original repository, pinned start and raw path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathHistoryCursor {
    root: PathBuf,
    git_dir: PathBuf,
    start: String,
    path: Vec<u8>,
    next: String,
}
impl PathHistoryCursor {
    pub fn memory_bytes(&self) -> usize {
        self.root.capacity()
            + self.git_dir.capacity()
            + self.start.capacity()
            + self.path.capacity()
            + self.next.capacity()
    }
}

#[derive(Debug)]
pub struct PathHistoryEntry {
    pub commit: String,
    pub parent: Option<String>,
    pub summary: Vec<u8>,
    pub author: Vec<u8>,
    pub change: Change,
}
impl PathHistoryEntry {
    pub fn memory_bytes(&self) -> usize {
        self.commit.capacity()
            + self.parent.as_ref().map_or(0, String::capacity)
            + self.summary.capacity()
            + self.author.capacity()
    }
}
pub struct PathHistoryPage {
    pub repository: Identity,
    pub start: String,
    pub path: Vec<u8>,
    pub entries: Vec<PathHistoryEntry>,
    pub next: Option<PathHistoryCursor>,
    pub scanned: usize,
    /// The next comparison base is outside the declared shallow boundary.
    /// No change is fabricated for the boundary commit, even if its tree exists.
    pub shallow: bool,
    /// Summary/author previews were clipped; commits and paths are never clipped.
    pub metadata_limited: bool,
}
impl PathHistoryPage {
    pub fn memory_bytes(&self) -> usize {
        self.repository.root.capacity()
            + self.repository.git_dir.capacity()
            + self.start.capacity()
            + self.path.capacity()
            + self.entries.capacity() * std::mem::size_of::<PathHistoryEntry>()
            + self
                .entries
                .iter()
                .map(PathHistoryEntry::memory_bytes)
                .sum::<usize>()
            + self
                .next
                .as_ref()
                .map_or(0, PathHistoryCursor::memory_bytes)
    }
}
impl std::fmt::Debug for PathHistoryPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PathHistoryPage")
            .field("root", &self.repository.root)
            .field("start", &self.start)
            .field("path", &self.path)
            .field("entries", &self.entries)
            .field("next", &self.next)
            .field("scanned", &self.scanned)
            .field("shallow", &self.shallow)
            .field("metadata_limited", &self.metadata_limited)
            .finish()
    }
}

fn oid(value: &str) -> Result<gix::ObjectId, Error> {
    if ![40, 64].contains(&value.len()) || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::Unsupported);
    }
    let id = gix::ObjectId::from_hex(value.as_bytes()).map_err(engine)?;
    if id.is_null() {
        return Err(Error::Unsupported);
    }
    Ok(id)
}

fn charge_commit(
    repo: &gix::Repository,
    id: gix::ObjectId,
    budget: &mut Budget,
    ctx: &Context<'_>,
) -> Result<(), Error> {
    ctx.check()?;
    let header = repo
        .try_find_header(id)
        .map_err(engine)?
        .ok_or(Error::Missing)?;
    let size = usize::try_from(header.size()).map_err(|_| Error::Limited)?;
    if size > COMMIT_BYTES {
        return Err(Error::Limited);
    }
    budget.charge(size)?;
    Ok(())
}

fn shallow(
    repo: &gix::Repository,
    limits: Limits,
    budget: &mut Budget,
    ctx: &Context<'_>,
) -> Result<Vec<gix::ObjectId>, Error> {
    ctx.check()?;
    let file = repo.shallow_file().map_err(engine)?;
    // A local config must not redirect this bounded control read to arbitrary
    // external data. The default shared Git control file is sufficient here.
    if file != repo.common_dir().join("shallow") {
        return Err(Error::Unsupported);
    }
    let Some(text) = super::control_path(&file, SHALLOW_BYTES.min(limits.max_bytes) as u64)? else {
        return Ok(Vec::new());
    };
    budget.charge(text.len().saturating_mul(2))?;
    let mut ids = Vec::new();
    for line in text.split_terminator('\n') {
        ctx.check()?;
        if ids.len() >= 512 {
            return Err(Error::Limited);
        }
        let id = oid(line.strip_suffix('\r').unwrap_or(line))?;
        if id.kind() != repo.object_hash() {
            return Err(Error::Unsupported);
        }
        budget.charge(std::mem::size_of::<gix::ObjectId>() * 2)?;
        ids.push(id);
    }
    Ok(ids)
}

/// At most 24 matching commits and 128 first-parent comparisons are admitted per
/// call. An empty page can still carry `next` after 128 unrelated commits. `None`
/// means traversal ended, except `shallow=true` explicitly marks an incomplete
/// declared boundary. Missing/corrupt objects elsewhere remain errors.
pub fn read_path_history(
    root: &Path,
    pinned_start: &str,
    path: &[u8],
    continuation: Option<&PathHistoryCursor>,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<PathHistoryPage, Error> {
    limits.admit()?;
    let start_id = oid(pinned_start)?;
    let start = start_id.to_string();
    path_valid(path, limits.max_path)?;
    let maximum = MATCHES.min(limits.max_entries);
    if maximum == 0 {
        return Err(Error::Limited);
    }
    controlled(limits.timeout, cancelled, |ctx| {
        let (repo, repository) = open(root, ctx)?;
        if start_id.kind() != repo.object_hash() {
            return Err(Error::Unsupported);
        }
        let next_id = match continuation {
            Some(c)
                if c.root == repository.root
                    && c.git_dir == repository.git_dir
                    && c.start == start
                    && c.path == path =>
            {
                oid(&c.next)?
            }
            Some(_) => return Err(Error::Stale),
            None => start_id,
        };
        let mut budget = Budget::new(limits);
        budget.charge(path.len().saturating_mul(2).saturating_add(256))?;
        let boundaries = shallow(&repo, limits, &mut budget, ctx)?;
        let mut entries = Vec::with_capacity(maximum);
        let mut pending = Some(next_id);
        let mut cached = None;
        let mut scanned = 0;
        let mut reached_shallow = false;
        let mut metadata_limited = false;
        while let Some(id) = pending.take() {
            ctx.check()?;
            let current = if let Some(cached) = cached.take() {
                cached
            } else {
                charge_commit(&repo, id, &mut budget, ctx)?;
                commit(&repo, id, limits, &mut budget, ctx)?
            };
            scanned += 1;
            let after = tree_entry(&repo, current.tree, path, limits, &mut budget, ctx)?;
            if boundaries.contains(&id) {
                reached_shallow = true;
                break;
            }
            let parent_id = current.parents.first().copied();
            let before = if let Some(parent) = parent_id {
                charge_commit(&repo, parent, &mut budget, ctx)?;
                let parent = commit(&repo, parent, limits, &mut budget, ctx)?;
                let entry = tree_entry(&repo, parent.tree, path, limits, &mut budget, ctx)?;
                cached = Some(parent);
                entry
            } else {
                None
            };
            if before != after {
                charge_commit(&repo, id, &mut budget, ctx)?;
                let value = verified(&repo, id, COMMIT_BYTES.min(limits.max_bytes), ctx)?
                    .try_into_commit()
                    .map_err(engine)?;
                let author = value.author().map_err(engine)?;
                let message = value.message_raw().map_err(engine)?;
                let summary = message
                    .as_bytes()
                    .split(|b| matches!(b, b'\r' | b'\n'))
                    .next()
                    .unwrap_or_default();
                let clipped = |bytes: &[u8], cap: usize, limited: &mut bool| {
                    *limited |= bytes.len() > cap;
                    bytes[..bytes.len().min(cap)].to_vec()
                };
                let change = match (before, after) {
                    (None, Some(_)) => Change::Added,
                    (Some(_), None) => Change::Deleted,
                    (Some(a), Some(b)) if a.mode & 0o170000 != b.mode & 0o170000 => {
                        Change::TypeChanged
                    }
                    _ => Change::Modified,
                };
                let item = PathHistoryEntry {
                    commit: id.to_string(),
                    parent: parent_id.map(|id| id.to_string()),
                    summary: clipped(summary, 256, &mut metadata_limited),
                    author: clipped(author.name.as_bytes(), 96, &mut metadata_limited),
                    change,
                };
                budget.charge(std::mem::size_of::<PathHistoryEntry>() * 2 + item.memory_bytes())?;
                entries.push(item);
            }
            pending = parent_id;
            if entries.len() >= maximum || scanned >= WALKED {
                break;
            }
        }
        let next = pending.map(|id| PathHistoryCursor {
            root: repository.root.clone(),
            git_dir: repository.git_dir.clone(),
            start: start.clone(),
            path: path.to_vec(),
            next: id.to_string(),
        });
        let page = PathHistoryPage {
            repository,
            start: start.clone(),
            path: path.to_vec(),
            entries,
            next,
            scanned,
            shallow: reached_shallow,
            metadata_limited,
        };
        if page.memory_bytes() > limits.max_bytes {
            return Err(Error::Limited);
        }
        ctx.check()?;
        Ok(page)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::BTreeMap,
        fs,
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
            assert!(base.starts_with(workspace) && !base.is_symlink());
            let root = base.join(format!(
                "path-history-{}-{}-{}",
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
            max_entries: 4096,
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
    fn save(
        repo: &gix::Repository,
        tree: gix::ObjectId,
        parents: &[gix::ObjectId],
        message: &str,
    ) -> gix::ObjectId {
        let sig = gix::actor::SignatureRef {
            name: b"Consumidor Rust".as_bstr(),
            email: b"fixture@invalid".as_bstr(),
            time: "1 +0000",
        };
        repo.commit_as(sig, sig, "HEAD", message, tree, parents.iter().copied())
            .unwrap()
            .detach()
    }
    fn query(f: &Fixture, start: gix::ObjectId) -> PathHistoryPage {
        read_path_history(
            &f.root,
            &start.to_string(),
            b"source.p",
            None,
            limits(),
            || false,
        )
        .unwrap()
    }
    fn object_path(f: &Fixture, id: gix::ObjectId) -> PathBuf {
        let name = id.to_string();
        let path = f
            .root
            .join(".git/objects")
            .join(&name[..2])
            .join(&name[2..]);
        // Only owned loose objects are made writable for fault injection.
        #[cfg(windows)]
        #[allow(clippy::permissions_set_readonly_false)]
        {
            let mut p = fs::metadata(&path).unwrap().permissions();
            p.set_readonly(false);
            fs::set_permissions(&path, p).unwrap();
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        }
        path
    }
    fn image(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
        fn walk(root: &Path, path: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
            for e in fs::read_dir(path).unwrap() {
                let p = e.unwrap().path();
                let m = fs::symlink_metadata(&p).unwrap();
                assert!(!m.file_type().is_symlink());
                if m.is_dir() {
                    walk(root, &p, out)
                } else {
                    out.insert(
                        p.strip_prefix(root).unwrap().to_owned(),
                        fs::read(p).unwrap(),
                    );
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(root, root, &mut out);
        out
    }

    #[test]
    fn path_history_exact_changes_creation_deletion_mode_and_merge_use_first_parent_without_blob_or_worktree_reads()
     {
        let f = Fixture::new();
        let repo = gix::init(&f.root).unwrap();
        let a = repo.write_blob(b"old\r\n\xff").unwrap().detach();
        let b = repo.write_blob(b"new\n").unwrap().detach();
        let ta = tree(&repo, &[(b"source.p", 0o100644, a)]);
        let root = save(&repo, ta, &[], "Creación");
        let unrelated = save(
            &repo,
            tree(
                &repo,
                &[(b"other", 0o100644, a), (b"source.p", 0o100644, a)],
            ),
            &[root],
            "Otro archivo",
        );
        let change = save(
            &repo,
            tree(&repo, &[(b"source.p", 0o100644, b)]),
            &[unrelated],
            "Cambio\nDetalle",
        );
        let mode = save(
            &repo,
            tree(&repo, &[(b"source.p", 0o100755, b)]),
            &[change],
            "Modo",
        );
        let empty = tree(&repo, &[]);
        let deletion = save(&repo, empty, &[mode], "Eliminación");
        let merge = save(&repo, empty, &[deletion, root], "Integración");
        drop(repo);
        // A listing only needs entry IDs and modes. It must not load either blob
        // or misinterpret an unrelated worktree buffer as a committed source.
        fs::remove_file(object_path(&f, a)).unwrap();
        fs::remove_file(object_path(&f, b)).unwrap();
        fs::write(f.root.join("source.p"), b"working bytes").unwrap();
        let before = image(&f.root);
        let page = query(&f, merge);
        assert_eq!(
            page.entries
                .iter()
                .map(|e| e.commit.clone())
                .collect::<Vec<_>>(),
            vec![
                deletion.to_string(),
                mode.to_string(),
                change.to_string(),
                root.to_string()
            ]
        );
        assert_eq!(
            page.entries.iter().map(|e| e.change).collect::<Vec<_>>(),
            vec![
                Change::Deleted,
                Change::Modified,
                Change::Modified,
                Change::Added
            ]
        );
        assert_eq!(page.entries[2].summary, "Cambio".as_bytes());
        assert_eq!(page.entries[2].author, b"Consumidor Rust");
        assert!(page.entries.last().unwrap().parent.is_none());
        assert!(page.next.is_none() && !page.shallow);
        assert_eq!(page.scanned, 6);
        assert_eq!(image(&f.root), before);
        let absent = read_path_history(
            &f.root,
            &merge.to_string(),
            b"never-present",
            None,
            limits(),
            || false,
        )
        .unwrap();
        assert!(absent.entries.is_empty() && absent.next.is_none() && !absent.shallow);
    }

    #[test]
    fn path_history_page_cursor_is_pinned_and_bound_to_repository_path_and_start() {
        let f = Fixture::new();
        let repo = gix::init(&f.root).unwrap();
        let mut parent = None;
        let mut ids = Vec::new();
        for n in 0..27 {
            let b = repo.write_blob(format!("{n}")).unwrap().detach();
            let id = save(
                &repo,
                tree(&repo, &[(b"source.p", 0o100644, b)]),
                &parent.into_iter().collect::<Vec<_>>(),
                "Cambio",
            );
            ids.push(id);
            parent = Some(id);
        }
        let pinned = ids[26];
        let first = query(&f, pinned);
        assert_eq!(first.entries.len(), 24);
        let cursor = first.next.unwrap();
        assert!(cursor.memory_bytes() > 0);
        let blob = repo.write_blob(b"later").unwrap().detach();
        save(
            &repo,
            tree(&repo, &[(b"source.p", 0o100644, blob)]),
            &[pinned],
            "HEAD posterior",
        );
        drop(repo);
        let second = read_path_history(
            &f.root,
            &pinned.to_string(),
            b"source.p",
            Some(&cursor),
            limits(),
            || false,
        )
        .unwrap();
        assert_eq!(
            second
                .entries
                .iter()
                .map(|e| e.commit.clone())
                .collect::<Vec<_>>(),
            ids[..3]
                .iter()
                .rev()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        );
        assert!(second.next.is_none());
        assert!(matches!(
            read_path_history(
                &f.root,
                &pinned.to_string(),
                b"other",
                Some(&cursor),
                limits(),
                || false
            ),
            Err(Error::Stale)
        ));
        assert!(matches!(
            read_path_history(
                &f.root,
                &ids[25].to_string(),
                b"source.p",
                Some(&cursor),
                limits(),
                || false
            ),
            Err(Error::Stale)
        ));
        let other = Fixture::new();
        drop(gix::init(&other.root).unwrap());
        assert!(matches!(
            read_path_history(
                &other.root,
                &pinned.to_string(),
                b"source.p",
                Some(&cursor),
                limits(),
                || false
            ),
            Err(Error::Stale)
        ));
    }

    #[test]
    fn path_history_empty_page_after_walk_limit_keeps_continuation() {
        let f = Fixture::new();
        let repo = gix::init(&f.root).unwrap();
        let blob = repo.write_blob(b"A").unwrap().detach();
        let t = tree(&repo, &[(b"source.p", 0o100644, blob)]);
        let root = save(&repo, t, &[], "Creación");
        let mut head = root;
        for _ in 0..129 {
            head = save(&repo, t, &[head], "Sin cambios en la ruta");
        }
        drop(repo);
        let first = query(&f, head);
        assert!(first.entries.is_empty());
        assert_eq!(first.scanned, 128);
        assert!(first.next.is_some());
        let second = read_path_history(
            &f.root,
            &head.to_string(),
            b"source.p",
            first.next.as_ref(),
            limits(),
            || false,
        )
        .unwrap();
        assert_eq!(second.entries.len(), 1);
        assert_eq!(second.entries[0].commit, root.to_string());
        assert!(second.next.is_none());
    }

    #[test]
    fn path_history_declared_shallow_boundary_is_partial_and_missing_parent_without_it_is_error() {
        let f = Fixture::new();
        let repo = gix::init(&f.root).unwrap();
        let a = repo.write_blob(b"A").unwrap().detach();
        let b = repo.write_blob(b"B").unwrap().detach();
        let root = save(
            &repo,
            tree(&repo, &[(b"source.p", 0o100644, a)]),
            &[],
            "Raíz",
        );
        let boundary = save(
            &repo,
            tree(&repo, &[(b"source.p", 0o100644, a)]),
            &[root],
            "Límite",
        );
        let head = save(
            &repo,
            tree(&repo, &[(b"source.p", 0o100644, b)]),
            &[boundary],
            "Cambio",
        );
        drop(repo);
        fs::remove_file(object_path(&f, root)).unwrap();
        assert!(
            read_path_history(
                &f.root,
                &head.to_string(),
                b"source.p",
                None,
                limits(),
                || false
            )
            .is_err()
        );
        fs::write(f.root.join(".git/shallow"), format!("{boundary}\n")).unwrap();
        let page = query(&f, head);
        assert!(page.shallow);
        assert!(page.next.is_none());
        assert_eq!(page.entries.len(), 1);
        assert_eq!(page.entries[0].commit, head.to_string());
    }

    #[test]
    fn path_history_corrupt_tree_invalid_path_cancellation_and_quotas_are_errors_not_empty_history()
    {
        let f = Fixture::new();
        let repo = gix::init(&f.root).unwrap();
        let blob = repo.write_blob(b"A").unwrap().detach();
        let t = tree(&repo, &[(b"source.p", 0o100644, blob)]);
        let head = save(&repo, t, &[], "Raíz");
        drop(repo);
        assert!(matches!(
            read_path_history(
                &f.root,
                &head.to_string(),
                b"source.p",
                None,
                limits(),
                || true
            ),
            Err(Error::Cancelled)
        ));
        assert!(matches!(
            read_path_history(
                &f.root,
                &head.to_string(),
                b"../source.p",
                None,
                limits(),
                || false
            ),
            Err(Error::Unsupported)
        ));
        let mut low = limits();
        low.max_bytes = 100;
        assert!(matches!(
            read_path_history(&f.root, &head.to_string(), b"source.p", None, low, || false),
            Err(Error::Limited)
        ));
        fs::write(object_path(&f, t), b"corrupt advertised tree").unwrap();
        assert!(
            read_path_history(
                &f.root,
                &head.to_string(),
                b"source.p",
                None,
                limits(),
                || false
            )
            .is_err()
        );
    }
}
