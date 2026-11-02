use jc_app::{MenuId, TabId};
use jc_text::TextEncoding;
use std::path::PathBuf;

#[derive(Debug, Clone, Default)]
pub struct LaunchOptions {
    pub files: Vec<PathBuf>,
    pub encoding: Option<TextEncoding>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    Open,
    New,
    SaveAs,
    OpenFolder,
    ProjectQuery,
    ProjectInclude,
    ProjectExclude,
    ProjectReplacement,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplaceField {
    Find,
    Replacement,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiOverlay {
    ProjectReplacePreview {
        selected: usize,
    },
    None,
    Prompt {
        kind: PromptKind,
        input: String,
    },
    Find {
        input: String,
    },
    Replace {
        find: String,
        replacement: String,
        field: ReplaceField,
    },
    Palette {
        input: String,
        selected: usize,
    },
    Menu {
        menu: MenuId,
        selected: usize,
    },
    CloseDialog {
        tab: TabId,
    },
    EncodingChoice,
}

impl Default for UiOverlay {
    fn default() -> Self {
        Self::None
    }
}
