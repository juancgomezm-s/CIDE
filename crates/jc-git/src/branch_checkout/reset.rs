//! Build a reset plan from commits and the index. No worktree paths are opened.
use super::{Checkout, Journal, checkout_target, index_id};
use crate::{
    Change, Context, Error, Identity, Limits, engine,
    local_merge::{MergeRow, ResetMode},
    local_pair::{self, IndexToken},
    revision::{self, Budget, Entry},
};
use gix::{
    ObjectId,
    bstr::ByteSlice,
    index::entry::{Flags, Mode},
};
use std::{
    collections::BTreeMap,
    time::{SystemTime, UNIX_EPOCH},
};

type Entries = BTreeMap<Vec<u8>, Entry>;

pub(crate) struct ResetTransition<'a> {
    pub mode: ResetMode,
    pub branch: &'a str,
    pub commits: (ObjectId, ObjectId),
    pub trees: (ObjectId, ObjectId),
}

impl Checkout {
    pub(crate) fn prepare_reset(
        repo: &gix::Repository,
        repository: &Identity,
        transition: ResetTransition<'_>,
        index: (&IndexToken, Option<&[u8]>),
        pending: Option<Journal>,
        limits: Limits,
        ctx: &Context<'_>,
    ) -> Result<(Self, Vec<MergeRow>), Error> {
        let ResetTransition {
            mode,
            branch,
            commits,
            trees,
        } = transition;
        let applied = pending
            .as_ref()
            .map(|journal| journal.index_state(index.0))
            .transpose()?
            .unwrap_or(false);
        let saved = if applied {
            Some(
                pending
                    .as_ref()
                    .ok_or(Error::Stale)?
                    .original_index(repository, ctx)?
                    .ok_or(Error::Stale)?,
            )
        } else {
            None
        };
        let original = saved.as_deref().or(index.1);
        let old_index = original
            .map(|bytes| index_id(bytes, ctx))
            .transpose()?
            .unwrap_or_else(|| gix::hash::Kind::Sha256.null());
        let mut budget = Budget::new(limits);
        let target = revision::tree_entries(repo, trees.1, limits, &mut budget, ctx)?;
        let before = match mode {
            ResetMode::Soft => revision::tree_entries(repo, trees.0, limits, &mut budget, ctx)?,
            ResetMode::Mixed | ResetMode::Hard => {
                let bytes = original.ok_or_else(|| {
                    Error::Engine(format!(
                        "Reset {} requiere un índice existente",
                        mode.label()
                    ))
                })?;
                let state = local_pair::decode_index(repo, Some(bytes), ctx)?;
                if state.entries().iter().any(|entry| entry.stage_raw() != 0) {
                    return Err(Error::Unmerged);
                }
                if state.entries().iter().any(|entry| {
                    entry
                        .flags
                        .intersects(Flags::INTENT_TO_ADD | Flags::SKIP_WORKTREE)
                }) {
                    return Err(Error::Unsupported);
                }
                state
                    .entries()
                    .iter()
                    .map(|entry| {
                        ctx.check()?;
                        let path = entry.path(&state).as_bytes();
                        revision::path_valid(path, limits.max_path)?;
                        budget.charge(path.len().saturating_add(96))?;
                        Ok((
                            path.to_vec(),
                            Entry {
                                id: entry.id,
                                mode: entry.mode.bits(),
                            },
                        ))
                    })
                    .collect::<Result<Entries, Error>>()?
            }
        };
        let rows = before
            .keys()
            .chain(target.keys().filter(|path| !before.contains_key(*path)))
            .filter_map(|path| {
                let old = before.get(path);
                let new = target.get(path);
                let change = match (old, new) {
                    (None, Some(_)) => Change::Added,
                    (Some(_), None) => Change::Deleted,
                    (Some(a), Some(b)) if a != b => Change::Modified,
                    _ => return None,
                };
                Some(MergeRow {
                    path: path.clone(),
                    change: Some(change),
                })
            })
            .collect::<Vec<_>>();
        if rows.len() > limits.max_entries {
            return Err(Error::Limited);
        }
        let mut rows = rows;
        rows.sort_unstable_by(|a, b| a.path.cmp(&b.path));
        let next = if mode != ResetMode::Soft && !rows.is_empty() {
            Some(encode_index(repo, repository, &target, ctx)?)
        } else {
            None
        };
        let new_index = next
            .as_deref()
            .map(|bytes| index_id(bytes, ctx))
            .transpose()?
            .unwrap_or(old_index);
        let resuming = pending.is_some();
        let mut journal = match pending {
            Some(journal)
                if journal.to == commits.1
                    && journal.old_index == old_index
                    && journal.new_index == new_index =>
            {
                journal
            }
            Some(_) => return Err(Error::Stale),
            None => {
                let timestamp = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(engine)?
                    .as_nanos();
                Journal {
                    source: branch.into(),
                    destination: commits.1.to_string(),
                    from: commits.0,
                    to: commits.1,
                    incoming: Some(commits.1),
                    commit_operation: None,
                    rebase: false,
                    stash: false,
                    reset: Some(mode),
                    hard_manifest: None,
                    mainline: None,
                    aborting: false,
                    old_index,
                    new_index,
                    nonce: format!("{:x}{timestamp:x}", std::process::id()),
                }
            }
        };
        let (files, attribute_sources, hard_backup) = if mode == ResetMode::Hard {
            let head = revision::tree_entries(repo, trees.0, limits, &mut budget, ctx)?;
            let (files, attributes, backup) = super::hard_reset::HardBackup::prepare(
                repo,
                repository,
                super::hard_reset::HardSources {
                    head: &head,
                    index: &before,
                    target: &target,
                    original: original.ok_or(Error::Stale)?,
                },
                &journal,
                limits,
                &mut budget,
                ctx,
            )?;
            journal.hard_manifest = Some(index_id(backup.manifest.as_bytes(), ctx)?);
            for file in &files {
                match rows.binary_search_by(|row| row.path.as_slice().cmp(file.path())) {
                    Ok(index) => rows[index].change = Some(file.change()),
                    Err(index) => rows.insert(
                        index,
                        MergeRow {
                            path: file.path().to_vec(),
                            change: Some(file.change()),
                        },
                    ),
                }
            }
            (files, attributes, Some(backup))
        } else {
            (Vec::new(), BTreeMap::new(), None)
        };
        if rows.len() > limits.max_entries {
            return Err(Error::Limited);
        }
        Ok((
            Self {
                files,
                journal,
                resuming,
                index_bytes: if applied { None } else { next },
                attribute_sources,
                merge: false,
                shapes: Vec::new(),
                hard_backup,
            },
            rows,
        ))
    }
}

fn encode_index(
    repo: &gix::Repository,
    repository: &Identity,
    target: &Entries,
    ctx: &Context<'_>,
) -> Result<Vec<u8>, Error> {
    let mut state = gix::index::State::new(repo.object_hash());
    for (path, entry) in target {
        ctx.check()?;
        checkout_target(repository, path)?;
        let mode = Mode::from_bits(entry.mode).ok_or(Error::Unsupported)?;
        state.dangerously_push_entry(
            Default::default(),
            entry.id,
            Flags::empty(),
            mode,
            path.as_bstr(),
        );
    }
    if state.entries().len() > crate::INDEX_ENTRIES
        || state.path_backing().len() > crate::TREE_PATH_BYTES
    {
        return Err(Error::Limited);
    }
    state.verify_entries().map_err(engine)?;
    let mut output = Vec::new();
    gix::index::File::from_state(state, repository.git_dir.join("index"))
        .write_to(&mut output, Default::default())
        .map_err(engine)?;
    if output.len() > crate::INDEX_BYTES as usize {
        return Err(Error::Limited);
    }
    Ok(output)
}
