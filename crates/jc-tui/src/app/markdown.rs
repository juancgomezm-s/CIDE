use super::TuiApp;
use crate::state::UiOverlay;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use jc_app::{
    markdown_preview::{
        resolve_destination, MarkdownAnchorProgress, MarkdownAnchorScan, MarkdownDestination,
        MarkdownPresentation, MarkdownTarget, MarkdownView,
    },
    PrimaryFocus, TaskStamp,
};
use std::sync::Arc;
pub(crate) struct MarkdownUi {
    pub stamp: TaskStamp,
    pub view: MarkdownView,
    pub targets: Vec<Arc<MarkdownTarget>>,
    pub selected: Option<usize>,
    anchor: Option<MarkdownAnchorScan>,
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
            targets: Vec::new(),
            selected: None,
            anchor: None,
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
        if matches!(key.code, KeyCode::Tab | KeyCode::BackTab)
            && (key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT)
        {
            if let Some(ui) = &mut self.markdown_ui {
                if !self.workbench.task_stamp_is_current(ui.stamp) {
                    ui.targets.clear();
                    ui.selected = None;
                    return;
                }
                if !ui.targets.is_empty() {
                    let back = key.code == KeyCode::BackTab || key.modifiers == KeyModifiers::SHIFT;
                    let count = ui.targets.len();
                    ui.selected = Some(match ui.selected {
                        None => {
                            if back {
                                count - 1
                            } else {
                                0
                            }
                        }
                        Some(index) => {
                            if back {
                                (index + count - 1) % count
                            } else {
                                (index + 1) % count
                            }
                        }
                    });
                }
            }
            return;
        }
        if key.code == KeyCode::Enter
            && (key.modifiers.is_empty() || key.modifiers == KeyModifiers::CONTROL)
        {
            self.activate_markdown_link_with(
                key.modifiers == KeyModifiers::CONTROL,
                jc_platform::web_link::open,
            );
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
        if matches!(
            key.code,
            KeyCode::Up
                | KeyCode::Down
                | KeyCode::PageUp
                | KeyCode::PageDown
                | KeyCode::Home
                | KeyCode::End
        ) {
            ui.targets.clear();
            ui.selected = None;
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
            ui.targets.clear();
            ui.selected = None;
            return Some(MarkdownPresentation {
                message: "El documento cambió; F5 actualiza, Esc vuelve a la fuente".into(),
                ..Default::default()
            });
        }
        let tab = self.workbench.document_by_id(ui.stamp.document)?.tab_id;
        let mut presentation = ui.view.prepare(&self.workbench, tab, rows, columns);
        ui.targets.clear();
        if !presentation.pending && !presentation.limited && ui.anchor.is_none() {
            for target in presentation
                .rows
                .iter()
                .flat_map(|row| &row.spans)
                .filter_map(|span| span.target.as_ref())
            {
                if ui.targets.len() < 128 && !ui.targets.iter().any(|old| Arc::ptr_eq(old, target))
                {
                    ui.targets.push(target.clone());
                }
            }
        }
        ui.selected = ui.selected.filter(|&index| index < ui.targets.len());
        presentation.message = if ui.anchor.is_some() {
            "Buscando ancla por tramos · Esc cancela".into()
        } else if let Some((index, target)) = ui
            .selected
            .and_then(|index| ui.targets.get(index).map(|target| (index, target)))
        {
            format!(
                "Enlace {}/{} · {} · Intro abrir · Ctrl+Intro al lado",
                index + 1,
                ui.targets.len(),
                jc_app::source_services::display_ui_label(&target.destination, 120)
            )
        } else if !ui.targets.is_empty() {
            format!(
                "{} enlaces visibles · Tab/Mayús+Tab elegir · Intro abrir · Ctrl+Intro al lado",
                ui.targets.len()
            )
        } else {
            presentation.message
        };
        if presentation.pending {
            self.syntax_pending = true;
            self.syntax_targets[0] = Some((self.workbench.panes().active(), tab));
            self.syntax_wakeup.signal();
        }
        Some(presentation)
    }
    fn activate_markdown_link_with(
        &mut self,
        to_side: bool,
        open_external: impl FnOnce(&str) -> Result<(), String>,
    ) {
        let Some(ui) = &self.markdown_ui else {
            return;
        };
        if !self.workbench.task_stamp_is_current(ui.stamp)
            || !self
                .workbench
                .active_document()
                .is_some_and(|doc| doc.id == ui.stamp.document)
        {
            self.status = "Cambió el documento; F5 actualiza los enlaces".into();
            return;
        }
        let Some(target) = ui.selected.and_then(|index| ui.targets.get(index)).cloned() else {
            self.status = "Tab elige un enlace visible antes de abrir".into();
            return;
        };
        let Some(doc) = self.workbench.document_by_id(ui.stamp.document) else {
            return;
        };
        let source = doc.session.path().to_path_buf();
        let destination = match resolve_destination(&source, &target.destination) {
            Ok(value) => value,
            Err(error) => {
                self.status = error;
                return;
            }
        };
        match destination {
            MarkdownDestination::External(url) => {
                if let Err(error) = jc_platform::web_link::validate(&url) {
                    self.status = error;
                    return;
                }
                self.status = match open_external(&url) {
                    Ok(()) => "Enlace abierto en el navegador".into(),
                    Err(error) => error,
                };
            }
            MarkdownDestination::Anchor(fragment) => self.start_markdown_anchor(fragment),
            MarkdownDestination::Local { path, fragment } => {
                if target.image {
                    self.status =
                        "La TUI muestra el texto alternativo; abre la imagen local en la GUI"
                            .into();
                    return;
                }
                let Some(root) = self
                    .project_root
                    .clone()
                    .or_else(|| source.parent().map(std::path::Path::to_path_buf))
                else {
                    self.status = "El documento no tiene carpeta para abrir el enlace".into();
                    return;
                };
                match self.workbench.open_project_file(
                    &root,
                    path,
                    self.default_encoding,
                    to_side,
                    16 * 1024 * 1024,
                ) {
                    Err(error) => self.status = error.to_string(),
                    Ok(_) => {
                        self.markdown_ui = None;
                        let _ = self.focus.focus(PrimaryFocus::Editor, &self.panels);
                        self.ensure_encoding_choice();
                        if self.registry.is_enabled(
                            jc_app::CommandId::ViewMarkdownPreview,
                            &self.command_context(),
                        ) && matches!(self.overlay, UiOverlay::None)
                        {
                            self.toggle_markdown();
                            if let Some(fragment) = fragment {
                                self.start_markdown_anchor(fragment);
                            }
                        }
                        self.status = "Enlace local abierto; fuente original intacta".into();
                    }
                }
            }
        }
    }
    fn start_markdown_anchor(&mut self, fragment: String) {
        if let Some(ui) = &mut self.markdown_ui {
            ui.anchor = Some(MarkdownAnchorScan::new(ui.stamp, fragment));
            ui.targets.clear();
            ui.selected = None;
            self.status = "Buscando ancla Markdown por tramos".into();
            self.syntax_wakeup.signal();
        }
    }
    pub(crate) fn poll_markdown_anchor(&mut self) -> bool {
        let Some(ui) = &mut self.markdown_ui else {
            return false;
        };
        let Some(scan) = &mut ui.anchor else {
            return false;
        };
        match scan.step(&self.workbench) {
            MarkdownAnchorProgress::Pending => {
                self.syntax_wakeup.signal();
                false
            }
            MarkdownAnchorProgress::Found(line) => {
                ui.view = MarkdownView::at_line(line);
                ui.anchor = None;
                ui.targets.clear();
                ui.selected = None;
                self.status = "Ancla Markdown encontrada".into();
                true
            }
            result => {
                ui.anchor = None;
                ui.targets.clear();
                ui.selected = None;
                self.status = match result {
                    MarkdownAnchorProgress::NotFound => "No se encontró el ancla Markdown",
                    MarkdownAnchorProgress::Stale => {
                        "Cambió el documento al buscar el ancla; F5 actualiza"
                    }
                    _ => "Búsqueda de ancla limitada por presupuesto; consulta la fuente",
                }
                .into();
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::LaunchOptions;
    use crossterm::event::Event;
    use jc_text::{TextEncoding, TextPosition};
    fn key(app: &mut TuiApp, code: KeyCode, modifiers: KeyModifiers) {
        app.handle_event(Event::Key(KeyEvent::new(code, modifiers)));
    }
    fn reading(source: &str) -> TuiApp {
        let mut app = TuiApp::new(LaunchOptions::default());
        app.panels.hide_sidebar();
        app.workbench.new_file("links.md", TextEncoding::Utf8);
        app.workbench.insert_text(source).unwrap();
        app.workbench
            .go_to_position(TextPosition { line: 0, column: 0 })
            .unwrap();
        app.toggle_markdown();
        app.prepare_frame(100, 35);
        app
    }
    #[test]
    fn markdown_tui_links_cycle_highlight_activate_explicit_web_and_anchor_without_source_edits() {
        let source="[Web](https://example.com/guia)\n[Destino](#destino)\n![Imagen remota](https://example.com/image.png)\n\nTexto\n\n# Destino\nFinal\n";
        let mut app = reading(source);
        let before = app.workbench.active_bytes().unwrap();
        let panes = format!("{:?}", app.workbench.panes());
        assert_eq!(app.markdown_ui.as_ref().unwrap().targets.len(), 3);
        key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(app.markdown_ui.as_ref().unwrap().selected, Some(0));
        let frame = app.frame(100, 35);
        assert_eq!(frame.cell(3, 5).style, crate::CellStyle::Reverse);
        let mut opened = Vec::new();
        app.activate_markdown_link_with(false, |url| {
            opened.push(url.to_string());
            Ok(())
        });
        assert_eq!(opened, ["https://example.com/guia"]);
        key(&mut app, KeyCode::BackTab, KeyModifiers::SHIFT);
        assert_eq!(app.markdown_ui.as_ref().unwrap().selected, Some(2));
        app.activate_markdown_link_with(false, |url| {
            opened.push(url.to_string());
            Ok(())
        });
        assert_eq!(opened[1], "https://example.com/image.png");
        key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.markdown_ui.as_ref().unwrap().anchor.is_some());
        app.poll_tasks();
        let frame = app.prepare_frame(100, 35);
        assert_eq!(
            frame
                .markdown
                .as_ref()
                .unwrap()
                .rows
                .first()
                .unwrap()
                .source_line,
            6
        );
        assert_eq!(app.workbench.active_bytes().unwrap(), before);
        assert_eq!(format!("{:?}", app.workbench.panes()), panes);
    }
    #[test]
    fn markdown_tui_local_links_use_project_scope_and_side_open_preserves_dirty_source() {
        use super::super::file_browser_tests::Fixture;
        use jc_app::PaneId;
        let fixture = Fixture::new();
        let source_path = fixture.put("docs/source.md", b"[Otro](../target.md#destino)\r\n");
        let target = fixture.put("target.md", b"# Inicio\r\nTexto\r\n# Destino\r\nFin\r\n");
        for side in [false, true] {
            let mut app = TuiApp::new(LaunchOptions {
                files: Vec::new(),
                encoding: Some(TextEncoding::Utf8),
            });
            app.project_root = Some(fixture.root.clone());
            app.panels.hide_sidebar();
            app.open_path(source_path.clone()).unwrap();
            app.workbench.insert_text("cambio sin guardar\n").unwrap();
            app.workbench
                .go_to_position(TextPosition { line: 0, column: 0 })
                .unwrap();
            let source = app.workbench.active_document().unwrap().id;
            let source_tab = app.workbench.active_document().unwrap().tab_id;
            let before = app.workbench.active_bytes().unwrap();
            let revision = app
                .workbench
                .active_document()
                .unwrap()
                .session
                .document()
                .revision();
            app.toggle_markdown();
            app.prepare_frame(100, 35);
            key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
            key(
                &mut app,
                KeyCode::Enter,
                if side {
                    KeyModifiers::CONTROL
                } else {
                    KeyModifiers::NONE
                },
            );
            app.poll_tasks();
            let frame = app.prepare_frame(100, 35);
            assert_eq!(
                frame
                    .markdown
                    .as_ref()
                    .unwrap()
                    .rows
                    .first()
                    .unwrap()
                    .source_line,
                2
            );
            assert_eq!(
                app.workbench.active_document().unwrap().session.path(),
                target
            );
            if side {
                assert_eq!(app.workbench.panes().active(), PaneId::Secondary);
                assert_eq!(
                    app.workbench
                        .panes()
                        .view(PaneId::Primary)
                        .unwrap()
                        .location()
                        .unwrap()
                        .tab,
                    source_tab
                );
            }
            let original = app.workbench.document_by_id(source).unwrap();
            assert_eq!(
                original
                    .session
                    .document()
                    .read_range(0..original.session.document().len())
                    .unwrap(),
                before
            );
            assert_eq!(original.session.document().revision(), revision);
            assert_eq!(
                std::fs::read(&source_path).unwrap(),
                b"[Otro](../target.md#destino)\r\n"
            );
            assert_eq!(
                std::fs::read(&target).unwrap(),
                b"# Inicio\r\nTexto\r\n# Destino\r\nFin\r\n"
            );
        }
    }
    #[test]
    fn markdown_tui_stale_links_and_local_images_do_not_launch_or_edit() {
        let mut app = reading("[Web](https://example.com)\n");
        key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        app.workbench.insert_text("nuevo").unwrap();
        let before = app.workbench.active_bytes().unwrap();
        app.activate_markdown_link_with(false, |_| panic!("stale link must not launch"));
        assert!(app.status.contains("Cambió"));
        assert_eq!(app.workbench.active_bytes().unwrap(), before);
        let fixture = super::super::file_browser_tests::Fixture::new();
        let path = fixture.put("image.md", b"![local](foto.png)\n");
        let mut app = TuiApp::new(LaunchOptions {
            files: vec![path],
            encoding: Some(TextEncoding::Utf8),
        });
        app.toggle_markdown();
        app.prepare_frame(100, 35);
        key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        app.activate_markdown_link_with(false, |_| panic!("local image is not a browser URL"));
        assert!(app.status.contains("texto alternativo"));
        assert!(app.markdown_ui.is_some());
    }
}
