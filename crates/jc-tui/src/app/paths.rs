use super::TuiApp;
use crate::state::{PromptKind, UiOverlay};
use jc_app::{Overlay, SidebarView};
use jc_text::TextEncoding;
use std::path::PathBuf;

impl TuiApp {
    pub(crate) fn open_prompt(&mut self, kind: PromptKind) {
        self.open_focus_overlay(Overlay::Dialog);
        self.overlay = UiOverlay::Prompt {
            kind,
            input: String::new(),
        };
    }

    pub(crate) fn submit_prompt(&mut self, kind: PromptKind, input: String) {
        if kind == PromptKind::GoToPosition {
            let parse = || {
                let mut fields = input.trim().split(':');
                let line = fields.next()?.parse::<usize>().ok()?.checked_sub(1)?;
                let column = fields
                    .next()
                    .unwrap_or("1")
                    .parse::<usize>()
                    .ok()?
                    .checked_sub(1)?;
                if fields.next().is_some() {
                    return None;
                }
                Some(jc_text::TextPosition { line, column })
            };
            if let Some(position) = parse() {
                match self.workbench.go_to_position(position) {
                    Ok(()) => {
                        self.close_overlay();
                        let _ = self.focus.focus(jc_app::PrimaryFocus::Editor, &self.panels);
                        self.status = "Position selected".into();
                    }
                    Err(error) => self.status = error.to_string(),
                }
            } else {
                self.status = "Use line[:column], one-based scalar coordinates".into();
            }
            return;
        }
        if matches!(
            kind,
            PromptKind::ProjectQuery
                | PromptKind::ProjectInclude
                | PromptKind::ProjectExclude
                | PromptKind::ProjectReplacement
        ) {
            self.submit_project_prompt(kind, input);
            return;
        }
        let value = input.trim();
        if value.is_empty() {
            self.status = "Path cannot be empty".to_owned();
            return;
        }
        let path = PathBuf::from(value);
        if matches!(kind, PromptKind::Open) {
            match self.workbench.open_file(path, self.default_encoding) {
                Ok(_) => {
                    self.close_overlay();
                    self.status = "Ready".to_owned();
                    self.ensure_encoding_choice();
                }
                Err(error) => self.status = error.to_string(),
            }
            return;
        }
        let result: Result<(), String> = match kind {
            PromptKind::GoToPosition => unreachable!(),
            PromptKind::Open => self.open_path(path),
            PromptKind::New => {
                let encoding = self.default_encoding.unwrap_or(TextEncoding::Utf8);
                self.workbench.new_file(path, encoding);
                Ok(())
            }
            PromptKind::SaveAs => self
                .workbench
                .save_active_as(path)
                .map_err(|e| e.to_string()),
            PromptKind::OpenFolder => {
                if path.is_dir() {
                    self.explorer.set_root(path).map(|()| {
                        self.project_search.clear();
                        self.cancel_project_replace();
                        self.project_results = false;
                        self.project_root = self.explorer.root().map(PathBuf::from);
                        self.panels.show_sidebar(SidebarView::Explorer);
                    })
                } else {
                    Err("Folder does not exist".to_owned())
                }
            }
            PromptKind::ProjectQuery
            | PromptKind::ProjectInclude
            | PromptKind::ProjectExclude
            | PromptKind::ProjectReplacement => {
                unreachable!()
            }
        };
        match result {
            Ok(()) => {
                self.close_overlay();
                self.status = "Ready".to_owned();
            }
            Err(error) => self.status = error,
        }
    }

    pub(crate) fn open_path(&mut self, path: PathBuf) -> Result<(), String> {
        self.workbench
            .open_file(path, self.default_encoding)
            .map_err(|error| error.to_string())?;
        self.ensure_encoding_choice();
        Ok(())
    }

    pub(crate) fn ensure_encoding_choice(&mut self) {
        if self.workbench.active_document().is_some() && self.workbench.active_encoding().is_none()
        {
            self.open_focus_overlay(Overlay::Dialog);
            self.overlay = UiOverlay::EncodingChoice;
            self.status = "Legacy encoding must be selected explicitly".to_owned();
        }
    }

    pub(crate) fn choose_encoding(&mut self, encoding: TextEncoding) {
        match self.workbench.set_encoding(encoding) {
            Ok(()) => {
                self.status = format!("Encoding: {}", encoding.name());
                self.close_overlay();
            }
            Err(error) => self.status = error.to_string(),
        }
    }
}
