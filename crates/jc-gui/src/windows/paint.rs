//! Preparation owns only visible cells. Painting borrows that presentation and
//! never accesses Workbench, document caches or a mutable editor widget.
use super::{ffi::*, wide, State};
use jc_app::PaneId;
use jc_text::TextPosition;
use std::ptr::{null, null_mut};

const BACKGROUND: u32 = 0x00272828;
const HEADER: u32 = 0x00202122;
const FOREGROUND: u32 = 0x00d5d7d8;
const DIM: u32 = 0x008d8b87;
const SELECTED: u32 = 0x00534b45;
const ACCENT: u32 = 0x00a6d9e6;
struct Cell {
    ch: char,
    selected: bool,
    marker: bool,
}
struct Row {
    number: Vec<u16>,
    cells: Vec<Cell>,
}
struct Pane {
    rect: Rect,
    header: Vec<u16>,
    rows: Vec<Row>,
    text_x: i32,
    text_y: i32,
    caret: Option<(i32, i32)>,
}
pub(super) struct Frame {
    pub width: i32,
    pub height: i32,
    font: Handle,
    cell_width: i32,
    cell_height: i32,
    top: Vec<u16>,
    status: Vec<u16>,
    panes: Vec<Pane>,
    selector: Option<super::selector::SelectorPresentation>,
    search: Option<super::search_panel::SearchPresentation>,
    navigation: Option<super::navigation_panel::NavigationPresentation>,
    project: Option<super::project_panel::ProjectPresentation>,
    replacement: Option<super::replace_panel::ReplaceFrame>,
    git: Option<super::git_panel::GitFrame>,
    explorer: Option<super::explorer_panel::ExplorerFrame>,
}
impl Frame {
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
    pub(super) fn contains_source_row(&self, needle: &str) -> bool {
        self.panes.iter().flat_map(|p| &p.rows).any(|row| {
            row.cells
                .iter()
                .map(|c| c.ch)
                .collect::<String>()
                .contains(needle)
        })
    }
}

