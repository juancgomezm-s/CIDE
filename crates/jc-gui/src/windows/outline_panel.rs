//! Persistent sidebar over the source controller. Painting never starts a job.
use super::{
    ffi::{Hdc, Rect},
    paint, shell, wide, State,
};
use jc_app::{
    source_services::{display_ui_label, SourceController, SourceMode, SourceRow, SourceState},
    CompletedTask, Modifiers,
};

pub(super) struct Outline {
    controller: SourceController,
    pub focused: bool,
    pub hits: Option<OutlineHits>,
    first: usize,
    wheel: i32,
}

impl Default for Outline {
    fn default() -> Self {
        Self {
            controller: SourceController::outline(),
            focused: false,
            hits: None,
            first: 0,
            wheel: 0,
        }
    }
}

pub(super) struct OutlineFrame {
    rect: Rect,
    dpi: super::dpi::Dpi,
    rows: Vec<SourceRow>,
    message: String,
    focused: bool,
}

pub(super) struct OutlineHits {
    rect: Rect,
    rows: Vec<usize>,
}

impl OutlineFrame {
    pub(super) fn hits(&self) -> OutlineHits {
        OutlineHits {
            rect: self.rect,
            rows: self.rows.iter().map(|row| row.index).collect(),
        }
    }
}

const HEAD: i32 = 46;
const ROW: i32 = 22;

fn provider(state: &State) -> bool {
    state
        .source_gui
        .registry()
        .supports_document(&state.workbench, SourceMode::Symbols)
}

pub(super) fn region(state: &State) -> Option<Rect> {
    let rect = super::explorer_panel::region(state)?;
    let height = rect.bottom - rect.top;
    if height < state.dpi.pixels(260) || !provider(state) {
        return None;
    }
    Some(Rect {
        top: rect.bottom - (height * 2 / 5).max(state.dpi.pixels(112)),
        ..rect
    })
}

fn capacity(rect: Rect, dpi: super::dpi::Dpi) -> usize {
    ((rect.bottom - rect.top - dpi.pixels(HEAD)) / dpi.pixels(ROW)).clamp(0, 64) as usize
}

fn contains(rect: Rect, x: i32, y: i32) -> bool {
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
}

impl State {
    pub(super) fn schedule_outline(&self) {
        let wanted = region(self).is_some();
        let controller = &self.outline.controller;
        if (wanted
            && controller.needs_outline_refresh(&self.workbench, self.project_root.as_deref()))
            || (!wanted
                && !matches!(
                    controller.state(),
                    SourceState::Idle | SourceState::Cancelled
                ))
        {
            self.bridge.signal();
        }
    }

    pub(super) fn tick_outline(&mut self) -> bool {
        let wanted = region(self).is_some();
        let controller = &mut self.outline.controller;
        if !wanted {
            if self.outline.focused {
                self.explorer_focused = false;
                self.outline.focused = false;
            }
            self.outline.hits = None;
            if matches!(
                controller.state(),
                SourceState::Idle | SourceState::Cancelled
            ) {
                return false;
            }
            controller.cancel(&mut self.jobs);
            return true;
        }
        let root = self.project_root.as_deref();
        let mut changed = controller.validate_context(&self.workbench, root, &mut self.jobs);
        if controller.needs_outline_refresh(&self.workbench, root) {
            self.outline.hits = None;
            self.outline.first = 0;
            let _ = controller.request(
                SourceMode::Symbols,
                self.source_gui.registry(),
                &self.workbench,
                root,
                &mut self.jobs,
            );
            changed = true;
        }
        controller.tick(&self.workbench, root, &mut self.jobs) | changed
    }

    pub(super) fn accept_outline(&mut self, completion: &mut CompletedTask) -> bool {
        let accepted = self.outline.controller.accept(
            &self.workbench,
            self.project_root.as_deref(),
            completion,
        );
        if accepted {
            self.outline.hits = None;
        }
        accepted
    }

    pub(super) fn outline_presentation(&self) -> Option<OutlineFrame> {
        let rect = region(self)?;
        let columns = ((rect.right - rect.left - 26) / self.cell_width.max(1)).max(1) as usize;
        let controller = &self.outline.controller;
        let ready = self.outline_ready();
        let rows = if ready {
            controller
                .items()
                .iter()
                .enumerate()
                .skip(
                    self.outline.first.min(
                        controller
                            .items()
                            .len()
                            .saturating_sub(capacity(rect, self.dpi)),
                    ),
                )
                .take(capacity(rect, self.dpi))
                .map(|(index, item)| SourceRow {
                    index,
                    label: display_ui_label(&item.label, columns),
                    selected: controller.selected() == index,
                })
                .collect()
        } else {
            Vec::new()
        };
        let message = if self.workbench.active_encoding().is_none() {
            "Elige la codificación del documento"
        } else if controller.needs_outline_refresh(&self.workbench, self.project_root.as_deref()) {
            "Actualizando símbolos…"
        } else {
            controller.message()
        };
        Some(OutlineFrame {
            rect,
            dpi: self.dpi,
            rows,
            message: display_ui_label(message, columns),
            focused: self.outline_focused(),
        })
    }

