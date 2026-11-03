//! Real owned recovery files; scheduling uses virtual time and worker progress
//! uses the same bounded Wakeup transport as the frontends.
#![cfg(windows)]
use jc_app::{
    session::{EditorRecovery, PeriodicRecovery, RecoveryPolicy},
    CaptureChunk, CommandId, PaneId, Wakeup, Workbench,
};
use jc_text::TextEncoding;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    root: PathBuf,
    parent: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        use std::os::windows::fs::MetadataExt;
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .canonicalize()
            .unwrap();
        let parent = workspace.join("work/test-temp").canonicalize().unwrap();
        assert!(parent.starts_with(&workspace));
        assert!(fs::symlink_metadata(&parent)
            .is_ok_and(|m| m.is_dir() && m.file_attributes() & 0x400 == 0));
        let root = parent.join(format!(
            "periodic-recovery-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self { root, parent }
    }
    fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }
    fn recovery(&self) -> Option<EditorRecovery> {
        let mut recovery = EditorRecovery::open_in(&self.path("recovery")).unwrap();
        recovery.base = self.root.clone();
        Some(recovery)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        use std::os::windows::fs::MetadataExt;
        if self.root.parent() == Some(self.parent.as_path())
            && fs::symlink_metadata(&self.root)
                .is_ok_and(|m| m.is_dir() && m.file_attributes() & 0x400 == 0)
            && self
                .root
                .canonicalize()
                .is_ok_and(|p| p.starts_with(&self.parent))
        {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}
fn policy() -> RecoveryPolicy {
    RecoveryPolicy {
        idle: Duration::from_millis(20),
        maximum_delay: Duration::from_millis(100),
        retry: Duration::from_millis(70),
    }
}
fn head_path(recovery: &Option<EditorRecovery>) -> PathBuf {
    recovery.as_ref().unwrap().store.slot_path().join("HEAD")
}
fn drain(
    controller: &mut PeriodicRecovery,
    recovery: &mut Option<EditorRecovery>,
    bench: &Workbench,
    frontend: &[u8],
    now: Instant,
    wake: &Wakeup,
) -> Vec<String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut errors = Vec::new();
    while controller.active() {
        assert!(
            Instant::now() < deadline,
            "El trabajador no devolvió la concesión en cinco segundos"
        );
        let ticket = wake.ticket();
        if let Some(error) = controller.poll(recovery, bench, frontend, now, wake) {
            errors.push(error);
        }
        if controller.active() {
            wake.wait(ticket, Some(Duration::from_millis(2)));
        }
    }
    assert!(recovery.is_some());
    errors
}
fn start(
    controller: &mut PeriodicRecovery,
    recovery: &mut Option<EditorRecovery>,
    bench: &Workbench,
    frontend: &[u8],
    now: Instant,
    wake: &Wakeup,
) {
    assert!(controller.needs_capture(now));
    assert!(controller
        .poll(recovery, bench, frontend, now, wake)
        .is_none());
    assert!(controller.active() && recovery.is_none());
}
fn signature(
    bench: &Workbench,
    base: &Path,
) -> Vec<(
    String,
    bool,
    Vec<u8>,
    usize,
    usize,
    jc_files::SessionRecoveryInfo,
)> {
    bench
        .tabs()
        .tabs()
        .iter()
        .map(|tab| {
            let session = &bench.document_by_tab(tab.id).unwrap().session;
            let document = session.document();
            let mut info = session.recovery_info();
            if info.path.is_relative() {
                info.path = base.join(info.path);
            }
            (
                tab.title.clone(),
                tab.dirty,
                document.to_bytes(),
                document.undo_steps(),
                document.redo_steps(),
                info,
            )
        })
        .collect()
}
fn view(
    bench: &Workbench,
    pane: PaneId,
) -> Option<(usize, Option<usize>, usize, usize, bool, bool)> {
    let view = bench.panes().view(pane)?;
    let state = view.state()?;
    Some((
        state.cursor,
        state.anchor,
        state.scroll_line,
        state.scroll_column,
        state.manual_scroll,
        view.is_pinned(),
    ))
}

