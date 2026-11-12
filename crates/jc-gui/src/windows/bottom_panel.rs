//! Read-model host for existing F3 output. Opening it never admits a task.
use super::{ffi::Rect, Action, State};
use jc_app::{
    task_panels::{PanelKind, PanelSelection},
    CommandId, Key, KeyScope, KeyStroke, Modifiers, Resolution,
};

pub(super) struct BottomState {
    pub visible: bool,
    pub focused: bool,
    pub kind: PanelKind,
    problems: PanelSelection,
    output: PanelSelection,
}
impl Default for BottomState {
    fn default() -> Self {
        Self {
            visible: false,
            focused: false,
            kind: PanelKind::Problems,
            problems: PanelSelection::default(),
            output: PanelSelection::default(),
        }
    }
}
impl BottomState {
    fn selection(&mut self) -> &mut PanelSelection {
        match self.kind {
            PanelKind::Problems => &mut self.problems,
            PanelKind::Output => &mut self.output,
        }
    }
}
pub(super) struct BottomRow {
    pub id: u64,
    pub label: String,
    pub selected: bool,
}
pub(super) struct BottomFrame {
    pub rect: Rect,
    pub kind: PanelKind,
    pub title: String,
    pub message: String,
    pub rows: Vec<BottomRow>,
    pub focused: bool,
}
pub(super) struct BottomHits {
    pub kind: PanelKind,
    pub rect: Rect,
    pub rows: Vec<(u64, Rect)>,
}
impl BottomFrame {
    pub fn hits(&self, ch: i32) -> BottomHits {
        BottomHits {
            kind: self.kind,
            rect: self.rect,
            rows: self
                .rows
                .iter()
                .enumerate()
                .map(|(i, row)| {
                    let top = self.rect.top + (i as i32 + 2) * ch;
                    (
                        row.id,
                        Rect {
                            left: self.rect.left,
                            top,
                            right: self.rect.right,
                            bottom: (top + ch).min(self.rect.bottom),
                        },
                    )
                })
                .collect(),
        }
    }
}
pub(super) fn region(s: &State) -> Option<Rect> {
    if !s.bottom.visible {
        return None;
    }
    let ch = s.cell_height.clamp(1, 8192);
    let height = s.height.clamp(1, 8192);
    if height < ch * 9 + 40 {
        return None;
    }
    let bottom = (height - ch - 8).max(0);
    Some(Rect {
        left: super::explorer_panel::region(s).map_or(0, |r| r.right + 1),
        top: (bottom - (height / 3).max(ch * 5)).max(ch * 4 + 20),
        right: super::outline_panel::region(s).map_or(s.width.clamp(1, 8192), |r| r.left - 1),
        bottom,
    })
}
fn safe_label(text: &str, columns: usize) -> String {
    let mut result = String::new();
    let mut count = 0;
    for ch in text.chars() {
        if count >= columns {
            break;
        }
        if ch.is_control() {
            for escaped in ch.escape_default() {
                if count >= columns {
                    break;
                }
                result.push(escaped);
                count += 1;
            }
        } else {
            result.push(ch);
            count += 1;
        }
    }
    result
}
impl State {
    pub(super) fn request_progress_schema(&mut self) {
        if self.jobs.has_job(jc_progress::editor::JOB_KEY) {
            self.error("Ya hay una tarea Progress en curso");
            return;
        }
        match self
            .progress_schema
            .request(self.project_root.as_deref(), self.wakeup.clone())
        {
            Ok(()) => {
                self.show_bottom(CommandId::ViewOutput);
                self.status = "Leyendo esquema ABL…".into();
            }
            Err(error) => self.error(error),
        }
    }

    pub(super) fn show_progress_schema(&mut self) {
        if let Some(first) = self.progress_schema.show(&mut self.workbench) {
            self.show_bottom(CommandId::ViewOutput);
            self.select_schema_output(first);
            self.status = "Esquema ABL · consulta Salida".into();
        }
    }

    fn select_schema_output(&mut self, first: u64) {
        self.bottom.output.selected = Some(first);
        self.bottom.output.scroll = self
            .workbench
            .output()
            .partition_point(|row| row.id < first);
    }

