//! Reviewed publication of a local commit from the exact staged index.
use super::{
    Change, Context, Error, Identity, Limits, controlled, engine, index_write, local_pair, revision,
};
use gix::{
    bstr::{BString, ByteSlice},
    lock::{File, Marker, acquire::Fail},
    objs::{Kind, WriteTo, tree::EntryKind},
    refs::{
        Target,
        store::WriteReflog,
        transaction::{PreviousValue, RefEdit},
    },
};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicU8},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

pub const MESSAGE_BYTES: usize = 16 * 1024;
const TREE_BYTES: usize = 4 * 1024 * 1024;
const LOG_BYTES: usize = 8 * 1024 * 1024;

pub struct CommitFile {
    pub path: Vec<u8>,
    pub change: Change,
}

#[derive(Clone)]
pub struct AmendTarget {
    /// Branch name as shown by repository status, without refs/heads/.
    pub branch: Vec<u8>,
    pub head: String,
}

pub struct CommitPlan {
    repository: Identity,
    index: local_pair::IndexToken,
    branch: gix::refs::FullName,
    expected_head: Option<gix::ObjectId>,
    amend: bool,
    commit: gix::objs::Commit,
    id: gix::ObjectId,
    trees: Vec<(gix::ObjectId, gix::objs::Tree)>,
    files: Vec<CommitFile>,
    limits: Limits,
}

impl CommitPlan {
    pub fn branch(&self) -> &[u8] {
        self.branch.as_bstr().as_bytes()
    }
    pub fn parent(&self) -> Option<gix::ObjectId> {
        self.commit.parents.first().copied()
    }
    pub fn replaced(&self) -> Option<gix::ObjectId> {
        self.expected_head.filter(|_| self.amend)
    }
    pub fn id(&self) -> gix::ObjectId {
        self.id
    }
    pub fn author(&self) -> (&[u8], &[u8]) {
        (
            self.commit.author.name.as_bytes(),
            self.commit.author.email.as_bytes(),
        )
    }
    pub fn message(&self) -> &str {
        self.commit
            .message
            .to_str()
            .expect("the reviewed message is UTF-8")
    }
    pub fn files(&self) -> &[CommitFile] {
        &self.files
    }
}

pub struct Publication {
    pub id: gix::ObjectId,
    pub reflog_warning: Option<String>,
}

pub(super) fn local_identity(repo: &gix::Repository) -> Result<gix::actor::Signature, Error> {
    let config = repo.config_snapshot();
    let field = |key, maximum| {
        config
            .string(key)
            .filter(|v| {
                let s = v.as_bstr();
                !s.trim().is_empty()
                    && s.trim() == s
                    && s.len() <= maximum
                    && !s
                        .as_bytes()
                        .iter()
                        .any(|b| b.is_ascii_control() || matches!(b, b'<' | b'>'))
            })
            .ok_or_else(|| Error::Engine(format!("Configura {key} en el repositorio")))
    };
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(engine)?
        .as_secs();
    Ok(gix::actor::Signature {
        name: field("user.name", 128)?,
        email: field("user.email", 254)?,
        time: gix::date::Time::new(i64::try_from(seconds).map_err(engine)?, 0),
    })
}

fn check_signing(repo: &gix::Repository) -> Result<(), Error> {
    if repo
        .config_snapshot()
        .boolean("commit.gpgSign")
        .unwrap_or(false)
    {
        return Err(Error::Engine(
            "El repositorio exige firmar los commits".into(),
        ));
    }
    Ok(())
}

fn branch_head(
    repo: &gix::Repository,
) -> Result<(gix::refs::FullName, Option<gix::ObjectId>), Error> {
    let head = repo.find_reference("HEAD").map_err(engine)?;
    let gix::refs::TargetRef::Symbolic(name) = head.target() else {
        return Err(Error::Engine(
            "Selecciona una rama local antes de crear un commit".into(),
        ));
    };
    if !name.as_bstr().starts_with(b"refs/heads/") {
        return Err(Error::Unsupported);
    }
    let relative = gix::path::from_bstr(name.as_bstr()).map_err(engine)?;
    confined_path(repo.git_dir(), &relative, false)?;
    let branch = name.to_owned();
    let parent = match repo.try_find_reference(branch.as_ref()).map_err(engine)? {
        None => None,
        Some(reference) => match reference.target() {
            gix::refs::TargetRef::Object(id) => Some(id.to_owned()),
            gix::refs::TargetRef::Symbolic(_) => return Err(Error::Unsupported),
        },
    };
    Ok((branch, parent))
}

