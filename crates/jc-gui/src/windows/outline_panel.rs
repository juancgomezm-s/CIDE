//! Persistent sidebar over the source controller. Painting never starts a job.
use super::{
    ffi::{Hdc, Rect},
    paint, shell, wide, State,
};
use jc_app::{
    source_services::{display_ui_label, SourceController, SourceMode, SourceState, ValueType},
    CompletedTask, Modifiers,
};
use std::path::PathBuf;

mod symbol_icon;
mod tree;
mod value_icon;
use tree::{Item, Kind, Tree};

const RELATION_MODES: [SourceMode; 2] = [SourceMode::Calls, SourceMode::Incoming];

pub(super) struct Outline {
    pub visible: bool,
    pub left: bool,
    mode: SourceMode,
    relations_view: bool,
    candidates: Option<Candidates>,
    controller: SourceController,
    dependencies: SourceController,
    relationships: [SourceController; 2],
    requested_relationships: [bool; 2],
    tree: Tree,
    tree_inputs: Option<[bool; 4]>,
    pub focused: bool,
    pub hits: Option<OutlineHits>,
    first: usize,
    wheel: i32,
}

impl Default for Outline {
    fn default() -> Self {
        Self {
            visible: true,
            left: false,
            mode: SourceMode::Symbols,
            relations_view: false,
            candidates: None,
            controller: SourceController::outline(),
            dependencies: SourceController::outline_dependencies(),
            relationships: RELATION_MODES.map(SourceController::outline_relationships),
            requested_relationships: [false; 2],
            tree: Tree::default(),
            tree_inputs: None,
            focused: false,
            hits: None,
            first: 0,
            wheel: 0,
        }
    }
}

impl Outline {
    fn controllers(&self) -> impl Iterator<Item = &SourceController> {
        [&self.controller, &self.dependencies]
            .into_iter()
            .chain(&self.relationships)
    }

    fn controller_for(&mut self, item: Item) -> (&mut SourceController, usize) {
        match item {
            Item::Symbol(index) => (&mut self.controller, index),
            Item::Dependency(index) => (&mut self.dependencies, index),
            Item::Outgoing(index) => (&mut self.relationships[0], index),
            Item::Incoming(index) => (&mut self.relationships[1], index),
        }
    }
}

pub(super) struct OutlineFrame {
    rect: Rect,
    dpi: super::dpi::Dpi,
    rows: Vec<Row>,
    message: String,
    focused: bool,
    mode: SourceMode,
    tools: Vec<(Rect, SourceMode, bool)>,
    close: Rect,
    breadcrumb: Option<String>,
    active_symbol: Option<usize>,
}

pub(super) struct OutlineHits {
    rect: Rect,
    rows: Vec<Target>,
    close: Rect,
    active_symbol: Option<usize>,
}

struct Candidates {
    paths: Vec<PathBuf>,
    selected: usize,
    item: Item,
}

#[derive(Clone, Copy)]
enum Target {
    Node(usize),
    Query(usize),
    Candidate(usize),
}

struct Row {
    target: Target,
    label: String,
    selected: bool,
    active: bool,
    depth: usize,
    kind: Kind,
    value_type: Option<ValueType>,
    branch: Option<bool>,
    folder: bool,
    directory: bool,
    unresolved: bool,
}

impl Row {
    fn has_value_icon(&self) -> bool {
        self.kind == Kind::Variable && !self.folder
    }

    fn name_offset(&self) -> i32 {
        if self.has_value_icon() {
            41
        } else {
            37
        }
    }
}

impl OutlineFrame {
    pub(super) fn hits(&self) -> OutlineHits {
        OutlineHits {
            rect: self.rect,
            rows: self.rows.iter().map(|row| row.target).collect(),
            close: self.close,
            active_symbol: self.active_symbol,
        }
    }
}

const HEAD: i32 = 76;
const ROW: i32 = 21;

fn provider(state: &State) -> bool {
    state
        .source_gui
        .registry()
        .supports_document(&state.workbench, SourceMode::Symbols)
}

pub(super) fn region(state: &State) -> Option<Rect> {
    if !state.outline.visible
        || state.width < state.dpi.pixels(900)
        || state.height < state.dpi.pixels(280)
    {
        return None;
    }
    let width = state.width.clamp(1, 8192);
    let explorer = super::explorer_panel::full_region(state);
    let edit_right = state
        .omnicore_region()
        .map_or(width, |panel| panel.left - 1);
    let top = state.chrome_layout().top();
    let bottom = (state.height.clamp(1, 8192) - state.cell_height - 8).max(0);
    if state.outline.left {
        let (top, right) = if let Some(tree) = explorer {
            let room = bottom - top;
            let outline_height = state.dpi.pixels(HEAD + ROW);
            let tree_height = state
                .dpi
                .pixels(super::shell::TREE_HEAD + super::shell::TREE_ROW);
            if room < outline_height + tree_height + 1 {
                return None;
            }
            (
                (top + room / 2).clamp(top + tree_height + 1, bottom - outline_height),
                tree.right,
            )
        } else {
            (top, state.dpi.pixels(250).min(width / 3))
        };
        if edit_right - right < state.dpi.pixels(200) {
            return None;
        }
        return Some(Rect {
            left: 0,
            top,
            right,
            bottom,
        });
    }
    let panel_width = state.dpi.pixels(260).min(width / 3);
    let left = explorer.map_or(0, |r| r.right + 1);
    let remaining = edit_right - left - panel_width - 1;
    if state.workbench.panes().is_split() && remaining / state.cell_width.max(1) < 90 {
        return None;
    }
    if remaining < state.dpi.pixels(200) {
        return None;
    }
    Some(Rect {
        left: edit_right - panel_width,
        right: edit_right,
        top,
        bottom,
    })
}

pub(super) fn editor_horizontal(state: &State) -> (i32, i32) {
    let mut left = super::explorer_panel::full_region(state).map_or(0, |panel| panel.right + 1);
    let mut right = state
        .omnicore_region()
        .map_or(state.width.clamp(1, 8192), |panel| panel.left - 1);
    if let Some(outline) = region(state) {
        if state.outline.left {
            left = outline.right + 1;
        } else {
            right = outline.left - 1;
        }
    }
    (left, right.max(left))
}

fn tools(rect: Rect, dpi: super::dpi::Dpi) -> Vec<(Rect, SourceMode)> {
    [SourceMode::Symbols, SourceMode::Calls]
        .into_iter()
        .enumerate()
        .map(|(i, mode)| {
            let width = (rect.right - rect.left - dpi.pixels(16)) / 2;
            let left = rect.left + dpi.pixels(8) + i as i32 * width;
            let top = rect.top + dpi.pixels(29);
            (
                Rect {
                    left,
                    top,
                    right: left + width - dpi.pixels(4),
                    bottom: top + dpi.pixels(24),
                },
                mode,
            )
        })
        .collect()
}

fn capacity(rect: Rect, dpi: super::dpi::Dpi) -> usize {
    ((rect.bottom - rect.top - dpi.pixels(HEAD)) / dpi.pixels(ROW)).clamp(0, 64) as usize
}

fn contains(rect: Rect, x: i32, y: i32) -> bool {
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
}

fn close_rect(rect: Rect, dpi: super::dpi::Dpi) -> Rect {
    Rect {
        left: rect.right - dpi.pixels(32),
        right: rect.right - dpi.pixels(8),
        top: rect.top + dpi.pixels(4),
        bottom: rect.top + dpi.pixels(28),
    }
}

fn same_rect(a: Rect, b: Rect) -> bool {
    (a.left, a.top, a.right, a.bottom) == (b.left, b.top, b.right, b.bottom)
}

fn running(controller: &SourceController) -> bool {
    matches!(
        controller.state(),
        SourceState::Queued | SourceState::Running
    )
}

impl State {
    pub(super) fn set_outline_visible(&mut self, visible: bool) {
        self.outline.visible = visible;
        self.exit_outline();
        self.tab_hits = None;
        self.cancel_outline();
        if visible {
            self.outline.mode = SourceMode::Symbols;
            self.schedule_outline();
            self.status = "Esquema visible · Intro abre o expande".into();
        } else {
            self.status =
                "Esquema oculto · pulsa Esquema en la barra inferior para mostrarlo".into();
        }
    }

    fn cancel_outline(&mut self) {
        let outline = &mut self.outline;
        outline.controller.cancel(&mut self.jobs);
        outline.dependencies.cancel(&mut self.jobs);
        for controller in &mut outline.relationships {
            controller.cancel(&mut self.jobs);
        }
        outline.tree_inputs = None;
        outline.candidates = None;
        outline.hits = None;
        outline.first = 0;
        outline.wheel = 0;
    }

