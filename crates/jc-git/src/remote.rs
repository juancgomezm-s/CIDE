//! Safe, bounded remote metadata and local-path synchronization.
//!
//! Network transports are deliberately not enabled in this first layer. In
//! particular, this module never shells out to a Git executable or credential
//! helper. Configured file remotes can be synchronized through reviewed plans.
use crate::{
    Context, Error, Identity, Limits, controlled, engine, graph, index_write, open,
    revision::Budget,
};
use gix::{
    bstr::ByteSlice,
    refs::{
        FullName, Target,
        transaction::{PreviousValue, RefEdit},
    },
    remote::Direction,
};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    io,
    path::{Path, PathBuf},
};

const MAX_OBJECTS: usize = 65_536;
const MAX_OBJECT_SIZE: usize = 16 * 1024 * 1024;
const MAX_REF_NAME: usize = 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteInfo {
    pub name: String,
    /// Sanitized transport label. URL credentials are never returned.
    pub transport: String,
    /// Host only; no username, password, query, or path is exposed.
    pub host: Option<String>,
    pub fetch_refs: usize,
    pub push_refs: usize,
    pub active_upstream: Option<String>,
    pub active_remote_branch: Option<String>,
}

#[derive(Clone)]
pub struct RemoteSnapshot {
    pub repository: Identity,
    pub active_branch: Option<String>,
    pub remotes: Vec<RemoteInfo>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefUpdatePreview {
    pub source: String,
    pub destination: String,
    pub old: Option<String>,
    pub new: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferPreview {
    pub remote: String,
    pub branch: String,
    /// Absolute repository directory that the accepted operation will update/create.
    pub destination_path: Option<String>,
    pub updates: Vec<RefUpdatePreview>,
    pub object_count: usize,
    pub object_bytes: usize,
    pub file_transport: bool,
}

pub struct FetchPlan {
    local: Identity,
    remote: Identity,
    limits: Limits,
    name: String,
    preview: TransferPreview,
    objects: Vec<(gix::ObjectId, gix::objs::Kind, Vec<u8>)>,
}

pub struct PushPlan {
    local: Identity,
    remote: Identity,
    limits: Limits,
    name: String,
    preview: TransferPreview,
    objects: Vec<(gix::ObjectId, gix::objs::Kind, Vec<u8>)>,
}

/// A local bare clone staged next to its destination and published by one rename.
pub struct LocalClonePlan {
    source: Identity,
    destination: PathBuf,
    staging: PathBuf,
    limits: Limits,
    remote_name: String,
    head: Option<String>,
    refs: BTreeMap<String, gix::ObjectId>,
    objects: Vec<(gix::ObjectId, gix::objs::Kind, Vec<u8>)>,
    preview: TransferPreview,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferResult {
    pub remote: String,
    pub updated: Vec<String>,
    pub objects_written: usize,
    pub bytes_written: usize,
}

pub fn clone_preview(plan: &LocalClonePlan) -> &TransferPreview {
    &plan.preview
}

fn stage_candidate(parent: &Path, target_name: &str) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let serial = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let name = format!(
        ".cide-clone-{}-{serial}-{}",
        std::process::id(),
        target_name
    );
    parent.join(name)
}

/// Prepare a bounded clone from a local bare repository. This is explicitly a
/// bare clone; network clone and worktree materialization are separate routes.
pub fn prepare_local_bare_clone(
    source_root: &Path,
    destination: &Path,
    remote_name: &str,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<LocalClonePlan, Error> {
    limits.admit()?;
    if remote_name.is_empty()
        || remote_name.len() > 64
        || !remote_name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return Err(Error::Unsupported);
    }
    if !cfg!(windows) {
        return Err(Error::Unsupported);
    }
    controlled(limits.timeout, cancelled, |ctx| {
        let (source_repo, source) = open_bare_remote(source_root, ctx)?;
        if source_repo.object_hash() != gix::hash::Kind::Sha1 {
            return Err(Error::Unsupported);
        }
        if !destination.is_absolute() {
            return Err(Error::InvalidRoot);
        }
        let parent_in = destination.parent().ok_or(Error::InvalidRoot)?;
        let parent = parent_in.canonicalize()?;
        let parent_meta = std::fs::symlink_metadata(&parent)?;
        if !parent_meta.is_dir() || parent_meta.file_type().is_symlink() {
            return Err(Error::Unsupported);
        }
        let leaf = destination.file_name().ok_or(Error::InvalidRoot)?;
        if leaf.to_string_lossy().is_empty() || leaf == "." || leaf == ".." {
            return Err(Error::InvalidRoot);
        }
        let destination = parent.join(leaf);
        if index_write::exists(&destination)? {
            return Err(Error::Stale);
        }
        let (_, source_refs) =
            graph::references(&source_repo, limits, &mut Budget::new(limits), ctx)?;
        let head = match source_repo.head_name().map_err(engine)? {
            Some(name) if name.as_bstr().as_bytes().starts_with(b"refs/heads/") => Some(
                name.as_bstr()
                    .to_str()
                    .map_err(|_| Error::Unsupported)?
                    .to_owned(),
            ),
            _ => None,
        };
        let mut selected = BTreeMap::new();
        let mut tips = Vec::new();
        for reference in source_refs.iter().filter(|entry| {
            entry.name.starts_with(b"refs/heads/") || entry.name.starts_with(b"refs/tags/")
        }) {
            if let graph::RefTarget::Direct(id) = &reference.target {
                let name = text(&reference.name)?;
                let oid = gix::ObjectId::from_hex(id.as_bytes()).map_err(engine)?;
                selected.insert(name, oid);
                tips.push(oid);
                if selected.len() > limits.max_entries {
                    return Err(Error::Limited);
                }
            }
        }
        let objects = object_closure(&source_repo, tips, limits, ctx)?;
        let object_bytes = objects.iter().map(|o| o.2.len()).sum();
        let staging = stage_candidate(&parent, &leaf.to_string_lossy());
        let object_count = objects.len();
        Ok(LocalClonePlan {
            source,
            destination: destination.clone(),
            staging,
            limits,
            remote_name: remote_name.to_owned(),
            head: head.clone(),
            refs: selected.clone(),
            objects,
            preview: TransferPreview {
                remote: remote_name.to_owned(),
                branch: head.as_deref().unwrap_or("sin rama activa").to_owned(),
                destination_path: Some(destination.to_string_lossy().into_owned()),
                updates: selected
                    .iter()
                    .map(|(name, id)| RefUpdatePreview {
                        source: name.clone(),
                        destination: name.clone(),
                        old: None,
                        new: id.to_string(),
                    })
                    .collect(),
                object_count,
                object_bytes,
                file_transport: true,
            },
        })
    })
}

pub fn apply_local_bare_clone(
    plan: LocalClonePlan,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<TransferResult, Error> {
    controlled(plan.limits.timeout, cancelled, |ctx| {
        let (source, identity) = open_bare_remote(&plan.source.root, ctx)?;
        if identity.root != plan.source.root || identity.git_dir != plan.source.git_dir {
            return Err(Error::Stale);
        }
        let (_, current_refs) =
            graph::references(&source, plan.limits, &mut Budget::new(plan.limits), ctx)?;
        let mut current_selected = BTreeMap::new();
        for entry in current_refs.iter().filter(|entry| {
            entry.name.starts_with(b"refs/heads/") || entry.name.starts_with(b"refs/tags/")
        }) {
            if let graph::RefTarget::Direct(id) = &entry.target {
                current_selected.insert(
                    text(&entry.name)?,
                    gix::ObjectId::from_hex(id.as_bytes()).map_err(engine)?,
                );
            } else {
                return Err(Error::Stale);
            }
        }
        let current_head = source
            .head_name()
            .map_err(engine)?
            .map(|head| {
                head.as_bstr()
                    .to_str()
                    .map(str::to_owned)
                    .map_err(|_| Error::Unsupported)
            })
            .transpose()?;
        if current_selected != plan.refs || current_head != plan.head {
            return Err(Error::Stale);
        }
        if index_write::exists(&plan.destination)? {
            return Err(Error::Stale);
        }
        std::fs::create_dir(&plan.staging)?;
        let created = plan.staging.clone();
        let result = (|| {
            let target = gix::init_bare(&created).map_err(engine)?;
            for (id, kind, bytes) in &plan.objects {
                ctx.check()?;
                let written = target
                    .write_object(RawObject {
                        kind: *kind,
                        bytes: bytes.clone(),
                    })
                    .map_err(engine)?;
                if written.detach() != *id {
                    return Err(Error::Stale);
                }
            }
            let mut edits = Vec::new();
            for (name, id) in &plan.refs {
                let name = safe_ref(name)?;
                edits.push(RefEdit::update(
                    name,
                    Target::Object(*id),
                    PreviousValue::MustNotExist,
                    String::new(),
                ));
            }
            if !edits.is_empty() {
                target.edit_references_as(edits, None).map_err(engine)?;
            }
            if let Some(branch) = &plan.head {
                let source_head = safe_ref(branch)?;
                let head = safe_ref("HEAD")?;
                let current_head = target
                    .find_reference("HEAD")
                    .map_err(engine)?
                    .target()
                    .try_name()
                    .ok_or(Error::Stale)?
                    .to_owned();
                target
                    .edit_references_as(
                        [RefEdit::update(
                            head,
                            Target::Symbolic(source_head),
                            PreviousValue::MustExistAndMatch(Target::Symbolic(current_head)),
                            String::new(),
                        )],
                        None,
                    )
                    .map_err(engine)?;
            }
            let config_path = created.join("config");
            let metadata = gix::config::file::Metadata::default();
            let options = gix::config::file::init::Options::default();
            let mut config = gix::config::File::from_bytes_no_includes(
                &std::fs::read(&config_path)?,
                metadata,
                options,
            )
            .map_err(|_| Error::Unsupported)?;
            let mut remote = config
                .new_section("remote", &plan.remote_name)
                .map_err(engine)?;
            let location = plan.source.root.to_string_lossy();
            if location.chars().any(char::is_control) {
                return Err(Error::Unsupported);
            }
            remote.set("url", location.as_ref()).map_err(engine)?;
            let fetch = format!("+refs/heads/*:refs/remotes/{}/*", plan.remote_name);
            remote.set("fetch", fetch.as_str()).map_err(engine)?;
            let mut bytes = Vec::new();
            config.write_to(&mut bytes)?;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(config_path)?;
            use io::Write as _;
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            ctx.check()?;
            if index_write::exists(&plan.destination)? {
                return Err(Error::Stale);
            }
            drop(target);
            publish_directory_create_only(&created, &plan.destination)?;
            Ok(())
        })();
        if let Err(error) = result {
            // Leave the private staging directory intact. It can contain valid
            // objects and is never recursively deleted by an error path.
            return Err(Error::Engine(format!(
                "{error}. Se conservó la preparación incompleta en {}",
                created.display()
            )));
        }
        Ok(TransferResult {
            remote: plan.remote_name,
            updated: plan.refs.keys().cloned().collect(),
            objects_written: plan.objects.len(),
            bytes_written: plan.preview.object_bytes,
        })
    })
}

#[cfg(windows)]
fn publish_directory_create_only(from: &Path, to: &Path) -> Result<(), Error> {
    use std::os::windows::ffi::OsStrExt;
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }
    let from: Vec<u16> = from
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let to: Vec<u16> = to
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // MOVEFILE_WRITE_THROUGH without REPLACE_EXISTING: atomic create-only publish.
    let result = unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0x8) };
    if result == 0 {
        Err(io::Error::last_os_error().into())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn publish_directory_create_only(_from: &Path, _to: &Path) -> Result<(), Error> {
    Err(Error::Unsupported)
}

fn text(bytes: &[u8]) -> Result<String, Error> {
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|_| Error::Unsupported)
}

fn safe_ref(name: &str) -> Result<FullName, Error> {
    if name.len() > MAX_REF_NAME || name.as_bytes().contains(&0) {
        return Err(Error::Limited);
    }
    FullName::try_from(gix::bstr::BString::from(name)).map_err(|_| Error::Unsupported)
}

fn sanitize_url(url: &gix::Url) -> (String, Option<String>) {
    let transport = url.scheme.as_str().to_ascii_uppercase();
    let host = url.host.as_deref().map(|h| h.chars().take(255).collect());
    (transport, host)
}

fn current_head(
    repo: &gix::Repository,
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<graph::Head, Error> {
    graph::references(repo, limits, &mut Budget::new(limits), ctx).map(|(head, _)| head)
}

fn branch_name(head: &graph::Head) -> Result<(String, FullName), Error> {
    let graph::Head::Attached { name, .. } = head else {
        return Err(Error::Engine(
            "La operación requiere una rama local activa".into(),
        ));
    };
    let short = name
        .strip_prefix(b"refs/heads/")
        .ok_or(Error::Unsupported)?;
    let short = text(short)?;
    let full = safe_ref(&format!("refs/heads/{short}"))?;
    Ok((short, full))
}

fn remote_root(repo: &gix::Repository, name: &str, direction: Direction) -> Result<PathBuf, Error> {
    let remote = repo
        .find_remote(name)
        .map_err(|_| Error::Engine("La configuración del remoto no es válida".into()))?;
    let url = remote.url(direction).ok_or(Error::Unsupported)?;
    if url.scheme != gix::url::Scheme::File || url.user.is_some() || url.password.is_some() {
        return Err(Error::Engine(
            "Este transporte aún no está disponible; por ahora se admite una ruta local".into(),
        ));
    }
    let path_text = url
        .path
        .as_bstr()
        .to_str()
        .map_err(|_| Error::Unsupported)?;
    #[cfg(windows)]
    let path_text = path_text
        .strip_prefix('/')
        .filter(|s| s.as_bytes().get(1) == Some(&b':'))
        .unwrap_or(path_text);
    let path = PathBuf::from(path_text);
    if !path.is_absolute() {
        return Err(Error::Unsupported);
    }
    Ok(path)
}

fn open_bare_remote(root: &Path, ctx: &Context<'_>) -> Result<(gix::Repository, Identity), Error> {
    ctx.check()?;
    let root = root.canonicalize()?;
    if !root.is_dir() {
        return Err(Error::InvalidRoot);
    }
    for (name, directory, cap) in [
        ("HEAD", false, 8 * 1024),
        ("config", false, 8 * 1024 * 1024),
        ("objects", true, 0),
        ("refs", true, 0),
    ] {
        let metadata =
            std::fs::symlink_metadata(root.join(name)).map_err(|_| Error::NotRepository)?;
        index_write::ordinary(&root.join(name), directory)?;
        if !directory && metadata.len() > cap {
            return Err(Error::Limited);
        }
    }
    let packed_refs = root.join("packed-refs");
    match std::fs::symlink_metadata(&packed_refs) {
        Ok(metadata) => {
            index_write::ordinary(&packed_refs, false)?;
            if metadata.len() > 1024 * 1024 {
                return Err(Error::Limited);
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    if index_write::exists(&root.join("objects/info/alternates"))? {
        return Err(Error::Unsupported);
    }
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
    if repo.workdir().is_some()
        || repo.git_dir().canonicalize()? != root
        || repo.common_dir().canonicalize()? != root
    {
        return Err(Error::Unsupported);
    }
    for key in ["include", "includeIf"] {
        if repo
            .config_snapshot()
            .plumbing()
            .sections_by_name(key)
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
    Ok((
        repo,
        Identity {
            root: root.clone(),
            git_dir: root,
        },
    ))
}

fn resolve_remote_name(
    repo: &gix::Repository,
    requested: &str,
    direction: Direction,
) -> Result<String, Error> {
    if !requested.is_empty() {
        if requested.len() > 255 || requested.chars().any(char::is_control) {
            return Err(Error::Unsupported);
        }
        return Ok(requested.to_owned());
    }
    {
        let upstream_remote = repo.head_name().ok().flatten().and_then(|name| {
            let short = name.as_bstr().strip_prefix(b"refs/heads/")?;
            let remote = repo.branch_remote_name(short, direction)?;
            std::str::from_utf8(remote.as_bstr().as_bytes())
                .ok()
                .map(str::to_owned)
        });
        if let Some(name) = upstream_remote {
            return Ok(name);
        }
    }
    let name = repo.remote_default_name(direction).ok_or_else(|| {
        Error::Engine("No hay un remoto predeterminado para esta operación".into())
    })?;
    text(name.as_bstr().as_bytes())
}

fn remote_list(
    repo: &gix::Repository,
    head: &graph::Head,
    limits: Limits,
) -> Result<Vec<RemoteInfo>, Error> {
    let mut list = Vec::new();
    let branch = match head {
        graph::Head::Attached { name, .. } => Some(
            FullName::try_from(gix::bstr::BString::from(name.clone()))
                .map_err(|_| Error::Unsupported)?,
        ),
        _ => None,
    };
    for bytes in repo.remote_names() {
        let name = text(bytes.as_bstr().as_bytes())?;
        if name.len() > limits.max_path {
            return Err(Error::Limited);
        }
        let remote = repo
            .find_remote(name.as_str())
            .map_err(|_| Error::Engine("La configuración del remoto no es válida".into()))?;
        let (transport, host) = remote
            .url(Direction::Fetch)
            .map(sanitize_url)
            .unwrap_or_else(|| ("SIN URL".into(), None));
        let mut active_upstream = None;
        let mut active_remote_branch = None;
        if let Some(ref branch) = branch {
            if repo
                .branch_remote_name(branch.shorten(), Direction::Fetch)
                .is_some_and(|remote| remote.as_bstr().as_bytes() == name.as_bytes())
            {
                active_upstream = repo
                    .branch_remote_tracking_ref_name(branch.as_ref(), Direction::Fetch)
                    .and_then(Result::ok)
                    .and_then(|r| r.as_bstr().to_str().ok().map(str::to_owned));
                active_remote_branch = repo
                    .branch_remote_ref_name(branch.as_ref(), Direction::Fetch)
                    .and_then(Result::ok)
                    .and_then(|r| r.as_bstr().to_str().ok().map(str::to_owned));
            }
        }
        list.push(RemoteInfo {
            name,
            transport,
            host,
            fetch_refs: remote.refspecs(Direction::Fetch).len(),
            push_refs: remote.refspecs(Direction::Push).len(),
            active_upstream,
            active_remote_branch,
        });
    }
    Ok(list)
}

pub fn read_remotes(
    root: &Path,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<RemoteSnapshot, Error> {
    limits.admit()?;
    controlled(limits.timeout, cancelled, |ctx| {
        let (repo, repository) = open(root, ctx)?;
        let head = current_head(&repo, limits, ctx)?;
        let active_branch = match &head {
            graph::Head::Attached { name, .. } => {
                text(name.strip_prefix(b"refs/heads/").unwrap_or(name)).ok()
            }
            _ => None,
        };
        Ok(RemoteSnapshot {
            repository,
            active_branch,
            remotes: remote_list(&repo, &head, limits)?,
        })
    })
}

fn fetch_mapping(repo: &gix::Repository, name: &str) -> Result<String, Error> {
    let remote = repo
        .find_remote(name)
        .map_err(|_| Error::Engine("La configuración del remoto no es válida".into()))?;
    let specs = remote.refspecs(Direction::Fetch);
    // Constrain initial implementation to the conventional, unambiguous mapping.
    let expected = format!("+refs/heads/*:refs/remotes/{name}/*");
    let valid = specs
        .iter()
        .any(|s| s.to_ref().to_bstring().as_bstr().as_bytes() == expected.as_bytes());
    if !valid {
        return Err(Error::Engine(
            "El remoto no usa el mapeo estándar de ramas".into(),
        ));
    }
    Ok(expected)
}

fn refs(
    repo: &gix::Repository,
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<BTreeMap<String, gix::ObjectId>, Error> {
    let (_, entries) = graph::references(repo, limits, &mut Budget::new(limits), ctx)?;
    let mut out = BTreeMap::new();
    for e in entries {
        if let graph::RefTarget::Direct(id) = e.target {
            out.insert(
                text(&e.name)?,
                gix::ObjectId::from_hex(id.as_bytes()).map_err(engine)?,
            );
        }
    }
    Ok(out)
}

struct RawObject {
    kind: gix::objs::Kind,
    bytes: Vec<u8>,
}
impl gix::objs::WriteTo for RawObject {
    fn write_to(&self, out: &mut dyn io::Write) -> io::Result<()> {
        out.write_all(&self.bytes)
    }
    fn kind(&self) -> gix::objs::Kind {
        self.kind
    }
    fn size(&self) -> u64 {
        self.bytes.len() as u64
    }
}

fn object_closure(
    source: &gix::Repository,
    tips: impl IntoIterator<Item = gix::ObjectId>,
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<Vec<(gix::ObjectId, gix::objs::Kind, Vec<u8>)>, Error> {
    let mut todo: VecDeque<_> = tips.into_iter().collect();
    let mut seen = BTreeSet::new();
    let mut objects = Vec::new();
    let mut bytes = 0usize;
    while let Some(id) = todo.pop_front() {
        ctx.check()?;
        if !seen.insert(id) {
            continue;
        }
        if seen.len() > MAX_OBJECTS || seen.len() > limits.max_entries.saturating_mul(4) {
            return Err(Error::Limited);
        }
        let obj = source
            .try_find_object(id)
            .map_err(engine)?
            .ok_or(Error::Missing)?;
        let mut obj = obj;
        if obj.data.len() > MAX_OBJECT_SIZE {
            return Err(Error::Limited);
        }
        bytes = bytes.checked_add(obj.data.len()).ok_or(Error::Limited)?;
        if bytes > limits.max_bytes {
            return Err(Error::Limited);
        }
        match gix::objs::ObjectRef::from_bytes(&obj.data, obj.kind, source.object_hash())
            .map_err(engine)?
        {
            gix::objs::ObjectRef::Commit(c) => {
                let tree = gix::ObjectId::from_hex(c.tree).map_err(engine)?;
                todo.push_back(tree);
                for parent in c.parents {
                    todo.push_back(gix::ObjectId::from_hex(parent).map_err(engine)?);
                }
            }
            gix::objs::ObjectRef::Tree(t) => {
                for entry in t.entries {
                    if !entry.mode.is_commit() {
                        todo.push_back(entry.oid.to_owned());
                    }
                }
            }
            gix::objs::ObjectRef::Tag(t) => {
                todo.push_back(gix::ObjectId::from_hex(t.target).map_err(engine)?)
            }
            gix::objs::ObjectRef::Blob(_) => {}
        }
        objects.push((id, obj.kind, std::mem::take(&mut obj.data)));
    }
    Ok(objects)
}

fn check_identity(
    root: &Path,
    expected: &Identity,
    ctx: &Context<'_>,
) -> Result<gix::Repository, Error> {
    let (repo, actual) = open(root, ctx)?;
    if actual.root != expected.root || actual.git_dir != expected.git_dir {
        return Err(Error::Stale);
    }
    Ok(repo)
}

pub fn prepare_configured_fetch(
    root: &Path,
    remote_name: &str,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<FetchPlan, Error> {
    limits.admit()?;
    if remote_name.len() > limits.max_path {
        return Err(Error::Limited);
    }
    controlled(limits.timeout, cancelled, |ctx| {
        let (local_repo, local) = open(root, ctx)?;
        let remote_name = resolve_remote_name(&local_repo, remote_name, Direction::Fetch)?;
        let _mapping = fetch_mapping(&local_repo, &remote_name)?;
        let remote_path = remote_root(&local_repo, &remote_name, Direction::Fetch)?;
        let (remote_repo, remote) = open_bare_remote(&remote_path, ctx)?;
        let remote_refs = refs(&remote_repo, limits, ctx)?;
        let local_refs = refs(&local_repo, limits, ctx)?;
        let mut updates = Vec::new();
        let mut tips = Vec::new();
        for (name, oid) in remote_refs
            .iter()
            .filter(|(name, _)| name.starts_with("refs/heads/"))
        {
            let short = &name["refs/heads/".len()..];
            let destination = format!("refs/remotes/{remote_name}/{short}");
            let old = local_refs.get(&destination).map(ToString::to_string);
            updates.push(RefUpdatePreview {
                source: name.clone(),
                destination,
                old,
                new: oid.to_string(),
            });
            tips.push(*oid);
            if updates.len() > limits.max_entries {
                return Err(Error::Limited);
            }
        }
        let objects = object_closure(&remote_repo, tips, limits, ctx)?;
        let object_bytes = objects.iter().map(|o| o.2.len()).sum();
        Ok(FetchPlan {
            local: local.clone(),
            remote,
            limits,
            name: remote_name.clone(),
            preview: TransferPreview {
                remote: remote_name,
                branch: "ramas remotas".into(),
                destination_path: Some(local.root.to_string_lossy().into_owned()),
                updates,
                object_count: objects.len(),
                object_bytes,
                file_transport: true,
            },
            objects,
        })
    })
}

pub fn fetch_preview(plan: &FetchPlan) -> &TransferPreview {
    &plan.preview
}

pub fn apply_local_fetch(
    plan: FetchPlan,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<TransferResult, Error> {
    controlled(plan.limits.timeout, cancelled, |ctx| {
        let mut local_repo = check_identity(&plan.local.root, &plan.local, ctx)?;
        let (remote_repo, actual_remote) = open_bare_remote(&plan.remote.root, ctx)?;
        if actual_remote.root != plan.remote.root || actual_remote.git_dir != plan.remote.git_dir {
            return Err(Error::Stale);
        }
        let source_refs = refs(&remote_repo, plan.limits, ctx)?;
        if plan.preview.updates.iter().any(|update| {
            source_refs
                .get(&update.source)
                .map(ToString::to_string)
                .as_deref()
                != Some(update.new.as_str())
        }) {
            return Err(Error::Stale);
        }
        for (source, _, _) in &plan.objects {
            if !remote_repo.has_object(source) {
                return Err(Error::Stale);
            }
        }
        for (id, kind, bytes) in &plan.objects {
            ctx.check()?;
            let written = local_repo
                .write_object(RawObject {
                    kind: *kind,
                    bytes: bytes.clone(),
                })
                .map_err(engine)?;
            if written.detach() != *id {
                return Err(Error::Engine(
                    "El objeto transferido no coincide con su identificador".into(),
                ));
            }
        }
        let before = refs(&local_repo, plan.limits, ctx)?;
        if plan.preview.updates.is_empty() {
            return Ok(TransferResult {
                remote: plan.name,
                updated: Vec::new(),
                objects_written: 0,
                bytes_written: 0,
            });
        }
        let mut edits = Vec::new();
        let mut paths = Vec::new();
        for update in &plan.preview.updates {
            ctx.check()?;
            let name = safe_ref(&update.destination)?;
            let old = before.get(&update.destination).copied();
            let expected = old.map_or(PreviousValue::MustNotExist, |old| {
                PreviousValue::MustExistAndMatch(Target::Object(old))
            });
            let oid = gix::ObjectId::from_hex(update.new.as_bytes()).map_err(engine)?;
            edits.push(RefEdit::update(
                name,
                Target::Object(oid),
                expected,
                format!("CIDE fetch {}", plan.name),
            ));
            paths.push(update.destination.clone());
        }
        // Keep configured reflogs enabled. Standard fetch also falls back to
        // a generic service identity when the repository has no committer.
        local_repo
            .committer_or_set_generic_fallback()
            .map_err(engine)?;
        let committer = local_repo
            .committer()
            .transpose()
            .map_err(engine)?
            .ok_or(Error::Unsupported)?;
        local_repo
            .edit_references_as(edits, Some(committer))
            .map_err(engine)?;
        Ok(TransferResult {
            remote: plan.name,
            updated: paths,
            objects_written: plan.objects.len(),
            bytes_written: plan.preview.object_bytes,
        })
    })
}

pub fn prepare_configured_push(
    root: &Path,
    remote_name: &str,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<PushPlan, Error> {
    limits.admit()?;
    controlled(limits.timeout, cancelled, |ctx| {
        let (local_repo, local) = open(root, ctx)?;
        let remote_name = resolve_remote_name(&local_repo, remote_name, Direction::Push)?;
        let head = current_head(&local_repo, limits, ctx)?;
        let (short, branch) = branch_name(&head)?;
        let configured = local_repo
            .branch_remote_name(branch.shorten(), Direction::Push)
            .ok_or_else(|| {
                Error::Engine("La rama activa no tiene remoto de subida configurado".into())
            })?;
        if configured.as_bstr().as_bytes() != remote_name.as_bytes() {
            return Err(Error::Stale);
        }
        let remote_branch = local_repo
            .branch_remote_ref_name(branch.as_ref(), Direction::Push)
            .and_then(Result::ok)
            .ok_or(Error::Unsupported)?;
        let destination = remote_branch
            .as_bstr()
            .to_str()
            .map_err(|_| Error::Unsupported)?
            .to_owned();
        let remote_path = remote_root(&local_repo, &remote_name, Direction::Push)?;
        let (remote_repo, remote) = open_bare_remote(&remote_path, ctx)?;
        let head_id = local_repo.head_id().map_err(engine)?.detach();
        let old = refs(&remote_repo, limits, ctx)?.get(&destination).copied();
        if let Some(old) = old {
            let fast_forward =
                match graph::contains_ancestor(&local_repo, &head_id.to_string(), old, limits, ctx)
                {
                    Ok(is_ancestor) => is_ancestor,
                    Err(Error::Missing) => false,
                    Err(error) => return Err(error),
                };
            if !fast_forward {
                return Err(Error::Engine("Rechazo no-fast-forward: el remoto contiene cambios que no están en la rama local".into()));
            }
        }
        let objects = object_closure(&local_repo, [head_id], limits, ctx)?;
        let object_bytes = objects.iter().map(|o| o.2.len()).sum();
        let update = RefUpdatePreview {
            source: format!("refs/heads/{short}"),
            destination,
            old: old.map(|id| id.to_string()),
            new: head_id.to_string(),
        };
        Ok(PushPlan {
            local,
            remote: remote.clone(),
            limits,
            name: remote_name.clone(),
            preview: TransferPreview {
                remote: remote_name,
                branch: short,
                destination_path: Some(remote.root.to_string_lossy().into_owned()),
                updates: vec![update],
                object_count: objects.len(),
                object_bytes,
                file_transport: true,
            },
            objects,
        })
    })
}

pub fn push_preview(plan: &PushPlan) -> &TransferPreview {
    &plan.preview
}

pub fn apply_local_push(
    plan: PushPlan,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<TransferResult, Error> {
    controlled(plan.limits.timeout, cancelled, |ctx| {
        let local_repo = check_identity(&plan.local.root, &plan.local, ctx)?;
        let Some(active_branch) = local_repo.head_name().map_err(engine)? else {
            return Err(Error::Stale);
        };
        if active_branch.as_bstr().to_str().ok() != Some(plan.preview.updates[0].source.as_str()) {
            return Err(Error::Stale);
        }
        let expected_head =
            gix::ObjectId::from_hex(plan.preview.updates[0].new.as_bytes()).map_err(engine)?;
        if local_repo.head_id().map_err(engine)?.detach() != expected_head {
            return Err(Error::Stale);
        }
        let branch =
            FullName::try_from(gix::bstr::BString::from(active_branch.as_bstr().as_bytes()))
                .map_err(|_| Error::Stale)?;
        let configured_remote = local_repo
            .branch_remote_name(branch.shorten(), Direction::Push)
            .ok_or(Error::Stale)?;
        if configured_remote.as_bstr().as_bytes() != plan.name.as_bytes() {
            return Err(Error::Stale);
        }
        let destination = local_repo
            .branch_remote_ref_name(branch.as_ref(), Direction::Push)
            .and_then(Result::ok)
            .ok_or(Error::Stale)?;
        if destination.as_bstr().to_str().ok() != Some(plan.preview.updates[0].destination.as_str())
        {
            return Err(Error::Stale);
        }
        let (mut remote_repo, actual_remote) = open_bare_remote(&plan.remote.root, ctx)?;
        if actual_remote.git_dir != plan.remote.git_dir || actual_remote.root != plan.remote.root {
            return Err(Error::Stale);
        }
        for (source, _, _) in &plan.objects {
            if !local_repo.has_object(source) {
                return Err(Error::Stale);
            }
        }
        let current = refs(&remote_repo, plan.limits, ctx)?
            .get(&plan.preview.updates[0].destination)
            .copied();
        if current.map(|id| id.to_string()) != plan.preview.updates[0].old {
            return Err(Error::Stale);
        }
        for (id, kind, bytes) in &plan.objects {
            ctx.check()?;
            let written = remote_repo
                .write_object(RawObject {
                    kind: *kind,
                    bytes: bytes.clone(),
                })
                .map_err(engine)?;
            if written.detach() != *id {
                return Err(Error::Engine(
                    "El objeto transferido no coincide con su identificador".into(),
                ));
            }
        }
        let update = &plan.preview.updates[0];
        let name = safe_ref(&update.destination)?;
        let oid = gix::ObjectId::from_hex(update.new.as_bytes()).map_err(engine)?;
        let expected = current.map_or(PreviousValue::MustNotExist, |current| {
            PreviousValue::MustExistAndMatch(Target::Object(current))
        });
        remote_repo.refs.write_reflog = gix::refs::store::WriteReflog::Disable;
        remote_repo
            .edit_references_as(
                [RefEdit::update(
                    name,
                    Target::Object(oid),
                    expected,
                    format!("CIDE push {}", plan.name),
                )],
                None,
            )
            .map_err(engine)?;
        Ok(TransferResult {
            remote: plan.name,
            updated: vec![update.destination.clone()],
            objects_written: plan.objects.len(),
            bytes_written: plan.preview.object_bytes,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .nth(2)
                .unwrap()
                .canonicalize()
                .unwrap();
            let parent = workspace.join("work/test-temp");
            std::fs::create_dir_all(&parent).unwrap();
            let parent = parent.canonicalize().unwrap();
            assert!(parent.starts_with(&workspace));
            let root = parent.join(format!(
                "remote-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir(&root).unwrap();
            Self(root)
        }
        fn p(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .nth(2)
                .unwrap()
                .canonicalize()
                .unwrap();
            assert!(self.0.starts_with(workspace.join("work/test-temp")));
            fn clean(path: &Path) {
                for entry in std::fs::read_dir(path).unwrap() {
                    let path = entry.unwrap().path();
                    let meta = std::fs::symlink_metadata(&path).unwrap();
                    assert!(!meta.file_type().is_symlink());
                    if meta.is_dir() {
                        clean(&path);
                    }
                }
            }
            clean(&self.0);
            std::fs::remove_dir_all(&self.0).unwrap();
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
        parent: Option<gix::ObjectId>,
        message: &str,
    ) -> gix::ObjectId {
        let tree = repo
            .write_object(gix::objs::Tree {
                entries: Vec::new(),
            })
            .unwrap()
            .detach();
        let sig = gix::actor::SignatureRef {
            name: b"CIDE test".as_bstr(),
            email: b"cide@example.invalid".as_bstr(),
            time: "1 +0000",
        };
        repo.write_object(gix::objs::Commit {
            tree,
            parents: parent.into_iter().collect(),
            author: sig.into(),
            committer: sig.into(),
            message: message.as_bytes().into(),
            encoding: None,
            extra_headers: Vec::new(),
        })
        .unwrap()
        .detach()
    }
    fn ref_update(
        repo: &mut gix::Repository,
        name: &str,
        oid: gix::ObjectId,
        previous: PreviousValue,
    ) {
        repo.refs.write_reflog = gix::refs::store::WriteReflog::Disable;
        repo.edit_references_as(
            [RefEdit::update(
                safe_ref(name).unwrap(),
                Target::Object(oid),
                previous,
                String::new(),
            )],
            None,
        )
        .unwrap();
    }
    fn head_to(repo: &mut gix::Repository, branch: &str) {
        repo.refs.write_reflog = gix::refs::store::WriteReflog::Disable;
        let head = safe_ref("HEAD").unwrap();
        let old = repo
            .find_reference("HEAD")
            .unwrap()
            .target()
            .try_name()
            .unwrap()
            .to_owned();
        repo.edit_references_as(
            [RefEdit::update(
                head,
                Target::Symbolic(safe_ref(branch).unwrap()),
                PreviousValue::MustExistAndMatch(Target::Symbolic(old)),
                String::new(),
            )],
            None,
        )
        .unwrap();
    }
    fn bare_with_commit(
        path: &Path,
        branch: &str,
        message: &str,
    ) -> (gix::Repository, gix::ObjectId) {
        let mut repo = gix::init_bare(path).unwrap();
        let id = commit(&repo, None, message);
        ref_update(&mut repo, branch, id, PreviousValue::MustNotExist);
        head_to(&mut repo, branch);
        (repo, id)
    }

    #[test]
    fn bare_clone_is_create_only_and_pins_heads_and_tags() {
        let fixture = Fixture::new();
        let source_path = fixture.p("source.git");
        let (mut source, tip) = bare_with_commit(&source_path, "refs/heads/main", "seed");
        let tag = commit(&source, Some(tip), "tagged");
        ref_update(
            &mut source,
            "refs/tags/v1",
            tag,
            PreviousValue::MustNotExist,
        );
        let destination = fixture.p("clone.git");
        let plan =
            prepare_local_bare_clone(&source_path, &destination, "origin", limits(), || false)
                .unwrap();
        assert_eq!(clone_preview(&plan).updates.len(), 2);
        apply_local_bare_clone(plan, || false).unwrap();
        let stop = std::sync::atomic::AtomicBool::new(false);
        let reason = std::sync::atomic::AtomicU8::new(0);
        let ctx = Context {
            stop: &stop,
            reason: &reason,
            deadline: std::time::Instant::now() + Duration::from_secs(5),
        };
        let cloned = open_bare_remote(&destination, &ctx).unwrap().0;
        assert_eq!(
            cloned
                .find_reference("refs/heads/main")
                .unwrap()
                .target()
                .try_id()
                .unwrap()
                .to_string(),
            tip.to_string()
        );
        assert!(cloned.find_reference("refs/tags/v1").is_ok());

        let occupied = fixture.p("occupied.git");
        std::fs::create_dir(&occupied).unwrap();
        std::fs::write(occupied.join("keep.txt"), b"keep").unwrap();
        assert!(matches!(
            prepare_local_bare_clone(&source_path, &occupied, "origin", limits(), || false),
            Err(Error::Stale)
        ));
        assert_eq!(std::fs::read(occupied.join("keep.txt")).unwrap(), b"keep");
    }

    #[test]
    fn configured_push_rejects_non_fast_forward_without_mutating_remote() {
        let fixture = Fixture::new();
        let remote_path = fixture.p("remote.git");
        let (remote, remote_tip) = bare_with_commit(&remote_path, "refs/heads/main", "remote-only");
        let local_path = fixture.p("local");
        let mut local = gix::init(&local_path).unwrap();
        let local_tip = commit(&local, None, "local-only");
        ref_update(
            &mut local,
            "refs/heads/main",
            local_tip,
            PreviousValue::MustNotExist,
        );
        head_to(&mut local, "refs/heads/main");
        let config_path = local.git_dir().join("config");
        let mut config = std::fs::OpenOptions::new()
            .append(true)
            .open(config_path)
            .unwrap();
        use io::Write as _;
        let remote_url = remote_path.to_string_lossy().replace('\\', "/");
        writeln!(config, "\n[user]\n name = CIDE Test\n email = cide@example.invalid\n[remote \"origin\"]\n url = {remote_url}\n fetch = +refs/heads/*:refs/remotes/origin/*\n[branch \"main\"]\n remote = origin\n merge = refs/heads/main").unwrap();
        config.sync_all().unwrap();
        let result = prepare_configured_push(&local_path, "", limits(), || false);
        assert!(
            matches!(&result, Err(Error::Engine(message)) if message.contains("no-fast-forward")),
            "resultado inesperado al rechazar no-fast-forward: {:?}",
            result.as_ref().err()
        );
        assert_eq!(
            remote
                .find_reference("refs/heads/main")
                .unwrap()
                .target()
                .try_id()
                .unwrap()
                .to_string(),
            remote_tip.to_string()
        );

        let fetch = prepare_configured_fetch(&local_path, "", limits(), || false).unwrap();
        assert_eq!(fetch_preview(&fetch).updates.len(), 1);
        apply_local_fetch(fetch, || false).unwrap();
        assert_eq!(
            local
                .find_reference("refs/remotes/origin/main")
                .unwrap()
                .target()
                .try_id()
                .unwrap()
                .to_string(),
            remote_tip.to_string()
        );

        let descendant = commit(&local, Some(remote_tip), "local fast-forward");
        ref_update(
            &mut local,
            "refs/heads/main",
            descendant,
            PreviousValue::MustExistAndMatch(Target::Object(local_tip)),
        );
        let push = prepare_configured_push(&local_path, "", limits(), || false).unwrap();
        assert_eq!(
            push_preview(&push).updates[0].old.clone(),
            Some(remote_tip.to_string())
        );
        apply_local_push(push, || false).unwrap();

        let remote_after = gix::open(&remote_path).unwrap();
        assert_eq!(
            remote_after
                .find_reference("refs/heads/main")
                .unwrap()
                .target()
                .try_id()
                .unwrap(),
            descendant
        );
        assert!(remote_after.has_object(descendant));
    }
}
