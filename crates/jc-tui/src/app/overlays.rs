use super::menus::{next_selectable, previous_selectable};
use super::TuiApp;
use crate::input::{is_plain_text, to_stroke};
use crate::state::{ReplaceField, UiOverlay};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use jc_app::{KeyScope, Resolution};
use jc_text::TextEncoding;

impl TuiApp {
    pub(crate) fn handle_overlay_key(&mut self, key: KeyEvent) {
        match self.overlay.clone() {
            UiOverlay::ProjectReplacePreview { mut selected } => {
                let count = self.replace_preview.as_ref().map_or(0, |p| p.lines().len());
                match key.code {
                    KeyCode::Esc => {
                        self.replace_preview = None;
                        self.close_overlay();
                        return;
                    }
                    KeyCode::Enter if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        self.accept_project_replace();
                        return;
                    }
                    KeyCode::F(9) => {
                        self.accept_project_replace();
                        return;
                    }
                    KeyCode::Up => selected = selected.saturating_sub(1),
                    KeyCode::Down => selected = (selected + 1).min(count.saturating_sub(1)),
                    KeyCode::Home => selected = 0,
                    KeyCode::End => selected = count.saturating_sub(1),
                    _ => {}
                }
                self.overlay = UiOverlay::ProjectReplacePreview { selected };
            }
            UiOverlay::Prompt { kind, mut input } => match key.code {
                KeyCode::Esc => self.close_overlay(),
                KeyCode::Enter => self.submit_prompt(kind, input),
                KeyCode::Backspace => {
                    input.pop();
                    self.overlay = UiOverlay::Prompt { kind, input };
                }
                KeyCode::Char(ch) if is_plain_text(key) => {
                    if matches!(
                        kind,
                        crate::state::PromptKind::ProjectQuery
                            | crate::state::PromptKind::ProjectInclude
                            | crate::state::PromptKind::ProjectExclude
                            | crate::state::PromptKind::ProjectReplacement
                    ) && input.len() + ch.len_utf8() > 4096
                    {
                        self.status = "Project query/filter exceeds 4096 bytes".into();
                        return;
                    }
                    input.push(ch);
                    self.overlay = UiOverlay::Prompt { kind, input };
                }
                _ => {}
            },
            UiOverlay::Find { mut input } => {
                if key.code == KeyCode::Esc {
                    self.close_overlay();
                    return;
                }
                if self.try_overlay_command(key) {
                    return;
                }
                match key.code {
                    KeyCode::Backspace => {
                        input.pop();
                        self.update_find(input);
                    }
                    KeyCode::Char(ch) if is_plain_text(key) => {
                        input.push(ch);
                        self.update_find(input);
                    }
                    _ => {}
                }
            }
            UiOverlay::Replace {
                mut find,
                mut replacement,
                mut field,
            } => {
                if key.code == KeyCode::Esc {
                    self.close_overlay();
                    return;
                }
                if key.code == KeyCode::Tab || key.code == KeyCode::BackTab {
                    field = match field {
                        ReplaceField::Find => ReplaceField::Replacement,
                        ReplaceField::Replacement => ReplaceField::Find,
                    };
                    self.overlay = UiOverlay::Replace {
                        find,
                        replacement,
                        field,
                    };
                    return;
                }
                if key.code == KeyCode::Enter {
                    if key.modifiers.contains(KeyModifiers::CONTROL) {
                        match self.workbench.replace_all(&replacement) {
                            Ok(count) => self.status = format!("Replaced {count} matches"),
                            Err(error) => self.status = error.to_string(),
                        }
                    } else {
                        match self.workbench.replace_current(&replacement) {
                            Ok(true) => self.status = "Replaced current match".to_owned(),
                            Ok(false) => self.status = "No match to replace".to_owned(),
                            Err(error) => self.status = error.to_string(),
                        }
                    }
                    self.overlay = UiOverlay::Replace {
                        find,
                        replacement,
                        field,
                    };
                    return;
                }
                match key.code {
                    KeyCode::Backspace => match field {
                        ReplaceField::Find => {
                            find.pop();
                            self.update_replace_query(&find);
                        }
                        ReplaceField::Replacement => {
                            replacement.pop();
                        }
                    },
                    KeyCode::Char(ch) if is_plain_text(key) => match field {
                        ReplaceField::Find => {
                            find.push(ch);
                            self.update_replace_query(&find);
                        }
                        ReplaceField::Replacement => replacement.push(ch),
                    },
                    _ => {}
                }
                self.overlay = UiOverlay::Replace {
                    find,
                    replacement,
                    field,
                };
            }
            UiOverlay::Palette {
                mut input,
                mut selected,
            } => match key.code {
                KeyCode::Esc => self.close_overlay(),
                KeyCode::Up => {
                    selected = selected.saturating_sub(1);
                    self.overlay = UiOverlay::Palette { input, selected };
                }
                KeyCode::Down => {
                    let count = self.palette_commands(&input).len();
                    if count > 0 {
                        selected = (selected + 1).min(count - 1);
                    }
                    self.overlay = UiOverlay::Palette { input, selected };
                }
                KeyCode::Enter => {
                    let commands = self.palette_commands(&input);
                    if let Some(command) = commands.get(selected).copied() {
                        let context = self.command_context();
                        if self.registry.is_enabled(command, &context) {
                            self.close_overlay();
                            self.dispatch(command);
                        } else {
                            self.status = "Selected command is disabled".to_owned();
                        }
                    }
                }
                KeyCode::Backspace => {
                    input.pop();
                    self.overlay = UiOverlay::Palette { input, selected: 0 };
                }
                KeyCode::Char(ch) if is_plain_text(key) => {
                    input.push(ch);
                    self.overlay = UiOverlay::Palette { input, selected: 0 };
                }
                _ => {}
            },
            UiOverlay::Menu { menu, mut selected } => {
                let entries = self.menu_commands(menu);
                match key.code {
                    KeyCode::Esc => self.close_overlay(),
                    KeyCode::Up => {
                        selected = previous_selectable(&entries, selected);
                        self.overlay = UiOverlay::Menu { menu, selected };
                    }
                    KeyCode::Down => {
                        selected = next_selectable(&entries, selected);
                        self.overlay = UiOverlay::Menu { menu, selected };
                    }
                    KeyCode::Left => self.switch_menu(menu, -1),
                    KeyCode::Right => self.switch_menu(menu, 1),
                    KeyCode::Enter => {
                        if let Some(Some(command)) = entries.get(selected) {
                            let command = *command;
                            self.close_overlay();
                            self.dispatch(command);
                        }
                    }
                    _ => {}
                }
            }
            UiOverlay::CloseDialog { tab } => match key.code {
                KeyCode::Esc => self.close_overlay(),
                KeyCode::Char(ch) if ch.eq_ignore_ascii_case(&'s') => {
                    match self.workbench.save_active() {
                        Ok(()) => {
                            let _ = self.workbench.close_active(true);
                            self.status = "Saved and closed".to_owned();
                            self.close_overlay();
                        }
                        Err(error) => self.status = error.to_string(),
                    }
                }
                KeyCode::Char(ch) if ch.eq_ignore_ascii_case(&'d') => {
                    if self.workbench.tabs().active_id() == Some(tab) {
                        let _ = self.workbench.close_active(true);
                    }
                    self.status = "Closed without saving".to_owned();
                    self.close_overlay();
                }
                _ => {}
            },
            UiOverlay::EncodingChoice => match key.code {
                KeyCode::Esc => self.close_overlay(),
                KeyCode::Char('1') => self.choose_encoding(TextEncoding::Windows1252),
                KeyCode::Char('2') => self.choose_encoding(TextEncoding::Iso8859_1),
                KeyCode::Char('3') => self.choose_encoding(TextEncoding::Ibm850),
                KeyCode::Char('4') => self.choose_encoding(TextEncoding::Utf8),
                _ => {}
            },
            UiOverlay::None => {}
        }
    }

    fn try_overlay_command(&mut self, key: KeyEvent) -> bool {
        let Some(stroke) = to_stroke(key) else {
            return false;
        };
        let scope = KeyScope::from_focus(self.focus.primary(), self.focus.active_overlay());
        let context = self.command_context();
        match self.keymap.resolve(scope, stroke, &self.registry, &context) {
            Resolution::Enabled(command) => {
                self.dispatch(command);
                true
            }
            Resolution::Disabled(_) => true,
            Resolution::None => false,
        }
    }

    fn update_find(&mut self, input: String) {
        match self.workbench.set_search_query(input.clone()) {
            Ok(()) => {
                self.overlay = UiOverlay::Find { input };
                self.status = format!("{} matches", self.workbench.search().matches.len());
            }
            Err(error) => {
                self.overlay = UiOverlay::Find { input };
                self.status = error.to_string();
            }
        }
    }

    fn update_replace_query(&mut self, query: &str) {
        match self.workbench.set_search_query(query.to_owned()) {
            Ok(()) => self.status = format!("{} matches", self.workbench.search().matches.len()),
            Err(error) => self.status = error.to_string(),
        }
    }

    pub(crate) fn append_overlay_text(&mut self, text: &str) {
        match self.overlay.clone() {
            UiOverlay::Prompt { kind, mut input } => {
                if matches!(
                    kind,
                    crate::state::PromptKind::ProjectQuery
                        | crate::state::PromptKind::ProjectInclude
                        | crate::state::PromptKind::ProjectExclude
                        | crate::state::PromptKind::ProjectReplacement
                ) && input.len() + text.len() > 4096
                {
                    self.status = "Project query/filter exceeds 4096 bytes".into();
                    return;
                }
                input.push_str(text);
                self.overlay = UiOverlay::Prompt { kind, input };
            }
            UiOverlay::Find { mut input } => {
                input.push_str(text);
                self.update_find(input);
            }
            UiOverlay::Replace {
                mut find,
                mut replacement,
                field,
            } => {
                match field {
                    ReplaceField::Find => {
                        find.push_str(text);
                        self.update_replace_query(&find);
                    }
                    ReplaceField::Replacement => replacement.push_str(text),
                }
                self.overlay = UiOverlay::Replace {
                    find,
                    replacement,
                    field,
                };
            }
            UiOverlay::Palette { mut input, .. } => {
                input.push_str(text);
                self.overlay = UiOverlay::Palette { input, selected: 0 };
            }
            _ => {}
        }
    }
}