#[test]
fn periodic_deadlines_use_idle_maximum_delay_and_validated_policy() {
    let now = Instant::now();
    let mut controller = PeriodicRecovery::new(policy(), now).unwrap();
    assert_eq!(controller.deadline(), Some(now + policy().idle));
    assert!(!controller.needs_capture(now));
    for milliseconds in [15, 30, 45, 60, 75, 90] {
        controller.note_activity(now + Duration::from_millis(milliseconds));
    }
    assert_eq!(controller.deadline(), Some(now + policy().maximum_delay));
    assert!(!controller.needs_capture(now + Duration::from_millis(99)));
    assert!(controller.needs_capture(now + Duration::from_millis(100)));
    controller.defer_failure(now + Duration::from_millis(100));
    assert!(!controller.needs_capture(now + Duration::from_millis(100)));
    assert_eq!(
        controller.deadline(),
        Some(now + Duration::from_millis(100) + policy().retry)
    );
    for bad in [
        RecoveryPolicy {
            idle: Duration::ZERO,
            ..policy()
        },
        RecoveryPolicy {
            maximum_delay: Duration::from_millis(1),
            ..policy()
        },
        RecoveryPolicy {
            retry: Duration::ZERO,
            ..policy()
        },
    ] {
        assert!(PeriodicRecovery::new(bad, now).is_err());
    }
}

#[test]
fn recovery_key_does_not_reuse_identical_tab_revision_metadata_from_another_workbench() {
    let fixture = Fixture::new();
    let mut first = Workbench::default();
    first.new_file(fixture.path("mismo.txt"), TextEncoding::Utf8);
    first.insert_text("A").unwrap();
    let mut second = Workbench::default();
    second.new_file(fixture.path("mismo.txt"), TextEncoding::Utf8);
    second.insert_text("B").unwrap();
    assert_eq!(first.tabs().active_id(), second.tabs().active_id());
    assert_eq!(
        first
            .active_document()
            .unwrap()
            .session
            .document()
            .revision(),
        second
            .active_document()
            .unwrap()
            .session
            .document()
            .revision()
    );
    let a = first
        .begin_recovery_capture(&fixture.root, b"same UI")
        .unwrap();
    let b = second
        .begin_recovery_capture(&fixture.root, b"same UI")
        .unwrap();
    assert_ne!(a.key(), b.key());
}