fn areas(state: &State) -> Vec<(PaneId, Rect)> {
    let width = state.width.clamp(1, 8192);
    let height = state.height.clamp(1, 8192);
    let bottom = (height - state.cell_height - 8).max(0);
    let top = (state.cell_height + 8).min(bottom);
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
struct TextArea {
    pane: PaneId,
    tab: jc_app::TabId,
    first: usize,
    scroll: usize,
    x: i32,
    y: i32,
    columns: usize,
    rows: usize,
}
fn text_areas(state: &State) -> Vec<TextArea> {
    let cw = state.cell_width.clamp(1, 8192);
    let ch = state.cell_height.clamp(1, 8192);
    areas(state)
        .into_iter()
        .filter_map(|(pane, rect)| {
            let view = state.workbench.panes().view(pane)?;
            let tab = view.location()?.tab;
            let count = state.workbench.line_count_for_tab(tab)?;
            let digits = count.max(1).to_string().len().clamp(3, 7);
            let x = rect.left + (digits as i32 + 2) * cw + 8;
            let y = (rect.top + ch + 6).min(rect.bottom);
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
            })
        })
        .collect()
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
            state
                .workbench
                .rectangle_position_from_scalar(area.tab, TextPosition { line, column })
                .map_err(|e| e.to_string())?
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
    state.validate_project_search();
    let areas = areas(state);
    let columns = areas.first().map_or(1, |(_, rect)| {
        ((rect.right - rect.left) / state.cell_width - 7).clamp(1, 256) as usize
    });
    let rows = areas.first().map_or(1, |(_, rect)| {
        ((rect.bottom - rect.top - state.cell_height - 6) / state.cell_height).clamp(1, 128)
            as usize
    });
    state.workbench.reveal_active_pane(rows, columns);
    let mut panes = Vec::with_capacity(areas.len());
    for (id, rect) in areas {
        let Some(view) = state.workbench.panes().view(id) else {
            continue;
        };
        let Some(location) = view.location() else {
            continue;
        };
        let Some(document) = state.workbench.document_by_tab(location.tab) else {
            continue;
        };
        let rectangle = state.workbench.rectangle_for_pane(id);
        let first = view.scroll_line();
        let invisible = state.workbench.invisibles_visible();
        let scroll = rectangle.map_or(view.scroll_column(), |rect| rect.scroll_column);
        let position = state.workbench.position_for_pane(id);
        let count = state
            .workbench
            .line_count_for_tab(location.tab)
            .unwrap_or(0);
        let digits = count.max(1).to_string().len().clamp(3, 7);
        let text_x = rect.left + (digits as i32 + 2) * state.cell_width + 8;
        let text_y = (rect.top + state.cell_height + 6).min(rect.bottom);
        let columns = ((rect.right - text_x - 8) / state.cell_width).clamp(0, 256) as usize;
        let rows = ((rect.bottom - text_y) / state.cell_height).clamp(0, 128) as usize;
        let selection = state.workbench.selection_for_pane(id);
        let mut prepared = Vec::with_capacity(rows);
        for line in first..count.min(first.saturating_add(rows)) {
            if invisible && rectangle.is_none() {
                let cells = state
                    .workbench
                    .display_row(location.tab, line, scroll, columns)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|cell| Cell {
                        ch: cell.ch,
                        marker: cell.marker,
                        selected: selection.as_ref().is_some_and(|range| {
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
            let visible = if rectangle.is_some() {
                state
                    .workbench
                    .rectangle_grid_line(location.tab, line, scroll, columns, invisible)
                    .unwrap_or_else(|_| {
                        "[unsupported rectangular layout]"
                            .chars()
                            .take(columns)
                            .map(|ch| (ch, usize::MAX))
                            .collect()
                    })
            } else {
                state
                    .workbench
                    .visible_line(location.tab, line, scroll, columns)
                    .unwrap_or_default()
            };
            let cells = visible
                .into_iter()
                .take(columns)
                .enumerate()
                .map(|(target, (ch, byte))| Cell {
                    marker: rectangle.is_some() && invisible && matches!(ch, '>' | '.'),
                    ch: if ch == '\t' {
                        '→'
                    } else if ch.is_control() {
                        '·'
                    } else {
                        ch
                    },
                    selected: selection
                        .as_ref()
                        .is_some_and(|range| range.contains(&byte))
                        || rectangle.is_some_and(|rect| {
                            line >= rect.first_line
                                && line <= rect.last_line
                                && (rect.left..rect.right).contains(&scroll.saturating_add(target))
                        }),
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
                position.column
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
        let name = document
            .session
            .path()
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("<non-scalar path>");
        let header = wide(&format!(
            "{} Group {} · {}{}{}",
            if active { ">" } else { " " },
            if id == PaneId::Primary { 1 } else { 2 },
            name.chars().take(80).collect::<String>(),
            if document.session.document().is_modified() {
                " *"
            } else {
                ""
            },
            if rectangle.is_some() {
                " · Rect TAB4"
            } else {
                ""
            }
        ));
        panes.push(Pane {
            rect,
            header,
            rows: prepared,
            text_x,
            text_y,
            caret,
        });
    }
    let mut top = String::from("CIDE  ");
    for tab in state.workbench.tabs().tabs().iter().take(12) {
        let title: String = tab.title.chars().take(24).collect();
        if Some(tab.id) == state.workbench.tabs().active_id() {
            top.push_str(&format!(" [{title}{}] ", if tab.dirty { "*" } else { "" }));
        } else {
            top.push_str(&format!(" {title} "));
        }
    }
    let position = state
        .workbench
        .active_position()
        .unwrap_or(TextPosition { line: 0, column: 0 });
    let encoding = state
        .workbench
        .active_encoding()
        .map_or("choose encoding", |encoding| encoding.name());
    let status = wide(&format!(
        "Ln {} Col {} (scalar) · {encoding}{} · {}",
        position.line + 1,
        position.column + 1,
        if state.workbench.panes().scroll_linked() {
            " · Scroll linked"
        } else {
            ""
        },
        state.status
    ));
    Frame {
        width: state.width.clamp(1, 8192),
        height: state.height.clamp(1, 8192),
        font: state.font,
        cell_width: state.cell_width,
        cell_height: state.cell_height,
        top: wide(&top),
        status,
        panes,
        search: state.search_presentation(),
        navigation: state.navigation_presentation(),
        project: state.project_presentation(),
        replacement: state.replace_presentation(),
        git: state.git_presentation(),
        explorer: state.explorer_presentation(),
        selector: state.selector_presentation(super::selector_paint::list_capacity(
            state.width,
            state.height,
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
    text(
        dc,
        Rect {
            left: 8,
            top: 4,
            right: frame.width,
            bottom: (frame.cell_height + 8).min(frame.height),
        },
        &frame.top,
        FOREGROUND,
        HEADER,
    )?;
    for pane in &frame.panes {
        text(
            dc,
            Rect {
                left: pane.rect.left + 8,
                top: pane.rect.top + 2,
                right: pane.rect.right,
                bottom: pane.text_y,
            },
            &pane.header,
            FOREGROUND,
            HEADER,
        )?;
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
            for (column, cell) in line.cells.iter().enumerate() {
                let x = pane.text_x + column as i32 * frame.cell_width;
                let mut units = [0u16; 2];
                let encoded = cell.ch.encode_utf16(&mut units);
                text(
                    dc,
                    Rect {
                        left: x,
                        top: y,
                        right: (x + frame.cell_width).min(pane.rect.right),
                        bottom: (y + frame.cell_height).min(pane.rect.bottom),
                    },
                    encoded,
                    if cell.marker { DIM } else { FOREGROUND },
                    if cell.selected { SELECTED } else { BACKGROUND },
                )?;
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
    }
    if let Some(explorer) = &frame.explorer {
        super::explorer_paint::draw(dc, explorer, frame.cell_height)?;
    }
    let top = (frame.height - frame.cell_height - 6).max(0);
    text(
        dc,
        Rect {
            left: 8,
            top,
            right: frame.width,
            bottom: frame.height,
        },
        &frame.status,
        DIM,
        HEADER,
    )?;
    if let Some(presentation) = &frame.selector {
        super::selector_paint::draw(
            dc,
            presentation,
            frame.width,
            frame.height,
            frame.cell_width,
            frame.cell_height,
        )?;
    }
    if let Some(presentation) = &frame.search {
        super::search_paint::draw(
            dc,
            presentation,
            frame.width,
            frame.height,
            frame.cell_width,
            frame.cell_height,
        )?;
    }
    if let Some(presentation) = &frame.navigation {
        super::navigation_paint::draw(
            dc,
            presentation,
            frame.width,
            frame.height,
            frame.cell_width,
            frame.cell_height,
        )?;
    }
    if let Some(project) = &frame.project {
        super::project_paint::draw(
            dc,
            project,
            frame.width,
            frame.height,
            frame.cell_width,
            frame.cell_height,
        )?;
    }
    if let Some(replacement) = &frame.replacement {
        super::project_paint::draw_labeled(
            dc,
            &replacement.panel,
            frame.width,
            frame.height,
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
        super::project_paint::draw_labeled(
            dc,
            &git.panel,
            frame.width,
            frame.height,
            frame.cell_width,
            frame.cell_height,
            super::project_paint::PanelLabels {
                draft: git.labels.draft,
                policy: git.labels.policy,
                footer: git.labels.footer,
            },
        )?;
    }
    drop(selected);
    Ok(())
}

pub(super) fn click(state: &mut State, x: i32, y: i32) {
    let Some((pane, rect)) = areas(state)
        .into_iter()
        .find(|(_, rect)| x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom)
    else {
        return;
    };
    state.workbench.focus_editor_pane(pane);
    let Some(view) = state.workbench.panes().view(pane) else {
        return;
    };
    let Some(location) = view.location() else {
        return;
    };
    let count = state
        .workbench
        .line_count_for_tab(location.tab)
        .unwrap_or(0);
    if count == 0 {
        return;
    }
    let digits = count.to_string().len().clamp(3, 7) as i32;
    let text_x = rect.left + (digits + 2) * state.cell_width + 8;
    let text_y = rect.top + state.cell_height + 6;
    if y < text_y || x < text_x {
        return;
    }
    let line =
        (view.scroll_line() + (y - text_y) as usize / state.cell_height as usize).min(count - 1);
    if let Some(rectangle) = state.workbench.rectangle_for_pane(pane) {
        let column = rectangle
            .scroll_column
            .saturating_add((x - text_x) as usize / state.cell_width as usize);
        let target = state
            .workbench
            .rectangle_byte_at(
                location.tab,
                jc_app::rectangle::VisualPosition {
                    line,
                    column: jc_app::rectangle::VisualColumn(column),
                },
            )
            .map_err(|error| error.to_string())
            .and_then(|byte| {
                state
                    .workbench
                    .position_for_tab(location.tab, byte)
                    .ok_or_else(|| "Rectangle click boundary unavailable".to_string())
            });
        match target {
            Ok(position) => {
                let select = state.modifiers().contains(jc_app::Modifiers::SHIFT);
                if let Err(error) = state.workbench.set_caret_position(position, select) {
                    state.error(error);
                }
            }
            Err(error) => state.error(error),
        }
        return;
    }
    let column = view.scroll_column() + (x - text_x) as usize / state.cell_width as usize;
    if state.workbench.invisibles_visible() {
        let target = state
            .workbench
            .display_byte_at(location.tab, line, column)
            .and_then(|byte| state.workbench.position_for_tab(location.tab, byte));
        if let Some(position) = target {
            let select = state.modifiers().contains(jc_app::Modifiers::SHIFT);
            if let Err(error) = state.workbench.set_caret_position(position, select) {
                state.error(error);
            }
        }
        return;
    }
    let end = state
        .workbench
        .line_range_for_tab(location.tab, line)
        .and_then(|(content, _)| state.workbench.position_for_tab(location.tab, content.end))
        .map_or(0, |position| position.column);
    let select = state.modifiers().contains(jc_app::Modifiers::SHIFT);
    if let Err(error) = state.workbench.set_caret_position(
        TextPosition {
            line,
            column: column.min(end),
        },
        select,
    ) {
        state.error(error);
    }
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
