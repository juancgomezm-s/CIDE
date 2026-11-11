mod file_browser;
mod git_code;
mod git_diff;
mod git_status;
mod markdown;
mod open_diff;
mod overlays;
mod snippets;
mod source_services;
pub(crate) use git_diff::git_sidebar_targets;
pub(crate) use overlays::menu_bounds;

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
        screen.text(0, 0, "CIDE: terminal pequeño (mínimo 40x10)");
        return screen;
    }
    if let Some(commit) = &frame.commit {
        for y in 0..height {
            screen.fill_row_styled(y, ' ', CellStyle::Panel);
        }
        screen.text_styled(2, 1, commit.title, CellStyle::PanelTitle);
        for (i, row) in commit.rows.iter().take(usize::from(height - 4)).enumerate() {
            screen.text_styled(
                2,
                2 + i as u16,
                &clip(row, width - 4),
                if i == 0 {
                    CellStyle::Reverse
                } else {
                    CellStyle::Panel
                },
            );
        }
        if let Some((column, row)) = commit.caret.filter(|(_, row)| *row < height - 4) {
            screen.set_cursor(2 + column.min(width - 4), 2 + row);
        }
        screen.text_styled(
            2,
            height - 1,
            &clip(&commit.hint, width - 4),
            CellStyle::Accent,
        );
        return screen;
    }
    if let Some(graph) = &frame.graph {
        git_status::draw_git_status(&mut screen, graph, 0, 0, width, height - 1, true);
        screen.text_styled(
            1,
            height - 1,
            &clip(if app.git_code.choose_commit {
                "↑↓ elegir revisión · Intro comparar en Código · R ramas/etiquetas · N más · Esc volver"
            } else if graph.title == "Ramas y etiquetas" {
                "O operaciones · ↑↓ seleccionar · Intro abrir · A abortar · Esc volver"
            } else {
                "C comparar en Código · ↑↓ · Intro archivos · O operaciones · P cherry-pick · V revertir · R refs · N más"
            }, width - 2),
            CellStyle::Accent,
        );
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
    if let Some(diff) = &frame.code_diff {
        git_code::draw(
            &mut screen,
            diff,
            editor_x,
            content_top,
            editor_width,
            content_height,
        );
    } else if two_columns {
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
    open_diff::draw(frame, &mut screen);
    draw_status(frame, &mut screen, status_y);
    source_services::popup(frame, &mut screen);
    snippets::draw(frame, &mut screen);
    markdown::draw(frame, &mut screen);
    overlays::draw(app, &mut screen);
    screen
}

fn draw_menu_bar(app: &TuiApp, screen: &mut Screen) {
    screen.fill_row_styled(0, ' ', CellStyle::Menu);
    let active_menu = match app.overlay {
        UiOverlay::Menu { menu, .. } => Some(menu),
        _ => None,
    };
    let mut x = 1u16;
    for menu in app.menus.menus() {
        let label = format!(" {} ", menu.title);
        let selected = active_menu == Some(menu.id);
        let style = if selected {
            CellStyle::MenuActive
        } else {
            CellStyle::Menu
        };
        screen.text_styled(x, 0, &label, style);
        if let Some((column, ch)) = label
            .chars()
            .enumerate()
            .find(|(_, ch)| ch.eq_ignore_ascii_case(&menu.mnemonic))
        {
            screen.put_styled(
                x.saturating_add(column as u16),
                0,
                ch,
                if selected {
                    CellStyle::MenuMnemonicActive
                } else {
                    CellStyle::MenuMnemonic
                },
            );
        }
        x = x.saturating_add(label.chars().count() as u16);
    }
}

fn draw_tabs(app: &TuiApp, screen: &mut Screen) {
    screen.fill_row_styled(1, ' ', CellStyle::Menu);
    let active = app.workbench.tabs().active_id();
    for label in crate::app::mouse::tab_labels(app, screen.width()) {
        let style = if Some(label.tab) == active {
            CellStyle::Reverse
        } else {
            CellStyle::Normal
        };
        screen.text_styled(label.x, 1, &label.text, style);
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
    for row in y..y.saturating_add(height) {
        screen.hline_styled(x, row, width, ' ', CellStyle::Panel);
    }
    let app = frame.app;
    if app.panels.sidebar() == SidebarView::Symbols {
        source_services::sidebar(frame, screen, x, y, width, height);
        return;
    }
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
        SidebarView::Explorer => " Explorador ",
        SidebarView::Database => " Base de datos ",
        SidebarView::Git => " Git ",
        SidebarView::GitStatus => " Estado Git ",
        SidebarView::Symbols => " Símbolos ",
    };
    screen.text_styled(x, y, title, CellStyle::PanelTitle);
    if height <= 1 {
        return;
    }
    match app.panels.sidebar() {
        SidebarView::Explorer => {
            screen.text_styled(
                x + 1,
                y + 1,
                &clip("F6: cambiar panel", width.saturating_sub(2)),
                CellStyle::Dim,
            );
            for (offset, entry) in frame
                .explorer
                .rows
                .iter()
                .take(usize::from(height.saturating_sub(4)))
                .enumerate()
            {
                let style = if focused && entry.selected {
                    CellStyle::Reverse
                } else {
                    CellStyle::Panel
                };
                screen.text_styled(
                    x + 1,
                    y + 2 + offset as u16,
                    &clip(&entry.label, width.saturating_sub(2)),
                    style,
                );
            }
            if height >= 5 {
                screen.text_styled(
                    x + 1,
                    y + height - 2,
                    &clip("↑↓ · → desplegar · rueda", width.saturating_sub(2)),
                    CellStyle::Accent,
                );
                screen.text_styled(
                    x + 1,
                    y + height - 1,
                    &clip("Intro abrir · Esc editor", width.saturating_sub(2)),
                    CellStyle::Accent,
                );
            }
            if let Some(bar) = crate::app::explorer_scrollbar(x, y, width, height, &frame.explorer)
            {
                for offset in 0..bar.length {
                    let thumb = bar.thumb_contains(offset);
                    screen.put_styled(
                        bar.x,
                        bar.y + offset,
                        if thumb { '┃' } else { '│' },
                        if thumb {
                            CellStyle::Accent
                        } else {
                            CellStyle::Dim
                        },
                    );
                }
            }
        }
        SidebarView::Database => {
            screen.text(x + 1, y + 1, "Proveedor de esquema");
            screen.text_styled(x + 1, y + 2, "disponible en M3/M4", CellStyle::Dim);
        }
        SidebarView::Git | SidebarView::GitStatus => {
            unreachable!("Git presentation is rendered above");
        }
        SidebarView::Symbols => {
            screen.text(x + 1, y + 1, "Símbolos del proveedor");
            screen.text_styled(x + 1, y + 2, "del lenguaje", CellStyle::Dim);
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
        .map_or("Vacío", |name| name.to_str().unwrap_or("<archivo>"));
    let header = format!(
        "{} Grupo {}: {}{}{}",
        if active { ">" } else { " " },
        if pane == PaneId::Primary { 1 } else { 2 },
        title,
        if view.is_pinned() { " [FIJO]" } else { "" },
        if app.workbench.panes().scroll_linked() {
            " [ENLACE]"
        } else {
            ""
        }
    );
    screen.text_styled(
        x,
        y,
        &clip(&header, width),
        if active {
            CellStyle::PanelTitle
        } else {
            CellStyle::Menu
        },
    );
    let y = y + 1;
    let Some(_document) = document else {
        screen.text(x + 2, y + 2, "CIDE");
        screen.text_styled(
            x + 2,
            y + 4,
            &clip(
                "Ctrl+O abrir · Ctrl+N nuevo · Ctrl+Shift+P comandos",
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
                "Elige la codificación del documento",
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
    let text_width = width.saturating_sub(number_width + 3);
    if text_width == 0 {
        return;
    }
    let visible_height = usize::from(frame.layout.editor_rows());
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
        rectangle
            .map(|bounds| bounds.head_column)
            .or(prepared.cursor_column)
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
                            .is_some_and(|range| intersects(range, &cell.source)));
                let style = if selected {
                    CellStyle::Reverse
                } else if cell.marker {
                    CellStyle::Dim
                } else {
                    syntax_style(prepared.syntax.kind_at(cell.source.start))
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
        for atom in visible {
            let hit = highlights.partition_point(|hit| hit.range.end <= atom.source.start);
            let rectangular = rectangle.is_some_and(|bounds| {
                line_index >= bounds.first_line
                    && line_index <= bounds.last_line
                    && bounds.left
                        < atom
                            .column
                            .saturating_add(hscroll)
                            .saturating_add(atom.width)
                    && bounds.right > atom.column.saturating_add(hscroll)
            });
            let selected = !atom.source.is_empty()
                && (highlights
                    .get(hit)
                    .is_some_and(|m| m.range.start < atom.source.end)
                    || selection
                        .as_ref()
                        .is_some_and(|range| intersects(range, &atom.source)));
            let style = if rectangular || selected {
                CellStyle::Reverse
            } else if atom.marker {
                CellStyle::Dim
            } else {
                syntax_style(prepared.syntax.kind_at(atom.source.start))
            };
            screen.put_glyph(
                text_x + atom.column as u16,
                row_y,
                &atom.glyph,
                atom.width,
                style,
            );
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
    if let Some(bars) = prepared.bars {
        for bar in bars {
            for offset in 0..bar.length {
                let thumb = bar.maximum > 0 && bar.thumb_contains(offset);
                let ch = match (bar.vertical, thumb) {
                    (true, true) => '┃',
                    (true, false) => '│',
                    (false, true) => '━',
                    (false, false) => '─',
                };
                screen.put_styled(
                    bar.x + if bar.vertical { 0 } else { offset },
                    bar.y + if bar.vertical { offset } else { 0 },
                    ch,
                    if thumb {
                        CellStyle::Accent
                    } else {
                        CellStyle::Dim
                    },
                );
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
        BottomPanel::Problems => " Problemas ",
        BottomPanel::Output => " Salida ",
        BottomPanel::SearchResults if app.project_results => " Búsqueda de proyecto ",
        BottomPanel::SearchResults => " Resultados de búsqueda ",
        BottomPanel::Terminal => " Terminal ",
        BottomPanel::Diff => unreachable!(),
    };
    screen.text_styled(x, y, title, CellStyle::PanelTitle);
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
            .map(|row| row.text.clone())
            .collect(),
        BottomPanel::Output => app
            .workbench
            .output()
            .iter()
            .skip(start)
            .take(available)
            .map(|row| row.text.clone())
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
                vec!["La consola integrada no está activa en este hito.".to_owned()]
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
    screen.fill_row_styled(y, ' ', CellStyle::Status);
    let left = if let Some(path) = app.workbench.active_path() {
        let language = detect(&path.to_string_lossy()).id;
        let encoding = app
            .workbench
            .active_encoding()
            .map_or("¿codificación?", |e| e.name());
        let position = frame
            .status_position
            .unwrap_or(jc_text::TextPosition { line: 0, column: 0 });
        format!(
            " {} | {} | {} | Lín {} Col {} ",
            path.display(),
            language,
            encoding,
            position.line + 1,
            position.column + 1
        )
    } else {
        " CIDE ".to_owned()
    };
    screen.text_styled(0, y, &clip(&left, screen.width()), CellStyle::Status);
    let status = if app.syntax_pending {
        format!("Preparando resaltado | {}", app.status)
    } else if frame.primary.syntax.limited || frame.secondary.syntax.limited {
        format!("Texto sin resaltado: límite | {}", app.status)
    } else if frame.editor(app.workbench.panes().active()).syntax.profile
        == jc_app::SyntaxProfile::Plain
        && frame
            .editor(app.workbench.panes().active())
            .syntax
            .encoding
            .is_some()
    {
        format!("Resaltado: texto plano | {}", app.status)
    } else {
        app.status.clone()
    };
    let status_width = status.chars().count() as u16 + 2;
    if status_width < screen.width() {
        screen.text_styled(
            screen.width() - status_width,
            y,
            &format!(" {status} "),
            CellStyle::Status,
        );
    }
}

fn syntax_style(kind: jc_app::SyntaxKind) -> CellStyle {
    if kind == jc_app::SyntaxKind::Plain {
        CellStyle::Normal
    } else {
        CellStyle::Syntax(kind)
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

fn intersects(left: &Range<usize>, right: &Range<usize>) -> bool {
    !left.is_empty() && !right.is_empty() && left.start < right.end && right.start < left.end
}

pub(crate) fn clip(text: &str, width: u16) -> String {
    jc_text::grapheme::clip(text, usize::from(width))
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
        assert!((0..screen.height()).any(|row| screen.row_text(row).contains("CIDE")));
        let _ = Workbench::default();
    }
}
