//! Reviewed branch switch when the current and destination commits share a tree.

use crate::{
    Context, Error, Identity, Limits, commit_write, controlled, engine, graph, index_write,
    local_pair::{self, IndexToken},
    ref_write::{self, RefKind},
    revision::{self, Budget},
};
use gix::{
    ObjectId,
    bstr::ByteSlice,
    lock::{File, Marker, acquire::Fail},
    refs::{FullName, store::WriteReflog},
};
use std::{
    io::Write,
    path::Path,
    sync::atomic::{AtomicBool, AtomicU8},
    time::Instant,
};

const HEADS: &str = "refs/heads/";
const HEAD_BYTES: usize = 8 * 1024;
const COMMIT_BYTES: usize = 64 * 1024;

pub struct BranchSwitchPlan {
    repository: Identity,
    limits: Limits,
    source: FullName,
    destination: FullName,
    source_target: ObjectId,
    target: ObjectId,
    tree: ObjectId,
    head_file: jc_fs::FileToken,
    index: IndexToken,
    write_reflog: bool,
}

impl BranchSwitchPlan {
    pub fn source(&self) -> &str {
        &name_text(&self.source)[HEADS.len()..]
    }

    pub fn destination(&self) -> &str {
        &name_text(&self.destination)[HEADS.len()..]
    }

    pub fn source_target(&self) -> ObjectId {
        self.source_target
    }

    pub fn target(&self) -> ObjectId {
        self.target
    }
}

#[derive(Debug)]
pub struct BranchSwitchPublication {
    pub source: String,
    pub destination: String,
    pub source_target: ObjectId,
    pub target: ObjectId,
    pub reflog_warning: Option<String>,
}

fn name_text(name: &FullName) -> &str {
    name.as_bstr().to_str().expect("UTF-8 branch name")
}

fn open_for_switch(root: &Path, ctx: &Context<'_>) -> Result<(gix::Repository, Identity), Error> {
    let (repo, identity) = index_write::open_for_write(root, ctx)?;
    for control in ["worktrees", "config.worktree", "BISECT_START"] {
        if index_write::exists(&identity.git_dir.join(control))? {
            return Err(Error::Engine(
                "Este repositorio requiere otro flujo de cambio de rama".into(),
            ));
        }
    }
    Ok((repo, identity))
}

fn direct_branch(refs: &[graph::RefEntry], name: &FullName) -> Result<ObjectId, Error> {
    let name = name.as_bstr().as_bytes();
    let index = refs
        .binary_search_by(|entry| entry.name.as_slice().cmp(name))
        .map_err(|_| Error::Stale)?;
    match &refs[index].target {
        graph::RefTarget::Direct(oid) => ObjectId::from_hex(oid.as_bytes()).map_err(engine),
        graph::RefTarget::Symbolic(_) => Err(Error::Unsupported),
    }
}

fn capture_head(
    git: &Path,
    branch: &FullName,
    ctx: &Context<'_>,
) -> Result<jc_fs::FileToken, Error> {
    let result = jc_fs::capture_in_root(git, &git.join("HEAD"), HEAD_BYTES, &mut || {
        ctx.check().is_err()
    });
    ctx.check()?;
    let jc_fs::Capture::Present { token, bytes, .. } = result? else {
        return Err(Error::Stale);
    };
    let expected = format!("ref: {}", name_text(branch));
    if !matches!(
        bytes.strip_prefix(expected.as_bytes()),
        Some(b"\n" | b"\r\n" | b"")
    ) {
        return Err(Error::Stale);
    }
    Ok(token)
}

