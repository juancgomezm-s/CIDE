//! Exact-byte, exact-path first-parent attribution. This intentionally does not
//! claim Git's all-parent/rename-following blame semantics. Unresolved lines
//! carry no author instead of inventing an origin at a resource/shallow limit.
use super::{Context, Error, Identity, Limits, controlled, engine, open};
use crate::{
    diff_model::{DiffLimits, DiffLine, DiffModel, DiffSide, DiffStatus},
    path_history::{charge_commit, shallow},
    revision::{Budget, Entry, commit, path_valid, tree_entry, verified},
};
use gix::bstr::ByteSlice;
use std::{ops::Range, path::Path, sync::Arc};

const PAGE: usize = 128;
const WALK: usize = 128;
const SOURCE: usize = 2 * 1024 * 1024;
const READS: usize = 16 * 1024 * 1024;
const WORK: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlameBoundary {
    Complete,
    WalkLimit,
    Shallow,
    ComparisonLimit,
    ReadLimit,
}
impl BlameBoundary {
    pub fn label(self) -> &'static str {
        match self {
            Self::Complete => "Atribución completa",
            Self::WalkLimit => "Recorrido limitado",
            Self::Shallow => "Historial superficial",
            Self::ComparisonLimit => "Comparación limitada",
            Self::ReadLimit => "Lectura limitada",
        }
    }
}

#[derive(Debug)]
pub struct BlameCommit {
    pub commit: String,
    pub parent: Option<String>,
    pub author: Vec<u8>,
    pub summary: Vec<u8>,
}
#[derive(Debug)]
pub struct BlameLine {
    /// Zero-based line in the pinned original file; range includes its terminator.
    pub line: usize,
    pub range: Range<usize>,
    /// Zero-based line in the attributed revision, or last known ancestor for an
    /// unresolved line. Only commit=Some makes this an attribution.
    pub original_line: usize,
    pub commit: Option<usize>,
}
pub struct BlamePage {
    pub repository: Identity,
    pub start: String,
    pub path: Vec<u8>,
    pub blob_oid: String,
    pub source: Arc<Vec<u8>>,
    pub first: usize,
    pub total_lines: usize,
    pub rows: Vec<BlameLine>,
    pub commits: Vec<BlameCommit>,
    pub boundary: BlameBoundary,
    pub metadata_limited: bool,
    pub scanned: usize,
}
impl BlamePage {
    pub fn memory_bytes(&self) -> usize {
        self.repository.root.capacity()
            + self.repository.git_dir.capacity()
            + self.start.capacity()
            + self.path.capacity()
            + self.blob_oid.capacity()
            + self.source.capacity()
            + self.rows.capacity() * std::mem::size_of::<BlameLine>()
            + self.commits.capacity() * std::mem::size_of::<BlameCommit>()
            + self
                .commits
                .iter()
                .map(|c| {
                    c.commit.capacity()
                        + c.parent.as_ref().map_or(0, String::capacity)
                        + c.author.capacity()
                        + c.summary.capacity()
                })
                .sum::<usize>()
    }
}
impl std::fmt::Debug for BlamePage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlamePage")
            .field("root", &self.repository.root)
            .field("start", &self.start)
            .field("path", &self.path)
            .field("blob_oid", &self.blob_oid)
            .field("first", &self.first)
            .field("total_lines", &self.total_lines)
            .field("rows", &self.rows)
            .field("commits", &self.commits)
            .field("boundary", &self.boundary)
            .field("scanned", &self.scanned)
            .finish()
    }
}
fn diff_limits(limits: Limits, work: usize) -> DiffLimits {
    DiffLimits {
        max_input_bytes: SOURCE * 2,
        max_lines: 200_000,
        max_work: work,
        max_metadata_bytes: limits.max_bytes.min(4 * 1024 * 1024),
        max_hunks: 4096,
    }
}
fn read_blob(
    repo: &gix::Repository,
    entry: Entry,
    limits: Limits,
    read: &mut usize,
    ctx: &Context<'_>,
) -> Result<Option<Arc<Vec<u8>>>, Error> {
    ctx.check()?;
    let h = repo
        .try_find_header(entry.id)
        .map_err(engine)?
        .ok_or(Error::Missing)?;
    let size = usize::try_from(h.size()).map_err(|_| Error::Limited)?;
    if size > limits.max_blob.min(SOURCE) {
        return Err(Error::Limited);
    }
    let next = read.checked_add(size).ok_or(Error::Limited)?;
    if next > READS {
        return Ok(None);
    }
    let mut object = verified(repo, entry.id, limits.max_blob.min(SOURCE), ctx)?;
    if object.kind != gix::object::Kind::Blob {
        return Err(Error::Unsupported);
    }
    *read = next;
    Ok(Some(Arc::new(std::mem::take(&mut object.data))))
}
fn origin(
    repo: &gix::Repository,
    id: gix::ObjectId,
    parent: Option<gix::ObjectId>,
    limits: Limits,
    budget: &mut Budget,
    ctx: &Context<'_>,
    limited: &mut bool,
) -> Result<BlameCommit, Error> {
    charge_commit(repo, id, budget, ctx)?;
    let c = verified(repo, id, 65536.min(limits.max_bytes), ctx)?
        .try_into_commit()
        .map_err(engine)?;
    let author = c.author().map_err(engine)?;
    let message = c.message_raw().map_err(engine)?;
    let summary = message
        .as_bytes()
        .split(|b| matches!(b, b'\r' | b'\n'))
        .next()
        .unwrap_or_default();
    *limited |= author.name.len() > 96 || summary.len() > 256;
    let result = BlameCommit {
        commit: id.to_string(),
        parent: parent.map(|id| id.to_string()),
        author: author.name.as_bytes()[..author.name.len().min(96)].to_vec(),
        summary: summary[..summary.len().min(256)].to_vec(),
    };
    budget.charge(
        std::mem::size_of::<BlameCommit>() * 2
            + result.author.capacity()
            + result.summary.capacity()
            + 128,
    )?;
    Ok(result)
}

