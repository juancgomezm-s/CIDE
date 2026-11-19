//! Native view of the shared immutable comparison. No source buffer or Git I/O.
use super::{ffi::*, paint, wide, Action, State};
use jc_app::{
    diff_model::{DiffLimits, DiffSide, DiffStatus, OwnedDiffModel},
    open_diff::OpenDiffState,
    open_diff_view::{OpenDiffPresentation, OpenDiffRowKind},
    source_services::display_ui_label,
    CommandId, Modifiers, TabId,
};
use std::{path::PathBuf, sync::Arc};

#[derive(Debug)]
pub(super) struct OmniCorePreview {
    model: OwnedDiffModel,
    grouped: bool,
}

impl OmniCorePreview {
    /// OmniCore supplies immutable text for display. Compute on its worker.
    pub(super) fn compute(before: String, after: String) -> Result<Self, String> {
        let (model, grouped) = OwnedDiffModel::compute_display(
            Arc::new(before.into_bytes()),
            Arc::new(after.into_bytes()),
            DiffLimits {
                max_input_bytes: 1024 * 1024,
                max_lines: 50_000,
                max_work: 16 * 1024 * 1024,
                max_metadata_bytes: 4 * 1024 * 1024,
                max_hunks: 4096,
            },
            || false,
        );
        match model.status() {
            DiffStatus::Exact => Ok(Self { model, grouped }),
            DiffStatus::Limited(limit) => Err(format!(
                "La comparación de OmniCore supera el límite de {}",
                limit.label()
            )),
            DiffStatus::Cancelled => Err("Se canceló la comparación de OmniCore".into()),
        }
    }
}

pub(super) struct OmniCoreComparison {
    tab: Option<TabId>,
    path: PathBuf,
    preview: OmniCorePreview,
    selected: usize,
    wheel_remainder: i32,
}

pub(super) fn is_command(c: CommandId) -> bool {
    matches!(
        c,
        CommandId::DiffCompareOpen
            | CommandId::DiffRefreshOpen
            | CommandId::DiffCancelOpen
            | CommandId::DiffOpenNavigateLeft
            | CommandId::DiffOpenNavigateRight
    )
}
pub(super) struct ComparisonFrame {
    pub presentation: OpenDiffPresentation,
    geometry: Geometry,
    origins: Option<[String; 2]>,
    summary: String,
    omnicore: bool,
}
#[derive(Clone, Copy)]
struct Geometry {
    rect: Rect,
    cw: i32,
    ch: i32,
    columns: usize,
    rows: usize,
    dual: bool,
}
impl Geometry {
    fn new(width: i32, height: (i32, i32), cw: i32, ch: i32) -> Self {
        let width = width.clamp(1, 8192);
        let (height, tabs_bottom) = height;
        let height = height.clamp(1, 8192);
        let cw = cw.clamp(1, 8192);
        let ch = ch.clamp(1, 8192);
        let rect = Rect {
            left: 0,
            top: tabs_bottom.min(height),
            right: width,
            bottom: (height - ch - 6).max(tabs_bottom.min(height)),
        };
        Self::in_rect(rect, cw, ch)
    }
    fn in_rect(rect: Rect, cw: i32, ch: i32) -> Self {
        let cw = cw.clamp(1, 8192);
        let ch = ch.clamp(1, 8192);
        let width = rect.right.saturating_sub(rect.left);
        let columns = ((width - 16).max(0) / cw).clamp(0, 256) as usize;
        let rows = ((rect.bottom - rect.top).max(0) / ch).clamp(0, 100) as usize;
        Self {
            rect,
            cw,
            ch,
            columns,
            rows,
            dual: columns >= 90,
        }
    }
    fn for_omnicore(state: &State) -> Self {
        let (left, right) = super::outline_panel::editor_horizontal(state);
        let tabs_bottom = state.dpi.pixels(super::shell::TABS);
        let bottom = super::bottom_panel::region(state)
            .map_or(state.height - state.cell_height - 8, |r| r.top)
            .max(0);
        Self::in_rect(
            Rect {
                left,
                right,
                top: tabs_bottom.min(bottom),
                bottom,
            },
            state.cell_width,
            state.cell_height,
        )
    }
    fn row(self, index: usize) -> Rect {
        let top = (self.rect.top + index.min(100) as i32 * self.ch).min(self.rect.bottom);
        Rect {
            left: (self.rect.left + 8).min(self.rect.right),
            top,
            right: self.rect.right,
            bottom: (top + self.ch).min(self.rect.bottom),
        }
    }
    fn split(self) -> i32 {
        self.row(0).left + ((self.columns - 3) / 2) as i32 * self.cw
    }
    fn close(self) -> Rect {
        let header = self.row(0);
        Rect {
            left: (header.right - (self.cw * 3).max(24)).max(header.left),
            ..header
        }
    }
}

