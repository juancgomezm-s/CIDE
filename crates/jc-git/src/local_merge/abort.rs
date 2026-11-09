//! Abort an interrupted CIDE merge without undoing independent local changes.
use super::*;
use crate::branch_checkout::rollback::Rollback;

pub struct MergeAbortPlan {
    pub rows: Vec<MergeRow>,
    repo: Identity,
    limits: Limits,
    journal: Journal,
    active: FullName,
    head: ObjectId,
    head_file: jc_fs::FileToken,
    index: IndexToken,
    original_index: Option<Vec<u8>>,
    rollback: Option<Rollback>,
}

impl MergeAbortPlan {
    pub fn active(&self) -> &str {
        &self.journal.source
    }
    pub fn incoming(&self) -> &str {
        &self.journal.destination
    }
    pub fn is_cherry_pick(&self) -> bool {
        self.operation() == Some(CommitOperation::CherryPick)
    }
    pub fn operation(&self) -> Option<CommitOperation> {
        self.journal.commit_operation
    }
    pub fn reset_mode(&self) -> Option<ResetMode> {
        self.journal.reset
    }
    pub fn is_rebase(&self) -> bool {
        self.journal.rebase
    }
    pub fn label(&self) -> &'static str {
        if self.is_rebase() {
            return "Rebase pendiente";
        }
        if let Some(mode) = self.reset_mode() {
            return match mode {
                ResetMode::Soft => "Reset soft pendiente",
                ResetMode::Mixed => "Reset mixed pendiente",
                ResetMode::Hard => "Reset hard pendiente",
            };
        }
        match self.operation() {
            Some(CommitOperation::CherryPick) => "Cherry-pick pendiente",
            Some(CommitOperation::Revert) => "Reversión pendiente",
            None => "Fusión pendiente",
        }
    }
    pub fn mainline(&self) -> Option<(usize, ObjectId)> {
        self.journal
            .mainline
            .map(|parent| (parent.number, parent.id))
    }
    pub fn restores_index(&self) -> bool {
        self.original_index.is_some()
    }
    pub fn restores_branch(&self) -> bool {
        self.head != self.journal.from
    }
}

