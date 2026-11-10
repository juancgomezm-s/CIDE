//! Byte-exact checkout into new private directories. No conversion drivers.
use crate::{Context, Error, Limits, engine, index_write, local_pair, revision};
use gix::{
    ObjectId,
    bstr::ByteSlice,
    index::entry::{Flags, Mode},
};
use jc_fs::{Capture, MissingToken};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
};

pub(crate) struct CheckoutTree {
    entries: Vec<Entry>,
    index: Vec<u8>,
    bytes: usize,
}

struct Entry {
    path: PathBuf,
    content: Option<Arc<[u8]>>,
}

pub(crate) fn prepare(
    repo: &gix::Repository,
    commit: ObjectId,
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<CheckoutTree, Error> {
    let mut budget = revision::Budget::new(limits);
    let tree = revision::commit(repo, commit, limits, &mut budget, ctx)?.tree;
    let sources = revision::checkout_tree_entries(repo, tree, limits, &mut budget, ctx)?;
    if sources.len() > limits.max_entries {
        return Err(Error::Limited);
    }
    let mut state = gix::index::State::new(repo.object_hash());
    let mut entries = Vec::with_capacity(sources.len());
    let mut blobs = BTreeMap::<ObjectId, Arc<[u8]>>::new();
    let mut names = BTreeMap::<String, String>::new();
    let mut leaves = BTreeSet::new();
    let mut bytes = 0usize;
    for (path, source) in sources {
        ctx.check()?;
        let text = std::str::from_utf8(&path).map_err(engine)?;
        for component in path.split(|&byte| byte == b'/') {
            gix::validate::path::component(component.as_bstr(), None, Default::default())
                .map_err(engine)?;
            if component.contains(&b'~') {
                return Err(Error::Unsupported);
            }
        }
        for end in text
            .match_indices('/')
            .map(|(offset, _)| offset)
            .chain([text.len()])
        {
            let exact = &text[..end];
            let key = exact.to_lowercase();
            if names.get(&key).is_some_and(|previous| previous != exact) || leaves.contains(&key) {
                return Err(Error::Engine(
                    "El commit contiene rutas incompatibles en Windows".into(),
                ));
            }
            names.entry(key.clone()).or_insert_with(|| exact.to_owned());
            if end == text.len() {
                leaves.insert(key);
            }
        }
        let mode = Mode::from_bits(source.mode).ok_or(Error::Unsupported)?;
        let content = match mode {
            Mode::COMMIT => None,
            Mode::FILE | Mode::FILE_EXECUTABLE => {
                let data = if let Some(data) = blobs.get(&source.id) {
                    Arc::clone(data)
                } else {
                    let blob = revision::verified(repo, source.id, limits.max_blob, ctx)?
                        .try_into_blob()
                        .map_err(engine)?;
                    let data: Arc<[u8]> = blob.data.as_slice().into();
                    blobs.insert(source.id, Arc::clone(&data));
                    data
                };
                budget.charge(data.len())?;
                bytes = bytes.checked_add(data.len()).ok_or(Error::Limited)?;
                Some(data)
            }
            _ => return Err(Error::Unsupported),
        };
        state.dangerously_push_entry(
            Default::default(),
            source.id,
            Flags::empty(),
            mode,
            path.as_bstr(),
        );
        entries.push(Entry {
            path: PathBuf::from(text),
            content,
        });
    }
    state.verify_entries().map_err(engine)?;
    let mut index = Vec::new();
    gix::index::File::from_state(state, "index")
        .write_to(&mut index, Default::default())
        .map_err(engine)?;
    if index.len() > crate::INDEX_BYTES as usize {
        return Err(Error::Limited);
    }
    Ok(CheckoutTree {
        entries,
        index,
        bytes,
    })
}

impl CheckoutTree {
    pub(crate) fn file_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.content.is_some())
            .count()
    }

    pub(crate) fn byte_count(&self) -> usize {
        self.bytes
    }

    /// The caller owns both staging directories and publishes only on success.
    pub(crate) fn write(
        &self,
        workdir: &Path,
        index: &Path,
        ctx: &Context<'_>,
    ) -> Result<(), Error> {
        index_write::ordinary(workdir, true)?;
        let gitdir = index.parent().ok_or(Error::InvalidRoot)?;
        index_write::ordinary(gitdir, true)?;
        let index_token = missing(gitdir, index, ctx)?;
        if !index_token.is_leaf() {
            return Err(Error::Stale);
        }
        for entry in &self.entries {
            let path = workdir.join(&entry.path);
            if let Some(bytes) = &entry.content {
                write_new(workdir, &path, bytes, ctx)?;
            } else {
                // An uninitialized gitlink gets an empty directory and an index
                // entry. No URL is contacted as a side effect of checkout.
                let sentinel = path.join(".cide-uninitialized");
                let token = missing(workdir, &sentinel, ctx)?;
                if token.is_leaf() {
                    return Err(Error::Stale);
                }
                jc_fs::create_missing_parents(workdir, &token, &mut || ctx.check().is_err())?;
            }
        }
        ctx.check()?;
        jc_fs::stage_missing_file(gitdir, &index_token, &self.index, &mut || {
            ctx.check().is_err()
        })?
        .publish()?;
        Ok(())
    }

    /// Resume a previously interrupted create-only checkout. First observe the
    /// complete file/index subset; pre-existing regular files must match the
    /// pinned bytes exactly. Only after the full preflight succeeds are missing
    /// files and directories created. Nothing is replaced or removed.
    pub(crate) fn resume_or_verify(
        &self,
        workdir: &Path,
        index: &Path,
        ctx: &Context<'_>,
    ) -> Result<(), Error> {
        index_write::ordinary(workdir, true)?;
        let gitdir = index.parent().ok_or(Error::InvalidRoot)?;
        index_write::ordinary(gitdir, true)?;

        let index_observation =
            local_pair::capture(gitdir, index, crate::INDEX_BYTES as usize, ctx)?;
        match &index_observation {
            Capture::Present { bytes, .. } if bytes != &self.index => return Err(Error::Stale),
            Capture::Present { .. } | Capture::Absent { .. } => {}
        }

        let mut missing_files = Vec::new();
        let mut missing_gitlinks = Vec::new();
        for entry in &self.entries {
            ctx.check()?;
            let path = workdir.join(&entry.path);
            match &entry.content {
                Some(bytes) => match local_pair::capture(workdir, &path, bytes.len(), ctx)? {
                    Capture::Present { bytes: current, .. }
                        if current.as_slice() == bytes.as_ref() => {}
                    Capture::Present { .. } => return Err(Error::Stale),
                    Capture::Absent { token } => missing_files.push((path, bytes.clone(), token)),
                },
                None => match std::fs::symlink_metadata(&path) {
                    Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                        if std::fs::read_dir(&path)?.next().transpose()?.is_some() {
                            return Err(Error::Stale);
                        }
                    }
                    Ok(_) => return Err(Error::Stale),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        missing_gitlinks.push(path);
                    }
                    Err(error) => return Err(error.into()),
                },
            }
        }

        for (path, bytes, mut token) in missing_files {
            ctx.check()?;
            if !token.is_leaf() {
                jc_fs::create_missing_parents(workdir, &token, &mut || ctx.check().is_err())?;
                let Capture::Absent { token: current } =
                    local_pair::capture(workdir, &path, 0, ctx)?
                else {
                    return Err(Error::Stale);
                };
                if !token.is_compatible_after_creating_parents(&current) || !current.is_leaf() {
                    return Err(Error::Stale);
                }
                token = current;
            }
            jc_fs::stage_missing_file(workdir, &token, &bytes, &mut || ctx.check().is_err())?
                .publish()?;
        }

        for path in missing_gitlinks {
            ctx.check()?;
            let sentinel = path.join(".cide-uninitialized");
            let token = missing(workdir, &sentinel, ctx)?;
            if !token.is_leaf() {
                jc_fs::create_missing_parents(workdir, &token, &mut || ctx.check().is_err())?;
                let current = missing(workdir, &sentinel, ctx)?;
                if !token.is_compatible_after_creating_parents(&current) {
                    return Err(Error::Stale);
                }
            }
        }

        if matches!(index_observation, Capture::Absent { .. }) {
            let token = missing(gitdir, index, ctx)?;
            if !token.is_leaf() {
                return Err(Error::Stale);
            }
            jc_fs::stage_missing_file(gitdir, &token, &self.index, &mut || ctx.check().is_err())?
                .publish()?;
        }
        Ok(())
    }
}

fn missing(root: &Path, path: &Path, ctx: &Context<'_>) -> Result<MissingToken, Error> {
    match local_pair::capture(root, path, 0, ctx)? {
        Capture::Absent { token } => Ok(token),
        Capture::Present { .. } => Err(Error::Stale),
    }
}

pub(crate) fn write_new(
    root: &Path,
    path: &Path,
    bytes: &[u8],
    ctx: &Context<'_>,
) -> Result<(), Error> {
    let mut token = missing(root, path, ctx)?;
    let mut cancelled = || ctx.check().is_err();
    if !token.is_leaf() {
        jc_fs::create_missing_parents(root, &token, &mut cancelled)?;
        let current = missing(root, path, ctx)?;
        if !token.is_compatible_after_creating_parents(&current) || !current.is_leaf() {
            return Err(Error::Stale);
        }
        token = current;
    }
    jc_fs::stage_missing_file(root, &token, bytes, &mut cancelled)?.publish()?;
    Ok(())
}