/// Returns at most128 original-file rows, attributed along at most128 first
/// parents. Traversal/read/comparison limits produce explicit partial rows;
/// invalid/missing/corrupt advertised objects remain errors, never origins.
pub fn read_blame(
    root: &Path,
    pinned_start: &str,
    path: &[u8],
    first_line: usize,
    count: usize,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<BlamePage, Error> {
    limits.admit()?;
    path_valid(path, limits.max_path)?;
    if ![40, 64].contains(&pinned_start.len())
        || !pinned_start.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(Error::Unsupported);
    }
    let id = gix::ObjectId::from_hex(pinned_start.as_bytes()).map_err(engine)?;
    if id.is_null() {
        return Err(Error::Unsupported);
    }
    let count = count.min(PAGE).min(limits.max_entries);
    if count == 0 {
        return Err(Error::Limited);
    }
    controlled(limits.timeout, cancelled, |ctx| {
        let (repo, repository) = open(root, ctx)?;
        if id.kind() != repo.object_hash() {
            return Err(Error::Unsupported);
        }
        let mut budget = Budget::new(limits);
        // Keep both header admission and the verified commit reader at64KiB.
        // These are logical/cooperative limits, not hostile-I/O allocation guarantees.
        let commit_limits = Limits {
            max_bytes: limits.max_bytes.min(65536),
            ..limits
        };
        budget.charge(path.len() * 2 + 512)?;
        let boundaries = shallow(&repo, limits, &mut budget, ctx)?;
        charge_commit(&repo, id, &mut budget, ctx)?;
        let mut current = commit(&repo, id, commit_limits, &mut budget, ctx)?;
        let mut entry = tree_entry(&repo, current.tree, path, limits, &mut budget, ctx)?
            .ok_or(Error::Missing)?;
        let blob_oid = entry.id.to_string();
        let mut read = 0;
        let source = read_blob(&repo, entry, limits, &mut read, ctx)?.ok_or(Error::Limited)?;
        let mut work = WORK;
        let index = DiffModel::compute(&source, &source, diff_limits(limits, work), || {
            ctx.check().is_err()
        });
        if index.status() == DiffStatus::Cancelled {
            ctx.check()?;
            return Err(Error::Cancelled);
        }
        if index.status() != DiffStatus::Exact {
            return Err(Error::Limited);
        }
        work = work.saturating_sub(index.stats().work_units);
        let total_lines = index.line_count(DiffSide::Right).ok_or(Error::Limited)?;
        let first = first_line.min(total_lines);
        let end = first.saturating_add(count).min(total_lines);
        let mut rows = (first..end)
            .map(|line| BlameLine {
                line,
                range: index
                    .line_byte_range(DiffSide::Right, DiffLine(line))
                    .expect("indexed line"),
                original_line: line,
                commit: None,
            })
            .collect::<Vec<_>>();
        drop(index);
        budget.charge(rows.capacity() * std::mem::size_of::<BlameLine>())?;
        let mut bytes = source.clone();
        let mut commits = Vec::new();
        let mut boundary = BlameBoundary::Complete;
        let mut scanned = 0;
        let mut metadata_limited = false;
        while rows.iter().any(|r| r.commit.is_none()) {
            ctx.check()?;
            if scanned >= WALK {
                boundary = BlameBoundary::WalkLimit;
                break;
            }
            scanned += 1;
            if boundaries.contains(&current.id) {
                boundary = BlameBoundary::Shallow;
                break;
            }
            let parent_id = current.parents.first().copied();
            let mut inherited = Vec::new();
            let next = if let Some(parent_id) = parent_id {
                charge_commit(&repo, parent_id, &mut budget, ctx)?;
                let parent = commit(&repo, parent_id, commit_limits, &mut budget, ctx)?;
                let before = tree_entry(&repo, parent.tree, path, limits, &mut budget, ctx)?;
                if let Some(before) = before {
                    let parent_bytes = if before.id == entry.id {
                        bytes.clone()
                    } else {
                        match read_blob(&repo, before, limits, &mut read, ctx)? {
                            Some(bytes) => bytes,
                            None => {
                                boundary = BlameBoundary::ReadLimit;
                                break;
                            }
                        }
                    };
                    if before.id == entry.id {
                        for (i, row) in rows.iter().enumerate() {
                            if row.commit.is_none() {
                                inherited.push((i, row.original_line));
                            }
                        }
                    } else {
                        let model = DiffModel::compute(
                            &parent_bytes,
                            &bytes,
                            diff_limits(limits, work),
                            || ctx.check().is_err(),
                        );
                        if model.status() == DiffStatus::Cancelled {
                            ctx.check()?;
                            return Err(Error::Cancelled);
                        }
                        work = work.saturating_sub(model.stats().work_units);
                        if model.status() != DiffStatus::Exact {
                            boundary = BlameBoundary::ComparisonLimit;
                            break;
                        }
                        for (i, row) in rows.iter().enumerate() {
                            if row.commit.is_none() {
                                if let Some(line) = model.corresponding_line(
                                    DiffSide::Right,
                                    DiffLine(row.original_line),
                                ) {
                                    inherited.push((i, line.0));
                                }
                            }
                        }
                    }
                    Some((parent, before, parent_bytes))
                } else {
                    None
                }
            } else {
                None
            };
            let assigned = rows
                .iter()
                .enumerate()
                .any(|(i, row)| row.commit.is_none() && !inherited.iter().any(|(j, _)| *j == i));
            if assigned {
                let record = origin(
                    &repo,
                    current.id,
                    parent_id,
                    limits,
                    &mut budget,
                    ctx,
                    &mut metadata_limited,
                )?;
                let ordinal = commits.len();
                commits.push(record);
                for (i, row) in rows.iter_mut().enumerate() {
                    if row.commit.is_none() && !inherited.iter().any(|(j, _)| *j == i) {
                        row.commit = Some(ordinal);
                    }
                }
            }
            for (i, line) in inherited {
                rows[i].original_line = line;
            }
            if let Some((parent, before, parent_bytes)) = next {
                current = parent;
                entry = before;
                bytes = parent_bytes;
            } else {
                break;
            }
        }
        ctx.check()?;
        let page = BlamePage {
            repository,
            start: id.to_string(),
            path: path.to_vec(),
            blob_oid,
            source,
            first,
            total_lines,
            rows,
            commits,
            boundary,
            metadata_limited,
            scanned,
        };
        if page.memory_bytes() > limits.max_bytes {
            return Err(Error::Limited);
        }
        Ok(page)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::BTreeMap,
        fs,
        path::PathBuf,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };
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
            assert!(base.starts_with(workspace));
            let root = base.join(format!(
                "blame-core-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir(&root).unwrap();
            Self { root, base }
        }
    }
    #[cfg(windows)]
    #[allow(clippy::permissions_set_readonly_false)]
    fn writable(path: &Path) {
        let mut p = fs::metadata(path).unwrap().permissions();
        p.set_readonly(false);
        fs::set_permissions(path, p).unwrap();
    }
    #[cfg(unix)]
    fn writable(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fn walk(path: &Path) {
                for e in fs::read_dir(path).unwrap() {
                    let p = e.unwrap().path();
                    let m = fs::symlink_metadata(&p).unwrap();
                    assert!(!m.file_type().is_symlink());
                    if m.is_dir() {
                        walk(&p)
                    } else if m.permissions().readonly() {
                        writable(&p);
                    }
                }
            }
            if self.root.parent() == Some(self.base.as_path())
                && !self.root.is_symlink()
                && self
                    .root
                    .canonicalize()
                    .is_ok_and(|p| p.starts_with(&self.base))
            {
                walk(&self.root);
                fs::remove_dir_all(&self.root).unwrap();
            }
        }
    }
    fn limits() -> Limits {
        Limits {
            max_blob: SOURCE,
            max_entries: 4096,
            max_path: 4096,
            max_bytes: 4 * 1024 * 1024,
            timeout: Duration::from_secs(5),
        }
    }
    fn save(
        repo: &gix::Repository,
        bytes: &[u8],
        parents: &[gix::ObjectId],
        message: &str,
    ) -> gix::ObjectId {
        let b = repo.write_blob(bytes).unwrap().detach();
        let tree = repo
            .write_object(gix::objs::Tree {
                entries: vec![gix::objs::tree::Entry {
                    filename: b"source.p".to_vec().into(),
                    mode: 0o100644u32.try_into().unwrap(),
                    oid: b,
                }],
            })
            .unwrap()
            .detach();
        let sig = gix::actor::SignatureRef {
            name: b"Autor Rust".as_bstr(),
            email: b"fixture@invalid".as_bstr(),
            time: "1 +0000",
        };
        repo.commit_as(sig, sig, "HEAD", message, tree, parents.iter().copied())
            .unwrap()
            .detach()
    }
    fn query(f: &Fixture, start: gix::ObjectId) -> BlamePage {
        read_blame(
            &f.root,
            &start.to_string(),
            b"source.p",
            0,
            128,
            limits(),
            || false,
        )
        .unwrap()
    }
    fn attributed(page: &BlamePage) -> Vec<Option<String>> {
        page.rows
            .iter()
            .map(|r| r.commit.map(|i| page.commits[i].commit.clone()))
            .collect()
    }
    fn image(path: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
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
        walk(path, path, &mut out);
        out
    }
    #[test]
    fn blame_tracks_insert_delete_and_raw_lines_with_pinned_read_only_sources() {
        let f = Fixture::new();
        let repo = gix::init(&f.root).unwrap();
        let root = save(&repo, b"uno\r\nigual\r\ndos\xff\n", &[], "Origen");
        let inserted = save(
            &repo,
            b"nuevo\nuno\r\nigual\r\ndos\xff\n",
            &[root],
            "Inserta\nDetalle",
        );
        let deleted = save(
            &repo,
            b"nuevo\nuno\r\ndos\xff\n",
            &[inserted],
            "Quita repetida",
        );
        save(&repo, b"HEAD posterior\n", &[deleted], "Posterior");
        drop(repo);
        fs::write(f.root.join("source.p"), b"working bytes ignored").unwrap();
        let before = image(&f.root);
        let page = query(&f, deleted);
        assert_eq!(page.boundary, BlameBoundary::Complete);
        assert_eq!(page.source.as_slice(), b"nuevo\nuno\r\ndos\xff\n");
        assert_eq!(
            attributed(&page),
            vec![
                Some(inserted.to_string()),
                Some(root.to_string()),
                Some(root.to_string())
            ]
        );
        assert_eq!(
            page.rows
                .iter()
                .map(|r| r.original_line)
                .collect::<Vec<_>>(),
            vec![0, 0, 2]
        );
        assert_eq!(&page.source[page.rows[2].range.clone()], b"dos\xff\n");
        assert_eq!(page.commits[0].summary, b"Inserta");
        assert_eq!(image(&f.root), before);
    }
    #[test]
    fn blame_page_ranges_empty_file_and_first_parent_merge_are_explicit() {
        let f = Fixture::new();
        let repo = gix::init(&f.root).unwrap();
        let root = save(&repo, b"a\na\nz\n", &[], "Raiz");
        let first = save(&repo, b"a\nx\na\nz\n", &[root], "Primer padre");
        let merge = save(&repo, b"a\nx\na\nz\n", &[first, root], "Integracion");
        let p = query(&f, merge);
        assert_eq!(
            attributed(&p),
            vec![
                Some(root.to_string()),
                Some(first.to_string()),
                Some(root.to_string()),
                Some(root.to_string())
            ]
        );
        let lines = (0..140).map(|n| format!("{n}\n")).collect::<String>();
        let wide = save(&repo, lines.as_bytes(), &[merge], "Muchas");
        let p = read_blame(
            &f.root,
            &wide.to_string(),
            b"source.p",
            130,
            999,
            limits(),
            || false,
        )
        .unwrap();
        assert_eq!((p.first, p.rows.len(), p.total_lines), (130, 10, 140));
        assert_eq!(&p.source[p.rows[0].range.clone()], b"130\n");
        let p = read_blame(
            &f.root,
            &wide.to_string(),
            b"source.p",
            usize::MAX,
            128,
            limits(),
            || false,
        )
        .unwrap();
        assert_eq!((p.first, p.rows.len()), (140, 0));
        let empty = save(&repo, b"", &[wide], "Vacio");
        let p = query(&f, empty);
        assert!(p.source.is_empty() && p.rows.is_empty());
        assert_eq!(p.total_lines, 0);
        assert!(matches!(
            read_blame(
                &f.root,
                &empty.to_string(),
                b"absent",
                0,
                128,
                limits(),
                || false
            ),
            Err(Error::Missing)
        ));
    }
    #[test]
    fn blame_shallow_and_walk_limits_never_fabricate_an_origin() {
        let f = Fixture::new();
        let repo = gix::init(&f.root).unwrap();
        let root = save(&repo, b"stable\nold\n", &[], "Origen");
        let child = save(&repo, b"stable\nnew\n", &[root], "Cambio");
        fs::write(f.root.join(".git/shallow"), format!("{root}\n")).unwrap();
        let p = query(&f, child);
        assert_eq!(p.boundary, BlameBoundary::Shallow);
        assert_eq!(attributed(&p), vec![None, Some(child.to_string())]);
        fs::remove_file(f.root.join(".git/shallow")).unwrap();
        let mut previous = child;
        for _ in 0..128 {
            previous = save(&repo, b"stable\nnew\n", &[previous], "Sin cambio");
        }
        let p = query(&f, previous);
        assert_eq!(p.boundary, BlameBoundary::WalkLimit);
        assert!(p.rows.iter().all(|r| r.commit.is_none()));
        assert_eq!(p.scanned, 128);
    }
    #[test]
    fn blame_comparison_limit_is_partial_and_errors_remain_errors() {
        let f = Fixture::new();
        let repo = gix::init(&f.root).unwrap();
        let before = format!(
            "stable\n{}",
            (0..500).map(|n| format!("old{n}\n")).collect::<String>()
        );
        let after = format!(
            "stable\n{}",
            (0..500).map(|n| format!("new{n}\n")).collect::<String>()
        );
        let root = save(&repo, before.as_bytes(), &[], "Origen");
        let child = save(&repo, after.as_bytes(), &[root], "Cambio");
        let mut small = limits();
        small.max_bytes = 65536;
        let p = read_blame(
            &f.root,
            &child.to_string(),
            b"source.p",
            0,
            1,
            small,
            || false,
        )
        .unwrap();
        assert_eq!(p.boundary, BlameBoundary::ComparisonLimit);
        assert!(p.rows[0].commit.is_none());
        assert!(matches!(
            read_blame(
                &f.root,
                &child.to_string(),
                b"../escape",
                0,
                1,
                limits(),
                || false
            ),
            Err(Error::Unsupported)
        ));
        assert!(matches!(
            read_blame(
                &f.root,
                &child.to_string(),
                b"source.p",
                0,
                0,
                limits(),
                || false
            ),
            Err(Error::Limited)
        ));
        assert!(matches!(
            read_blame(
                &f.root,
                &child.to_string(),
                b"source.p",
                0,
                1,
                limits(),
                || true
            ),
            Err(Error::Cancelled)
        ));
        let name = root.to_string();
        let object = f
            .root
            .join(".git/objects")
            .join(&name[..2])
            .join(&name[2..]);
        drop(repo);
        writable(&object);
        fs::remove_file(object).unwrap();
        assert!(matches!(
            read_blame(
                &f.root,
                &child.to_string(),
                b"source.p",
                0,
                1,
                limits(),
                || false
            ),
            Err(Error::Missing)
        ));
    }
    #[test]
    fn blame_cumulative_blob_read_limit_keeps_unresolved_lines_without_an_author() {
        let f = Fixture::new();
        let repo = gix::init(&f.root).unwrap();
        let mut parent = None;
        let tail = "x".repeat(1024 * 1024);
        for n in 0..18 {
            let text = format!("stable\n{n}{tail}\n");
            parent = Some(save(
                &repo,
                text.as_bytes(),
                &parent.into_iter().collect::<Vec<_>>(),
                "Otra linea",
            ));
        }
        let p = read_blame(
            &f.root,
            &parent.unwrap().to_string(),
            b"source.p",
            0,
            1,
            limits(),
            || false,
        )
        .unwrap();
        assert_eq!(p.boundary, BlameBoundary::ReadLimit);
        assert!(p.rows[0].commit.is_none());
        assert!(p.commits.is_empty());
        assert_eq!(&p.source[p.rows[0].range.clone()], b"stable\n");
    }
}
