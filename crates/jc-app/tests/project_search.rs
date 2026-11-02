use jc_app::*;
use jc_text::{encode, TextEncoding, TextPosition};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "jc-project-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }
    fn put(&self, path: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        assert!(self
            .0
            .starts_with(std::env::temp_dir().canonicalize().unwrap()));
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn finish(search: &mut ProjectSearch, app: &Workbench, wake: &Wakeup) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while search.state == ProjectSearchState::Running {
        let ticket = wake.ticket();
        search.poll(app, 4);
        assert!(Instant::now() < deadline, "search did not finish");
        if search.state == ProjectSearchState::Running && !search.ready() {
            wake.wait(ticket, Some(Duration::from_millis(100)));
        }
    }
}
fn hits(search: &ProjectSearch) -> Vec<&ProjectSearchMatch> {
    search
        .rows()
        .iter()
        .filter_map(|r| {
            if let ProjectSearchRow::Match(m) = r {
                Some(m)
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn replacement_preview_preserves_bytes_and_open_files_use_one_undo_without_save() {
    let fixture = Fixture::new();
    let open = fixture.put("open.p", b"ni\xf1o ni\xf1o\r\n");
    let disk = fixture.put("closed.p", "\u{feff}niño niño\r\n".as_bytes());
    let mut app = Workbench::default();
    app.open_file(&open, Some(TextEncoding::Iso8859_1)).unwrap();
    app.open_active_to_side();
    let wake = Wakeup::default();
    let mut search = ProjectSearch::new(wake.clone());
    search
        .start(&app, &fixture.0, ProjectSearchOptions::literal("niño"))
        .unwrap();
    finish(&mut search, &app, &wake);
    let mut preview = search.preview_replace(&app, "niña".into()).unwrap();
    assert_eq!(preview.files_remaining(), 2);
    assert_eq!(preview.matches(), 4);
    assert!(preview
        .lines()
        .iter()
        .any(|line| line.starts_with("+ niña")));
    assert_eq!(fs::read(&open).unwrap(), b"ni\xf1o ni\xf1o\r\n");
    while let Some(outcome) = search.apply_next_replace(&mut preview, &mut app) {
        assert!(outcome.error.is_none());
        assert_eq!(outcome.matches, 2);
    }
    assert_eq!(app.active_bytes().unwrap(), b"ni\xf1a ni\xf1a\r\n");
    assert!(app
        .active_document()
        .unwrap()
        .session
        .document()
        .is_modified());
    assert_eq!(fs::read(&open).unwrap(), b"ni\xf1o ni\xf1o\r\n");
    assert_eq!(fs::read(&disk).unwrap(), "\u{feff}niña niña\r\n".as_bytes());
    app.execute(CommandId::EditUndo).unwrap();
    assert_eq!(app.active_bytes().unwrap(), b"ni\xf1o ni\xf1o\r\n");
}

#[test]
fn replacement_revalidates_disk_open_revision_project_and_strict_encoding() {
    let fixture = Fixture::new();
    let disk = fixture.put("disk.p", b"a a\r\n");
    let opened = fixture.put("open.p", b"a a\r\n");
    let mut app = Workbench::default();
    app.open_file(&opened, Some(TextEncoding::Iso8859_1))
        .unwrap();
    let wake = Wakeup::default();
    let mut search = ProjectSearch::new(wake.clone());
    search
        .start(&app, &fixture.0, ProjectSearchOptions::literal("a"))
        .unwrap();
    finish(&mut search, &app, &wake);
    assert!(search.preview_replace(&app, "😀".into()).is_err());
    let mut preview = search.preview_replace(&app, "b".into()).unwrap();
    fs::write(&disk, b"external").unwrap();
    app.insert_text("local ").unwrap();
    let one = search.apply_next_replace(&mut preview, &mut app).unwrap();
    let two = search.apply_next_replace(&mut preview, &mut app).unwrap();
    assert!(one.error.is_some());
    assert!(two.error.is_some());
    assert_eq!(fs::read(&disk).unwrap(), b"external");
    assert_eq!(app.active_bytes().unwrap(), b"local a a\r\n");
    search
        .start(&app, &fixture.0, ProjectSearchOptions::literal("a"))
        .unwrap();
    finish(&mut search, &app, &wake);
    let mut preview = search.preview_replace(&app, "b".into()).unwrap();
    search.clear();
    assert!(search
        .apply_next_replace(&mut preview, &mut app)
        .unwrap()
        .error
        .is_some());
}

#[test]
fn replacement_skips_noops_and_rejects_files_opened_after_search() {
    let fixture = Fixture::new();
    let disk = fixture.put("disk.p", b"a a");
    let mut app = Workbench::default();
    let wake = Wakeup::default();
    let mut search = ProjectSearch::new(wake.clone());
    search
        .start(&app, &fixture.0, ProjectSearchOptions::literal("a"))
        .unwrap();
    finish(&mut search, &app, &wake);
    let mut preview = search.preview_replace(&app, "a".into()).unwrap();
    let outcome = search.apply_next_replace(&mut preview, &mut app).unwrap();
    assert_eq!(outcome.matches, 0);
    assert!(!outcome.saved_to_disk);
    assert!(outcome.error.is_none());
    let mut preview = search.preview_replace(&app, "b".into()).unwrap();
    app.open_file(&disk, Some(TextEncoding::Utf8)).unwrap();
    app.insert_text("dirty ").unwrap();
    assert!(search
        .apply_next_replace(&mut preview, &mut app)
        .unwrap()
        .error
        .is_some());
    assert_eq!(fs::read(disk).unwrap(), b"a a");
}

#[test]
fn replacement_rejects_a_directory_redirected_outside_root_after_preview() {
    let fixture = Fixture::new();
    let root = fixture.0.join("root");
    let dir = root.join("sub");
    let outside = fixture.0.join("outside");
    fs::create_dir_all(&dir).unwrap();
    fs::create_dir(&outside).unwrap();
    fs::write(dir.join("file.p"), b"a a").unwrap();
    fs::write(outside.join("file.p"), b"external").unwrap();
    let mut app = Workbench::default();
    let wake = Wakeup::default();
    let mut search = ProjectSearch::new(wake.clone());
    search
        .start(&app, &root, ProjectSearchOptions::literal("a"))
        .unwrap();
    finish(&mut search, &app, &wake);
    let mut preview = search.preview_replace(&app, "b".into()).unwrap();
    let backup = root.join("original");
    fs::rename(&dir, &backup).unwrap();
    create_directory_link(&outside, &dir);
    let outcome = search.apply_next_replace(&mut preview, &mut app).unwrap();
    assert!(outcome.error.is_some());
    assert_eq!(fs::read(outside.join("file.p")).unwrap(), b"external");
    assert_eq!(fs::read(backup.join("file.p")).unwrap(), b"a a");
    #[cfg(windows)]
    fs::remove_dir(&dir).unwrap();
    #[cfg(unix)]
    fs::remove_file(&dir).unwrap();
}

#[test]
fn recursive_globs_unicode_coordinates_notices_and_safe_navigation() {
    let fixture = Fixture::new();
    let path = fixture.put("sub/niño.p", "\u{feff}é\r\nniño\r\n".as_bytes());
    fixture.put("sub/unknown.p", b"ni\xf1o");
    fixture.put("binary.p", b"ni\0no");
    fixture.put("target/ignored.p", "niño".as_bytes());
    fixture.put("ignored.txt", "niño".as_bytes());
    let mut app = Workbench::default();
    let wake = Wakeup::default();
    let mut search = ProjectSearch::new(wake.clone());
    let mut options = ProjectSearchOptions::literal("niño");
    options.include = vec!["**/*.p".into()];
    search.start(&app, &fixture.0, options).unwrap();
    finish(&mut search, &app, &wake);
    assert_eq!(search.state, ProjectSearchState::Complete);
    let results = hits(&search);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].path, path);
    assert_eq!(results[0].range, 7..12);
    assert_eq!(results[0].position, TextPosition { line: 1, column: 0 });
    assert!(search.rows().iter().any(
        |r| matches!(r,ProjectSearchRow::Issue{message,..} if message.contains("Unknown legacy"))
    ));
    let row = search
        .rows()
        .iter()
        .position(|r| matches!(r, ProjectSearchRow::Match(_)))
        .unwrap();
    search.select(row, &mut app).unwrap();
    assert_eq!(
        app.active_position().unwrap(),
        TextPosition { line: 1, column: 0 }
    );
    fs::write(&path, b"changed").unwrap();
    assert!(search.select(row, &mut app).is_err());
}

#[test]
fn opened_dirty_snapshots_override_disk_and_stale_revisions_are_removed() {
    let fixture = Fixture::new();
    let path = fixture.put("source.p", b"disk");
    let mut app = Workbench::default();
    app.open_file(&path, Some(TextEncoding::Utf8)).unwrap();
    app.insert_text("niño").unwrap();
    let wake = Wakeup::default();
    let mut search = ProjectSearch::new(wake.clone());
    search
        .start(&app, &fixture.0, ProjectSearchOptions::literal("niño"))
        .unwrap();
    finish(&mut search, &app, &wake);
    assert_eq!(hits(&search).len(), 1);
    assert!(matches!(hits(&search)[0].origin, SearchOrigin::Open(_)));
    assert_eq!(fs::read(&path).unwrap(), b"disk");
    app.insert_text("changed").unwrap();
    assert!(search.select(0, &mut app).is_err());
    assert!(search.poll(&app, 4));
    assert!(hits(&search).is_empty());
    search
        .start(&app, &fixture.0, ProjectSearchOptions::literal("niño"))
        .unwrap();
    finish(&mut search, &app, &wake);
    assert_eq!(hits(&search).len(), 1);
    app.set_encoding(TextEncoding::Iso8859_1).unwrap();
    search.poll(&app, 4);
    assert!(hits(&search).is_empty());
}

#[test]
fn explicit_legacy_encoding_preserves_byte_ranges_and_read_only_files() {
    for encoding in [
        TextEncoding::Windows1252,
        TextEncoding::Iso8859_1,
        TextEncoding::Ibm850,
    ] {
        let fixture = Fixture::new();
        let bytes = encode("á\r\nniño", encoding).unwrap();
        let path = fixture.put("legacy.p", &bytes);
        let app = Workbench::default();
        let wake = Wakeup::default();
        let mut search = ProjectSearch::new(wake.clone());
        let mut options = ProjectSearchOptions::literal("niño");
        options.encoding = Some(encoding);
        search.start(&app, &fixture.0, options).unwrap();
        finish(&mut search, &app, &wake);
        let results = hits(&search);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].range, 3..7);
        assert_eq!(results[0].position, TextPosition { line: 1, column: 0 });
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
}

#[test]
fn limits_generations_and_cancellation_release_a_full_result_queue() {
    let fixture = Fixture::new();
    fixture.put("many.p", &b"a\n".repeat(10000));
    fixture.put("latest.p", b"new query");
    let app = Workbench::default();
    let wake = Wakeup::default();
    let mut search = ProjectSearch::new(wake.clone());
    search
        .start(&app, &fixture.0, ProjectSearchOptions::literal("a"))
        .unwrap();
    let ticket = wake.ticket();
    if !search.ready() {
        wake.wait(ticket, Some(Duration::from_secs(1)));
    }
    assert!(search.ready());
    search
        .start(&app, &fixture.0, ProjectSearchOptions::literal("new query"))
        .unwrap();
    finish(&mut search, &app, &wake);
    assert_eq!(hits(&search).len(), 1);
    assert_eq!(hits(&search)[0].snippet, "new query");
    let mut options = ProjectSearchOptions::literal("a");
    options.max_results = 3;
    search.start(&app, &fixture.0, options).unwrap();
    finish(&mut search, &app, &wake);
    assert_eq!(search.state, ProjectSearchState::Limited);
    assert_eq!(search.rows().len(), 3);
    search
        .start(&app, &fixture.0, ProjectSearchOptions::literal("a"))
        .unwrap();
    let ticket = wake.ticket();
    if !search.ready() {
        wake.wait(ticket, Some(Duration::from_secs(1)));
    }
    search.cancel();
    assert_eq!(search.state, ProjectSearchState::Cancelled);
    drop(search); // joins even under producer backpressure
}

#[test]
fn file_and_snapshot_budgets_are_explicit_and_crlf_interiors_never_match() {
    let fixture = Fixture::new();
    let path = fixture.put("source.p", b"a\r\nb");
    let mut app = Workbench::default();
    let wake = Wakeup::default();
    let mut search = ProjectSearch::new(wake.clone());
    search
        .start(&app, &fixture.0, ProjectSearchOptions::literal("\n"))
        .unwrap();
    finish(&mut search, &app, &wake);
    assert!(hits(&search).is_empty());
    search
        .start(&app, &fixture.0, ProjectSearchOptions::literal("\r\n"))
        .unwrap();
    finish(&mut search, &app, &wake);
    assert_eq!(hits(&search)[0].range, 1..3);
    let mut options = ProjectSearchOptions::literal("a");
    options.max_file_bytes = 2;
    search.start(&app, &fixture.0, options.clone()).unwrap();
    finish(&mut search, &app, &wake);
    assert!(search
        .rows()
        .iter()
        .any(|r| matches!(r, ProjectSearchRow::Issue { .. })));
    app.open_file(path, Some(TextEncoding::Utf8)).unwrap();
    assert!(search
        .start(&app, &fixture.0, options)
        .unwrap_err()
        .contains("snapshot budget"));
}

#[test]
fn links_and_junctions_do_not_expose_files_outside_root() {
    let fixture = Fixture::new();
    let root = fixture.0.join("root");
    let outside = fixture.0.join("outside");
    fs::create_dir(&root).unwrap();
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("secret.p"), b"secret").unwrap();
    let link = root.join("link");
    create_directory_link(&outside, &link);
    let app = Workbench::default();
    let wake = Wakeup::default();
    let mut search = ProjectSearch::new(wake.clone());
    search
        .start(&app, &root, ProjectSearchOptions::literal("secret"))
        .unwrap();
    finish(&mut search, &app, &wake);
    assert!(hits(&search).is_empty());
    assert!(search.rows().iter().any(
        |r| matches!(r,ProjectSearchRow::Issue{message,..} if message.contains("Link/reparse"))
    ));
    #[cfg(windows)]
    fs::remove_dir(link).unwrap(); // remove junction itself before fixture cleanup
    #[cfg(unix)]
    fs::remove_file(link).unwrap();
}
#[cfg(windows)]
fn create_directory_link(target: &Path, link: &Path) {
    assert!(std::process::Command::new("powershell.exe").args(["-NoProfile","-NonInteractive","-Command","New-Item -ItemType Junction -Path $env:JC_TEST_LINK -Target $env:JC_TEST_TARGET | Out-Null"])
        .env("JC_TEST_LINK",link).env("JC_TEST_TARGET",target).status().unwrap().success());
}
#[cfg(unix)]
fn create_directory_link(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}