    pub(super) fn outline_focused(&self) -> bool {
        self.explorer_focused && self.outline.focused && region(self).is_some()
    }

    pub(super) fn outline_ready(&self) -> bool {
        self.outline
            .controller
            .ready(&self.workbench, self.project_root.as_deref())
    }

    fn activate_outline(&mut self) {
        let index = self.outline.controller.selected();
        match self.outline.controller.activate_at_index(
            &mut self.workbench,
            self.project_root.as_deref(),
            index,
            false,
        ) {
            Ok(_) => {
                self.exit_outline();
                self.status = "Símbolo localizado en el documento".into();
            }
            Err(error) => self.error(error),
        }
    }

    pub(super) fn refresh_outline(&mut self) {
        self.outline.controller.cancel(&mut self.jobs);
        self.outline.hits = None;
        self.schedule_outline();
    }

    pub(super) fn exit_outline(&mut self) {
        self.explorer_focused = false;
        self.outline.focused = false;
    }

    pub(super) fn outline_key(&mut self, code: usize, modifiers: Modifiers) {
        if modifiers != Modifiers::NONE || !self.outline_ready() {
            return;
        }
        if code == 0x0d {
            self.activate_outline();
            return;
        }
        let controller = &mut self.outline.controller;
        match code {
            0x26 => controller.select_relative(-1),
            0x28 => controller.select_relative(1),
            0x21 => controller.select_relative(-10),
            0x22 => controller.select_relative(10),
            0x24 => controller.first(),
            0x23 => controller.last(),
            _ => return,
        }
        let selected = controller.selected();
        let rows = region(self)
            .map_or(1, |rect| capacity(rect, self.dpi))
            .max(1);
        self.outline.first = self
            .outline
            .first
            .clamp(selected.saturating_sub(rows - 1), selected);
        self.outline.hits = None;
    }

    pub(super) fn outline_click(&mut self, x: i32, y: i32, double: bool) -> bool {
        let Some(rect) = region(self).filter(|r| contains(*r, x, y)) else {
            return false;
        };
        self.explorer_focused = true;
        self.outline.focused = true;
        self.bottom.focused = false;
        if self.outline_ready() {
            let index = self
                .outline
                .hits
                .as_ref()
                .filter(|hits| {
                    hits.rect.top == rect.top
                        && hits.rect.right == rect.right
                        && hits.rect.bottom == rect.bottom
                })
                .and_then(|hits| {
                    hits.rows.get(
                        usize::try_from(y - rect.top - self.dpi.pixels(HEAD)).ok()?
                            / self.dpi.pixels(ROW) as usize,
                    )
                })
                .copied();
            if let Some(index) = index {
                self.outline.controller.first();
                self.outline.controller.select_relative(index as isize);
                if double {
                    self.activate_outline();
                }
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
        if step != 0 && self.outline_ready() {
            self.outline.first = self
                .outline
                .first
                .saturating_add_signed(-step)
                .min(self.outline.controller.items().len().saturating_sub(rows));
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
        left: rect.left + 16,
        right: rect.right - 8,
        top,
        bottom,
    };
    paint::text(
        dc,
        label(rect.top + px(6), rect.top + px(25)),
        &wide("ESQUEMA"),
        shell::TEXT,
        shell::SIDEBAR,
    )?;
    paint::text(
        dc,
        label(rect.top + px(25), rect.top + px(HEAD)),
        &wide("Intro · ir al símbolo"),
        shell::MUTED,
        shell::SIDEBAR,
    )?;
    if frame.rows.is_empty() {
        paint::text(
            dc,
            label(rect.top + px(HEAD), rect.bottom),
            &wide(&frame.message),
            shell::MUTED,
            shell::SIDEBAR,
        )?;
    }
    for (i, row) in frame.rows.iter().enumerate() {
        let top = rect.top + px(HEAD) + i as i32 * px(ROW);
        let background = if row.selected {
            shell::SELECTED
        } else {
            shell::SIDEBAR
        };
        paint::fill(
            dc,
            Rect {
                left: rect.left + 6,
                top,
                right: rect.right - 6,
                bottom: top + px(ROW),
            },
            background,
        )?;
        if row.selected && frame.focused {
            paint::fill(
                dc,
                Rect {
                    left: rect.left + 6,
                    top,
                    right: rect.left + 8,
                    bottom: top + px(ROW),
                },
                shell::ACCENT,
            )?;
        }
        paint::text(
            dc,
            label(top + px(2), top + px(ROW)),
            &wide(&row.label),
            shell::TEXT,
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
        while matches!(
            state.outline.controller.state(),
            SourceState::Queued | SourceState::Running
        ) {
            state.poll_tasks();
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
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
        let frame = state.outline_presentation().unwrap();
        assert_eq!(frame.rows.len(), 2);
        let rect = frame.rect;
        let x = rect.left + 24;
        let y = rect.top + HEAD + ROW + 2;
        state.outline.hits = Some(frame.hits());
        assert!(state.explorer_click(x, y, true));
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
        let frame = state.outline_presentation().unwrap();
        assert!(frame.rows.iter().any(|row| row.label.contains("xbeta")));
    }
}
