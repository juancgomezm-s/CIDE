//! Read-only native Markdown surface over the same bounded range presenter.
use super::{ffi::*, paint, wide, Action, State};
use jc_app::{
    markdown_preview::{
        MarkdownBlock, MarkdownInlineStyle, MarkdownPresentation, MarkdownRow, MarkdownView,
    },
    CommandId, Modifiers, TaskStamp,
};
use std::{ptr::null_mut, rc::Rc, sync::Arc};
#[path = "markdown_interaction.rs"]
mod interaction;
use super::markdown_images::{DecodedImage, ImageContext, ImageSource};
pub(super) use interaction::MarkdownHits;
use jc_app::markdown_preview::{MarkdownAnchorScan, MarkdownTarget};
use std::path::{Path, PathBuf};
#[link(name = "user32")]
extern "system" {
    fn DrawTextW(dc: Hdc, text: *const u16, count: i32, rect: *mut Rect, flags: u32) -> i32;
}
#[link(name = "gdi32")]
extern "system" {
    fn SetBkMode(dc: Hdc, mode: i32) -> i32;
    fn GetTextExtentPoint32W(dc: Hdc, text: *const u16, count: i32, size: *mut TextSize) -> i32;
}
#[repr(C)]
#[derive(Default)]
struct TextSize {
    width: i32,
    height: i32,
}
pub(super) struct MarkdownGui {
    stamp: TaskStamp,
    view: MarkdownView,
    fonts: Option<Rc<Fonts>>,
    page_rows: usize,
    wheel: i32,
    token: Rc<()>,
    hits: Option<MarkdownHits>,
    selected: usize,
    keyboard_focus: bool,
    context: ImageContext,
    document_path: PathBuf,
    wanted: Vec<ImageSource>,
    sent: Vec<ImageSource>,
    remote_allowed: Vec<String>,
    anchor: Option<MarkdownAnchorScan>,
    project: Option<std::path::PathBuf>,
    extents: Vec<(ImageSource, u32, u32)>,
}
pub(super) struct MarkdownFrame {
    pub presentation: MarkdownPresentation,
    width: i32,
    viewport_left: i32,
    height: i32,
    top: i32,
    ch: i32,
    dpi: super::dpi::Dpi,
    fonts: Rc<Fonts>,
    pub(super) hits: MarkdownHits,
    images: Vec<interaction::RowImage>,
    selected: Option<Arc<MarkdownTarget>>,
}
impl State {
    pub(super) fn toggle_markdown(&mut self) -> Action {
        if self.markdown_gui.take().is_some() {
            self.markdown_images.cancel();
            self.status = "Vista de fuente; documento sin cambios".into();
            return Action::None;
        }
        if !self
            .registry
            .is_enabled(CommandId::ViewMarkdownPreview, &self.context())
        {
            return Action::None;
        }
        self.close_selector();
        self.close_source_ui();
        self.snippet_gui = None;
        self.open_diff_visible = false;
        let Some(doc) = self.workbench.active_document() else {
            return Action::None;
        };
        let Some(encoding) = doc.session.encoding() else {
            self.error("Elige la codificación del documento");
            return Action::None;
        };
        let stamp = TaskStamp {
            document: doc.id,
            revision: doc.session.document().revision(),
            encoding,
            view: self
                .workbench
                .panes()
                .view(self.workbench.panes().active())
                .and_then(|v| v.view_id()),
        };
        let mut document_path = doc.session.path().to_owned();
        // Match the canonical project root before resolving links or images.
        // Keep the filename as opened and do no filesystem work during paint.
        if document_path.is_absolute() || !doc.session.is_new() {
            if let Some((parent, name)) = document_path.parent().zip(document_path.file_name()) {
                let parent = if parent.as_os_str().is_empty() {
                    Path::new(".")
                } else {
                    parent
                };
                if let Ok(parent) = parent.canonicalize() {
                    document_path = parent.join(name);
                }
            }
        }
        let root = self
            .project_root
            .as_ref()
            .filter(|root| doc.session.path().starts_with(root) || document_path.starts_with(root))
            .cloned()
            .or_else(|| {
                document_path
                    .parent()
                    .filter(|p| p.is_absolute())
                    .map(|p| p.to_path_buf())
            })
            .or_else(|| std::env::current_dir().ok())
            .and_then(|p| p.canonicalize().ok());
        let Some(root) = root else {
            self.error("No se pudo fijar la carpeta del Markdown");
            return Action::None;
        };
        let color = super::shell::BACKGROUND;
        let context = ImageContext {
            stamp,
            root,
            background: [color as u8, (color >> 8) as u8, (color >> 16) as u8],
        };
        self.markdown_images.cancel();
        self.markdown_gui = Some(MarkdownGui {
            stamp,
            fonts: None,
            page_rows: 1,
            wheel: 0,
            token: Rc::new(()),
            hits: None,
            selected: 0,
            keyboard_focus: false,
            context,
            document_path,
            wanted: Vec::new(),
            sent: Vec::new(),
            remote_allowed: Vec::new(),
            anchor: None,
            project: self.project_root.clone(),
            extents: Vec::new(),
            view: MarkdownView::at_line(self.workbench.active_position().map_or(0, |p| p.line)),
        });
        self.status = "Markdown · lectura · Ctrl+Alt+M o Esc vuelve a la fuente".into();
        Action::None
    }
    pub(super) fn markdown_key(&mut self, code: usize, mods: Modifiers) -> Action {
        if code == 0x09 && (mods == Modifiers::NONE || mods == Modifiers::SHIFT) {
            self.markdown_select_link(mods == Modifiers::SHIFT);
            return Action::None;
        }
        if code == 0x0d && (mods == Modifiers::NONE || mods == Modifiers::CTRL) {
            return self.markdown_activate_selected(mods == Modifiers::CTRL);
        }
        if code == 0x1b {
            self.markdown_gui = None;
            self.markdown_images.cancel();
            return Action::None;
        }
        if code == 0x74 {
            self.markdown_gui = None;
            return self.execute(CommandId::ViewMarkdownPreview);
        }
        let Some(ui) = &mut self.markdown_gui else {
            return Action::None;
        };
        let Some(doc) = self.workbench.document_by_id(ui.stamp.document) else {
            return Action::None;
        };
        let tab = doc.tab_id;
        let page = ui.page_rows.clamp(1, 96) as isize;
        if matches!(code, 0x26 | 0x28 | 0x21 | 0x22 | 0x24 | 0x23) {
            ui.token = Rc::new(());
            ui.hits = None;
            ui.keyboard_focus = false;
        }
        match code {
            0x26 => ui.view.scroll(-1, &self.workbench, tab),
            0x28 => ui.view.scroll(1, &self.workbench, tab),
            0x21 => ui.view.scroll(-page, &self.workbench, tab),
            0x22 => ui.view.scroll(page, &self.workbench, tab),
            0x24 => ui.view.first(),
            0x23 => ui.view.last(&self.workbench, tab),
            _ => {}
        }
        Action::None
    }
    pub(super) fn markdown_presentation(&mut self) -> Option<MarkdownFrame> {
        if self.markdown_gui.is_none() {
            // Commands can close the view without a subsequent task message.
            // Release its pixels/queued work when replacing the retained frame,
            // rather than depending on an unrelated background completion.
            self.markdown_images.cancel();
            return None;
        }
        if self
            .markdown_gui
            .as_ref()
            .is_some_and(|ui| ui.project != self.project_root)
        {
            self.markdown_gui = None;
            self.markdown_images.cancel();
            return None;
        }
        if self.markdown_gui.as_ref().is_some_and(|ui| {
            self.workbench
                .active_document()
                .is_none_or(|doc| doc.id != ui.stamp.document)
        }) {
            self.markdown_gui = None;
            self.markdown_images.cancel();
            return None;
        }
        let viewport_left = super::explorer_panel::region(self).map_or(0, |r| r.right + 1);
        let ui = self.markdown_gui.as_mut()?;
        let tab = self.workbench.document_by_id(ui.stamp.document)?.tab_id;
        let current = self.workbench.task_stamp_is_current(ui.stamp)
            && self
                .workbench
                .active_document()
                .is_some_and(|d| d.id == ui.stamp.document);
        let (width, height) = (self.width.clamp(1, 8192), self.height.clamp(1, 8192));
        let top = self.dpi.pixels(super::shell::TABS).min(height);
        let (cw, ch) = (
            self.cell_width.clamp(1, 8192),
            self.cell_height.clamp(1, 8192),
        );
        if ui.fonts.as_ref().is_none_or(|fonts| fonts.dpi != self.dpi) {
            match unsafe { Fonts::new(self.dpi) } {
                Ok(fonts) => ui.fonts = Some(Rc::new(fonts)),
                Err(error) => {
                    self.status = error;
                    return None;
                }
            }
        }
        let mut presentation = if current {
            ui.view.prepare(
                &self.workbench,
                tab,
                (((height - top).max(0) / ch).saturating_sub(5)).clamp(0, 96) as usize,
                (((width - viewport_left).min(self.dpi.pixels(1000)) - self.dpi.pixels(96)).max(0)
                    / cw.max(self.dpi.pixels(12)))
                .clamp(12, 256) as usize,
            )
        } else {
            MarkdownPresentation {
                message: "El documento cambió; F5 actualiza, Esc vuelve a la fuente".into(),
                ..Default::default()
            }
        };
        if !presentation.pending && !presentation.limited && current {
            presentation.message="Tab selecciona enlaces · Intro abre · Ctrl+Intro al lado · Clic carga imágenes remotas".into();
        }
        if presentation.pending {
            self.syntax_pending = true;
            self.syntax_targets[0] = Some((self.workbench.panes().active(), tab));
            self.wakeup.signal();
        }
        let images = interaction::prepare_images(
            &presentation,
            interaction::ImageLayout {
                cache: &self.markdown_images,
                context: &ui.context,
                document: &ui.document_path,
                remote_allowed: &ui.remote_allowed,
                extents: &ui.extents,
                width: width - viewport_left,
                dpi: self.dpi,
                max_group_height: (height - top - self.dpi.pixels(144)).max(1),
            },
        );
        let mut used = self.dpi.pixels(48);
        let available = height - top - self.dpi.pixels(30);
        let keep = presentation
            .rows
            .iter()
            .enumerate()
            .take_while(|(index, row)| {
                used += row_height(row, self.dpi)
                    + images
                        .iter()
                        .filter(|image| image.row == *index)
                        .map(|image| image.height)
                        .sum::<i32>();
                used <= available
            })
            .count();
        let keep = if available > self.dpi.pixels(48) && !presentation.rows.is_empty() {
            keep.max(1)
        } else {
            keep
        };
        presentation.rows.truncate(keep);
        let images: Vec<_> = images
            .into_iter()
            .filter(|image| image.row < keep)
            .collect();
        let wanted: Vec<_> = images
            .iter()
            .filter(|image| image.allowed)
            .filter_map(|image| image.source.clone())
            .take(16)
            .collect();
        if ui.wanted != wanted {
            ui.wanted = wanted;
            self.wakeup.signal();
        }
        ui.page_rows = keep.max(1);
        let mut frame = MarkdownFrame {
            presentation,
            width,
            viewport_left,
            height,
            top,
            ch,
            dpi: self.dpi,
            fonts: ui.fonts.as_ref().unwrap().clone(),
            hits: MarkdownHits::new(
                ui.token.clone(),
                ui.stamp,
                width,
                height,
                self.dpi,
                viewport_left,
            ),
            images,
            selected: None,
        };
        frame.prepare_hits();
        if ui.keyboard_focus {
            frame.selected = frame.hits.targets().get(ui.selected).cloned();
        }
        Some(frame)
    }
    pub(super) fn markdown_wheel(&mut self, delta: i32, lines: u32) -> bool {
        let Some(ui) = &mut self.markdown_gui else {
            return false;
        };
        let total = ui.wheel.saturating_add(delta);
        ui.wheel = total % 120;
        let rows = if lines == u32::MAX {
            ui.page_rows
        } else {
            (lines as usize).min(96)
        };
        let steps = (total / 120).saturating_mul(rows as i32).clamp(-96, 96);
        if steps != 0 {
            ui.token = Rc::new(());
            ui.hits = None;
            ui.keyboard_focus = false;
            if let Some(doc) = self.workbench.document_by_id(ui.stamp.document) {
                ui.view
                    .scroll(-(steps as isize), &self.workbench, doc.tab_id);
            }
        }
        true
    }
}

