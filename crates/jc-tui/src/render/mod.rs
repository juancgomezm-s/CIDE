mod git_diff;
mod git_status;
mod overlays;

// Same visible-row bound as the shared escaped-display API. Preparation uses
// this when revealing a caret, so a huge terminal never requests an invalid row.
pub(crate) const MAX_DISPLAY_COLUMNS: usize = 4096;

use crate::app::{Presentation, TuiApp};
use crate::screen::{CellStyle, Screen};
use crate::state::UiOverlay;
use jc_app::{BottomPanel, PaneId, PrimaryFocus, SidebarView};
use jc_languages::detect;
use std::ops::Range;

pub(crate) fn render(frame: &Presentation<'_>) -> Screen {
    let app = frame.app;
    let width = frame.layout.width;
    let height = frame.layout.height;
    let mut screen = Screen::new(width, height);
    if width < 40 || height < 10 {
        screen.text(0, 0, "JC-IDE: terminal too small (minimum 40x10)");
        return screen;
    }

    draw_menu_bar(app, &mut screen);
    draw_tabs(app, &mut screen);

    let status_y = height - 1;
    let bottom_height = frame.layout.bottom_height;
    let content_top = 2;
    let content_bottom = status_y.saturating_sub(bottom_height);
    let content_height = frame.layout.content_height;
    let sidebar_width = frame.layout.sidebar_width;
    if sidebar_width > 0 {
        draw_sidebar(
            frame,
            &mut screen,
            0,
            content_top,
            sidebar_width,
            content_height,
        );
        screen.vline(sidebar_width, content_top, content_height, '│');
    }
    let editor_x = frame.layout.editor_x;
    let editor_width = frame.layout.editor_width;
    let two_columns = frame.layout.two_columns;
    let pane_width = frame.layout.pane_width;
    if two_columns {
        draw_editor(
            frame,
            &mut screen,
            PaneId::Primary,
            (editor_x, content_top, pane_width, content_height),
        );
        screen.vline(editor_x + pane_width, content_top, content_height, '│');
        draw_editor(
            frame,
            &mut screen,
            PaneId::Secondary,
            (
                editor_x + pane_width + 1,
                content_top,
                editor_width - pane_width - 1,
                content_height,
            ),
        );
    } else {
        draw_editor(
            frame,
            &mut screen,
            app.workbench.panes().active(),
            (editor_x, content_top, editor_width, content_height),
        );
    }

    if bottom_height > 0 {
        screen.hline(0, content_bottom, width, '─');
        draw_bottom(
            frame,
            &mut screen,
            0,
            content_bottom + 1,
            width,
            bottom_height.saturating_sub(1),
        );
    }
    draw_status(frame, &mut screen, status_y);
    overlays::draw(app, &mut screen);
    screen
}

fn draw_menu_bar(app: &TuiApp, screen: &mut Screen) {
    screen.fill_row_styled(0, ' ', CellStyle::Dim);
    let active_menu = match app.overlay {
        UiOverlay::Menu { menu, .. } => Some(menu),
        _ => None,
    };
    let mut x = 1u16;
    for menu in app.menus.menus() {
        let label = format!(" {} ", menu.title);
        let style = if active_menu == Some(menu.id) {
            CellStyle::Reverse
        } else {
            CellStyle::Dim
        };
        screen.text_styled(x, 0, &label, style);
        x = x.saturating_add(label.chars().count() as u16);
    }
}

fn draw_tabs(app: &TuiApp, screen: &mut Screen) {
    screen.fill_row(1, ' ');
    let active = app.workbench.tabs().active_id();
    let mut x = 0u16;
    for (index, tab) in app.workbench.tabs().tabs().iter().enumerate() {
        let dirty = if tab.dirty { "*" } else { "" };
        let label = format!(" {}:{}{} ", index + 1, tab.title, dirty);
        let style = if Some(tab.id) == active {
            CellStyle::Reverse
        } else {
            CellStyle::Normal
        };
        screen.text_styled(x, 1, &label, style);
        x = x.saturating_add(label.chars().count() as u16);
        if x >= screen.width() {
            break;
        }
    }
}

