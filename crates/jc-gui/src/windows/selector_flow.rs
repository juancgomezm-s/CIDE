//! Owner-thread selector routing. Workers supply metadata; only this message
//! path accepts their results. Painting only prepares bounded visible rows.
use super::{
    selector::{Selector, SelectorKind, SelectorPresentation},
    supported, Action, State,
};
use jc_app::Modifiers;
use std::path::PathBuf;

impl State {
    pub(super) fn set_project_root(&mut self, path: PathBuf) -> Result<(), String> {
        let root = path.canonicalize().map_err(|e| e.to_string())?;
        if !root.is_dir() {
            return Err("Project root must be a directory".into());
        }
        self.close_selector();
        self.project_root = Some(root);
        self.status = "Project folder selected · Ctrl+P Quick Open".into();
        Ok(())
    }
    pub(super) fn close_selector(&mut self) {
        self.selector = None;
        self.search_panel = None;
        self.encoding_target = None;
        self.pending_surrogate = None;
        self.quick_open.clear();
    }
    pub(super) fn open_selector(&mut self, kind: SelectorKind) {
        self.close_selector();
        if kind == SelectorKind::Files && self.project_root.is_none() {
            self.status = "Open a project folder first".into();
            return;
        }
        self.selector = Some(Selector::new(kind));
        self.request_files();
    }
    pub(super) fn ensure_encoding_selector(&mut self) {
        if self.workbench.active_document().is_some() && self.workbench.active_encoding().is_none()
        {
            self.open_encoding_selector();
        }
    }
    pub(super) fn open_encoding_selector(&mut self) {
        self.close_selector();
        let Some(document) = self.workbench.active_document() else {
            return;
        };
        self.encoding_target = Some((
            document.id,
            document.session.document().revision(),
            document.session.encoding(),
        ));
        self.selector = Some(Selector::new(SelectorKind::Encoding));
    }
    pub(super) fn request_files(&mut self) {
        let Some(selector) = self
            .selector
            .as_ref()
            .filter(|s| s.kind() == SelectorKind::Files)
        else {
            return;
        };
        let Some(root) = &self.project_root else {
            self.close_selector();
            return;
        };
        if let Err(error) = self
            .quick_open
            .request(root.clone(), selector.input().to_owned())
        {
            self.status = format!("Quick Open request rejected: {error:?}");
        }
    }
    pub(super) fn poll_tasks(&mut self) -> bool {
        let failed = self.bridge.acknowledge() | self.wakeup.take_notifier_failure();
        let mut changed = failed;
        self.deliveries = self.deliveries.saturating_add(1);
        if let (Some(root), Some(selector)) = (
            &self.project_root,
            self.selector
                .as_ref()
                .filter(|s| s.kind() == SelectorKind::Files),
        ) {
            changed |= self.quick_open.poll(root, selector.input());
        } else {
            self.quick_open.clear();
        }
        if self.quick_open.take_recovery() {
            changed = true;
            self.status = "Quick Open worker recovered from a failure; refresh to retry".into();
        }
        if failed {
            self.status = "Background notification failed; refresh Quick Open".into();
        }
        // Completion publication can race the acknowledgement. Coalescing leaves
        // at most one queued message and readiness closes the notification gap.
        if self.quick_open.ready() {
            self.bridge.signal();
        }
        changed |= self.workbench.poll_document_search();
        self.schedule_search();
        changed
    }
    pub(super) fn selector_presentation(&mut self, rows: usize) -> Option<SelectorPresentation> {
        let context = self.context();
        let selector = self.selector.as_mut()?;
        Some(match selector.kind() {
            SelectorKind::Palette => {
                selector.prepare_palette(&self.registry, &self.keymap, &context, supported, rows)
            }
            SelectorKind::Files => selector.prepare_files(&self.quick_open, rows),
            SelectorKind::Encoding => selector.prepare_encoding(rows),
        })
    }
    pub(super) fn selector_key(&mut self, code: usize, modifiers: Modifiers) -> Action {
        match code {
            0x1b => {
                self.close_selector();
                self.status = "Selector cancelled".into();
            }
            0x08 => {
                if self.selector.as_mut().is_some_and(Selector::backspace) {
                    self.request_files();
                }
            }
            0x26 | 0x28 | 0x21 | 0x22 => {
                // Normalize BEFORE a relative move, so End/overshoot cannot
                // leave a saturated index and make Up appear unresponsive.
                self.selector_presentation(0);
                if let Some(selector) = self.selector.as_mut() {
                    selector.move_selection(match code {
                        0x26 => -1,
                        0x28 => 1,
                        0x21 => -10,
                        _ => 10,
                    });
                }
                self.selector_presentation(0);
            }
            0x24 => {
                if let Some(selector) = self.selector.as_mut() {
                    selector.first();
                }
            }
            0x23 => {
                if let Some(selector) = self.selector.as_mut() {
                    selector.last();
                }
                self.selector_presentation(0);
            }
            0x74 => self.request_files(),
            0x0d => return self.accept_selector(modifiers.contains(Modifiers::CTRL)),
            _ => {}
        }
        Action::None
    }
    fn accept_selector(&mut self, side: bool) -> Action {
        self.selector_presentation(0);
        let Some(selector) = self.selector.as_ref() else {
            return Action::None;
        };
        match selector.kind() {
            SelectorKind::Encoding => {
                let Some(encoding) = selector.encoding_target() else {
                    return Action::None;
                };
                let current = self
                    .workbench
                    .active_document()
                    .map(|d| (d.id, d.session.document().revision(), d.session.encoding()));
                if self.encoding_target.is_none() || current != self.encoding_target {
                    self.close_selector();
                    self.status = "Encoding target changed; reopen the selector".into();
                    return Action::None;
                }
                match self.workbench.set_encoding(encoding) {
                    Ok(()) => {
                        self.close_selector();
                        self.status = format!(
                            "Interpretation: {} · bytes unchanged; no conversion/save",
                            encoding.name()
                        );
                    }
                    Err(error) => self.error(error),
                }
                Action::None
            }
            SelectorKind::Palette => {
                let Some(command) = selector.palette_target(
                    &self.registry,
                    &self.keymap,
                    &self.context(),
                    supported,
                ) else {
                    return Action::None;
                };
                if !supported(command) || !self.registry.is_enabled(command, &self.context()) {
                    self.status = "Command unavailable in this frontend/context".into();
                    return Action::None;
                }
                self.close_selector();
                self.execute(command)
            }
            SelectorKind::Files => {
                let Some(root) = &self.project_root else {
                    self.close_selector();
                    return Action::None;
                };
                let target =
                    self.quick_open
                        .selected_target(root, selector.input(), selector.selected());
                match target {
                    Ok(path) => match self.workbench.open_project_file(
                        root,
                        path,
                        self.encoding,
                        side,
                        16 * 1024 * 1024,
                    ) {
                        Ok(_) => {
                            self.close_selector();
                            self.status = "Opened project file · shared document/undo".into();
                            self.ensure_encoding_selector();
                        }
                        Err(error) => self.error(error),
                    },
                    Err(error) => self.status = format!("Quick Open target unavailable: {error:?}"),
                }
                Action::None
            }
        }
    }
}
