//! Field intentions are distinct from palette commands targeting their invoker.
use super::{line_edit::EditTarget, selector::SelectorKind, Action, State};
use jc_app::{CommandContext, CommandId, FieldEdit, PrimaryFocus};
use std::path::PathBuf;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct SelectorInputTarget {
    generation: u64,
    input_revision: u64,
    candidate: usize,
    kind: SelectorKind,
    root: Option<PathBuf>,
    source: Option<EditTarget>,
    invoker: Option<EditTarget>,
    focus: PrimaryFocus,
    source_path: Option<PathBuf>,
    encoding: Option<(jc_app::DocumentId, u64, Option<jc_text::TextEncoding>)>,
}

pub(super) fn field_command(command: CommandId) -> bool {
    matches!(
        command,
        CommandId::EditCut | CommandId::EditCopy | CommandId::EditPaste | CommandId::EditSelectAll
    )
}
impl State {
    /// The visible field owns Paste before any menu closes its invoking surface.
    /// This only captures an intention; no clipboard or source read occurs here.
    pub(super) fn field_paste_action(&self) -> Option<Action> {
        if self.selector.is_some() {
            return None; // selector selection/clipboard has its own typed route
        }
        if self.snippet_gui.is_some() {
            return self.snippet_input_target().map(Action::PasteSnippet);
        }
        if self.source_visible() || self.open_diff_visible || self.markdown_gui.is_some() {
            return None;
        }
        if self.git_panel.is_some() {
            return self.git_search_input_token().map(Action::PasteGitSearch);
        }
        if let Some(panel) = &self.replace_panel {
            return (panel.stage == super::replace_panel::ReplaceStage::Draft)
                .then_some(Action::PasteReplacement(panel.generation));
        }
        if let Some(panel) = self.project_panel.as_ref().filter(|p| p.visible) {
            return (!panel.results).then_some(Action::PasteProject(panel.generation));
        }
        if let Some(panel) = &self.search_panel {
            return Some(Action::PasteSearch(panel.generation));
        }
        self.navigation_panel
            .as_ref()
            .filter(|_| self.navigation_target_current())
            .map(|panel| Action::PasteNavigation(panel.generation))
    }
    pub(super) fn entry_context(&self, command: CommandId) -> CommandContext {
        let mut context = self.context();
        if command == CommandId::EditPaste && self.field_paste_action().is_some() {
            context.field_paste = true;
            context.clipboard_has_text = true;
        }
        if let Some(selector) = self.selector.as_ref().filter(|_| field_command(command)) {
            context.editor_input = false;
            context.field_input = true;
            context.has_selection = selector.selected_text().is_some();
            context.clipboard_has_text = true; // explicit-action intention; never reads OS
        }
        context
    }
    pub(super) fn selector_input_target(&self) -> Option<SelectorInputTarget> {
        let selector = self.selector.as_ref()?;
        Some(SelectorInputTarget {
            generation: self.selector_generation,
            input_revision: selector.input_revision(),
            candidate: selector.selected(),
            kind: selector.kind(),
            root: self.project_root.clone(),
            source: self.source_edit_target(),
            invoker: self.selector_edit_target,
            focus: self.context().focus,
            encoding: self.encoding_target,
            source_path: self
                .workbench
                .active_document()
                .map(|doc| doc.session.path().to_owned()),
        })
    }
    pub(super) fn selector_field_action(&mut self, command: CommandId) -> Action {
        if command == CommandId::EditSelectAll {
            if let Some(selector) = self.selector.as_mut() {
                if let Err(error) = selector.select_all_input() {
                    self.error(error);
                }
            }
            return Action::None;
        }
        self.selector_input_target().map_or(Action::None, |target| {
            Action::SelectorClipboard(command, Box::new(target))
        })
    }
    pub(super) fn edit_selector_input(&mut self, edit: FieldEdit<'_>, selecting: bool) {
        let result = self
            .selector
            .as_mut()
            .map(|selector| selector.edit_input(edit, selecting));
        match result {
            Some(Ok(true)) => self.request_files(),
            Some(Ok(false)) | None => {}
            Some(Err(error)) => self.error(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn project_menu_paste_targets_draft_but_not_read_only_results() {
        let mut state = State::new(&crate::GuiOptions::default()).unwrap();
        state.workbench.insert_text("sin guardar").unwrap();
        let bytes = state.workbench.active_bytes().unwrap();
        let panes = format!("{:?}", state.workbench.panes());
        state.project_root =
            Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../work/test-temp"));
        state.open_project_search(false);
        let generation = state.project_panel.as_ref().unwrap().generation;
        assert!(state.registry.is_enabled(
            CommandId::EditPaste,
            &state.entry_context(CommandId::EditPaste)
        ));
        assert!(
            matches!(state.execute_menu(CommandId::EditPaste), Action::PasteProject(g) if g == generation)
        );
        assert!(state.project_visible());
        assert_eq!(state.project_panel.as_ref().unwrap().generation, generation);
        state.open_project_search(true);
        assert!(state.field_paste_action().is_none());
        assert!(!state.registry.is_enabled(
            CommandId::EditPaste,
            &state.entry_context(CommandId::EditPaste)
        ));
        assert!(matches!(
            state.execute_menu(CommandId::EditPaste),
            Action::None
        ));
        assert!(state.project_visible());
        assert!(state.project_panel.as_ref().unwrap().results);
        assert_eq!(state.workbench.active_bytes().unwrap(), bytes);
        assert_eq!(format!("{:?}", state.workbench.panes()), panes);
    }
}
