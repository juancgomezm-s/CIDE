//! Bounded literal search of pinned first-parent commit metadata. No editor,
//! worktree/blob reads, external executable or persistent index participates.
use super::{Context, Error, Identity, Limits, controlled, engine, open};
use crate::{
    path_history::{charge_commit, shallow},
    revision::{Budget, commit, verified},
};
use gix::bstr::ByteSlice;
use std::path::{Path, PathBuf};

const QUERY: usize = 256;
const MATCHES: usize = 24;
const WALKED: usize = 128;
const COMMIT_BYTES: usize = 65536;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchField {
    All,
    Message,
    Author,
    Commit,
}
impl SearchField {
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "Todo",
            Self::Message => "Mensaje",
            Self::Author => "Autor",
            Self::Commit => "Identificador",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitSearchCursor {
    root: PathBuf,
    git_dir: PathBuf,
    start: String,
    query: String,
    field: SearchField,
    next: String,
}
impl CommitSearchCursor {
    pub fn memory_bytes(&self) -> usize {
        self.root.capacity()
            + self.git_dir.capacity()
            + self.start.capacity()
            + self.query.capacity()
            + self.next.capacity()
    }
}
#[derive(Debug)]
pub struct CommitSearchEntry {
    pub commit: String,
    pub parent: Option<String>,
    pub summary: Vec<u8>,
    pub author: Vec<u8>,
    pub matched: SearchField,
}
impl CommitSearchEntry {
    pub fn memory_bytes(&self) -> usize {
        self.commit.capacity()
            + self.parent.as_ref().map_or(0, String::capacity)
            + self.summary.capacity()
            + self.author.capacity()
    }
}
pub struct CommitSearchPage {
    pub repository: Identity,
    pub start: String,
    pub query: String,
    pub field: SearchField,
    pub entries: Vec<CommitSearchEntry>,
    pub next: Option<CommitSearchCursor>,
    pub scanned: usize,
    pub shallow: bool,
    pub metadata_limited: bool,
}
impl std::fmt::Debug for CommitSearchPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommitSearchPage")
            .field("root", &self.repository.root)
            .field("start", &self.start)
            .field("query", &self.query)
            .field("field", &self.field)
            .field("entries", &self.entries)
            .field("next", &self.next)
            .field("scanned", &self.scanned)
            .field("shallow", &self.shallow)
            .field("metadata_limited", &self.metadata_limited)
            .finish()
    }
}
impl CommitSearchPage {
    pub fn memory_bytes(&self) -> usize {
        self.repository.root.capacity()
            + self.repository.git_dir.capacity()
            + self.start.capacity()
            + self.query.capacity()
            + self.entries.capacity() * std::mem::size_of::<CommitSearchEntry>()
            + self
                .entries
                .iter()
                .map(CommitSearchEntry::memory_bytes)
                .sum::<usize>()
            + self
                .next
                .as_ref()
                .map_or(0, CommitSearchCursor::memory_bytes)
    }
}

