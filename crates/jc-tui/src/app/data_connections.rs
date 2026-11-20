use super::TuiApp;
use crate::{input::is_plain_text, state::UiOverlay};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use jc_app::{
    data_connections::{
        ConnectionTest, Draft, Engine, ProfileStore, MAX_VALUE_BYTES, SQL_TLS_FIELD, SQL_TLS_MODES,
    },
    edit_field, CommandId, FieldEdit, Overlay,
};
use jc_progress::editor::{ProfileSetup, OPENEDGE_FIELDS, OPENEDGE_VALUE_BYTES};
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
    pub(crate) draft: SetupDraft,
    editing: Option<usize>,
    pub(crate) field: usize,
    pub(crate) cursor: usize,
    pub(crate) error: Option<String>,
    pub(crate) test_result: Option<String>,
    pub(crate) probe: Option<ConnectionTest>,
}

pub(crate) enum SetupDraft {
    Sql(Draft),
    OpenEdge(Box<ProfileSetup>),
}

impl SetupDraft {
    fn open(
        root: &std::path::Path,
        engine: Engine,
        existing: Option<&Draft>,
    ) -> Result<Self, String> {
        if engine == Engine::OpenEdge {
            ProfileSetup::open(root).map(Box::new).map(Self::OpenEdge)
        } else {
            Ok(Self::Sql(
                existing.cloned().unwrap_or_else(|| Draft::new(engine)),
            ))
        }
    }

    pub(crate) fn engine(&self) -> Engine {
        match self {
            Self::Sql(draft) => draft.engine(),
            Self::OpenEdge(_) => Engine::OpenEdge,
        }
    }

    pub(crate) fn len(&self) -> usize {
        match self {
            Self::Sql(draft) => draft.fields().len(),
            Self::OpenEdge(_) => OPENEDGE_FIELDS.len(),
        }
    }

    pub(crate) fn label(&self, index: usize) -> &str {
        match self {
            Self::Sql(draft) => draft.fields()[index].label(),
            Self::OpenEdge(_) => OPENEDGE_FIELDS[index].0,
        }
    }

    pub(crate) fn guidance(&self, index: usize) -> &str {
        match self {
            Self::Sql(draft) => draft.fields()[index].guidance(),
            Self::OpenEdge(_) => OPENEDGE_FIELDS[index].1,
        }
    }

    pub(crate) fn value(&self, index: usize) -> &str {
        match self {
            Self::Sql(draft) => draft.value(index),
            Self::OpenEdge(draft) => draft.value(index),
        }
    }

    fn set_value(&mut self, index: usize, value: &str) -> Result<(), String> {
        match self {
            Self::Sql(draft) => draft.set_value(index, value),
            Self::OpenEdge(draft) => draft.set_value(index, value),
        }
    }

    fn limit(&self) -> usize {
        match self {
            Self::Sql(_) => MAX_VALUE_BYTES,
            Self::OpenEdge(_) => OPENEDGE_VALUE_BYTES,
        }
    }

    fn test(&self, wakeup: jc_app::Wakeup) -> Result<ConnectionTest, String> {
        match self {
            Self::OpenEdge(setup) => {
                let probe = setup.test_request();
                ConnectionTest::start(move |cancel| probe.run(cancel), move || wakeup.signal())
            }
            #[cfg(windows)]
            Self::Sql(draft) => {
                use jc_platform::{sql, sql_worker};
                let engine = if draft.engine() == Engine::PostgreSql {
                    sql::Engine::PostgreSql
                } else {
                    sql::Engine::MySql
                };
                let options =
                    sql_worker::options(engine, std::array::from_fn(|index| draft.value(index)))?;
                ConnectionTest::start(
                    move |cancel| sql_worker::test_connection(&options, || cancel.is_cancelled()),
                    move || wakeup.signal(),
                )
            }
            #[cfg(not(windows))]
            Self::Sql(_) => Err("La prueba SQL por ODBC está disponible en Windows.".into()),
        }
    }
}

impl ConnectionsDialog {
    pub(crate) fn profile_row(&self) -> usize {
        self.profile_row
    }

    pub(crate) fn virtual_openedge(&self) -> bool {
        self.has_progress_profile
            && !self
                .store
                .profiles()
                .iter()
                .any(|draft| draft.engine() == Engine::OpenEdge)
    }

    fn profile_at(&self, row: usize) -> Option<&Draft> {
        self.store.profiles().get(row.checked_sub(1)?)
    }

    fn open_form(&mut self, engine: Engine, editing: Option<usize>) -> Result<(), String> {
        let existing = editing.and_then(|index| self.store.profiles().get(index));
        self.stage = Stage::Form(Form::new(
            SetupDraft::open(&self.root, engine, existing)?,
            editing,
        ));
        Ok(())
    }
}

