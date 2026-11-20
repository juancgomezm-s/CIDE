//! Read-only directory navigation and bounded tabs over one historical comparison.
use super::*;

const FILE_TABS: usize = 8;

#[derive(Default)]
pub(in crate::windows) struct HistoryBrowser {
    showing_tree: bool,
    directory: Vec<u8>,
    selected: usize,
    tabs: Vec<Vec<u8>>,
    active: Option<usize>,
}

impl HistoryBrowser {
    pub(super) fn showing_tree(&self) -> bool {
        self.showing_tree
    }

    fn remember(&mut self, path: &[u8]) -> Option<usize> {
        let index = if let Some(index) = self.tabs.iter().position(|tab| *tab == path) {
            index
        } else {
            if self.tabs.len() == FILE_TABS {
                return None;
            }
            self.tabs.push(path.to_vec());
            self.tabs.len() - 1
        };
        self.active = Some(index);
        Some(index)
    }
}

impl State {
    pub(in crate::windows) fn show_git_history_summary(&mut self) {
        self.git_workspace.history_file = None;
        self.git_workspace.history_browser.active = None;
        self.git_workspace.invalidate();
    }

    pub(in crate::windows) fn set_git_history_tree(&mut self, tree: bool) {
        if self.git_revision.revision().is_none() {
            return;
        }
        let browser = &mut self.git_workspace.history_browser;
        if tree && self.git_revision.directory().is_none() {
            let path = browser.directory.clone();
            if !self.request_git_history_directory(path) {
                return;
            }
        } else {
            browser.showing_tree = tree;
        }
        self.git_workspace.active = Column::Files;
        self.git_workspace.focus_diff = false;
        self.git_workspace.invalidate();
    }

    fn request_git_history_directory(&mut self, path: Vec<u8>) -> bool {
        let Some(root) = self.git_root().map(std::path::Path::to_owned) else {
            return false;
        };
        if self.git_workspace.history_file.is_some()
            && self.git_revision.comparison().is_none()
            && matches!(
                self.git_revision.state(),
                jc_app::git_revision::GitRevisionState::Queued
                    | jc_app::git_revision::GitRevisionState::Running
            )
        {
            self.git_workspace.notice = Some("Espera a que termine la comparación".into());
            self.git_workspace.invalidate();
            return false;
        }
        if let Err(error) = self.git_revision.request_directory(root, path.clone()) {
            self.git_workspace.notice = Some(error);
            self.git_workspace.invalidate();
            return false;
        }
        let browser = &mut self.git_workspace.history_browser;
        browser.directory = path;
        browser.selected = 0;
        browser.showing_tree = true;
        self.git_workspace.notice = None;
        self.git_workspace.invalidate();
        true
    }

    fn accepted_git_history_directory(&self) -> Option<&jc_app::git_revision::RevisionDirectory> {
        let browser = &self.git_workspace.history_browser;
        self.git_revision
            .directory()
            .filter(|directory| directory.path == browser.directory)
    }

    fn git_history_tree_rows(&self) -> usize {
        self.accepted_git_history_directory()
            .map_or(0, |directory| directory.entries.len())
            + usize::from(!self.git_workspace.history_browser.directory.is_empty())
    }

    pub(in crate::windows) fn move_git_history_files(&mut self, delta: isize) {
        if self.git_workspace.history_browser.showing_tree {
            let last = self.git_history_tree_rows().saturating_sub(1);
            let selection = &mut self.git_workspace.history_browser.selected;
            *selection = selection.saturating_add_signed(delta).min(last);
        } else {
            let total = self.git_revision.revision().map_or(0, |r| r.files.len());
            let selection = &mut self.git_workspace.history_selected;
            *selection = selection
                .saturating_add_signed(delta)
                .min(total.saturating_sub(1));
        }
    }

    pub(in crate::windows) fn open_git_history_tree_entry(&mut self, index: usize) {
        let browser = &self.git_workspace.history_browser;
        if !browser.showing_tree {
            return;
        }
        if index == 0 && !browser.directory.is_empty() {
            let parent = browser
                .directory
                .iter()
                .rposition(|&byte| byte == b'/')
                .map_or_else(Vec::new, |i| browser.directory[..i].to_vec());
            self.request_git_history_directory(parent);
            return;
        }
        let Some((directory, entry)) =
            self.accepted_git_history_directory().and_then(|directory| {
                directory
                    .entries
                    .get(index - usize::from(!directory.path.is_empty()))
                    .map(|entry| (directory, entry))
            })
        else {
            return;
        };
        let mut path = Vec::with_capacity(
            directory.path.len() + usize::from(!directory.path.is_empty()) + entry.name.len(),
        );
        path.extend_from_slice(&directory.path);
        if !path.is_empty() {
            path.push(b'/');
        }
        path.extend_from_slice(&entry.name);
        if entry.is_directory() {
            self.request_git_history_directory(path);
        } else if entry.is_comparable() {
            self.git_workspace.history_browser.selected = index;
            self.request_git_history_path(path);
        }
    }

