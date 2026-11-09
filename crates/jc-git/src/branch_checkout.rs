//! Recoverable updates of the paths that differ between two pinned trees.
use crate::{
    Context, Error, Identity, Limits, engine, index_write, local_pair, ref_write, revision,
};
use gix::{
    ObjectId,
    bstr::ByteSlice,
    index::entry::{Flags, Mode},
};
use jc_fs::Capture;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub(super) const JOURNAL_NAME: &str = "cide-switch.pending";
const JOURNAL_BYTES: usize = 16 * 1024;
const MAX_FILES: usize = 128;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Journal {
    pub source: String,
    pub destination: String,
    pub from: ObjectId,
    pub to: ObjectId,
    old_index: ObjectId,
    new_index: ObjectId,
    nonce: String,
}

impl Journal {
    pub fn read(repository: &Identity, ctx: &Context<'_>) -> Result<Option<Self>, Error> {
        let path = repository.git_dir.join(JOURNAL_NAME);
        let Capture::Present { bytes, token, .. } =
            local_pair::capture(&repository.git_dir, &path, JOURNAL_BYTES, ctx)?
        else {
            return Ok(None);
        };
        if !token.has_single_link() {
            return Err(Error::Unsupported);
        }
        let mut lines = std::str::from_utf8(&bytes).map_err(engine)?.lines();
        if lines.next() != Some("CIDE-SWITCH-1") {
            return Err(Error::Unsupported);
        }
        let source = lines.next().ok_or(Error::Unsupported)?.to_owned();
        let destination = lines.next().ok_or(Error::Unsupported)?.to_owned();
        let mut oid = || {
            ObjectId::from_hex(lines.next().ok_or(Error::Unsupported)?.as_bytes()).map_err(engine)
        };
        let (from, to, old_index, new_index) = (oid()?, oid()?, oid()?, oid()?);
        let nonce = lines.next().ok_or(Error::Unsupported)?.to_owned();
        if nonce.is_empty()
            || nonce.len() > 64
            || !nonce.bytes().all(|b| b.is_ascii_hexdigit())
            || lines.next().is_some()
        {
            return Err(Error::Unsupported);
        }
        Ok(Some(Self {
            source,
            destination,
            from,
            to,
            old_index,
            new_index,
            nonce,
        }))
    }

    fn bytes(&self) -> Vec<u8> {
        format!(
            "CIDE-SWITCH-1\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n",
            self.source,
            self.destination,
            self.from,
            self.to,
            self.old_index,
            self.new_index,
            self.nonce
        )
        .into_bytes()
    }

    fn backup(&self, repository: &Identity, file: usize) -> PathBuf {
        repository
            .git_dir
            .join(format!("cide-switch-{}-{file}.bak", self.nonce))
    }
}

#[derive(PartialEq, Eq)]
enum Observed {
    Before(jc_fs::FileToken),
    Missing(jc_fs::MissingToken),
    Applied,
}

pub struct CheckoutFile {
    path: Vec<u8>,
    target: PathBuf,
    before: Option<Vec<u8>>,
    after: Option<Vec<u8>>,
    observed: Observed,
}

impl CheckoutFile {
    pub fn path(&self) -> &[u8] {
        &self.path
    }

    pub fn change(&self) -> crate::Change {
        match (&self.before, &self.after) {
            (None, _) => crate::Change::Added,
            (_, None) => crate::Change::Deleted,
            _ => crate::Change::Modified,
        }
    }
}

pub(super) struct Checkout {
    pub files: Vec<CheckoutFile>,
    pub journal: Journal,
    pub resuming: bool,
    pub index_bytes: Option<Vec<u8>>,
    attribute_sources: BTreeMap<Vec<u8>, Option<ObjectId>>,
}

pub(super) struct Transition<'a> {
    pub branches: (&'a str, &'a str),
    pub commits: (ObjectId, ObjectId),
    pub trees: (ObjectId, ObjectId),
}

fn conflict(path: &[u8]) -> Error {
    Error::Engine(format!(
        "Cambios locales o ruta incompatible en {}",
        String::from_utf8_lossy(path)
    ))
}

fn is_attributes(path: &[u8]) -> bool {
    path.rsplit(|&b| b == b'/')
        .next()
        .is_some_and(|name| name.eq_ignore_ascii_case(b".gitattributes"))
}

