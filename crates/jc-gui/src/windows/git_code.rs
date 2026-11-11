//! Native code comparison over the shared, accepted Git snapshots.
use super::{ffi::*, git_panel::GitIntent, paint, shell, wide, Action, State};
use jc_app::{
    diff_model::DiffSide,
    git_read::{GitBlobSource, GitCommitId},
    Modifiers, TaskStamp,
};
use std::{path::PathBuf, rc::Rc};

#[derive(Default)]
pub(super) struct View {
    pub active: bool,
    unified: bool,
    first: usize,
    column: usize,
    selecting: bool,
    references: bool,
    selected: usize,
    wheel_remainder: i32,
    wheel_horizontal: bool,
    token: Rc<()>,
    pub hits: Option<Hits>,
    drag: Option<Drag>,
}
impl View {
    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }
    pub fn end_drag(&mut self) {
        self.drag = None;
    }
    pub fn invalidate(&mut self) {
        self.token = Rc::new(());
        self.hits = None;
    }
}

struct Drag {
    rail: Rail,
    grab: i32,
    stamp: TaskStamp,
    root: Option<PathBuf>,
    geometry: (i32, i32, i32, i32, i32),
    source: GitBlobSource,
}

#[derive(Clone)]
enum Intent {
    History,
    Toggle,
    Close,
    Commit(String),
    Next,
    References,
    Reference(usize),
}

#[derive(Clone)]
pub(super) struct Hits {
    token: Rc<()>,
    stamp: TaskStamp,
    root: Option<PathBuf>,
    geometry: (i32, i32, i32, i32, i32),
    targets: Vec<(Rect, Intent)>,
    rails: Vec<Rail>,
}

#[derive(Clone, Copy)]
struct Rail {
    area: Rect,
    maximum: usize,
    offset: usize,
    page: usize,
    vertical: bool,
}
impl Rail {
    fn thumb(self) -> Rect {
        let length = if self.vertical {
            self.area.bottom - self.area.top
        } else {
            self.area.right - self.area.left
        };
        let size = ((length.max(0) as u128 * self.page as u128)
            / self.maximum.saturating_add(self.page).max(1) as u128)
            .max(6)
            .min(length.max(0) as u128) as i32;
        let first = ((length - size).max(0) as u128 * self.offset.min(self.maximum) as u128
            / self.maximum.max(1) as u128) as i32;
        if self.vertical {
            rect(
                self.area.left,
                self.area.top + first,
                self.area.right,
                self.area.top + first + size,
            )
        } else {
            rect(
                self.area.left + first,
                self.area.top,
                self.area.left + first + size,
                self.area.bottom,
            )
        }
    }
    fn target(self, x: i32, y: i32, grab: i32) -> usize {
        let (position, length) = if self.vertical {
            (y - self.area.top, self.area.bottom - self.area.top)
        } else {
            (x - self.area.left, self.area.right - self.area.left)
        };
        let thumb = self.thumb();
        let size = if self.vertical {
            thumb.bottom - thumb.top
        } else {
            thumb.right - thumb.left
        };
        let travel = (length - size).max(0);
        ((position - grab).clamp(0, travel) as u128 * self.maximum as u128 / travel.max(1) as u128)
            as usize
    }
}

struct Cell {
    area: Rect,
    number: String,
    text: String,
    changed: bool,
    side: DiffSide,
}
pub(super) struct Frame {
    area: Rect,
    dpi: super::dpi::Dpi,
    divider: Option<i32>,
    unit: i32,
    row: i32,
    gutter: i32,
    left: String,
    right: String,
    unified: bool,
    message: String,
    cells: Vec<Cell>,
    buttons: Vec<(Rect, String)>,
    picker: Option<Rect>,
    choices: Vec<(Rect, String, bool)>,
    pub hits: Hits,
}

fn rect(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
    Rect {
        left,
        top,
        right,
        bottom,
    }
}
fn inside(r: Rect, x: i32, y: i32) -> bool {
    x >= r.left && x < r.right && y >= r.top && y < r.bottom
}

