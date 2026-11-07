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

pub struct CommitPlan {
    repository: Identity,
    index: local_pair::IndexToken,
    branch: gix::refs::FullName,
    parent: Option<gix::ObjectId>,
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
        self.parent
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

fn local_identity(repo: &gix::Repository) -> Result<gix::actor::Signature, Error> {
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
    limits.admit()?;
    if message.len() >= MESSAGE_BYTES
        || message.trim().is_empty()
        || message
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
    {
        return Err(Error::Engine("Escribe un mensaje de commit válido".into()));
    }
    let mut message = message.trim_end_matches('\n').to_owned();
    message.push('\n');
    controlled(limits.timeout, cancelled, |ctx| {
        let (repo, repository) = index_write::open_for_write(root, ctx)?;
        check_signing(&repo)?;
        let (branch, parent) = branch_head(&repo)?;
        let (index, bytes) = local_pair::index_capture(&repo, &repository.git_dir, ctx)?;
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
        let mut before = match parent {
            Some(id) => {
                let prior = revision::commit(&repo, id, limits, &mut budget, ctx)?;
                if prior.tree == tree {
                    return Err(Error::Engine(
                        "No hay cambios preparados para el commit".into(),
                    ));
                }
                revision::tree_entries(&repo, prior.tree, limits, &mut budget, ctx)?
            }
            None => BTreeMap::new(),
        };
        let mut files = Vec::new();
        for (path, after) in entries {
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
        if files.is_empty() {
            return Err(Error::Engine(
                "No hay cambios preparados para el commit".into(),
            ));
        }
        files.sort_unstable_by(|a, b| a.path.cmp(&b.path));
        let author = local_identity(&repo)?;
        let commit = gix::objs::Commit {
            tree,
            parents: parent.into_iter().collect(),
            committer: author.clone(),
            author,
            encoding: None,
            message: message.into(),
            extra_headers: Vec::new(),
        };
        let mut data = Vec::new();
        commit.write_to(&mut data)?;
        let id =
            gix::objs::compute_hash(repo.object_hash(), Kind::Commit, &data).map_err(engine)?;
        if branch_head(&repo)? != (branch.clone(), parent)
            || local_pair::index_capture(&repo, &repository.git_dir, ctx)?.0 != index
        {
            return Err(Error::Stale);
        }
        Ok(CommitPlan {
            repository,
            index,
            branch,
            parent,
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
    let path = confined_path(root, relative, true)?;
    let mut lock =
        File::acquire_to_update_resource(&path, Fail::Immediately, None, 0).map_err(engine)?;
    match jc_fs::capture_in_root(root, &path, LOG_BYTES, &mut || false)? {
        jc_fs::Capture::Present { bytes, .. } => {
            if !bytes.is_empty() && !bytes.ends_with(b"\n") {
                return Err(Error::Unsupported);
            }
            lock.write_all(&bytes)?;
        }
        jc_fs::Capture::Absent { .. } => {}
    }
    line.write_to(&mut lock)?;
    lock.with_mut(|file| file.sync_all())?;
    Ok(lock)
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
        if branch_head(&fresh)? != (plan.branch.clone(), plan.parent)
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
        previous_oid: plan.parent.unwrap_or_else(|| repo.object_hash().null()),
        new_oid: plan.id,
        signature: plan.commit.committer.clone(),
        message: BString::from(format!(
            "commit: {}",
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
    let expected = plan.parent.map_or(PreviousValue::MustNotExist, |id| {
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
