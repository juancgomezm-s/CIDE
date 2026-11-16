//! Reviewed creation, deletion and reversal of row presence.
use super::{
    decode_cell, hex, io_error, journal_directory, new_backup_id, parse_snapshot, publish_verified,
    read_in_root, run_mode, snapshot_wire, value_wire, write_database, AvmMode, BrowseTable,
    ColumnKind, RecordAction, RecordController, RecordEvent, RecordPreview, Selection, Snapshot,
    MAX_PROTOCOL,
};
use jc_app::{Cancellation, Wakeup};
use jc_files::ContentFingerprint;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

const HEADER: &str = "JCIDE-FILA/1";
const ABSENT: &str = "ABSENT\nEND\n";

pub(super) struct Operation {
    pub(super) selection: Selection,
    database: PathBuf,
    before: Option<Snapshot>,
    action: Change,
    source: Option<Source>,
}

enum Change {
    Insert(String),
    Delete,
    Recreate(Snapshot),
}

struct Source {
    id: String,
    sealed: ContentFingerprint,
}

impl RecordController {
    pub fn request_create(
        &mut self,
        root: &Path,
        table: &BrowseTable,
        key: i32,
        values: Vec<(usize, String)>,
        wakeup: Wakeup,
    ) -> Result<(), String> {
        let selection = choose(table, key)?;
        selection.check_catalog(root)?;
        let mut columns = BTreeSet::new();
        let mut assignments = Vec::new();
        let mut summary = vec![format!("Clave {key} · Valores iniciales de la tabla")];
        for (column, value) in values {
            if !table.can_edit_column(column) || !columns.insert(column) {
                return Err("La alta contiene una columna repetida o incompatible".into());
            }
            let field = &table.target.columns[column].0;
            let mut field_selection = selection.clone();
            field_selection.column = column;
            let wire = value_wire(&field_selection, &value)?;
            assignments.push(format!("{field}:{wire}"));
            summary.push(format!("{field} = «{value}»"));
        }
        self.start(root, wakeup, false, move |root, cancel| {
            let database = write_database(root, &selection)?;
            if run_mode(root, &selection, &database, AvmMode::Inspect, cancel)?.is_some() {
                return Err(format!(
                    "Conflicto: ya existe un registro con la clave {key}"
                ));
            }
            let operation = Operation {
                selection,
                database,
                before: None,
                action: Change::Insert(assignments.join(",")),
                source: None,
            };
            Ok(preview(
                operation,
                "Alta de registro",
                None,
                Some(summary.join("\n")),
            ))
        })
    }

    pub fn request_delete(
        &mut self,
        root: &Path,
        table: &BrowseTable,
        row: &[Option<String>],
        wakeup: Wakeup,
    ) -> Result<(), String> {
        let key = Selection::new(table, row, 0)?.key;
        let selection = choose(table, key)?;
        selection.check_catalog(root)?;
        let visible = row
            .get(selection.column)
            .cloned()
            .ok_or("Selecciona un registro")?;
        self.start(root, wakeup, false, move |root, cancel| {
            let database = write_database(root, &selection)?;
            let before = run_mode(root, &selection, &database, AvmMode::Inspect, cancel)?
                .ok_or("Conflicto: el registro ya no existe")?;
            if decode_cell(&before.value, &selection.table.catalog.codec)? != visible {
                return Err("Conflicto: cambió el registro seleccionado. Recarga la página".into());
            }
            let summary = describe(&selection, Some(&before))?;
            let operation = Operation {
                selection,
                database,
                before: Some(before),
                action: Change::Delete,
                source: None,
            };
            Ok(preview(operation, "Borrado de registro", summary, None))
        })
    }

