//! Reviewed creation of local branches and lightweight tags.

use crate::{Context, Error, Identity, Limits, controlled, engine, graph, index_write, revision};
use gix::{
    bstr::{BString, ByteSlice},
    lock::acquire::Fail,
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

fn reference_name(kind: RefKind, name: &str, limits: Limits) -> Result<FullName, Error> {
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

fn namespace_conflicts(existing: &[u8], requested: &str) -> bool {
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
    if refs
        .iter()
        .any(|entry| namespace_conflicts(&entry.name, name))
    {
        return Ok(false);
    }
    // Also guard empty directories and filesystem aliases absent from gix's
    // ref enumeration. A transaction must not remove or reuse such a target.
    let mut path = identity.git_dir.clone();
    let mut parts = name.split('/').peekable();
    while let Some(part) = parts.next() {
        ctx.check()?;
        path.push(part);
        if !index_write::exists(&path)? {
            return Ok(true);
        }
        if parts.peek().is_none() {
            return Ok(false);
        }
        index_write::ordinary(&path, true)?;
        if path.canonicalize()? != path {
            return Ok(false);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::PathBuf,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    struct Fixture {
        root: PathBuf,
        parent: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
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
                "git-ref-write-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
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
}