impl Form {
    fn new(draft: SetupDraft, editing: Option<usize>) -> Self {
        let cursor = draft.value(0).len();
        Self {
            draft,
            editing,
            field: 0,
            cursor,
            error: None,
            test_result: None,
            probe: None,
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
        #[cfg(windows)]
        if key.code == KeyCode::F(6) {
            if let Some(profile) = self.selected_sql_connection().cloned() {
                self.open_sql_browser(&profile);
                return;
            }
        }
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
                let last = dialog.store.profiles().len() + usize::from(dialog.virtual_openedge());
                move_row(&mut dialog.profile_row, last, key.code);
                if key.code == KeyCode::Enter {
                    if dialog.profile_row == 0 {
                        dialog.stage = Stage::Engines { selected: 0 };
                        self.status = "Elige el motor de la conexión".into();
                    } else {
                        let engine = dialog
                            .profile_at(dialog.profile_row)
                            .map_or(Engine::OpenEdge, Draft::engine);
                        if let Err(error) =
                            dialog.open_form(engine, dialog.profile_row.checked_sub(1))
                        {
                            self.status = error;
                        }
                    }
                }
            }
            Stage::Engines { selected } => {
                move_row(selected, Engine::ALL.len() - 1, key.code);
                if key.code == KeyCode::Enter {
                    let engine = Engine::ALL[*selected];
                    if let Err(error) = dialog.open_form(engine, None) {
                        self.status = error;
                    }
                }
            }
            Stage::Form(form) => {
                if form.probe.is_some() {
                    return;
                }
                if key.code == KeyCode::F(8) {
                    form.error = None;
                    match form.draft.test(self.syntax_wakeup.clone()) {
                        Ok(probe) => {
                            form.probe = Some(probe);
                            form.test_result = Some("Probando conexión…".into());
                        }
                        Err(error) => {
                            form.test_result =
                                Some(format!("No se pudo probar la conexión. {error}"))
                        }
                    }
                    return;
                }
                if key.code == KeyCode::F(9)
                    || key.code == KeyCode::Enter && key.modifiers == KeyModifiers::CONTROL
                {
                    let result = match &mut form.draft {
                        SetupDraft::Sql(draft) => {
                            dialog.store.save(draft.clone(), form.editing).map(|()| {
                                format!(
                                    "Conexión «{}» guardada en el proyecto",
                                    draft.value(0).trim()
                                )
                            })
                        }
                        SetupDraft::OpenEdge(draft) => draft.save().map(|()| {
                            "Perfil OpenEdge guardado · F5 en Conexiones actualiza el esquema"
                                .into()
                        }),
                    };
                    match result {
                        Ok(message) => {
                            if form.draft.engine() == Engine::OpenEdge {
                                self.progress_schema.clear();
                            }
                            self.close_overlay();
                            self.status = message;
                        }
                        Err(error) => form.error = Some(error),
                    }
                    return;
                }
                if key.code == KeyCode::F(4)
                    && form.field == SQL_TLS_FIELD
                    && form.draft.engine() != Engine::OpenEdge
                {
                    let next = SQL_TLS_MODES
                        .iter()
                        .position(|(value, _)| *value == form.draft.value(form.field))
                        .map_or(0, |index| (index + 1) % SQL_TLS_MODES.len());
                    let value = SQL_TLS_MODES[next].0;
                    form.error = form.draft.set_value(form.field, value).err();
                    form.test_result = None;
                    form.cursor = value.len();
                    return;
                }
                let field = match key.code {
                    KeyCode::Up | KeyCode::BackTab => form.field.saturating_sub(1),
                    KeyCode::Down | KeyCode::Tab | KeyCode::Enter => {
                        (form.field + 1).min(form.draft.len() - 1)
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

    #[cfg(windows)]
    pub(crate) fn selected_sql_connection(&self) -> Option<&Draft> {
        let dialog = self.data_connections.as_ref()?;
        if self.project_root.as_ref() != Some(&dialog.root)
            || !matches!(dialog.stage, Stage::Profiles)
        {
            return None;
        }
        dialog
            .store
            .profiles()
            .get(dialog.profile_row.checked_sub(1)?)
            .filter(|profile| profile.engine() != Engine::OpenEdge)
    }

    fn openedge_profile_selected(&self) -> bool {
        let Some(dialog) = &self.data_connections else {
            return false;
        };
        dialog.has_progress_profile
            && self.project_root.as_ref() == Some(&dialog.root)
            && matches!(dialog.stage, Stage::Profiles)
            && (dialog
                .profile_at(dialog.profile_row)
                .is_some_and(|draft| draft.engine() == Engine::OpenEdge)
                || (dialog.virtual_openedge()
                    && dialog.profile_row == dialog.store.profiles().len() + 1))
    }

    pub(super) fn data_connection_paste_budget(&self) -> Option<usize> {
        let Stage::Form(form) = &self.data_connections.as_ref()?.stage else {
            return None;
        };
        if form.probe.is_some() {
            return None;
        }
        Some(
            form.draft
                .limit()
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
        if form.probe.is_some() {
            return;
        }
        if matches!(edit, FieldEdit::Insert(text) if text.chars().any(char::is_control)) {
            form.error = Some("Usa una sola línea por campo".into());
            return;
        }
        let mut value = form.draft.value(form.field).to_owned();
        let mut cursor = form.cursor;
        let result = edit_field(&mut value, &mut cursor, edit, form.draft.limit())
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
                    form.test_result = None;
                }
            }
            Err(error) => form.error = Some(error),
        }
    }

    pub(super) fn poll_data_connection_test(&mut self) -> bool {
        let Some(ConnectionsDialog {
            stage: Stage::Form(form),
            root,
            ..
        }) = &mut self.data_connections
        else {
            return false;
        };
        if self.project_root.as_ref() != Some(root) {
            return false;
        }
        let Some(result) = form.probe.as_ref().and_then(ConnectionTest::poll) else {
            return false;
        };
        form.probe = None;
        form.test_result =
            Some(result.unwrap_or_else(|error| format!("Falló la conexión. {error}")));
        true
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
            let _ = fs::remove_file(self.0.join("avm.exe"));
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

    #[cfg(windows)]
    #[test]
    fn project_connections_sql_opens_saved_profile_and_keeps_editor_on_error() {
        use jc_text::TextEncoding;

        let project = Project::new();
        let mut draft = Draft::new(Engine::PostgreSql);
        for (field, value) in [
            (0, "Demo SQL"),
            (1, "127.0.0.1"),
            (2, "1"),
            (3, "demo"),
            (4, "lector"),
        ] {
            draft.set_value(field, value).unwrap();
        }
        ProfileStore::open(&project.0)
            .unwrap()
            .save(draft, None)
            .unwrap();

        let mut app = TuiApp::new(LaunchOptions::default());
        app.set_explorer_root(project.0.clone()).unwrap();
        app.workbench.new_file("editor.txt", TextEncoding::Utf8);
        app.workbench.insert_text("Documento en edición").unwrap();
        let original = app.workbench.active_bytes().unwrap();
        app.dispatch(CommandId::ProjectDataConnections);
        key(&mut app, KeyCode::Down);
        assert!(screen_contains(&mut app, "F6 conectar y explorar SQL"));
        key(&mut app, KeyCode::F(6));
        assert_eq!(app.overlay, UiOverlay::SqlRecords);
        assert!(screen_contains(&mut app, "Registros SQL"));
        let deadline = Instant::now() + Duration::from_secs(20);
        while app.sql_browser.as_ref().unwrap().busy() {
            assert!(Instant::now() < deadline);
            app.poll_tasks();
            thread::sleep(Duration::from_millis(15));
        }
        let browser = app.sql_browser.as_ref().unwrap();
        assert!(browser.tables.is_empty());
        assert!(
            browser.message.contains("ODBC") || browser.message.contains("conectar"),
            "{}",
            browser.message
        );
        key(&mut app, KeyCode::Esc);
        assert_eq!(app.overlay, UiOverlay::None);
        assert!(app.sql_browser.is_none());
        assert_eq!(app.workbench.active_bytes().unwrap(), original);
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
        assert!(screen_contains(
            &mut app,
            if cfg!(windows) {
                "SQL de solo lectura"
            } else {
                "Perfiles SQL"
            }
        ));
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

    #[test]
    fn project_connections_wizard_saves_openedge_and_lists_it_without_a_json_profile() {
        let project = Project::new();
        fs::write(project.0.join("avm.exe"), "fixture").unwrap();
        let mut app = TuiApp::new(LaunchOptions::default());
        app.set_explorer_root(project.0.clone()).unwrap();
        app.dispatch(CommandId::ProjectDataConnections);
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Enter);
        assert!(screen_contains(&mut app, "Ejecutable AVM"));
        assert!(screen_contains(&mut app, "F8 probar conexión"));
        app.handle_event(Event::Key(KeyEvent::new(
            KeyCode::Char('u'),
            KeyModifiers::CONTROL,
        )));
        key(&mut app, KeyCode::F(8));
        let deadline = Instant::now() + Duration::from_secs(3);
        while !screen_contains(&mut app, "Falló la conexión") {
            assert!(Instant::now() < deadline);
            app.poll_tasks();
            thread::sleep(Duration::from_millis(10));
        }
        assert!(!project.0.join(".cide").exists());
        app.handle_event(Event::Paste("avm.exe".into()));
        assert!(!screen_contains(&mut app, "Falló la conexión"));
        key(&mut app, KeyCode::F(9));
        assert_eq!(app.overlay, UiOverlay::None, "{}", app.status);
        let ini = fs::read_to_string(project.0.join(".cide/progress.local.ini")).unwrap();
        assert!(ini.contains("executable=avm.exe\n"));
        assert!(!project.0.join(".cide/conexiones.local.json").exists());
        app.dispatch(CommandId::ProjectDataConnections);
        key(&mut app, KeyCode::Down);
        assert!(screen_contains(&mut app, "OpenEdge · Perfil del proyecto"));
        assert!(screen_contains(&mut app, "F5 actualizar esquema"));
    }
}
