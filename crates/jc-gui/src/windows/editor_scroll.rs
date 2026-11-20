//! Per-pane scroll rails and a sampled source overview. No document access in GDI drawing.
use super::{
    appearance,
    ffi::{Hdc, Rect},
    paint,
    pointer_input::PointerContext,
    shell, State,
};
use jc_app::{PaneId, SyntaxKind, SyntaxProfile, TabId, ViewId, Workbench};
use jc_text::{DecodedScalars, TextEncoding};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};

mod minimap;

const RAIL: i32 = 10;
const MAX_SAMPLES: usize = 512;
const PREFIX_BYTES: usize = 192;
const MAX_MARKS_PER_ROW: usize = 48;
const RESIDENT_SAMPLE_BATCH: usize = 32;
const PAGED_SAMPLE_BYTES: usize = 512;
const PAGED_SAMPLE_BATCH: usize = 16;

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
        Self::with_minimap(pane, dpi, true, appearance::DEFAULT_MINIMAP)
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
        let preferred = minimap_width.clamp(appearance::MIN_MINIMAP, appearance::MAX_MINIMAP);
        let width = dpi
            .pixels(i32::from(preferred))
            .min(right - pane.left - dpi.pixels(320));
        let map = (minimap_visible
            && bottom > top
            && width >= dpi.pixels(i32::from(appearance::MIN_MINIMAP)))
        .then_some(Rect {
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

#[derive(Clone, Copy, PartialEq, Eq)]
struct SampleKey {
    identity: u64,
    revision: u64,
    lines: usize,
    count: usize,
    tab: TabId,
    profile: SyntaxProfile,
    encoding: Option<TextEncoding>,
}

#[derive(Clone, Copy)]
struct Mark {
    start: u8,
    end: u8,
    kind: SyntaxKind,
}
#[derive(Clone, Default)]
struct MapRow {
    marks: Vec<Mark>,
    glyphs: Vec<u8>,
}

fn row_marks(
    bytes: &[u8],
    complete: bool,
    profile: SyntaxProfile,
    encoding: Option<TextEncoding>,
) -> MapRow {
    let mut colors = [SyntaxKind::Plain; PREFIX_BYTES];
    if encoding.is_some() && profile != SyntaxProfile::Plain {
        jc_app::scan_syntax_snippet(profile, bytes, complete, |span| {
            colors[span.source].fill(span.kind);
        });
    }
    let encoding = encoding.unwrap_or(TextEncoding::Iso8859_1);
    // The file-level BOM has already been handled by the caller.
    let scalars = DecodedScalars::new(bytes, encoding, 1);
    let mut column = 0u8;
    let mut row = MapRow::default();
    for scalar in scalars {
        let ch = scalar.character;
        let start = column;
        let width = match ch {
            '\t' => 4 - column % 4,
            ch if ch.is_ascii() => 1,
            ch => jc_text::grapheme::width(ch.encode_utf8(&mut [0; 4])) as u8,
        };
        column = column.saturating_add(width).min(PREFIX_BYTES as u8);
        if ch.is_whitespace() || start == column {
            continue;
        }
        let kind = colors[scalar.source_range.start - 1];
        if let Some(mark) = row
            .marks
            .last_mut()
            .filter(|mark| mark.end == start && mark.kind == kind)
        {
            mark.end = column;
        } else if row.marks.len() < MAX_MARKS_PER_ROW {
            row.marks.push(Mark {
                start,
                end: column,
                kind,
            });
        } else {
            break;
        }
        row.glyphs.resize(usize::from(start), b' ');
        let glyph = if ch.is_ascii_graphic() { ch as u8 } else { 127 };
        row.glyphs.resize(usize::from(column), glyph);
    }
    row
}

fn sample_position(index: usize, count: usize, last: usize) -> usize {
    (index as u128 * last as u128 / count.saturating_sub(1).max(1) as u128) as usize
}

#[derive(Default)]
pub(super) struct MapCache {
    key: Option<SampleKey>,
    samples: Arc<[MapRow]>,
    prepared: usize,
    paged: PagedMap,
    raster: minimap::Cache,
}
impl MapCache {
    fn prepare(&mut self, app: &Workbench, tab: TabId, lines: usize, pixels: i32) -> bool {
        let Some(doc) = app.document_by_tab(tab) else {
            return false;
        };
        let source = doc.session.document();
        let count = lines.min(MAX_SAMPLES).min((pixels / 2).max(0) as usize);
        let key = SampleKey {
            identity: source.recovery_identity(),
            revision: source.revision(),
            lines,
            count,
            tab,
            profile: app.syntax_profile_for_tab(tab),
            encoding: doc.session.encoding(),
        };
        if self.key != Some(key) {
            self.prepared = 0;
            self.samples = vec![MapRow::default(); count].into();
            self.key = Some(key);
        }
        if self.prepared == count {
            return false;
        }
        let end = (self.prepared + RESIDENT_SAMPLE_BATCH).min(count);
        let samples = Arc::make_mut(&mut self.samples);
        for (index, sample) in samples.iter_mut().enumerate().take(end).skip(self.prepared) {
            let line = sample_position(index, count, lines.saturating_sub(1));
            let Some((range, _)) = app.line_range_for_tab(tab, line) else {
                continue;
            };
            let end = range.start + range.len().min(PREFIX_BYTES);
            let Ok(bytes) = source.read_range(range.start..end) else {
                continue;
            };
            let bytes = sample_without_bom(&bytes, range.start == 0, key.encoding);
            *sample = row_marks(bytes, end == range.end, key.profile, key.encoding);
        }
        self.prepared = end;
        end < count
    }
}

fn sample_without_bom(bytes: &[u8], first: bool, encoding: Option<TextEncoding>) -> &[u8] {
    if first && encoding == Some(TextEncoding::Utf8) {
        bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes)
    } else {
        bytes
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct PagedMapKey {
    tab: TabId,
    identity: u64,
    revision: u64,
    bytes: usize,
    count: usize,
    profile: SyntaxProfile,
    encoding: Option<TextEncoding>,
}

impl PagedMapKey {
    fn for_area(app: &Workbench, area: &paint::TextArea) -> Option<Self> {
        let rect = area.geometry.map?;
        let session = &app.document_by_tab(area.tab)?.session;
        let doc = session.document();
        if !doc.is_paged() || doc.is_empty() {
            return None;
        }
        let count = ((rect.bottom - rect.top).max(0) as usize / 2)
            .min(MAX_SAMPLES)
            .min(doc.len().div_ceil(PREFIX_BYTES));
        (count != 0).then(|| Self {
            tab: area.tab,
            identity: doc.recovery_identity(),
            revision: doc.revision(),
            bytes: doc.len(),
            count,
            profile: app.syntax_profile_for_tab(area.tab),
            encoding: session.encoding(),
        })
    }

    fn sample_offset(self, index: usize) -> usize {
        sample_position(
            index,
            self.count,
            self.bytes.saturating_sub(PAGED_SAMPLE_BYTES),
        )
    }
}

type PagedBatch = Result<(usize, Vec<MapRow>), String>;

struct PagedMapWorker {
    result: mpsc::Receiver<PagedBatch>,
    cancel: Arc<AtomicBool>,
}

impl Drop for PagedMapWorker {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

#[derive(Default)]
struct PagedMap {
    key: Option<PagedMapKey>,
    worker: Option<PagedMapWorker>,
    samples: Arc<[MapRow]>,
}

impl PagedMap {
    // Only the GUI's task poll admits I/O or publishes results.
    fn poll(
        &mut self,
        app: &Workbench,
        wanted: Option<PagedMapKey>,
        wakeup: &jc_app::Wakeup,
    ) -> Result<bool, String> {
        let mut changed = false;
        if let Some(worker) = &self.worker {
            if self.key != wanted {
                worker.cancel.store(true, Ordering::Relaxed);
                // The same key may become visible again before the old
                // worker finishes. Its partial result must stay rejected.
                self.key = None;
                self.samples = Arc::default();
            }
            match worker.result.try_recv() {
                Ok(Ok((start, rows))) if self.key.is_some() && self.key == wanted => {
                    let samples = Arc::make_mut(&mut self.samples);
                    for (sample, row) in samples[start..].iter_mut().zip(rows) {
                        *sample = row;
                    }
                    changed = true;
                    // A worker can publish the next batch before this delivery
                    // is acknowledged. Poll again even if its wake coalesces.
                    wakeup.signal();
                }
                Ok(Err(error)) if self.key.is_some() && self.key == wanted => {
                    self.worker = None;
                    return Err(error);
                }
                Ok(_) => {
                    wakeup.signal();
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.worker = None;
                }
            }
        }
        if self.worker.is_some() || self.key == wanted {
            return Ok(changed);
        }
        self.key = wanted;
        let Some(key) = wanted else {
            self.samples = Arc::default();
            return Ok(changed);
        };
        self.samples = vec![MapRow::default(); key.count].into();
        let doc = app
            .document_by_tab(key.tab)
            .ok_or("El documento del minimapa ya no está abierto")?;
        let source = doc.session.document();
        let plans = (0..key.count)
            .map(|index| {
                let start = key.sample_offset(index);
                let end = start.saturating_add(PAGED_SAMPLE_BYTES).min(key.bytes);
                source.prepare_paged_read(start..end, PAGED_SAMPLE_BYTES)
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;

        let (sender, receiver) = mpsc::sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        let wakeup = wakeup.clone();
        std::thread::Builder::new()
            .name("cide-minimapa".into())
            .spawn(move || {
                let sampled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut batch = Vec::with_capacity(PAGED_SAMPLE_BATCH);
                    let mut first = 0;
                    for (index, plan) in plans.into_iter().enumerate() {
                        if worker_cancel.load(Ordering::Relaxed) {
                            break;
                        }
                        let range = plan.range();
                        let mut bytes = Vec::with_capacity(range.len());
                        if let Err(error) = plan.visit(|chunk| {
                            bytes.extend_from_slice(chunk);
                            Ok(())
                        }) {
                            let _ =
                                sender.send(Err(format!("No se pudo leer el minimapa: {error}")));
                            break;
                        }
                        batch.push(paged_sample_row(
                            &bytes,
                            index == 0,
                            range.end == key.bytes,
                            key.profile,
                            key.encoding,
                        ));
                        if batch.len() == PAGED_SAMPLE_BATCH || index + 1 == key.count {
                            let rows = std::mem::replace(
                                &mut batch,
                                Vec::with_capacity(PAGED_SAMPLE_BATCH),
                            );
                            if sender.send(Ok((first, rows))).is_err() {
                                break;
                            }
                            first = index + 1;
                            wakeup.signal();
                        }
                    }
                }));
                if sampled.is_err() {
                    let _ = sender.send(Err("Falló el trabajador del minimapa".into()));
                }
                drop(sender);
                wakeup.signal();
            })
            .map_err(|error| format!("No se pudo iniciar el minimapa: {error}"))?;
        self.worker = Some(PagedMapWorker {
            result: receiver,
            cancel,
        });
        Ok(true)
    }
}

fn paged_sample_row(
    bytes: &[u8],
    first: bool,
    eof: bool,
    profile: SyntaxProfile,
    encoding: Option<TextEncoding>,
) -> MapRow {
    let mut lines = bytes.split_inclusive(|byte| matches!(byte, b'\r' | b'\n'));
    let fragment = lines.next().unwrap_or_default();
    let (line, profile) = if first {
        (sample_without_bom(fragment, true, encoding), profile)
    } else {
        lines
            .find(|line| line.iter().any(|byte| !byte.is_ascii_whitespace()))
            .map_or((fragment, SyntaxProfile::Plain), |line| (line, profile))
    };
    let end = line
        .last()
        .is_some_and(|byte| matches!(byte, b'\r' | b'\n'));
    let body = &line[..line.len() - usize::from(end)];
    row_marks(
        &body[..body.len().min(PREFIX_BYTES)],
        (end || eof) && body.len() <= PREFIX_BYTES,
        profile,
        encoding,
    )
}

pub(super) struct Frame {
    vertical: Track,
    horizontal: Track,
    overview: Option<minimap::Frame>,
    theme: appearance::Theme,
    pub(super) overview_pending: bool,
    dpi: super::dpi::Dpi,
}
impl Frame {
    pub fn prepare(state: &mut State, area: &paint::TextArea) -> Self {
        let geometry = area.geometry;
        let app = &state.workbench;
        let page = area.capacity_rows.max(1);
        let mut vertical = Track {
            axis: Axis::Vertical,
            rect: geometry.vertical,
            offset: area.first,
            limit: area.count.saturating_sub(page),
            page,
            minimum_thumb: state.dpi.pixels(12),
        };
        let paged = app.is_paged_tab(area.tab);
        if paged {
            if let Some(document) = app.document_by_tab(area.tab) {
                vertical.limit = document.session.document().len();
            }
            vertical.page = 1;
            if let Some(viewport) = app.paged_viewport_for_tab(area.tab) {
                vertical.offset = viewport
                    .rows()
                    .get(area.first)
                    .map_or(viewport.origin(), |row| row.content.start);
                vertical.page = viewport
                    .rows()
                    .get(area.first + area.rows.saturating_sub(1))
                    .map_or(1, |row| {
                        row.content.end.saturating_sub(vertical.offset).max(1)
                    });
            }
        }
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
            let cache = &mut state.editor_maps[usize::from(area.pane == PaneId::Secondary)];
            if paged {
                let key = PagedMapKey::for_area(app, area);
                overview_pending = cache.paged.key != key && cache.paged.worker.is_none();
                let samples = if cache.paged.key == key {
                    Arc::clone(&cache.paged.samples)
                } else {
                    Arc::default()
                };
                return cache.raster.prepare(
                    rect,
                    Track { rect, ..vertical },
                    samples,
                    state.dpi,
                    state.editor_theme,
                );
            }
            let mini_line = state.dpi.pixels(3).max(1);
            let content_height =
                area.count
                    .saturating_mul(mini_line as usize)
                    .min((rect.bottom - rect.top).max(0) as usize) as i32;
            let channel = Rect {
                bottom: rect.top + content_height,
                ..rect
            };
            overview_pending = cache.prepare(app, area.tab, area.count, content_height);
            cache.raster.prepare(
                rect,
                Track {
                    rect: channel,
                    ..vertical
                },
                Arc::clone(&cache.samples),
                state.dpi,
                state.editor_theme,
            )
        });
        Self {
            vertical,
            horizontal,
            overview,
            overview_pending,
            theme: state.editor_theme,
            dpi: state.dpi,
        }
    }

    pub unsafe fn draw(&self, dc: Hdc) -> Result<(), String> {
        for track in [self.vertical, self.horizontal] {
            paint::fill(dc, track.rect, self.theme.role_surface())?;
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
                paint::fill(dc, thumb, self.theme.muted())?;
            }
        }
        if let Some(map) = &self.overview {
            map.draw(dc, self.dpi, self.theme)?;
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
    paged: Option<PagedDrag>,
}

struct PagedDrag {
    tab: TabId,
    identity: u64,
    revision: u64,
    view: ViewId,
    last_target: Option<usize>,
}

impl State {
    pub(super) fn poll_paged_editor_maps(&mut self) -> bool {
        let mut wanted = [None; 2];
        for area in paint::text_areas(self) {
            wanted[usize::from(area.pane == PaneId::Secondary)] =
                PagedMapKey::for_area(&self.workbench, &area);
        }
        let mut changed = false;
        for (cache, key) in self.editor_maps.iter_mut().zip(wanted) {
            match cache.paged.poll(&self.workbench, key, &self.wakeup) {
                Ok(updated) => changed |= updated,
                Err(message) => {
                    self.status = message;
                    changed = true;
                }
            }
        }
        changed
    }

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
        } else if let Some(map) = frame
            .overview
            .as_ref()
            .filter(|map| contains(map.track.rect, x, y))
        {
            map.track
        } else {
            return false;
        };
        let paged = if track.axis == Axis::Vertical && self.workbench.is_paged_tab(area.tab) {
            if !self.workbench.focus_editor_pane(area.pane) {
                return false;
            }
            self.explorer_focused = false;
            self.bottom.focused = false;
            let doc = self
                .workbench
                .document_by_tab(area.tab)
                .unwrap()
                .session
                .document();
            let Some(view) = self
                .workbench
                .panes()
                .view(area.pane)
                .and_then(|view| view.view_id())
            else {
                return false;
            };
            Some(PagedDrag {
                tab: area.tab,
                identity: doc.recovery_identity(),
                revision: doc.revision(),
                view,
                last_target: Some(track.offset),
            })
        } else {
            None
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
            paged,
        });
        self.update_editor_scroll(x, y);
        true
    }
    pub(super) fn update_editor_scroll(&mut self, x: i32, y: i32) -> bool {
        let Some(drag) = &self.editor_scroll_drag else {
            return false;
        };
        let context_matches = if let Some(paged) = &drag.paged {
            self.workbench.panes().active() == drag.pane
                && self.workbench.panes().view(drag.pane).is_some_and(|view| {
                    view.view_id() == Some(paged.view)
                        && view
                            .location()
                            .is_some_and(|location| location.tab == paged.tab)
                })
                && self
                    .workbench
                    .document_by_tab(paged.tab)
                    .is_some_and(|doc| {
                        let source = doc.session.document();
                        source.recovery_identity() == paged.identity
                            && source.revision() == paged.revision
                    })
        } else {
            self.pointer_context(drag.pane).as_ref() == Some(&drag.context)
        };
        if !self.pointer_source_available() || !context_matches {
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
        if let Some(paged) = self
            .editor_scroll_drag
            .as_mut()
            .and_then(|drag| drag.paged.as_mut())
        {
            if paged.last_target == Some(target) {
                return false;
            }
            paged.last_target = Some(target);
            if let Err(error) = self.workbench.go_to_paged_near_byte(target) {
                self.error(error);
            }
            return true;
        }
        let Some(drag) = &self.editor_scroll_drag else {
            return false;
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
    use std::path::PathBuf;

    #[test]
    #[cfg(target_pointer_width = "64")]
    fn paged_minimap_samples_the_full_extent_and_drags_to_safe_boundaries() {
        use std::{
            fs,
            time::{Duration, Instant, SystemTime, UNIX_EPOCH},
        };

        let root = std::env::temp_dir().join(format!(
            "cide-minimapa-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let path = root.join("muestra.txt");
        struct Fixture(PathBuf, PathBuf);
        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = fs::remove_file(&self.1);
                let _ = fs::remove_dir(&self.0);
            }
        }
        let _fixture = Fixture(root.clone(), path.clone());
        let text = "    resumen ☀ valor\r\n".repeat(20_000);
        fs::write(&path, &text).unwrap();
        let mut state = State::new(&GuiOptions::default()).unwrap();
        state.width = 1100;
        state.height = 660;
        assert_eq!(
            state
                .workbench
                .request_file_open_in_root(&root, &path, Some(TextEncoding::Utf8), false, 16)
                .unwrap(),
            None
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        let tab = loop {
            if let Some(result) = state.workbench.poll_large_open() {
                break result.unwrap();
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        };
        let area = paint::text_areas(&state).pop().unwrap();
        let frame = Frame::prepare(&mut state, &area);
        assert!(frame.overview_pending);
        assert!(frame.overview.as_ref().unwrap().samples.is_empty());
        loop {
            state.poll_paged_editor_maps();
            let map = &state.editor_maps[0].paged;
            if map.key.is_some() && map.worker.is_none() {
                assert!(map.samples.len() <= MAX_SAMPLES);
                assert!(map.samples.iter().all(|row| !row.marks.is_empty()));
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        let frame = Frame::prepare(&mut state, &area);
        let track = frame.overview.unwrap().track;
        assert_eq!(track.limit, text.len());
        let large = PagedMapKey {
            bytes: 7 * 1024 * 1024 * 1024,
            count: MAX_SAMPLES,
            ..state.editor_maps[0].paged.key.unwrap()
        };
        assert_eq!(large.sample_offset(0), 0);
        assert_eq!(
            large.sample_offset(large.count - 1) + PAGED_SAMPLE_BYTES,
            large.bytes
        );
        let large_track = Track {
            limit: large.bytes,
            offset: (1usize << 32) + 7,
            page: 64 * 1024,
            ..track
        };
        assert!(large_track.thumb().0 > track.rect.top + track.length() / 2);
        assert_eq!(large_track.target(track.rect.bottom - 1, 0), large.bytes);
        let x = track.rect.left + 1;
        let y = track.rect.top + track.length() / 2;
        assert!(state.begin_editor_scroll(x, y));
        loop {
            if let Some(result) = state.workbench.poll_large_view() {
                result.unwrap();
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        let middle = state.workbench.active_paged_byte().unwrap();
        assert!((text.len() / 3..text.len() * 2 / 3).contains(&middle));
        assert!(text.is_char_boundary(middle));
        assert!(state.workbench.position_for_tab(tab, middle).is_some());
        assert!(state.update_editor_scroll(x, track.rect.bottom - 1));
        loop {
            if let Some(result) = state.workbench.poll_large_view() {
                result.unwrap();
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert_eq!(state.workbench.active_paged_byte(), Some(text.len()));
        assert_eq!(fs::read(&path).unwrap(), text.as_bytes());
    }

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
        let overview = frame.overview.unwrap().track.rect;
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
                dpi.pixels(i32::from(appearance::DEFAULT_MINIMAP))
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
        let wide = Geometry::with_minimap(pane, dpi, true, 200);
        assert_eq!(narrow.map.unwrap().right - narrow.map.unwrap().left, 60);
        assert_eq!(wide.map.unwrap().right - wide.map.unwrap().left, 200);
        assert!(Geometry::with_minimap(pane, dpi, false, 96).map.is_none());
        let compact = Geometry::with_minimap(Rect { right: 430, ..pane }, dpi, true, 200);
        assert_eq!(compact.text.right - compact.text.left, 320);
        assert_eq!(compact.map.unwrap().right - compact.map.unwrap().left, 100);

        let mut state = State::new(&GuiOptions::default()).unwrap();
        state.width = 1100;
        state.height = 700;
        state.workbench.new_file("overview.p", TextEncoding::Utf8);
        let source = format!(
            "\tDEFINE VARIABLE nombre AS CHARACTER. IF YES THEN DISPLAY \"área\".{}42.\n",
            " ".repeat(40)
        )
        .repeat(160);
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
        let offscreen = &rows[rows.len() - 2];
        assert_eq!(offscreen.marks[0].start, 4);
        for kind in [
            SyntaxKind::Keyword,
            SyntaxKind::Type,
            SyntaxKind::Boolean,
            SyntaxKind::String,
        ] {
            assert!(offscreen.marks.iter().any(|mark| mark.kind == kind));
        }
        assert!(offscreen
            .marks
            .iter()
            .any(|mark| mark.kind == SyntaxKind::Number && mark.start > 80));
        assert_eq!(state.editor_maps[0].prepared, rows.len());
    }

    #[test]
    fn paged_minimap_colors_complete_lines_without_claiming_a_partial_first_token() {
        let profile = SyntaxProfile::Abl;
        let encoding = Some(TextEncoding::Utf8);
        let sample = paged_sample_row(
            b"fragmento\r\n\tDISPLAY YES, 42.",
            false,
            true,
            profile,
            encoding,
        );
        assert_eq!(sample.marks[0].start, 4);
        assert_eq!(sample.marks[0].kind, SyntaxKind::Keyword);
        assert!(sample
            .marks
            .iter()
            .any(|mark| mark.kind == SyntaxKind::Boolean));
        let fragment = paged_sample_row(b"DISPLAY YES,", false, false, profile, encoding);
        assert!(fragment
            .marks
            .iter()
            .all(|mark| mark.kind == SyntaxKind::Plain));
    }
}