pub fn prepare_commit(
    root: &Path,
    message: &str,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<CommitPlan, Error> {
    prepare(root, message, None, limits, cancelled)
}

/// Replaces the reviewed tip while preserving its author and parent list.
/// An empty message preserves the original message bytes.
pub fn prepare_amend(
    root: &Path,
    message: &str,
    target: &AmendTarget,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<CommitPlan, Error> {
    prepare(root, message, Some(target), limits, cancelled)
}

fn prepare(
    root: &Path,
    message: &str,
    amend: Option<&AmendTarget>,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<CommitPlan, Error> {
    limits.admit()?;
    if message.len() >= MESSAGE_BYTES
        || (message.trim().is_empty() && !(amend.is_some() && message.is_empty()))
        || message
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
    {
        return Err(Error::Engine("Escribe un mensaje de commit válido".into()));
    }
    let mut message = BString::from(message.trim_end_matches('\n'));
    if !message.is_empty() {
        message.push(b'\n');
    }
    controlled(limits.timeout, cancelled, |ctx| {
        let (repo, repository) = index_write::open_for_write(root, ctx)?;
        check_signing(&repo)?;
        let (branch, expected_head) = branch_head(&repo)?;
        let original = if let Some(target) = amend {
            let expected = gix::ObjectId::from_hex(target.head.as_bytes()).map_err(engine)?;
            if expected_head != Some(expected)
                || branch.as_bstr().as_bytes().strip_prefix(b"refs/heads/")
                    != Some(target.branch.as_slice())
            {
                return Err(Error::Stale);
            }
            let original = revision::verified(&repo, expected, limits.max_bytes, ctx)?
                .try_into_commit()
                .map_err(engine)?
                .decode()
                .map_err(engine)?
                .into_owned()
                .map_err(engine)?;
            if !original.extra_headers.is_empty() || original.encoding.is_some() {
                return Err(Error::Engine(
                    "Este commit tiene firma o metadatos que requieren otro flujo".into(),
                ));
            }
            if original.parents.len() > 8 {
                return Err(Error::Limited);
            }
            Some(original)
        } else {
            None
        };
        let (index, bytes) = local_pair::index_capture(&repo, &repository.git_dir, ctx)?;
        if original.is_some() && bytes.is_none() {
            return Err(Error::Missing);
        }
        let state = local_pair::decode_index(&repo, bytes.as_deref(), ctx)?;
        if state.entries().len() > 4096 {
            return Err(Error::Limited);
        }
        let mut entries = BTreeMap::new();
        let mut budget = revision::Budget::new(limits);
        for entry in state.entries() {
            ctx.check()?;
            budget.visit()?;
            if entry.stage_raw() != 0 {
                return Err(Error::Unmerged);
            }
            let mode = entry.mode.bits();
            if entry.flags.intersects(
                gix::index::entry::Flags::INTENT_TO_ADD | gix::index::entry::Flags::SKIP_WORKTREE,
            ) || !matches!(mode, 0o100644 | 0o100755)
                || entry.id.is_null()
            {
                return Err(Error::Unsupported);
            }
            let path = entry.path(&state).as_bytes();
            revision::path_valid(path, limits.max_path)?;
            budget.charge(path.len().saturating_mul(2).saturating_add(192))?;
            for component in path.split(|b| *b == b'/') {
                gix::validate::path::component(component.as_bstr(), None, Default::default())
                    .map_err(engine)?;
            }
            if path.split(|b| *b == b'/').count() > 128 {
                return Err(Error::Limited);
            }
            if repo.find_header(entry.id).map_err(engine)?.kind() != Kind::Blob {
                return Err(Error::Unsupported);
            }
            if entries
                .insert(path.to_vec(), revision::Entry { id: entry.id, mode })
                .is_some()
            {
                return Err(Error::Unsupported);
            }
        }
        let mut editor = gix::objs::tree::Editor::new(
            gix::objs::Tree {
                entries: Vec::new(),
            },
            &repo.objects,
            repo.object_hash(),
        );
        for (path, entry) in &entries {
            for (offset, _) in path.iter().enumerate().filter(|(_, b)| **b == b'/') {
                if entries.contains_key(&path[..offset]) {
                    return Err(Error::Unsupported);
                }
            }
            let kind = if entry.mode == 0o100755 {
                EntryKind::BlobExecutable
            } else {
                EntryKind::Blob
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
        let mut tree_bytes = 0usize;
        let tree = editor
            .write(|tree| {
                ctx.check()?;
                let mut data = Vec::new();
                tree.write_to(&mut data)?;
                tree_bytes = tree_bytes.saturating_add(data.len());
                if tree_bytes > TREE_BYTES {
                    return Err(Error::Limited);
                }
                let id = gix::objs::compute_hash(repo.object_hash(), Kind::Tree, &data)
                    .map_err(engine)?;
                trees.push((id, tree.clone()));
                Ok::<_, Error>(id)
            })
            .map_err(engine)?;
        let mut before = match expected_head {
            Some(id) => {
                let prior_tree = match &original {
                    Some(prior) => prior.tree,
                    None => revision::commit(&repo, id, limits, &mut budget, ctx)?.tree,
                };
                if prior_tree == tree && original.is_none() {
                    return Err(Error::Engine(
                        "No hay cambios preparados para el commit".into(),
                    ));
                }
                if prior_tree == tree {
                    BTreeMap::new()
                } else {
                    revision::tree_entries(&repo, prior_tree, limits, &mut budget, ctx)?
                }
            }
            None => BTreeMap::new(),
        };
        let mut files = Vec::new();
        let tree_unchanged = original.as_ref().is_some_and(|prior| prior.tree == tree);
        for (path, after) in entries.into_iter().filter(|_| !tree_unchanged) {
            let change = match before.remove(&path) {
                Some(prior) if prior == after => continue,
                Some(_) => Change::Modified,
                None => Change::Added,
            };
            files.push(CommitFile { path, change });
        }
        files.extend(before.into_keys().map(|path| CommitFile {
            path,
            change: Change::Deleted,
        }));
        if files.is_empty() && original.is_none() {
            return Err(Error::Engine(
                "No hay cambios preparados para el commit".into(),
            ));
        }
        files.sort_unstable_by(|a, b| a.path.cmp(&b.path));
        let committer = local_identity(&repo)?;
        let (author, parents) = if let Some(prior) = original {
            if message.is_empty() {
                if prior.message.len() > MESSAGE_BYTES
                    || prior.message.to_str().is_err()
                    || prior.message.as_bytes().contains(&0)
                {
                    return Err(Error::Engine(
                        "Escribe un mensaje nuevo para este commit".into(),
                    ));
                }
                message = prior.message.clone();
            }
            if tree_unchanged && message == prior.message {
                return Err(Error::Engine(
                    "Cambia el mensaje o prepara archivos para modificar el commit".into(),
                ));
            }
            (prior.author, prior.parents)
        } else {
            (committer.clone(), expected_head.into_iter().collect())
        };
        let commit = gix::objs::Commit {
            tree,
            parents,
            committer,
            author,
            encoding: None,
            message,
            extra_headers: Vec::new(),
        };
        let mut data = Vec::new();
        commit.write_to(&mut data)?;
        let id =
            gix::objs::compute_hash(repo.object_hash(), Kind::Commit, &data).map_err(engine)?;
        if branch_head(&repo)? != (branch.clone(), expected_head)
            || local_pair::index_capture(&repo, &repository.git_dir, ctx)?.0 != index
        {
            return Err(Error::Stale);
        }
        Ok(CommitPlan {
            repository,
            index,
            branch,
            expected_head,
            amend: amend.is_some(),
            commit,
            id,
            trees,
            files,
            limits,
        })
    })
}

fn confined_path(root: &Path, relative: &Path, create_dirs: bool) -> Result<PathBuf, Error> {
    let mut path = root.to_path_buf();
    let mut parts = relative.components().peekable();
    while let Some(component) = parts.next() {
        let std::path::Component::Normal(name) = component else {
            return Err(Error::Unsupported);
        };
        path.push(name);
        let directory = parts.peek().is_some();
        if directory && create_dirs && !index_write::exists(&path)? {
            match fs::create_dir(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e.into()),
            }
        }
        if index_write::exists(&path)? {
            index_write::ordinary(&path, directory)?;
        }
    }
    Ok(path)
}

fn stage_log(root: &Path, relative: &Path, line: &gix::refs::log::Line) -> Result<File, Error> {
    Ok(stage_log_with_source(root, relative, line)?.0)
}

pub(super) fn capture_log(root: &Path, path: &Path) -> Result<jc_fs::Capture, Error> {
    Ok(jc_fs::capture_in_root(root, path, LOG_BYTES, &mut || {
        false
    })?)
}

pub(super) fn stage_log_with_source(
    root: &Path,
    relative: &Path,
    line: &gix::refs::log::Line,
) -> Result<(File, Option<jc_fs::FileToken>), Error> {
    let path = confined_path(root, relative, true)?;
    let mut lock =
        File::acquire_to_update_resource(&path, Fail::Immediately, None, 0).map_err(engine)?;
    let source = match capture_log(root, &path)? {
        jc_fs::Capture::Present { token, bytes, .. } => {
            if !bytes.is_empty() && !bytes.ends_with(b"\n") {
                return Err(Error::Unsupported);
            }
            lock.write_all(&bytes)?;
            Some(token)
        }
        jc_fs::Capture::Absent { .. } => None,
    };
    line.write_to(&mut lock)?;
    lock.with_mut(|file| file.sync_all())?;
    Ok((lock, source))
}

fn store_object(
    repo: &gix::Repository,
    root: &Path,
    id: gix::ObjectId,
    object: &impl WriteTo,
) -> Result<(), Error> {
    let hex = id.to_string();
    let relative = Path::new("objects").join(&hex[..2]).join(&hex[2..]);
    confined_path(root, &relative, false)?;
    if repo.write_object(object).map_err(engine)?.detach() != id {
        return Err(Error::Engine(
            "El objeto Git difiere de la vista previa".into(),
        ));
    }
    Ok(())
}

pub fn apply_commit(
    plan: CommitPlan,
    mut cancelled: impl FnMut() -> bool,
) -> Result<Publication, Error> {
    let stop = AtomicBool::new(false);
    let reason = AtomicU8::new(0);
    let ctx = Context {
        stop: &stop,
        reason: &reason,
        deadline: Instant::now() + plan.limits.timeout,
    };
    let check = || {
        if cancelled() {
            Err(Error::Cancelled)
        } else {
            ctx.check()
        }
    };
    let mut check = check;
    check()?;
    let root = &plan.repository.git_dir;
    let (mut repo, identity) = index_write::open_for_write(&plan.repository.root, &ctx)?;
    index_write::same_repository(&plan.repository, &identity)?;
    if local_pair::index_capture(&repo, root, &ctx)?.0 != plan.index {
        return Err(Error::Stale);
    }
    let _index = Marker::acquire_to_hold_resource(root.join("index"), Fail::Immediately, None, 0)
        .map_err(engine)?;
    let _head = Marker::acquire_to_hold_resource(root.join("HEAD"), Fail::Immediately, None, 0)
        .map_err(engine)?;
    let verify = || -> Result<(), Error> {
        let (fresh, identity) = index_write::open_for_write(&plan.repository.root, &ctx)?;
        index_write::same_repository(&plan.repository, &identity)?;
        check_signing(&fresh)?;
        if branch_head(&fresh)? != (plan.branch.clone(), plan.expected_head)
            || !local_pair::index_capture(&fresh, root, &ctx)?
                .0
                .matches_under_lock(&plan.index)
        {
            return Err(Error::Stale);
        }
        Ok(())
    };
    verify()?;
    let branch = gix::path::from_bstr(plan.branch.as_bstr()).map_err(engine)?;
    confined_path(root, &branch, false)?;
    let write_reflog = repo.refs.write_reflog != WriteReflog::Disable;
    // Stage both reflogs before changing the reference. Replacement avoids
    // appending through a hard link. A late failure is reported after the CAS.
    repo.refs.write_reflog = WriteReflog::Disable;
    let line = gix::refs::log::Line {
        previous_oid: plan
            .expected_head
            .unwrap_or_else(|| repo.object_hash().null()),
        new_oid: plan.id,
        signature: plan.commit.committer.clone(),
        message: BString::from(format!(
            "{}: {}",
            if plan.amend {
                "commit (amend)"
            } else {
                "commit"
            },
            plan.message()
                .lines()
                .next()
                .unwrap_or_default()
                .chars()
                .filter(|c| !c.is_control())
                .take(128)
                .collect::<String>()
        )),
    };
    let logs = if write_reflog {
        Some([
            stage_log(root, &Path::new("logs").join(&branch), &line)?,
            stage_log(root, Path::new("logs/HEAD"), &line)?,
        ])
    } else {
        None
    };
    for (id, tree) in &plan.trees {
        check()?;
        store_object(&repo, root, *id, tree)?;
    }
    store_object(&repo, root, plan.id, &plan.commit)?;
    let expected = plan
        .expected_head
        .map_or(PreviousValue::MustNotExist, |id| {
            PreviousValue::MustExistAndMatch(Target::Object(id))
        });
    let tx = repo
        .refs
        .transaction()
        .prepare(
            [RefEdit::update(
                plan.branch.clone(),
                Target::Object(plan.id),
                expected,
                BString::default(),
            )],
            Fail::Immediately,
            Fail::Immediately,
        )
        .map_err(engine)?;
    verify()?;
    check()?;
    tx.commit(None).map_err(|e| {
        Error::Engine(format!(
            "Publicación de {} incierta. Actualiza el repositorio antes de reintentar ({})",
            plan.id,
            engine(e),
        ))
    })?;
    let mut reflog_warning = None;
    if let Some(logs) = logs {
        for lock in logs {
            if let Err(error) = lock.commit() {
                reflog_warning = Some(format!(
                    "Commit {} publicado. Falló la escritura del reflog ({})",
                    plan.id,
                    engine(error)
                ));
            }
        }
    }
    Ok(Publication {
        id: plan.id,
        reflog_warning,
    })
}