    pub fn request_structure_restore(
        &mut self,
        root: &Path,
        table: &BrowseTable,
        key: i32,
        wakeup: Wakeup,
    ) -> Result<(), String> {
        let selection = choose(table, key)?;
        selection.check_catalog(root)?;
        self.start(root, wakeup, false, move |root, cancel| {
            let database = write_database(root, &selection)?;
            let directory = journal_directory(root, &selection, false)?;
            let source =
                latest_id(&directory)?.ok_or("La clave no tiene un alta o borrado confirmado")?;
            let (before, after, sealed) = read_verified(root, &directory, &database, &source)?;
            let live = run_mode(root, &selection, &database, AvmMode::Inspect, cancel)?;
            if live != after {
                return Err("Conflicto: la clave o el registro cambió desde el respaldo".into());
            }
            let current = describe(&selection, live.as_ref())?;
            let proposed = describe(&selection, before.as_ref())?;
            let action = match (before, &live) {
                (Some(row), None) => Change::Recreate(row),
                (None, Some(_)) => Change::Delete,
                _ => return Err("El respaldo no describe un alta o borrado recuperable".into()),
            };
            let operation = Operation {
                selection,
                database,
                before: live,
                action,
                source: Some(Source { id: source, sealed }),
            };
            Ok(preview(
                operation,
                "Restauración de alta o borrado",
                current,
                proposed,
            ))
        })
    }
}

pub(super) fn choose(table: &BrowseTable, key: i32) -> Result<Selection, String> {
    let column = table
        .target
        .columns
        .iter()
        .enumerate()
        .find(|(i, (_, kind))| table.can_edit_column(*i) && matches!(kind, ColumnKind::Character))
        .or_else(|| {
            table
                .target
                .columns
                .iter()
                .enumerate()
                .find(|(i, _)| table.can_edit_column(*i))
        })
        .map(|(i, _)| i)
        .ok_or("Se requiere una clave primaria entera y una columna editable")?;
    Ok(Selection {
        table: table.clone(),
        key,
        column,
    })
}

fn describe(selection: &Selection, record: Option<&Snapshot>) -> Result<Option<String>, String> {
    record
        .map(|record| {
            let value = decode_cell(&record.value, &selection.table.catalog.codec)?;
            Ok(format!(
                "Registro completo · Clave {} · {} = {}",
                selection.key,
                selection.column_name(),
                value.map_or_else(|| "?".into(), |value| format!("«{value}»"))
            ))
        })
        .transpose()
}

fn preview(
    operation: Operation,
    title: &'static str,
    before: Option<String>,
    after: Option<String>,
) -> RecordEvent {
    RecordEvent::Preview(RecordPreview {
        column: format!("Clave {}", operation.selection.key),
        before,
        after,
        restoring: operation.source.is_some(),
        structural_title: Some(title),
        operation: RecordAction::Structure(Box::new(operation)),
    })
}

pub(super) fn apply(
    root: &Path,
    operation: &Operation,
    cancel: &Cancellation,
) -> Result<RecordEvent, String> {
    let selection = &operation.selection;
    let database = write_database(root, selection)?;
    if database != operation.database {
        return Err("Cambió la base desde la vista previa".into());
    }
    let directory = journal_directory(root, selection, true)?;
    if let Some(source) = &operation.source {
        let (_, after, sealed) = read_verified(root, &directory, &database, &source.id)?;
        if sealed != source.sealed || after != operation.before {
            return Err("Conflicto: cambió el respaldo revisado".into());
        }
    }
    let id = new_backup_id()?;
    let path = database.to_str().ok_or("Ruta de base inválida")?;
    let backup = format!(
        "{HEADER}\n{}\n{}",
        hex(path.as_bytes()),
        image_wire(operation.before.as_ref())
    );
    publish_verified(
        root,
        &directory.join(format!("{id}.fila")),
        backup.as_bytes(),
    )?;
    let mode = match &operation.action {
        Change::Insert(values) => AvmMode::Insert(values),
        Change::Delete => AvmMode::Delete(
            operation
                .before
                .as_ref()
                .ok_or("Falta la imagen del registro")?,
        ),
        Change::Recreate(image) => AvmMode::Recreate(image),
    };
    let after = run_mode(root, selection, &database, mode, cancel)
        .map_err(|error| format!("{error}. Respaldo verificado {id}"))?;
    let receipt = format!("{backup}{}", image_wire(after.as_ref()));
    publish_verified(root, &directory.join(format!("{id}.fila-confirmada")), receipt.as_bytes())
        .map_err(|error| format!("OpenEdge confirmó la operación de fila. Se conserva el respaldo {id}, pero falló el recibo: {error}"))?;
    Ok(RecordEvent::Applied)
}

