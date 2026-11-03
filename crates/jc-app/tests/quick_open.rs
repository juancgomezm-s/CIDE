//! Quick Open observes metadata and retains original paths, never file content.
use jc_app::quick_open::{
    QuickOpenAcceptError, QuickOpenController, QuickOpenLimit, QuickOpenLimits,
    QuickOpenRequestError, QuickOpenState,
};
use jc_app::Wakeup;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    base: PathBuf,
    root: PathBuf,
}
fn is_link(path: &Path) -> bool {
    let metadata = fs::symlink_metadata(path).unwrap();
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
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
        for part in ["work", "test-temp"] {
            base.push(part);
            match fs::create_dir(&base) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("fixture directory: {error}"),
            }
            assert!(!is_link(&base));
            base = base.canonicalize().unwrap();
            assert!(base.starts_with(&workspace));
        }
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = base.join(format!(
            "quick-open-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self {
            base,
            root: root.canonicalize().unwrap(),
        }
    }
    fn put(&self, name: impl AsRef<Path>, bytes: &[u8]) -> PathBuf {
        let path = self.root.join(name);
        assert!(path.starts_with(&self.root));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap()
            .write_all(bytes)
            .unwrap();
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        assert!(!is_link(&self.root));
        assert_eq!(self.root.canonicalize().unwrap(), self.root);
        assert_eq!(self.root.parent(), Some(self.base.as_path()));
        assert!(self
            .root
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("quick-open-"));
        fs::remove_dir_all(&self.root).unwrap();
    }
}
fn settle(controller: &mut QuickOpenController, wake: &Wakeup, root: &Path, query: &str) {
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        let ticket = wake.ticket();
        controller.poll(root, query);
        if !matches!(
            controller.state(),
            QuickOpenState::Queued | QuickOpenState::Running
        ) {
            break;
        }
        assert!(Instant::now() < end, "Quick Open delivery did not finish");
        wake.wait(ticket, Some(Duration::from_millis(10)));
    }
}
#[test]
fn unicode_path_substring_uses_metadata_and_safe_labels_not_content() {
    let fixture = Fixture::new();
    let bytes = [0xff, 0xfe, 0, b'\r', b'\n'];
    let target = fixture.put("nested/Straße-Σς.bin", &bytes);
    fixture.put(".git/hidden.bin", b"ignored");
    fixture.put("other.bin", b"strasse");
    let wake = Wakeup::default();
    let mut controller = QuickOpenController::new(wake.clone());
    for query in ["NESTED/STRASSE-ΣΣ", "nested\\strasse-σσ"] {
        controller
            .request(fixture.root.clone(), query.into())
            .unwrap();
        settle(&mut controller, &wake, &fixture.root, query);
        assert_eq!(controller.state(), QuickOpenState::Complete);
        assert_eq!(controller.results().len(), 1);
        assert_eq!(controller.results()[0].ordinal(), 0);
        assert_eq!(controller.results()[0].path(), target);
        assert!(controller.results()[0].label().contains("Straße"));
        assert_eq!(
            controller.selected_target(&fixture.root, query, 0).unwrap(),
            target.canonicalize().unwrap()
        );
    }
    controller
        .request(fixture.root.clone(), String::new())
        .unwrap();
    settle(&mut controller, &wake, &fixture.root, "");
    assert_eq!(controller.results().len(), 2);
    assert_eq!(controller.counters().excluded_directories, 1);
    assert_eq!(fs::read(target).unwrap(), bytes);
    assert!(controller.counters().owned_bytes <= QuickOpenLimits::default().max_owned_bytes);
}
#[test]
fn stale_cancel_latest_query_and_pending_completion_never_open_old_target() {
    let fixture = Fixture::new();
    fixture.put("old.txt", b"x");
    let latest = fixture.put("latest.txt", b"y");
    let wake = Wakeup::default();
    let mut controller = QuickOpenController::new(wake.clone());
    controller
        .request(fixture.root.clone(), "old".into())
        .unwrap();
    controller
        .request(fixture.root.clone(), "latest".into())
        .unwrap();
    settle(&mut controller, &wake, &fixture.root, "latest");
    assert_eq!(controller.results().len(), 1);
    assert_eq!(controller.results()[0].path(), latest);
    assert_eq!(
        controller.selected_target(&fixture.root, "old", 0),
        Err(QuickOpenAcceptError::Stale)
    );
    assert!(controller.poll(&fixture.root, "changed"));
    assert_eq!(controller.state(), QuickOpenState::Stale);
    assert!(controller.results().is_empty());
    controller
        .request(fixture.root.clone(), String::new())
        .unwrap();
    controller.cancel();
    assert_eq!(controller.state(), QuickOpenState::Cancelled);
    assert!(controller.results().is_empty());
    controller
        .request(fixture.root.clone(), "latest".into())
        .unwrap();
    settle(&mut controller, &wake, &fixture.root, "latest");
    let changed_root = fixture.root.join("nested");
    assert!(controller.poll(&changed_root, "latest"));
    assert!(controller.results().is_empty());
    controller.clear();
    assert_eq!(controller.state(), QuickOpenState::Idle);
    controller
        .request(fixture.root.clone(), "old".into())
        .unwrap();
    let end = Instant::now() + Duration::from_secs(5);
    while !controller.ready() {
        let ticket = wake.ticket();
        assert!(Instant::now() < end);
        wake.wait(ticket, Some(Duration::from_millis(10)));
    }
    // Drop joins even when the completion has never been acknowledged by poll.
    drop(controller);
}
#[test]
fn quotas_are_partial_exact_results_are_complete_and_expiry_is_typed() {
    let fixture = Fixture::new();
    fixture.put("one.txt", b"x");
    fixture.put("two.txt", b"y");
    let wake = Wakeup::default();
    let mut limits = QuickOpenLimits {
        max_results: 1,
        ..QuickOpenLimits::default()
    };
    let mut controller = QuickOpenController::with_limits(wake.clone(), limits).unwrap();
    controller
        .request(fixture.root.clone(), String::new())
        .unwrap();
    settle(&mut controller, &wake, &fixture.root, "");
    assert_eq!(controller.state(), QuickOpenState::Limited);
    assert_eq!(controller.limitation(), Some(QuickOpenLimit::Results));
    assert_eq!(controller.results().len(), 1);
    assert!(controller.selected_target(&fixture.root, "", 0).is_ok());
    drop(controller);
    limits.max_results = 2;
    let mut controller = QuickOpenController::with_limits(wake.clone(), limits).unwrap();
    controller
        .request(fixture.root.clone(), String::new())
        .unwrap();
    settle(&mut controller, &wake, &fixture.root, "");
    assert_eq!(controller.state(), QuickOpenState::Complete);
    drop(controller);
    limits.max_entries = 1;
    let mut controller = QuickOpenController::with_limits(wake.clone(), limits).unwrap();
    controller
        .request(fixture.root.clone(), String::new())
        .unwrap();
    settle(&mut controller, &wake, &fixture.root, "");
    assert_eq!(controller.limitation(), Some(QuickOpenLimit::Entries));
    drop(controller);
    limits.timeout = Duration::from_nanos(1);
    let mut controller = QuickOpenController::with_limits(wake.clone(), limits).unwrap();
    controller
        .request(fixture.root.clone(), String::new())
        .unwrap();
    settle(&mut controller, &wake, &fixture.root, "");
    assert_eq!(controller.state(), QuickOpenState::TimedOut);
    assert!(controller.results().is_empty());
    assert_eq!(
        controller.request(PathBuf::from("relative"), String::new()),
        Err(QuickOpenRequestError::InvalidRoot)
    );
    assert_eq!(
        controller.request(fixture.root.clone(), "x".repeat(257)),
        Err(QuickOpenRequestError::InvalidQuery)
    );
}
#[test]
fn directory_depth_and_owned_capacity_limits_are_visible_and_accept_rechecks_type() {
    let fixture = Fixture::new();
    fixture.put("deep/child.txt", b"x");
    let target = fixture.put("target.txt", b"y");
    let wake = Wakeup::default();
    let limits = QuickOpenLimits {
        max_depth: 1,
        ..QuickOpenLimits::default()
    };
    let mut controller = QuickOpenController::with_limits(wake.clone(), limits).unwrap();
    controller
        .request(fixture.root.clone(), String::new())
        .unwrap();
    settle(&mut controller, &wake, &fixture.root, "");
    assert_eq!(controller.state(), QuickOpenState::Limited);
    assert_eq!(controller.limitation(), Some(QuickOpenLimit::Depth));
    assert_eq!(controller.results().len(), 1);
    fs::remove_file(&target).unwrap();
    fs::create_dir(&target).unwrap();
    assert_eq!(
        controller.selected_target(&fixture.root, "", 0),
        Err(QuickOpenAcceptError::NotFile)
    );
    drop(controller);
    let limits = QuickOpenLimits {
        max_owned_bytes: 256,
        ..QuickOpenLimits::default()
    };
    assert!(limits.valid());
    let mut controller = QuickOpenController::with_limits(wake.clone(), limits).unwrap();
    controller
        .request(fixture.root.clone(), String::new())
        .unwrap();
    settle(&mut controller, &wake, &fixture.root, "");
    assert_eq!(controller.limitation(), Some(QuickOpenLimit::OwnedBytes));
    assert!(controller.counters().owned_bytes <= limits.max_owned_bytes);
}
#[cfg(unix)]
#[test]
fn non_unicode_paths_are_opaque_empty_query_candidates_and_not_lossy_matches() {
    use std::os::unix::ffi::OsStringExt;
    let fixture = Fixture::new();
    let name = std::ffi::OsString::from_vec(b"opaque-\xff.txt".to_vec());
    let target = fixture.put(Path::new(&name), b"x");
    let wake = Wakeup::default();
    let mut controller = QuickOpenController::new(wake.clone());
    controller
        .request(fixture.root.clone(), String::new())
        .unwrap();
    settle(&mut controller, &wake, &fixture.root, "");
    assert_eq!(controller.results()[0].path(), target);
    assert!(controller.results()[0].label().contains("\\xFF"));
    controller
        .request(fixture.root.clone(), "opaque".into())
        .unwrap();
    settle(&mut controller, &wake, &fixture.root, "opaque");
    assert_eq!(controller.state(), QuickOpenState::Limited);
    assert_eq!(controller.counters().skipped_non_unicode, 1);
    assert!(controller.results().is_empty());
}
#[cfg(any(windows, unix))]
#[test]
fn links_are_excluded_and_link_replacement_is_rejected_before_accept() {
    let fixture = Fixture::new();
    let outside = Fixture::new();
    let external = outside.put("outside.txt", b"external");
    let target = fixture.put("target.txt", b"source");
    let link = fixture.root.join("alias.txt");
    fixture_link(&external, &outside.root, &link);
    let wake = Wakeup::default();
    let mut controller = QuickOpenController::new(wake.clone());
    controller
        .request(fixture.root.clone(), String::new())
        .unwrap();
    settle(&mut controller, &wake, &fixture.root, "");
    assert_eq!(controller.results().len(), 1);
    assert_eq!(controller.counters().skipped_links, 1);
    fs::remove_file(&target).unwrap();
    fixture_link(&external, &outside.root, &target);
    assert_eq!(
        controller.selected_target(&fixture.root, "", 0),
        Err(QuickOpenAcceptError::Link)
    );
    assert_eq!(fs::read(external).unwrap(), b"external");
    #[cfg(windows)]
    {
        fs::remove_dir(&link).unwrap();
        fs::remove_dir(&target).unwrap();
    }
}

#[cfg(windows)]
fn fixture_link(_: &Path, outside_dir: &Path, link: &Path) {
    use std::os::windows::process::CommandExt;
    assert!(std::process::Command::new("cmd.exe")
        .raw_arg("/D /V:OFF /C mklink /J \"%JC_TEST_LINK%\" \"%JC_TEST_TARGET%\"")
        .env("JC_TEST_LINK", link)
        .env("JC_TEST_TARGET", outside_dir)
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW; owned fixture only.
        .output()
        .unwrap()
        .status
        .success());
}
#[cfg(unix)]
fn fixture_link(external: &Path, _: &Path, link: &Path) {
    std::os::unix::fs::symlink(external, link).unwrap();
}
