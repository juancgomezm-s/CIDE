#![cfg(windows)]
//! Recovery-only owned fixtures. Original source files are never recovery targets.
use jc_files::recovery::{LoadedRecovery, RecoveryLimits, RecoveryStore};
use std::{
    fs::{self, OpenOptions},
    io::{self, Read, Seek, Write},
    os::windows::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    root: PathBuf,
    base: PathBuf,
    original: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .unwrap()
            .canonicalize()
            .unwrap();
        let mut base = workspace.clone();
        for name in ["work", "test-temp"] {
            base.push(name);
            match fs::create_dir(&base) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("fixture: {error}"),
            }
            assert_eq!(
                fs::symlink_metadata(&base).unwrap().file_attributes() & 0x400,
                0
            );
            base = base.canonicalize().unwrap();
            assert!(base.starts_with(&workspace));
        }
        let root = base.join(format!(
            "recovery-gc-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let original = root.join("original-source.txt");
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&original)
            .unwrap()
            .write_all(b"ORIGINAL\xff\r\n\xef\xbb\xbfpreserve\x00")
            .unwrap();
        Self {
            root,
            base,
            original,
        }
    }
    fn store(&self, limits: RecoveryLimits) -> RecoveryStore {
        RecoveryStore::open(&self.root.join("sessions"), limits).unwrap()
    }
    fn original_intact(&self) {
        assert_eq!(
            fs::read(&self.original).unwrap(),
            b"ORIGINAL\xff\r\n\xef\xbb\xbfpreserve\x00"
        );
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(self.root.parent(), Some(self.base.as_path()));
        assert_eq!(self.root.canonicalize().unwrap(), self.root);
        assert_eq!(
            fs::symlink_metadata(&self.root).unwrap().file_attributes() & 0x400,
            0
        );
        assert!(self
            .root
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("recovery-gc-"));
        fs::remove_dir_all(&self.root).unwrap();
    }
}
fn generation(store: &RecoveryStore, id: u64) -> PathBuf {
    store.slot_path().join(format!("generation-{id:020}"))
}
fn checkpoint(store: &mut RecoveryStore, metadata: &[u8], docs: &[&[u8]]) -> u64 {
    store
        .checkpoint(metadata, docs.len(), |index, writer| {
            for chunk in docs[index].chunks(3) {
                writer.write_all(chunk)?;
            }
            Ok(())
        })
        .unwrap()
}
fn contents(mut loaded: LoadedRecovery) -> (Vec<u8>, Vec<Vec<u8>>, bool) {
    let mut values = vec![];
    for mut file in loaded.documents.drain(..) {
        assert_eq!(file.stream_position().unwrap(), 0);
        let mut bytes = vec![];
        file.read_to_end(&mut bytes).unwrap();
        values.push(bytes);
    }
    (loaded.metadata, values, loaded.used_backup)
}

#[test]
fn recovery_gc_retains_two_confirmed_generations_and_other_slots() {
    let fixture = Fixture::new();
    let mut store = fixture.store(Default::default());
    let ids: Vec<_> = (0..10)
        .map(|index| checkpoint(&mut store, &[index], &[b"bytes\xff\r\n"]))
        .collect();
    let mut other = fixture.store(Default::default());
    let other_id = checkpoint(&mut other, b"other", &[b"untouched"]);
    let other_head = fs::read(other.slot_path().join("HEAD")).unwrap();
    let report = store.prune_obsolete().unwrap();
    assert_eq!(report.protected_generations, 2);
    assert_eq!(report.removed_generations, 8);
    assert_eq!(report.removed_files, 24);
    assert!(report.reclaimed_bytes > 0);
    for id in &ids[..8] {
        assert!(!generation(&store, *id).exists());
    }
    for id in &ids[8..] {
        assert!(generation(&store, *id).is_dir());
    }
    assert_eq!(store.prune_obsolete().unwrap().removed_generations, 0);
    assert_eq!(contents(store.load_latest().unwrap().unwrap()).0, [9]);
    assert_eq!(
        fs::read(other.slot_path().join("HEAD")).unwrap(),
        other_head
    );
    assert_eq!(other.load_latest().unwrap().unwrap().generation, other_id);
    fixture.original_intact();
}

#[test]
fn recovery_gc_corrupt_head_or_backup_fails_before_any_removal() {
    let fixture = Fixture::new();
    let mut store = fixture.store(Default::default());
    let ids: Vec<_> = (0..5)
        .map(|index| checkpoint(&mut store, &[index], &[b"intact"]))
        .collect();
    for id in [ids[4], ids[3]] {
        let path = generation(&store, id).join("doc-0000.bin");
        let saved = fs::read(&path).unwrap();
        fs::write(&path, b"bad fingerprint").unwrap();
        assert!(store.prune_obsolete().is_err());
        for id in &ids {
            assert!(generation(&store, *id).is_dir());
        }
        fs::write(path, saved).unwrap();
    }
    let path = store.slot_path().join("BACKUP");
    let saved = fs::read(&path).unwrap();
    fs::write(&path, b"truncated").unwrap();
    assert!(store.prune_obsolete().is_err());
    for id in &ids {
        assert!(generation(&store, *id).is_dir());
    }
    fs::write(path, saved).unwrap();
    assert_eq!(store.prune_obsolete().unwrap().removed_generations, 3);
    fixture.original_intact();
}

