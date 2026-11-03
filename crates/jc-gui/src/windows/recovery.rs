use super::State;
use jc_app::{session::EditorRecovery, Workbench};
use std::io::{Cursor, Read};
pub(super) const TIMER_ID: usize = 0xc1de1;
#[link(name = "user32")]
extern "system" {
    fn SetTimer(hwnd: super::ffi::Hwnd, id: usize, milliseconds: u32, callback: usize) -> usize;
    fn KillTimer(hwnd: super::ffi::Hwnd, id: usize) -> i32;
}
pub(super) unsafe fn arm(hwnd: super::ffi::Hwnd) {
    let Some(owner) = super::owner(hwnd) else {
        return;
    };
    let delay = {
        let state = owner.state.borrow();
        if state.periodic_recovery.active() {
            Some(250)
        } else if state.recovery.is_some() {
            state.periodic_recovery.deadline().map(|deadline| {
                deadline
                    .saturating_duration_since(std::time::Instant::now())
                    .as_millis()
                    .clamp(10, u32::MAX as u128) as u32
            })
        } else {
            None
        }
    };
    if delay == Some(10)
        && owner
            .state
            .borrow()
            .periodic_recovery
            .deadline()
            .is_some_and(|deadline| deadline <= std::time::Instant::now())
    {
        owner.state.borrow().wakeup.signal();
    }
    if let Some(delay) = delay {
        if SetTimer(hwnd, TIMER_ID, delay, 0) == 0 {
            owner
                .state
                .borrow_mut()
                .error("No se pudo programar la recuperación automática");
        }
    } else {
        KillTimer(hwnd, TIMER_ID);
    }
}

