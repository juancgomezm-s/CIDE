mod overlays;

use crate::app::TuiApp;
use crate::screen::{CellStyle, Screen};
use crate::state::UiOverlay;
use jc_app::{BottomPanel, PrimaryFocus, SidebarView};
use jc_languages::detect;
use std::ops::Range;

pub fn render(app: &TuiApp, width: u16, height: u16) -> Screen {
    let mut screen = Screen::new(width, height);
    if width < 40 || height < 10 {
        screen.text(0, 0, "JC-IDE: terminal too small (minimum 40x10)");
        return screen;
    }

    draw_menu_bar(app, &mut screen);
    draw_tabs(app, &mut screen);

    let status_y = height - 1;
    let bottom_height = if app.panels.bottom_visible() {
        ((height - 4) / 3).clamp(4, 9)
    } else {
        0
    };
    let content_top = 2;
    let content_bottom = status_y.saturating_sub(bottom_height);
    let content_height = content_bottom.saturating_sub(content_top);
    let sidebar_width = if app.panels.sidebar_visible() {
        (width / 4).clamp(18, 28).min(width.saturating_sub(20))
    } else {
        0
    };
    if sidebar_width > 0 {
        draw_sidebar(app, &mut screen, 0, content_top, sidebar_width, content_height);
        screen.vline(sidebar_width, content_top, content_height, '│');
    }
    let editor_x = sidebar_width.saturating_add(u16::from(sidebar_width > 0));
    let editor_width = width.saturating_sub(editor_x);
    draw_editor(
        app,
        &mut screen,
        editor_x,
        content_top,
        editor_width,
        content_height,
    );

    if bottom_height > 0 {
        screen.hline(0, content_bottom, width, '─');
        draw_bottom(
            app,
            &mut screen,
            0,
            content_bottom + 1,
            width,
            bottom_height.saturating_sub(1),
        );
    }
    draw_status(app, &mut screen, status_y);
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
        if x >= screen.width() { break; }
    }
}

fn draw_sidebar(
    app: &TuiApp,
    screen: &mut Screen,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
) {
    let focused = app.focus.primary() == PrimaryFocus::Sidebar;
    let title = match app.panels.sidebar() {
        SidebarView::Explorer => " Explorer ",
        SidebarView::Database => " Database ",
        SidebarView::Git => " Git ",
        SidebarView::Symbols => " Symbols ",
    };
    screen.text_styled(
        x,
        y,
        title,
        if focused { CellStyle::Reverse } else { CellStyle::Dim },
    );
    if height <= 1 { return; }
    match app.panels.sidebar() {
        SidebarView::Explorer => {
            let mut row = y + 1;
            if let Some(root) = &app.project_root {
                screen.text(x + 1, row, &clip(&format!("{}", root.display()), width - 2));
                row += 1;
            }
            for tab in app.workbench.tabs().tabs() {
                if row >= y + height { break; }
                let marker = if app.workbench.tabs().active_id() == Some(tab.id) { '>' } else { ' ' };
                let text = format!("{marker} {}{}", tab.title, if tab.dirty { " *" } else { "" });
                screen.text(x + 1, row, &clip(&text, width - 2));
                row += 1;
            }
        }
        SidebarView::Database => {
            screen.text(x + 1, y + 1, "Schema provider");
            screen.text_styled(x + 1, y + 2, "available in M3/M4", CellStyle::Dim);
        }
        SidebarView::Git => {
            screen.text(x + 1, y + 1, "Git service is optional");
            screen.text_styled(x + 1, y + 2, "planned for M5", CellStyle::Dim);
        }
        SidebarView::Symbols => {
            screen.text(x + 1, y + 1, "Symbols follow language");
            screen.text_styled(x + 1, y + 2, "providers", CellStyle::Dim);
        }
    }
}

