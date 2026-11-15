//! Bounded submodule inventory and reviewed, local-only initialization/update.
//!
//! Network fetching is composed by the remote transport. Paths, gitlink OIDs,
//! and `.gitmodules` are always pinned to the superproject commit selected when
//! a plan is prepared. No operation shells out to Git.

use crate::{Context, Error, Identity, Limits, controlled, open};
use gix::bstr::ByteSlice;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

const GITMODULES_BYTES: usize = 256 * 1024;
const MAX_DEPTH: usize = 128;

#[derive(Debug, Clone)]
pub struct Submodule {
    /// Raw Git path bytes. This may not be valid UTF-8.
    pub path: Vec<u8>,
    pub object_id: String,
    /// The subsection name in `.gitmodules`, if the commit has a registration.
    pub name: Option<Vec<u8>>,
    /// URL from the selected commit's `.gitmodules`; never fetched automatically.
    pub url: Option<Vec<u8>>,
    pub registered: bool,
}

#[derive(Clone)]
pub struct Inventory {
    pub repository: Identity,
    pub commit: Option<String>,
    pub submodules: Vec<Submodule>,
}

struct Scan {
    entries: usize,
    max_entries: usize,
    max_path: usize,
    gitlinks: BTreeMap<Vec<u8>, String>,
    gitmodules: Option<Vec<u8>>,
}

type Registration = (Vec<u8>, Option<Vec<u8>>);
type Registrations = BTreeMap<Vec<u8>, Registration>;

const INIT_JOURNAL: &str = "cide-submodule.pending";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitPreview {
    pub operation_id: String,
    pub path: String,
    pub name: String,
    pub source: String,
    pub commit: String,
    pub files: usize,
    pub bytes: usize,
    pub replacing_empty_checkout_placeholder: bool,
}

/// Reviewed initialization. Dropping a plan never writes to the repository.
pub struct InitPlan {
    repository: Identity,
    limits: Limits,
    path: Vec<u8>,
    name: Vec<u8>,
    source: Source,
    source_stage: Option<PathBuf>,
    commit: gix::ObjectId,
    parent_commit: gix::ObjectId,
    index: crate::local_pair::IndexToken,
    worktree_target: WorktreeTarget,
    module_missing: jc_fs::MissingToken,
    preview: InitPreview,
    nonce: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdatePreview {
    pub operation_id: String,
    pub path: String,
    pub name: String,
    pub source: String,
    pub previous_commit: String,
    pub target_commit: String,
    pub files: usize,
    pub bytes: usize,
}

pub struct UpdatePlan {
    repository: Identity,
    nested_repository: Identity,
    limits: Limits,
    path: Vec<u8>,
    name: Vec<u8>,
    source: Source,
    source_stage: Option<PathBuf>,
    parent_commit: gix::ObjectId,
    previous_commit: gix::ObjectId,
    target_commit: gix::ObjectId,
    parent_index: crate::local_pair::IndexToken,
    nested_index: crate::local_pair::IndexToken,
    preview: UpdatePreview,
    nonce: String,
}

impl UpdatePlan {
    pub fn preview(&self) -> &UpdatePreview {
        &self.preview
    }
}

impl Drop for InitPlan {
    fn drop(&mut self) {
        if let Some(path) = self.source_stage.take() {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

impl Drop for UpdatePlan {
    fn drop(&mut self) {
        if let Some(path) = self.source_stage.take() {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

#[derive(Clone)]
enum WorktreeTarget {
    Missing(jc_fs::MissingToken),
    Placeholder(jc_fs::MissingToken),
}

impl InitPlan {
    pub fn preview(&self) -> &InitPreview {
        &self.preview
    }
}

/// A source admitted from `.gitmodules`. Relative URLs are intentionally not
/// guessed: Git resolves them against a configured superproject remote, which
/// is a separate pinned input and is added by the transport integration.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Source {
    Local(PathBuf),
    Network {
        url: String,
        transport: String,
        host: String,
    },
}

fn source_from_url(url: &[u8]) -> Result<Source, Error> {
    let url = std::str::from_utf8(url).map_err(|_| Error::Unsupported)?;
    if url.is_empty() || url.len() > 4096 || url.chars().any(char::is_control) || url.contains('#')
    {
        return Err(Error::Unsupported);
    }
    // Detect native absolute paths before parsing URL schemes: `C:/repo` can
    // otherwise be interpreted as SCP syntax by Git URL parsers.
    let url = url
        .strip_prefix("//?/")
        .filter(|rest| {
            rest.len() >= 3
                && rest.as_bytes()[0].is_ascii_alphabetic()
                && rest.as_bytes()[1] == b':'
                && rest.as_bytes()[2] == b'/'
        })
        .unwrap_or(url);
    let local = if let Some(file) = url.strip_prefix("file:///") {
        #[cfg(windows)]
        let file = file
            .strip_prefix('/')
            .filter(|rest| rest.as_bytes().get(1) == Some(&b':'))
            .unwrap_or(file);
        Some(PathBuf::from(
            file.replace('/', std::path::MAIN_SEPARATOR_STR),
        ))
    } else if url.starts_with("file://") {
        return Err(Error::Unsupported);
    } else {
        #[cfg(windows)]
        let drive_path = url.len() >= 3
            && url.as_bytes()[0].is_ascii_alphabetic()
            && url.as_bytes()[1] == b':'
            && matches!(url.as_bytes()[2], b'/' | b'\\');
        #[cfg(not(windows))]
        let drive_path = false;
        let path = PathBuf::from(url);
        if path.is_absolute() || drive_path {
            Some(path)
        } else {
            None
        }
    };
    if let Some(path) = local {
        let canonical = path.canonicalize()?;
        crate::index_write::ordinary(&canonical, true)?;
        if !canonical.is_dir() {
            return Err(Error::Unsupported);
        }
        return Ok(Source::Local(canonical));
    }
    if url.contains('?') {
        return Err(Error::Unsupported);
    }
    let parsed = gix::url::parse(url.as_bytes().as_bstr()).map_err(|_| Error::Unsupported)?;
    if parsed.user.is_some() || parsed.password.is_some() {
        return Err(Error::Unsupported);
    }
    let transport = match parsed.scheme {
        gix::url::Scheme::Https => "HTTPS",
        gix::url::Scheme::Ssh => "SSH",
        _ => return Err(Error::Unsupported),
    };
    let host = parsed.host.as_deref().ok_or(Error::Unsupported)?;
    Ok(Source::Network {
        url: url.to_owned(),
        transport: transport.to_owned(),
        host: host.to_owned(),
    })
}

fn source_label(source: &Source) -> Result<String, Error> {
    match source {
        Source::Local(path) => path_text(path),
        Source::Network {
            transport, host, ..
        } => Ok(format!("{transport} · {host}")),
    }
}

fn source_for_registration(repo: &gix::Repository, url: &[u8]) -> Result<Source, Error> {
    let resolved = crate::remote::resolve_relative_submodule_url(repo, url)?;
    source_from_url(resolved.as_deref().unwrap_or(url))
}

fn stage_error(stage: &str, error: Error) -> Error {
    Error::Engine(format!("Submódulo · {stage}: {error}"))
}

fn prepare_source_repo(
    source: &Source,
    commit: gix::ObjectId,
    stage: &Path,
    credential: Option<&crate::remote::OperationCredential>,
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<(gix::Repository, Option<PathBuf>), Error> {
    match source {
        Source::Local(path) => Ok((crate::remote::open_clone_source(path, true, ctx)?.0, None)),
        Source::Network { url, .. } => {
            if crate::index_write::exists(stage)? {
                return Err(Error::Stale);
            }
            let parent = stage.parent().ok_or(Error::InvalidRoot)?;
            match std::fs::symlink_metadata(parent) {
                Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
                Ok(_) => return Err(Error::Unsupported),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    std::fs::create_dir(parent)?
                }
                Err(error) => return Err(error.into()),
            }
            let repo = gix::init_bare(stage).map_err(crate::engine)?;
            let result =
                crate::remote::fetch_submodule_url(&repo, url, commit, credential, limits, ctx);
            if let Err(error) = result {
                let mut budget = 0;
                let _ = remove_owned_tree(stage, &mut budget, limits.max_entries);
                return Err(error);
            }
            Ok((repo, Some(stage.to_path_buf())))
        }
    }
}

fn open_source_repo(
    source: &Source,
    staged: Option<&Path>,
    ctx: &Context<'_>,
) -> Result<gix::Repository, Error> {
    if let Some(path) = staged {
        return Ok(crate::remote::open_clone_source(path, false, ctx)?.0);
    }
    match source {
        Source::Local(path) => Ok(crate::remote::open_clone_source(path, true, ctx)?.0),
        Source::Network { .. } => Err(Error::Stale),
    }
}

fn module_storage_name(name: &[u8], ctx: &Context<'_>) -> Result<String, Error> {
    if name.is_empty() || name.len() > 4096 || name.contains(&0) {
        return Err(Error::Unsupported);
    }
    // A digest prevents a subsection name such as `../../hooks` from ever
    // becoming a filesystem path. Keep the readable prefix short and bounded.
    let digest = crate::local_pair::sha256(name, ctx)?;
    let prefix: String = name
        .iter()
        .take(20)
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"._-".contains(byte) {
                *byte as char
            } else {
                '_'
            }
        })
        .collect();
    Ok(format!("{prefix}-{}", hex(&digest)))
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(DIGITS[(byte >> 4) as usize] as char);
        output.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    output
}

fn unhex(text: &str) -> Result<Vec<u8>, Error> {
    if text.len() % 2 != 0 || text.len() > 8192 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::Unsupported);
    }
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let hi = (pair[0] as char).to_digit(16).ok_or(Error::Unsupported)?;
            let lo = (pair[1] as char).to_digit(16).ok_or(Error::Unsupported)?;
            Ok(((hi << 4) | lo) as u8)
        })
        .collect()
}

fn worktree_submodule_path(root: &Path, raw: &[u8]) -> Result<PathBuf, Error> {
    let text = std::str::from_utf8(raw).map_err(|_| Error::Unsupported)?;
    crate::revision::path_valid(raw, 4096)?;
    let mut result = root.to_path_buf();
    for component in text.split('/') {
        if component.is_empty()
            || component == "."
            || component == ".."
            || component.contains('~')
            || component.eq_ignore_ascii_case(".git")
        {
            return Err(Error::Unsupported);
        }
        gix::validate::path::component(component.as_bytes().as_bstr(), None, Default::default())
            .map_err(crate::engine)?;
        result.push(component);
    }
    Ok(result)
}

