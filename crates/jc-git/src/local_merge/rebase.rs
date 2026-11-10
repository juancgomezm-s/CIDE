//! Replay a bounded linear sequence in memory before publishing its final tree.
use super::{
    Blobs, Checkout, MergeCheckout, MergeKind, MergePlan, MergeRow, Objects, Resolutions,
    Transition, base, combine_entries, commit_write, make_trees, resolve_candidate, tree_id,
};
use crate::{
    Context, Error, engine,
    revision::{self, Budget},
};
use gix::{
    ObjectId,
    objs::{Commit, Kind, WriteTo},
};
use std::collections::BTreeMap;

const MAX_REPLAY: usize = 32;

pub struct RebaseStep {
    pub original: ObjectId,
    pub state: RebaseStepState,
}

pub enum RebaseStepState {
    Pending,
    Replayed(ObjectId),
    Combined(ObjectId),
    Omitted,
    AlreadyApplied,
    Conflict,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RebaseAction {
    Pick,
    Squash,
    Drop,
}

#[derive(Clone)]
pub struct RebaseTodo {
    pub original: ObjectId,
    pub action: RebaseAction,
    pub subject: String,
}

#[derive(Clone)]
pub(super) struct ConflictReview {
    pub current: ObjectId,
    choices: BTreeMap<ObjectId, Resolutions>,
    committer: gix::actor::Signature,
}

pub(super) fn review(
    repo: &gix::Repository,
    plan: &mut MergePlan,
    index_bytes: &[u8],
    continuation: Option<(&ConflictReview, Resolutions)>,
    ctx: &Context<'_>,
) -> Result<(), Error> {
    let limits = plan.limits;
    let common = base(repo, plan.from, plan.theirs, limits, ctx)?;
    if common == plan.theirs && (!plan.source.is_interactive() || common == plan.from) {
        if continuation.is_some() {
            return Err(Error::Stale);
        }
        return Ok(());
    }

    let mut budget = Budget::new(limits);
    let mut sequence = Vec::new();
    let mut next = plan.from;
    while next != common {
        if sequence.len() == MAX_REPLAY.min(limits.max_entries) {
            return Err(Error::Limited);
        }
        let commit = revision::commit(repo, next, limits, &mut budget, ctx)?;
        let [parent] = commit.parents.as_slice() else {
            return Err(Error::Engine(
                "El rebase local admite una secuencia sin commits de fusión".into(),
            ));
        };
        if commit.limited {
            return Err(Error::Limited);
        }
        next = *parent;
        sequence.push(commit);
    }
    sequence.reverse();
    let old_tree = sequence.last().map_or_else(
        || tree_id(repo, plan.from, limits, ctx),
        |commit| Ok(commit.tree),
    )?;
    if plan.source.is_interactive() && plan.todo.is_none() && !sequence.is_empty() {
        let mut todo = Vec::with_capacity(sequence.len());
        for commit in &sequence {
            let original = original_commit(repo, commit.id, limits, &mut budget, ctx)?;
            let subject = String::from_utf8_lossy(&original.message)
                .lines()
                .next()
                .unwrap_or_default()
                .chars()
                .filter(|ch| !ch.is_control())
                .take(100)
                .collect();
            todo.push(RebaseTodo {
                original: commit.id,
                action: RebaseAction::Pick,
                subject,
            });
        }
        plan.todo = Some(todo);
        plan.kind = MergeKind::RebaseTodo;
        return Ok(());
    }
    let actions = if let Some(todo) = &plan.todo {
        let mut commits: BTreeMap<_, _> = sequence
            .drain(..)
            .map(|commit| (commit.id, commit))
            .collect();
        let mut has_pick = false;
        let mut actions = Vec::with_capacity(todo.len());
        for item in todo {
            if item.action == RebaseAction::Squash && !has_pick {
                return Err(Error::Engine(
                    "Para combinar un commit debe haber antes uno que se reaplique".into(),
                ));
            }
            has_pick |= item.action == RebaseAction::Pick;
            sequence.push(commits.remove(&item.original).ok_or(Error::Stale)?);
            actions.push(item.action);
        }
        if !commits.is_empty() {
            return Err(Error::Stale);
        }
        actions
    } else {
        vec![RebaseAction::Pick; sequence.len()]
    };
    plan.rebase_steps = sequence
        .iter()
        .map(|commit| RebaseStep {
            original: commit.id,
            state: RebaseStepState::Pending,
        })
        .collect();

    let mut tree = tree_id(repo, plan.theirs, limits, ctx)?;
    let mut rebased = revision::merge_tree_entries(repo, tree, limits, &mut budget, ctx)?;
    let mut tip = plan.theirs;
    let mut blobs = Blobs::new();
    let mut trees = BTreeMap::new();
    let mut commits = Vec::new();
    let mut decisions = BTreeMap::new();
    let mut applied_decisions = 0;
    if !sequence.is_empty() {
        commit_write::check_signing(repo)?;
        let local = commit_write::local_identity(repo)?;
        let signature = if let Some((review, choices)) = continuation {
            if review.committer.name != local.name || review.committer.email != local.email {
                return Err(Error::Stale);
            }
            decisions = review.choices.clone();
            if decisions.insert(review.current, choices).is_some() {
                return Err(Error::Stale);
            }
            review.committer.clone()
        } else {
            local
        };
        let base_tree = tree_id(repo, common, limits, ctx)?;
        let mut previous = revision::merge_tree_entries(repo, base_tree, limits, &mut budget, ctx)?;
        let mut previous_id = common;
        let mut group_start = 0;
        for (index, (original, &action)) in sequence.into_iter().zip(&actions).enumerate() {
            ctx.check()?;
            if action == RebaseAction::Drop {
                plan.rebase_steps[index].state = RebaseStepState::Omitted;
                continue;
            }
            let parent = original.parents[0];
            if previous_id != parent {
                let parent_tree = tree_id(repo, parent, limits, ctx)?;
                previous =
                    revision::merge_tree_entries(repo, parent_tree, limits, &mut budget, ctx)?;
            }
            let current =
                revision::merge_tree_entries(repo, original.tree, limits, &mut budget, ctx)?;
            let mut candidate = combine_entries(
                repo,
                [&previous, &rebased, &current],
                &blobs,
                limits,
                &mut budget,
                ctx,
            )?;
            if let Some(choices) = decisions.get(&original.id) {
                resolve_candidate(repo, &mut candidate, choices, limits, &mut budget, ctx)?;
                applied_decisions += 1;
            } else if !candidate.conflicts.is_empty() {
                if applied_decisions != decisions.len() {
                    return Err(Error::Stale);
                }
                plan.rebase_steps[index].state = RebaseStepState::Conflict;
                plan.kind = MergeKind::Conflicts;
                plan.rows = candidate
                    .conflicts
                    .keys()
                    .cloned()
                    .map(|path| MergeRow { path, change: None })
                    .collect();
                plan.conflicts = candidate.conflicts;
                plan.rebase_review = Some(ConflictReview {
                    current: original.id,
                    choices: decisions,
                    committer: signature,
                });
                return Ok(());
            }
            let originally_empty = current == previous;
            previous = current;
            previous_id = original.id;
            let starts_group = actions[index + 1..]
                .iter()
                .find(|&&action| action != RebaseAction::Drop)
                == Some(&RebaseAction::Squash);
            if candidate.entries == rebased
                && !originally_empty
                && action == RebaseAction::Pick
                && !starts_group
            {
                plan.rebase_steps[index].state = RebaseStepState::AlreadyApplied;
                continue;
            }
            if candidate.entries != rebased {
                let (next_tree, generated) = make_trees(repo, &candidate.entries, limits, ctx)?;
                tree = next_tree;
                for (id, object) in generated {
                    if let std::collections::btree_map::Entry::Vacant(slot) = trees.entry(id) {
                        budget
                            .charge(usize::try_from(object.size()).map_err(|_| Error::Limited)?)?;
                        slot.insert(object);
                    }
                }
                blobs.extend(candidate.blobs);
                rebased = candidate.entries;
            }
            let original = original_commit(repo, original.id, limits, &mut budget, ctx)?;
            if action == RebaseAction::Squash {
                let (id, commit) = commits.last_mut().ok_or(Error::Stale)?;
                let commit: &mut Commit = commit;
                if commit.encoding != original.encoding {
                    return Err(Error::Engine(
                        "Los mensajes de los commits usan codificaciones distintas".into(),
                    ));
                }
                budget.charge(2 + original.message.len())?;
                commit.message.extend_from_slice(b"\n\n");
                commit.message.extend_from_slice(&original.message);
                commit.tree = tree;
                tip = commit_id(repo, commit)?;
                *id = tip;
                for step in &mut plan.rebase_steps[group_start..index] {
                    match &mut step.state {
                        RebaseStepState::Replayed(id) | RebaseStepState::Combined(id) => *id = tip,
                        _ => {}
                    }
                }
                plan.rebase_steps[index].state = RebaseStepState::Combined(tip);
            } else {
                let commit = Commit {
                    tree,
                    parents: [tip].into_iter().collect(),
                    committer: signature.clone(),
                    extra_headers: Vec::new(),
                    ..original
                };
                tip = commit_id(repo, &commit)?;
                commits.push((tip, commit));
                group_start = index;
                plan.rebase_steps[index].state = RebaseStepState::Replayed(tip);
            }
        }
    }
    if applied_decisions != decisions.len() {
        return Err(Error::Stale);
    }
    if tip == plan.from {
        return Ok(());
    }
    let checkout = Checkout::prepare(
        repo,
        &plan.repository,
        Transition {
            branches: (plan.active(), plan.incoming()),
            commits: (plan.from, tip),
            trees: (old_tree, tree),
            merge: Some(MergeCheckout {
                incoming: plan.theirs,
                commit_operation: None,
                rebase: true,
                mainline: None,
                entries: &rebased,
                blobs: &blobs,
            }),
        },
        index_bytes,
        None,
        limits,
        ctx,
    )?;
    plan.kind = if commits.is_empty() {
        MergeKind::FastForward
    } else {
        MergeKind::Commit
    };
    plan.rows = checkout
        .files
        .iter()
        .map(|file| MergeRow {
            path: file.path().to_vec(),
            change: Some(file.change()),
        })
        .collect();
    plan.checkout = Some(checkout);
    plan.result = Some((tip, tree));
    if !commits.is_empty() {
        plan.objects = Some(Objects {
            blobs,
            trees: trees.into_iter().collect(),
            commits,
        });
    }
    Ok(())
}

fn original_commit(
    repo: &gix::Repository,
    id: ObjectId,
    limits: crate::Limits,
    budget: &mut Budget,
    ctx: &Context<'_>,
) -> Result<Commit, Error> {
    let object = revision::verified(
        repo,
        id,
        limits.max_bytes.min(crate::OBJECT_BYTES as usize),
        ctx,
    )?;
    budget.charge(object.data.len())?;
    object
        .try_into_commit()
        .map_err(engine)?
        .decode()
        .map_err(engine)?
        .into_owned()
        .map_err(engine)
}

fn commit_id(repo: &gix::Repository, commit: &Commit) -> Result<ObjectId, Error> {
    let mut bytes = Vec::new();
    commit.write_to(&mut bytes)?;
    gix::objs::compute_hash(repo.object_hash(), Kind::Commit, &bytes).map_err(engine)
}