#[test]
fn recovery_gc_unknown_file_and_hardlink_preflight_preserves_entire_batch() {
    let fixture = Fixture::new();
    let mut store = fixture.store(Default::default());
    let ids: Vec<_> = (0..4)
        .map(|index| checkpoint(&mut store, &[index], &[b"source"]))
        .collect();
    let unknown = generation(&store, ids[1]).join("user-data.txt");
    fs::write(&unknown, b"must not delete").unwrap();
    assert!(store.prune_obsolete().is_err());
    assert!(generation(&store, ids[0]).join("doc-0000.bin").exists());
    assert_eq!(fs::read(&unknown).unwrap(), b"must not delete");
    fs::remove_file(unknown).unwrap();
    let aliased = generation(&store, ids[1]).join("doc-0000.bin");
    fs::remove_file(&aliased).unwrap();
    fs::hard_link(&fixture.original, &aliased).unwrap();
    assert!(store.prune_obsolete().is_err());
    assert!(generation(&store, ids[0]).join("doc-0000.bin").exists());
    fixture.original_intact();
    fs::remove_file(aliased).unwrap();
    assert_eq!(store.prune_obsolete().unwrap().removed_generations, 2);
}

#[test]
fn recovery_gc_locked_and_interrupted_candidates_resume_idempotently() {
    let fixture = Fixture::new();
    let mut store = fixture.store(Default::default());
    let ids: Vec<_> = (0..5)
        .map(|index| checkpoint(&mut store, &[index], &[b"source"]))
        .collect();
    // Simulate an earlier interrupted cleanup: a known subset remains.
    fs::remove_file(generation(&store, ids[0]).join("metadata.bin")).unwrap();
    let locked = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(generation(&store, ids[1]).join("doc-0000.bin"))
        .unwrap();
    let report = store.prune_obsolete().unwrap();
    assert_eq!(report.removed_generations, 2);
    assert_eq!(report.conflicted_generations, 1);
    assert!(generation(&store, ids[1]).join("metadata.bin").exists());
    assert!(generation(&store, ids[1]).join("doc-0000.bin").exists());
    drop(locked);
    assert_eq!(store.prune_obsolete().unwrap().removed_generations, 1);
    assert_eq!(store.prune_obsolete().unwrap().removed_generations, 0);
    assert_eq!(store.load_latest().unwrap().unwrap().generation, ids[4]);
    fixture.original_intact();
}

#[test]
fn recovery_gc_revalidates_corruption_before_replacing_backup() {
    let fixture = Fixture::new();
    let mut store = fixture.store(Default::default());
    let first = checkpoint(&mut store, b"valid backup", &[b"original valid"]);
    let second = checkpoint(&mut store, b"second", &[b"newer"]);
    fs::write(
        generation(&store, second).join("doc-0000.bin"),
        b"corrupt second",
    )
    .unwrap();
    let third = checkpoint(&mut store, b"third", &[b"newest"]);
    fs::write(
        generation(&store, third).join("doc-0000.bin"),
        b"corrupt third",
    )
    .unwrap();
    let loaded = store.load_latest().unwrap().unwrap();
    assert_eq!(loaded.generation, first);
    assert!(loaded.used_backup);
    assert_eq!(contents(loaded).0, b"valid backup");
    assert!(store.prune_obsolete().is_err());
    assert!(generation(&store, second).exists());
    fs::write(
        generation(&store, first).join("doc-0000.bin"),
        b"corrupt both",
    )
    .unwrap();
    assert!(store
        .checkpoint(b"reject", 1, |_, _| panic!("must fail before writer"))
        .is_err());
    assert!(store.prune_obsolete().is_err());
    fixture.original_intact();
}

#[test]
fn recovery_gc_failed_initial_streams_leave_no_unpublished_payloads() {
    let fixture = Fixture::new();
    let mut store = fixture.store(Default::default());
    for _ in 0..4 {
        assert!(store
            .checkpoint(b"cancelled", 1, |_, writer| {
                writer.write_all(&[0x81; 65536])?;
                Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "cancelled fixture",
                ))
            })
            .is_err());
        assert!(!store.slot_path().join("HEAD").exists());
        assert!(!fs::read_dir(store.slot_path()).unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("generation-")));
    }
    let committed = checkpoint(&mut store, b"committed", &[b"preserved"]);
    assert_eq!(store.load_latest().unwrap().unwrap().generation, committed);
    fixture.original_intact();
}

#[test]
fn recovery_gc_reparse_candidate_never_follows_owned_outside_target() {
    use std::os::windows::process::CommandExt;
    let fixture = Fixture::new();
    let outside = Fixture::new();
    let mut store = fixture.store(Default::default());
    let ids: Vec<_> = (0..4)
        .map(|index| checkpoint(&mut store, &[index], &[b"source"]))
        .collect();
    let link = generation(&store, ids[1]).join("doc-0000.bin");
    fs::remove_file(&link).unwrap();
    let setup = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(&link)
        .arg(&outside.root)
        .creation_flags(0x0800_0000)
        .output()
        .unwrap();
    assert!(
        setup.status.success(),
        "owned junction setup failed: {:?} {:?}",
        setup.stdout,
        setup.stderr
    );
    // Remove exactly the owned junction before allowing fixture teardown.
    // No recursive operation is ever directed at this link or its target.
    let result = store.prune_obsolete();
    fs::remove_dir(&link).unwrap();
    assert!(result.is_err());
    assert!(generation(&store, ids[0]).join("doc-0000.bin").exists());
    outside.original_intact();
    fixture.original_intact();
    assert_eq!(store.prune_obsolete().unwrap().removed_generations, 2);
}
