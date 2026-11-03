//! Public recovery integration: canonical documents once, persistent views and
//! opaque frontend metadata. All disk fixtures remain under workspace/work.
#![cfg(windows)]
use jc_app::{CommandId, PaneId, TabId, Workbench};
use jc_core::ChangeSet;
use jc_files::{
    recovery::{RecoveryLimits, RecoveryStore},
    SessionRecoveryInfo,
};
use jc_text::{TextEncoding, TextPosition};
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Workspace {
    root: PathBuf,
    base: PathBuf,
}
impl Workspace {
    fn new() -> Self {
        use std::os::windows::fs::MetadataExt;
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .canonicalize()
            .unwrap();
        let base = workspace
            .join("work/test-temp")
            .canonicalize()
            .expect("Debe existir work/test-temp");
        assert!(base.starts_with(&workspace));
        assert!(fs::symlink_metadata(&base)
            .is_ok_and(|m| m.is_dir() && m.file_attributes() & 0x400 == 0));
        let root = base.join(format!(
            "workbench-recovery-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self { root, base }
    }
    fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }
    fn store(&self, name: &str) -> RecoveryStore {
        RecoveryStore::open(&self.path(name), RecoveryLimits::default()).unwrap()
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        use std::os::windows::fs::MetadataExt;
        if self.root.parent() == Some(self.base.as_path())
            && fs::symlink_metadata(&self.root)
                .is_ok_and(|m| m.is_dir() && m.file_attributes() & 0x400 == 0)
            && self
                .root
                .canonicalize()
                .is_ok_and(|p| p.starts_with(&self.base))
        {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}
#[derive(Debug, PartialEq, Eq)]
struct DocumentState {
    title: String,
    dirty: bool,
    info: SessionRecoveryInfo,
    bytes: Vec<u8>,
    revision: u64,
    undo: usize,
    redo: usize,
    history_steps: usize,
    history_bytes: usize,
    last_change: Option<ChangeSet>,
}
fn documents(bench: &Workbench, base: &Path) -> Vec<DocumentState> {
    bench
        .tabs()
        .tabs()
        .iter()
        .map(|tab| {
            let session = &bench.document_by_tab(tab.id).unwrap().session;
            let mut info = session.recovery_info();
            if info.path.is_relative() {
                info.path = base.join(info.path);
            }
            let source = session.document();
            DocumentState {
                title: tab.title.clone(),
                dirty: tab.dirty,
                info,
                bytes: source.to_bytes(),
                revision: source.revision(),
                undo: source.undo_steps(),
                redo: source.redo_steps(),
                history_steps: source.history_limit(),
                history_bytes: source.history_bytes_limit(),
                last_change: source.last_change().cloned(),
            }
        })
        .collect()
}
#[derive(Debug, PartialEq, Eq)]
struct PaneState {
    location: Option<(usize, usize)>,
    cursor: Option<usize>,
    anchor: Option<usize>,
    scroll: Option<(usize, usize, bool)>,
    rectangle: Option<(usize, usize, usize, u64)>,
    pinned: bool,
    back: bool,
    forward: bool,
}
fn pane(bench: &Workbench, id: PaneId) -> Option<PaneState> {
    let view = bench.panes().view(id)?;
    let location = view.location().map(|loc| {
        (
            bench
                .tabs()
                .tabs()
                .iter()
                .position(|tab| tab.id == loc.tab)
                .unwrap(),
            loc.byte_offset,
        )
    });
    let state = view.state();
    Some(PaneState {
        location,
        cursor: state.map(|s| s.cursor),
        anchor: state.and_then(|s| s.anchor),
        scroll: state.map(|s| (s.scroll_line, s.scroll_column, s.manual_scroll)),
        rectangle: state.and_then(|s| s.rectangle).map(|r| {
            (
                r.anchor_column.0,
                r.head_column.0,
                r.scroll_column.0,
                r.revision,
            )
        }),
        pinned: view.is_pinned(),
        back: view.can_go_back(),
        forward: view.can_go_forward(),
    })
}
fn same(left: &Workbench, right: &Workbench, base: &Path) {
    assert_eq!(documents(left, base), documents(right, base));
    assert_eq!(left.panes().active(), right.panes().active());
    assert_eq!(left.panes().is_split(), right.panes().is_split());
    assert_eq!(left.panes().scroll_linked(), right.panes().scroll_linked());
    assert_eq!(left.invisibles_visible(), right.invisibles_visible());
    for id in [PaneId::Primary, PaneId::Secondary] {
        assert_eq!(pane(left, id), pane(right, id));
    }
}
fn selection(bench: &mut Workbench, tab: TabId, line: usize, column: usize) {
    let anchor = bench
        .byte_offset_for_tab(tab, TextPosition { line, column })
        .unwrap();
    let head = bench
        .byte_offset_for_tab(
            tab,
            TextPosition {
                line,
                column: column + 3,
            },
        )
        .unwrap();
    bench.set_linear_selection_bytes(anchor, head).unwrap();
}
fn restored(store: &mut RecoveryStore) -> (Workbench, Vec<u8>, bool) {
    Workbench::restore_recovery(store)
        .unwrap()
        .expect("Debe existir una generación recuperable")
}

#[test]
fn full_workbench_recovery_keeps_binary_documents_histories_split_and_inactive_view_geometry() {
    let workspace = Workspace::new();
    let utf8 = workspace.path("código.rs");
    let legacy = workspace.path("legado.p");
    let binary = workspace.path("binario.dat");
    let mut utf8_bytes = b"\xef\xbb\xbf".to_vec();
    for index in 0..48 {
        utf8_bytes
            .extend_from_slice(format!("línea {index:02} {}\r\n", "abcdef".repeat(24)).as_bytes());
    }
    let legacy_bytes = b"\x81ni\xf1o\r\n\x00\xff\rfin";
    let binary_bytes = b"\x00\xff\xf0\x81\r\n\x00";
    fs::write(&utf8, &utf8_bytes).unwrap();
    fs::write(&legacy, legacy_bytes).unwrap();
    fs::write(&binary, binary_bytes).unwrap();
    let mut bench = Workbench::default();
    let text_tab = bench.open_file(&utf8, Some(TextEncoding::Utf8)).unwrap();
    bench.insert_text("prefijo ").unwrap();
    bench.insert_text("paso posterior").unwrap();
    bench.undo().unwrap();
    // Leave a gap in old TabIds: recovery must map references, not use indexes
    // accidentally as identities or duplicate the source behind a split view.
    bench.new_file(workspace.path("temporal.rs"), TextEncoding::Utf8);
    bench.close_active(true).unwrap();
    bench
        .open_file(&legacy, Some(TextEncoding::Windows1252))
        .unwrap();
    bench.insert_text("á ").unwrap();
    bench.open_file(&binary, None).unwrap();
    bench.new_file(PathBuf::from("sin-titulo.rs"), TextEncoding::Utf8);
    bench.insert_text("borrador\r\n").unwrap();
    assert!(bench.select_tab(1));
    selection(&mut bench, text_tab, 12, 110);
    bench.reveal_active_pane(5, 25);
    assert!(bench.scroll_view_vertical(PaneId::Primary, 4, 5));
    let inactive_primary = pane(&bench, PaneId::Primary).unwrap();
    assert!(inactive_primary.scroll.unwrap().1 > 0);
    assert!(bench.split_editor_right());
    assert!(bench.focus_editor_pane(PaneId::Secondary));
    selection(&mut bench, text_tab, 25, 70);
    bench.reveal_active_pane(6, 20);
    assert!(bench.scroll_view_vertical(PaneId::Secondary, -3, 6));
    assert_eq!(bench.toggle_reference_pin(), Some(true));
    assert_eq!(bench.toggle_scroll_link(), Some(true));
    assert!(bench.focus_editor_pane(PaneId::Primary));
    assert!(bench.select_tab(2));
    bench.set_linear_selection_bytes(0, 2).unwrap();
    bench.execute(CommandId::ViewToggleInvisibles).unwrap();
    let frontend = b"\0CIDE-TUI\xff\x81\0paneles\r\n";
    let before = documents(&bench, &workspace.root);
    let primary = pane(&bench, PaneId::Primary);
    let secondary = pane(&bench, PaneId::Secondary);
    let mut store = workspace.store("recovery");
    bench
        .checkpoint_recovery(&mut store, &workspace.root, frontend)
        .unwrap();
    assert_eq!(documents(&bench, &workspace.root), before);
    assert_eq!(pane(&bench, PaneId::Primary), primary);
    assert_eq!(pane(&bench, PaneId::Secondary), secondary);
    let (mut recovered, opaque, backup) = restored(&mut store);
    assert_eq!(opaque, frontend);
    assert!(!backup);
    same(&bench, &recovered, &workspace.root);
    assert_eq!(recovered.tabs().tabs().len(), 4);
    assert!(recovered.select_tab(1));
    assert!(bench.select_tab(1));
    assert_eq!(pane(&recovered, PaneId::Primary), Some(inactive_primary));
    same(&bench, &recovered, &workspace.root);
    bench.undo().unwrap();
    recovered.undo().unwrap();
    same(&bench, &recovered, &workspace.root);
    bench.redo().unwrap();
    recovered.redo().unwrap();
    same(&bench, &recovered, &workspace.root);
    assert_eq!(fs::read(&utf8).unwrap(), utf8_bytes);
    assert_eq!(fs::read(&legacy).unwrap(), legacy_bytes);
    assert_eq!(fs::read(&binary).unwrap(), binary_bytes);
    assert!(!workspace.path("sin-titulo.rs").exists());
}

#[test]
fn recovery_absolutizes_relative_drafts_and_restores_missing_originals_without_disk_io_or_implicit_save(
) {
    let workspace = Workspace::new();
    let original = workspace.path("original.p");
    fs::write(&original, b"ni\xf1o\r\n").unwrap();
    let base = workspace.path("base-que-no-existe");
    let relative = PathBuf::from("subcarpeta/sin-titulo.rs");
    let mut bench = Workbench::default();
    bench.new_file(&relative, TextEncoding::Utf8);
    bench.insert_text("nuevo\r\ntexto").unwrap();
    bench
        .open_file(&original, Some(TextEncoding::Windows1252))
        .unwrap();
    bench.insert_text("local ").unwrap();
    let mut store = workspace.store("recovery");
    bench
        .checkpoint_recovery(&mut store, &base, b"GUI\0\xff")
        .unwrap();
    fs::remove_file(&original).unwrap();
    let (mut recovered, opaque, backup) = restored(&mut store);
    assert_eq!(opaque, b"GUI\0\xff");
    assert!(!backup);
    assert!(!base.exists());
    assert!(!original.exists());
    let tabs = recovered.tabs().tabs();
    assert_eq!(tabs.len(), 2);
    let draft = &recovered.document_by_tab(tabs[0].id).unwrap().session;
    assert_eq!(draft.path(), base.join(relative));
    assert!(draft.is_new());
    assert!(draft.document().is_modified());
    assert_eq!(draft.document().to_bytes(), b"nuevo\r\ntexto");
    let missing = &recovered.document_by_tab(tabs[1].id).unwrap().session;
    assert_eq!(missing.document().to_bytes(), b"local ni\xf1o\r\n");
    assert!(recovered.save_active().is_err());
    assert!(!original.exists());
    assert!(recovered
        .active_document()
        .unwrap()
        .session
        .document()
        .is_modified());
}

#[test]
fn valid_store_checksum_with_hostile_application_metadata_falls_back_to_complete_previous_generation(
) {
    let workspace = Workspace::new();
    let mut bench = Workbench::default();
    bench.new_file(workspace.path("borrador.rs"), TextEncoding::Utf8);
    bench.insert_text("primera versión\r\n").unwrap();
    let mut store = workspace.store("recovery");
    bench
        .checkpoint_recovery(&mut store, &workspace.root, b"frontend anterior")
        .unwrap();
    let expected = documents(&bench, &workspace.root);
    bench.insert_text("versión más nueva").unwrap();
    let mut hostile = b"CIDEAPP1".to_vec();
    hostile.extend_from_slice(&u64::MAX.to_le_bytes());
    let tabs = bench.tabs().tabs();
    store
        .checkpoint(&hostile, tabs.len(), |index, writer| {
            bench
                .document_by_tab(tabs[index].id)
                .unwrap()
                .session
                .document()
                .write_recovery(writer)
        })
        .unwrap();
    // The storage layer accepts these bytes and verifies their checksums. Only
    // the application codec can detect the semantically invalid count.
    let loaded = store.load_latest().unwrap().unwrap();
    assert!(!loaded.used_backup);
    assert_eq!(loaded.metadata, hostile);
    drop(loaded);
    let (recovered, opaque, backup) = restored(&mut store);
    assert!(backup);
    assert_eq!(opaque, b"frontend anterior");
    assert_eq!(documents(&recovered, &workspace.root), expected);
    assert!(!workspace.path("borrador.rs").exists());
}

#[test]
fn rejected_recovery_limits_preserve_previous_generation_and_never_replace_the_live_workbench() {
    let workspace = Workspace::new();
    let mut bench = Workbench::default();
    bench.new_file(workspace.path("borrador.rs"), TextEncoding::Utf8);
    bench.insert_text("sin guardar\r\n").unwrap();
    let mut store = workspace.store("recovery");
    let generation = bench
        .checkpoint_recovery(&mut store, &workspace.root, b"estado")
        .unwrap();
    let before = documents(&bench, &workspace.root);
    assert!(bench
        .checkpoint_recovery(&mut store, Path::new("base-relativa"), b"no")
        .is_err());
    assert!(bench
        .checkpoint_recovery(&mut store, &workspace.root, &vec![0; 1024 * 1024 + 1])
        .is_err());
    assert_eq!(documents(&bench, &workspace.root), before);
    assert_eq!(store.load_latest().unwrap().unwrap().generation, generation);
    let (recovered, opaque, backup) = restored(&mut store);
    assert!(!backup);
    assert_eq!(opaque, b"estado");
    assert_eq!(documents(&recovered, &workspace.root), before);
    let mut invalid_store = workspace.store("invalid-recovery");
    let mut bad = b"CIDEAPP1".to_vec();
    bad.extend_from_slice(&u64::MAX.to_le_bytes());
    invalid_store
        .checkpoint(&bad, 0, |_, _| {
            Err(io::Error::other("No debe solicitar un documento"))
        })
        .unwrap();
    assert!(Workbench::restore_recovery(&mut invalid_store).is_err());
    assert_eq!(documents(&bench, &workspace.root), before);
    assert!(!workspace.path("borrador.rs").exists());
}
