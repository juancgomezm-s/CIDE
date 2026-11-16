//! Per-pane scroll rails and a sampled source overview. No document access in GDI drawing.
use super::{
    ffi::{Hdc, Rect},
    paint,
    pointer_input::PointerContext,
    shell, State,
};
use jc_app::{PaneId, TabId};
use std::{path::PathBuf, sync::Arc};

const RAIL: i32 = 10;
#[cfg(test)]
const MAP_WIDTH: i32 = 76;
const MAX_SAMPLES: usize = 256;
const PREFIX_BYTES: usize = 80;
const MAX_MARKS_PER_ROW: usize = 16;

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
    #[cfg(test)]
    pub fn new(pane: Rect, dpi: super::dpi::Dpi) -> Self {
        Self::with_minimap(pane, dpi, true, MAP_WIDTH as u16)
    }
    pub fn with_minimap(
        pane: Rect,
        dpi: super::dpi::Dpi,
        minimap_visible: bool,
        minimap_width: u16,
    ) -> Self {
        let top = (pane.top + dpi.pixels(shell::HEADER)).min(pane.bottom);
        let bottom = (pane.bottom - dpi.pixels(RAIL)).max(top);
        let right = (pane.right - dpi.pixels(RAIL)).max(pane.left);
        let width = dpi.pixels(i32::from(minimap_width.clamp(60, 128)));
        let map =
            (minimap_visible && right - pane.left - width >= dpi.pixels(320)).then_some(Rect {
                left: right - width,
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
    minimum_thumb: i32,
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
            .clamp(length.min(self.minimum_thumb), length);
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

#[derive(Clone, PartialEq, Eq)]
struct SampleKey {
    identity: u64,
    revision: u64,
    count: usize,
    tab: TabId,
    path: PathBuf,
    encoding: Option<jc_text::TextEncoding>,
}
#[derive(Clone, Copy)]
struct Mark {
    start: u8,
    end: u8,
    source_start: u8,
    kind: jc_app::SyntaxKind,
}
type MapRow = Vec<Mark>;
#[derive(Default)]
pub(super) struct MapCache {
    key: Option<SampleKey>,
    samples: Arc<[MapRow]>,
    lines: Vec<usize>,
    colored: usize,
    color_limited: bool,
}
impl MapCache {
    fn prepare(&mut self, app: &jc_app::Workbench, tab: TabId, lines: usize, pixels: i32) -> bool {
        let Some(doc) = app.document_by_tab(tab) else {
            return false;
        };
        let source = doc.session.document();
        let count = lines.min(MAX_SAMPLES).min((pixels / 2).max(0) as usize);
        let key = SampleKey {
            identity: source.recovery_identity(),
            revision: source.revision(),
            count,
            tab,
            path: doc.session.path().to_owned(),
            encoding: doc.session.encoding(),
        };
        if self.key.as_ref() != Some(&key) {
            self.lines = (0..count)
                .map(|index| {
                    if count <= 1 {
                        0
                    } else {
                        (index as u128 * lines.saturating_sub(1) as u128 / (count - 1) as u128)
                            as usize
                    }
                })
                .collect();
            self.colored = 0;
            self.color_limited = false;
            let marks = self
                .lines
                .iter()
                .map(|&line| {
                    let Some((range, _)) = app.line_range_for_tab(tab, line) else {
                        return Vec::new();
                    };
                    let end = range.start + range.len().min(PREFIX_BYTES);
                    let Ok(bytes) = source.read_range(range.start..end) else {
                        return Vec::new();
                    };
                    let mut column = 0usize;
                    let mut marks = Vec::<Mark>::with_capacity(8);
                    let mut token_start: Option<(u8, u8, u8)> = None;
                    for (byte_index, byte) in bytes.into_iter().enumerate() {
                        let next = if byte == b'\t' {
                            column + 4 - column % 4
                        } else {
                            column + 1
                        };
                        if byte.is_ascii_whitespace() {
                            if let Some((start, end, source_start)) = token_start.take() {
                                if marks.len() < MAX_MARKS_PER_ROW {
                                    marks.push(Mark {
                                        start,
                                        end,
                                        source_start,
                                        kind: jc_app::SyntaxKind::Plain,
                                    });
                                }
                            }
                            column = next.min(PREFIX_BYTES);
                            continue;
                        }
                        if let Some(start) = token_start.as_mut() {
                            start.1 = next.min(PREFIX_BYTES) as u8;
                        } else {
                            token_start = Some((
                                column.min(PREFIX_BYTES - 1) as u8,
                                next.min(PREFIX_BYTES) as u8,
                                byte_index.min(PREFIX_BYTES - 1) as u8,
                            ));
                        }
                        column = next.min(PREFIX_BYTES);
                    }
                    if let Some((start, end, source_start)) = token_start {
                        if marks.len() < MAX_MARKS_PER_ROW {
                            marks.push(Mark {
                                start,
                                end,
                                source_start,
                                kind: jc_app::SyntaxKind::Plain,
                            });
                        }
                    }
                    marks
                })
                .collect::<Vec<_>>();
            self.samples = marks.into();
            self.key = Some(key);
        }
        if self.color_limited || self.colored >= self.samples.len() {
            return false;
        }
        let finish = self.colored.saturating_add(24).min(self.samples.len());
        let syntax = app.syntax_samples(tab, &self.lines[self.colored..finish], PREFIX_BYTES);
        if syntax.limited {
            self.color_limited = true;
            self.colored = self.samples.len();
            return false;
        }
        let mut updated = self.samples.to_vec();
        for row in syntax.rows {
            let Some(index) = self.lines[self.colored..finish]
                .iter()
                .position(|line| *line == row.line)
                .map(|index| self.colored + index)
            else {
                continue;
            };
            if row.spans.is_empty() {
                continue;
            }
            let marks = std::mem::take(&mut updated[index]);
            for mark in marks {
                let start = row.source.start + usize::from(mark.source_start);
                let end = start + usize::from(mark.end - mark.start);
                let mut cursor = start;
                let mut piece = |from: usize, to: usize, kind| {
                    if from < to && updated[index].len() < MAX_MARKS_PER_ROW {
                        updated[index].push(Mark {
                            start: mark.start + (from - start) as u8,
                            end: mark.start + (to - start) as u8,
                            source_start: (from - row.source.start) as u8,
                            kind,
                        });
                    }
                };
                for span in &row.spans {
                    let from = span.source.start.max(cursor);
                    let to = span.source.end.min(end);
                    if from < to {
                        piece(cursor, from, jc_app::SyntaxKind::Plain);
                        piece(from, to, span.kind);
                        cursor = to;
                    }
                }
                piece(cursor, end, jc_app::SyntaxKind::Plain);
            }
        }
        self.samples = updated.into();
        self.colored = finish;
        self.colored < self.samples.len()
    }
}

pub(super) struct Frame {
    vertical: Track,
    horizontal: Track,
    overview: Option<(Rect, Track, Arc<[MapRow]>)>,
    pub(super) overview_pending: bool,
    dpi: super::dpi::Dpi,
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
            minimum_thumb: state.dpi.pixels(12),
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
                    app.invisibles_visible_for_tab(area.tab),
                )
                .max(area.scroll),
            page: area.columns.max(1),
            minimum_thumb: state.dpi.pixels(12),
        };
        let mut overview_pending = false;
        let overview = geometry.map.map(|rect| {
            let mini_line = state.dpi.pixels(2).max(1);
            let content_height =
                area.count
                    .saturating_mul(mini_line as usize)
                    .min((rect.bottom - rect.top).max(0) as usize) as i32;
            let channel = Rect {
                bottom: rect.top + content_height,
                ..rect
            };
            let cache = &mut state.editor_maps[usize::from(area.pane == PaneId::Secondary)];
            overview_pending = cache.prepare(app, area.tab, area.count, content_height);
            (
                rect,
                Track {
                    rect: channel,
                    ..vertical
                },
                Arc::clone(&cache.samples),
            )
        });
        Self {
            vertical,
            horizontal,
            overview,
            overview_pending,
            dpi: state.dpi,
        }
    }

    pub unsafe fn draw(&self, dc: Hdc) -> Result<(), String> {
        for track in [self.vertical, self.horizontal] {
            paint::fill(dc, track.rect, shell::SURFACE)?;
            if track.limit != 0 {
                let mut thumb = track.thumb_rect();
                match track.axis {
                    Axis::Vertical => {
                        thumb.left += self.dpi.pixels(2);
                        thumb.right -= self.dpi.pixels(2);
                    }
                    Axis::Horizontal => {
                        thumb.top += self.dpi.pixels(2);
                        thumb.bottom -= self.dpi.pixels(2);
                    }
                }
                paint::fill(dc, thumb, shell::MUTED)?;
            }
        }
        if let Some((channel, map, samples)) = &self.overview {
            paint::fill(dc, *channel, shell::SURFACE)?;
            let window = map.thumb_rect();
            paint::fill(dc, window, shell::SELECTED)?;
            let left = map.rect.left + self.dpi.pixels(3);
            let width = map.rect.right - left - self.dpi.pixels(3);
            let row_height = (map.length() / samples.len().max(1) as i32).clamp(1, 3);
            let mut marks = Vec::with_capacity(samples.len().saturating_mul(MAX_MARKS_PER_ROW));
            for (index, row) in samples.iter().enumerate() {
                let y = map.rect.top + scale(index, samples.len(), map.length());
                for mark in row {
                    let x = left + scale(usize::from(mark.start), PREFIX_BYTES, width);
                    let end = left + scale(usize::from(mark.end), PREFIX_BYTES, width);
                    marks.push((
                        Rect {
                            left: x,
                            right: end.max(x + 1).min(map.rect.right),
                            top: y,
                            bottom: (y + row_height).min(map.rect.bottom),
                        },
                        syntax_color(mark.kind),
                    ));
                }
            }
            paint::fill_color_marks(dc, marks.into_iter())?;
            // Keep the visible-range boundary clear above the miniature code.
            paint::fill(
                dc,
                Rect {
                    bottom: window.top + self.dpi.pixels(1),
                    ..window
                },
                shell::ACCENT,
            )?;
            paint::fill(
                dc,
                Rect {
                    top: window.bottom - self.dpi.pixels(1),
                    ..window
                },
                shell::ACCENT,
            )?;
        }
        Ok(())
    }
}