fn latest_id(directory: &Path) -> Result<Option<String>, String> {
    let mut latest: Option<String> = None;
    for (index, entry) in fs::read_dir(directory).map_err(io_error)?.enumerate() {
        if index >= 8192 {
            return Err("El historial de la clave supera su límite de revisión".into());
        }
        let name = entry.map_err(io_error)?.file_name();
        let Some(id) = name
            .to_str()
            .and_then(|name| name.strip_suffix(".fila-confirmada"))
        else {
            continue;
        };
        if valid_id(id) && latest.as_deref().is_none_or(|previous| id > previous) {
            latest = Some(id.to_owned());
        }
    }
    Ok(latest)
}

fn valid_id(id: &str) -> bool {
    id.len() == 71
        && id.bytes().enumerate().all(|(position, byte)| {
            if matches!(position, 39 | 50) {
                byte == b'-'
            } else {
                byte.is_ascii_digit()
            }
        })
}

fn read_verified(
    root: &Path,
    directory: &Path,
    database: &Path,
    id: &str,
) -> Result<(Option<Snapshot>, Option<Snapshot>, ContentFingerprint), String> {
    let invalid = "El respaldo de la fila no coincide con su recibo";
    if !valid_id(id) {
        return Err(invalid.into());
    }
    let (_, bytes) = read_in_root(root, &directory.join(format!("{id}.fila")), MAX_PROTOCOL)
        .map_err(io_error)?;
    let (_, receipt) = read_in_root(
        root,
        &directory.join(format!("{id}.fila-confirmada")),
        MAX_PROTOCOL,
    )
    .map_err(io_error)?;
    let header = format!(
        "{HEADER}\n{}\n",
        hex(database.to_str().ok_or(invalid)?.as_bytes())
    );
    let before = bytes
        .strip_prefix(header.as_bytes())
        .ok_or("El respaldo pertenece a otra base")?;
    let after = receipt.strip_prefix(bytes.as_slice()).ok_or(invalid)?;
    let sealed = ContentFingerprint::of_chunks([receipt.as_slice()]);
    Ok((read_image(before)?, read_image(after)?, sealed))
}

fn read_image(bytes: &[u8]) -> Result<Option<Snapshot>, String> {
    if bytes == ABSENT.as_bytes() {
        return Ok(None);
    }
    let wire = std::str::from_utf8(bytes).map_err(|_| "La imagen respaldada no es legible")?;
    parse_snapshot(wire.lines()).map(Some)
}

