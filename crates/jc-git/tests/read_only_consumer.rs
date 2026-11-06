//! An external consumer of the public Rust Git API. Fixture preparation uses
//! gix directly; queries and comparison require neither an editor nor Git CLI.
use gix::bstr::ByteSlice;
use jc_git::{
    Change, Limits,
    diff_model::{DiffLimits, DiffLine, DiffSide, DiffStatus, OwnedDiffModel},
    revision::{read_file_pair, read_revision},
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
