//! Explicit handoff, no polling or clipboard reads in command availability.
use super::TuiApp;
use crate::state::{ReplaceField, UiOverlay};
use jc_app::{CommandId, PrimaryFocus};
const LIMIT: usize = 2 * 1024 * 1024;
pub(super) trait ClipboardTransport {
    fn read(&mut self, limit: usize) -> Result<Option<String>, String>;
    fn write(&mut self, text: &str) -> Result<(), String>;
}
#[cfg(windows)]
impl ClipboardTransport for jc_platform::WindowsClipboard {
    fn read(&mut self, limit: usize) -> Result<Option<String>, String> {
        self.read(limit)
    }
    fn write(&mut self, text: &str) -> Result<(), String> {
        self.write(text)
    }
}
impl TuiApp {
    #[cfg(windows)]
    pub(crate) fn connect_windows_clipboard(&mut self) {
        self.clipboard_transport = Some(Box::<jc_platform::WindowsClipboard>::default());
    }
    pub(super) fn clipboard_field_budget(&self) -> Option<usize> {
        match &self.overlay {
            UiOverlay::DataConnections => self.data_connection_paste_budget(),
            UiOverlay::BrowseRecords => self.browse_filter_paste_budget(),
            UiOverlay::Find { input } => Some(4096usize.saturating_sub(input.len()).max(1)),
            UiOverlay::Replace {
                find,
                replacement,
                field,
            } => {
                let value = match field {
                    ReplaceField::Find => find,
                    ReplaceField::Replacement => replacement,
                };
                Some(4096usize.saturating_sub(value.len()).max(1))
            }
            UiOverlay::Prompt { input, .. } if self.file_browser.active() => {
                Some(self.browser_field_budget(input))
            }
            UiOverlay::Prompt { kind, input } => Some(
                super::overlays::prompt_limit(*kind)
                    .saturating_sub(input.len())
                    .max(1),
            ),
            UiOverlay::Palette { input, .. } | UiOverlay::QuickOpen { input, .. } => {
                Some(256usize.saturating_sub(input.len()).max(1))
            }
            UiOverlay::None if self.git_commit.view().is_some() => self.git_commit.paste_budget(),
            UiOverlay::None => self
                .commit_search_paste_budget()
                .or_else(|| self.snippet_clipboard_budget()),
            _ => None,
        }
    }
    pub(super) fn clipboard_command(&mut self, command: CommandId) -> bool {
        if self.clipboard_transport.is_none()
            || !matches!(
                command,
                CommandId::EditCopy | CommandId::EditCut | CommandId::EditPaste
            )
        {
            return false;
        }
        if command == CommandId::EditPaste {
            if let Some(limit) = self.clipboard_field_budget() {
                let result = self.clipboard_transport.as_mut().unwrap().read(limit);
                match result {
                    Ok(Some(text)) if text.len() <= limit => {
                        // Same precedence and atomic field quotas as Event::Paste.
                        self.handle_event(crossterm::event::Event::Paste(text));
                    }
                    Ok(Some(_)) => {
                        self.status = "El portapapeles supera el límite de bytes del campo".into()
                    }
                    Ok(None) => self.status = "El portapapeles no contiene texto Unicode".into(),
                    Err(error) => self.status = error,
                }
                return true;
            }
        }
        // Rectangular metadata stays internal, as on the GUI; it cannot be
        // reconstructed by guessing from CF_UNICODETEXT or a newline count.
        if self.workbench.context().rectangular_selection
            || command == CommandId::EditPaste && self.workbench.clipboard_is_rectangular()
        {
            return false;
        }
        if self.focus.primary() != PrimaryFocus::Editor {
            return false;
        }
        let result = (|| {
            match command {
                CommandId::EditCopy | CommandId::EditCut => {
                    let Some(export) = self
                        .workbench
                        .export_selection(LIMIT)
                        .map_err(|e| e.to_string())?
                    else {
                        return Ok(());
                    };
                    self.clipboard_transport
                        .as_mut()
                        .unwrap()
                        .write(&export.text)?;
                    if command == CommandId::EditCut {
                        self.workbench.delete_exported_selection(export)
                    } else {
                        self.workbench.remember_exported_copy(export)
                    }
                    .map_err(|e| e.to_string())?;
                }
                CommandId::EditPaste => {
                    let target = self
                        .workbench
                        .capture_clipboard_target()
                        .map_err(|e| e.to_string())?;
                    if let Some(text) = self.clipboard_transport.as_mut().unwrap().read(LIMIT)? {
                        self.workbench
                            .paste_external_text(target, &text, LIMIT)
                            .map_err(|e| e.to_string())?;
                    }
                }
                _ => unreachable!(),
            }
            Ok::<_, String>(())
        })();
        self.status = result
            .map(|()| {
                "Portapapeles Unicode de Windows · rectángulos en portapapeles interno".into()
            })
            .unwrap_or_else(|e| e);
        true
    }
}