impl State {
    fn code_geometry(&self) -> (i32, i32, i32, i32, i32) {
        (
            self.width,
            self.height,
            self.dpi.pixels(96),
            self.cell_width,
            self.cell_height,
        )
    }
    pub(super) fn open_git_code(&mut self, source: GitBlobSource) -> Action {
        if self.project_root.is_none() {
            self.error("Abre la carpeta del repositorio para comparar en Código");
            return Action::None;
        }
        if self.git_commit.view().is_some()
            || self.git_index.state() == jc_app::git_index::IndexState::Applying
        {
            self.error("Termina o cancela la operación Git antes de abrir Código");
            return Action::None;
        }
        let _ = self.git_index.discard_review();
        let unified = self.git_code.unified;
        let action = self.request_git(GitIntent::Diff(source));
        self.git_code = View {
            active: self.git_panel.is_some(),
            unified,
            ..Default::default()
        };
        action
    }
    pub(super) fn open_git_code_from_workspace(&mut self) -> Action {
        let graph = self.git_workspace.graph.as_ref().filter(|v| v.showing);
        if graph.is_some_and(|v| v.reference_selected.is_some()) {
            self.error("Abre la rama o etiqueta y elige el commit que quieres comparar");
            return Action::None;
        }
        if (self.git_workspace.history_commit.is_some() || graph.is_some())
            && self.git_revision.state() != jc_app::git_revision::GitRevisionState::Ready
        {
            self.error("Espera a que termine la consulta de la revisión seleccionada");
            return Action::None;
        }
        let commit = self.git_workspace.history_commit.clone().or_else(|| {
            self.git_workspace
                .graph
                .as_ref()
                .filter(|v| v.showing)
                .and_then(|v| {
                    self.git_revision
                        .graph()?
                        .entries
                        .get(v.selected)
                        .map(|e| e.commit.id.clone())
                })
        });
        let path = if self.git_workspace.history_commit.is_some() {
            self.git_revision
                .revision()
                .and_then(|r| r.files.get(self.git_workspace.history_selected))
                .map(|f| f.path.clone())
        } else {
            self.git_workspace.selected_path.clone().or_else(|| {
                self.git_status
                    .result()?
                    .status()
                    .entries()
                    .get(self.git_view.selected_status())
                    .map(|e| e.path().to_vec())
            })
        };
        if self.git_commit.view().is_some()
            || self.git_index.state() == jc_app::git_index::IndexState::Applying
        {
            self.error("Termina o cancela la operación Git antes de abrir Código");
            return Action::None;
        }
        let source = match commit {
            Some(id) => match GitCommitId::from_hex(&id) {
                Ok(id) => GitBlobSource::Commit(id),
                Err(error) => {
                    self.error(error);
                    return Action::None;
                }
            },
            None => match self.git_workspace.local_kind {
                Some(jc_app::git_revision::PairKind::Prepared) => GitBlobSource::Head,
                Some(jc_app::git_revision::PairKind::Worktree) => GitBlobSource::Index,
                None => self.git_source,
            },
        };
        if let Some(path) = path {
            if let Err(error) = self.open_git_working_path(&path) {
                self.error(error);
                return Action::None;
            }
        }
        self.open_git_code(source)
    }
    fn choose_code_revision(&mut self) {
        if self.git_code.selecting {
            self.git_code.selecting = false;
            self.git_revision.cancel();
        } else if let Some(root) = self.project_root.clone() {
            match self.git_revision.request_graph(root) {
                Ok(()) => {
                    self.git_code.selecting = true;
                    self.git_code.selected = 0;
                    self.git_code.references = false;
                }
                Err(error) => self.error(error),
            }
        }
        self.git_code.invalidate();
    }
    fn code_action(&mut self, intent: Intent) -> Action {
        self.git_code.end_drag();
        match intent {
            Intent::History => self.choose_code_revision(),
            Intent::Toggle => {
                self.git_code.unified = !self.git_code.unified;
                self.git_code.first = 0;
            }
            Intent::Close => {
                self.git_diff.cancel(&mut self.jobs);
                self.git_revision.cancel();
                self.git_panel = None;
                self.git_code.active = false;
                self.git_code.selecting = false;
                self.status = "Edición del documento".into();
            }
            Intent::Commit(id) => {
                if self.git_revision.state() != jc_app::git_revision::GitRevisionState::Ready {
                    return Action::None;
                }
                match GitCommitId::from_hex(&id) {
                    Ok(id) => return self.open_git_code(GitBlobSource::Commit(id)),
                    Err(error) => self.error(error),
                }
            }
            Intent::References => {
                self.git_code.references = !self.git_code.references;
                self.git_code.selected = 0;
            }
            Intent::Reference(row) => {
                if let Some(root) = self.project_root.clone() {
                    match self.git_revision.request_graph_reference(root, row) {
                        Ok(()) => {
                            self.git_code.references = false;
                            self.git_code.selected = 0;
                        }
                        Err(error) => self.error(error),
                    }
                }
            }
            Intent::Next => {
                if self.git_code.references {
                    return Action::None;
                }
                if let Some(root) = self.project_root.clone() {
                    match self.git_revision.request_graph_next(root) {
                        Ok(()) => self.git_code.selected = 0,
                        Err(error) => self.error(error),
                    }
                }
            }
        }
        self.git_code.invalidate();
        Action::None
    }
    pub(super) fn git_code_key(&mut self, code: usize, modifiers: Modifiers) -> Action {
        if modifiers != Modifiers::NONE {
            return Action::None;
        }
        if code == 0x73 {
            return self.code_action(Intent::History);
        }
        if self.git_code.selecting {
            let total = self.git_revision.graph().map_or(0, |g| {
                if self.git_code.references {
                    g.reference_count()
                } else {
                    g.entries.len()
                }
            });
            match code {
                0x1b => return self.code_action(Intent::History),
                0x0d => {
                    if self.git_code.references {
                        return self.code_action(Intent::Reference(self.git_code.selected));
                    }
                    if let Some(id) = self
                        .git_revision
                        .graph()
                        .and_then(|g| g.entries.get(self.git_code.selected))
                        .map(|e| e.commit.id.clone())
                    {
                        return self.code_action(Intent::Commit(id));
                    }
                }
                0x26 => self.git_code.selected = self.git_code.selected.saturating_sub(1),
                0x28 => {
                    self.git_code.selected = self
                        .git_code
                        .selected
                        .saturating_add(1)
                        .min(total.saturating_sub(1))
                }
                0x21 => self.git_code.selected = self.git_code.selected.saturating_sub(10),
                0x22 => {
                    self.git_code.selected = self
                        .git_code
                        .selected
                        .saturating_add(10)
                        .min(total.saturating_sub(1))
                }
                0x24 => self.git_code.selected = 0,
                0x23 => self.git_code.selected = total.saturating_sub(1),
                0x4e => return self.code_action(Intent::Next),
                0x52 => return self.code_action(Intent::References),
                _ => {}
            }
        } else {
            match code {
                0x1b => return self.code_action(Intent::Close),
                0x56 => return self.code_action(Intent::Toggle),
                0x74 => return self.open_git_code(self.git_source),
                0x26 => self.git_code.first = self.git_code.first.saturating_sub(1),
                0x28 => self.git_code.first = self.git_code.first.saturating_add(1),
                0x21 => self.git_code.first = self.git_code.first.saturating_sub(10),
                0x22 => self.git_code.first = self.git_code.first.saturating_add(10),
                0x24 => self.git_code.first = 0,
                0x23 => self.git_code.first = usize::MAX,
                0x25 => self.git_code.column = self.git_code.column.saturating_sub(4),
                0x27 => self.git_code.column = self.git_code.column.saturating_add(4).min(16384),
                _ => {}
            }
        }
        self.git_code.invalidate();
        Action::None
    }
    pub(super) fn git_code_click(&mut self, x: i32, y: i32) -> Action {
        if !self.git_code.active {
            return Action::None;
        }
        let Some(hits) = self.git_code.hits.as_ref().filter(|h| {
            Rc::ptr_eq(&h.token, &self.git_code.token)
                && h.root == self.project_root
                && h.geometry == self.code_geometry()
                && self.workbench.task_stamp_is_current(h.stamp)
        }) else {
            return Action::None;
        };
        if !self.git_code.selecting {
            if let Some(rail) = hits
                .rails
                .iter()
                .find(|r| r.maximum != 0 && inside(r.area, x, y))
                .copied()
            {
                let thumb = rail.thumb();
                let grab = if inside(thumb, x, y) {
                    if rail.vertical {
                        y - thumb.top
                    } else {
                        x - thumb.left
                    }
                } else if rail.vertical {
                    (thumb.bottom - thumb.top) / 2
                } else {
                    (thumb.right - thumb.left) / 2
                };
                self.git_code.drag = Some(Drag {
                    rail,
                    grab,
                    stamp: hits.stamp,
                    root: hits.root.clone(),
                    geometry: hits.geometry,
                    source: self.git_source,
                });
                self.update_git_code_drag(x, y);
                return Action::CapturePointer;
            }
        }
        let intent = hits
            .targets
            .iter()
            .rev()
            .find(|(r, _)| inside(*r, x, y))
            .map(|(_, i)| i.clone());
        if self.git_code.selecting && matches!(intent, None | Some(Intent::Toggle | Intent::Close))
        {
            return self.code_action(Intent::History);
        }
        match intent {
            Some(intent) => self.code_action(intent),
            None => Action::None,
        }
    }
    pub(super) fn update_git_code_drag(&mut self, x: i32, y: i32) -> bool {
        let Some(drag) = self.git_code.drag.as_ref() else {
            return false;
        };
        if !self.git_code.active
            || drag.geometry != self.code_geometry()
            || drag.root != self.project_root
            || drag.source != self.git_source
            || !self.workbench.task_stamp_is_current(drag.stamp)
        {
            self.git_code.end_drag();
            return false;
        }
        let target = drag.rail.target(x, y, drag.grab);
        if drag.rail.vertical {
            self.git_code.first = target;
        } else {
            self.git_code.column = target;
        }
        self.git_code.invalidate();
        true
    }
    pub(super) fn git_code_wheel(&mut self, delta: i32, horizontal: bool, lines: u32) -> bool {
        self.git_code.end_drag();
        if lines == 0 || delta == 0 {
            return false;
        }
        if self.git_code.wheel_horizontal != horizontal {
            self.git_code.wheel_remainder = 0;
            self.git_code.wheel_horizontal = horizontal;
        }
        let total = self.git_code.wheel_remainder.saturating_add(delta);
        let steps = total / 120;
        self.git_code.wheel_remainder = total % 120;
        if steps == 0 {
            return false;
        }
        let rows = if horizontal {
            4
        } else if lines == u32::MAX {
            (self.height / self.cell_height.max(1)).clamp(1, 128) as usize
        } else {
            lines.min(128) as usize
        };
        let amount = (steps.unsigned_abs() as usize).saturating_mul(rows);
        if self.git_code.selecting {
            let total = self.git_revision.graph().map_or(0, |g| g.entries.len());
            self.git_code.selected = if steps > 0 {
                self.git_code.selected.saturating_sub(amount)
            } else {
                self.git_code
                    .selected
                    .saturating_add(amount)
                    .min(total.saturating_sub(1))
            };
        } else {
            let value = if horizontal {
                &mut self.git_code.column
            } else {
                &mut self.git_code.first
            };
            *value = if steps > 0 {
                value.saturating_sub(amount)
            } else {
                value.saturating_add(amount)
            };
        }
        self.git_code.invalidate();
        true
    }

