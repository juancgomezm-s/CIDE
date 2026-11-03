use super::*;
use jc_files::{DirectoryEntry, DirectoryListing};
use std::{
    ffi::OsString,
    sync::{Condvar, Mutex},
    time::Instant,
};

fn root(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(name)
}
fn listing(entries: &[(&str, bool)]) -> DirectoryListing {
    DirectoryListing {
        entries: entries
            .iter()
            .map(|(name, is_dir)| DirectoryEntry {
                name: OsString::from(name),
                is_dir: *is_dir,
            })
            .collect(),
        limited: false,
        examined: entries.len(),
        skipped: 0,
    }
}
fn custom(
    provider: impl Fn(&worker::Request, usize) -> std::io::Result<DirectoryListing>
        + Send
        + Sync
        + 'static,
    limits: ExplorerLimits,
) -> ProjectExplorer {
    let wake = Wakeup::default();
    let mut catalog = ProjectExplorer::new(wake.clone(), limits).unwrap();
    catalog.shared = Arc::new(worker::Shared::with_provider(wake, Arc::new(provider)));
    catalog
}
fn until(catalog: &mut ProjectExplorer, predicate: impl Fn(&ProjectExplorer) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let ticket = catalog.shared.wake.ticket();
        let root = catalog.root().unwrap().to_owned();
        catalog.poll(&root, 2);
        if predicate(catalog) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "Explorer test delivery did not complete"
        );
        catalog.shared.wake.wait(
            ticket,
            Some(deadline.saturating_duration_since(Instant::now())),
        );
    }
}
fn ready(catalog: &mut ProjectExplorer, id: NodeId) {
    until(catalog, |c| {
        c.node(id)
            .is_some_and(|n| n.state() == ExplorerLoadState::Ready)
    });
}
fn named(catalog: &ProjectExplorer, name: &str) -> NodeId {
    catalog
        .nodes
        .iter()
        .find(|n| n.relative.file_name() == Some(std::ffi::OsStr::new(name)))
        .unwrap()
        .id
}

#[test]
fn lazy_expansion_sorted_ids_and_readonly_view_survive_collapse_refresh_and_resize() {
    let mut catalog = custom(
        |request, _| {
            Ok(if request.relative.as_os_str().is_empty() {
                listing(&[("z.txt", false), ("folder", true), ("a.txt", false)])
            } else {
                listing(&[("niño\t.txt", false)])
            })
        },
        Default::default(),
    );
    catalog.set_root(root("virtual-explorer-a")).unwrap();
    let id = catalog.root_id().unwrap();
    ready(&mut catalog, id);
    assert_eq!(catalog.nodes().len(), 4);
    let folder = named(&catalog, "folder");
    assert_eq!(catalog.node(id).unwrap().children()[0], folder);
    assert_eq!(
        catalog.node(folder).unwrap().state(),
        ExplorerLoadState::Unloaded
    );
    catalog.expand(folder).unwrap();
    ready(&mut catalog, folder);
    let leaf = named(&catalog, "niño\t.txt");
    let mut view = ExplorerView::default();
    view.prepare(&catalog, 128, 140);
    view.select(leaf);
    let p = view.prepare(&catalog, 1, 45);
    assert_eq!(p.rows.len(), 1);
    assert_eq!(p.rows[0].id, leaf);
    assert!(p.rows[0].selected);
    assert!(p.rows[0].label.contains(r"ni\u{F1}o\x09.txt"));
    let before = (
        catalog.generation(),
        catalog.revision(),
        catalog.owned_bytes(),
    );
    for (rows, cols) in [
        (0, 0),
        (1, 1),
        (45, 45),
        (128, 140),
        (usize::MAX, usize::MAX),
    ] {
        let p = view.prepare(&catalog, rows, cols);
        assert!(p.rows.len() <= rows.min(128));
        assert!(p.rows.iter().all(|r| r.label.len() <= cols.min(256)
            && r.label.is_ascii()
            && !r.label.chars().any(char::is_control)));
        assert_eq!(
            (
                catalog.generation(),
                catalog.revision(),
                catalog.owned_bytes()
            ),
            before
        );
    }
    catalog.collapse(folder).unwrap();
    view.prepare(&catalog, 128, 140);
    assert_eq!(view.selected(), Some(folder));
    assert!(catalog.node(leaf).is_none());
    catalog.expand(folder).unwrap();
    ready(&mut catalog, folder);
    assert_ne!(named(&catalog, "niño\t.txt"), leaf);
    catalog.refresh(id).unwrap();
    ready(&mut catalog, id);
    assert!(catalog.node(folder).is_none());
    view.last(&catalog);
    view.move_selection(&catalog, -1);
    let p = view.prepare(&catalog, 1, 45);
    assert_eq!(p.rows.len(), 1);
    assert!(p.rows[0].selected);
    view.move_selection(&catalog, isize::MIN);
    assert_eq!(view.selected(), Some(id));
    view.parent(&catalog);
    assert_eq!(view.selected(), Some(id));
}