fn attribute_sources(
    paths: &[&Vec<u8>],
    index: &gix::index::State,
    tree: &BTreeMap<Vec<u8>, revision::Entry>,
    ctx: &Context<'_>,
) -> Result<BTreeMap<Vec<u8>, Option<ObjectId>>, Error> {
    let mut sources = BTreeMap::new();
    for path in paths {
        for prefix in std::iter::once(0).chain(
            path.iter()
                .enumerate()
                .filter_map(|(i, &b)| (b == b'/').then_some(i + 1)),
        ) {
            ctx.check()?;
            let mut name = path[..prefix].to_vec();
            name.extend_from_slice(b".gitattributes");
            if sources.contains_key(&name) {
                continue;
            }
            if sources.len() == 512 {
                return Err(Error::Limited);
            }
            let id = match (tree.get(&name), index.entry_by_path(name.as_bstr())) {
                (None, None) => None,
                (Some(expected), Some(entry))
                    if expected.mode == 0o100644
                        && entry.mode == Mode::FILE
                        && expected.id == entry.id
                        && !entry
                            .flags
                            .intersects(Flags::SKIP_WORKTREE | Flags::INTENT_TO_ADD) =>
                {
                    Some(expected.id)
                }
                _ => return Err(Error::Filter),
            };
            sources.insert(name, id);
        }
    }
    Ok(sources)
}

fn check_attribute_sources(
    repo: &gix::Repository,
    repository: &Identity,
    sources: &BTreeMap<Vec<u8>, Option<ObjectId>>,
    ctx: &Context<'_>,
) -> Result<(), Error> {
    const MAX_ATTRIBUTE_BYTES: usize = 64 * 1024;
    index_write::reject_config_conversion(repo, repository)?;
    let mut total = 0usize;
    for (name, id) in sources {
        let path = gix::path::from_bstr(name.as_bstr()).map_err(engine)?;
        let current = local_pair::capture(
            &repository.root,
            &repository.root.join(path),
            MAX_ATTRIBUTE_BYTES,
            ctx,
        )?;
        match (current, id) {
            (Capture::Absent { .. }, None) => {}
            (Capture::Present { bytes, .. }, Some(id)) => {
                total += bytes.len();
                if total > 512 * 1024 {
                    return Err(Error::Limited);
                }
                let blob = revision::verified(repo, *id, MAX_ATTRIBUTE_BYTES, ctx)?;
                if blob.kind != gix::objs::Kind::Blob
                    || bytes != blob.data
                    || gix::attrs::parse(&bytes).any(|line| line.is_err())
                {
                    return Err(Error::Filter);
                }
            }
            _ => return Err(Error::Filter),
        }
    }
    Ok(())
}

fn check_effective_attributes(
    attributes: &mut gix::AttributeStack<'_>,
    path: &[u8],
    sources: (Option<&[u8]>, Option<&[u8]>),
) -> Result<(), Error> {
    let mut outcome = attributes.selected_attribute_matches([
        "text",
        "eol",
        "crlf",
        "filter",
        "ident",
        "working-tree-encoding",
    ]);
    attributes
        .at_entry(path.as_bstr(), Some(Mode::FILE))
        .map_err(engine)?
        .matching_attributes(&mut outcome);
    let mut text = gix::attrs::StateRef::Unspecified;
    let mut lf = false;
    for attr in outcome.iter_selected() {
        let state = attr.assignment.state;
        if state.is_unspecified() || state.is_unset() {
            continue;
        }
        match attr.assignment.name.as_str() {
            "text" => text = state,
            "eol"
                if state
                    .as_bstr()
                    .is_some_and(|value| value.as_bytes() == b"lf") =>
            {
                lf = true
            }
            _ => return Err(Error::Filter),
        }
    }
    if text.is_unspecified() && !lf {
        return Ok(());
    }
    if lf
        && (text.is_unspecified() || text.is_set())
        && [sources.0, sources.1]
            .into_iter()
            .flatten()
            .all(|bytes| !bytes.windows(2).any(|pair| pair == b"\r\n"))
    {
        return Ok(());
    }
    Err(Error::Filter)
}

