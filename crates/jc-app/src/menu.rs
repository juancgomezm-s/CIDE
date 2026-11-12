use crate::command::{CommandContext, CommandId, CommandRegistry};
use crate::keymap::{KeyMap, KeyStroke};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MenuId {
    File,
    Edit,
    Search,
    Project,
    Build,
    Tools,
    View,
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuItem {
    Command(CommandId),
    Separator,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Menu {
    pub id: MenuId,
    pub title: &'static str,
    pub mnemonic: char,
    pub items: Vec<MenuItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuValidationError {
    UnknownCommand {
        menu: MenuId,
        command: CommandId,
    },
    DuplicateMnemonic {
        first: MenuId,
        second: MenuId,
        mnemonic: char,
    },
    LeadingSeparator(MenuId),
    TrailingSeparator(MenuId),
    ConsecutiveSeparator(MenuId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuEntryView {
    pub command: CommandId,
    pub label: &'static str,
    pub shortcut: Option<KeyStroke>,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuModel {
    menus: Vec<Menu>,
}

impl MenuModel {
    pub fn new(menus: Vec<Menu>) -> Self {
        Self { menus }
    }
    pub fn menus(&self) -> &[Menu] {
        &self.menus
    }
    pub fn get(&self, id: MenuId) -> Option<&Menu> {
        self.menus.iter().find(|menu| menu.id == id)
    }

    pub fn validate(&self, registry: &CommandRegistry) -> Vec<MenuValidationError> {
        let mut errors = Vec::new();
        for (index, menu) in self.menus.iter().enumerate() {
            for earlier in &self.menus[..index] {
                if earlier.mnemonic.eq_ignore_ascii_case(&menu.mnemonic) {
                    errors.push(MenuValidationError::DuplicateMnemonic {
                        first: earlier.id,
                        second: menu.id,
                        mnemonic: menu.mnemonic,
                    });
                }
            }
            if matches!(menu.items.first(), Some(MenuItem::Separator)) {
                errors.push(MenuValidationError::LeadingSeparator(menu.id));
            }
            if matches!(menu.items.last(), Some(MenuItem::Separator)) {
                errors.push(MenuValidationError::TrailingSeparator(menu.id));
            }
            for pair in menu.items.windows(2) {
                if pair == [MenuItem::Separator, MenuItem::Separator] {
                    errors.push(MenuValidationError::ConsecutiveSeparator(menu.id));
                }
            }
            for item in &menu.items {
                if let MenuItem::Command(command) = item {
                    if !registry.contains(*command) {
                        errors.push(MenuValidationError::UnknownCommand {
                            menu: menu.id,
                            command: *command,
                        });
                    }
                }
            }
        }
        errors
    }

    pub fn entries(
        &self,
        id: MenuId,
        registry: &CommandRegistry,
        keymap: &KeyMap,
        context: &CommandContext,
    ) -> Vec<Option<MenuEntryView>> {
        let Some(menu) = self.get(id) else {
            return Vec::new();
        };
        menu.items
            .iter()
            .map(|item| match item {
                MenuItem::Separator => None,
                MenuItem::Command(command) => registry.get(*command).map(|spec| MenuEntryView {
                    command: *command,
                    label: spec.label,
                    shortcut: keymap.primary_shortcut(*command),
                    enabled: registry.is_enabled(*command, context),
                }),
            })
            .collect()
    }
}

fn menu(id: MenuId, title: &'static str, mnemonic: char, items: &[MenuItem]) -> Menu {
    Menu {
        id,
        title,
        mnemonic,
        items: items.to_vec(),
    }
}
fn command(id: CommandId) -> MenuItem {
    MenuItem::Command(id)
}
const SEP: MenuItem = MenuItem::Separator;

pub fn default_menu_model() -> MenuModel {
    use CommandId::*;
    use MenuId::*;
    MenuModel::new(vec![
        menu(
            File,
            "Archivo",
            'a',
            &[
                command(FileNew),
                command(FileOpen),
                command(FileQuickOpen),
                command(FileOpenFolder),
                command(FileChooseEncoding),
                SEP,
                command(FileSave),
                command(FileSaveAs),
                command(FileSaveAll),
                SEP,
                command(FileClose),
                command(FileReopenClosed),
                SEP,
                command(FileExit),
            ],
        ),
        menu(
            Edit,
            "Editar",
            'e',
            &[
                command(EditUndo),
                command(EditRedo),
                SEP,
                command(EditCut),
                command(EditCopy),
                command(EditPaste),
                command(EditSelectAll),
                SEP,
                command(SourceComplete),
                command(SourceLspComplete),
                command(SourceSchemaComplete),
                command(SourceRename),
                command(EditInsertSnippet),
                command(EditSurroundWith),
                SEP,
                command(EditDuplicate),
                command(EditDeleteLine),
                command(EditMoveLineUp),
                command(EditMoveLineDown),
                command(EditToggleLineComment),
                command(EditToggleBlockComment),
                SEP,
                command(EditIndent),
                command(EditOutdent),
                command(EditJoinLines),
                command(EditSplitLine),
                SEP,
                command(EditRectangleStart),
            ],
        ),
        menu(
            Search,
            "Buscar",
            'b',
            &[
                command(SearchFind),
                command(SearchReplace),
                command(SearchFindNext),
                command(SearchFindPrevious),
                SEP,
                command(SearchProject),
                command(SearchCancelProject),
                command(SearchReplaceProject),
                SEP,
                command(SearchSelectionNext),
                command(SearchSelectionPrevious),
                command(SearchGoToPosition),
            ],
        ),
        menu(
            Project,
            "Proyecto",
            'p',
            &[
                command(ExplorerRefresh),
                SEP,
                command(ViewSymbols),
                command(SourceReferences),
                command(SourceCalls),
                command(SourceIncoming),
                command(SourceDependencies),
                SEP,
                command(SourceDefinition),
                command(SourceLspReferences),
            ],
        ),
        menu(
            Tools,
            "Herramientas",
            'h',
            &[
                command(PaletteOpen),
                command(SourceLspDiagnostics),
                SEP,
                command(ProgressCheck),
                command(ProgressCompile),
                command(ProgressRun),
                command(ProgressRefreshSchema),
                command(ProgressCancel),
            ],
        ),
        menu(
            View,
            "Ver",
            'v',
            &[
                command(ViewToggleInvisibles),
                command(ViewMarkdownPreview),
                command(ViewExplorer),
                command(ViewGit),
                SEP,
                command(ViewProblems),
                command(ViewOutput),
                command(ViewDatabase),
                command(ViewSearchResults),
                command(ViewToggleBottomPanel),
                SEP,
                command(EditorSplitRight),
                command(EditorOneGroup),
                command(EditorTogglePin),
                command(EditorToggleScrollLink),
                command(EditorOpenToSide),
                command(NavigateBack),
                command(NavigateForward),
                SEP,
                command(GitDiffHead),
                command(GitDiffIndex),
                command(GitDiffRefresh),
                command(GitDiffCancel),
                command(GitRepositoryStatus),
                command(GitCloneWorktree),
                command(GitStatusRefresh),
                command(GitStatusCancel),
                SEP,
                command(DiffCompareOpen),
            ],
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn menus_have_unique_entry_points_and_keep_context_mechanics_in_registry() {
        let model = default_menu_model();
        let registry = crate::default_registry();
        assert!(model.validate(&registry).is_empty());
        assert!(model.get(MenuId::Build).is_none());
        assert!(model.get(MenuId::Help).is_none());
        let mut commands = BTreeSet::new();
        for menu in model.menus() {
            for item in &menu.items {
                if let MenuItem::Command(command) = item {
                    assert!(
                        commands.insert(*command),
                        "duplicate menu entry {command:?}"
                    );
                }
            }
        }
        for command in [
            CommandId::EditRectangleLeft,
            CommandId::SourceAccept,
            CommandId::SearchHistoryNext,
            CommandId::SnippetNext,
            CommandId::ViewTerminal,
        ] {
            assert!(registry.contains(command));
            assert!(!commands.contains(&command));
        }
        assert!(commands.contains(&CommandId::SourceIncoming));
        assert!(commands.contains(&CommandId::SourceDependencies));
        assert!(commands.contains(&CommandId::ProgressCompile));
        assert!(commands.contains(&CommandId::ProgressRefreshSchema));
        assert!(commands.contains(&CommandId::ViewDatabase));
        assert!(commands.contains(&CommandId::SourceRename));
    }
}