#[test]
fn catalog_and_visible_budgets_are_global_not_per_directory() {
    let limits = ExplorerLimits {
        max_nodes: 12,
        max_owned_bytes: 8192,
        ..Default::default()
    };
    let mut catalog = custom(
        |request, _| {
            Ok(if request.relative.as_os_str().is_empty() {
                listing(&[("one", true), ("two", true)])
            } else {
                DirectoryListing {
                    entries: (0..30)
                        .map(|i| DirectoryEntry {
                            name: format!("file-{i}.txt").into(),
                            is_dir: false,
                        })
                        .collect(),
                    limited: false,
                    examined: 30,
                    skipped: 0,
                }
            })
        },
        limits,
    );
    catalog.set_root(root("virtual-budget")).unwrap();
    let id = catalog.root_id().unwrap();
    ready(&mut catalog, id);
    let one = named(&catalog, "one");
    let two = named(&catalog, "two");
    catalog.expand(one).unwrap();
    until(&mut catalog, |c| {
        c.node(one).unwrap().state() == ExplorerLoadState::Limited
    });
    catalog.expand(two).unwrap();
    until(&mut catalog, |c| {
        c.node(two).unwrap().state() == ExplorerLoadState::Limited
    });
    assert!(catalog.nodes().len() <= 12);
    assert!(catalog.owned_bytes() <= 8192);
    let mut view = ExplorerView::default();
    view.last(&catalog);
    assert!(view.prepare(&catalog, usize::MAX, usize::MAX).rows.len() <= 12);
    catalog.collapse(one).unwrap();
    assert!(catalog.owned_bytes() <= 8192);
    assert!(catalog.node(two).is_some());
}

#[derive(Default)]
struct Gate {
    state: Mutex<(bool, bool)>,
    changed: Condvar,
}
impl Gate {
    fn block(&self) -> std::io::Result<()> {
        let mut state = self.state.lock().unwrap();
        state.0 = true;
        self.changed.notify_all();
        let (state, timeout) = self
            .changed
            .wait_timeout_while(state, Duration::from_secs(5), |state| !state.1)
            .unwrap();
        if timeout.timed_out() && !state.1 {
            return Err(std::io::Error::other("test gate deadline"));
        }
        Ok(())
    }
    fn entered(&self) {
        let state = self.state.lock().unwrap();
        let (state, timeout) = self
            .changed
            .wait_timeout_while(state, Duration::from_secs(5), |state| !state.0)
            .unwrap();
        assert!(!timeout.timed_out() || state.0);
    }
    fn release(&self) {
        self.state.lock().unwrap().1 = true;
        self.changed.notify_all();
    }
}

#[test]
fn superseded_root_cannot_publish_old_nodes_even_when_provider_returns_after_cancel() {
    let gate = Arc::new(Gate::default());
    let provider_gate = gate.clone();
    let mut catalog = custom(
        move |request, _| {
            if request.root.ends_with("virtual-old") {
                provider_gate.block()?;
                Ok(listing(&[("old.txt", false)]))
            } else {
                Ok(listing(&[("new.txt", false)]))
            }
        },
        Default::default(),
    );
    catalog.set_root(root("virtual-old")).unwrap();
    let old = catalog.root_id().unwrap();
    gate.entered();
    catalog.set_root(root("virtual-new")).unwrap();
    let new = catalog.root_id().unwrap();
    gate.release();
    ready(&mut catalog, new);
    assert!(catalog.node(old).is_none());
    assert_eq!(catalog.nodes().len(), 2);
    assert!(catalog
        .nodes
        .iter()
        .all(|n| n.relative != Path::new("old.txt")));
    let mut view = ExplorerView::default();
    view.select(old);
    let p = view.prepare(&catalog, 128, 140);
    assert_eq!(view.selected(), Some(new));
    assert_eq!(p.generation, new.generation);
    let wrong = root("virtual-wrong");
    assert!(catalog.poll(&wrong, 2));
    assert!(catalog.root().is_none());
    assert!(catalog.nodes().is_empty());
}