fn file_bytes(
    repo: &gix::Repository,
    entry: Option<revision::Entry>,
    limits: Limits,
    budget: &mut revision::Budget,
    ctx: &Context<'_>,
) -> Result<Option<Vec<u8>>, Error> {
    let Some(entry) = entry else {
        return Ok(None);
    };
    if entry.mode != 0o100644 {
        return Err(Error::Unsupported);
    }
    let blob = revision::verified(repo, entry.id, limits.max_blob, ctx)?;
    if blob.kind != gix::objs::Kind::Blob {
        return Err(Error::Unsupported);
    }
    budget.charge(blob.data.len())?;
    Ok(Some(blob.data.clone()))
}

fn index_id(bytes: &[u8], ctx: &Context<'_>) -> Result<ObjectId, Error> {
    Ok(ObjectId::from_bytes_or_panic(&local_pair::sha256(
        bytes, ctx,
    )?))
}

impl Checkout {
    pub fn prepare(
        repo: &gix::Repository,
        repository: &Identity,
        transition: Transition<'_>,
        bytes: &[u8],
        pending: Option<Journal>,
        limits: Limits,
        ctx: &Context<'_>,
    ) -> Result<Self, Error> {
        let Transition {
            branches,
            commits,
            trees,
        } = transition;
        let mut budget = revision::Budget::new(limits);
        let old = revision::tree_entries(repo, trees.0, limits, &mut budget, ctx)?;
        let new = revision::tree_entries(repo, trees.1, limits, &mut budget, ctx)?;
        let mut index = local_pair::decode_index(repo, Some(bytes), ctx)?;
        if index.resolve_undo().is_some()
            || index.fs_monitor().is_some()
            || index.untracked().is_some()
        {
            return Err(Error::Unsupported);
        }
        let current_index = index_id(bytes, ctx)?;
        let index_applied = pending
            .as_ref()
            .is_some_and(|p| p.new_index == current_index);
        if pending
            .as_ref()
            .is_some_and(|p| p.old_index != current_index && !index_applied)
        {
            return Err(Error::Engine(
                "El índice cambió durante la recuperación".into(),
            ));
        }

        let mut paths: Vec<_> = old
            .keys()
            .chain(new.keys().filter(|p| !old.contains_key(*p)))
            .filter(|p| old.get(*p) != new.get(*p))
            .collect();
        paths.sort_unstable();
        if paths.len() > limits.max_entries.min(MAX_FILES) {
            return Err(Error::Limited);
        }
        if paths.iter().any(|p| is_attributes(p))
            || index.entries().iter().any(|entry| {
                let path = entry.path(&index).as_bytes();
                is_attributes(path) && !path.ends_with(b".gitattributes")
            })
        {
            return Err(Error::Filter);
        }
        let attribute_sources = attribute_sources(&paths, &index, &old, ctx)?;
        check_attribute_sources(repo, repository, &attribute_sources, ctx)?;
        let mut attributes = repo
            .attributes_only(
                &index,
                gix::worktree::stack::state::attributes::Source::IdMapping,
            )
            .map_err(engine)?;
        let mut files = Vec::with_capacity(paths.len());
        for path in paths {
            ctx.check()?;
            let text = std::str::from_utf8(path).map_err(engine)?;
            for part in path.split(|b| *b == b'/') {
                gix::validate::path::component(part.as_bstr(), None, Default::default())
                    .map_err(engine)?;
                if part.contains(&b'~') || part.eq_ignore_ascii_case(b".git") {
                    return Err(Error::Unsupported);
                }
            }
            // The index can include staged additions unrelated to either tree.
            // Check their Windows aliases too before admitting a path.
            if old
                .keys()
                .chain(new.keys())
                .map(Vec::as_slice)
                .chain(index.entries().iter().map(|e| e.path(&index).as_bytes()))
                .any(|other| {
                    other != path.as_slice() && ref_write::namespace_conflicts(other, text)
                })
            {
                return Err(conflict(path));
            }
            let before = old.get(path).copied();
            let after = new.get(path).copied();
            let expected = if index_applied { after } else { before };
            match (index.entry_by_path(path.as_bstr()), expected) {
                (None, None) => {}
                (Some(entry), Some(expected))
                    if entry.id == expected.id
                        && entry.mode.bits() == expected.mode
                        && !entry
                            .flags
                            .intersects(Flags::INTENT_TO_ADD | Flags::SKIP_WORKTREE) => {}
                _ => return Err(conflict(path)),
            }
            let target = repository
                .root
                .join(text.replace('/', std::path::MAIN_SEPARATOR_STR));
            let before = file_bytes(repo, before, limits, &mut budget, ctx)?;
            let after = file_bytes(repo, after, limits, &mut budget, ctx)?;
            check_effective_attributes(
                &mut attributes,
                path,
                (before.as_deref(), after.as_deref()),
            )?;
            let observed = observe(
                repository,
                &target,
                path,
                (before.as_deref(), after.as_deref()),
                pending
                    .as_ref()
                    .map(|p| p.backup(repository, files.len()))
                    .as_deref(),
                limits,
                ctx,
            )?;
            if index_applied && observed != Observed::Applied {
                return Err(conflict(path));
            }
            files.push(CheckoutFile {
                path: path.clone(),
                target,
                before,
                after,
                observed,
            });
        }

        let index_bytes = if index_applied {
            None
        } else {
            index.remove_entries(|_, path, _| {
                files
                    .binary_search_by(|file| file.path.as_slice().cmp(path.as_bytes()))
                    .is_ok()
            });
            for file in &files {
                if let Some(entry) = new.get(&file.path) {
                    index.dangerously_push_entry(
                        Default::default(),
                        entry.id,
                        Flags::empty(),
                        Mode::FILE,
                        file.path.as_bstr(),
                    );
                }
            }
            if index.entries().len() > crate::INDEX_ENTRIES
                || index.path_backing().len() > crate::TREE_PATH_BYTES
            {
                return Err(Error::Limited);
            }
            index.sort_entries();
            index.remove_tree();
            index.verify_entries().map_err(engine)?;
            let mut output = Vec::new();
            gix::index::File::from_state(index, repository.git_dir.join("index"))
                .write_to(&mut output, Default::default())
                .map_err(engine)?;
            if output.len() > crate::INDEX_BYTES as usize {
                return Err(Error::Limited);
            }
            Some(output)
        };
        let new_index = index_bytes
            .as_ref()
            .map_or(Ok(current_index), |bytes| index_id(bytes, ctx))?;
        let resuming = pending.is_some();
        let journal = match pending {
            Some(journal) if journal.new_index == new_index => journal,
            Some(_) => return Err(Error::Stale),
            None => {
                let timestamp = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(engine)?
                    .as_nanos();
                Journal {
                    source: branches.0.into(),
                    destination: branches.1.into(),
                    from: commits.0,
                    to: commits.1,
                    old_index: current_index,
                    new_index,
                    nonce: format!("{:x}{timestamp:x}", std::process::id()),
                }
            }
        };
        Ok(Self {
            files,
            journal,
            resuming,
            index_bytes,
            attribute_sources,
        })
    }