#[test]
fn periodic_checkpoint_restores_binary_documents_views_and_frontend_without_unchanged_writes() {
    let fixture = Fixture::new();
    let original = b"\xef\xbb\xbfuno\r\ndos\r\ntres\r\n";
    let binary = [0, 255, 128, 13, 10, 0];
    fs::write(fixture.path("texto.rs"), original).unwrap();
    fs::write(fixture.path("binario.dat"), binary).unwrap();
    let mut bench = Workbench::default();
    bench
        .open_file(fixture.path("texto.rs"), Some(TextEncoding::Utf8))
        .unwrap();
    bench.insert_text("uno ").unwrap();
    bench.insert_text("dos ").unwrap();
    bench.undo().unwrap();
    bench.set_linear_selection_bytes(3, 6).unwrap();
    bench.reveal_active_pane(2, 5);
    assert!(bench.scroll_view_vertical(PaneId::Primary, 1, 2));
    assert!(bench.split_editor_right());
    assert!(bench.focus_editor_pane(PaneId::Secondary));
    bench.set_linear_selection_bytes(7, 9).unwrap();
    bench.reveal_active_pane(2, 5);
    assert_eq!(bench.toggle_reference_pin(), Some(true));
    assert_eq!(bench.toggle_scroll_link(), Some(true));
    assert!(bench.focus_editor_pane(PaneId::Primary));
    bench.open_file(fixture.path("binario.dat"), None).unwrap();
    bench.new_file(PathBuf::from("borrador.rs"), TextEncoding::Utf8);
    bench.insert_text("sin guardar\r\n").unwrap();
    bench.execute(CommandId::ViewToggleInvisibles).unwrap();
    let frontend = b"\0GUI\xff\x81\r\npanel=explorador";
    let expected = signature(&bench, &fixture.root);
    let primary = view(&bench, PaneId::Primary);
    let secondary = view(&bench, PaneId::Secondary);
    let wake = Wakeup::default();
    let now = Instant::now();
    let mut controller = PeriodicRecovery::new(policy(), now).unwrap();
    let mut recovery = fixture.recovery();
    let due = now + policy().idle;
    start(&mut controller, &mut recovery, &bench, frontend, due, &wake);
    assert!(drain(&mut controller, &mut recovery, &bench, frontend, due, &wake).is_empty());
    assert_eq!(controller.statistics().confirmed, 1);
    assert!(controller.statistics().max_chunk_bytes <= 65536);
    let head = fs::read(head_path(&recovery)).unwrap();
    let (restored, opaque, backup) =
        Workbench::restore_recovery(&mut recovery.as_mut().unwrap().store)
            .unwrap()
            .unwrap();
    assert!(!backup);
    assert_eq!(opaque, frontend);
    assert_eq!(signature(&restored, &fixture.root), expected);
    assert_eq!(view(&restored, PaneId::Primary), primary);
    assert_eq!(view(&restored, PaneId::Secondary), secondary);
    assert!(
        restored.panes().is_split()
            && restored.panes().scroll_linked()
            && restored.invisibles_visible()
    );
    controller.note_activity(due);
    assert!(controller
        .poll(&mut recovery, &bench, frontend, due + policy().idle, &wake)
        .is_none());
    assert!(!controller.active());
    assert_eq!(controller.statistics().unchanged, 1);
    assert_eq!(fs::read(head_path(&recovery)).unwrap(), head);
    assert_eq!(fs::read(fixture.path("texto.rs")).unwrap(), original);
    assert_eq!(fs::read(fixture.path("binario.dat")).unwrap(), binary);
}

#[test]
fn periodic_stale_partial_capture_never_publishes_and_retry_captures_latest() {
    let fixture = Fixture::new();
    fs::write(fixture.path("grande.txt"), vec![b'a'; 256 * 1024]).unwrap();
    let mut bench = Workbench::default();
    bench
        .open_file(fixture.path("grande.txt"), Some(TextEncoding::Utf8))
        .unwrap();
    let wake = Wakeup::default();
    let now = Instant::now();
    let due = now + policy().idle;
    let mut controller = PeriodicRecovery::new(policy(), now).unwrap();
    let mut recovery = fixture.recovery();
    start(
        &mut controller,
        &mut recovery,
        &bench,
        b"inicial",
        due,
        &wake,
    );
    assert!(drain(
        &mut controller,
        &mut recovery,
        &bench,
        b"inicial",
        due,
        &wake
    )
    .is_empty());
    let head = fs::read(head_path(&recovery)).unwrap();
    bench.insert_text("nuevo ").unwrap();
    controller.note_activity(due);
    let next = due + policy().idle;
    start(
        &mut controller,
        &mut recovery,
        &bench,
        b"actual",
        next,
        &wake,
    );
    let chunks = controller.statistics().chunks;
    controller.poll(&mut recovery, &bench, b"actual", next, &wake);
    assert_eq!(controller.statistics().chunks, chunks + 1);
    bench.insert_text("posterior ").unwrap();
    assert!(drain(
        &mut controller,
        &mut recovery,
        &bench,
        b"actual",
        next,
        &wake
    )
    .is_empty());
    assert_eq!(controller.statistics().canceled, 1);
    assert_eq!(controller.statistics().confirmed, 1);
    assert_eq!(fs::read(head_path(&recovery)).unwrap(), head);
    let retry = controller.deadline().unwrap();
    start(
        &mut controller,
        &mut recovery,
        &bench,
        b"actual",
        retry,
        &wake,
    );
    assert!(drain(
        &mut controller,
        &mut recovery,
        &bench,
        b"actual",
        retry,
        &wake
    )
    .is_empty());
    let (restored, opaque, _) = Workbench::restore_recovery(&mut recovery.as_mut().unwrap().store)
        .unwrap()
        .unwrap();
    assert_eq!(opaque, b"actual");
    assert_eq!(
        signature(&restored, &fixture.root),
        signature(&bench, &fixture.root)
    );
    assert_eq!(
        fs::read(fixture.path("grande.txt")).unwrap(),
        vec![b'a'; 256 * 1024]
    );
    // Explicit cancellation wakes recv(), joins, and returns the exclusive store lease.
    bench.insert_text("cancelar ").unwrap();
    controller.note_activity(retry);
    start(
        &mut controller,
        &mut recovery,
        &bench,
        b"cancelar",
        retry + policy().idle,
        &wake,
    );
    controller.cancel_and_join(&mut recovery);
    assert!(!controller.active() && recovery.is_some());
    let _ = recovery.as_mut().unwrap().store.load_latest().unwrap();
}

