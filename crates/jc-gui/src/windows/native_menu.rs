//! Native menu composition from the shared model plus implemented GUI handlers.
//! Command IDs are identities, never category ranges. A popup owns no editor
//! borrow, and its result is revalidated before the ordinary WM_COMMAND route.
use super::{command_id, ffi::*, owner, supported, wide, State, COMMANDS, OPEN_SIDE};
use jc_app::{
    default_menu_model, default_registry, CommandCategory, CommandId, KeyMap, KeyStroke, MenuId,
    MenuItem, Modifiers,
};
use std::{collections::BTreeSet, ptr::null};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NativeMenuGroup {
    File,
    Edit,
    Search,
    Project,
    Tools,
    View,
    Navigation,
    Git,
}
impl NativeMenuGroup {
    fn title(self) -> &'static str {
        match self {
            Self::File => "&File",
            Self::Edit => "&Edit",
            Self::Search => "&Search",
            Self::Project => "&Project",
            Self::Tools => "&Tools",
            Self::View => "&View",
            Self::Navigation => "&Navigation",
            Self::Git => "&Git",
        }
    }
}
pub(super) struct MenuSpec {
    pub group: NativeMenuGroup,
    pub title: &'static str,
    pub items: Vec<Option<usize>>,
}
pub(super) fn group_for_command(command: CommandId) -> Option<NativeMenuGroup> {
    match command {
        CommandId::MenuFile => Some(NativeMenuGroup::File),
        CommandId::MenuEdit => Some(NativeMenuGroup::Edit),
        CommandId::MenuSearch => Some(NativeMenuGroup::Search),
        CommandId::MenuProject => Some(NativeMenuGroup::Project),
        CommandId::MenuTools => Some(NativeMenuGroup::Tools),
        CommandId::MenuView => Some(NativeMenuGroup::View),
        _ => None,
    }
}
/// Independent of `supported`/definitions so the frontend can admit these six
/// native-menu macros without creating a support/composition recursion.
pub(super) fn is_supported_macro(command: CommandId) -> bool {
    group_for_command(command).is_some()
}
pub(super) fn mnemonic(code: usize, modifiers: Modifiers) -> Option<NativeMenuGroup> {
    if modifiers != Modifiers::ALT {
        return None;
    }
    match code {
        0x46 => Some(NativeMenuGroup::File),
        0x45 => Some(NativeMenuGroup::Edit),
        0x53 => Some(NativeMenuGroup::Search),
        0x50 => Some(NativeMenuGroup::Project),
        0x54 => Some(NativeMenuGroup::Tools),
        0x56 => Some(NativeMenuGroup::View),
        0x4e => Some(NativeMenuGroup::Navigation),
        0x47 => Some(NativeMenuGroup::Git),
        _ => None,
    }
}
fn shared_group(id: MenuId) -> Option<NativeMenuGroup> {
    match id {
        MenuId::File => Some(NativeMenuGroup::File),
        MenuId::Edit => Some(NativeMenuGroup::Edit),
        MenuId::Search => Some(NativeMenuGroup::Search),
        MenuId::Project => Some(NativeMenuGroup::Project),
        MenuId::Tools => Some(NativeMenuGroup::Tools),
        MenuId::View => Some(NativeMenuGroup::View),
        _ => None,
    }
}
fn git_command(command: CommandId) -> bool {
    matches!(
        command,
        CommandId::ViewGit
            | CommandId::GitDiffHead
            | CommandId::GitDiffIndex
            | CommandId::GitDiffRefresh
            | CommandId::GitDiffCancel
            | CommandId::GitConfigureExecutable
            | CommandId::GitRepositoryStatus
            | CommandId::GitStatusRefresh
            | CommandId::GitStatusCancel
    )
}
fn destination(
    command: CommandId,
    category: CommandCategory,
    fallback: NativeMenuGroup,
) -> NativeMenuGroup {
    if git_command(command) {
        NativeMenuGroup::Git
    } else if category == CommandCategory::Navigation || command == CommandId::SearchGoToPosition {
        NativeMenuGroup::Navigation
    } else {
        fallback
    }
}
fn category_group(category: CommandCategory) -> Option<NativeMenuGroup> {
    match category {
        CommandCategory::File => Some(NativeMenuGroup::File),
        CommandCategory::Edit => Some(NativeMenuGroup::Edit),
        CommandCategory::Search => Some(NativeMenuGroup::Search),
        CommandCategory::Project => Some(NativeMenuGroup::Project),
        CommandCategory::Tools => Some(NativeMenuGroup::Tools),
        CommandCategory::View => Some(NativeMenuGroup::View),
        CommandCategory::Navigation => Some(NativeMenuGroup::Navigation),
        _ => None,
    }
}
fn spec_mut(specs: &mut [MenuSpec], group: NativeMenuGroup) -> &mut MenuSpec {
    specs.iter_mut().find(|spec| spec.group == group).unwrap()
}
fn separator(items: &mut Vec<Option<usize>>) {
    if items.last().is_some_and(Option::is_some) {
        items.push(None);
    }
}
pub(super) fn definitions() -> Vec<MenuSpec> {
    let mut specs = [
        NativeMenuGroup::File,
        NativeMenuGroup::Edit,
        NativeMenuGroup::Search,
        NativeMenuGroup::Project,
        NativeMenuGroup::Tools,
        NativeMenuGroup::View,
        NativeMenuGroup::Navigation,
        NativeMenuGroup::Git,
    ]
    .into_iter()
    .map(|group| MenuSpec {
        group,
        title: group.title(),
        items: Vec::new(),
    })
    .collect::<Vec<_>>();
    let model = default_menu_model();
    let registry = default_registry();
    let mut seen = BTreeSet::new();
    for menu in model.menus() {
        let Some(group) = shared_group(menu.id) else {
            continue;
        };
        for item in &menu.items {
            match item {
                MenuItem::Separator => separator(&mut spec_mut(&mut specs, group).items),
                MenuItem::Command(command) => {
                    let id = command_id(*command);
                    let Some(command_spec) = registry.get(*command) else {
                        continue;
                    };
                    if id == 0 || !supported(*command) || !seen.insert(id) {
                        continue;
                    }
                    let destination = destination(*command, command_spec.category, group);
                    spec_mut(&mut specs, destination).items.push(Some(id));
                }
            }
        }
    }
    // The shared model intentionally does not list every registry command.
    // Include implemented Search flags, indexed tabs and focus routes once.
    for (id, command, _) in COMMANDS {
        if seen.contains(id) || !supported(*command) {
            continue;
        }
        let Some(command_spec) = registry.get(*command) else {
            continue;
        };
        let Some(group) = category_group(command_spec.category) else {
            continue;
        };
        let group = destination(*command, command_spec.category, group);
        if seen.insert(*id) {
            spec_mut(&mut specs, group).items.push(Some(*id));
        }
    }
    if seen.insert(OPEN_SIDE) {
        let items = &mut spec_mut(&mut specs, NativeMenuGroup::File).items;
        let position = items
            .iter()
            .position(|id| *id == Some(command_id(CommandId::FileOpen)))
            .map_or(items.len(), |index| index + 1);
        items.insert(position, Some(OPEN_SIDE));
    }
    for spec in &mut specs {
        while spec.items.last() == Some(&None) {
            spec.items.pop();
        }
    }
    specs.retain(|spec| !spec.items.is_empty());
    specs
}

