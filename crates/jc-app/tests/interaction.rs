use jc_app::*;

fn editor_context() -> CommandContext {
    CommandContext {
        has_document: true,
        document_dirty: true,
        dirty_documents: 1,
        can_undo: true,
        can_redo: true,
        has_selection: true,
        clipboard_has_text: true,
        has_project: true,
        active_language: Some("abl".into()),
        has_progress_profile: true,
        has_database_schema: true,
        can_reopen_closed_tab: true,
        terminal_available: true,
        ..CommandContext::default()
    }
}

#[test]
fn default_registry_keymap_and_menus_are_internally_consistent() {
    let registry = default_registry();
    let keymap = default_keymap();
    let menus = default_menu_model();
    assert!(keymap.validate(&registry).is_empty());
    assert!(menus.validate(&registry).is_empty());
    assert!(registry.iter().count() > 50);
}

#[test]
fn save_is_the_same_command_from_menu_and_keyboard() {
    let registry = default_registry();
    let keymap = default_keymap();
    let menus = default_menu_model();
    let ctx = editor_context();
    let resolution = keymap.resolve(
        KeyScope::Editor,
        KeyStroke::ctrl(Key::Char('s')),
        &registry,
        &ctx,
    );
    assert_eq!(resolution, Resolution::Enabled(CommandId::FileSave));
    let entry = menus
        .entries(MenuId::File, &registry, &keymap, &ctx)
        .into_iter()
        .flatten()
        .find(|entry| entry.command == CommandId::FileSave)
        .unwrap();
    assert_eq!(entry.shortcut, Some(KeyStroke::ctrl(Key::Char('s'))));
    assert!(entry.enabled);
}

#[test]
fn save_binding_remains_visible_but_disabled_when_document_is_clean() {
    let registry = default_registry();
    let keymap = default_keymap();
    let mut ctx = editor_context();
    ctx.document_dirty = false;
    ctx.dirty_documents = 0;
    assert_eq!(
        keymap.resolve(KeyScope::Editor, KeyStroke::ctrl(Key::Char('s')), &registry, &ctx),
        Resolution::Disabled(CommandId::FileSave)
    );
}

#[test]
fn progress_shortcuts_require_abl_and_a_progress_profile() {
    let registry = default_registry();
    let keymap = default_keymap();
    let mut ctx = editor_context();
    assert_eq!(
        keymap.resolve(KeyScope::Editor, KeyStroke::plain(Key::Function(7)), &registry, &ctx),
        Resolution::Enabled(CommandId::ProgressCheck)
    );
    ctx.active_language = Some("rust".into());
    assert_eq!(
        keymap.resolve(KeyScope::Editor, KeyStroke::plain(Key::Function(7)), &registry, &ctx),
        Resolution::Disabled(CommandId::ProgressCheck)
    );
    ctx.active_language = Some("abl".into());
    ctx.has_progress_profile = false;
    assert_eq!(
        keymap.resolve(KeyScope::Editor, KeyStroke::ctrl(Key::Function(7)), &registry, &ctx),
        Resolution::Disabled(CommandId::ProgressCompile)
    );
}

#[test]
fn contextual_bottom_panel_shortcuts_override_global_tab_shortcuts() {
    let registry = default_registry();
    let keymap = default_keymap();
    let ctx = editor_context();
    let alt_1 = KeyStroke::alt(Key::Char('1'));
    assert_eq!(
        keymap.resolve(KeyScope::Editor, alt_1, &registry, &ctx),
        Resolution::Enabled(CommandId::Tab1)
    );
    assert_eq!(
        keymap.resolve(KeyScope::BottomPanel, alt_1, &registry, &ctx),
        Resolution::Enabled(CommandId::ViewProblems)
    );
}

#[test]
fn ctrl_tab_and_ctrl_page_down_share_one_semantic_command() {
    let registry = default_registry();
    let keymap = default_keymap();
    let ctx = editor_context();
    for stroke in [KeyStroke::ctrl(Key::Tab), KeyStroke::ctrl(Key::PageDown)] {
        assert_eq!(
            keymap.resolve(KeyScope::Editor, stroke, &registry, &ctx),
            Resolution::Enabled(CommandId::TabNext)
        );
    }
}

