use super::TuiApp;
use jc_app::{session::EditorRecovery, BottomPanel, PrimaryFocus, SidebarView, Workbench};
use std::io::{Cursor, Read};

impl TuiApp {
    pub(crate) fn enable_recovery(&mut self) -> Result<(), String> {
        let Some(recovery) = EditorRecovery::open("tui")? else {
            return Ok(());
        };
        self.restore_from(recovery)
    }
    /// Inject an explicit owned depot without changing environment or AppData.
    pub(crate) fn restore_from(&mut self, mut recovery: EditorRecovery) -> Result<(), String> {
        if let Some((workbench, metadata, backup)) =
            Workbench::restore_recovery(&mut recovery.store)?
        {
            let mut reader = Cursor::new(metadata.as_slice());
            let mut flags = [0; 7];
            reader.read_exact(&mut flags).map_err(|e| e.to_string())?;
            if flags[0] != 1
                || [flags[1], flags[2], flags[4]].iter().any(|v| *v > 1)
                || flags[3] > 4
                || flags[5] > 4
                || flags[6] > 2
            {
                return Err("Estado visual de recuperación inválido".into());
            }
            let root = match flags[1] {
                0 => None,
                1 => Some(
                    jc_app::session::read_recovery_path(&mut reader, 32768)
                        .map_err(|e| e.to_string())?,
                ),
                _ => return Err("Raíz de sesión inválida".into()),
            };
            if reader.position() != metadata.len() as u64 {
                return Err("Metadatos de sesión incompletos".into());
            }
            self.workbench = workbench;
            if let Some(root) = root {
                if let Err(error) = self.set_explorer_root(root) {
                    self.status = error;
                }
            }
            let sidebar = match flags[3] {
                0 => SidebarView::Explorer,
                1 => SidebarView::Database,
                2 => SidebarView::Git,
                3 => SidebarView::GitStatus,
                4 => SidebarView::Symbols,
                _ => return Err("Panel de sesión inválido".into()),
            };
            self.panels.show_sidebar(sidebar);
            if flags[2] == 0 {
                self.panels.hide_sidebar();
            }
            let bottom = match flags[5] {
                0 => BottomPanel::Problems,
                1 => BottomPanel::Output,
                2 => BottomPanel::SearchResults,
                3 => BottomPanel::Diff,
                4 => BottomPanel::Terminal,
                _ => return Err("Panel inferior inválido".into()),
            };
            self.panels.show_bottom(bottom);
            if flags[4] == 0 {
                self.panels.hide_bottom();
            }
            let focus = match flags[6] {
                0 => PrimaryFocus::Editor,
                1 => PrimaryFocus::Sidebar,
                2 => PrimaryFocus::BottomPanel,
                _ => PrimaryFocus::Editor,
            };
            let _ = self.focus.focus(focus, &self.panels);
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
    pub(crate) fn checkpoint_session(&mut self) -> Result<(), String> {
        let Some(recovery) = self.recovery.as_mut() else {
            return Ok(());
        };
        let sidebar = match self.panels.sidebar() {
            SidebarView::Explorer => 0,
            SidebarView::Database => 1,
            SidebarView::Git => 2,
            SidebarView::GitStatus => 3,
            SidebarView::Symbols => 4,
        };
        let bottom = match self.panels.bottom() {
            BottomPanel::Problems => 0,
            BottomPanel::Output => 1,
            BottomPanel::SearchResults => 2,
            BottomPanel::Diff => 3,
            BottomPanel::Terminal => 4,
        };
        let focus = match self.focus.primary() {
            PrimaryFocus::Editor => 0,
            PrimaryFocus::Sidebar => 1,
            PrimaryFocus::BottomPanel => 2,
        };
        let mut metadata = vec![
            1,
            u8::from(self.project_root.is_some()),
            u8::from(self.panels.sidebar_visible()),
            sidebar,
            u8::from(self.panels.bottom_visible()),
            bottom,
            focus,
        ];
        if let Some(root) = &self.project_root {
            jc_app::session::write_recovery_path(root, &mut metadata).map_err(|e| e.to_string())?;
        }
        self.workbench
            .checkpoint_recovery(&mut recovery.store, &recovery.base, &metadata)?;
        Ok(())
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::LaunchOptions;
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
                "tui-recovery-{}-{}",
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
    fn tui_recovery_restores_frontend_panels_focus_project_and_dirty_editor_without_environment() {
        let fixture = Fixture::new();
        let original = fixture.root.join("original.p");
        fs::write(&original, b"ni\xf1o\r\nnext\r\n").unwrap();
        let mut app = TuiApp::new(LaunchOptions::default());
        assert!(app.recovery.is_none());
        app.set_explorer_root(fixture.root.clone()).unwrap();
        app.workbench
            .open_file(&original, Some(TextEncoding::Windows1252))
            .unwrap();
        app.workbench.insert_text("á ").unwrap();
        app.workbench.insert_text("temporal").unwrap();
        app.workbench.undo().unwrap();
        let expected = app.workbench.active_bytes().unwrap();
        let revision = app
            .workbench
            .active_document()
            .unwrap()
            .session
            .document()
            .revision();
        app.workbench.split_editor_right();
        app.workbench.focus_editor_pane(PaneId::Secondary);
        app.workbench.toggle_reference_pin();
        app.workbench.toggle_scroll_link();
        app.workbench.focus_editor_pane(PaneId::Primary);
        app.workbench
            .execute(CommandId::ViewToggleInvisibles)
            .unwrap();
        app.panels.show_sidebar(SidebarView::Symbols);
        app.panels.show_bottom(BottomPanel::Output);
        assert!(app.focus.focus(PrimaryFocus::BottomPanel, &app.panels));
        app.restore_from(fixture.recovery()).unwrap();
        app.checkpoint_session().unwrap();
        drop(app);
        let mut recovered = TuiApp::new(LaunchOptions::default());
        recovered.restore_from(fixture.recovery()).unwrap();
        assert_eq!(
            recovered.project_root.as_deref(),
            Some(fixture.root.as_path())
        );
        assert!(recovered.panels.sidebar_visible());
        assert_eq!(recovered.panels.sidebar(), SidebarView::Symbols);
        assert!(recovered.panels.bottom_visible());
        assert_eq!(recovered.panels.bottom(), BottomPanel::Output);
        assert_eq!(recovered.focus.primary(), PrimaryFocus::BottomPanel);
        assert!(recovered.workbench.invisibles_visible());
        assert!(recovered.workbench.panes().scroll_linked());
        assert!(recovered
            .workbench
            .panes()
            .view(PaneId::Secondary)
            .unwrap()
            .is_pinned());
        assert_eq!(recovered.workbench.active_bytes().unwrap(), expected);
        let source = recovered
            .workbench
            .active_document()
            .unwrap()
            .session
            .document();
        assert_eq!(source.revision(), revision);
        assert!(source.is_modified());
        assert_eq!(source.redo_steps(), 1);
        recovered.workbench.redo().unwrap();
        recovered.workbench.undo().unwrap();
        assert_eq!(recovered.workbench.active_bytes().unwrap(), expected);
        assert_eq!(fs::read(&original).unwrap(), b"ni\xf1o\r\nnext\r\n");
        assert!(!recovered.should_exit());
    }
    #[test]
    fn tui_failed_hot_exit_keeps_should_exit_false_and_live_edits_until_checkpoint_retry_succeeds()
    {
        let fixture = Fixture::new();
        let original = fixture.root.join("original.rs");
        fs::write(&original, b"disk\r\n").unwrap();
        let mut app = TuiApp::new(LaunchOptions::default());
        app.workbench
            .open_file(&original, Some(TextEncoding::Utf8))
            .unwrap();
        app.workbench.insert_text("uno ").unwrap();
        app.restore_from(fixture.recovery()).unwrap();
        app.checkpoint_session().unwrap();
        let first = app.workbench.active_bytes().unwrap();
        let head = app
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
        app.workbench.insert_text("dos ").unwrap();
        let live = app.workbench.active_bytes().unwrap();
        app.request_exit();
        assert!(!app.should_exit());
        assert!(app.status.starts_with("No se cerró el editor:"));
        assert_eq!(app.workbench.active_bytes().unwrap(), live);
        assert_eq!(fs::read(&head).unwrap(), head_before);
        drop(lock);
        fs::remove_file(&head).unwrap();
        let (previous, _, backup) =
            Workbench::restore_recovery(&mut app.recovery.as_mut().unwrap().store)
                .unwrap()
                .unwrap();
        assert!(backup);
        assert_eq!(previous.active_bytes().unwrap(), first);
        assert_eq!(app.workbench.active_bytes().unwrap(), live);
        app.request_exit();
        assert!(app.should_exit());
        drop(app);
        let mut recovered = TuiApp::new(LaunchOptions::default());
        recovered.restore_from(fixture.recovery()).unwrap();
        assert_eq!(recovered.workbench.active_bytes().unwrap(), live);
        assert_eq!(fs::read(&original).unwrap(), b"disk\r\n");
    }
}
