//! Reviewed changes to the repository-local Git configuration.
use crate::{
    Context, Error, Identity, Limits, branch_switch, controlled, engine, index_write, open,
    ref_write,
};
use gix::{
    bstr::{BStr, ByteSlice},
    config::{
        File as Config,
        file::{Metadata, init::Options},
    },
    lock::{File, Marker, acquire::Fail},
    refs::FullName,
};
use jc_fs::{Capture, FileToken};
use std::{
    collections::BTreeSet,
    io::Write,
    path::{Path, PathBuf},
};

pub enum Change<'a> {
    AddRemote { name: &'a str, location: &'a str },
    RemoveRemote { name: &'a str },
    SetUpstream { remote: &'a str, branch: &'a str },
    ClearUpstream,
}

pub struct Review {
    repository: Identity,
    target: PathBuf,
    original: FileToken,
    before: Vec<u8>,
    after: Vec<u8>,
    head: Option<(FullName, FileToken)>,
    limits: Limits,
    rows: Vec<String>,
}

impl Review {
    pub fn rows(&self) -> &[String] {
        &self.rows
    }
}

fn name(value: &str, limits: Limits) -> Result<(), Error> {
    if value.len() > 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return Err(Error::Engine("El nombre del remoto no es válido".into()));
    }
    ref_write::reference_name(ref_write::RefKind::Branch, value, limits).map(|_| ())
}

fn validate_location(value: &str) -> Result<(), Error> {
    if value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
        return Err(Error::Engine("La ubicación del remoto no es válida".into()));
    }
    // Git's file paths may contain spaces. URL credentials are supplied out of
    // band and are never accepted as part of a configured endpoint.
    let parsed = gix::url::parse(value.as_bytes().as_bstr())
        .map_err(|_| Error::Engine("La ubicación del remoto no es válida".into()))?;
    use gix::url::Scheme;
    if parsed.password().is_some()
        || matches!(parsed.scheme, Scheme::Http | Scheme::Https)
            && (parsed.user().is_some() || value.contains(['?', '#']))
    {
        return Err(Error::Engine(
            "Especifica una ubicación sin credenciales".into(),
        ));
    }
    if !matches!(parsed.scheme, Scheme::File | Scheme::Https | Scheme::Ssh) {
        return Err(Error::Unsupported);
    }
    Ok(())
}

