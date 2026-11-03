//! Intentions and bounded read-model preparation for comparing open groups.
use super::TuiApp;
use crate::state::UiOverlay;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use jc_app::{diff_model::DiffSide, open_diff::OpenDiffState, CommandId, PrimaryFocus};

pub(crate) fn is_command(command: CommandId) -> bool {
    matches!(
        command,
        CommandId::DiffCompareOpen
            | CommandId::DiffRefreshOpen
            | CommandId::DiffCancelOpen
            | CommandId::DiffOpenNavigateLeft
            | CommandId::DiffOpenNavigateRight
    )
}
impl TuiApp {
    pub(crate) fn open_diff_command(&mut self, command: CommandId) {
        match command {
            CommandId::DiffCompareOpen | CommandId::DiffRefreshOpen => {
                self.close_overlay();
                self.open_diff_visible = true;
                self.open_diff_view.first();
                self.status = match self.open_diff.request(&self.workbench, &mut self.tasks) {
                    Ok(()) => self.open_diff.message().into(),
                    Err(e) => e,
                };
            }
            CommandId::DiffCancelOpen => {
                self.open_diff.cancel(&mut self.tasks);
                self.open_diff_visible = false;
                self.status = self.open_diff.message().into();
            }
            CommandId::DiffOpenNavigateLeft | CommandId::DiffOpenNavigateRight => {
                let side = if command == CommandId::DiffOpenNavigateLeft {
                    DiffSide::Left
                } else {
                    DiffSide::Right
                };
                match self.open_diff.select_hunk(
                    self.open_diff.selected(),
                    side,
                    &mut self.workbench,
                ) {
                    Ok(()) => {
                        self.open_diff_visible = false;
                        self.close_overlay();
                        let _ = self.focus.focus(PrimaryFocus::Editor, &self.panels);
                        self.status = "Comparison hunk selected; source unchanged".into();
                    }
                    Err(e) => self.status = e,
                }
            }
            _ => {}
        }
    }
    pub(crate) fn open_diff_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Esc {
            self.dispatch(CommandId::DiffCancelOpen);
            return;
        }
        if key.code == KeyCode::F(5) && key.modifiers.is_empty() {
            self.dispatch(CommandId::DiffRefreshOpen);
            return;
        }
        if key.code == KeyCode::Enter
            && (key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT)
        {
            self.dispatch(if key.modifiers == KeyModifiers::SHIFT {
                CommandId::DiffOpenNavigateLeft
            } else {
                CommandId::DiffOpenNavigateRight
            });
            return;
        }
        if key.modifiers.is_empty() {
            match key.code {
                KeyCode::Up => {
                    self.open_diff.select_relative(-1);
                    self.open_diff_view.first();
                }
                KeyCode::Down => {
                    self.open_diff.select_relative(1);
                    self.open_diff_view.first();
                }
                KeyCode::PageUp => self.open_diff_view.scroll(-10),
                KeyCode::PageDown => self.open_diff_view.scroll(10),
                KeyCode::Home => self.open_diff_view.first(),
                KeyCode::End => self.open_diff_view.last(),
                _ => {}
            }
        }
    }
    pub(crate) fn open_diff_context(&self, context: &mut jc_app::CommandContext) {
        context.open_diff_active = !matches!(
            self.open_diff.state(),
            OpenDiffState::Idle | OpenDiffState::Cancelled
        );
        context.open_diff_ready = self.open_diff.state() == OpenDiffState::Ready;
        if self.open_diff_visible {
            context.editor_input = false;
        }
    }
    pub(crate) fn keep_open_diff_for(command: CommandId) -> bool {
        is_command(command)
            || matches!(
                command,
                CommandId::PaletteOpen
                    | CommandId::ViewToggleInvisibles
                    | CommandId::MenuFile
                    | CommandId::MenuEdit
                    | CommandId::MenuSearch
                    | CommandId::MenuProject
                    | CommandId::MenuBuild
                    | CommandId::MenuTools
                    | CommandId::MenuView
                    | CommandId::MenuHelp
            )
    }
    pub(crate) fn showing_open_diff(&self) -> bool {
        self.open_diff_visible && matches!(self.overlay, UiOverlay::None)
    }
}
