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
    pub(super) fn entry_context(&self, command: CommandId) -> CommandContext {
        let mut context = self.context();
        if command == CommandId::EditPaste && self.git_search_input_token().is_some() {
            context.field_paste = true;
            context.clipboard_has_text = true;
        }
        if let Some(selector) = self.selector.as_ref().filter(|_| field_command(command)) {
            context.editor_input = false;
            context.field_input = true;
            context.has_selection = selector.selected_text().is_some();
            context.clipboard_has_text = true; // explicit-action intention; never reads OS
        }
        if self.selector.is_none()
            && command == CommandId::EditPaste
            && self.snippet_input_target().is_some()
        {
            context.field_paste = true;
            context.clipboard_has_text = true;
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
