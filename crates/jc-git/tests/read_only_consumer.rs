//! An external consumer of the public Rust Git API. Fixture preparation uses
//! gix directly; queries and comparison require neither an editor nor Git CLI.
use gix::bstr::ByteSlice;
use jc_git::{
    Change, Error, Limits,
    diff_model::{DiffLimits, DiffLine, DiffSide, DiffStatus, OwnedDiffModel},
    revision::{HeadCommit, read_file_pair, read_head_commit, read_revision},
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    root: PathBuf,
    base: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .unwrap()
            .canonicalize()
            .unwrap();
        let base = workspace.join("work/test-temp");
        fs::create_dir_all(&base).unwrap();
        let base = base.canonicalize().unwrap();
        assert!(base.starts_with(&workspace) && !base.is_symlink());
        let root = base.join(format!(
            "git-independent-consumer-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self { root, base }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if self.root.parent() == Some(self.base.as_path())
            && !self.root.is_symlink()
            && self
                .root
                .canonicalize()
                .is_ok_and(|root| root.starts_with(&self.base))
        {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}
fn commit(repo: &gix::Repository, bytes: &[u8], parent: Option<gix::ObjectId>) -> gix::ObjectId {
    let blob = repo.write_blob(bytes).unwrap().detach();
    let tree = repo
        .write_object(gix::objs::Tree {
            entries: vec![gix::objs::tree::Entry {
                mode: 0o100644u32.try_into().unwrap(),
                filename: b"fuente.p".to_vec().into(),
                oid: blob,
            }],
        })
        .unwrap()
        .detach();
    let author = gix::actor::SignatureRef {
        name: b"independent Rust consumer".as_bstr(),
        email: b"fixture@example.invalid".as_bstr(),
        time: "1 +0000",
    };
    repo.commit_as(
        author,
        author,
        "HEAD",
        "owned consumer fixture",
        tree,
        parent,
    )
    .unwrap()
    .detach()
}
fn image(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, path: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            let metadata = fs::symlink_metadata(&path).unwrap();
            assert!(!metadata.file_type().is_symlink());
            if metadata.is_dir() {
                visit(root, &path, files);
            } else {
                files.insert(
                    path.strip_prefix(root).unwrap().to_owned(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}

#[test]
fn public_head_commit_distinguishes_unborn_and_preserves_exact_parent_order() {
    let fixture = Fixture::new();
    let limits = Limits {
        max_blob: 0,
        max_entries: 32,
        max_path: 4096,
        max_bytes: 128 * 1024,
        timeout: Duration::from_secs(3),
    };
    assert!(matches!(
        read_head_commit(&fixture.root, limits, || false),
        Err(Error::NotRepository)
    ));
    let repo = gix::init(&fixture.root).unwrap();
    assert_eq!(
        read_head_commit(&fixture.root, limits, || false).unwrap(),
        None
    );
    let first = commit(&repo, b"first", None);
    let second = commit(&repo, b"second", Some(first));
    let tree = repo
        .find_commit(second)
        .unwrap()
        .tree_id()
        .unwrap()
        .detach();
    let author = gix::actor::SignatureRef {
        name: b"independent Rust consumer".as_bstr(),
        email: b"fixture@example.invalid".as_bstr(),
        time: "1 +0000",
    };
    let merge = repo
        .commit_as(
            author,
            author,
            "HEAD",
            "merge fixture",
            tree,
            [second, first],
        )
        .unwrap()
        .detach();
    let expected = HeadCommit {
        commit: merge.to_string(),
        tree: tree.to_string(),
        parents: vec![second.to_string(), first.to_string()],
    };
    let before = image(&fixture.root);
    assert_eq!(
        read_head_commit(&fixture.root, limits, || false).unwrap(),
        Some(expected)
    );
    assert_eq!(image(&fixture.root), before);
    assert!(matches!(
        read_head_commit(&fixture.root, limits, || true),
        Err(Error::Cancelled)
    ));
    assert!(matches!(
        read_head_commit(
            &fixture.root,
            Limits {
                max_bytes: 1,
                ..limits
            },
            || false
        ),
        Err(Error::Limited)
    ));

    let mut parents = Vec::from([merge]);
    for byte in 0..8 {
        parents.push(commit(&repo, &[byte], parents.last().copied()));
    }
    repo.commit_as(
        author,
        author,
        "HEAD",
        "too many parents",
        tree,
        parents.into_iter().rev(),
    )
    .unwrap();
    assert!(matches!(
        read_head_commit(&fixture.root, limits, || false),
        Err(Error::Limited)
    ));

    drop(repo);
    fs::write(fixture.root.join(".git/HEAD"), format!("{first}\n")).unwrap();
    let detached = read_head_commit(&fixture.root, limits, || false)
        .unwrap()
        .unwrap();
    assert_eq!(detached.commit, first.to_string());
    assert!(detached.parents.is_empty());
}

#[test]
fn public_core_enumerates_pinned_revisions_and_compares_exact_bytes_without_host_or_git_cli() {
    const OLD: &[u8] = b"\x80caf\xe9\r\nkeep\r\n";
    const NEW: &[u8] = b"\x80nuevo\xe9\r\nkeep\r\n\xfffin";
    const WORKTREE: &[u8] = b"personal working bytes are not historical bytes\n";
    let fixture = Fixture::new();
    let repo = gix::init(&fixture.root).unwrap();
    let root = commit(&repo, OLD, None);
    let next = commit(&repo, NEW, Some(root));
    // A real, valid empty index differs from either committed tree. Historical
    // queries must not refresh it or silently compare the worktree instead.
    gix::index::File::from_state(
        gix::index::State::new(repo.object_hash()),
        fixture.root.join(".git/index"),
    )
    .write(gix::index::write::Options::default())
    .unwrap();
    drop(repo);
    fs::write(fixture.root.join("fuente.p"), WORKTREE).unwrap();
    let before = image(&fixture.root);
    assert!(before.contains_key(Path::new(".git/index")));
    assert!(before.keys().any(|path| path.starts_with(".git/refs")));

    let limits = Limits {
        max_blob: 2 * 1024 * 1024,
        max_entries: 32,
        max_path: 4096,
        max_bytes: 4 * 1024 * 1024,
        timeout: Duration::from_secs(5),
    };
    let revision = read_revision(&fixture.root, &root.to_string(), limits, || false).unwrap();
    assert!(revision.parent.is_none());
    assert_eq!(revision.files.len(), 1);
    assert_eq!(revision.files[0].change, Change::Added);
    assert_eq!(revision.files[0].path, b"fuente.p");
    assert!(revision.files[0].before.is_none());
    let root_pair = read_file_pair(
        &fixture.root,
        &root.to_string(),
        b"fuente.p",
        limits,
        || false,
    )
    .unwrap();
    assert!(root_pair.before.is_none());
    assert_eq!(root_pair.after.unwrap().bytes, OLD);

    let revision = read_revision(&fixture.root, &next.to_string(), limits, || false).unwrap();
    assert_eq!(revision.parent, Some(root.to_string()));
    assert_eq!(revision.files.len(), 1);
    assert_eq!(revision.files[0].change, Change::Modified);
    let pair = read_file_pair(
        &fixture.root,
        &revision.commit,
        &revision.files[0].path,
        limits,
        || false,
    )
    .unwrap();
    let left = Arc::new(pair.before.unwrap().bytes);
    let right = Arc::new(pair.after.unwrap().bytes);
    let model =
        OwnedDiffModel::compute(left.clone(), right.clone(), DiffLimits::default(), || false);
    assert_eq!(model.status(), DiffStatus::Exact);
    assert_eq!(model.source_bytes(DiffSide::Left), OLD);
    assert_eq!(model.source_bytes(DiffSide::Right), NEW);
    assert_eq!(
        model.line_bytes(DiffSide::Right, DiffLine(0)),
        Some(b"\x80nuevo\xe9\r\n".as_slice())
    );
    assert_eq!(
        model.line_bytes(DiffSide::Right, DiffLine(2)),
        Some(b"\xfffin".as_slice())
    );
    assert!(model.hunks().next().is_some());
    assert_eq!(
        model.corresponding_line(DiffSide::Left, DiffLine(1)),
        Some(DiffLine(1))
    );
    let cancelled = OwnedDiffModel::compute(left, right, DiffLimits::default(), || true);
    assert_eq!(cancelled.status(), DiffStatus::Cancelled);
    assert_eq!(fs::read(fixture.root.join("fuente.p")).unwrap(), WORKTREE);
    assert_eq!(
        image(&fixture.root),
        before,
        "An independent read changed worktree, index, refs or Git objects"
    );
}

#[cfg(windows)]
fn local_index(
    repo: &gix::Repository,
    entries: &[(&[u8], gix::ObjectId, gix::index::entry::Flags)],
) {
    let mut state = gix::index::State::new(repo.object_hash());
    for (path, id, flags) in entries {
        state.dangerously_push_entry(
            Default::default(),
            *id,
            *flags,
            gix::index::entry::Mode::FILE,
            path.as_bstr(),
        );
    }
    state.sort_entries();
    gix::index::File::from_state(state, repo.git_dir().join("index"))
        .write(Default::default())
        .unwrap();
}

#[cfg(windows)]
#[test]
fn restore_deleted_file_publishes_exact_index_bytes_and_keeps_git_metadata() {
    use gix::index::entry::Flags;
    use jc_git::restore::{apply_restore, prepare_restore};
    let fixture = Fixture::new();
    let repo = gix::init(&fixture.root).unwrap();
    let bytes = b"primer bloque\r\n\xffsin salto final";
    let id = repo.write_blob(bytes).unwrap().detach();
    local_index(&repo, &[(b"fuentes/dato.p", id, Flags::empty())]);
    fs::create_dir(fixture.root.join("fuentes")).unwrap();
    let baseline = image(&fixture.root.join(".git"));
    let limits = Limits {
        max_blob: 2 * 1024 * 1024,
        max_entries: 128,
        max_path: 1024,
        max_bytes: 4 * 1024 * 1024,
        timeout: Duration::from_secs(2),
    };
    let plan = prepare_restore(&fixture.root, b"fuentes/dato.p", limits, || false).unwrap();
    let target = fixture.root.join("fuentes/dato.p");
    assert!(!target.exists());
    assert_eq!(plan.pair().before.bytes(), bytes);
    assert!(plan.pair().after.identity().is_absent());
    apply_restore(plan, || false).unwrap();
    assert_eq!(fs::read(&target).unwrap(), bytes);
    assert_eq!(image(&fixture.root.join(".git")), baseline);
    assert_eq!(fs::read_dir(target.parent().unwrap()).unwrap().count(), 1);
}

#[cfg(windows)]
#[test]
fn restore_deleted_file_rejects_stale_index_and_unavailable_destination() {
    use gix::index::entry::Flags;
    use jc_git::{
        Error,
        restore::{apply_restore, prepare_restore},
    };
    let fixture = Fixture::new();
    let repo = gix::init(&fixture.root).unwrap();
    let original = repo.write_blob(b"original").unwrap().detach();
    let different = repo.write_blob(b"nuevo").unwrap().detach();
    let flags = Flags::empty();
    let limits = Limits {
        max_blob: 2 * 1024 * 1024,
        max_entries: 128,
        max_path: 1024,
        max_bytes: 4 * 1024 * 1024,
        timeout: Duration::from_secs(2),
    };
    local_index(&repo, &[(b"f.p", original, flags)]);
    let plan = prepare_restore(&fixture.root, b"f.p", limits, || false).unwrap();
    local_index(&repo, &[(b"f.p", different, flags)]);
    assert!(matches!(apply_restore(plan, || false), Err(Error::Stale)));
    let path = fixture.root.join("f.p");
    assert!(!path.exists());
    let plan = prepare_restore(&fixture.root, b"f.p", limits, || false).unwrap();
    let lock = fixture.root.join(".git/index.lock");
    fs::write(&lock, b"ajeno").unwrap();
    assert!(apply_restore(plan, || false).is_err());
    assert_eq!(fs::read(&lock).unwrap(), b"ajeno");
    fs::remove_file(lock).unwrap();
    let plan = prepare_restore(&fixture.root, b"f.p", limits, || false).unwrap();
    assert!(matches!(
        apply_restore(plan, || true),
        Err(Error::Cancelled)
    ));
    let plan = prepare_restore(&fixture.root, b"f.p", limits, || false).unwrap();
    fs::write(&path, b"creado por otro proceso").unwrap();
    assert!(matches!(apply_restore(plan, || false), Err(Error::Stale)));
    assert_eq!(fs::read(&path).unwrap(), b"creado por otro proceso");
    fs::write(&path, b"nuevo").unwrap();
    assert!(prepare_restore(&fixture.root, b"f.p", limits, || false).is_err());
    local_index(&repo, &[(b"missing/f.p", original, flags)]);
    assert!(prepare_restore(&fixture.root, b"missing/f.p", limits, || false).is_err());
    assert!(!fixture.root.join("missing").exists());
}

#[cfg(windows)]
#[test]
fn restore_existing_file_preserves_original_at_reported_backup() {
    use gix::index::entry::Flags;
    use jc_git::restore::{apply_restore, prepare_restore};
    let fixture = Fixture::new();
    let repo = gix::init(&fixture.root).unwrap();
    let staged = b"primer bloque\r\n\xffsin salto final";
    let edited = b"cambios locales\r\n\xfe";
    let id = repo.write_blob(staged).unwrap().detach();
    local_index(&repo, &[(b"nested/f.p", id, Flags::empty())]);
    fs::create_dir(fixture.root.join("nested")).unwrap();
    let target = fixture.root.join("nested/f.p");
    fs::write(&target, edited).unwrap();
    let before = image(&fixture.root.join(".git"));
    let limits = Limits {
        max_blob: 2 * 1024 * 1024,
        max_entries: 128,
        max_path: 1024,
        max_bytes: 4 * 1024 * 1024,
        timeout: Duration::from_secs(2),
    };
    let plan = prepare_restore(&fixture.root, b"nested/f.p", limits, || false).unwrap();
    assert_eq!(plan.pair().before.bytes(), staged);
    assert_eq!(plan.pair().after.bytes(), edited);
    assert_eq!(fs::read(&target).unwrap(), edited);
    assert_eq!(image(&fixture.root.join(".git")), before);
    let backup = apply_restore(plan, || false).unwrap().backup.unwrap();
    assert_eq!(backup.parent(), Some(fixture.root.join(".git").as_path()));
    assert_eq!(fs::read(&target).unwrap(), staged);
    assert_eq!(fs::read(&backup).unwrap(), edited);
    let mut after = image(&fixture.root.join(".git"));
    assert_eq!(
        after.remove(Path::new(backup.file_name().unwrap())),
        Some(edited.to_vec())
    );
    assert_eq!(after, before);
    assert_eq!(fs::read_dir(target.parent().unwrap()).unwrap().count(), 1);
}

#[cfg(windows)]
#[test]
fn restore_existing_file_rejects_stale_worktree_and_hardlink() {
    use gix::index::entry::Flags;
    use jc_git::{
        Error,
        restore::{apply_restore, prepare_restore},
    };
    let fixture = Fixture::new();
    let repo = gix::init(&fixture.root).unwrap();
    let indexed = repo.write_blob(b"indexed").unwrap().detach();
    local_index(&repo, &[(b"f.p", indexed, Flags::empty())]);
    let target = fixture.root.join("f.p");
    fs::write(&target, b"reviewed").unwrap();
    let baseline = image(&fixture.root.join(".git"));
    let limits = Limits {
        max_blob: 2 * 1024 * 1024,
        max_entries: 128,
        max_path: 1024,
        max_bytes: 4 * 1024 * 1024,
        timeout: Duration::from_secs(2),
    };
    let plan = prepare_restore(&fixture.root, b"f.p", limits, || false).unwrap();
    fs::write(&target, b"changed after preview").unwrap();
    assert!(matches!(apply_restore(plan, || false), Err(Error::Stale)));
    assert_eq!(fs::read(&target).unwrap(), b"changed after preview");
    let alias = fixture.root.join("alias.p");
    fs::hard_link(&target, &alias).unwrap();
    assert!(matches!(
        prepare_restore(&fixture.root, b"f.p", limits, || false),
        Err(Error::Unsupported)
    ));
    assert_eq!(fs::read(&alias).unwrap(), b"changed after preview");
    assert_eq!(image(&fixture.root.join(".git")), baseline);
}

#[cfg(windows)]
#[test]
fn public_local_pairs_keep_head_index_disk_and_absence_distinct_without_writes() {
    use gix::index::entry::Flags;
    use jc_git::{
        Error,
        local_pair::{
            AbsentReason, IndexChecksum, PairKind, SourceIdentity, SourceOrigin, read_local_pair,
        },
    };
    const HEAD: &[u8] = b"\x80HEAD\xe9\r\n";
    const INDEX: &[u8] = b"\xffINDEX\r\n";
    const DISK: &[u8] = b"\x81DISCO\n";
    let fixture = Fixture::new();
    let repo = gix::init(&fixture.root).unwrap();
    let head = commit(&repo, HEAD, None);
    let indexed = repo.write_blob(INDEX).unwrap().detach();
    let empty = repo.write_blob([]).unwrap().detach();
    local_index(
        &repo,
        &[
            (b"fuente.p", indexed, Flags::empty()),
            (b"vacio.p", empty, Flags::empty()),
        ],
    );
    fs::write(fixture.root.join("fuente.p"), DISK).unwrap();
    fs::write(fixture.root.join("vacio.p"), []).unwrap();
    fs::write(fixture.root.join("nuevo.p"), []).unwrap();
    let limits = Limits {
        max_blob: 2 * 1024 * 1024,
        max_entries: 32,
        max_path: 4096,
        max_bytes: 4 * 1024 * 1024,
        timeout: Duration::from_secs(5),
    };
    let original = image(&fixture.root);
    let prepared = read_local_pair(
        &fixture.root,
        b"fuente.p",
        PairKind::Prepared,
        limits,
        || false,
    )
    .unwrap();
    assert_eq!(
        prepared.head.as_ref().unwrap().commit,
        Some(head.to_string())
    );
    assert_eq!(prepared.before.bytes(), HEAD);
    assert_eq!(prepared.after.bytes(), INDEX);
    assert_eq!(prepared.before.identity().origin(), SourceOrigin::Head);
    assert_eq!(
        prepared.after.identity().oid(),
        Some(indexed.to_string().as_str())
    );
    assert_eq!(prepared.index.checksum(), IndexChecksum::Verified);
    assert!(prepared.index.sha256().is_some());
    let working = read_local_pair(
        &fixture.root,
        b"fuente.p",
        PairKind::Worktree,
        limits,
        || false,
    )
    .unwrap();
    assert!(working.head.is_none());
    assert_eq!(working.before.bytes(), INDEX);
    assert_eq!(working.after.bytes(), DISK);
    assert_eq!(working.index, prepared.index);
    assert!(matches!(
        working.after.identity(),
        SourceIdentity::Present {
            origin: SourceOrigin::Worktree,
            oid: None,
            worktree: Some(_),
            ..
        }
    ));
    let (_, left) = working.before.into_parts();
    let (_, right) = working.after.into_parts();
    let diff = OwnedDiffModel::compute(
        Arc::new(left),
        Arc::new(right),
        DiffLimits::default(),
        || false,
    );
    assert_eq!(diff.status(), DiffStatus::Exact);
    assert_eq!(diff.source_bytes(DiffSide::Left), INDEX);
    assert_eq!(diff.source_bytes(DiffSide::Right), DISK);
    let empty = read_local_pair(
        &fixture.root,
        b"vacio.p",
        PairKind::Prepared,
        limits,
        || false,
    )
    .unwrap();
    assert!(matches!(
        empty.before.identity(),
        SourceIdentity::Absent {
            reason: AbsentReason::TreeEntryMissing,
            ..
        }
    ));
    assert!(!empty.after.identity().is_absent());
    assert!(empty.after.bytes().is_empty());
    let new = read_local_pair(
        &fixture.root,
        b"nuevo.p",
        PairKind::Worktree,
        limits,
        || false,
    )
    .unwrap();
    assert!(matches!(
        new.before.identity(),
        SourceIdentity::Absent {
            reason: AbsentReason::IndexEntryMissing,
            ..
        }
    ));
    assert!(!new.after.identity().is_absent());
    let missing = read_local_pair(
        &fixture.root,
        b"ausente.p",
        PairKind::Worktree,
        limits,
        || false,
    )
    .unwrap();
    assert!(matches!(
        missing.after.identity(),
        SourceIdentity::Absent {
            reason: AbsentReason::WorktreeEntryMissing,
            ..
        }
    ));
    assert_eq!(
        image(&fixture.root),
        original,
        "Read-only consumers changed source, index, refs or objects"
    );

    // Preserve a valid trailer while corrupting the body: parsing alone does
    // not authenticate index bytes and must not publish a false comparison.
    let index_path = repo.git_dir().join("index");
    let valid = fs::read(&index_path).unwrap();
    let mut corrupt = valid.clone();
    corrupt[12] ^= 1;
    fs::write(&index_path, &corrupt).unwrap();
    let before = image(&fixture.root);
    assert!(matches!(
        read_local_pair(
            &fixture.root,
            b"fuente.p",
            PairKind::Prepared,
            limits,
            || false
        ),
        Err(Error::Engine(_))
    ));
    assert_eq!(image(&fixture.root), before);
    // A legitimate skipped checksum remains content pinned by SHA-256.
    let mut skipped = valid;
    let trailer = skipped.len() - repo.object_hash().len_in_bytes();
    skipped[trailer..].fill(0);
    fs::write(&index_path, skipped).unwrap();
    let pair = read_local_pair(
        &fixture.root,
        b"fuente.p",
        PairKind::Prepared,
        limits,
        || false,
    )
    .unwrap();
    assert_eq!(pair.index.checksum(), IndexChecksum::Skipped);
    assert_ne!(pair.index, prepared.index);

    for (flags, conflict) in [
        // The on-disk extended flag is required: the low-level gix fixture
        // writer intentionally does not infer it from INTENT_TO_ADD.
        (Flags::INTENT_TO_ADD | Flags::EXTENDED, false),
        (Flags::from_stage(gix::index::entry::Stage::Ours), true),
    ] {
        local_index(&repo, &[(b"fuente.p", indexed, flags)]);
        let before = image(&fixture.root);
        let result = read_local_pair(
            &fixture.root,
            b"fuente.p",
            PairKind::Prepared,
            limits,
            || false,
        );
        assert!(
            if conflict {
                matches!(&result, Err(Error::Unmerged))
            } else {
                matches!(&result, Err(Error::Unsupported))
            },
            "flags={flags:?}, conflict={conflict}, result={result:?}"
        );
        assert_eq!(image(&fixture.root), before);
    }
    let missing_oid = gix::ObjectId::from_hex(b"1111111111111111111111111111111111111111").unwrap();
    local_index(&repo, &[(b"fuente.p", missing_oid, Flags::empty())]);
    let before = image(&fixture.root);
    assert!(
        read_local_pair(
            &fixture.root,
            b"fuente.p",
            PairKind::Prepared,
            limits,
            || false
        )
        .is_err(),
        "An advertised missing blob is corruption, not an absent entry"
    );
    assert_eq!(image(&fixture.root), before);

    let unborn_fixture = Fixture::new();
    let unborn_repo = gix::init(&unborn_fixture.root).unwrap();
    let empty = unborn_repo.write_blob([]).unwrap().detach();
    local_index(&unborn_repo, &[(b"vacio.p", empty, Flags::empty())]);
    let before = image(&unborn_fixture.root);
    let pair = read_local_pair(
        &unborn_fixture.root,
        b"vacio.p",
        PairKind::Prepared,
        limits,
        || false,
    )
    .unwrap();
    assert!(pair.head.as_ref().unwrap().commit.is_none());
    assert!(matches!(
        pair.before.identity(),
        SourceIdentity::Absent {
            reason: AbsentReason::UnbornHead,
            ..
        }
    ));
    assert!(!pair.after.identity().is_absent());
    assert!(pair.after.bytes().is_empty());
    assert_eq!(image(&unborn_fixture.root), before);
}