    pub fn validate(
        &self,
        repo: &gix::Repository,
        repository: &Identity,
        index: &[u8],
        limits: Limits,
        ctx: &Context<'_>,
    ) -> Result<(), Error> {
        let pending = Journal::read(repository, ctx)?;
        if pending.as_ref() != self.resuming.then_some(&self.journal) {
            return Err(Error::Stale);
        }
        check_attribute_sources(repo, repository, &self.attribute_sources, ctx)?;
        let index = local_pair::decode_index(repo, Some(index), ctx)?;
        let mut attributes = repo
            .attributes_only(
                &index,
                gix::worktree::stack::state::attributes::Source::IdMapping,
            )
            .map_err(engine)?;
        for (i, file) in self.files.iter().enumerate() {
            check_effective_attributes(
                &mut attributes,
                &file.path,
                (file.before.as_deref(), file.after.as_deref()),
            )?;
            let backup = self.resuming.then(|| self.journal.backup(repository, i));
            let current = observe(
                repository,
                &file.target,
                &file.path,
                (file.before.as_deref(), file.after.as_deref()),
                backup.as_deref(),
                limits,
                ctx,
            )?;
            if current != file.observed {
                return Err(conflict(&file.path));
            }
        }
        Ok(())
    }

    pub fn publish_files(
        &self,
        repository: &Identity,
        limits: Limits,
        ctx: &Context<'_>,
        check: &mut impl FnMut() -> Result<(), Error>,
    ) -> Result<(), Error> {
        if !self.resuming {
            let path = repository.git_dir.join(JOURNAL_NAME);
            let Capture::Absent { token } =
                local_pair::capture(&repository.git_dir, &path, 0, ctx)?
            else {
                return Err(Error::Stale);
            };
            jc_fs::stage_missing_file(
                &repository.git_dir,
                &token,
                &self.journal.bytes(),
                &mut || ctx.check().is_err(),
            )?
            .publish()?;
        }
        for (i, file) in self.files.iter().enumerate() {
            check()?;
            let backup = self.journal.backup(repository, i);
            match &file.observed {
                Observed::Applied => {}
                Observed::Before(token) => {
                    let pending = jc_fs::stage_existing_file(
                        &repository.root,
                        token,
                        file.before.as_deref().ok_or(Error::Stale)?,
                        file.after.as_deref().unwrap_or_default(),
                        &repository.git_dir,
                        &mut || ctx.check().is_err(),
                    )?;
                    pending
                        .publish_checkout(&backup, file.after.is_some())
                        .map_err(|error| match error {
                            jc_fs::ReplaceError::Io(error) => Error::Io(error),
                            jc_fs::ReplaceError::Preserved { error, backup } => {
                                Error::Engine(format!("{error}. Original en {}", backup.display()))
                            }
                        })?;
                }
                Observed::Missing(_) => {
                    let Capture::Absent { token } =
                        local_pair::capture(&repository.root, &file.target, 0, ctx)?
                    else {
                        return Err(conflict(&file.path));
                    };
                    jc_fs::create_missing_parents(&repository.root, &token, &mut || {
                        ctx.check().is_err()
                    })?;
                    let Capture::Absent { token } =
                        local_pair::capture(&repository.root, &file.target, 0, ctx)?
                    else {
                        return Err(conflict(&file.path));
                    };
                    jc_fs::stage_missing_file(
                        &repository.root,
                        &token,
                        file.after.as_deref().ok_or(Error::Stale)?,
                        &mut || ctx.check().is_err(),
                    )?
                    .publish()?;
                }
            }
        }
        for (i, file) in self.files.iter().enumerate() {
            let backup = self.journal.backup(repository, i);
            if observe(
                repository,
                &file.target,
                &file.path,
                (file.before.as_deref(), file.after.as_deref()),
                Some(&backup),
                limits,
                ctx,
            )? != Observed::Applied
            {
                return Err(conflict(&file.path));
            }
        }
        Ok(())
    }