struct Fonts {
    dpi: super::dpi::Dpi,
    body: Handle,
    bold: Handle,
    italic: Handle,
    strike: Handle,
    code: Handle,
    link: Handle,
    headings: [Handle; 6],
}
impl Fonts {
    unsafe fn new(dpi: super::dpi::Dpi) -> Result<Self, String> {
        let mut fonts = Self {
            dpi,
            body: null_mut(),
            bold: null_mut(),
            italic: null_mut(),
            strike: null_mut(),
            code: null_mut(),
            link: null_mut(),
            headings: [null_mut(); 6],
        };
        let create = |size, weight, italic, underline, strike, face: &str| {
            CreateFontW(
                -dpi.pixels(size),
                0,
                0,
                0,
                weight,
                italic,
                underline,
                strike,
                1,
                0,
                0,
                5,
                0,
                wide(face).as_ptr(),
            )
        };
        fonts.body = create(16, 400, 0, 0, 0, "Segoe UI");
        fonts.bold = create(16, 700, 0, 0, 0, "Segoe UI");
        fonts.italic = create(16, 400, 1, 0, 0, "Segoe UI");
        fonts.strike = create(16, 400, 0, 0, 1, "Segoe UI");
        fonts.code = create(14, 400, 0, 0, 0, "Consolas");
        fonts.link = create(16, 400, 0, 1, 0, "Segoe UI");
        for (i, size) in [28, 24, 21, 19, 17, 16].into_iter().enumerate() {
            fonts.headings[i] = create(size, 700, 0, 0, 0, "Segoe UI");
        }
        if [
            fonts.body,
            fonts.bold,
            fonts.italic,
            fonts.strike,
            fonts.code,
            fonts.link,
        ]
        .into_iter()
        .chain(fonts.headings)
        .any(|h| h.is_null())
        {
            return Err("No se pudieron preparar las fuentes de Markdown".into());
        }
        Ok(fonts)
    }
}
impl Drop for Fonts {
    fn drop(&mut self) {
        unsafe {
            for font in [
                self.body,
                self.bold,
                self.italic,
                self.strike,
                self.code,
                self.link,
            ]
            .into_iter()
            .chain(self.headings)
            {
                if !font.is_null() {
                    DeleteObject(font);
                }
            }
        }
    }
}
fn row_height(row: &MarkdownRow, dpi: super::dpi::Dpi) -> i32 {
    dpi.pixels(match row.block {
        MarkdownBlock::Heading(level) => {
            if level <= 2 {
                66
            } else {
                50
            }
        }
        MarkdownBlock::Blank => 14,
        MarkdownBlock::Rule => 20,
        MarkdownBlock::Code { fence: true } => 24,
        MarkdownBlock::Table {
            separator: true, ..
        } => 4,
        _ => 28,
    })
}
fn area(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
    Rect {
        left,
        top,
        right,
        bottom,
    }
}
fn span_font(
    fonts: &Fonts,
    row: &MarkdownRow,
    span: &jc_app::markdown_preview::MarkdownSpan,
) -> Handle {
    if let MarkdownBlock::Heading(level) = row.block {
        return fonts.headings[level.clamp(1, 6) as usize - 1];
    }
    if matches!(row.block, MarkdownBlock::Code { .. }) || span.kind == jc_app::SyntaxKind::Code {
        return fonts.code;
    }
    match span.style {
        MarkdownInlineStyle::Emphasis => fonts.italic,
        MarkdownInlineStyle::Strike => fonts.strike,
        _ => match span.kind {
            jc_app::SyntaxKind::Emphasis => fonts.bold,
            jc_app::SyntaxKind::Link => fonts.link,
            _ => fonts.body,
        },
    }
}
fn span_text<'a>(
    row: &MarkdownRow,
    index: usize,
    span: &'a jc_app::markdown_preview::MarkdownSpan,
) -> &'a str {
    if row.block == MarkdownBlock::Quote && index == 0 {
        span.text.strip_prefix("│ ").unwrap_or(&span.text)
    } else if matches!(row.block, MarkdownBlock::Code { fence: true }) {
        span.text
            .trim_start()
            .strip_prefix("Código")
            .unwrap_or(&span.text)
            .trim()
    } else if matches!(row.block, MarkdownBlock::Code { fence: false }) && index == 0 {
        span.text.strip_prefix("  ").unwrap_or(&span.text)
    } else {
        &span.text
    }
}

