//! A self-contained backup of the local content a hard reset can discard.
use super::{
    CheckoutFile, Journal, MAX_FILES, Observed, attribute_sources, check_attribute_sources,
    check_effective_attributes, checkout_target, conflict, file_bytes, index_id, is_attributes,
    read_record,
};
use crate::{
    Context, Error, Identity, Limits, engine, local_pair,
    revision::{self, Budget, Entry},
};
use gix::{ObjectId, objs::Kind};
use jc_fs::Capture;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write,
    path::{Path, PathBuf},
};

const MANIFEST: &str = "manifest.tsv";
const HEADER: &str = "CIDE-RESET-HARD-BACKUP-1";
type Entries = BTreeMap<Vec<u8>, Entry>;
type Attributes = BTreeMap<Vec<u8>, Option<ObjectId>>;

pub(super) struct HardSources<'a> {
    pub head: &'a Entries,
    pub index: &'a Entries,
    pub target: &'a Entries,
    pub original: &'a [u8],
}

pub(super) struct HardBackup {
    pub manifest: String,
    staged: BTreeMap<ObjectId, Vec<u8>>,
}

impl HardBackup {
    pub fn prepare(
        repo: &gix::Repository,
        identity: &Identity,
        sources: HardSources<'_>,
        journal: &Journal,
        limits: Limits,
        budget: &mut Budget,
        ctx: &Context<'_>,
    ) -> Result<(Vec<CheckoutFile>, Attributes, Self), Error> {
        let paths: BTreeSet<_> = sources.index.keys().chain(sources.target.keys()).collect();
        let mut aliases = BTreeMap::new();
        let mut files = Vec::new();
        let mut manifest = format!(
            "{HEADER}\nOrigen\t{}\nDestino\t{}\nÍndice SHA-256\t{}\n",
            journal.from, journal.to, journal.old_index
        );
        for path in paths {
            ctx.check()?;
            let text = path_text(path)?;
            let target = checkout_target(identity, path)?;
            let old = sources.index.get(path);
            let new = sources.target.get(path);
            if old
                .into_iter()
                .chain(new)
                .any(|entry| entry.mode != 0o100644)
            {
                return Err(conflict(path));
            }
            for end in text.match_indices('/').map(|(i, _)| i).chain([text.len()]) {
                let prefix = &text[..end];
                if end != text.len()
                    && (sources.index.contains_key(prefix.as_bytes())
                        || sources.target.contains_key(prefix.as_bytes()))
                {
                    return Err(conflict(path));
                }
                let folded = prefix.to_uppercase();
                match aliases.entry(folded) {
                    std::collections::btree_map::Entry::Occupied(entry)
                        if *entry.get() != prefix =>
                    {
                        return Err(conflict(path));
                    }
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        budget.charge(entry.key().len().saturating_add(64))?;
                        entry.insert(prefix);
                    }
                    _ => {}
                }
            }
            let (before, observed) =
                match local_pair::capture(&identity.root, &target, limits.max_blob, ctx)? {
                    Capture::Present { bytes, token, .. } => {
                        if let Some(entry) = new
                            && gix::objs::compute_hash(entry.id.kind(), Kind::Blob, &bytes)
                                .map_err(engine)?
                                == entry.id
                        {
                            continue;
                        }
                        // An untracked path, hard link or directory collision must stay intact.
                        if old.is_none() || !token.has_single_link() {
                            return Err(conflict(path));
                        }
                        budget.charge(bytes.len())?;
                        (Some(bytes), Observed::Before(token))
                    }
                    Capture::Absent { token } if new.is_some() => (None, Observed::Missing(token)),
                    Capture::Absent { .. } => continue,
                };
            if files.len() == MAX_FILES || is_attributes(path) {
                return Err(if is_attributes(path) {
                    Error::Filter
                } else {
                    Error::Limited
                });
            }
            let after = file_bytes(repo, new.copied(), limits, budget, ctx)?;
            let digest = before
                .as_deref()
                .map(|bytes| index_id(bytes, ctx))
                .transpose()?;
            writeln!(
                manifest,
                "archivo\t{}\t{}\t{}\t{text}",
                files.len(),
                oid_text(digest),
                oid_text(new.map(|entry| entry.id))
            )
            .map_err(engine)?;
            files.push(CheckoutFile {
                path: path.clone(),
                target,
                before,
                after,
                observed,
            });
        }
        let index = local_pair::decode_index(repo, Some(sources.original), ctx)?;
        let changed = files.iter().map(|file| &file.path).collect::<Vec<_>>();
        let attributes = attribute_sources(&changed, &index, sources.head, ctx)?;
        if !files.is_empty() {
            check_attribute_sources(repo, identity, &attributes, ctx)?;
            let mut stack = repo
                .attributes_only(
                    &index,
                    gix::worktree::stack::state::attributes::Source::IdMapping,
                )
                .map_err(engine)?;
            for file in &files {
                check_effective_attributes(
                    &mut stack,
                    &file.path,
                    (file.before.as_deref(), file.after.as_deref()),
                    false,
                )?;
            }
        }

