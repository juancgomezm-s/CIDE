//! Per-pane scroll rails and a sampled source overview. No document access in GDI drawing.
use super::{
    ffi::{Hdc, Rect},
    paint,
    pointer_input::PointerContext,
    shell, State,
};
use jc_app::{PaneId, TabId};
use std::sync::Arc;

const RAIL: i32 = 10;
const MAP_WIDTH: i32 = 62;
const MAX_SAMPLES: usize = 192;
const PREFIX_BYTES: usize = 80;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Axis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy)]
pub(super) struct Geometry {
    pub text: Rect,
    vertical: Rect,
    horizontal: Rect,
    map: Option<Rect>,
}
impl Geometry {
    pub fn new(pane: Rect) -> Self {
        let top = (pane.top + shell::HEADER).min(pane.bottom);
        let bottom = (pane.bottom - RAIL).max(top);
        let right = (pane.right - RAIL).max(pane.left);
        let map = (right - pane.left >= 320).then_some(Rect {
            left: right - MAP_WIDTH,
            right,
            top,
            bottom,
        });
        let text = Rect {
            left: pane.left,
            right: map.map_or(right, |r| r.left),
            top,
            bottom,
        };
        Self {
            text,
            vertical: Rect {
                left: right,
                right: pane.right,
                top,
                bottom,
            },
            horizontal: Rect {
                left: pane.left,
                right: text.right,
                top: bottom,
                bottom: pane.bottom,
            },
            map,
        }
    }
}

