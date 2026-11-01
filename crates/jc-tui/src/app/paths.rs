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
        let value = input.trim();
        if value.is_empty() {
            self.status = "Path cannot be empty".to_owned();
            return;
        }
        let path = PathBuf::from(value);
        let result: Result<(), String> = match kind {
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
                        self.project_root = self.explorer.root().map(PathBuf::from);
                        self.panels.show_sidebar(SidebarView::Explorer);
                    })
                } else {
                    Err("Folder does not exist".to_owned())
                }
            }
        };
        match result {
            Ok(()) => {
                self.status = "Ready".to_owned();
                self.close_overlay();
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
        if self.workbench.active_encoding().is_none() {
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
