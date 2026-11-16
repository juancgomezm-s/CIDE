//! Per-project connection profiles and explicit record browsing.
use super::{
    command_id,
    ffi::{Hwnd, SendMessageW, WM_COMMAND},
    message_dialog::{form, sql_records},
    owner,
};
use jc_app::data_connections::{Draft, Engine, ProfileStore, MAX_VALUE_BYTES};
use jc_app::CommandId;
use jc_platform::{sql, sql_worker};
use std::path::Path;

const TITLE: &str = "Conexiones de datos";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Choice {
    Edit(usize, Engine),
    New(Engine),
    RefreshSchema,
    BrowseRecords,
    BrowseSql(usize, sql::Engine),
}

enum Outcome {
    Saved,
    Cancelled,
    RefreshSchema,
    BrowseRecords,
}

fn choices(profiles: &[Draft], has_abl_profile: bool, has_catalog: bool) -> Vec<(Choice, String)> {
    let refresh_available = has_abl_profile
        && profiles
            .iter()
            .any(|profile| profile.engine() == Engine::OpenEdge);
    profiles
        .iter()
        .enumerate()
        .map(|(index, profile)| {
            let engine = profile.engine();
            (
                Choice::Edit(index, engine),
                format!("{} · {}", profile.value(0), engine.label()),
            )
        })
        .chain(refresh_available.then(|| {
            (
                Choice::RefreshSchema,
                "OpenEdge · Actualizar esquema ABL".into(),
            )
        }))
        .chain(
            (refresh_available && has_catalog)
                .then(|| (Choice::BrowseRecords, "OpenEdge · Ver registros".into())),
        )
        .chain(profiles.iter().enumerate().filter_map(|(index, profile)| {
            let engine = match profile.engine() {
                Engine::OpenEdge => return None,
                Engine::PostgreSql => sql::Engine::PostgreSql,
                Engine::MySql => sql::Engine::MySql,
            };
            Some((
                Choice::BrowseSql(index, engine),
                format!(
                    "Consultar {} · {}",
                    profile.value(0),
                    profile.engine().label()
                ),
            ))
        }))
        .chain(Engine::ALL.into_iter().map(|engine| {
            (
                Choice::New(engine),
                format!("Nueva conexión · {}", engine.label()),
            )
        }))
        .collect()
}

fn engine_details(engine: Engine, has_abl_profile: bool) -> &'static str {
    match engine {
        Engine::OpenEdge if has_abl_profile => {
            "Usa el perfil ABL del proyecto. Con un perfil OpenEdge guardado, puedes actualizar el esquema desde la lista de conexiones."
        }
        Engine::OpenEdge => {
            "Configura .cide/progress.local.ini en este proyecto para poder actualizar el esquema ABL."
        }
        Engine::PostgreSql | Engine::MySql => {
            "Consulta el perfil guardado desde la lista de conexiones. TLS admite verificar-servidor, requerido o deshabilitado."
        }
    }
}

unsafe fn manage(hwnd: Hwnd, root: &Path, has_catalog: bool) -> Result<Outcome, String> {
    let mut store = ProfileStore::open(root)?;
    let has_abl_profile = root.join(".cide/progress.local.ini").is_file();
    let (actions, labels): (Vec<_>, Vec<_>) =
        choices(store.profiles(), has_abl_profile, has_catalog)
            .into_iter()
            .unzip();
    loop {
        let Some(selection) = form::choose(
            hwnd,
            TITLE,
            "Selecciona un perfil, el motor de una nueva conexión o una acción disponible.",
            &labels,
        )?
        else {
            return Ok(Outcome::Cancelled);
        };
        let (editing, engine) = match actions[selection] {
            Choice::Edit(index, engine) => (Some(index), engine),
            Choice::New(engine) => (None, engine),
            Choice::RefreshSchema => return Ok(Outcome::RefreshSchema),
            Choice::BrowseRecords => return Ok(Outcome::BrowseRecords),
            Choice::BrowseSql(index, engine) => {
                let profile = &store.profiles()[index];
                let options =
                    sql_worker::options(engine, std::array::from_fn(|index| profile.value(index)))?;
                let title = format!(
                    "Registros SQL · {} · {}",
                    profile.value(0),
                    profile.engine().label()
                );
                sql_records::show(hwnd, &title, options)?;
                continue;
            }
        };
        let defaults = Draft::new(engine);
        let initial = editing.map_or(&defaults, |index| &store.profiles()[index]);
        let values: Vec<_> = initial
            .fields()
            .iter()
            .enumerate()
            .map(|(index, _)| initial.value(index).to_owned())
            .collect();
        let fields: Vec<_> = defaults
            .fields()
            .iter()
            .zip(&values)
            .map(|(field, value)| form::Field {
                label: field.label(),
                value,
                limit: MAX_VALUE_BYTES,
            })
            .collect();
        let title = format!("{TITLE} · {}", engine.label());
        let mut submit = |values: &[String]| {
            let mut draft = Draft::new(engine);
            for (index, value) in values.iter().enumerate() {
                draft.set_value(index, value)?;
            }
            store.save(draft, editing)
        };
        if form::edit(
            hwnd,
            &title,
            engine_details(engine, has_abl_profile),
            &fields,
            &mut submit,
        )? {
            return Ok(Outcome::Saved);
        }
    }
}

