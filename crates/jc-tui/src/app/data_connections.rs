use super::TuiApp;
use crate::{input::is_plain_text, state::UiOverlay};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use jc_app::{
    data_connections::{Draft, Engine, ProfileStore, MAX_VALUE_BYTES},
    edit_field, CommandId, FieldEdit, Overlay,
};
use std::path::PathBuf;

pub(super) const PROJECT_REQUIRED: &str =
    "Abre una carpeta de proyecto para configurar conexiones de datos";

pub(crate) struct ConnectionsDialog {
    root: PathBuf,
    has_progress_profile: bool,
    pub(crate) store: ProfileStore,
    pub(crate) stage: Stage,
    profile_row: usize,
}

pub(crate) enum Stage {
    Profiles,
    Engines { selected: usize },
    Form(Form),
}

pub(crate) struct Form {
    pub(crate) draft: Draft,
    editing: Option<usize>,
    pub(crate) field: usize,
    pub(crate) cursor: usize,
    pub(crate) error: Option<String>,
}

impl ConnectionsDialog {
    pub(crate) fn profile_row(&self) -> usize {
        self.profile_row
    }
}

impl Form {
    fn new(draft: Draft, editing: Option<usize>) -> Self {
        let cursor = draft.value(0).len();
        Self {
            draft,
            editing,
            field: 0,
            cursor,
            error: None,
        }
    }
}

impl TuiApp {
    pub(super) fn open_data_connections(&mut self) {
        let Some(root) = self.project_root.clone() else {
            self.status = PROJECT_REQUIRED.into();
            return;
        };
        let store = match ProfileStore::open(&root) {
            Ok(store) => store,
            Err(error) => {
                self.status = error;
                return;
            }
        };
        self.open_focus_overlay(Overlay::Dialog);
        self.overlay = UiOverlay::DataConnections;
        self.data_connections = Some(ConnectionsDialog {
            has_progress_profile: root.join(".cide/progress.local.ini").is_file(),
            root,
            store,
            stage: Stage::Profiles,
            profile_row: 0,
        });
        self.status = "Conexiones de datos del proyecto".into();
    }

    pub(super) fn handle_data_connections_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::F(5) && self.can_refresh_connection_schema() {
            self.close_overlay();
            self.dispatch(CommandId::ProgressRefreshSchema);
            return;
        }
        if key.code == KeyCode::F(6) && self.can_browse_connection_records() {
            self.close_overlay();
            self.dispatch(CommandId::ProgressBrowseRecords);
            return;
        }
        let Some(dialog) = self.data_connections.as_mut() else {
            return;
        };
        if self.project_root.as_ref() != Some(&dialog.root) {
            self.close_overlay();
            self.status =
                "Cambió la carpeta del proyecto; vuelve a abrir Conexiones de datos".into();
            return;
        }
        if key.code == KeyCode::Esc {
            if matches!(dialog.stage, Stage::Profiles) {
                self.close_overlay();
            } else {
                dialog.stage = Stage::Profiles;
            }
            self.status = "Conexiones de datos · edición cerrada".into();
            return;
        }

