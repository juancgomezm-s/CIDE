use super::{
    large_document::LargeViewportRequest, LargeViewport, PreparedLargeDocument, Workbench,
    WorkbenchError,
};
use crate::{NavLocation, PaneId, TabId, ViewId, Wakeup};
use jc_files::{FileError, FileSession};
use jc_text::TextEncoding;
use std::{
    io,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
        Arc,
    },
};

pub(super) const RESIDENT_LIMIT: usize = 64 * 1024 * 1024;
const SNAPSHOT_LIMIT: u64 = 7 * 1024 * 1024 * 1024;

#[derive(Default)]
pub(super) struct LargeOpen {
    pub(super) wakeup: Wakeup,
    pending: Option<PendingOpen>,
    view: Option<PendingView>,
    next_view: Option<ViewRequest>,
}

impl std::fmt::Debug for LargeOpen {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LargeOpen")
            .field("pending", &self.pending.is_some())
            .finish()
    }
}

struct PendingOpen {
    result: Receiver<Result<PreparedLargeDocument, FileError>>,
    cancel: Arc<AtomicBool>,
    pane: PaneId,
    origin: Option<NavLocation>,
    to_side: bool,
}

struct ViewRequest {
    tab: TabId,
    plan: LargeViewportRequest,
    jump: Option<ViewJump>,
}

struct ViewJump {
    pane: PaneId,
    view: ViewId,
    origin: NavLocation,
    offset: Option<usize>,
    select: bool,
    search_selection: Option<(u64, u64, usize)>,
}

struct PendingView {
    tab: TabId,
    result: Receiver<io::Result<LargeViewport>>,
    jump: Option<ViewJump>,
}

impl Drop for PendingOpen {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}

impl Workbench {
    pub fn go_to_paged_input(&mut self, input: &str) -> Result<(), WorkbenchError> {
        let offset = input
            .trim()
            .strip_prefix('@')
            .and_then(|text| text.parse().ok())
            .ok_or_else(|| {
                WorkbenchError::LineCommand("Usa @byte, por ejemplo @4294967296".into())
            })?;
        self.go_to_paged_byte(offset, false)
    }

    pub fn set_large_open_wakeup(&mut self, wakeup: Wakeup) {
        self.large_open.wakeup = wakeup;
    }

    pub fn large_open_pending(&self) -> bool {
        self.large_open.pending.is_some()
    }

    pub fn request_file_open(
        &mut self,
        path: impl Into<PathBuf>,
        encoding: Option<TextEncoding>,
        to_side: bool,
    ) -> Result<Option<TabId>, WorkbenchError> {
        self.request_file_open_with_scope(path.into(), encoding, to_side, None, RESIDENT_LIMIT)
    }

    pub fn request_file_open_in_root(
        &mut self,
        root: &Path,
        path: impl Into<PathBuf>,
        encoding: Option<TextEncoding>,
        to_side: bool,
        resident_limit: usize,
    ) -> Result<Option<TabId>, WorkbenchError> {
        let root = root.canonicalize().map_err(FileError::from)?;
        self.request_file_open_with_scope(
            path.into(),
            encoding,
            to_side,
            Some(root),
            resident_limit.min(RESIDENT_LIMIT),
        )
    }

