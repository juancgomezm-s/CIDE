#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogKind {
    UnsavedChanges,
    ConfirmExit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogAction {
    Save,
    Discard,
    Cancel,
    Confirm,
}

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
            title: "Cambios sin guardar".into(),
            message: format!("{document} tiene cambios sin guardar."),
            buttons: vec![
                DialogButton {
                    mnemonic: 'g',
                    label: "Guardar",
                    action: DialogAction::Save,
                },
                DialogButton {
                    mnemonic: 'n',
                    label: "No guardar",
                    action: DialogAction::Discard,
                },
                DialogButton {
                    mnemonic: '\u{1b}',
                    label: "Cancelar",
                    action: DialogAction::Cancel,
                },
            ],
        }
    }

    pub fn resolve_mnemonic(&self, key: char) -> Option<DialogAction> {
        let key = match key.to_ascii_lowercase() {
            // Hidden compatibility aliases; visible choices are Spanish G/N.
            's' => 'g',
            'd' => 'n',
            key => key,
        };
        self.buttons
            .iter()
            .find(|button| button.mnemonic.to_ascii_lowercase() == key)
            .map(|button| button.action)
    }

    pub fn has_unique_mnemonics(&self) -> bool {
        for (index, button) in self.buttons.iter().enumerate() {
            if self.buttons[index + 1..]
                .iter()
                .any(|other| other.mnemonic.eq_ignore_ascii_case(&button.mnemonic))
            {
                return false;
            }
        }
        true
    }
}
