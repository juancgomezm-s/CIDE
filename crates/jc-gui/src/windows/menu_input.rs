//! Popup selection is a deferred intention against the context at opening.
use super::{pointer_input::PointerContext, selector_input::SelectorInputTarget, State};
use jc_app::{CommandId, KeyScope, KeyStroke, Resolution};

#[derive(PartialEq, Eq)]
pub(super) struct MenuStamp {
    root: Option<std::path::PathBuf>,
    source: Option<PointerContext>,
    field: Option<SelectorInputTarget>,
    focus: jc_app::PrimaryFocus,
    explorer: Option<jc_app::explorer::NodeId>,
    input: bool,
    selector: u64,
    navigation: Option<u64>,
    search: Option<(u64, bool, bool)>,
    project: Option<u64>,
    replace: Option<u64>,
    git: Option<super::git_panel::GitIntent>,
    epoch: u64,
    comparison: (bool, jc_app::open_diff::OpenDiffState, usize),
    source_ui: (
        u64,
        jc_app::source_services::SourceState,
        Option<jc_app::source_services::SourceMode>,
        usize,
        bool,
    ),
    snippet: Option<super::snippet_panel::SnippetInputTarget>,
}
impl State {
    pub(super) fn current_menu_stamp(&self) -> MenuStamp {
        let context = self.context();
        MenuStamp {
            root: self.project_root.clone(),
            source: self.pointer_context(self.workbench.panes().active()),
            field: self.selector_input_target(),
            focus: context.focus,
            explorer: self.explorer_view.selected(),
            input: context.editor_input,
            selector: self.selector_generation,
            navigation: self
                .navigation_panel
                .as_ref()
                .map(|_| self.navigation_generation),
            search: self
                .search_panel
                .as_ref()
                .map(|p| (p.generation, p.replace, p.replacement_active)),
            project: self
                .project_panel
                .as_ref()
                .filter(|_| self.project_visible())
                .map(|p| p.generation),
            replace: self.replace_panel.as_ref().map(|p| p.generation),
            git: self.git_panel,
            epoch: self.interaction_epoch,
            comparison: (
                self.open_diff_visible,
                self.open_diff.state(),
                self.open_diff.selected(),
            ),
            source_ui: self.source_gui.stamp(),
            snippet: self.snippet_stamp(),
        }
    }
    pub(super) fn accept_menu_selection(&mut self) -> bool {
        if self
            .menu_stamp
            .take()
            .is_some_and(|stamp| *stamp != self.current_menu_stamp())
        {
            self.status = "Cambió el destino del menú; acción descartada".into();
            return false;
        }
        true
    }
    pub(super) fn native_shortcut(&self, command: CommandId) -> Option<KeyStroke> {
        let context = self.entry_context(command);
        if !self.registry.is_enabled(command, &context) {
            return None;
        }
        let global = matches!(
            command,
            CommandId::ViewToggleInvisibles
                | CommandId::FileExit
                | CommandId::SearchReplaceProject
                | CommandId::SearchGoToPosition
                | CommandId::NavigateBack
                | CommandId::NavigateForward
                | CommandId::PaletteOpen
        );
        let scope = if context.field_input {
            KeyScope::Editor
        } else if let Some(panel) = &self.search_panel {
            if !global
                && !matches!(
                    command,
                    CommandId::SearchHistoryPrevious
                        | CommandId::SearchHistoryNext
                        | CommandId::SearchToggleCase
                        | CommandId::SearchToggleWholeWord
                        | CommandId::SearchFindNext
                        | CommandId::SearchFindPrevious
                )
            {
                return None;
            }
            if panel.replace {
                KeyScope::Replace
            } else {
                KeyScope::Find
            }
        } else if !self.pointer_source_available() && !global {
            return None;
        } else {
            match context.focus {
                jc_app::PrimaryFocus::Editor => KeyScope::Editor,
                jc_app::PrimaryFocus::Sidebar => KeyScope::Sidebar,
                jc_app::PrimaryFocus::BottomPanel => KeyScope::BottomPanel,
            }
        };
        self.keymap.shortcuts_for(command).find_map(|binding| {
            let resolved = self
                .keymap
                .resolve(scope, binding.stroke, &self.registry, &context);
            (matches!(resolved, Resolution::Enabled(id) if id == command)).then_some(binding.stroke)
        })
    }
}
