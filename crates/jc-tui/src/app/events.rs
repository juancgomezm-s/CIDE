use super::explorer::ExplorerAction;
use super::TuiApp;
use crate::input::{is_action_event, is_plain_text, to_stroke};
use crate::state::UiOverlay;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use jc_app::{BottomPanel, KeyScope, PrimaryFocus, Resolution, SidebarView};

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
        if self.panels.sidebar() != SidebarView::Explorer {
            if key.code == KeyCode::Enter {
                let _ = self.focus.focus(PrimaryFocus::Editor, &self.panels);
            }
            return;
        }

        match key.code {
            KeyCode::Up => self.explorer.move_selection(-1),
            KeyCode::Down => self.explorer.move_selection(1),
            KeyCode::Home => self.explorer.select_first(),
            KeyCode::End => self.explorer.select_last(),
            KeyCode::Left | KeyCode::Backspace => match self.explorer.parent() {
                Ok(true) => self.status = "Explorer: parent folder".to_owned(),
                Ok(false) => {}
                Err(error) => self.status = error,
            },
            KeyCode::Right => match self.explorer.enter_directory() {
                Ok(ExplorerAction::DirectoryChanged) => {
                    self.status = "Explorer: folder opened".to_owned()
                }
                Ok(_) => {}
                Err(error) => self.status = error,
            },
            KeyCode::Enter => self.activate_explorer_entry(),
            KeyCode::Char('r') if key.modifiers.is_empty() => match self.explorer.refresh() {
                Ok(()) => self.status = "Explorer refreshed".to_owned(),
                Err(error) => self.status = error,
            },
            _ => {}
        }
    }

    fn activate_explorer_entry(&mut self) {
        match self.explorer.activate() {
            Ok(ExplorerAction::OpenFile(path)) => match self.open_path(path) {
                Ok(()) => {
                    if matches!(self.overlay, UiOverlay::None) {
                        let _ = self.focus.focus(PrimaryFocus::Editor, &self.panels);
                    }
                    self.status = "File opened".to_owned();
                }
                Err(error) => self.status = error,
            },
            Ok(ExplorerAction::DirectoryChanged) => {
                self.status = "Explorer: folder opened".to_owned()
            }
            Ok(ExplorerAction::None) => {}
            Err(error) => self.status = error,
        }
    }

    fn handle_bottom_key(&mut self, key: KeyEvent) {
        let count = match self.panels.bottom() {
            BottomPanel::Problems => self.workbench.problems().len(),
            BottomPanel::Output => self.workbench.output().len(),
            BottomPanel::SearchResults => self.workbench.search().matches.len(),
            BottomPanel::Terminal => 1,
        };
        match key.code {
            KeyCode::Up => self.bottom_selected = self.bottom_selected.saturating_sub(1),
            KeyCode::Down if count > 0 => {
                self.bottom_selected = (self.bottom_selected + 1).min(count - 1)
            }
            KeyCode::Home => self.bottom_selected = 0,
            KeyCode::End if count > 0 => self.bottom_selected = count - 1,
            KeyCode::Enter if self.panels.bottom() == BottomPanel::SearchResults => {
                match self.workbench.select_search_match(self.bottom_selected) {
                    Ok(true) => {
                        let _ = self.focus.focus(PrimaryFocus::Editor, &self.panels);
                        self.status = format!("Search result {}", self.bottom_selected + 1);
                    }
                    Ok(false) => self.status = "No search result selected".to_owned(),
                    Err(error) => self.status = error.to_string(),
                }
            }
            KeyCode::Enter => {
                let _ = self.focus.focus(PrimaryFocus::Editor, &self.panels);
            }
            _ => {}
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