fn draw_sidebar(
    frame: &Presentation<'_>,
    screen: &mut Screen,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
) {
    let app = frame.app;
    if app.panels.sidebar() == SidebarView::GitStatus {
        git_status::draw_git_status(
            screen,
            &frame.git_status,
            x,
            y,
            width,
            height,
            app.focus.primary() == PrimaryFocus::Sidebar,
        );
        return;
    }
    if app.panels.sidebar() == SidebarView::Git {
        git_diff::sidebar(screen, &frame.git, x, y, width, height);
        return;
    }
    let focused = app.focus.primary() == PrimaryFocus::Sidebar;
    let title = match app.panels.sidebar() {
        SidebarView::Explorer => " Explorer ",
        SidebarView::Database => " Database ",
        SidebarView::Git => " Git ",
        SidebarView::GitStatus => " Git Status ",
        SidebarView::Symbols => " Symbols ",
    };
    screen.text_styled(
        x,
        y,
        title,
        if focused {
            CellStyle::Reverse
        } else {
            CellStyle::Dim
        },
    );
    if height <= 1 {
        return;
    }
    match app.panels.sidebar() {
        SidebarView::Explorer => {
            let mut row = y + 1;
            if let (Some(root), Some(directory)) = (app.explorer.root(), app.explorer.directory()) {
                let relative = directory.strip_prefix(root).unwrap_or(directory);
                let location = if relative.as_os_str().is_empty() {
                    ".".to_owned()
                } else {
                    relative.display().to_string()
                };
                screen.text_styled(x + 1, row, &clip(&location, width - 2), CellStyle::Dim);
                row += 1;
            } else {
                screen.text_styled(x + 1, row, "Open Folder to browse", CellStyle::Dim);
                return;
            }

            let available = usize::from((y + height).saturating_sub(row));
            let selected = app.explorer.selected();
            let start = selected.saturating_sub(available.saturating_sub(1));
            for (index, entry) in app
                .explorer
                .entries()
                .iter()
                .enumerate()
                .skip(start)
                .take(available)
            {
                let marker = if entry.is_dir { "▸ " } else { "  " };
                let text = format!("{marker}{}", entry.name);
                let style = if focused && index == selected {
                    CellStyle::Reverse
                } else {
                    CellStyle::Normal
                };
                screen.text_styled(x + 1, row, &clip(&text, width - 2), style);
                row += 1;
            }
        }
        SidebarView::Database => {
            screen.text(x + 1, y + 1, "Schema provider");
            screen.text_styled(x + 1, y + 2, "available in M3/M4", CellStyle::Dim);
        }
        SidebarView::Git | SidebarView::GitStatus => {
            unreachable!("Git presentation is rendered above");
        }
        SidebarView::Symbols => {
            screen.text(x + 1, y + 1, "Symbols follow language");
            screen.text_styled(x + 1, y + 2, "providers", CellStyle::Dim);
        }
    }
}

