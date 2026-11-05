//! Pixel rail and wheel intentions over the existing ExplorerView catalog.
use super::{
    dpi::Dpi,
    explorer_panel::{self, ExplorerFrame},
    ffi::{Hdc, Rect},
    paint, shell, State,
};

#[derive(Default)]
pub(super) struct Interaction {
    pub drag: Option<Drag>,
    wheel: Option<(Stamp, i32)>,
}
impl Interaction {
    pub fn clear_wheel(&mut self) {
        self.wheel = None;
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
struct Stamp {
    generation: u64,
    revision: u64,
    first: usize,
    bounds: (i32, i32, i32, i32),
    epoch: u64,
}
#[derive(Clone, Copy)]
struct Rail {
    rect: Rect,
    first: usize,
    limit: usize,
    rows: usize,
    minimum: i32,
}
pub(super) struct Drag {
    stamp: Stamp,
    rail: Rail,
    grab: i32,
    anchor: i32,
}

pub(super) fn capacity(rect: Rect, dpi: Dpi) -> usize {
    ((rect.bottom - rect.top - dpi.pixels(shell::TREE_HEAD)) / dpi.pixels(shell::TREE_ROW))
        .clamp(0, 128) as usize
}
fn inside(rect: Rect, x: i32, y: i32) -> bool {
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
}
impl Rail {
    fn new(rect: Rect, dpi: Dpi, first: usize, total: usize, rows: usize) -> Self {
        Self {
            rect: Rect {
                left: (rect.right - dpi.pixels(10)).max(rect.left),
                top: (rect.top + dpi.pixels(shell::TREE_HEAD)).min(rect.bottom),
                ..rect
            },
            first,
            limit: total.saturating_sub(rows),
            rows,
            minimum: dpi.pixels(12),
        }
    }
    fn thumb(self) -> (i32, i32) {
        let length = (self.rect.bottom - self.rect.top).max(0);
        let total = self.limit.saturating_add(self.rows).max(1);
        let size = ((self.rows as u128 * length as u128 / total as u128) as i32)
            .clamp(length.min(self.minimum), length);
        let travel = length - size;
        let offset =
            self.first.min(self.limit) as u128 * travel as u128 / self.limit.max(1) as u128;
        (self.rect.top + offset as i32, size)
    }
    fn target(self, y: i32, grab: i32) -> usize {
        let (_, size) = self.thumb();
        let travel = self.rect.bottom - self.rect.top - size;
        if travel <= 0 {
            return 0;
        }
        let pixel = (y - self.rect.top - grab).clamp(0, travel);
        ((pixel as u128 * self.limit as u128 + travel as u128 / 2) / travel as u128) as usize
    }
}
pub(super) unsafe fn draw(dc: Hdc, frame: &ExplorerFrame) -> Result<(), String> {
    let rail = Rail::new(
        frame.rect,
        frame.dpi,
        frame.presentation.first,
        frame.presentation.total,
        capacity(frame.rect, frame.dpi),
    );
    paint::fill(dc, rail.rect, shell::SIDEBAR)?;
    if rail.limit > 0 {
        let (top, size) = rail.thumb();
        paint::fill(
            dc,
            Rect {
                left: rail.rect.left + frame.dpi.pixels(2),
                right: rail.rect.right - frame.dpi.pixels(2),
                top,
                bottom: top + size,
            },
            shell::MUTED,
        )?;
    }
    Ok(())
}
impl State {
    fn explorer_scroll_stamp(&self) -> Option<Stamp> {
        let rect = explorer_panel::region(self)?;
        Some(Stamp {
            generation: self.explorer.generation(),
            revision: self.explorer.revision(),
            first: self.explorer_view.scroll(),
            bounds: (rect.left, rect.top, rect.right, rect.bottom),
            epoch: self.interaction_epoch,
        })
    }
    pub(super) fn validate_explorer_scroll(&mut self) {
        if self.explorer_scroll.drag.as_ref().is_some_and(|drag| {
            !self.pointer_source_available() || self.explorer_scroll_stamp() != Some(drag.stamp)
        }) {
            self.explorer_scroll.drag = None;
        }
    }
    pub(super) fn explorer_wheel(
        &mut self,
        x: i32,
        y: i32,
        delta: i32,
        lines: u32,
    ) -> Option<bool> {
        let rect = explorer_panel::region(self).filter(|r| inside(*r, x, y))?;
        let stamp = self.explorer_scroll_stamp()?;
        let previous = self
            .explorer_scroll
            .wheel
            .take()
            .filter(|(s, _)| *s == stamp)
            .map_or(0, |(_, value)| value);
        let total = previous.saturating_add(delta);
        let rows = capacity(rect, self.dpi);
        let lines = if lines == u32::MAX {
            rows
        } else {
            lines.min(128) as usize
        };
        let step = -(total / 120) as isize * lines as isize;
        let changed = step != 0 && self.explorer_view.scroll_by(&self.explorer, step, rows);
        self.explorer_scroll.wheel = self
            .explorer_scroll_stamp()
            .map(|stamp| (stamp, total % 120));
        if changed {
            self.explorer_hits = None;
        }
        Some(changed)
    }
    pub(super) fn begin_explorer_scroll(&mut self, x: i32, y: i32) -> bool {
        let Some(rect) = explorer_panel::region(self) else {
            return false;
        };
        let Some(stamp) = self.explorer_scroll_stamp() else {
            return false;
        };
        let Some(hits) = self.explorer_hits.as_ref().filter(|h| {
            h.generation == stamp.generation
                && h.revision == stamp.revision
                && h.first == stamp.first
                && (h.rect.left, h.rect.top, h.rect.right, h.rect.bottom) == stamp.bounds
        }) else {
            return false;
        };
        let rail = Rail::new(rect, self.dpi, hits.first, hits.total, hits.capacity);
        if !inside(rail.rect, x, y) {
            return false;
        }
        let (start, size) = rail.thumb();
        let on_thumb = (start..start + size).contains(&y);
        let grab = if on_thumb { y - start } else { size / 2 };
        self.explorer_scroll.drag = Some(Drag {
            stamp,
            rail,
            grab,
            anchor: if on_thumb { y } else { i32::MIN },
        });
        self.update_explorer_scroll(x, y);
        true
    }
    pub(super) fn update_explorer_scroll(&mut self, _x: i32, y: i32) -> bool {
        self.validate_explorer_scroll();
        let Some(drag) = self.explorer_scroll.drag.as_ref() else {
            return false;
        };
        let first = if y == drag.anchor {
            drag.rail.first
        } else {
            drag.rail.target(y, drag.grab)
        };
        let changed = self
            .explorer_view
            .scroll_to(&self.explorer, first, drag.rail.rows);
        let stamp = self.explorer_scroll_stamp();
        if let (Some(drag), Some(stamp)) = (&mut self.explorer_scroll.drag, stamp) {
            drag.stamp = stamp;
        }
        if changed {
            self.explorer_hits = None;
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };

    fn populated() -> State {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("work/test-temp");
        fs::create_dir_all(&base).unwrap();
        let root = base.join(format!(
            "gui-explorer-scroll-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        for index in 0..80 {
            fs::write(
                root.join(format!("archivo-{index:03}.txt")),
                b"owned fixture\r\n",
            )
            .unwrap();
        }
        let mut state = State::new(&crate::GuiOptions::default()).unwrap();
        state.width = 1200;
        state.height = 550;
        state.key_override = Some(jc_app::Modifiers::NONE);
        state.explorer_visible = true;
        state
            .set_project_root(root.canonicalize().unwrap())
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let root = state.project_root.clone().unwrap();
            state.explorer.poll(&root, 2);
            if state.explorer.nodes().len() == 81 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "Owned explorer fixture did not load"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        publish(&mut state);
        state
    }
    fn publish(state: &mut State) {
        let frame = state.explorer_presentation().unwrap();
        state.explorer_hits = Some(frame.hits(state.cell_height));
    }
    #[test]
    fn wheel_scrolls_files_without_changing_focus_selection_or_document_and_keyboard_reveals() {
        let mut state = populated();
        let selected = state.explorer_view.selected();
        let source = state.workbench.active_bytes().unwrap();
        let rect = explorer_panel::region(&state).unwrap();
        let (x, y) = (rect.left + 20, rect.top + 100);
        assert!(!state.pointer_wheel(x, y, -60, 3, 0));
        assert_eq!(state.explorer_view.scroll(), 0);
        assert!(state.pointer_wheel(x, y, -60, 3, 0));
        assert_eq!(state.explorer_view.scroll(), 3);
        assert!(!state.explorer_focused);
        assert_eq!(state.explorer_view.selected(), selected);
        assert!(state.explorer_hits.is_none());
        publish(&mut state);
        assert_eq!(state.explorer_hits.as_ref().unwrap().first, 3);
        assert!(state.pointer_wheel(x, y, -120, u32::MAX, 0));
        assert!(state.explorer_view.scroll() > 3);
        state.explorer_key(0x28, jc_app::Modifiers::NONE);
        publish(&mut state);
        assert_eq!(state.explorer_view.scroll(), 1);
        assert_eq!(state.workbench.active_bytes().unwrap(), source);
    }
    #[test]
    fn rail_drag_reaches_both_ends_preserves_selection_and_rejects_stale_or_resized_hits() {
        let mut state = populated();
        let selected = state.explorer_view.selected();
        let rect = explorer_panel::region(&state).unwrap();
        let x = rect.right - 4;
        assert!(state.begin_explorer_scroll(x, rect.bottom - 1));
        assert!(state.explorer_view.scroll() > 0);
        assert_eq!(state.explorer_view.selected(), selected);
        assert!(!state.explorer_focused);
        assert!(state.update_explorer_scroll(x, rect.top - 100));
        assert_eq!(state.explorer_view.scroll(), 0);
        assert!(!state.update_explorer_scroll(x, rect.top - 100));
        state.cancel_pointer();
        assert!(state.explorer_scroll.drag.is_none());
        publish(&mut state);
        state
            .explorer_view
            .scroll_by(&state.explorer, 5, capacity(rect, state.dpi));
        assert!(!state.begin_explorer_scroll(x, rect.bottom - 1));
        publish(&mut state);
        assert!(state.begin_explorer_scroll(x, rect.bottom - 1));
        let first = state.explorer_view.scroll();
        state.height += 50;
        assert!(!state.update_explorer_scroll(x, rect.top));
        assert!(state.explorer_scroll.drag.is_none());
        assert_eq!(state.explorer_view.scroll(), first);
        publish(&mut state);
        assert!(state.begin_explorer_scroll(x, rect.bottom - 1));
        state.explorer.clear();
        assert!(!state.update_explorer_scroll(x, rect.top));
        assert!(state.explorer_scroll.drag.is_none());
    }
    #[test]
    fn rail_geometry_scales_and_stays_inside_panel_at_supported_dpi() {
        for raw in [96, 144, 192] {
            let dpi = Dpi::from_raw(raw);
            let rect = Rect {
                left: 0,
                top: 50,
                right: dpi.pixels(250),
                bottom: 50 + dpi.pixels(500),
            };
            let rail = Rail::new(rect, dpi, 0, 100, capacity(rect, dpi));
            let (top, size) = rail.thumb();
            assert_eq!(rail.rect.right - rail.rect.left, dpi.pixels(10));
            assert!(top >= rect.top && top + size <= rect.bottom);
            assert_eq!(rail.target(rect.bottom + 100, size / 2), rail.limit);
            assert_eq!(rail.target(rect.top - 100, size / 2), 0);
        }
    }
}