fn config_file(
    root: &Path,
    target: &Path,
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<(FileToken, Vec<u8>), Error> {
    match crate::local_pair::capture(
        root,
        target,
        limits.max_bytes.min(crate::INDEX_BYTES as usize),
        ctx,
    )? {
        Capture::Present { token, bytes, .. } => Ok((token, bytes)),
        Capture::Absent { .. } => Err(Error::Stale),
    }
}

fn clear_branch_upstream(config: &mut Config, branch: &BStr) -> Result<(), Error> {
    let ids: Vec<_> = config
        .sections_by_name("branch")
        .into_iter()
        .flatten()
        .filter(|section| section.header().subsection_name() == Some(branch))
        .map(|section| section.id())
        .collect();
    for id in ids {
        let mut section = config.section_mut_by_id(id).ok_or(Error::Stale)?;
        while section.remove("remote").is_some() {}
        while section.remove("merge").is_some() {}
    }
    Ok(())
}

pub fn prepare(
    root: &Path,
    change: Change<'_>,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<Review, Error> {
    if !cfg!(windows) {
        return Err(Error::Unsupported);
    }
    limits.admit()?;
    controlled(limits.timeout, cancelled, |ctx| {
        let (repo, repository) = open(root, ctx)?;
        let common = repo.common_dir().canonicalize()?;
        if common != repository.git_dir
            || repo.config_snapshot().boolean("extensions.worktreeConfig") == Some(true)
        {
            return Err(Error::Unsupported);
        }
        let target = common.join("config");
        let (original, before) = config_file(&common, &target, limits, ctx)?;
        let mut config =
            Config::from_bytes_no_includes(&before, Metadata::default(), Options::default())
                .map_err(|_| {
                    Error::Engine("La configuración local de Git no se puede interpretar".into())
                })?;
        let mut rows = Vec::new();
        let mut head = None;
        match change {
            Change::AddRemote {
                name: remote,
                location,
            } => {
                name(remote, limits)?;
                validate_location(location)?;
                if config
                    .sections_by_name("remote")
                    .into_iter()
                    .flatten()
                    .any(|section| {
                        section
                            .header()
                            .subsection_name()
                            .is_some_and(|subsection| {
                                subsection.eq_ignore_ascii_case(remote.as_bytes())
                            })
                    })
                {
                    return Err(Error::Engine("Ya existe un remoto con ese nombre".into()));
                }
                let mut section = config.new_section("remote", remote).map_err(engine)?;
                section.set("url", location).map_err(engine)?;
                let spec = format!("+refs/heads/*:refs/remotes/{remote}/*");
                section.set("fetch", spec.as_str()).map_err(engine)?;
                rows.push(format!("Agregar remoto {remote}"));
                rows.push(location.into());
                rows.push(spec);
            }
            Change::RemoveRemote { name: remote } => {
                name(remote, limits)?;
                if config.remove_section("remote", remote).is_none() {
                    return Err(Error::Missing);
                }
                while config.remove_section("remote", remote).is_some() {}
                rows.push(format!("Quitar la configuración del remoto {remote}"));
                rows.push("Se conservan las referencias de seguimiento y sus commits".into());
                let branches: BTreeSet<_> = config
                    .sections_by_name("branch")
                    .into_iter()
                    .flatten()
                    .filter(|section| {
                        section
                            .values("remote")
                            .iter()
                            .any(|value| value.as_bstr() == remote.as_bytes().as_bstr())
                    })
                    .filter_map(|section| section.header().subsection_name().map(ToOwned::to_owned))
                    .collect();
                if branches.len().saturating_add(rows.len()) > limits.max_entries {
                    return Err(Error::Limited);
                }
                for branch in branches {
                    ctx.check()?;
                    let values = config
                        .raw_values_by("branch", Some(branch.as_bstr()), "remote")
                        .map_err(|_| Error::Stale)?;
                    if values
                        .iter()
                        .any(|value| value.as_bstr() != remote.as_bytes().as_bstr())
                    {
                        return Err(Error::Engine(
                            "Hay una configuración de seguimiento ambigua para ese remoto".into(),
                        ));
                    }
                    clear_branch_upstream(&mut config, branch.as_ref())?;
                    rows.push(format!("Quitar seguimiento de {}", branch.to_str_lossy()));
                }
            }
            Change::SetUpstream { .. } | Change::ClearUpstream => {
                let reference = repo
                    .head_name()
                    .map_err(engine)?
                    .ok_or(Error::Unsupported)?;
                let branch = reference
                    .as_bstr()
                    .strip_prefix(b"refs/heads/")
                    .ok_or(Error::Unsupported)?
                    .as_bstr();
                if branch.len() > limits.max_path || branch.iter().any(u8::is_ascii_control) {
                    return Err(Error::Unsupported);
                }
                let token = branch_switch::capture_head(&repository.git_dir, &reference, ctx)?;
                clear_branch_upstream(&mut config, branch)?;
                if let Change::SetUpstream {
                    remote,
                    branch: upstream,
                } = change
                {
                    name(remote, limits)?;
                    if config
                        .section("remote", remote)
                        .map_err(|_| Error::Missing)?
                        .value("url")
                        .is_none()
                    {
                        return Err(Error::Missing);
                    }
                    let full =
                        ref_write::reference_name(ref_write::RefKind::Branch, upstream, limits)?;
                    let mut section = config
                        .section_mut_or_create_new("branch", branch)
                        .map_err(engine)?;
                    section.set("remote", remote).map_err(engine)?;
                    section.set("merge", full.as_bstr()).map_err(engine)?;
                    rows.push(format!(
                        "{} seguirá a {remote}/{upstream}",
                        branch.to_str_lossy()
                    ));
                } else {
                    rows.push(format!(
                        "Quitar el seguimiento de {}",
                        branch.to_str_lossy()
                    ));
                }
                head = Some((reference, token));
            }
        }
        let mut after = Vec::new();
        config.write_to(&mut after)?;
        if before.len().saturating_add(after.len()) > limits.max_bytes {
            return Err(Error::Limited);
        }
        if before == after {
            return Err(Error::Engine(
                "La configuración ya coincide con lo solicitado".into(),
            ));
        }
        ctx.check()?;
        Ok(Review {
            repository,
            target,
            original,
            before,
            after,
            head,
            limits,
            rows,
        })
    })
}

pub fn apply(review: Review, cancelled: impl FnMut() -> bool + Send) -> Result<(), Error> {
    controlled(review.limits.timeout, cancelled, |ctx| {
        let git = &review.repository.git_dir;
        let _head = review
            .head
            .as_ref()
            .map(|_| {
                Marker::acquire_to_hold_resource(git.join("HEAD"), Fail::Immediately, None, 0)
                    .map_err(engine)
            })
            .transpose()?;
        let mut lock = File::acquire_to_update_resource(&review.target, Fail::Immediately, None, 0)
            .map_err(engine)?;
        let (repo, identity) = open(&review.repository.root, ctx)?;
        index_write::same_repository(&review.repository, &identity)?;
        if repo.common_dir().canonicalize()? != *git {
            return Err(Error::Stale);
        }
        if let Some((branch, token)) = &review.head
            && branch_switch::capture_head(git, branch, ctx)? != *token
        {
            return Err(Error::Stale);
        }
        if config_file(git, &review.target, review.limits, ctx)? != (review.original, review.before)
        {
            return Err(Error::Stale);
        }
        lock.write_all(&review.after)?;
        lock.with_mut(|file| file.sync_all())?;
        ctx.check()?;
        lock.commit().map_err(engine)?;
        Ok(())
    })
}
