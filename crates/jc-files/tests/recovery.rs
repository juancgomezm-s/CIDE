#![cfg(windows)]
//! Recovery-only owned fixtures. Original source files are never recovery targets.
use jc_files::recovery::{LoadedRecovery, RecoveryError, RecoveryLimits, RecoveryStore};
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
            "recovery-store-{}-{}-{}",
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
            .starts_with("recovery-store-"));
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
fn recovery_partial_writer_and_head_failure_keep_confirmed_generation_and_originals() {
    let fixture = Fixture::new();
    let mut store = fixture.store(Default::default());
    assert!(store.load_latest().unwrap().is_none());
    let first = checkpoint(
        &mut store,
        b"views\x00\xff",
        &[b"\xef\xbb\xbfnew\r\n\x81", b"dirty\0\xff\r"],
    );
    let head = fs::read(store.slot_path().join("HEAD")).unwrap();
    let failed = store.checkpoint(b"not committed", 1, |_, writer| {
        writer.write_all(b"incomplete")?;
        Err(io::Error::new(io::ErrorKind::Interrupted, "cancel fixture"))
    });
    assert!(failed.is_err());
    assert_eq!(fs::read(store.slot_path().join("HEAD")).unwrap(), head);
    let loaded = store.load_latest().unwrap().unwrap();
    assert_eq!(loaded.generation, first);
    assert_eq!(
        contents(loaded),
        (
            b"views\0\xff".to_vec(),
            vec![
                b"\xef\xbb\xbfnew\r\n\x81".to_vec(),
                b"dirty\0\xff\r".to_vec()
            ],
            false
        )
    );
    // Deny replacing only this owned HEAD. Publishing a new generation must
    // report failure while preserving the previously committed HEAD verbatim.
    let guard = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(store.slot_path().join("HEAD"))
        .unwrap();
    assert!(store
        .checkpoint(b"publish fails", 1, |_, writer| writer
            .write_all(b"new data"))
        .is_err());
    assert_eq!(fs::read(store.slot_path().join("HEAD")).unwrap(), head);
    drop(guard);
    assert_eq!(store.load_latest().unwrap().unwrap().generation, first);
    assert!(generation(&store, first).join("manifest.bin").is_file());
    fixture.original_intact();
}

#[test]
fn recovery_corrupt_latest_document_and_head_fall_back_without_overwriting_prior_files() {
    let fixture = Fixture::new();
    let mut store = fixture.store(Default::default());
    let first = checkpoint(&mut store, b"first views", &[b"first\xff\r\n"]);
    let first_data = fs::read(generation(&store, first).join("doc-0000.bin")).unwrap();
    let second = checkpoint(&mut store, b"second views", &[b"second\x81\r\n"]);
    fs::write(
        generation(&store, second).join("doc-0000.bin"),
        b"CORRUPTED!",
    )
    .unwrap();
    let loaded = store.load_latest().unwrap().unwrap();
    assert_eq!(loaded.generation, first);
    assert_eq!(
        contents(loaded),
        (b"first views".to_vec(), vec![first_data.clone()], true)
    );
    fs::write(store.slot_path().join("HEAD"), b"truncated HEAD").unwrap();
    let slot = store.slot_path().to_owned();
    drop(store);
    let mut reopened = fixture.store(Default::default());
    assert_eq!(reopened.slot_path(), slot);
    let loaded = reopened.load_latest().unwrap().unwrap();
    assert_eq!(loaded.generation, first);
    assert!(contents(loaded).2);
    assert_eq!(
        fs::read(generation(&reopened, first).join("doc-0000.bin")).unwrap(),
        first_data
    );
    fixture.original_intact();
}