fn inside(rect: Rect, x: i32, y: i32) -> bool {
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
}

impl State {
    pub(super) fn open_omnicore_diff(&mut self, path: PathBuf, preview: OmniCorePreview) {
        self.open_diff.clear(&mut self.jobs);
        self.open_diff_view.first();
        self.omnicore_diff = Some(OmniCoreComparison {
            tab: self.workbench.active_document().map(|doc| doc.tab_id),
            path,
            preview,
            selected: 0,
            wheel_remainder: 0,
        });
        self.explorer_focused = false;
        self.outline.focused = false;
        self.bottom.focused = false;
        self.open_diff_visible = true;
        self.status = "Comparación de OmniCore".into();
    }

    pub(super) fn clear_omnicore_diff(&mut self) -> bool {
        if self.omnicore_diff.take().is_none() {
            return false;
        }
        self.open_diff_visible = false;
        true
    }

    fn close_omnicore_diff(&mut self) -> bool {
        let closed = self.clear_omnicore_diff();
        if closed {
            self.status = "Comparación de OmniCore cerrada".into();
        }
        closed
    }

    pub(super) fn close_omnicore_diff_for(&mut self, tab: TabId) -> bool {
        if self.open_diff_visible
            && self
                .omnicore_diff
                .as_ref()
                .is_some_and(|diff| diff.tab == Some(tab))
        {
            self.close_omnicore_diff()
        } else {
            false
        }
    }

    pub(super) fn omnicore_diff_click(&mut self, x: i32, y: i32) -> bool {
        if !self.open_diff_visible || self.omnicore_diff.is_none() {
            return false;
        }
        let geometry = Geometry::for_omnicore(self);
        if geometry.rows > 0 && inside(geometry.close(), x, y) {
            self.close_omnicore_diff()
        } else {
            inside(geometry.rect, x, y)
        }
    }

    pub(super) fn omnicore_diff_wheel(&mut self, x: i32, y: i32, delta: i32) -> bool {
        if !self.open_diff_visible
            || self.omnicore_diff.is_none()
            || !inside(Geometry::for_omnicore(self).rect, x, y)
        {
            return false;
        }
        let Some(diff) = &mut self.omnicore_diff else {
            return false;
        };
        let pending = diff.wheel_remainder.saturating_add(delta);
        diff.wheel_remainder = pending % 120;
        self.open_diff_view.scroll(-((pending / 120) as isize) * 3);
        true
    }

    fn omnicore_diff_key(&mut self, code: usize, mods: Modifiers) -> Action {
        if code == 0x1b || (code == b'W' as usize && mods == Modifiers::CTRL) {
            self.close_omnicore_diff();
            return Action::None;
        }
        if mods != Modifiers::NONE {
            return Action::None;
        }
        let geometry = Geometry::for_omnicore(self);
        let Some(diff) = &mut self.omnicore_diff else {
            return Action::None;
        };
        match code {
            0x26 | 0x28 => {
                let last = diff.preview.model.hunks().count().saturating_sub(1);
                let selected = diff
                    .selected
                    .saturating_add_signed(if code == 0x26 { -1 } else { 1 })
                    .min(last);
                if selected != diff.selected {
                    diff.selected = selected;
                    self.open_diff_view.first();
                }
            }
            0x21 | 0x22 => {
                let page = geometry
                    .rows
                    .saturating_sub(if geometry.dual { 5 } else { 6 })
                    .saturating_sub(1)
                    .max(1) as isize;
                self.open_diff_view
                    .scroll(if code == 0x21 { -page } else { page });
            }
            0x24 => self.open_diff_view.first(),
            0x23 => self.open_diff_view.last(),
            _ => {}
        }
        Action::None
    }

