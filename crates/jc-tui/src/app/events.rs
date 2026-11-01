use super::TuiApp;
use crate::input::{is_action_event, is_plain_text, to_stroke};
use crate::state::UiOverlay;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use jc_app::{KeyScope, PrimaryFocus, Resolution};

impl TuiApp {
    pub fn handle_event(&mut self, event: Event) {
        match event {
            Event::Key(key) if is_action_event(key) => self.handle_key(key),
            Event::Paste(text) => self.handle_paste(&text),
            Event::Resize(_, _) | Event::FocusGained | Event::FocusLost | Event::Mouse(_) => {}
            _ => {}
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if !matches!(self.overlay, UiOverlay::None) {
            self.handle_overlay_key(key);
            return;
        }
        if key.code == KeyCode::Esc {
            let _ = self.focus.escape();
            return;
        }
        if let Some(stroke) = to_stroke(key) {
            let scope = KeyScope::from_focus(self.focus.primary(), self.focus.active_overlay());
            let mut context = self.workbench.context();
            context.focus = self.focus.primary();
            match self.keymap.resolve(scope, stroke, &self.registry, &context) {
                Resolution::Enabled(command) => {
                    self.dispatch(command);
                    return;
                }
                Resolution::Disabled(command) => {
                    self.status = self
                        .registry
                        .get(command)
                        .map(|spec| format!("{} is unavailable in this context", spec.label))
                        .unwrap_or_else(|| "Command unavailable".to_owned());
                    return;
                }
                Resolution::None => {}
            }
        }
        match self.focus.primary() {
            PrimaryFocus::Editor => self.handle_editor_key(key),
            PrimaryFocus::Sidebar => self.handle_sidebar_key(key),
            PrimaryFocus::BottomPanel => self.handle_bottom_key(key),
        }
    }

    fn handle_editor_key(&mut self, key: KeyEvent) {
        let select = key.modifiers.contains(KeyModifiers::SHIFT);
        let result = match key.code {
            KeyCode::Left => self.workbench.move_left(select),
            KeyCode::Right => self.workbench.move_right(select),
            KeyCode::Up => self.workbench.move_vertical(-1, select),
            KeyCode::Down => self.workbench.move_vertical(1, select),
            KeyCode::PageUp => self.workbench.move_vertical(-10, select),
            KeyCode::PageDown => self.workbench.move_vertical(10, select),
            KeyCode::Home => self.workbench.move_home(select),
            KeyCode::End => self.workbench.move_end(select),
            KeyCode::Backspace if key.modifiers.is_empty() => self.workbench.backspace(),
            KeyCode::Delete if key.modifiers.is_empty() => self.workbench.delete_forward(),
            KeyCode::Enter if key.modifiers.is_empty() => self.workbench.insert_newline(),
            KeyCode::Tab if key.modifiers.is_empty() => self.workbench.insert_tab(),
            KeyCode::Char(ch) if is_plain_text(key) => self.workbench.insert_text(&ch.to_string()),
            _ => return,
        };
        self.apply_result(result);
    }

    fn handle_sidebar_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up => {
                self.workbench.previous_tab();
            }
            KeyCode::Down => {
                self.workbench.next_tab();
            }
            KeyCode::Enter => {
                let _ = self.focus.focus(PrimaryFocus::Editor, &self.panels);
            }
            _ => {}
        }
    }

    fn handle_bottom_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Enter {
            let _ = self.focus.focus(PrimaryFocus::Editor, &self.panels);
        }
    }

    fn handle_paste(&mut self, text: &str) {
        if matches!(self.overlay, UiOverlay::None) && self.focus.primary() == PrimaryFocus::Editor {
            let result = self.workbench.insert_text(text);
            self.apply_result(result);
        } else {
            self.append_overlay_text(text);
        }
    }

    pub(crate) fn apply_result(&mut self, result: Result<(), jc_app::WorkbenchError>) {
        match result {
            Ok(()) => self.status = "Ready".to_owned(),
            Err(error) => self.status = error.to_string(),
        }
    }
}
