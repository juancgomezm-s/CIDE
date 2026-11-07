use super::TuiApp;
use crate::state::UiOverlay;
use crossterm::event::{KeyCode, KeyEvent};
use jc_app::{
    markdown_preview::{MarkdownPresentation, MarkdownView},
    TaskStamp,
};
pub(crate) struct MarkdownUi {
    pub stamp: TaskStamp,
    pub view: MarkdownView,
}
impl TuiApp {
    pub(crate) fn showing_markdown(&self) -> bool {
        self.markdown_ui.is_some() && matches!(self.overlay, UiOverlay::None)
    }
    pub(crate) fn toggle_markdown(&mut self) {
        if self.markdown_ui.take().is_some() {
            self.status = "Vista de fuente; documento sin cambios".into();
            return;
        }
        if !self.registry.is_enabled(
            jc_app::CommandId::ViewMarkdownPreview,
            &self.command_context(),
        ) {
            self.status = "La vista renderizada solo está disponible para Markdown".into();
            return;
        }
        self.close_overlay();
        self.snippet_ui = None;
        self.source_popup = false;
        self.open_diff_visible = false;
        let Some(doc) = self.workbench.active_document() else {
            return;
        };
        let Some(encoding) = doc.session.encoding() else {
            self.status = "Elige la codificación del Markdown".into();
            return;
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
        self.markdown_ui = Some(MarkdownUi {
            stamp,
            view: MarkdownView::at_line(self.workbench.active_position().map_or(0, |p| p.line)),
        });
        self.status = "Markdown: lectura · Ctrl+Alt+M o Esc vuelve a la fuente".into();
    }
    pub(crate) fn markdown_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Esc {
            self.markdown_ui = None;
            self.status = "Vista de fuente; documento sin cambios".into();
            return;
        }
        if key.code == KeyCode::F(5) {
            self.markdown_ui = None;
            self.toggle_markdown();
            return;
        }
        let Some(ui) = &mut self.markdown_ui else {
            return;
        };
        let Some(doc) = self.workbench.document_by_id(ui.stamp.document) else {
            return;
        };
        let tab = doc.tab_id;
        if !self.workbench.task_stamp_is_current(ui.stamp) {
            return;
        }
        match key.code {
            KeyCode::Up => ui.view.scroll(-1, &self.workbench, tab),
            KeyCode::Down => ui.view.scroll(1, &self.workbench, tab),
            KeyCode::PageUp => ui.view.scroll(
                -(self.last_frame_height.saturating_sub(7) as isize),
                &self.workbench,
                tab,
            ),
            KeyCode::PageDown => ui.view.scroll(
                self.last_frame_height.saturating_sub(7) as isize,
                &self.workbench,
                tab,
            ),
            KeyCode::Home => ui.view.first(),
            KeyCode::End => ui.view.last(&self.workbench, tab),
            _ => {}
        }
    }
    pub(crate) fn prepare_markdown(
        &mut self,
        rows: usize,
        columns: usize,
    ) -> Option<MarkdownPresentation> {
        let ui = self.markdown_ui.as_mut()?;
        let current = self.workbench.task_stamp_is_current(ui.stamp)
            && self
                .workbench
                .active_document()
                .is_some_and(|d| d.id == ui.stamp.document);
        if !current {
            return Some(MarkdownPresentation {
                message: "El documento cambió; F5 actualiza, Esc vuelve a la fuente".into(),
                ..Default::default()
            });
        }
        let tab = self.workbench.document_by_id(ui.stamp.document)?.tab_id;
        let presentation = ui.view.prepare(&self.workbench, tab, rows, columns);
        if presentation.pending {
            self.syntax_pending = true;
            self.syntax_targets[0] = Some((self.workbench.panes().active(), tab));
            self.syntax_wakeup.signal();
        }
        Some(presentation)
    }
}
