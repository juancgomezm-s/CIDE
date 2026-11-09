//! Reverse only changes proven by the journal, the original backups and
//! the identities recorded before publishing the new files.
use super::{
    CheckoutFile, Journal, Observed, changed_paths, checkout_target, conflict, file_bytes,
    read_record, shape,
};
use crate::{Change, Context, Error, Identity, Limits, local_pair, revision};
use gix::ObjectId;
use jc_fs::{Capture, DirectoryToken, FileToken, MissingToken, TreeNode};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(PartialEq, Eq)]
enum Action {
    Keep,
    Restore(Option<MissingToken>),
    Remove(FileToken),
    Replace(FileToken),
}

#[derive(PartialEq, Eq)]
struct Observation {
    actions: Vec<Action>,
    directories: BTreeMap<PathBuf, DirectoryToken>,
}

struct Shape {
    target: PathBuf,
    slots: Vec<usize>,
    old_file: bool,
}

pub(crate) struct Rollback {
    files: Vec<CheckoutFile>,
    shapes: Vec<Shape>,
    reviewed: Observation,
}

impl Rollback {
    pub(crate) fn prepare(
        repo: &gix::Repository,
        identity: &Identity,
        journal: &Journal,
        trees: (ObjectId, ObjectId),
        limits: Limits,
        ctx: &Context<'_>,
    ) -> Result<Self, Error> {
        let mut budget = revision::Budget::new(limits);
        let old = revision::tree_entries(repo, trees.0, limits, &mut budget, ctx)?;
        let new = revision::tree_entries(repo, trees.1, limits, &mut budget, ctx)?;
        let paths = changed_paths(&old, &new, limits)?;
        let roots = shape::roots(&paths, &old, &new);
        let mut files = Vec::with_capacity(paths.len());
        for path in paths {
            ctx.check()?;
            files.push(CheckoutFile {
                target: checkout_target(identity, path)?,
                path: path.clone(),
                before: file_bytes(repo, old.get(path).copied(), limits, &mut budget, ctx)?,
                after: file_bytes(repo, new.get(path).copied(), limits, &mut budget, ctx)?,
                observed: Observed::Structural,
            });
        }
        let mut shapes = Vec::new();
        for root in roots {
            let slots: Vec<_> = files
                .iter()
                .enumerate()
                .filter_map(|(slot, file)| shape::contains(&root, &file.path).then_some(slot))
                .collect();
            let file = &files[*slots.first().ok_or(Error::Stale)?];
            if file.path != root {
                return Err(Error::Stale);
            }
            shapes.push(Shape {
                target: file.target.clone(),
                slots,
                old_file: file.before.is_some(),
            });
        }
        let reviewed = observe(&files, &shapes, identity, journal, limits, ctx)?;
        Ok(Self {
            files,
            shapes,
            reviewed,
        })
    }

    pub(crate) fn rows(&self) -> impl Iterator<Item = (&[u8], Change)> {
        self.files
            .iter()
            .zip(&self.reviewed.actions)
            .filter_map(|(file, action)| {
                let change = match action {
                    Action::Keep => return None,
                    Action::Restore(_) => Change::Added,
                    Action::Remove(_) => Change::Deleted,
                    Action::Replace(_) => Change::Modified,
                };
                Some((file.path.as_slice(), change))
            })
    }

    pub(crate) fn slots(&self) -> usize {
        self.files.len()
    }

    pub(crate) fn validate(
        &self,
        repo: &Identity,
        journal: &Journal,
        limits: Limits,
        ctx: &Context<'_>,
    ) -> Result<(), Error> {
        if observe(&self.files, &self.shapes, repo, journal, limits, ctx)? != self.reviewed {
            return Err(Error::Stale);
        }
        Ok(())
    }

    pub(crate) fn apply(
        &self,
        repo: &Identity,
        journal: &Journal,
        limits: Limits,
        ctx: &Context<'_>,
        check: &mut impl FnMut() -> Result<(), Error>,
    ) -> Result<(), Error> {
        let _parents = self
            .shapes
            .iter()
            .map(|shape| jc_fs::pin_tree_parents(&repo.root, &shape.target))
            .collect::<Result<Vec<_>, _>>()?;
        self.validate(repo, journal, limits, ctx)?;
        for (slot, (file, action)) in self.files.iter().zip(&self.reviewed.actions).enumerate() {
            check()?;
            let (token, replace) = match action {
                Action::Remove(token) => (token, false),
                Action::Replace(token) => (token, true),
                _ => continue,
            };
            jc_fs::stage_existing_file(
                &repo.root,
                token,
                file.after.as_deref().ok_or(Error::Stale)?,
                file.before.as_deref().unwrap_or_default(),
                &repo.git_dir,
                &mut || ctx.check().is_err(),
            )?
            .publish_checkout(&undo_backup(journal, repo, slot), replace)
            .map_err(engine_replace)?;
        }
        for (path, token) in self.reviewed.directories.iter().rev() {
            check()?;
            jc_fs::remove_empty_directory(&repo.root, path, token)?;
        }
        for (file, action) in self.files.iter().zip(&self.reviewed.actions) {
            let Action::Restore(expected) = action else {
                continue;
            };
            check()?;
            let Capture::Absent { token } = local_pair::capture(&repo.root, &file.target, 0, ctx)?
            else {
                return Err(conflict(&file.path));
            };
            let expected = expected.as_ref().unwrap_or(&token);
            if !expected.is_compatible_after_creating_parents(&token) {
                return Err(conflict(&file.path));
            }
            jc_fs::create_missing_parents(&repo.root, &token, &mut || ctx.check().is_err())?;
            let Capture::Absent { token: leaf } =
                local_pair::capture(&repo.root, &file.target, 0, ctx)?
            else {
                return Err(conflict(&file.path));
            };
            if !token.is_compatible_after_creating_parents(&leaf) {
                return Err(conflict(&file.path));
            }
            jc_fs::stage_missing_file(
                &repo.root,
                &leaf,
                file.before.as_deref().ok_or(Error::Stale)?,
                &mut || ctx.check().is_err(),
            )?
            .publish()?;
        }
        let result = observe(&self.files, &self.shapes, repo, journal, limits, ctx)?;
        if !result.directories.is_empty()
            || result.actions.iter().any(|action| *action != Action::Keep)
        {
            return Err(Error::Stale);
        }
        Ok(())
    }
}

