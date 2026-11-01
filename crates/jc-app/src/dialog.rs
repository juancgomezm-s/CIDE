#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogKind { UnsavedChanges, ConfirmExit }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogAction { Save, Discard, Cancel, Confirm }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialogButton {
    pub mnemonic: char,
    pub label: &'static str,
    pub action: DialogAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogSpec {
    pub kind: DialogKind,
    pub title: String,
    pub message: String,
    pub buttons: Vec<DialogButton>,
}

impl DialogSpec {
    pub fn unsaved_changes(document: &str) -> Self {
        Self {
            kind: DialogKind::UnsavedChanges,
            title: "Unsaved changes".into(),
            message: format!("{document} has unsaved changes."),
            buttons: vec![
                DialogButton { mnemonic: 's', label: "Save", action: DialogAction::Save },
                DialogButton { mnemonic: 'd', label: "Don't Save", action: DialogAction::Discard },
                DialogButton { mnemonic: '\u{1b}', label: "Cancel", action: DialogAction::Cancel },
            ],
        }
    }

    pub fn resolve_mnemonic(&self, key: char) -> Option<DialogAction> {
        let key = key.to_ascii_lowercase();
        self.buttons
            .iter()
            .find(|button| button.mnemonic.to_ascii_lowercase() == key)
            .map(|button| button.action)
    }

    pub fn has_unique_mnemonics(&self) -> bool {
        for (index, button) in self.buttons.iter().enumerate() {
            if self.buttons[index + 1..].iter().any(|other| {
                other.mnemonic.eq_ignore_ascii_case(&button.mnemonic)
            }) {
                return false;
            }
        }
        true
    }
}
