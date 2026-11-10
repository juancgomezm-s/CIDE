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
    AlreadyApplied,
    Conflict,
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
    if common == plan.theirs {
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
    plan.rebase_steps = sequence
        .iter()
        .map(|commit| RebaseStep {
            original: commit.id,
            state: RebaseStepState::Pending,
        })
        .collect();

    let old_tree = sequence.last().map_or_else(
        || tree_id(repo, plan.from, limits, ctx),
        |commit| Ok(commit.tree),
    )?;
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
        for (step, original) in plan.rebase_steps.iter_mut().zip(sequence) {
            ctx.check()?;
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
                step.state = RebaseStepState::Conflict;
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
            if candidate.entries == rebased && !originally_empty {
                step.state = RebaseStepState::AlreadyApplied;
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
            let object = revision::verified(
                repo,
                original.id,
                limits.max_bytes.min(crate::OBJECT_BYTES as usize),
                ctx,
            )?;
            budget.charge(object.data.len())?;
            let original = object
                .try_into_commit()
                .map_err(engine)?
                .decode()
                .map_err(engine)?
                .into_owned()
                .map_err(engine)?;
            let commit = Commit {
                tree,
                parents: [tip].into_iter().collect(),
                committer: signature.clone(),
                extra_headers: Vec::new(),
                ..original
            };
            let mut bytes = Vec::new();
            commit.write_to(&mut bytes)?;
            tip = gix::objs::compute_hash(repo.object_hash(), Kind::Commit, &bytes)
                .map_err(engine)?;
            commits.push((tip, commit));
            step.state = RebaseStepState::Replayed(tip);
        }
    }
    if applied_decisions != decisions.len() {
        return Err(Error::Stale);
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
