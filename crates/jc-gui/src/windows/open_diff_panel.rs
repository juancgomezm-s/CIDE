//! Native view of the shared immutable comparison. No source buffer or Git I/O.
use super::{ffi::*, paint, wide, Action, State};
use jc_app::{
    diff_model::DiffSide,
    open_diff::OpenDiffState,
    open_diff_view::{OpenDiffPresentation, OpenDiffRowKind},
    CommandId, Modifiers,
};

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
    fn new(width: i32, height: i32, cw: i32, ch: i32) -> Self {
        let width = width.clamp(1, 8192);
        let height = height.clamp(1, 8192);
        let cw = cw.clamp(1, 8192);
        let ch = ch.clamp(1, 8192);
        let rect = Rect {
            left: 0,
            top: (ch + 8).min(height),
            right: width,
            bottom: (height - ch - 6).max(0),
        };
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
    fn row(self, index: usize) -> Rect {
        let top = (self.rect.top + index.min(100) as i32 * self.ch).min(self.rect.bottom);
        Rect {
            left: 8.min(self.rect.right),
            top,
            right: self.rect.right,
            bottom: (top + self.ch).min(self.rect.bottom),
        }
    }
}
impl State {
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
        self.open_diff
            .validate_context(&self.workbench, &mut self.jobs)
    }
    pub(super) fn open_diff_command(&mut self, command: CommandId) -> Action {
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
                    let g =
                        Geometry::new(self.width, self.height, self.cell_width, self.cell_height);
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
        let geometry = Geometry::new(self.width, self.height, self.cell_width, self.cell_height);
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
        })
    }
}
pub(super) unsafe fn draw(dc: Hdc, frame: &ComparisonFrame) -> Result<(), String> {
    let g = frame.geometry;
    let p = &frame.presentation;
    const BG: u32 = 0x00272828;
    const HEADER: u32 = 0x00202122;
    const FG: u32 = 0x00d5d7d8;
    const DIM: u32 = 0x008d8b87;
    paint::fill(dc, g.rect, BG)?;
    let line =
        |row: usize, text: &str, fg: u32, bg: u32| paint::text(dc, g.row(row), &wide(text), fg, bg);
    if g.rows >= 1 {
        line(0, &p.title, FG, HEADER)?;
    }
    let first = if let Some([left, right]) = &frame.origins {
        if g.dual {
            if g.rows >= 2 {
                let rect = g.row(1);
                let split = 8 + ((g.columns - 3) / 2) as i32 * g.cw;
                paint::text(
                    dc,
                    Rect {
                        right: split,
                        ..rect
                    },
                    &wide(&format!("I: {left}")),
                    FG,
                    HEADER,
                )?;
                paint::text(
                    dc,
                    Rect {
                        left: split,
                        ..rect
                    },
                    &wide(&format!("D: {right}")),
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
                line(1, &format!("I: {left}"), FG, HEADER)?;
            }
            if g.rows >= 3 {
                line(2, &format!("D: {right}"), FG, HEADER)?;
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
            let split = 8 + ((g.columns - 3) / 2) as i32 * g.cw;
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
            "Copias de solo lectura | diferencias de bytes | sin mezcla ni escrituras",
            DIM,
            HEADER,
        )?;
    }
    if g.rows >= 4 {
        line(g.rows-1,"↑↓ bloque | Pág contenido | Intro derecha / Mayús+Intro izquierda | F5 actualizar | Esc",DIM,HEADER)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn comparison_geometry_fits_client_and_switches_to_unified_without_overflow() {
        for (w, h, cw, ch) in [
            (1, 1, 0, 0),
            (360, 240, 9, 20),
            (1400, 800, 9, 20),
            (i32::MAX, i32::MAX, i32::MAX, i32::MAX),
        ] {
            let g = Geometry::new(w, h, cw, ch);
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
        assert!(!Geometry::new(360, 240, 9, 20).dual);
        assert!(Geometry::new(1400, 800, 9, 20).dual);
    }
}