    pub(super) fn open_selected_git_history_tree_entry(&mut self) {
        self.open_git_history_tree_entry(self.git_workspace.history_browser.selected);
    }

    pub(in crate::windows) fn open_git_history_tab(&mut self, index: usize) {
        if let Some(path) = self.git_workspace.history_browser.tabs.get(index).cloned() {
            self.request_git_history_path(path);
        }
    }

    pub(in crate::windows) fn step_git_history_tab(&mut self, delta: isize) {
        let browser = &self.git_workspace.history_browser;
        if browser.tabs.is_empty() {
            return;
        }
        let index = match browser.active {
            Some(index) => {
                (index as isize + delta).rem_euclid(browser.tabs.len() as isize) as usize
            }
            None if delta < 0 => browser.tabs.len() - 1,
            None => 0,
        };
        self.open_git_history_tab(index);
    }

    pub(in crate::windows) fn close_git_history_tab(&mut self) {
        let browser = &mut self.git_workspace.history_browser;
        if let Some(index) = browser.active.take() {
            browser.tabs.remove(index);
            if browser.tabs.is_empty() {
                self.show_git_history_summary();
            } else {
                let next = index.min(browser.tabs.len() - 1);
                self.open_git_history_tab(next);
            }
        }
    }

    pub(super) fn request_git_history_path(&mut self, path: Vec<u8>) {
        let Some(root) = self.git_root().map(std::path::Path::to_owned) else {
            return;
        };
        let Some(revision) = self.git_revision.revision() else {
            return;
        };
        let commit = revision.commit.clone();
        let file_index = revision
            .files
            .binary_search_by(|file| file.path.cmp(&path))
            .ok();
        let browser = &mut self.git_workspace.history_browser;
        if browser.remember(&path).is_none() {
            self.git_workspace.notice = Some(format!(
                "Cierra una pestaña para abrir otra (máximo {FILE_TABS})"
            ));
            self.git_workspace.invalidate();
            return;
        }
        self.git_workspace.active = Column::Comparison;
        self.git_workspace.diff_column = 0;
        self.git_workspace.focus_diff = true;
        if let Some(index) = file_index {
            self.git_workspace.history_selected = index;
        }
        self.git_workspace.history_file = Some(file_index.unwrap_or(usize::MAX));
        self.git_workspace.history_hunk = 0;
        self.git_workspace.diff_first = 0;
        self.git_workspace.notice = self
            .git_revision
            .request_revision_file(root, commit, path)
            .err();
        self.git_workspace.invalidate();
    }

    pub(super) fn git_history_browser_key(&mut self, code: usize, modifiers: Modifiers) -> bool {
        if self.git_revision.revision().is_none() {
            return false;
        }
        if modifiers == Modifiers::CTRL {
            match code {
                0x21 => self.step_git_history_tab(-1),
                0x22 => self.step_git_history_tab(1),
                0x57 => self.close_git_history_tab(),
                _ => return false,
            }
            return true;
        }
        if modifiers != Modifiers::NONE {
            return false;
        }
        match code {
            0x54 => self.set_git_history_tree(!self.git_workspace.history_browser.showing_tree),
            0x08 if self.git_workspace.history_browser.showing_tree
                && !self.git_workspace.focus_diff
                && !self.git_workspace.history_browser.directory.is_empty() =>
            {
                self.open_git_history_tree_entry(0);
            }
            _ => return false,
        }
        true
    }

    pub(super) fn prepare_git_history_browser(&self, frame: &mut Frame, l: &Layout) {
        frame.history_browser = true;
        let p = |v| self.dpi.pixels(v);
        let browser = &self.git_workspace.history_browser;
        for (index, (name, tree)) in [("Cambios", false), ("Archivos", true)]
            .into_iter()
            .enumerate()
        {
            let left = l.files.left + p(5 + index as i32 * 77);
            let r = rect(left, l.files.top + p(4), left + p(74), l.files.top + p(31));
            if r.right <= l.files.right {
                frame
                    .buttons
                    .push((r, name.into(), browser.showing_tree == tree));
                frame.hits.actions.push((r, Intent::HistoryFileMode(tree)));
            }
        }
        frame.footer = format!("T cambios/archivos · {}", frame.footer);
        self.prepare_git_history_tabs(frame, l);
    }

