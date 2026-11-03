//! Project metadata lookup; only explicit acceptance opens source bytes.
use super::TuiApp;
use crate::{input::is_plain_text, state::UiOverlay};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use jc_app::{Overlay, PrimaryFocus};

const QUERY_BYTES: usize = 256;
const OPEN_BYTES: usize = 16 * 1024 * 1024;

fn valid_query(text: &str) -> bool {
    text.len() <= QUERY_BYTES && !text.chars().any(|ch| ch.is_control()
        || matches!(ch, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{200e}' | '\u{200f}'))
}

impl TuiApp {
    pub(crate) fn open_quick_open(&mut self) {
        let Some(root) = self.project_root.clone() else {
            self.status = "Open a project folder before Quick Open".into();
            return;
        };
        self.open_focus_overlay(Overlay::CommandPalette);
        self.quick_open_root = Some(root.clone());
        self.overlay = UiOverlay::QuickOpen {
            input: String::new(),
            selected: 0,
        };
        self.status = match self.quick_open.request(root, String::new()) {
            Ok(()) => "Quick Open: scanning project paths; no file contents read".into(),
            Err(error) => error.to_string(),
        };
    }

    pub(crate) fn validate_quick_open_context(&mut self) -> bool {
        if matches!(self.overlay, UiOverlay::QuickOpen { .. })
            && self.quick_open_root.is_some()
            && self.quick_open_root != self.project_root
        {
            self.quick_open.clear();
            self.quick_open_root = None;
            self.status = "Project changed; reopen Quick Open".into();
            return true;
        }
        false
    }

    pub(crate) fn poll_quick_open(&mut self) -> bool {
        let mut changed = self.validate_quick_open_context();
        if let UiOverlay::QuickOpen { input, selected } = &mut self.overlay {
            if let Some(root) = self.quick_open_root.as_deref() {
                changed |= self.quick_open.poll(root, input);
                if changed {
                    *selected = (*selected).min(self.quick_open.results().len().saturating_sub(1));
                    self.status = format!(
                        "Quick Open: {} ({} paths)",
                        self.quick_open.message(),
                        self.quick_open.results().len()
                    );
                }
            }
        }
        changed
    }

    fn request_quick_query(&mut self, input: String) {
        if !valid_query(&input) {
            self.status =
                "Quick Open query: max 256 UTF-8 bytes, no controls/bidi formatting".into();
            return;
        }
        self.validate_quick_open_context();
        let Some(root) = self.quick_open_root.clone() else {
            return;
        };
        self.status = match self.quick_open.request(root, input.clone()) {
            Ok(()) => "Quick Open: scanning project paths".into(),
            Err(error) => error.to_string(),
        };
        self.overlay = UiOverlay::QuickOpen { input, selected: 0 };
    }

    pub(crate) fn append_quick_open_text(&mut self, mut input: String, text: &str) {
        if input.len().saturating_add(text.len()) > QUERY_BYTES {
            self.status = "Quick Open query exceeds 256 UTF-8 bytes".into();
            return;
        }
        if text.is_empty() {
            return;
        }
        input.push_str(text);
        self.request_quick_query(input);
    }

    pub(crate) fn handle_quick_open_key(
        &mut self,
        key: KeyEvent,
        mut input: String,
        mut selected: usize,
    ) {
        match key.code {
            KeyCode::Esc => {
                self.close_overlay();
                return;
            }
            KeyCode::Enter
                if key.modifiers.is_empty() || key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.accept_quick_open(
                    &input,
                    selected,
                    key.modifiers.contains(KeyModifiers::CONTROL),
                );
                return;
            }
            KeyCode::Backspace if key.modifiers.is_empty() => {
                if input.pop().is_some() {
                    self.request_quick_query(input);
                }
                return;
            }
            KeyCode::Char(ch) if is_plain_text(key) => {
                self.append_quick_open_text(input, &ch.to_string());
                return;
            }
            KeyCode::F(5) if key.modifiers.is_empty() => {
                self.request_quick_query(input);
                return;
            }
            KeyCode::Up if key.modifiers.is_empty() => selected = selected.saturating_sub(1),
            KeyCode::Down if key.modifiers.is_empty() => selected = selected.saturating_add(1),
            KeyCode::PageUp if key.modifiers.is_empty() => selected = selected.saturating_sub(10),
            KeyCode::PageDown if key.modifiers.is_empty() => selected = selected.saturating_add(10),
            KeyCode::Home if key.modifiers.is_empty() => selected = 0,
            KeyCode::End if key.modifiers.is_empty() => selected = usize::MAX,
            _ => return,
        }
        selected = selected.min(self.quick_open.results().len().saturating_sub(1));
        self.overlay = UiOverlay::QuickOpen { input, selected };
    }

    fn accept_quick_open(&mut self, input: &str, selected: usize, to_side: bool) {
        self.validate_quick_open_context();
        let Some(root) = self.quick_open_root.clone() else {
            return;
        };
        let target = match self.quick_open.selected_target(&root, input, selected) {
            Ok(target) => target,
            Err(error) => {
                self.status = error.to_string();
                return;
            }
        };
        match self.workbench.open_project_file(
            &root,
            target,
            self.default_encoding,
            to_side,
            OPEN_BYTES,
        ) {
            Ok(_) => {
                self.close_overlay();
                let _ = self.focus.focus(PrimaryFocus::Editor, &self.panels);
                self.ensure_encoding_choice();
                self.status = "Quick Open: file opened (16 MiB admission limit)".into();
            }
            Err(error) => self.status = error.to_string(),
        }
    }
}
