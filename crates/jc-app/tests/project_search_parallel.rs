//! Observable repository-search contracts shared by serial and parallel modes.
//! No timing/speedup assertions and no inspection of helper/channel internals.
use jc_app::{
    CommandId, ProjectSearch, ProjectSearchOptions, ProjectSearchRow, ProjectSearchState,
    SearchOrigin, Wakeup, Workbench,
};
use jc_text::{encode, TextEncoding, TextPosition};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    ops::Range,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
const BLOCK: usize = 64 * 1024;

struct Fixture {
    workspace: PathBuf,
    base: PathBuf,
    root: PathBuf,
    files: Vec<PathBuf>,
}
fn reparse(path: &Path) -> bool {
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
        let work = workspace.join("work");
        if !work.exists() {
            match fs::create_dir(&work) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("create fixture work directory: {error}"),
            }
        }
        assert!(!reparse(&work));
        let work = work.canonicalize().unwrap();
        assert!(work.starts_with(&workspace));
        let base = work.join("test-temp");
        if !base.exists() {
            match fs::create_dir(&base) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("create fixture base: {error}"),
            }
        }
        assert!(!reparse(&base));
        let base = base.canonicalize().unwrap();
        assert!(base.starts_with(&workspace));
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = base.join(format!(
            "project-search-parallel-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        Self {
            workspace,
            base,
            root,
            files: Vec::new(),
        }
    }

    fn create(&mut self, name: &str) -> (PathBuf, fs::File) {
        assert!(Path::new(name).components().count() == 1);
        let path = self.root.join(name);
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        self.files.push(path.clone());
        (path, file)
    }

    fn put(&mut self, name: &str, bytes: &[u8]) -> PathBuf {
        let (path, mut file) = self.create(name);
        for chunk in bytes.chunks(BLOCK) {
            file.write_all(chunk).unwrap();
        }
        path
    }

    fn repeat(&mut self, name: &str, unit: &[u8], mut count: usize) -> PathBuf {
        assert!(!unit.is_empty() && unit.len() <= BLOCK);
        let (path, mut file) = self.create(name);
        let units = BLOCK / unit.len();
        let block = unit.repeat(units);
        while count > 0 {
            let take = count.min(units);
            file.write_all(&block[..take * unit.len()]).unwrap();
            count -= take;
        }
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // No recursive removal; every file was created with create_new. Check
        // resolved ownership again before deleting any of these exact paths.
        let root = self.root.canonicalize().unwrap();
        assert_eq!(root, self.root);
        assert_eq!(root.parent(), Some(self.base.as_path()));
        assert!(self
            .base
            .canonicalize()
            .unwrap()
            .starts_with(&self.workspace));
        assert!(!reparse(&root));
        for path in &self.files {
            assert!(!reparse(path));
            assert!(path.canonicalize().unwrap().starts_with(&root));
            fs::remove_file(path).unwrap();
        }
        fs::remove_dir(root).unwrap();
    }
}

fn options(query: &str, workers: usize) -> ProjectSearchOptions {
    let mut options = ProjectSearchOptions::literal(query);
    options.workers = workers;
    options
}

fn finish(search: &mut ProjectSearch, app: &Workbench, wake: &Wakeup) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while search.state == ProjectSearchState::Running {
        let ticket = wake.ticket();
        search.poll(app, 4);
        assert!(
            Instant::now() < deadline,
            "search did not reach a terminal state"
        );
        if search.state == ProjectSearchState::Running && !search.ready() {
            wake.wait(
                ticket,
                Some(deadline.saturating_duration_since(Instant::now())),
            );
        }
    }
}

fn unacknowledged_stream(search: &ProjectSearch, wake: &Wakeup, initial: u64) {
    // A dense producer emits several notifications while its consumer never
    // acknowledges rows. This exercises bounded-output pressure without peeking
    // into a helper's channel or assuming a scheduling order between helpers.
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let ticket = wake.ticket();
        if search.ready() && ticket.wrapping_sub(initial) >= 4 {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "stream did not produce notifications"
        );
        wake.wait(
            ticket,
            Some(deadline.saturating_duration_since(Instant::now())),
        );
    }
}