fn syntax_color(kind: jc_app::SyntaxKind) -> u32 {
    if kind == jc_app::SyntaxKind::Plain {
        return shell::MUTED;
    }
    let rgb = jc_app::syntax_theme::foreground(kind);
    u32::from(rgb.r) | (u32::from(rgb.g) << 8) | (u32::from(rgb.b) << 16)
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
        } else if let Some((_, track, _)) = frame
            .overview
            .as_ref()
            .filter(|(_, track, _)| contains(track.rect, x, y))
        {
            *track
        } else {
            return false;
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
    pub(super) fn update_editor_scroll(&mut self, x: i32, y: i32) -> bool {
        let Some(drag) = &self.editor_scroll_drag else {
            return false;
        };
        if !self.pointer_source_available()
            || self.pointer_context(drag.pane).as_ref() != Some(&drag.context)
        {
            self.editor_scroll_drag = None;
            return false;
        }
        let track = drag.track;
        let pixel = track.position(x, y);
        let target = if drag.anchor_pixel == Some(pixel) {
            track.offset
        } else {
            track.target(pixel, drag.grab)
        };
        let Some(view) = self.workbench.panes().view(drag.pane) else {
            return false;
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
        delta != 0
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
        assert!(state.update_editor_scroll(horizontal.left - 1, y));
        assert!(!state.update_editor_scroll(horizontal.left - 1, y));
        assert_eq!(state.workbench.horizontal_scroll_for_pane(pane), Some(0));
        state.cancel_pointer();
        let overview = frame.overview.unwrap().1.rect;
        let x = overview.left + 1;
        assert!(state.begin_editor_scroll(x, overview.bottom - 1));
        assert!(state.workbench.panes().view(pane).unwrap().scroll_line() > 0);
        assert!(state.update_editor_scroll(x, overview.top - 1));
        assert!(!state.update_editor_scroll(x, overview.top - 1));
        assert_eq!(state.workbench.panes().view(pane).unwrap().scroll_line(), 0);
        assert_eq!(state.workbench.active_editor().unwrap().cursor, caret);
        assert_eq!(state.workbench.active_bytes().unwrap(), source.as_bytes());
    }

    #[test]
    fn native_scroll_geometry_scales_and_remains_inside_compact_panes() {
        for raw in [96, 144, 192] {
            let dpi = super::super::dpi::Dpi::from_raw(raw);
            let pane = Rect {
                left: 40,
                top: 80,
                right: 40 + dpi.pixels(600),
                bottom: 80 + dpi.pixels(400),
            };
            let geometry = Geometry::new(pane, dpi);
            assert_eq!(
                geometry.vertical.right - geometry.vertical.left,
                dpi.pixels(RAIL)
            );
            assert_eq!(
                geometry.horizontal.bottom - geometry.horizontal.top,
                dpi.pixels(RAIL)
            );
            assert_eq!(
                geometry.map.unwrap().right - geometry.map.unwrap().left,
                dpi.pixels(MAP_WIDTH)
            );
            let compact = Geometry::new(
                Rect {
                    right: 42,
                    bottom: 82,
                    ..pane
                },
                dpi,
            );
            assert!(compact.map.is_none());
            assert!(compact.text.left <= compact.text.right);
            assert!(compact.text.top <= compact.text.bottom);
        }
    }

    #[test]
    fn minimap_is_configurable_and_colors_bounded_samples_off_viewport() {
        let dpi = super::super::dpi::Dpi::from_raw(96);
        let pane = Rect {
            left: 0,
            top: 0,
            right: 900,
            bottom: 620,
        };
        let narrow = Geometry::with_minimap(pane, dpi, true, 60);
        let wide = Geometry::with_minimap(pane, dpi, true, 96);
        assert_eq!(narrow.map.unwrap().right - narrow.map.unwrap().left, 60);
        assert_eq!(wide.map.unwrap().right - wide.map.unwrap().left, 96);
        assert!(Geometry::with_minimap(pane, dpi, false, 96).map.is_none());

        let mut state = State::new(&GuiOptions::default()).unwrap();
        state.width = 1100;
        state.height = 700;
        state.workbench.new_file("overview.p", TextEncoding::Utf8);
        let source = "IF condition THEN DISPLAY \"value\".\n".repeat(160);
        state.workbench.insert_text(&source).unwrap();
        let area = paint::text_areas(&state)
            .into_iter()
            .find(|area| area.pane == state.workbench.panes().active())
            .unwrap();
        // Several short owner-thread batches color the whole bounded overview,
        // including samples far outside the currently visible editor rows.
        for _ in 0..12 {
            let frame = Frame::prepare(&mut state, &area);
            if !frame.overview_pending {
                break;
            }
        }
        let rows = &state.editor_maps[0].samples;
        assert!(rows.len() > area.capacity_rows);
        assert!(rows
            .iter()
            .flatten()
            .any(|mark| mark.kind == jc_app::SyntaxKind::Keyword));
        assert!(state.editor_maps[0].colored >= rows.len());
    }
}
