//! Opens records for a table in the accepted project schema.
use super::{message_dialog, owner, Hwnd};

pub(super) unsafe fn open(hwnd: Hwnd) {
    let Some(window) = owner(hwnd) else {
        return;
    };
    let (root, tables) = {
        let state = window.state.borrow();
        (
            state.project_root.clone(),
            state.progress_schema.browse_tables(),
        )
    };
    let Some(root) = root else {
        return;
    };
    let result = if tables.is_empty() {
        Err("El esquema ABL no contiene tablas para consultar".into())
    } else {
        let labels = tables
            .iter()
            .map(|table| format!("{}.{}", table.database, table.name))
            .collect::<Vec<_>>();
        match message_dialog::form::choose(
            hwnd,
            "Ver registros",
            "Selecciona una tabla del esquema ABL.",
            &labels,
        ) {
            Ok(Some(index)) => {
                if owner(hwnd).is_some_and(|window| {
                    let state = window.state.borrow();
                    state.project_root.as_ref() == Some(&root) && !state.progress_schema.busy()
                }) {
                    message_dialog::records::show(hwnd, &root, &tables[index])
                } else {
                    Ok(())
                }
            }
            Ok(None) => Ok(()),
            Err(error) => Err(error),
        }
    };
    if let Err(error) = result {
        if let Some(window) = owner(hwnd) {
            window.state.borrow_mut().error(error);
        }
    }
}