fn wait_ready(search: &ProjectSearch, wake: &Wakeup) {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let ticket = wake.ticket();
        if search.ready() {
            return;
        }
        assert!(Instant::now() < deadline, "search did not publish a batch");
        wake.wait(
            ticket,
            Some(deadline.saturating_duration_since(Instant::now())),
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Row {
    Match {
        path: PathBuf,
        range: Range<usize>,
        position: TextPosition,
        encoding: TextEncoding,
        snippet: String,
        source_len: usize,
        origin: SearchOrigin,
    },
    Issue {
        path: PathBuf,
        message: String,
    },
}
fn rows(search: &ProjectSearch) -> Vec<Row> {
    search
        .rows()
        .iter()
        .map(|row| match row {
            ProjectSearchRow::Match(hit) => Row::Match {
                path: hit.path.clone(),
                range: hit.range.clone(),
                position: hit.position,
                encoding: hit.encoding,
                snippet: hit.snippet.clone(),
                source_len: hit.source_len,
                origin: hit.origin,
            },
            ProjectSearchRow::Issue { path, message } => Row::Issue {
                path: path.clone(),
                message: message.clone(),
            },
        })
        .collect()
}

fn complete(app: &Workbench, root: &Path, options: ProjectSearchOptions) -> Vec<Row> {
    let wake = Wakeup::default();
    let mut search = ProjectSearch::new(wake.clone());
    search.start(app, root, options).unwrap();
    finish(&mut search, app, &wake);
    assert_eq!(search.state, ProjectSearchState::Complete);
    rows(&search)
}

#[test]
fn worker_counts_preserve_order_bytes_scalar_positions_encodings_and_notices() {
    let mut fixture = Fixture::new();
    let utf8 = "\u{feff}é\r\nStraße niño\r\nNIÑO".as_bytes();
    let legacy = encode("é\r\nStraße niño\r\nNIÑO", TextEncoding::Windows1252).unwrap();
    let utf8_path = fixture.put("utf8-0.p", utf8);
    let legacy_path = fixture.put("legacy-0.p", &legacy);
    for ordinal in 1..8 {
        fixture.put(&format!("utf8-{ordinal}.p"), "niño\rniño\n".as_bytes());
        fixture.put(
            &format!("legacy-{ordinal}.p"),
            &encode("niño\rniño\n", TextEncoding::Windows1252).unwrap(),
        );
    }
    fixture.put("utf8-binary.p", b"ni\0no");
    fixture.put("legacy-binary.p", b"ni\0no");
    let app = Workbench::default();
    // S1 ProjectSearch is case-sensitive (unlike the default S0 document
    // search): final NIÑO stays in the fixture as an explicit non-match.
    for (encoding, glob, checked_path, expected_range) in [
        (TextEncoding::Utf8, "utf8-*.p", &utf8_path, 15..20),
        (
            TextEncoding::Windows1252,
            "legacy-*.p",
            &legacy_path,
            10..14,
        ),
    ] {
        let mut reference = None;
        for workers in [1, 2, 4] {
            let mut request = options("niño", workers);
            request.encoding = Some(encoding);
            request.include = vec![glob.into()];
            let actual = complete(&app, &fixture.root, request);
            assert_eq!(
                actual
                    .iter()
                    .filter(|row| matches!(row, Row::Match { .. }))
                    .count(),
                15
            );
            assert_eq!(
                actual
                    .iter()
                    .filter(|row| matches!(row, Row::Issue { .. }))
                    .count(),
                1
            );
            let checked: Vec<_> = actual
                .iter()
                .filter_map(|row| match row {
                    Row::Match {
                        path,
                        range,
                        position,
                        encoding: actual_encoding,
                        ..
                    } if path == checked_path => {
                        assert_eq!(*actual_encoding, encoding);
                        Some((range.clone(), *position))
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(
                checked,
                vec![(expected_range.clone(), TextPosition { line: 1, column: 7 })]
            );
            if let Some(reference) = &reference {
                assert_eq!(&actual, reference);
            } else {
                reference = Some(actual);
            }
        }
    }
    assert_eq!(fs::read(utf8_path).unwrap(), utf8);
    assert_eq!(fs::read(legacy_path).unwrap(), legacy);
}

#[test]
fn result_and_scan_budgets_are_global_and_keep_the_serial_prefix() {
    let mut fixture = Fixture::new();
    let mut block = vec![b'x'; BLOCK];
    for offset in [0, 64, 128, 192] {
        block[offset..offset + 6].copy_from_slice(b"needle");
    }
    block[BLOCK - 2..].copy_from_slice(b"\r\n");
    for ordinal in 0..8 {
        fixture.put(&format!("source-{ordinal}.p"), &block);
    }
    fixture.put("zero.p", b"");
    let app = Workbench::default();
    let reference = complete(&app, &fixture.root, options("needle", 1));
    assert_eq!(reference.len(), 32);
    for workers in [1, 2, 4] {
        for scan_limited in [false, true] {
            let mut request = options("needle", workers);
            let expected = if scan_limited {
                // Source-byte quota is global; reader sentinels are not source.
                request.max_scan_bytes = 2 * BLOCK;
                8
            } else {
                request.max_results = 5;
                5
            };
            let wake = Wakeup::default();
            let mut search = ProjectSearch::new(wake.clone());
            search.start(&app, &fixture.root, request).unwrap();
            finish(&mut search, &app, &wake);
            assert_eq!(search.state, ProjectSearchState::Limited);
            assert_eq!(rows(&search), reference[..expected]);
        }

        for eligible in [2, 3] {
            let mut names: Vec<_> = (0..eligible)
                .map(|ordinal| format!("source-{ordinal}.p"))
                .collect();
            names.push("zero.p".into()); // Zero payload still fits an exhausted source quota.
            let expected: Vec<_> = reference
                .iter()
                .filter(|row| match row {
                    Row::Match { path, .. } => names
                        .iter()
                        .any(|name| path.file_name().unwrap() == name.as_str()),
                    _ => false,
                })
                .take(8)
                .cloned()
                .collect();
            let mut request = options("needle", workers);
            request.include = names;
            request.max_scan_bytes = 2 * BLOCK;
            let wake = Wakeup::default();
            let mut search = ProjectSearch::new(wake.clone());
            search.start(&app, &fixture.root, request).unwrap();
            finish(&mut search, &app, &wake);
            assert_eq!(
                search.state,
                if eligible == 2 {
                    ProjectSearchState::Complete
                } else {
                    ProjectSearchState::Limited
                }
            );
            assert_eq!(rows(&search), expected);
        }
    }
}

#[test]
fn dirty_legacy_buffer_precedes_disk_and_stale_rows_do_not_change_bytes_or_undo() {
    let mut fixture = Fixture::new();
    let disk_open = b"disk legacy\r\n";
    let open_path = fixture.put("open.p", disk_open);
    let disk_bytes = "\u{feff}niño disk\r\n".as_bytes();
    let disk_path = fixture.put("disk.p", disk_bytes);
    for workers in [1, 2, 4] {
        let mut app = Workbench::default();
        app.open_file(&open_path, Some(TextEncoding::Windows1252))
            .unwrap();
        app.insert_text("niño local\r\n").unwrap();
        let dirty = app.active_bytes().unwrap();
        let wake = Wakeup::default();
        let mut search = ProjectSearch::new(wake.clone());
        search
            .start(&app, &fixture.root, options("niño", workers))
            .unwrap();
        finish(&mut search, &app, &wake);
        assert_eq!(search.state, ProjectSearchState::Complete);
        assert_eq!(search.rows().len(), 2);
        match &search.rows()[0] {
            ProjectSearchRow::Match(hit) => {
                assert_eq!(hit.path, open_path);
                assert_eq!(hit.range, 0..4);
                assert_eq!(hit.encoding, TextEncoding::Windows1252);
                assert_eq!(hit.position, TextPosition { line: 0, column: 0 });
                assert!(matches!(hit.origin, SearchOrigin::Open(_)));
            }
            _ => panic!("open snapshot must be delivered first"),
        }
        match &search.rows()[1] {
            ProjectSearchRow::Match(hit) => {
                assert_eq!(hit.path, disk_path);
                assert_eq!(hit.range, 3..8);
                assert_eq!(hit.encoding, TextEncoding::Utf8);
                assert!(matches!(hit.origin, SearchOrigin::Disk(_)));
            }
            _ => panic!("expected disk match"),
        }
        assert_eq!(app.active_bytes().unwrap(), dirty);
        app.insert_text("changed").unwrap();
        assert!(search.select(0, &mut app).is_err());
        assert!(search.poll(&app, 4));
        assert!(search.is_stale());
        assert_eq!(search.rows().len(), 1);
        assert!(matches!(&search.rows()[0], ProjectSearchRow::Match(hit) if hit.path == disk_path));
        app.execute(CommandId::EditUndo).unwrap();
        assert_eq!(app.active_bytes().unwrap(), dirty);

        // Invalidate another captured revision before the consumer has accepted
        // even its first batch; arrival must not revive the old open-buffer row.
        search
            .start(&app, &fixture.root, options("niño", workers))
            .unwrap();
        wait_ready(&search, &wake);
        app.insert_text("newer").unwrap();
        finish(&mut search, &app, &wake);
        assert!(search.is_stale());
        assert_eq!(search.rows().len(), 1);
        assert!(matches!(&search.rows()[0], ProjectSearchRow::Match(hit) if hit.path == disk_path));
        app.execute(CommandId::EditUndo).unwrap();
        assert_eq!(app.active_bytes().unwrap(), dirty);
        app.execute(CommandId::EditUndo).unwrap();
        assert_eq!(app.active_bytes().unwrap(), disk_open);
        assert_eq!(fs::read(&open_path).unwrap(), disk_open);
        assert_eq!(fs::read(&disk_path).unwrap(), disk_bytes);
    }
}

#[test]
fn supersession_cancel_and_drop_release_unacknowledged_streams_for_all_worker_counts() {
    let mut fixture = Fixture::new();
    for ordinal in 0..8 {
        fixture.repeat(&format!("dense-{ordinal}.p"), b"a\n", BLOCK / 2);
    }
    let latest = fixture.put("latest.p", b"LATEST_TOKEN\r\n");
    let app = Workbench::default();
    for workers in [1, 2, 4] {
        let wake = Wakeup::default();
        let mut search = ProjectSearch::new(wake.clone());
        let ticket = wake.ticket();
        search
            .start(&app, &fixture.root, options("a", workers))
            .unwrap();
        unacknowledged_stream(&search, &wake, ticket);
        search
            .start(&app, &fixture.root, options("LATEST_TOKEN", workers))
            .unwrap();
        finish(&mut search, &app, &wake);
        assert_eq!(search.state, ProjectSearchState::Complete);
        assert_eq!(search.rows().len(), 1);
        assert!(
            matches!(&search.rows()[0], ProjectSearchRow::Match(hit) if hit.path == latest && hit.range == (0..12))
        );

        let ticket = wake.ticket();
        search
            .start(&app, &fixture.root, options("a", workers))
            .unwrap();
        unacknowledged_stream(&search, &wake, ticket);
        search.cancel();
        assert_eq!(search.state, ProjectSearchState::Cancelled);
        search.poll(&app, 4);
        assert!(search.rows().is_empty());
        let ticket = wake.ticket();
        search
            .start(&app, &fixture.root, options("a", workers))
            .unwrap();
        unacknowledged_stream(&search, &wake, ticket);
        drop(search); // Direct Drop must cancel/join without a prior cancel/poll.
    }
}

#[test]
fn changed_root_replaces_a_backpressured_generation_and_worker_bounds_are_admitted() {
    let mut old = Fixture::new();
    for ordinal in 0..6 {
        old.repeat(&format!("dense-{ordinal}.p"), b"a\n", BLOCK / 2);
    }
    let mut latest = Fixture::new();
    let path = latest.put("fresh.p", b"fresh\r\n");
    let app = Workbench::default();
    let wake = Wakeup::default();
    let mut search = ProjectSearch::new(wake.clone());
    for workers in [0, 5] {
        assert!(search
            .start(&app, &old.root, options("a", workers))
            .is_err());
        assert!(search.rows().is_empty());
    }
    let ticket = wake.ticket();
    search.start(&app, &old.root, options("a", 4)).unwrap();
    unacknowledged_stream(&search, &wake, ticket);
    search
        .start(&app, &latest.root, options("fresh", 2))
        .unwrap();
    finish(&mut search, &app, &wake);
    assert_eq!(search.root(), Some(latest.root.as_path()));
    assert_eq!(search.state, ProjectSearchState::Complete);
    assert_eq!(search.rows().len(), 1);
    assert!(
        matches!(&search.rows()[0], ProjectSearchRow::Match(hit) if hit.path == path && hit.range == (0..5))
    );
}