pub fn prepare_merge_abort(
    root: &Path,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<MergeAbortPlan, Error> {
    if !cfg!(windows) {
        return Err(Error::Unsupported);
    }
    limits.admit()?;
    controlled(limits.timeout, cancelled, |ctx| {
        let (repo, identity) = open(root, ctx)?;
        let journal = Journal::read(&identity, ctx)?
            .filter(|journal| journal.incoming.is_some())
            .ok_or_else(|| {
                Error::Engine("No hay una operación de integración de CIDE pendiente".into())
            })?;
        let (active, refs) = refs(&repo, limits, ctx)?;
        if name_text(&active)[HEADS.len()..] != journal.source {
            return Err(Error::Stale);
        }
        let head = direct_branch(&refs, &active)?;
        if head != journal.from && head != journal.to {
            return Err(Error::Stale);
        }
        let head_file = capture_head(&identity.git_dir, &active, ctx)?;
        let (index, _) = local_pair::index_capture(&repo, &identity.git_dir, ctx)?;
        let original_index = if journal.index_state(&index)? {
            Some(journal.original_index(&identity, ctx)?.ok_or_else(|| {
                Error::Engine(
                    "El registro anterior no contiene una copia verificable del índice original"
                        .into(),
                )
            })?)
        } else {
            None
        };
        let rollback = if journal.reset == Some(ResetMode::Hard) {
            Some(Rollback::prepare_hard(
                &repo, &identity, &journal, limits, ctx,
            )?)
        } else if journal.reset.is_none() {
            let trees = (
                tree_id(&repo, journal.from, limits, ctx)?,
                tree_id(&repo, journal.to, limits, ctx)?,
            );
            Some(Rollback::prepare(
                &repo, &identity, &journal, trees, limits, ctx,
            )?)
        } else {
            None
        };
        let rows = rollback
            .iter()
            .flat_map(Rollback::rows)
            .map(|(path, change)| MergeRow {
                path: path.to_vec(),
                change: Some(change),
            })
            .collect();
        Ok(MergeAbortPlan {
            rows,
            repo: identity,
            limits,
            journal,
            active,
            head,
            head_file,
            index,
            original_index,
            rollback,
        })
    })
}

pub fn apply_merge_abort(
    plan: MergeAbortPlan,
    mut cancelled: impl FnMut() -> bool,
) -> Result<MergePublication, Error> {
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
    let git = &plan.repo.git_dir;
    let hold = |name: &str, boundary| {
        Marker::acquire_to_hold_resource(git.join(name), Fail::Immediately, boundary, 0)
            .map_err(engine)
    };
    check()?;
    let (repo, identity) = open(&plan.repo.root, &ctx)?;
    index_write::same_repository(&plan.repo, &identity)?;
    if local_pair::index_capture(&repo, git, &ctx)?.0 != plan.index {
        return Err(Error::Stale);
    }
    let index = File::acquire_to_update_resource(git.join("index"), Fail::Immediately, None, 0)
        .map_err(engine)?;
    let _head = hold("HEAD", None)?;
    let _config = hold("config", None)?;
    let _packed = hold("packed-refs", None)?;
    let active = name_text(&plan.active);
    ref_write::ordinary_path_absent(git, active, &ctx)?;
    let mut branch = File::acquire_to_update_resource(
        git.join(active),
        Fail::Immediately,
        Some(git.join("refs")),
        0,
    )
    .map_err(engine)?;
    let (fresh, identity) = open(&plan.repo.root, &ctx)?;
    index_write::same_repository(&plan.repo, &identity)?;
    let verify_head = |repo: &gix::Repository, expected| -> Result<(), Error> {
        let (current, refs) = refs(repo, plan.limits, &ctx)?;
        if current != plan.active
            || direct_branch(&refs, &current)? != expected
            || capture_head(git, &current, &ctx)? != plan.head_file
        {
            return Err(Error::Stale);
        }
        Ok(())
    };
    verify_head(&fresh, plan.head)?;
    if Journal::read(&identity, &ctx)?.as_ref() != Some(&plan.journal)
        || !local_pair::index_capture(&fresh, git, &ctx)?
            .0
            .matches_under_lock(&plan.index)
    {
        return Err(Error::Stale);
    }
    if let Some(bytes) = &plan.original_index
        && plan.journal.original_index(&identity, &ctx)?.as_ref() != Some(bytes)
    {
        return Err(Error::Stale);
    }
    if let Some(rollback) = &plan.rollback {
        rollback.validate(&identity, &plan.journal, plan.limits, &ctx)?;
    }
    let logs = if plan.restores_branch() && fresh.refs.write_reflog != WriteReflog::Disable {
        let line = gix::refs::log::Line {
            previous_oid: plan.head,
            new_oid: plan.journal.from,
            signature: commit_write::local_identity(&fresh)?,
            message: match plan.operation() {
                Some(CommitOperation::CherryPick) => "cherry-pick: abortar operación de CIDE",
                Some(CommitOperation::Revert) => "revert: abortar operación de CIDE",
                None if plan.journal.reset.is_some() => "reset: abortar operación de CIDE",
                None if plan.is_rebase() => "rebase: abortar operación de CIDE",
                None => "merge: abortar fusión de CIDE",
            }
            .into(),
        };
        [
            Path::new("logs").join(active),
            Path::new("logs/HEAD").into(),
        ]
        .into_iter()
        .map(|path| {
            let (lock, before) = commit_write::stage_log_with_source(git, &path, &line)?;
            Ok((path, lock, before))
        })
        .collect::<Result<Vec<_>, Error>>()?
    } else {
        Vec::new()
    };
    if plan.restores_branch() {
        writeln!(branch, "{}", plan.journal.from)?;
        branch.with_mut(|file| file.sync_all())?;
    }
    check()?;
    let journal = plan.journal.begin_abort(&identity, &ctx)?;
    if let Some(rollback) = &plan.rollback {
        rollback.apply(&identity, &journal, plan.limits, &ctx, &mut check)?;
    }
    check()?;
    if !local_pair::index_capture(&fresh, git, &ctx)?
        .0
        .matches_under_lock(&plan.index)
    {
        return Err(Error::Stale);
    }
    let _index_again;
    if let Some(bytes) = &plan.original_index {
        let mut index = index;
        index.write_all(bytes)?;
        index.with_mut(|file| file.sync_all())?;
        index.commit().map_err(engine)?;
        _index_again = hold("index", None)?;
    }
    if journal.index_state(&local_pair::index_capture(&fresh, git, &ctx)?.0)? {
        return Err(Error::Stale);
    }
    check()?;
    let _branch_again;
    if plan.restores_branch() {
        branch.commit().map_err(engine)?;
        _branch_again = hold(active, Some(git.join("refs")))?;
        let (fresh, identity) = open(&plan.repo.root, &ctx)?;
        index_write::same_repository(&plan.repo, &identity)?;
        verify_head(&fresh, journal.from)?;
    }
    let mut reflog_warning = None;
    for (path, lock, before) in logs {
        let publish = || -> Result<(), Error> {
            let current = match commit_write::capture_log(git, &git.join(path))? {
                jc_fs::Capture::Present { token, .. } => Some(token),
                jc_fs::Capture::Absent { .. } => None,
            };
            if current != before {
                return Err(Error::Stale);
            }
            lock.commit().map_err(engine)?;
            Ok(())
        };
        if let Err(error) = publish() {
            reflog_warning = Some(format!("Operación abortada. Falló el reflog ({error})"));
        }
    }
    journal.finish(
        &identity,
        plan.rollback.as_ref().map_or(0, Rollback::slots),
        &ctx,
    )?;
    Ok(MergePublication {
        id: journal.from,
        reflog_warning,
        index_backup: None,
        hard_backup: journal.hard_backup_path(&identity),
    })
}