/// Unicode scalar lowercase, not normalization, accent removal or full folding.
/// KMP operates on valid UTF-8 runs and resets on invalid bytes; metadata remains
/// raw, and a replacement character cannot create a match across corrupt bytes.
struct Literal {
    chars: Vec<char>,
    prefix: Vec<usize>,
}
impl Literal {
    fn new(query: &str) -> Self {
        let chars = query
            .chars()
            .flat_map(char::to_lowercase)
            .collect::<Vec<_>>();
        let mut prefix = vec![0; chars.len()];
        let mut j = 0;
        for i in 1..chars.len() {
            while j > 0 && chars[i] != chars[j] {
                j = prefix[j - 1];
            }
            if chars[i] == chars[j] {
                j += 1;
            }
            prefix[i] = j;
        }
        Self { chars, prefix }
    }
    fn contains(&self, mut bytes: &[u8], ctx: &Context<'_>) -> Result<bool, Error> {
        while !bytes.is_empty() {
            ctx.check()?;
            let (valid, invalid) = match std::str::from_utf8(bytes) {
                Ok(text) => (text, 0),
                Err(error) => {
                    let n = error.valid_up_to();
                    (
                        std::str::from_utf8(&bytes[..n]).map_err(engine)?,
                        error.error_len().unwrap_or(bytes.len() - n),
                    )
                }
            };
            let mut j = 0;
            for (i, c) in valid.chars().flat_map(char::to_lowercase).enumerate() {
                if i % 128 == 0 {
                    ctx.check()?;
                }
                while j > 0 && c != self.chars[j] {
                    j = self.prefix[j - 1];
                }
                if c == self.chars[j] {
                    j += 1;
                }
                if j == self.chars.len() {
                    return Ok(true);
                }
            }
            if invalid == 0 {
                break;
            }
            bytes = &bytes[valid.len() + invalid..];
        }
        Ok(false)
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

/// Search full messages, author name/email, or an OID prefix without resolving
/// HEAD. Pages can be empty with a continuation after128 nonmatching commits.
/// A shallow boundary includes the boundary commit's searchable metadata, then
/// marks the unavailable ancestry explicitly. Missing advertised objects remain
/// errors everywhere else. Commit/read quotas are logical and cooperative.
pub fn search_commits(
    root: &Path,
    pinned_start: &str,
    query: &str,
    field: SearchField,
    continuation: Option<&CommitSearchCursor>,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<CommitSearchPage, Error> {
    limits.admit()?;
    if query.is_empty() || query.trim().is_empty() {
        return Err(Error::Unsupported);
    }
    if query.len() > QUERY {
        return Err(Error::Limited);
    }
    if field == SearchField::Commit
        && (query.len() > 64 || !query.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err(Error::Unsupported);
    }
    let start_id = oid(pinned_start)?;
    let start = start_id.to_string();
    let maximum = MATCHES.min(limits.max_entries);
    if maximum == 0 {
        return Err(Error::Limited);
    }
    controlled(limits.timeout, cancelled, |ctx| {
        let (repo, repository) = open(root, ctx)?;
        if start_id.kind() != repo.object_hash() {
            return Err(Error::Unsupported);
        }
        let next = match continuation {
            Some(c)
                if c.root == repository.root
                    && c.git_dir == repository.git_dir
                    && c.start == start
                    && c.query == query
                    && c.field == field =>
            {
                oid(&c.next)?
            }
            Some(_) => return Err(Error::Stale),
            None => start_id,
        };
        let mut budget = Budget::new(limits);
        let needle = Literal::new(query);
        budget.charge(
            query.len() * 2
                + needle.chars.capacity() * std::mem::size_of::<char>()
                + needle.prefix.capacity() * std::mem::size_of::<usize>()
                + 256,
        )?;
        let boundaries = shallow(&repo, limits, &mut budget, ctx)?;
        let commit_limits = Limits {
            max_bytes: limits.max_bytes.min(COMMIT_BYTES),
            ..limits
        };
        let mut entries = Vec::with_capacity(maximum);
        let mut pending = Some(next);
        let mut scanned = 0;
        let mut reached_shallow = false;
        let mut metadata_limited = false;
        while let Some(id) = pending.take() {
            ctx.check()?;
            charge_commit(&repo, id, &mut budget, ctx)?;
            let current = commit(&repo, id, commit_limits, &mut budget, ctx)?;
            scanned += 1;
            charge_commit(&repo, id, &mut budget, ctx)?;
            let value = verified(&repo, id, COMMIT_BYTES.min(limits.max_bytes), ctx)?
                .try_into_commit()
                .map_err(engine)?;
            let author = value.author().map_err(engine)?;
            let message = value.message_raw().map_err(engine)?;
            let name = id.to_string();
            let sha_match = matches!(field, SearchField::All | SearchField::Commit)
                && query.len() <= name.len()
                && name[..query.len()].eq_ignore_ascii_case(query);
            let matched = if sha_match {
                Some(SearchField::Commit)
            } else if matches!(field, SearchField::All | SearchField::Message)
                && needle.contains(message.as_bytes(), ctx)?
            {
                Some(SearchField::Message)
            } else if matches!(field, SearchField::All | SearchField::Author)
                && (needle.contains(author.name.as_bytes(), ctx)?
                    || needle.contains(author.email.as_bytes(), ctx)?)
            {
                Some(SearchField::Author)
            } else {
                None
            };
            let parent = current.parents.first().copied();
            if let Some(matched) = matched {
                let summary = message
                    .as_bytes()
                    .split(|b| matches!(b, b'\r' | b'\n'))
                    .next()
                    .unwrap_or_default();
                metadata_limited |= summary.len() > 256 || author.name.len() > 96;
                let item = CommitSearchEntry {
                    commit: name,
                    parent: parent.map(|id| id.to_string()),
                    summary: summary[..summary.len().min(256)].to_vec(),
                    author: author.name[..author.name.len().min(96)].to_vec(),
                    matched,
                };
                budget
                    .charge(std::mem::size_of::<CommitSearchEntry>() * 2 + item.memory_bytes())?;
                entries.push(item);
            }
            if boundaries.contains(&id) {
                reached_shallow = true;
                break;
            }
            pending = parent;
            if entries.len() >= maximum || scanned >= WALKED {
                break;
            }
        }
        let next = pending.map(|id| CommitSearchCursor {
            root: repository.root.clone(),
            git_dir: repository.git_dir.clone(),
            start: start.clone(),
            query: query.into(),
            field,
            next: id.to_string(),
        });
        let page = CommitSearchPage {
            repository,
            start: start.clone(),
            query: query.into(),
            field,
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
                "commit-search-{}-{}",
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
    fn writable(path: &Path) {
        #[cfg(windows)]
        #[allow(clippy::permissions_set_readonly_false)]
        {
            let mut permissions = fs::metadata(path).unwrap().permissions();
            permissions.set_readonly(false);
            fs::set_permissions(path, permissions).unwrap();
        }
        #[cfg(not(windows))]
        let _ = path;
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            assert_eq!(self.root.parent(), Some(self.base.as_path()));
            assert_eq!(self.root.canonicalize().unwrap(), self.root);
            assert!(!self.root.is_symlink());
            fn clean(path: &Path) {
                for entry in fs::read_dir(path).unwrap() {
                    let path = entry.unwrap().path();
                    let meta = fs::symlink_metadata(&path).unwrap();
                    assert!(!meta.file_type().is_symlink());
                    if meta.is_dir() {
                        clean(&path);
                    } else {
                        writable(&path);
                    }
                }
            }
            clean(&self.root);
            fs::remove_dir_all(&self.root).unwrap();
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
    fn empty(repo: &gix::Repository) -> gix::ObjectId {
        repo.write_object(gix::objs::Tree {
            entries: Vec::new(),
        })
        .unwrap()
        .detach()
    }
    fn save(
        repo: &gix::Repository,
        parent: Option<gix::ObjectId>,
        message: &str,
        author: &[u8],
        email: &[u8],
    ) -> gix::ObjectId {
        let signature = gix::actor::SignatureRef {
            name: author.as_bstr(),
            email: email.as_bstr(),
            time: "1 +0000",
        };
        repo.commit_as(signature, signature, "HEAD", message, empty(repo), parent)
            .unwrap()
            .detach()
    }
    fn raw(
        repo: &gix::Repository,
        parent: Option<gix::ObjectId>,
        message: &[u8],
        author: &[u8],
    ) -> gix::ObjectId {
        let signature = gix::actor::SignatureRef {
            name: author.as_bstr(),
            email: b"raw@invalid".as_bstr(),
            time: "1 +0000",
        };
        repo.write_object(gix::objs::Commit {
            tree: empty(repo),
            parents: parent.into_iter().collect(),
            author: signature.into(),
            committer: signature.into(),
            message: message.to_vec().into(),
            encoding: None,
            extra_headers: Default::default(),
        })
        .unwrap()
        .detach()
    }
    fn query(f: &Fixture, id: gix::ObjectId, text: &str, field: SearchField) -> CommitSearchPage {
        search_commits(
            &f.root,
            &id.to_string(),
            text,
            field,
            None,
            limits(),
            || false,
        )
        .unwrap()
    }
    fn image(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
        fn walk(root: &Path, p: &Path, map: &mut BTreeMap<PathBuf, Vec<u8>>) {
            for e in fs::read_dir(p).unwrap() {
                let p = e.unwrap().path();
                let m = fs::symlink_metadata(&p).unwrap();
                assert!(!m.file_type().is_symlink());
                if m.is_dir() {
                    walk(root, &p, map)
                } else {
                    map.insert(
                        p.strip_prefix(root).unwrap().to_owned(),
                        fs::read(p).unwrap(),
                    );
                }
            }
        }
        let mut map = BTreeMap::new();
        walk(root, root, &mut map);
        map
    }
    #[test]
    fn commit_search_message_body_author_email_sha_and_unicode_lowercase_are_read_only() {
        let f = Fixture::new();
        let repo = gix::init(&f.root).unwrap();
        let root = save(
            &repo,
            None,
            "Inicial\nDetalle ÚNICO",
            "ÑANDÚ".as_bytes(),
            b"Especial@Dominio.test",
        );
        let child = save(
            &repo,
            Some(root),
            "Corrección de teléfono",
            b"Otro",
            b"otro@invalid",
        );
        drop(repo);
        let before = image(&f.root);
        let p = query(&f, child, "detalle único", SearchField::Message);
        assert_eq!(p.entries.len(), 1);
        assert_eq!(p.entries[0].commit, root.to_string());
        assert_eq!(p.entries[0].summary, b"Inicial");
        assert_eq!(p.entries[0].matched, SearchField::Message);
        assert_eq!(
            query(&f, child, "ñandú", SearchField::Author).entries[0].author,
            "ÑANDÚ".as_bytes()
        );
        assert_eq!(
            query(&f, child, "especial@dominio", SearchField::Author).entries[0].commit,
            root.to_string()
        );
        assert_eq!(
            query(
                &f,
                child,
                &child.to_string()[..10].to_ascii_uppercase(),
                SearchField::Commit
            )
            .entries[0]
                .commit,
            child.to_string()
        );
        assert_eq!(
            query(&f, child, "TELÉFONO", SearchField::All).entries[0].matched,
            SearchField::Message
        );
        assert!(
            query(&f, child, "inexistente", SearchField::All)
                .entries
                .is_empty()
        );
        assert_eq!(image(&f.root), before);
    }
    #[test]
    fn commit_search_invalid_utf8_does_not_join_runs_or_lose_preview_bytes() {
        let f = Fixture::new();
        let repo = gix::init(&f.root).unwrap();
        let id = raw(&repo, None, b"a\xffb\nfin", b"X\xffY");
        drop(repo);
        assert!(query(&f, id, "ab", SearchField::Message).entries.is_empty());
        assert!(query(&f, id, "xy", SearchField::Author).entries.is_empty());
        let p = query(&f, id, "fin", SearchField::All);
        assert_eq!(p.entries[0].summary, b"a\xffb");
        assert_eq!(p.entries[0].author, b"X\xffY");
        assert!(query(&f, id, "a�b", SearchField::All).entries.is_empty());
    }
    #[test]
    fn commit_search_paging_cursor_binds_query_field_start_and_repository_despite_later_head() {
        let f = Fixture::new();
        let repo = gix::init(&f.root).unwrap();
        let mut id = None;
        for n in 0..27 {
            id = Some(save(
                &repo,
                id,
                &format!("Coincide {n}"),
                b"Autor",
                b"fixture@invalid",
            ));
        }
        let pinned = id.unwrap();
        let p = query(&f, pinned, "coincide", SearchField::Message);
        assert_eq!(p.entries.len(), 24);
        let cursor = p.next.unwrap();
        assert!(cursor.memory_bytes() > 0);
        save(
            &repo,
            Some(pinned),
            "Coincide posterior",
            b"Autor",
            b"fixture@invalid",
        );
        drop(repo);
        let p = search_commits(
            &f.root,
            &pinned.to_string(),
            "coincide",
            SearchField::Message,
            Some(&cursor),
            limits(),
            || false,
        )
        .unwrap();
        assert_eq!(p.entries.len(), 3);
        assert!(p.next.is_none());
        for (term, field) in [
            ("otro", SearchField::Message),
            ("coincide", SearchField::All),
        ] {
            assert!(matches!(
                search_commits(
                    &f.root,
                    &pinned.to_string(),
                    term,
                    field,
                    Some(&cursor),
                    limits(),
                    || false
                ),
                Err(Error::Stale)
            ));
        }
        assert!(matches!(
            search_commits(
                &f.root,
                &p.entries[0].commit,
                "coincide",
                SearchField::Message,
                Some(&cursor),
                limits(),
                || false
            ),
            Err(Error::Stale)
        ));
        let other = Fixture::new();
        drop(gix::init(&other.root).unwrap());
        assert!(matches!(
            search_commits(
                &other.root,
                &pinned.to_string(),
                "coincide",
                SearchField::Message,
                Some(&cursor),
                limits(),
                || false
            ),
            Err(Error::Stale)
        ));
    }
    #[test]
    fn commit_search_walk_limit_empty_page_continues_and_shallow_boundary_remains_explicit() {
        let f = Fixture::new();
        let repo = gix::init(&f.root).unwrap();
        let root = save(&repo, None, "objetivo", b"Autor", b"fixture@invalid");
        let mut id = root;
        for n in 0..130 {
            id = save(
                &repo,
                Some(id),
                &format!("Otro {n}"),
                b"Autor",
                b"fixture@invalid",
            );
        }
        drop(repo);
        let first = query(&f, id, "objetivo", SearchField::Message);
        assert_eq!(first.scanned, 128);
        assert!(first.entries.is_empty());
        let cursor = first.next.unwrap();
        let last = search_commits(
            &f.root,
            &id.to_string(),
            "objetivo",
            SearchField::Message,
            Some(&cursor),
            limits(),
            || false,
        )
        .unwrap();
        assert_eq!(last.entries[0].commit, root.to_string());
        assert!(last.next.is_none());
        fs::write(f.root.join(".git/shallow"), format!("{id}\n")).unwrap();
        let p = query(&f, id, "Otro", SearchField::Message);
        assert!(p.shallow && p.next.is_none());
        assert_eq!(p.entries.len(), 1);
    }
    #[test]
    fn commit_search_rejects_invalid_inputs_corrupt_objects_cancellation_and_quotas() {
        let f = Fixture::new();
        let repo = gix::init(&f.root).unwrap();
        let id = save(
            &repo,
            None,
            &"Largo".repeat(80),
            &[b'A'; 100],
            b"fixture@invalid",
        );
        drop(repo);
        let p = query(&f, id, "Largo", SearchField::All);
        assert!(p.metadata_limited);
        assert_eq!(p.entries[0].summary.len(), 256);
        assert_eq!(p.entries[0].author.len(), 96);
        for term in ["", "   "] {
            assert!(matches!(
                search_commits(
                    &f.root,
                    &id.to_string(),
                    term,
                    SearchField::All,
                    None,
                    limits(),
                    || false
                ),
                Err(Error::Unsupported)
            ));
        }
        assert!(matches!(
            search_commits(
                &f.root,
                &id.to_string(),
                &"q".repeat(257),
                SearchField::All,
                None,
                limits(),
                || false
            ),
            Err(Error::Limited)
        ));
        assert!(matches!(
            search_commits(
                &f.root,
                &id.to_string(),
                "no-hex",
                SearchField::Commit,
                None,
                limits(),
                || false
            ),
            Err(Error::Unsupported)
        ));
        assert!(matches!(
            search_commits(
                &f.root,
                &id.to_string(),
                "Largo",
                SearchField::All,
                None,
                limits(),
                || true
            ),
            Err(Error::Cancelled)
        ));
        let mut small = limits();
        small.max_bytes = 64;
        assert!(matches!(
            search_commits(
                &f.root,
                &id.to_string(),
                "Largo",
                SearchField::All,
                None,
                small,
                || false
            ),
            Err(Error::Limited)
        ));
        let name = id.to_string();
        let object = f
            .root
            .join(".git/objects")
            .join(&name[..2])
            .join(&name[2..]);
        writable(&object);
        fs::write(&object, b"corrupt").unwrap();
        assert!(
            search_commits(
                &f.root,
                &name,
                "Largo",
                SearchField::All,
                None,
                limits(),
                || false
            )
            .is_err()
        );
    }
}