    pub(super) fn poll_progress_schema(&mut self) -> bool {
        let Some(update) = self
            .progress_schema
            .poll(self.project_root.as_deref(), &mut self.workbench)
        else {
            return false;
        };
        self.status = update.message.into();
        if self.bottom.visible && self.bottom.kind == PanelKind::Output {
            if update.problems {
                self.bottom.kind = PanelKind::Problems;
                self.bottom
                    .problems
                    .last(self.workbench.task_panels(), PanelKind::Problems);
            } else if let Some(first) = update.first_output_row {
                self.select_schema_output(first);
            }
            self.bottom_hits = None;
        }
        true
    }

    pub(super) fn show_bottom(&mut self, command: CommandId) {
        self.close_selector();
        if command == CommandId::ViewToggleBottomPanel {
            self.bottom.visible = !self.bottom.visible;
        } else {
            self.bottom.visible = true;
            self.bottom.kind = if command == CommandId::ViewOutput {
                PanelKind::Output
            } else {
                PanelKind::Problems
            };
        }
        self.explorer_focused = false;
        self.bottom.focused = self.bottom.visible && region(self).is_some();
        self.bottom_hits = None;
    }
    pub(super) fn cycle_regions(&mut self, forward: bool) {
        let sidebar = super::explorer_panel::region(self).is_some();
        let bottom = region(self).is_some();
        let mut regions = vec![0];
        if sidebar {
            regions.push(1);
        }
        if super::outline_panel::region(self).is_some() {
            regions.push(3);
        }
        if bottom {
            regions.push(2);
        }
        let current = if self.bottom.focused {
            2
        } else if self.outline_focused() {
            3
        } else if self.explorer_focused {
            1
        } else {
            0
        };
        let i = regions.iter().position(|r| *r == current).unwrap_or(0);
        let next = if forward {
            (i + 1) % regions.len()
        } else {
            (i + regions.len() - 1) % regions.len()
        };
        self.explorer_focused = matches!(regions[next], 1 | 3);
        self.outline.focused = regions[next] == 3;
        self.bottom.focused = regions[next] == 2;
    }
    pub(super) fn bottom_presentation(&mut self) -> Option<BottomFrame> {
        let rect = region(self)?;
        let kind = self.bottom.kind;
        let capacity =
            ((rect.bottom - rect.top) / self.cell_height.clamp(1, 8192) - 3).clamp(0, 128) as usize;
        let columns =
            ((rect.right - rect.left - 12) / self.cell_width.clamp(1, 8192)).clamp(0, 512) as usize;
        let log = self.workbench.task_panels();
        let selection = self.bottom.selection();
        selection.normalize(log, kind, capacity);
        let selected = selection.selected;
        let scroll = selection.scroll;
        let rows = log.rows(kind);
        let total = rows.len();
        let presentation = rows
            .iter()
            .skip(scroll)
            .take(capacity)
            .map(|r| {
                let stale = r.target.as_ref().is_some_and(|t| {
                    !self.workbench.task_stamp_is_current(t.stamp)
                        || self
                            .workbench
                            .document_by_id(t.stamp.document)
                            .is_none_or(|d| d.session.path() != t.path)
                });
                let origin = r.target.as_ref().map_or_else(String::new, |t| {
                    format!(
                        "{:?} [D{} r{} {} {}..{}] ",
                        t.path.file_name().unwrap_or(t.path.as_os_str()),
                        t.stamp.document.0,
                        t.stamp.revision,
                        t.stamp.encoding.name(),
                        t.range.start,
                        t.range.end
                    )
                });
                BottomRow {
                    id: r.id,
                    label: safe_label(
                        &format!(
                            "{}{}{}",
                            if stale { "[obsoleto] " } else { "" },
                            origin,
                            r.text
                        ),
                        columns,
                    ),
                    selected: selected == Some(r.id),
                }
            })
            .collect();
        Some(BottomFrame {
            rect,
            kind,
            title: format!(
                "{} · {} retenidos · {} descartados · {} truncados",
                match kind {
                    PanelKind::Problems => "Problemas",
                    PanelKind::Output => "Salida",
                },
                total,
                log.dropped(kind),
                log.truncated(kind)
            ),
            message: format!(
                "Resultados F3 · {} pendientes (incluye Git) · {}",
                self.jobs.outstanding(),
                if total == 0 {
                    "Vacío; ningún proveedor de negocio se inicia automáticamente"
                } else {
                    "Intro navega problemas vigentes; la salida es texto inerte"
                }
            ),
            rows: presentation,
            focused: self.bottom.focused,
        })
    }
    pub(super) fn activate_bottom(&mut self) {
        if self.bottom.kind == PanelKind::Output {
            self.status = "La salida es de solo lectura; no se ejecutó nada".into();
            return;
        }
        let kind = self.bottom.kind;
        let log = self.workbench.task_panels();
        self.bottom.selection().normalize(log, kind, 1);
        let Some(id) = self.bottom.selection().selected else {
            self.status = "No hay problema seleccionado".into();
            return;
        };
        match self.workbench.activate_problem(id) {
            Ok(()) => {
                self.bottom.focused = false;
                self.explorer_focused = false;
                self.status = "Problema seleccionado; fuente compartido intacto".into();
            }
            Err(error) => self.error(error),
        }
    }
    pub(super) fn bottom_key(
        &mut self,
        code: usize,
        key: Option<Key>,
        modifiers: Modifiers,
    ) -> Action {
        if code == 0x1b {
            self.bottom.focused = false;
            return Action::None;
        }
        if let Some(key) = key {
            match self.keymap.resolve(
                KeyScope::BottomPanel,
                KeyStroke::new(modifiers, key),
                &self.registry,
                &self.context(),
            ) {
                Resolution::Enabled(command) if super::supported(command) => {
                    return self.execute(command)
                }
                Resolution::Disabled(command) if super::supported(command) => return Action::None,
                _ => {}
            }
        }
        if code == 0x09 {
            self.cycle_regions(!modifiers.contains(Modifiers::SHIFT));
            return Action::None;
        }
        if modifiers != Modifiers::NONE {
            return Action::None;
        }
        let kind = self.bottom.kind;
        let log = self.workbench.task_panels();
        let selection = self.bottom.selection();
        match code {
            0x26 => selection.move_by(log, kind, -1),
            0x28 => selection.move_by(log, kind, 1),
            0x21 => selection.move_by(log, kind, -10),
            0x22 => selection.move_by(log, kind, 10),
            0x24 => selection.first(log, kind),
            0x23 => selection.last(log, kind),
            0x0d => self.activate_bottom(),
            _ => {}
        }
        Action::None
    }
    pub(super) fn bottom_click(&mut self, x: i32, y: i32, double: bool) -> bool {
        let Some(rect) = region(self) else {
            return false;
        };
        if x < rect.left || x >= rect.right || y < rect.top || y >= rect.bottom {
            return false;
        }
        let Some(hits) = &self.bottom_hits else {
            return true;
        };
        if hits.kind != self.bottom.kind
            || hits.rect.left != rect.left
            || hits.rect.top != rect.top
            || hits.rect.right != rect.right
            || hits.rect.bottom != rect.bottom
        {
            return true;
        }
        self.bottom.focused = true;
        self.explorer_focused = false;
        let id = hits
            .rows
            .iter()
            .find(|(_, r)| y >= r.top && y < r.bottom)
            .map(|(id, _)| *id);
        if let Some(id) = id.filter(|id| {
            self.workbench
                .task_panels()
                .rows(self.bottom.kind)
                .iter()
                .any(|r| r.id == *id)
        }) {
            self.bottom.selection().selected = Some(id);
            self.bottom.focused = true;
            self.explorer_focused = false;
            if double {
                self.activate_bottom();
            }
        }
        true
    }
}

