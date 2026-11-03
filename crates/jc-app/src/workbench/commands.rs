use super::{CommandOutcome, Workbench, WorkbenchError};
use crate::CommandId;

impl Workbench {
    pub fn execute(&mut self, command: CommandId) -> Result<CommandOutcome, WorkbenchError> {
        use CommandId::*;
        match command {
            ViewMarkdownPreview => Ok(CommandOutcome::View(ViewMarkdownPreview)),
            ViewToggleInvisibles => {
                self.toggle_invisibles();
                Ok(CommandOutcome::View(ViewToggleInvisibles))
            }
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
            FileClose => match self.close_active(false)? {
                Some(id) => Ok(CommandOutcome::NeedsCloseDecision(id)),
                None => Ok(CommandOutcome::None),
            },
            FileReopenClosed => {
                self.try_reopen_closed()?;
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
            EditRectangleStart | EditRectangleLeft | EditRectangleRight | EditRectangleUp
            | EditRectangleDown | EditRectangleCancel => {
                match command {
                    EditRectangleStart => self.start_rectangle()?,
                    EditRectangleLeft => self.extend_rectangle(-1, 0)?,
                    EditRectangleRight => self.extend_rectangle(1, 0)?,
                    EditRectangleUp => self.extend_rectangle(0, -1)?,
                    EditRectangleDown => self.extend_rectangle(0, 1)?,
                    EditRectangleCancel => {
                        self.cancel_rectangle();
                    }
                    _ => unreachable!(),
                }
                Ok(CommandOutcome::None)
            }
            EditJoinLines => {
                self.join_lines()?;
                Ok(CommandOutcome::None)
            }
            EditSplitLine => {
                self.split_line()?;
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
            SearchFind => {
                self.set_search_query(self.search.query.clone())?;
                Ok(CommandOutcome::ShowFind)
            }
            SearchReplace => {
                self.set_search_query(self.search.query.clone())?;
                Ok(CommandOutcome::ShowReplace)
            }
            SearchFindNext => {
                let _ = self.find_next()?;
                Ok(CommandOutcome::None)
            }
            SearchFindPrevious => {
                let _ = self.find_previous()?;
                Ok(CommandOutcome::None)
            }
            SearchSelectionNext => {
                self.find_selection(true)?;
                Ok(CommandOutcome::None)
            }
            SearchSelectionPrevious => {
                self.find_selection(false)?;
                Ok(CommandOutcome::None)
            }
            SearchGoToPosition => Ok(CommandOutcome::ShowGoToPosition),
            SearchCancel => {
                self.cancel_rectangle();
                self.cancel_search();
                Ok(CommandOutcome::None)
            }
            SearchHistoryPrevious => {
                self.cycle_search_history(true)?;
                Ok(CommandOutcome::None)
            }
            SearchHistoryNext => {
                self.cycle_search_history(false)?;
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
            EditorSplitRight
            | EditorOneGroup
            | EditorFocusPrimary
            | EditorFocusSecondary
            | EditorTogglePin
            | EditorToggleScrollLink
            | EditorOpenToSide
            | NavigateBack
            | NavigateForward => {
                match command {
                    EditorSplitRight => {
                        self.split_editor_right();
                    }
                    EditorOneGroup => {
                        self.close_split_editor();
                    }
                    EditorFocusPrimary => {
                        self.focus_editor_pane(crate::PaneId::Primary);
                    }
                    EditorFocusSecondary => {
                        self.focus_editor_pane(crate::PaneId::Secondary);
                    }
                    EditorTogglePin => {
                        self.toggle_reference_pin();
                    }
                    EditorToggleScrollLink => {
                        self.toggle_scroll_link();
                    }
                    EditorOpenToSide => {
                        self.open_active_to_side();
                    }
                    NavigateBack => {
                        self.navigation_back();
                    }
                    NavigateForward => {
                        self.navigation_forward();
                    }
                    _ => unreachable!(),
                }
                Ok(CommandOutcome::View(command))
            }
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
            | GitDiffHead
            | GitDiffIndex
            | GitDiffRefresh
            | GitDiffCancel
            | GitConfigureExecutable
            | GitRepositoryStatus
            | GitStatusRefresh
            | GitStatusCancel
            | DiffCompareOpen
            | DiffRefreshOpen
            | DiffCancelOpen
            | DiffOpenNavigateLeft
            | DiffOpenNavigateRight
            | ViewSymbols
            | SourceReferences
            | SourceComplete
            | SourceRefresh
            | SourceCancel
            | SourceAccept
            | SourceOpenToSide
            | EditInsertSnippet
            | EditSurroundWith
            | SnippetNext
            | SnippetPrevious
            | SnippetAccept
            | SnippetCancel
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
            FileChooseEncoding | FileQuickOpen | FileOpenFolder | SearchProject
            | SearchCancelProject | SearchReplaceProject | ExplorerRefresh | ExplorerCancel => {
                Ok(CommandOutcome::View(command))
            }
        }
    }
}
