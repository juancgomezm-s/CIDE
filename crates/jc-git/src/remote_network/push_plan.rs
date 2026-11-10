//! Pins the source branch and remote ref before building a bounded update.
use super::super::{
    NetworkPlan, OperationCredential, PushPlan, RefUpdatePreview, TransferPreview, TransferResult,
    branch_name, current_head, object_closure_ignoring, resolve_remote_name,
};
use super::{connect_transport, send_pack};
use crate::{Context, Error, Identity, Limits, controlled, engine, graph, open, revision};
use gix::{ObjectId, bstr::ByteSlice, remote::Direction};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

struct Source {
    repository: Identity,
    name: String,
    branch: String,
    destination: String,
    tip: ObjectId,
    url: gix::url::Url,
}

fn observe(
    root: &Path,
    requested: &str,
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<(gix::Repository, Source), Error> {
    let (repo, repository) = open(root, ctx)?;
    let name = resolve_remote_name(&repo, requested, Direction::Push)?;
    let (short, branch) = branch_name(&current_head(&repo, limits, ctx)?)?;
    if repo
        .branch_remote_name(branch.shorten(), Direction::Push)
        .is_none_or(|configured| configured.as_bstr().as_bytes() != name.as_bytes())
    {
        return Err(Error::Engine(
            "Configura el remoto de envío de la rama activa".into(),
        ));
    }
    let destination = repo
        .branch_remote_ref_name(branch.as_ref(), Direction::Push)
        .and_then(Result::ok)
        .ok_or(Error::Unsupported)?;
    let destination = destination
        .as_bstr()
        .to_str()
        .map_err(|_| Error::Unsupported)?
        .to_owned();
    if !destination.starts_with("refs/heads/") {
        return Err(Error::Unsupported);
    }
    let remote = repo
        .find_remote(&name)
        .map_err(|_| Error::Engine("La configuración del remoto no es válida".into()))?;
    let url = remote
        .url(Direction::Push)
        .ok_or(Error::Unsupported)?
        .clone();
    let location = url.to_bstring();
    let supported = matches!(url.scheme, gix::url::Scheme::Https | gix::url::Scheme::Ssh)
        || (url.scheme == gix::url::Scheme::Http && super::super::test_loopback(&url));
    if !supported
        || url.password().is_some()
        || (url.scheme == gix::url::Scheme::Https && url.user().is_some())
        || location
            .iter()
            .any(|&byte| byte.is_ascii_control() || matches!(byte, b'?' | b'#'))
    {
        return Err(Error::Unsupported);
    }
    let tip = repo.head_id().map_err(engine)?.detach();
    Ok((
        repo,
        Source {
            repository,
            name,
            branch: short,
            destination,
            tip,
            url,
        },
    ))
}

/// The old commit and all objects in its root tree are known to the server.
/// Do not inflate unchanged blobs into the outgoing plan.
fn existing_objects(
    repo: &gix::Repository,
    old: ObjectId,
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<BTreeSet<ObjectId>, Error> {
    let mut budget = revision::Budget::new(limits);
    let commit = revision::commit(repo, old, limits, &mut budget, ctx)?;
    let mut known = BTreeSet::from([old]);
    let mut pending = vec![commit.tree];
    while let Some(id) = pending.pop() {
        ctx.check()?;
        if !known.insert(id) {
            continue;
        }
        if let Some(tree) = revision::verified_tree(repo, id, limits, &mut budget, ctx)? {
            for entry in tree.iter() {
                ctx.check()?;
                budget.visit()?;
                let entry = entry.map_err(engine)?;
                match entry.mode().value() {
                    0o40000 => pending.push(entry.object_id()),
                    0o160000 => {}
                    _ => {
                        known.insert(entry.object_id());
                    }
                }
            }
        }
    }
    Ok(known)
}

pub(super) fn prepare(
    root: &Path,
    requested: &str,
    credential: Option<&OperationCredential>,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<PushPlan, Error> {
    limits.admit()?;
    controlled(limits.timeout, cancelled, |ctx| {
        let (repo, source) = observe(root, requested, limits, ctx)?;
        let mut transport = connect_transport(&repo, &source.url, true, credential, limits, ctx)?;
        let old = send_pack::advertise(
            &mut *transport,
            &source.destination,
            repo.object_hash(),
            limits,
            ctx,
        )?;
        drop(transport);
        let mut existing = BTreeSet::new();
        if let Some(old) = old {
            if old == source.tip {
                existing.insert(old);
            } else {
                let ahead = match graph::contains_ancestor(
                    &repo,
                    &source.tip.to_string(),
                    old,
                    limits,
                    ctx,
                ) {
                    Ok(ahead) => ahead,
                    Err(Error::Missing) => false,
                    Err(error) => return Err(error),
                };
                if !ahead {
                    return Err(Error::Engine(
                        "Envío rechazado: el remoto no es antecesor de la rama local".into(),
                    ));
                }
                existing = existing_objects(&repo, old, limits, ctx)?;
            }
        }
        let objects = object_closure_ignoring(&repo, [source.tip], &existing, limits, ctx)?;
        let url = source
            .url
            .to_bstring()
            .to_str()
            .map_err(|_| Error::Unsupported)?
            .to_owned();
        let preview = TransferPreview {
            remote: source.name.clone(),
            branch: source.branch.clone(),
            destination_path: Some(url.clone()),
            updates: vec![RefUpdatePreview {
                source: format!("refs/heads/{}", source.branch),
                destination: source.destination.clone(),
                old: old.map(|oid| oid.to_string()),
                new: source.tip.to_string(),
            }],
            object_count: objects.len(),
            object_bytes: objects.iter().map(|object| object.2.len()).sum(),
            file_transport: false,
        };
        let mut advertised = BTreeMap::new();
        if let Some(oid) = old {
            advertised.insert(source.destination, oid);
        }
        Ok(PushPlan {
            local: source.repository.clone(),
            remote: source.repository,
            limits,
            name: source.name,
            preview,
            objects,
            network: Some(NetworkPlan { url, advertised }),
        })
    })
}

pub(super) fn apply(
    plan: PushPlan,
    credential: Option<&OperationCredential>,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<TransferResult, Error> {
    controlled(plan.limits.timeout, cancelled, |ctx| {
        let network = plan.network.ok_or(Error::Unsupported)?;
        let update = plan.preview.updates.first().ok_or(Error::Stale)?;
        let (repo, source) = observe(&plan.local.root, &plan.name, plan.limits, ctx)?;
        crate::index_write::same_repository(&plan.local, &source.repository)?;
        if source.url.to_bstring().as_slice() != network.url.as_bytes()
            || source.destination != update.destination
            || source.branch != plan.preview.branch
            || source.tip != ObjectId::from_hex(update.new.as_bytes()).map_err(|_| Error::Stale)?
        {
            return Err(Error::Stale);
        }
        let old = network.advertised.get(&source.destination).copied();
        let pack = send_pack::pack_v2(&plan.objects, repo.object_hash(), plan.limits, ctx)?;
        let mut transport =
            connect_transport(&repo, &source.url, true, credential, plan.limits, ctx)?;
        send_pack::push_pack(
            &mut *transport,
            &source.destination,
            old,
            source.tip,
            &pack,
            plan.limits,
            ctx,
        )?;
        Ok(TransferResult {
            remote: plan.name,
            updated: (old != Some(source.tip))
                .then_some(source.destination)
                .into_iter()
                .collect(),
            objects_written: plan.objects.len(),
            bytes_written: plan.preview.object_bytes,
        })
    })
}
