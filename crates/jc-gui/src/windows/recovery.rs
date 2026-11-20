use super::State;
use jc_app::{session::EditorRecovery, Workbench};
use std::io::{Cursor, Read};
pub(super) const TIMER_ID: usize = 0xc1de1;
const GUI_METADATA_VERSION: u8 = 8;
const APPEARANCE_BYTES: usize = 6;
const OUTLINE_POSITION_BYTES: usize = 1;
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
            if !matches!(header[0], 1..=GUI_METADATA_VERSION) || header[1..8].iter().any(|v| *v > 1)
            {
                return Err("Estado visual de la sesión inválido".into());
            }
            let width = u32::from_le_bytes(header[8..12].try_into().unwrap());
            let height = u32::from_le_bytes(header[12..16].try_into().unwrap());
            let next = u32::from_le_bytes(header[16..20].try_into().unwrap());
            if !(320..=8192).contains(&width) || !(200..=8192).contains(&height) {
                return Err("Tamaño de ventana recuperado inválido".into());
            }
            let code_visible = if header[0] >= 2 {
                let mut flag = [0];
                reader.read_exact(&mut flag).map_err(|e| e.to_string())?;
                if flag[0] > 1 {
                    return Err("Estado del panel Código inválido".into());
                }
                flag[0] == 1
            } else {
                true
            };
            let root = if header[1] == 1 {
                Some(
                    jc_app::session::read_recovery_path(&mut reader, 32768)
                        .map_err(|e| e.to_string())?,
                )
            } else {
                None
            };
            let _legacy_git = if header[0] == 3 {
                let mut flag = [0];
                reader.read_exact(&mut flag).map_err(|e| e.to_string())?;
                match flag[0] {
                    0 => None,
                    1 => {
                        let path = jc_app::session::read_recovery_path(&mut reader, 4096)
                            .map_err(|e| e.to_string())?;
                        if !path.is_absolute() {
                            return Err("Ruta Git recuperada inválida".into());
                        }
                        Some(path)
                    }
                    _ => return Err("Configuración Git recuperada inválida".into()),
                }
            } else {
                None
            };
            let preferences = if header[0] >= 5 {
                let mut bytes = [0; 7];
                reader.read_exact(&mut bytes).map_err(|e| e.to_string())?;
                let mut validation = super::git_workspace::Workspace::default();
                validation.restore_preferences(&bytes)?;
                Some(bytes)
            } else {
                None
            };
            let appearance = if header[0] >= 6 {
                let mut bytes = [0; APPEARANCE_BYTES];
                reader.read_exact(&mut bytes).map_err(|e| e.to_string())?;
                let [ui, editor, zoom, visible, lo, hi] = bytes;
                let width = match u16::from_le_bytes([lo, hi]) {
                    76 => super::appearance::DEFAULT_MINIMAP,
                    width => width,
                };
                if !super::appearance::valid(ui, editor, zoom, visible, width) {
                    return Err("Preferencias de apariencia recuperadas inválidas".into());
                }
                Some((ui, editor, zoom, visible == 1, width))
            } else {
                None
            };
            let outline_left = if header[0] >= 7 {
                let mut side = [0; OUTLINE_POSITION_BYTES];
                reader.read_exact(&mut side).map_err(|e| e.to_string())?;
                match side[0] {
                    0 => false,
                    1 => true,
                    _ => return Err("Posición recuperada de Esquema inválida".into()),
                }
            } else {
                false
            };
            let editor_theme = if header[0] >= 8 {
                let mut id = [0];
                reader.read_exact(&mut id).map_err(|e| e.to_string())?;
                super::appearance::Theme::from_id(id[0]).ok_or("Tema recuperado inválido")?
            } else {
                super::appearance::Theme::Current
            };
            if reader.position() != metadata.len() as u64 {
                return Err("Metadatos de ventana incompletos".into());
            }
            self.workbench = workbench;
            self.editor_theme = editor_theme;
            self.outline.visible = code_visible;
            self.outline.left = outline_left;
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
            if let Some(bytes) = preferences {
                self.git_workspace.restore_preferences(&bytes)?;
            }
            if let Some((ui, editor, zoom, visible, width)) = appearance {
                self.ui_text_size = ui;
                self.editor_text_size = editor;
                self.editor_zoom = zoom;
                self.minimap_visible = visible;
                self.minimap_width = width;
            }
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
            GUI_METADATA_VERSION,
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
        metadata.push(u8::from(self.outline.visible));
        if let Some(root) = &self.project_root {
            jc_app::session::write_recovery_path(root, &mut metadata).map_err(|e| e.to_string())?;
        }
        metadata.extend_from_slice(&self.git_workspace.preference_bytes());
        metadata.extend_from_slice(&[
            self.ui_text_size,
            self.editor_text_size,
            self.editor_zoom,
            u8::from(self.minimap_visible),
        ]);
        metadata.extend_from_slice(&self.minimap_width.to_le_bytes());
        metadata.push(u8::from(self.outline.left));
        metadata.push(self.editor_theme as u8);
        Ok(metadata)
    }
    #[cfg(test)]
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
    pub(super) fn request_exit(&mut self) -> Result<bool, String> {
        if self.workbench.save_pending() || self.save_all_active {
            self.exit_waiting_for_save = true;
            self.status = "Cerrando cuando termine el guardado…".into();
            return Ok(false);
        }
        self.exit_waiting_for_save = false;
        if self.recovery.is_none() && !self.periodic_recovery.active() {
            if self.workbench.modified_documents().next().is_some() {
                return Err(
                    "La recuperación está desactivada. Guarda los cambios antes de cerrar".into(),
                );
            }
            return Ok(true);
        }
        self.periodic_recovery.request_final(&self.wakeup);
        self.status = "Guardando la sesión antes de cerrar…".into();
        Ok(false)
    }
    pub(super) fn poll_periodic_recovery(&mut self) -> bool {
        let now = std::time::Instant::now();
        let metadata = if self.recovery.is_some() && self.periodic_recovery.needs_capture(now) {
            match self.recovery_metadata() {
                Ok(value) => value,
                Err(error) => {
                    self.periodic_recovery.defer_failure(now);
                    if self.periodic_recovery.finalizing() {
                        self.periodic_recovery.cancel_final();
                        self.error(format!("No se cerró el editor: {error}"));
                    } else {
                        self.status = error;
                    }
                    return true;
                }
            }
        } else {
            Vec::new()
        };
        let message = self.periodic_recovery.poll(
            &mut self.recovery,
            &self.workbench,
            &metadata,
            now,
            &self.wakeup,
        );
        if self.periodic_recovery.finalizing()
            && !self.periodic_recovery.active()
            && !self.workbench.save_pending()
            && !self.save_all_active
        {
            let result = self.recovery_metadata().and_then(|live| {
                self.periodic_recovery
                    .final_result(
                        self.recovery.as_ref(),
                        &self.workbench,
                        &live,
                        message.as_deref(),
                        &self.wakeup,
                    )
                    .transpose()
            });
            match result {
                Ok(Some(())) => {
                    self.exit_ready = true;
                    return true;
                }
                Err(error) => {
                    self.periodic_recovery.cancel_final();
                    self.error(format!("No se cerró el editor: {error}"));
                    return true;
                }
                Ok(None) => {}
            }
        }
        if let Some(message) = message {
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
    fn settle_exit(state: &mut State) {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while state.periodic_recovery.finalizing() {
            assert!(
                std::time::Instant::now() < until,
                "Cierre pendiente: {}",
                state.status
            );
            state.poll_periodic_recovery();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
    #[test]
    fn paged_gui_opens_draws_saves_and_recovers_an_unsaved_edit() {
        let fixture = Fixture::new();
        let path = fixture.root.join("paginas.txt");
        let mut bytes = vec![b'x'; 140_000];
        bytes.extend_from_slice(b"\r\nfinal\r\n");
        fs::write(&path, &bytes).unwrap();
        let cursor = bytes.len() - 7;
        let mut state = State::new(&GuiOptions::default()).unwrap();
        state.restore_from(fixture.recovery()).unwrap();
        assert_eq!(
            state
                .workbench
                .request_file_open_in_root(
                    &fixture.root,
                    &path,
                    Some(TextEncoding::Utf8),
                    false,
                    16
                )
                .unwrap(),
            None
        );
        let poll_until = |state: &mut State, done: fn(&State) -> bool| {
            let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while !done(state) {
                assert!(
                    std::time::Instant::now() < until,
                    "Entrega de archivo paginado incompleta"
                );
                state.poll_tasks();
                state.poll_periodic_recovery();
                std::thread::yield_now();
            }
        };
        poll_until(&mut state, |state| {
            state.workbench.active_paged_byte().is_some()
        });
        state.open_search_panel(false);
        state.edit_search_input(jc_app::FieldEdit::Insert("final"));
        state.execute_menu(CommandId::SearchFindNext);
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while state.workbench.selection_for_pane(PaneId::Primary) != Some(cursor..cursor + 5) {
            assert!(
                std::time::Instant::now() < until,
                "Buscar no saltó al resultado paginado"
            );
            state.poll_tasks();
            std::thread::yield_now();
        }
        state.close_selector();
        state.workbench.set_search_query("").unwrap();
        state.open_navigation_panel();
        let input = format!("@{cursor}");
        state.edit_navigation_input(jc_app::FieldEdit::Insert(&input));
        state.navigation_key(0x0d, jc_app::Modifiers::NONE);
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while state.workbench.active_paged_byte() != Some(cursor) {
            assert!(std::time::Instant::now() < until, "El salto no llegó");
            state.poll_tasks();
            std::thread::yield_now();
        }
        super::super::paint::prepare(&mut state);
        state.workbench.insert_text("guardado ").unwrap();
        state.execute_menu(CommandId::FileSave);
        assert!(state.workbench.save_pending());
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if let Some(result) = state.workbench.poll_save() {
                result.unwrap();
                break;
            }
            assert!(std::time::Instant::now() < until);
            std::thread::yield_now();
        }
        bytes.splice(cursor..cursor, b"guardado ".iter().copied());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        state.workbench.insert_text("pendiente ").unwrap();
        assert!(!state.request_exit().unwrap());
        assert!(!state.exit_ready);
        poll_until(&mut state, |state| state.exit_ready);
        drop(state);
        let mut restored = State::new(&GuiOptions::default()).unwrap();
        restored.restore_from(fixture.recovery()).unwrap();
        assert_eq!(restored.workbench.active_paged_byte(), Some(cursor + 19));
        assert!(restored.workbench.active_position().is_some());
        let source = restored
            .workbench
            .active_document()
            .unwrap()
            .session
            .document();
        assert!(source.is_modified());
        assert_eq!(
            source.read_range(cursor..cursor + 19).unwrap(),
            b"guardado pendiente "
        );
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
    #[test]
    fn gui_exit_without_recovery_keeps_dirty_document_and_history() {
        let mut state = State::new(&GuiOptions::default()).unwrap();
        state.workbench = Workbench::default();
        state
            .workbench
            .new_file("sin-guardar.txt", TextEncoding::Utf8);
        state.workbench.insert_text("pendiente\r\n").unwrap();
        assert!(state.request_exit().is_err());
        assert_eq!(state.workbench.active_bytes().unwrap(), b"pendiente\r\n");
        state.workbench.undo().unwrap();
        assert_eq!(state.workbench.active_bytes().unwrap(), b"");
    }
    #[test]
    fn gui_empty_recovered_session_restarts_draft_names_after_old_counter() {
        let fixture = Fixture::new();
        let mut state = State::new(&GuiOptions::default()).unwrap();
        state.workbench = Workbench::default();
        state.next_untitled = 5;
        state.outline.visible = false;
        state.restore_from(fixture.recovery()).unwrap();
        assert!(state.checkpoint_session().unwrap());
        drop(state);
        let mut recovered = State::new(&GuiOptions::default()).unwrap();
        recovered.restore_from(fixture.recovery()).unwrap();
        assert!(!recovered.outline.visible);
        recovered.new_document();
        assert_eq!(
            recovered
                .workbench
                .active_document()
                .unwrap()
                .session
                .path()
                .file_name()
                .unwrap(),
            "Sin título 1.txt"
        );
    }
    #[test]
    fn gui_legacy_visual_metadata_restores_with_code_panel_default() {
        let fixture = Fixture::new();
        let state = State::new(&GuiOptions::default()).unwrap();
        let mut legacy = state.recovery_metadata().unwrap();
        legacy.truncate(legacy.len() - 7 - APPEARANCE_BYTES - OUTLINE_POSITION_BYTES - 1);
        legacy[0] = 1;
        legacy.remove(20);
        let mut recovery = fixture.recovery();
        state
            .workbench
            .checkpoint_recovery(&mut recovery.store, &recovery.base, &legacy)
            .unwrap();
        drop(recovery);
        let mut recovered = State::new(&GuiOptions::default()).unwrap();
        recovered.restore_from(fixture.recovery()).unwrap();
        assert!(recovered.outline.visible);
    }
    #[test]
    fn gui_recovery_ignores_legacy_git_path_and_writes_embedded_metadata() {
        let fixture = Fixture::new();
        let state = State::new(&GuiOptions::default()).unwrap();
        let mut legacy = state.recovery_metadata().unwrap();
        assert_eq!(legacy[0], GUI_METADATA_VERSION);
        legacy.truncate(legacy.len() - 7 - APPEARANCE_BYTES - OUTLINE_POSITION_BYTES - 1);
        legacy[0] = 3;
        legacy.push(1);
        let path = fixture.root.join("obsolete-git.exe");
        jc_app::session::write_recovery_path(&path, &mut legacy).unwrap();
        let mut depot = fixture.recovery();
        state
            .workbench
            .checkpoint_recovery(&mut depot.store, &depot.base, &legacy)
            .unwrap();
        drop(depot);
        let mut restored = State::new(&GuiOptions::default()).unwrap();
        restored.restore_from(fixture.recovery()).unwrap();
        assert_eq!(
            restored.recovery_metadata().unwrap()[0],
            GUI_METADATA_VERSION
        );
        assert!(!restored.git_status_due);
        drop(restored);
        let other = Fixture::new();
        let mut v2 = state.recovery_metadata().unwrap();
        v2.truncate(v2.len() - 7 - APPEARANCE_BYTES - OUTLINE_POSITION_BYTES - 1);
        v2[0] = 2;
        let mut depot = other.recovery();
        state
            .workbench
            .checkpoint_recovery(&mut depot.store, &depot.base, &v2)
            .unwrap();
        drop(depot);
        let mut restored = State::new(&GuiOptions::default()).unwrap();
        restored.restore_from(other.recovery()).unwrap();
        assert_eq!(
            restored.recovery_metadata().unwrap()[0],
            GUI_METADATA_VERSION
        );
    }
    #[test]
    fn gui_git_layout_preferences_survive_restart_and_v4_defaults_remain_readable() {
        let fixture = Fixture::new();
        let mut state = State::new(&GuiOptions::default()).unwrap();
        let prefs = [1, 2, 250, 0, 244, 1, 1];
        state.git_workspace.restore_preferences(&prefs).unwrap();
        state.ui_text_size = 16;
        state.editor_text_size = 20;
        state.editor_zoom = 125;
        state.minimap_visible = false;
        state.editor_theme = super::super::appearance::Theme::Light;
        state.minimap_width = 200;
        state.outline.left = true;
        state.clear_git();
        assert_eq!(state.git_workspace.preference_bytes(), prefs);
        let mut depot = fixture.recovery();
        state
            .workbench
            .checkpoint_recovery(
                &mut depot.store,
                &depot.base,
                &state.recovery_metadata().unwrap(),
            )
            .unwrap();
        drop(depot);
        let mut restored = State::new(&GuiOptions::default()).unwrap();
        restored.restore_from(fixture.recovery()).unwrap();
        assert_eq!(restored.git_workspace.preference_bytes(), prefs);
        assert_eq!(
            (
                restored.ui_text_size,
                restored.editor_text_size,
                restored.editor_zoom
            ),
            (16, 20, 125)
        );
        assert!(!restored.minimap_visible);
        assert_eq!(
            restored.editor_theme,
            super::super::appearance::Theme::Light
        );
        assert_eq!(restored.minimap_width, 200);
        assert!(restored.outline.left);
        drop(restored);
        let legacy = Fixture::new();
        let defaults = State::new(&GuiOptions::default()).unwrap();
        let mut metadata = defaults.recovery_metadata().unwrap();
        metadata.truncate(metadata.len() - 7 - APPEARANCE_BYTES - OUTLINE_POSITION_BYTES - 1);
        metadata[0] = 4;
        let mut depot = legacy.recovery();
        defaults
            .workbench
            .checkpoint_recovery(&mut depot.store, &depot.base, &metadata)
            .unwrap();
        drop(depot);
        let mut restored = State::new(&GuiOptions::default()).unwrap();
        restored.restore_from(legacy.recovery()).unwrap();
        assert_eq!(
            restored.git_workspace.preference_bytes(),
            defaults.git_workspace.preference_bytes()
        );
        assert_eq!(restored.editor_zoom, super::super::appearance::DEFAULT_ZOOM);
        assert!(!restored.outline.left);
    }
    #[test]
    fn gui_v6_appearance_keeps_its_values_and_defaults_outline_to_right() {
        let fixture = Fixture::new();
        let mut state = State::new(&GuiOptions::default()).unwrap();
        state.editor_zoom = 125;
        state.minimap_width = 76;
        state.outline.left = true;
        let mut v6 = state.recovery_metadata().unwrap();
        v6.truncate(v6.len() - OUTLINE_POSITION_BYTES - 1);
        v6[0] = 6;
        let mut recovery = fixture.recovery();
        state
            .workbench
            .checkpoint_recovery(&mut recovery.store, &recovery.base, &v6)
            .unwrap();
        drop(recovery);

        let mut restored = State::new(&GuiOptions::default()).unwrap();
        restored.restore_from(fixture.recovery()).unwrap();
        assert_eq!(
            restored.editor_theme,
            super::super::appearance::Theme::Current
        );
        assert!(!restored.outline.left);
        assert_eq!(restored.editor_zoom, 125);
        assert_eq!(
            restored.minimap_width,
            super::super::appearance::DEFAULT_MINIMAP
        );
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
            .new_file(fixture.root.join("Sin título 1.txt"), TextEncoding::Utf8);
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
            super::super::untitled::next_name(&recovered.workbench),
            "Sin título 2.txt"
        );
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
        assert!(!fixture.root.join("Sin título 1.txt").exists());
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
        assert!(!state.request_exit().unwrap());
        settle_exit(&mut state);
        assert!(!state.exit_ready && state.status.starts_with("No se cerró el editor:"));
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
        assert!(!state.request_exit().unwrap());
        settle_exit(&mut state);
        assert!(state.exit_ready);
        drop(state);
        let mut recovered = State::new(&GuiOptions::default()).unwrap();
        recovered.restore_from(fixture.recovery()).unwrap();
        assert_eq!(recovered.workbench.active_bytes().unwrap(), live);
        assert_eq!(fs::read(&original).unwrap(), b"disk\r\n");
    }

    #[test]
    fn gui_hot_exit_finishes_active_periodic_writer_then_checkpoints_latest_edits() {
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
        // receives an edit that the final checkpoint must capture.
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
        assert!(!state.request_exit().unwrap());
        assert!(!state.exit_ready);
        settle_exit(&mut state);
        assert!(state.exit_ready);
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
