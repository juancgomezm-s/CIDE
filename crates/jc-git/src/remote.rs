//! Bounded remote synchronization and cloning with reviewed publication.
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

#[path = "remote_network/mod.rs"]
mod remote_network;

const MAX_OBJECTS: usize = 65_536;
const MAX_OBJECT_SIZE: usize = 16 * 1024 * 1024;
const MAX_REF_NAME: usize = 1024;

type RawObjects = Vec<(gix::ObjectId, gix::objs::Kind, Vec<u8>)>;
type AdvertisedRefs = BTreeMap<String, gix::ObjectId>;

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
    network: Option<NetworkPlan>,
}

pub struct PushPlan {
    local: Identity,
    remote: Identity,
    limits: Limits,
    name: String,
    preview: TransferPreview,
    objects: Vec<(gix::ObjectId, gix::objs::Kind, Vec<u8>)>,
    network: Option<NetworkPlan>,
}

pub(super) struct NetworkPlan {
    pub(super) url: String,
    pub(super) advertised: BTreeMap<String, gix::ObjectId>,
}

/// A short-lived secret entered for one Git operation. Its bytes are cleared
/// on drop and it deliberately has no `Clone` or `Debug` implementation.
pub struct SecretString(Vec<u8>);

impl SecretString {
    pub fn new(secret: String) -> Self {
        Self(secret.into_bytes())
    }

    pub fn expose(&self) -> &str {
        // Construction only accepts Rust strings, so this invariant survives
        // byte overwrites only during Drop, after all borrows have ended.
        std::str::from_utf8(&self.0).expect("credential secret is valid UTF-8")
    }
}

impl Drop for SecretString {
    fn drop(&mut self) {
        for byte in &mut self.0 {
            unsafe { std::ptr::write_volatile(byte, 0) };
        }
    }
}

pub struct OperationCredential {
    pub username: String,
    pub secret: SecretString,
    pub host_fingerprint_pin: Option<String>,
    pub private_key: Option<PathBuf>,
}

/// A local clone staged next to its destination and published by one rename.
pub struct LocalClonePlan {
    source: Identity,
    destination: PathBuf,
    destination_pin: jc_fs::MissingToken,
    staging: PathBuf,
    limits: Limits,
    remote_name: String,
    head: CloneHead,
    refs: BTreeMap<String, gix::ObjectId>,
    objects: Vec<(gix::ObjectId, gix::objs::Kind, Vec<u8>)>,
    worktree: bool,
    checkout: Option<crate::worktree_materialize::CheckoutTree>,
    preview: TransferPreview,
}

#[derive(PartialEq, Eq)]
enum CloneHead {
    Branch(String),
    Detached(gix::ObjectId),
}

impl CloneHead {
    fn read(repo: &gix::Repository) -> Result<Self, Error> {
        if let Some(name) = repo.head_name().map_err(engine)? {
            let name = text(name.as_bstr())?;
            if !name.starts_with("refs/heads/") {
                return Err(Error::Unsupported);
            }
            Ok(Self::Branch(name))
        } else {
            Ok(Self::Detached(repo.head_id().map_err(engine)?.detach()))
        }
    }

    fn tip(&self, refs: &BTreeMap<String, gix::ObjectId>) -> Option<gix::ObjectId> {
        match self {
            Self::Branch(name) => refs.get(name).copied(),
            Self::Detached(id) => Some(*id),
        }
    }

