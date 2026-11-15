use super::menus::{next_selectable, page_selectable, previous_selectable};
use super::TuiApp;
use crate::input::{is_plain_text, to_stroke};
use crate::state::UiOverlay;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use jc_app::{KeyScope, Resolution};
use jc_text::TextEncoding;

impl TuiApp {
    pub(crate) fn handle_overlay_key(&mut self, key: KeyEvent) {
        if self.file_browser.active() {
            if let UiOverlay::Prompt { kind, input } = self.overlay.clone() {
                self.handle_browser_key(key, kind, input);
                return;
            }
        }
        if matches!(
            self.overlay,
            UiOverlay::Find { .. } | UiOverlay::Replace { .. }
        ) {
            self.handle_search_input(key);
            return;
        }
        let page_rows = self.overlay_page_rows();
        match self.overlay.clone() {
            UiOverlay::DataConnections => self.handle_data_connections_key(key),
            UiOverlay::QuickOpen { input, selected } => {
                self.handle_quick_open_key(key, input, selected)
            }
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
                    KeyCode::PageUp => selected = selected.saturating_sub(page_rows),
                    KeyCode::PageDown => {
                        selected = selected
                            .saturating_add(page_rows)
                            .min(count.saturating_sub(1))
                    }
                    KeyCode::Home => selected = 0,
                    KeyCode::End => selected = count.saturating_sub(1),
                    _ => {}
                }
                self.overlay = UiOverlay::ProjectReplacePreview { selected };
            }
            UiOverlay::Prompt { kind, mut input } => match key.code {
                KeyCode::Char('r')
                    if kind == crate::state::PromptKind::ProjectQuery
                        && key.modifiers == KeyModifiers::ALT =>
                {
                    self.status = match self.workbench.toggle_search_regex() {
                        Ok(()) => format!(
                            "Regex del proyecto: {} · reemplazo literal",
                            self.workbench.search().regex
                        ),
                        Err(error) => error.to_string(),
                    };
                }
                KeyCode::Esc => {
                    self.close_overlay();
                    if kind == crate::state::PromptKind::GitExportPatch {
                        self.status = "Exportación de parche cancelada".into();
                    }
                }
                KeyCode::Enter => self.submit_prompt(kind, input),
                KeyCode::Backspace => {
                    let from =
                        jc_text::grapheme::previous_boundary(&input, input.len()).unwrap_or(0);
                    input.truncate(from);
                    self.overlay = UiOverlay::Prompt { kind, input };
                }
                KeyCode::Char('u') if key.modifiers == KeyModifiers::CONTROL => {
                    self.overlay = UiOverlay::Prompt {
                        kind,
                        input: String::new(),
                    };
                }
                KeyCode::Char(ch) if is_plain_text(key) => {
                    self.append_overlay_text(&ch.to_string())
                }
                _ => {}
            },
            UiOverlay::Find { .. } | UiOverlay::Replace { .. } => {
                unreachable!("search fields handled above")
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
                            self.status = "El comando seleccionado está deshabilitado".to_owned();
                        }
                    }
                }
                KeyCode::Backspace => {
                    let from =
                        jc_text::grapheme::previous_boundary(&input, input.len()).unwrap_or(0);
                    input.truncate(from);
                    self.overlay = UiOverlay::Palette { input, selected: 0 };
                }
                KeyCode::Char('u') if key.modifiers == KeyModifiers::CONTROL => {
                    self.overlay = UiOverlay::Palette {
                        input: String::new(),
                        selected: 0,
                    };
                }
                KeyCode::Home | KeyCode::End | KeyCode::PageUp | KeyCode::PageDown => {
                    let last = self.palette_commands(&input).len().saturating_sub(1);
                    selected = match key.code {
                        KeyCode::Home => 0,
                        KeyCode::End => last,
                        KeyCode::PageUp => selected.saturating_sub(page_rows),
                        _ => selected.saturating_add(page_rows).min(last),
                    };
                    self.overlay = UiOverlay::Palette { input, selected };
                }
                KeyCode::Char(ch) if is_plain_text(key) => {
                    self.append_overlay_text(&ch.to_string())
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
                    KeyCode::Home | KeyCode::End | KeyCode::PageUp | KeyCode::PageDown => {
                        selected = match key.code {
                            KeyCode::Home => next_selectable(&entries, usize::MAX),
                            KeyCode::End => page_selectable(&entries, 0, entries.len(), true),
                            KeyCode::PageUp => {
                                page_selectable(&entries, selected, page_rows, false)
                            }
                            _ => page_selectable(&entries, selected, page_rows, true),
                        };
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
                KeyCode::Char(ch)
                    if ch.eq_ignore_ascii_case(&'g') || ch.eq_ignore_ascii_case(&'s') =>
                {
                    match self.workbench.save_active() {
                        Ok(()) => {
                            let _ = self.workbench.close_active(true);
                            self.status = "Guardado y cerrado".to_owned();
                            self.close_overlay();
                        }
                        Err(error) => self.status = error.to_string(),
                    }
                }
                KeyCode::Char(ch)
                    if ch.eq_ignore_ascii_case(&'n') || ch.eq_ignore_ascii_case(&'d') =>
                {
                    if self.workbench.tabs().active_id() == Some(tab) {
                        let _ = self.workbench.close_active(true);
                    }
                    self.status = "Cerrado sin guardar".to_owned();
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

    pub(super) fn try_overlay_command(&mut self, key: KeyEvent) -> bool {
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

    pub(crate) fn append_overlay_text(&mut self, text: &str) {
        if self.file_browser.active() && matches!(self.overlay, UiOverlay::Prompt { .. }) {
            self.edit_browser_field(jc_app::FieldEdit::Insert(text), false);
            return;
        }
        if matches!(
            self.overlay,
            UiOverlay::Find { .. } | UiOverlay::Replace { .. }
        ) {
            self.edit_search_field(jc_app::FieldEdit::Insert(text));
            return;
        }
        let bounded = match &self.overlay {
            UiOverlay::Prompt { kind, input } => Some((input.len(), prompt_limit(*kind))),
            UiOverlay::Palette { input, .. } => Some((input.len(), 256)),
            _ => None,
        };
        if let Some((len, limit)) = bounded {
            if text.len() > limit.saturating_sub(len) {
                self.status = format!("La entrada supera {limit} bytes UTF-8");
                return;
            }
        }
        match self.overlay.clone() {
            UiOverlay::DataConnections => {
                self.edit_data_connection(jc_app::FieldEdit::Insert(text))
            }
            UiOverlay::QuickOpen { input, .. } => self.append_quick_open_text(input, text),
            UiOverlay::Prompt { kind, mut input } => {
                input.push_str(text);
                self.overlay = UiOverlay::Prompt { kind, input };
            }
            UiOverlay::Find { .. } | UiOverlay::Replace { .. } => {
                unreachable!("search fields handled above")
            }
            UiOverlay::Palette { mut input, .. } => {
                input.push_str(text);
                self.overlay = UiOverlay::Palette { input, selected: 0 };
            }
            _ => {}
        }
    }
    fn overlay_page_rows(&self) -> usize {
        let height = self.last_frame_height.saturating_sub(2);
        usize::from(
            match self.overlay {
                UiOverlay::Menu { .. } => height.saturating_sub(2),
                UiOverlay::Palette { .. } => height.min(16).saturating_sub(4),
                UiOverlay::ProjectReplacePreview { .. } => height.min(22).saturating_sub(6),
                _ => 10,
            }
            .max(1),
        )
    }
}

pub(super) fn prompt_limit(kind: crate::state::PromptKind) -> usize {
    if matches!(
        kind,
        crate::state::PromptKind::GoToPosition | crate::state::PromptKind::RenameSymbol
    ) {
        64
    } else {
        4096
    }
}