fn undo_backup(journal: &Journal, repo: &Identity, slot: usize) -> PathBuf {
    journal.auxiliary(repo, &format!("{slot}.abort.bak"))
}

fn engine_replace(error: jc_fs::ReplaceError) -> Error {
    match error {
        jc_fs::ReplaceError::Io(error) => Error::Io(error),
        jc_fs::ReplaceError::Preserved { error, backup } => {
            Error::Engine(format!("{error}. Respaldo en {}", backup.display()))
        }
    }
}

fn observe(
    files: &[CheckoutFile],
    shapes: &[Shape],
    repo: &Identity,
    journal: &Journal,
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<Observation, Error> {
    let mut actions: Vec<_> = files.iter().map(|_| None).collect();
    let mut directories = BTreeMap::new();
    for shape in shapes {
        let nodes =
            jc_fs::inspect_tree(&repo.root, &shape.target, 512, &mut || ctx.check().is_err())?;
        let new_shape = nodes
            .get(&shape.target)
            .is_some_and(|node| matches!(node, TreeNode::File) != shape.old_file);
        for (path, node) in &nodes {
            let known = shape.slots.iter().map(|&slot| &files[slot]).any(|file| {
                let belongs = if new_shape {
                    file.after.is_some()
                } else {
                    file.before.is_some()
                };
                belongs
                    && match node {
                        TreeNode::File => file.target == *path,
                        TreeNode::Directory(_) => {
                            file.target != *path && file.target.starts_with(path)
                        }
                    }
            });
            if !known {
                return Err(Error::Engine(format!(
                    "La ruta local {} impide abortar la fusión",
                    path.display()
                )));
            }
            if new_shape && let TreeNode::Directory(token) = node {
                directories.insert(path.clone(), token.clone());
            }
        }
        for &slot in &shape.slots {
            let file = &files[slot];
            let present = matches!(nodes.get(&file.target), Some(TreeNode::File));
            actions[slot] = Some(observe_file(
                file, slot, repo, journal, limits, ctx, present,
            )?);
        }
    }
    for (slot, file) in files.iter().enumerate() {
        if actions[slot].is_none() {
            actions[slot] = Some(observe_file(file, slot, repo, journal, limits, ctx, true)?);
        }
    }
    Ok(Observation {
        actions: actions.into_iter().map(Option::unwrap).collect(),
        directories,
    })
}

fn observe_file(
    file: &CheckoutFile,
    slot: usize,
    repo: &Identity,
    journal: &Journal,
    limits: Limits,
    ctx: &Context<'_>,
    capture_path: bool,
) -> Result<Action, Error> {
    ctx.check()?;
    let capture = if capture_path {
        Some(local_pair::capture(
            &repo.root,
            &file.target,
            limits.max_blob,
            ctx,
        )?)
    } else {
        None
    };
    if let Some(Capture::Present { bytes, token, .. }) = capture {
        if Some(bytes.as_slice()) == file.before.as_deref() {
            return Ok(Action::Keep);
        }
        if Some(bytes.as_slice()) != file.after.as_deref()
            || !token.has_single_link()
            || journal.output(repo, slot, ctx)? != Some(token.object_key())
        {
            return Err(conflict(&file.path));
        }
        if file.before.is_none() {
            return Ok(Action::Remove(token));
        }
        require_original(file, slot, repo, journal, limits, ctx)?;
        return Ok(Action::Replace(token));
    }
    if file.before.is_none() {
        return Ok(Action::Keep);
    }
    require_original(file, slot, repo, journal, limits, ctx)?;
    if let Some(after) = &file.after {
        // An absent replacement could be a later local deletion. Only our
        // undo backup proves that the rollback itself already removed it.
        if !journal.aborting
            || read_record(
                repo,
                &undo_backup(journal, repo, slot),
                limits.max_blob,
                ctx,
            )?
            .as_ref()
                != Some(after)
        {
            return Err(conflict(&file.path));
        }
    }
    let token = match capture {
        Some(Capture::Absent { token }) => Some(token),
        _ => None,
    };
    Ok(Action::Restore(token))
}

fn require_original(
    file: &CheckoutFile,
    slot: usize,
    repo: &Identity,
    journal: &Journal,
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<(), Error> {
    if read_record(repo, &journal.backup(repo, slot), limits.max_blob, ctx)?.as_ref()
        != file.before.as_ref()
    {
        return Err(conflict(&file.path));
    }
    Ok(())
}