#[link(name = "user32")]
extern "system" {
    fn GetSubMenu(menu: Handle, position: i32) -> Handle;
    fn IsMenu(menu: Handle) -> i32;
    fn GetMenuState(menu: Handle, item: u32, flags: u32) -> u32;
    fn ModifyMenuW(menu: Handle, item: u32, flags: u32, new_item: usize, label: *const u16) -> i32;
    fn GetMenuItemRect(hwnd: Hwnd, menu: Handle, item: u32, rect: *mut Rect) -> i32;
    fn ClientToScreen(hwnd: Hwnd, point: *mut Point) -> i32;
    fn TrackPopupMenuEx(
        menu: Handle,
        flags: u32,
        x: i32,
        y: i32,
        hwnd: Hwnd,
        params: *const std::ffi::c_void,
    ) -> i32;
}
fn command_for_id(id: usize) -> Option<CommandId> {
    COMMANDS.iter().find(|item| item.0 == id).map(|item| item.1)
}
fn label(id: usize, shortcut: Option<KeyStroke>) -> Result<Vec<u16>, String> {
    let text = if id == OPEN_SIDE {
        "Open File to &Side..."
    } else {
        COMMANDS
            .iter()
            .find(|item| item.0 == id)
            .map(|item| item.2)
            .ok_or("Native menu references an unimplemented command")?
    };
    Ok(wide(&match shortcut {
        Some(shortcut) => format!("{text}\t{shortcut}"),
        None => text.to_owned(),
    }))
}
/// On success the HWND caller adopts the returned native menu. DestroyMenu on
/// the root releases attached popups on every partial-construction failure.
pub(super) unsafe fn create(keymap: &KeyMap) -> Result<Handle, String> {
    let bar = CreateMenu();
    if bar.is_null() {
        return Err(super::os_error("CreateMenu"));
    }
    let result = (|| -> Result<(), String> {
        for spec in definitions() {
            let popup = CreatePopupMenu();
            if popup.is_null() {
                return Err(super::os_error("CreatePopupMenu"));
            }
            if AppendMenuW(bar, 0x10, popup as usize, wide(spec.title).as_ptr()) == 0 {
                let error = super::os_error("AppendMenu group");
                DestroyMenu(popup);
                return Err(error);
            }
            for item in spec.items {
                match item {
                    None => {
                        if AppendMenuW(popup, 0x800, 0, null()) == 0 {
                            return Err(super::os_error("AppendMenu separator"));
                        }
                    }
                    Some(id) => {
                        let shortcut =
                            command_for_id(id).and_then(|command| keymap.primary_shortcut(command));
                        if AppendMenuW(popup, 0, id, label(id, shortcut)?.as_ptr()) == 0 {
                            return Err(super::os_error("AppendMenu item"));
                        }
                    }
                }
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        DestroyMenu(bar);
        return Err(error);
    }
    Ok(bar)
}
fn checked(command: CommandId, state: &State) -> bool {
    match command {
        CommandId::ViewToggleInvisibles => state.workbench.invisibles_visible(),
        CommandId::EditorToggleScrollLink => state.workbench.panes().scroll_linked(),
        CommandId::EditorTogglePin => state
            .workbench
            .panes()
            .view(state.workbench.panes().active())
            .is_some_and(|view| view.is_pinned()),
        CommandId::ViewExplorer => state.explorer_visible,
        CommandId::ViewToggleBottomPanel => state.bottom.visible,
        CommandId::ViewProblems => {
            state.bottom.visible && state.bottom.kind == jc_app::task_panels::PanelKind::Problems
        }
        CommandId::ViewOutput => {
            state.bottom.visible && state.bottom.kind == jc_app::task_panels::PanelKind::Output
        }
        CommandId::SearchToggleCase => state.workbench.search().case_sensitive,
        CommandId::SearchToggleWholeWord => state.workbench.search().whole_word,
        CommandId::SearchToggleRegex => state.workbench.search().regex,
        _ => false,
    }
}
pub(super) unsafe fn refresh(hwnd: Hwnd, state: &State) -> Result<(), String> {
    let bar = GetMenu(hwnd);
    if bar.is_null() || IsMenu(bar) == 0 {
        return Err("Owned native menu is unavailable".into());
    }
    for (index, spec) in definitions().iter().enumerate() {
        let popup = GetSubMenu(bar, index as i32);
        if popup.is_null() || IsMenu(popup) == 0 {
            return Err("Native menu group is missing".into());
        }
        for id in spec.items.iter().flatten().copied() {
            let command = command_for_id(id);
            let capability = if id == OPEN_SIDE {
                Some(CommandId::FileOpen)
            } else {
                command
            };
            let enabled = capability.is_some_and(|command| {
                state
                    .registry
                    .is_enabled(command, &state.entry_context(command))
            });
            let checked = command.is_some_and(|command| checked(command, state));
            let shortcut = command.and_then(|command| state.native_shortcut(command));
            let flags = if enabled { 0 } else { 1 } | if checked { 8 } else { 0 };
            if ModifyMenuW(popup, id as u32, flags, id, label(id, shortcut)?.as_ptr()) == 0 {
                return Err(super::os_error("Modify native menu item"));
            }
        }
    }
    Ok(())
}
unsafe fn clear_stamp(hwnd: Hwnd) {
    if IsWindow(hwnd) != 0 {
        if let Some(model) = owner(hwnd) {
            model.state.borrow_mut().menu_stamp = None;
        }
    }
}
unsafe fn current_menu(hwnd: Hwnd, bar: Handle, popup: Handle, index: i32) -> bool {
    IsWindow(hwnd) != 0
        && IsMenu(bar) != 0
        && IsMenu(popup) != 0
        && GetMenu(hwnd) == bar
        && GetSubMenu(bar, index) == popup
}
/// The transport runs without a State/Workbench borrow. Native delayed actions
/// may reenter the owner, so validate the exact HWND/menu and group again after
/// it returns. Root's WM_COMMAND gate additionally validates its source stamp.
pub(super) unsafe fn open_with(
    hwnd: Hwnd,
    group: NativeMenuGroup,
    transport: impl FnOnce(Handle, i32, i32) -> usize,
) -> Result<(), String> {
    let result = (|| -> Result<(), String> {
        if IsWindow(hwnd) == 0 {
            return Err("Menu owner window is closed".into());
        }
        let specs = definitions();
        let index = specs
            .iter()
            .position(|spec| spec.group == group)
            .ok_or("Native menu group is not implemented")?;
        let bar = GetMenu(hwnd);
        let popup = if bar.is_null() {
            std::ptr::null_mut()
        } else {
            GetSubMenu(bar, index as i32)
        };
        if !current_menu(hwnd, bar, popup, index as i32) {
            return Err("Native menu ownership changed".into());
        }
        SendMessageW(hwnd, WM_INITMENU, bar as usize, 0);
        if !current_menu(hwnd, bar, popup, index as i32) {
            return Err("Native menu changed during initialization".into());
        }
        let mut rect = Rect::default();
        let (x, y) = if GetMenuItemRect(hwnd, bar, index as u32, &mut rect) != 0 {
            (rect.left, rect.bottom)
        } else {
            if GetClientRect(hwnd, &mut rect) == 0 {
                return Err(super::os_error("Menu client rectangle"));
            }
            let mut point = Point {
                x: rect.left,
                y: rect.top,
            };
            if ClientToScreen(hwnd, &mut point) == 0 {
                return Err(super::os_error("Menu client origin"));
            }
            (point.x, point.y)
        };
        let selected = transport(popup, x, y);
        if !current_menu(hwnd, bar, popup, index as i32) {
            return Err("Native menu changed during selection".into());
        }
        if selected == 0 {
            clear_stamp(hwnd);
            return Ok(());
        }
        if !specs[index].items.contains(&Some(selected)) {
            return Err("Chosen command does not belong to the opened native menu".into());
        }
        let selected_u32 =
            u32::try_from(selected).map_err(|_| "Native command identity is out of range")?;
        let state = GetMenuState(popup, selected_u32, 0);
        if state == u32::MAX || state & 3 != 0 {
            return Err("Chosen native menu command is missing or disabled".into());
        }
        SendMessageW(hwnd, WM_COMMAND, selected, 0);
        Ok(())
    })();
    if let Err(error) = &result {
        clear_stamp(hwnd);
        if IsWindow(hwnd) != 0 {
            if let Some(model) = owner(hwnd) {
                model.state.borrow_mut().error(error);
            }
        }
    }
    result
}
pub(super) unsafe fn open(hwnd: Hwnd, group: NativeMenuGroup) -> Result<(), String> {
    let choice = {
        let model = owner(hwnd).ok_or("Menu owner window is closed")?;
        let mut state = model.state.borrow_mut();
        if state.smoke {
            Some(state.smoke_menu_choices.pop_front().flatten().unwrap_or(0))
        } else {
            None
        }
    };
    open_with(hwnd, group, |popup, x, y| {
        choice.unwrap_or_else(|| {
            TrackPopupMenuEx(popup, 0x100 | 0x80, x, y, hwnd, null()) as u32 as usize
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn definitions_cover_each_implemented_command_once_and_have_clean_separators() {
        let specs = definitions();
        let mut ids = BTreeSet::new();
        for spec in &specs {
            assert!(!spec.items.is_empty());
            assert!(spec.items.first().unwrap().is_some());
            assert!(spec.items.last().unwrap().is_some());
            assert!(!spec.items.windows(2).any(|pair| pair == [None, None]));
            for id in spec.items.iter().flatten() {
                assert!(ids.insert(*id), "duplicate native command {id}");
            }
        }
        let expected = COMMANDS
            .iter()
            .map(|item| item.0)
            .chain([OPEN_SIDE])
            .collect::<BTreeSet<_>>();
        assert_eq!(
            expected.len(),
            COMMANDS.len() + 1,
            "handler IDs must be globally unique, including the explicit file-side picker"
        );
        assert_eq!(ids, expected);
        let file = specs
            .iter()
            .find(|spec| spec.group == NativeMenuGroup::File)
            .unwrap();
        assert!(file.items.contains(&Some(OPEN_SIDE)));
        let search = specs
            .iter()
            .find(|spec| spec.group == NativeMenuGroup::Search)
            .unwrap();
        for command in [
            CommandId::SearchToggleCase,
            CommandId::SearchToggleWholeWord,
        ] {
            assert!(search.items.contains(&Some(command_id(command))));
        }
        let nav = specs
            .iter()
            .find(|spec| spec.group == NativeMenuGroup::Navigation)
            .unwrap();
        for command in [
            CommandId::SearchGoToPosition,
            CommandId::NavigateBack,
            CommandId::NavigateForward,
            CommandId::Tab1,
            CommandId::Tab9,
        ] {
            assert!(nav.items.contains(&Some(command_id(command))));
        }
        let git = specs
            .iter()
            .find(|spec| spec.group == NativeMenuGroup::Git)
            .unwrap();
        assert!(git
            .items
            .iter()
            .flatten()
            .all(|id| git_command(command_for_id(*id).unwrap())));
    }
    #[test]
    fn native_macros_and_mnemonics_are_explicit_without_unsupported_menu_providers() {
        for command in [
            CommandId::MenuFile,
            CommandId::MenuEdit,
            CommandId::MenuSearch,
            CommandId::MenuProject,
            CommandId::MenuTools,
            CommandId::MenuView,
        ] {
            assert!(is_supported_macro(command));
            assert!(group_for_command(command).is_some());
        }
        for command in [
            CommandId::MenuBuild,
            CommandId::MenuHelp,
            CommandId::FileOpen,
        ] {
            assert!(!is_supported_macro(command));
        }
        for (code, group) in [
            (0x46, NativeMenuGroup::File),
            (0x45, NativeMenuGroup::Edit),
            (0x53, NativeMenuGroup::Search),
            (0x50, NativeMenuGroup::Project),
            (0x54, NativeMenuGroup::Tools),
            (0x56, NativeMenuGroup::View),
            (0x4e, NativeMenuGroup::Navigation),
            (0x47, NativeMenuGroup::Git),
        ] {
            assert_eq!(mnemonic(code, Modifiers::ALT), Some(group));
            assert_eq!(mnemonic(code, Modifiers::CTRL.union(Modifiers::ALT)), None);
        }
        assert_eq!(mnemonic(0x42, Modifiers::ALT), None);
        assert_eq!(mnemonic(0x48, Modifiers::ALT), None);
    }
}
