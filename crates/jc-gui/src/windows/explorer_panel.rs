//! Sidebar routing uses catalog IDs, never labels. Only the actual painted frame
//! publishes hit metadata; generation/revision are rechecked before activation.
use super::{ffi::Rect, State};
use jc_app::{
    explorer::{ExplorerLoadState, ExplorerPresentation, NodeId},
    Modifiers,
};

pub(super) struct ExplorerFrame {
    pub rect: Rect,
    pub presentation: ExplorerPresentation,
    pub focused: bool,
}
pub(super) struct ExplorerHits {
    pub generation: u64,
    pub revision: u64,
    pub rect: Rect,
    pub rows: Vec<(NodeId, usize, Rect)>,
}
impl ExplorerFrame {
    pub(super) fn hits(&self, ch: i32) -> ExplorerHits {
        ExplorerHits {
            generation: self.presentation.generation,
            revision: self.presentation.revision,
            rect: self.rect,
            rows: self
                .presentation
                .rows
                .iter()
                .take(128)
                .enumerate()
                .map(|(i, row)| {
                    let top = self.rect.top + (i as i32 + 2) * ch;
                    (
                        row.id,
                        row.indent / 2,
                        Rect {
                            left: self.rect.left,
                            top,
                            right: self.rect.right,
                            bottom: (top + ch).min(self.rect.bottom),
                        },
                    )
                })
                .collect(),
        }
    }
}
pub(super) fn region(state: &State) -> Option<Rect> {
    let width = state.width.clamp(1, 8192);
    let height = state.height.clamp(1, 8192);
    if !state.explorer_visible || width < 640 || height < 200 {
        return None;
    }
    let cw = state.cell_width.clamp(1, 8192);
    let ch = state.cell_height.clamp(1, 8192);
    if width / cw < 60 || height / ch < 8 {
        return None;
    }
    Some(Rect {
        left: 0,
        top: ch + 8,
        right: (cw * 28).clamp(160, 320).min(width / 3),
        bottom: (height - ch - 8).max(ch + 8),
    })
}
impl State {
    pub(super) fn show_explorer(&mut self) {
        let collapse = self.explorer_visible && self.explorer_focused;
        self.close_selector();
        if collapse {
            self.explorer_visible = false;
            self.explorer_focused = false;
            self.explorer_hits = None;
            return;
        }
        self.explorer_visible = true;
        self.explorer_focused = region(self).is_some();
        self.explorer_hits = None;
        if self.project_root.is_none() {
            self.status = "Open a project folder to browse".into();
        }
    }
    pub(super) fn poll_explorer(&mut self) -> bool {
        let mut changed = false;
        if let Some(root) = &self.project_root {
            changed |= self.explorer.poll(root, 2);
        }
        if self.explorer.take_recovery() {
            self.status = "Explorer worker recovered from failure; refresh directory".into();
            changed = true;
        }
        if self.explorer.ready() {
            self.bridge.signal();
        }
        changed
    }
    pub(super) fn explorer_presentation(&mut self) -> Option<ExplorerFrame> {
        if self.explorer.root() != self.project_root.as_deref() {
            self.explorer.clear();
            self.explorer_hits = None;
        }
        let rect = region(self)?;
        let rows =
            ((rect.bottom - rect.top) / self.cell_height.clamp(1, 8192) - 2).clamp(0, 128) as usize;
        let columns =
            ((rect.right - rect.left - 12) / self.cell_width.clamp(1, 8192)).clamp(0, 256) as usize;
        Some(ExplorerFrame {
            rect,
            presentation: self.explorer_view.prepare(&self.explorer, rows, columns),
            focused: self.explorer_focused,
        })
    }
    fn explorer_directory(&self) -> Option<NodeId> {
        let node = self
            .explorer
            .node(self.explorer_view.selected().or(self.explorer.root_id())?)?;
        if node.is_dir() {
            Some(node.id())
        } else {
            node.parent()
        }
    }
    pub(super) fn refresh_explorer(&mut self) {
        self.explorer_view.prepare(&self.explorer, 0, 0);
        self.explorer_hits = None;
        if let Some(id) = self.explorer_directory() {
            if let Err(error) = self.explorer.refresh(id) {
                self.error(error);
            }
        }
    }
    pub(super) fn cancel_explorer(&mut self) {
        self.explorer_view.prepare(&self.explorer, 0, 0);
        if let Some(id) = self.explorer_directory() {
            if self.explorer.node(id).is_some_and(|n| {
                matches!(
                    n.state(),
                    ExplorerLoadState::Queued | ExplorerLoadState::Loading
                )
            }) {
                if let Err(error) = self.explorer.cancel(id) {
                    self.error(error);
                }
            }
        }
        self.explorer_focused = false;
    }
    pub(super) fn activate_explorer(&mut self, side: bool, expand_only: bool) {
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
            let result = if expand_only && expanded {
                if let Some(id) = first {
                    self.explorer_view.select(id);
                }
                Ok(())
            } else if expanded {
                self.explorer.collapse(id)
            } else {
                self.explorer.expand(id)
            };
            self.explorer_hits = None;
            if let Err(error) = result {
                self.error(error);
            }
            return;
        }
        if expand_only {
            return;
        }
        match self.explorer.open_file(
            id,
            &mut self.workbench,
            self.encoding,
            side,
            16 * 1024 * 1024,
        ) {
            Ok(_) => {
                self.explorer_focused = false;
                self.status = "Explorer file opened; shared document retained".into();
                self.ensure_encoding_selector();
            }
            Err(error) => self.error(error),
        }
    }
    pub(super) fn explorer_key(&mut self, code: usize, modifiers: Modifiers) {
        self.explorer_view.prepare(&self.explorer, 0, 0);
        match code {
            0x26 => self.explorer_view.move_selection(&self.explorer, -1),
            0x28 => self.explorer_view.move_selection(&self.explorer, 1),
            0x21 => self.explorer_view.move_selection(&self.explorer, -10),
            0x22 => self.explorer_view.move_selection(&self.explorer, 10),
            0x24 => self.explorer_view.first(&self.explorer),
            0x23 => self.explorer_view.last(&self.explorer),
            0x27 => self.activate_explorer(false, true),
            0x0d => self.activate_explorer(modifiers.contains(Modifiers::CTRL), false),
            0x25 | 0x08 => {
                if let Some(id) = self.explorer_view.selected() {
                    if self
                        .explorer
                        .node(id)
                        .is_some_and(|n| n.is_dir() && n.expanded())
                    {
                        if let Err(error) = self.explorer.collapse(id) {
                            self.error(error);
                        }
                    } else {
                        self.explorer_view.parent(&self.explorer);
                    }
                }
            }
            _ => {}
        }
    }
    pub(super) fn explorer_click(&mut self, x: i32, y: i32, double: bool) -> bool {
        let Some(region) = region(self) else {
            return false;
        };
        if x < region.left || x >= region.right || y < region.top || y >= region.bottom {
            return false;
        }
        let Some(hits) = &self.explorer_hits else {
            return true;
        };
        if hits.generation != self.explorer.generation()
            || hits.revision != self.explorer.revision()
            || hits.rect.left != region.left
            || hits.rect.right != region.right
            || hits.rect.top != region.top
            || hits.rect.bottom != region.bottom
        {
            return true;
        }
        let Some((id, depth, _)) = hits
            .rows
            .iter()
            .find(|(_, _, r)| x >= r.left && x < r.right && y >= r.top && y < r.bottom)
            .copied()
        else {
            return true;
        };
        let Some(node) = self.explorer.node(id) else {
            return true;
        };
        let is_dir = node.is_dir();
        let disclosure = x < region.left + 8 + (depth.min(32) * 2 + 3) as i32 * self.cell_width;
        self.explorer_focused = true;
        self.explorer_view.select(id);
        if (is_dir && disclosure && !double) || (double && !disclosure) || (double && !is_dir) {
            let side = self.modifiers().contains(Modifiers::CTRL);
            self.activate_explorer(side, false);
        }
        true
    }
}
