//! Reviewed local branch and tag mutations.

use crate::{Context, Error, Identity, Limits, controlled, engine, graph, index_write, revision};
use gix::{
    bstr::{BString, ByteSlice},
    lock::{Marker, acquire::Fail},
    refs::{
        FullName, Target,
        store::WriteReflog,
        transaction::{Change, PreviousValue, RefEdit},
    },
};
use std::{
    path::Path,
    sync::atomic::{AtomicBool, AtomicU8},
    time::Instant,
};

pub const REF_NAME_BYTES: usize = 240;
const COMMIT_BYTES: usize = 64 * 1024;
const REFLOG_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefKind {
    Branch,
    Tag,
}

impl RefKind {
    fn prefix(self) -> &'static str {
        match self {
            Self::Branch => "refs/heads/",
            Self::Tag => "refs/tags/",
        }
    }
}

/// Pins the exact commit and a previously absent destination. Only applying
/// this plan can create the reference.
pub struct RefPlan {
    repository: Identity,
    limits: Limits,
    kind: RefKind,
    reference: FullName,
    target: gix::ObjectId,
}

impl RefPlan {
    pub fn kind(&self) -> RefKind {
        self.kind
    }

    pub fn name(&self) -> &str {
        &self.full_name()[self.kind.prefix().len()..]
    }

    pub fn full_name(&self) -> &str {
        self.reference.as_bstr().to_str().expect("UTF-8 ref name")
    }