    fn label(&self) -> String {
        match self {
            Self::Branch(name) => name.clone(),
            Self::Detached(id) => format!("HEAD separado · {id}"),
        }
    }
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

pub fn clone_checkout(plan: &LocalClonePlan) -> Option<(usize, usize)> {
    plan.worktree.then(|| {
        plan.checkout.as_ref().map_or((0, 0), |checkout| {
            (checkout.file_count(), checkout.byte_count())
        })
    })
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

/// Prepare a bounded bare clone from a local bare repository.
pub fn prepare_local_bare_clone(
    source_root: &Path,
    destination: &Path,
    remote_name: &str,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<LocalClonePlan, Error> {
    prepare_local_clone(
        source_root,
        destination,
        remote_name,
        false,
        limits,
        cancelled,
    )
}

/// Prepare a checked-out clone from a local ordinary or bare repository.
pub fn prepare_local_worktree_clone(
    source_root: &Path,
    destination: &Path,
    remote_name: &str,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<LocalClonePlan, Error> {
    prepare_local_clone(
        source_root,
        destination,
        remote_name,
        true,
        limits,
        cancelled,
    )
}

pub(crate) fn open_clone_source(
    source: &Path,
    worktree: bool,
    ctx: &Context<'_>,
) -> Result<(gix::Repository, Identity), Error> {
    if worktree && index_write::exists(&source.join(".git"))? {
        open(source, ctx)
    } else {
        open_bare_remote(source, ctx)
    }
}

fn clone_source_refs(
    repo: &gix::Repository,
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<BTreeMap<String, gix::ObjectId>, Error> {
    let (_, entries) = graph::references(repo, limits, &mut Budget::new(limits), ctx)?;
    entries
        .into_iter()
        .filter(|entry| {
            entry.name.starts_with(b"refs/heads/") || entry.name.starts_with(b"refs/tags/")
        })
        .map(|entry| {
            let graph::RefTarget::Direct(id) = entry.target else {
                return Err(Error::Unsupported);
            };
            Ok((
                text(&entry.name)?,
                gix::ObjectId::from_hex(id.as_bytes()).map_err(engine)?,
            ))
        })
        .collect()
}

fn clone_ref_updates(
    source: &BTreeMap<String, gix::ObjectId>,
    head: &CloneHead,
    remote: &str,
    worktree: bool,
) -> Vec<RefUpdatePreview> {
    let mut updates = Vec::new();
    for (name, id) in source {
        let mut update = RefUpdatePreview {
            source: name.clone(),
            destination: name.clone(),
            old: None,
            new: id.to_string(),
        };
        if worktree && let Some(short) = name.strip_prefix("refs/heads/") {
            if matches!(head, CloneHead::Branch(active) if active == name) {
                updates.push(update.clone());
            }
            update.destination = format!("refs/remotes/{remote}/{short}");
        }
        updates.push(update);
    }
    updates
}

fn prepare_local_clone(
    source_root: &Path,
    destination: &Path,
    remote_name: &str,
    worktree: bool,
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
        let (source_repo, source) = open_clone_source(source_root, worktree, ctx)?;
        if source_repo.object_hash() != gix::hash::Kind::Sha1 {
            return Err(Error::Unsupported);
        }
        if !destination.is_absolute() {
            return Err(Error::InvalidRoot);
        }
        let parent_in = destination.parent().ok_or(Error::InvalidRoot)?;
        let parent = parent_in.canonicalize()?;
        index_write::ordinary(&parent, true)?;
        let leaf = destination.file_name().ok_or(Error::InvalidRoot)?;
        let leaf_name = leaf.to_str().ok_or(Error::InvalidRoot)?;
        gix::validate::path::component(leaf_name.as_bytes().as_bstr(), None, Default::default())
            .map_err(engine)?;
        if leaf_name.contains('~') {
            return Err(Error::Unsupported);
        }
        let destination = parent.join(leaf);
        if destination.starts_with(&source.git_dir) {
            return Err(Error::Stale);
        }
        let destination_pin = clone_destination_pin(&destination, ctx)?;
        let selected = clone_source_refs(&source_repo, limits, ctx)?;
        let head = CloneHead::read(&source_repo)?;
        let tip = head.tip(&selected);
        let checkout = if worktree {
            tip.map(|oid| crate::worktree_materialize::prepare(&source_repo, oid, limits, ctx))
                .transpose()?
        } else {
            None
        };
        let objects = object_closure(
            &source_repo,
            selected.values().copied().chain(tip),
            limits,
            ctx,
        )?;
        let object_bytes = objects.iter().map(|o| o.2.len()).sum();
        let staging = stage_candidate(&parent, leaf_name);
        let object_count = objects.len();
        let updates = clone_ref_updates(&selected, &head, remote_name, worktree);
        let branch = head.label();
        Ok(LocalClonePlan {
            source,
            destination: destination.clone(),
            destination_pin,
            staging,
            limits,
            remote_name: remote_name.to_owned(),
            head,
            refs: selected,
            objects,
            worktree,
            checkout,
            preview: TransferPreview {
                remote: remote_name.to_owned(),
                branch,
                destination_path: Some(destination.to_string_lossy().into_owned()),
                updates,
                object_count,
                object_bytes,
                file_transport: true,
            },
        })
    })
}

fn clone_destination_pin(path: &Path, ctx: &Context<'_>) -> Result<jc_fs::MissingToken, Error> {
    let parent = path.parent().ok_or(Error::InvalidRoot)?;
    if index_write::exists(path)? {
        return Err(Error::Stale);
    }
    match crate::local_pair::capture(parent, path, 0, ctx)? {
        jc_fs::Capture::Absent { token } if token.is_leaf() => Ok(token),
        _ => Err(Error::Stale),
    }
}

fn set_clone_head(target: &gix::Repository, destination: &CloneHead) -> Result<(), Error> {
    let head = safe_ref("HEAD")?;
    let current = target
        .find_reference("HEAD")
        .map_err(engine)?
        .target()
        .into_owned();
    let destination = match destination {
        CloneHead::Branch(branch) => Target::Symbolic(safe_ref(branch)?),
        CloneHead::Detached(id) => Target::Object(*id),
    };
    if current != destination {
        target
            .edit_references_as(
                [RefEdit::update(
                    head,
                    destination,
                    PreviousValue::MustExistAndMatch(current),
                    String::new(),
                )],
                None,
            )
            .map_err(engine)?;
    }
    Ok(())
}

fn write_clone_config(
    target: &gix::Repository,
    location: &str,
    remote_name: &str,
    tracking: Option<&str>,
) -> Result<(), Error> {
    use io::Write;
    let config_path = target.git_dir().join("config");
    let mut config = gix::config::File::from_bytes_no_includes(
        &std::fs::read(&config_path)?,
        gix::config::file::Metadata::default(),
        gix::config::file::init::Options::default(),
    )
    .map_err(|_| Error::Unsupported)?;
    let mut remote = config.new_section("remote", remote_name).map_err(engine)?;
    remote.set("url", location).map_err(engine)?;
    let fetch = format!("+refs/heads/*:refs/remotes/{remote_name}/*");
    remote.set("fetch", fetch.as_str()).map_err(engine)?;
    if let Some(name) = tracking {
        let short = name.strip_prefix("refs/heads/").ok_or(Error::Unsupported)?;
        let mut branch = config.new_section("branch", short).map_err(engine)?;
        branch.set("remote", remote_name).map_err(engine)?;
        branch.set("merge", name).map_err(engine)?;
    }
    let mut bytes = Vec::new();
    config.write_to(&mut bytes)?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(config_path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(())
}

fn revalidate_clone_source(plan: &LocalClonePlan, ctx: &Context<'_>) -> Result<(), Error> {
    let (source, identity) = open_clone_source(&plan.source.root, plan.worktree, ctx)?;
    index_write::same_repository(&plan.source, &identity)?;
    if clone_source_refs(&source, plan.limits, ctx)? != plan.refs
        || CloneHead::read(&source)? != plan.head
    {
        return Err(Error::Stale);
    }
    Ok(())
}

pub fn apply_local_bare_clone(
    plan: LocalClonePlan,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<TransferResult, Error> {
    if plan.worktree {
        return Err(Error::Unsupported);
    }
    apply_local_clone(plan, cancelled)
}

pub fn apply_local_worktree_clone(
    plan: LocalClonePlan,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<TransferResult, Error> {
    if !plan.worktree {
        return Err(Error::Unsupported);
    }
    apply_local_clone(plan, cancelled)
}

fn apply_local_clone(
    plan: LocalClonePlan,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<TransferResult, Error> {
    controlled(plan.limits.timeout, cancelled, |ctx| {
        revalidate_clone_source(&plan, ctx)?;
        let parent = plan.destination.parent().ok_or(Error::InvalidRoot)?;
        let _parent_guard = jc_fs::pin_tree_parents(parent, &plan.destination)?;
        if !plan
            .destination_pin
            .same_location_as(&clone_destination_pin(&plan.destination, ctx)?)
        {
            return Err(Error::Stale);
        }
        std::fs::create_dir(&plan.staging)?;
        let created = plan.staging.clone();
        let result = (|| {
            let mut target = if plan.worktree {
                gix::init(&created)
            } else {
                gix::init_bare(&created)
            }
            .map_err(engine)?;
            target.refs.write_reflog = gix::refs::store::WriteReflog::Disable;
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
            for update in &plan.preview.updates {
                let name = safe_ref(&update.destination)?;
                edits.push(RefEdit::update(
                    name,
                    Target::Object(gix::ObjectId::from_hex(update.new.as_bytes()).map_err(engine)?),
                    PreviousValue::MustNotExist,
                    String::new(),
                ));
            }
            if !edits.is_empty() {
                target.edit_references_as(edits, None).map_err(engine)?;
            }
            set_clone_head(&target, &plan.head)?;
            let location = plan.source.root.to_str().ok_or(Error::Unsupported)?;
            if location.chars().any(char::is_control) {
                return Err(Error::Unsupported);
            }
            let location = location
                .strip_prefix(r"\\?\")
                .filter(|path| path.as_bytes().get(1) == Some(&b':'))
                .unwrap_or(location)
                .replace('\\', "/");
            let tracking = match (&plan.head, plan.worktree) {
                (CloneHead::Branch(name), true) => Some(name.as_str()),
                _ => None,
            };
            write_clone_config(&target, &location, &plan.remote_name, tracking)?;
            if let Some(checkout) = &plan.checkout {
                checkout.write(&created, &target.git_dir().join("index"), ctx)?;
            }
            ctx.check()?;
            revalidate_clone_source(&plan, ctx)?;
            if !plan
                .destination_pin
                .same_location_as(&clone_destination_pin(&plan.destination, ctx)?)
            {
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
            updated: plan
                .preview
                .updates
                .into_iter()
                .map(|update| update.destination)
                .collect(),
            objects_written: plan.objects.len(),
            bytes_written: plan.preview.object_bytes,
        })
    })
}

/// A reviewed remote endpoint and a create-only destination. The handshake
/// and pack are received only after confirmation, inside private staging.
pub struct NetworkClonePlan {
    source: gix::url::Url,
    source_label: String,
    destination: PathBuf,
    destination_pin: jc_fs::MissingToken,
    staging: PathBuf,
    limits: Limits,
}

pub struct NetworkCloneResult {
    pub destination: PathBuf,
    pub files: usize,
    pub bytes: usize,
}

impl NetworkClonePlan {
    pub fn source_label(&self) -> &str {
        &self.source_label
    }

    pub fn destination(&self) -> &Path {
        &self.destination
    }
}

pub fn prepare_network_worktree_clone(
    location: &str,
    destination: &Path,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<NetworkClonePlan, Error> {
    limits.admit()?;
    if !cfg!(windows)
        || !destination.is_absolute()
        || location.len() > 4096
        || location.chars().any(char::is_control)
        || location.contains(['?', '#'])
    {
        return Err(Error::Unsupported);
    }
    let source = gix::url::parse(location.as_bytes().as_bstr()).map_err(|_| Error::Unsupported)?;
    let protocol = match source.scheme {
        gix::url::Scheme::Https if source.user().is_none() => "HTTPS",
        gix::url::Scheme::Ssh => "SSH",
        gix::url::Scheme::Http if test_loopback(&source) && source.user().is_none() => "HTTP local",
        _ => return Err(Error::Unsupported),
    };
    if source.password().is_some() {
        return Err(Error::Engine(
            "Proporciona las credenciales fuera de la URL".into(),
        ));
    }
    let host = source
        .host()
        .filter(|host| !host.is_empty())
        .ok_or(Error::Unsupported)?;
    let source_label = format!("{protocol} · {host}");
    controlled(limits.timeout, cancelled, |ctx| {
        let parent = destination
            .parent()
            .ok_or(Error::InvalidRoot)?
            .canonicalize()?;
        index_write::ordinary(&parent, true)?;
        let leaf = destination
            .file_name()
            .and_then(|leaf| leaf.to_str())
            .ok_or(Error::InvalidRoot)?;
        gix::validate::path::component(leaf.as_bytes().as_bstr(), None, Default::default())
            .map_err(engine)?;
        if leaf.contains('~') {
            return Err(Error::Unsupported);
        }
        let destination = parent.join(leaf);
        Ok(NetworkClonePlan {
            source,
            source_label,
            destination_pin: clone_destination_pin(&destination, ctx)?,
            destination,
            staging: stage_candidate(&parent, leaf),
            limits,
        })
    })
}

fn test_loopback(url: &gix::url::Url) -> bool {
    cfg!(test)
        && url
            .host()
            .and_then(|host| host.parse::<std::net::IpAddr>().ok())
            .is_some_and(|host| host.is_loopback())
}

pub fn apply_network_worktree_clone(
    plan: NetworkClonePlan,
    credential: Option<&OperationCredential>,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<NetworkCloneResult, Error> {
    controlled(plan.limits.timeout, cancelled, |ctx| {
        let parent = plan.destination.parent().ok_or(Error::InvalidRoot)?;
        let _guard = jc_fs::pin_tree_parents(parent, &plan.destination)?;
        if !plan
            .destination_pin
            .same_location_as(&clone_destination_pin(&plan.destination, ctx)?)
        {
            return Err(Error::Stale);
        }
        std::fs::create_dir(&plan.staging)?;
        let result = (|| {
            drop(gix::init(&plan.staging).map_err(engine)?);
            let (mut target, _) = open(&plan.staging, ctx)?;
            target.refs.write_reflog = gix::refs::store::WriteReflog::Disable;
            let (branch, tip) = remote_network::fetch_clone_into_staging(
                &target,
                &plan.source,
                credential,
                plan.limits,
                ctx,
            )?;
            let tracking = branch.as_deref();
            if let Some(branch) = tracking {
                let name = safe_ref(branch)?;
                if !branch.starts_with("refs/heads/") {
                    return Err(Error::Unsupported);
                }
                if let Some(oid) = tip {
                    target
                        .edit_references_as(
                            [RefEdit::update(
                                name,
                                Target::Object(oid),
                                PreviousValue::MustNotExist,
                                String::new(),
                            )],
                            None,
                        )
                        .map_err(engine)?;
                }
                set_clone_head(&target, &CloneHead::Branch(branch.to_owned()))?;
            } else if let Some(oid) = tip {
                set_clone_head(&target, &CloneHead::Detached(oid))?;
            }
            let location = plan.source.to_bstring();
            let location = location.to_str().map_err(|_| Error::Unsupported)?;
            write_clone_config(&target, location, "origin", tracking)?;
            let (files, bytes) = if let Some(oid) = tip {
                let checkout =
                    crate::worktree_materialize::prepare(&target, oid, plan.limits, ctx)?;
                checkout.write(&plan.staging, &target.git_dir().join("index"), ctx)?;
                (checkout.file_count(), checkout.byte_count())
            } else {
                (0, 0)
            };
            drop(target);
            ctx.check()?;
            if !plan
                .destination_pin
                .same_location_as(&clone_destination_pin(&plan.destination, ctx)?)
            {
                return Err(Error::Stale);
            }
            publish_directory_create_only(&plan.staging, &plan.destination)?;
            Ok(NetworkCloneResult {
                destination: plan.destination,
                files,
                bytes,
            })
        })();
        result.map_err(|error: Error| {
            Error::Engine(format!(
                "{error}. Se conservó la preparación en {}",
                plan.staging.display()
            ))
        })
    })
}

#[cfg(windows)]
pub(crate) fn publish_directory_create_only(from: &Path, to: &Path) -> Result<(), Error> {
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
pub(crate) fn publish_directory_create_only(_from: &Path, _to: &Path) -> Result<(), Error> {
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

/// Return whether the configured remote uses a supported network scheme.
/// This only inspects configuration; it never opens a connection or invokes a
/// credential helper. `push` selects `pushurl`/pushInsteadOf semantics.
pub fn configured_remote_is_network(
    root: &Path,
    requested_remote: &str,
    push: bool,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<bool, Error> {
    limits.admit()?;
    controlled(limits.timeout, cancelled, |ctx| {
        let (repo, _) = open(root, ctx)?;
        let direction = if push {
            Direction::Push
        } else {
            Direction::Fetch
        };
        let name = resolve_remote_name(&repo, requested_remote, direction)?;
        let remote = repo
            .find_remote(&name)
            .map_err(|_| Error::Engine("La configuración del remoto no es válida".into()))?;
        let url = remote.url(direction).ok_or(Error::Unsupported)?;
        match url.scheme {
            gix::url::Scheme::File => Ok(false),
            gix::url::Scheme::Http | gix::url::Scheme::Https | gix::url::Scheme::Ssh => Ok(true),
            _ => Err(Error::Unsupported),
        }
    })
}

pub fn prepare_network_push(
    root: &Path,
    requested_remote: &str,
    credential: Option<&OperationCredential>,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<PushPlan, Error> {
    remote_network::prepare_network_push(root, requested_remote, credential, limits, cancelled)
}

pub fn apply_network_push(
    plan: PushPlan,
    credential: Option<&OperationCredential>,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<TransferResult, Error> {
    remote_network::apply_network_push(plan, credential, cancelled)
}

pub fn prepare_network_fetch(
    root: &Path,
    requested_remote: &str,
    credential: Option<&OperationCredential>,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<FetchPlan, Error> {
    limits.admit()?;
    controlled(limits.timeout, cancelled, |ctx| {
        let (local_repo, local) = open(root, ctx)?;
        let name = resolve_remote_name(&local_repo, requested_remote, Direction::Fetch)?;
        let _mapping = fetch_mapping(&local_repo, &name)?;
        let remote = local_repo.find_remote(&name).map_err(engine)?;
        let url = remote.url(Direction::Fetch).ok_or(Error::Unsupported)?;
        if !matches!(
            url.scheme,
            gix::url::Scheme::Http | gix::url::Scheme::Https | gix::url::Scheme::Ssh
        ) {
            return Err(Error::Unsupported);
        }
        let (objects, advertised) =
            network_fetch_objects(&local_repo, url, credential, limits, ctx)?;
        let local_refs = refs(&local_repo, limits, ctx)?;
        let mut updates = Vec::new();
        let mut tips = Vec::new();
        for (source, oid) in advertised
            .iter()
            .filter(|(name, _)| name.starts_with("refs/heads/"))
        {
            let short = &source["refs/heads/".len()..];
            let destination = format!("refs/remotes/{name}/{short}");
            updates.push(RefUpdatePreview {
                source: source.clone(),
                destination: destination.clone(),
                old: local_refs.get(&destination).map(ToString::to_string),
                new: oid.to_string(),
            });
            tips.push(*oid);
            if updates.len() > limits.max_entries {
                return Err(Error::Limited);
            }
        }
        let object_bytes = objects.iter().try_fold(0usize, |total, entry| {
            total.checked_add(entry.2.len()).ok_or(Error::Limited)
        })?;
        Ok(FetchPlan {
            local: local.clone(),
            remote: Identity {
                root: PathBuf::new(),
                git_dir: PathBuf::new(),
            },
            limits,
            name: name.clone(),
            preview: TransferPreview {
                remote: name,
                branch: "ramas remotas".into(),
                destination_path: Some(local.root.to_string_lossy().into_owned()),
                updates,
                object_count: objects.len(),
                object_bytes,
                file_transport: false,
            },
            objects,
            network: Some(NetworkPlan {
                url: url.to_bstring().to_string(),
                advertised,
            }),
        })
    })
}

pub fn apply_network_fetch(
    plan: FetchPlan,
    credential: Option<&OperationCredential>,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<TransferResult, Error> {
    let network = plan.network.as_ref().ok_or(Error::Unsupported)?;
    let url = gix::url::parse(network.url.as_bytes().as_bstr()).map_err(|_| Error::Stale)?;
    controlled(plan.limits.timeout, cancelled, |ctx| {
        let mut local = check_identity(&plan.local.root, &plan.local, ctx)?;
        let configured = local.find_remote(&plan.name).map_err(|_| Error::Stale)?;
        let configured_url = configured.url(Direction::Fetch).ok_or(Error::Stale)?;
        if configured_url.to_bstring().as_bstr().as_bytes() != network.url.as_bytes()
            || fetch_mapping(&local, &plan.name).is_err()
        {
            return Err(Error::Stale);
        }
        let current = network_snapshot(&local, &url, credential, plan.limits, ctx)?;
        if current != network.advertised {
            return Err(Error::Stale);
        }
        for (source, kind, bytes) in &plan.objects {
            if gix::objs::compute_hash(local.object_hash(), *kind, bytes).map_err(engine)?
                != *source
            {
                return Err(Error::Stale);
            }
        }
        for (id, kind, bytes) in &plan.objects {
            ctx.check()?;
            if local
                .write_object(RawObject {
                    kind: *kind,
                    bytes: bytes.clone(),
                })
                .map_err(engine)?
                .detach()
                != *id
            {
                return Err(Error::Stale);
            }
        }
        let before = refs(&local, plan.limits, ctx)?;
        let mut edits = Vec::new();
        let mut paths = Vec::new();
        for update in &plan.preview.updates {
            let current = before.get(&update.destination).copied();
            if current.map(|id| id.to_string()) != update.old {
                return Err(Error::Stale);
            }
            let expected = current.map_or(PreviousValue::MustNotExist, |old| {
                PreviousValue::MustExistAndMatch(Target::Object(old))
            });
            edits.push(RefEdit::update(
                safe_ref(&update.destination)?,
                Target::Object(gix::ObjectId::from_hex(update.new.as_bytes()).map_err(engine)?),
                expected,
                format!("CIDE fetch {}", plan.name),
            ));
            paths.push(update.destination.clone());
        }
        if !edits.is_empty() {
            local.committer_or_set_generic_fallback().map_err(engine)?;
            let committer = local
                .committer()
                .transpose()
                .map_err(engine)?
                .ok_or(Error::Unsupported)?;
            local
                .edit_references_as(edits, Some(committer))
                .map_err(engine)?;
        }
        Ok(TransferResult {
            remote: plan.name,
            updated: paths,
            objects_written: plan.objects.len(),
            bytes_written: plan.preview.object_bytes,
        })
    })
}

fn network_snapshot(
    local_repo: &gix::Repository,
    url: &gix::url::Url,
    credential: Option<&OperationCredential>,
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<AdvertisedRefs, Error> {
    let (_guard, stage) = network_stage(local_repo, ctx)?;
    let result =
        network_advertise_into(&stage, url, credential, limits, ctx).map(|result| result.0);
    drop(stage);
    result
}

fn network_fetch_objects(
    local_repo: &gix::Repository,
    url: &gix::url::Url,
    credential: Option<&OperationCredential>,
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<(RawObjects, AdvertisedRefs), Error> {
    let (_guard, stage) = network_stage(local_repo, ctx)?;
    let (advertised, _) = network_fetch_into(&stage, url, credential, limits, ctx)?;
    let tips = advertised.values().copied().collect::<Vec<_>>();
    let objects = object_closure(&stage, tips, limits, ctx)?;
    drop(stage);
    Ok((objects, advertised))
}

struct NetworkStage {
    path: PathBuf,
    parent: PathBuf,
}
impl Drop for NetworkStage {
    fn drop(&mut self) {
        let mut cancelled = || false;
        let Ok(nodes) = jc_fs::inspect_tree(&self.parent, &self.path, 65_536, &mut cancelled)
        else {
            return;
        };
        let mut nodes = nodes.into_iter().collect::<Vec<_>>();
        nodes.sort_by_key(|(path, _)| std::cmp::Reverse(path.components().count()));
        for (path, node) in nodes {
            match node {
                jc_fs::TreeNode::File => {
                    if let Ok(jc_fs::Capture::Present { token, .. }) =
                        jc_fs::capture_in_root(&self.parent, &path, 64 * 1024 * 1024, &mut || false)
                    {
                        let _ = jc_fs::remove_captured_file(&self.parent, &token);
                    }
                }
                jc_fs::TreeNode::Directory(token) => {
                    let _ = jc_fs::remove_empty_directory(&self.parent, &path, &token);
                }
            }
        }
    }
}

fn network_stage(
    local_repo: &gix::Repository,
    ctx: &Context<'_>,
) -> Result<(NetworkStage, gix::Repository), Error> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let parent = local_repo.git_dir();
    for _ in 0..32 {
        ctx.check()?;
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = parent.join(format!("cide-network-stage-{}-{id}", std::process::id()));
        match std::fs::create_dir(&path) {
            Ok(()) => {
                let guard = NetworkStage {
                    path: path.clone(),
                    parent: parent.to_path_buf(),
                };
                let initial = gix::init_bare(&path).map_err(engine)?;
                drop(initial);
                let repo = gix::open_opts(
                    &path,
                    gix::open::Options::isolated().config_overrides([
                        "gitoxide.objects.allocLimit=16777216",
                        "pack.threads=1",
                    ]),
                )
                .map_err(engine)?;
                return Ok((guard, repo));
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(Error::Io(error)),
        }
    }
    Err(Error::Limited)
}

fn network_fetch_into(
    stage: &gix::Repository,
    url: &gix::url::Url,
    credential: Option<&OperationCredential>,
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<(AdvertisedRefs, Option<String>), Error> {
    let url_string = url.to_bstring().to_string();
    let remote = stage
        .remote_at_without_url_rewrite(url_string)
        .map_err(engine)?
        .with_refspecs(
            [
                "+refs/heads/*:refs/remotes/origin/*",
                "+refs/tags/*:refs/tags/*",
                "HEAD:refs/remotes/origin/HEAD",
            ],
            Direction::Fetch,
        )
        .map_err(engine)?;
    let transport = remote_network::connect_transport(stage, url, false, credential, limits, ctx)?;
    let connection = remote
        .to_connection_with_transport(transport)
        .with_credentials(|_| Ok(None));
    let prepare = connection
        .prepare_fetch(
            gix::progress::Discard,
            gix::remote::ref_map::Options::default(),
        )
        .map_err(engine)?;
    let advertised = advertised_refs(prepare.ref_map(), limits)?;
    let outcome = prepare
        .receive(gix::progress::Discard, ctx.stop)
        .map_err(engine)?;
    if outcome.ref_map.object_hash != stage.object_hash() {
        return Err(Error::Unsupported);
    }
    ctx.check()?;
    Ok(advertised)
}

/// Fetch a pinned submodule commit into an operation-owned staging repository.
/// The transport URL is parsed in isolation; credentials are transient and are
/// never written to Git configuration or returned to the UI.
pub(crate) fn fetch_submodule_url(
    repo: &gix::Repository,
    url: &str,
    expected_oid: gix::ObjectId,
    credential: Option<&OperationCredential>,
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<(), Error> {
    if url.len() > 4096 || url.chars().any(char::is_control) {
        return Err(Error::Unsupported);
    }
    let parsed = gix::url::parse(url.as_bytes().as_bstr()).map_err(|_| Error::Unsupported)?;
    if parsed.user.is_some()
        || parsed.password.is_some()
        || parsed.host.is_none()
        || !matches!(
            parsed.scheme,
            gix::url::Scheme::Https | gix::url::Scheme::Ssh
        )
    {
        return Err(Error::Unsupported);
    }
    let _refs = network_fetch_into(repo, &parsed, credential, limits, ctx)?;
    let object =
        crate::revision::verified(repo, expected_oid, limits.max_blob.max(64 * 1024), ctx)?;
    if object.kind != gix::objs::Kind::Commit {
        return Err(Error::Unsupported);
    }
    Ok(())
}

/// Resolve a gitmodules relative URL against the selected superproject remote.
/// Only the path can change. The caller re-observes this effective source
/// alongside the pinned gitlink and registration before publication.
pub(crate) fn resolve_relative_submodule_url(
    repo: &gix::Repository,
    value: &[u8],
) -> Result<Option<Vec<u8>>, Error> {
    if !value.starts_with(b"./") && !value.starts_with(b"../") {
        return Ok(None);
    }
    let relative = std::str::from_utf8(value).map_err(|_| Error::Unsupported)?;
    if relative.len() > 4096
        || relative
            .chars()
            .any(|ch| ch.is_control() || matches!(ch, '\\' | '?' | '#'))
    {
        return Err(Error::Unsupported);
    }
    let name = resolve_remote_name(repo, "", Direction::Fetch)?;
    let remote = repo.find_remote(&name).map_err(|_| Error::Unsupported)?;
    let mut url = remote
        .url(Direction::Fetch)
        .ok_or(Error::Unsupported)?
        .clone();
    if url.scheme == gix::url::Scheme::File {
        let mut path = remote_root(repo, &name, Direction::Fetch)?;
        for component in relative.split('/') {
            match component {
                "." | "" => {}
                ".." if !path.pop() => return Err(Error::Unsupported),
                ".." => {}
                name => {
                    gix::validate::path::component(
                        name.as_bytes().as_bstr(),
                        None,
                        Default::default(),
                    )
                    .map_err(|_| Error::Unsupported)?;
                    path.push(name);
                }
            }
        }
        let path = path.to_str().ok_or(Error::Unsupported)?;
        return Ok(Some(path.as_bytes().to_vec()));
    }
    if !matches!(url.scheme, gix::url::Scheme::Https | gix::url::Scheme::Ssh)
        || url.password().is_some()
        || (url.scheme == gix::url::Scheme::Https && url.user().is_some())
        || url.host().is_none()
    {
        return Err(Error::Unsupported);
    }
    let base = url.path.to_str().map_err(|_| Error::Unsupported)?;
    if base.contains(['?', '#', '\\']) {
        return Err(Error::Unsupported);
    }
    let rooted = base.starts_with('/');
    let mut parts = base
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.iter().any(|part| matches!(*part, "." | "..")) {
        return Err(Error::Unsupported);
    }
    for part in relative.split('/') {
        match part {
            "." | "" => {}
            ".." if parts.pop().is_none() => return Err(Error::Unsupported),
            ".." => {}
            name => parts.push(name),
        }
    }
    if parts.is_empty() {
        return Err(Error::Unsupported);
    }
    let mut path = String::from(if rooted { "/" } else { "" });
    path.push_str(&parts.join("/"));
    url.path = path.into();
    Ok(Some(url.to_bstring().into()))
}

fn network_advertise_into(
    stage: &gix::Repository,
    url: &gix::url::Url,
    credential: Option<&OperationCredential>,
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<(AdvertisedRefs, Option<String>), Error> {
    let remote = stage
        .remote_at_without_url_rewrite(url.to_bstring().to_string())
        .map_err(engine)?
        .with_refspecs(
            [
                "+refs/heads/*:refs/remotes/origin/*",
                "+refs/tags/*:refs/tags/*",
                "HEAD:refs/remotes/origin/HEAD",
            ],
            Direction::Fetch,
        )
        .map_err(engine)?;
    let transport = remote_network::connect_transport(stage, url, false, credential, limits, ctx)?;
    let connection = remote
        .to_connection_with_transport(transport)
        .with_credentials(|_| Ok(None));
    let prepare = connection
        .prepare_fetch(
            gix::progress::Discard,
            gix::remote::ref_map::Options::default(),
        )
        .map_err(engine)?;
    if prepare.ref_map().object_hash != stage.object_hash() {
        return Err(Error::Unsupported);
    }
    advertised_refs(prepare.ref_map(), limits)
}

fn advertised_refs(
    ref_map: &gix::remote::fetch::RefMap,
    limits: Limits,
) -> Result<(BTreeMap<String, gix::ObjectId>, Option<String>), Error> {
    let mut refs = BTreeMap::new();
    let mut default_branch = None;
    if ref_map.remote_refs.len() > limits.max_entries {
        return Err(Error::Limited);
    }
    for reference in &ref_map.remote_refs {
        let (name, oid) = match reference {
            gix::protocol::handshake::Ref::Direct {
                full_ref_name,
                object,
            } => (full_ref_name, *object),
            gix::protocol::handshake::Ref::Peeled {
                full_ref_name, tag, ..
            } => (full_ref_name, *tag),
            gix::protocol::handshake::Ref::Symbolic {
                full_ref_name,
                target,
                ..
            } if full_ref_name.as_bstr().as_bytes() == b"HEAD" => {
                if !target.as_bstr().as_bytes().starts_with(b"refs/heads/") {
                    return Err(Error::Unsupported);
                }
                default_branch = Some(text(target.as_bstr())?);
                continue;
            }
            gix::protocol::handshake::Ref::Symbolic { .. }
            | gix::protocol::handshake::Ref::Unborn { .. } => continue,
        };
        let name = text(name.as_bstr())?;
        if (name == "HEAD" || name.starts_with("refs/heads/") || name.starts_with("refs/tags/"))
            && refs.insert(name, oid).is_some()
        {
            return Err(Error::Unsupported);
        }
    }
    Ok((refs, default_branch))
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
    object_closure_ignoring(source, tips, &BTreeSet::new(), limits, ctx)
}

fn object_closure_ignoring(
    source: &gix::Repository,
    tips: impl IntoIterator<Item = gix::ObjectId>,
    excluded: &BTreeSet<gix::ObjectId>,
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<Vec<(gix::ObjectId, gix::objs::Kind, Vec<u8>)>, Error> {
    let mut todo: VecDeque<_> = tips.into_iter().collect();
    let mut seen = BTreeSet::new();
    let mut objects = Vec::new();
    let mut bytes = 0usize;
    while let Some(id) = todo.pop_front() {
        ctx.check()?;
        if excluded.contains(&id) || !seen.insert(id) {
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

pub(crate) fn copy_commit_objects(
    source: &gix::Repository,
    target: &gix::Repository,
    oid: gix::ObjectId,
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<(), Error> {
    if oid.kind() != source.object_hash() || target.object_hash() != source.object_hash() {
        return Err(Error::Unsupported);
    }
    for (id, kind, bytes) in object_closure(source, [oid], limits, ctx)? {
        ctx.check()?;
        if target
            .write_object(RawObject { kind, bytes })
            .map_err(engine)?
            .detach()
            != id
        {
            return Err(Error::Stale);
        }
    }
    Ok(())
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
        let (remote_repo, remote) = open_clone_source(&remote_path, true, ctx)?;
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
            network: None,
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
    if plan.network.is_some() {
        return Err(Error::Unsupported);
    }
    controlled(plan.limits.timeout, cancelled, |ctx| {
        let mut local_repo = check_identity(&plan.local.root, &plan.local, ctx)?;
        let (remote_repo, actual_remote) = open_clone_source(&plan.remote.root, true, ctx)?;
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
            network: None,
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
    if plan.network.is_some() {
        return Err(Error::Unsupported);
    }
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
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    #[cfg(windows)]
    struct GitHttpFixture {
        url: String,
        stop: std::sync::Arc<AtomicBool>,
        server: Option<std::thread::JoinHandle<()>>,
    }

    #[cfg(windows)]
    impl GitHttpFixture {
        fn start(root: &Path) -> Self {
            use std::net::TcpListener;
            let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
            let url = format!("http://{}/server.git", listener.local_addr().unwrap());
            listener.set_nonblocking(true).unwrap();
            let stop = std::sync::Arc::new(AtomicBool::new(false));
            let signal = stop.clone();
            let root = root.to_owned();
            let server = std::thread::spawn(move || {
                while !signal.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((stream, _)) => serve_git_http(&root, stream),
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => panic!("{error}"),
                    }
                }
            });
            Self {
                url,
                stop,
                server: Some(server),
            }
        }
    }

    #[cfg(windows)]
    impl Drop for GitHttpFixture {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            let result = self.server.take().unwrap().join();
            if !std::thread::panicking() {
                result.unwrap();
            }
        }
    }

    #[cfg(windows)]
    fn serve_git_http(root: &Path, mut stream: std::net::TcpStream) {
        use std::{
            io::{BufRead, BufReader, Read, Write},
            os::windows::process::CommandExt,
            process::{Command, Stdio},
        };
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut reader = BufReader::new(&mut stream);
        let mut request = String::new();
        assert_ne!(reader.read_line(&mut request).unwrap(), 0);
        let mut tokens = request.split_whitespace();
        let method = tokens.next().unwrap();
        let target = tokens.next().unwrap();
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        assert!(path.starts_with("/server.git/"));
        let mut length = 0usize;
        let mut content_type = String::new();
        let mut git_protocol = String::new();
        loop {
            let mut line = String::new();
            assert_ne!(reader.read_line(&mut line).unwrap(), 0);
            if line == "\r\n" {
                break;
            }
            if let Some((name, value)) = line.split_once(':') {
                let value = value.trim();
                if name.eq_ignore_ascii_case("Content-Length") {
                    length = value.parse().unwrap();
                } else if name.eq_ignore_ascii_case("Content-Type") {
                    content_type = value.to_owned();
                } else if name.eq_ignore_ascii_case("Git-Protocol") {
                    git_protocol = value.to_owned();
                }
            }
        }
        assert!(length <= 4 * 1024 * 1024);
        let mut body = vec![0; length];
        reader.read_exact(&mut body).unwrap();
        let git = std::env::var_os("JC_GIT_TEST_EXE").unwrap_or_else(|| "git".into());
        let cgi_root = root
            .to_str()
            .unwrap()
            .trim_start_matches(r"\\?\")
            .replace('\\', "/");
        let mut child = Command::new(git)
            .arg("http-backend")
            .current_dir(root)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "NUL")
            .env("GIT_HTTP_EXPORT_ALL", "1")
            .env("GIT_PROJECT_ROOT", cgi_root)
            .env("REQUEST_METHOD", method)
            .env("PATH_INFO", path)
            .env("QUERY_STRING", query)
            .env("CONTENT_TYPE", content_type)
            .env("CONTENT_LENGTH", length.to_string())
            .env("HTTP_GIT_PROTOCOL", git_protocol)
            .env("REMOTE_USER", "fixture")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .creation_flags(0x08000000)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(&body).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let split = output
            .stdout
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .unwrap();
        let headers = std::str::from_utf8(&output.stdout[..split]).unwrap();
        let status = headers
            .lines()
            .find_map(|line| line.strip_prefix("Status: "))
            .unwrap_or("200 OK");
        assert!(
            status.starts_with("200 "),
            "{method} {path}: {status}, {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let body = &output.stdout[split + 4..];
        write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n",
            body.len()
        )
        .unwrap();
        for header in headers.lines().filter(|line| !line.starts_with("Status: ")) {
            write!(stream, "{header}\r\n").unwrap();
        }
        stream.write_all(b"\r\n").unwrap();
        stream.write_all(body).unwrap();
    }

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
        commit_tree(repo, tree, parent, message)
    }

    fn commit_tree(
        repo: &gix::Repository,
        tree: gix::ObjectId,
        parent: Option<gix::ObjectId>,
        message: &str,
    ) -> gix::ObjectId {
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

    #[cfg(windows)]
    #[test]
    fn worktree_clone_checks_out_exact_bytes_and_tracks_only_default_branch() {
        let fixture = Fixture::new();
        let source = fixture.p("source");
        let mut repo = gix::init(&source).unwrap();
        let bytes = b"uno\r\ndos\xff";
        let blob = repo.write_blob(bytes).unwrap().detach();
        let tree = repo
            .write_object(gix::objs::Tree {
                entries: vec![gix::objs::tree::Entry {
                    mode: gix::objs::tree::EntryKind::Blob.into(),
                    filename: "archivo.txt".into(),
                    oid: blob,
                }],
            })
            .unwrap()
            .detach();
        let tip = commit_tree(&repo, tree, None, "contenido");
        ref_update(
            &mut repo,
            "refs/heads/main",
            tip,
            PreviousValue::MustNotExist,
        );
        ref_update(
            &mut repo,
            "refs/heads/topic",
            tip,
            PreviousValue::MustNotExist,
        );
        head_to(&mut repo, "refs/heads/main");
        std::fs::write(source.join("archivo.txt"), b"edicion sin preparar").unwrap();
        let destination = fixture.p("checkout");
        let plan =
            prepare_local_worktree_clone(&source, &destination, "origin", limits(), || false)
                .unwrap();
        assert_eq!(clone_checkout(&plan), Some((1, bytes.len())));
        assert!(!destination.exists());
        apply_local_worktree_clone(plan, || false).unwrap();
        assert_eq!(
            std::fs::read(destination.join("archivo.txt")).unwrap(),
            bytes
        );
        assert_eq!(
            std::fs::read(source.join("archivo.txt")).unwrap(),
            b"edicion sin preparar"
        );
        let clone = gix::open(&destination).unwrap();
        assert_eq!(clone.head_id().unwrap().detach(), tip);
        assert!(clone.find_reference("refs/heads/topic").is_err());
        assert!(clone.find_reference("refs/remotes/origin/topic").is_ok());
        let index = clone.index().unwrap();
        assert_eq!(
            index.entry_by_path(b"archivo.txt".as_bstr()).unwrap().id,
            blob
        );
        assert_eq!(
            clone
                .branch_remote_tracking_ref_name(
                    safe_ref("refs/heads/main").unwrap().as_ref(),
                    Direction::Fetch,
                )
                .unwrap()
                .unwrap()
                .as_bstr(),
            b"refs/remotes/origin/main".as_bstr(),
        );
    }

    #[cfg(windows)]
    #[test]
    fn worktree_clone_refuses_a_changed_source_or_occupied_destination() {
        let fixture = Fixture::new();
        let source = fixture.p("source.git");
        let (mut repo, before) = bare_with_commit(&source, "refs/heads/main", "antes");
        let destination = fixture.p("checkout");
        let prepare = || {
            prepare_local_worktree_clone(&source, &destination, "origin", limits(), || false)
                .unwrap()
        };
        let stale = prepare();
        let after = commit(&repo, Some(before), "después");
        ref_update(
            &mut repo,
            "refs/heads/main",
            after,
            PreviousValue::MustExistAndMatch(Target::Object(before)),
        );
        assert!(matches!(
            apply_local_worktree_clone(stale, || false),
            Err(Error::Stale)
        ));
        assert!(!destination.exists());
        let occupied = prepare();
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(destination.join("keep"), b"local").unwrap();
        assert!(matches!(
            apply_local_worktree_clone(occupied, || false),
            Err(Error::Stale)
        ));
        assert_eq!(std::fs::read(destination.join("keep")).unwrap(), b"local");
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "publica commits en un repositorio HTTPS de prueba autorizado"]
    fn authenticated_https_clone_fetch_and_push() {
        use std::io::{BufRead, Read};

        let url =
            std::env::var("CIDE_TEST_HTTPS_URL").expect("falta la URL del repositorio de prueba");
        assert!(url.starts_with("https://"), "se requiere HTTPS");
        let mut input = std::io::stdin().lock().take(16 * 1024).lines();
        let mut read = || {
            input
                .next()
                .and_then(Result::ok)
                .filter(|value| !value.is_empty() && !value.chars().any(char::is_control))
                .expect("se requieren dos líneas por stdin: usuario y secreto")
        };
        let credential = OperationCredential {
            username: read(),
            secret: SecretString::new(read()),
            host_fingerprint_pin: None,
            private_key: None,
        };
        verify_remote_branch(&url, "https", &credential);
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "publica commits en un repositorio SSH de prueba autorizado"]
    fn authenticated_ssh_clone_fetch_and_push() {
        use std::io::{BufRead, Read};

        let url = std::env::var("CIDE_TEST_SSH_URL").expect("falta la URL SSH de prueba");
        assert!(url.starts_with("ssh://"), "se requiere SSH");
        let pin = std::env::var("CIDE_TEST_SSH_HOST_PIN").expect("falta la huella SSH verificada");
        let key = PathBuf::from(std::env::var_os("CIDE_TEST_SSH_KEY").expect("falta la clave SSH"));
        assert!(
            key.is_absolute(),
            "se requiere la ruta absoluta de la clave"
        );
        let passphrase = std::io::stdin()
            .lock()
            .take(16 * 1024)
            .lines()
            .next()
            .transpose()
            .expect("no se pudo leer la frase de la clave")
            .unwrap_or_default();
        let credential = OperationCredential {
            username: String::new(),
            secret: SecretString::new(passphrase),
            host_fingerprint_pin: Some(pin),
            private_key: Some(key),
        };
        verify_remote_branch(&url, "ssh", &credential);
    }

    #[cfg(windows)]
    fn verify_remote_branch(url: &str, protocol: &str, credential: &OperationCredential) {
        let variable = format!("CIDE_TEST_{}_BRANCH", protocol.to_ascii_uppercase());
        let short = std::env::var(variable).expect("falta la rama de prueba");
        let prefix = format!("cide-{protocol}-");
        assert!(short.starts_with(&prefix), "usa una rama nueva {prefix}*");
        let branch = format!("refs/heads/{short}");
        safe_ref(&branch).expect("rama de prueba inválida");

        let fixture = Fixture::new();
        let limits = Limits {
            timeout: Duration::from_secs(30),
            ..limits()
        };
        let clone = |name| {
            let destination = fixture.p(name);
            let plan = prepare_network_worktree_clone(url, &destination, limits, || false).unwrap();
            let result = apply_network_worktree_clone(plan, Some(credential), || false)
                .expect("falló el clon autenticado");
            assert!(
                result.files > 0,
                "el repositorio de prueba debe contener un archivo"
            );
            let mut repo = gix::open(&destination).unwrap();
            assert!(
                repo.head_tree()
                    .unwrap()
                    .lookup_entry_by_path(".github/workflows")
                    .unwrap()
                    .is_none(),
                "el repositorio de prueba no puede contener workflows"
            );
            assert_ne!(
                repo.head_name().unwrap().unwrap().as_bstr(),
                branch.as_bytes(),
                "la rama de prueba no puede ser la predeterminada"
            );
            let original = repo.head_id().unwrap().detach();
            ref_update(&mut repo, &branch, original, PreviousValue::MustNotExist);
            set_clone_head(&repo, &CloneHead::Branch(branch.clone())).unwrap();
            write_clone_config(&repo, url, "cide-proof", Some(&branch)).unwrap();
            gix::open(destination).unwrap()
        };
        let mut writer = clone("writer");
        let mut reader = clone("reader");
        let original = writer.head_id().unwrap().detach();
        assert_eq!(reader.head_id().unwrap().detach(), original);

        let push = |repo: &gix::Repository, old: Option<gix::ObjectId>, new: gix::ObjectId| {
            let plan = prepare_network_push(
                repo.workdir().unwrap(),
                "cide-proof",
                Some(credential),
                limits,
                || false,
            )
            .expect("falló la vista previa de envío autenticado");
            let [update] = push_preview(&plan).updates.as_slice() else {
                panic!("se esperaba actualizar exactamente la rama de prueba");
            };
            assert_eq!(update.destination, branch);
            assert_eq!(update.old, old.map(|id| id.to_string()));
            assert_eq!(update.new, new.to_string());
            let sent = apply_network_push(plan, Some(credential), || false)
                .expect("falló el envío autenticado");
            assert_eq!(sent.updated, [branch.as_str()]);
        };
        let advance = |repo: &mut gix::Repository, base: gix::ObjectId, message: &str| {
            let tree = repo.find_commit(base).unwrap().tree_id().unwrap().detach();
            let current = repo.head_id().unwrap().detach();
            let next = commit_tree(repo, tree, Some(base), message);
            ref_update(
                repo,
                &branch,
                next,
                PreviousValue::MustExistAndMatch(Target::Object(current)),
            );
            next
        };
        let fetch = |repo: &gix::Repository, expected: gix::ObjectId| {
            let workdir = repo.workdir().unwrap();
            let plan =
                prepare_network_fetch(workdir, "cide-proof", Some(credential), limits, || false)
                    .expect("falló la vista previa de obtención autenticada");
            let tracking = format!("refs/remotes/cide-proof/{short}");
            assert!(
                fetch_preview(&plan)
                    .updates
                    .iter()
                    .any(|update| { update.destination == tracking && update.new == expected })
            );
            apply_network_fetch(plan, Some(credential), || false)
                .expect("falló la obtención autenticada");
            assert_eq!(
                repo.find_reference(&tracking).unwrap().id().detach(),
                expected
            );
            assert!(repo.has_object(expected));
        };
        let first = advance(
            &mut writer,
            original,
            &format!("Prueba CIDE de envío {protocol}"),
        );
        assert!(
            prepare_network_push(
                writer.workdir().unwrap(),
                "cide-proof",
                None,
                limits,
                || false
            )
            .is_err(),
            "el servidor de prueba debe exigir credenciales para el envío"
        );
        push(&writer, None, first);
        fetch(&reader, first);
        let second = advance(
            &mut reader,
            first,
            &format!("Prueba CIDE de obtención y envío {protocol}"),
        );
        push(&reader, Some(first), second);
        fetch(&writer, second);
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "consulta el repositorio público RAG en GitHub"]
    fn rag_worktree_lifecycle() {
        use crate::worktrees;

        let fixture = Fixture::new();
        let root = fixture.p("clone");
        let limits = rag_limits();
        let clone = prepare_network_worktree_clone(RAG_URL, &root, limits, || false).unwrap();
        apply_network_worktree_clone(clone, None, || false).unwrap();
        let mut repo = gix::open(&root).unwrap();
        let original_head = repo.head_id().unwrap().detach();
        let original_name = repo.head_name().unwrap();
        let branch = "refs/heads/cide-rag-worktree";
        let target = gix::ObjectId::from_hex(RAG_UPDATED.as_bytes()).unwrap();
        ref_update(&mut repo, branch, target, PreviousValue::MustNotExist);
        drop(repo);

        let first = fixture.p("worktree-initial");
        let moved = fixture.p("worktree-moved");
        let review = worktrees::prepare_create(&root, &first, branch, limits, || false).unwrap();
        assert_eq!(review.commit(), RAG_UPDATED);
        let created = worktrees::apply_create(review, || false).unwrap();
        assert_rag_license(&first);
        std::fs::write(first.join("local.txt"), b"keep local bytes\n").unwrap();

        let review = worktrees::prepare_move(&root, &created.id, &moved, limits, || false).unwrap();
        let relocated = worktrees::apply_move(review, || false).unwrap();
        assert_eq!(relocated.path, moved);
        assert!(!first.exists());
        assert_rag_license(&moved);
        let review = worktrees::prepare_retire(&root, &created.id, limits, || false).unwrap();
        let retired = worktrees::apply_retire(review, || false).unwrap();
        assert!(retired.preserved_files && !moved.exists());
        assert_eq!(
            std::fs::read(retired.worktree.join("local.txt")).unwrap(),
            b"keep local bytes\n"
        );
        let review =
            worktrees::prepare_restore_retired(&root, &created.id, limits, || false).unwrap();
        let restored = worktrees::apply_restore_retired(review, || false).unwrap();
        assert_eq!(restored.path, moved);
        assert_eq!(restored.head.as_deref(), Some(RAG_UPDATED));
        assert_eq!(restored.branch.as_deref(), Some(branch));
        assert_rag_license(&moved);
        assert_eq!(
            std::fs::read(moved.join("local.txt")).unwrap(),
            b"keep local bytes\n"
        );
        let repo = gix::open(&root).unwrap();
        assert_eq!(repo.head_id().unwrap().detach(), original_head);
        assert_eq!(repo.head_name().unwrap(), original_name);
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "consulta el repositorio público RAG en GitHub"]
    fn rag_submodule_initialize_and_update() {
        use crate::submodules;

        let fixture = Fixture::new();
        let root = fixture.p("superproject");
        std::fs::create_dir(&root).unwrap();
        let mut repo = gix::init(&root).unwrap();
        let branch = "refs/heads/cide-rag-superproject";
        head_to(&mut repo, branch);
        let modules = format!("[submodule \"rag\"]\n\tpath = rag\n\turl = {RAG_URL}\n");
        std::fs::write(root.join(".gitmodules"), &modules).unwrap();
        let registration = repo.write_blob(modules.as_bytes()).unwrap().detach();
        let limits = rag_limits();
        let child = root.join("rag");
        let mut parent = None;
        for (previous, target) in [(None, RAG_INITIAL), (Some(RAG_INITIAL), RAG_UPDATED)] {
            let gitlink = gix::ObjectId::from_hex(target.as_bytes()).unwrap();
            let tree = repo
                .write_object(gix::objs::Tree {
                    entries: vec![
                        gix::objs::tree::Entry {
                            mode: gix::objs::tree::EntryKind::Blob.into(),
                            filename: ".gitmodules".into(),
                            oid: registration,
                        },
                        gix::objs::tree::Entry {
                            mode: gix::objs::tree::EntryKind::Commit.into(),
                            filename: "rag".into(),
                            oid: gitlink,
                        },
                    ],
                })
                .unwrap()
                .detach();
            let next = commit_tree(&repo, tree, parent, "Gitlink de prueba para RAG");
            let expected = parent.map_or(PreviousValue::MustNotExist, |oid| {
                PreviousValue::MustExistAndMatch(Target::Object(oid))
            });
            ref_update(&mut repo, branch, next, expected);
            repo.index_from_tree(&tree)
                .unwrap()
                .write(Default::default())
                .unwrap();
            if let Some(previous) = previous {
                let review =
                    submodules::prepare_update(&root, b"rag", previous, target, limits, || false)
                        .unwrap();
                assert_eq!(review.preview().previous_commit, previous);
                assert_eq!(review.preview().target_commit, target);
                submodules::apply_update(review, || false).unwrap();
            } else {
                let review =
                    submodules::prepare_init(&root, b"rag", target, limits, || false).unwrap();
                assert_eq!(review.preview().commit, target);
                submodules::apply_init(review, || false).unwrap();
            }
            let nested = gix::open(&child).unwrap();
            assert!(nested.head_name().unwrap().is_none());
            assert_eq!(nested.head_id().unwrap().detach(), gitlink);
            assert_rag_license(&child);
            assert!(!root.join(".git/cide-submodule.pending").exists());
            parent = Some(next);
        }
    }

    #[cfg(windows)]
    const RAG_URL: &str = "https://github.com/juancgomezm-s/RAG.git";
    #[cfg(windows)]
    const RAG_INITIAL: &str = "4fa213b32f96b309d4911a0d74a198b81db7b994";
    #[cfg(windows)]
    const RAG_UPDATED: &str = "513d25bf2635713fd26e373f9cab73d3959bb665";

    #[cfg(windows)]
    fn rag_limits() -> Limits {
        Limits {
            timeout: Duration::from_secs(30),
            ..limits()
        }
    }

    #[cfg(windows)]
    fn assert_rag_license(root: &Path) {
        let bytes = std::fs::read(root.join("LICENSE")).unwrap();
        let blob =
            gix::objs::compute_hash(gix::hash::Kind::Sha1, gix::objs::Kind::Blob, &bytes).unwrap();
        assert_eq!(blob.to_string(), "d159169d1050894d3ea3b98e1c965c4058208fe1");
    }

    #[cfg(windows)]
    #[test]
    fn smart_http_clone_fetch_and_push_in_private_repositories() {
        let fixture = Fixture::new();
        let server_path = fixture.p("server.git");
        let mut server = gix::init_bare(&server_path).unwrap();
        let blob = server.write_blob(b"Contenido de red\r\n").unwrap().detach();
        let tree = server
            .write_object(gix::objs::Tree {
                entries: vec![gix::objs::tree::Entry {
                    mode: gix::objs::tree::EntryKind::Blob.into(),
                    filename: "red.txt".into(),
                    oid: blob,
                }],
            })
            .unwrap()
            .detach();
        let first = commit_tree(&server, tree, None, "base");
        ref_update(
            &mut server,
            "refs/heads/main",
            first,
            PreviousValue::MustNotExist,
        );
        head_to(&mut server, "refs/heads/main");
        let http = GitHttpFixture::start(&fixture.0);
        let destination = fixture.p("clone");
        let limits = Limits {
            timeout: Duration::from_secs(15),
            ..limits()
        };
        let plan =
            prepare_network_worktree_clone(&http.url, &destination, limits, || false).unwrap();
        assert!(!destination.exists());
        let clone = apply_network_worktree_clone(plan, None, || false).unwrap();
        assert_eq!(clone.files, 1);
        assert_eq!(
            std::fs::read(destination.join("red.txt")).unwrap(),
            b"Contenido de red\r\n"
        );
        let mut local = gix::open(&destination).unwrap();
        assert_eq!(local.head_id().unwrap().detach(), first);
        let second = commit_tree(&server, tree, Some(first), "avance remoto");
        ref_update(
            &mut server,
            "refs/heads/main",
            second,
            PreviousValue::MustExistAndMatch(Target::Object(first)),
        );
        let plan = prepare_network_fetch(&destination, "origin", None, limits, || false).unwrap();
        assert_eq!(fetch_preview(&plan).updates[0].new, second.to_string());
        apply_network_fetch(plan, None, || false).unwrap();
        assert_eq!(
            local
                .find_reference("refs/remotes/origin/main")
                .unwrap()
                .target()
                .try_id(),
            Some(second.as_ref()),
        );
        let third = commit_tree(&local, tree, Some(second), "avance local");
        ref_update(
            &mut local,
            "refs/heads/main",
            third,
            PreviousValue::MustExistAndMatch(Target::Object(first)),
        );
        let push = prepare_network_push(&destination, "origin", None, limits, || false).unwrap();
        assert_eq!(push_preview(&push).object_count, 1);
        apply_network_push(push, None, || false).unwrap();
        let after = gix::open(&server_path).unwrap();
        assert_eq!(after.head_id().unwrap().detach(), third);
        assert!(after.has_object(third));
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