fn draw_editor(
    app: &TuiApp,
    screen: &mut Screen,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
) {
    if width < 6 || height == 0 { return; }
    let Some(document) = app.workbench.active_document() else {
        screen.text(x + 2, y + 2, "JC-IDE");
        screen.text_styled(x + 2, y + 4, "Ctrl+O Open   Ctrl+N New   Ctrl+Shift+P Commands", CellStyle::Dim);
        return;
    };
    let Some(decoded) = app.workbench.active_decoded() else {
        screen.text(x + 2, y + 2, "Select source encoding: 1 CP1252 / 2 ISO-8859-1 / 3 IBM850 / 4 UTF-8");
        return;
    };
    let cursor = app.workbench.active_position().unwrap_or(jc_text::TextPosition { line: 0, column: 0 });
    let lines = logical_lines(&decoded);
    let number_width = (lines.len().max(1).to_string().len() as u16).clamp(3, 7);
    let text_x = x + number_width + 2;
    let text_width = width.saturating_sub(number_width + 2);
    if text_width == 0 { return; }
    let visible_height = usize::from(height);
    let first_line = cursor.line.saturating_sub(visible_height.saturating_sub(1) / 2);
    let hscroll = cursor.column.saturating_sub(usize::from(text_width).saturating_sub(2));
    let selection = document.selection();

    for row in 0..visible_height {
        let line_index = first_line + row;
        if line_index >= lines.len() { break; }
        let row_y = y + row as u16;
        let number = format!("{:>width$} ", line_index + 1, width = number_width as usize);
        screen.text_styled(x, row_y, &number, CellStyle::Dim);
        let mut visual_col = 0usize;
        for &(ch, source_offset) in &lines[line_index] {
            if visual_col >= hscroll {
                let target = visual_col - hscroll;
                if target >= usize::from(text_width) { break; }
                let style = if selection.as_ref().is_some_and(|range| contains_byte(range, source_offset)) {
                    CellStyle::Reverse
                } else {
                    CellStyle::Normal
                };
                screen.put_styled(text_x + target as u16, row_y, ch, style);
            }
            visual_col += 1;
        }
    }

    if app.focus.primary() == PrimaryFocus::Editor
        && cursor.line >= first_line
        && cursor.line < first_line + visible_height
        && cursor.column >= hscroll
    {
        let cx = cursor.column - hscroll;
        if cx < usize::from(text_width) {
            screen.set_cursor(text_x + cx as u16, y + (cursor.line - first_line) as u16);
        }
    }
}

fn draw_bottom(app: &TuiApp, screen: &mut Screen, x: u16, y: u16, width: u16, height: u16) {
    if height == 0 { return; }
    let focused = app.focus.primary() == PrimaryFocus::BottomPanel;
    let title = match app.panels.bottom() {
        BottomPanel::Problems => " Problems ",
        BottomPanel::Output => " Output ",
        BottomPanel::SearchResults => " Search Results ",
        BottomPanel::Terminal => " Terminal ",
    };
    screen.text_styled(x, y, title, if focused { CellStyle::Reverse } else { CellStyle::Dim });
    let rows: Vec<String> = match app.panels.bottom() {
        BottomPanel::Problems => app.workbench.problems().to_vec(),
        BottomPanel::Output => app.workbench.output().to_vec(),
        BottomPanel::SearchResults => app
            .workbench
            .search()
            .matches
            .iter()
            .enumerate()
            .map(|(i, range)| format!("{}: bytes {}..{}", i + 1, range.start, range.end))
            .collect(),
        BottomPanel::Terminal => vec!["Embedded shell is not active in this milestone.".to_owned()],
    };
    let available = usize::from(height.saturating_sub(1));
    let start = rows.len().saturating_sub(available);
    for (offset, line) in rows[start..].iter().enumerate() {
        screen.text(x + 1, y + 1 + offset as u16, &clip(line, width.saturating_sub(2)));
    }
}

fn draw_status(app: &TuiApp, screen: &mut Screen, y: u16) {
    screen.fill_row_styled(y, ' ', CellStyle::Reverse);
    let left = if let Some(path) = app.workbench.active_path() {
        let language = detect(&path.to_string_lossy()).id;
        let encoding = app.workbench.active_encoding().map_or("encoding?", |e| e.name());
        let position = app.workbench.active_position().unwrap_or(jc_text::TextPosition { line: 0, column: 0 });
        format!(" {} | {} | {} | Ln {} Col {} ", path.display(), language, encoding, position.line + 1, position.column + 1)
    } else {
        " JC-IDE ".to_owned()
    };
    screen.text_styled(0, y, &clip(&left, screen.width()), CellStyle::Reverse);
    let status_width = app.status.chars().count() as u16 + 2;
    if status_width < screen.width() {
        screen.text_styled(screen.width() - status_width, y, &format!(" {} ", app.status), CellStyle::Reverse);
    }
}

fn logical_lines(decoded: &jc_text::DecodedText) -> Vec<Vec<(char, usize)>> {
    let chars: Vec<char> = decoded.text.chars().collect();
    let mut lines = vec![Vec::new()];
    let mut index = 0usize;
    while index < chars.len() {
        let ch = chars[index];
        if ch == '\r' {
            if index + 1 < chars.len() && chars[index + 1] == '\n' { index += 1; }
            lines.push(Vec::new());
        } else if ch == '\n' {
            lines.push(Vec::new());
        } else {
            let offset = decoded.source_offsets.get(index).copied().unwrap_or(decoded.content_start);
            lines.last_mut().unwrap().push((ch, offset));
        }
        index += 1;
    }
    lines
}

fn contains_byte(range: &Range<usize>, offset: usize) -> bool {
    offset >= range.start && offset < range.end
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
        let app = TuiApp::new(crate::state::LaunchOptions::default());
        let screen = render(&app, 80, 24);
        assert!((0..screen.height()).any(|row| screen.row_text(row).contains("JC-IDE")));
        let _ = Workbench::default();
    }
}
