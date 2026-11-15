//! Per-project connection profiles. Opening or saving a profile never opens a
//! database session. OpenEdge schema refresh stays on its existing ABL route.
use super::{
    command_id,
    ffi::{Hwnd, SendMessageW, WM_COMMAND},
    message_dialog::form,
    owner,
};
use jc_app::data_connections::{Draft, Engine, ProfileStore, MAX_VALUE_BYTES};
use jc_app::CommandId;
use std::path::Path;

const TITLE: &str = "Conexiones de datos";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Choice {
    Edit(usize, Engine),
    New(Engine),
    RefreshSchema,
}

enum Outcome {
    Saved,
    Cancelled,
    RefreshSchema,
}

fn choices(profiles: &[Draft], has_abl_profile: bool) -> Vec<(Choice, String)> {
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
            "Configuración local. El proveedor aún no permite probar la conexión ni consultar tablas. TLS admite verificar-servidor, requerido o deshabilitado."
        }
    }
}

unsafe fn manage(hwnd: Hwnd, root: &Path) -> Result<Outcome, String> {
    let mut store = ProfileStore::open(root)?;
    let has_abl_profile = root.join(".cide/progress.local.ini").is_file();
    let (actions, labels): (Vec<_>, Vec<_>) = choices(store.profiles(), has_abl_profile)
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
    let root = owner(hwnd).and_then(|owner| owner.state.borrow().project_root.clone());
    let result = match root.as_deref() {
        Some(root) => manage(hwnd, root),
        None => Err("Abre una carpeta de proyecto para configurar conexiones de datos".into()),
    };
    if matches!(result, Ok(Outcome::RefreshSchema)) {
        if owner(hwnd).is_some_and(|owner| owner.state.borrow().project_root == root) {
            // The modal has closed. Use the same worker and command path as
            // Herramientas → Actualizar esquema de base de datos.
            SendMessageW(
                hwnd,
                WM_COMMAND,
                command_id(CommandId::ProgressRefreshSchema),
                0,
            );
        }
        return;
    }
    if let Some(owner) = owner(hwnd) {
        let mut state = owner.state.borrow_mut();
        match result {
            Ok(Outcome::Saved) => state.status = "Perfil de conexión guardado".into(),
            Ok(Outcome::Cancelled | Outcome::RefreshSchema) => {}
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
        let items = choices(&[profile], true);
        assert_eq!(items.len(), 2 + Engine::ALL.len());
        assert!(items[0].1.starts_with("Inventario · "));
        assert_eq!(items[0].0, Choice::Edit(0, Engine::OpenEdge));
        assert_eq!(items[1].0, Choice::RefreshSchema);
        for ((action, item), engine) in items[2..].iter().zip(Engine::ALL) {
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
            assert!(choices(&profiles, has_abl_profile)
                .iter()
                .all(|(action, _)| *action != Choice::RefreshSchema));
        }
    }
}
