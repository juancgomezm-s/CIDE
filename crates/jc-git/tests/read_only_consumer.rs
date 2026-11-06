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
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

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
            "git-independent-consumer-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
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
