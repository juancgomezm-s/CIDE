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
    pub visible: bool,
    mode: SourceMode,
    candidates: Option<(Vec<std::path::PathBuf>, usize)>,
    controller: SourceController,
    pub focused: bool,
    pub hits: Option<OutlineHits>,
    first: usize,
    wheel: i32,
}

impl Default for Outline {
    fn default() -> Self {
        Self {
            visible: true,
            mode: SourceMode::Symbols,
            candidates: None,
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
    mode: SourceMode,
    tools: Vec<(Rect, SourceMode, bool)>,
    close: Rect,
}

pub(super) struct OutlineHits {
    rect: Rect,
    rows: Vec<usize>,
    close: Rect,
}

impl OutlineFrame {
    pub(super) fn hits(&self) -> OutlineHits {
        OutlineHits {
            rect: self.rect,
            rows: self.rows.iter().map(|row| row.index).collect(),
            close: self.close,
        }
    }
}

const HEAD: i32 = 114;
const ROW: i32 = 22;

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
    let panel_width = state.dpi.pixels(260).min(state.width / 3);
    let left = super::explorer_panel::region(state).map_or(0, |r| r.right + 1);
    if state.workbench.panes().is_split()
        && (state.width - left - panel_width - 1) / state.cell_width.max(1) < 90
    {
        return None;
    }
    Some(Rect {
        left: state.width - panel_width,
        right: state.width,
        top: state.chrome_layout().top(),
        bottom: (state.height - state.cell_height - 8).max(0),
    })
}

fn tools(rect: Rect, dpi: super::dpi::Dpi) -> Vec<(Rect, SourceMode)> {
    [
        SourceMode::Symbols,
        SourceMode::Calls,
        SourceMode::Incoming,
        SourceMode::Dependencies,
    ]
    .into_iter()
    .enumerate()
    .map(|(i, mode)| {
        let width = (rect.right - rect.left - dpi.pixels(16)) / 2;
        let left = rect.left + dpi.pixels(8) + (i % 2) as i32 * width;
        let top = rect.top + dpi.pixels(32 + (i / 2) as i32 * 28);
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

impl State {
    pub(super) fn set_outline_visible(&mut self, visible: bool) {
        self.outline.visible = visible;
        self.exit_outline();
        self.outline.hits = None;
        self.tab_hits = None;
        self.outline.candidates = None;
        self.outline.first = 0;
        self.outline.wheel = 0;
        if visible {
            self.outline.mode = SourceMode::Symbols;
            self.schedule_outline();
            self.status = "Esquema visible · selecciona un símbolo y pulsa Intro".into();
        } else {
            self.outline.controller.cancel(&mut self.jobs);
            self.status =
                "Esquema oculto · pulsa Esquema en la barra inferior para mostrarlo".into();
        }
    }
    pub(super) fn schedule_outline(&self) {
        let wanted = region(self).is_some() && provider(self);
        let controller = &self.outline.controller;
        if (wanted
            && self.outline.mode == SourceMode::Symbols
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
        let wanted = region(self).is_some() && provider(self);
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
        if self.outline.mode == SourceMode::Symbols
            && controller.needs_outline_refresh(&self.workbench, root)
        {
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
        let rows = if ready && self.outline.candidates.is_some() {
            let (paths, selected) = self.outline.candidates.as_ref().unwrap();
            paths
                .iter()
                .enumerate()
                .skip(self.outline.first)
                .take(capacity(rect, self.dpi))
                .map(|(index, path)| SourceRow {
                    index,
                    label: display_ui_label(&path.display().to_string(), columns),
                    selected: *selected == index,
                })
                .collect()
        } else if ready {
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
        let message = if !provider(self) {
            "Sin proveedor para este lenguaje"
        } else if self.workbench.active_encoding().is_none() {
            "Elige la codificación del documento"
        } else if self.outline.mode == SourceMode::Symbols
            && controller.needs_outline_refresh(&self.workbench, self.project_root.as_deref())
        {
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
            mode: self.outline.mode,
            tools: tools(rect, self.dpi)
                .into_iter()
                .map(|(r, mode)| {
                    let enabled = if mode == SourceMode::Incoming {
                        self.source_gui
                            .registry()
                            .supports_incoming(&self.workbench, self.project_root.as_deref())
                    } else {
                        self.source_gui
                            .registry()
                            .supports_document(&self.workbench, mode)
                    };
                    (r, mode, enabled)
                })
                .collect(),
            close: close_rect(rect, self.dpi),
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

    pub(super) fn activate_outline(&mut self, side: bool) {
        if let Some((paths, selected)) = &self.outline.candidates {
            let result = paths
                .get(*selected)
                .ok_or_else(|| "Elige una referencia".to_owned())
                .and_then(|path| {
                    self.outline.controller.activate_candidate(
                        &mut self.workbench,
                        self.project_root.as_deref(),
                        path,
                        side,
                    )
                });
            match result {
                Ok(()) => {
                    self.outline.candidates = None;
                    self.exit_outline();
                    self.ensure_encoding_selector();
                }
                Err(error) => self.error(error),
            }
            return;
        }
        let index = self.outline.controller.selected();
        match self.outline.controller.activate_at_index(
            &mut self.workbench,
            self.project_root.as_deref(),
            index,
            side,
        ) {
            Ok(Some(paths)) => {
                self.outline.candidates = Some((paths, 0));
                self.outline.first = 0;
                self.outline.hits = None;
                self.status = "Referencia ambigua: elige un candidato en el panel Esquema".into();
            }
            Ok(None) => {
                self.exit_outline();
                self.ensure_encoding_selector();
                self.status = "Destino léxico abierto desde Esquema".into();
            }
            Err(error) => self.error(error),
        }
    }

    pub(super) fn refresh_outline(&mut self) {
        self.outline.mode = SourceMode::Symbols;
        self.outline.controller.cancel(&mut self.jobs);
        self.outline.hits = None;
        self.schedule_outline();
    }

    pub(super) fn exit_outline(&mut self) {
        self.explorer_focused = false;
        self.outline.focused = false;
    }

    pub(super) fn outline_key(&mut self, code: usize, modifiers: Modifiers) {
        if (modifiers != Modifiers::NONE && modifiers != Modifiers::CTRL) || !self.outline_ready() {
            return;
        }
        if code == 0x0d {
            self.activate_outline(modifiers == Modifiers::CTRL);
            return;
        }
        if let Some((paths, selected)) = &mut self.outline.candidates {
            *selected = match code {
                0x26 => selected.saturating_sub(1),
                0x28 => selected
                    .saturating_add(1)
                    .min(paths.len().saturating_sub(1)),
                0x24 | 0x21 => 0,
                0x23 | 0x22 => paths.len().saturating_sub(1),
                _ => return,
            };
            let rows = capacity(region(self).unwrap(), self.dpi).max(1);
            let selected = self.outline.candidates.as_ref().unwrap().1;
            self.outline.first = self
                .outline
                .first
                .clamp(selected.saturating_sub(rows - 1), selected);
            self.outline.hits = None;
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
        if self.outline.hits.as_ref().is_some_and(|hits| {
            same_rect(hits.rect, rect)
                && same_rect(hits.close, close_rect(rect, self.dpi))
                && contains(hits.close, x, y)
        }) {
            self.set_outline_visible(false);
            return true;
        }
        if let Some((_, mode)) = tools(rect, self.dpi)
            .into_iter()
            .find(|(r, _)| contains(*r, x, y))
        {
            let enabled = if mode == SourceMode::Incoming {
                self.source_gui
                    .registry()
                    .supports_incoming(&self.workbench, self.project_root.as_deref())
            } else {
                self.source_gui
                    .registry()
                    .supports_document(&self.workbench, mode)
            };
            if enabled {
                self.explorer_focused = true;
                self.outline.focused = true;
                self.bottom.focused = false;
                self.outline.mode = mode;
                self.outline.first = 0;
                self.outline.hits = None;
                self.outline.candidates = None;
                if let Err(error) = self.outline.controller.request(
                    mode,
                    self.source_gui.registry(),
                    &self.workbench,
                    self.project_root.as_deref(),
                    &mut self.jobs,
                ) {
                    self.error(error);
                }
            }
            return true;
        }
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
                        && hits.rect.left == rect.left
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
                if let Some((_, selected)) = &mut self.outline.candidates {
                    *selected = index;
                } else {
                    self.outline.controller.first();
                    self.outline.controller.select_relative(index as isize);
                }
                if double {
                    self.activate_outline(self.modifiers().contains(Modifiers::CTRL));
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
            self.outline.first = self.outline.first.saturating_add_signed(-step).min(
                self.outline
                    .candidates
                    .as_ref()
                    .map_or(self.outline.controller.items().len(), |(paths, _)| {
                        paths.len()
                    })
                    .saturating_sub(rows),
            );
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
        Rect {
            right: frame.close.left - px(6),
            ..label(rect.top + px(6), rect.top + px(25))
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
    paint::text(
        dc,
        label(rect.top + px(90), rect.top + px(HEAD)),
        &wide("Doble clic / Intro · Ctrl: al lado"),
        shell::MUTED,
        shell::SIDEBAR,
    )?;
    for (r, mode, enabled) in &frame.tools {
        let background = if *mode == frame.mode {
            shell::SELECTED
        } else {
            shell::RAISED
        };
        paint::fill(dc, *r, background)?;
        paint::text(
            dc,
            Rect {
                left: r.left + px(5),
                top: r.top + px(2),
                ..*r
            },
            &wide(mode.label()),
            if *enabled { shell::TEXT } else { shell::MUTED },
            background,
        )?;
    }
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
        assert_eq!(frame.rows.len(), 2);
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
        assert_eq!(frame.rows.len(), 2);
        let row_x = frame.rect.left + 24;
        let row_y = frame.rect.top + state.dpi.pixels(HEAD) + 2;
        state.outline.hits = Some(frame.hits());
        assert!(state.explorer_click(row_x, row_y, false));
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

    #[test]
    fn right_code_panel_runs_explicit_abl_and_javascript_queries_without_editing() {
        for (path, text, mode) in [
            (
                "code.p",
                "RUN beta.\r\n{common.i}\r\n",
                SourceMode::Dependencies,
            ),
            (
                "code.js",
                "import { inner } from './other.js';\nfunction outer() { inner(); }\n",
                SourceMode::Dependencies,
            ),
        ] {
            let mut s = State::new(&GuiOptions::default()).unwrap();
            s.width = 1400;
            s.height = 900;
            s.explorer_visible = true;
            s.workbench.new_file(path, TextEncoding::Utf8);
            s.workbench.insert_text(text).unwrap();
            finish(&mut s);
            let r = region(&s).unwrap();
            let tree = super::super::explorer_panel::region(&s).unwrap();
            assert!(r.left > tree.right);
            assert!(super::super::paint::areas(&s)
                .iter()
                .all(|(_, area)| area.right < r.left));
            let (button, _) = tools(r, s.dpi)
                .into_iter()
                .find(|(_, m)| *m == mode)
                .unwrap();
            assert!(s.outline_click(button.left + 2, button.top + 2, false));
            finish(&mut s);
            assert_eq!(s.outline.controller.mode(), Some(mode));
            assert_eq!(
                s.outline.controller.state(),
                SourceState::Ready,
                "{}",
                s.outline.controller.message()
            );
            assert!(!s.outline_presentation().unwrap().rows.is_empty());
            assert_eq!(s.workbench.active_bytes().unwrap(), text.as_bytes());
            s.workbench.insert_text(" ").unwrap();
            s.poll_tasks();
            assert!(s.outline_presentation().unwrap().rows.is_empty());
            assert_eq!(s.outline.controller.mode(), Some(mode));
        }
    }

    #[test]
    fn right_code_dependencies_open_to_side_using_propath_and_javascript_relative_paths() {
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
        for (name, source, target) in [
            ("main.p", "{common.i}\r\n", "inc/common.i"),
            ("main.js", "import './other.js';\n", "other.js"),
        ] {
            let path = root.join(name);
            let destination = root.join(target);
            std::fs::write(&path, source).unwrap();
            std::fs::write(&destination, b"// destino\n").unwrap();
            let mut s = State::new(&GuiOptions::default()).unwrap();
            s.width = 1400;
            s.height = 900;
            s.set_project_root(root.clone()).unwrap();
            s.workbench
                .open_file(&path, Some(TextEncoding::Utf8))
                .unwrap();
            let r = region(&s).unwrap();
            let (button, _) = tools(r, s.dpi)
                .into_iter()
                .find(|(_, mode)| *mode == SourceMode::Dependencies)
                .unwrap();
            s.outline_click(button.left + 2, button.top + 2, false);
            finish(&mut s);
            assert!(s.outline_ready(), "{}", s.outline.controller.message());
            s.activate_outline(true);
            assert!(s.workbench.panes().is_split());
            assert_eq!(
                s.workbench.active_document().unwrap().session.path(),
                destination.canonicalize().unwrap()
            );
            assert_eq!(std::fs::read(&path).unwrap(), source.as_bytes());
            assert_eq!(s.workbench.active_bytes().unwrap(), b"// destino\n");
        }
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
        s.outline.controller.last();
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