#[test]
fn recovery_truncated_metadata_hostile_counts_and_quotas_are_rejected_before_publication() {
    let fixture = Fixture::new();
    let limits = RecoveryLimits {
        max_documents: 2,
        max_metadata_bytes: 64,
        max_document_bytes: 128,
        max_total_bytes: 4096,
        ..Default::default()
    };
    let mut store = fixture.store(limits);
    assert!(store
        .checkpoint(b"meta", 3, |_, _| panic!("rejected before callback"))
        .is_err());
    assert!(store
        .checkpoint(&[0; 65], 0, |_, _| panic!("rejected before callback"))
        .is_err());
    assert!(!store.slot_path().join("HEAD").exists());
    let first = checkpoint(&mut store, b"small metadata", &[b"source"]);
    let head = fs::read(store.slot_path().join("HEAD")).unwrap();
    assert!(store
        .checkpoint(b"too big", 1, |_, writer| {
            let _ = writer.write_all(&[7; 129]);
            Ok(())
        })
        .is_err());
    assert_eq!(fs::read(store.slot_path().join("HEAD")).unwrap(), head);
    fs::write(generation(&store, first).join("metadata.bin"), b"short").unwrap();
    assert!(store.load_latest().is_err());
    fs::write(store.slot_path().join("HEAD"), vec![0; 4096]).unwrap();
    assert!(store.load_latest().is_err());
    for suffix in [
        "sessions/../outside",
        r"sessions\..\outside",
        "sessions/./outside",
    ] {
        let mut raw = fixture.root.as_os_str().to_os_string();
        raw.push(r"\");
        raw.push(suffix);
        assert!(
            RecoveryStore::open(&PathBuf::from(raw), limits).is_err(),
            "raw traversal spelling must be rejected: {suffix}"
        );
    }
    assert!(!fixture.root.join("outside").exists());
    fixture.original_intact();
}

#[test]
fn recovery_concurrent_slots_are_exclusive_and_reopen_chooses_latest_free_confirmed_slot() {
    let fixture = Fixture::new();
    let mut first = fixture.store(Default::default());
    checkpoint(&mut first, b"window one", &[b"first unsaved"]);
    let first_slot = first.slot_path().to_owned();
    let head = fs::read(first_slot.join("HEAD")).unwrap();
    let mut second = fixture.store(Default::default());
    assert_ne!(first.slot_path(), second.slot_path());
    checkpoint(&mut second, b"window two", &[b"second unsaved"]);
    let second_slot = second.slot_path().to_owned();
    assert_eq!(fs::read(first_slot.join("HEAD")).unwrap(), head);
    drop(first);
    let mut third = fixture.store(Default::default());
    assert_eq!(third.slot_path(), first_slot);
    assert_eq!(
        contents(third.load_latest().unwrap().unwrap()).0,
        b"window one"
    );
    drop(third);
    drop(second);
    let mut latest = fixture.store(Default::default());
    assert_eq!(latest.slot_path(), second_slot);
    let loaded = latest.load_latest().unwrap().unwrap();
    assert_eq!(
        contents(loaded),
        (
            b"window two".to_vec(),
            vec![b"second unsaved".to_vec()],
            false
        )
    );
    fixture.original_intact();
}

#[test]
fn recovery_read_handles_are_immutable_and_retained_generation_quota_never_deletes_good_data() {
    let fixture = Fixture::new();
    let limits = RecoveryLimits {
        max_documents: 1,
        max_metadata_bytes: 128,
        max_document_bytes: 128 * 1024,
        max_total_bytes: 300 * 1024,
        ..Default::default()
    };
    let mut store = fixture.store(limits);
    let chunk = [0x81; 64 * 1024];
    let first = store
        .checkpoint(b"streamed", 1, |_, writer| {
            // The core codec emits many small integer fields. The transport
            // must buffer them without changing its bytes or fingerprint.
            for _ in 0..(128 * 1024 / 8) {
                writer.write_all(&[0x81; 8])?;
            }
            Ok(())
        })
        .unwrap();
    let loaded = store.load_latest().unwrap().unwrap();
    assert!(OpenOptions::new()
        .write(true)
        .open(generation(&store, first).join("doc-0000.bin"))
        .is_err());
    assert_eq!(contents(loaded).1[0].len(), 128 * 1024);
    let second = store
        .checkpoint(b"streamed twice", 1, |_, writer| {
            writer.write_all(&chunk)?;
            writer.write_all(&chunk)
        })
        .unwrap();
    let head = fs::read(store.slot_path().join("HEAD")).unwrap();
    assert!(matches!(
        store.checkpoint(b"quota exceeded", 1, |_, writer| writer.write_all(&chunk)),
        Err(RecoveryError::Io(_)) | Err(RecoveryError::Limit(_))
    ));
    assert_eq!(fs::read(store.slot_path().join("HEAD")).unwrap(), head);
    assert!(generation(&store, first).join("doc-0000.bin").is_file());
    assert_eq!(store.load_latest().unwrap().unwrap().generation, second);
    fixture.original_intact();
}