fn draw_editor(
    frame: &Presentation<'_>,
    screen: &mut Screen,
    pane: PaneId,
    area: (u16, u16, u16, u16),
) {
    let app = frame.app;
    let (x, y, width, height) = area;
    if width < 6 || height == 0 {
        return;
    }
    let Some(view) = app.workbench.panes().view(pane) else {
        return;
    };
    let active = pane == app.workbench.panes().active();
    let document = view
        .location()
        .and_then(|location| app.workbench.document_by_tab(location.tab));
    let title = document
        .and_then(|doc| doc.session.path().file_name())
        .map_or("Empty", |name| name.to_str().unwrap_or("<file>"));
    let header = format!(
        "{} Group {}: {}{}{}",
        if active { ">" } else { " " },
        if pane == PaneId::Primary { 1 } else { 2 },
        title,
        if view.is_pinned() { " [PIN]" } else { "" },
        if app.workbench.panes().scroll_linked() {
            " [LINK]"
        } else {
            ""
        }
    );
    screen.text_styled(
        x,
        y,
        &clip(&header, width),
        if active {
            CellStyle::Reverse
        } else {
            CellStyle::Dim
        },
    );
    let y = y + 1;
    let height = height.saturating_sub(1);
    let Some(_document) = document else {
        screen.text(x + 2, y + 2, "JC-IDE");
        screen.text_styled(
            x + 2,
            y + 4,
            &clip(
                "Ctrl+O Open   Ctrl+N New   Ctrl+Shift+P Commands",
                width.saturating_sub(2),
            ),
            CellStyle::Dim,
        );
        return;
    };
    let Some(line_count) = frame.editor(pane).line_count else {
        screen.text(
            x + 2,
            y + 2,
            &clip(
                "Select source encoding via command dialog",
                width.saturating_sub(2),
            ),
        );
        return;
    };
    let cursor = frame
        .editor(pane)
        .position
        .unwrap_or(jc_text::TextPosition { line: 0, column: 0 });
    let number_width = (line_count.max(1).to_string().len() as u16).clamp(3, 7);
    let text_x = x + number_width + 2;
    let text_width = width.saturating_sub(number_width + 2);
    if text_width == 0 {
        return;
    }
    let visible_height = usize::from(height);
    let first_line = view.scroll_line().min(line_count.saturating_sub(1));
    let prepared = frame.editor(pane);
    let rectangle = prepared.rectangle;
    let all = prepared.display_rows.is_some();
    let hscroll = if all {
        view.scroll_column()
    } else {
        rectangle.map_or_else(|| view.scroll_column(), |bounds| bounds.scroll_column)
    };
    let cursor_column = if all {
        prepared.cursor_column
    } else {
        Some(rectangle.map_or(cursor.column, |bounds| bounds.head_column))
    };
    let selection = app.workbench.selection_for_pane(pane);
    let highlights = app.workbench.search_matches_for_tab(_document.tab_id);

    for row in 0..visible_height {
        let line_index = first_line + row;
        if line_index >= line_count {
            break;
        }
        let row_y = y + row as u16;
        let number = format!("{:>width$} ", line_index + 1, width = number_width as usize);
        screen.text_styled(x, row_y, &number, CellStyle::Dim);
        if let Some(display) = &prepared.display_rows {
            let rectangle_source = prepared
                .rectangle_source_ranges
                .get(row)
                .and_then(Option::as_ref);
            for (target, cell) in display
                .get(row)
                .map(Vec::as_slice)
                .unwrap_or(&[])
                .iter()
                .take(usize::from(text_width))
                .enumerate()
            {
                // Display columns have a different scale than source/rectangle
                // columns. Select the complete atomic escape by byte overlap;
                // empty source ranges never turn padding into source text.
                let hit = highlights.partition_point(|hit| hit.range.end <= cell.source.start);
                let selected = !cell.source.is_empty()
                    && (highlights
                        .get(hit)
                        .is_some_and(|hit| hit.range.start < cell.source.end)
                        || selection
                            .as_ref()
                            .is_some_and(|range| intersects(range, &cell.source))
                        || rectangle_source.is_some_and(|range| intersects(range, &cell.source)));
                let style = if selected {
                    CellStyle::Reverse
                } else if cell.marker {
                    CellStyle::Dim
                } else {
                    CellStyle::Normal
                };
                screen.put_styled(text_x + target as u16, row_y, cell.ch, style);
            }
            continue;
        }
        let visible = frame
            .editor(pane)
            .rows
            .get(row)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        for (target, (ch, source_offset)) in visible
            .iter()
            .copied()
            .take(usize::from(text_width))
            .enumerate()
        {
            let hit = highlights.partition_point(|hit| hit.range.end <= source_offset);
            // Prepared rows already expand tabs at absolute visual stops and
            // include required virtual spaces. Rectangular highlighting uses
            // cells, never a byte test that would select an entire partial tab.
            let rectangular = rectangle.is_some_and(|bounds| {
                line_index >= bounds.first_line
                    && line_index <= bounds.last_line
                    && (bounds.left..bounds.right).contains(&target.saturating_add(hscroll))
            });
            let style = if rectangular
                || highlights
                    .get(hit)
                    .is_some_and(|m| m.range.start <= source_offset)
                || selection
                    .as_ref()
                    .is_some_and(|range| contains_byte(range, source_offset))
            {
                CellStyle::Reverse
            } else {
                CellStyle::Normal
            };
            screen.put_styled(text_x + target as u16, row_y, ch, style);
        }
    }

    if app.focus.primary() == PrimaryFocus::Editor
        && active
        && cursor.line >= first_line
        && cursor.line < first_line + visible_height
    {
        if let Some(column) = cursor_column.filter(|column| *column >= hscroll) {
            let cx = column - hscroll;
            let cursor_width = if all {
                usize::from(text_width).min(MAX_DISPLAY_COLUMNS)
            } else {
                usize::from(text_width)
            };
            if cx < cursor_width {
                screen.set_cursor(text_x + cx as u16, y + (cursor.line - first_line) as u16);
            }
        }
    }
}