    fn request_file_open_with_scope(
        &mut self,
        path: PathBuf,
        encoding: Option<TextEncoding>,
        to_side: bool,
        root: Option<PathBuf>,
        resident_limit: usize,
    ) -> Result<Option<TabId>, WorkbenchError> {
        let canonical = path.canonicalize().map_err(FileError::from)?;
        if root
            .as_ref()
            .is_some_and(|root| !canonical.starts_with(root))
        {
            return Err(FileError::from(io::Error::other(
                "El archivo está fuera de la raíz del proyecto",
            ))
            .into());
        }
        if let Some(tab) = self.open_tab_for_canonical_path(&canonical) {
            let cursor = self.cursor_for_tab(tab);
            if to_side {
                self.prepare_side_target();
            }
            self.select_opened_tab(tab, cursor, true);
            return Ok(Some(tab));
        }
        let metadata = std::fs::metadata(&canonical).map_err(FileError::from)?;
        if !metadata.is_file() {
            return Err(FileError::from(io::Error::other(
                "El destino debe ser un archivo regular",
            ))
            .into());
        }
        if metadata.len() <= resident_limit as u64 {
            let session = if let Some(root) = root {
                FileSession::open_in_root(&root, path, encoding, resident_limit)?
            } else {
                FileSession::open_bounded(path, encoding, resident_limit)?
            };
            return Ok(Some(self.admit_open_session(session, None, to_side, true)));
        }
        if self.large_open_pending() {
            return Err(FileError::from(io::Error::other(
                "Ya se está preparando otro archivo grande",
            ))
            .into());
        }
        if metadata.len() > SNAPSHOT_LIMIT {
            return Err(FileError::from(io::Error::other(
                "El archivo excede el límite de instantánea de 7 GiB",
            ))
            .into());
        }
        self.check_paged_admission(metadata.len())?;

        let (send, result) = mpsc::sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let wakeup = self.large_open.wakeup.clone();
        std::thread::Builder::new()
            .name("cide-archivo-grande".into())
            .spawn(move || {
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let session = FileSession::open_private_snapshot_scoped(
                        canonical,
                        encoding,
                        root.as_deref(),
                        &std::env::temp_dir(),
                        SNAPSHOT_LIMIT,
                        |_, _| {
                            if worker_cancel.load(Ordering::Acquire) {
                                return Err(io::Error::other("Se canceló la apertura del archivo"));
                            }
                            Ok(())
                        },
                    )?;
                    PreparedLargeDocument::prepare(session)
                }))
                .unwrap_or_else(|_| {
                    Err(io::Error::other("Falló el trabajador de apertura").into())
                });
                let _ = send.send(outcome);
                wakeup.signal();
            })
            .map_err(FileError::from)?;
        let pane = self.panes.active();
        self.large_open.pending = Some(PendingOpen {
            result,
            cancel,
            pane,
            origin: self.panes.view(pane).and_then(|view| view.location()),
            to_side,
        });
        Ok(None)
    }

    pub fn poll_large_open(&mut self) -> Option<Result<TabId, WorkbenchError>> {
        let pending = self.large_open.pending.as_ref()?;
        let outcome = match pending.result.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => {
                Err(io::Error::other("Se cerró el trabajador de apertura").into())
            }
        };
        let pending = self.large_open.pending.take().unwrap();
        let activate = self.panes.active() == pending.pane
            && self
                .panes
                .view(pending.pane)
                .and_then(|view| view.location())
                == pending.origin;
        Some(outcome.map_err(WorkbenchError::from).and_then(|prepared| {
            let (session, viewport) = prepared.into_parts();
            if let Some(tab) = self.open_tab_for_canonical_path(session.path()) {
                if activate {
                    let cursor = self.cursor_for_tab(tab);
                    if pending.to_side {
                        self.prepare_side_target();
                    }
                    self.select_opened_tab(tab, cursor, true);
                }
                return Ok(tab);
            }
            self.check_paged_admission(session.document().len() as u64)?;
            Ok(self.admit_open_session(session, Some(viewport), pending.to_side, activate))
        }))
    }

    fn check_paged_admission(&self, incoming: u64) -> Result<(), WorkbenchError> {
        // Reserve space in the recovery payload for resident tabs, edit arenas,
        // and transaction history. Recheck when the async admission arrives.
        let existing = self
            .documents
            .iter()
            .filter(|document| document.session.document().is_paged())
            .try_fold(incoming, |total, document| {
                document
                    .session
                    .document()
                    .recovery_encoded_len()
                    .ok()
                    .and_then(|bytes| total.checked_add(bytes))
            });
        if existing
            .is_none_or(|total| total > crate::session::MAX_RECOVERY_PAYLOAD - 1024 * 1024 * 1024)
        {
            return Err(WorkbenchError::Save(
                "Las instantáneas abiertas superarían la capacidad de recuperación de esta sesión"
                    .into(),
            ));
        }
        Ok(())
    }

    fn open_tab_for_canonical_path(&self, canonical: &Path) -> Option<TabId> {
        self.documents
            .iter()
            .find(|document| {
                document.session.path() == canonical
                    || document
                        .session
                        .path()
                        .canonicalize()
                        .as_deref()
                        .is_ok_and(|path| path == canonical)
            })
            .map(|document| document.tab_id)
    }

    pub fn go_to_paged_byte(&mut self, offset: usize, select: bool) -> Result<(), WorkbenchError> {
        let pane = self.panes.active();
        let current = self.panes.view(pane).ok_or(WorkbenchError::NoDocument)?;
        let view = current.view_id().ok_or(WorkbenchError::NoDocument)?;
        let origin = current.location().ok_or(WorkbenchError::NoDocument)?;
        let document = self
            .document_by_tab(origin.tab)
            .ok_or(WorkbenchError::NoDocument)?;
        if !document.session.document().is_paged() || offset > document.session.document().len() {
            return Err(WorkbenchError::LineCommand(
                "El byte solicitado está fuera del documento paginado".into(),
            ));
        }
        if self.position_for_tab(origin.tab, offset).is_some() {
            return self.move_cursor_to(offset, select);
        }
        self.queue_paged_view(
            origin.tab,
            offset.saturating_sub(96),
            Some(ViewJump {
                pane,
                view,
                origin,
                offset: Some(offset),
                select,
                search_selection: None,
            }),
        )
    }

    /// Move to a prepared text boundary near a byte chosen from an overview.
    pub fn go_to_paged_near_byte(&mut self, offset: usize) -> Result<(), WorkbenchError> {
        let pane = self.panes.active();
        let current = self.panes.view(pane).ok_or(WorkbenchError::NoDocument)?;
        let view = current.view_id().ok_or(WorkbenchError::NoDocument)?;
        let origin = current.location().ok_or(WorkbenchError::NoDocument)?;
        self.queue_paged_view(
            origin.tab,
            offset,
            Some(ViewJump {
                pane,
                view,
                origin,
                offset: None,
                select: false,
                search_selection: None,
            }),
        )
    }

    pub(super) fn step_paged_view(
        &mut self,
        forward: bool,
        select: bool,
    ) -> Result<bool, WorkbenchError> {
        let pane = self.panes.active();
        let current = self.panes.view(pane).ok_or(WorkbenchError::NoDocument)?;
        let view = current.view_id().ok_or(WorkbenchError::NoDocument)?;
        let origin = current.location().ok_or(WorkbenchError::NoDocument)?;
        let Some(viewport) = self.paged_viewport_for_tab(origin.tab) else {
            return Ok(false);
        };
        let start = if forward {
            let last = viewport.rows().last().ok_or(WorkbenchError::NoDocument)?;
            let end = last
                .ending
                .as_ref()
                .map_or(last.content.end, |ending| ending.end);
            let len = self
                .document_by_tab(origin.tab)
                .ok_or(WorkbenchError::NoDocument)?
                .session
                .document()
                .len();
            if end >= len || end <= viewport.origin() {
                return Ok(false);
            }
            end
        } else {
            if viewport.origin() == 0 {
                return Ok(false);
            }
            viewport.origin().saturating_sub(96)
        };
        self.queue_paged_view(
            origin.tab,
            start,
            Some(ViewJump {
                pane,
                view,
                origin,
                offset: None,
                select,
                search_selection: None,
            }),
        )?;
        Ok(true)
    }

    pub(super) fn refresh_paged_view(&mut self, tab: TabId) {
        let Some(document) = self.document_by_tab(tab) else {
            return;
        };
        let Some(old) = document.large_viewport.as_ref() else {
            return;
        };
        if old.matches(document.session.document(), document.session.encoding()) {
            return;
        }
        let origin = old.origin();
        let cursor = self.panes.remembered_cursor(tab).unwrap_or(origin);
        let start = if cursor < origin || cursor > origin.saturating_add(64 * 1024 - 16) {
            cursor.saturating_sub(96)
        } else {
            origin
        };
        if let Err(error) = self.queue_paged_view(tab, start, None) {
            self.push_problem(error.to_string());
        }
    }

    pub(super) fn reveal_paged_search_result(
        &mut self,
        range: std::ops::Range<usize>,
    ) -> Result<(), WorkbenchError> {
        let pane = self.panes.active();
        let current = self.panes.view(pane).ok_or(WorkbenchError::NoDocument)?;
        let view = current.view_id().ok_or(WorkbenchError::NoDocument)?;
        let origin = current.location().ok_or(WorkbenchError::NoDocument)?;
        self.search_navigation_epoch = self.search_navigation_epoch.wrapping_add(1);
        if self.position_for_tab(origin.tab, range.end).is_some() {
            self.move_cursor_to(range.end, false)?;
            self.panes
                .active_state_mut()
                .ok_or(WorkbenchError::NoDocument)?
                .anchor = Some(range.start);
            return Ok(());
        }
        self.queue_paged_view(
            origin.tab,
            range.end.saturating_sub(96),
            Some(ViewJump {
                pane,
                view,
                origin,
                offset: Some(range.end),
                select: false,
                search_selection: Some((
                    self.search.generation,
                    self.search_navigation_epoch,
                    range.start,
                )),
            }),
        )
    }

    fn queue_paged_view(
        &mut self,
        tab: TabId,
        start: usize,
        jump: Option<ViewJump>,
    ) -> Result<(), WorkbenchError> {
        let document = self
            .document_by_tab(tab)
            .ok_or(WorkbenchError::NoDocument)?;
        let source = document.session.document();
        let plan = LargeViewportRequest::new(
            source,
            document.session.encoding(),
            start.min(source.len()),
        )?;
        let request = ViewRequest { tab, plan, jump };
        if self.large_open.view.is_some() {
            self.large_open.next_view = Some(request);
            Ok(())
        } else {
            self.start_paged_view(request)
        }
    }

    fn start_paged_view(&mut self, request: ViewRequest) -> Result<(), WorkbenchError> {
        let (send, result) = mpsc::sync_channel(1);
        let wakeup = self.large_open.wakeup.clone();
        let ViewRequest { tab, plan, jump } = request;
        std::thread::Builder::new()
            .name("cide-vista-paginada".into())
            .spawn(move || {
                let result =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| plan.prepare()))
                        .unwrap_or_else(|_| {
                            Err(io::Error::other("Falló el trabajador de vista paginada"))
                        });
                let _ = send.send(result);
                wakeup.signal();
            })
            .map_err(FileError::from)?;
        self.large_open.view = Some(PendingView { tab, result, jump });
        Ok(())
    }

    pub fn poll_large_view(&mut self) -> Option<Result<(), WorkbenchError>> {
        let pending = self.large_open.view.as_ref()?;
        let result = match pending.result.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => {
                Err(io::Error::other("Se cerró el trabajador de vista paginada"))
            }
        };
        let pending = self.large_open.view.take().unwrap();
        if let Some(next) = self.large_open.next_view.take() {
            let superseded = next.tab == pending.tab;
            if let Err(error) = self.start_paged_view(next) {
                return Some(Err(error));
            }
            if superseded {
                return Some(Ok(()));
            }
        }
        let result = result
            .map_err(FileError::from)
            .map_err(WorkbenchError::from);
        Some(result.and_then(|viewport| {
            if let Some(jump) = &pending.jump {
                if self.panes.active() != jump.pane
                    || self.panes.view(jump.pane).is_none_or(|view| {
                        view.view_id() != Some(jump.view) || view.location() != Some(jump.origin)
                    })
                    || jump.search_selection.is_some_and(|(generation, epoch, _)| {
                        generation != self.search.generation
                            || epoch != self.search_navigation_epoch
                    })
                {
                    return Ok(());
                }
            }
            let Some(document) = self
                .documents
                .iter_mut()
                .find(|document| document.tab_id == pending.tab)
            else {
                return Ok(());
            };
            if !viewport.matches(document.session.document(), document.session.encoding()) {
                return Ok(());
            }
            let offset = pending.jump.as_ref().and_then(|jump| {
                jump.offset
                    .or_else(|| viewport.rows().first().map(|row| row.content.start))
            });
            if let Some(offset) = offset {
                if viewport.position(offset).is_none() {
                    return Err(WorkbenchError::LineCommand(
                        "El byte solicitado no es un límite de texto válido".into(),
                    ));
                }
            }
            let origin_changed = document
                .large_viewport
                .as_ref()
                .is_none_or(|prior| prior.origin() != viewport.origin());
            document.large_viewport = Some(viewport);
            if origin_changed {
                for pane in [PaneId::Primary, PaneId::Secondary] {
                    if self
                        .panes
                        .view(pane)
                        .and_then(|view| view.location())
                        .is_some_and(|location| location.tab == pending.tab)
                    {
                        self.panes.set_scroll(pane, 0, 0);
                    }
                }
            }
            if let (Some(jump), Some(offset)) = (pending.jump, offset) {
                self.move_cursor_to(offset, jump.select)?;
                if let Some((_, _, start)) = jump.search_selection {
                    self.panes
                        .active_state_mut()
                        .ok_or(WorkbenchError::NoDocument)?
                        .anchor = Some(start);
                }
            }
            Ok(())
        }))
    }
}