#[test]
fn bounded_pending_busy_and_cancelled_subtree_do_not_resurrect() {
    let gate = Arc::new(Gate::default());
    let pg = gate.clone();
    let mut catalog = custom(
        move |request, _| {
            if request.relative.as_os_str().is_empty() {
                Ok(DirectoryListing {
                    entries: (0..11)
                        .map(|i| DirectoryEntry {
                            name: format!("dir-{i:02}").into(),
                            is_dir: true,
                        })
                        .collect(),
                    limited: false,
                    examined: 11,
                    skipped: 0,
                })
            } else {
                if request.relative == Path::new("dir-00") {
                    pg.block()?;
                }
                Ok(listing(&[("child.txt", false)]))
            }
        },
        Default::default(),
    );
    catalog.set_root(root("virtual-queue")).unwrap();
    let root = catalog.root_id().unwrap();
    ready(&mut catalog, root);
    let directories = catalog.node(root).unwrap().children().to_vec();
    catalog.expand(directories[0]).unwrap();
    gate.entered();
    for id in &directories[1..9] {
        catalog.expand(*id).unwrap();
    }
    let before = catalog.revision();
    assert!(catalog.expand(directories[9]).is_err());
    assert_eq!(catalog.revision(), before);
    assert_eq!(
        catalog.node(directories[9]).unwrap().state(),
        ExplorerLoadState::Unloaded
    );
    {
        let q = catalog.shared.lock();
        assert_eq!(q.pending.len(), 8);
        assert!(q.pending.iter().map(worker::Request::charge).sum::<usize>() <= 128 * 1024);
    }
    catalog.collapse(directories[0]).unwrap();
    gate.release();
    until(&mut catalog, |c| {
        directories[1..9]
            .iter()
            .all(|id| c.node(*id).unwrap().state() == ExplorerLoadState::Ready)
    });
    assert!(!catalog.node(directories[0]).unwrap().expanded());
    assert!(catalog.node(directories[0]).unwrap().children().is_empty());
    assert!(catalog
        .nodes()
        .iter()
        .all(|n| n.parent() != Some(directories[0])));
}

#[test]
fn provider_panic_is_terminal_and_refresh_recovers_without_document_snapshots() {
    use std::sync::atomic::AtomicUsize;
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let mut catalog = custom(
        move |_, _| {
            if count.fetch_add(1, Ordering::SeqCst) == 0 {
                panic!("injected provider panic");
            }
            Ok(listing(&[("recovered.txt", false)]))
        },
        Default::default(),
    );
    catalog.set_root(root("virtual-panic")).unwrap();
    let id = catalog.root_id().unwrap();
    until(&mut catalog, |c| {
        c.node(id).unwrap().state() == ExplorerLoadState::Failed
    });
    assert!(catalog.take_recovery());
    assert!(!catalog.take_recovery());
    catalog.refresh(id).unwrap();
    ready(&mut catalog, id);
    assert_eq!(catalog.nodes().len(), 2);
    let before = catalog.generation();
    catalog.clear();
    assert!(catalog.generation() > before);
    assert_eq!(catalog.owned_bytes(), 0);
}

#[test]
fn view_escapes_complete_tokens_and_keeps_opaque_path_bytes() {
    assert_eq!(view::escaped(b"a\\\x00\xff\t", 256), r"a\\\x00\xFF\x09");
    assert_eq!(
        view::escaped("é\u{202e}😀".as_bytes(), 256),
        r"\u{E9}\u{202E}\u{1F600}"
    );
    assert_eq!(view::escaped("xé".as_bytes(), 5), "x...");
    assert!(ProjectExplorer::new(
        Wakeup::default(),
        ExplorerLimits {
            max_nodes: 4097,
            ..Default::default()
        }
    )
    .is_err());
}
