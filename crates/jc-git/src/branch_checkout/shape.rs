//! File/directory transitions use the same journal and per-file backups as checkout.
use super::{CheckoutFile, Journal, Observed, conflict, observe};
use crate::{Context, Error, Identity, Limits, local_pair, revision::Entry};
use jc_fs::{Capture, FileToken, TreeNode};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

pub(super) fn contains(root: &[u8], path: &[u8]) -> bool {
    root == path
        || path
            .strip_prefix(root)
            .is_some_and(|tail| tail.starts_with(b"/"))
}

pub(super) fn roots(
    paths: &[&Vec<u8>],
    old: &BTreeMap<Vec<u8>, Entry>,
    new: &BTreeMap<Vec<u8>, Entry>,
) -> Vec<Vec<u8>> {
    paths
        .iter()
        .filter(|path| {
            let other = if old.contains_key(**path) { new } else { old };
            !other.contains_key(**path)
                && paths.iter().any(|child| {
                    *path != child && contains(path, child) && other.contains_key(*child)
                })
        })
        .map(|path| (*path).clone())
        .collect()
}

#[derive(PartialEq, Eq)]
struct State {
    nodes: BTreeMap<PathBuf, TreeNode>,
    before: Vec<(usize, FileToken)>,
    after: Vec<usize>,
    old_shape: bool,
}

pub(super) struct CheckoutShape {
    path: Vec<u8>,
    target: PathBuf,
    slots: Vec<usize>,
    old_file: bool,
    state: State,
}

impl CheckoutShape {
    pub fn prepare(
        path: Vec<u8>,
        files: &[CheckoutFile],
        repo: &Identity,
        pending: Option<&Journal>,
        limits: Limits,
        ctx: &Context<'_>,
    ) -> Result<Self, Error> {
        let root = files
            .iter()
            .find(|file| file.path == path)
            .ok_or(Error::Stale)?;
        let mut shape = Self {
            target: root.target.clone(),
            slots: files
                .iter()
                .enumerate()
                .filter(|(_, file)| contains(&path, &file.path))
                .map(|(slot, _)| slot)
                .collect(),
            old_file: root.before.is_some(),
            path,
            state: State {
                nodes: BTreeMap::new(),
                before: Vec::new(),
                after: Vec::new(),
                old_shape: false,
            },
        };
        shape.state = shape.observe(files, repo, pending, limits, ctx)?;
        Ok(shape)
    }

    pub fn is_applied(&self) -> bool {
        !self.state.old_shape && self.state.after.is_empty()
    }

    fn observe(
        &self,
        files: &[CheckoutFile],
        repo: &Identity,
        pending: Option<&Journal>,
        limits: Limits,
        ctx: &Context<'_>,
    ) -> Result<State, Error> {
        let nodes =
            jc_fs::inspect_tree(&repo.root, &self.target, 512, &mut || ctx.check().is_err())?;
        let old_shape = nodes
            .get(&self.target)
            .is_some_and(|node| matches!(node, TreeNode::File) == self.old_file);
        if pending.is_none() && !old_shape {
            return Err(conflict(&self.path));
        }
        let mut expected = BTreeMap::new();
        let mut dirs = BTreeSet::new();
        for &slot in &self.slots {
            let file = &files[slot];
            if (old_shape && file.before.is_some()) || (!old_shape && file.after.is_some()) {
                expected.insert(&file.target, slot);
                dirs.extend(
                    file.target
                        .ancestors()
                        .skip(1)
                        .take_while(|path| path.starts_with(&self.target)),
                );
            }
        }
        for (path, node) in &nodes {
            let known = match node {
                TreeNode::File => expected.contains_key(path),
                TreeNode::Directory(_) => dirs.contains(path.as_path()),
            };
            if !known {
                return Err(Error::Engine(format!(
                    "La ruta local {} impide cambiar el tipo de {}",
                    path.display(),
                    self.target.display()
                )));
            }
        }
        let mut before = Vec::new();
        let mut after = Vec::new();
        for &slot in &self.slots {
            ctx.check()?;
            let file = &files[slot];
            let backup = pending.map(|journal| journal.backup(repo, slot));
            if let Some(bytes) = &file.before {
                if old_shape && nodes.contains_key(&file.target) {
                    let Observed::Before(token) = observe(
                        repo,
                        &file.target,
                        &file.path,
                        (Some(bytes), None),
                        backup.as_deref(),
                        limits,
                        ctx,
                    )?
                    else {
                        return Err(conflict(&file.path));
                    };
                    before.push((slot, token));
                } else {
                    let path = backup.as_ref().ok_or_else(|| conflict(&file.path))?;
                    let Capture::Present {
                        bytes: preserved,
                        token,
                        ..
                    } = local_pair::capture(&repo.git_dir, path, limits.max_blob, ctx)?
                    else {
                        return Err(conflict(&file.path));
                    };
                    if !token.has_single_link() || &preserved != bytes {
                        return Err(conflict(&file.path));
                    }
                }
            }
            if let Some(bytes) = &file.after {
                if old_shape || !nodes.contains_key(&file.target) {
                    after.push(slot);
                } else if observe(
                    repo,
                    &file.target,
                    &file.path,
                    (None, Some(bytes)),
                    backup.as_deref(),
                    limits,
                    ctx,
                )? != Observed::Applied
                {
                    return Err(conflict(&file.path));
                }
            }
        }
        Ok(State {
            nodes,
            before,
            after,
            old_shape,
        })
    }

