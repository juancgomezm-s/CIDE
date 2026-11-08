use super::TuiApp;
use crate::input::{is_action_event, is_plain_text, to_stroke};
use crate::state::UiOverlay;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use jc_app::{BottomPanel, KeyScope, PrimaryFocus, Resolution, SidebarView};

impl TuiApp {
    pub fn handle_event(&mut self, event: Event) {
        if !matches!(event, Event::Mouse(_)) {
            self.invalidate_mouse();
        }
        match event {
            Event::Key(key) if is_action_event(key) => self.handle_key(key),
            Event::Paste(text) => self.handle_paste(&text),
            Event::Mouse(event) => {
                if self.graph_showing() {
                    use crossterm::event::MouseEventKind;
                    match event.kind {
                        MouseEventKind::ScrollUp => self.move_git_graph(-3),
                        MouseEventKind::ScrollDown => self.move_git_graph(3),
                        _ => {}
                    }
                } else if self.git_commit.view().is_none() {
                    self.handle_mouse(event);
                } else {
                    use crossterm::event::MouseEventKind;
                    match event.kind {
                        MouseEventKind::ScrollUp => self.git_commit.scroll(-3),
                        MouseEventKind::ScrollDown => self.git_commit.scroll(3),
                        _ => {}
                    }
                }
            }
            Event::Resize(_, _) | Event::FocusGained | Event::FocusLost => {}
            _ => {}
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if self.clipboard_transport.is_some()
            && key.code == KeyCode::Char('v')
            && key.modifiers == KeyModifiers::CONTROL
            && self.clipboard_field_budget().is_some()
        {
            self.dispatch(jc_app::CommandId::EditPaste);
            return;
        }
        if !matches!(self.overlay, UiOverlay::None) {
            self.handle_overlay_key(key);
            return;
        }
        if self.handle_git_create_commit_key(key) {
            return;
        }
        if self.handle_git_graph_key(key) {
            return;
        }
        if self.showing_markdown() {
            if let Some(stroke) = to_stroke(key) {
                let scope = KeyScope::from_focus(self.focus.primary(), self.focus.active_overlay());
                if let Resolution::Enabled(command) =
                    self.keymap
                        .resolve(scope, stroke, &self.registry, &self.command_context())
                {
                    if matches!(
                        command,
                        jc_app::CommandId::MenuFile
                            | jc_app::CommandId::MenuEdit
                            | jc_app::CommandId::MenuSearch
                            | jc_app::CommandId::MenuProject
                            | jc_app::CommandId::MenuBuild
                            | jc_app::CommandId::MenuTools
                            | jc_app::CommandId::MenuView
                            | jc_app::CommandId::MenuHelp
                            | jc_app::CommandId::PaletteOpen
                            | jc_app::CommandId::ViewMarkdownPreview
                            | jc_app::CommandId::FileExit
                    ) {
                        self.dispatch(command);
                        return;
                    }
                }
            }
            self.markdown_key(key);
            return;
        }
        if self.showing_snippet() {
            if let Some(stroke) = to_stroke(key) {
                let scope = KeyScope::from_focus(self.focus.primary(), self.focus.active_overlay());
                if let Resolution::Enabled(command) =
                    self.keymap
                        .resolve(scope, stroke, &self.registry, &self.command_context())
                {
                    if Self::keep_snippet_for(command) {
                        self.dispatch(command);
                        return;
                    }
                }
            }
            self.snippet_key(key);
            return;
        }
        if self.showing_source_popup() {
            if let Some(stroke) = to_stroke(key) {
                let scope = KeyScope::from_focus(self.focus.primary(), self.focus.active_overlay());
                if let Resolution::Enabled(command) =
                    self.keymap
                        .resolve(scope, stroke, &self.registry, &self.command_context())
                {
                    if Self::keep_source_for(command) {
                        self.dispatch(command);
                        return;
                    }
                }
            }
            self.source_key(key);
            return;
        }
        if self.focus.primary() == PrimaryFocus::Sidebar
            && self.panels.sidebar() == SidebarView::Symbols
            && (key.modifiers.is_empty()
                || key.code == KeyCode::Enter && key.modifiers == KeyModifiers::CONTROL)
            && matches!(
                key.code,
                KeyCode::Up
                    | KeyCode::Down
                    | KeyCode::PageUp
                    | KeyCode::PageDown
                    | KeyCode::Home
                    | KeyCode::End
                    | KeyCode::Enter
                    | KeyCode::F(5)
                    | KeyCode::Esc
            )
        {
            self.source_key(key);
            return;
        }
        if self.showing_open_diff() {
            if let Some(stroke) = to_stroke(key) {
                let scope = KeyScope::from_focus(self.focus.primary(), self.focus.active_overlay());
                if let Resolution::Enabled(command) =
                    self.keymap
                        .resolve(scope, stroke, &self.registry, &self.command_context())
                {
                    if Self::keep_open_diff_for(command) {
                        self.dispatch(command);
                        return;
                    }
                }
            }
            self.open_diff_key(key);
            return;
        }
        let history_context = self.panels.sidebar_visible()
            && self.panels.sidebar() == SidebarView::GitStatus
            && self.focus.primary() == PrimaryFocus::Sidebar
            || self.git_history.immutable()
                && self.panels.bottom_visible()
                && self.panels.bottom() == BottomPanel::Diff
                && self.focus.primary() == PrimaryFocus::BottomPanel;
        if history_context && self.handle_git_history_key(key) {
            return;
        }
        if let Some(stroke) = to_stroke(key) {
            let scope = KeyScope::from_focus(self.focus.primary(), self.focus.active_overlay());
            let context = self.command_context();
            match self.keymap.resolve(scope, stroke, &self.registry, &context) {
                Resolution::Enabled(command) => {
                    self.dispatch(command);
                    return;
                }
                Resolution::Disabled(_) if key.code == KeyCode::Esc => {}
                Resolution::Disabled(command) => {
                    self.status = self
                        .registry
                        .get(command)
                        .map(|spec| format!("{} no está disponible en este contexto", spec.label))
                        .unwrap_or_else(|| "Comando no disponible".to_owned());
                    return;
                }
                Resolution::None => {}
            }
        }
        if key.code == KeyCode::Esc {
            let _ = self.focus.escape();
            return;
        }
        match self.focus.primary() {
            PrimaryFocus::Editor => self.handle_editor_key(key),
            PrimaryFocus::Sidebar => self.handle_sidebar_key(key),
            PrimaryFocus::BottomPanel => self.handle_bottom_key(key),
        }
    }

    fn handle_editor_key(&mut self, key: KeyEvent) {
        let select = key.modifiers.contains(KeyModifiers::SHIFT);
        let result = match key.code {
            KeyCode::Left => self.workbench.move_left(select),
            KeyCode::Right => self.workbench.move_right(select),
            KeyCode::Up => self.workbench.move_vertical(-1, select),
            KeyCode::Down => self.workbench.move_vertical(1, select),
            KeyCode::PageUp => self.workbench.move_vertical(-10, select),
            KeyCode::PageDown => self.workbench.move_vertical(10, select),
            KeyCode::Home => self.workbench.move_home(select),
            KeyCode::End => self.workbench.move_end(select),
            KeyCode::Backspace if key.modifiers.is_empty() => self.workbench.backspace(),
            KeyCode::Delete if key.modifiers.is_empty() => self.workbench.delete_forward(),
            KeyCode::Tab if key.modifiers.is_empty() => self.workbench.insert_tab(),
            KeyCode::Char(ch) if is_plain_text(key) => self.workbench.insert_text(&ch.to_string()),
            _ => return,
        };
        self.apply_result(result);
    }

    fn handle_sidebar_key(&mut self, key: KeyEvent) {
        if self.panels.sidebar() == SidebarView::GitStatus {
            self.handle_git_status_sidebar_key(key);
            return;
        }
        if self.panels.sidebar() == SidebarView::Git {
            self.handle_git_sidebar_key(key);
            return;
        }
        if self.panels.sidebar() != SidebarView::Explorer {
            return;
        }
        match key.code {
            KeyCode::Up => self.explorer_view.move_selection(&self.explorer, -1),
            KeyCode::Down => self.explorer_view.move_selection(&self.explorer, 1),
            KeyCode::PageUp => self.explorer_view.move_selection(&self.explorer, -10),
            KeyCode::PageDown => self.explorer_view.move_selection(&self.explorer, 10),
            KeyCode::Home => self.explorer_view.first(&self.explorer),
            KeyCode::End => self.explorer_view.last(&self.explorer),
            KeyCode::Left | KeyCode::Backspace => self.explorer_parent(),
            KeyCode::Right => self.activate_explorer(false, true),
            KeyCode::Enter => {
                self.activate_explorer(key.modifiers.contains(KeyModifiers::CONTROL), false)
            }
            KeyCode::Char('r') if key.modifiers.is_empty() => self.refresh_explorer(),
            _ => {}
        }
    }

    pub(super) fn handle_bottom_key(&mut self, key: KeyEvent) {
        if self.panels.bottom() == BottomPanel::Diff {
            self.handle_git_diff_key(key);
            return;
        }
        let count = match self.panels.bottom() {
            BottomPanel::Problems => self.workbench.problems().len(),
            BottomPanel::Output => self.workbench.output().len(),
            BottomPanel::SearchResults => self.search_result_count(),
            BottomPanel::Terminal => 1,
            BottomPanel::Diff => 0,
        };
        match key.code {
            KeyCode::Up => self.bottom_selected = self.bottom_selected.saturating_sub(1),
            KeyCode::Down if count > 0 => {
                self.bottom_selected = (self.bottom_selected + 1).min(count - 1)
            }
            KeyCode::Home => self.bottom_selected = 0,
            KeyCode::End if count > 0 => self.bottom_selected = count - 1,
            KeyCode::Enter
                if self.panels.bottom() == BottomPanel::SearchResults && self.project_results =>
            {
                self.select_project_result()
            }
            KeyCode::Enter if self.panels.bottom() == BottomPanel::SearchResults => {
                match self.workbench.select_search_match(self.bottom_selected) {
                    Ok(true) => {
                        let _ = self.focus.focus(PrimaryFocus::Editor, &self.panels);
                        self.status = format!("Resultado de búsqueda {}", self.bottom_selected + 1);
                    }
                    Ok(false) => {
                        self.status = "No hay un resultado de búsqueda seleccionado".to_owned()
                    }
                    Err(error) => self.status = error.to_string(),
                }
            }
            KeyCode::Enter if self.panels.bottom() == BottomPanel::Problems => {
                let id = self
                    .workbench
                    .problems()
                    .get(self.bottom_selected)
                    .map(|row| row.id);
                if let Some(id) = id {
                    match self.workbench.activate_problem(id) {
                        Ok(()) => {
                            let _ = self.focus.focus(PrimaryFocus::Editor, &self.panels);
                        }
                        Err(error) => self.status = error,
                    }
                }
            }
            KeyCode::Enter => {
                let _ = self.focus.focus(PrimaryFocus::Editor, &self.panels);
            }
            _ => {}
        }
    }

    fn handle_paste(&mut self, text: &str) {
        // Keyboard and paste share the topmost overlay. A palette/menu may be
        // opened above a source selector, snippet or read-only comparison.
        if !matches!(self.overlay, UiOverlay::None) {
            self.append_overlay_text(text);
            return;
        }
        if self.git_commit.view().is_some() {
            self.git_commit.edit(jc_app::FieldEdit::Insert(text));
            return;
        }
        if self.graph_showing() {
            return;
        }
        if self.paste_commit_search(text) {
            return;
        }
        if self.showing_markdown() {
            return;
        }
        if self.showing_snippet() {
            self.edit_snippet(jc_app::FieldEdit::Insert(text));
            return;
        }
        if self.showing_open_diff() || self.showing_source_popup() {
            return;
        }
        if matches!(self.overlay, UiOverlay::None) && self.focus.primary() == PrimaryFocus::Editor {
            let result = self.workbench.insert_text(text);
            self.apply_result(result);
        } else {
            self.append_overlay_text(text);
        }
    }

    pub(crate) fn apply_result(&mut self, result: Result<(), jc_app::WorkbenchError>) {
        match result {
            Ok(()) => self.status = "Listo".to_owned(),
            Err(error) => self.status = error.to_string(),
        }
    }
}
