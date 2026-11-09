//! Reviewed branch switch over pinned references and a recoverable worktree plan.

pub use crate::branch_checkout::CheckoutFile;
pub(super) use crate::branch_checkout::JOURNAL_NAME;
use crate::{
    Context, Error, Identity, Limits,
    branch_checkout::{Checkout, Journal, Transition},
    commit_write, controlled, engine, graph, index_write,
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
    target_tree: ObjectId,
    head_file: jc_fs::FileToken,
    index: IndexToken,
    write_reflog: bool,
    checkout: Option<Checkout>,
    completed: bool,
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

    pub fn files(&self) -> &[CheckoutFile] {
        self.checkout
            .as_ref()
            .map_or(&[], |checkout| checkout.files.as_slice())
    }

    pub fn recovering(&self) -> bool {
        self.checkout
            .as_ref()
            .is_some_and(|checkout| checkout.resuming)
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

pub(super) fn name_text(name: &FullName) -> &str {
    name.as_bstr().to_str().expect("UTF-8 branch name")
}

pub(super) fn open_for_switch(
    root: &Path,
    ctx: &Context<'_>,
) -> Result<(gix::Repository, Identity), Error> {
    let (repo, identity) = index_write::open_for_write_during_switch(root, ctx)?;
    for control in ["worktrees", "config.worktree", "BISECT_START"] {
        if index_write::exists(&identity.git_dir.join(control))? {
            return Err(Error::Engine(
                "Este repositorio requiere otro flujo de cambio de rama".into(),
            ));
        }
    }
    Ok((repo, identity))
}

pub(super) fn direct_branch(refs: &[graph::RefEntry], name: &FullName) -> Result<ObjectId, Error> {
    let name = name.as_bstr().as_bytes();
    let index = refs
        .binary_search_by(|entry| entry.name.as_slice().cmp(name))
        .map_err(|_| Error::Stale)?;
    match &refs[index].target {
        graph::RefTarget::Direct(oid) => ObjectId::from_hex(oid.as_bytes()).map_err(engine),
        graph::RefTarget::Symbolic(_) => Err(Error::Unsupported),
    }
}

pub(super) fn capture_head(
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

/// Review a switch from an attached HEAD to an exact local branch tip.
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
        let current = ref_write::reference_name(RefKind::Branch, source, limits)?;
        let pending = Journal::read(&repository, ctx)?;
        let source = if let Some(journal) = &pending {
            if journal.incoming.is_some() {
                return Err(Error::Engine(
                    "Hay una fusión pendiente. Elige Fusionar rama para reanudarla".into(),
                ));
            }
            let source = ref_write::reference_name(RefKind::Branch, &journal.source, limits)?;
            if journal.destination != name_text(&destination)[HEADS.len()..]
                || journal.to != target
                || (current != source && current != destination)
            {
                return Err(Error::Engine(format!(
                    "Hay un cambio pendiente. Vuelve a elegir la rama {} para recuperarlo",
                    journal.destination
                )));
            }
            source
        } else {
            current.clone()
        };
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
        let target_tree = commit_tree(&repo, target, limits, ctx)?;
        if pending
            .as_ref()
            .is_some_and(|journal| journal.from != source_target || tree == target_tree)
        {
            return Err(Error::Stale);
        }
        revision::verified_tree(&repo, tree, limits, &mut budget, ctx)?;
        let head_file = capture_head(&repository.git_dir, &current, ctx)?;
        let (index, bytes) = local_pair::index_capture(&repo, &repository.git_dir, ctx)?;
        let state = local_pair::decode_index(&repo, bytes.as_deref(), ctx)?;
        if state.entries().iter().any(|entry| entry.stage_raw() != 0) {
            return Err(Error::Unmerged);
        }
        let write_reflog = repo.refs.write_reflog != WriteReflog::Disable;
        if write_reflog {
            commit_write::local_identity(&repo)?;
        }
        let completed = current == destination;
        let checkout = if tree != target_tree {
            let checkout = Checkout::prepare(
                &repo,
                &repository,
                Transition {
                    branches: (
                        &name_text(&source)[HEADS.len()..],
                        &name_text(&destination)[HEADS.len()..],
                    ),
                    commits: (source_target, target),
                    trees: (tree, target_tree),
                    merge: None,
                },
                bytes.as_deref().ok_or(Error::Unsupported)?,
                pending,
                limits,
                ctx,
            )?;
            if completed && !checkout.index_published(&index) {
                return Err(Error::Stale);
            }
            Some(checkout)
        } else {
            None
        };
        Ok(BranchSwitchPlan {
            repository,
            limits,
            source,
            destination,
            source_target,
            target,
            tree,
            target_tree,
            head_file,
            index,
            write_reflog,
            checkout,
            completed,
        })
    })
}

