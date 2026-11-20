//! Per-project connection profiles and explicit record browsing.
use super::{
    command_id,
    ffi::{Hwnd, SendMessageW, WM_COMMAND},
    message_dialog::{form, sql_records},
    owner,
};
use jc_app::data_connections::{
    ConnectionTest, Draft, Engine, ProfileStore, MAX_VALUE_BYTES, SQL_TLS_FIELD, SQL_TLS_MODES,
};
use jc_app::CommandId;
use jc_platform::{sql, sql_worker};
use jc_progress::editor::{OpenEdgeTest, ProfileSetup, OPENEDGE_FIELDS, OPENEDGE_VALUE_BYTES};
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
    SavedOpenEdge,
    Cancelled,
    RefreshSchema,
    BrowseRecords,
}

fn choices(profiles: &[Draft], has_abl_profile: bool, has_catalog: bool) -> Vec<(Choice, String)> {
    let refresh_available = has_abl_profile;
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
                if engine == Engine::OpenEdge {
                    "Asistente OpenEdge".into()
                } else {
                    format!("Nueva conexión · {}", engine.label())
                },
            )
        }))
        .collect()
}

unsafe fn openedge_setup(hwnd: Hwnd, root: &Path) -> Result<bool, String> {
    let mut setup = ProfileSetup::open(root)?;
    let values: Vec<_> = (0..OPENEDGE_FIELDS.len())
        .map(|index| setup.value(index).to_owned())
        .collect();
    let fields: Vec<_> = OPENEDGE_FIELDS
        .iter()
        .zip(&values)
        .map(|((label, _), value)| form::Field {
            label,
            value,
            limit: OPENEDGE_VALUE_BYTES,
        })
        .collect();
    form::edit_config(hwnd, "Asistente OpenEdge", "Elige el ejecutable del AVM. Para conectar una base, usa un .pf existente o indica una copia local .db para generar uno de solo lectura.", "Guardar perfil", &fields, &mut |values| {
        for (index, value) in values.iter().enumerate() { setup.set_value(index, value)?; }
        setup.save()
    }, form::ConfigTest { paths: Some((root, &[0, 3, 4, 7])), start: &|values| {
        let probe = OpenEdgeTest::new(root, values)?;
        ConnectionTest::start(move |cancel| probe.run(cancel), || {})
    } })
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
        if engine == Engine::OpenEdge {
            if openedge_setup(hwnd, root)? {
                return Ok(Outcome::SavedOpenEdge);
            }
            continue;
        }
        let defaults = Draft::new(engine);
        let initial = editing.map_or(&defaults, |index| &store.profiles()[index]);
        let mut tls = SQL_TLS_MODES;
        if let Some(current) = tls
            .iter()
            .position(|(value, _)| *value == initial.value(SQL_TLS_FIELD))
        {
            tls.swap(0, current);
        }
        let modes = tls.map(|(_, label)| label.to_owned());
        let title = format!("Asistente {} · 1 de 2", engine.label());
        let Some(selected) = form::choose(
            hwnd,
            &title,
            "Elige cómo proteger la conexión con el servidor SQL.",
            &modes,
        )?
        else {
            continue;
        };
        let indices: Vec<_> = (0..initial.fields().len())
            .filter(|index| *index != SQL_TLS_FIELD)
            .collect();
        let values: Vec<_> = indices
            .iter()
            .map(|&index| initial.value(index).to_owned())
            .collect();
        let fields: Vec<_> = indices
            .iter()
            .zip(&values)
            .map(|(&index, value)| form::Field {
                label: initial.fields()[index].label(),
                value,
                limit: MAX_VALUE_BYTES,
            })
            .collect();
        let title = format!("Asistente {} · 2 de 2", engine.label());
        let draft_from = |values: &[String]| {
            let mut draft = Draft::new(engine);
            draft.set_value(SQL_TLS_FIELD, tls[selected].0)?;
            for (&index, value) in indices.iter().zip(values) {
                draft.set_value(index, value)?;
            }
            Ok::<_, String>(draft)
        };
        let sql_engine = match engine {
            Engine::PostgreSql => sql::Engine::PostgreSql,
            Engine::MySql => sql::Engine::MySql,
            Engine::OpenEdge => unreachable!(),
        };
        if form::edit_config(
            hwnd,
            &title,
            "Completa los datos del servidor. Si necesita contraseña, indica el nombre cide/datadigger/… del Administrador de credenciales de Windows.",
            "Guardar",
            &fields,
            &mut |values| store.save(draft_from(values)?, editing),
            form::ConfigTest { paths: None, start: &|values| {
                let draft = draft_from(values)?;
                let options = sql_worker::options(sql_engine, std::array::from_fn(|index| draft.value(index)))?;
                ConnectionTest::start(move |cancel| sql_worker::test_connection(&options, || cancel.is_cancelled()), || {})
            } },
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
            Ok(Outcome::SavedOpenEdge) if state.project_root == root => {
                state.progress_schema.clear();
                state.status =
                    "Perfil OpenEdge guardado. Ya puedes actualizar el esquema ABL.".into();
            }
            Ok(
                Outcome::Cancelled
                | Outcome::RefreshSchema
                | Outcome::BrowseRecords
                | Outcome::SavedOpenEdge,
            ) => {}
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
            assert_eq!(
                *item,
                if engine == Engine::OpenEdge {
                    "Asistente OpenEdge".into()
                } else {
                    format!("Nueva conexión · {}", engine.label())
                }
            );
        }
    }

    #[test]
    fn project_connections_offers_refresh_for_the_abl_config_without_a_legacy_profile() {
        for profiles in [vec![Draft::new(Engine::OpenEdge)], Vec::new()] {
            assert!(choices(&profiles, false, true)
                .iter()
                .all(|(action, _)| !matches!(
                    action,
                    Choice::RefreshSchema | Choice::BrowseRecords
                )));
        }
        assert!(choices(&[], true, false)
            .iter()
            .any(|(action, _)| *action == Choice::RefreshSchema));
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
