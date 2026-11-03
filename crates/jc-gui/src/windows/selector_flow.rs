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
        self.explorer.set_root(root.clone())?;
        self.jobs.cancel_all();
        self.workbench.reset_task_panels();
        self.bottom_hits = None;
        self.explorer_view = Default::default();
        self.explorer_hits = None;
        self.close_selector();
        self.clear_git();
        self.project_search.clear();
        self.project_panel = None;
        self.project_options = None;
        self.project_root = Some(root);
        self.status = "Project folder selected · Ctrl+P Quick Open".into();
        Ok(())
    }
    pub(super) fn close_selector(&mut self) {
        self.selector_generation = self.selector_generation.saturating_add(1);
        self.cancel_pointer();
        self.selector_editor_input = false;
        self.selector_edit_target = None;
        self.selector_search_panel = None;
        self.selector_explorer_focus = false;
        self.explorer_focused = false;
        self.git_panel = None;
        self.cancel_project_replace();
        self.selector = None;
        self.search_panel = None;
        self.navigation_panel = None;
        if let Some(panel) = self.project_panel.as_mut() {
            panel.visible = false;
        }
        self.encoding_target = None;
        self.pending_surrogate = None;
        self.quick_open.clear();
    }
    pub(super) fn open_selector(&mut self, kind: SelectorKind) {
        let explorer_focus = self.explorer_focused || self.selector_explorer_focus;
        let editor_input = self.context().editor_input;
        let edit_target = self.source_edit_target();
        let search = if kind == SelectorKind::Palette {
            self.search_panel.take()
        } else {
            None
        };
        self.close_selector();
        if kind == SelectorKind::Files && self.project_root.is_none() {
            self.status = "Open a project folder first".into();
            return;
        }
        if self.selector_generation == u64::MAX {
            self.error("Selector generation exhausted");
            return;
        }
        self.selector = Some(Selector::new(kind));
        self.selector_explorer_focus = explorer_focus;
        self.selector_search_panel = search;
        self.selector_editor_input = kind == SelectorKind::Palette && editor_input;
        self.selector_edit_target = (kind == SelectorKind::Palette)
            .then_some(edit_target)
            .flatten();
        if kind == SelectorKind::Palette {
            self.workbench.cancel_search_navigation();
        }
        self.request_files();
    }
    pub(super) fn ensure_encoding_selector(&mut self) {
        if self.workbench.active_document().is_some() && self.workbench.active_encoding().is_none()
        {
            self.open_encoding_selector();
        }
    }
    pub(super) fn open_encoding_selector(&mut self) {
        let explorer_focus = self.explorer_focused || self.selector_explorer_focus;
        self.close_selector();
        let Some(document) = self.workbench.active_document() else {
            return;
        };
        self.encoding_target = Some((
            document.id,
            document.session.document().revision(),
            document.session.encoding(),
        ));
        if self.selector_generation == u64::MAX {
            self.error("Selector generation exhausted");
            return;
        }
        self.selector = Some(Selector::new(SelectorKind::Encoding));
        self.selector_explorer_focus = explorer_focus;
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
        let mut changed = failed | self.syntax_pending;
        if self.syntax_pending {
            self.syntax_pending = false;
            let mut previous = None;
            for (pane, tab) in self.syntax_targets.into_iter().flatten() {
                if previous != Some(tab)
                    && self
                        .workbench
                        .panes()
                        .view(pane)
                        .and_then(|v| v.location())
                        .is_some_and(|loc| loc.tab == tab)
                {
                    self.syntax_pending |= self.workbench.continue_syntax(tab);
                    previous = Some(tab);
                }
            }
            if self.syntax_pending {
                self.wakeup.signal();
            }
        }
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
            self.status = "Background notification failed; refresh the active query".into();
        }
        // Completion publication can race the acknowledgement. Coalescing leaves
        // at most one queued message and readiness closes the notification gap.
        if self.quick_open.ready() {
            self.bridge.signal();
        }
        changed |= self.workbench.poll_document_search();
        changed |= self.project_search.poll(&self.workbench, 2);
        changed |= self.apply_project_replace_step();
        changed |= self.poll_git();
        changed |= self.poll_explorer();
        if self.project_search.take_recovery() {
            self.status = "Project search worker failed; refresh to retry".into();
            changed = true;
        }
        if self.project_search.ready() {
            self.bridge.signal();
        }
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
        use jc_app::{CommandId, FieldEdit};
        if code == 0x1b {
            let search = self.selector_search_panel.take();
            let explorer = self.selector_explorer_focus;
            self.close_selector();
            self.search_panel = search;
            self.explorer_focused = explorer;
            self.status = "Selector cancelled".into();
            return Action::None;
        }
        if modifiers.contains(Modifiers::CTRL) && !modifiers.contains(Modifiers::ALT) {
            let command = match code {
                0x41 => Some(CommandId::EditSelectAll),
                0x43 => Some(CommandId::EditCopy),
                0x58 => Some(CommandId::EditCut),
                0x56 => Some(CommandId::EditPaste),
                _ => None,
            };
            if let Some(command) = command {
                return self.execute(command);
            }
            if code == 0x55 {
                self.edit_selector_input(FieldEdit::Clear, false);
                return Action::None;
            }
            if code == 0x24 || code == 0x23 {
                if let Some(selector) = self.selector.as_mut() {
                    if code == 0x24 {
                        selector.first();
                    } else {
                        selector.last();
                    }
                }
                self.selector_presentation(0);
                return Action::None;
            }
            if code != 0x0d {
                return Action::None;
            }
        }
        if modifiers.contains(Modifiers::ALT) {
            return Action::None;
        }
        let selecting = modifiers.contains(Modifiers::SHIFT);
        let edit = match code {
            0x08 => Some(FieldEdit::Backspace),
            0x2e => Some(FieldEdit::Delete),
            0x25 => Some(FieldEdit::Left),
            0x27 => Some(FieldEdit::Right),
            0x24 => Some(FieldEdit::Home),
            0x23 => Some(FieldEdit::End),
            _ => None,
        };
        if let Some(edit) = edit {
            self.edit_selector_input(edit, selecting);
            return Action::None;
        }
        match code {
            0x26 | 0x28 | 0x21 | 0x22 | 0x09 => {
                self.selector_presentation(0);
                if let Some(selector) = self.selector.as_mut() {
                    selector.move_selection(match code {
                        0x26 => -1,
                        0x28 => 1,
                        0x21 => -10,
                        0x22 => 10,
                        _ => {
                            if selecting {
                                -1
                            } else {
                                1
                            }
                        }
                    });
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
                        self.bottom.focused = false;
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
                let explorer_focus = self.selector_explorer_focus;
                let search = self.selector_search_panel.take();
                self.close_selector();
                self.explorer_focused = explorer_focus;
                if super::native_menu::is_supported_macro(command)
                    || matches!(
                        command,
                        jc_app::CommandId::SearchHistoryPrevious
                            | jc_app::CommandId::SearchHistoryNext
                            | jc_app::CommandId::SearchToggleCase
                            | jc_app::CommandId::SearchToggleWholeWord
                    )
                {
                    self.search_panel = search;
                }
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
                            self.bottom.focused = false;
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