pub(super) unsafe fn open(hwnd: Hwnd) {
    let Some(window) = owner(hwnd) else {
        return;
    };
    let (root, has_catalog) = {
        let state = window.state.borrow();
        (
            state.project_root.clone(),
            state.progress_schema.has_catalog(),
        )
    };
    let result = match root.as_deref() {
        Some(root) => manage(hwnd, root, has_catalog),
        None => Err("Abre una carpeta de proyecto para configurar conexiones de datos".into()),
    };
    let command = match result {
        Ok(Outcome::RefreshSchema) => Some(CommandId::ProgressRefreshSchema),
        Ok(Outcome::BrowseRecords) => Some(CommandId::ProgressBrowseRecords),
        _ => None,
    };
    if let Some(command) = command {
        if owner(hwnd).is_some_and(|owner| owner.state.borrow().project_root == root) {
            // The modal has closed. Revalidate through the ordinary menu route.
            SendMessageW(hwnd, WM_COMMAND, command_id(command), 0);
        }
        return;
    }
    if let Some(owner) = owner(hwnd) {
        let mut state = owner.state.borrow_mut();
        match result {
            Ok(Outcome::Saved) => state.status = "Perfil de conexión guardado".into(),
            Ok(Outcome::Cancelled | Outcome::RefreshSchema | Outcome::BrowseRecords) => {}
            Err(error) => state.error(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_connections_lists_saved_profiles_before_new_engines() {
        let mut profile = Draft::new(Engine::OpenEdge);
        profile.set_value(0, "Inventario").unwrap();
        let items = choices(&[profile], true, true);
        assert_eq!(items.len(), 3 + Engine::ALL.len());
        assert!(items[0].1.starts_with("Inventario · "));
        assert_eq!(items[0].0, Choice::Edit(0, Engine::OpenEdge));
        assert_eq!(items[1].0, Choice::RefreshSchema);
        assert_eq!(items[2].0, Choice::BrowseRecords);
        for ((action, item), engine) in items[3..].iter().zip(Engine::ALL) {
            assert_eq!(*action, Choice::New(engine));
            assert_eq!(*item, format!("Nueva conexión · {}", engine.label()));
        }
    }

    #[test]
    fn project_connections_only_offers_refresh_for_a_saved_openedge_profile_with_abl_config() {
        for (profiles, has_abl_profile) in [
            (vec![Draft::new(Engine::PostgreSql)], true),
            (vec![Draft::new(Engine::OpenEdge)], false),
            (Vec::new(), true),
        ] {
            assert!(choices(&profiles, has_abl_profile, true)
                .iter()
                .all(|(action, _)| !matches!(
                    action,
                    Choice::RefreshSchema | Choice::BrowseRecords
                )));
        }
        let profile = Draft::new(Engine::OpenEdge);
        assert!(choices(&[profile], true, false)
            .iter()
            .all(|(action, _)| *action != Choice::BrowseRecords));
    }

    #[test]
    fn project_connections_offers_a_separate_sql_query_for_each_saved_profile() {
        let mut postgres = Draft::new(Engine::PostgreSql);
        postgres.set_value(0, "Inventario").unwrap();
        let mut mysql = Draft::new(Engine::MySql);
        mysql.set_value(0, "Ventas").unwrap();
        let items = choices(&[postgres, mysql], false, false);
        assert_eq!(items[0].0, Choice::Edit(0, Engine::PostgreSql));
        assert_eq!(items[1].0, Choice::Edit(1, Engine::MySql));
        assert_eq!(
            items[2],
            (
                Choice::BrowseSql(0, sql::Engine::PostgreSql),
                "Consultar Inventario · PostgreSQL".into()
            ),
        );
        assert_eq!(
            items[3],
            (
                Choice::BrowseSql(1, sql::Engine::MySql),
                "Consultar Ventas · MySQL".into()
            ),
        );
    }
}