    pub fn validate(
        &self,
        files: &[CheckoutFile],
        repo: &Identity,
        pending: Option<&Journal>,
        limits: Limits,
        ctx: &Context<'_>,
    ) -> Result<(), Error> {
        if self.observe(files, repo, pending, limits, ctx)? != self.state {
            return Err(conflict(&self.path));
        }
        Ok(())
    }

    pub fn publish(
        &self,
        files: &[CheckoutFile],
        repo: &Identity,
        journal: &Journal,
        limits: Limits,
        ctx: &Context<'_>,
        check: &mut impl FnMut() -> Result<(), Error>,
    ) -> Result<(), Error> {
        let _parents = jc_fs::pin_tree_parents(&repo.root, &self.target)?;
        self.validate(files, repo, Some(journal), limits, ctx)?;
        for (slot, token) in &self.state.before {
            check()?;
            let file = &files[*slot];
            let original = file.before.as_deref().ok_or(Error::Stale)?;
            let backup = journal.backup(repo, *slot);
            jc_fs::stage_existing_file(
                &repo.root,
                token,
                original,
                b"",
                &repo.git_dir,
                &mut || ctx.check().is_err(),
            )?
            .publish_checkout(&backup, false)
            .map_err(|error| match error {
                jc_fs::ReplaceError::Io(error) => Error::Io(error),
                jc_fs::ReplaceError::Preserved { error, backup } => {
                    Error::Engine(format!("{error}. Original en {}", backup.display()))
                }
            })?;
        }
        if self.state.old_shape {
            // Reverse order puts every descendant ahead of its parent.
            for (path, node) in self.state.nodes.iter().rev() {
                if let TreeNode::Directory(token) = node {
                    check()?;
                    jc_fs::remove_empty_directory(&repo.root, path, token)?;
                }
            }
        }
        for &slot in &self.state.after {
            check()?;
            let file = &files[slot];
            let Capture::Absent { token } = local_pair::capture(&repo.root, &file.target, 0, ctx)?
            else {
                return Err(conflict(&file.path));
            };
            jc_fs::create_missing_parents(&repo.root, &token, &mut || ctx.check().is_err())?;
            let Capture::Absent { token: leaf } =
                local_pair::capture(&repo.root, &file.target, 0, ctx)?
            else {
                return Err(conflict(&file.path));
            };
            if !token.is_compatible_after_creating_parents(&leaf) {
                return Err(conflict(&file.path));
            }
            let pending = jc_fs::stage_missing_file(
                &repo.root,
                &leaf,
                file.after.as_deref().ok_or(Error::Stale)?,
                &mut || ctx.check().is_err(),
            )?;
            journal.record_output(repo, slot, pending.object_key()?, ctx)?;
            pending.publish()?;
        }
        self.verify_applied(files, repo, journal, limits, ctx)
    }

    pub fn verify_applied(
        &self,
        files: &[CheckoutFile],
        repo: &Identity,
        journal: &Journal,
        limits: Limits,
        ctx: &Context<'_>,
    ) -> Result<(), Error> {
        let state = self.observe(files, repo, Some(journal), limits, ctx)?;
        if state.old_shape || !state.after.is_empty() {
            return Err(conflict(&self.path));
        }
        Ok(())
    }
}
