//! TUI intentions over the shared catalog. No enumeration or alternative opener.
use super::TuiApp;
use jc_app::{
    explorer::{ExplorerLoadState, NodeId},
    PrimaryFocus, SidebarView,
};
use std::path::PathBuf;

impl TuiApp {
    pub(crate) fn set_explorer_root(&mut self, path: PathBuf) -> Result<(), String> {
        let root = path.canonicalize().map_err(|e| e.to_string())?;
        if !root.is_dir() {
            return Err("Project root must be a directory".into());
        }
        self.explorer.set_root(root.clone())?;
        self.tasks.cancel_all();
        self.workbench.reset_task_panels();
        self.explorer_view = Default::default();
        self.project_search.clear();
        self.cancel_project_replace();
        self.project_results = false;
        self.git_diff.clear(&mut self.tasks);
        self.git_status.clear();
        self.quick_open.clear();
        self.quick_open_root = None;
        self.git_configuration_target = None;
        self.project_root = Some(root);
        self.panels.show_sidebar(SidebarView::Explorer);
        Ok(())
    }
    pub(crate) fn explorer_directory(&self) -> Option<NodeId> {
        let node = self.explorer.node(self.explorer_view.selected()?)?;
        if node.is_dir() {
            Some(node.id())
        } else {
            node.parent()
        }
    }
    pub(crate) fn activate_explorer(&mut self, side: bool, expand_only: bool) {
        self.explorer_view.prepare(&self.explorer, 0, 0);
        let Some(id) = self.explorer_view.selected().or(self.explorer.root_id()) else {
            return;
        };
        let Some(node) = self.explorer.node(id) else {
            return;
        };
        if node.is_dir() {
            let expanded = node.expanded();
            let first = node.children().first().copied();
            let result = if expanded && expand_only {
                if let Some(child) = first {
                    self.explorer_view.select(child);
                }
                Ok(())
            } else if expanded {
                self.explorer.collapse(id)
            } else {
                self.explorer.expand(id)
            };
            if let Err(error) = result {
                self.status = error;
            }
            return;
        }
        if expand_only {
            return;
        }
        match self.explorer.open_file(
            id,
            &mut self.workbench,
            self.default_encoding,
            side,
            16 * 1024 * 1024,
        ) {
            Ok(_) => {
                self.ensure_encoding_choice();
                if matches!(self.overlay, crate::state::UiOverlay::None) {
                    let _ = self.focus.focus(PrimaryFocus::Editor, &self.panels);
                }
                self.status = "Explorer file opened; shared document retained".into();
            }
            Err(error) => self.status = error,
        }
    }
    pub(crate) fn explorer_parent(&mut self) {
        self.explorer_view.prepare(&self.explorer, 0, 0);
        let Some(id) = self.explorer_view.selected() else {
            return;
        };
        if self
            .explorer
            .node(id)
            .is_some_and(|n| n.is_dir() && n.expanded())
        {
            if let Err(error) = self.explorer.collapse(id) {
                self.status = error;
            }
        } else {
            self.explorer_view.parent(&self.explorer);
        }
    }
    pub(crate) fn refresh_explorer(&mut self) {
        self.status = "Explorer refresh requested".into();
        self.explorer_view.prepare(&self.explorer, 0, 0);
        if let Some(id) = self.explorer_directory().or(self.explorer.root_id()) {
            if let Err(error) = self.explorer.refresh(id) {
                self.status = error;
            }
        }
    }
    pub(crate) fn cancel_explorer(&mut self) {
        self.status = "Explorer: editor focus".into();
        self.explorer_view.prepare(&self.explorer, 0, 0);
        if let Some(id) = self.explorer_directory().or(self.explorer.root_id()) {
            if self.explorer.node(id).is_some_and(|n| {
                matches!(
                    n.state(),
                    ExplorerLoadState::Queued | ExplorerLoadState::Loading
                )
            }) {
                if let Err(error) = self.explorer.cancel(id) {
                    self.status = error;
                }
            }
        }
    }
    pub(crate) fn activate_explorer_entry_to_side(&mut self) {
        if self.panels.sidebar() == SidebarView::Explorer {
            self.activate_explorer(true, false);
        }
    }
}