fn image_wire(image: Option<&Snapshot>) -> String {
    image.map_or_else(|| ABSENT.into(), snapshot_wire)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::SchemaController;
    use std::time::{Duration, Instant};

    fn wait_for<T>(wakeup: &Wakeup, mut poll: impl FnMut() -> Option<T>) -> T {
        let deadline = Instant::now() + Duration::from_secs(22);
        loop {
            let ticket = wakeup.ticket();
            if let Some(value) = poll() {
                return value;
            }
            assert!(
                Instant::now() < deadline,
                "OpenEdge no respondió a la prueba de filas"
            );
            wakeup.wait(ticket, Some(Duration::from_millis(40)));
        }
    }

    #[test]
    #[ignore = "requiere una copia de Sports2000 configurada para editar"]
    fn progress_real_record_structure_creates_deletes_and_restores() {
        let root =
            PathBuf::from(std::env::var_os("CIDE_TEST_PROGRESS_RECORD_EDIT_PROJECT").unwrap())
                .canonicalize()
                .unwrap();
        let wake = Wakeup::default();
        let mut schema = SchemaController::default();
        let mut workbench = jc_app::Workbench::default();
        schema.request(Some(&root), wake.clone()).unwrap();
        assert!(
            !wait_for(&wake, || schema.poll(Some(&root), &mut workbench)).problems,
            "{:?}",
            workbench.problems()
        );
        let table = schema
            .browse_tables()
            .into_iter()
            .find(|table| table.name.eq_ignore_ascii_case("Customer"))
            .unwrap();
        let key = 19999;
        let selection = choose(&table, key).unwrap();
        let db = write_database(&root, &selection).unwrap();
        let cancel = Cancellation::new(Duration::from_secs(75));
        let inspect = || run_mode(&root, &selection, &db, AvmMode::Inspect, &cancel).unwrap();
        assert!(inspect().is_none());
        let column = selection.column;
        let mut row = vec![None; table.columns.len()];
        row[table.edit_key_column().unwrap()] = Some(key.to_string());
        let text = "Alta CIDE ñ";
        row[column] = Some(text.into());
        let mut controller = RecordController::default();
        let read = |controller: &mut RecordController| wait_for(&wake, || controller.poll(&root));
        let apply = |controller: &mut RecordController| {
            let preview = match read(controller) {
                Ok(RecordEvent::Preview(preview)) => preview,
                Err(error) => panic!("{error}"),
                _ => panic!("Se esperaba una vista previa"),
            };
            assert!(preview.structural_title.is_some());
            controller.apply(&root, preview, wake.clone()).unwrap();
            match read(controller) {
                Ok(RecordEvent::Applied) => {}
                Err(error) => panic!("{error}"),
                _ => panic!("Se esperaba confirmación de OpenEdge"),
            }
        };

        // Capture an absent row before creating it, then restore the absence.
        let values = vec![(column, text.into())];
        controller
            .request_create(&root, &table, key, values.clone(), wake.clone())
            .unwrap();
        apply(&mut controller);
        let added = inspect().unwrap();
        assert_eq!(
            decode_cell(&added.value, &table.catalog.codec)
                .unwrap()
                .as_deref(),
            Some(text)
        );
        controller
            .request_create(&root, &table, key, values.clone(), wake.clone())
            .unwrap();
        assert!(read(&mut controller).err().unwrap().contains("Conflicto"));
        controller
            .request_structure_restore(&root, &table, key, wake.clone())
            .unwrap();
        apply(&mut controller);
        assert!(inspect().is_none());

        // Recreate from a full binary backup taken before deletion.
        controller
            .request_create(&root, &table, key, values, wake.clone())
            .unwrap();
        apply(&mut controller);
        let before = inspect().unwrap();
        controller
            .request_delete(&root, &table, &row, wake.clone())
            .unwrap();
        apply(&mut controller);
        assert!(inspect().is_none());
        controller
            .request_structure_restore(&root, &table, key, wake.clone())
            .unwrap();
        let Ok(RecordEvent::Preview(preview)) = read(&mut controller) else {
            panic!("Falta la vista previa de restauración");
        };
        let external = format!(
            "{}:{}",
            selection.column_name(),
            value_wire(&selection, "Otro registro").unwrap()
        );
        let concurrent = run_mode(&root, &selection, &db, AvmMode::Insert(&external), &cancel)
            .unwrap()
            .unwrap();
        controller.apply(&root, preview, wake.clone()).unwrap();
        assert!(read(&mut controller).err().unwrap().contains("Conflicto"));
        assert!(inspect().unwrap() == concurrent);
        run_mode(
            &root,
            &selection,
            &db,
            AvmMode::Delete(&concurrent),
            &cancel,
        )
        .unwrap();

        controller
            .request_structure_restore(&root, &table, key, wake.clone())
            .unwrap();
        apply(&mut controller);
        let restored = inspect().unwrap();
        assert_eq!(restored.image, before.image);
        assert_eq!(restored.value, before.value);
        controller
            .request_delete(&root, &table, &row, wake.clone())
            .unwrap();
        apply(&mut controller);
        assert!(inspect().is_none());
    }
}
