//! Multiline document fields. Query and replacement retain exact pasted EOL.
use super::TuiApp;
use crate::{
    input::is_plain_text,
    state::{ReplaceField, UiOverlay},
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use jc_app::{edit_field, CommandId, FieldEdit};

impl TuiApp {
    pub(super) fn edit_search_field(&mut self, edit: FieldEdit<'_>) {
        let result = match &mut self.overlay {
            UiOverlay::Find { input } => edit_field(input, &mut self.find_cursor, edit, 4096)
                .map(|changed| if changed { Some(input.clone()) } else { None }),
            UiOverlay::Replace {
                find,
                replacement,
                field,
            } => match field {
                ReplaceField::Find => edit_field(find, &mut self.find_cursor, edit, 4096)
                    .map(|changed| if changed { Some(find.clone()) } else { None }),
                ReplaceField::Replacement => {
                    edit_field(replacement, &mut self.replacement_cursor, edit, 4096).map(|_| None)
                }
            },
            _ => return,
        };
        match result {
            Ok(Some(query)) => {
                if let Err(error) = self.workbench.set_search_query(query) {
                    self.status = error.to_string();
                } else {
                    self.status = if self.workbench.search().regex {
                        "Regex query updated · replacement is literal"
                    } else {
                        "Literal query updated · exact EOL"
                    }
                    .into();
                }
            }
            Ok(None) => self.status = "Field ready · exact source text; no normalization".into(),
            Err(error) => self.status = error.into(),
        }
    }
    pub(super) fn handle_search_input(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Char('u') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.edit_search_field(FieldEdit::Clear);
            return;
        }
        if key.code == KeyCode::Esc {
            self.workbench.cancel_search();
            self.close_overlay();
            return;
        }
        let insert = match key.code {
            KeyCode::F(2) if key.modifiers.is_empty() => Some("\n"),
            KeyCode::F(4) if key.modifiers.is_empty() => Some("\r\n"),
            KeyCode::F(7) if key.modifiers.is_empty() => Some("\t"),
            KeyCode::F(8) if key.modifiers.is_empty() => Some("\r"),
            KeyCode::Enter
                if key
                    .modifiers
                    .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) =>
            {
                Some("\n")
            }
            _ => None,
        };
        if let Some(text) = insert {
            self.edit_search_field(FieldEdit::Insert(text));
            return;
        }
        if matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
            if let UiOverlay::Replace { field, .. } = &mut self.overlay {
                *field = match field {
                    ReplaceField::Find => ReplaceField::Replacement,
                    ReplaceField::Replacement => ReplaceField::Find,
                };
            }
            return;
        }
        if key.code == KeyCode::Enter || matches!(key.code, KeyCode::F(6) | KeyCode::F(9)) {
            if let UiOverlay::Replace { replacement, .. } = &self.overlay {
                let all =
                    key.code == KeyCode::F(9) || key.modifiers.contains(KeyModifiers::CONTROL);
                self.status = if all {
                    self.workbench
                        .replace_all(replacement)
                        .map(|n| format!("Replaced {n} matches"))
                } else {
                    self.workbench.replace_current(replacement).map(|yes| {
                        if yes {
                            "Replaced current match".into()
                        } else {
                            "No match to replace".into()
                        }
                    })
                }
                .unwrap_or_else(|e| e.to_string());
            } else {
                self.dispatch(CommandId::SearchFindNext);
            }
            return;
        }
        if self.try_overlay_command(key) {
            return;
        }
        let edit = match key.code {
            KeyCode::Left => Some(FieldEdit::Left),
            KeyCode::Right => Some(FieldEdit::Right),
            KeyCode::Up => Some(FieldEdit::Up),
            KeyCode::Down => Some(FieldEdit::Down),
            KeyCode::Home => Some(FieldEdit::Home),
            KeyCode::End => Some(FieldEdit::End),
            KeyCode::Backspace => Some(FieldEdit::Backspace),
            KeyCode::Delete => Some(FieldEdit::Delete),
            _ => None,
        };
        if let Some(edit) = edit {
            self.edit_search_field(edit);
        } else if let KeyCode::Char(ch) = key.code {
            if is_plain_text(key) {
                self.edit_search_field(FieldEdit::Insert(&ch.to_string()));
            }
        }
    }
}