#[test]
fn plain_tab_is_never_a_global_or_editor_action_shortcut() {
    let keymap = default_keymap();
    assert!(!keymap.bindings().iter().any(|binding| {
        matches!(binding.scope, KeyScope::Global | KeyScope::Editor)
            && binding.stroke == KeyStroke::plain(Key::Tab)
    }));
}

#[test]
fn escape_closes_transient_layers_before_returning_to_editor() {
    let mut panels = PanelManager::default();
    panels.show_bottom(BottomPanel::Output);
    let mut focus = FocusManager::default();
    assert!(focus.focus(PrimaryFocus::BottomPanel, &panels));
    focus.open_overlay(Overlay::Find);
    focus.open_overlay(Overlay::Dialog);
    assert_eq!(focus.escape(), EscapeOutcome::ClosedOverlay(Overlay::Dialog));
    assert_eq!(focus.escape(), EscapeOutcome::ClosedOverlay(Overlay::Find));
    assert_eq!(focus.escape(), EscapeOutcome::FocusedEditor);
    assert_eq!(focus.primary(), PrimaryFocus::Editor);
    assert_eq!(focus.escape(), EscapeOutcome::NoChange);
}

#[test]
fn f6_focus_cycle_only_visits_visible_primary_areas() {
    let mut panels = PanelManager::default();
    let mut focus = FocusManager::default();
    assert_eq!(focus.next(&panels), PrimaryFocus::Sidebar);
    assert_eq!(focus.next(&panels), PrimaryFocus::Editor);
    panels.show_bottom(BottomPanel::Problems);
    assert_eq!(focus.next(&panels), PrimaryFocus::Sidebar);
    assert_eq!(focus.next(&panels), PrimaryFocus::BottomPanel);
    assert_eq!(focus.next(&panels), PrimaryFocus::Editor);
    assert_eq!(focus.previous(&panels), PrimaryFocus::BottomPanel);
    focus.open_overlay(Overlay::Menu);
    assert_eq!(focus.next(&panels), PrimaryFocus::BottomPanel);
}

#[test]
fn dirty_tabs_cannot_close_without_an_explicit_choice() {
    let mut tabs = TabManager::default();
    let first = tabs.open("one.p");
    let second = tabs.open("two.p");
    assert!(tabs.set_dirty(second, true));
    assert_eq!(tabs.request_close(second), CloseRequest::NeedsSave(second));
    assert_eq!(tabs.tabs().len(), 2);
    assert_eq!(tabs.discard_and_close(second), CloseRequest::Closed(Tab {
        id: second,
        title: "two.p".into(),
        dirty: true,
    }));
    assert_eq!(tabs.active_id(), Some(first));
    assert!(tabs.can_reopen());
    assert_eq!(tabs.reopen_last(), Some(second));
    assert!(!tabs.active().unwrap().dirty);
}

#[test]
fn tab_navigation_wraps_and_alt_9_semantics_can_select_last() {
    let mut tabs = TabManager::default();
    let one = tabs.open("1");
    let two = tabs.open("2");
    let three = tabs.open("3");
    assert_eq!(tabs.active_id(), Some(three));
    assert_eq!(tabs.next(), Some(one));
    assert_eq!(tabs.previous(), Some(three));
    assert!(tabs.select_index(2));
    assert_eq!(tabs.active_id(), Some(two));
    assert!(tabs.select_last());
    assert_eq!(tabs.active_id(), Some(three));
}

#[test]
fn unsaved_dialog_has_stable_unique_keyboard_choices() {
    let dialog = DialogSpec::unsaved_changes("customer.p");
    assert!(dialog.has_unique_mnemonics());
    assert_eq!(dialog.resolve_mnemonic('S'), Some(DialogAction::Save));
    assert_eq!(dialog.resolve_mnemonic('d'), Some(DialogAction::Discard));
    assert_eq!(dialog.resolve_mnemonic('\u{1b}'), Some(DialogAction::Cancel));
    assert_eq!(dialog.resolve_mnemonic('x'), None);
}

#[test]
fn display_strings_are_stable_and_human_readable() {
    assert_eq!(KeyStroke::ctrl_shift(Key::Char('p')).to_string(), "Ctrl+Shift+P");
    assert_eq!(KeyStroke::alt(Key::Function(4)).to_string(), "Alt+F4");
    assert_eq!(KeyStroke::ctrl(Key::Char('`')).to_string(), "Ctrl+`");
}