    pub(super) fn prepare_git_code(&mut self) -> Option<Frame> {
        if !self.git_code.active {
            return None;
        }
        let doc = self.workbench.active_document()?;
        let encoding = doc.session.encoding()?;
        let stamp = TaskStamp {
            document: doc.id,
            revision: doc.session.document().revision(),
            encoding,
            view: self
                .workbench
                .panes()
                .view(self.workbench.panes().active())
                .and_then(|v| v.view_id()),
        };
        let p = |n| self.dpi.pixels(n);
        let area = rect(
            super::explorer_panel::region(self).map_or(0, |r| r.right + 1),
            p(shell::TABS),
            super::outline_panel::region(self).map_or(self.width, |r| r.left - 1),
            super::bottom_panel::region(self).map_or(self.height - self.cell_height - 5, |r| r.top),
        );
        if area.right - area.left < p(80) || area.bottom - area.top < p(100) {
            return None;
        }
        let unit = p(1).max(1);
        let unified = self.git_code.unified || area.right - area.left < p(420);
        let caption = match self.git_source {
            GitBlobSource::Head => "HEAD".into(),
            GitBlobSource::Index => "Índice".into(),
            GitBlobSource::Commit(id) => format!("Commit {}", &id.as_str()[..8]),
        };
        let mut frame = Frame {
            area,
            dpi: self.dpi,
            divider: None,
            unit,
            row: self.cell_height.max(p(12)),
            gutter: (self.cell_width * 7).max(p(40)) + p(8),
            left: caption,
            right: doc
                .session
                .path()
                .file_name()
                .map_or_else(|| "Documento".into(), |v| v.to_string_lossy().into_owned()),
            unified,
            message: format!(
                "{} · F4 revisión · V vista · Esc editar",
                self.git_diff.state().label()
            ),
            cells: Vec::new(),
            buttons: Vec::new(),
            picker: None,
            choices: Vec::new(),
            hits: Hits {
                token: self.git_code.token.clone(),
                stamp,
                root: self.project_root.clone(),
                geometry: self.code_geometry(),
                targets: Vec::new(),
                rails: Vec::new(),
            },
        };
        let slot = ((area.right - area.left - p(16)) / 3).min(p(132));
        for (i, label, intent) in [
            (0, "Revisión ▾", Intent::History),
            (
                1,
                if unified { "Unificada" } else { "Paralela" },
                Intent::Toggle,
            ),
            (2, "Volver a editar", Intent::Close),
        ] {
            let left = area.left + p(8) + i * (slot + p(4));
            let r = rect(
                left,
                area.top + p(4),
                (left + slot).min(area.right - p(4)),
                area.top + p(30),
            );
            frame.buttons.push((r, label.into()));
            frame.hits.targets.push((r, intent));
        }
        let body = rect(
            area.left,
            area.top + p(58),
            area.right - p(9),
            area.bottom - p(30),
        );
        frame.divider = (!unified).then_some(body.left + (body.right - body.left) / 2);
        let page_size = ((body.bottom - body.top).max(0) / frame.row) as usize;
        if let Some(result) = self.git_diff.result() {
            let page =
                jc_app::diff_rows::page(&result.model, self.git_code.first, page_size, !unified);
            self.git_code.first = page.first;
            let width = if unified {
                body.right - body.left
            } else {
                (body.right - body.left) / 2
            };
            let columns = ((width - frame.gutter - p(6)).max(0) / self.cell_width.max(1)) as usize;
            let mut extent = 0;
            for pass in 0..2 {
                frame.cells.clear();
                for (index, row) in page.rows.iter().enumerate() {
                    for (side, line) in [(DiffSide::Left, row.left), (DiffSide::Right, row.right)] {
                        if unified
                            && (line.is_none() || side == DiffSide::Left && row.right.is_some())
                        {
                            continue;
                        }
                        let from = if !unified && side == DiffSide::Right {
                            body.left + width + unit
                        } else {
                            body.left
                        };
                        let to = if !unified && side == DiffSide::Left {
                            body.left + width
                        } else {
                            body.right
                        };
                        let (text, length) = line
                            .and_then(|n| result.model.line_byte_range(side, n))
                            .map(|range| {
                                super::git_workspace::compare_rows::text(
                                    &result.model.source_bytes(side)[range],
                                    encoding,
                                    self.git_code.column,
                                )
                            })
                            .unwrap_or_default();
                        extent = extent.max(length);
                        frame.cells.push(Cell {
                            area: rect(
                                from,
                                body.top + index as i32 * frame.row,
                                to,
                                body.top + (index + 1) as i32 * frame.row,
                            ),
                            number: line.map_or_else(String::new, |n| (n.0 + 1).to_string()),
                            text,
                            changed: row.changed && line.is_some(),
                            side,
                        });
                    }
                }
                let maximum = extent.saturating_sub(columns);
                if self.git_code.column <= maximum || pass == 1 {
                    break;
                }
                self.git_code.column = maximum;
            }
            frame.hits.rails.push(Rail {
                area: rect(body.right, body.top, area.right, body.bottom),
                maximum: page.total.saturating_sub(page.rows.len()),
                offset: page.first,
                page: page.rows.len(),
                vertical: true,
            });
            frame.hits.rails.push(Rail {
                area: rect(body.left, body.bottom, body.right, body.bottom + p(8)),
                maximum: extent.saturating_sub(columns),
                offset: self.git_code.column,
                page: columns,
                vertical: false,
            });
        }
        if self.git_code.selecting {
            let top = area.top + p(34);
            let height = frame.row + p(4);
            let count = ((area.bottom - top - p(54)).max(0) / height).clamp(1, 12) as usize;
            let mut bottom = top + height;
            if let Some(graph) = self.git_revision.graph() {
                let references = self.git_code.references;
                let total = if references {
                    graph.reference_count()
                } else {
                    graph.entries.len()
                };
                self.git_code.selected = self.git_code.selected.min(total.saturating_sub(1));
                let first = self.git_code.selected.saturating_sub(count - 1);
                for index in first..total.min(first + count) {
                    let r = rect(
                        area.left + p(8),
                        bottom,
                        (area.left + p(680)).min(area.right - p(8)),
                        bottom + height,
                    );
                    let (title, intent) = if references {
                        (
                            graph.reference_label(index).unwrap_or_default().to_owned(),
                            Intent::Reference(index),
                        )
                    } else {
                        let entry = &graph.entries[index];
                        (
                            jc_app::source_services::display_ui_label(
                                &format!(
                                    "{}  {}",
                                    &entry.commit.id[..8],
                                    super::git_workspace::opaque(&entry.commit.summary, 160)
                                ),
                                256,
                            ),
                            Intent::Commit(entry.commit.id.clone()),
                        )
                    };
                    frame
                        .choices
                        .push((r, title, index == self.git_code.selected));
                    frame.hits.targets.push((r, intent));
                    bottom += height;
                }
                for (visible, title, intent) in [
                    (
                        !references && graph.has_next(),
                        "Siguiente página · N",
                        Intent::Next,
                    ),
                    (
                        true,
                        if references {
                            "Volver a commits · R"
                        } else {
                            "Ramas y etiquetas · R"
                        },
                        Intent::References,
                    ),
                ] {
                    if visible && bottom + height <= area.bottom - p(8) {
                        let r = rect(
                            area.left + p(8),
                            bottom,
                            (area.left + p(680)).min(area.right - p(8)),
                            bottom + height,
                        );
                        frame.choices.push((r, title.into(), false));
                        frame.hits.targets.push((r, intent));
                        bottom += height;
                    }
                }
            }
            frame.picker = Some(rect(
                area.left + p(8),
                top,
                (area.left + p(680)).min(area.right - p(8)),
                bottom + unit,
            ));
        }
        Some(frame)
    }
}