/// Change the symbolic HEAD. Keep the destination and current branch stable
/// under Git locks, and publish a HEAD reflog entry when configured.
pub fn apply_branch_switch(
    plan: BranchSwitchPlan,
    mut cancelled: impl FnMut() -> bool,
) -> Result<BranchSwitchPublication, Error> {
    let recovery = plan.checkout.is_some();
    let journal = plan.repository.git_dir.join(JOURNAL_NAME);
    let destination = plan.destination().to_owned();
    apply(&plan, &mut cancelled).map_err(|error| {
        if recovery && journal.exists() {
            Error::Engine(format!(
                "{error}. Para reanudar, vuelve a elegir la rama {destination}. Registro en {}",
                journal.display()
            ))
        } else {
            error
        }
    })
}

fn apply(
    plan: &BranchSwitchPlan,
    cancelled: &mut impl FnMut() -> bool,
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
    let current = if plan.completed {
        &plan.destination
    } else {
        &plan.source
    };
    let hold = |name: &str, boundary| {
        Marker::acquire_to_hold_resource(git.join(name), Fail::Immediately, boundary, 0)
            .map_err(engine)
    };
    check()?;
    let (repo, identity) = open_for_switch(&plan.repository.root, &ctx)?;
    index_write::same_repository(&plan.repository, &identity)?;
    if capture_head(git, current, &ctx)? != plan.head_file
        || local_pair::index_capture(&repo, git, &ctx)?.0 != plan.index
    {
        return Err(Error::Stale);
    }
    let mut index_lock = Some(
        File::acquire_to_update_resource(git.join("index"), Fail::Immediately, None, 0)
            .map_err(engine)?,
    );
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
    let (index_now, index_bytes) = local_pair::index_capture(&fresh, git, &ctx)?;
    if direct_branch(&refs, &plan.source)? != plan.source_target
        || direct_branch(&refs, &plan.destination)? != plan.target
        || capture_head(git, current, &ctx)? != plan.head_file
        || !index_now.matches_under_lock(&plan.index)
        || (fresh.refs.write_reflog != WriteReflog::Disable) != plan.write_reflog
    {
        return Err(Error::Stale);
    }
    if commit_tree(&fresh, plan.source_target, plan.limits, &ctx)? != plan.tree
        || commit_tree(&fresh, plan.target, plan.limits, &ctx)? != plan.target_tree
    {
        return Err(Error::Stale);
    }
    revision::verified_tree(&fresh, plan.tree, plan.limits, &mut budget, &ctx)?;
    let reflog = if plan.write_reflog && !plan.completed {
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
    if !plan.completed {
        writeln!(head, "ref: {destination}")?;
        head.with_mut(|file| file.sync_all())?;
    }
    check()?;
    let mut index_again = None;
    if let Some(checkout) = &plan.checkout {
        checkout.validate(
            &fresh,
            &identity,
            index_bytes.as_deref().ok_or(Error::Stale)?,
            plan.limits,
            &ctx,
        )?;
        if let Some(bytes) = &checkout.index_bytes {
            let lock = index_lock.as_mut().ok_or(Error::Stale)?;
            lock.write_all(bytes)?;
            lock.with_mut(|file| file.sync_all())?;
        }
        check()?;
        checkout.publish_files(&identity, plan.limits, &ctx, &mut check)?;
        if checkout.index_bytes.is_some() {
            check()?;
            if local_pair::index_capture(&fresh, git, &ctx)?.0 != plan.index {
                return Err(Error::Stale);
            }
            index_lock
                .take()
                .ok_or(Error::Stale)?
                .commit()
                .map_err(engine)?;
            index_again = Some(hold("index", None)?);
        }
        if !checkout.index_published(&local_pair::index_capture(&fresh, git, &ctx)?.0) {
            return Err(Error::Stale);
        }
    } else if Journal::read(&identity, &ctx)?.is_some() {
        return Err(Error::Stale);
    }
    check()?;
    if plan.completed {
        plan.checkout
            .as_ref()
            .ok_or(Error::Stale)?
            .finish(&identity, &ctx)?;
    } else {
        head.commit().map_err(|error| {
            Error::Engine(format!(
                "Cambio de rama incierto. Actualiza el repositorio antes de reintentar ({})",
                engine(error)
            ))
        })?;
    }
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
    if !plan.completed
        && let Some(checkout) = &plan.checkout
    {
        let _head = hold("HEAD", None)?;
        capture_head(git, &plan.destination, &ctx)?;
        checkout.finish(&identity, &ctx)?;
    }
    drop(index_again);
    Ok(BranchSwitchPublication {
        source: plan.source().to_owned(),
        destination: plan.destination().to_owned(),
        source_target: plan.source_target,
        target: plan.target,
        reflog_warning,
    })
}