        let mut staged = BTreeMap::new();
        for (path, entry) in sources.index {
            ctx.check()?;
            if sources.head.get(path) == Some(entry) {
                continue;
            }
            if let std::collections::btree_map::Entry::Vacant(slot) = staged.entry(entry.id) {
                slot.insert(
                    file_bytes(repo, Some(*entry), limits, budget, ctx)?.ok_or(Error::Stale)?,
                );
            }
            writeln!(manifest, "preparado\t{}\t{}", entry.id, path_text(path)?).map_err(engine)?;
        }
        for path in sources
            .head
            .keys()
            .filter(|path| !sources.index.contains_key(*path))
        {
            ctx.check()?;
            writeln!(manifest, "eliminación preparada\t{}", path_text(path)?).map_err(engine)?;
        }
        budget.charge(manifest.len())?;
        Ok((files, attributes, Self { manifest, staged }))
    }

    pub fn save(
        &self,
        identity: &Identity,
        journal: &Journal,
        index: &[u8],
        files: &[CheckoutFile],
        ctx: &Context<'_>,
    ) -> Result<(), Error> {
        let directory = journal.hard_backup_path(identity).ok_or(Error::Stale)?;
        let index_path = directory.join("index");
        if index_id(index, ctx)? != journal.old_index
            || Some(index_id(self.manifest.as_bytes(), ctx)?) != journal.hard_manifest
        {
            return Err(Error::Stale);
        }
        let Capture::Absent { token } =
            local_pair::capture(&identity.git_dir, &index_path, 0, ctx)?
        else {
            return Err(Error::Stale);
        };
        if token.is_leaf() {
            return Err(Error::Stale);
        }
        save_file(identity, &index_path, index, ctx)?;
        for (slot, file) in files.iter().enumerate() {
            if let Some(bytes) = &file.before {
                save_file(identity, &worktree_file(&directory, slot), bytes, ctx)?;
            }
        }
        for (id, bytes) in &self.staged {
            save_file(
                identity,
                &directory.join("preparados").join(id.to_string()),
                bytes,
                ctx,
            )?;
        }
        // A complete manifest is the last publication. The journal is created
        // only after all backup files have been written and flushed.
        save_file(
            identity,
            &directory.join(MANIFEST),
            self.manifest.as_bytes(),
            ctx,
        )
    }
}

pub(super) fn load_files(
    repo: &gix::Repository,
    identity: &Identity,
    journal: &Journal,
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<Vec<CheckoutFile>, Error> {
    let directory = journal.hard_backup_path(identity).ok_or(Error::Stale)?;
    let manifest = read_record(identity, &directory.join(MANIFEST), limits.max_bytes, ctx)?
        .ok_or(Error::Stale)?;
    if Some(index_id(&manifest, ctx)?) != journal.hard_manifest {
        return Err(Error::Stale);
    }
    let mut budget = Budget::new(limits);
    budget.charge(manifest.len())?;
    let mut lines = std::str::from_utf8(&manifest).map_err(engine)?.lines();
    if lines.next() != Some(HEADER) {
        return Err(Error::Stale);
    }
    let mut files = Vec::new();
    for line in lines.filter(|line| line.starts_with("archivo\t")) {
        ctx.check()?;
        let fields = line.split('\t').collect::<Vec<_>>();
        let ["archivo", number, before, after, text] = fields.as_slice() else {
            return Err(Error::Stale);
        };
        let slot = files.len();
        if slot == MAX_FILES || number.parse::<usize>().ok() != Some(slot) {
            return Err(Error::Stale);
        }
        let path = text.as_bytes();
        revision::path_valid(path, limits.max_path)?;
        let before = if let Some(id) = parse_oid(before)? {
            let bytes = read_record(
                identity,
                &worktree_file(&directory, slot),
                limits.max_blob,
                ctx,
            )?
            .ok_or(Error::Stale)?;
            if id != index_id(&bytes, ctx)? {
                return Err(Error::Stale);
            }
            budget.charge(bytes.len())?;
            Some(bytes)
        } else {
            None
        };
        let after = file_bytes(
            repo,
            parse_oid(after)?.map(|id| Entry { id, mode: 0o100644 }),
            limits,
            &mut budget,
            ctx,
        )?;
        files.push(CheckoutFile {
            path: path.to_vec(),
            target: checkout_target(identity, path)?,
            before,
            after,
            observed: Observed::Structural,
        });
    }
    Ok(files)
}

fn path_text(path: &[u8]) -> Result<&str, Error> {
    let text = std::str::from_utf8(path).map_err(engine)?;
    if text
        .chars()
        .any(|ch| ch.is_control() || matches!(ch, ':' | '\\'))
    {
        return Err(Error::Unsupported);
    }
    Ok(text)
}

fn oid_text(id: Option<ObjectId>) -> String {
    id.map_or_else(|| "-".into(), |id| id.to_string())
}

fn parse_oid(text: &str) -> Result<Option<ObjectId>, Error> {
    (text != "-")
        .then(|| ObjectId::from_hex(text.as_bytes()).map_err(engine))
        .transpose()
}

fn worktree_file(root: &Path, slot: usize) -> PathBuf {
    root.join("archivos").join(slot.to_string())
}

fn save_file(
    identity: &Identity,
    path: &Path,
    bytes: &[u8],
    ctx: &Context<'_>,
) -> Result<(), Error> {
    let root = &identity.git_dir;
    let Capture::Absent { token } = local_pair::capture(root, path, 0, ctx)? else {
        return Err(Error::Stale);
    };
    jc_fs::create_missing_parents(root, &token, &mut || ctx.check().is_err())?;
    let Capture::Absent { token: leaf } = local_pair::capture(root, path, 0, ctx)? else {
        return Err(Error::Stale);
    };
    if !token.is_compatible_after_creating_parents(&leaf) {
        return Err(Error::Stale);
    }
    jc_fs::stage_missing_file(root, &leaf, bytes, &mut || ctx.check().is_err())?.publish()?;
    Ok(())
}