    pub fn finish(&self, repository: &Identity, ctx: &Context<'_>) -> Result<(), Error> {
        if Journal::read(repository, ctx)?.as_ref() != Some(&self.journal) {
            return Err(Error::Stale);
        }
        std::fs::remove_file(repository.git_dir.join(JOURNAL_NAME))?;
        Ok(())
    }

    pub fn index_published(&self, token: &local_pair::IndexToken) -> bool {
        token
            .sha256()
            .is_some_and(|digest| digest.as_slice() == self.journal.new_index.as_slice())
    }
}

fn observe(
    repository: &Identity,
    target: &Path,
    path: &[u8],
    (before, after): (Option<&[u8]>, Option<&[u8]>),
    backup: Option<&Path>,
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<Observed, Error> {
    let mut preserved = false;
    if let Some(backup) = backup {
        if let Capture::Present { bytes, .. } =
            local_pair::capture(&repository.git_dir, backup, limits.max_blob, ctx)?
        {
            if before != Some(bytes.as_slice()) {
                return Err(conflict(path));
            }
            preserved = true;
        }
    }
    match local_pair::capture(&repository.root, target, limits.max_blob, ctx)? {
        Capture::Present { token, bytes, .. } => {
            if backup.is_some()
                && (before.is_none() || preserved)
                && after == Some(bytes.as_slice())
            {
                return Ok(Observed::Applied);
            }
            if !preserved && token.has_single_link() && before == Some(bytes.as_slice()) {
                return Ok(Observed::Before(token));
            }
            Err(conflict(path))
        }
        Capture::Absent { .. } if preserved && after.is_some() => {
            let backup = backup.ok_or(Error::Stale)?;
            Err(Error::Engine(format!(
                "Falta {}. Para conservar una posible eliminación local, verifica el respaldo en {} antes de reanudar",
                target.display(),
                backup.display()
            )))
        }
        Capture::Absent { token } if before.is_none() || preserved => {
            if after.is_none() {
                Ok(Observed::Applied)
            } else {
                Ok(Observed::Missing(token))
            }
        }
        Capture::Absent { .. } => Err(conflict(path)),
    }
}