#[cfg(test)]
#[test]
#[ignore = "requiere un proyecto de prueba con un AVM local configurado"]
fn progress_real_gui_command_routes_to_output_and_problems() {
    use std::{
        path::PathBuf,
        thread,
        time::{Duration, Instant},
    };
    let root = PathBuf::from(std::env::var_os("CIDE_TEST_PROGRESS_PROJECT").unwrap());
    let mut state = State::new(&crate::GuiOptions {
        paths: vec![root.join("valido.p")],
        project_root: Some(root.clone()),
        ..Default::default()
    })
    .unwrap();
    let poll = |state: &mut State| {
        assert!(
            state.jobs.has_job(jc_progress::editor::JOB_KEY),
            "{}",
            state.status
        );
        let deadline = Instant::now() + Duration::from_secs(8);
        while state.jobs.has_job(jc_progress::editor::JOB_KEY) {
            state.poll_tasks();
            assert!(Instant::now() < deadline, "{}", state.status);
            thread::sleep(Duration::from_millis(15));
        }
    };
    state.execute_menu(CommandId::ProgressRun);
    poll(&mut state);
    assert!(state.bottom.visible);
    assert_eq!(state.bottom.kind, PanelKind::Output);
    assert!(state.workbench.output().iter().any(|r| r.text == "CIDE-M3"));
    state
        .workbench
        .open_file(root.join("invalido.p"), None)
        .unwrap();
    state.execute_menu(CommandId::ProgressCompile);
    poll(&mut state);
    assert_eq!(state.bottom.kind, PanelKind::Problems);
    assert!(state
        .workbench
        .problems()
        .iter()
        .any(|r| r.contains("ausente.i") && r.target.is_some()));
}