    pub fn target(&self) -> gix::ObjectId {
        self.target
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct RefPublication {
    pub kind: RefKind,
    pub name: String,
    pub target: gix::ObjectId,
}

pub(super) fn reference_name(kind: RefKind, name: &str, limits: Limits) -> Result<FullName, Error> {
    if name.len() > REF_NAME_BYTES || name.len() + kind.prefix().len() > limits.max_path {
        return Err(Error::Limited);
    }
    let invalid = || Error::Engine("El nombre de la referencia no es válido".into());
    if name.starts_with('-')
        || name == "@"
        || (kind == RefKind::Branch && name.eq_ignore_ascii_case("HEAD"))
    {
        return Err(invalid());
    }
    for part in name.split('/') {
        gix::validate::path::component(part.as_bytes().as_bstr(), None, Default::default())
            .map_err(|_| invalid())?;
        let stem = part.split('.').next().unwrap_or_default();
        let superscript_device = stem.strip_suffix(['¹', '²', '³']).is_some_and(|prefix| {
            prefix.eq_ignore_ascii_case("COM") || prefix.eq_ignore_ascii_case("LPT")
        });
        if part
            .as_bytes()
            .last_chunk::<5>()
            .is_some_and(|suffix| suffix.eq_ignore_ascii_case(b".lock"))
            || superscript_device
            || part.chars().any(char::is_control)
        {
            return Err(invalid());
        }
    }
    FullName::try_from(BString::from(format!("{}{name}", kind.prefix()))).map_err(|_| invalid())
}

fn verify_target(
    repo: &gix::Repository,
    target: gix::ObjectId,
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<(), Error> {
    if target.is_null() || target.kind() != repo.object_hash() {
        return Err(Error::Unsupported);
    }
    revision::verified(repo, target, limits.max_bytes.min(COMMIT_BYTES), ctx)?
        .try_into_commit()
        .map_err(|_| Error::Engine("El destino de la referencia debe ser un commit".into()))?
        .tree_id()
        .map_err(engine)?;
    Ok(())
}

pub(super) fn namespace_conflicts(existing: &[u8], requested: &str) -> bool {
    for (existing, requested) in existing
        .split(|byte| *byte == b'/')
        .zip(requested.split('/'))
    {
        if existing == requested.as_bytes() {
            continue;
        }
        // Reject case aliases even in an ancestor directory. Packed refs have
        // no filesystem path whose casing could reveal the collision.
        return existing.eq_ignore_ascii_case(requested.as_bytes())
            || std::str::from_utf8(existing)
                .is_ok_and(|existing| existing.to_uppercase() == requested.to_uppercase());
    }
    // Identical paths or one ends while the other continues in that namespace.
    true
}

fn destination_available(
    repo: &gix::Repository,
    identity: &Identity,
    name: &str,
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<bool, Error> {
    let (_, refs) = graph::references(repo, limits, &mut revision::Budget::new(limits), ctx)?;
    destination_absent(identity, name, &refs, ctx)
}

fn destination_absent(
    identity: &Identity,
    name: &str,
    refs: &[graph::RefEntry],
    ctx: &Context<'_>,
) -> Result<bool, Error> {
    if refs
        .iter()
        .any(|entry| namespace_conflicts(&entry.name, name))
    {
        return Ok(false);
    }
    // Also guard empty directories and filesystem aliases absent from gix's
    // ref enumeration. A transaction must not remove or reuse such a target.
    ordinary_path_absent(&identity.git_dir, name, ctx)
}

pub(super) fn ordinary_path_absent(
    root: &Path,
    name: &str,
    ctx: &Context<'_>,
) -> Result<bool, Error> {
    let mut path = root.to_owned();
    let mut parts = name.split('/').peekable();
    while let Some(part) = parts.next() {
        ctx.check()?;
        path.push(part);
        if !index_write::exists(&path)? {
            return Ok(true);
        }
        if parts.peek().is_none() {
            index_write::ordinary(&path, false)?;
            if path.canonicalize()? != path {
                return Err(Error::Unsupported);
            }
            return Ok(false);
        }
        index_write::ordinary(&path, true)?;
        if path.canonicalize()? != path {
            return Err(Error::Unsupported);
        }
    }
    Ok(false)
}

/// Read-only review of an explicit full commit ID. HEAD and the editor's live
/// files are not sources for the operation.
pub fn prepare_ref(
    root: &Path,
    target: &str,
    kind: RefKind,
    name: &str,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<RefPlan, Error> {
    limits.admit()?;
    let reference = reference_name(kind, name, limits)?;
    let target = gix::ObjectId::from_hex(target.as_bytes()).map_err(engine)?;
    controlled(limits.timeout, cancelled, |ctx| {
        let (repo, repository) = index_write::open_for_write(root, ctx)?;
        verify_target(&repo, target, limits, ctx)?;
        let plan = RefPlan {
            repository,
            limits,
            kind,
            reference,
            target,
        };
        if !destination_available(&repo, &plan.repository, plan.full_name(), limits, ctx)? {
            return Err(Error::Engine(
                "El nombre ya existe o entra en conflicto con otra referencia".into(),
            ));
        }
        Ok(plan)
    })
}

/// Publish one direct ref using the embedded store's lock and CAS. Neither
/// HEAD, the index, the working tree nor any reflog is updated.
pub fn apply_ref(
    plan: RefPlan,
    mut cancelled: impl FnMut() -> bool,
) -> Result<RefPublication, Error> {
    let stop = AtomicBool::new(false);
    let reason = AtomicU8::new(0);
    let ctx = Context {
        stop: &stop,
        reason: &reason,
        deadline: Instant::now() + plan.limits.timeout,
    };
    let mut check = || {
        if cancelled() {
            Err(Error::Cancelled)
        } else {
            ctx.check()
        }
    };
    let verify_destination = |repo: &gix::Repository, identity: &Identity| {
        index_write::same_repository(&plan.repository, identity)?;
        if !destination_available(repo, identity, plan.full_name(), plan.limits, &ctx)? {
            return Err(Error::Stale);
        }
        Ok(())
    };
    check()?;
    let (mut repo, identity) = index_write::open_for_write(&plan.repository.root, &ctx)?;
    verify_destination(&repo, &identity)?;
    verify_target(&repo, plan.target, plan.limits, &ctx)?;
    check()?;
    repo.refs.write_reflog = WriteReflog::Disable;
    let transaction = repo
        .refs
        .transaction()
        .prepare(
            [RefEdit::update(
                plan.reference.clone(),
                Target::Object(plan.target),
                PreviousValue::MustNotExist,
                BString::default(),
            )],
            Fail::Immediately,
            Fail::Immediately,
        )
        .map_err(engine)?;
    let (fresh, identity) = index_write::open_for_write(&plan.repository.root, &ctx)?;
    verify_destination(&fresh, &identity)?;
    verify_target(&fresh, plan.target, plan.limits, &ctx)?;
    let publication = RefPublication {
        kind: plan.kind,
        name: plan.name().to_owned(),
        target: plan.target,
    };
    check()?;
    let edits = transaction.commit(None).map_err(|error| {
        Error::Engine(format!(
            "Publicación de {} incierta. Actualiza el repositorio antes de reintentar ({})",
            plan.full_name(),
            engine(error)
        ))
    })?;
    // gix accepts an identical existing target as a no-op under MustNotExist
    // and rewrites its expectation. A create-only operation cannot report it
    // as success, even if the other writer removed the ref before our recheck.
    if !matches!(
        edits.as_slice(),
        [RefEdit {
            change: Change::Update {
                expected: PreviousValue::MustNotExist,
                ..
            },
            ..
        }]
    ) {
        return Err(Error::Stale);
    }
    Ok(publication)
}

/// A reviewed rename or removal of an existing direct reference.
pub struct RefChangePlan {
    repository: Identity,
    limits: Limits,
    kind: RefKind,
    source: FullName,
    destination: Option<FullName>,
    target: gix::ObjectId,
    deletion_head: Option<graph::Head>,
    reflog: Option<jc_fs::FileToken>,
}

impl RefChangePlan {
    pub fn kind(&self) -> RefKind {
        self.kind
    }

    pub fn source(&self) -> &str {
        &full_name(&self.source)[self.kind.prefix().len()..]
    }

    pub fn destination(&self) -> Option<&str> {
        self.destination
            .as_ref()
            .map(|name| &full_name(name)[self.kind.prefix().len()..])
    }

    pub fn target(&self) -> gix::ObjectId {
        self.target
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct RefChangePublication {
    pub kind: RefKind,
    pub source: String,
    pub destination: Option<String>,
    pub target: gix::ObjectId,
}

fn full_name(name: &FullName) -> &str {
    name.as_bstr().to_str().expect("UTF-8 ref name")
}

fn reflog_name(name: &FullName) -> String {
    format!("logs/{}", full_name(name))
}

fn capture_reflog(
    plan: &RefChangePlan,
    ctx: &Context<'_>,
) -> Result<Option<(jc_fs::FileToken, Vec<u8>)>, Error> {
    let git = &plan.repository.git_dir;
    let name = reflog_name(&plan.source);
    if ordinary_path_absent(git, &name, ctx)? {
        return Ok(None);
    }
    let capture = jc_fs::capture_in_root(git, &git.join(name), REFLOG_BYTES, &mut || {
        ctx.check().is_err()
    });
    ctx.check()?;
    match capture? {
        jc_fs::Capture::Present { token, bytes, .. } => Ok(Some((token, bytes))),
        jc_fs::Capture::Absent { .. } => Err(Error::Stale),
    }
}

fn stage_new_file(
    git: &Path,
    name: &str,
    bytes: &[u8],
    ctx: &Context<'_>,
) -> Result<jc_fs::PendingMissingFile, Error> {
    let mut cancelled = || ctx.check().is_err();
    let token = jc_fs::capture_in_root(git, &git.join(name), bytes.len(), &mut cancelled);
    ctx.check()?;
    let jc_fs::Capture::Absent { token } = token? else {
        return Err(Error::Stale);
    };
    let pending = jc_fs::stage_missing_file(git, &token, bytes, &mut cancelled);
    ctx.check()?;
    Ok(pending?)
}

impl RefChangePlan {
    fn inspect(
        &self,
        repo: &gix::Repository,
        identity: &Identity,
        ctx: &Context<'_>,
    ) -> Result<graph::Head, Error> {
        index_write::same_repository(&self.repository, identity)?;
        if index_write::exists(&identity.git_dir.join("worktrees"))? {
            return Err(Error::Engine(
                "Las referencias de repositorios con árboles vinculados requieren otro flujo"
                    .into(),
            ));
        }
        let (head, refs) = graph::references(
            repo,
            self.limits,
            &mut revision::Budget::new(self.limits),
            ctx,
        )?;
        let name = full_name(&self.source);
        let index = refs
            .binary_search_by(|entry| entry.name.as_slice().cmp(name.as_bytes()))
            .map_err(|_| Error::Stale)?;
        let expected = self.target.to_string();
        if !matches!(&refs[index].target, graph::RefTarget::Direct(id) if id == &expected) {
            return Err(Error::Stale);
        }
        let source_is_active = match &head {
            graph::Head::Attached { name: active, .. } | graph::Head::Unborn { name: active } => {
                namespace_conflicts(active, name)
            }
            graph::Head::Detached { .. } => false,
        };
        if source_is_active
            || refs.iter().any(|entry| {
                matches!(&entry.target, graph::RefTarget::Symbolic(target) if namespace_conflicts(target, name))
            })
        {
            return Err(Error::Engine(
                "La referencia está activa o es destino de una referencia simbólica".into(),
            ));
        }
        if self.kind == RefKind::Branch {
            let config = repo.config_snapshot();
            if [Some(self.source()), self.destination()]
                .into_iter()
                .flatten()
                .any(|name| {
                    config
                        .plumbing()
                        .section("branch", Some(name.as_bytes().as_bstr()))
                        .is_ok()
                })
            {
                return Err(Error::Engine(
                    "La rama tiene configuración propia. Su traslado requiere otro flujo".into(),
                ));
            }
        }
        if let Some(destination) = &self.destination {
            if !destination_absent(identity, full_name(destination), &refs, ctx)?
                || !ordinary_path_absent(&identity.git_dir, &reflog_name(destination), ctx)?
            {
                return Err(Error::Engine(
                    "El destino o su historial ya existe o entra en conflicto con otra referencia"
                        .into(),
                ));
            }
        }
        // Reject a loose source reached through a filesystem alias, including
        // case-insensitive aliases that would not have matched the ref snapshot.
        ordinary_path_absent(&identity.git_dir, name, ctx)?;
        Ok(head)
    }

    fn check_branch_deletion(
        &self,
        repo: &gix::Repository,
        head: &graph::Head,
        ctx: &Context<'_>,
    ) -> Result<(), Error> {
        let merged = match head.target() {
            Some(start) => graph::contains_ancestor(repo, start, self.target, self.limits, ctx)?,
            None => false,
        };
        if !merged {
            return Err(Error::Engine(
                "La rama no está integrada en HEAD. Conserva su referencia".into(),
            ));
        }
        Ok(())
    }
}

/// Review an exact short source name and its observed raw object ID. An absent
/// destination requests deletion. An annotated tag retains its tag object.
pub fn prepare_ref_change(
    root: &Path,
    kind: RefKind,
    source: &str,
    expected_target: &str,
    destination: Option<&str>,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<RefChangePlan, Error> {
    if destination.is_some() && !cfg!(windows) {
        return Err(Error::Unsupported);
    }
    limits.admit()?;
    let source = reference_name(kind, source, limits)?;
    let destination = destination
        .map(|name| reference_name(kind, name, limits))
        .transpose()?;
    let target = gix::ObjectId::from_hex(expected_target.as_bytes()).map_err(engine)?;
    controlled(limits.timeout, cancelled, |ctx| {
        let (repo, repository) = index_write::open_for_write(root, ctx)?;
        let mut plan = RefChangePlan {
            repository,
            limits,
            kind,
            source,
            destination,
            target,
            deletion_head: None,
            reflog: None,
        };
        let head = plan.inspect(&repo, &plan.repository, ctx)?;
        if kind == RefKind::Branch {
            if plan.destination.is_none() {
                plan.check_branch_deletion(&repo, &head, ctx)?;
                plan.deletion_head = Some(head);
            } else {
                verify_target(&repo, plan.target, plan.limits, ctx)?;
            }
        }
        plan.reflog = capture_reflog(&plan, ctx)?.map(|(token, _)| token);
        Ok(plan)
    })
}

/// Publishes with source CAS and an absent destination. A rename creates
/// the new reference before removing the old one. A late I/O error
/// may leave both names and must be re-observed rather than retried blindly.
pub fn apply_ref_change(
    plan: RefChangePlan,
    mut cancelled: impl FnMut() -> bool,
) -> Result<RefChangePublication, Error> {
    let stop = AtomicBool::new(false);
    let reason = AtomicU8::new(0);
    let ctx = Context {
        stop: &stop,
        reason: &reason,
        deadline: Instant::now() + plan.limits.timeout,
    };
    let mut check = || {
        if cancelled() {
            Err(Error::Cancelled)
        } else {
            ctx.check()
        }
    };
    let git = &plan.repository.git_dir;
    let hold = |name: &str| {
        Marker::acquire_to_hold_resource(git.join(name), Fail::Immediately, None, 0).map_err(engine)
    };
    check()?;
    let (_, identity) = index_write::open_for_write(&plan.repository.root, &ctx)?;
    index_write::same_repository(&plan.repository, &identity)?;
    let _index = hold("index")?;
    let _head = hold("HEAD")?;
    let _config = hold("config")?;
    let (mut repo, identity) = index_write::open_for_write(&plan.repository.root, &ctx)?;
    let head = plan.inspect(&repo, &identity, &ctx)?;
    if plan
        .deletion_head
        .as_ref()
        .is_some_and(|reviewed| reviewed != &head)
    {
        return Err(Error::Stale);
    }
    let _head_tip = match &plan.deletion_head {
        Some(graph::Head::Attached { name, .. }) => {
            let name = std::str::from_utf8(name).map_err(|_| Error::Unsupported)?;
            // Use the same confinement and Windows checks as source refs before
            // letting the filesystem lock API open the reviewed HEAD name.
            let short = name
                .strip_prefix(RefKind::Branch.prefix())
                .ok_or(Error::Unsupported)?;
            reference_name(RefKind::Branch, short, plan.limits)?;
            ordinary_path_absent(git, name, &ctx)?;
            let active = repo.find_reference(name).map_err(engine)?;
            if !matches!(active.target(), gix::refs::TargetRef::Object(_)) {
                return Err(Error::Unsupported);
            }
            Some(
                Marker::acquire_to_hold_resource(
                    git.join(name),
                    Fail::Immediately,
                    Some(git.join("refs")),
                    0,
                )
                .map_err(engine)?,
            )
        }
        _ => None,
    };
    let _log = if plan.reflog.is_some() {
        ordinary_path_absent(git, &reflog_name(&plan.source), &ctx)?;
        Some(hold(&reflog_name(&plan.source))?)
    } else {
        None
    };
    let packed_exists = index_write::exists(&git.join("packed-refs"))?;
    repo.refs.write_reflog = WriteReflog::Disable;
    let delete = RefEdit::delete(
        plan.source.clone(),
        PreviousValue::MustExistAndMatch(Target::Object(plan.target)),
    );
    let transaction = repo
        .refs
        .transaction()
        .prepare([delete], Fail::Immediately, Fail::Immediately)
        .map_err(engine)?;
    // Retain this independent destination lock even after publication. An
    // ordinary Git writer cannot remove the new ref before the source CAS.
    let _destination = plan
        .destination
        .as_ref()
        .map(|name| {
            Marker::acquire_to_hold_resource(
                git.join(full_name(name)),
                Fail::Immediately,
                Some(git.join("refs")),
                0,
            )
            .map_err(engine)
        })
        .transpose()?;
    let packed_lock = git.join("packed-refs.lock");
    let _packed = if packed_exists {
        if !index_write::exists(&packed_lock)? || !index_write::exists(&git.join("packed-refs"))? {
            return Err(Error::Stale);
        }
        None
    } else {
        // The embedded store has nothing packed to delete. Prevent pack-refs
        // from creating a packed copy after validation and before loose removal.
        let lock = hold("packed-refs")?;
        if index_write::exists(&git.join("packed-refs"))? {
            return Err(Error::Stale);
        }
        Some(lock)
    };
    let (fresh, identity) = index_write::open_for_write(&plan.repository.root, &ctx)?;
    let head = plan.inspect(&fresh, &identity, &ctx)?;
    if let Some(reviewed) = &plan.deletion_head {
        if *reviewed != head {
            return Err(Error::Stale);
        }
        plan.check_branch_deletion(&fresh, &head, &ctx)?;
    } else if plan.kind == RefKind::Branch {
        verify_target(&fresh, plan.target, plan.limits, &ctx)?;
    }
    let log = capture_reflog(&plan, &ctx)?;
    if log.as_ref().map(|(token, _)| token) != plan.reflog.as_ref() {
        return Err(Error::Stale);
    }
    let _destination_log = if let (Some(name), Some(_)) = (&plan.destination, &log) {
        Some(
            Marker::acquire_to_hold_resource(
                git.join(reflog_name(name)),
                Fail::Immediately,
                Some(git.clone()),
                0,
            )
            .map_err(engine)?,
        )
    } else {
        None
    };
    let staged_log = match (&plan.destination, log) {
        (Some(name), Some((_, bytes))) => {
            Some(stage_new_file(git, &reflog_name(name), &bytes, &ctx)?)
        }
        _ => None,
    };
    let staged_destination = plan
        .destination
        .as_ref()
        .map(|name| {
            let contents = format!("{}\n", plan.target);
            stage_new_file(git, full_name(name), contents.as_bytes(), &ctx)
        })
        .transpose()?;
    let publication = RefChangePublication {
        kind: plan.kind(),
        source: plan.source().to_owned(),
        destination: plan.destination().map(str::to_owned),
        target: plan.target(),
    };
    check()?;
    // Keep the old ref and reflog until their exact copies are durable. Once
    // published, finish without cancellation; late errors may have effects.
    let uncertain = |error: &dyn std::fmt::Display| {
        Error::Engine(format!(
            "Publicación de {} incierta. Actualiza el repositorio antes de reintentar ({})",
            full_name(&plan.source),
            engine(error)
        ))
    };
    if let Some(destination) = staged_destination {
        destination.publish().map_err(|error| uncertain(&error))?;
    }
    if let Some(log) = staged_log {
        log.publish().map_err(|error| uncertain(&error))?;
    }
    transaction
        .commit(None)
        .map_err(|error| uncertain(&error))?;
    let (fresh, identity) = index_write::open_for_write(&plan.repository.root, &ctx)
        .map_err(|error| uncertain(&error))?;
    index_write::same_repository(&plan.repository, &identity).map_err(|error| uncertain(&error))?;
    let source = full_name(&plan.source);
    if fresh
        .try_find_reference(source)
        .map_err(|error| uncertain(&error))?
        .is_some_and(|reference| reference.name().as_bstr().as_bytes() == source.as_bytes())
    {
        return Err(uncertain(&"La referencia de origen sigue presente"));
    }
    Ok(publication)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    struct Fixture {
        root: PathBuf,
        parent: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .nth(2)
                .unwrap()
                .canonicalize()
                .unwrap();
            let parent = workspace.join("work/test-temp");
            fs::create_dir_all(&parent).unwrap();
            let parent = parent.canonicalize().unwrap();
            assert!(parent.starts_with(&workspace));
            let root = parent.join(format!(
                "git-ref-write-{}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            Self { root, parent }
        }

        fn git(&self) -> PathBuf {
            self.root.join(".git")
        }

        fn prepare(
            &self,
            target: gix::ObjectId,
            kind: RefKind,
            name: &str,
        ) -> Result<RefPlan, Error> {
            prepare_ref(
                &self.root,
                &target.to_string(),
                kind,
                name,
                limits(),
                || false,
            )
        }

        fn change(
            &self,
            target: gix::ObjectId,
            kind: RefKind,
            source: &str,
            destination: Option<&str>,
        ) -> Result<RefChangePlan, Error> {
            prepare_ref_change(
                &self.root,
                kind,
                source,
                &target.to_string(),
                destination,
                limits(),
                || false,
            )
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            assert_eq!(self.root.parent(), Some(self.parent.as_path()));
            assert_eq!(self.root.canonicalize().unwrap(), self.root);
            fn clean(path: &Path) {
                let metadata = fs::symlink_metadata(path).unwrap();
                index_write::ordinary(path, metadata.is_dir()).unwrap();
                if metadata.is_dir() {
                    for entry in fs::read_dir(path).unwrap() {
                        clean(&entry.unwrap().path());
                    }
                    fs::remove_dir(path).unwrap();
                } else {
                    #[cfg(windows)]
                    #[allow(clippy::permissions_set_readonly_false)]
                    if metadata.permissions().readonly() {
                        let mut permissions = metadata.permissions();
                        permissions.set_readonly(false);
                        fs::set_permissions(path, permissions).unwrap();
                    }
                    fs::remove_file(path).unwrap();
                }
            }
            clean(&self.root);
        }
    }

    fn limits() -> Limits {
        Limits {
            max_blob: 2 * 1024 * 1024,
            max_entries: 4096,
            max_path: 4096,
            max_bytes: 4 * 1024 * 1024,
            timeout: Duration::from_secs(5),
        }
    }

    fn commit(repo: &gix::Repository, parents: &[gix::ObjectId], message: &str) -> gix::ObjectId {
        let tree = repo
            .write_object(gix::objs::Tree {
                entries: Vec::new(),
            })
            .unwrap()
            .detach();
        commit_at_tree(repo, tree, parents, message)
    }

    fn commit_at_tree(
        repo: &gix::Repository,
        tree: gix::ObjectId,
        parents: &[gix::ObjectId],
        message: &str,
    ) -> gix::ObjectId {
        let actor = gix::actor::SignatureRef {
            name: b"Autor de prueba".as_bstr(),
            email: b"fixture@invalid".as_bstr(),
            time: "1 +0000",
        };
        repo.write_object(gix::objs::Commit {
            tree,
            parents: parents.iter().copied().collect(),
            author: actor.into(),
            committer: actor.into(),
            message: message.into(),
            encoding: None,
            extra_headers: Default::default(),
        })
        .unwrap()
        .detach()
    }

    #[cfg(windows)]
    mod local_merge {
        use super::*;
        use crate::local_merge::{
            MergeKind, MergePlan, apply_merge, apply_merge_abort, prepare_cherry_pick,
            prepare_merge, prepare_merge_abort,
        };
        use std::io::Write;

        fn scenario(
            fixture: &Fixture,
            divergent: bool,
            right: &[u8],
        ) -> (gix::Repository, gix::ObjectId, gix::ObjectId) {
            use gix::objs::{
                Tree,
                tree::{Entry, EntryKind},
            };
            let repo = gix::init(&fixture.root).unwrap();
            let base = b"uno\r\ndos\r\ntres\r\nfin";
            let left = b"UNO\r\ndos\r\ntres\r\nfin";
            let make_tree = |bytes: &[u8]| {
                let id = repo.write_blob(bytes).unwrap().detach();
                let tree = repo
                    .write_object(Tree {
                        entries: vec![Entry {
                            mode: EntryKind::Blob.into(),
                            filename: "a.txt".into(),
                            oid: id,
                        }],
                    })
                    .unwrap()
                    .detach();
                (id, tree)
            };
            let (original, tree) = make_tree(base);
            let common = commit_at_tree(&repo, tree, &[], "base");
            let (active_blob, active) = if divergent {
                let (blob, tree) = make_tree(left);
                (blob, commit_at_tree(&repo, tree, &[common], "izquierda"))
            } else {
                (original, common)
            };
            let (_, tree) = make_tree(right);
            let incoming = commit_at_tree(&repo, tree, &[common], "derecha");
            let git = fixture.git();
            fs::write(git.join("HEAD"), "ref: refs/heads/activa\n").unwrap();
            fs::write(git.join("refs/heads/activa"), format!("{active}\n")).unwrap();
            fs::write(git.join("refs/heads/entrante"), format!("{incoming}\n")).unwrap();
            fs::OpenOptions::new().append(true).open(git.join("config")).unwrap()
                .write_all(b"\n[core]\nlogAllRefUpdates = true\n[user]\nname = CIDE\nemail = cide@invalid\n")
                .unwrap();
            let local = repo.write_blob(b"preparado local").unwrap().detach();
            let mut state = gix::index::State::new(repo.object_hash());
            for (path, id) in [(b"a.txt".as_slice(), active_blob), (b"local", local)] {
                state.dangerously_push_entry(
                    Default::default(),
                    id,
                    gix::index::entry::Flags::empty(),
                    gix::index::entry::Mode::FILE,
                    path.as_bstr(),
                );
            }
            gix::index::File::from_state(state, git.join("index"))
                .write(Default::default())
                .unwrap();
            fs::write(
                fixture.root.join("a.txt"),
                if divergent { left } else { base },
            )
            .unwrap();
            fs::write(fixture.root.join("local"), b"no preparado local").unwrap();
            fs::write(fixture.root.join("sin-seguimiento"), b"archivo privado").unwrap();
            (repo, active, incoming)
        }

        fn review(fixture: &Fixture, incoming: gix::ObjectId) -> Result<MergePlan, Error> {
            prepare_merge(
                &fixture.root,
                "entrante",
                &incoming.to_string(),
                limits(),
                || false,
            )
        }

        #[test]
        fn merge_divergent_combines_independent_edits_and_preserves_local_state() {
            let fixture = Fixture::new();
            let (_, active, incoming) = scenario(&fixture, true, b"uno\r\ndos\r\ntres\r\nFIN");
            let git = fixture.git();
            let plan = review(&fixture, incoming).unwrap();
            let id = plan.result().unwrap();
            let repo = gix::open(&fixture.root).unwrap();
            assert!(
                repo.try_find_object(id).unwrap().is_none(),
                "la vista previa escribió un objeto"
            );
            let staged = repo.open_index().unwrap();
            let previous = staged.entry_by_path(b"local".as_bstr()).unwrap().clone();
            assert_eq!(plan.kind, MergeKind::Commit);
            assert_eq!(plan.rows[0].path, b"a.txt");
            assert_eq!(
                fs::read(fixture.root.join("a.txt")).unwrap(),
                b"UNO\r\ndos\r\ntres\r\nfin"
            );
            let applied = apply_merge(plan, || false).unwrap();
            assert_eq!(applied.id, id);
            assert!(applied.reflog_warning.is_none());
            let repo = gix::open(&fixture.root).unwrap();
            let commit = repo.find_commit(id).unwrap();
            assert_eq!(
                commit
                    .parent_ids()
                    .map(|id| id.detach())
                    .collect::<Vec<_>>(),
                [active, incoming]
            );
            assert_eq!(commit.tree().unwrap().iter().count(), 1);
            assert_eq!(
                repo.open_index()
                    .unwrap()
                    .entry_by_path(b"local".as_bstr())
                    .unwrap()
                    .id,
                previous.id
            );
            assert_eq!(
                fs::read(fixture.root.join("local")).unwrap(),
                b"no preparado local"
            );
            assert_eq!(
                fs::read(fixture.root.join("sin-seguimiento")).unwrap(),
                b"archivo privado"
            );
            assert_eq!(
                fs::read(fixture.root.join("a.txt")).unwrap(),
                b"UNO\r\ndos\r\ntres\r\nFIN"
            );
            assert_eq!(
                fs::read(git.join("HEAD")).unwrap(),
                b"ref: refs/heads/activa\n"
            );
            assert_eq!(
                fs::read_to_string(git.join("refs/heads/entrante"))
                    .unwrap()
                    .trim(),
                incoming.to_string()
            );
            assert_eq!(repo.head_id().unwrap().detach(), id);
            assert!(
                fs::read_to_string(git.join("logs/HEAD"))
                    .unwrap()
                    .contains(&format!("{active} {id}"))
            );
            assert!(
                fs::read_to_string(git.join("logs/refs/heads/activa"))
                    .unwrap()
                    .contains(&format!("{active} {id}"))
            );
            let review = review(&fixture, incoming).unwrap();
            assert_eq!(review.kind, MergeKind::UpToDate);
            assert!(!review.can_apply());
        }

        #[test]
        fn merge_conflict_cannot_publish_and_leaves_all_local_bytes_intact() {
            let fixture = Fixture::new();
            let (_, active, incoming) = scenario(&fixture, true, b"otro\r\ndos\r\ntres\r\nfin");
            let git = fixture.git();
            let index = fs::read(git.join("index")).unwrap();
            let plan = review(&fixture, incoming).unwrap();
            assert_eq!(plan.kind, MergeKind::Conflicts);
            assert_eq!(plan.rows[0].path, b"a.txt");
            assert!(plan.rows[0].change.is_none());
            assert!(apply_merge(plan, || false).is_err());
            assert_eq!(fs::read(git.join("index")).unwrap(), index);
            assert_eq!(
                fs::read(fixture.root.join("a.txt")).unwrap(),
                b"UNO\r\ndos\r\ntres\r\nfin"
            );
            assert_eq!(
                fs::read_to_string(git.join("refs/heads/activa"))
                    .unwrap()
                    .trim(),
                active.to_string()
            );
            assert!(!git.join(crate::branch_checkout::JOURNAL_NAME).exists());
        }

        #[test]
        fn merge_resolved_result_is_pinned_and_recovers_after_index_publication() {
            use crate::local_merge::{MergeResolution, MergeSide, prepare_resolved_merge};
            let fixture = Fixture::new();
            let (_, active, incoming) = scenario(&fixture, true, b"otro\r\ndos\r\ntres\r\nfin");
            let path = b"a.txt";
            let index = fixture.git().join("index");
            let before = fs::read(&index).unwrap();
            let file = fixture.root.join("a.txt");
            let original = fs::read(&file).unwrap();
            let draft = review(&fixture, incoming).unwrap();
            let conflict = draft.conflict(path).unwrap();
            assert_eq!(
                conflict.bytes(MergeSide::Base),
                Some(b"uno\r\ndos\r\ntres\r\nfin".as_slice())
            );
            assert_eq!(
                conflict.bytes(MergeSide::Current),
                Some(original.as_slice())
            );
            assert_eq!(
                conflict.bytes(MergeSide::Incoming),
                Some(b"otro\r\ndos\r\ntres\r\nfin".as_slice())
            );
            let result = "Solución\r\nfin";
            let prepare = || {
                prepare_resolved_merge(
                    draft.resolution_review().unwrap(),
                    std::collections::BTreeMap::from([(
                        path.to_vec(),
                        MergeResolution::Edit(result.into()),
                    )]),
                    || false,
                )
            };
            fs::write(&file, b"cambio local").unwrap();
            assert!(prepare().is_err());
            assert_eq!(fs::read(&file).unwrap(), b"cambio local");
            fs::write(&file, &original).unwrap();
            let plan = prepare().unwrap();
            assert_eq!(fs::read(&index).unwrap(), before);
            assert_eq!(fs::read(&file).unwrap(), original);
            fs::write(&file, b"edicion despues de revisar").unwrap();
            assert!(apply_merge(plan, || false).is_err());
            assert_eq!(fs::read(&file).unwrap(), b"edicion despues de revisar");
            fs::write(&file, &original).unwrap();
            let plan = prepare().unwrap();
            let id = plan.result().unwrap();
            assert!(apply_merge(plan, || fs::read(&index).unwrap() != before).is_err());
            let resumed = review(&fixture, incoming).unwrap();
            assert!(resumed.recovering());
            assert_eq!(resumed.result(), Some(id));
            assert_eq!(apply_merge(resumed, || false).unwrap().id, id);
            assert_eq!(fs::read(&file).unwrap(), result.as_bytes());
            assert_eq!(
                fs::read(fixture.root.join("local")).unwrap(),
                b"no preparado local"
            );
            let repo = gix::open(&fixture.root).unwrap();
            let commit = repo.find_commit(id).unwrap();
            assert_eq!(
                commit
                    .parent_ids()
                    .map(|id| id.detach())
                    .collect::<Vec<_>>(),
                [active, incoming]
            );
        }

        #[test]
        fn merge_structural_conflict_protects_local_files_and_resumes_both_directions() {
            use crate::local_merge::{MergeResolution, MergeSide, prepare_resolved_merge};
            use gix::objs::{
                Tree,
                tree::{Entry, EntryKind},
            };
            for old_file in [true, false] {
                let fixture = Fixture::new();
                let (repo, _, _) = scenario(&fixture, false, b"original");
                let git = fixture.git();
                let base = commit(&repo, &[], "base");
                let bytes = b"contenido de prueba";
                let blob = repo.write_blob(bytes).unwrap().detach();
                let tree = |name: &str, kind: EntryKind, oid| {
                    repo.write_object(Tree {
                        entries: vec![Entry {
                            filename: name.into(),
                            mode: kind.into(),
                            oid,
                        }],
                    })
                    .unwrap()
                    .detach()
                };
                let plain = tree("ruta", EntryKind::Blob, blob);
                let nested = tree(
                    "ruta",
                    EntryKind::Tree,
                    tree(
                        "sub",
                        EntryKind::Tree,
                        tree("archivo.txt", EntryKind::Blob, blob),
                    ),
                );
                let (old, new) = if old_file {
                    (plain, nested)
                } else {
                    (nested, plain)
                };
                let active = commit_at_tree(&repo, old, &[base], "actual");
                let incoming = commit_at_tree(&repo, new, &[base], "entrante");
                fs::write(git.join("refs/heads/activa"), format!("{active}\n")).unwrap();
                fs::write(git.join("refs/heads/entrante"), format!("{incoming}\n")).unwrap();
                let leaf = if old_file {
                    "ruta"
                } else {
                    "ruta/sub/archivo.txt"
                };
                let from = fixture.root.join(leaf);
                fs::create_dir_all(from.parent().unwrap()).unwrap();
                fs::write(&from, bytes).unwrap();
                let mut index = gix::index::State::new(repo.object_hash());
                index.dangerously_push_entry(
                    Default::default(),
                    blob,
                    gix::index::entry::Flags::empty(),
                    gix::index::entry::Mode::FILE,
                    leaf.as_bytes().as_bstr(),
                );
                gix::index::File::from_state(index, git.join("index"))
                    .write(Default::default())
                    .unwrap();
                let draft = review(&fixture, incoming).unwrap();
                assert_eq!(draft.rows.len(), 1);
                let conflict = draft.conflict(b"ruta").unwrap();
                assert!(conflict.is_file_directory());
                let paths = conflict
                    .version_paths(MergeSide::Incoming)
                    .unwrap()
                    .collect::<Vec<_>>();
                let expected: &[u8] = if old_file {
                    b"ruta/sub/archivo.txt"
                } else {
                    b"ruta"
                };
                assert_eq!(paths, [expected]);
                let prepare = || {
                    prepare_resolved_merge(
                        review(&fixture, incoming)?.resolution_review().unwrap(),
                        std::collections::BTreeMap::from([(
                            b"ruta".to_vec(),
                            MergeResolution::Use(MergeSide::Incoming),
                        )]),
                        || false,
                    )
                };
                fs::write(&from, "edición local").unwrap();
                assert!(prepare().is_err());
                fs::write(&from, bytes).unwrap();
                if !old_file {
                    let untracked = fixture.root.join("ruta/sub/local.txt");
                    fs::write(&untracked, b"sin seguimiento").unwrap();
                    assert!(prepare().is_err());
                    assert_eq!(fs::read(&untracked).unwrap(), b"sin seguimiento");
                    fs::remove_file(untracked).unwrap();
                }
                let plan = prepare().unwrap();
                fs::write(&from, "edición posterior").unwrap();
                assert!(apply_merge(plan, || false).is_err());
                assert_eq!(fs::read(&from).unwrap(), "edición posterior".as_bytes());
                fs::write(&from, bytes).unwrap();
                let index = git.join("index");
                let before = fs::read(&index).unwrap();
                assert!(
                    apply_merge(prepare().unwrap(), || fs::read(&index).unwrap() != before)
                        .is_err()
                );
                let abort = prepare_merge_abort(&fixture.root, limits(), || false).unwrap();
                assert!(abort.restores_index());
                apply_merge_abort(abort, || false).unwrap();
                assert_eq!(fs::read(&from).unwrap(), bytes);
                assert_eq!(fs::read(&index).unwrap(), before);
                let plan = prepare().unwrap();
                let result = plan.result().unwrap();
                let concurrent = fixture.root.join("ruta/sub/concurrente.txt");
                assert!(
                    apply_merge(plan, || {
                        if from.is_file() {
                            return false;
                        }
                        if old_file {
                            return true;
                        }
                        if !concurrent.exists() {
                            fs::write(&concurrent, b"creado durante la publicacion").unwrap();
                        }
                        false
                    })
                    .is_err()
                );
                assert!(!from.exists());
                if !old_file {
                    assert_eq!(
                        fs::read(&concurrent).unwrap(),
                        b"creado durante la publicacion"
                    );
                    assert!(review(&fixture, incoming).is_err());
                    fs::remove_file(concurrent).unwrap();
                }
                let resumed = review(&fixture, incoming).unwrap();
                assert!(resumed.recovering());
                assert_eq!(resumed.result(), Some(result));
                assert_eq!(apply_merge(resumed, || false).unwrap().id, result);
                let to = fixture.root.join(std::str::from_utf8(expected).unwrap());
                assert_eq!(fs::read(&to).unwrap(), bytes);
                assert_eq!(
                    fs::read(fixture.root.join("sin-seguimiento")).unwrap(),
                    b"archivo privado"
                );
                assert!(!git.join(crate::branch_checkout::JOURNAL_NAME).exists());
            }
        }

        #[test]
        fn merge_competing_renames_publish_one_path_and_resume_safely() {
            use crate::local_merge::{MergeResolution, MergeSide, prepare_resolved_merge};
            use gix::objs::{
                Tree,
                tree::{Entry, EntryKind},
            };
            let fixture = Fixture::new();
            let (repo, _, _) = scenario(&fixture, false, b"original");
            let git = fixture.git();
            let bytes = b"contenido del renombre";
            let blob = repo.write_blob(bytes).unwrap().detach();
            let tree = |names: &[&str]| {
                repo.write_object(Tree {
                    entries: names
                        .iter()
                        .map(|name| Entry {
                            filename: (*name).into(),
                            mode: EntryKind::Blob.into(),
                            oid: blob,
                        })
                        .collect(),
                })
                .unwrap()
                .detach()
            };
            let base = commit_at_tree(&repo, tree(&["origen.txt"]), &[], "base");
            let active = commit_at_tree(&repo, tree(&["actual.txt"]), &[base], "actual");
            let incoming = commit_at_tree(&repo, tree(&["entrante.txt"]), &[base], "entrante");
            fs::write(git.join("refs/heads/activa"), format!("{active}\n")).unwrap();
            fs::write(git.join("refs/heads/entrante"), format!("{incoming}\n")).unwrap();
            let from = fixture.root.join("actual.txt");
            let to = fixture.root.join("entrante.txt");
            fs::write(&from, bytes).unwrap();
            let mut index = gix::index::State::new(repo.object_hash());
            index.dangerously_push_entry(
                Default::default(),
                blob,
                gix::index::entry::Flags::empty(),
                gix::index::entry::Mode::FILE,
                b"actual.txt".as_bstr(),
            );
            gix::index::File::from_state(index, git.join("index"))
                .write(Default::default())
                .unwrap();

            let draft = review(&fixture, incoming).unwrap();
            assert_eq!(draft.rows.len(), 1);
            let conflict = draft.conflict(b"origen.txt").unwrap();
            assert!(conflict.is_rename());
            for (side, path) in [
                (MergeSide::Base, b"origen.txt".as_slice()),
                (MergeSide::Current, b"actual.txt".as_slice()),
                (MergeSide::Incoming, b"entrante.txt".as_slice()),
            ] {
                assert_eq!(
                    conflict.version_paths(side).unwrap().collect::<Vec<_>>(),
                    [path]
                );
            }
            let prepare = |side| {
                prepare_resolved_merge(
                    draft.resolution_review().unwrap(),
                    std::collections::BTreeMap::from([(
                        b"origen.txt".to_vec(),
                        MergeResolution::Use(side),
                    )]),
                    || false,
                )
            };
            assert!(prepare(MergeSide::Current).unwrap().rows.is_empty());
            assert!(
                prepare(MergeSide::Base)
                    .unwrap()
                    .rows
                    .iter()
                    .any(|row| row.path == b"origen.txt")
            );
            fs::write(&to, b"archivo local").unwrap();
            assert!(prepare(MergeSide::Incoming).is_err());
            assert_eq!(fs::read(&to).unwrap(), b"archivo local");
            fs::remove_file(&to).unwrap();

            let plan = prepare(MergeSide::Incoming).unwrap();
            fs::write(&from, b"edicion posterior").unwrap();
            assert!(apply_merge(plan, || false).is_err());
            assert_eq!(fs::read(&from).unwrap(), b"edicion posterior");
            fs::write(&from, bytes).unwrap();
            let plan = prepare(MergeSide::Incoming).unwrap();
            let result = plan.result().unwrap();
            assert!(apply_merge(plan, || !from.exists()).is_err());
            fs::write(&to, b"creado durante la recuperacion").unwrap();
            assert!(review(&fixture, incoming).is_err());
            assert_eq!(fs::read(&to).unwrap(), b"creado durante la recuperacion");
            fs::remove_file(&to).unwrap();
            let resumed = review(&fixture, incoming).unwrap();
            assert!(resumed.recovering());
            assert_eq!(apply_merge(resumed, || false).unwrap().id, result);
            assert_eq!(fs::read(&to).unwrap(), bytes);
            assert!(!from.exists());
            assert!(!fixture.root.join("origen.txt").exists());
            assert_eq!(
                fs::read(fixture.root.join("sin-seguimiento")).unwrap(),
                b"archivo privado"
            );
            let commit = repo.find_commit(result).unwrap();
            let result_tree = commit.tree().unwrap();
            let paths = result_tree
                .iter()
                .map(|entry| entry.unwrap().filename().to_vec())
                .collect::<Vec<_>>();
            assert_eq!(paths, [b"entrante.txt".to_vec()]);

            // A copied blob leaves two plausible destinations, so the
            // resolver must not offer an arbitrary selection.
            let ambiguous = commit_at_tree(
                &repo,
                tree(&["copia-a.txt", "copia-b.txt"]),
                &[base],
                "copia",
            );
            fs::write(git.join("refs/heads/activa"), format!("{active}\n")).unwrap();
            fs::write(git.join("refs/heads/entrante"), format!("{ambiguous}\n")).unwrap();
            let draft = review(&fixture, ambiguous).unwrap();
            assert!(draft.resolution_review().is_none());

            let deleted = commit_at_tree(&repo, tree(&[]), &[base], "borrado");
            fs::write(git.join("refs/heads/entrante"), format!("{deleted}\n")).unwrap();
            let draft = review(&fixture, deleted).unwrap();
            let conflict = draft.conflict(b"origen.txt").unwrap();
            assert!(conflict.is_rename());
            assert_eq!(
                conflict.version_paths(MergeSide::Incoming).unwrap().count(),
                0
            );
        }

        #[test]
        fn merge_fast_forward_rejects_local_edits_and_changed_ref() {
            let fixture = Fixture::new();
            let (_, active, incoming) = scenario(&fixture, false, b"entrante");
            let git = fixture.git();
            let file = fixture.root.join("a.txt");
            let reviewed = review(&fixture, incoming).unwrap();
            assert_eq!(reviewed.kind, MergeKind::FastForward);
            fs::write(&file, "edición entre revisión y confirmación").unwrap();
            assert!(apply_merge(reviewed, || false).is_err());
            assert!(review(&fixture, incoming).is_err());
            assert_eq!(
                fs::read(&file).unwrap(),
                "edición entre revisión y confirmación".as_bytes()
            );
            fs::write(&file, b"uno\r\ndos\r\ntres\r\nfin").unwrap();
            let reviewed = review(&fixture, incoming).unwrap();
            fs::write(git.join("refs/heads/entrante"), format!("{active}\n")).unwrap();
            assert!(matches!(apply_merge(reviewed, || false), Err(Error::Stale)));
            assert!(!git.join(crate::branch_checkout::JOURNAL_NAME).exists());
        }

        #[test]
        fn merge_resumes_after_index_publication_without_replacing_a_later_edit() {
            let fixture = Fixture::new();
            let (_, active, incoming) = scenario(&fixture, true, b"uno\r\ndos\r\ntres\r\nFIN");
            let git = fixture.git();
            let index = git.join("index");
            let before = fs::read(&index).unwrap();
            let plan = review(&fixture, incoming).unwrap();
            let result = plan.result().unwrap();
            assert!(apply_merge(plan, || fs::read(&index).unwrap() != before).is_err());
            let pending = git.join(crate::branch_checkout::JOURNAL_NAME);
            assert!(pending.exists());
            assert_eq!(
                fs::read_to_string(git.join("refs/heads/activa"))
                    .unwrap()
                    .trim(),
                active.to_string()
            );
            let file = fixture.root.join("a.txt");
            let merged = fs::read(&file).unwrap();
            fs::write(&file, b"edicion local posterior").unwrap();
            assert!(review(&fixture, incoming).is_err());
            assert_eq!(fs::read(&file).unwrap(), b"edicion local posterior");
            fs::write(&file, merged).unwrap();
            let resumed = review(&fixture, incoming).unwrap();
            assert!(resumed.recovering());
            assert_eq!(resumed.result(), Some(result));
            assert_eq!(apply_merge(resumed, || false).unwrap().id, result);
            assert!(!pending.exists());
        }

        #[test]
        fn merge_abort_reverses_publications_and_resumes_without_overwriting_local_changes() {
            let fixture = Fixture::new();
            let (_, active, incoming) = scenario(&fixture, true, b"uno\r\ndos\r\ntres\r\nFIN");
            let git = fixture.git();
            let index = git.join("index");
            let branch = git.join("refs/heads/activa");
            let pending = git.join(crate::branch_checkout::JOURNAL_NAME);
            let file = fixture.root.join("a.txt");
            let original = fs::read(&file).unwrap();
            let before = fs::read(&index).unwrap();
            let abort = || prepare_merge_abort(&fixture.root, limits(), || false);
            assert!(abort().is_err());
            let plan = review(&fixture, incoming).unwrap();
            let result = plan.result().unwrap();
            let published_branch = format!("{result}\n");
            assert!(
                apply_merge(plan, || {
                    fs::read_to_string(&branch).unwrap() == published_branch
                })
                .is_err()
            );
            let published = fs::read(&index).unwrap();
            let merged = fs::read(&file).unwrap();
            assert_ne!(published, before);
            let plan = abort().unwrap();
            assert!(plan.restores_index() && plan.restores_branch());
            fs::write(&file, b"edicion local posterior").unwrap();
            assert!(apply_merge_abort(plan, || false).is_err());
            assert_eq!(fs::read(&file).unwrap(), b"edicion local posterior");
            fs::write(&file, &merged).unwrap();
            let moved = fixture.root.join("aparte");
            fs::rename(&file, &moved).unwrap();
            fs::write(&file, &merged).unwrap();
            assert!(abort().is_err());
            fs::remove_file(&file).unwrap();
            fs::rename(&moved, &file).unwrap();
            let plan = abort().unwrap();
            assert!(
                apply_merge_abort(plan, || fs::read(&file).ok().as_ref() == Some(&original))
                    .is_err()
            );
            assert!(pending.exists());
            assert_eq!(fs::read(&index).unwrap(), published);
            assert!(review(&fixture, incoming).is_err());
            assert_eq!(
                apply_merge_abort(abort().unwrap(), || false).unwrap().id,
                active
            );
            assert_eq!(fs::read(&file).unwrap(), original);
            assert_eq!(fs::read(&index).unwrap(), before);
            assert_eq!(
                fs::read_to_string(branch).unwrap().trim(),
                active.to_string()
            );
            assert_eq!(
                fs::read(fixture.root.join("local")).unwrap(),
                b"no preparado local"
            );
            assert_eq!(
                fs::read(fixture.root.join("sin-seguimiento")).unwrap(),
                b"archivo privado"
            );
            assert!(!pending.exists());
            assert!(abort().is_err());
        }

        #[test]
        fn cherry_pick_applies_only_the_selected_delta_and_preserves_author_and_message() {
            use gix::objs::{
                Tree,
                tree::{Entry, EntryKind},
            };
            let fixture = Fixture::new();
            let (repo, active, prior) =
                scenario(&fixture, true, b"uno\r\nDOS AJENO\r\ntres\r\nfin");
            let blob = repo
                .write_blob(b"uno\r\nDOS AJENO\r\ntres\r\nFIN")
                .unwrap()
                .detach();
            let tree = repo
                .write_object(Tree {
                    entries: vec![Entry {
                        mode: EntryKind::Blob.into(),
                        filename: "a.txt".into(),
                        oid: blob,
                    }],
                })
                .unwrap()
                .detach();
            let selected = commit_at_tree(&repo, tree, &[prior], "Mensaje original\n\nDetalle");
            let pick =
                || prepare_cherry_pick(&fixture.root, &selected.to_string(), limits(), || false);
            let index = fixture.git().join("index");
            let before = fs::read(&index).unwrap();
            let staged = repo
                .open_index()
                .unwrap()
                .entry_by_path(b"local".as_bstr())
                .unwrap()
                .id;
            let plan = pick().unwrap();
            assert!(plan.is_cherry_pick());
            assert_eq!(plan.rows.len(), 1);
            assert_eq!(fs::read(&index).unwrap(), before);
            let id = apply_merge(plan, || false).unwrap().id;
            assert_ne!(id, selected);
            assert_eq!(
                fs::read(fixture.root.join("a.txt")).unwrap(),
                b"UNO\r\ndos\r\ntres\r\nFIN"
            );
            let applied = repo.find_commit(id).unwrap();
            let original = repo.find_commit(selected).unwrap();
            assert_eq!(
                applied
                    .parent_ids()
                    .map(|parent| parent.detach())
                    .collect::<Vec<_>>(),
                [active]
            );
            assert_eq!(
                applied.decode().unwrap().author,
                original.decode().unwrap().author
            );
            assert_eq!(
                applied.decode().unwrap().message,
                original.decode().unwrap().message
            );
            assert_eq!(
                fs::read(fixture.root.join("local")).unwrap(),
                b"no preparado local"
            );
            assert_eq!(
                repo.open_index()
                    .unwrap()
                    .entry_by_path(b"local".as_bstr())
                    .unwrap()
                    .id,
                staged
            );
            assert!(
                applied
                    .tree()
                    .unwrap()
                    .lookup_entry_by_path("local")
                    .unwrap()
                    .is_none()
            );
            assert_eq!(
                fs::read(fixture.root.join("sin-seguimiento")).unwrap(),
                b"archivo privado"
            );
            assert_eq!(pick().unwrap().kind, MergeKind::UpToDate);
            let tree = repo
                .write_object(Tree {
                    entries: vec![Entry {
                        mode: EntryKind::Blob.into(),
                        filename: "raiz.txt".into(),
                        oid: blob,
                    }],
                })
                .unwrap()
                .detach();
            let root = commit_at_tree(&repo, tree, &[], "Commit raíz");
            let root_plan =
                prepare_cherry_pick(&fixture.root, &root.to_string(), limits(), || false).unwrap();
            assert_eq!(root_plan.rows[0].path, b"raiz.txt");
            apply_merge(root_plan, || false).unwrap();
            assert_eq!(
                fs::read(fixture.root.join("raiz.txt")).unwrap(),
                b"uno\r\nDOS AJENO\r\ntres\r\nFIN"
            );
        }

        #[test]
        fn cherry_pick_mainline_changes_the_delta_and_is_pinned_through_recovery() {
            use crate::local_merge::prepare_cherry_pick_mainline;
            use gix::objs::{
                Tree,
                tree::{Entry, EntryKind},
            };
            let fixture = Fixture::new();
            let (repo, active, other) =
                scenario(&fixture, true, b"uno\r\nDOS AJENO\r\ntres\r\nfin");
            let blob = repo
                .write_blob(b"uno\r\nDOS AJENO\r\ntres\r\nFIN")
                .unwrap()
                .detach();
            let tree = repo
                .write_object(Tree {
                    entries: vec![Entry {
                        mode: EntryKind::Blob.into(),
                        filename: "a.txt".into(),
                        oid: blob,
                    }],
                })
                .unwrap()
                .detach();
            let merge = commit_at_tree(&repo, tree, &[active, other], "Fusión elegida");
            let pick =
                || prepare_cherry_pick(&fixture.root, &merge.to_string(), limits(), || false);
            let git = fixture.git();
            let pending = git.join(crate::branch_checkout::JOURNAL_NAME);
            let index = git.join("index");
            let branch = git.join("refs/heads/activa");
            let original_index = fs::read(&index).unwrap();
            let file = fixture.root.join("a.txt");
            let original = fs::read(&file).unwrap();
            let selector = pick().unwrap();
            assert_eq!(selector.kind, MergeKind::ChooseMainline);
            assert_eq!(selector.mainline_choices(), [active, other]);
            assert!(!selector.can_apply());
            assert!(selector.mainline_review(2).is_none());
            assert_eq!(fs::read(&index).unwrap(), original_index);
            assert!(!pending.exists());
            let first =
                prepare_cherry_pick_mainline(selector.mainline_review(0).unwrap(), || false)
                    .unwrap();
            let second = selector.mainline_review(1).unwrap();
            assert_eq!(first.mainline(), Some((1, active)));
            let published_ref = format!("{}\n", first.result().unwrap());
            assert!(
                apply_merge(first, || fs::read_to_string(&branch).unwrap()
                    == published_ref)
                .is_err()
            );
            assert_eq!(fs::read(&file).unwrap(), b"uno\r\nDOS AJENO\r\ntres\r\nFIN");
            assert!(prepare_cherry_pick_mainline(second, || false).is_err());
            let resumed = pick().unwrap();
            assert!(resumed.recovering());
            assert!(resumed.mainline_choices().is_empty());
            assert_eq!(resumed.mainline(), Some((1, active)));
            let abort = || prepare_merge_abort(&fixture.root, limits(), || false).unwrap();
            let plan = abort();
            assert_eq!(plan.mainline(), Some((1, active)));
            assert!(plan.restores_index() && plan.restores_branch());
            assert!(
                apply_merge_abort(plan, || fs::read(&file).ok().as_ref() == Some(&original))
                    .is_err()
            );
            assert!(pick().is_err());
            assert_eq!(abort().mainline(), Some((1, active)));
            assert_eq!(apply_merge_abort(abort(), || false).unwrap().id, active);
            assert_eq!(fs::read(&index).unwrap(), original_index);
            assert_eq!(fs::read(&file).unwrap(), original);
            assert!(!pending.exists());
            let second =
                prepare_cherry_pick_mainline(pick().unwrap().mainline_review(1).unwrap(), || false)
                    .unwrap();
            assert_eq!(second.mainline(), Some((2, other)));
            let id = apply_merge(second, || false).unwrap().id;
            assert_eq!(fs::read(&file).unwrap(), b"UNO\r\ndos\r\ntres\r\nFIN");
            let commit = repo.find_commit(id).unwrap();
            assert_eq!(
                commit
                    .parent_ids()
                    .map(|parent| parent.detach())
                    .collect::<Vec<_>>(),
                [active]
            );
            assert_eq!(
                fs::read(fixture.root.join("local")).unwrap(),
                b"no preparado local"
            );
        }

        #[test]
        fn cherry_pick_conflicts_are_read_only_and_interrupted_publication_can_be_aborted() {
            use crate::local_merge::{MergeResolution, MergeSide, prepare_resolved_merge};
            let fixture = Fixture::new();
            let (_, active, selected) = scenario(&fixture, true, b"ENTRANTE\r\ndos\r\ntres\r\nfin");
            let git = fixture.git();
            let index = git.join("index");
            let pending = git.join(crate::branch_checkout::JOURNAL_NAME);
            let original = fs::read(&index).unwrap();
            let file = fixture.root.join("a.txt");
            let bytes = fs::read(&file).unwrap();
            let pick =
                || prepare_cherry_pick(&fixture.root, &selected.to_string(), limits(), || false);
            let draft = pick().unwrap();
            assert_eq!(draft.kind, MergeKind::Conflicts);
            assert!(!draft.can_apply());
            assert!(!pending.exists());
            assert_eq!(fs::read(&index).unwrap(), original);
            assert_eq!(fs::read(&file).unwrap(), bytes);
            let resolve = |draft: MergePlan| {
                prepare_resolved_merge(
                    draft.resolution_review().unwrap(),
                    std::collections::BTreeMap::from([(
                        b"a.txt".to_vec(),
                        MergeResolution::Use(MergeSide::Incoming),
                    )]),
                    || false,
                )
                .unwrap()
            };
            let plan = resolve(draft);
            fs::write(&file, b"edicion local").unwrap();
            assert!(apply_merge(plan, || false).is_err());
            assert!(!pending.exists());
            assert_eq!(fs::read(&file).unwrap(), b"edicion local");
            fs::write(&file, &bytes).unwrap();
            assert!(
                apply_merge(resolve(pick().unwrap()), || fs::read(&index).unwrap()
                    != original)
                .is_err()
            );
            assert!(review(&fixture, selected).is_err());
            assert!(pick().unwrap().recovering());
            let abort = prepare_merge_abort(&fixture.root, limits(), || false).unwrap();
            assert!(abort.is_cherry_pick() && abort.restores_index());
            assert_eq!(apply_merge_abort(abort, || false).unwrap().id, active);
            assert_eq!(fs::read(&index).unwrap(), original);
            assert_eq!(fs::read(&file).unwrap(), bytes);
            assert!(!pending.exists());
        }

        #[test]
        fn revert_merge_uses_the_selected_parent_and_preserves_later_and_staged_changes() {
            use crate::local_merge::{
                CommitOperation, prepare_commit_mainline, prepare_commit_operation,
            };
            use gix::objs::{
                Tree,
                tree::{Entry, EntryKind},
            };
            let fixture = Fixture::new();
            let (repo, first, second) = scenario(&fixture, true, b"uno\r\nDOS\r\ntres\r\nfin");
            let version = |bytes: &[u8]| {
                let blob = repo.write_blob(bytes).unwrap().detach();
                let tree = repo
                    .write_object(Tree {
                        entries: vec![Entry {
                            mode: EntryKind::Blob.into(),
                            filename: "a.txt".into(),
                            oid: blob,
                        }],
                    })
                    .unwrap()
                    .detach();
                (blob, tree)
            };
            let (_, tree) = version(b"UNO\r\nDOS\r\ntres\r\nfin");
            let merge = commit_at_tree(&repo, tree, &[first, second], "Fusión original");
            let bytes = b"UNO\r\nDOS\r\ntres\r\nFIN";
            let (blob, tree) = version(bytes);
            let head = commit_at_tree(&repo, tree, &[merge], "Edición posterior");
            let git = fixture.git();
            fs::write(git.join("refs/heads/activa"), format!("{head}\n")).unwrap();
            fs::write(fixture.root.join("a.txt"), bytes).unwrap();
            let staged = repo
                .open_index()
                .unwrap()
                .entry_by_path(b"local".as_bstr())
                .unwrap()
                .id;
            let mut state = gix::index::State::new(repo.object_hash());
            for (path, id) in [(b"a.txt".as_slice(), blob), (b"local", staged)] {
                state.dangerously_push_entry(
                    Default::default(),
                    id,
                    gix::index::entry::Flags::empty(),
                    gix::index::entry::Mode::FILE,
                    path.as_bstr(),
                );
            }
            let index = git.join("index");
            gix::index::File::from_state(state, &index)
                .write(Default::default())
                .unwrap();
            let before = fs::read(&index).unwrap();
            let choose = || {
                prepare_commit_operation(
                    &fixture.root,
                    &merge.to_string(),
                    CommitOperation::Revert,
                    limits(),
                    || false,
                )
                .unwrap()
            };
            let selector = choose();
            assert_eq!(selector.mainline_choices(), [first, second]);
            let plan =
                prepare_commit_mainline(selector.mainline_review(0).unwrap(), || false).unwrap();
            assert_eq!(plan.operation(), Some(CommitOperation::Revert));
            assert_eq!(plan.mainline(), Some((1, first)));
            assert_eq!(fs::read(&index).unwrap(), before);
            assert_eq!(fs::read(fixture.root.join("a.txt")).unwrap(), bytes);
            let id = apply_merge(plan, || false).unwrap().id;
            assert_eq!(
                fs::read(fixture.root.join("a.txt")).unwrap(),
                b"UNO\r\ndos\r\ntres\r\nFIN"
            );
            let commit = repo.find_commit(id).unwrap();
            assert_eq!(
                commit
                    .parent_ids()
                    .map(|parent| parent.detach())
                    .collect::<Vec<_>>(),
                [head]
            );
            assert!(
                commit
                    .decode()
                    .unwrap()
                    .message
                    .as_bytes()
                    .windows(40)
                    .any(|s| s == merge.to_string().as_bytes())
            );
            assert!(
                commit
                    .tree()
                    .unwrap()
                    .lookup_entry_by_path("local")
                    .unwrap()
                    .is_none()
            );
            assert_eq!(
                repo.open_index()
                    .unwrap()
                    .entry_by_path(b"local".as_bstr())
                    .unwrap()
                    .id,
                staged
            );
            assert_eq!(
                fs::read(fixture.root.join("local")).unwrap(),
                b"no preparado local"
            );
            assert_eq!(
                fs::read(fixture.root.join("sin-seguimiento")).unwrap(),
                b"archivo privado"
            );
            let repeated =
                prepare_commit_mainline(choose().mainline_review(0).unwrap(), || false).unwrap();
            assert_eq!(repeated.kind, MergeKind::UpToDate);
        }

        #[test]
        fn revert_root_removes_its_paths_and_preserves_unrelated_files() {
            use crate::local_merge::{CommitOperation, prepare_commit_operation};
            let fixture = Fixture::new();
            let (_, root, _) = scenario(&fixture, false, b"otra rama");
            let plan = prepare_commit_operation(
                &fixture.root,
                &root.to_string(),
                CommitOperation::Revert,
                limits(),
                || false,
            )
            .unwrap();
            assert_eq!(plan.rows.len(), 1);
            assert_eq!(plan.rows[0].change, Some(crate::Change::Deleted));
            assert!(fixture.root.join("a.txt").exists());
            apply_merge(plan, || false).unwrap();
            assert!(!fixture.root.join("a.txt").exists());
            assert_eq!(
                fs::read(fixture.root.join("local")).unwrap(),
                b"no preparado local"
            );
        }

        #[test]
        fn revert_conflict_can_abort_an_interrupted_publication_and_preserve_local_edits() {
            use crate::local_merge::{
                CommitOperation, MergeResolution, MergeSide, prepare_commit_operation,
                prepare_resolved_merge,
            };
            let fixture = Fixture::new();
            let (_, active, selected) = scenario(&fixture, true, b"ENTRANTE\r\ndos\r\ntres\r\nfin");
            let git = fixture.git();
            let index = git.join("index");
            let pending = git.join(crate::branch_checkout::JOURNAL_NAME);
            let original = fs::read(&index).unwrap();
            let file = fixture.root.join("a.txt");
            let bytes = fs::read(&file).unwrap();
            let revert = || {
                prepare_commit_operation(
                    &fixture.root,
                    &selected.to_string(),
                    CommitOperation::Revert,
                    limits(),
                    || false,
                )
            };
            let draft = revert().unwrap();
            assert_eq!(draft.kind, MergeKind::Conflicts);
            assert_eq!(
                draft.conflict(b"a.txt").unwrap().bytes(MergeSide::Base),
                Some(b"ENTRANTE\r\ndos\r\ntres\r\nfin".as_slice())
            );
            assert_eq!(
                draft.conflict(b"a.txt").unwrap().bytes(MergeSide::Incoming),
                Some(b"uno\r\ndos\r\ntres\r\nfin".as_slice())
            );
            assert!(!pending.exists());
            let plan = prepare_resolved_merge(
                draft.resolution_review().unwrap(),
                std::collections::BTreeMap::from([(
                    b"a.txt".to_vec(),
                    MergeResolution::Use(MergeSide::Incoming),
                )]),
                || false,
            )
            .unwrap();
            assert!(apply_merge(plan, || fs::read(&index).unwrap() != original).is_err());
            assert!(pending.exists());
            assert!(
                prepare_cherry_pick(&fixture.root, &selected.to_string(), limits(), || false)
                    .is_err()
            );
            assert!(revert().unwrap().recovering());
            let abort = || prepare_merge_abort(&fixture.root, limits(), || false);
            let review = abort().unwrap();
            assert_eq!(review.operation(), Some(CommitOperation::Revert));
            let published = fs::read(&file).unwrap();
            fs::write(&file, b"edicion local posterior").unwrap();
            assert!(apply_merge_abort(review, || false).is_err());
            assert_eq!(fs::read(&file).unwrap(), b"edicion local posterior");
            fs::write(&file, published).unwrap();
            assert_eq!(
                apply_merge_abort(abort().unwrap(), || false).unwrap().id,
                active
            );
            assert_eq!(fs::read(&index).unwrap(), original);
            assert_eq!(fs::read(&file).unwrap(), bytes);
            assert!(!pending.exists());
        }
        #[test]
        fn reset_soft_keeps_exact_index_and_current_files() {
            use crate::local_merge::{ResetMode, prepare_reset};
            let fixture = Fixture::new();
            let (repo, active, _) = scenario(&fixture, true, b"otra rama");
            let target = repo
                .find_commit(active)
                .unwrap()
                .parent_ids()
                .next()
                .unwrap()
                .detach();
            let git = fixture.git();
            let branch = git.join("refs/heads/activa");
            let index = git.join("index");
            let before = fs::read(&index).unwrap();
            let file = fixture.root.join("a.txt");
            let reset = |id: gix::ObjectId| {
                prepare_reset(
                    &fixture.root,
                    &id.to_string(),
                    ResetMode::Soft,
                    limits(),
                    || false,
                )
                .unwrap()
            };
            let plan = reset(target);
            assert!(plan.can_apply());
            assert_eq!(plan.rows.len(), 1);
            assert_eq!(fs::read(&index).unwrap(), before);
            assert_eq!(
                fs::read_to_string(&branch).unwrap().trim(),
                active.to_string()
            );
            fs::write(&file, b"edicion posterior a la vista previa").unwrap();
            let publication = apply_merge(plan, || false).unwrap();
            assert_eq!(publication.id, target);
            assert!(publication.index_backup.is_none());
            assert_eq!(fs::read(&index).unwrap(), before);
            assert_eq!(
                fs::read(&file).unwrap(),
                b"edicion posterior a la vista previa"
            );
            assert_eq!(
                fs::read_to_string(&branch).unwrap().trim(),
                target.to_string()
            );
            fs::remove_file(&index).unwrap();
            assert_eq!(apply_merge(reset(active), || false).unwrap().id, active);
            assert!(!index.exists());
        }

        #[test]
        fn reset_mixed_retains_staged_bytes_and_recovers_partial_publication() {
            use crate::local_merge::{ResetMode, prepare_reset};
            let fixture = Fixture::new();
            let (repo, active, _) = scenario(&fixture, true, b"otra rama");
            let target = repo
                .find_commit(active)
                .unwrap()
                .parent_ids()
                .next()
                .unwrap()
                .detach();
            let git = fixture.git();
            let index = git.join("index");
            let before = fs::read(&index).unwrap();
            let pending = git.join(crate::branch_checkout::JOURNAL_NAME);
            let file = fixture.root.join("a.txt");
            fs::write(&file, b"edicion local").unwrap();
            let reset = |id: gix::ObjectId, mode| {
                prepare_reset(&fixture.root, &id.to_string(), mode, limits(), || false)
            };
            let plan = reset(target, ResetMode::Mixed).unwrap();
            assert_eq!(plan.rows.len(), 2);
            assert_eq!(fs::read(&index).unwrap(), before);
            assert!(!pending.exists());
            assert!(apply_merge(plan, || fs::read(&index).unwrap() != before).is_err());
            assert!(pending.exists());
            assert!(reset(target, ResetMode::Soft).is_err());
            assert!(reset(target, ResetMode::Mixed).unwrap().recovering());
            let abort = prepare_merge_abort(&fixture.root, limits(), || false).unwrap();
            assert!(abort.restores_index());
            assert!(abort.rows.is_empty());
            assert_eq!(apply_merge_abort(abort, || false).unwrap().id, active);
            assert_eq!(fs::read(&index).unwrap(), before);
            assert_eq!(fs::read(&file).unwrap(), b"edicion local");
            assert!(!pending.exists());
            let same_head = reset(active, ResetMode::Mixed).unwrap();
            assert_eq!(same_head.rows.len(), 1);
            let publication = apply_merge(same_head, || false).unwrap();
            assert_eq!(publication.id, active);
            assert_eq!(fs::read(publication.index_backup.unwrap()).unwrap(), before);
            assert!(
                repo.open_index()
                    .unwrap()
                    .entry_by_path(b"local".as_bstr())
                    .is_none()
            );
            assert_eq!(
                fs::read(fixture.root.join("local")).unwrap(),
                b"no preparado local"
            );
            assert_eq!(
                fs::read(fixture.root.join("sin-seguimiento")).unwrap(),
                b"archivo privado"
            );
            assert_eq!(fs::read(&file).unwrap(), b"edicion local");
            assert!(!reset(active, ResetMode::Mixed).unwrap().can_apply());
        }
        #[test]
        fn reset_hard_retains_worktree_and_staged_only_bytes_in_the_backup() {
            use crate::local_merge::{ResetMode, prepare_reset};
            let fixture = Fixture::new();
            let (repo, active, _) = scenario(&fixture, true, b"otra rama");
            let git = fixture.git();
            let index = git.join("index");
            let file = fixture.root.join("a.txt");
            fs::write(&file, b"edicion local").unwrap();
            let before = fs::read(&index).unwrap();
            let staged = repo
                .open_index()
                .unwrap()
                .entry_by_path(b"local".as_bstr())
                .unwrap()
                .id;
            let plan = prepare_reset(
                &fixture.root,
                &active.to_string(),
                ResetMode::Hard,
                limits(),
                || false,
            )
            .unwrap();
            assert!(
                plan.can_apply()
                    && plan.hard_changes_file(b"a.txt")
                    && plan.hard_changes_file(b"local")
            );
            assert_eq!(fs::read(&file).unwrap(), b"edicion local");
            assert!(!git.join(crate::branch_checkout::JOURNAL_NAME).exists());
            let backup = apply_merge(plan, || false).unwrap().hard_backup.unwrap();
            assert_eq!(fs::read(&file).unwrap(), b"UNO\r\ndos\r\ntres\r\nfin");
            assert!(!fixture.root.join("local").exists());
            assert_eq!(
                fs::read(fixture.root.join("sin-seguimiento")).unwrap(),
                b"archivo privado"
            );
            assert_eq!(fs::read(backup.join("index")).unwrap(), before);
            assert_eq!(
                fs::read(backup.join("archivos/0")).unwrap(),
                b"edicion local"
            );
            assert_eq!(
                fs::read(backup.join("archivos/1")).unwrap(),
                b"no preparado local"
            );
            assert_eq!(
                fs::read(backup.join("preparados").join(staged.to_string())).unwrap(),
                b"preparado local"
            );
            let manifest = fs::read_to_string(backup.join("manifest.tsv")).unwrap();
            assert!(manifest.contains(&format!("preparado\t{staged}\tlocal")));
            assert!(manifest.contains("\ta.txt\n"));
        }

        #[test]
        fn reset_hard_abort_restores_disk_index_and_branch_and_blocks_later_edits() {
            use crate::local_merge::{ResetMode, prepare_reset};
            let fixture = Fixture::new();
            let (repo, active, _) = scenario(&fixture, true, b"otra rama");
            let target = repo
                .find_commit(active)
                .unwrap()
                .parent_ids()
                .next()
                .unwrap()
                .detach();
            let git = fixture.git();
            let index = git.join("index");
            let branch = git.join("refs/heads/activa");
            let pending = git.join(crate::branch_checkout::JOURNAL_NAME);
            let file = fixture.root.join("a.txt");
            let before = fs::read(&index).unwrap();
            fs::write(&file, b"edicion revisada").unwrap();
            let hard = || {
                prepare_reset(
                    &fixture.root,
                    &target.to_string(),
                    ResetMode::Hard,
                    limits(),
                    || false,
                )
            };
            let stale = hard().unwrap();
            fs::write(&file, b"edicion antes de confirmar").unwrap();
            assert!(apply_merge(stale, || false).is_err());
            assert!(!pending.exists());
            assert_eq!(fs::read(&file).unwrap(), b"edicion antes de confirmar");
            let target_ref = format!("{target}\n");
            assert!(
                apply_merge(hard().unwrap(), || fs::read_to_string(&branch).unwrap()
                    == target_ref)
                .is_err()
            );
            assert!(pending.exists());
            assert!(hard().is_err());
            let abort = || prepare_merge_abort(&fixture.root, limits(), || false);
            let review = abort().unwrap();
            assert!(review.restores_branch() && review.restores_index());
            assert_eq!(review.reset_mode(), Some(ResetMode::Hard));
            let output = fs::read(&file).unwrap();
            fs::write(&file, b"edicion posterior").unwrap();
            assert!(apply_merge_abort(review, || false).is_err());
            assert_eq!(fs::read(&file).unwrap(), b"edicion posterior");
            fs::write(&file, output).unwrap();
            let result = apply_merge_abort(abort().unwrap(), || false).unwrap();
            assert_eq!(result.id, active);
            assert_eq!(fs::read(&index).unwrap(), before);
            assert_eq!(fs::read(&file).unwrap(), b"edicion antes de confirmar");
            assert_eq!(
                fs::read(fixture.root.join("local")).unwrap(),
                b"no preparado local"
            );
            assert!(result.hard_backup.unwrap().join("manifest.tsv").is_file());
            assert!(!pending.exists());
        }

        #[test]
        fn reset_hard_does_not_replace_an_untracked_collision() {
            use crate::local_merge::{ResetMode, prepare_reset};
            let fixture = Fixture::new();
            let (repo, active, _) = scenario(&fixture, true, b"otra rama");
            let git = fixture.git();
            let index = git.join("index");
            gix::index::File::from_state(gix::index::State::new(repo.object_hash()), &index)
                .write(Default::default())
                .unwrap();
            let file = fixture.root.join("a.txt");
            fs::write(&file, b"contenido no rastreado").unwrap();
            assert!(
                prepare_reset(
                    &fixture.root,
                    &active.to_string(),
                    ResetMode::Hard,
                    limits(),
                    || false
                )
                .is_err()
            );
            assert_eq!(fs::read(file).unwrap(), b"contenido no rastreado");
            assert!(!git.join(crate::branch_checkout::JOURNAL_NAME).exists());
        }
        #[test]
        fn rebase_replays_in_order_without_writing_preview_and_can_abort_publication() {
            use crate::local_merge::{RebaseStepState, prepare_rebase};
            use gix::objs::{
                Tree,
                tree::{Entry, EntryKind},
            };
            let fixture = Fixture::new();
            let (repo, first, onto) = scenario(&fixture, true, b"uno\r\ndos\r\ntres\r\nFIN");
            let original = b"UNO\r\ndos\r\nTRES\r\nfin";
            let blob = repo.write_blob(original).unwrap().detach();
            let tree = repo
                .write_object(Tree {
                    entries: vec![Entry {
                        mode: EntryKind::Blob.into(),
                        filename: "a.txt".into(),
                        oid: blob,
                    }],
                })
                .unwrap()
                .detach();
            let active = commit_at_tree(&repo, tree, &[first], "Segundo cambio");
            let git = fixture.git();
            let branch = git.join("refs/heads/activa");
            let index = git.join("index");
            let pending = git.join(crate::branch_checkout::JOURNAL_NAME);
            let file = fixture.root.join("a.txt");
            fs::write(&branch, format!("{active}\n")).unwrap();
            fs::write(&file, original).unwrap();
            let mut staged = repo.open_index().unwrap();
            let (entry, _) = staged
                .entries_mut_with_paths()
                .find(|(_, path)| path.as_bytes() == b"a.txt")
                .unwrap();
            entry.id = blob;
            staged.write(Default::default()).unwrap();
            let before = fs::read(&index).unwrap();
            let rebase = || {
                prepare_rebase(
                    &fixture.root,
                    "entrante",
                    &onto.to_string(),
                    limits(),
                    || false,
                )
            };
            let plan = rebase().unwrap();
            assert!(plan.is_rebase() && plan.can_apply());
            assert_eq!(
                plan.rebase_steps()
                    .iter()
                    .map(|step| step.original)
                    .collect::<Vec<_>>(),
                [first, active]
            );
            let RebaseStepState::Replayed(rewritten_first) = plan.rebase_steps()[0].state else {
                panic!("first commit not replayed")
            };
            let result = plan.result().unwrap();
            assert!(
                repo.find_commit(rewritten_first).is_err() && repo.find_commit(result).is_err()
            );
            assert!(!pending.exists());
            assert_eq!(fs::read(&index).unwrap(), before);
            assert_eq!(fs::read(&file).unwrap(), original);
            let target_ref = format!("{result}\n");
            assert!(
                apply_merge(plan, || fs::read_to_string(&branch).unwrap() == target_ref).is_err()
            );
            assert!(pending.exists());
            assert!(rebase().is_err());
            let abort = prepare_merge_abort(&fixture.root, limits(), || false).unwrap();
            assert!(abort.is_rebase() && abort.restores_index() && abort.restores_branch());
            assert_eq!(apply_merge_abort(abort, || false).unwrap().id, active);
            assert_eq!(fs::read(&file).unwrap(), original);
            assert_eq!(fs::read(&index).unwrap(), before);
            assert!(!pending.exists());

            let plan = rebase().unwrap();
            let RebaseStepState::Replayed(first_result) = plan.rebase_steps()[0].state else {
                panic!("first commit not replayed")
            };
            let id = apply_merge(plan, || false).unwrap().id;
            assert_eq!(fs::read(&file).unwrap(), b"UNO\r\ndos\r\nTRES\r\nFIN");
            let replayed_first = repo.find_commit(first_result).unwrap();
            assert_eq!(replayed_first.parent_ids().next().unwrap().detach(), onto);
            let replayed_last = repo.find_commit(id).unwrap();
            assert_eq!(
                replayed_last
                    .parent_ids()
                    .map(|p| p.detach())
                    .collect::<Vec<_>>(),
                [first_result]
            );
            assert_eq!(
                replayed_last.decode().unwrap().author,
                repo.find_commit(active).unwrap().decode().unwrap().author
            );
            assert_eq!(
                replayed_last.decode().unwrap().message.as_bytes(),
                b"Segundo cambio"
            );
            assert_eq!(
                fs::read(fixture.root.join("local")).unwrap(),
                b"no preparado local"
            );
            assert_eq!(
                fs::read(fixture.root.join("sin-seguimiento")).unwrap(),
                b"archivo privado"
            );
        }

        #[test]
        fn interactive_rebase_reorders_combines_and_omits_before_confirming() {
            use crate::local_merge::{
                RebaseAction, RebaseStepState, prepare_interactive_rebase, prepare_rebase_todo,
            };
            use gix::objs::{
                Tree,
                tree::{Entry, EntryKind},
            };
            let fixture = Fixture::new();
            let (repo, first, onto) = scenario(&fixture, true, b"uno\r\ndos\r\ntres\r\nFIN");
            let make = |parent, bytes: &[u8], message| {
                let blob = repo.write_blob(bytes).unwrap().detach();
                let tree = repo
                    .write_object(Tree {
                        entries: vec![Entry {
                            mode: EntryKind::Blob.into(),
                            filename: "a.txt".into(),
                            oid: blob,
                        }],
                    })
                    .unwrap()
                    .detach();
                (blob, commit_at_tree(&repo, tree, &[parent], message))
            };
            let (_, second) = make(first, b"UNO\r\ndos\r\nTRES\r\nfin", "Segundo cambio");
            let original = b"UNO\r\nDOS\r\nTRES\r\nfin";
            let (blob, third) = make(second, original, "Omitir este");
            let git = fixture.git();
            let active = git.join("refs/heads/activa");
            let file = fixture.root.join("a.txt");
            fs::write(&active, format!("{third}\n")).unwrap();
            fs::write(&file, original).unwrap();
            let mut index = repo.open_index().unwrap();
            index
                .entries_mut_with_paths()
                .find(|(_, path)| path.as_bytes() == b"a.txt")
                .unwrap()
                .0
                .id = blob;
            index.write(Default::default()).unwrap();
            let before = fs::read(git.join("index")).unwrap();
            let open = || {
                prepare_interactive_rebase(
                    &fixture.root,
                    "entrante",
                    &onto.to_string(),
                    limits(),
                    || false,
                )
                .unwrap()
            };
            let mut draft = open();
            assert!(!draft.can_apply());
            let todo = draft.rebase_todo_mut().unwrap();
            assert_eq!(todo[1].subject, "Segundo cambio");
            todo.swap(0, 1);
            todo[0].action = RebaseAction::Squash;
            assert!(prepare_rebase_todo(draft.rebase_todo_review().unwrap(), || false).is_err());
            let todo = draft.rebase_todo_mut().unwrap();
            todo[0].action = RebaseAction::Pick;
            todo[1].action = RebaseAction::Squash;
            todo[2].action = RebaseAction::Drop;
            let preview = || prepare_rebase_todo(draft.rebase_todo_review().unwrap(), || false);
            let plan = preview().unwrap();
            let tip = plan.result().unwrap();
            assert!(matches!(
                plan.rebase_steps(),
                [
                    crate::local_merge::RebaseStep { original: a, state: RebaseStepState::Replayed(x) },
                    crate::local_merge::RebaseStep { original: b, state: RebaseStepState::Combined(y) },
                    crate::local_merge::RebaseStep { original: c, state: RebaseStepState::Omitted },
                ] if (*a, *b, *c, *x, *y) == (second, first, third, tip, tip)
            ));
            assert!(repo.find_commit(tip).is_err());
            assert_eq!(fs::read(&file).unwrap(), original);
            assert_eq!(fs::read(git.join("index")).unwrap(), before);
            fs::write(&file, b"Cambio local").unwrap();
            assert!(apply_merge(plan, || false).is_err());
            assert_eq!(fs::read(&file).unwrap(), b"Cambio local");
            fs::write(&file, original).unwrap();
            let plan = preview().unwrap();
            let target = format!("{}\n", plan.result().unwrap());
            assert!(apply_merge(plan, || fs::read_to_string(&active).unwrap() == target).is_err());
            let abort = prepare_merge_abort(&fixture.root, limits(), || false).unwrap();
            assert!(abort.is_rebase());
            assert_eq!(apply_merge_abort(abort, || false).unwrap().id, third);
            assert_eq!(fs::read(&file).unwrap(), original);
            assert_eq!(fs::read(git.join("index")).unwrap(), before);
            assert!(matches!(preview(), Err(Error::Stale)));
            let mut draft = open();
            let todo = draft.rebase_todo_mut().unwrap();
            todo.swap(0, 1);
            todo[1].action = RebaseAction::Squash;
            todo[2].action = RebaseAction::Drop;
            let plan = prepare_rebase_todo(draft.rebase_todo_review().unwrap(), || false).unwrap();
            let applied = apply_merge(plan, || false).unwrap().id;
            let commit = repo.find_commit(applied).unwrap();
            assert_eq!(
                commit
                    .parent_ids()
                    .map(|id| id.detach())
                    .collect::<Vec<_>>(),
                [onto]
            );
            assert_eq!(
                commit.decode().unwrap().message.as_bytes(),
                b"Segundo cambio\n\nizquierda"
            );
            assert_eq!(fs::read(&file).unwrap(), b"UNO\r\ndos\r\nTRES\r\nFIN");
            assert_eq!(
                fs::read(fixture.root.join("local")).unwrap(),
                b"no preparado local"
            );
        }

        #[test]
        fn rebase_conflict_review_does_not_publish_and_local_overlap_blocks_a_clean_plan() {
            use crate::local_merge::{RebaseStepState, prepare_rebase};
            let fixture = Fixture::new();
            let (_, active, onto) = scenario(&fixture, true, b"ENTRANTE\r\ndos\r\ntres\r\nfin");
            let git = fixture.git();
            let index = git.join("index");
            let before = fs::read(&index).unwrap();
            let plan = prepare_rebase(
                &fixture.root,
                "entrante",
                &onto.to_string(),
                limits(),
                || false,
            )
            .unwrap();
            assert_eq!(plan.kind, MergeKind::Conflicts);
            assert_eq!(plan.rows[0].path, b"a.txt");
            assert!(matches!(
                plan.rebase_steps()[0].state,
                RebaseStepState::Conflict
            ));
            assert!(!plan.can_apply());
            assert!(apply_merge(plan, || false).is_err());
            assert_eq!(fs::read(&index).unwrap(), before);
            assert!(!git.join(crate::branch_checkout::JOURNAL_NAME).exists());
            assert_eq!(
                fs::read_to_string(git.join("refs/heads/activa"))
                    .unwrap()
                    .trim(),
                active.to_string()
            );

            let clean_fixture = Fixture::new();
            let (_, _, onto) = scenario(&clean_fixture, true, b"uno\r\ndos\r\ntres\r\nFIN");
            let plan = prepare_rebase(
                &clean_fixture.root,
                "entrante",
                &onto.to_string(),
                limits(),
                || false,
            )
            .unwrap();
            let file = clean_fixture.root.join("a.txt");
            fs::write(&file, b"edicion local posterior").unwrap();
            assert!(apply_merge(plan, || false).is_err());
            assert_eq!(fs::read(&file).unwrap(), b"edicion local posterior");
            assert!(
                !clean_fixture
                    .git()
                    .join(crate::branch_checkout::JOURNAL_NAME)
                    .exists()
            );
        }
        #[test]
        fn rebase_keeps_distinct_edits_for_successive_conflicts_on_one_path() {
            use crate::local_merge::{
                MergeResolution, MergeSide, RebaseStepState, prepare_rebase, prepare_resolved_merge,
            };
            use gix::objs::{
                Tree,
                tree::{Entry, EntryKind},
            };
            let fixture = Fixture::new();
            let (repo, first, onto) = scenario(&fixture, true, b"ENTRANTE\r\ndos\r\ntres\r\nfin");
            let original = b"SEGUNDO\r\ndos\r\ntres\r\nfin";
            let blob = repo.write_blob(original).unwrap().detach();
            let tree = repo
                .write_object(Tree {
                    entries: vec![Entry {
                        mode: EntryKind::Blob.into(),
                        filename: "a.txt".into(),
                        oid: blob,
                    }],
                })
                .unwrap()
                .detach();
            let active = commit_at_tree(&repo, tree, &[first], "Segundo cambio");
            let git = fixture.git();
            let branch = git.join("refs/heads/activa");
            let incoming = git.join("refs/heads/entrante");
            let index = git.join("index");
            let file = fixture.root.join("a.txt");
            fs::write(&branch, format!("{active}\n")).unwrap();
            fs::write(&file, original).unwrap();
            let mut staged = repo.open_index().unwrap();
            let (entry, _) = staged
                .entries_mut_with_paths()
                .find(|(_, path)| path.as_bytes() == b"a.txt")
                .unwrap();
            entry.id = blob;
            staged.write(Default::default()).unwrap();
            let before = fs::read(&index).unwrap();
            let initial = prepare_rebase(
                &fixture.root,
                "entrante",
                &onto.to_string(),
                limits(),
                || false,
            )
            .unwrap();
            assert_eq!(initial.rebase_conflict_commit(), Some(first));
            let choices = |text: &str| {
                std::collections::BTreeMap::from([(
                    b"a.txt".to_vec(),
                    MergeResolution::Edit(text.into()),
                )])
            };
            let first_text = "PRIMERA RESOLUCION\r\ndos\r\ntres\r\nfin";
            let second_text = "SEGUNDA RESOLUCION\r\ndos\r\ntres\r\nfin";
            let paused = prepare_resolved_merge(
                initial.resolution_review().unwrap(),
                choices(first_text),
                || false,
            )
            .unwrap();
            assert_eq!(paused.rebase_conflict_commit(), Some(active));
            assert_eq!(
                paused.conflict(b"a.txt").unwrap().bytes(MergeSide::Current),
                Some(first_text.as_bytes())
            );
            assert!(!paused.can_apply());
            let RebaseStepState::Replayed(rewritten_first) = paused.rebase_steps()[0].state else {
                panic!("first resolution was not replayed");
            };
            fs::write(&incoming, format!("{first}\n")).unwrap();
            assert!(
                prepare_resolved_merge(
                    paused.resolution_review().unwrap(),
                    choices(second_text),
                    || false
                )
                .is_err()
            );
            fs::write(&incoming, format!("{onto}\n")).unwrap();
            let final_review = prepare_resolved_merge(
                paused.resolution_review().unwrap(),
                choices(second_text),
                || false,
            )
            .unwrap();
            assert!(final_review.can_apply());
            assert!(
                matches!(final_review.rebase_steps()[0].state, RebaseStepState::Replayed(id) if id == rewritten_first)
            );
            assert!(repo.find_commit(rewritten_first).is_err());
            assert_eq!(fs::read(&file).unwrap(), original);
            assert_eq!(fs::read(&index).unwrap(), before);
            assert_eq!(
                fs::read_to_string(&branch).unwrap().trim(),
                active.to_string()
            );
            assert!(!git.join(crate::branch_checkout::JOURNAL_NAME).exists());
            let result = apply_merge(final_review, || false).unwrap();
            assert_eq!(fs::read(&file).unwrap(), second_text.as_bytes());
            let parent = repo
                .find_commit(result.id)
                .unwrap()
                .parent_ids()
                .next()
                .unwrap()
                .detach();
            assert_eq!(parent, rewritten_first);
            let first_commit = repo.find_commit(parent).unwrap();
            let first_tree = first_commit.tree().unwrap();
            let entry = first_tree.lookup_entry_by_path("a.txt").unwrap().unwrap();
            assert_eq!(
                repo.find_blob(entry.object_id()).unwrap().data,
                first_text.as_bytes()
            );
        }
    }

    #[cfg(windows)]
    mod branch_switch {
        use super::*;
        use crate::branch_switch::{BranchSwitchPlan, apply_branch_switch, prepare_branch_switch};
        use std::io::Write;

        fn setup(fixture: &Fixture) -> (gix::Repository, gix::ObjectId, gix::ObjectId) {
            let repo = gix::init(&fixture.root).unwrap();
            let source = commit(&repo, &[], "antes");
            let target = commit(&repo, &[source], "mismo árbol");
            let git = fixture.git();
            fs::write(git.join("HEAD"), b"ref: refs/heads/activa\n").unwrap();
            fs::write(git.join("refs/heads/activa"), format!("{source}\n")).unwrap();
            fs::write(git.join("refs/heads/destino"), format!("{target}\n")).unwrap();
            fs::OpenOptions::new()
                .append(true)
                .open(git.join("config"))
                .unwrap()
                .write_all(b"\n[core]\nlogAllRefUpdates = true\n[user]\nname = Persona\nemail = persona@invalid\n")
                .unwrap();
            (repo, source, target)
        }

        fn review(fixture: &Fixture, target: gix::ObjectId) -> Result<BranchSwitchPlan, Error> {
            prepare_branch_switch(
                &fixture.root,
                "destino",
                &target.to_string(),
                limits(),
                || false,
            )
        }

        #[test]
        fn preserves_staged_unstaged_and_untracked_bytes_and_updates_head_reflog() {
            let fixture = Fixture::new();
            let (repo, source, target) = setup(&fixture);
            let git = fixture.git();
            let path = fixture.root.join("trabajo.p");
            let blob = repo.write_blob(b"bytes preparados\n").unwrap().detach();
            let mut index = gix::index::State::new(repo.object_hash());
            index.dangerously_push_entry(
                Default::default(),
                blob,
                gix::index::entry::Flags::empty(),
                gix::index::entry::Mode::FILE,
                b"trabajo.p".as_bstr(),
            );
            gix::index::File::from_state(index, git.join("index"))
                .write(Default::default())
                .unwrap();
            let index = fs::read(git.join("index")).unwrap();
            fs::write(&path, b"bytes no preparados\r\n").unwrap();
            fs::write(fixture.root.join("nuevo"), b"archivo sin seguimiento").unwrap();
            let log = git.join("logs/HEAD");
            fs::create_dir(log.parent().unwrap()).unwrap();
            let history = format!(
                "{} {source} Autor <correo@invalid> 1 +0000\tcreada\n",
                source.kind().null()
            );
            fs::write(&log, &history).unwrap();
            let config = fs::read(git.join("config")).unwrap();
            let plan = review(&fixture, target).unwrap();
            assert_eq!(plan.source(), "activa");
            assert_eq!(plan.destination(), "destino");
            assert_eq!(plan.source_target(), source);
            assert_eq!(plan.target(), target);
            let published = apply_branch_switch(plan, || false).unwrap();
            assert_eq!(published.target, target);
            assert!(published.reflog_warning.is_none());
            assert_eq!(
                fs::read(git.join("HEAD")).unwrap(),
                b"ref: refs/heads/destino\n"
            );
            assert_eq!(
                fs::read_to_string(git.join("refs/heads/activa")).unwrap(),
                format!("{source}\n")
            );
            assert_eq!(
                fs::read_to_string(git.join("refs/heads/destino")).unwrap(),
                format!("{target}\n")
            );
            assert_eq!(fs::read(git.join("index")).unwrap(), index);
            assert_eq!(fs::read(&path).unwrap(), b"bytes no preparados\r\n");
            assert_eq!(
                fs::read(fixture.root.join("nuevo")).unwrap(),
                b"archivo sin seguimiento"
            );
            assert_eq!(fs::read(git.join("config")).unwrap(), config);
            let history_after = fs::read_to_string(&log).unwrap();
            assert!(history_after.starts_with(&history));
            assert!(
                history_after.contains(&format!("{source} {target} Persona <persona@invalid> "))
            );
            assert!(history_after.ends_with("\tcheckout: moving from activa to destino\n"));
        }

        #[test]
        fn rejects_switch_without_index_symbolic_destinations_and_other_worktrees() {
            let fixture = Fixture::new();
            let (repo, source, target) = setup(&fixture);
            let git = fixture.git();
            let tree = repo
                .write_object(gix::objs::Tree {
                    entries: vec![gix::objs::tree::Entry {
                        mode: gix::objs::tree::EntryKind::Blob.into(),
                        filename: "otro".into(),
                        oid: repo
                            .write_blob("cambia el árbol".as_bytes())
                            .unwrap()
                            .detach(),
                    }],
                })
                .unwrap()
                .detach();
            let different = commit_at_tree(&repo, tree, &[source], "cambia");
            let dest = git.join("refs/heads/destino");
            fs::write(&dest, format!("{different}\n")).unwrap();
            assert!(matches!(
                review(&fixture, different),
                Err(Error::Unsupported)
            ));
            fs::write(&dest, b"ref: refs/heads/activa\n").unwrap();
            assert!(matches!(review(&fixture, source), Err(Error::Unsupported)));
            fs::write(&dest, format!("{target}\n")).unwrap();
            fs::create_dir(git.join("worktrees")).unwrap();
            assert!(review(&fixture, target).is_err());
            assert_eq!(
                fs::read(git.join("HEAD")).unwrap(),
                b"ref: refs/heads/activa\n"
            );
            assert!(!git.join("logs/HEAD").exists());
        }

        #[test]
        fn switches_different_trees_and_resumes_after_worktree_and_index_publication() {
            use gix::objs::{
                Tree,
                tree::{Entry, EntryKind},
            };
            let fixture = Fixture::new();
            let (repo, _, _) = setup(&fixture);
            let git = fixture.git();
            let blob = |bytes: &[u8]| repo.write_blob(bytes).unwrap().detach();
            let old = blob(b"antes\r\n");
            let new = blob(b"despues\r\n");
            let removed = blob(b"se retira");
            let added = blob(b"ruta nueva\xff");
            let staged = blob(b"cambio preparado independiente");
            let rules = b"* -text\nnueva/c.txt text eol=lf\nconvertir.txt text eol=crlf\n";
            let attributes = blob(rules);
            let entry = |name: &str, kind: EntryKind, oid| Entry {
                filename: name.into(),
                mode: kind.into(),
                oid,
            };
            let write_tree = |entries| repo.write_object(Tree { entries }).unwrap().detach();
            let old_tree = write_tree(vec![
                entry(".gitattributes", EntryKind::Blob, attributes),
                entry("a.txt", EntryKind::Blob, old),
                entry("b.txt", EntryKind::Blob, removed),
            ]);
            let sub = write_tree(vec![entry("c.txt", EntryKind::Blob, added)]);
            let new_tree = write_tree(vec![
                entry(".gitattributes", EntryKind::Blob, attributes),
                entry("a.txt", EntryKind::Blob, new),
                entry("nueva", EntryKind::Tree, sub),
            ]);
            let source = commit_at_tree(&repo, old_tree, &[], "antes");
            let target = commit_at_tree(&repo, new_tree, &[source], "después");
            fs::write(git.join("refs/heads/activa"), format!("{source}\n")).unwrap();
            fs::write(git.join("refs/heads/destino"), format!("{target}\n")).unwrap();
            let mut state = gix::index::State::new(repo.object_hash());
            for (name, id) in [
                (b".gitattributes".as_slice(), attributes),
                (b"a.txt".as_slice(), old),
                (b"b.txt", removed),
                (b"local", staged),
            ] {
                state.dangerously_push_entry(
                    Default::default(),
                    id,
                    gix::index::entry::Flags::empty(),
                    gix::index::entry::Mode::FILE,
                    name.as_bstr(),
                );
            }
            let index = git.join("index");
            gix::index::File::from_state(state, index.clone())
                .write(Default::default())
                .unwrap();
            let original_index = fs::read(&index).unwrap();
            let a = fixture.root.join("a.txt");
            let b = fixture.root.join("b.txt");
            let c = fixture.root.join("nueva/c.txt");
            let local = fixture.root.join("local");
            let journal = git.join(crate::branch_switch::JOURNAL_NAME);
            fs::write(fixture.root.join(".gitattributes"), b"* text eol=crlf\n").unwrap();
            fs::write(&a, b"cambio local").unwrap();
            fs::write(&b, b"se retira").unwrap();
            fs::write(&local, b"cambio no preparado independiente").unwrap();
            assert!(matches!(review(&fixture, target), Err(Error::Filter)));
            fs::write(fixture.root.join(".gitattributes"), rules).unwrap();
            assert!(review(&fixture, target).is_err());
            assert!(!journal.exists());
            fs::write(&a, b"antes\r\n").unwrap();
            let conversion_tree = write_tree(vec![
                entry(".gitattributes", EntryKind::Blob, attributes),
                entry("a.txt", EntryKind::Blob, old),
                entry("b.txt", EntryKind::Blob, removed),
                entry("convertir.txt", EntryKind::Blob, blob(b"contenido\n")),
            ]);
            let conversion = commit_at_tree(&repo, conversion_tree, &[source], "conversión");
            let destination = git.join("refs/heads/destino");
            fs::write(&destination, format!("{conversion}\n")).unwrap();
            assert!(matches!(review(&fixture, conversion), Err(Error::Filter)));
            fs::write(destination, format!("{target}\n")).unwrap();
            let plan = review(&fixture, target).unwrap();
            assert_eq!(
                plan.files().iter().map(|f| f.change()).collect::<Vec<_>>(),
                vec![
                    crate::Change::Modified,
                    crate::Change::Deleted,
                    crate::Change::Added
                ]
            );
            assert_eq!(fs::read(&a).unwrap(), b"antes\r\n");
            assert!(
                apply_branch_switch(plan, || fs::read(&a).is_ok_and(|b| b == b"despues\r\n"))
                    .is_err()
            );
            assert!(journal.is_file());
            assert_eq!(fs::read(&index).unwrap(), original_index);
            assert!(b.is_file() && !c.exists());
            fs::write(&b, b"edicion posterior al fallo").unwrap();
            assert!(review(&fixture, target).is_err());
            assert_eq!(fs::read(&b).unwrap(), b"edicion posterior al fallo");
            fs::write(&b, b"se retira").unwrap();
            let plan = review(&fixture, target).unwrap();
            assert!(plan.recovering());
            assert!(
                apply_branch_switch(plan, || fs::read(&index).is_ok_and(|b| b != original_index))
                    .is_err()
            );
            assert_eq!(
                fs::read(git.join("HEAD")).unwrap(),
                b"ref: refs/heads/activa\n"
            );
            assert!(journal.is_file());
            let saved_journal = fs::read(&journal).unwrap();
            let plan = review(&fixture, target).unwrap();
            apply_branch_switch(plan, || false).unwrap();
            assert!(!journal.exists());
            assert_eq!(fs::read(&a).unwrap(), b"despues\r\n");
            assert!(!b.exists());
            assert_eq!(fs::read(c).unwrap(), b"ruta nueva\xff");
            assert_eq!(
                fs::read(local).unwrap(),
                b"cambio no preparado independiente"
            );
            let switched = repo.open_index().unwrap();
            assert_eq!(
                switched.entry_by_path(b"local".as_bstr()).unwrap().id,
                staged
            );
            assert_eq!(switched.entry_by_path(b"a.txt".as_bstr()).unwrap().id, new);
            assert!(switched.entry_by_path(b"b.txt".as_bstr()).is_none());
            assert_eq!(
                fs::read(git.join("HEAD")).unwrap(),
                b"ref: refs/heads/destino\n"
            );
            fs::write(&journal, saved_journal).unwrap();
            apply_branch_switch(review(&fixture, target).unwrap(), || false).unwrap();
            assert!(!journal.exists());
        }

        #[test]
        fn stale_review_and_cancellation_cannot_publish_head() {
            let fixture = Fixture::new();
            let (repo, source, target) = setup(&fixture);
            let git = fixture.git();
            let dest = git.join("refs/heads/destino");
            let plan = review(&fixture, target).unwrap();
            fs::write(&dest, format!("{source}\n")).unwrap();
            assert!(matches!(
                apply_branch_switch(plan, || false),
                Err(Error::Stale)
            ));
            fs::write(&dest, format!("{target}\n")).unwrap();
            let plan = review(&fixture, target).unwrap();
            gix::index::File::from_state(
                gix::index::State::new(repo.object_hash()),
                git.join("index"),
            )
            .write(Default::default())
            .unwrap();
            assert!(matches!(
                apply_branch_switch(plan, || false),
                Err(Error::Stale)
            ));
            let plan = review(&fixture, target).unwrap();
            assert!(matches!(
                apply_branch_switch(plan, || git.join("HEAD.lock").exists()),
                Err(Error::Cancelled)
            ));
            assert_eq!(
                fs::read(git.join("HEAD")).unwrap(),
                b"ref: refs/heads/activa\n"
            );
            assert!(!git.join("HEAD.lock").exists());
            assert!(!git.join("logs/HEAD").exists());
            assert!(!git.join("refs/heads/destino.lock").exists());
        }

        #[test]
        fn an_intervening_reflog_writer_is_not_overwritten() {
            let fixture = Fixture::new();
            let (_, _, target) = setup(&fixture);
            let git = fixture.git();
            let plan = review(&fixture, target).unwrap();
            let log = git.join("logs/HEAD");
            let published = apply_branch_switch(plan, || {
                if git.join("logs/HEAD.lock").exists() {
                    fs::write(&log, b"escritura concurrente\n").unwrap();
                }
                false
            })
            .unwrap();
            assert!(published.reflog_warning.is_some());
            assert_eq!(fs::read(&log).unwrap(), b"escritura concurrente\n");
            assert_eq!(
                fs::read(git.join("HEAD")).unwrap(),
                b"ref: refs/heads/destino\n"
            );
            assert!(!git.join("HEAD.lock").exists());
            assert!(!git.join("logs/HEAD.lock").exists());
        }
    }

    #[test]
    fn reviewed_branch_and_lightweight_tag_pin_the_commit_and_preserve_editor_sources() {
        let fixture = Fixture::new();
        let repo = gix::init(&fixture.root).unwrap();
        let original = commit(&repo, &[], "original");
        let later = commit(&repo, &[original], "posterior");
        let git = fixture.git();
        fs::write(git.join("HEAD"), format!("{original}\n")).unwrap();
        fs::write(git.join("index"), b"bytes del indice").unwrap();
        fs::write(fixture.root.join("archivo"), b"bytes del usuario").unwrap();
        let branch = fixture
            .prepare(original, RefKind::Branch, "pruebas/Árbol")
            .unwrap();
        let tag = fixture.prepare(original, RefKind::Tag, "v1").unwrap();
        assert_eq!(branch.name(), "pruebas/Árbol");
        assert_eq!(branch.full_name(), "refs/heads/pruebas/Árbol");
        assert_eq!(branch.target(), original);
        assert!(!git.join("refs/heads/pruebas").exists());
        assert!(!git.join(tag.full_name()).exists());
        // A later HEAD does not change the reviewed commit.
        let later_head = format!("{later}\n");
        fs::write(git.join("HEAD"), &later_head).unwrap();
        for plan in [branch, tag] {
            let path = git.join(plan.full_name());
            let publication = RefPublication {
                kind: plan.kind(),
                name: plan.name().to_owned(),
                target: original,
            };
            assert_eq!(apply_ref(plan, || false).unwrap(), publication);
            assert_eq!(fs::read_to_string(path).unwrap(), format!("{original}\n"));
        }
        assert_eq!(fs::read(git.join("HEAD")).unwrap(), later_head.as_bytes());
        assert_eq!(fs::read(git.join("index")).unwrap(), b"bytes del indice");
        assert_eq!(
            fs::read(fixture.root.join("archivo")).unwrap(),
            b"bytes del usuario"
        );
        assert!(!git.join("logs/HEAD").exists());
        assert!(!git.join("logs/refs/heads/pruebas/Árbol").exists());
    }

    #[test]
    fn a_same_target_writer_between_preflight_and_lock_does_not_count_as_creation() {
        let fixture = Fixture::new();
        let repo = gix::init(&fixture.root).unwrap();
        let target = commit(&repo, &[], "original");
        let plan = fixture.prepare(target, RefKind::Branch, "ocupada").unwrap();
        let path = fixture.git().join(plan.full_name());
        let contents = format!("{target}\n");
        let mut checks = 0;
        assert!(matches!(
            apply_ref(plan, || {
                checks += 1;
                if checks == 2 {
                    fs::write(&path, &contents).unwrap();
                }
                false
            }),
            Err(Error::Stale)
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), contents);
        assert!(!fixture.git().join("refs/heads/ocupada.lock").exists());
        assert!(fixture.prepare(target, RefKind::Branch, "ocupada").is_err());
    }

    #[test]
    fn packed_names_prevent_exact_prefix_and_case_aliases() {
        let fixture = Fixture::new();
        let repo = gix::init(&fixture.root).unwrap();
        let target = commit(&repo, &[], "original");
        let packed = fixture.git().join("packed-refs");
        let contents = format!(
            "# pack-refs with: sorted\n{target} refs/heads/Equipo/base\n{target} refs/tags/V1\n"
        );
        fs::write(&packed, &contents).unwrap();
        for name in [
            "Equipo",
            "Equipo/base",
            "equipo/nueva",
            "Equipo/base/anidada",
        ] {
            assert!(
                fixture.prepare(target, RefKind::Branch, name).is_err(),
                "{name}"
            );
        }
        assert!(fixture.prepare(target, RefKind::Tag, "v1").is_err());
        let next = fixture
            .prepare(target, RefKind::Branch, "Equipo/nueva")
            .unwrap();
        assert!(apply_ref(next, || false).is_ok());
        assert_eq!(fs::read_to_string(packed).unwrap(), contents);
    }

    #[test]
    fn cancellation_releases_the_prepared_lock_without_publication() {
        let fixture = Fixture::new();
        let repo = gix::init(&fixture.root).unwrap();
        let target = commit(&repo, &[], "original");
        let plan = fixture.prepare(target, RefKind::Tag, "cancelada").unwrap();
        let git = fixture.git();
        let lock = git.join("refs/tags/cancelada.lock");
        assert!(matches!(
            apply_ref(plan, || lock.exists()),
            Err(Error::Cancelled)
        ));
        assert!(!lock.exists());
        assert!(!git.join("refs/tags/cancelada").exists());
        assert!(matches!(
            prepare_ref(
                &fixture.root,
                &target.to_string(),
                RefKind::Tag,
                "cancelada",
                limits(),
                || true
            ),
            Err(Error::Cancelled)
        ));
    }

    #[test]
    fn invalid_windows_names_and_noncommit_destinations_are_rejected() {
        for name in [
            "",
            "HEAD",
            "-opcion",
            "@",
            "a//b",
            "../subir",
            "a.lock/b",
            "a.LOCK",
            "nul.txt",
            "CONOUT$",
            "aux/base",
            "x/COM¹.log",
            "x/lpt²",
            "a:b",
            "a\\b",
            "a./b",
        ] {
            assert!(
                reference_name(RefKind::Branch, name, limits()).is_err(),
                "{name:?}"
            );
        }
        assert!(matches!(
            reference_name(RefKind::Branch, &"x".repeat(REF_NAME_BYTES + 1), limits()),
            Err(Error::Limited)
        ));
        let fixture = Fixture::new();
        let repo = gix::init(&fixture.root).unwrap();
        let blob = repo.write_blob(b"contenido").unwrap().detach();
        assert!(fixture.prepare(blob, RefKind::Tag, "solo-commits").is_err());
        assert!(
            prepare_ref(
                &fixture.root,
                "HEAD",
                RefKind::Tag,
                "simbolica",
                limits(),
                || false
            )
            .is_err()
        );
        let target = commit(&repo, &[], "original");
        let directory = fixture.git().join("refs/tags/reservada");
        fs::create_dir(&directory).unwrap();
        assert!(fixture.prepare(target, RefKind::Tag, "reservada").is_err());
        assert!(directory.is_dir());
    }

    #[cfg(windows)]
    #[test]
    fn renaming_a_branch_preserves_reflog_and_never_touches_editor_sources() {
        let fixture = Fixture::new();
        let repo = gix::init(&fixture.root).unwrap();
        let target = commit(&repo, &[], "original");
        let git = fixture.git();
        let branch = git.join("refs/heads/vieja");
        let log = git.join("logs/refs/heads/vieja");
        fs::create_dir_all(log.parent().unwrap()).unwrap();
        fs::write(&branch, format!("{target}\n")).unwrap();
        let history = format!(
            "{} {target} Autor <correo@invalid> 1 +0000\tcreada\n",
            target.kind().null()
        );
        fs::write(&log, &history).unwrap();
        let head = fs::read(git.join("HEAD")).unwrap();
        fs::write(git.join("index"), b"index fixtures").unwrap();
        fs::write(fixture.root.join("archivo"), b"contenido usuario").unwrap();
        let plan = fixture
            .change(target, RefKind::Branch, "vieja", Some("tema/nueva"))
            .unwrap();
        assert_eq!(plan.source(), "vieja");
        assert_eq!(plan.destination(), Some("tema/nueva"));
        let destination = git.join("refs/heads/tema/nueva");
        assert!(!destination.exists());
        let publication = apply_ref_change(plan, || false).unwrap();
        assert_eq!(publication.target, target);
        assert!(!branch.exists());
        assert!(!log.exists());
        assert_eq!(
            fs::read_to_string(destination).unwrap(),
            format!("{target}\n")
        );
        assert_eq!(
            fs::read_to_string(git.join("logs/refs/heads/tema/nueva")).unwrap(),
            history
        );
        assert_eq!(fs::read(git.join("HEAD")).unwrap(), head);
        assert_eq!(fs::read(git.join("index")).unwrap(), b"index fixtures");
        assert_eq!(
            fs::read(fixture.root.join("archivo")).unwrap(),
            b"contenido usuario"
        );
        assert!(!git.join("packed-refs").exists());
        assert!(!git.join("packed-refs.lock").exists());
    }

    #[cfg(windows)]
    #[test]
    fn renaming_and_removing_a_packed_tag_preserves_its_raw_object() {
        let fixture = Fixture::new();
        let repo = gix::init(&fixture.root).unwrap();
        let target = commit(&repo, &[], "original");
        let tag = repo
            .write_object(gix::objs::Tag {
                target,
                target_kind: gix::objs::Kind::Commit,
                name: "anotada".into(),
                tagger: None,
                message: "mensaje".into(),
                signature: None,
            })
            .unwrap()
            .detach();
        let git = fixture.git();
        let packed = git.join("packed-refs");
        fs::write(&packed, format!("# pack-refs with: peeled sorted\n{tag} refs/tags/anotada\n^{target}\n{target} refs/tags/otra\n")).unwrap();
        assert!(
            fixture
                .change(target, RefKind::Tag, "anotada", Some("nueva"))
                .is_err()
        );
        let plan = fixture
            .change(tag, RefKind::Tag, "anotada", Some("nueva"))
            .unwrap();
        apply_ref_change(plan, || false).unwrap();
        let packed_contents = fs::read_to_string(&packed).unwrap();
        assert!(!packed_contents.contains("refs/tags/anotada"));
        assert!(packed_contents.contains(&format!("{target} refs/tags/otra")));
        assert_eq!(
            fs::read_to_string(git.join("refs/tags/nueva")).unwrap(),
            format!("{tag}\n")
        );
        let plan = fixture.change(tag, RefKind::Tag, "nueva", None).unwrap();
        apply_ref_change(plan, || false).unwrap();
        assert!(!git.join("refs/tags/nueva").exists());
        assert!(fixture.change(target, RefKind::Tag, "otra", None).is_ok());
    }

    #[test]
    fn branch_deletion_requires_reachability_from_head_and_an_exact_review() {
        let fixture = Fixture::new();
        let repo = gix::init(&fixture.root).unwrap();
        let target = commit(&repo, &[], "original");
        let unrelated = commit(&repo, &[], "sin integrar");
        let merge = commit(&repo, &[unrelated, target], "merge");
        let git = fixture.git();
        let active = git.join("refs/heads/activa");
        let source = git.join("refs/heads/tema");
        fs::write(git.join("HEAD"), b"ref: refs/heads/activa\n").unwrap();
        fs::write(&active, format!("{unrelated}\n")).unwrap();
        fs::write(&source, format!("{target}\n")).unwrap();
        assert!(
            fixture
                .change(unrelated, RefKind::Branch, "activa", Some("nueva"))
                .is_err()
        );
        assert!(
            fixture
                .change(target, RefKind::Branch, "tema", None)
                .is_err()
        );
        fs::write(&active, format!("{merge}\n")).unwrap();
        let plan = fixture
            .change(target, RefKind::Branch, "tema", None)
            .unwrap();
        fs::write(&source, format!("{unrelated}\n")).unwrap();
        assert!(matches!(
            apply_ref_change(plan, || false),
            Err(Error::Stale)
        ));
        assert_eq!(
            fs::read_to_string(&source).unwrap(),
            format!("{unrelated}\n")
        );
        fs::write(&source, format!("{target}\n")).unwrap();
        let plan = fixture
            .change(target, RefKind::Branch, "tema", None)
            .unwrap();
        fs::write(&active, format!("{unrelated}\n")).unwrap();
        assert!(matches!(
            apply_ref_change(plan, || false),
            Err(Error::Stale)
        ));
        fs::write(&active, format!("{merge}\n")).unwrap();
        let plan = fixture
            .change(target, RefKind::Branch, "tema", None)
            .unwrap();
        apply_ref_change(plan, || false).unwrap();
        assert!(!source.exists());
        assert_eq!(fs::read_to_string(&active).unwrap(), format!("{merge}\n"));
    }

    #[test]
    fn protected_references_and_interrupted_changes_preserve_sources() {
        let fixture = Fixture::new();
        let repo = gix::init(&fixture.root).unwrap();
        let target = commit(&repo, &[], "original");
        let git = fixture.git();
        let source = git.join("refs/tags/origen");
        fs::write(&source, format!("{target}\n")).unwrap();
        fs::write(git.join("refs/tags/alias"), b"ref: refs/tags/origen\n").unwrap();
        assert!(
            fixture
                .change(target, RefKind::Tag, "origen", None)
                .is_err()
        );
        fs::remove_file(git.join("refs/tags/alias")).unwrap();
        fs::create_dir(git.join("worktrees")).unwrap();
        assert!(
            fixture
                .change(target, RefKind::Tag, "origen", None)
                .is_err()
        );
        fs::remove_dir(git.join("worktrees")).unwrap();
        let plan = fixture
            .change(target, RefKind::Tag, "origen", None)
            .unwrap();
        let lock = git.join("refs/tags/origen.lock");
        assert!(matches!(
            apply_ref_change(plan, || lock.exists()),
            Err(Error::Cancelled)
        ));
        assert!(!lock.exists());
        assert_eq!(fs::read_to_string(&source).unwrap(), format!("{target}\n"));
        #[cfg(windows)]
        {
            let plan = fixture
                .change(target, RefKind::Tag, "origen", Some("ocupada"))
                .unwrap();
            let destination = git.join("refs/tags/ocupada");
            fs::write(&destination, format!("{target}\n")).unwrap();
            assert!(apply_ref_change(plan, || false).is_err());
            assert!(source.exists());
            assert!(destination.exists());
        }
    }
}