impl State {
    pub(super) fn enable_recovery(&mut self) -> Result<(), String> {
        let Some(recovery) = EditorRecovery::open("gui")? else {
            return Ok(());
        };
        self.restore_from(recovery)
    }
    /// Explicit depot injection keeps fixtures independent of process environment
    /// and of the user's AppData. Production enable_recovery opens its own depot.
    pub(super) fn restore_from(&mut self, mut recovery: EditorRecovery) -> Result<(), String> {
        if let Some((workbench, metadata, backup)) =
            Workbench::restore_recovery(&mut recovery.store)?
        {
            let mut reader = Cursor::new(metadata.as_slice());
            let mut header = [0; 20];
            reader.read_exact(&mut header).map_err(|e| e.to_string())?;
            if header[0] != 1 || header[1..8].iter().any(|v| *v > 1) {
                return Err("Estado visual de la sesión inválido".into());
            }
            let width = u32::from_le_bytes(header[8..12].try_into().unwrap());
            let height = u32::from_le_bytes(header[12..16].try_into().unwrap());
            let next = u32::from_le_bytes(header[16..20].try_into().unwrap());
            if !(320..=8192).contains(&width) || !(200..=8192).contains(&height) {
                return Err("Tamaño de ventana recuperado inválido".into());
            }
            let root = if header[1] == 1 {
                Some(
                    jc_app::session::read_recovery_path(&mut reader, 32768)
                        .map_err(|e| e.to_string())?,
                )
            } else {
                None
            };
            if reader.position() != metadata.len() as u64 {
                return Err("Metadatos de ventana incompletos".into());
            }
            self.workbench = workbench;
            if let Some(root) = root {
                if let Err(error) = self.set_project_root(root) {
                    self.status = error;
                }
            }
            self.explorer_visible = header[2] == 1;
            self.explorer_focused = header[3] == 1 && self.explorer_visible;
            self.bottom.visible = header[4] == 1;
            self.bottom.focused = header[5] == 1 && self.bottom.visible;
            self.bottom.kind = if header[6] == 1 {
                jc_app::task_panels::PanelKind::Output
            } else {
                jc_app::task_panels::PanelKind::Problems
            };
            self.restore_window = Some((width as i32, height as i32, header[7] == 1));
            self.next_untitled = next as usize;
            self.status = if backup {
                "Sesión recuperada desde la copia válida anterior"
            } else {
                "Sesión recuperada con cambios sin guardar"
            }
            .into();
        }
        self.recovery = Some(recovery);
        Ok(())
    }
    pub(super) fn recovery_metadata(&self) -> Result<Vec<u8>, String> {
        let mut metadata = vec![
            1,
            u8::from(self.project_root.is_some()),
            u8::from(self.explorer_visible),
            u8::from(self.explorer_focused),
            u8::from(self.bottom.visible),
            u8::from(self.bottom.focused),
            u8::from(self.bottom.kind == jc_app::task_panels::PanelKind::Output),
            u8::from(self.maximized),
        ];
        metadata.extend_from_slice(&(self.width.clamp(320, 8192) as u32).to_le_bytes());
        metadata.extend_from_slice(&(self.height.clamp(200, 8192) as u32).to_le_bytes());
        metadata
            .extend_from_slice(&(self.next_untitled.min(u32::MAX as usize) as u32).to_le_bytes());
        if let Some(root) = &self.project_root {
            jc_app::session::write_recovery_path(root, &mut metadata).map_err(|e| e.to_string())?;
        }
        Ok(metadata)
    }
    pub(super) fn checkpoint_session(&mut self) -> Result<bool, String> {
        self.periodic_recovery.cancel_and_join(&mut self.recovery);
        if self.recovery.is_none() {
            return Ok(false);
        }
        let metadata = self.recovery_metadata()?;
        let recovery = self.recovery.as_mut().unwrap();
        let _ = recovery.store.prune_obsolete();
        self.workbench
            .checkpoint_recovery(&mut recovery.store, &recovery.base, &metadata)?;
        let _ = recovery.store.prune_obsolete();
        Ok(true)
    }
    pub(super) fn poll_periodic_recovery(&mut self) -> bool {
        let now = std::time::Instant::now();
        let metadata = if self.recovery.is_some() && self.periodic_recovery.needs_capture(now) {
            match self.recovery_metadata() {
                Ok(value) => value,
                Err(error) => {
                    self.periodic_recovery.defer_failure(now);
                    self.status = error;
                    return true;
                }
            }
        } else {
            Vec::new()
        };
        if let Some(message) = self.periodic_recovery.poll(
            &mut self.recovery,
            &self.workbench,
            &metadata,
            now,
            &self.wakeup,
        ) {
            self.status = message;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GuiOptions;
    use jc_app::{CommandId, PaneId};
    use jc_text::TextEncoding;
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Fixture {
        root: PathBuf,
        base: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .canonicalize()
                .unwrap();
            let base = workspace.join("work/test-temp").canonicalize().unwrap();
            assert!(base.starts_with(&workspace));
            let root = base.join(format!(
                "gui-recovery-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            Self { root, base }
        }
        fn recovery(&self) -> EditorRecovery {
            let mut result = EditorRecovery::open_in(&self.root.join("recovery")).unwrap();
            result.base = self.root.clone();
            result
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
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
    #[test]
    fn gui_recovery_restores_window_panels_project_and_dirty_editor_without_environment_or_original_writes(
    ) {
        let fixture = Fixture::new();
        let original = fixture.root.join("original.rs");
        let bytes = b"\xef\xbb\xbfbase\r\nnext\r\n";
        fs::write(&original, bytes).unwrap();
        let mut state = State::new(&GuiOptions::default()).unwrap();
        assert!(state.recovery.is_none());
        state.workbench = Workbench::default();
        state.set_project_root(fixture.root.clone()).unwrap();
        state
            .workbench
            .open_file(&original, Some(TextEncoding::Utf8))
            .unwrap();
        state.workbench.insert_text("prefijo ").unwrap();
        state.workbench.insert_text("temporal").unwrap();
        state.workbench.undo().unwrap();
        let expected_original = state.workbench.active_bytes().unwrap();
        let undo = state
            .workbench
            .active_document()
            .unwrap()
            .session
            .document()
            .undo_steps();
        assert!(state.workbench.split_editor_right());
        assert!(state.workbench.focus_editor_pane(PaneId::Secondary));
        assert_eq!(state.workbench.toggle_reference_pin(), Some(true));
        assert_eq!(state.workbench.toggle_scroll_link(), Some(true));
        state.workbench.focus_editor_pane(PaneId::Primary);
        state
            .workbench
            .new_file(fixture.root.join("borrador.rs"), TextEncoding::Utf8);
        state.workbench.insert_text("borrador\r\n").unwrap();
        state
            .workbench
            .execute(CommandId::ViewToggleInvisibles)
            .unwrap();
        state.width = 1234;
        state.height = 777;
        state.maximized = true;
        state.next_untitled = 42;
        state.explorer_visible = true;
        state.explorer_focused = false;
        state.bottom.visible = true;
        state.bottom.focused = true;
        state.bottom.kind = jc_app::task_panels::PanelKind::Output;
        state.restore_from(fixture.recovery()).unwrap();
        assert!(state.checkpoint_session().unwrap());
        drop(state);
        let mut recovered = State::new(&GuiOptions::default()).unwrap();
        recovered.restore_from(fixture.recovery()).unwrap();
        assert_eq!(recovered.restore_window, Some((1234, 777, true)));
        assert_eq!(recovered.next_untitled, 42);
        assert_eq!(
            recovered.project_root.as_deref(),
            Some(fixture.root.as_path())
        );
        assert!(recovered.explorer_visible);
        assert!(!recovered.explorer_focused);
        assert!(recovered.bottom.visible && recovered.bottom.focused);
        assert_eq!(
            recovered.bottom.kind,
            jc_app::task_panels::PanelKind::Output
        );
        assert!(recovered.workbench.invisibles_visible());
        assert!(recovered.workbench.panes().scroll_linked());
        assert!(recovered
            .workbench
            .panes()
            .view(PaneId::Secondary)
            .unwrap()
            .is_pinned());
        assert_eq!(recovered.workbench.tabs().tabs().len(), 2);
        assert_eq!(recovered.workbench.active_bytes().unwrap(), b"borrador\r\n");
        assert!(recovered.workbench.select_tab(1));
        assert_eq!(
            recovered.workbench.active_bytes().unwrap(),
            expected_original
        );
        let source = recovered
            .workbench
            .active_document()
            .unwrap()
            .session
            .document();
        assert_eq!(source.undo_steps(), undo);
        assert_eq!(source.redo_steps(), 1);
        assert!(source.is_modified());
        recovered.workbench.redo().unwrap();
        recovered.workbench.undo().unwrap();
        assert_eq!(
            recovered.workbench.active_bytes().unwrap(),
            expected_original
        );
        assert_eq!(fs::read(&original).unwrap(), bytes);
        assert!(!fixture.root.join("borrador.rs").exists());
    }
    #[test]
    fn gui_failed_hot_exit_checkpoint_keeps_live_edits_and_last_confirmed_backup_until_retry() {
        let fixture = Fixture::new();
        let original = fixture.root.join("original.rs");
        fs::write(&original, b"disk\r\n").unwrap();
        let mut state = State::new(&GuiOptions::default()).unwrap();
        state.workbench = Workbench::default();
        state
            .workbench
            .open_file(&original, Some(TextEncoding::Utf8))
            .unwrap();
        state.workbench.insert_text("uno ").unwrap();
        state.restore_from(fixture.recovery()).unwrap();
        assert!(state.checkpoint_session().unwrap());
        let first = state.workbench.active_bytes().unwrap();
        let head = state
            .recovery
            .as_ref()
            .unwrap()
            .store
            .slot_path()
            .join("HEAD");
        let head_before = fs::read(&head).unwrap();
        let lock = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&head)
            .unwrap();
        state.workbench.insert_text("dos ").unwrap();
        let live = state.workbench.active_bytes().unwrap();
        assert!(state.checkpoint_session().is_err());
        assert_eq!(state.workbench.active_bytes().unwrap(), live);
        assert!(state
            .workbench
            .active_document()
            .unwrap()
            .session
            .document()
            .is_modified());
        assert_eq!(fs::read(&head).unwrap(), head_before);
        drop(lock);
        fs::remove_file(&head).unwrap();
        let (previous, _, backup) =
            Workbench::restore_recovery(&mut state.recovery.as_mut().unwrap().store)
                .unwrap()
                .unwrap();
        assert!(backup);
        assert_eq!(previous.active_bytes().unwrap(), first);
        assert_eq!(state.workbench.active_bytes().unwrap(), live);
        assert!(state.checkpoint_session().unwrap());
        drop(state);
        let mut recovered = State::new(&GuiOptions::default()).unwrap();
        recovered.restore_from(fixture.recovery()).unwrap();
        assert_eq!(recovered.workbench.active_bytes().unwrap(), live);
        assert_eq!(fs::read(&original).unwrap(), b"disk\r\n");
    }

    #[test]
    fn gui_hot_exit_joins_active_periodic_writer_then_checkpoints_latest_edits() {
        use jc_app::session::{PeriodicRecovery, RecoveryPolicy};
        use std::time::{Duration, Instant};
        let fixture = Fixture::new();
        let original = fixture.root.join("original.rs");
        let bytes = b"\xef\xbb\xbforiginal\r\n";
        fs::write(&original, bytes).unwrap();
        let mut state = State::new(&GuiOptions::default()).unwrap();
        state.workbench = Workbench::default();
        state
            .workbench
            .open_file(&original, Some(TextEncoding::Utf8))
            .unwrap();
        state.workbench.insert_text("primero ").unwrap();
        state.restore_from(fixture.recovery()).unwrap();
        assert!(state.checkpoint_session().unwrap());
        let baseline = state.workbench.active_bytes().unwrap();
        let mut baseline_encoded = Vec::new();
        state
            .workbench
            .active_document()
            .unwrap()
            .session
            .document()
            .write_recovery(&mut baseline_encoded)
            .unwrap();
        let now = Instant::now();
        state.periodic_recovery = PeriodicRecovery::new(
            RecoveryPolicy {
                idle: Duration::from_millis(1),
                maximum_delay: Duration::from_millis(10),
                retry: Duration::from_millis(10),
            },
            now,
        )
        .unwrap();
        let metadata = state.recovery_metadata().unwrap();
        assert!(state
            .periodic_recovery
            .poll(
                &mut state.recovery,
                &state.workbench,
                &metadata,
                now + Duration::from_millis(1),
                &state.wakeup
            )
            .is_none());
        assert!(state.periodic_recovery.active() && state.recovery.is_none());
        // No End has been emitted. The worker owns the store while the model
        // receives an edit that must be captured by the final synchronous save.
        state.workbench.insert_text("último ").unwrap();
        let latest = state.workbench.active_bytes().unwrap();
        let caret = state
            .workbench
            .panes()
            .view(PaneId::Primary)
            .unwrap()
            .state()
            .unwrap()
            .cursor;
        let undo = state
            .workbench
            .active_document()
            .unwrap()
            .session
            .document()
            .undo_steps();
        assert!(state.checkpoint_session().unwrap());
        assert!(!state.periodic_recovery.active() && state.recovery.is_some());
        let mut backup = state
            .recovery
            .as_mut()
            .unwrap()
            .store
            .load_backup()
            .unwrap()
            .unwrap();
        let mut previous = Vec::new();
        std::io::Read::read_to_end(&mut backup.documents[0], &mut previous).unwrap();
        assert_eq!(previous, baseline_encoded);
        drop(backup);
        drop(state);
        let mut recovered = State::new(&GuiOptions::default()).unwrap();
        recovered.restore_from(fixture.recovery()).unwrap();
        assert_eq!(recovered.workbench.active_bytes().unwrap(), latest);
        let source = recovered
            .workbench
            .active_document()
            .unwrap()
            .session
            .document();
        assert!(source.is_modified());
        assert_eq!(source.undo_steps(), undo);
        assert_eq!(
            recovered
                .workbench
                .panes()
                .view(PaneId::Primary)
                .unwrap()
                .state()
                .unwrap()
                .cursor,
            caret
        );
        recovered.workbench.undo().unwrap();
        assert_eq!(recovered.workbench.active_bytes().unwrap(), baseline);
        recovered.workbench.redo().unwrap();
        assert_eq!(recovered.workbench.active_bytes().unwrap(), latest);
        assert_eq!(fs::read(&original).unwrap(), bytes);
    }
}