#[cfg(all(test, windows, target_pointer_width = "64"))]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn settle<T>(wake: &Wakeup, timeout: Duration, mut poll: impl FnMut() -> Option<T>) -> T {
        let deadline = Instant::now() + timeout;
        loop {
            let ticket = wake.ticket();
            if let Some(result) = poll() {
                return result;
            }
            assert!(
                Instant::now() < deadline,
                "El trabajador paginado no completó"
            );
            wake.wait(ticket, Some(Duration::from_millis(20)));
        }
    }

    #[test]
    fn saved_paged_tab_reopens_at_a_byte_and_keeps_its_private_source() {
        let root = std::env::temp_dir();
        let path = root.join(format!("cide-open-worker-{}.txt", std::process::id()));
        let mut contents = vec![b'x'; 200_000];
        let tail = contents.len() - 16;
        contents[tail..].copy_from_slice(b"\r\nultimo texto\r\n");
        std::fs::write(&path, &contents).unwrap();
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
        let _cleanup = Cleanup(path.clone());
        let wake = Wakeup::default();
        let mut bench = Workbench::default();
        bench.set_large_open_wakeup(wake.clone());
        assert_eq!(
            bench
                .request_file_open_in_root(&root, &path, Some(TextEncoding::Utf8), false, 16)
                .unwrap(),
            None
        );
        assert!(bench.active_document().is_none());
        let tab = settle(&wake, Duration::from_secs(10), || bench.poll_large_open()).unwrap();
        assert!(bench.is_paged_tab(tab));
        assert!(bench.active_bytes().is_none());
        assert!(!bench.visual_row(tab, 0, 0, 10).unwrap().is_empty());
        bench
            .set_search_limits(crate::SearchLimits {
                max_results: 2,
                ..Default::default()
            })
            .unwrap();
        bench.set_search_query("x").unwrap();
        settle(&wake, Duration::from_secs(10), || {
            bench.poll_document_search();
            (!bench.search_running()).then_some(())
        });
        assert_eq!(bench.search().status, crate::SearchStatus::Complete);
        assert_eq!(bench.search().total_matches, tail + 1);
        assert_eq!(bench.search().matches.len(), 2);
        bench.toggle_search_regex().unwrap();
        bench.set_search_query("ul.imo").unwrap();
        settle(&wake, Duration::from_secs(10), || {
            bench.poll_document_search();
            (!bench.search_running()).then_some(())
        });
        assert_eq!(bench.search().status, crate::SearchStatus::Complete);
        assert_eq!(bench.find_next().unwrap(), Some(tail + 2..tail + 8));
        settle(&wake, Duration::from_secs(10), || bench.poll_large_view()).unwrap();
        assert_eq!(
            bench.active_editor().unwrap().selection(),
            Some(tail + 2..tail + 8)
        );
        bench.set_search_query("").unwrap();
        bench.go_to_paged_byte(0, false).unwrap();
        settle(&wake, Duration::from_secs(10), || bench.poll_large_view()).unwrap();
        bench.go_to_paged_byte(tail + 3, false).unwrap();
        bench.go_to_paged_byte(tail + 2, false).unwrap();
        settle(&wake, Duration::from_secs(10), || bench.poll_large_view()).unwrap();
        settle(&wake, Duration::from_secs(10), || bench.poll_large_view()).unwrap();
        assert_eq!(bench.active_editor().unwrap().cursor, tail + 2);
        bench.insert_text("á ").unwrap();
        settle(&wake, Duration::from_secs(10), || bench.poll_large_view()).unwrap();
        assert!(bench.active_position().is_some());
        assert!(!bench.request_save_active(None).unwrap());
        assert!(bench.save_pending());
        assert!(bench.close_active(true).is_err());
        bench.insert_text("pendiente ").unwrap();
        settle(&wake, Duration::from_secs(10), || bench.poll_save()).unwrap();
        assert!(bench
            .active_document()
            .unwrap()
            .session
            .document()
            .is_modified());
        contents.splice(tail + 2..tail + 2, "á ".bytes());
        assert_eq!(std::fs::read(&path).unwrap(), contents);
        bench.undo().unwrap();
        assert!(!bench
            .active_document()
            .unwrap()
            .session
            .document()
            .is_modified());
        std::fs::write(&path, b"cambio externo").unwrap();
        assert_eq!(
            bench
                .active_document()
                .unwrap()
                .session
                .document()
                .read_range(tail + 2..tail + 5)
                .unwrap(),
            "á ".as_bytes()
        );
        assert!(!bench.request_save_active(None).unwrap());
        assert!(settle(&wake, Duration::from_secs(10), || bench.poll_save()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"cambio externo");

        let destination = path.with_extension("nuevo.txt");
        let _destination_cleanup = Cleanup(destination.clone());
        std::fs::write(&destination, b"existente").unwrap();
        assert!(!bench
            .request_save_active(Some(destination.clone()))
            .unwrap());
        assert!(settle(&wake, Duration::from_secs(10), || bench.poll_save()).is_err());
        assert_eq!(std::fs::read(&destination).unwrap(), b"existente");
        std::fs::remove_file(&destination).unwrap();
        assert!(!bench
            .request_save_active(Some(destination.clone()))
            .unwrap());
        settle(&wake, Duration::from_secs(10), || bench.poll_save()).unwrap();
        assert_eq!(bench.active_path(), Some(destination.as_path()));
        assert_eq!(std::fs::read(&destination).unwrap(), contents);
        assert_eq!(std::fs::read(&path).unwrap(), b"cambio externo");
    }

    #[test]
    #[ignore = "usa alrededor de 8 GiB de espacio temporal; ejecutar en Windows x64"]
    fn shared_editor_opens_saves_and_recovers_over_four_gib() {
        exercise_large_file(false);
    }

    #[test]
    #[ignore = "usa alrededor de 4 GiB de espacio temporal; ejecutar en Windows x64"]
    fn paged_search_scans_and_selects_above_four_gib() {
        exercise_large_file(true);
    }

    fn exercise_large_file(search_only: bool) {
        use crate::session::EditorRecovery;
        use std::{
            ffi::c_void,
            fs,
            io::{Seek, SeekFrom, Write},
            os::windows::{
                fs::{FileExt, MetadataExt},
                io::AsRawHandle,
            },
        };
        #[link(name = "kernel32")]
        extern "system" {
            fn DeviceIoControl(
                handle: *mut c_void,
                control: u32,
                input: *const c_void,
                input_len: u32,
                output: *mut c_void,
                output_len: u32,
                result: *mut u32,
                overlapped: *mut c_void,
            ) -> i32;
        }
        struct Fixture {
            root: PathBuf,
            base: PathBuf,
        }
        impl Drop for Fixture {
            fn drop(&mut self) {
                if self.root.parent() == Some(self.base.as_path())
                    && self.root.canonicalize().is_ok_and(|path| path == self.root)
                    && fs::symlink_metadata(&self.root)
                        .is_ok_and(|m| m.is_dir() && m.file_attributes() & 0x400 == 0)
                {
                    let _ = fs::remove_dir_all(&self.root);
                }
            }
        }
        let base = std::env::temp_dir().canonicalize().unwrap();
        let root = base.join(format!("cide-editor-grande-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let fixture = Fixture { root, base };
        let path = fixture.root.join("grande.txt");
        let depot = fixture.root.join("sesion");
        let mut file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        let mut result = 0;
        assert_ne!(
            unsafe {
                DeviceIoControl(
                    file.as_raw_handle(),
                    0x900c4,
                    std::ptr::null(),
                    0,
                    std::ptr::null_mut(),
                    0,
                    &mut result,
                    std::ptr::null_mut(),
                )
            },
            0
        );
        const OFFSET: usize = (1usize << 32) + 9;
        file.write_all(b"inicio\r\n").unwrap();
        file.seek(SeekFrom::Start(OFFSET as u64)).unwrap();
        file.write_all(b"\r\nfinal\r\n").unwrap();
        file.sync_all().unwrap();
        drop(file);
        let wake = Wakeup::default();
        let mut workbench = Workbench::default();
        workbench.set_large_open_wakeup(wake.clone());
        assert_eq!(
            workbench
                .request_file_open(&path, Some(TextEncoding::Utf8), false)
                .unwrap(),
            None
        );
        let tab = settle(&wake, Duration::from_secs(300), || {
            workbench.poll_large_open()
        })
        .unwrap();
        assert!(workbench.is_paged_tab(tab));
        if search_only {
            workbench.set_search_query("final").unwrap();
            settle(&wake, Duration::from_secs(300), || {
                workbench.poll_document_search();
                (!workbench.search_running()).then_some(())
            });
            assert_eq!(workbench.search().scanned_bytes, OFFSET + 9);
            assert_eq!(workbench.search().status, crate::SearchStatus::Complete);
            assert_eq!(workbench.find_next().unwrap(), Some(OFFSET + 2..OFFSET + 7));
            settle(&wake, Duration::from_secs(10), || {
                workbench.poll_large_view()
            })
            .unwrap();
            assert_eq!(
                workbench.active_editor().unwrap().selection(),
                Some(OFFSET + 2..OFFSET + 7)
            );
            println!(
                "Búsqueda completa: {} bytes recorridos. Coincidencia seleccionada en {}..{}.",
                OFFSET + 9,
                OFFSET + 2,
                OFFSET + 7
            );
            return;
        }
        workbench
            .go_to_paged_input(&format!("@{}", OFFSET + 2))
            .unwrap();
        settle(&wake, Duration::from_secs(10), || {
            workbench.poll_large_view()
        })
        .unwrap();
        assert_eq!(workbench.active_paged_byte(), Some(OFFSET + 2));
        workbench.insert_text("publicado ").unwrap();
        assert!(!workbench.request_save_active(None).unwrap());
        settle(&wake, Duration::from_secs(300), || workbench.poll_save()).unwrap();
        let mut disk = [0; 10];
        assert_eq!(
            fs::File::open(&path)
                .unwrap()
                .seek_read(&mut disk, OFFSET as u64 + 2)
                .unwrap(),
            disk.len()
        );
        assert_eq!(&disk, b"publicado ");
        workbench.insert_text("pendiente ").unwrap();
        // Verify that recovery uses its immutable arena even if the original
        // disappears. Removing this owned fixture also bounds peak disk usage.
        fs::remove_file(&path).unwrap();
        let mut recovery = Some(EditorRecovery::open_in(&depot).unwrap());
        let mut periodic = crate::session::PeriodicRecovery::default();
        periodic.request_final(&wake);
        settle(&wake, Duration::from_secs(300), || {
            let message = periodic.poll(&mut recovery, &workbench, b"", Instant::now(), &wake);
            periodic.final_result(
                recovery.as_ref(),
                &workbench,
                b"",
                message.as_deref(),
                &wake,
            )
        })
        .unwrap();
        drop(recovery);
        drop(workbench);
        let mut recovery = EditorRecovery::open_in(&depot).unwrap();
        let (mut restored, _, used_backup) = Workbench::restore_recovery(&mut recovery.store)
            .unwrap()
            .unwrap();
        assert!(!used_backup);
        assert_eq!(restored.active_paged_byte(), Some(OFFSET + 22));
        assert!(restored.active_position().is_some());
        let current = restored.active_document().unwrap().session.document();
        assert!(current.is_modified());
        assert!(current.retained_source_bytes() <= 64 * 1024);
        assert_eq!(
            current.read_range(OFFSET + 2..OFFSET + 22).unwrap(),
            b"publicado pendiente "
        );
        restored.undo().unwrap();
        assert!(!restored
            .active_document()
            .unwrap()
            .session
            .document()
            .is_modified());
        println!(
            "Archivos de {} bytes: apertura, guardado, recuperación y deshacer desde el byte {}",
            OFFSET + 9,
            OFFSET + 2
        );
    }
}
