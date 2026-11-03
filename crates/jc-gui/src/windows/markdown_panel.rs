//! Read-only native Markdown surface over the same bounded range presenter.
use super::{ffi::*, paint, wide, Action, State};
use jc_app::{
    markdown_preview::{MarkdownPresentation, MarkdownView},
    CommandId, Modifiers, TaskStamp,
};
pub(super) struct MarkdownGui {
    stamp: TaskStamp,
    view: MarkdownView,
}
pub(super) struct MarkdownFrame {
    pub presentation: MarkdownPresentation,
    width: i32,
    height: i32,
    cw: i32,
    ch: i32,
}
impl State {
    pub(super) fn toggle_markdown(&mut self) -> Action {
        if self.markdown_gui.take().is_some() {
            self.status = "Vista de fuente; documento sin cambios".into();
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
        self.markdown_gui = Some(MarkdownGui {
            stamp,
            view: MarkdownView::at_line(self.workbench.active_position().map_or(0, |p| p.line)),
        });
        self.status = "Markdown · lectura · Ctrl+Alt+M o Esc vuelve a la fuente".into();
        Action::None
    }
    pub(super) fn markdown_key(&mut self, code: usize, _mods: Modifiers) -> Action {
        if code == 0x1b {
            self.markdown_gui = None;
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
        let page =
            ((self.height / self.cell_height.max(1)).saturating_sub(6)).clamp(1, 96) as isize;
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
        let ui = self.markdown_gui.as_mut()?;
        let tab = self.workbench.document_by_id(ui.stamp.document)?.tab_id;
        let current = self.workbench.task_stamp_is_current(ui.stamp)
            && self
                .workbench
                .active_document()
                .is_some_and(|d| d.id == ui.stamp.document);
        let (width, height) = (self.width.clamp(1, 8192), self.height.clamp(1, 8192));
        let (cw, ch) = (
            self.cell_width.clamp(1, 8192),
            self.cell_height.clamp(1, 8192),
        );
        let presentation = if current {
            ui.view.prepare(
                &self.workbench,
                tab,
                ((height / ch).saturating_sub(6)).clamp(0, 96) as usize,
                ((width - 32).max(0) / cw).clamp(12, 256) as usize,
            )
        } else {
            MarkdownPresentation {
                message: "El documento cambió; F5 actualiza, Esc vuelve a la fuente".into(),
                ..Default::default()
            }
        };
        if presentation.pending {
            self.syntax_pending = true;
            self.syntax_targets[0] = Some((self.workbench.panes().active(), tab));
            self.wakeup.signal();
        }
        Some(MarkdownFrame {
            presentation,
            width,
            height,
            cw,
            ch,
        })
    }
}
pub(super) unsafe fn draw(dc: Hdc, frame: &MarkdownFrame) -> Result<(), String> {
    let bg = 0x00272828;
    let panel = 0x00202122;
    let fg = 0x00d5d7d8;
    let accent = 0x00d0d467;
    let top = (frame.ch + 8).min(frame.height);
    let bottom = (frame.height - frame.ch - 6).max(top);
    paint::fill(
        dc,
        Rect {
            left: 0,
            top,
            right: frame.width,
            bottom,
        },
        bg,
    )?;
    for (i, text) in [
        "Markdown · vista previa",
        "Flechas desplazar · RePág/AvPág · Inicio/Fin · Esc fuente",
    ]
    .iter()
    .enumerate()
    {
        let y = top + i as i32 * frame.ch;
        paint::text(
            dc,
            Rect {
                left: 16.min(frame.width),
                top: y,
                right: frame.width,
                bottom: (y + frame.ch).min(bottom),
            },
            &wide(text),
            accent,
            panel,
        )?;
    }
    for (i, row) in frame.presentation.rows.iter().enumerate() {
        let y = top + (i as i32 + 3) * frame.ch;
        let mut x = 16;
        for span in &row.spans {
            let cells = jc_text::grapheme::width(&span.text);
            let end = (x + cells as i32 * frame.cw).min(frame.width);
            let color = jc_app::syntax_theme::foreground(span.kind);
            let color = u32::from(color.r) | (u32::from(color.g) << 8) | (u32::from(color.b) << 16);
            paint::text(
                dc,
                Rect {
                    left: x,
                    top: y,
                    right: end,
                    bottom: (y + frame.ch).min(bottom),
                },
                &wide(&span.text),
                color,
                bg,
            )?;
            x = end;
        }
    }
    paint::text(
        dc,
        Rect {
            left: 8.min(frame.width),
            top: (bottom - frame.ch).max(top),
            right: frame.width,
            bottom,
        },
        &wide(&frame.presentation.message),
        fg,
        panel,
    )
}