    fn needs_outline(&self, dependency: bool) -> bool {
        let controller = if dependency {
            if !self.outline_tool_enabled(SourceMode::Dependencies) {
                return false;
            }
            &self.outline.dependencies
        } else {
            &self.outline.controller
        };
        controller.needs_outline_refresh(&self.workbench, self.project_root.as_deref())
    }

    pub(super) fn schedule_outline(&self) {
        let wanted = region(self).is_some() && provider(self);
        if (wanted
            && if self.outline.mode == SourceMode::Symbols {
                self.needs_outline(false)
                    || self.needs_outline(true)
                    || (0..2).any(|index| self.needs_tree_relationship(index))
            } else {
                self.needs_outline_relationships()
            })
            || (!wanted
                && self
                    .outline
                    .controllers()
                    .any(|c| !matches!(c.state(), SourceState::Idle | SourceState::Cancelled)))
        {
            self.bridge.signal();
        }
    }

    fn needs_outline_relationships(&self) -> bool {
        let mode = self.outline.mode;
        matches!(mode, SourceMode::Calls | SourceMode::Incoming)
            && self.outline_tool_enabled(mode)
            && self.outline.controller.needs_outline_refresh_for(
                &self.workbench,
                self.project_root.as_deref(),
                mode,
            )
    }

    fn needs_tree_relationship(&self, index: usize) -> bool {
        let mode = RELATION_MODES[index];
        self.outline.requested_relationships[index]
            && self.outline_tool_enabled(mode)
            && self.outline.relationships[index].needs_outline_refresh_for(
                &self.workbench,
                self.project_root.as_deref(),
                mode,
            )
    }

    fn request_outline(&mut self, mode: SourceMode) -> Result<(), String> {
        let root = self.project_root.as_deref();
        let registry = self.source_gui.registry();
        if mode == SourceMode::Symbols {
            if let Some(provider) = self.progress_schema.outline(&self.workbench, root) {
                return self.outline.controller.request_catalog(
                    mode,
                    provider,
                    registry,
                    &self.workbench,
                    root,
                    &mut self.jobs,
                );
            }
        }
        let controller = if mode == SourceMode::Dependencies {
            &mut self.outline.dependencies
        } else {
            &mut self.outline.controller
        };
        controller.request(mode, registry, &self.workbench, root, &mut self.jobs)
    }

    pub(super) fn tick_outline(&mut self) -> bool {
        if region(self).is_none() || !provider(self) {
            self.exit_outline_if_focused();
            if self
                .outline
                .controllers()
                .all(|c| matches!(c.state(), SourceState::Idle | SourceState::Cancelled))
            {
                return false;
            }
            self.cancel_outline();
            return true;
        }
        let root = self.project_root.as_deref();
        let mut changed =
            self.outline
                .controller
                .validate_context(&self.workbench, root, &mut self.jobs);
        changed |=
            self.outline
                .dependencies
                .validate_context(&self.workbench, root, &mut self.jobs);
        for controller in &mut self.outline.relationships {
            changed |= controller.validate_context(&self.workbench, root, &mut self.jobs);
        }
        if self.outline.mode == SourceMode::Symbols {
            for (dependency, mode) in [
                (false, SourceMode::Symbols),
                (true, SourceMode::Dependencies),
            ] {
                if self.needs_outline(dependency) {
                    self.outline.hits = None;
                    self.outline.tree_inputs = None;
                    self.outline.candidates = None;
                    self.outline.first = 0;
                    let _ = self.request_outline(mode);
                    changed = true;
                }
            }
            for (index, mode) in RELATION_MODES.into_iter().enumerate() {
                if self.needs_tree_relationship(index) {
                    self.outline.tree_inputs = None;
                    self.outline.candidates = None;
                    let _ = self.outline.relationships[index].request(
                        mode,
                        self.source_gui.registry(),
                        &self.workbench,
                        self.project_root.as_deref(),
                        &mut self.jobs,
                    );
                    changed = true;
                }
            }
        } else if self.needs_outline_relationships() {
            self.outline.hits = None;
            self.outline.candidates = None;
            self.outline.first = 0;
            let _ = self.request_outline(self.outline.mode);
            changed = true;
        }
        let root = self.project_root.as_deref();
        changed |= self
            .outline
            .controller
            .tick(&self.workbench, root, &mut self.jobs);
        for controller in &mut self.outline.relationships {
            changed |= controller.tick(&self.workbench, root, &mut self.jobs);
        }
        changed |= self
            .outline
            .dependencies
            .tick(&self.workbench, root, &mut self.jobs);
        if changed {
            self.outline.hits = None;
            self.update_outline_tree();
        }
        changed
    }

    pub(super) fn accept_outline(&mut self, completion: &mut CompletedTask) -> bool {
        let root = self.project_root.as_deref();
        let accepted = self
            .outline
            .controller
            .accept(&self.workbench, root, completion)
            || self
                .outline
                .dependencies
                .accept(&self.workbench, root, completion)
            || self
                .outline
                .relationships
                .iter_mut()
                .any(|controller| controller.accept(&self.workbench, root, completion));
        if accepted {
            self.outline.hits = None;
            self.outline.tree_inputs = None;
            self.update_outline_tree();
        }
        accepted
    }

    fn update_outline_tree(&mut self) {
        let outline = &mut self.outline;
        if outline.mode != SourceMode::Symbols
            || outline.tree_inputs.is_some()
            || running(&outline.controller)
            || running(&outline.dependencies)
        {
            return;
        }
        let root = self.project_root.as_deref();
        let symbols = outline.controller.ready(&self.workbench, root);
        let dependencies = outline.dependencies.ready(&self.workbench, root);
        let available_results = outline
            .relationships
            .each_ref()
            .map(|controller| controller.ready(&self.workbench, root));
        let relationships = std::array::from_fn(|index| {
            if !outline.relations_view {
                return None;
            }
            let controller = &outline.relationships[index];
            Some(tree::Relationships {
                items: if available_results[index] {
                    controller.items()
                } else {
                    &[]
                },
                notice: if running(controller) {
                    Some("Buscando…")
                } else if !available_results[index] {
                    Some(controller.message())
                } else if controller.items().is_empty() {
                    Some("Sin resultados")
                } else {
                    None
                },
            })
        });
        outline.tree.rebuild(
            if symbols && !outline.relations_view {
                outline.controller.items()
            } else {
                &[]
            },
            if dependencies && !outline.relations_view {
                outline.dependencies.items()
            } else {
                &[]
            },
            relationships,
        );
        outline.tree_inputs = Some([
            symbols,
            dependencies,
            available_results[0],
            available_results[1],
        ]);
    }

    fn outline_tree_is_current(&self) -> bool {
        let outline = &self.outline;
        let root = self.project_root.as_deref();
        outline.tree_inputs.is_some_and(|ready| {
            outline
                .controllers()
                .zip(ready)
                .all(|(controller, had_results)| {
                    !had_results || controller.ready(&self.workbench, root)
                })
        })
    }

    fn outline_tool_enabled(&self, mode: SourceMode) -> bool {
        let registry = self.source_gui.registry();
        if mode == SourceMode::Incoming {
            registry.supports_incoming(&self.workbench, self.project_root.as_deref())
        } else {
            registry.supports_document(&self.workbench, mode)
        }
    }

    fn outline_message(&self) -> &str {
        let outline = &self.outline;
        if !provider(self) {
            "Sin esquema para este lenguaje"
        } else if self.workbench.active_encoding().is_none() {
            "Elige la codificación del documento"
        } else if outline.candidates.is_some() {
            "Elige el archivo de destino"
        } else if outline.mode != SourceMode::Symbols {
            outline.controller.message()
        } else if running(&outline.controller)
            || running(&outline.dependencies)
            || !self.outline_tree_is_current()
        {
            "Actualizando esquema…"
        } else if matches!(
            outline.controller.state(),
            SourceState::Failed | SourceState::Limited
        ) {
            outline.controller.message()
        } else if matches!(
            outline.dependencies.state(),
            SourceState::Failed | SourceState::Limited
        ) {
            outline.dependencies.message()
        } else if let Some((_, notice)) = outline.controller.message().split_once(" · ") {
            notice
        } else if outline.tree.rows.is_empty() {
            "Sin declaraciones ni dependencias"
        } else {
            ""
        }
    }