fn draw_bottom(
    frame: &Presentation<'_>,
    screen: &mut Screen,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
) {
    let app = frame.app;
    if height == 0 {
        return;
    }
    if app.panels.bottom() == BottomPanel::Diff {
        git_diff::unified(screen, &frame.git, x, y, width, height);
        return;
    }
    let focused = app.focus.primary() == PrimaryFocus::BottomPanel;
    let title = match app.panels.bottom() {
        BottomPanel::Problems => " Problems ",
        BottomPanel::Output => " Output ",
        BottomPanel::SearchResults if app.project_results => {
            " Project Search: literal / case sensitive "
        }
        BottomPanel::SearchResults => " Search Results ",
        BottomPanel::Terminal => " Terminal ",
        BottomPanel::Diff => unreachable!(),
    };
    screen.text_styled(
        x,
        y,
        title,
        if focused {
            CellStyle::Reverse
        } else {
            CellStyle::Dim
        },
    );
    let count = match app.panels.bottom() {
        BottomPanel::Problems => app.workbench.problems().len(),
        BottomPanel::Output => app.workbench.output().len(),
        BottomPanel::SearchResults => app.search_result_count(),
        BottomPanel::Terminal => 1,
        BottomPanel::Diff => unreachable!(),
    };
    let available = usize::from(height.saturating_sub(1));
    let selected = app.bottom_selected.min(count.saturating_sub(1));
    let start = if focused {
        selected.saturating_sub(available.saturating_sub(1))
    } else {
        count.saturating_sub(available)
    };
    let rows: Vec<String> = match app.panels.bottom() {
        BottomPanel::Problems => app
            .workbench
            .problems()
            .iter()
            .skip(start)
            .take(available)
            .cloned()
            .collect(),
        BottomPanel::Output => app
            .workbench
            .output()
            .iter()
            .skip(start)
            .take(available)
            .cloned()
            .collect(),
        BottomPanel::SearchResults if app.project_results => app
            .project_search
            .rows()
            .iter()
            .skip(start)
            .take(available)
            .map(|row| {
                let root = app
                    .project_search
                    .root()
                    .unwrap_or(std::path::Path::new(""));
                match row {
                    jc_app::ProjectSearchRow::Match(m) => format!(
                        "{}:{}:{}  {}",
                        m.path.strip_prefix(root).unwrap_or(&m.path).display(),
                        m.position.line + 1,
                        m.position.column + 1,
                        m.snippet
                    ),
                    jc_app::ProjectSearchRow::Issue { path, message } => format!(
                        "{}: {}",
                        path.strip_prefix(root).unwrap_or(path).display(),
                        message
                    ),
                }
            })
            .collect(),
        BottomPanel::SearchResults => app
            .workbench
            .search()
            .matches
            .iter()
            .enumerate()
            .skip(start)
            .take(available)
            .map(|(i, range)| {
                frame.search_position(i).map_or_else(
                    || {
                        format!(
                            "{}: bytes {}..{}",
                            i + 1,
                            range.range.start,
                            range.range.end
                        )
                    },
                    |position| {
                        format!(
                            "{}: Ln {} Col {} (bytes {}..{})",
                            i + 1,
                            position.line + 1,
                            position.column + 1,
                            range.range.start,
                            range.range.end
                        )
                    },
                )
            })
            .collect(),
        BottomPanel::Terminal => {
            if available > 0 && start == 0 {
                vec!["Embedded shell is not active in this milestone.".to_owned()]
            } else {
                Vec::new()
            }
        }
        BottomPanel::Diff => unreachable!(),
    };
    for (offset, line) in rows.iter().enumerate() {
        let index = start + offset;
        let style = if focused && index == selected {
            CellStyle::Reverse
        } else {
            CellStyle::Normal
        };
        screen.text_styled(
            x + 1,
            y + 1 + offset as u16,
            &clip(line, width.saturating_sub(2)),
            style,
        );
    }
}