#[derive(Clone, Copy)]
struct Track {
    axis: Axis,
    rect: Rect,
    offset: usize,
    limit: usize,
    page: usize,
}
impl Track {
    fn start(self) -> i32 {
        match self.axis {
            Axis::Vertical => self.rect.top,
            Axis::Horizontal => self.rect.left,
        }
    }
    fn length(self) -> i32 {
        match self.axis {
            Axis::Vertical => self.rect.bottom - self.rect.top,
            Axis::Horizontal => self.rect.right - self.rect.left,
        }
        .max(0)
    }
    fn thumb(self) -> (i32, i32) {
        let length = self.length();
        let size = scale(self.page, self.limit.saturating_add(self.page), length)
            .clamp(length.min(12), length);
        (
            self.start() + scale(self.offset, self.limit, length - size),
            size,
        )
    }
    fn position(self, x: i32, y: i32) -> i32 {
        match self.axis {
            Axis::Vertical => y,
            Axis::Horizontal => x,
        }
    }
    fn target(self, pixel: i32, grab: i32) -> usize {
        let (_, size) = self.thumb();
        let travel = self.length() - size;
        let position = (pixel - self.start() - grab).clamp(0, travel);
        if travel == 0 {
            return 0;
        }
        ((position as u128 * self.limit as u128 + travel as u128 / 2) / travel as u128) as usize
    }
    fn thumb_rect(self) -> Rect {
        let (start, size) = self.thumb();
        match self.axis {
            Axis::Vertical => Rect {
                top: start,
                bottom: start + size,
                ..self.rect
            },
            Axis::Horizontal => Rect {
                left: start,
                right: start + size,
                ..self.rect
            },
        }
    }
}
fn scale(value: usize, total: usize, pixels: i32) -> i32 {
    if total == 0 {
        return 0;
    }
    (value.min(total) as u128 * pixels as u128 / total as u128) as i32
}
fn contains(rect: Rect, x: i32, y: i32) -> bool {
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct SampleKey {
    identity: u64,
    revision: u64,
    count: usize,
}
#[derive(Clone, Copy)]
struct Mark {
    start: u8,
    end: u8,
}
#[derive(Default)]
pub(super) struct MapCache {
    key: Option<SampleKey>,
    samples: Arc<[Mark]>,
}
impl MapCache {
    fn prepare(&mut self, app: &jc_app::Workbench, tab: TabId, lines: usize, pixels: i32) {
        let Some(doc) = app.document_by_tab(tab) else {
            return;
        };
        let source = doc.session.document();
        let count = lines.min(MAX_SAMPLES).min((pixels / 2).max(0) as usize);
        let key = SampleKey {
            identity: source.recovery_identity(),
            revision: source.revision(),
            count,
        };
        if self.key == Some(key) {
            return;
        }
        let marks = (0..count)
            .map(|index| {
                let line = if count <= 1 {
                    0
                } else {
                    (index as u128 * lines.saturating_sub(1) as u128 / (count - 1) as u128) as usize
                };
                let Some((range, _)) = app.line_range_for_tab(tab, line) else {
                    return Mark { start: 0, end: 0 };
                };
                let end = range.start + range.len().min(PREFIX_BYTES);
                let Ok(bytes) = source.read_range(range.start..end) else {
                    return Mark { start: 0, end: 0 };
                };
                let mut column = 0usize;
                let mut mark = Mark { start: 0, end: 0 };
                for byte in bytes {
                    let next = if byte == b'\t' {
                        column + 4 - column % 4
                    } else {
                        column + 1
                    };
                    if !byte.is_ascii_whitespace() {
                        if mark.end == 0 {
                            mark.start = column.min(PREFIX_BYTES - 1) as u8;
                        }
                        mark.end = next.min(PREFIX_BYTES) as u8;
                    }
                    column = next;
                }
                mark
            })
            .collect::<Vec<_>>();
        self.samples = marks.into();
        self.key = Some(key);
    }
}

pub(super) struct Frame {
    vertical: Track,
    horizontal: Track,
    overview: Option<(Track, Arc<[Mark]>)>,
}
impl Frame {
    pub fn prepare(state: &mut State, area: &paint::TextArea) -> Self {
        let geometry = area.geometry;
        let app = &state.workbench;
        let page = area.capacity_rows.max(1);
        let vertical = Track {
            axis: Axis::Vertical,
            rect: geometry.vertical,
            offset: area.first,
            limit: area.count.saturating_sub(page),
            page,
        };
        let horizontal = Track {
            axis: Axis::Horizontal,
            rect: geometry.horizontal,
            offset: area.scroll,
            limit: app
                .horizontal_scroll_max(
                    area.tab,
                    area.first,
                    area.rows,
                    area.columns,
                    app.invisibles_visible(),
                )
                .max(area.scroll),
            page: area.columns.max(1),
        };
        let overview = geometry.map.map(|rect| {
            let cache = &mut state.editor_maps[usize::from(area.pane == PaneId::Secondary)];
            cache.prepare(app, area.tab, area.count, rect.bottom - rect.top);
            (Track { rect, ..vertical }, Arc::clone(&cache.samples))
        });
        Self {
            vertical,
            horizontal,
            overview,
        }
    }

    pub unsafe fn draw(&self, dc: Hdc) -> Result<(), String> {
        for track in [self.vertical, self.horizontal] {
            paint::fill(dc, track.rect, shell::SURFACE)?;
            if track.limit != 0 {
                let mut thumb = track.thumb_rect();
                match track.axis {
                    Axis::Vertical => {
                        thumb.left += 2;
                        thumb.right -= 2;
                    }
                    Axis::Horizontal => {
                        thumb.top += 2;
                        thumb.bottom -= 2;
                    }
                }
                paint::fill(dc, thumb, shell::MUTED)?;
            }
        }
        if let Some((map, samples)) = &self.overview {
            paint::fill(dc, map.rect, shell::SIDEBAR)?;
            let window = map.thumb_rect();
            paint::fill(dc, window, shell::SELECTED)?;
            let left = map.rect.left + 5;
            let width = map.rect.right - left - 4;
            let marks = samples
                .iter()
                .enumerate()
                .filter(|(_, mark)| mark.end != 0)
                .map(|(index, mark)| {
                    let y = map.rect.top + scale(index, samples.len(), map.length());
                    let x = left + scale(usize::from(mark.start), PREFIX_BYTES, width);
                    let end = left + scale(usize::from(mark.end), PREFIX_BYTES, width);
                    Rect {
                        left: x,
                        right: end.max(x + 1),
                        top: y,
                        bottom: y + 1,
                    }
                });
            paint::fill_marks(dc, marks, shell::MUTED)?;
        }
        Ok(())
    }
}

pub(super) struct Drag {
    context: PointerContext,
    pane: PaneId,
    track: Track,
    grab: i32,
    anchor_pixel: Option<i32>,
}
impl State {
    pub(super) fn begin_editor_scroll(&mut self, x: i32, y: i32) -> bool {
        if !self.pointer_source_available() {
            return false;
        }
        let Some(area) = paint::text_areas(self).into_iter().find(|area| {
            let g = area.geometry;
            contains(g.vertical, x, y)
                || contains(g.horizontal, x, y)
                || g.map.is_some_and(|r| contains(r, x, y))
        }) else {
            return false;
        };
        let frame = Frame::prepare(self, &area);
        let track = if contains(frame.vertical.rect, x, y) {
            frame.vertical
        } else if contains(frame.horizontal.rect, x, y) {
            frame.horizontal
        } else {
            frame.overview.as_ref().unwrap().0
        };
        let Some(context) = self.pointer_context(area.pane) else {
            return false;
        };
        let pixel = track.position(x, y);
        let (start, size) = track.thumb();
        let on_thumb = (start..start + size).contains(&pixel);
        let grab = if on_thumb { pixel - start } else { size / 2 };
        self.editor_scroll_drag = Some(Drag {
            context,
            pane: area.pane,
            track,
            grab,
            anchor_pixel: on_thumb.then_some(pixel),
        });
        self.update_editor_scroll(x, y);
        true
    }
    pub(super) fn update_editor_scroll(&mut self, x: i32, y: i32) {
        let Some(drag) = &self.editor_scroll_drag else {
            return;
        };
        if !self.pointer_source_available()
            || self.pointer_context(drag.pane).as_ref() != Some(&drag.context)
        {
            self.editor_scroll_drag = None;
            return;
        }
        let track = drag.track;
        let pixel = track.position(x, y);
        let target = if drag.anchor_pixel == Some(pixel) {
            track.offset
        } else {
            track.target(pixel, drag.grab)
        };
        let Some(view) = self.workbench.panes().view(drag.pane) else {
            return;
        };
        let old = match track.axis {
            Axis::Vertical => view.scroll_line(),
            Axis::Horizontal => self
                .workbench
                .rectangle_for_pane(drag.pane)
                .map_or(view.scroll_column(), |rect| rect.scroll_column),
        };
        let delta = if target >= old {
            (target - old).min(isize::MAX as usize) as isize
        } else {
            -((old - target).min(isize::MAX as usize) as isize)
        };
        match track.axis {
            Axis::Vertical => self
                .workbench
                .scroll_view_vertical(drag.pane, delta, track.page),
            Axis::Horizontal => {
                self.workbench
                    .scroll_view_horizontal(drag.pane, delta, track.limit)
            }
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GuiOptions;
    use jc_text::TextEncoding;

    #[test]
    fn native_editor_scroll_preserves_caret_and_uses_real_view_origins() {
        let mut state = State::new(&GuiOptions::default()).unwrap();
        state.width = 1100;
        state.height = 600;
        state.workbench.new_file("scroll.txt", TextEncoding::Utf8);
        let source = format!("{}\n", "fragmento ".repeat(120)).repeat(90);
        state.workbench.insert_text(&source).unwrap();
        let pane = state.workbench.panes().active();
        let caret = state.workbench.active_editor().unwrap().cursor;
        let area = paint::text_areas(&state).pop().unwrap();
        let frame = Frame::prepare(&mut state, &area);
        assert!(frame.overview.is_some());
        assert!(frame.horizontal.limit > 0);
        let horizontal = frame.horizontal.rect;
        let y = horizontal.top + 1;
        assert!(state.begin_editor_scroll(horizontal.right - 1, y));
        assert!(state.workbench.horizontal_scroll_for_pane(pane).unwrap() > 0);
        state.update_editor_scroll(horizontal.left - 1, y);
        assert_eq!(state.workbench.horizontal_scroll_for_pane(pane), Some(0));
        state.cancel_pointer();
        let overview = frame.overview.unwrap().0.rect;
        let x = overview.left + 1;
        assert!(state.begin_editor_scroll(x, overview.bottom - 1));
        assert!(state.workbench.panes().view(pane).unwrap().scroll_line() > 0);
        state.update_editor_scroll(x, overview.top - 1);
        assert_eq!(state.workbench.panes().view(pane).unwrap().scroll_line(), 0);
        assert_eq!(state.workbench.active_editor().unwrap().cursor, caret);
        assert_eq!(state.workbench.active_bytes().unwrap(), source.as_bytes());
    }
}