    pub(super) fn outline_presentation(&self) -> Option<OutlineFrame> {
        let rect = region(self)?;
        let columns = ((rect.right - rect.left - self.dpi.pixels(30)) / self.cell_width.max(1))
            .max(1) as usize;
        let root = self.project_root.as_deref();
        let outline = &self.outline;
        let mut path = outline.controller.symbol_path(&self.workbench, root);
        let active = path.next_back().filter(|_| outline.candidates.is_none());
        let active_symbol = active.map(|(index, _)| index);
        let breadcrumb = active.map(|(_, leaf)| {
            let mut text = String::new();
            for (_, ancestor) in path {
                text.push_str(ancestor);
                text.push_str(" › ");
            }
            text.push_str(leaf);
            let fitted = display_ui_label(&text, columns);
            if fitted == text {
                fitted
            } else {
                display_ui_label(leaf, columns)
            }
        });
        let count = capacity(rect, self.dpi);
        let total = self.outline_count();
        let selected = self.outline_selected();
        let first = outline.first.min(total.saturating_sub(count));
        let mut rows = Vec::with_capacity(total.min(count));
        for index in first..total.min(first + count) {
            let target = self.outline_target(index);
            let mut row = Row {
                target,
                label: String::new(),
                selected: index == selected,
                active: false,
                depth: 0,
                kind: Kind::Include,
                value_type: None,
                branch: None,
                folder: false,
                directory: false,
                unresolved: false,
            };
            match target {
                Target::Node(id) => {
                    let node = &outline.tree.nodes[id];
                    row.active = node.item.is_some_and(
                        |item| matches!(item, Item::Symbol(i) if Some(i) == active_symbol),
                    );
                    row.depth = node.depth.min(8);
                    row.kind = node.kind;
                    row.value_type = node.value_type;
                    row.branch = (!node.children.is_empty()).then_some(node.expanded);
                    row.folder = node.item.is_none();
                    row.directory = node.directory;
                    row.unresolved = node.unresolved;
                    let space = (rect.right
                        - rect.left
                        - self
                            .dpi
                            .pixels(14 + row.name_offset() + row.depth as i32 * 12))
                        / self.cell_width.max(1);
                    row.label = display_ui_label(&node.name, space.max(1) as usize);
                    if let Some(origin) = &node.origin {
                        let label = format!("{} · {origin}", row.label);
                        row.label = display_ui_label(&label, space.max(1) as usize);
                    }
                }
                Target::Query(index) => {
                    use jc_app::source_services::RelationMark;
                    let item = &outline.controller.items()[index];
                    row.label = display_ui_label(&item.label, columns);
                    row.kind = match item.relation_mark(outline.mode) {
                        Some(RelationMark::Outgoing) => Kind::CallOutgoing,
                        Some(RelationMark::Incoming) => Kind::CallIncoming,
                        Some(RelationMark::Unresolved) => Kind::CallUnresolved,
                        None => Kind::Other,
                    };
                }
                Target::Candidate(index) => {
                    let path = &outline.candidates.as_ref().unwrap().paths[index];
                    row.label = display_ui_label(
                        &path
                            .strip_prefix(root.unwrap_or(std::path::Path::new("")))
                            .unwrap_or(path)
                            .to_string_lossy(),
                        columns,
                    );
                }
            }
            rows.push(row);
        }
        let message = display_ui_label(self.outline_message(), columns);
        Some(OutlineFrame {
            rect,
            dpi: self.dpi,
            rows,
            active_symbol: message.is_empty().then_some(active_symbol).flatten(),
            message,
            focused: self.outline_focused(),
            mode: if outline.relations_view {
                SourceMode::Calls
            } else {
                SourceMode::Symbols
            },
            tools: tools(rect, self.dpi)
                .into_iter()
                .map(|(r, mode)| (r, mode, self.outline_tool_enabled(mode)))
                .collect(),
            close: close_rect(rect, self.dpi),
            breadcrumb,
        })
    }

    pub(super) fn outline_focused(&self) -> bool {
        self.explorer_focused && self.outline.focused && region(self).is_some()
    }

    fn outline_count(&self) -> usize {
        let outline = &self.outline;
        let root = self.project_root.as_deref();
        if let Some(candidates) = &outline.candidates {
            let controller = match candidates.item {
                Item::Dependency(_) => &outline.dependencies,
                Item::Symbol(_) => &outline.controller,
                Item::Outgoing(_) => &outline.relationships[0],
                Item::Incoming(_) => &outline.relationships[1],
            };
            if controller.ready(&self.workbench, root) {
                candidates.paths.len()
            } else {
                0
            }
        } else if outline.mode == SourceMode::Symbols {
            if self.outline_tree_is_current() {
                outline.tree.rows.len()
            } else {
                0
            }
        } else if outline.controller.ready(&self.workbench, root) {
            outline.controller.items().len()
        } else {
            0
        }
    }

    fn outline_target(&self, index: usize) -> Target {
        if self.outline.candidates.is_some() {
            Target::Candidate(index)
        } else if self.outline.mode == SourceMode::Symbols {
            Target::Node(self.outline.tree.rows[index])
        } else {
            Target::Query(index)
        }
    }

    fn outline_selected(&self) -> usize {
        let outline = &self.outline;
        if let Some(candidates) = &outline.candidates {
            candidates.selected
        } else if outline.mode == SourceMode::Symbols {
            outline
                .tree
                .rows
                .iter()
                .position(|&id| Some(id) == outline.tree.selected)
                .unwrap_or(0)
        } else {
            outline.controller.selected()
        }
    }

    pub(super) fn outline_ready(&self) -> bool {
        self.outline_count() > 0
            && match self.outline_target(self.outline_selected()) {
                Target::Node(id) => {
                    let node = &self.outline.tree.nodes[id];
                    node.item.is_some()
                }
                _ => true,
            }
    }

    fn activate_outline_item(&mut self, item: Item, side: bool) {
        let (controller, index) = self.outline.controller_for(item);
        match controller.activate_at_index(
            &mut self.workbench,
            self.project_root.as_deref(),
            index,
            side,
        ) {
            Ok(Some(paths)) => {
                controller.first();
                controller.select_relative(index as isize);
                self.outline.candidates = Some(Candidates {
                    paths,
                    selected: 0,
                    item,
                });
                self.outline.first = 0;
                self.outline.hits = None;
                self.status = "Referencia ambigua: elige un candidato en Esquema".into();
            }
            Ok(None) => {
                self.exit_outline();
                self.ensure_encoding_selector();
                self.status = "Destino abierto desde Esquema".into();
            }
            Err(error) => self.error(error),
        }
    }

    pub(super) fn activate_outline(&mut self, side: bool) {
        if self.outline_count() == 0 {
            return;
        }
        match self.outline_target(self.outline_selected()) {
            Target::Candidate(index) => {
                let outline = &mut self.outline;
                let candidates = outline.candidates.as_ref().unwrap();
                let (item, path) = (candidates.item, candidates.paths[index].clone());
                let (controller, _) = outline.controller_for(item);
                match controller.activate_candidate(
                    &mut self.workbench,
                    self.project_root.as_deref(),
                    &path,
                    side,
                ) {
                    Ok(()) => {
                        outline.candidates = None;
                        self.exit_outline();
                        self.ensure_encoding_selector();
                    }
                    Err(error) => self.error(error),
                }
            }
            Target::Query(index) => self.activate_outline_item(Item::Symbol(index), side),
            Target::Node(id) => {
                if let Some(item) = self.outline.tree.nodes[id].item {
                    self.activate_outline_item(item, side);
                } else if !side {
                    let expanded = self.outline.tree.nodes[id].expanded;
                    self.expand_outline_node(id, !expanded);
                    self.reveal_outline_selection();
                }
            }
        }
    }

    fn expand_outline_node(&mut self, id: usize, expanded: bool) {
        self.outline.tree.set_expanded(id, expanded);
        if expanded {
            if let Some(index) = self.outline.tree.relationship_group(id) {
                self.outline.requested_relationships[index] = true;
                self.schedule_outline();
            }
        }
    }

    pub(super) fn refresh_outline(&mut self) {
        self.outline.mode = SourceMode::Symbols;
        self.outline.relations_view = false;
        self.cancel_outline();
        self.schedule_outline();
    }

    pub(super) fn exit_outline(&mut self) {
        self.explorer_focused = false;
        self.outline.focused = false;
    }

    fn exit_outline_if_focused(&mut self) {
        if self.outline.focused {
            self.exit_outline();
        }
    }

    fn select_outline_target(&mut self, target: Target) {
        match target {
            Target::Node(id) => self.outline.tree.selected = Some(id),
            Target::Query(index) => {
                self.outline.controller.first();
                self.outline.controller.select_relative(index as isize);
            }
            Target::Candidate(index) => self.outline.candidates.as_mut().unwrap().selected = index,
        }
    }

    fn reveal_outline_selection(&mut self) {
        let rows = region(self)
            .map_or(1, |rect| capacity(rect, self.dpi))
            .max(1);
        let selected = self.outline_selected();
        self.outline.first = self
            .outline
            .first
            .clamp(selected.saturating_sub(rows - 1), selected);
        self.outline.hits = None;
    }