        match &mut dialog.stage {
            Stage::Profiles => {
                move_row(
                    &mut dialog.profile_row,
                    dialog.store.profiles().len(),
                    key.code,
                );
                if key.code == KeyCode::Enter {
                    dialog.stage = match dialog.profile_row.checked_sub(1) {
                        None => Stage::Engines { selected: 0 },
                        Some(index) => {
                            let draft = dialog.store.profiles()[index].clone();
                            Stage::Form(Form::new(draft, Some(index)))
                        }
                    };
                    self.status = "Conexiones de datos del proyecto".into();
                }
            }
            Stage::Engines { selected } => {
                move_row(selected, Engine::ALL.len() - 1, key.code);
                if key.code == KeyCode::Enter {
                    dialog.stage = Stage::Form(Form::new(Draft::new(Engine::ALL[*selected]), None));
                }
            }
            Stage::Form(form) => {
                if key.code == KeyCode::F(9)
                    || key.code == KeyCode::Enter && key.modifiers == KeyModifiers::CONTROL
                {
                    let name = form.draft.value(0).trim().to_owned();
                    match dialog.store.save(form.draft.clone(), form.editing) {
                        Ok(()) => {
                            self.close_overlay();
                            self.status = format!("Conexión «{name}» guardada en el proyecto");
                        }
                        Err(error) => form.error = Some(error),
                    }
                    return;
                }
                let field = match key.code {
                    KeyCode::Up | KeyCode::BackTab => form.field.saturating_sub(1),
                    KeyCode::Down | KeyCode::Tab | KeyCode::Enter => {
                        (form.field + 1).min(form.draft.fields().len() - 1)
                    }
                    _ => form.field,
                };
                if field != form.field {
                    form.field = field;
                    form.cursor = form.draft.value(field).len();
                    return;
                }
                let edit = match key.code {
                    KeyCode::Left => FieldEdit::Left,
                    KeyCode::Right => FieldEdit::Right,
                    KeyCode::Home => FieldEdit::Home,
                    KeyCode::End => FieldEdit::End,
                    KeyCode::Backspace => FieldEdit::Backspace,
                    KeyCode::Delete => FieldEdit::Delete,
                    KeyCode::Char('u') if key.modifiers == KeyModifiers::CONTROL => {
                        FieldEdit::Clear
                    }
                    KeyCode::Char(ch) if is_plain_text(key) => {
                        self.edit_data_connection(FieldEdit::Insert(&ch.to_string()));
                        return;
                    }
                    _ => return,
                };
                self.edit_data_connection(edit);
            }
        }
    }

    pub(crate) fn can_refresh_connection_schema(&self) -> bool {
        self.openedge_profile_selected()
            && self
                .registry
                .is_enabled(CommandId::ProgressRefreshSchema, &self.command_context())
    }

    pub(crate) fn can_browse_connection_records(&self) -> bool {
        self.openedge_profile_selected()
            && self
                .registry
                .is_enabled(CommandId::ProgressBrowseRecords, &self.command_context())
    }

    fn openedge_profile_selected(&self) -> bool {
        let Some(dialog) = &self.data_connections else {
            return false;
        };
        dialog.has_progress_profile
            && self.project_root.as_ref() == Some(&dialog.root)
            && matches!(dialog.stage, Stage::Profiles)
            && dialog
                .profile_row
                .checked_sub(1)
                .is_some_and(|index| dialog.store.profiles()[index].engine() == Engine::OpenEdge)
    }

    pub(super) fn data_connection_paste_budget(&self) -> Option<usize> {
        let Stage::Form(form) = &self.data_connections.as_ref()?.stage else {
            return None;
        };
        Some(
            MAX_VALUE_BYTES
                .saturating_sub(form.draft.value(form.field).len())
                .max(1),
        )
    }

    pub(super) fn edit_data_connection(&mut self, edit: FieldEdit<'_>) {
        let Some(ConnectionsDialog {
            stage: Stage::Form(form),
            ..
        }) = &mut self.data_connections
        else {
            return;
        };
        if matches!(edit, FieldEdit::Insert(text) if text.chars().any(char::is_control)) {
            form.error = Some("Usa una sola línea por campo".into());
            return;
        }
        let mut value = form.draft.value(form.field).to_owned();
        let mut cursor = form.cursor;
        let result = edit_field(&mut value, &mut cursor, edit, MAX_VALUE_BYTES)
            .map_err(str::to_owned)
            .and_then(|changed| {
                if changed {
                    form.draft.set_value(form.field, &value)?;
                }
                Ok(changed)
            });
        match result {
            Ok(changed) => {
                form.cursor = cursor;
                if changed {
                    form.error = None;
                }
            }
            Err(error) => form.error = Some(error),
        }
    }
}