    fn prepare_git_history_tabs(&self, frame: &mut Frame, l: &Layout) {
        let p = |v| self.dpi.pixels(v);
        let browser = &self.git_workspace.history_browser;
        let left = l.preview.left + p(5);
        let right = l.preview.right - p(5);
        let top = l.preview.top + p(2);
        let bottom = l.preview.top + p(29);
        let summary = rect(left, top, left + p(88), bottom);
        if summary.right > right {
            return;
        }
        frame.buttons.push((
            summary,
            "Resumen".into(),
            self.git_workspace.history_file.is_none(),
        ));
        frame.hits.actions.push((summary, Intent::HistorySummary));
        if self.git_workspace.history_file.is_none() {
            let local = rect(
                left,
                l.preview.top + p(128),
                left + p(132),
                l.preview.top + p(152),
            );
            if local.right <= right && local.bottom <= l.preview.bottom {
                frame.buttons.push((local, "Cambios locales".into(), false));
                frame.hits.actions.push((local, Intent::Local));
            }
        }
        if browser.tabs.is_empty() {
            return;
        }
        let tabs_left = summary.right + p(4);
        let slots = ((right - tabs_left - p(78)) / p(103).max(1)).max(1) as usize;
        let overflow = browser.tabs.len() > slots;
        let first = browser.active.unwrap_or(0).saturating_sub(slots - 1);
        let end = right - p(if overflow { 52 } else { 0 });
        let width = ((end - tabs_left) / slots as i32).clamp(0, p(175));
        for (index, path) in browser.tabs.iter().enumerate().skip(first).take(slots) {
            let x = tabs_left + (index - first) as i32 * width;
            let selected =
                self.git_workspace.history_file.is_some() && browser.active == Some(index);
            let close_width = if selected { p(23) } else { 0 };
            let r = rect(x, top, x + width - close_width - p(3), bottom);
            if r.right - r.left < p(24) || r.right > end {
                break;
            }
            let basename = path.rsplit(|&byte| byte == b'/').next().unwrap_or(path);
            frame.buttons.push((r, opaque(basename, 80), selected));
            frame.hits.actions.push((r, Intent::HistoryTab(index)));
            if selected {
                let close = rect(r.right, top, r.right + close_width, bottom);
                frame.buttons.push((close, "×".into(), false));
                frame.hits.actions.push((close, Intent::HistoryTabClose));
            }
        }
        if overflow {
            for (index, (text, delta)) in [("‹", -1), ("›", 1)].into_iter().enumerate() {
                let x = end + index as i32 * p(26);
                let r = rect(x, top, x + p(24), bottom);
                if r.left >= tabs_left {
                    frame.buttons.push((r, text.into(), false));
                    frame.hits.actions.push((r, Intent::HistoryTabStep(delta)));
                }
            }
        }
        frame.footer = format!(
            "Ctrl+W cerrar pestaña · Ctrl+RePág/AvPág recorrer · {}",
            frame.footer
        );
    }

    pub(super) fn prepare_git_history_tree_rows(&self, frame: &mut Frame, l: &Layout) {
        let browser = &self.git_workspace.history_browser;
        let directory = self.accepted_git_history_directory();
        let up = usize::from(!browser.directory.is_empty());
        let total = self.git_history_tree_rows();
        let capacity =
            ((l.files.bottom - l.files.top - l.head) / l.row.max(1)).clamp(0, 128) as usize;
        let first = browser
            .selected
            .saturating_sub(capacity.saturating_sub(1))
            .min(total.saturating_sub(capacity));
        let location = if up == 0 {
            "Raíz del repositorio".into()
        } else {
            opaque(&browser.directory, 180)
        };
        frame.count = format!("{total} entradas");
        frame.footer = format!("{location} · {}", frame.footer);
        frame.message = if directory.is_none() && up == 0 {
            self.git_revision.message().into()
        } else if total == 0 {
            "El árbol de la revisión está vacío".into()
        } else {
            String::new()
        };
        if directory.is_none() && up > 0 {
            frame.footer = format!("{} · {}", self.git_revision.message(), frame.footer);
        }
        for index in first..total.min(first + capacity) {
            let top = l.files.top + l.head + (index - first) as i32 * l.row;
            let r = rect(
                l.files.left,
                top,
                l.files.right,
                (top + l.row).min(l.files.bottom),
            );
            let (name, tag, detail, open) = if index < up {
                ("..".into(), "↑", location.clone(), true)
            } else if let Some(entry) =
                directory.and_then(|directory| directory.entries.get(index - up))
            {
                let (tag, detail) = if entry.is_directory() {
                    ("▸", "Carpeta")
                } else if entry.is_comparable() {
                    ("·", "Archivo de la revisión")
                } else {
                    ("·", "Entrada especial · sin comparación")
                };
                (
                    opaque(&entry.name, 256),
                    tag,
                    detail.into(),
                    entry.is_directory() || entry.is_comparable(),
                )
            } else {
                continue;
            };
            frame.rows.push(FileRow {
                rect: r,
                name,
                detail,
                tag,
                color: ACCENT,
                selected: index == browser.selected,
            });
            if open {
                frame
                    .hits
                    .actions
                    .push((r, Intent::HistoryTreeEntry(index)));
            }
        }
    }
}