#[cfg(test)]
#[test]
#[ignore = "requiere el proyecto sintético de esquema de OpenEdge"]
fn progress_real_schema_gui_refreshes_and_reopens_the_catalog() {
    let root = std::env::var_os("CIDE_TEST_PROGRESS_SCHEMA_PROJECT").unwrap();
    exercise_progress_schema_gui(root.into(), true);
}

#[cfg(test)]
#[test]
#[ignore = "requiere CIDE_TEST_PROGRESS_ERP_PROJECT con un perfil de desarrollo autorizado"]
fn progress_real_erp_gui_refreshes_and_reopens_the_catalog() {
    let root = std::env::var_os("CIDE_TEST_PROGRESS_ERP_PROJECT").unwrap();
    exercise_progress_schema_gui(root.into(), false);
}

#[cfg(test)]
fn exercise_progress_schema_gui(root: std::path::PathBuf, synthetic: bool) {
    use std::{
        thread,
        time::{Duration, Instant},
    };
    let mut state = State::new(&crate::GuiOptions {
        project_root: Some(root),
        ..Default::default()
    })
    .unwrap();
    assert!(!state.context().has_database_schema);
    state.execute_menu(CommandId::ProgressRefreshSchema);
    assert!(state.progress_schema.busy(), "{}", state.status);
    let deadline = Instant::now() + Duration::from_secs(if synthetic { 8 } else { 35 });
    while state.progress_schema.busy() {
        state.poll_tasks();
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(15));
    }
    assert!(state.context().has_database_schema, "{}", state.status);
    assert_eq!(state.bottom.kind, PanelKind::Output);
    let output = state.workbench.output();
    assert!(output
        .iter()
        .any(|row| row.starts_with("Esquema ABL") && row.contains("OpenEdge AVM ")));
    assert!(output.iter().any(|row| row.starts_with("Tabla ")));
    if synthetic {
        assert!(output
            .iter()
            .any(|row| row.contains("Tabla devlogica.customer")));
    }
    let old_count = output.len();
    state.execute_menu(CommandId::ViewDatabase);
    assert!(state.workbench.output().len() > old_count);
    assert!(!state.progress_schema.busy());
    assert!(state.bottom.output.selected.is_some());
    assert!(state.bottom.output.scroll > 0);
    assert!(state.workbench.output()[state.bottom.output.scroll + 1].starts_with("Base "));
    if synthetic {
        assert!(state
            .workbench
            .output()
            .iter()
            .any(|row| row.starts_with("Base vacia")));
    }
    assert_eq!(state.status, "Esquema ABL · consulta Salida");
}