fn move_row(row: &mut usize, last: usize, key: KeyCode) {
    *row = match key {
        KeyCode::Up => row.saturating_sub(1),
        KeyCode::Down => (*row + 1).min(last),
        KeyCode::Home => 0,
        KeyCode::End => last,
        _ => *row,
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::LaunchOptions;
    use crossterm::event::Event;
    use jc_app::{CommandId, MenuId};
    use std::{
        fs, thread,
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };

    struct Project(PathBuf);

    impl Project {
        fn new() -> Self {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "cide-tui-connections-{}-{stamp}",
                std::process::id()
            ));
            fs::create_dir(&root).unwrap();
            Self(root)
        }
    }

    impl Drop for Project {
        fn drop(&mut self) {
            let _ = fs::remove_file(self.0.join(".cide/conexiones.local.json"));
            let _ = fs::remove_file(self.0.join(".cide/progress.local.ini"));
            let _ = fs::remove_dir(self.0.join(".cide"));
            let _ = fs::remove_dir(&self.0);
        }
    }

    fn key(app: &mut TuiApp, code: KeyCode) {
        app.handle_event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
    }

    fn screen_contains(app: &mut TuiApp, label: &str) -> bool {
        let screen = app.frame(84, 24);
        (0..screen.height()).any(|row| screen.row_text(row).contains(label))
    }

    #[test]
    fn project_connections_keyboard_creates_edits_and_cancels() {
        let project = Project::new();
        let mut app = TuiApp::new(LaunchOptions::default());
        app.dispatch(CommandId::ProjectDataConnections);
        assert_eq!(app.status, PROJECT_REQUIRED);
        assert_eq!(app.overlay, UiOverlay::None);

        app.set_explorer_root(project.0.clone()).unwrap();
        app.dispatch(CommandId::MenuProject);
        let entries = app.menu_commands(MenuId::Project);
        for _ in 0..entries.len() {
            let UiOverlay::Menu { selected, .. } = app.overlay else {
                break;
            };
            if entries[selected] == Some(CommandId::ProjectDataConnections) {
                key(&mut app, KeyCode::Enter);
                break;
            }
            key(&mut app, KeyCode::Down);
        }
        assert!(screen_contains(&mut app, "Conexiones de datos"));
        let _ = app.frame(10, 7);
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Down);
        assert!(screen_contains(&mut app, "proveedor pendiente"));
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::F(9));
        assert!(screen_contains(&mut app, "Completa el campo «Nombre»"));
        app.handle_event(Event::Paste("Datos ñ".into()));
        for _ in 0..3 {
            key(&mut app, KeyCode::Tab);
        }
        app.handle_event(Event::Paste("inventario".into()));
        key(&mut app, KeyCode::Tab);
        app.handle_event(Event::Paste("lectura".into()));
        key(&mut app, KeyCode::F(9));
        assert_eq!(app.overlay, UiOverlay::None, "{}", app.status);
        let store = ProfileStore::open(&project.0).unwrap();
        assert_eq!(store.profiles().len(), 1);
        assert_eq!(store.profiles()[0].value(0), "Datos ñ");
        assert_eq!(store.profiles()[0].value(3), "inventario");

        app.dispatch(CommandId::ProjectDataConnections);
        key(&mut app, KeyCode::Down);
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Backspace);
        key(&mut app, KeyCode::Esc);
        key(&mut app, KeyCode::Enter);
        app.handle_event(Event::Paste(" de prueba".into()));
        key(&mut app, KeyCode::F(9));
        assert_eq!(app.overlay, UiOverlay::None, "{}", app.status);
        let store = ProfileStore::open(&project.0).unwrap();
        assert_eq!(store.profiles().len(), 1);
        assert_eq!(store.profiles()[0].value(0), "Datos ñ de prueba");
        assert_eq!(store.profiles()[0].engine(), Engine::PostgreSql);
    }

    #[test]
    fn project_connections_openedge_refresh_uses_existing_schema_provider() {
        let project = Project::new();
        let mut store = ProfileStore::open(&project.0).unwrap();
        let mut draft = Draft::new(Engine::OpenEdge);
        draft.set_value(0, "Laboratorio").unwrap();
        store.save(draft, None).unwrap();
        let mut app = TuiApp::new(LaunchOptions::default());
        app.set_explorer_root(project.0.clone()).unwrap();
        app.dispatch(CommandId::ProjectDataConnections);
        key(&mut app, KeyCode::Down);
        assert!(!screen_contains(&mut app, "F5 actualizar esquema"));
        key(&mut app, KeyCode::F(5));
        assert!(!app.progress_schema.busy());
        key(&mut app, KeyCode::Esc);

        fs::write(
            project.0.join(".cide/progress.local.ini"),
            "# El ejecutable no está configurado\n",
        )
        .unwrap();
        app.dispatch(CommandId::ProjectDataConnections);
        key(&mut app, KeyCode::Down);
        assert!(screen_contains(&mut app, "F5 actualizar esquema"));
        key(&mut app, KeyCode::F(5));
        assert_eq!(app.overlay, UiOverlay::None);
        assert!(app.progress_schema.busy());
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.progress_schema.busy() {
            app.poll_tasks();
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        assert!(app.workbench.problems().iter().any(|problem| {
            problem
                .text
                .contains("Falta executable en .cide/progress.local.ini")
        }));
    }
}
