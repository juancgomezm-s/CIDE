//! Bounded, read-only inventory of committed submodule registrations.
//!
//! No submodule is initialized, fetched, opened as a nested repository, or
//! updated. This keeps inventory independent of network and external programs.

use crate::{Context, Error, Identity, Limits, controlled, open};
use gix::bstr::ByteSlice;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

const GITMODULES_BYTES: usize = 256 * 1024;
const MAX_DEPTH: usize = 128;

#[derive(Debug, Clone)]
pub struct Submodule {
    /// Raw Git path bytes. This may not be valid UTF-8.
    pub path: Vec<u8>,
    pub object_id: String,
    /// The subsection name in `.gitmodules`, if the commit has a registration.
    pub name: Option<Vec<u8>>,
    /// URL from the selected commit's `.gitmodules`; never fetched automatically.
    pub url: Option<Vec<u8>>,
    pub registered: bool,
}

#[derive(Clone)]
pub struct Inventory {
    pub repository: Identity,
    pub commit: Option<String>,
    pub submodules: Vec<Submodule>,
}

struct Scan {
    entries: usize,
    max_entries: usize,
    max_path: usize,
    gitlinks: BTreeMap<Vec<u8>, String>,
    gitmodules: Option<Vec<u8>>,
}

type Registration = (Vec<u8>, Option<Vec<u8>>);
type Registrations = BTreeMap<Vec<u8>, Registration>;

fn scan_tree(
    repo: &gix::Repository,
    tree_id: gix::ObjectId,
    prefix: &[u8],
    depth: usize,
    scan: &mut Scan,
    ctx: &Context<'_>,
) -> Result<(), Error> {
    if depth > MAX_DEPTH {
        return Err(Error::Limited);
    }
    ctx.check()?;
    let tree = repo.find_tree(tree_id).map_err(crate::engine)?;
    for item in tree.iter() {
        ctx.check()?;
        scan.entries = scan.entries.checked_add(1).ok_or(Error::Limited)?;
        if scan.entries > scan.max_entries {
            return Err(Error::Limited);
        }
        let item = item.map_err(crate::engine)?;
        let name = item.filename().as_bytes();
        if name.is_empty()
            || name.contains(&0)
            || name.contains(&b'/')
            || name == b"."
            || name == b".."
        {
            return Err(Error::Unsupported);
        }
        let mut path =
            Vec::with_capacity(prefix.len().saturating_add(name.len()).saturating_add(1));
        if !prefix.is_empty() {
            path.extend_from_slice(prefix);
            path.push(b'/');
        }
        path.extend_from_slice(name);
        if path.len() > scan.max_path {
            return Err(Error::Limited);
        }
        let mode = item.mode().value() as u32;
        match mode {
            0o160000 => {
                if scan
                    .gitlinks
                    .insert(path, item.object_id().to_string())
                    .is_some()
                {
                    return Err(Error::Unsupported);
                }
            }
            0o40000 => scan_tree(repo, item.object_id(), &path, depth + 1, scan, ctx)?,
            0o100644 | 0o100755 if prefix.is_empty() && name == b".gitmodules" => {
                let header = repo.find_header(item.object_id()).map_err(crate::engine)?;
                let size = usize::try_from(header.size()).map_err(|_| Error::Limited)?;
                if size > GITMODULES_BYTES {
                    return Err(Error::Limited);
                }
                let object = repo.find_object(item.object_id()).map_err(crate::engine)?;
                if object.kind != gix::object::Kind::Blob || object.data.len() > GITMODULES_BYTES {
                    return Err(Error::Unsupported);
                }
                scan.gitmodules = Some(object.data.to_vec());
            }
            _ => {}
        }
    }
    Ok(())
}

fn registrations(bytes: Option<&[u8]>, max_path: usize) -> Result<Registrations, Error> {
    let Some(bytes) = bytes else {
        return Ok(BTreeMap::new());
    };
    let source = std::str::from_utf8(bytes).map_err(|_| Error::Unsupported)?;
    let config = gix::config::File::try_from(source).map_err(crate::engine)?;
    let mut paths = BTreeMap::new();
    let mut names = BTreeSet::new();
    for section in config.sections_by_name("submodule").into_iter().flatten() {
        let name = section
            .header()
            .subsection_name()
            .ok_or(Error::Unsupported)?
            .as_bytes()
            .to_vec();
        if name.is_empty() || !names.insert(name.clone()) {
            return Err(Error::Unsupported);
        }
        let key = std::str::from_utf8(&name).map_err(|_| Error::Unsupported)?;
        let raw_path = config
            .string_by("submodule", key, "path")
            .ok_or(Error::Unsupported)?;
        let path = raw_path.as_bytes().to_vec();
        crate::revision::path_valid(&path, max_path)?;
        let url = config
            .string_by("submodule", key, "url")
            .map(|value| value.to_vec());
        if paths.insert(path, (name, url)).is_some() {
            return Err(Error::Unsupported);
        }
    }
    Ok(paths)
}

fn list_inner(root: &Path, limits: Limits, ctx: &Context<'_>) -> Result<Inventory, Error> {
    limits.admit()?;
    let (repo, identity) = open(root, ctx)?;
    let commit = match repo.head_commit() {
        Ok(commit) => Some(commit),
        Err(error) if error.is_not_found() => None,
        Err(error) => return Err(crate::engine(error)),
    };
    let Some(commit) = commit else {
        return Ok(Inventory {
            repository: identity,
            commit: None,
            submodules: Vec::new(),
        });
    };
    let commit_id = commit.id().detach().to_string();
    let mut scan = Scan {
        entries: 0,
        max_entries: limits.max_entries,
        max_path: limits.max_path,
        gitlinks: BTreeMap::new(),
        gitmodules: None,
    };
    scan_tree(
        &repo,
        commit.tree_id().map_err(crate::engine)?.detach(),
        &[],
        0,
        &mut scan,
        ctx,
    )?;
    let registrations = registrations(scan.gitmodules.as_deref(), limits.max_path)?;
    let mut submodules = Vec::with_capacity(scan.gitlinks.len());
    for (path, object_id) in scan.gitlinks {
        ctx.check()?;
        let registration = registrations.get(&path).cloned();
        let registered = registration.is_some();
        let (name, url) = registration.map_or((None, None), |(name, url)| (Some(name), url));
        submodules.push(Submodule {
            path,
            object_id,
            name,
            url,
            registered,
        });
    }
    // Mark registration by exact raw path; do not infer it from directory presence.
    for submodule in &mut submodules {
        submodule.registered = registrations.contains_key(&submodule.path);
    }
    Ok(Inventory {
        repository: identity,
        commit: Some(commit_id),
        submodules,
    })
}

/// List gitlinks in the current committed tree and match them to the same
/// commit's `.gitmodules`. Missing registration remains visible as such.
pub fn list(
    root: &Path,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<Inventory, Error> {
    controlled(limits.timeout, cancelled, |ctx| {
        list_inner(root, limits, ctx)
    })
}