/// The painter consumes only bounded immutable presentation, never document bytes.
pub(super) unsafe fn draw(dc: Hdc, frame: &MarkdownFrame) -> Result<(), String> {
    let bg = super::shell::BACKGROUND;
    let surface = super::shell::SIDEBAR;
    let fg = super::shell::TEXT;
    let muted = super::shell::MUTED;
    let accent = super::shell::ACCENT;
    let p = |v| frame.dpi.pixels(v);
    let bottom = (frame.height - frame.ch - p(6)).max(frame.top);
    paint::fill(
        dc,
        area(frame.viewport_left, frame.top, frame.width, bottom),
        bg,
    )?;
    let (left, right) = frame.content_bounds();
    let old = SelectObject(dc, frame.fonts.body);
    let old_mode = SetBkMode(dc, 1);
    let result = (|| {
        paint::text(
            dc,
            area(left, frame.top + p(10), right, frame.top + p(35)),
            &wide("Markdown · vista renderizada"),
            muted,
            bg,
        )?;
        let mut y = frame.top + p(48);
        for (row_index, row) in frame.presentation.rows.iter().enumerate() {
            let height = row_height(row, frame.dpi)
                + frame
                    .images
                    .iter()
                    .filter(|image| image.row == row_index)
                    .map(|image| image.height)
                    .sum::<i32>();
            let box_rect = area(left, y, right, (y + height).min(bottom));
            let mut x = left;
            let mut row_bg = bg;
            match row.block {
                MarkdownBlock::Blank => {
                    y += height;
                    continue;
                }
                MarkdownBlock::Rule => {
                    paint::fill(dc, area(left, y + p(9), right, y + p(10)), muted)?;
                    y += height;
                    continue;
                }
                MarkdownBlock::Code { .. } => {
                    row_bg = surface;
                    paint::fill(dc, box_rect, row_bg)?;
                    x += p(12);
                }
                MarkdownBlock::Quote => {
                    row_bg = surface;
                    paint::fill(dc, box_rect, row_bg)?;
                    paint::fill(dc, area(left, y, left + p(3), y + height), accent)?;
                    x += p(16);
                }
                MarkdownBlock::Table { columns, separator } => {
                    if separator {
                        paint::fill(dc, area(left, y, right, y + p(1)), muted)?;
                        y += height;
                        continue;
                    }
                    let text: String = row.spans.iter().map(|s| s.text.as_str()).collect();
                    let cells: Vec<_> = text.trim().trim_matches('│').split('│').take(8).collect();
                    let count = usize::from(columns).max(1);
                    let cell_width = (right - left) / count as i32;
                    let header = frame
                        .presentation
                        .rows
                        .get(row_index + 1)
                        .is_some_and(|next| {
                            matches!(
                                next.block,
                                MarkdownBlock::Table {
                                    separator: true,
                                    ..
                                }
                            )
                        });
                    SelectObject(
                        dc,
                        if header {
                            frame.fonts.bold
                        } else {
                            frame.fonts.body
                        },
                    );
                    for (n, cell) in cells.iter().take(count).enumerate() {
                        let a = left + n as i32 * cell_width;
                        paint::fill(dc, area(a, y, a + cell_width, y + height), surface)?;
                        paint::fill(dc, area(a, y, a + p(1), y + height), muted)?;
                        paint::text(
                            dc,
                            area(a + p(8), y + p(3), a + cell_width - p(5), y + height - p(2)),
                            &wide(cell.trim()),
                            fg,
                            surface,
                        )?;
                    }
                    paint::fill(dc, area(left, y + height - p(1), right, y + height), muted)?;
                    paint::fill(dc, area(right - p(1), y, right, y + height), muted)?;
                    y += height;
                    continue;
                }
                _ => {}
            }
            for (index, span) in row.spans.iter().enumerate() {
                let heading = if let MarkdownBlock::Heading(level) = row.block {
                    Some(level.clamp(1, 6) as usize - 1)
                } else {
                    None
                };
                let font = span_font(&frame.fonts, row, span);
                SelectObject(dc, font);
                let text = span_text(row, index, span);
                if text.is_empty() {
                    continue;
                }
                let value = wide(text);
                let mut measured = TextSize::default();
                if GetTextExtentPoint32W(
                    dc,
                    value.as_ptr(),
                    (value.len() - 1) as i32,
                    &mut measured,
                ) == 0
                {
                    return Err("No se pudo medir el texto Markdown".into());
                }
                let color = if span.kind == jc_app::SyntaxKind::Link {
                    0x00e5c28a
                } else if heading.is_some() {
                    fg
                } else if matches!(row.block, MarkdownBlock::Code { .. }) {
                    0x00ded4c5
                } else {
                    fg
                };
                let inline_code = span.kind == jc_app::SyntaxKind::Code
                    && !matches!(row.block, MarkdownBlock::Code { .. });
                let ink = if inline_code { surface } else { row_bg };
                SetTextColor(dc, color);
                SetBkColor(dc, ink);
                SetBkMode(dc, 2);
                let mut target = area(x, y + p(3), right - p(4), box_rect.bottom - p(2));
                if heading.is_some() {
                    DrawTextW(
                        dc,
                        value.as_ptr(),
                        (value.len() - 1) as i32,
                        &mut target,
                        0x10 | 0x800,
                    );
                } else {
                    paint::text(
                        dc,
                        area(
                            x,
                            y + p(3),
                            (x + measured.width + p(4)).min(right),
                            box_rect.bottom - p(2),
                        ),
                        &value,
                        color,
                        ink,
                    )?;
                }
                x = (x + measured.width).min(right);
            }
            let mut image_top = y + row_height(row, frame.dpi);
            for image in frame.images.iter().filter(|image| image.row == row_index) {
                interaction::draw_image(dc, frame, image, left, right, image_top)?;
                image_top += image.height;
            }
            y += height;
        }
        if let Some(selected) = &frame.selected {
            for (rect, target) in &frame.hits.actions {
                if Arc::ptr_eq(target, selected) {
                    paint::fill(
                        dc,
                        area(rect.left, rect.bottom - p(1), rect.right, rect.bottom),
                        accent,
                    )?;
                }
            }
        }
        SelectObject(dc, frame.fonts.body);
        paint::text(
            dc,
            area(left, (bottom - p(24)).max(frame.top), right, bottom),
            &wide(&frame.presentation.message),
            muted,
            bg,
        )
    })();
    SelectObject(dc, old);
    SetBkMode(dc, old_mode);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn markdown_native_painter_draws_blocks_styles_and_grid_without_editing_source() {
        let mut state = State::new(&crate::GuiOptions::default()).unwrap();
        state.width = 1100;
        state.height = 900;
        state
            .workbench
            .new_file("markdown-native.md", jc_text::TextEncoding::Utf8);
        state.workbench.insert_text("# Markdown en CIDE\n\nTexto con **negrita**, *cursiva*, ~~tachado~~ y `código`.\n\n> Una cita con franja propia.\n\n- Primer elemento\n- [x] Tarea completa\n\n| Elemento | Estado |\n| --- | --- |\n| Vista nativa | Lista |\n\n---\n\n```rust\nfn main() {}\n```\n").unwrap();
        state
            .workbench
            .go_to_position(jc_text::TextPosition { line: 0, column: 0 })
            .unwrap();
        let before = state.workbench.active_bytes().unwrap();
        state.toggle_markdown();
        let tab = state.workbench.active_document().unwrap().tab_id;
        let _ = state.workbench.syntax_rows(tab, 0, 64);
        for _ in 0..16 {
            state.workbench.continue_syntax(tab);
        }
        let preview = state.markdown_presentation().unwrap();
        assert!(preview.presentation.rows.iter().any(|row| matches!(
            row.block,
            MarkdownBlock::Table {
                separator: false,
                ..
            }
        )));
        assert!(preview
            .presentation
            .rows
            .iter()
            .flat_map(|row| &row.spans)
            .any(|span| span.style == MarkdownInlineStyle::Emphasis));
        let frame = super::super::paint::prepare(&mut state);
        let folder = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../work/markdown-review-20261106");
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join(format!("native-raster-{}.bmp", std::process::id()));
        unsafe {
            super::super::paint::bitmap(null_mut(), &frame, &path).unwrap();
        }
        assert!(std::fs::metadata(&path).unwrap().len() > 54);
        assert_eq!(state.workbench.active_bytes().unwrap(), before);
        println!("Native Markdown raster: {}", path.display());
    }
}

#[cfg(test)]
#[path = "markdown_media_tests.rs"]
mod media_tests;
