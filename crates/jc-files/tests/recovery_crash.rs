#![cfg(windows)]
use jc_files::recovery::RecoveryStore;
use std::{fs, io::Read, process::Command};

#[test]
fn abrupt_process_exit_during_write_keeps_previous_commit_and_after_commit_keeps_new_data() {
    let root = std::env::current_dir()
        .unwrap()
        .join("../../work/recovery-crash")
        .join(format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    // Normalize the test's construction before admitting it to the depot.
    fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let mut store = RecoveryStore::open(&root, Default::default()).unwrap();
    store
        .checkpoint(b"confirmed", 1, |_, w| {
            w.write_all(b"previous complete bytes")
        })
        .unwrap();
    drop(store);
    for stage in ["partial", "committed"] {
        let status = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "crash_writer_child", "--ignored", "--nocapture"])
            .env("CIDE_CRASH_TEST_ROOT", &root)
            .env("CIDE_CRASH_TEST_STAGE", stage)
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(86));
        let mut store = RecoveryStore::open(&root, Default::default()).unwrap();
        let loaded = store.load_latest().unwrap().unwrap();
        let mut bytes = Vec::new();
        (&loaded.documents[0]).read_to_end(&mut bytes).unwrap();
        if stage == "partial" {
            assert_eq!(loaded.metadata, b"confirmed");
            assert_eq!(bytes, b"previous complete bytes");
        } else {
            assert_eq!(loaded.metadata, b"new confirmed");
            assert_eq!(bytes, b"new complete bytes");
        }
    }
}

#[test]
#[ignore = "Only invoked by its parent with an owned fixture directory"]
fn crash_writer_child() {
    let root = std::path::PathBuf::from(
        std::env::var_os("CIDE_CRASH_TEST_ROOT").expect("parent-owned fixture"),
    );
    let mut store = RecoveryStore::open(&root, Default::default()).unwrap();
    if std::env::var("CIDE_CRASH_TEST_STAGE").unwrap() == "partial" {
        let _ = store.checkpoint(b"incomplete", 1, |_, writer| {
            writer.write_all(&[0xa5; 128 * 1024])?;
            std::process::exit(86);
        });
        panic!("child must exit during the write");
    }
    store
        .checkpoint(b"new confirmed", 1, |_, writer| {
            writer.write_all(b"new complete bytes")
        })
        .unwrap();
    std::process::exit(86);
}
