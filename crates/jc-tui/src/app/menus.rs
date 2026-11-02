use super::TuiApp;
use crate::state::UiOverlay;
use jc_app::{CommandId, MenuId, Overlay};

impl TuiApp {
    pub(crate) fn open_menu(&mut self, command: CommandId) {
        if let Some(menu) = menu_for_command(command) {
            self.open_focus_overlay(Overlay::Menu);
            let entries = self.menu_commands(menu);
            self.overlay = UiOverlay::Menu {
                menu,
                selected: next_selectable(&entries, usize::MAX),
            };
        }
    }

    pub(crate) fn switch_menu(&mut self, menu: MenuId, delta: isize) {
        let menus = self.menus.menus();
        let Some(index) = menus.iter().position(|candidate| candidate.id == menu) else {
            return;
        };
        let next = if delta < 0 {
            if index == 0 {
                menus.len() - 1
            } else {
                index - 1
            }
        } else {
            (index + 1) % menus.len()
        };
        let menu = menus[next].id;
        let entries = self.menu_commands(menu);
        self.overlay = UiOverlay::Menu {
            menu,
            selected: next_selectable(&entries, usize::MAX),
        };
    }

    pub(crate) fn menu_commands(&self, menu: MenuId) -> Vec<Option<CommandId>> {
        let context = self.command_context();
        self.menus
            .entries(menu, &self.registry, &self.keymap, &context)
            .into_iter()
            .map(|entry| entry.map(|value| value.command))
            .collect()
    }

    pub(crate) fn palette_commands(&self, query: &str) -> Vec<CommandId> {
        let query = query.to_ascii_lowercase();
        self.registry
            .iter()
            .filter(|spec| query.is_empty() || spec.label.to_ascii_lowercase().contains(&query))
            .map(|spec| spec.id)
            .collect()
    }

    pub(crate) fn open_focus_overlay(&mut self, overlay: Overlay) {
        while self.focus.close_overlay().is_some() {}
        self.focus.open_overlay(overlay);
    }

    pub(crate) fn close_overlay(&mut self) {
        if matches!(
            self.overlay,
            UiOverlay::Prompt {
                kind: crate::state::PromptKind::GitExecutable,
                ..
            }
        ) {
            self.git_configuration_target = None;
        }
        self.overlay = UiOverlay::None;
        let _ = self.focus.close_overlay();
    }
}

pub(crate) fn menu_for_command(command: CommandId) -> Option<MenuId> {
    use CommandId::*;
    Some(match command {
        MenuFile => MenuId::File,
        MenuEdit => MenuId::Edit,
        MenuSearch => MenuId::Search,
        MenuProject => MenuId::Project,
        MenuBuild => MenuId::Build,
        MenuTools => MenuId::Tools,
        MenuView => MenuId::View,
        MenuHelp => MenuId::Help,
        _ => return None,
    })
}

pub(crate) fn next_selectable(entries: &[Option<CommandId>], current: usize) -> usize {
    if entries.is_empty() {
        return 0;
    }
    let start = if current == usize::MAX {
        0
    } else {
        (current + 1) % entries.len()
    };
    for offset in 0..entries.len() {
        let index = (start + offset) % entries.len();
        if entries[index].is_some() {
            return index;
        }
    }
    0
}

pub(crate) fn previous_selectable(entries: &[Option<CommandId>], current: usize) -> usize {
    if entries.is_empty() {
        return 0;
    }
    for offset in 1..=entries.len() {
        let index = (current + entries.len() - (offset % entries.len())) % entries.len();
        if entries[index].is_some() {
            return index;
        }
    }
    0
}