pub(super) unsafe fn draw(
    dc: Hdc,
    frame: &Frame,
    font: Handle,
    ui_font: Handle,
) -> Result<(), String> {
    let area = frame.area;
    let unit = frame.unit;
    let p = |n| frame.dpi.pixels(n);
    paint::fill(dc, area, shell::BACKGROUND)?;
    SelectObject(dc, ui_font);
    for (r, label) in &frame.buttons {
        paint::fill(dc, *r, shell::RAISED)?;
        paint::text(
            dc,
            rect(r.left + p(4), r.top + p(3), r.right - p(3), r.bottom),
            &wide(label),
            shell::TEXT,
            shell::RAISED,
        )?;
    }
    let left = area.left + p(8);
    let middle = frame.divider.unwrap_or(area.right);
    let heading = if frame.unified {
        format!("{} → {}", frame.left, frame.right)
    } else {
        frame.left.clone()
    };
    paint::text(
        dc,
        rect(
            left,
            area.top + p(34),
            if frame.unified { area.right } else { middle },
            area.top + p(56),
        ),
        &wide(&heading),
        shell::MUTED,
        shell::BACKGROUND,
    )?;
    if !frame.unified {
        paint::text(
            dc,
            rect(
                middle + p(8),
                area.top + p(34),
                area.right,
                area.top + p(56),
            ),
            &wide(&frame.right),
            shell::MUTED,
            shell::BACKGROUND,
        )?;
    }
    SelectObject(dc, font);
    for cell in &frame.cells {
        let background = if cell.changed {
            if cell.side == DiffSide::Left {
                0x00312b50
            } else {
                0x00314423
            }
        } else {
            shell::BACKGROUND
        };
        paint::fill(dc, cell.area, background)?;
        let number = rect(
            cell.area.left + p(4),
            cell.area.top,
            cell.area.left + frame.gutter,
            cell.area.bottom,
        );
        paint::text(dc, number, &wide(&cell.number), shell::MUTED, background)?;
        paint::text(
            dc,
            rect(
                number.right + p(6),
                cell.area.top,
                cell.area.right,
                cell.area.bottom,
            ),
            &wide(&cell.text),
            shell::TEXT,
            background,
        )?;
    }
    if !frame.unified {
        paint::fill(
            dc,
            rect(middle, area.top + p(34), middle + unit, area.bottom - p(30)),
            0x00594b42,
        )?;
    }
    for rail in &frame.hits.rails {
        paint::fill(dc, rail.area, shell::SIDEBAR)?;
        if rail.maximum != 0 {
            paint::fill(dc, rail.thumb(), shell::MUTED)?;
        }
    }
    SelectObject(dc, ui_font);
    paint::text(
        dc,
        rect(left, area.bottom - p(20), area.right, area.bottom),
        &wide(&frame.message),
        shell::MUTED,
        shell::BACKGROUND,
    )?;
    if let Some(picker) = frame.picker {
        paint::fill(dc, picker, 0x00594b42)?;
        paint::fill(
            dc,
            rect(
                picker.left + unit,
                picker.top + unit,
                picker.right - unit,
                picker.bottom - unit,
            ),
            shell::RAISED,
        )?;
        paint::text(
            dc,
            rect(
                picker.left + p(6),
                picker.top + unit,
                picker.right,
                picker.top + frame.row,
            ),
            &wide("Elegir commit · ↑↓ · Intro comparar · Esc volver"),
            shell::TEXT,
            shell::RAISED,
        )?;
        for (r, label, selected) in &frame.choices {
            let bg = if *selected {
                shell::SELECTED
            } else {
                shell::RAISED
            };
            paint::fill(dc, *r, bg)?;
            paint::text(
                dc,
                rect(r.left + p(6), r.top, r.right, r.bottom),
                &wide(label),
                shell::TEXT,
                bg,
            )?;
        }
    }
    Ok(())
}

