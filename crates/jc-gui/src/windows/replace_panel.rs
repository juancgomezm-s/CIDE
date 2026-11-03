//! Explicit project replacement: draft -> immutable preview -> one file/event.
//! Plans stay on the application owner. Workers and drawing never apply edits.
use super::{
    project_panel::ProjectPresentation,
    project_results::{ProjectResultRow, ProjectResultsPresentation},
    replace_view::ReplaceView,
    Action, State,
};
use jc_app::{edit_field, present_field, FieldEdit, Modifiers, ProjectReplacePreview};
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ReplaceStage {
    Draft,
    Preview,
    Applying,
    Finished,
}

pub(super) struct ReplacePanel {
    pub stage: ReplaceStage,
    pub draft: String,
    pub cursor: usize,
    pub generation: u64,
    pub preview: Option<ProjectReplacePreview>,
    pub view: ReplaceView,
    pub changed: usize,
    pub conflicts: usize,
    pub unchanged: usize,
    pub total: usize,
    pub processed: usize,
    outcomes: VecDeque<String>,
}
pub(super) struct ReplaceFrame {
    pub panel: ProjectPresentation,
    pub labels: super::project_paint::PanelLabels,
}
impl ReplacePanel {
    fn new(generation: u64) -> Self {
        Self {
            stage: ReplaceStage::Draft,
            draft: String::new(),
            cursor: 0,
            generation,
            preview: None,
            view: ReplaceView::default(),
            changed: 0,
            conflicts: 0,
            unchanged: 0,
            total: 0,
            processed: 0,
            outcomes: VecDeque::new(),
        }
    }
    fn summary(&self, action: &str) -> String {
        format!(
            "{action}: {} cambiados; {} conflictos; {} sin cambios; {} pendientes",
            self.changed,
            self.conflicts,
            self.unchanged,
            self.total.saturating_sub(self.processed)
        )
    }
}
impl State {
    fn bump_replace_generation(&mut self) -> u64 {
        self.replace_generation = self.replace_generation.wrapping_add(1);
        self.replace_generation
    }
    pub(super) fn open_project_replace(&mut self) {
        self.validate_project_search();
        if !self.project_search.can_replace() {
            self.error(
                "Primero completa una búsqueda del proyecto sin avisos ni resultados obsoletos",
            );
            return;
        }
        self.close_selector();
        let generation = self.bump_replace_generation();
        self.replace_panel = Some(ReplacePanel::new(generation));
        self.status =
            "Borrador de reemplazo: Intro previsualiza; vacío elimina coincidencias".into();
    }
    pub(super) fn cancel_project_replace(&mut self) {
        self.bump_replace_generation();
        if let Some(panel) = self.replace_panel.take() {
            let summary = panel.summary(if panel.stage == ReplaceStage::Applying {
                "Reemplazo detenido"
            } else {
                "Reemplazo cerrado"
            });
            if panel.stage == ReplaceStage::Applying {
                self.workbench.push_output(summary.clone());
            }
            self.status = summary;
        }
    }
    /// Context checks may discard a preview, but never consume/apply its plans.
    pub(super) fn validate_replace_preview(&mut self) -> bool {
        if self
            .replace_panel
            .as_ref()
            .is_some_and(|p| p.stage == ReplaceStage::Preview)
            && !self.project_search.can_replace()
        {
            let generation = self.bump_replace_generation();
            let panel = self.replace_panel.as_mut().unwrap();
            panel.preview = None;
            panel.stage = ReplaceStage::Draft;
            panel.generation = generation;
            self.status =
                "Cambió el fuente/consulta: vista previa descartada; vuelve a buscar".into();
            return true;
        }
        false
    }
    pub(super) fn edit_replace_field(&mut self, edit: FieldEdit<'_>) {
        let generation = self.bump_replace_generation();
        let Some(panel) = self
            .replace_panel
            .as_mut()
            .filter(|p| p.stage == ReplaceStage::Draft)
        else {
            return;
        };
        panel.generation = generation;
        match edit_field(&mut panel.draft, &mut panel.cursor, edit, 4096) {
            Ok(_) => {
                self.status = "Intro genera vista previa; F9 confirma después de revisarla".into()
            }
            Err(error) => self.error(error),
        }
    }
    fn build_replace_preview(&mut self) {
        self.validate_project_search();
        let Some(panel) = self
            .replace_panel
            .as_ref()
            .filter(|p| p.stage == ReplaceStage::Draft)
        else {
            return;
        };
        match self
            .project_search
            .preview_replace(&self.workbench, panel.draft.clone())
        {
            Ok(preview) => {
                let panel = self.replace_panel.as_mut().unwrap();
                panel.total = preview.files_remaining();
                panel.preview = Some(preview);
                panel.stage = ReplaceStage::Preview;
                panel.view.first();
                self.status =
                    "Revisa líneas -/+; F9 o Ctrl+Intro confirma; Intro solo no aplica".into();
            }
            Err(error) => self.error(error),
        }
    }
    fn accept_project_replace(&mut self) {
        self.validate_project_search();
        let Some(panel) = self
            .replace_panel
            .as_mut()
            .filter(|p| p.stage == ReplaceStage::Preview)
        else {
            return;
        };
        if panel.preview.is_none() {
            return;
        }
        panel.stage = ReplaceStage::Applying;
        panel.changed = 0;
        panel.conflicts = 0;
        panel.unchanged = 0;
        panel.processed = 0;
        panel.outcomes.clear();
        self.status = panel.summary("Reemplazo en cola");
        self.bridge.signal();
    }
    /// Exactly one file per native task turn. The main loop prioritizes input
    /// and paint before the next posted turn, so Escape can drop remaining work.
    pub(super) fn apply_project_replace_step(&mut self) -> bool {
        let Some(panel) = self
            .replace_panel
            .as_mut()
            .filter(|p| p.stage == ReplaceStage::Applying)
        else {
            return false;
        };
        let Some(preview) = panel.preview.as_mut() else {
            return false;
        };
        if let Some(outcome) = self
            .project_search
            .apply_next_replace(preview, &mut self.workbench)
        {
            panel.processed += 1;
            let path = outcome
                .path
                .strip_prefix(
                    self.project_root
                        .as_deref()
                        .unwrap_or(std::path::Path::new("")),
                )
                .unwrap_or(&outcome.path);
            let path = path
                .as_os_str()
                .as_encoded_bytes()
                .escape_ascii()
                .to_string();
            let message = if let Some(error) = outcome.error {
                panel.conflicts += 1;
                format!("CONFLICTO {path}: {error}")
            } else if outcome.matches == 0 {
                panel.unchanged += 1;
                format!("Sin cambios {path}")
            } else {
                panel.changed += 1;
                format!(
                    "{} {path}: {} ediciones",
                    if outcome.saved_to_disk {
                        "Guardado"
                    } else {
                        "Documento abierto editado (sin guardar)"
                    },
                    outcome.matches
                )
            };
            self.workbench.push_output(message.clone());
            if panel.outcomes.len() == 32 {
                panel.outcomes.pop_front();
            }
            panel
                .outcomes
                .push_back(message.chars().take(512).collect());
        }
        if preview.files_remaining() == 0 {
            panel.preview = None;
            panel.stage = ReplaceStage::Finished;
            self.status = panel.summary("Reemplazo terminado");
            self.workbench.push_output(self.status.clone());
        } else {
            self.status = panel.summary("Reemplazando; Esc detiene los pendientes");
            self.bridge.signal();
        }
        true
    }
    pub(super) fn replace_key(&mut self, code: usize, modifiers: Modifiers) -> Action {
        if code == 0x1b {
            self.cancel_project_replace();
            return Action::None;
        }
        self.validate_project_search();
        let Some(panel) = self.replace_panel.as_ref() else {
            return Action::None;
        };
        match panel.stage {
            ReplaceStage::Applying | ReplaceStage::Finished => return Action::None,
            ReplaceStage::Preview => {
                if (code == 0x78 && modifiers == Modifiers::NONE)
                    || (code == 0x0d && modifiers == Modifiers::CTRL)
                {
                    self.accept_project_replace();
                    return Action::None;
                }
                let panel = self.replace_panel.as_mut().unwrap();
                if code == 0x71 && modifiers == Modifiers::NONE {
                    panel.preview = None;
                    panel.stage = ReplaceStage::Draft;
                    return Action::None;
                }
                if let Some(preview) = &panel.preview {
                    panel.view.prepare(preview, 0, 0);
                }
                match code {
                    0x26 => panel.view.move_selection(-1),
                    0x28 => panel.view.move_selection(1),
                    0x21 => panel.view.move_selection(-10),
                    0x22 => panel.view.move_selection(10),
                    0x24 => panel.view.first(),
                    0x23 => panel.view.last(),
                    _ => {}
                }
                return Action::None;
            }
            ReplaceStage::Draft => {}
        }
        if modifiers == Modifiers::CTRL {
            match code {
                0x56 => {
                    return Action::PasteReplacement(
                        self.replace_panel.as_ref().unwrap().generation,
                    )
                }
                0x55 => {
                    self.edit_replace_field(FieldEdit::Clear);
                    return Action::None;
                }
                _ => return Action::None,
            }
        }
        if code == 0x0d && modifiers == Modifiers::NONE {
            self.build_replace_preview();
            return Action::None;
        }
        let insert = match (code, modifiers) {
            (0x71, Modifiers::NONE) | (0x0d, Modifiers::SHIFT) => Some("\n"),
            (0x73, Modifiers::NONE) => Some("\r\n"),
            (0x76, Modifiers::NONE) => Some("\t"),
            (0x77, Modifiers::NONE) => Some("\r"),
            _ => None,
        };
        if let Some(text) = insert {
            self.edit_replace_field(FieldEdit::Insert(text));
            return Action::None;
        }
        if modifiers != Modifiers::NONE {
            return Action::None;
        }
        let edit = match code {
            0x25 => Some(FieldEdit::Left),
            0x27 => Some(FieldEdit::Right),
            0x26 => Some(FieldEdit::Up),
            0x28 => Some(FieldEdit::Down),
            0x24 => Some(FieldEdit::Home),
            0x23 => Some(FieldEdit::End),
            0x08 => Some(FieldEdit::Backspace),
            0x2e => Some(FieldEdit::Delete),
            _ => None,
        };
        if let Some(edit) = edit {
            self.edit_replace_field(edit);
        }
        Action::None
    }
    pub(super) fn replace_presentation(&mut self) -> Option<ReplaceFrame> {
        self.validate_project_search();
        let panel = self.replace_panel.as_mut()?;
        let draft = panel.stage == ReplaceStage::Draft;
        let (columns, rows) = super::project_paint::field_geometry(
            self.width,
            self.height,
            self.cell_width,
            self.cell_height,
            if draft { 3 } else { 1 },
        );
        let fields = if draft {
            vec![(
                "Reemplazo (vacío elimina coincidencias)",
                present_field(&panel.draft, panel.cursor, columns, rows),
            )]
        } else {
            Vec::new()
        };
        let capacity = super::project_paint::result_capacity(
            self.width,
            self.height,
            self.cell_width,
            self.cell_height,
        );
        let results = if let Some(preview) = &panel.preview {
            let presentation = panel.view.prepare(preview, capacity, columns);
            Some(ProjectResultsPresentation {
                title: presentation.title,
                message: if panel.stage == ReplaceStage::Applying {
                    panel.summary("Aplicando")
                } else {
                    presentation.message
                },
                selected: presentation.selected,
                total: presentation.total,
                rows: presentation
                    .rows
                    .into_iter()
                    .map(|row| ProjectResultRow {
                        index: row.index,
                        label: row.label,
                        selected: row.selected,
                        is_match: true,
                    })
                    .collect(),
            })
        } else if panel.stage == ReplaceStage::Finished {
            Some(ProjectResultsPresentation {
                title: "Reemplazo de proyecto: terminado".into(),
                message: panel.summary("Terminado"),
                selected: 0,
                total: panel.outcomes.len(),
                rows: panel
                    .outcomes
                    .iter()
                    .take(capacity)
                    .enumerate()
                    .map(|(index, line)| ProjectResultRow {
                        index,
                        label: line.clone(),
                        selected: false,
                        is_match: !line.starts_with("CONFLICTO"),
                    })
                    .collect(),
            })
        } else {
            None
        };
        let footer = match panel.stage {
            ReplaceStage::Draft => "Intro previsualiza | F2 LF F4 CRLF F7 TAB F8 CR | Ctrl+V pega | Esc cancela",
            ReplaceStage::Preview => "F9/Ctrl+Intro CONFIRMA | Intro no aplica | F2 edita | flechas revisan | Esc cancela",
            ReplaceStage::Applying => "Un archivo por turno | Esc detiene pendientes | se conservan los completados",
            ReplaceStage::Finished => "Terminado | abiertos: deshacer por documento, sin guardar | Esc cierra",
        };
        Some(ReplaceFrame { panel: ProjectPresentation { fields, active: 0, results, status: self.status.clone() }, labels: super::project_paint::PanelLabels {
            draft: "Reemplazar en proyecto: borrador", policy: "Abiertos: un deshacer, sin guardar | cerrados: guardado individual | sin reversión global", footer,
        } })
    }
}
