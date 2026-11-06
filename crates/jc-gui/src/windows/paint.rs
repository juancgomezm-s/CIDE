//! Preparation owns only visible cells. Painting borrows that presentation and
//! never accesses Workbench, document caches or a mutable editor widget.
use super::{editor_scroll, ffi::*, wide, State};
use jc_app::{PaneId, SyntaxKind, SyntaxProfile};
use jc_text::TextPosition;
use std::ptr::{null, null_mut};

const BACKGROUND: u32 = super::shell::BACKGROUND;
const FOREGROUND: u32 = super::shell::TEXT;
const DIM: u32 = super::shell::MUTED;
const SELECTED: u32 = super::shell::SELECTED;
const ACCENT: u32 = super::shell::ACCENT;
fn search_at(matches: &[jc_app::SearchMatch], source: std::ops::Range<usize>) -> bool {
    // S0 delivers source-ordered ranges. One binary lookup per visible cell;
    // diagnostic escape fragments retain their original scalar's byte start.
    let index = matches.partition_point(|hit| hit.range.end <= source.start);
    matches
        .get(index)
        .is_some_and(|hit| hit.range.start < source.end)
}
struct Cell {
    glyph: jc_app::Glyph,
    column: usize,
    width: usize,
    selected: bool,
    search: bool,
    marker: bool,
    kind: SyntaxKind,
}
impl Cell {
    fn foreground(&self) -> u32 {
        if self.selected || self.search {
            return FOREGROUND;
        }
        if self.marker {
            return DIM;
        }
        let color = jc_app::syntax_theme::foreground(self.kind);
        u32::from(color.r) | (u32::from(color.g) << 8) | (u32::from(color.b) << 16)
    }
    fn background(&self) -> u32 {
        if self.selected || self.search {
            SELECTED
        } else {
            BACKGROUND
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SyntaxCell {
    pub ch: char,
    pub kind: SyntaxKind,
    pub selected: bool,
    pub search: bool,
    pub marker: bool,
    pub foreground: u32,
    pub background: u32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SyntaxBudget {
    pub processed_bytes: usize,
    pub cache_bytes: usize,
    pub scanned_to: usize,
    pub pending: bool,
    pub limited: bool,
}
impl From<&Cell> for SyntaxCell {
    fn from(cell: &Cell) -> Self {
        Self {
            ch: cell.glyph.first(),
            kind: cell.kind,
            selected: cell.selected,
            search: cell.search,
            marker: cell.marker,
            foreground: cell.foreground(),
            background: cell.background(),
        }
    }
}
struct Row {
    number: Vec<u16>,
    cells: Vec<Cell>,
}
fn run_length(cells: &[Cell], start: usize, safe: &[bool; 128]) -> usize {
    let first = &cells[start];
    if first.width != 1
        || !matches!(first.glyph,jc_app::Glyph::Scalar(c) if c.is_ascii() && safe[c as usize])
    {
        return 1;
    }
    cells[start..]
        .iter()
        .take(256)
        .enumerate()
        .take_while(|(n, cell)| {
            cell.width == 1
                && cell.column == first.column + n
                && cell.foreground() == first.foreground()
                && cell.background() == first.background()
                && matches!(cell.glyph,jc_app::Glyph::Scalar(c) if c.is_ascii() && safe[c as usize])
        })
        .count()
        .max(1)
}
struct Pane {
    id: PaneId,
    first_line: usize,
    syntax: SyntaxBudget,
    rect: Rect,
    rows: Vec<Row>,
    text_x: i32,
    text_y: i32,
    caret: Option<(i32, i32)>,
    scroll: editor_scroll::Frame,
}
pub(super) struct Frame {
    pub width: i32,
    pub height: i32,
    font: Handle,
    ui_font: Handle,
    ascii_runs: [bool; 128],
    cell_width: i32,
    cell_height: i32,
    tabs: super::tab_bar::Bars,
    chrome: super::chrome::Frame,
    status: Vec<u16>,
    status_bar: super::status_bar::Frame,
    panes: Vec<Pane>,
    selector: Option<super::selector::SelectorPresentation>,
    search: Option<super::search_panel::SearchPresentation>,
    navigation: Option<super::navigation_panel::NavigationPresentation>,
    project: Option<super::project_panel::ProjectPresentation>,
    replacement: Option<super::replace_panel::ReplaceFrame>,
    git: Option<super::git_workspace::Frame>,
    comparison: Option<super::open_diff_panel::ComparisonFrame>,
    source: Option<super::git_panel::GitFrame>,
    snippet: Option<super::git_panel::GitFrame>,
    markdown: Option<super::markdown_panel::MarkdownFrame>,
    explorer: Option<super::explorer_panel::ExplorerFrame>,
    outline: Option<super::outline_panel::OutlineFrame>,
    bottom: Option<super::bottom_panel::BottomFrame>,
}
impl Frame {
    pub(super) fn git_hits(&self) -> Option<super::git_workspace::Hits> {
        self.git.as_ref().map(|frame| frame.hits.clone())
    }
    pub(super) fn source_draw_counts(&self) -> (usize, usize) {
        let mut cells = 0;
        let mut runs = 0;
        for row in self.panes.iter().flat_map(|p| &p.rows) {
            cells += row.cells.len();
            let mut at = 0;
            while at < row.cells.len() {
                at += run_length(&row.cells, at, &self.ascii_runs);
                runs += 1;
            }
        }
        (cells, runs)
    }
    pub(super) fn tab_hits(&self) -> super::tab_bar::Bars {
        self.tabs.clone()
    }
    /// Inspection of already-owned visible presentation only. These helpers do
    /// not consult Workbench, lexical caches or source data during rasterization.
    pub(super) fn syntax_cell_at(
        &self,
        pane: PaneId,
        line: usize,
        column: usize,
    ) -> Option<SyntaxCell> {
        let pane = self.panes.iter().find(|p| p.id == pane)?;
        pane.rows
            .get(line.checked_sub(pane.first_line)?)?
            .cells
            .iter()
            .find(|cell| column >= cell.column && column < cell.column + cell.width)
            .map(SyntaxCell::from)
    }
    pub(super) fn syntax_budget(&self, pane: PaneId) -> Option<SyntaxBudget> {
        self.panes
            .iter()
            .find(|p| p.id == pane)
            .map(|pane| pane.syntax)
    }
    pub(super) fn bottom_hits(&self) -> Option<super::bottom_panel::BottomHits> {
        self.bottom
            .as_ref()
            .map(|frame| frame.hits(self.cell_height))
    }
    pub(super) fn contains_status(&self, needle: &str) -> bool {
        let end = self
            .status
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(self.status.len());
        String::from_utf16(&self.status[..end]).is_ok_and(|text| text.contains(needle))
    }
    pub(super) fn explorer_hits(&self) -> Option<super::explorer_panel::ExplorerHits> {
        self.explorer
            .as_ref()
            .map(|frame| frame.hits(self.cell_height))
    }
    pub(super) fn outline_hits(&self) -> Option<super::outline_panel::OutlineHits> {
        self.outline.as_ref().map(|frame| frame.hits())
    }
    pub(super) fn contains_source_row(&self, needle: &str) -> bool {
        self.panes.iter().flat_map(|p| &p.rows).any(|row| {
            let mut text = String::new();
            for cell in &row.cells {
                cell.glyph.with_str(|s| text.push_str(s));
            }
            text.contains(needle)
        })
    }
}

pub(super) fn areas(state: &State) -> Vec<(PaneId, Rect)> {
    let width =
        super::outline_panel::region(state).map_or(state.width.clamp(1, 8192), |r| r.left - 1);
    let height = state.height.clamp(1, 8192);
    let bottom = super::bottom_panel::region(state)
        .map_or((height - state.cell_height - 8).max(0), |r| r.top);
    let top = state.dpi.pixels(super::shell::TABS).min(bottom);
    let left = super::explorer_panel::region(state).map_or(0, |r| r.right + 1);
    let available = width - left;
    if state.workbench.panes().is_split() && available / state.cell_width.clamp(1, 8192) >= 90 {
        let middle = left + available / 2;
        vec![
            (
                PaneId::Primary,
                Rect {
                    left,
                    top,
                    right: middle - 1,
                    bottom,
                },
            ),
            (
                PaneId::Secondary,
                Rect {
                    left: middle + 1,
                    top,
                    right: width,
                    bottom,
                },
            ),
        ]
    } else {
        vec![(
            state.workbench.panes().active(),
            Rect {
                left,
                top,
                right: width,
                bottom,
            },
        )]
    }
}

/// The same bounded text-cell rectangle used by preparation. No source is
/// decoded here; the typed conversion below belongs to Workbench's layout.
pub(super) struct TextArea {
    pub pane: PaneId,
    pub tab: jc_app::TabId,
    pub first: usize,
    pub scroll: usize,
    pub x: i32,
    pub y: i32,
    pub columns: usize,
    pub rows: usize,
    pub capacity_rows: usize,
    pub count: usize,
    pub geometry: editor_scroll::Geometry,
}
pub(super) fn text_areas(state: &State) -> Vec<TextArea> {
    let cw = state.cell_width.clamp(1, 8192);
    let ch = state.cell_height.clamp(1, 8192);
    areas(state)
        .into_iter()
        .filter_map(|(pane, rect)| {
            let view = state.workbench.panes().view(pane)?;
            let tab = view.location()?.tab;
            let count = state.workbench.line_count_for_tab(tab)?;
            let geometry = editor_scroll::Geometry::new(rect, state.dpi);
            let rect = geometry.text;
            let digits = count.max(1).to_string().len().clamp(3, 7);
            let x = rect.left + (digits as i32 + 2) * cw + 8;
            let y = rect.top;
            let columns = ((rect.right - x - 8) / cw).clamp(0, 256) as usize;
            let rows = ((rect.bottom - y) / ch).clamp(0, 128) as usize;
            let first = view.scroll_line();
            Some(TextArea {
                pane,
                tab,
                first,
                scroll: state
                    .workbench
                    .rectangle_for_pane(pane)
                    .map_or(view.scroll_column(), |rect| rect.scroll_column),
                x,
                y,
                columns,
                rows: rows.min(count.saturating_sub(first)),
                capacity_rows: rows,
                count,
                geometry,
            })
        })
        .collect()
}

pub(super) fn source_pane_at(state: &State, x: i32, y: i32) -> Option<(PaneId, usize)> {
    for (pane, rect) in areas(state) {
        let text = editor_scroll::Geometry::new(rect, state.dpi).text;
        let top = text.top;
        let rows = ((text.bottom - top) / state.cell_height.clamp(1, 8192)).clamp(0, 128) as usize;
        if rows > 0
            && x >= rect.left
            && x < rect.right
            && y >= top
            && y < top + rows as i32 * state.cell_height
        {
            return Some((pane, rows));
        }
    }
    None
}
pub(super) fn focus_pane_at(state: &State, x: i32, y: i32) -> Option<PaneId> {
    areas(state)
        .into_iter()
        .find(|(_, r)| x >= r.left && x < r.right && y >= r.top && y < r.bottom)
        .map(|(pane, _)| pane)
}
/// Current scalar/diagnostic/TAB4 projection -> legal source byte boundary.
/// Captured drags clamp to their original visible text area, without autoscroll.
pub(super) fn linear_hit(
    state: &State,
    x: i32,
    y: i32,
    locked: Option<PaneId>,
) -> Result<Option<(PaneId, usize)>, String> {
    let cw = state.cell_width.clamp(1, 8192);
    let ch = state.cell_height.clamp(1, 8192);
    for area in text_areas(state) {
        if area.columns == 0
            || area.capacity_rows == 0
            || locked.is_some_and(|pane| pane != area.pane)
        {
            continue;
        }
        let right = area.x + area.columns as i32 * cw;
        let bottom = area.y + area.capacity_rows as i32 * ch;
        if locked.is_none() && (x < area.x || x >= right || y < area.y || y >= bottom) {
            continue;
        }
        let count = state
            .workbench
            .line_count_for_tab(area.tab)
            .ok_or("Source line catalog unavailable")?;
        let line = (area.first + ((y.clamp(area.y, bottom - 1) - area.y) / ch) as usize)
            .min(count.saturating_sub(1));
        let column = area.scroll + ((x.clamp(area.x, right - 1) - area.x) / cw) as usize;
        let (content, _) = state
            .workbench
            .line_range_for_tab(area.tab, line)
            .ok_or("Source hit line unavailable")?;
        let byte = if state.workbench.rectangle_for_pane(area.pane).is_some() {
            state
                .workbench
                .rectangle_byte_at(
                    area.tab,
                    jc_app::rectangle::VisualPosition {
                        line,
                        column: jc_app::rectangle::VisualColumn(column),
                    },
                )
                .map_err(|e| e.to_string())?
        } else if state.workbench.invisibles_visible() {
            state
                .workbench
                .display_byte_at(area.tab, line, column)
                .ok_or("Diagnostic source hit unavailable")?
                .min(content.end)
                .max(content.start)
        } else {
            state
                .workbench
                .visual_byte_at(area.tab, line, column)
                .ok_or("Source visual boundary unavailable")?
        };
        return Ok(Some((area.pane, byte)));
    }
    Ok(None)
}

/// A first hit converts the currently painted scalar/diagnostic coordinates;
/// a captured drag uses TAB4 coordinates directly and stays in its original
/// pane. Neither path changes focus, scroll, selection or worker delivery.
pub(super) fn rectangle_hit(
    state: &State,
    x: i32,
    y: i32,
    locked: Option<PaneId>,
) -> Result<Option<(PaneId, jc_app::rectangle::VisualPosition)>, String> {
    use jc_app::rectangle::{VisualColumn, VisualPosition};
    let cw = state.cell_width.clamp(1, 8192);
    let ch = state.cell_height.clamp(1, 8192);
    for area in text_areas(state) {
        if area.columns == 0 || area.rows == 0 || locked.is_some_and(|pane| pane != area.pane) {
            continue;
        }
        let right = area.x + area.columns as i32 * cw;
        let bottom = area.y + area.rows as i32 * ch;
        if locked.is_none() && (x < area.x || x >= right || y < area.y || y >= bottom) {
            continue;
        }
        let x = x.clamp(area.x, right - 1);
        let y = y.clamp(area.y, bottom - 1);
        let line = area.first + ((y - area.y) / ch) as usize;
        let column = area.scroll.saturating_add(((x - area.x) / cw) as usize);
        let point = if state.workbench.rectangle_for_pane(area.pane).is_some() {
            VisualPosition {
                line,
                column: VisualColumn(column),
            }
        } else if state.workbench.invisibles_visible() {
            let (content, eol) = state
                .workbench
                .line_range_for_tab(area.tab, line)
                .ok_or("Rectangle hit line disappeared")?;
            let end_column = state
                .workbench
                .display_column_at_byte(area.tab, line, content.end)
                .ok_or("Rectangle display boundary unavailable")?;
            let after_indicators = end_column.saturating_add(eol.len().saturating_mul(2));
            if column >= after_indicators {
                let mut end = state
                    .workbench
                    .rectangle_position_from_byte(area.tab, content.end)
                    .map_err(|e| e.to_string())?;
                end.column.0 = end.column.0.saturating_add(column - after_indicators);
                end
            } else {
                let byte = state
                    .workbench
                    .display_byte_at(area.tab, line, column)
                    .ok_or("Rectangle display hit unavailable")?;
                // CRLF and BOM indicators map atomically to legal content
                // boundaries; never attempt to navigate inside CRLF.
                state
                    .workbench
                    .rectangle_position_from_byte(
                        area.tab,
                        byte.min(content.end).max(content.start),
                    )
                    .map_err(|e| e.to_string())?
            }
        } else {
            VisualPosition {
                line,
                column: VisualColumn(column),
            }
        };
        return Ok(Some((area.pane, point)));
    }
    Ok(None)
}

/// Grid pixel for owned smoke input. Before activation callers use an ASCII
/// prefix (before TAB); after activation columns have the shared TAB4 meaning.
pub(super) fn rectangle_pixel(
    state: &State,
    pane: PaneId,
    line: usize,
    column: usize,
) -> Option<(i32, i32)> {
    let cw = state.cell_width.clamp(1, 8192);
    let ch = state.cell_height.clamp(1, 8192);
    let area = text_areas(state)
        .into_iter()
        .find(|area| area.pane == pane)?;
    let row = line.checked_sub(area.first)?;
    let col = column.checked_sub(area.scroll)?;
    if row >= area.rows || col >= area.columns {
        return None;
    }
    Some((
        area.x + col as i32 * cw + cw / 2,
        area.y + row as i32 * ch + ch / 2,
    ))
}

pub(super) fn prepare(state: &mut State) -> Frame {
    state.syntax_pending = false;
    state.syntax_targets = [None, None];
    state.validate_project_search();
    let covered = state.open_diff_visible || state.markdown_gui.is_some();
    if !covered {
        if let Some(area) = text_areas(state)
            .into_iter()
            .find(|area| area.pane == state.workbench.panes().active())
        {
            state
                .workbench
                .reveal_active_pane(area.capacity_rows.max(1), area.columns.max(1));
        }
    }
    let areas = if covered {
        Vec::new()
    } else {
        text_areas(state)
    };
    let mut panes = Vec::with_capacity(areas.len());
    let mut glyph_budget = jc_app::MAX_PRESENTATION_TEXT_BYTES;
    let mut syntax_limited = false;
    let mut syntax_plain = false;
    for area in areas {
        let id = area.pane;
        let rect = area.geometry.text;
        let tab = area.tab;
        let scroll_frame = editor_scroll::Frame::prepare(state, &area);
        let rectangle = state.workbench.rectangle_for_pane(id);
        let first = area.first;
        let invisible = state.workbench.invisibles_visible();
        let scroll = area.scroll;
        let position = state.workbench.position_for_pane(id);
        let count = area.count;
        let digits = count.max(1).to_string().len().clamp(3, 7);
        let text_x = area.x;
        let text_y = area.y;
        let columns = area.columns;
        let rows = area.capacity_rows;
        let selection = state.workbench.selection_for_pane(id);
        let syntax = state.workbench.syntax_rows(tab, first, rows);
        state.syntax_targets[usize::from(id == PaneId::Secondary)] = Some((id, tab));
        state.syntax_pending |= syntax.pending;
        syntax_limited |= syntax.limited;
        if id == state.workbench.panes().active() {
            syntax_plain = syntax.profile == SyntaxProfile::Plain && syntax.encoding.is_some();
        }
        let matches = state.workbench.search_matches_for_tab(tab);
        let mut prepared = Vec::with_capacity(rows);
        for line in first..count.min(first.saturating_add(rows)) {
            if invisible && rectangle.is_none() {
                let cells = state
                    .workbench
                    .display_row(tab, line, scroll, columns)
                    .unwrap_or_default()
                    .into_iter()
                    .enumerate()
                    .map(|(column, cell)| Cell {
                        glyph: jc_app::Glyph::Scalar(cell.ch),
                        column,
                        width: 1,
                        marker: cell.marker,
                        kind: if cell.source.is_empty() {
                            SyntaxKind::Plain
                        } else {
                            syntax.kind_at(cell.source.start)
                        },
                        search: !cell.source.is_empty() && search_at(matches, cell.source.clone()),
                        selected: !cell.source.is_empty()
                            && selection.as_ref().is_some_and(|range| {
                                range.start < cell.source.end && cell.source.start < range.end
                            }),
                    })
                    .collect();
                prepared.push(Row {
                    number: wide(&format!("{:>digits$}", line + 1)),
                    cells,
                });
                continue;
            }
            let visible = if glyph_budget == 0 {
                None
            } else if rectangle.is_some() {
                state
                    .workbench
                    .rectangle_glyph_line(tab, line, scroll, columns, invisible)
                    .ok()
            } else {
                state.workbench.visual_row(tab, line, scroll, columns)
            }
            .filter(|row| {
                let bytes = row.iter().map(|a| a.glyph.byte_len()).sum::<usize>();
                if bytes > glyph_budget {
                    glyph_budget = 0;
                    false
                } else {
                    glyph_budget -= bytes;
                    true
                }
            })
            .unwrap_or_else(|| {
                "[Layout Unicode no disponible: límite]"
                    .chars()
                    .take(columns)
                    .enumerate()
                    .map(|(column, ch)| jc_app::VisualAtom {
                        source: 0..0,
                        column,
                        width: 1,
                        glyph: jc_app::Glyph::Scalar(ch),
                        marker: true,
                    })
                    .collect()
            });
            let cells = visible
                .into_iter()
                .map(|atom| Cell {
                    kind: if atom.source.is_empty() {
                        SyntaxKind::Plain
                    } else {
                        syntax.kind_at(atom.source.start)
                    },
                    search: !atom.source.is_empty() && search_at(matches, atom.source.clone()),
                    selected: !atom.source.is_empty()
                        && selection.as_ref().is_some_and(|r| {
                            r.start < atom.source.end && atom.source.start < r.end
                        })
                        || rectangle.is_some_and(|rect| {
                            line >= rect.first_line
                                && line <= rect.last_line
                                && rect.left < scroll + atom.column + atom.width
                                && rect.right > scroll + atom.column
                        }),
                    glyph: atom.glyph,
                    column: atom.column,
                    width: atom.width,
                    marker: atom.marker,
                })
                .collect();
            prepared.push(Row {
                number: wide(&format!("{:>digits$}", line + 1)),
                cells,
            });
        }
        let active = id == state.workbench.panes().active();
        let caret = position.and_then(|position| {
            let column = if let Some(rectangle) = rectangle {
                rectangle.head_column
            } else if invisible {
                state.workbench.display_column_for_pane(id)?
            } else {
                state.workbench.visual_column_for_pane(id)?
            };
            (active
                && state.focused
                && state.selector.is_none()
                && state.context().editor_input
                && position.line >= first
                && position.line < first.saturating_add(rows)
                && column >= scroll
                && column - scroll < columns)
                .then(|| {
                    (
                        text_x + (column - scroll) as i32 * state.cell_width,
                        text_y + (position.line - first) as i32 * state.cell_height,
                    )
                })
        });
        panes.push(Pane {
            id,
            first_line: first,
            syntax: SyntaxBudget {
                processed_bytes: syntax.stats.processed_bytes,
                cache_bytes: syntax.stats.cache_bytes,
                scanned_to: syntax.stats.scanned_to,
                pending: syntax.pending,
                limited: syntax.limited,
            },
            rect,
            rows: prepared,
            text_x,
            text_y,
            caret,
            scroll: scroll_frame,
        });
    }
    let tabs = super::tab_bar::prepare(state);
    let position = state
        .workbench
        .active_position()
        .unwrap_or(TextPosition { line: 0, column: 0 });
    let encoding = state
        .workbench
        .active_encoding()
        .map_or("elige codificación", |encoding| encoding.name());
    let status = wide(&format!(
        "Lín {} Col {} (escalar) · {encoding}{}{}{} · {}",
        position.line + 1,
        position.column + 1,
        if state.workbench.panes().scroll_linked() {
            " · Desplazamiento vinculado"
        } else {
            ""
        },
        if state.syntax_pending {
            " · Preparando resaltado"
        } else if syntax_limited {
            " · Texto plano: límite"
        } else if syntax_plain {
            " · Resaltado: texto plano"
        } else {
            ""
        },
        if state
            .workbench
            .panes()
            .view(state.workbench.panes().active())
            .is_some_and(|v| v.is_pinned())
        {
            " · Referencia anclada"
        } else {
            ""
        },
        state.status
    ));
    if state.syntax_pending {
        state.wakeup.signal();
    }
    Frame {
        width: state.width.clamp(1, 8192),
        height: state.height.clamp(1, 8192),
        font: state.font,
        ui_font: state.ui_font,
        ascii_runs: state.ascii_runs,
        cell_width: state.cell_width,
        cell_height: state.cell_height,
        tabs,
        chrome: super::chrome::Frame::new(
            state.chrome_layout(),
            state.ui_font,
            state.icon,
            &state.workbench.active_document().map_or_else(
                || "CIDE".to_owned(),
                |doc| {
                    format!(
                        "CIDE · {}{}",
                        doc.session
                            .path()
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("<ruta no escalar>"),
                        if doc.session.document().is_modified() {
                            " ●"
                        } else {
                            ""
                        }
                    )
                },
            ),
            state.focused,
            state.chrome_hover,
        ),
        status,
        status_bar: super::status_bar::prepare(state),
        panes,
        search: state.search_presentation(),
        navigation: state.navigation_presentation(),
        project: state.project_presentation(),
        replacement: state.replace_presentation(),
        git: state.git_presentation(),
        comparison: state.open_diff_presentation(),
        source: state.source_presentation(),
        snippet: state.snippet_presentation(),
        markdown: state.markdown_presentation(),
        explorer: state.explorer_presentation(),
        outline: state.outline_presentation(),
        bottom: state.bottom_presentation(),
        selector: state.selector_presentation(super::selector_paint::list_capacity(
            state.width,
            (state.height, state.chrome_layout().top()),
            state.cell_width,
            state.cell_height,
        )),
    }
}

pub(super) unsafe fn fill(dc: Hdc, rect: Rect, color: u32) -> Result<(), String> {
    if rect.right <= rect.left || rect.bottom <= rect.top {
        return Ok(());
    }
    if RectVisible(dc, &rect) == 0 {
        return Ok(());
    }
    let brush = CreateSolidBrush(color);
    if brush.is_null() {
        return Err(super::os_error("CreateSolidBrush"));
    }
    let result = FillRect(dc, &rect, brush);
    DeleteObject(brush);
    if result == 0 {
        // FillRect doesn't define an extended GetLastError contract.
        Err("GDI FillRect returned zero".into())
    } else {
        Ok(())
    }
}

pub(super) unsafe fn fill_marks(
    dc: Hdc,
    rectangles: impl Iterator<Item = Rect>,
    color: u32,
) -> Result<(), String> {
    let brush = CreateSolidBrush(color);
    if brush.is_null() {
        return Err(super::os_error("CreateSolidBrush"));
    }
    let mut result = Ok(());
    for rect in rectangles {
        if RectVisible(dc, &rect) != 0 && FillRect(dc, &rect, brush) == 0 {
            result = Err(super::os_error("Dibujar marcas del minimapa"));
            break;
        }
    }
    DeleteObject(brush);
    result
}
pub(super) unsafe fn text(
    dc: Hdc,
    rect: Rect,
    value: &[u16],
    foreground: u32,
    background: u32,
) -> Result<(), String> {
    if rect.right <= rect.left || rect.bottom <= rect.top {
        return Ok(());
    }
    SetTextColor(dc, foreground);
    SetBkColor(dc, background);
    let length = value
        .len()
        .saturating_sub(usize::from(value.last() == Some(&0)));
    if ExtTextOutW(
        dc,
        rect.left,
        rect.top,
        6,
        &rect,
        value.as_ptr(),
        length as u32,
        null(),
    ) == 0
    {
        Err(super::os_error("ExtTextOutW"))
    } else {
        Ok(())
    }
}
struct FontSelection {
    dc: Hdc,
    old: Handle,
}
impl Drop for FontSelection {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
        }
    }
}
pub(super) unsafe fn draw(dc: Hdc, frame: &Frame) -> Result<(), String> {
    draw_internal(dc, frame, false)
}
/// Synthetic comparison for the owned-HWND smoke only; not an application path.
pub(super) unsafe fn draw_reference_cells(dc: Hdc, frame: &Frame) -> Result<(), String> {
    draw_internal(dc, frame, true)
}
unsafe fn draw_internal(dc: Hdc, frame: &Frame, reference: bool) -> Result<(), String> {
    // BeginPaint clips to the update/visible region. Hidden windows and empty
    // invalidations may legitimately have no drawable pixels (NULLREGION=1).
    let mut clip = Rect::default();
    match GetClipBox(dc, &mut clip) {
        0 => return Err("GDI clipping query failed".into()),
        1 => return Ok(()),
        _ => {}
    }
    let selected = FontSelection {
        dc,
        old: SelectObject(dc, frame.font),
    };
    SelectObject(dc, frame.ui_font);
    fill(
        dc,
        Rect {
            left: 0,
            top: 0,
            right: frame.width,
            bottom: frame.height,
        },
        BACKGROUND,
    )?;
    super::tab_bar::draw(dc, &frame.tabs)?;
    for pane in &frame.panes {
        SelectObject(dc, frame.font);
        for (row, line) in pane.rows.iter().enumerate() {
            let y = pane.text_y + row as i32 * frame.cell_height;
            text(
                dc,
                Rect {
                    left: pane.rect.left + 8,
                    top: y,
                    right: pane.text_x - frame.cell_width,
                    bottom: y + frame.cell_height,
                },
                &line.number,
                DIM,
                BACKGROUND,
            )?;
            let mut cell_index = 0;
            while cell_index < line.cells.len() {
                let cell = &line.cells[cell_index];
                let x = pane.text_x + cell.column as i32 * frame.cell_width;
                let count = if reference {
                    1
                } else {
                    run_length(&line.cells, cell_index, &frame.ascii_runs)
                };
                if count > 1 {
                    let mut units = [0u16; 256];
                    let advances = [frame.cell_width; 256];
                    for (n, item) in line.cells[cell_index..cell_index + count]
                        .iter()
                        .enumerate()
                    {
                        units[n] = item.glyph.first() as u16;
                    }
                    let rect = Rect {
                        left: x,
                        top: y,
                        right: (x + count as i32 * frame.cell_width).min(pane.rect.right),
                        bottom: (y + frame.cell_height).min(pane.rect.bottom),
                    };
                    SetTextColor(dc, cell.foreground());
                    SetBkColor(dc, cell.background());
                    if ExtTextOutW(
                        dc,
                        x,
                        y,
                        6,
                        &rect,
                        units.as_ptr(),
                        count as u32,
                        advances.as_ptr(),
                    ) == 0
                    {
                        return Err(super::os_error("Dibujar tramo de código"));
                    }
                    cell_index += count;
                    continue;
                }
                let mut units = [0u16; 2];
                let cluster;
                let encoded = match &cell.glyph {
                    jc_app::Glyph::Scalar(ch) => ch.encode_utf16(&mut units),
                    jc_app::Glyph::Cluster(text) => {
                        cluster = text.encode_utf16().collect::<Vec<_>>();
                        cluster.as_slice()
                    }
                };
                text(
                    dc,
                    Rect {
                        left: x,
                        top: y,
                        right: (x + cell.width as i32 * frame.cell_width).min(pane.rect.right),
                        bottom: (y + frame.cell_height).min(pane.rect.bottom),
                    },
                    encoded,
                    cell.foreground(),
                    cell.background(),
                )?;
                cell_index += 1;
            }
        }
        if let Some((x, y)) = pane.caret {
            fill(
                dc,
                Rect {
                    left: x,
                    top: y,
                    right: x + 2,
                    bottom: (y + frame.cell_height).min(pane.rect.bottom),
                },
                ACCENT,
            )?;
        }
        pane.scroll.draw(dc)?;
    }
    SelectObject(dc, frame.ui_font);
    if let Some(explorer) = &frame.explorer {
        super::explorer_paint::draw(dc, explorer, frame.cell_height)?;
    }
    if let Some(outline) = &frame.outline {
        super::outline_panel::draw(dc, outline)?;
    }
    if let Some(bottom) = &frame.bottom {
        super::bottom_paint::draw(dc, bottom, frame.cell_height)?;
    }
    super::status_bar::draw(dc, &frame.status_bar, &frame.status)?;
    if let Some(presentation) = &frame.selector {
        super::selector_paint::draw(
            dc,
            presentation,
            frame.width,
            (frame.height, frame.chrome.layout.top()),
            frame.cell_width,
            frame.cell_height,
        )?;
    }
    if let Some(presentation) = &frame.search {
        SelectObject(dc, frame.font);
        super::search_paint::draw(
            dc,
            presentation,
            frame.width,
            (frame.height, frame.chrome.layout.top()),
            frame.cell_width,
            frame.cell_height,
        )?;
    }
    if let Some(presentation) = &frame.navigation {
        super::navigation_paint::draw(
            dc,
            presentation,
            frame.width,
            (frame.height, frame.chrome.layout.top()),
            frame.cell_width,
            frame.cell_height,
        )?;
    }
    if let Some(project) = &frame.project {
        super::project_paint::draw(
            dc,
            project,
            frame.width,
            (frame.height, frame.chrome.layout.top()),
            frame.cell_width,
            frame.cell_height,
        )?;
    }
    if let Some(replacement) = &frame.replacement {
        super::project_paint::draw_labeled(
            dc,
            &replacement.panel,
            frame.width,
            (frame.height, frame.chrome.layout.top()),
            frame.cell_width,
            frame.cell_height,
            super::project_paint::PanelLabels {
                draft: replacement.labels.draft,
                policy: replacement.labels.policy,
                footer: replacement.labels.footer,
            },
        )?;
    }
    if let Some(git) = &frame.git {
        SelectObject(dc, frame.ui_font);
        super::git_workspace::draw(dc, git, frame.font, frame.ui_font)?;
    }
    if let Some(p) = &frame.comparison {
        SelectObject(dc, frame.font);
        super::open_diff_panel::draw(dc, p)?;
    }
    if let Some(p) = &frame.markdown {
        SelectObject(dc, frame.font);
        super::markdown_panel::draw(dc, p)?;
    }
    for p in [&frame.source, &frame.snippet].into_iter().flatten() {
        super::project_paint::draw_labeled(
            dc,
            &p.panel,
            frame.width,
            (frame.height, frame.chrome.layout.top()),
            frame.cell_width,
            frame.cell_height,
            super::project_paint::PanelLabels {
                draft: p.labels.draft,
                policy: p.labels.policy,
                footer: p.labels.footer,
            },
        )?;
    }
    if let Some(p) = &frame.selector {
        super::selector_paint::draw(
            dc,
            p,
            frame.width,
            (frame.height, frame.chrome.layout.top()),
            frame.cell_width,
            frame.cell_height,
        )?;
    }
    super::chrome::draw(dc, &frame.chrome)?;
    drop(selected);
    Ok(())
}

/// Actual GDI rasterization into a DIB, not a simulated screenshot. The BMP is
/// owned create_new output; it establishes raster composition, not physical DPI.
pub(super) unsafe fn bitmap(
    hwnd: Hwnd,
    frame: &Frame,
    path: &std::path::Path,
) -> Result<(), String> {
    use std::io::Write;
    if frame.width > 2048 || frame.height > 2048 {
        return Err("Smoke bitmap exceeds its pixel budget".into());
    }
    let reference = GetDC(hwnd);
    if reference.is_null() {
        return Err(super::os_error("GetDC bitmap"));
    }
    let dc = CreateCompatibleDC(reference);
    ReleaseDC(hwnd, reference);
    if dc.is_null() {
        return Err(super::os_error("CreateCompatibleDC"));
    }
    let header = BitmapHeader {
        size: 40,
        width: frame.width,
        height: -frame.height,
        planes: 1,
        bit_count: 32,
        compression: 0,
        image_size: 0,
        x_meter: 0,
        y_meter: 0,
        used: 0,
        important: 0,
    };
    let info = BitmapInfo { header, color: 0 };
    let mut bits = null_mut();
    let bitmap = CreateDIBSection(dc, &info, 0, &mut bits, null_mut(), 0);
    if bitmap.is_null() || bits.is_null() {
        if !bitmap.is_null() {
            DeleteObject(bitmap);
        }
        DeleteDC(dc);
        return Err(super::os_error("CreateDIBSection"));
    }
    let old = SelectObject(dc, bitmap);
    let result = (|| {
        // Exercise exposure/partial invalidation as well as the full raster.
        if IntersectClipRect(dc, 0, 0, 1, 1) == 0 {
            return Err("GDI partial clipping setup failed".into());
        }
        draw(dc, frame)?;
        if SelectClipRgn(dc, null_mut()) == 0 {
            return Err("GDI clipping restore failed".into());
        }
        draw(dc, frame)?;
        if GdiFlush() == 0 {
            return Err(super::os_error("GdiFlush"));
        }
        let size = frame.width as usize * frame.height as usize * 4;
        let pixels = std::slice::from_raw_parts(bits.cast::<u8>(), size);
        let first = &pixels[..4];
        if !pixels.chunks_exact(4).any(|pixel| pixel != first) {
            return Err("GDI bitmap has no contrasting painted cells".into());
        }
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| e.to_string())?;
        let mut file_header = [0u8; 54];
        file_header[..2].copy_from_slice(b"BM");
        file_header[2..6].copy_from_slice(&(54 + size as u32).to_le_bytes());
        file_header[10..14].copy_from_slice(&54u32.to_le_bytes());
        file_header[14..18].copy_from_slice(&40u32.to_le_bytes());
        file_header[18..22].copy_from_slice(&header.width.to_le_bytes());
        file_header[22..26].copy_from_slice(&header.height.to_le_bytes());
        file_header[26..28].copy_from_slice(&1u16.to_le_bytes());
        file_header[28..30].copy_from_slice(&32u16.to_le_bytes());
        output
            .write_all(&file_header)
            .and_then(|_| output.write_all(pixels))
            .map_err(|e| e.to_string())
    })();
    SelectObject(dc, old);
    DeleteObject(bitmap);
    DeleteDC(dc);
    result
}