fn timestamp_nonce() -> Result<String, Error> {
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(crate::engine)?
        .as_nanos();
    Ok(format!("{:x}{time:x}", std::process::id()))
}

fn path_text(path: &Path) -> Result<String, Error> {
    let path = path.to_str().ok_or(Error::Unsupported)?;
    if path.chars().any(char::is_control) {
        return Err(Error::Unsupported);
    }
    Ok(path.to_owned())
}

fn capture_missing(
    root: &Path,
    path: &Path,
    ctx: &Context<'_>,
) -> Result<jc_fs::MissingToken, Error> {
    match crate::local_pair::capture(root, path, 0, ctx)? {
        jc_fs::Capture::Absent { token } => Ok(token),
        jc_fs::Capture::Present { .. } => Err(Error::Stale),
    }
}

fn worktree_target(root: &Path, path: &Path, ctx: &Context<'_>) -> Result<WorktreeTarget, Error> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(WorktreeTarget::Missing(capture_missing(root, path, ctx)?))
        }
        Err(error) => Err(error.into()),
        Ok(metadata) => {
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err(Error::Stale);
            }
            if std::fs::read_dir(path)?.next().transpose()?.is_some() {
                return Err(Error::Stale);
            }
            // CheckoutTree creates only the directories for a gitlink by
            // asking for the absence of this sentinel path. It does not write
            // the sentinel file itself. The missing token pins the empty
            // directory's parent identity across the publication.
            Ok(WorktreeTarget::Placeholder(capture_missing(
                root,
                &path.join(".cide-uninitialized"),
                ctx,
            )?))
        }
    }
}

fn target_matches(
    root: &Path,
    path: &Path,
    expected: &WorktreeTarget,
    ctx: &Context<'_>,
) -> Result<bool, Error> {
    Ok(match (expected, worktree_target(root, path, ctx)?) {
        (WorktreeTarget::Missing(a), WorktreeTarget::Missing(b)) => a == &b,
        (WorktreeTarget::Placeholder(a), WorktreeTarget::Placeholder(b)) => a == &b,
        _ => false,
    })
}

fn target_same_location(
    root: &Path,
    path: &Path,
    expected: &WorktreeTarget,
    ctx: &Context<'_>,
) -> Result<bool, Error> {
    Ok(match (expected, worktree_target(root, path, ctx)?) {
        (WorktreeTarget::Missing(a), WorktreeTarget::Missing(b)) => a.same_location_as(&b),
        (WorktreeTarget::Placeholder(a), WorktreeTarget::Placeholder(b)) => a.same_location_as(&b),
        _ => false,
    })
}