    pub(super) fn outline_key(&mut self, code: usize, modifiers: Modifiers) {
        if code == 0x74 && modifiers == Modifiers::NONE && self.outline.mode == SourceMode::Symbols
        {
            self.refresh_outline();
            return;
        }
        if self.outline_count() == 0
            || (modifiers != Modifiers::NONE && modifiers != Modifiers::CTRL)
        {
            return;
        }
        if code == 0x0d {
            self.activate_outline(modifiers == Modifiers::CTRL);
            return;
        }
        let selected = self.outline_selected();
        let index = match code {
            0x26 => selected.saturating_sub(1),
            0x28 => selected.saturating_add(1),
            0x21 => selected.saturating_sub(10),
            0x22 => selected.saturating_add(10),
            0x24 => 0,
            0x23 => self.outline_count() - 1,
            0x25 | 0x27 if modifiers == Modifiers::NONE => {
                if let Target::Node(id) = self.outline_target(selected) {
                    let node = &self.outline.tree.nodes[id];
                    if code == 0x25 && node.expanded
                        || code == 0x27 && !node.expanded && !node.children.is_empty()
                    {
                        self.expand_outline_node(id, code == 0x27);
                    } else if let Some(target) = if code == 0x25 {
                        node.parent
                    } else {
                        node.children.first().copied()
                    } {
                        self.outline.tree.selected = Some(target);
                    }
                    self.reveal_outline_selection();
                }
                return;
            }
            _ => return,
        };
        self.select_outline_target(self.outline_target(index.min(self.outline_count() - 1)));
        self.reveal_outline_selection();
    }

    pub(super) fn outline_click(&mut self, x: i32, y: i32, double: bool) -> bool {
        let Some(rect) = region(self).filter(|r| contains(*r, x, y)) else {
            return false;
        };
        let hits = self
            .outline
            .hits
            .as_ref()
            .filter(|hits| same_rect(hits.rect, rect));
        if hits.is_some_and(|hits| {
            same_rect(hits.close, close_rect(rect, self.dpi)) && contains(hits.close, x, y)
        }) {
            self.set_outline_visible(false);
            return true;
        }
        if let Some((_, mode)) = tools(rect, self.dpi)
            .into_iter()
            .find(|(r, _)| contains(*r, x, y))
        {
            if self.outline_tool_enabled(mode) {
                self.outline.relations_view = mode != SourceMode::Symbols;
                self.outline.mode = SourceMode::Symbols;
                self.outline.requested_relationships = [self.outline.relations_view; 2];
                self.outline.first = 0;
                self.outline.hits = None;
                self.outline.candidates = None;
                self.outline.tree_inputs = None;
                self.schedule_outline();
                self.update_outline_tree();
                self.explorer_focused = true;
                self.outline.focused = true;
                self.bottom.focused = false;
                return true;
            }
            self.explorer_focused = true;
            self.outline.focused = true;
            self.bottom.focused = false;
            return true;
        }
        let side = self.modifiers().contains(Modifiers::CTRL);
        if (rect.top + self.dpi.pixels(54)..rect.top + self.dpi.pixels(HEAD)).contains(&y)
            && self.outline.candidates.is_none()
        {
            if let Some((index, _)) = self
                .outline
                .controller
                .active_symbol(&self.workbench, self.project_root.as_deref())
                .filter(|(index, _)| hits.is_some_and(|hits| hits.active_symbol == Some(*index)))
            {
                self.activate_outline_item(Item::Symbol(index), side);
                return true;
            }
        }
        self.explorer_focused = true;
        self.outline.focused = true;
        self.bottom.focused = false;
        if self.outline_count() == 0 {
            return true;
        }
        let row = usize::try_from(y - rect.top - self.dpi.pixels(HEAD))
            .ok()
            .map(|top| top / self.dpi.pixels(ROW) as usize);
        if let Some(target) = row.and_then(|row| hits?.rows.get(row)).copied() {
            self.select_outline_target(target);
            match target {
                Target::Node(id) => {
                    let node = &self.outline.tree.nodes[id];
                    let toggle = node.item.is_none()
                        || x < rect.left + self.dpi.pixels(23 + node.depth.min(8) as i32 * 12);
                    if toggle && !node.children.is_empty() {
                        // Windows sends both a click and a double-click message.
                        if !double {
                            self.expand_outline_node(id, !node.expanded);
                            self.reveal_outline_selection();
                        }
                    } else if double {
                        self.activate_outline(side);
                    }
                }
                _ if double => self.activate_outline(side),
                _ => {}
            }
        }
        true
    }

    pub(super) fn outline_wheel(&mut self, x: i32, y: i32, delta: i32, lines: u32) -> bool {
        let Some(rect) = region(self).filter(|r| contains(*r, x, y)) else {
            return false;
        };
        let rows = capacity(rect, self.dpi);
        let lines = if lines == u32::MAX {
            rows
        } else {
            lines.min(128) as usize
        };
        let total = self.outline.wheel.saturating_add(delta);
        self.outline.wheel = total % 120;
        let step = (total / 120) as isize * lines as isize;
        if step != 0 {
            self.outline.first = self
                .outline
                .first
                .saturating_add_signed(-step)
                .min(self.outline_count().saturating_sub(rows));
            self.outline.hits = None;
        }
        true
    }
}

