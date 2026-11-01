use super::{CommandOutcome, Workbench, WorkbenchError};
use crate::CommandId;

impl Workbench {
    pub fn execute(&mut self, command: CommandId) -> Result<CommandOutcome, WorkbenchError> {
        use CommandId::*;
        match command {
            FileNew => Ok(CommandOutcome::PromptNew),
            FileOpen => Ok(CommandOutcome::PromptOpen),
            FileSave => {
                self.save_active()?;
                Ok(CommandOutcome::None)
            }
            FileSaveAs => Ok(CommandOutcome::PromptSaveAs),
            FileSaveAll => {
                self.save_all()?;
                Ok(CommandOutcome::None)
            }
            FileClose => {
                let id = self.tabs.active_id().ok_or(WorkbenchError::NoDocument)?;
                if self
                    .active_document()
                    .is_some_and(|doc| doc.session.document().is_modified())
                {
                    Ok(CommandOutcome::NeedsCloseDecision(id))
                } else {
                    let _ = self.close_active(true)?;
                    Ok(CommandOutcome::None)
                }
            }
            FileReopenClosed => {
                self.reopen_closed();
                Ok(CommandOutcome::None)
            }
            FileExit => Ok(CommandOutcome::ExitRequested),
            EditUndo => {
                self.undo()?;
                Ok(CommandOutcome::None)
            }
            EditRedo => {
                self.redo()?;
                Ok(CommandOutcome::None)
            }
            EditCut => {
                self.cut_selection()?;
                Ok(CommandOutcome::None)
            }
            EditCopy => {
                self.copy_selection()?;
                Ok(CommandOutcome::None)
            }
            EditPaste => {
                self.paste()?;
                Ok(CommandOutcome::None)
            }
            EditSelectAll => {
                self.select_all()?;
                Ok(CommandOutcome::None)
            }
            EditDuplicate => {
                self.duplicate_selection_or_line()?;
                Ok(CommandOutcome::None)
            }
            EditDeleteLine => {
                self.delete_line()?;
                Ok(CommandOutcome::None)
            }
            EditMoveLineUp => {
                self.move_line_up()?;
                Ok(CommandOutcome::None)
            }
            EditMoveLineDown => {
                self.move_line_down()?;
                Ok(CommandOutcome::None)
            }
            EditToggleLineComment => {
                self.toggle_line_comment()?;
                Ok(CommandOutcome::None)
            }
            EditToggleBlockComment => {
                self.toggle_block_comment()?;
                Ok(CommandOutcome::None)
            }
            EditIndent => {
                self.indent_lines()?;
                Ok(CommandOutcome::None)
            }
            EditOutdent => {
                self.outdent_lines()?;
                Ok(CommandOutcome::None)
            }
            SearchFind => Ok(CommandOutcome::ShowFind),
            SearchReplace => Ok(CommandOutcome::ShowReplace),
            SearchFindNext => {
                let _ = self.find_next()?;
                Ok(CommandOutcome::None)
            }
            SearchFindPrevious => {
                let _ = self.find_previous()?;
                Ok(CommandOutcome::None)
            }
            SearchToggleCase => {
                self.toggle_search_case()?;
                Ok(CommandOutcome::None)
            }
            SearchToggleWholeWord => {
                self.toggle_search_whole_word()?;
                Ok(CommandOutcome::None)
            }
            SearchToggleRegex => {
                self.toggle_search_regex()?;
                Ok(CommandOutcome::None)
            }
            PaletteOpen => Ok(CommandOutcome::ShowPalette),
            TabNext => {
                self.next_tab();
                Ok(CommandOutcome::None)
            }
            TabPrevious => {
                self.previous_tab();
                Ok(CommandOutcome::None)
            }
            Tab1 | Tab2 | Tab3 | Tab4 | Tab5 | Tab6 | Tab7 | Tab8 => {
                let index = match command {
                    Tab1 => 1,
                    Tab2 => 2,
                    Tab3 => 3,
                    Tab4 => 4,
                    Tab5 => 5,
                    Tab6 => 6,
                    Tab7 => 7,
                    Tab8 => 8,
                    _ => unreachable!(),
                };
                self.select_tab(index);
                Ok(CommandOutcome::None)
            }
            Tab9 => {
                self.last_tab();
                Ok(CommandOutcome::None)
            }
            MenuFile | MenuEdit | MenuSearch | MenuProject | MenuBuild | MenuTools | MenuView
            | MenuHelp => Ok(CommandOutcome::OpenMenu(command)),
            ViewExplorer
            | ViewDatabase
            | ViewGit
            | ViewSymbols
            | ViewToggleBottomPanel
            | ViewTerminal
            | ViewProblems
            | ViewOutput
            | ViewSearchResults
            | FocusNext
            | FocusPrevious => Ok(CommandOutcome::View(command)),
            ProgressCheck | ProgressCompile | ProgressRun | ProgressRefreshSchema => {
                Ok(CommandOutcome::Progress(command))
            }
            FileOpenFolder | SearchProject | SearchReplaceProject => {
                Ok(CommandOutcome::View(command))
            }
        }
    }
}