fn selected_submodule(
    root: &Path,
    path: &[u8],
    expected_oid: &str,
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<(gix::Repository, Identity, gix::ObjectId, Submodule), Error> {
    let (repo, identity) = open(root, ctx)?;
    let inventory = list_inner(root, limits, ctx)?;
    let parent_commit = inventory.commit.as_deref().ok_or(Error::Missing)?;
    let parent_commit = gix::ObjectId::from_hex(parent_commit.as_bytes()).map_err(crate::engine)?;
    let entry = inventory
        .submodules
        .into_iter()
        .find(|entry| entry.path == path)
        .ok_or(Error::Stale)?;
    if !entry.registered
        || entry.name.is_none()
        || entry.url.is_none()
        || (!expected_oid.is_empty() && entry.object_id != expected_oid)
    {
        return Err(Error::Stale);
    }
    Ok((repo, identity, parent_commit, entry))
}

fn init_inner(
    root: &Path,
    path: &[u8],
    expected_oid: &str,
    credential: Option<&crate::remote::OperationCredential>,
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<InitPlan, Error> {
    limits.admit()?;
    if !cfg!(windows) {
        return Err(Error::Unsupported);
    }
    let (repo, repository, parent_commit, submodule) =
        selected_submodule(root, path, expected_oid, limits, ctx)
            .map_err(|error| stage_error("validar gitlink y registro", error))?;
    let name = submodule.name.ok_or(Error::Unsupported)?;
    let url = submodule.url.ok_or(Error::Unsupported)?;
    let source = source_for_registration(&repo, &url)
        .map_err(|error| stage_error("resolver la URL del submódulo", error))?;
    let commit = gix::ObjectId::from_hex(submodule.object_id.as_bytes()).map_err(crate::engine)?;
    let nonce = timestamp_nonce()?;
    let source_stage_path = repository
        .git_dir
        .join("modules")
        .join(format!(".cide-fetch-{nonce}"));
    let (source_repo, source_stage) =
        prepare_source_repo(&source, commit, &source_stage_path, credential, limits, ctx)
            .map_err(|error| stage_error("obtener el commit de origen", error))?;
    let checkout = crate::worktree_materialize::prepare(&source_repo, commit, limits, ctx)
        .map_err(|error| stage_error("validar el árbol del commit", error))?;
    let worktree = worktree_submodule_path(&repository.root, path)?;
    let modules = repository.git_dir.join("modules");
    match std::fs::symlink_metadata(&modules) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
        Ok(_) => return Err(Error::Unsupported),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let module_name = module_storage_name(&name, ctx)?;
    let module_dir = modules.join(module_name);
    let worktree_target = worktree_target(&repository.root, &worktree, ctx)?;
    let module_missing = capture_missing(&repository.git_dir, &module_dir, ctx)?;
    let (index, _) = crate::local_pair::index_capture(&repo, &repository.git_dir, ctx)?;
    let name_text = std::str::from_utf8(&name)
        .map_err(|_| Error::Unsupported)?
        .to_owned();
    let display_path = std::str::from_utf8(path)
        .map_err(|_| Error::Unsupported)?
        .to_owned();
    let source_label = source_label(&source)?;
    Ok(InitPlan {
        repository,
        limits,
        path: path.to_vec(),
        name,
        source,
        source_stage,
        commit,
        parent_commit,
        index,
        worktree_target: worktree_target.clone(),
        module_missing,
        preview: InitPreview {
            operation_id: nonce.clone(),
            path: display_path,
            name: name_text,
            source: source_label,
            commit: commit.to_string(),
            files: checkout.file_count(),
            bytes: checkout.byte_count(),
            replacing_empty_checkout_placeholder: matches!(
                worktree_target,
                WorktreeTarget::Placeholder(_)
            ),
        },
        nonce,
    })
}

/// Prepare initialization from the exact URL and gitlink recorded in the
/// committed superproject. The current transport admits absolute local paths.
pub fn prepare_init(
    root: &Path,
    path: &[u8],
    expected_oid: &str,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<InitPlan, Error> {
    prepare_init_with_credentials(root, path, expected_oid, None, limits, cancelled)
}

pub fn prepare_init_with_credentials(
    root: &Path,
    path: &[u8],
    expected_oid: &str,
    credential: Option<&crate::remote::OperationCredential>,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<InitPlan, Error> {
    controlled(limits.timeout, cancelled, |ctx| {
        init_inner(root, path, expected_oid, credential, limits, ctx)
    })
}

fn require_clean(root: &Path, limits: Limits, ctx: &Context<'_>) -> Result<(), Error> {
    let status = crate::read_status(root, limits, || ctx.check().is_err())?;
    if status.entries.iter().any(|entry| {
        entry.index != crate::Change::Unchanged
            || entry.worktree != crate::Change::Unchanged
            || entry.kind != crate::Kind::Tracked
    }) {
        return Err(Error::Engine(
            "El submódulo tiene cambios locales; guárdalos antes de actualizar".into(),
        ));
    }
    verify_no_ignored_or_untracked(root, limits, ctx)?;
    Ok(())
}

struct TrackedTree<'a, 'ctx> {
    files: &'a std::collections::BTreeSet<String>,
    directories: &'a std::collections::BTreeSet<String>,
    count: usize,
    bytes: usize,
    limits: Limits,
    ctx: &'a Context<'ctx>,
}

impl TrackedTree<'_, '_> {
    fn walk(&mut self, directory: &Path, relative: &str) -> Result<(), Error> {
        for entry in std::fs::read_dir(directory)? {
            self.ctx.check()?;
            let entry = entry?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| Error::Unsupported)?;
            if relative.is_empty() && name == ".git" {
                crate::index_write::ordinary(&entry.path(), false)?;
                continue;
            }
            self.count = self.count.checked_add(1).ok_or(Error::Limited)?;
            if self.count > self.limits.max_entries {
                return Err(Error::Limited);
            }
            let path = if relative.is_empty() {
                name
            } else {
                format!("{relative}/{name}")
            };
            let absolute = entry.path();
            let metadata = std::fs::symlink_metadata(&absolute)?;
            if metadata.is_dir() {
                crate::index_write::ordinary(&absolute, true)?;
                if !self.directories.contains(&path) {
                    return Err(Error::Stale);
                }
                self.walk(&absolute, &path)?;
            } else if metadata.is_file() {
                crate::index_write::ordinary(&absolute, false)?;
                if !self.files.contains(&path) {
                    return Err(Error::Stale);
                }
                self.bytes = self
                    .bytes
                    .checked_add(usize::try_from(metadata.len()).map_err(|_| Error::Limited)?)
                    .ok_or(Error::Limited)?;
                if self.bytes > self.limits.max_bytes {
                    return Err(Error::Limited);
                }
            } else {
                return Err(Error::Unsupported);
            }
        }
        Ok(())
    }
}

fn update_inner(
    root: &Path,
    path: &[u8],
    expected_current: &str,
    expected_target: &str,
    credential: Option<&crate::remote::OperationCredential>,
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<UpdatePlan, Error> {
    limits.admit()?;
    if !cfg!(windows) {
        return Err(Error::Unsupported);
    }
    let (parent_repo, repository, parent_commit, submodule) =
        selected_submodule(root, path, expected_target, limits, ctx)?;
    if crate::index_write::exists(&repository.git_dir.join(INIT_JOURNAL))? {
        return Err(Error::Engine(
            "Hay una inicialización/actualización de submódulo pendiente; reanúdala primero".into(),
        ));
    }
    let name = submodule.name.ok_or(Error::Unsupported)?;
    let source = source_for_registration(
        &parent_repo,
        submodule.url.as_deref().ok_or(Error::Unsupported)?,
    )?;
    let target_commit =
        gix::ObjectId::from_hex(submodule.object_id.as_bytes()).map_err(crate::engine)?;
    let worktree = worktree_submodule_path(&repository.root, path)?;
    let (nested_repo, nested_repository) = open(&worktree, ctx)?;
    let previous_commit = if expected_current.is_empty() {
        nested_repo.head_id().map_err(crate::engine)?.detach()
    } else {
        gix::ObjectId::from_hex(expected_current.as_bytes()).map_err(crate::engine)?
    };
    let module_dir = repository
        .git_dir
        .join("modules")
        .join(module_storage_name(&name, ctx)?)
        .canonicalize()?;
    if nested_repository.git_dir.canonicalize()? != module_dir
        || nested_repo.head_name().map_err(crate::engine)?.is_some()
        || nested_repo.head_id().map_err(crate::engine)?.detach() != previous_commit
    {
        return Err(Error::Stale);
    }
    if previous_commit == target_commit {
        return Err(Error::Engine(
            "El submódulo ya está en el commit requerido".into(),
        ));
    }
    require_clean(&worktree, limits, ctx)?;
    let (parent_index, _) =
        crate::local_pair::index_capture(&parent_repo, &repository.git_dir, ctx)?;
    let (nested_index, _) =
        crate::local_pair::index_capture(&nested_repo, &nested_repository.git_dir, ctx)?;
    let nonce = timestamp_nonce()?;
    let source_stage_path = module_dir
        .parent()
        .ok_or(Error::InvalidRoot)?
        .join(format!(".cide-fetch-{nonce}"));
    let (source_repo, source_stage) = prepare_source_repo(
        &source,
        target_commit,
        &source_stage_path,
        credential,
        limits,
        ctx,
    )?;
    let checkout = crate::worktree_materialize::prepare(&source_repo, target_commit, limits, ctx)?;
    Ok(UpdatePlan {
        repository,
        nested_repository,
        limits,
        path: path.to_vec(),
        name: name.clone(),
        source: source.clone(),
        source_stage,
        parent_commit,
        previous_commit,
        target_commit,
        parent_index,
        nested_index,
        preview: UpdatePreview {
            operation_id: nonce.clone(),
            path: std::str::from_utf8(path)
                .map_err(|_| Error::Unsupported)?
                .to_owned(),
            name: std::str::from_utf8(&name)
                .map_err(|_| Error::Unsupported)?
                .to_owned(),
            source: source_label(&source)?,
            previous_commit: previous_commit.to_string(),
            target_commit: target_commit.to_string(),
            files: checkout.file_count(),
            bytes: checkout.byte_count(),
        },
        nonce,
    })
}

/// Prepare update to the current superproject's exact gitlink. The module must
/// be initialized, detached at `expected_current`, and have a completely clean
/// index/worktree (including untracked files).
pub fn prepare_update(
    root: &Path,
    path: &[u8],
    expected_current: &str,
    expected_target: &str,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<UpdatePlan, Error> {
    prepare_update_with_credentials(
        root,
        path,
        expected_current,
        expected_target,
        None,
        limits,
        cancelled,
    )
}

pub fn prepare_update_with_credentials(
    root: &Path,
    path: &[u8],
    expected_current: &str,
    expected_target: &str,
    credential: Option<&crate::remote::OperationCredential>,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<UpdatePlan, Error> {
    controlled(limits.timeout, cancelled, |ctx| {
        update_inner(
            root,
            path,
            expected_current,
            expected_target,
            credential,
            limits,
            ctx,
        )
    })
}

fn update_journal_bytes(plan: &UpdatePlan) -> Vec<u8> {
    let parent_index = plan
        .parent_index
        .sha256()
        .map_or_else(|| "-".to_owned(), |hash| hex(hash));
    let nested_index = plan
        .nested_index
        .sha256()
        .map_or_else(|| "-".to_owned(), |hash| hex(hash));
    format!(
        "CIDE-SUBMODULE-UPDATE-1\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n",
        plan.parent_commit,
        plan.previous_commit,
        plan.target_commit,
        hex(&plan.path),
        hex(&plan.name),
        parent_index,
        nested_index,
        plan.nonce,
        hex(plan.preview.source.as_bytes()),
        plan.preview.files,
        plan.preview.bytes,
    )
    .into_bytes()
}

fn remove_owned_tree(path: &Path, budget: &mut usize, limit: usize) -> Result<(), Error> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(Error::Unsupported);
    }
    if metadata.is_dir() {
        for entry in std::fs::read_dir(path)? {
            *budget = budget.checked_add(1).ok_or(Error::Limited)?;
            if *budget > limit {
                return Err(Error::Limited);
            }
            remove_owned_tree(&entry?.path(), budget, limit)?;
        }
        std::fs::remove_dir(path)?;
    } else if metadata.is_file() {
        std::fs::remove_file(path)?;
    } else {
        return Err(Error::Unsupported);
    }
    Ok(())
}

fn verify_no_ignored_or_untracked(
    root: &Path,
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<(), Error> {
    use std::collections::BTreeSet;
    let (repo, _) = open(root, ctx)?;
    let commit = repo.head_commit().map_err(crate::engine)?;
    let tree = commit.tree_id().map_err(crate::engine)?.detach();
    let entries = crate::revision::tree_entries(
        &repo,
        tree,
        limits,
        &mut crate::revision::Budget::new(limits),
        ctx,
    )?;
    let mut files = BTreeSet::<String>::new();
    let mut directories = BTreeSet::<String>::new();
    for (path, entry) in entries {
        ctx.check()?;
        if !matches!(entry.mode, 0o100644 | 0o100755) {
            return Err(Error::Unsupported);
        }
        let text = std::str::from_utf8(&path).map_err(|_| Error::Unsupported)?;
        files.insert(text.to_owned());
        for (offset, _) in text.match_indices('/') {
            directories.insert(text[..offset].to_owned());
        }
    }
    TrackedTree {
        files: &files,
        directories: &directories,
        count: 0,
        bytes: 0,
        limits,
        ctx,
    }
    .walk(root, "")
}

fn apply_update_inner(
    plan: UpdatePlan,
    ctx: &Context<'_>,
    check: &mut impl FnMut() -> Result<(), Error>,
) -> Result<UpdatePreview, Error> {
    let (parent_repo, parent_identity, parent_commit, submodule) = selected_submodule(
        &plan.repository.root,
        &plan.path,
        &plan.target_commit.to_string(),
        plan.limits,
        ctx,
    )?;
    crate::index_write::same_repository(&plan.repository, &parent_identity)?;
    let (parent_index, _) =
        crate::local_pair::index_capture(&parent_repo, &parent_identity.git_dir, ctx)?;
    if parent_commit != plan.parent_commit || parent_index != plan.parent_index {
        return Err(Error::Stale);
    }
    if submodule.name.as_deref() != Some(plan.name.as_slice())
        || source_for_registration(&parent_repo, submodule.url.as_deref().ok_or(Error::Stale)?)?
            != plan.source
    {
        return Err(Error::Stale);
    }

    let worktree = worktree_submodule_path(&plan.repository.root, &plan.path)?;
    let (nested, nested_identity) = open(&worktree, ctx)?;
    crate::index_write::same_repository(&plan.nested_repository, &nested_identity)?;
    if nested.head_name().map_err(crate::engine)?.is_some()
        || nested.head_id().map_err(crate::engine)?.detach() != plan.previous_commit
    {
        return Err(Error::Stale);
    }
    let (nested_index, _) =
        crate::local_pair::index_capture(&nested, &nested_identity.git_dir, ctx)?;
    if nested_index != plan.nested_index {
        return Err(Error::Stale);
    }
    require_clean(&worktree, plan.limits, ctx)?;

    let module = nested_identity.git_dir.clone();
    let stage_module = module
        .parent()
        .ok_or(Error::InvalidRoot)?
        .join(format!(".cide-stage-{}", plan.nonce));
    let stage_worktree = plan
        .repository
        .root
        .join(format!(".cide-submodule-{}", plan.nonce));
    let backup_module = module
        .parent()
        .ok_or(Error::InvalidRoot)?
        .join(format!(".cide-module-old-{}", plan.nonce));
    let backup_worktree = placeholder_backup(&worktree, &format!("old-{}", plan.nonce))?;
    if crate::index_write::exists(&stage_module)?
        || crate::index_write::exists(&stage_worktree)?
        || crate::index_write::exists(&backup_module)?
        || crate::index_write::exists(&backup_worktree)?
    {
        return Err(Error::Stale);
    }
    std::fs::create_dir(&stage_worktree)?;
    let mut stage_repo = gix::init_bare(&stage_module).map_err(crate::engine)?;
    stage_repo.refs.write_reflog = gix::refs::store::WriteReflog::Disable;
    let stage_result = (|| {
        let source_repo = open_source_repo(&plan.source, plan.source_stage.as_deref(), ctx)?;
        crate::remote::copy_commit_objects(
            &source_repo,
            &stage_repo,
            plan.target_commit,
            plan.limits,
            ctx,
        )?;
        let head = stage_repo
            .find_reference("HEAD")
            .map_err(crate::engine)?
            .target()
            .into_owned();
        stage_repo
            .edit_references_as(
                [gix::refs::transaction::RefEdit::update(
                    gix::refs::FullName::try_from("HEAD").map_err(crate::engine)?,
                    gix::refs::Target::Object(plan.target_commit),
                    gix::refs::transaction::PreviousValue::MustExistAndMatch(head),
                    String::new(),
                )],
                None,
            )
            .map_err(crate::engine)?;
        let checkout = crate::worktree_materialize::prepare(
            &stage_repo,
            plan.target_commit,
            plan.limits,
            ctx,
        )?;
        checkout.write(&stage_worktree, &stage_module.join("index"), ctx)?;
        set_module_metadata(&mut stage_repo, &worktree)?;
        let git_file = format!("gitdir: {}\n", path_text(&module)?.replace('\\', "/"));
        crate::worktree_materialize::write_new(
            &stage_worktree,
            &stage_worktree.join(".git"),
            git_file.as_bytes(),
            ctx,
        )?;
        check()?;
        Ok(())
    })();
    if let Err(error) = stage_result {
        let mut budget = 0;
        let _ = remove_owned_tree(&stage_worktree, &mut budget, plan.limits.max_entries);
        budget = 0;
        let _ = remove_owned_tree(&stage_module, &mut budget, plan.limits.max_entries);
        return Err(error);
    }
    let journal_path = plan.repository.git_dir.join(INIT_JOURNAL);
    let journal_token = capture_missing(&plan.repository.git_dir, &journal_path, ctx)?;
    jc_fs::stage_missing_file(
        &plan.repository.git_dir,
        &journal_token,
        &update_journal_bytes(&plan),
        &mut || check().is_err(),
    )?
    .publish()?;
    let publish_result = (|| {
        check()?;
        require_clean(&worktree, plan.limits, ctx)?;
        std::fs::rename(&worktree, &backup_worktree)?;
        check()?;
        crate::index_write::ordinary(&module, true)?;
        std::fs::rename(&module, &backup_module)?;
        check()?;
        publish_stage(&stage_module, &module)?;
        publish_stage(&stage_worktree, &worktree)?;
        let (updated, _) = open(&worktree, ctx)?;
        if updated.head_id().map_err(crate::engine)?.detach() != plan.target_commit {
            return Err(Error::Stale);
        }
        require_clean(&worktree, plan.limits, ctx)?;
        let mut budget = 0;
        remove_owned_tree(&backup_worktree, &mut budget, plan.limits.max_entries)?;
        budget = 0;
        remove_owned_tree(&backup_module, &mut budget, plan.limits.max_entries)?;
        let jc_fs::Capture::Present { token, bytes, .. } =
            crate::local_pair::capture(&plan.repository.git_dir, &journal_path, 16 * 1024, ctx)?
        else {
            return Err(Error::Stale);
        };
        if bytes != update_journal_bytes(&plan) {
            return Err(Error::Stale);
        }
        jc_fs::remove_captured_file(&plan.repository.git_dir, &token)?;
        Ok(())
    })();
    publish_result
        .map(|()| plan.preview.clone())
        .map_err(|error| {
            if plan.repository.git_dir.join(INIT_JOURNAL).exists() {
                Error::Engine(format!(
                    "{error}. La actualización quedó registrada para recuperarse en {}",
                    plan.repository.git_dir.join(INIT_JOURNAL).display()
                ))
            } else {
                error
            }
        })
}

/// Apply update only after revalidating a clean, detached worktree and the
/// superproject's exact gitlink. A journal retains both prior trees on failure.
pub fn apply_update(
    plan: UpdatePlan,
    mut cancelled: impl FnMut() -> bool,
) -> Result<UpdatePreview, Error> {
    let stop = std::sync::atomic::AtomicBool::new(false);
    let reason = std::sync::atomic::AtomicU8::new(0);
    let ctx = Context {
        stop: &stop,
        reason: &reason,
        deadline: std::time::Instant::now() + plan.limits.timeout,
    };
    let mut check = || {
        if cancelled() {
            Err(Error::Cancelled)
        } else {
            ctx.check()
        }
    };
    check()?;
    apply_update_inner(plan, &ctx, &mut check)
}

fn read_detached_head(git_dir: &Path, ctx: &Context<'_>) -> Result<gix::ObjectId, Error> {
    let head = git_dir.join("HEAD");
    let jc_fs::Capture::Present { bytes, .. } =
        crate::local_pair::capture(git_dir, &head, 128, ctx)?
    else {
        return Err(Error::Stale);
    };
    let value = std::str::from_utf8(&bytes)
        .map_err(|_| Error::Unsupported)?
        .trim();
    if value.starts_with("ref:") {
        return Err(Error::Stale);
    }
    gix::ObjectId::from_hex(value.as_bytes()).map_err(crate::engine)
}

fn parse_index_digest(value: &str) -> Result<Option<Vec<u8>>, Error> {
    if value == "-" {
        Ok(None)
    } else {
        Ok(Some(unhex(value)?))
    }
}

fn recover_update_inner(
    root: &Path,
    expected: Option<&UpdatePreview>,
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<bool, Error> {
    let (_, repository) = open(root, ctx)?;
    let journal = repository.git_dir.join(INIT_JOURNAL);
    let jc_fs::Capture::Present {
        token: journal_token,
        bytes: journal_bytes,
        ..
    } = crate::local_pair::capture(&repository.git_dir, &journal, 16 * 1024, ctx)?
    else {
        return Ok(false);
    };
    if !journal_token.has_single_link() {
        return Err(Error::Unsupported);
    }
    let text = std::str::from_utf8(&journal_bytes).map_err(|_| Error::Unsupported)?;
    let mut lines = text.lines();
    if lines.next() != Some("CIDE-SUBMODULE-UPDATE-1") {
        return Err(Error::Stale);
    }
    let parent = gix::ObjectId::from_hex(lines.next().ok_or(Error::Unsupported)?.as_bytes())
        .map_err(crate::engine)?;
    let previous = gix::ObjectId::from_hex(lines.next().ok_or(Error::Unsupported)?.as_bytes())
        .map_err(crate::engine)?;
    let target = gix::ObjectId::from_hex(lines.next().ok_or(Error::Unsupported)?.as_bytes())
        .map_err(crate::engine)?;
    let path = unhex(lines.next().ok_or(Error::Unsupported)?)?;
    let name = unhex(lines.next().ok_or(Error::Unsupported)?)?;
    let parent_index_hex = lines.next().ok_or(Error::Unsupported)?;
    let parent_index = parse_index_digest(parent_index_hex)?;
    let nested_index_hex = lines.next().ok_or(Error::Unsupported)?;
    let nested_index = parse_index_digest(nested_index_hex)?;
    let nonce = lines.next().ok_or(Error::Unsupported)?;
    let source = String::from_utf8(unhex(lines.next().ok_or(Error::Unsupported)?)?)
        .map_err(|_| Error::Unsupported)?;
    let files = lines
        .next()
        .and_then(|v| v.parse::<usize>().ok())
        .ok_or(Error::Unsupported)?;
    let byte_count = lines
        .next()
        .and_then(|v| v.parse::<usize>().ok())
        .ok_or(Error::Unsupported)?;
    if lines.next().is_some()
        || nonce.is_empty()
        || nonce.len() > 64
        || !nonce.bytes().all(|b| b.is_ascii_hexdigit())
        || files > limits.max_entries
        || byte_count > limits.max_bytes
    {
        return Err(Error::Unsupported);
    }
    let preview = UpdatePreview {
        operation_id: nonce.to_owned(),
        path: std::str::from_utf8(&path)
            .map_err(|_| Error::Unsupported)?
            .to_owned(),
        name: std::str::from_utf8(&name)
            .map_err(|_| Error::Unsupported)?
            .to_owned(),
        source: source.clone(),
        previous_commit: previous.to_string(),
        target_commit: target.to_string(),
        files,
        bytes: byte_count,
    };
    if expected.is_some_and(|value| value != &preview) {
        return Err(Error::Stale);
    }
    let (parent_repo, identity, parent_now, submodule) =
        selected_submodule(&repository.root, &path, &target.to_string(), limits, ctx)?;
    crate::index_write::same_repository(&repository, &identity)?;
    let (index_now, _) = crate::local_pair::index_capture(&parent_repo, &identity.git_dir, ctx)?;
    if parent_now != parent
        || submodule.name.as_deref() != Some(name.as_slice())
        || source_label(&source_for_registration(
            &parent_repo,
            submodule.url.as_deref().ok_or(Error::Stale)?,
        )?)? != source
        || index_now.sha256().map(|v| v.as_slice()) != parent_index.as_deref()
    {
        return Err(Error::Stale);
    }
    let worktree = worktree_submodule_path(&identity.root, &path)?;
    let module = identity
        .git_dir
        .join("modules")
        .join(module_storage_name(&name, ctx)?);
    let stage_worktree = identity.root.join(format!(".cide-submodule-{nonce}"));
    let stage_module = module
        .parent()
        .ok_or(Error::InvalidRoot)?
        .join(format!(".cide-stage-{nonce}"));
    let backup_worktree = placeholder_backup(&worktree, &format!("old-{nonce}"))?;
    let backup_module = module
        .parent()
        .ok_or(Error::InvalidRoot)?
        .join(format!(".cide-module-old-{nonce}"));

    let old_module = if crate::index_write::exists(&backup_module)? {
        Some(backup_module.as_path())
    } else if crate::index_write::exists(&module)? && read_detached_head(&module, ctx)? == previous
    {
        Some(module.as_path())
    } else {
        None
    };
    if let Some(old_module) = old_module {
        let index_path = old_module.join("index");
        match crate::local_pair::capture(
            old_module.parent().ok_or(Error::InvalidRoot)?,
            &index_path,
            crate::INDEX_BYTES as usize,
            ctx,
        )? {
            jc_fs::Capture::Present { bytes, .. }
                if nested_index.as_deref().is_some_and(|expected| {
                    crate::local_pair::sha256(&bytes, ctx)
                        .is_ok_and(|actual| actual.as_slice() == expected)
                }) => {}
            jc_fs::Capture::Absent { .. } if nested_index.is_none() => {}
            _ => return Err(Error::Stale),
        }
    }

    // At each crash boundary, either the old final or its backup remains. Move
    // it only when its detached OID still matches the reviewed old revision.
    if crate::index_write::exists(&worktree)? {
        match open(&worktree, ctx) {
            Ok((nested, _)) if nested.head_id().map_err(crate::engine)?.detach() == target => {}
            Ok((nested, _)) if nested.head_id().map_err(crate::engine)?.detach() == previous => {
                require_clean(&worktree, limits, ctx)?;
                if crate::index_write::exists(&backup_worktree)? {
                    return Err(Error::Stale);
                }
                std::fs::rename(&worktree, &backup_worktree)?;
            }
            _ => return Err(Error::Stale),
        }
    }
    if crate::index_write::exists(&module)? {
        let oid = read_detached_head(&module, ctx)?;
        if oid == previous {
            if crate::index_write::exists(&backup_module)? {
                return Err(Error::Stale);
            }
            std::fs::rename(&module, &backup_module)?;
        } else if oid != target {
            return Err(Error::Stale);
        }
    }
    if !crate::index_write::exists(&module)? {
        if !crate::index_write::exists(&stage_module)? {
            return Err(Error::Stale);
        }
        publish_stage(&stage_module, &module)?;
    } else if crate::index_write::exists(&stage_module)? {
        return Err(Error::Stale);
    }
    if !crate::index_write::exists(&worktree)? {
        if !crate::index_write::exists(&stage_worktree)? {
            return Err(Error::Stale);
        }
        let missing = capture_missing(&identity.root, &worktree, ctx)?;
        jc_fs::create_missing_parents(&identity.root, &missing, &mut || ctx.check().is_err())?;
        let current = capture_missing(&identity.root, &worktree, ctx)?;
        if !missing.is_compatible_after_creating_parents(&current) {
            return Err(Error::Stale);
        }
        publish_stage(&stage_worktree, &worktree)?;
    } else if crate::index_write::exists(&stage_worktree)? {
        return Err(Error::Stale);
    }
    let (updated, nested_identity) = open(&worktree, ctx)?;
    if updated.head_id().map_err(crate::engine)?.detach() != target {
        return Err(Error::Stale);
    }
    crate::index_write::ordinary(&nested_identity.git_dir, true)?;
    require_clean(&worktree, limits, ctx)?;
    let mut budget = 0;
    if crate::index_write::exists(&backup_worktree)? {
        remove_owned_tree(&backup_worktree, &mut budget, limits.max_entries)?;
    }
    budget = 0;
    if crate::index_write::exists(&backup_module)? {
        remove_owned_tree(&backup_module, &mut budget, limits.max_entries)?;
    }
    let jc_fs::Capture::Present { token, bytes, .. } =
        crate::local_pair::capture(&identity.git_dir, &journal, 16 * 1024, ctx)?
    else {
        return Err(Error::Stale);
    };
    if bytes != journal_bytes {
        return Err(Error::Stale);
    }
    jc_fs::remove_captured_file(&identity.git_dir, &token)?;
    Ok(true)
}

/// Preview the exact pending update recovery without applying filesystem changes.
pub fn preview_update_recovery(
    root: &Path,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<Option<UpdatePreview>, Error> {
    limits.admit()?;
    controlled(limits.timeout, cancelled, |ctx| {
        let (_, repository) = open(root, ctx)?;
        let journal = repository.git_dir.join(INIT_JOURNAL);
        let jc_fs::Capture::Present { bytes, .. } =
            crate::local_pair::capture(&repository.git_dir, &journal, 16 * 1024, ctx)?
        else {
            return Ok(None);
        };
        let text = std::str::from_utf8(&bytes).map_err(|_| Error::Unsupported)?;
        let mut lines = text.lines();
        if lines.next() != Some("CIDE-SUBMODULE-UPDATE-1") {
            return Err(Error::Stale);
        }
        let _parent = lines.next().ok_or(Error::Unsupported)?;
        let previous = lines.next().ok_or(Error::Unsupported)?.to_owned();
        let target = lines.next().ok_or(Error::Unsupported)?.to_owned();
        let path = unhex(lines.next().ok_or(Error::Unsupported)?)?;
        let name = unhex(lines.next().ok_or(Error::Unsupported)?)?;
        let _parent_index = lines.next().ok_or(Error::Unsupported)?;
        let _nested_index = lines.next().ok_or(Error::Unsupported)?;
        let operation_id = lines.next().ok_or(Error::Unsupported)?.to_owned();
        let source = String::from_utf8(unhex(lines.next().ok_or(Error::Unsupported)?)?)
            .map_err(|_| Error::Unsupported)?;
        let files = lines
            .next()
            .and_then(|v| v.parse::<usize>().ok())
            .ok_or(Error::Unsupported)?;
        let bytes = lines
            .next()
            .and_then(|v| v.parse::<usize>().ok())
            .ok_or(Error::Unsupported)?;
        if lines.next().is_some() || files > limits.max_entries || bytes > limits.max_bytes {
            return Err(Error::Unsupported);
        }
        Ok(Some(UpdatePreview {
            operation_id,
            path: std::str::from_utf8(&path)
                .map_err(|_| Error::Unsupported)?
                .to_owned(),
            name: std::str::from_utf8(&name)
                .map_err(|_| Error::Unsupported)?
                .to_owned(),
            source,
            previous_commit: previous,
            target_commit: target,
            files,
            bytes,
        }))
    })
}

/// Resume only the exact pending update shown in the accepted preview.
pub fn recover_update_expected(
    root: &Path,
    expected: &UpdatePreview,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<bool, Error> {
    controlled(limits.timeout, cancelled, |ctx| {
        recover_update_inner(root, Some(expected), limits, ctx)
    })
}

fn verify_init_pin(
    plan: &InitPlan,
    ctx: &Context<'_>,
) -> Result<(gix::Repository, Submodule), Error> {
    let (repo, identity, parent_commit, submodule) = selected_submodule(
        &plan.repository.root,
        &plan.path,
        &plan.preview.commit,
        plan.limits,
        ctx,
    )?;
    crate::index_write::same_repository(&plan.repository, &identity)?;
    let (index, _) = crate::local_pair::index_capture(&repo, &identity.git_dir, ctx)?;
    if parent_commit != plan.parent_commit || index != plan.index {
        return Err(Error::Stale);
    }
    let name = submodule.name.as_deref().ok_or(Error::Stale)?;
    let expected_url = submodule.url.as_deref().ok_or(Error::Stale)?;
    if name != plan.name || source_for_registration(&repo, expected_url)? != plan.source {
        return Err(Error::Stale);
    }
    if !target_matches(
        &identity.root,
        &worktree_submodule_path(&identity.root, &plan.path)?,
        &plan.worktree_target,
        ctx,
    )? {
        return Err(Error::Stale);
    }
    let module_dir = identity
        .git_dir
        .join("modules")
        .join(module_storage_name(&plan.name, ctx)?);
    if capture_missing(&identity.git_dir, &module_dir, ctx)? != plan.module_missing {
        return Err(Error::Stale);
    }
    Ok((repo, submodule))
}

fn init_journal_bytes(plan: &InitPlan) -> Result<Vec<u8>, Error> {
    let path = hex(&plan.path);
    let name = hex(&plan.name);
    let index = plan
        .index
        .sha256()
        .map_or_else(|| "-".to_owned(), |hash| hex(hash));
    let placeholder = if plan.preview.replacing_empty_checkout_placeholder {
        "1"
    } else {
        "0"
    };
    let source = hex(plan.preview.source.as_bytes());
    Ok(format!(
        "CIDE-SUBMODULE-INIT-1\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n",
        plan.parent_commit,
        plan.commit,
        path,
        name,
        index,
        plan.nonce,
        placeholder,
        source,
        plan.preview.files,
        plan.preview.bytes
    )
    .into_bytes())
}

fn set_module_metadata(repo: &mut gix::Repository, worktree: &Path) -> Result<(), Error> {
    let path = repo.git_dir().join("config");
    let mut config = gix::config::File::from_bytes_no_includes(
        &std::fs::read(&path)?,
        gix::config::file::Metadata::default(),
        gix::config::file::init::Options::default(),
    )
    .map_err(|_| Error::Unsupported)?;
    let worktree = path_text(worktree)?.replace('\\', "/");
    let mut core = config.new_section("core", None).map_err(crate::engine)?;
    core.set("bare", "false").map_err(crate::engine)?;
    core.set("worktree", worktree.as_str())
        .map_err(crate::engine)?;
    let mut bytes = Vec::new();
    config.write_to(&mut bytes)?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)?;
    use std::io::Write;
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(())
}

fn publish_stage(from: &Path, to: &Path) -> Result<(), Error> {
    crate::remote::publish_directory_create_only(from, to)
}

fn placeholder_backup(path: &Path, nonce: &str) -> Result<PathBuf, Error> {
    Ok(path
        .parent()
        .ok_or(Error::InvalidRoot)?
        .join(format!(".cide-submodule-empty-{nonce}")))
}

fn move_placeholder_to_backup(
    root: &Path,
    path: &Path,
    backup: &Path,
    ctx: &Context<'_>,
) -> Result<(), Error> {
    match worktree_target(root, path, ctx)? {
        WorktreeTarget::Placeholder(_) => {
            if crate::index_write::exists(backup)? {
                return Err(Error::Stale);
            }
            // The empty clone sentinel is retained under a recovery name until
            // the initialized worktree has been verified at the original path.
            std::fs::rename(path, backup)?;
            Ok(())
        }
        WorktreeTarget::Missing(_) => Ok(()),
    }
}

fn remove_placeholder_backup(root: &Path, backup: &Path, ctx: &Context<'_>) -> Result<(), Error> {
    if !crate::index_write::exists(backup)? {
        return Ok(());
    }
    let WorktreeTarget::Placeholder(_) = worktree_target(root, backup, ctx)? else {
        return Err(Error::Stale);
    };
    std::fs::remove_dir(backup)?;
    Ok(())
}

/// Apply the reviewed initialization. A durable journal remains if either of
/// the two create-only directory publications is interrupted; `recover_init`
/// can complete it without deleting user data.
pub fn apply_init(
    plan: InitPlan,
    mut cancelled: impl FnMut() -> bool,
) -> Result<InitPreview, Error> {
    let stop = std::sync::atomic::AtomicBool::new(false);
    let reason = std::sync::atomic::AtomicU8::new(0);
    let ctx = Context {
        stop: &stop,
        reason: &reason,
        deadline: std::time::Instant::now() + plan.limits.timeout,
    };
    let mut check = || {
        if cancelled() {
            Err(Error::Cancelled)
        } else {
            ctx.check()
        }
    };
    check()?;
    verify_init_pin(&plan, &ctx)?;
    let worktree = worktree_submodule_path(&plan.repository.root, &plan.path)?;
    let module_dir = plan
        .repository
        .git_dir
        .join("modules")
        .join(module_storage_name(&plan.name, &ctx)?);
    let modules = module_dir.parent().ok_or(Error::InvalidRoot)?;
    match std::fs::symlink_metadata(modules) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
        Ok(_) => return Err(Error::Unsupported),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => std::fs::create_dir(modules)?,
        Err(error) => return Err(error.into()),
    }
    let stage_module = modules.join(format!(".cide-stage-{}", plan.nonce));
    let stage_worktree = plan
        .repository
        .root
        .join(format!(".cide-submodule-{}", plan.nonce));
    if crate::index_write::exists(&stage_module)? || crate::index_write::exists(&stage_worktree)? {
        return Err(Error::Stale);
    }
    std::fs::create_dir(&stage_worktree)?;
    let mut stage_repo = match gix::init_bare(&stage_module) {
        Ok(repo) => repo,
        Err(error) => {
            let _ = std::fs::remove_dir(&stage_worktree);
            return Err(crate::engine(error));
        }
    };
    let result = (|| {
        stage_repo.refs.write_reflog = gix::refs::store::WriteReflog::Disable;
        let source_repo = open_source_repo(&plan.source, plan.source_stage.as_deref(), &ctx)?;
        crate::remote::copy_commit_objects(
            &source_repo,
            &stage_repo,
            plan.commit,
            plan.limits,
            &ctx,
        )?;
        let head = stage_repo
            .find_reference("HEAD")
            .map_err(crate::engine)?
            .target()
            .into_owned();
        stage_repo
            .edit_references_as(
                [gix::refs::transaction::RefEdit::update(
                    gix::refs::FullName::try_from("HEAD").map_err(crate::engine)?,
                    gix::refs::Target::Object(plan.commit),
                    gix::refs::transaction::PreviousValue::MustExistAndMatch(head),
                    String::new(),
                )],
                None,
            )
            .map_err(crate::engine)?;
        let checkout =
            crate::worktree_materialize::prepare(&stage_repo, plan.commit, plan.limits, &ctx)?;
        checkout.write(&stage_worktree, &stage_module.join("index"), &ctx)?;
        set_module_metadata(&mut stage_repo, &worktree)?;
        let git_file = format!("gitdir: {}\n", path_text(&module_dir)?.replace('\\', "/"));
        crate::worktree_materialize::write_new(
            &stage_worktree,
            &stage_worktree.join(".git"),
            git_file.as_bytes(),
            &ctx,
        )?;
        check()?;
        let journal_path = plan.repository.git_dir.join(INIT_JOURNAL);
        let journal_token = capture_missing(&plan.repository.git_dir, &journal_path, &ctx)?;
        jc_fs::stage_missing_file(
            &plan.repository.git_dir,
            &journal_token,
            &init_journal_bytes(&plan)?,
            &mut || check().is_err(),
        )?
        .publish()?;
        check()?;
        let module_token = capture_missing(&plan.repository.git_dir, &module_dir, &ctx)?;
        if !plan
            .module_missing
            .is_compatible_after_creating_parents(&module_token)
        {
            return Err(Error::Stale);
        }
        publish_stage(&stage_module, &module_dir)?;
        check()?;
        let backup = placeholder_backup(&worktree, &plan.nonce)?;
        if !target_same_location(
            &plan.repository.root,
            &worktree,
            &plan.worktree_target,
            &ctx,
        )? {
            return Err(Error::Stale);
        }
        if matches!(plan.worktree_target, WorktreeTarget::Placeholder(_)) {
            move_placeholder_to_backup(&plan.repository.root, &worktree, &backup, &ctx)?;
        }
        let current_worktree_token = capture_missing(&plan.repository.root, &worktree, &ctx)?;
        if let WorktreeTarget::Missing(expected) = &plan.worktree_target {
            if !expected.is_compatible_after_creating_parents(&current_worktree_token) {
                return Err(Error::Stale);
            }
        }
        jc_fs::create_missing_parents(&plan.repository.root, &current_worktree_token, &mut || {
            check().is_err()
        })?;
        let current_worktree_token = capture_missing(&plan.repository.root, &worktree, &ctx)?;
        if !current_worktree_token.is_leaf()
            || matches!(&plan.worktree_target, WorktreeTarget::Missing(expected)
                if !expected.is_compatible_after_creating_parents(&current_worktree_token))
        {
            return Err(Error::Stale);
        }
        publish_stage(&stage_worktree, &worktree)?;
        let (opened, _) = open(&worktree, &ctx)?;
        if opened.head_id().map_err(crate::engine)?.detach() != plan.commit {
            return Err(Error::Stale);
        }
        if plan.preview.replacing_empty_checkout_placeholder {
            remove_placeholder_backup(&plan.repository.root, &backup, &ctx)?;
        }
        let journal_capture =
            crate::local_pair::capture(&plan.repository.git_dir, &journal_path, 4096, &ctx)?;
        if let jc_fs::Capture::Present { token, bytes, .. } = journal_capture {
            if bytes != init_journal_bytes(&plan)? {
                return Err(Error::Stale);
            }
            jc_fs::remove_captured_file(&plan.repository.git_dir, &token)?;
        } else {
            return Err(Error::Stale);
        }
        Ok(())
    })();
    if result.is_err() && !plan.repository.git_dir.join(INIT_JOURNAL).exists() {
        // Before the journal is durable these directories are private staging
        // owned by this call. After journaling they are retained for recovery.
        let _ = std::fs::remove_dir_all(&stage_worktree);
        let _ = std::fs::remove_dir_all(&stage_module);
    }
    result.map(|()| plan.preview.clone()).map_err(|error| {
        if plan.repository.git_dir.join(INIT_JOURNAL).exists() {
            Error::Engine(format!(
                "{error}. La inicialización quedó registrada para recuperarse en {}",
                plan.repository.git_dir.join(INIT_JOURNAL).display()
            ))
        } else {
            error
        }
    })
}

fn recover_init_inner(
    root: &Path,
    limits: Limits,
    ctx: &Context<'_>,
    expected: Option<&InitPreview>,
) -> Result<bool, Error> {
    let (_, repository) = open(root, ctx)?;
    let path = repository.git_dir.join(INIT_JOURNAL);
    let jc_fs::Capture::Present { token, bytes, .. } =
        crate::local_pair::capture(&repository.git_dir, &path, 16 * 1024, ctx)?
    else {
        return Ok(false);
    };
    if !token.has_single_link() {
        return Err(Error::Unsupported);
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| Error::Unsupported)?;
    let mut lines = text.lines();
    if lines.next() != Some("CIDE-SUBMODULE-INIT-1") {
        return Err(Error::Unsupported);
    }
    let parent = gix::ObjectId::from_hex(lines.next().ok_or(Error::Unsupported)?.as_bytes())
        .map_err(crate::engine)?;
    let commit = gix::ObjectId::from_hex(lines.next().ok_or(Error::Unsupported)?.as_bytes())
        .map_err(crate::engine)?;
    let raw_path = unhex(lines.next().ok_or(Error::Unsupported)?)?;
    let name = unhex(lines.next().ok_or(Error::Unsupported)?)?;
    let index_hex = lines.next().ok_or(Error::Unsupported)?;
    let expected_index = if index_hex == "-" {
        None
    } else {
        Some(unhex(index_hex)?)
    };
    let nonce = lines.next().ok_or(Error::Unsupported)?;
    let placeholder = match lines.next() {
        Some("0") => false,
        Some("1") => true,
        _ => return Err(Error::Unsupported),
    };
    let source = String::from_utf8(unhex(lines.next().ok_or(Error::Unsupported)?)?)
        .map_err(|_| Error::Unsupported)?;
    let files = lines
        .next()
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or(Error::Unsupported)?;
    let bytes = lines
        .next()
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or(Error::Unsupported)?;
    if lines.next().is_some()
        || nonce.is_empty()
        || nonce.len() > 64
        || !nonce.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(Error::Unsupported);
    }
    let (super_repo, identity, current_parent, submodule) = selected_submodule(
        &repository.root,
        &raw_path,
        &commit.to_string(),
        limits,
        ctx,
    )?;
    crate::index_write::same_repository(&repository, &identity)?;
    let (index, _) = crate::local_pair::index_capture(&super_repo, &repository.git_dir, ctx)?;
    if current_parent != parent
        || submodule.name.as_deref() != Some(name.as_slice())
        || source_label(&source_for_registration(
            &super_repo,
            submodule.url.as_deref().ok_or(Error::Stale)?,
        )?)? != source
        || index.sha256().map(|hash| hash.as_slice()) != expected_index.as_deref()
        || files > limits.max_entries
        || bytes > limits.max_bytes
    {
        return Err(Error::Stale);
    }
    let recovery_preview = InitPreview {
        operation_id: nonce.to_owned(),
        path: std::str::from_utf8(&raw_path)
            .map_err(|_| Error::Unsupported)?
            .to_owned(),
        name: std::str::from_utf8(&name)
            .map_err(|_| Error::Unsupported)?
            .to_owned(),
        source: source.clone(),
        commit: commit.to_string(),
        files,
        bytes,
        replacing_empty_checkout_placeholder: placeholder,
    };
    if expected.is_some_and(|preview| preview != &recovery_preview) {
        return Err(Error::Stale);
    }
    let module = repository
        .git_dir
        .join("modules")
        .join(module_storage_name(&name, ctx)?);
    let stage_module = module
        .parent()
        .ok_or(Error::InvalidRoot)?
        .join(format!(".cide-stage-{nonce}"));
    let worktree = worktree_submodule_path(&repository.root, &raw_path)?;
    let stage_worktree = repository.root.join(format!(".cide-submodule-{nonce}"));
    let module_exists = crate::index_write::exists(&module)?;
    let worktree_exists = crate::index_write::exists(&worktree)?;
    if worktree_exists && !module_exists {
        return Err(Error::Stale);
    }
    if !module_exists {
        if !crate::index_write::exists(&stage_module)? {
            return Err(Error::Stale);
        }
        publish_stage(&stage_module, &module)?;
    } else if crate::index_write::exists(&stage_module)? {
        return Err(Error::Stale);
    }
    crate::index_write::ordinary(&module, true)?;
    let backup = placeholder_backup(&worktree, nonce)?;
    let initialized = if worktree_exists {
        match open(&worktree, ctx) {
            Ok((nested, _)) if nested.head_id().map_err(crate::engine)?.detach() == commit => true,
            _ if placeholder => {
                move_placeholder_to_backup(&repository.root, &worktree, &backup, ctx)?;
                false
            }
            _ => return Err(Error::Stale),
        }
    } else {
        false
    };
    if !initialized {
        if !crate::index_write::exists(&stage_worktree)? {
            return Err(Error::Stale);
        }
        let missing = capture_missing(&repository.root, &worktree, ctx)?;
        jc_fs::create_missing_parents(&repository.root, &missing, &mut || ctx.check().is_err())?;
        let current = capture_missing(&repository.root, &worktree, ctx)?;
        if !missing.is_compatible_after_creating_parents(&current) || !current.is_leaf() {
            return Err(Error::Stale);
        }
        publish_stage(&stage_worktree, &worktree)?;
    } else if crate::index_write::exists(&stage_worktree)? {
        return Err(Error::Stale);
    }
    let (nested, _) = open(&worktree, ctx)?;
    if nested.head_id().map_err(crate::engine)?.detach() != commit {
        return Err(Error::Stale);
    }
    if placeholder {
        remove_placeholder_backup(&repository.root, &backup, ctx)?;
    }
    jc_fs::remove_captured_file(&repository.git_dir, &token)?;
    Ok(true)
}

/// Resume a durable two-destination initialization journal, if one exists.
/// Recovery is explicit and only rolls forward to the already pinned gitlink.
pub fn recover_init(
    root: &Path,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<bool, Error> {
    controlled(limits.timeout, cancelled, |ctx| {
        recover_init_inner(root, limits, ctx, None)
    })
}

/// Recover only the exact operation whose preview the host accepted.
pub fn recover_init_expected(
    root: &Path,
    expected: &InitPreview,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<bool, Error> {
    controlled(limits.timeout, cancelled, |ctx| {
        recover_init_inner(root, limits, ctx, Some(expected))
    })
}

/// Recreate the user-facing preview for a pending initialization without
/// changing either published path. The preview is returned only if the
/// superproject still pins the same gitlink, registration, and index.
pub fn preview_init_recovery(
    root: &Path,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<Option<InitPreview>, Error> {
    limits.admit()?;
    controlled(limits.timeout, cancelled, |ctx| {
        let (_, repository) = open(root, ctx)?;
        let journal = repository.git_dir.join(INIT_JOURNAL);
        let jc_fs::Capture::Present { token, bytes, .. } =
            crate::local_pair::capture(&repository.git_dir, &journal, 16 * 1024, ctx)?
        else {
            return Ok(None);
        };
        if !token.has_single_link() {
            return Err(Error::Unsupported);
        }
        let text = std::str::from_utf8(&bytes).map_err(|_| Error::Unsupported)?;
        let mut lines = text.lines();
        if lines.next() != Some("CIDE-SUBMODULE-INIT-1") {
            return Err(Error::Unsupported);
        }
        let parent = gix::ObjectId::from_hex(lines.next().ok_or(Error::Unsupported)?.as_bytes())
            .map_err(crate::engine)?;
        let commit = gix::ObjectId::from_hex(lines.next().ok_or(Error::Unsupported)?.as_bytes())
            .map_err(crate::engine)?;
        let path = unhex(lines.next().ok_or(Error::Unsupported)?)?;
        let name = unhex(lines.next().ok_or(Error::Unsupported)?)?;
        let index_hex = lines.next().ok_or(Error::Unsupported)?;
        let expected_index = if index_hex == "-" {
            None
        } else {
            Some(unhex(index_hex)?)
        };
        let nonce = lines.next().ok_or(Error::Unsupported)?;
        let placeholder = match lines.next() {
            Some("0") => false,
            Some("1") => true,
            _ => return Err(Error::Unsupported),
        };
        let source = String::from_utf8(unhex(lines.next().ok_or(Error::Unsupported)?)?)
            .map_err(|_| Error::Unsupported)?;
        let files = lines
            .next()
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or(Error::Unsupported)?;
        let bytes = lines
            .next()
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or(Error::Unsupported)?;
        if lines.next().is_some()
            || nonce.is_empty()
            || nonce.len() > 64
            || !nonce.bytes().all(|b| b.is_ascii_hexdigit())
            || files > limits.max_entries
            || bytes > limits.max_bytes
        {
            return Err(Error::Unsupported);
        }
        let path_text = std::str::from_utf8(&path)
            .map_err(|_| Error::Unsupported)?
            .to_owned();
        let name_text = std::str::from_utf8(&name)
            .map_err(|_| Error::Unsupported)?
            .to_owned();
        let (repo, identity, current_parent, submodule) =
            selected_submodule(&repository.root, &path, &commit.to_string(), limits, ctx)?;
        crate::index_write::same_repository(&repository, &identity)?;
        let (index, _) = crate::local_pair::index_capture(&repo, &identity.git_dir, ctx)?;
        if current_parent != parent
            || submodule.name.as_deref() != Some(name.as_slice())
            || source_label(&source_for_registration(
                &repo,
                submodule.url.as_deref().ok_or(Error::Stale)?,
            )?)? != source
            || index.sha256().map(|hash| hash.as_slice()) != expected_index.as_deref()
        {
            return Err(Error::Stale);
        }
        worktree_submodule_path(&identity.root, &path)?;
        Ok(Some(InitPreview {
            operation_id: nonce.to_owned(),
            path: path_text,
            name: name_text,
            source,
            commit: commit.to_string(),
            files,
            bytes,
            replacing_empty_checkout_placeholder: placeholder,
        }))
    })
}

fn scan_tree(
    repo: &gix::Repository,
    tree_id: gix::ObjectId,
    prefix: &[u8],
    depth: usize,
    scan: &mut Scan,
    ctx: &Context<'_>,
) -> Result<(), Error> {
    if depth > MAX_DEPTH {
        return Err(Error::Limited);
    }
    ctx.check()?;
    let tree = repo.find_tree(tree_id).map_err(crate::engine)?;
    for item in tree.iter() {
        ctx.check()?;
        scan.entries = scan.entries.checked_add(1).ok_or(Error::Limited)?;
        if scan.entries > scan.max_entries {
            return Err(Error::Limited);
        }
        let item = item.map_err(crate::engine)?;
        let name = item.filename().as_bytes();
        if name.is_empty()
            || name.contains(&0)
            || name.contains(&b'/')
            || name == b"."
            || name == b".."
        {
            return Err(Error::Unsupported);
        }
        let mut path =
            Vec::with_capacity(prefix.len().saturating_add(name.len()).saturating_add(1));
        if !prefix.is_empty() {
            path.extend_from_slice(prefix);
            path.push(b'/');
        }
        path.extend_from_slice(name);
        if path.len() > scan.max_path {
            return Err(Error::Limited);
        }
        let mode = item.mode().value() as u32;
        match mode {
            0o160000 => {
                if scan
                    .gitlinks
                    .insert(path, item.object_id().to_string())
                    .is_some()
                {
                    return Err(Error::Unsupported);
                }
            }
            0o40000 => scan_tree(repo, item.object_id(), &path, depth + 1, scan, ctx)?,
            0o100644 | 0o100755 if prefix.is_empty() && name == b".gitmodules" => {
                let header = repo.find_header(item.object_id()).map_err(crate::engine)?;
                let size = usize::try_from(header.size()).map_err(|_| Error::Limited)?;
                if size > GITMODULES_BYTES {
                    return Err(Error::Limited);
                }
                let object = repo.find_object(item.object_id()).map_err(crate::engine)?;
                if object.kind != gix::object::Kind::Blob || object.data.len() > GITMODULES_BYTES {
                    return Err(Error::Unsupported);
                }
                scan.gitmodules = Some(object.data.to_vec());
            }
            _ => {}
        }
    }
    Ok(())
}

fn registrations(bytes: Option<&[u8]>, max_path: usize) -> Result<Registrations, Error> {
    let Some(bytes) = bytes else {
        return Ok(BTreeMap::new());
    };
    let source = std::str::from_utf8(bytes).map_err(|_| Error::Unsupported)?;
    let config = gix::config::File::try_from(source).map_err(crate::engine)?;
    let mut paths = BTreeMap::new();
    let mut names = BTreeSet::new();
    for section in config.sections_by_name("submodule").into_iter().flatten() {
        let name = section
            .header()
            .subsection_name()
            .ok_or(Error::Unsupported)?
            .as_bytes()
            .to_vec();
        if name.is_empty() || !names.insert(name.clone()) {
            return Err(Error::Unsupported);
        }
        let key = std::str::from_utf8(&name).map_err(|_| Error::Unsupported)?;
        let raw_path = config
            .string_by("submodule", key, "path")
            .ok_or(Error::Unsupported)?;
        let path = raw_path.as_bytes().to_vec();
        crate::revision::path_valid(&path, max_path)?;
        let url = config
            .string_by("submodule", key, "url")
            .map(|value| value.to_vec());
        if paths.insert(path, (name, url)).is_some() {
            return Err(Error::Unsupported);
        }
    }
    Ok(paths)
}

fn list_inner(root: &Path, limits: Limits, ctx: &Context<'_>) -> Result<Inventory, Error> {
    limits.admit()?;
    let (repo, identity) = open(root, ctx)?;
    let commit = match repo.head_commit() {
        Ok(commit) => Some(commit),
        Err(error) if error.is_not_found() => None,
        Err(error) => return Err(crate::engine(error)),
    };
    let Some(commit) = commit else {
        return Ok(Inventory {
            repository: identity,
            commit: None,
            submodules: Vec::new(),
        });
    };
    let commit_id = commit.id().detach().to_string();
    let mut scan = Scan {
        entries: 0,
        max_entries: limits.max_entries,
        max_path: limits.max_path,
        gitlinks: BTreeMap::new(),
        gitmodules: None,
    };
    scan_tree(
        &repo,
        commit.tree_id().map_err(crate::engine)?.detach(),
        &[],
        0,
        &mut scan,
        ctx,
    )?;
    let registrations = registrations(scan.gitmodules.as_deref(), limits.max_path)?;
    let mut submodules = Vec::with_capacity(scan.gitlinks.len());
    for (path, object_id) in scan.gitlinks {
        ctx.check()?;
        let registration = registrations.get(&path).cloned();
        let registered = registration.is_some();
        let (name, url) = registration.map_or((None, None), |(name, url)| (Some(name), url));
        submodules.push(Submodule {
            path,
            object_id,
            name,
            url,
            registered,
        });
    }
    // Mark registration by exact raw path; do not infer it from directory presence.
    for submodule in &mut submodules {
        submodule.registered = registrations.contains_key(&submodule.path);
    }
    Ok(Inventory {
        repository: identity,
        commit: Some(commit_id),
        submodules,
    })
}

/// List gitlinks in the current committed tree and match them to the same
/// commit's `.gitmodules`. Missing registration remains visible as such.
pub fn list(
    root: &Path,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<Inventory, Error> {
    controlled(limits.timeout, cancelled, |ctx| {
        list_inner(root, limits, ctx)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[cfg(windows)]
    use std::time::{SystemTime, UNIX_EPOCH};

    #[cfg(windows)]
    struct Fixture(PathBuf);
    #[cfg(windows)]
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
            let path = parent.join(format!(
                "submodules-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn p(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }
    #[cfg(windows)]
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
                    let child = entry.unwrap().path();
                    let metadata = std::fs::symlink_metadata(&child).unwrap();
                    assert!(!metadata.file_type().is_symlink());
                    if metadata.is_dir() {
                        clean(&child);
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
            max_entries: 64,
            max_path: 4096,
            max_bytes: 4 * 1024 * 1024,
            timeout: Duration::from_secs(10),
        }
    }
    #[cfg(windows)]
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
    #[cfg(windows)]
    fn commit(repo: &gix::Repository, tree: gix::ObjectId) -> gix::ObjectId {
        let signature = gix::actor::SignatureRef {
            name: b"CIDE test".as_bstr(),
            email: b"cide@example.invalid".as_bstr(),
            time: "1 +0000",
        };
        repo.write_object(gix::objs::Commit {
            tree,
            parents: Default::default(),
            author: signature.into(),
            committer: signature.into(),
            message: b"fixture".into(),
            encoding: None,
            extra_headers: Vec::new(),
        })
        .unwrap()
        .detach()
    }
    #[cfg(windows)]
    fn head(repo: &mut gix::Repository, oid: gix::ObjectId) {
        repo.refs.write_reflog = gix::refs::store::WriteReflog::Disable;
        let old = repo.find_reference("HEAD").unwrap().target().into_owned();
        repo.edit_references_as(
            [gix::refs::transaction::RefEdit::update(
                gix::refs::FullName::try_from("HEAD").unwrap(),
                gix::refs::Target::Object(oid),
                gix::refs::transaction::PreviousValue::MustExistAndMatch(old),
                String::new(),
            )],
            None,
        )
        .unwrap();
    }

    #[test]
    #[cfg(windows)]
    fn initializes_exact_gitlink_and_recovers_clone_empty_directory_safely() {
        let fixture = Fixture::new();
        let source = fixture.p("child-source");
        std::fs::create_dir(&source).unwrap();
        let child = gix::init(&source).unwrap();
        let blob = child
            .write_object(gix::objs::Blob {
                data: b"child bytes\n".to_vec(),
            })
            .unwrap()
            .detach();
        std::fs::write(source.join("child.txt"), b"child bytes\n").unwrap();
        let child_tree = tree(&child, &[(b"child.txt", 0o100644, blob)]);
        let child_commit = commit(&child, child_tree);

        let root = fixture.p("superproject");
        std::fs::create_dir(&root).unwrap();
        let mut super_repo = gix::init(&root).unwrap();
        let url = source.to_str().unwrap().replace('\\', "/");
        assert_eq!(
            source_from_url(url.as_bytes()).unwrap(),
            Source::Local(source.canonicalize().unwrap())
        );
        let modules = format!("[submodule \"nested\"]\n\tpath = nested\n\turl = {url}\n");
        let modules_blob = super_repo
            .write_object(gix::objs::Blob {
                data: modules.as_bytes().to_vec(),
            })
            .unwrap()
            .detach();
        let super_tree = tree(
            &super_repo,
            &[
                (b".gitmodules", 0o100644, modules_blob),
                (b"nested", 0o160000, child_commit),
            ],
        );
        let super_commit = commit(&super_repo, super_tree);
        head(&mut super_repo, super_commit);
        let inventory = list(&root, limits(), || false).unwrap();
        assert_eq!(inventory.submodules[0].url.as_deref(), Some(url.as_bytes()));
        let placeholder = root.join("nested");
        std::fs::create_dir(&placeholder).unwrap();

        let plan = prepare_init(
            &root,
            b"nested",
            &child_commit.to_string(),
            limits(),
            || false,
        )
        .unwrap();
        assert!(plan.preview().replacing_empty_checkout_placeholder);
        assert_eq!(plan.preview().files, 1);
        assert!(std::fs::read_dir(&placeholder).unwrap().next().is_none());
        assert!(!root.join(".git/modules").exists());
        let preview = apply_init(plan, || false).unwrap();
        assert_eq!(preview.commit, child_commit.to_string());
        assert_eq!(
            std::fs::read(placeholder.join("child.txt")).unwrap(),
            b"child bytes\n"
        );
        assert!(placeholder.join(".git").is_file());
        controlled(
            limits().timeout,
            || false,
            |ctx| {
                let (nested, _) = open(&placeholder, ctx)?;
                assert_eq!(
                    nested.head_id().map_err(crate::engine)?.detach(),
                    child_commit
                );
                Ok(())
            },
        )
        .unwrap();
        assert!(!root.join(".git/cide-submodule.pending").exists());
        assert!(!root.join(".cide-submodule-empty").exists());
        assert!(!recover_init(&root, limits(), || false).unwrap());
    }

    #[test]
    #[cfg(windows)]
    fn update_rejects_tracked_and_ignored_changes_then_publishes_exact_gitlink() {
        let fixture = Fixture::new();
        let source = fixture.p("child-source");
        std::fs::create_dir(&source).unwrap();
        let child = gix::init(&source).unwrap();
        let old_blob = child
            .write_object(gix::objs::Blob {
                data: b"old contents\n".to_vec(),
            })
            .unwrap()
            .detach();
        let ignore_blob = child
            .write_object(gix::objs::Blob {
                data: b"*.tmp\n".to_vec(),
            })
            .unwrap()
            .detach();
        std::fs::write(source.join("child.txt"), b"old contents\n").unwrap();
        std::fs::write(source.join(".gitignore"), b"*.tmp\n").unwrap();
        let old_tree = tree(
            &child,
            &[
                (b".gitignore", 0o100644, ignore_blob),
                (b"child.txt", 0o100644, old_blob),
            ],
        );
        let old_commit = commit(&child, old_tree);

        let root = fixture.p("superproject");
        std::fs::create_dir(&root).unwrap();
        let mut parent_repo = gix::init(&root).unwrap();
        let url = source.to_str().unwrap().replace('\\', "/");
        let modules = format!("[submodule \"nested\"]\n\tpath = nested\n\turl = {url}\n");
        let modules_blob = parent_repo
            .write_object(gix::objs::Blob {
                data: modules.as_bytes().to_vec(),
            })
            .unwrap()
            .detach();
        let parent_tree = tree(
            &parent_repo,
            &[
                (b".gitmodules", 0o100644, modules_blob),
                (b"nested", 0o160000, old_commit),
            ],
        );
        let parent_commit = commit(&parent_repo, parent_tree);
        head(&mut parent_repo, parent_commit);
        std::fs::create_dir(root.join("nested")).unwrap();
        apply_init(
            prepare_init(&root, b"nested", &old_commit.to_string(), limits(), || {
                false
            })
            .unwrap(),
            || false,
        )
        .unwrap();

        // Advance the source and the superproject's exact gitlink.
        let new_blob = child
            .write_object(gix::objs::Blob {
                data: b"new contents\n".to_vec(),
            })
            .unwrap()
            .detach();
        std::fs::write(source.join("child.txt"), b"new contents\n").unwrap();
        let new_tree = tree(
            &child,
            &[
                (b".gitignore", 0o100644, ignore_blob),
                (b"child.txt", 0o100644, new_blob),
            ],
        );
        let new_commit = commit(&child, new_tree);
        let next_parent_tree = tree(
            &parent_repo,
            &[
                (b".gitmodules", 0o100644, modules_blob),
                (b"nested", 0o160000, new_commit),
            ],
        );
        let next_parent_commit = commit(&parent_repo, next_parent_tree);
        head(&mut parent_repo, next_parent_commit);

        // Ignored files still count as local data and block replacement.
        std::fs::write(root.join("nested/ignored.tmp"), b"preserve me\n").unwrap();
        assert!(prepare_update(&root, b"nested", "", "", limits(), || false).is_err());
        assert_eq!(
            std::fs::read(root.join("nested/ignored.tmp")).unwrap(),
            b"preserve me\n"
        );
        std::fs::remove_file(root.join("nested/ignored.tmp")).unwrap();

        std::fs::write(root.join("nested/child.txt"), b"local edit\n").unwrap();
        assert!(prepare_update(&root, b"nested", "", "", limits(), || false).is_err());
        assert_eq!(
            std::fs::read(root.join("nested/child.txt")).unwrap(),
            b"local edit\n"
        );
        std::fs::write(root.join("nested/child.txt"), b"old contents\n").unwrap();

        let plan = prepare_update(&root, b"nested", "", "", limits(), || false).unwrap();
        assert_eq!(plan.preview().previous_commit, old_commit.to_string());
        assert_eq!(plan.preview().target_commit, new_commit.to_string());
        let preview = apply_update(plan, || false).unwrap();
        assert_eq!(preview.target_commit, new_commit.to_string());
        assert_eq!(
            std::fs::read(root.join("nested/child.txt")).unwrap(),
            b"new contents\n"
        );
        assert!(root.join("nested/.git").is_file());
        assert!(!root.join(".git/cide-submodule.pending").exists());
        assert!(!recover_update_expected(&root, &preview, limits(), || false).unwrap());
    }

    #[test]
    fn rejects_relative_urls_and_path_traversal_for_private_module_storage() {
        assert!(source_from_url(b"../child").is_err());
        assert!(matches!(
            source_from_url(b"https://example.invalid/repo"),
            Ok(Source::Network { .. })
        ));
        let context = controlled(
            limits().timeout,
            || false,
            |ctx| {
                let a = module_storage_name(b"../../hooks", ctx)?;
                let b = module_storage_name(b"nested", ctx)?;
                assert!(!a.contains('/') && !a.contains('\\') && a != b);
                assert!(worktree_submodule_path(Path::new("C:/repo"), b"../outside").is_err());
                assert!(worktree_submodule_path(Path::new("C:/repo"), b".git/config").is_err());
                Ok(())
            },
        );
        assert!(context.is_ok());
    }
}