pub(super) unsafe fn draw(dc: Hdc, frame: &OutlineFrame) -> Result<(), String> {
    let rect = frame.rect;
    let px = |value| frame.dpi.pixels(value);
    paint::fill(dc, rect, shell::SIDEBAR)?;
    paint::fill(
        dc,
        Rect {
            bottom: rect.top + 1,
            ..rect
        },
        shell::SURFACE,
    )?;
    paint::fill(
        dc,
        Rect {
            left: rect.right - 1,
            ..rect
        },
        shell::SURFACE,
    )?;

    let label = |top, bottom| Rect {
        left: rect.left + px(12),
        right: rect.right - px(8),
        top: rect.top + px(top),
        bottom: rect.top + px(bottom),
    };
    paint::text(
        dc,
        Rect {
            right: frame.close.left - px(4),
            ..label(8, 27)
        },
        &wide("ESQUEMA"),
        shell::TEXT,
        shell::SIDEBAR,
    )?;
    paint::text(
        dc,
        Rect {
            left: frame.close.left + px(7),
            top: frame.close.top + px(2),
            ..frame.close
        },
        &wide("×"),
        shell::MUTED,
        shell::SIDEBAR,
    )?;
    let divider = rect.top + px(52);
    paint::fill(
        dc,
        Rect {
            top: divider,
            bottom: divider + 1,
            ..rect
        },
        shell::SURFACE,
    )?;
    for (r, mode, enabled) in &frame.tools {
        let active = *mode == frame.mode;
        paint::text(
            dc,
            Rect {
                left: r.left + px(5),
                top: r.top + px(2),
                ..*r
            },
            &wide(if *mode == SourceMode::Symbols {
                "Contenido"
            } else {
                "Relaciones"
            }),
            if active {
                shell::ACCENT
            } else if *enabled {
                shell::TEXT
            } else {
                shell::MUTED
            },
            shell::SIDEBAR,
        )?;
        if active {
            paint::fill(
                dc,
                Rect {
                    left: r.left + px(5),
                    right: r.right - px(4),
                    top: divider - px(2),
                    bottom: divider,
                },
                shell::ACCENT,
            )?;
        }
    }
    if let Some(text) = frame
        .breadcrumb
        .as_deref()
        .filter(|_| frame.message.is_empty())
        .or_else(|| (!frame.message.is_empty()).then_some(frame.message.as_str()))
    {
        paint::text(
            dc,
            label(56, HEAD),
            &wide(text),
            if frame.message.is_empty() {
                shell::ACCENT
            } else {
                shell::MUTED
            },
            shell::SIDEBAR,
        )?;
    }
    for (i, row) in frame.rows.iter().enumerate() {
        let top = rect.top + px(HEAD) + i as i32 * px(ROW);
        let bottom = top + px(ROW);
        let background = if row.selected && frame.focused {
            shell::SELECTED
        } else if row.active {
            shell::RAISED
        } else {
            shell::SIDEBAR
        };
        let area = Rect {
            left: rect.left + px(5),
            top,
            right: rect.right - px(6),
            bottom,
        };
        if background != shell::SIDEBAR {
            paint::fill(dc, area, background)?;
        }
        let x = rect.left + px(11 + 12 * row.depth as i32);
        if row.depth > 0 {
            let left = x - px(9);
            paint::fill(
                dc,
                Rect {
                    left,
                    top,
                    right: left + 1,
                    bottom,
                },
                shell::SURFACE,
            )?;
        }
        if let Some(expanded) = row.branch {
            shell::chevron(dc, x, top + px(7), expanded, shell::MUTED)?;
        }
        let (_, color) = row.kind.icon();
        if row.directory {
            shell::file_icon(dc, x + px(12), top + px(4), true, color, px(16))?;
        } else if row.has_value_icon() {
            let icon = Rect {
                left: x + px(12),
                right: x + px(35),
                top: top + px(3),
                bottom: top + px(19),
            };
            if icon.right <= area.right {
                value_icon::draw(dc, icon, frame.dpi, row.value_type, background)?;
            }
        } else {
            let bounds = Rect {
                left: x + px(12),
                right: x + px(32),
                top: top + px(1),
                bottom: top + px(19),
            };
            if bounds.right <= area.right {
                symbol_icon::draw(dc, bounds, frame.dpi, row.kind, background, row.unresolved)?;
            }
        }
        paint::text(
            dc,
            Rect {
                left: x + px(row.name_offset()),
                top: top + px(2),
                ..area
            },
            &wide(&row.label),
            if row.active {
                shell::ACCENT
            } else if row.unresolved {
                shell::MUTED
            } else {
                shell::TEXT
            },
            background,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GuiOptions;
    use jc_text::TextEncoding;
    use std::time::{Duration, Instant};

    fn finish(state: &mut State) {
        state.poll_tasks();
        let deadline = Instant::now() + Duration::from_secs(5);
        while state.outline.controllers().any(running) {
            state.poll_tasks();
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
    }

    #[test]
    fn outline_relationships_have_their_own_folders_and_navigate_both_directions() {
        let mut state = State::new(&GuiOptions::default()).unwrap();
        state.width = 1400;
        state.height = 800;
        let source = "fn principal() { destino(); externo(); } fn destino() {}";
        state
            .workbench
            .new_file("relaciones.rs", TextEncoding::Utf8);
        state.workbench.insert_text(source).unwrap();
        let at = source.find("principal").unwrap();
        state.workbench.set_linear_selection_bytes(at, at).unwrap();
        finish(&mut state);
        assert!(state
            .outline
            .tree
            .nodes
            .iter()
            .all(|node| !matches!(node.kind, Kind::CallIncoming | Kind::CallOutgoing)));
        let rect = region(&state).unwrap();
        let (area, _) = tools(rect, state.dpi)
            .into_iter()
            .find(|(_, mode)| *mode == SourceMode::Calls)
            .unwrap();
        state.outline_click(area.left + 1, area.top + 1, false);
        finish(&mut state);
        assert_eq!(state.outline.tree.rows.len(), 2);
        let outgoing = state
            .outline
            .tree
            .nodes
            .iter()
            .position(|node| node.name == "Llama a")
            .unwrap();
        state.expand_outline_node(outgoing, true);
        select_tree_row(&mut state, |node| node.name == "destino");
        click_tree_selection(&mut state, true);
        finish(&mut state);
        let incoming = state
            .outline
            .tree
            .nodes
            .iter()
            .position(|node| node.name == "Llamado por")
            .unwrap();
        state.expand_outline_node(incoming, true);
        select_tree_row(&mut state, |node| node.name == "principal");
        click_tree_selection(&mut state, true);
        assert_eq!(
            state.workbench.active_editor().unwrap().cursor,
            source.find("destino()").unwrap()
        );
    }

    fn select_tree_row(state: &mut State, predicate: impl Fn(&tree::Node) -> bool) {
        let tree = &mut state.outline.tree;
        let id = tree
            .nodes
            .iter()
            .position(predicate)
            .expect("missing outline row");
        let mut parent = tree.nodes[id].parent;
        while let Some(ancestor) = parent {
            tree.set_expanded(ancestor, true);
            parent = tree.nodes[ancestor].parent;
        }
        tree.selected = Some(id);
        state.reveal_outline_selection();
    }

    fn click_tree_selection(state: &mut State, double: bool) {
        let frame = state.outline_presentation().unwrap();
        let row = frame.rows.iter().position(|row| row.selected).unwrap();
        let x = frame.rect.left + state.dpi.pixels(50 + 12 * frame.rows[row].depth as i32);
        let y = frame.rect.top + state.dpi.pixels(HEAD) + row as i32 * state.dpi.pixels(ROW) + 2;
        state.outline.hits = Some(frame.hits());
        assert!(state.explorer_click(x, y, double));
    }

    #[test]
    fn left_outline_shares_the_explorer_column_without_moving_editor() {
        let mut state = State::new(&GuiOptions::default()).unwrap();
        state.width = 1400;
        state.height = 900;
        state.explorer_visible = true;
        let original = super::super::explorer_panel::region(&state).unwrap();
        assert!(region(&state).unwrap().left > original.right);

        state.outline.left = true;
        let outline = region(&state).unwrap();
        let explorer = super::super::explorer_panel::region(&state).unwrap();
        assert_eq!(outline.left, explorer.left);
        assert_eq!(outline.right, explorer.right);
        assert_eq!(outline.top, explorer.bottom + 1);
        assert_eq!(outline.bottom, original.bottom);
        let (left, right) = editor_horizontal(&state);
        assert_eq!((left, right), (outline.right + 1, state.width));
        assert!(super::super::paint::areas(&state)
            .iter()
            .all(|(_, area)| area.left == left && area.right == right));

        state.explorer_visible = false;
        let full = region(&state).unwrap();
        assert_eq!(full.top, original.top);
        assert_eq!(full.right, outline.right);
        assert_eq!(editor_horizontal(&state), (left, right));

        state.outline.visible = false;
        assert_eq!(editor_horizontal(&state), (0, right));
    }

    #[test]
    #[ignore = "requiere el proyecto sintético de OpenEdge con el ejemplo de asistencia"]
    fn progress_real_outline_gui_lists_program_and_nested_include_and_opens_to_side() {
        let root = std::path::PathBuf::from(
            std::env::var_os("CIDE_TEST_PROGRESS_SCHEMA_PROJECT").unwrap(),
        );
        let source = root.join("asistencia.p");
        let include = root.join("primero/detalle.i");
        let declarations = std::fs::read_to_string(&include).unwrap();
        let mut state = State::new(&GuiOptions {
            paths: vec![source.clone()],
            project_root: Some(root),
            encoding: Some(TextEncoding::Utf8),
            ..GuiOptions::default()
        })
        .unwrap();
        state.width = 1600;
        state.height = 900;
        state.set_outline_visible(true);
        finish(&mut state);
        let nodes = &state.outline.tree.nodes;
        assert!(nodes.iter().any(|row| row.name == "vPrograma"));
        assert!(nodes
            .iter()
            .any(|row| row.name == "Codigo" && row.kind == Kind::Field));
        assert!(nodes
            .iter()
            .any(|row| row.name == "porCodigo" && row.kind == Kind::Index));
        assert!(!nodes.iter().any(|row| row.name == "vIncorrecto"));
        select_tree_row(&mut state, |row| row.name == "pIncluido");
        click_tree_selection(&mut state, false);
        assert!(state.outline_focused());
        state.outline_key(0x0d, Modifiers::CTRL);
        assert!(state.workbench.panes().is_split());
        assert_eq!(
            state.workbench.active_path().unwrap(),
            include.canonicalize().unwrap()
        );
        assert_eq!(
            state.workbench.active_editor().unwrap().cursor,
            declarations.find("pIncluido").unwrap()
        );
        assert_eq!(
            state.workbench.active_bytes().unwrap(),
            declarations.as_bytes()
        );
    }

    #[test]
    fn outline_close_reopen_and_enter_navigation_keep_document_and_undo() {
        let mut state = State::new(&GuiOptions::default()).unwrap();
        state.width = 1200;
        state.height = 800;
        state.workbench.new_file("esquema.p", TextEncoding::Utf8);
        let source = "PROCEDURE uno:\nEND PROCEDURE.\nPROCEDURE dos:\nEND PROCEDURE.\n";
        state.workbench.insert_text(source).unwrap();
        let revision = state
            .workbench
            .active_document()
            .unwrap()
            .session
            .document()
            .revision();
        let cursor = state.workbench.active_editor().unwrap().cursor;
        finish(&mut state);
        let frame = state.outline_presentation().unwrap();
        assert_eq!(frame.rows[0].label, "Procedimientos");
        let close = frame.close;
        state.outline.hits = Some(frame.hits());
        state.outline.focused = true;
        state.explorer_focused = true;
        assert!(state.explorer_click(close.left + 2, close.top + 2, false));
        assert!(!state.outline.visible);
        assert!(!state.outline.focused && !state.explorer_focused);
        assert!(state.outline.hits.is_none());
        assert_eq!(state.outline.controller.state(), SourceState::Cancelled);
        assert!(state.outline_presentation().is_none());
        assert!(!state.outline_click(close.left + 2, close.top + 2, false));
        assert_eq!(state.workbench.active_editor().unwrap().cursor, cursor);

        assert!(state.status_click(state.width - 2, state.height - 2));
        assert!(state.outline.visible);
        assert!(state.status.contains("Esquema visible"));
        finish(&mut state);
        let frame = state.outline_presentation().unwrap();
        assert_eq!(frame.rows[0].label, "Procedimientos");
        state.outline.hits = Some(frame.hits());
        assert!(state.explorer_click(frame.rect.left + 24, frame.rect.top + HEAD + 2, false));
        assert_eq!(
            state
                .outline_presentation()
                .unwrap()
                .rows
                .iter()
                .map(|row| row.label.as_str())
                .collect::<Vec<_>>(),
            ["Procedimientos", "dos", "uno"],
        );
        select_tree_row(&mut state, |row| row.name == "uno");
        click_tree_selection(&mut state, false);
        assert!(state.outline_focused());
        state.outline_key(0x0d, Modifiers::NONE);
        assert_eq!(
            state.workbench.active_editor().unwrap().cursor,
            source.find("uno").unwrap()
        );
        assert_eq!(state.workbench.active_bytes().unwrap(), source.as_bytes());
        assert_eq!(
            state
                .workbench
                .active_document()
                .unwrap()
                .session
                .document()
                .revision(),
            revision
        );
        state.workbench.undo().unwrap();
        assert!(state.workbench.active_bytes().unwrap().is_empty());
    }

    #[test]
    fn abl_breadcrumb_tracks_cursor_and_requires_current_revision_to_jump() {
        let root = std::env::current_dir().unwrap();
        let mut state = State::new(&GuiOptions {
            project_root: Some(root.clone()),
            ..GuiOptions::default()
        })
        .unwrap();
        state.width = 1600;
        state.height = 900;
        let tab = state
            .workbench
            .new_file(root.join("Ruta.cls"), TextEncoding::Utf8);
        let source = "CLASS demo.Ruta:\nMETHOD PUBLIC VOID Trabajo():\n  DO:\n    MESSAGE 1.\n  END.\nEND METHOD.\n\nEND CLASS.\n";
        state.workbench.insert_text(source).unwrap();
        let body = source.find("MESSAGE").unwrap();
        assert!(state.workbench.jump_to_location(tab, body));
        finish(&mut state);
        let frame = state.outline_presentation().unwrap();
        assert_eq!(frame.breadcrumb.as_deref(), Some("demo.Ruta › Trabajo"));
        let x = frame.rect.left + 24;
        let y = frame.rect.top + state.dpi.pixels(57);
        state.outline.hits = Some(frame.hits());
        assert!(state.outline_click(x, y, false));
        assert_eq!(
            state.workbench.active_editor().unwrap().cursor,
            source.find("Trabajo").unwrap()
        );

        assert!(state
            .workbench
            .jump_to_location(tab, source.find("END CLASS").unwrap()));
        assert_eq!(
            state.outline_presentation().unwrap().breadcrumb.as_deref(),
            Some("demo.Ruta")
        );
        assert_eq!(
            state.jobs.outstanding(),
            0,
            "changing cursor does not restart the outline"
        );

        assert!(state.workbench.jump_to_location(tab, body));
        state.workbench.insert_text(" ").unwrap();
        assert!(state.outline_presentation().unwrap().breadcrumb.is_none());
        assert!(state.outline_click(x, y, false));
        assert_eq!(state.workbench.active_editor().unwrap().cursor, body + 1);
    }

    #[test]
    fn outline_close_hit_expires_when_geometry_changes() {
        let mut state = State::new(&GuiOptions::default()).unwrap();
        state.width = 1200;
        state.height = 800;
        state.workbench.new_file("esquema.js", TextEncoding::Utf8);
        state
            .workbench
            .insert_text("function ejemplo() {}\n")
            .unwrap();
        let original = state.workbench.active_bytes().unwrap();
        let frame = state.outline_presentation().unwrap();
        let close = frame.close;
        state.outline.hits = Some(frame.hits());
        state.width += 100;
        state.outline_click(close.left + 2, close.top + 2, false);
        assert!(state.outline.visible);
        assert_eq!(state.workbench.active_bytes().unwrap(), original);
    }

    #[test]
    fn sidebar_symbols_navigate_and_old_hits_cannot_jump_after_edit() {
        let mut state = State::new(&GuiOptions::default()).unwrap();
        state.width = 1200;
        state.height = 800;
        state.explorer_visible = true;
        state.workbench.new_file("outline.p", TextEncoding::Utf8);
        let source = "PROCEDURE alpha:\r\nEND PROCEDURE.\r\nPROCEDURE beta:\r\nEND PROCEDURE.";
        state.workbench.insert_text(source).unwrap();
        assert!(state.outline_presentation().unwrap().rows.is_empty());
        assert_eq!(
            state.jobs.outstanding(),
            0,
            "painting must not start analysis"
        );
        finish(&mut state);
        select_tree_row(&mut state, |row| row.name == "beta");
        click_tree_selection(&mut state, true);
        let rect = region(&state).unwrap();
        let x = rect.left + 24;
        assert_eq!(
            state.workbench.active_editor().unwrap().cursor,
            source.find("beta").unwrap()
        );
        assert_eq!(state.workbench.active_bytes().unwrap(), source.as_bytes());

        state.workbench.insert_text("x").unwrap();
        let after_edit = state.workbench.active_editor().unwrap().cursor;
        assert!(state.explorer_click(x, rect.top + HEAD + 2, true));
        assert_eq!(state.workbench.active_editor().unwrap().cursor, after_edit);
        finish(&mut state);
        assert!(state
            .outline
            .tree
            .nodes
            .iter()
            .any(|row| row.name == "xbeta"));
    }

    #[test]
    fn outline_merges_dependencies_with_declarations_and_keeps_call_tabs() {
        let mut state = State::new(&GuiOptions::default()).unwrap();
        state.width = 1400;
        state.height = 900;
        state.workbench.new_file("arbol.p", TextEncoding::Utf8);
        let source = concat!(
            "{modulos/Zeta.i}\n{modulos/Alfa.i}\n",
            "DEFINE VARIABLE zeta AS INTEGER NO-UNDO.\n",
            "DEFINE VARIABLE alfa AS INTEGER NO-UNDO.\n",
            "FUNCTION Ejemplo RETURNS INTEGER () FORWARD.\n",
            "FUNCTION Ejemplo RETURNS INTEGER ():\n  RETURN 0.\nEND FUNCTION.\n",
            "RUN principal.\nRUN PRINCIPAL.\nPROCEDURE principal:\nEND PROCEDURE.\n",
        );
        state.workbench.insert_text(source).unwrap();
        finish(&mut state);
        let frame = state.outline_presentation().unwrap();
        assert_eq!(
            frame
                .rows
                .iter()
                .map(|row| row.label.as_str())
                .collect::<Vec<_>>(),
            [
                "Funciones",
                "Inclusiones",
                "Procedimientos",
                "Prototipos de función",
                "Variables y constantes",
            ]
        );
        let tree = &state.outline.tree;
        let folder = tree
            .nodes
            .iter()
            .find(|node| node.name == "modulos")
            .unwrap();
        assert_eq!(
            folder
                .children
                .iter()
                .map(|&id| tree.nodes[id].name.as_str())
                .collect::<Vec<_>>(),
            ["Alfa.i", "Zeta.i"]
        );
        assert!(folder
            .children
            .iter()
            .all(|&id| matches!(tree.nodes[id].item, Some(Item::Dependency(_)))));

        let (calls, _) = tools(frame.rect, state.dpi)
            .into_iter()
            .find(|(_, mode)| *mode == SourceMode::Calls)
            .unwrap();
        state.outline_click(calls.left + 2, calls.top + 2, false);
        finish(&mut state);
        assert!(state.outline.relations_view);
        assert_eq!(state.outline.relationships[0].state(), SourceState::Ready);
        assert_eq!(state.outline.relationships[0].items().len(), 1);
        assert_eq!(state.workbench.active_bytes().unwrap(), source.as_bytes());
    }

    #[test]
    fn outline_content_opens_an_include_and_a_project_caller_with_double_click() {
        use std::fs;
        let temp = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../work/test-temp");
        fs::create_dir_all(&temp).unwrap();
        let fixture = tempfile::tempdir_in(temp.canonicalize().unwrap()).unwrap();
        let smoke_file = fixture.path().join("fixture.txt");
        fs::write(&smoke_file, b"alpha\r\nbeta\r\n").unwrap();
        let project = fixture.path().join("erp");
        for folder in [".cide", "ventas", "incluidos"] {
            fs::create_dir_all(project.join(folder)).unwrap();
        }
        fs::write(project.join(".cide/propath"), ".\n").unwrap();
        let target = project.join("ventas/destino.p");
        let include = project.join("incluidos/datos.i");
        let caller = project.join("origen.p");
        fs::write(
            &target,
            "/* destino */\n{incluidos/ausente.i}\n{incluidos/datos.i}\nRUN ventas/auxiliar.p.\n",
        )
        .unwrap();
        fs::write(&include, "DEFINE VARIABLE dato AS INTEGER NO-UNDO.\n").unwrap();
        fs::write(project.join("ventas/auxiliar.p"), "RETURN.\n").unwrap();
        fs::write(
            &caller,
            "PROCEDURE Iniciar:\n  RUN ventas/destino.p.\nEND PROCEDURE.\n",
        )
        .unwrap();
        crate::windows::run_with_smoke(
            GuiOptions {
                project_root: Some(fixture.path().to_owned()),
                smoke: Some(crate::SmokeOptions {
                    fixture: smoke_file,
                    report: fixture.path().join("outline.json"),
                    ..crate::SmokeOptions::default()
                }),
                ..GuiOptions::default()
            },
            exercise_outline_window,
        )
        .unwrap();
    }

    unsafe fn exercise_outline_window(
        hwnd: crate::windows::ffi::Hwnd,
        options: &crate::SmokeOptions,
    ) -> Result<(), String> {
        use crate::windows::{explorer_smoke::OwnSize, ffi::*, owner};
        let gui = owner(hwnd).ok_or("No se creó la ventana de Esquema")?;
        let dpi = gui.state.borrow().dpi;
        let size = OwnSize::new(hwnd)?;
        size.resize(dpi.pixels(1400), dpi.pixels(900))?;
        gui.state.borrow_mut().set_outline_visible(true);
        let target = options
            .fixture
            .with_file_name("erp")
            .join("ventas/destino.p");
        let prepare = |open: bool| -> Result<(), String> {
            let mut state = gui.state.borrow_mut();
            if open {
                state
                    .workbench
                    .open_file(&target, Some(TextEncoding::Utf8))
                    .map_err(|error| error.to_string())?;
            }
            finish(&mut state);
            gui.frame_dirty.set(true);
            Ok(())
        };
        let click = |label: &str, double: bool| -> Result<(), String> {
            InvalidateRect(hwnd, std::ptr::null(), 0);
            SendMessageW(hwnd, WM_PAINT, 0, 0);
            let state = gui.state.borrow();
            let frame = state.outline_presentation().ok_or("Esquema oculto")?;
            let row = frame
                .rows
                .iter()
                .position(|row| row.label.starts_with(label))
                .ok_or_else(|| format!("No aparece {label} en Esquema"))?;
            let x = frame.rect.left + state.dpi.pixels(50 + 12 * frame.rows[row].depth as i32);
            let y =
                frame.rect.top + state.dpi.pixels(HEAD) + row as i32 * state.dpi.pixels(ROW) + 2;
            drop(state);
            let coordinates = ((y as isize) << 16) | x as isize;
            SendMessageW(hwnd, WM_LBUTTONDOWN, 1, coordinates);
            SendMessageW(hwnd, WM_LBUTTONUP, 0, coordinates);
            if double {
                SendMessageW(hwnd, WM_LBUTTONDBLCLK, 1, coordinates);
                SendMessageW(hwnd, WM_LBUTTONUP, 0, coordinates);
            }
            Ok(())
        };
        let at = |filename: &str, statement: Option<&str>| -> Result<(), String> {
            let state = gui.state.borrow();
            let active = state
                .workbench
                .active_editor()
                .ok_or("Documento activo ausente")?;
            let source = state
                .workbench
                .active_bytes()
                .ok_or("Fuente activa ausente")?;
            if active
                .session
                .path()
                .file_name()
                .is_none_or(|name| name != filename)
                || statement
                    .is_some_and(|code| !source[active.cursor..].starts_with(code.as_bytes()))
            {
                return Err(format!(
                    "Esquema no abrió {filename} en el destino esperado: {}",
                    state.status
                ));
            }
            Ok(())
        };
        prepare(true)?;
        click("Inclusiones", false)?;
        click("incluidos", false)?;
        click("ausente.i", true)?;
        at("destino.p", Some("{incluidos/ausente.i}"))?;
        prepare(false)?;
        click("datos.i", true)?;
        at("datos.i", None)?;
        prepare(true)?;
        let tab = |mode| -> Result<(), String> {
            let state = gui.state.borrow();
            let rect = region(&state).ok_or("Esquema oculto")?;
            let (area, _) = tools(rect, state.dpi)
                .into_iter()
                .find(|(_, tool)| *tool == mode)
                .ok_or("Pestaña de relaciones ausente")?;
            let coordinates = (((area.top + 2) as isize) << 16) | (area.left + 2) as isize;
            drop(state);
            SendMessageW(hwnd, WM_LBUTTONDOWN, 1, coordinates);
            SendMessageW(hwnd, WM_LBUTTONUP, 0, coordinates);
            Ok(())
        };
        tab(SourceMode::Calls)?;
        prepare(false)?;
        click("Llama a", false)?;
        click("ventas", false)?;
        click("auxiliar.p", true)?;
        at("auxiliar.p", None)?;
        prepare(true)?;
        tab(SourceMode::Calls)?;
        prepare(false)?;
        click("Llamado por", false)?;
        click("Iniciar", true)?;
        at("origen.p", Some("RUN ventas/destino.p"))?;

        use crate::windows::smoke::{key, position};
        let wait_source = || -> Result<(), String> {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let mut state = gui.state.borrow_mut();
                state.poll_tasks();
                if state.jobs.outstanding() == 0 {
                    return Ok(());
                }
                if Instant::now() >= deadline {
                    return Err(state.status.clone());
                }
                drop(state);
                std::thread::yield_now();
            }
        };
        position(hwnd, 1, 6)?;
        key(hwnd, 0x0d, Modifiers::CTRL)?;
        wait_source()?;
        at("destino.p", None)?;
        key(hwnd, 0x25, Modifiers::CTRL)?;
        at("origen.p", Some("ventas/destino.p"))?;
        key(hwnd, 0x27, Modifiers::CTRL)?;
        at("destino.p", None)?;
        position(hwnd, 3, 4)?;
        let (x, y) = {
            let state = gui.state.borrow();
            paint::rectangle_pixel(&state, state.workbench.panes().active(), 3, 4)
                .ok_or("No se encontró la línea RUN en la GUI")?
        };
        let coordinates = ((y as isize) << 16) | x as isize;
        SendMessageW(hwnd, WM_LBUTTONDOWN, 1, coordinates);
        SendMessageW(hwnd, WM_LBUTTONUP, 0, coordinates);
        SendMessageW(hwnd, WM_LBUTTONDBLCLK, 1, coordinates);
        wait_source()?;
        at("auxiliar.p", None)?;
        SendMessageW(hwnd, WM_CHAR, ' ' as usize, 0);
        key(hwnd, 0x25, Modifiers::CTRL)?;
        key(hwnd, usize::from(b'Q'), Modifiers::CTRL)?;
        at("auxiliar.p", Some(" RETURN."))
    }

    #[test]
    fn outline_value_type_icons_follow_abl_declarations_in_both_symbol_providers() {
        for root in [None, Some(std::env::current_dir().unwrap())] {
            let path = root
                .as_ref()
                .map_or_else(|| PathBuf::from("tipos.p"), |root| root.join("tipos.p"));
            let mut state = State::new(&GuiOptions {
                project_root: root,
                ..GuiOptions::default()
            })
            .unwrap();
            state.width = 1400;
            state.height = 900;
            state.workbench.new_file(path, TextEncoding::Utf8);
            state
                .workbench
                .insert_text(concat!(
                    "DEFINE VARIABLE cFecha AS DATE NO-UNDO.\n",
                    "DEFINE VARIABLE copiado LIKE cFecha NO-UNDO.\n",
                    "DEFINE VARIABLE importe AS DECIMAL NO-UNDO.\n",
                    "DEFINE VARIABLE nombre AS CHARACTER NO-UNDO.\n",
                    "DEFINE VARIABLE numero AS INT64 NO-UNDO.\n",
                    "DEFINE VARIABLE permiso AS LOGICAL NO-UNDO.\n",
                ))
                .unwrap();
            finish(&mut state);
            assert_eq!(state.outline_count(), 1);
            let frame = state.outline_presentation().unwrap();
            assert!(frame.rows[0].folder);
            assert!(!frame.rows[0].has_value_icon());
            state.outline_key(0x27, Modifiers::NONE);
            let frame = state.outline_presentation().unwrap();
            let variables = &frame.rows[1..7];
            assert!(variables.iter().all(Row::has_value_icon));
            assert_eq!(
                variables
                    .iter()
                    .map(|row| (row.label.as_str(), row.value_type))
                    .collect::<Vec<_>>(),
                [
                    ("cFecha", Some(ValueType::Date)),
                    ("copiado", None),
                    ("importe", Some(ValueType::Decimal)),
                    ("nombre", Some(ValueType::Text)),
                    ("numero", Some(ValueType::Integer)),
                    ("permiso", Some(ValueType::Boolean)),
                ]
            );
        }
    }

    #[test]
    fn rust_outline_opens_the_real_document_rs_and_nests_its_members() {
        let file =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../jc-core/src/document.rs");
        let bytes = std::fs::read(&file).unwrap();
        let mut state = State::new(&GuiOptions::default()).unwrap();
        state.width = 1400;
        state.height = 900;
        state
            .workbench
            .open_file(&file, Some(TextEncoding::Utf8))
            .unwrap();
        finish(&mut state);
        assert_eq!(
            state.outline.controller.state(),
            SourceState::Ready,
            "{}",
            state.outline.controller.message()
        );
        assert!(state.outline.controller.items().len() > 64);
        let tree = &state.outline.tree;
        let node = |name| tree.nodes.iter().find(|node| node.name == name).unwrap();
        let enclosing = |child: &tree::Node| {
            let category = &tree.nodes[child.parent.unwrap()];
            tree.nodes[category.parent.unwrap()].name.as_str()
        };
        assert_eq!(enclosing(node("range")), "Replacement");
        assert_eq!(enclosing(node("map_offset")), "ChangeSet");
        let item = state
            .outline
            .controller
            .items()
            .iter()
            .position(|item| item.label == "map_offset · Método")
            .unwrap();
        state
            .outline
            .controller
            .activate_at_index(&mut state.workbench, None, item, false)
            .unwrap();
        let caret = state.workbench.active_editor().unwrap().cursor;
        assert!(bytes[caret..].starts_with(b"map_offset"));
        assert_eq!(state.workbench.active_bytes().unwrap(), bytes);
    }

    #[test]
    fn outline_nests_abl_members_and_supports_keyboard_folders() {
        let root = std::env::current_dir().unwrap();
        let mut state = State::new(&GuiOptions {
            project_root: Some(root.clone()),
            ..GuiOptions::default()
        })
        .unwrap();
        state.width = 1400;
        state.height = 900;
        state
            .workbench
            .new_file(root.join("EsquemaDemo.cls"), TextEncoding::Utf8);
        let source = concat!(
            "CLASS EsquemaDemo:\n",
            "  DEFINE TEMP-TABLE ttDatos NO-UNDO FIELD zeta AS INTEGER FIELD alfa AS INTEGER ",
            "INDEX porAlfa alfa.\n",
            "  METHOD PUBLIC VOID Ejecutar():\n",
            "    DEFINE VARIABLE local AS INTEGER NO-UNDO.\n",
            "  END METHOD.\n",
            "END CLASS.\n",
        );
        state.workbench.insert_text(source).unwrap();
        finish(&mut state);
        let tree = &state.outline.tree;
        let children = |parent: usize| {
            tree.nodes[parent]
                .children
                .iter()
                .map(|&id| tree.nodes[id].name.as_str())
                .collect::<Vec<_>>()
        };
        let table = tree
            .nodes
            .iter()
            .position(|node| node.name == "ttDatos")
            .unwrap();
        assert_eq!(children(table), ["Campos", "Índices"]);
        assert_eq!(children(tree.nodes[table].children[0]), ["alfa", "zeta"]);
        assert_eq!(children(tree.nodes[table].children[1]), ["porAlfa"]);
        let method = tree
            .nodes
            .iter()
            .position(|node| node.name == "Ejecutar")
            .unwrap();
        let method_group = tree.nodes[method].parent.unwrap();
        let class = tree.nodes[method_group].parent.unwrap();
        assert_eq!(tree.nodes[class].name, "EsquemaDemo");
        assert_eq!(children(method), ["Variables y constantes"]);
        assert_eq!(tree.rows.len(), 1);
        state.outline_key(0x27, Modifiers::NONE);
        assert_eq!(state.outline_count(), 2);
        state.outline_key(0x27, Modifiers::NONE);
        state.outline_key(0x27, Modifiers::NONE);
        assert_eq!(state.outline_count(), 4);
        state.outline_key(0x25, Modifiers::NONE);
        assert_eq!(state.outline_count(), 2);
        assert_eq!(state.workbench.active_bytes().unwrap(), source.as_bytes());
    }

    #[test]
    fn right_code_dependencies_open_to_side_across_languages() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("work/test-temp")
            .join(format!(
                "right-code-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        std::fs::create_dir_all(root.join("inc")).unwrap();
        std::fs::create_dir_all(root.join(".cide")).unwrap();
        std::fs::write(root.join(".cide/propath"), b"inc\n").unwrap();
        std::fs::write(root.join(".cide/source-roots"), b".\nsources\n").unwrap();
        for (name, source, target) in [
            ("main.p", "{common.i}\r\n", "inc/common.i"),
            (
                "main.js",
                "import './other.js';\nfunction ejemplo() {}\n",
                "other.js",
            ),
            (
                "main.cjs",
                "const dato = require('./common');\n",
                "common/index.js",
            ),
            (
                "main.ts",
                "import type { Model } from './model.js';\ninterface Consulta {}\n",
                "model.ts",
            ),
            (
                "main.php",
                "<?php require __DIR__ . '/vista.php';\nfunction ejemplo() {}\n",
                "vista.php",
            ),
            (
                "Main.java",
                "import modelo.Item;\nclass Main { void ejemplo() {} }\n",
                "sources/modelo/Item.java",
            ),
            (
                "Main.cs",
                "using static App.Util;\nclass Main { void ejemplo() {} }\n",
                "sources/App/Util.cs",
            ),
        ] {
            let path = root.join(name);
            let destination = root.join(target);
            std::fs::write(&path, source).unwrap();
            std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
            std::fs::write(&destination, b"// destino\n").unwrap();
            let mut s = State::new(&GuiOptions::default()).unwrap();
            s.width = 1400;
            s.height = 900;
            s.set_project_root(root.clone()).unwrap();
            s.workbench
                .open_file(&path, Some(TextEncoding::Utf8))
                .unwrap();
            let tab = s.workbench.tabs().active_id().unwrap();
            assert_eq!(
                s.workbench
                    .syntax_rows(tab, 0, 1)
                    .kind_at(usize::from(name.ends_with(".php")) * 6),
                match name {
                    "main.p" => jc_app::SyntaxKind::Preprocessor,
                    _ => jc_app::SyntaxKind::Keyword,
                }
            );
            finish(&mut s);
            select_tree_row(&mut s, |node| {
                matches!(node.item, Some(Item::Dependency(_)))
            });
            assert!(s.outline_ready(), "{}", s.outline.dependencies.message());
            s.activate_outline(true);
            assert!(s.workbench.panes().is_split());
            assert_eq!(
                s.workbench.active_document().unwrap().session.path(),
                destination.canonicalize().unwrap()
            );
            assert_eq!(std::fs::read(&path).unwrap(), source.as_bytes());
            assert_eq!(s.workbench.active_bytes().unwrap(), b"// destino\n");
        }
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn right_code_symbol_side_open_shares_document_and_keeps_origin_caret() {
        let mut s = State::new(&GuiOptions::default()).unwrap();
        s.workbench = jc_app::Workbench::default();
        s.width = 1600;
        s.height = 900;
        s.workbench.new_file("simbolos.p", TextEncoding::Utf8);
        let source = "PROCEDURE uno:\nEND PROCEDURE.\nPROCEDURE dos:\nEND PROCEDURE.\n";
        s.workbench.insert_text(source).unwrap();
        s.workbench.move_home(false).unwrap();
        let original = s.workbench.active_editor().unwrap().cursor;
        finish(&mut s);
        select_tree_row(&mut s, |row| row.name == "dos");
        s.activate_outline(true);
        assert!(s.workbench.panes().is_split());
        assert_eq!(s.workbench.panes().active(), jc_app::PaneId::Secondary);
        assert_eq!(
            s.workbench.active_editor().unwrap().cursor,
            source.find("dos").unwrap()
        );
        assert_eq!(
            s.workbench
                .panes()
                .view(jc_app::PaneId::Primary)
                .unwrap()
                .state()
                .unwrap()
                .cursor,
            original
        );
        assert_eq!(s.workbench.tabs().tabs().len(), 1);
        assert_eq!(s.workbench.active_bytes().unwrap(), source.as_bytes());
        s.workbench.undo().unwrap();
        assert!(s.workbench.active_bytes().unwrap().is_empty());
    }
}