fn draw_status(frame: &Presentation<'_>, screen: &mut Screen, y: u16) {
    let app = frame.app;
    screen.fill_row_styled(y, ' ', CellStyle::Reverse);
    let left = if let Some(path) = app.workbench.active_path() {
        let language = detect(&path.to_string_lossy()).id;
        let encoding = app
            .workbench
            .active_encoding()
            .map_or("encoding?", |e| e.name());
        let position = frame
            .status_position
            .unwrap_or(jc_text::TextPosition { line: 0, column: 0 });
        format!(
            " {} | {} | {} | Ln {} Col {} ",
            path.display(),
            language,
            encoding,
            position.line + 1,
            position.column + 1
        )
    } else {
        " JC-IDE ".to_owned()
    };
    screen.text_styled(0, y, &clip(&left, screen.width()), CellStyle::Reverse);
    let status_width = app.status.chars().count() as u16 + 2;
    if status_width < screen.width() {
        screen.text_styled(
            screen.width() - status_width,
            y,
            &format!(" {} ", app.status),
            CellStyle::Reverse,
        );
    }
}

#[cfg(test)]
fn logical_lines(decoded: &jc_text::DecodedText) -> Vec<Vec<(char, usize)>> {
    let chars: Vec<char> = decoded.text.chars().collect();
    let mut lines = vec![Vec::new()];
    let mut index = 0usize;
    while index < chars.len() {
        let ch = chars[index];
        if ch == '\r' {
            if index + 1 < chars.len() && chars[index + 1] == '\n' {
                index += 1;
            }
            lines.push(Vec::new());
        } else if ch == '\n' {
            lines.push(Vec::new());
        } else {
            let offset = decoded
                .source_offsets
                .get(index)
                .copied()
                .unwrap_or(decoded.content_start);
            lines.last_mut().unwrap().push((ch, offset));
        }
        index += 1;
    }
    lines
}

fn contains_byte(range: &Range<usize>, offset: usize) -> bool {
    offset >= range.start && offset < range.end
}

fn intersects(left: &Range<usize>, right: &Range<usize>) -> bool {
    !left.is_empty() && !right.is_empty() && left.start < right.end && right.start < left.end
}

pub(crate) fn clip(text: &str, width: u16) -> String {
    text.chars().take(usize::from(width)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use jc_app::Workbench;
    use jc_text::{decode, TextEncoding};

    #[test]
    fn logical_lines_handle_all_supported_line_endings() {
        let decoded = decode(b"a\r\nb\nc\rd", TextEncoding::Utf8);
        let lines = logical_lines(&decoded);
        let text: Vec<String> = lines
            .iter()
            .map(|line| line.iter().map(|(ch, _)| *ch).collect())
            .collect();
        assert_eq!(text, vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn empty_app_renders_without_panicking() {
        let mut app = TuiApp::new(crate::state::LaunchOptions::default());
        let frame = app.prepare_frame(80, 24);
        let screen = render(&frame);
        assert!((0..screen.height()).any(|row| screen.row_text(row).contains("JC-IDE")));
        let _ = Workbench::default();
    }
}
