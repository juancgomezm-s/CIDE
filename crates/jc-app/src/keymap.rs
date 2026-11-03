use crate::command::{CommandContext, CommandId, CommandRegistry};
use crate::focus::{Overlay, PrimaryFocus};
use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Modifiers(u8);

impl Modifiers {
    pub const NONE: Self = Self(0);
    pub const CTRL: Self = Self(1);
    pub const ALT: Self = Self(2);
    pub const SHIFT: Self = Self(4);

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Key {
    Char(char),
    Function(u8),
    Tab,
    Enter,
    Escape,
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Home,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyStroke {
    pub modifiers: Modifiers,
    pub key: Key,
}

impl KeyStroke {
    pub const fn new(modifiers: Modifiers, key: Key) -> Self {
        Self { modifiers, key }
    }
    pub const fn plain(key: Key) -> Self {
        Self::new(Modifiers::NONE, key)
    }
    pub const fn ctrl(key: Key) -> Self {
        Self::new(Modifiers::CTRL, key)
    }
    pub const fn alt(key: Key) -> Self {
        Self::new(Modifiers::ALT, key)
    }
    pub const fn shift(key: Key) -> Self {
        Self::new(Modifiers::SHIFT, key)
    }
    pub const fn ctrl_shift(key: Key) -> Self {
        Self::new(Modifiers::CTRL.union(Modifiers::SHIFT), key)
    }
    pub const fn ctrl_alt(key: Key) -> Self {
        Self::new(Modifiers::CTRL.union(Modifiers::ALT), key)
    }
    pub const fn alt_shift(key: Key) -> Self {
        Self::new(Modifiers::ALT.union(Modifiers::SHIFT), key)
    }
}

impl fmt::Display for KeyStroke {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.modifiers.contains(Modifiers::CTRL) {
            f.write_str("Ctrl+")?;
        }
        if self.modifiers.contains(Modifiers::ALT) {
            f.write_str("Alt+")?;
        }
        if self.modifiers.contains(Modifiers::SHIFT) {
            f.write_str("Shift+")?;
        }
        match self.key {
            Key::Char(c) => write!(f, "{}", c.to_ascii_uppercase()),
            Key::Function(n) => write!(f, "F{n}"),
            Key::Tab => f.write_str("Tab"),
            Key::Enter => f.write_str("Enter"),
            Key::Escape => f.write_str("Esc"),
            Key::Up => f.write_str("Up"),
            Key::Down => f.write_str("Down"),
            Key::Left => f.write_str("Left"),
            Key::Right => f.write_str("Right"),
            Key::PageUp => f.write_str("PageUp"),
            Key::PageDown => f.write_str("PageDown"),
            Key::Home => f.write_str("Home"),
            Key::End => f.write_str("End"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KeyScope {
    Global,
    Editor,
    Sidebar,
    BottomPanel,
    Menu,
    Dialog,
    Find,
    Replace,
    CommandPalette,
}

impl KeyScope {
    pub fn from_focus(primary: PrimaryFocus, overlay: Option<Overlay>) -> Self {
        match overlay {
            Some(Overlay::Menu) => Self::Menu,
            Some(Overlay::Dialog) => Self::Dialog,
            Some(Overlay::Find) => Self::Find,
            Some(Overlay::Replace) => Self::Replace,
            Some(Overlay::CommandPalette) => Self::CommandPalette,
            None => match primary {
                PrimaryFocus::Editor => Self::Editor,
                PrimaryFocus::Sidebar => Self::Sidebar,
                PrimaryFocus::BottomPanel => Self::BottomPanel,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    pub scope: KeyScope,
    pub stroke: KeyStroke,
    pub command: CommandId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyMapError {
    UnknownCommand(CommandId),
    Duplicate {
        scope: KeyScope,
        stroke: KeyStroke,
        first: CommandId,
        second: CommandId,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    Enabled(CommandId),
    Disabled(CommandId),
    None,
}

#[derive(Debug, Clone)]
pub struct KeyMap {
    bindings: Vec<Binding>,
}

impl KeyMap {
    pub fn new(bindings: Vec<Binding>) -> Self {
        Self { bindings }
    }
    pub fn bindings(&self) -> &[Binding] {
        &self.bindings
    }

    pub fn validate(&self, registry: &CommandRegistry) -> Vec<KeyMapError> {
        let mut errors = Vec::new();
        let mut seen: BTreeMap<(KeyScope, KeyStroke), CommandId> = BTreeMap::new();
        for binding in &self.bindings {
            if !registry.contains(binding.command) {
                errors.push(KeyMapError::UnknownCommand(binding.command));
            }
            if let Some(first) = seen.insert((binding.scope, binding.stroke), binding.command) {
                if first != binding.command {
                    errors.push(KeyMapError::Duplicate {
                        scope: binding.scope,
                        stroke: binding.stroke,
                        first,
                        second: binding.command,
                    });
                }
            }
        }
        errors
    }

    pub fn resolve(
        &self,
        scope: KeyScope,
        stroke: KeyStroke,
        registry: &CommandRegistry,
        context: &CommandContext,
    ) -> Resolution {
        let command = self
            .bindings
            .iter()
            .find(|binding| binding.scope == scope && binding.stroke == stroke)
            .or_else(|| {
                self.bindings
                    .iter()
                    .find(|binding| binding.scope == KeyScope::Global && binding.stroke == stroke)
            })
            .map(|binding| binding.command);
        match command {
            Some(id) if registry.is_enabled(id, context) => Resolution::Enabled(id),
            Some(id) => Resolution::Disabled(id),
            None => Resolution::None,
        }
    }

    pub fn shortcuts_for(&self, command: CommandId) -> impl Iterator<Item = &Binding> {
        self.bindings
            .iter()
            .filter(move |binding| binding.command == command)
    }

    pub fn primary_shortcut(&self, command: CommandId) -> Option<KeyStroke> {
        self.shortcuts_for(command)
            .next()
            .map(|binding| binding.stroke)
    }
}

fn bind(scope: KeyScope, stroke: KeyStroke, command: CommandId) -> Binding {
    Binding {
        scope,
        stroke,
        command,
    }
}

pub fn default_keymap() -> KeyMap {
    use CommandId::*;
    use Key::*;
    use KeyScope::*;
    let c = |ch| Char(ch);
    let f = |n| Function(n);
    KeyMap::new(vec![
        bind(Global, KeyStroke::ctrl(c('n')), FileNew),
        bind(Global, KeyStroke::ctrl(c('o')), FileOpen),
        bind(Global, KeyStroke::ctrl_shift(c('o')), FileOpenFolder),
        bind(Global, KeyStroke::ctrl(c('s')), FileSave),
        bind(Global, KeyStroke::ctrl_shift(c('s')), FileSaveAs),
        bind(Global, KeyStroke::ctrl_alt(c('s')), FileSaveAll),
        bind(Global, KeyStroke::ctrl(c('w')), FileClose),
        bind(Global, KeyStroke::ctrl_shift(c('t')), FileReopenClosed),
        bind(Global, KeyStroke::alt(f(4)), FileExit),
        bind(Editor, KeyStroke::ctrl(c('z')), EditUndo),
        bind(Editor, KeyStroke::ctrl(c('y')), EditRedo),
        bind(Editor, KeyStroke::ctrl(c('x')), EditCut),
        bind(Editor, KeyStroke::ctrl(c('c')), EditCopy),
        bind(Editor, KeyStroke::ctrl(c('v')), EditPaste),
        bind(Editor, KeyStroke::ctrl(c('a')), EditSelectAll),
        bind(Editor, KeyStroke::alt_shift(Left), EditRectangleLeft),
        bind(Editor, KeyStroke::alt_shift(Right), EditRectangleRight),
        bind(Editor, KeyStroke::alt_shift(Up), EditRectangleUp),
        bind(Editor, KeyStroke::alt_shift(Down), EditRectangleDown),
        bind(Editor, KeyStroke::ctrl(c('d')), EditDuplicate),
        bind(Editor, KeyStroke::ctrl_shift(c('d')), EditDeleteLine),
        bind(Editor, KeyStroke::plain(Tab), EditIndent),
        bind(Editor, KeyStroke::shift(Tab), EditOutdent),
        bind(Editor, KeyStroke::alt(Up), EditMoveLineUp),
        bind(Editor, KeyStroke::alt(Down), EditMoveLineDown),
        bind(Editor, KeyStroke::ctrl(c('/')), EditToggleLineComment),
        bind(
            Editor,
            KeyStroke::ctrl_shift(c('/')),
            EditToggleBlockComment,
        ),
        bind(Editor, KeyStroke::ctrl(c('f')), SearchFind),
        bind(Editor, KeyStroke::ctrl(c('h')), SearchReplace),
        bind(Editor, KeyStroke::plain(f(3)), SearchFindNext),
        bind(Editor, KeyStroke::shift(f(3)), SearchFindPrevious),
        bind(Editor, KeyStroke::ctrl(f(3)), SearchSelectionNext),
        bind(Editor, KeyStroke::ctrl_shift(f(3)), SearchSelectionPrevious),
        bind(Global, KeyStroke::ctrl(c('g')), SearchGoToPosition),
        bind(Editor, KeyStroke::plain(Escape), SearchCancel),
        bind(Find, KeyStroke::plain(f(3)), SearchFindNext),
        bind(Find, KeyStroke::shift(f(3)), SearchFindPrevious),
        bind(Replace, KeyStroke::plain(f(3)), SearchFindNext),
        bind(Replace, KeyStroke::shift(f(3)), SearchFindPrevious),
        bind(Find, KeyStroke::alt(Up), SearchHistoryPrevious),
        bind(Find, KeyStroke::alt(Down), SearchHistoryNext),
        bind(Replace, KeyStroke::alt(Up), SearchHistoryPrevious),
        bind(Replace, KeyStroke::alt(Down), SearchHistoryNext),
        bind(Global, KeyStroke::ctrl_shift(c('f')), SearchProject),
        bind(Global, KeyStroke::ctrl_shift(c('h')), SearchReplaceProject),
        bind(Find, KeyStroke::plain(Enter), SearchFindNext),
        bind(Find, KeyStroke::shift(Enter), SearchFindPrevious),
        bind(Find, KeyStroke::alt(c('c')), SearchToggleCase),
        bind(Find, KeyStroke::alt(c('w')), SearchToggleWholeWord),
        bind(Find, KeyStroke::alt(c('r')), SearchToggleRegex),
        bind(Replace, KeyStroke::plain(Enter), SearchFindNext),
        bind(Replace, KeyStroke::shift(Enter), SearchFindPrevious),
        bind(Replace, KeyStroke::alt(c('c')), SearchToggleCase),
        bind(Replace, KeyStroke::alt(c('w')), SearchToggleWholeWord),
        bind(Replace, KeyStroke::alt(c('r')), SearchToggleRegex),
        bind(Global, KeyStroke::ctrl_shift(c('p')), PaletteOpen),
        bind(Global, KeyStroke::alt_shift(c('2')), EditorSplitRight),
        bind(Global, KeyStroke::alt_shift(c('1')), EditorOneGroup),
        bind(Global, KeyStroke::ctrl(c('1')), EditorFocusPrimary),
        bind(Global, KeyStroke::ctrl(c('2')), EditorFocusSecondary),
        bind(Global, KeyStroke::ctrl_alt(c('p')), EditorTogglePin),
        bind(Global, KeyStroke::ctrl_alt(c('l')), EditorToggleScrollLink),
        bind(Editor, KeyStroke::ctrl(Enter), EditorOpenToSide),
        bind(Sidebar, KeyStroke::ctrl(Enter), EditorOpenToSide),
        bind(Global, KeyStroke::alt(Left), NavigateBack),
        bind(Global, KeyStroke::alt(Right), NavigateForward),
        bind(Global, KeyStroke::ctrl(Tab), TabNext),
        bind(Global, KeyStroke::ctrl_shift(Tab), TabPrevious),
        bind(Global, KeyStroke::ctrl(PageDown), TabNext),
        bind(Global, KeyStroke::ctrl(PageUp), TabPrevious),
        bind(Global, KeyStroke::alt(c('1')), Tab1),
        bind(Global, KeyStroke::alt(c('2')), Tab2),
        bind(Global, KeyStroke::alt(c('3')), Tab3),
        bind(Global, KeyStroke::alt(c('4')), Tab4),
        bind(Global, KeyStroke::alt(c('5')), Tab5),
        bind(Global, KeyStroke::alt(c('6')), Tab6),
        bind(Global, KeyStroke::alt(c('7')), Tab7),
        bind(Global, KeyStroke::alt(c('8')), Tab8),
        bind(Global, KeyStroke::alt(c('9')), Tab9),
        bind(Global, KeyStroke::ctrl_shift(c('e')), ViewExplorer),
        bind(Global, KeyStroke::ctrl_alt(c('d')), ViewDatabase),
        bind(Global, KeyStroke::ctrl_shift(c('g')), ViewGit),
        bind(Global, KeyStroke::ctrl(c('j')), ViewToggleBottomPanel),
        bind(Global, KeyStroke::ctrl(c('`')), ViewTerminal),
        bind(BottomPanel, KeyStroke::alt(c('1')), ViewProblems),
        bind(BottomPanel, KeyStroke::alt(c('2')), ViewOutput),
        bind(BottomPanel, KeyStroke::alt(c('3')), ViewSearchResults),
        bind(BottomPanel, KeyStroke::plain(Escape), SearchCancelProject),
        bind(BottomPanel, KeyStroke::alt(c('4')), ViewTerminal),
        bind(Global, KeyStroke::plain(f(6)), FocusNext),
        bind(Global, KeyStroke::shift(f(6)), FocusPrevious),
        bind(Global, KeyStroke::plain(f(5)), ProgressRun),
        bind(Global, KeyStroke::plain(f(7)), ProgressCheck),
        bind(Global, KeyStroke::ctrl(f(7)), ProgressCompile),
        bind(Global, KeyStroke::shift(f(7)), ProgressRefreshSchema),
        bind(Global, KeyStroke::alt(c('f')), MenuFile),
        bind(Global, KeyStroke::alt(c('e')), MenuEdit),
        bind(Global, KeyStroke::alt(c('s')), MenuSearch),
        bind(Global, KeyStroke::alt(c('p')), MenuProject),
        bind(Global, KeyStroke::alt(c('b')), MenuBuild),
        bind(Global, KeyStroke::alt(c('t')), MenuTools),
        bind(Global, KeyStroke::alt(c('v')), MenuView),
        bind(Global, KeyStroke::alt(c('h')), MenuHelp),
    ])
}