    pub(super) fn open_diff_context(&self, context: &mut jc_app::CommandContext) {
        context.open_diff_active = !matches!(
            self.open_diff.state(),
            OpenDiffState::Idle | OpenDiffState::Cancelled
        );
        context.open_diff_ready = self.open_diff.state() == OpenDiffState::Ready;
        if self.open_diff_visible {
            context.editor_input = false;
        }
    }
    pub(super) fn validate_open_diff(&mut self) -> bool {
        if !self.open_diff_visible {
            self.omnicore_diff = None;
        } else if self
            .omnicore_diff
            .as_ref()
            .is_some_and(|diff| diff.tab != self.workbench.active_document().map(|doc| doc.tab_id))
        {
            return self.clear_omnicore_diff();
        }
        self.open_diff
            .validate_context(&self.workbench, &mut self.jobs)
    }
    pub(super) fn open_diff_command(&mut self, command: CommandId) -> Action {
        self.clear_omnicore_diff();
        match command {
            CommandId::DiffCompareOpen | CommandId::DiffRefreshOpen => {
                self.close_selector();
                self.bottom.focused = false;
                self.explorer_focused = false;
                self.open_diff_visible = true;
                self.open_diff_view.first();
                match self.open_diff.request(&self.workbench, &mut self.jobs) {
                    Ok(()) => self.status = self.open_diff.message().into(),
                    Err(e) => self.error(e),
                }
            }
            CommandId::DiffCancelOpen => {
                self.open_diff.cancel(&mut self.jobs);
                self.open_diff_visible = false;
                self.status = "Comparación de abiertos cancelada; fuente sin cambios".into();
            }
            CommandId::DiffOpenNavigateLeft | CommandId::DiffOpenNavigateRight => {
                let side = if command == CommandId::DiffOpenNavigateLeft {
                    DiffSide::Left
                } else {
                    DiffSide::Right
                };
                match self.open_diff.select_hunk(
                    self.open_diff.selected(),
                    side,
                    &mut self.workbench,
                ) {
                    Ok(()) => {
                        self.close_selector();
                        self.open_diff_visible = false;
                        self.bottom.focused = false;
                        self.explorer_focused = false;
                        self.status =
                            "Bloque seleccionado en el grupo original; fuente sin cambios".into();
                    }
                    Err(e) => self.error(e),
                }
            }
            _ => {}
        }
        Action::None
    }
    pub(super) fn open_diff_key(&mut self, code: usize, mods: Modifiers) -> Action {
        self.validate_open_diff();
        if self.omnicore_diff.is_some() {
            return self.omnicore_diff_key(code, mods);
        }
        let command = match (code, mods) {
            (0x1b, _) => Some(CommandId::DiffCancelOpen),
            (0x74, Modifiers::NONE) => Some(CommandId::DiffRefreshOpen),
            (0x0d, Modifiers::NONE) => Some(CommandId::DiffOpenNavigateRight),
            (0x0d, Modifiers::SHIFT) => Some(CommandId::DiffOpenNavigateLeft),
            _ => None,
        };
        if let Some(command) = command {
            return self.execute(command);
        }
        if mods == Modifiers::NONE {
            match code {
                0x26 | 0x28 => {
                    self.open_diff
                        .select_relative(if code == 0x26 { -1 } else { 1 });
                    self.open_diff_view.first();
                }
                0x21 | 0x22 => {
                    let g = Geometry::new(
                        self.width,
                        (self.height, self.dpi.pixels(super::shell::TABS)),
                        self.cell_width,
                        self.cell_height,
                    );
                    self.open_diff_view.scroll(if code == 0x21 {
                        -(g.rows.saturating_sub(4) as isize)
                    } else {
                        g.rows.saturating_sub(4) as isize
                    });
                }
                0x24 => self.open_diff_view.first(),
                0x23 => self.open_diff_view.last(),
                _ => {}
            }
        }
        Action::None
    }
    pub(super) fn open_diff_presentation(&mut self) -> Option<ComparisonFrame> {
        self.validate_open_diff();
        if !self.open_diff_visible {
            return None;
        }
        if let Some(diff) = &self.omnicore_diff {
            let geometry = Geometry::for_omnicore(self);
            let mut presentation = self.open_diff_view.prepare_utf8_comparison(
                &diff.preview.model,
                diff.selected,
                geometry
                    .rows
                    .saturating_sub(if geometry.dual { 5 } else { 6 }),
                geometry.columns,
                geometry.dual,
            );
            presentation.title = display_ui_label(
                &format!(
                    "OmniCore · {}",
                    diff.path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("Archivo")
                ),
                geometry.columns.saturating_sub(4),
            );
            let summary = format!(
                "{} {}/{}",
                if diff.preview.grouped {
                    "Cambio agrupado · bloque"
                } else {
                    "Bloque"
                },
                usize::from(presentation.total > 0) + presentation.selected,
                presentation.total,
            );
            return Some(ComparisonFrame {
                presentation,
                geometry,
                origins: Some(["Antes".into(), "Después".into()]),
                summary,
                omnicore: true,
            });
        }
        let geometry = Geometry::new(
            self.width,
            (self.height, self.dpi.pixels(super::shell::TABS)),
            self.cell_width,
            self.cell_height,
        );
        let origins = self.open_diff.result().map(|result| {
            [&result.left, &result.right].map(|origin| {
                jc_app::source_services::display_ui_label(
                    &format!(
                        "{} [{}{}]",
                        origin
                            .path
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("[ruta no Unicode]"),
                        origin.stamp.encoding.name(),
                        if origin.modified { " *" } else { "" }
                    ),
                    geometry.columns / 2,
                )
            })
        });
        let total = self
            .open_diff
            .result()
            .map_or(0, |r| r.model.hunks().count());
        let summary = format!(
            "{} · bloque {}/{}",
            self.open_diff.state().label(),
            usize::from(total > 0) + self.open_diff.selected(),
            total
        );
        Some(ComparisonFrame {
            presentation: self.open_diff_view.prepare(
                &self.open_diff,
                geometry
                    .rows
                    .saturating_sub(if geometry.dual { 5 } else { 6 }),
                geometry.columns,
                geometry.dual,
            ),
            geometry,
            origins,
            summary,
            omnicore: false,
        })
    }
}
pub(super) unsafe fn draw(dc: Hdc, frame: &ComparisonFrame) -> Result<(), String> {
    let g = frame.geometry;
    let p = &frame.presentation;
    const BG: u32 = super::shell::BACKGROUND;
    const HEADER: u32 = super::shell::SURFACE;
    const FG: u32 = 0x00d5d7d8;
    const DIM: u32 = 0x008d8b87;
    paint::fill(dc, g.rect, BG)?;
    let line =
        |row: usize, text: &str, fg: u32, bg: u32| paint::text(dc, g.row(row), &wide(text), fg, bg);
    if g.rows >= 1 {
        if frame.omnicore {
            let close = g.close();
            paint::text(
                dc,
                Rect {
                    right: close.left,
                    ..g.row(0)
                },
                &wide(&p.title),
                FG,
                HEADER,
            )?;
            paint::text(dc, close, &wide("×"), FG, HEADER)?;
        } else {
            line(0, &p.title, FG, HEADER)?;
        }
    }
    let first = if let Some([left, right]) = &frame.origins {
        let left = if frame.omnicore {
            left.clone()
        } else {
            format!("I: {left}")
        };
        let right = if frame.omnicore {
            right.clone()
        } else {
            format!("D: {right}")
        };
        if g.dual {
            if g.rows >= 2 {
                let rect = g.row(1);
                let split = g.split();
                paint::text(
                    dc,
                    Rect {
                        right: split,
                        ..rect
                    },
                    &wide(&left),
                    FG,
                    HEADER,
                )?;
                paint::text(
                    dc,
                    Rect {
                        left: split,
                        ..rect
                    },
                    &wide(&right),
                    FG,
                    HEADER,
                )?;
            }
            if g.rows >= 3 {
                line(2, &frame.summary, DIM, HEADER)?;
            }
            3
        } else {
            if g.rows >= 2 {
                line(1, &left, FG, HEADER)?;
            }
            if g.rows >= 3 {
                line(2, &right, FG, HEADER)?;
            }
            if g.rows >= 4 {
                line(3, &frame.summary, DIM, HEADER)?;
            }
            4
        }
    } else {
        if g.rows >= 2 {
            line(1, &p.message, DIM, HEADER)?;
        }
        2
    };
    for (i, row) in p
        .rows
        .iter()
        .take(g.rows.saturating_sub(first + 2))
        .enumerate()
    {
        let rect = g.row(i + first);
        let bg = match row.kind {
            OpenDiffRowKind::Removed => 0x00313549,
            OpenDiffRowKind::Added => 0x00364635,
            OpenDiffRowKind::Header => 0x003b3632,
            _ => BG,
        };
        if row.kind == OpenDiffRowKind::Changed && g.dual {
            let split = g.split();
            let (left, right) = row
                .label
                .split_at(((g.columns - 3) / 2).min(row.label.len()));
            paint::text(
                dc,
                Rect {
                    right: split,
                    ..rect
                },
                &wide(left),
                FG,
                0x00313549,
            )?;
            paint::text(
                dc,
                Rect {
                    left: split,
                    ..rect
                },
                &wide(right),
                FG,
                0x00364635,
            )?;
        } else {
            line(i + first, &row.label, FG, bg)?;
        }
    }
    if g.rows >= 3 {
        line(
            g.rows - 2,
            if frame.omnicore {
                "Vista previa de solo lectura proporcionada por OmniCore"
            } else {
                "Copias de solo lectura | diferencias de bytes | sin mezcla ni escrituras"
            },
            DIM,
            HEADER,
        )?;
    }
    if g.rows >= 4 {
        line(
            g.rows - 1,
            if frame.omnicore {
                "↑↓ bloque · Pág contenido · Esc cerrar"
            } else {
                "↑↓ bloque | Pág contenido | Intro derecha / Mayús+Intro izquierda | F5 actualizar | Esc"
            },
            DIM,
            HEADER,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omnicore_preview_projects_both_revisions_and_keeps_utf8() {
        let before = "const texto = \"añadir\";\r\n";
        let after = "const texto = \"cambiar\";\n";
        let preview = OmniCorePreview::compute(before.into(), after.into()).unwrap();
        assert_eq!(
            preview.model.source_bytes(DiffSide::Left),
            before.as_bytes()
        );
        assert_eq!(
            preview.model.source_bytes(DiffSide::Right),
            after.as_bytes()
        );
        let mut view = jc_app::open_diff_view::OpenDiffView::default();
        let unified = view.prepare_utf8_comparison(&preview.model, 0, 8, 80, false);
        assert_eq!(unified.total, 1);
        assert!(unified
            .rows
            .iter()
            .any(|row| row.kind == OpenDiffRowKind::Removed
                && row.label.contains(r"a\u{F1}adir")
                && row.label.contains("[CRLF]")));
        assert!(unified
            .rows
            .iter()
            .any(|row| row.kind == OpenDiffRowKind::Added
                && row.label.contains("cambiar")
                && row.label.contains("[LF]")));
        let side_by_side = view.prepare_utf8_comparison(&preview.model, 0, 8, 160, true);
        assert!(side_by_side.rows.iter().any(|row| {
            row.kind == OpenDiffRowKind::Changed
                && row.label.contains(r"a\u{F1}adir")
                && row.label.contains("cambiar")
        }));
    }

    #[test]
    fn omnicore_preview_controls_respect_the_comparison_bounds() {
        let g = Geometry::in_rect(
            Rect {
                left: 300,
                top: 84,
                right: 1300,
                bottom: 600,
            },
            9,
            20,
        );
        assert!(g.dual);
        assert_eq!(g.row(0).left, 308);
        assert!(g.split() > 300 && g.split() < 1300);
        let close = g.close();
        assert!(inside(close, 1290, 88));
        assert!(!inside(close, 1305, 88));
    }

    #[test]
    fn omnicore_preview_close_keeps_the_editable_tab_and_undo() {
        let mut state = State::new(&crate::GuiOptions::default()).unwrap();
        state.width = 1400;
        state.height = 800;
        let path = PathBuf::from("fuente.abl");
        state
            .workbench
            .new_file(path.clone(), jc_text::TextEncoding::Utf8);
        state.workbench.insert_text("edición local").unwrap();
        let tab = state.workbench.active_document().unwrap().tab_id;

        state.open_omnicore_diff(
            path,
            OmniCorePreview::compute("anterior".into(), "posterior".into()).unwrap(),
        );
        assert!(state.open_diff_visible);
        let frame = state.open_diff_presentation().unwrap();
        assert_eq!(
            frame.geometry.rect.top,
            state.dpi.pixels(super::super::shell::TABS)
        );
        assert_eq!(
            state.workbench.active_bytes().unwrap(),
            "edición local".as_bytes()
        );
        assert!(state.close_omnicore_diff_for(tab));
        assert!(!state.open_diff_visible);
        assert_eq!(state.workbench.active_document().unwrap().tab_id, tab);
        state.workbench.execute(CommandId::EditUndo).unwrap();
        assert!(state.workbench.active_bytes().unwrap().is_empty());
    }

    #[test]
    fn comparison_geometry_fits_client_and_switches_to_unified_without_overflow() {
        for (w, h, cw, ch) in [
            (1, 1, 0, 0),
            (360, 240, 9, 20),
            (1400, 800, 9, 20),
            (i32::MAX, i32::MAX, i32::MAX, i32::MAX),
        ] {
            let g = Geometry::new(w, (h, super::super::shell::TABS), cw, ch);
            assert!(g.columns <= 256 && g.rows <= 100);
            for i in 0..g.rows {
                let r = g.row(i);
                assert!(
                    r.left <= r.right
                        && r.top <= r.bottom
                        && r.right <= w.min(8192)
                        && r.bottom <= h.min(8192)
                );
            }
        }
        assert!(!Geometry::new(360, (240, super::super::shell::TABS), 9, 20).dual);
        assert!(Geometry::new(1400, (800, super::super::shell::TABS), 9, 20).dual);
    }
}