fn commit_tree(
    repo: &gix::Repository,
    id: ObjectId,
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<ObjectId, Error> {
    revision::verified(repo, id, limits.max_bytes.min(COMMIT_BYTES), ctx)?
        .try_into_commit()
        .map_err(engine)?
        .tree_id()
        .map_err(engine)
        .map(|id| id.detach())
}

/// Review a switch only if HEAD is attached to a local branch and the exact
/// destination commit has the same tree. The index is pinned but left intact.
pub fn prepare_branch_switch(
    root: &Path,
    destination: &str,
    expected_target: &str,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<BranchSwitchPlan, Error> {
    if !cfg!(windows) {
        return Err(Error::Unsupported);
    }
    limits.admit()?;
    let destination = ref_write::reference_name(RefKind::Branch, destination, limits)?;
    let target = ObjectId::from_hex(expected_target.as_bytes()).map_err(engine)?;
    controlled(limits.timeout, cancelled, |ctx| {
        let (repo, repository) = open_for_switch(root, ctx)?;
        let mut budget = Budget::new(limits);
        let (head, refs) = graph::references(&repo, limits, &mut budget, ctx)?;
        let graph::Head::Attached { name, .. } = head else {
            return Err(Error::Engine(
                "El cambio requiere una rama local activa con un commit".into(),
            ));
        };
        let source = name
            .strip_prefix(HEADS.as_bytes())
            .and_then(|short| std::str::from_utf8(short).ok())
            .ok_or(Error::Unsupported)?;
        let source = ref_write::reference_name(RefKind::Branch, source, limits)?;
        if ref_write::namespace_conflicts(source.as_bstr().as_bytes(), name_text(&destination)) {
            return Err(Error::Engine(
                "Elige una rama distinta de la activa, con un nombre sin colisiones".into(),
            ));
        }
        if direct_branch(&refs, &destination)? != target {
            return Err(Error::Stale);
        }
        let source_target = direct_branch(&refs, &source)?;
        let tree = commit_tree(&repo, source_target, limits, ctx)?;
        if tree != commit_tree(&repo, target, limits, ctx)? {
            return Err(Error::Engine(
                "La rama tiene otro árbol. Cambiar sus archivos requiere un flujo recuperable"
                    .into(),
            ));
        }
        revision::verified_tree(&repo, tree, limits, &mut budget, ctx)?;
        let head_file = capture_head(&repository.git_dir, &source, ctx)?;
        let (index, bytes) = local_pair::index_capture(&repo, &repository.git_dir, ctx)?;
        let state = local_pair::decode_index(&repo, bytes.as_deref(), ctx)?;
        if state.entries().iter().any(|entry| entry.stage_raw() != 0) {
            return Err(Error::Unmerged);
        }
        let write_reflog = repo.refs.write_reflog != WriteReflog::Disable;
        if write_reflog {
            commit_write::local_identity(&repo)?;
        }
        Ok(BranchSwitchPlan {
            repository,
            limits,
            source,
            destination,
            source_target,
            target,
            tree,
            head_file,
            index,
            write_reflog,
        })
    })
}

/// Change the symbolic HEAD. Keep the destination and current branch stable
/// under Git locks, and publish a HEAD reflog entry when configured.
pub fn apply_branch_switch(
    plan: BranchSwitchPlan,
    mut cancelled: impl FnMut() -> bool,
) -> Result<BranchSwitchPublication, Error> {
    let stop = AtomicBool::new(false);
    let reason = AtomicU8::new(0);
    let ctx = Context {
        stop: &stop,
        reason: &reason,
        deadline: Instant::now() + plan.limits.timeout,
    };
    let mut check = || {
        if cancelled() {
            Err(Error::Cancelled)
        } else {
            ctx.check()
        }
    };
    let git = &plan.repository.git_dir;
    let hold = |name: &str, boundary| {
        Marker::acquire_to_hold_resource(git.join(name), Fail::Immediately, boundary, 0)
            .map_err(engine)
    };
    check()?;
    let (repo, identity) = open_for_switch(&plan.repository.root, &ctx)?;
    index_write::same_repository(&plan.repository, &identity)?;
    if capture_head(git, &plan.source, &ctx)? != plan.head_file
        || local_pair::index_capture(&repo, git, &ctx)?.0 != plan.index
    {
        return Err(Error::Stale);
    }
    let _index = hold("index", None)?;
    let mut head = File::acquire_to_update_resource(git.join("HEAD"), Fail::Immediately, None, 0)
        .map_err(engine)?;
    let _config = hold("config", None)?;
    let _packed = hold("packed-refs", None)?;
    let source = name_text(&plan.source);
    let destination = name_text(&plan.destination);
    ref_write::ordinary_path_absent(git, source, &ctx)?;
    ref_write::ordinary_path_absent(git, destination, &ctx)?;
    let _source = hold(source, Some(git.join("refs")))?;
    let _destination = hold(destination, Some(git.join("refs")))?;
    let (fresh, identity) = open_for_switch(&plan.repository.root, &ctx)?;
    index_write::same_repository(&plan.repository, &identity)?;
    let mut budget = Budget::new(plan.limits);
    let (_, refs) = graph::references(&fresh, plan.limits, &mut budget, &ctx)?;
    if direct_branch(&refs, &plan.source)? != plan.source_target
        || direct_branch(&refs, &plan.destination)? != plan.target
        || capture_head(git, &plan.source, &ctx)? != plan.head_file
        || !local_pair::index_capture(&fresh, git, &ctx)?
            .0
            .matches_under_lock(&plan.index)
        || (fresh.refs.write_reflog != WriteReflog::Disable) != plan.write_reflog
    {
        return Err(Error::Stale);
    }
    if commit_tree(&fresh, plan.source_target, plan.limits, &ctx)? != plan.tree
        || commit_tree(&fresh, plan.target, plan.limits, &ctx)? != plan.tree
    {
        return Err(Error::Stale);
    }
    revision::verified_tree(&fresh, plan.tree, plan.limits, &mut budget, &ctx)?;
    let reflog = if plan.write_reflog {
        let line = gix::refs::log::Line {
            previous_oid: plan.source_target,
            new_oid: plan.target,
            signature: commit_write::local_identity(&fresh)?,
            message: format!(
                "checkout: moving from {} to {}",
                plan.source(),
                plan.destination()
            )
            .into(),
        };
        Some(commit_write::stage_log_with_source(
            git,
            Path::new("logs/HEAD"),
            &line,
        )?)
    } else {
        None
    };
    writeln!(head, "ref: {destination}")?;
    head.with_mut(|file| file.sync_all())?;
    check()?;
    head.commit().map_err(|error| {
        Error::Engine(format!(
            "Cambio de rama incierto. Actualiza el repositorio antes de reintentar ({})",
            engine(error)
        ))
    })?;
    let reflog_warning = reflog.and_then(|(log, before)| {
        let publish = || -> Result<(), Error> {
            // Publishing HEAD releases HEAD.lock. Reacquire it before touching
            // the log, and preserve any intervening external writer's bytes.
            let _head = hold("HEAD", None)?;
            capture_head(git, &plan.destination, &ctx)?;
            let current = match commit_write::capture_log(git, &git.join("logs/HEAD"))? {
                jc_fs::Capture::Present { token, .. } => Some(token),
                jc_fs::Capture::Absent { .. } => None,
            };
            if current != before {
                return Err(Error::Stale);
            }
            log.commit().map_err(engine)?;
            Ok(())
        };
        publish().err().map(|error| {
            format!(
                "Rama {} activa. Falló la escritura del reflog de HEAD ({})",
                plan.destination(),
                engine(error)
            )
        })
    });
    Ok(BranchSwitchPublication {
        source: plan.source().to_owned(),
        destination: plan.destination().to_owned(),
        source_target: plan.source_target,
        target: plan.target,
        reflog_warning,
    })
}