#[test]
fn cooperative_session_capture_rejects_closed_documents_and_save_info_changes() {
    let fixture = Fixture::new();
    for save in [false, true] {
        let mut bench = Workbench::default();
        bench.new_file(fixture.path("borrador.txt"), TextEncoding::Utf8);
        bench.insert_text("cambios").unwrap();
        let mut capture = bench
            .begin_recovery_capture(&fixture.root, b"estado")
            .unwrap();
        assert!(matches!(
            capture.next_chunk(&bench).unwrap(),
            Some(CaptureChunk::Data { .. })
        ));
        if save {
            let document = bench.active_document().unwrap();
            let target = bench
                .capture_save_target(document.id, document.session.document().revision())
                .unwrap();
            bench
                .save_target(target, Some(fixture.path("guardado.txt")))
                .unwrap();
        } else {
            bench.close_active(true).unwrap();
        }
        assert!(capture.next_chunk(&bench).is_err());
    }
}

#[test]
fn periodic_completed_document_keeps_initial_snapshot_and_schedules_following_activity() {
    let fixture = Fixture::new();
    let mut bench = Workbench::default();
    bench.new_file(fixture.path("primero.txt"), TextEncoding::Utf8);
    bench.insert_text("estado inicial").unwrap();
    bench.new_file(fixture.path("segundo.txt"), TextEncoding::Utf8);
    bench.insert_text(&"b".repeat(256 * 1024)).unwrap();
    let initial = signature(&bench, &fixture.root);
    let wake = Wakeup::default();
    let now = Instant::now();
    let due = now + policy().idle;
    let mut controller = PeriodicRecovery::new(policy(), now).unwrap();
    let mut recovery = fixture.recovery();
    start(
        &mut controller,
        &mut recovery,
        &bench,
        b"inicio",
        due,
        &wake,
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    // One small document produces Data followed by End. Once those two messages
    // are accepted its immutable initial bytes no longer depend on the live model.
    while controller.statistics().chunks < 2 {
        assert!(Instant::now() < deadline);
        let ticket = wake.ticket();
        assert!(controller
            .poll(&mut recovery, &bench, b"inicio", due, &wake)
            .is_none());
        wake.wait(ticket, Some(Duration::from_millis(2)));
    }
    assert!(bench.select_tab(1));
    bench.insert_text(" posterior").unwrap();
    controller.note_activity(due);
    assert!(drain(
        &mut controller,
        &mut recovery,
        &bench,
        b"posterior",
        due,
        &wake
    )
    .is_empty());
    assert_eq!(controller.statistics().canceled, 0);
    let (restored, opaque, _) = Workbench::restore_recovery(&mut recovery.as_mut().unwrap().store)
        .unwrap()
        .unwrap();
    assert_eq!(opaque, b"inicio");
    assert_eq!(signature(&restored, &fixture.root), initial);
    let next = controller
        .deadline()
        .expect("La actividad posterior requiere otra captura");
    start(
        &mut controller,
        &mut recovery,
        &bench,
        b"posterior",
        next,
        &wake,
    );
    assert!(drain(
        &mut controller,
        &mut recovery,
        &bench,
        b"posterior",
        next,
        &wake
    )
    .is_empty());
    let (latest, opaque, _) = Workbench::restore_recovery(&mut recovery.as_mut().unwrap().store)
        .unwrap()
        .unwrap();
    assert_eq!(opaque, b"posterior");
    assert_eq!(
        signature(&latest, &fixture.root),
        signature(&bench, &fixture.root)
    );
    assert_eq!(controller.statistics().confirmed, 2);
}

#[test]
fn periodic_head_lock_preserves_confirmed_generation_and_retries_after_backoff() {
    use std::os::windows::fs::OpenOptionsExt;
    let fixture = Fixture::new();
    let mut bench = Workbench::default();
    bench.new_file(fixture.path("borrador.txt"), TextEncoding::Utf8);
    bench.insert_text("primero").unwrap();
    let wake = Wakeup::default();
    let now = Instant::now();
    let due = now + policy().idle;
    let mut controller = PeriodicRecovery::new(policy(), now).unwrap();
    let mut recovery = fixture.recovery();
    start(
        &mut controller,
        &mut recovery,
        &bench,
        b"primero",
        due,
        &wake,
    );
    assert!(drain(
        &mut controller,
        &mut recovery,
        &bench,
        b"primero",
        due,
        &wake
    )
    .is_empty());
    let head = fs::read(head_path(&recovery)).unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(head_path(&recovery))
        .unwrap();
    bench.insert_text(" segundo").unwrap();
    controller.note_activity(due);
    let next = due + policy().idle;
    start(
        &mut controller,
        &mut recovery,
        &bench,
        b"segundo",
        next,
        &wake,
    );
    let errors = drain(
        &mut controller,
        &mut recovery,
        &bench,
        b"segundo",
        next,
        &wake,
    );
    assert_eq!(errors.len(), 1);
    assert_eq!(controller.statistics().failed, 1);
    assert_eq!(fs::read(head_path(&recovery)).unwrap(), head);
    let (previous, opaque, _) = Workbench::restore_recovery(&mut recovery.as_mut().unwrap().store)
        .unwrap()
        .unwrap();
    assert_eq!(opaque, b"primero");
    assert_eq!(
        previous
            .active_document()
            .unwrap()
            .session
            .document()
            .to_bytes(),
        b"primero"
    );
    let retry = controller.deadline().unwrap();
    assert!(retry >= next + policy().retry);
    assert!(!controller.needs_capture(retry - Duration::from_millis(1)));
    drop(lock);
    start(
        &mut controller,
        &mut recovery,
        &bench,
        b"segundo",
        retry,
        &wake,
    );
    assert!(drain(
        &mut controller,
        &mut recovery,
        &bench,
        b"segundo",
        retry,
        &wake
    )
    .is_empty());
    assert_eq!(controller.statistics().confirmed, 2);
    let (latest, opaque, _) = Workbench::restore_recovery(&mut recovery.as_mut().unwrap().store)
        .unwrap()
        .unwrap();
    assert_eq!(opaque, b"segundo");
    assert_eq!(
        signature(&latest, &fixture.root),
        signature(&bench, &fixture.root)
    );
    assert!(!fixture.path("borrador.txt").exists());
}

#[test]
fn periodic_deferred_cleanup_retries_without_rewriting_confirmed_documents() {
    use std::os::windows::fs::OpenOptionsExt;
    let fixture = Fixture::new();
    let original = b"original\r\nconservar\r\n";
    fs::write(fixture.path("original.txt"), original).unwrap();
    let mut bench = Workbench::default();
    bench
        .open_file(fixture.path("original.txt"), Some(TextEncoding::Utf8))
        .unwrap();
    let mut recovery = fixture.recovery();
    let mut generations = Vec::new();
    for index in 0..3 {
        bench.insert_text(&format!("paso {index} ")).unwrap();
        let owner = recovery.as_mut().unwrap();
        generations.push(
            bench
                .checkpoint_recovery(&mut owner.store, &owner.base, &[index])
                .unwrap(),
        );
    }
    let slot = recovery.as_ref().unwrap().store.slot_path().to_owned();
    let generation = |id| slot.join(format!("generation-{id:020}"));
    let obsolete = generation(generations[0]);
    let lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(obsolete.join("doc-0000.bin"))
        .unwrap();
    let wake = Wakeup::default();
    let now = Instant::now();
    let due = now + policy().idle;
    let mut controller = PeriodicRecovery::new(policy(), now).unwrap();
    start(
        &mut controller,
        &mut recovery,
        &bench,
        b"confirmado",
        due,
        &wake,
    );
    let warnings = drain(
        &mut controller,
        &mut recovery,
        &bench,
        b"confirmado",
        due,
        &wake,
    );
    assert_eq!(
        warnings.len(),
        1,
        "La copia se confirma y la limpieza bloqueada se comunica aparte"
    );
    assert_eq!(controller.statistics().confirmed, 1);
    assert_eq!(controller.statistics().failed, 0);
    assert!(obsolete.join("doc-0000.bin").exists());
    let owner = recovery.as_mut().unwrap();
    let latest = owner.store.load_latest().unwrap().unwrap();
    let latest_id = latest.generation;
    assert!(!latest.used_backup);
    drop(latest);
    let backup = owner.store.load_backup().unwrap().unwrap();
    assert_eq!(backup.generation, generations[2]);
    drop(backup);
    let protected = [generation(generations[2]), generation(latest_id)];
    let contents = |directory: &Path| {
        let mut files: Vec<_> = fs::read_dir(directory)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), fs::read(entry.path()).unwrap())
            })
            .collect();
        files.sort_by(|left, right| left.0.cmp(&right.0));
        files
    };
    let protected_bytes = [contents(&protected[0]), contents(&protected[1])];
    let head = fs::read(slot.join("HEAD")).unwrap();
    let backup_head = fs::read(slot.join("BACKUP")).unwrap();
    let before = controller.statistics();
    let retry = controller
        .deadline()
        .expect("La limpieza pendiente conserva su plazo");
    assert_eq!(retry, due + policy().retry);
    assert!(!controller.needs_capture(retry));
    drop(lock);
    assert!(controller
        .poll(&mut recovery, &bench, b"", retry, &wake)
        .is_none());
    assert!(controller.active() && recovery.is_none());
    assert!(drain(&mut controller, &mut recovery, &bench, b"", retry, &wake).is_empty());
    assert!(!obsolete.exists());
    assert!(!generation(generations[1]).exists());
    assert_eq!(contents(&protected[0]), protected_bytes[0]);
    assert_eq!(contents(&protected[1]), protected_bytes[1]);
    assert_eq!(fs::read(slot.join("HEAD")).unwrap(), head);
    assert_eq!(fs::read(slot.join("BACKUP")).unwrap(), backup_head);
    let after = controller.statistics();
    assert_eq!(
        (after.confirmed, after.chunks, after.unchanged, after.failed),
        (
            before.confirmed,
            before.chunks,
            before.unchanged,
            before.failed
        )
    );
    assert!(controller.deadline().is_none());
    let (restored, frontend, used_backup) =
        Workbench::restore_recovery(&mut recovery.as_mut().unwrap().store)
            .unwrap()
            .unwrap();
    assert!(!used_backup);
    assert_eq!(frontend, b"confirmado");
    assert_eq!(
        signature(&restored, &fixture.root),
        signature(&bench, &fixture.root)
    );
    assert_eq!(fs::read(fixture.path("original.txt")).unwrap(), original);
}
