//! Native S1 surface. Drafts own only bounded configuration, never editor data.
use super::{
    project_results::{ProjectResultsPresentation, ProjectResultsView},
    Action, State,
};
use jc_app::{
    edit_field, present_field, FieldEdit, FieldPresentation, Modifiers, ProjectSearchOptions,
};

pub(super) struct ProjectPanel {
    pub visible: bool,
    pub results: bool,
    pub fields: [String; 3],
    pub cursors: [usize; 3],
    pub active: usize,
    pub generation: u64,
    pub view: ProjectResultsView,
}
impl Default for ProjectPanel {
    fn default() -> Self {
        Self {
            visible: false,
            results: false,
            fields: std::array::from_fn(|_| String::new()),
            cursors: [0; 3],
            active: 0,
            generation: 0,
            view: ProjectResultsView::default(),
        }
    }
}
pub(super) struct ProjectPresentation {
    pub fields: Vec<(&'static str, FieldPresentation)>,
    pub active: usize,
    pub results: Option<ProjectResultsPresentation>,
    pub status: String,
}
fn patterns(text: &str) -> Vec<String> {
    text.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}
impl State {
    pub(super) fn project_visible(&self) -> bool {
        self.project_panel.as_ref().is_some_and(|p| p.visible)
    }
    pub(super) fn open_project_search(&mut self, results: bool) {
        self.close_selector();
        if self.project_root.is_none() {
            self.status = "Open a project folder first".into();
            return;
        }
        let panel = self.project_panel.get_or_insert_with(ProjectPanel::default);
        panel.visible = true;
        panel.results = results;
        panel.generation = panel.generation.wrapping_add(1);
        self.validate_project_search();
    }
    pub(super) fn validate_project_search(&mut self) -> bool {
        let wrong_root = self
            .project_search
            .root()
            .is_some_and(|root| Some(root) != self.project_root.as_deref());
        if wrong_root {
            self.cancel_project_replace();
            self.project_search.clear();
            self.project_options = None;
            self.project_panel = None;
            return true;
        }
        let changed = self.project_search.validate_context(&self.workbench);
        self.validate_replace_preview() | changed
    }
    pub(super) fn start_project_search(&mut self, refresh: bool) {
        let Some(root) = self.project_root.as_ref() else {
            self.error("Open a project folder first");
            return;
        };
        let options = if refresh {
            let Some(options) = self.project_options.clone() else {
                self.error("Run a project query before refresh");
                return;
            };
            options
        } else {
            let Some(panel) = self.project_panel.as_ref() else {
                return;
            };
            let mut options = ProjectSearchOptions::literal(panel.fields[0].clone());
            if !panel.fields[1].trim().is_empty() {
                options.include = patterns(&panel.fields[1]);
            }
            options.exclude.extend(patterns(&panel.fields[2]));
            options.encoding = self.encoding;
            options.workers = self.project_workers;
            options
        };
        match self
            .project_search
            .start(&self.workbench, root, options.clone())
        {
            Ok(()) => {
                self.project_options = Some(options);
                let panel = self.project_panel.get_or_insert_with(ProjectPanel::default);
                panel.visible = true;
                panel.results = true;
                panel.view.first();
                self.status = "Search Plus running · literal, case-sensitive, exact EOL".into();
            }
            Err(error) => self.error(error),
        }
    }
    pub(super) fn edit_project_field(&mut self, edit: FieldEdit<'_>) {
        let Some(panel) = self
            .project_panel
            .as_mut()
            .filter(|p| p.visible && !p.results)
        else {
            return;
        };
        panel.generation = panel.generation.wrapping_add(1);
        match edit_field(
            &mut panel.fields[panel.active],
            &mut panel.cursors[panel.active],
            edit,
            4096,
        ) {
            Ok(_) => self.status = "Project draft ready · Enter starts a bounded scan".into(),
            Err(error) => self.error(error),
        }
    }
    pub(super) fn project_key(&mut self, code: usize, modifiers: Modifiers) -> Action {
        if code == 0x1b {
            self.project_search.cancel();
            if let Some(panel) = self.project_panel.as_mut() {
                panel.visible = false;
            }
            self.status = "Project search cancelled/hidden".into();
            return Action::None;
        }
        let results = self.project_panel.as_ref().is_some_and(|p| p.results);
        if results {
            self.validate_project_search();
            if let Some(panel) = self.project_panel.as_mut() {
                panel.view.prepare(&self.project_search, 0, 0);
            }
            match code {
                0x71 => {
                    if let Some(panel) = self.project_panel.as_mut() {
                        panel.results = false;
                    }
                }
                0x74 => self.start_project_search(true),
                0x26 | 0x28 | 0x21 | 0x22 => {
                    if let Some(panel) = self.project_panel.as_mut() {
                        panel.view.move_selection(match code {
                            0x26 => -1,
                            0x28 => 1,
                            0x21 => -10,
                            _ => 10,
                        });
                    }
                }
                0x24 => {
                    if let Some(panel) = self.project_panel.as_mut() {
                        panel.view.first();
                    }
                }
                0x23 => {
                    if let Some(panel) = self.project_panel.as_mut() {
                        panel.view.last();
                    }
                }
                0x0d => {
                    if self.project_search.is_stale() {
                        self.error("Project results are stale; F5 refresh before opening");
                        return Action::None;
                    }
                    let index = self.project_panel.as_ref().map_or(0, |p| p.view.selected());
                    match self.project_search.select(index, &mut self.workbench) {
                        Ok(()) => {
                            self.bottom.focused = false;
                            if let Some(panel) = self.project_panel.as_mut() {
                                panel.visible = false;
                            }
                            self.status =
                                "Project result opened · source coordinates revalidated".into();
                        }
                        Err(error) => self.error(error),
                    }
                }
                _ => {}
            }
            return Action::None;
        }
        if modifiers.contains(Modifiers::CTRL) && code == 0x56 {
            return Action::PasteProject(self.project_panel.as_ref().unwrap().generation);
        }
        if modifiers.contains(Modifiers::CTRL) && code == 0x55 {
            self.edit_project_field(FieldEdit::Clear);
            return Action::None;
        }
        if code == 0x0d
            && !modifiers.contains(Modifiers::SHIFT)
            && !modifiers.contains(Modifiers::ALT)
        {
            self.start_project_search(false);
            return Action::None;
        }
        if code == 0x09 {
            let panel = self.project_panel.as_mut().unwrap();
            panel.active = if modifiers.contains(Modifiers::SHIFT) {
                (panel.active + 2) % 3
            } else {
                (panel.active + 1) % 3
            };
            panel.generation = panel.generation.wrapping_add(1);
            return Action::None;
        }
        let insert = match code {
            0x71 => Some("\n"),
            0x73 => Some("\r\n"),
            0x76 => Some("\t"),
            0x77 => Some("\r"),
            0x0d => Some("\n"),
            _ => None,
        };
        if let Some(value) = insert {
            self.edit_project_field(FieldEdit::Insert(value));
            return Action::None;
        }
        let edit = match code {
            0x25 => Some(FieldEdit::Left),
            0x27 => Some(FieldEdit::Right),
            0x26 => Some(FieldEdit::Up),
            0x28 => Some(FieldEdit::Down),
            0x24 => Some(FieldEdit::Home),
            0x23 => Some(FieldEdit::End),
            0x08 => Some(FieldEdit::Backspace),
            0x2e => Some(FieldEdit::Delete),
            _ => None,
        };
        if let Some(edit) = edit {
            self.edit_project_field(edit);
        }
        Action::None
    }
    pub(super) fn project_presentation(&mut self) -> Option<ProjectPresentation> {
        self.validate_project_search();
        let panel = self.project_panel.as_mut().filter(|p| p.visible)?;
        let count = if panel.results { 1 } else { 3 };
        let (cols, rows) = super::project_paint::field_geometry(
            self.width,
            self.height,
            self.cell_width,
            self.cell_height,
            count,
        );
        let labels = [
            "Query (literal, exact EOL)",
            "Include globs (comma; empty=all)",
            "Extra excludes (comma)",
        ];
        let fields = (0..count)
            .map(|i| {
                (
                    labels[i],
                    present_field(&panel.fields[i], panel.cursors[i], cols, rows),
                )
            })
            .collect();
        let results = panel.results.then(|| {
            panel.view.prepare(
                &self.project_search,
                super::project_paint::result_capacity(
                    self.width,
                    self.height,
                    self.cell_width,
                    self.cell_height,
                ),
                cols,
            )
        });
        Some(ProjectPresentation {
            fields,
            active: if panel.results { 0 } else { panel.active },
            results,
            status: self.status.clone(),
        })
    }
}