pub(super) unsafe fn smoke(
    hwnd: Hwnd,
    bitmap: Option<&std::path::Path>,
    checks: &mut Vec<&'static str>,
) -> Result<(), String> {
    use super::{
        git_smoke, owner,
        project_smoke::snapshot,
        smoke::{command, key},
    };
    use jc_app::CommandId;
    use std::time::{Duration, Instant};
    let before = snapshot(hwnd)?;
    let bytes = owner(hwnd)
        .ok_or("Code smoke lost HWND")?
        .state
        .borrow()
        .workbench
        .active_bytes()
        .ok_or("Code smoke lost source bytes")?;
    command(hwnd, CommandId::GitDiffHead)?;
    git_smoke::diff_ready(hwnd)?;
    {
        let mut state = owner(hwnd).unwrap().state.borrow_mut();
        if !state.git_code.active || state.git_presentation().is_some() {
            return Err("The comparison command did not open inside Code".into());
        }
        let frame = state
            .prepare_git_code()
            .ok_or("Missing native Code frame")?;
        if !frame
            .cells
            .iter()
            .any(|c| c.changed && c.side == DiffSide::Left)
            || !frame
                .cells
                .iter()
                .any(|c| c.changed && c.side == DiffSide::Right)
        {
            return Err("Code comparison omitted deleted or added rows".into());
        }
    }
    if let Some(path) = bitmap {
        git_smoke::readonly_frame(
            hwnd,
            Some(&path.with_file_name("native-git-code-parallel.bmp")),
        )?;
    }
    SendMessageW(hwnd, WM_CHAR, b'X' as usize, 0);
    SendMessageW(hwnd, WM_UNICHAR, b'Y' as usize, 0);
    if owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .active_bytes()
        .ok_or("Code smoke lost source bytes")?
        != bytes
    {
        return Err("Code comparison accepted hidden edits".into());
    }
    // A hidden HWND has an empty display clip. Compose the same immutable frame
    // into the owned GDI bitmap, then admit its hits only in this test harness.
    // Production still publishes Code hits only after a successful display paint.
    owner(hwnd).unwrap().frame_dirty.set(true);
    InvalidateRect(hwnd, std::ptr::null(), 0);
    SendMessageW(hwnd, WM_PAINT, 0, 0);
    let hits = owner(hwnd)
        .unwrap()
        .frame
        .borrow()
        .as_ref()
        .and_then(|f| f.git_code_hits())
        .ok_or("Missing retained Code frame")?;
    owner(hwnd).unwrap().state.borrow_mut().git_code.hits = Some(hits);
    let button = {
        let state = owner(hwnd).unwrap().state.borrow();
        state
            .git_code
            .hits
            .as_ref()
            .and_then(|h| {
                h.targets
                    .iter()
                    .find_map(|(r, i)| matches!(i, Intent::History).then_some(*r))
            })
            .ok_or("Code history button was not published by paint")?
    };
    let point = (
        (button.left + button.right) / 2,
        (button.top + button.bottom) / 2,
    );
    let packed = (point.0 as u16 as usize | ((point.1 as u16 as usize) << 16)) as isize;
    SendMessageW(hwnd, WM_LBUTTONDOWN, 1, packed);
    SendMessageW(hwnd, WM_LBUTTONUP, 0, packed);
    let until = Instant::now() + Duration::from_secs(5);
    while owner(hwnd).unwrap().state.borrow().git_revision.state()
        != jc_app::git_revision::GitRevisionState::Ready
    {
        if Instant::now() >= until {
            return Err("Code revision picker did not settle".into());
        }
        git_smoke::one_task(hwnd)?;
    }
    if !owner(hwnd).unwrap().state.borrow().git_code.selecting {
        return Err("History click did not open the Code picker".into());
    }
    if let Some(path) = bitmap {
        git_smoke::readonly_frame(
            hwnd,
            Some(&path.with_file_name("native-git-code-history.bmp")),
        )?;
    }
    key(hwnd, 0x23, Modifiers::NONE)?;
    let commit = {
        let state = owner(hwnd).unwrap().state.borrow();
        state
            .git_revision
            .graph()
            .and_then(|g| g.entries.get(state.git_code.selected))
            .ok_or("Code picker has no selected commit")?
            .commit
            .id
            .clone()
    };
    key(hwnd, 0x0d, Modifiers::NONE)?;
    git_smoke::diff_ready(hwnd)?;
    {
        let state = owner(hwnd).unwrap().state.borrow();
        let result = state
            .git_diff
            .result()
            .ok_or("No selected Code comparison")?;
        if result.identity.commit.as_ref().map(|id| id.as_str()) != Some(commit.as_str())
            || result.model.source_bytes(DiffSide::Right) != bytes
        {
            return Err("Code picker compared the wrong revision or buffer".into());
        }
    }
    if let Some(path) = bitmap {
        git_smoke::readonly_frame(
            hwnd,
            Some(&path.with_file_name("native-git-code-commit.bmp")),
        )?;
    }
    key(hwnd, b'V' as usize, Modifiers::NONE)?;
    if let Some(path) = bitmap {
        git_smoke::readonly_frame(
            hwnd,
            Some(&path.with_file_name("native-git-code-unified.bmp")),
        )?;
    }
    key(hwnd, 0x1b, Modifiers::NONE)?;
    if owner(hwnd).unwrap().state.borrow().git_code.active || snapshot(hwnd)? != before {
        return Err("Returning from Code comparison changed the editor".into());
    }
    {
        let mut state = owner(hwnd).unwrap().state.borrow_mut();
        state.request_git(GitIntent::Status);
        state.request_git_history(commit.clone());
    }
    let until = Instant::now() + Duration::from_secs(5);
    while owner(hwnd).unwrap().state.borrow().git_revision.state()
        != jc_app::git_revision::GitRevisionState::Ready
    {
        if Instant::now() >= until {
            return Err("Workspace revision did not settle before opening Code".into());
        }
        git_smoke::one_task(hwnd)?;
    }
    command(hwnd, CommandId::ViewGit)?;
    git_smoke::diff_ready(hwnd)?;
    {
        let state = owner(hwnd).unwrap().state.borrow();
        let result = state
            .git_diff
            .result()
            .ok_or("Workspace lost its Code comparison")?;
        if !state.git_code.active
            || result.identity.commit.as_ref().map(|id| id.as_str()) != Some(commit.as_str())
            || result.model.source_bytes(DiffSide::Right) != bytes
        {
            return Err("Workspace Code action lost the selected revision or dirty file".into());
        }
    }
    key(hwnd, 0x1b, Modifiers::NONE)?;
    if snapshot(hwnd)? != before {
        return Err("Workspace Code action changed the original view".into());
    }
    checks.push("git_code_native_menu_painted_history_click_selected_commit_and_return");
    checks.push("git_code_native_parallel_unified_readonly_source_and_view_preserved");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GuiOptions;

    fn state() -> State {
        let mut state = State::new(&GuiOptions::default()).unwrap();
        state.width = 1200;
        state.height = 800;
        state.dpi = super::super::dpi::Dpi::BASE;
        state
            .workbench
            .new_file("code.txt", jc_text::TextEncoding::Utf8);
        state.workbench.insert_text("sin guardar\r\n").unwrap();
        state.git_code.active = true;
        state.git_panel = Some(GitIntent::Diff(GitBlobSource::Head));
        state
    }

    #[test]
    fn code_comparison_rejects_unpainted_resized_and_changed_document_targets() {
        let mut state = state();
        let before = state.workbench.active_bytes().unwrap();
        let frame = state.prepare_git_code().unwrap();
        let button = frame
            .hits
            .targets
            .iter()
            .find_map(|(r, i)| matches!(i, Intent::Toggle).then_some(*r))
            .unwrap();
        let (x, y) = (button.left + 1, button.top + 1);
        state.git_code_click(x, y);
        assert!(!state.git_code.unified);
        state.git_code.hits = Some(frame.hits.clone());
        state.width += 1;
        state.git_code_click(x, y);
        assert!(!state.git_code.unified);
        state.width -= 1;
        state.workbench.insert_text("otra edición").unwrap();
        state.git_code_click(x, y);
        assert!(!state.git_code.unified);
        state.workbench.undo().unwrap();
        let frame = state.prepare_git_code().unwrap();
        state.git_code.hits = Some(frame.hits);
        state.git_code_click(x, y);
        assert!(state.git_code.unified);
        assert_eq!(state.workbench.active_bytes().unwrap(), before);
    }

    #[test]
    fn code_comparison_partial_wheel_and_paired_rails_preserve_source() {
        let mut state = state();
        let before = state.workbench.active_bytes().unwrap();
        assert!(!state.git_code_wheel(-60, false, 3));
        assert_eq!(state.git_code.first, 0);
        assert!(state.git_code_wheel(-60, false, 3));
        assert_eq!(state.git_code.first, 3);
        assert!(state.git_code_wheel(-120, true, 3));
        assert_eq!(state.git_code.column, 4);
        for vertical in [false, true] {
            let rail = Rail {
                area: rect(20, 30, 220, 230),
                maximum: 1000,
                offset: 500,
                page: 100,
                vertical,
            };
            let thumb = rail.thumb();
            let target = rail.target(thumb.left, thumb.top, 0);
            assert!(target.abs_diff(500) < 6);
            assert_eq!(rail.target(220, 230, 0), 1000);
            assert_eq!(rail.target(-100, -100, 0), 0);
        }
        assert_eq!(state.workbench.active_bytes().unwrap(), before);
    }
}
