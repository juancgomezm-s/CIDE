use super::{
    decode_cell, fs, io, io_error, parse_snapshot, read_in_root, Path, Selection, Snapshot,
    MAX_PROTOCOL,
};
use serde_json::{json, Value};

pub(super) fn document(
    selection: &Selection,
    database: &Path,
    operation: &str,
    before: Option<&Snapshot>,
) -> Result<Value, String> {
    let codec = &selection.table.catalog.codec;
    let before = before
        .map(|row| {
            let value = decode_cell(&row.value, codec)?;
            Ok::<_, String>(json!({
                "rowid": row.row_id,
                "imagen_hex": row.image,
                "valor_hex": row.value,
                "valor": value
            }))
        })
        .transpose()?;
    Ok(json!({
        "version": 1,
        "base": selection.table.database,
        "archivo": database.to_str().ok_or("Ruta de base inválida")?,
        "tabla": selection.table.name,
        "clave": selection.key,
        "columna": selection.column_name(),
        "codificacion": codec,
        "operacion": operation,
        "anterior": before
    }))
}

pub(super) fn encode(value: &Value) -> Result<Vec<u8>, String> {
    let mut bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("Respaldo JSON inválido: {error}"))?;
    bytes.push(b'\n');
    Ok(bytes)
}

pub(super) fn read(
    root: &Path,
    directory: &Path,
    database: &Path,
    id: &str,
    suffix: &str,
) -> Result<(Vec<u8>, Option<Value>), String> {
    let path = directory.join(format!("{id}.{suffix}.json"));
    match fs::symlink_metadata(&path) {
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let (_, bytes) = read_in_root(
                root,
                &directory.join(format!("{id}.{suffix}")),
                MAX_PROTOCOL,
            )
            .map_err(io_error)?;
            return Ok((bytes, None));
        }
        Err(error) => return Err(io_error(error)),
    }
    let (_, bytes) = read_in_root(root, &path, MAX_PROTOCOL).map_err(io_error)?;
    let document: Value =
        serde_json::from_slice(&bytes).map_err(|_| "El respaldo JSON está incompleto")?;
    if document["version"].as_u64() != Some(1)
        || document["archivo"].as_str() != database.to_str()
        || !document
            .as_object()
            .is_some_and(|entry| entry.contains_key("anterior"))
    {
        return Err("El respaldo pertenece a otra base o versión".into());
    }
    Ok((bytes, Some(document)))
}

pub(super) fn image(value: &Value) -> Result<Option<Snapshot>, String> {
    if value.is_null() {
        return Ok(None);
    }
    let invalid = "La imagen JSON del registro está incompleta";
    let rowid = value["rowid"].as_str().ok_or(invalid)?;
    let raw = value["imagen_hex"].as_str().ok_or(invalid)?;
    let wire = value["valor_hex"].as_str().ok_or(invalid)?;
    let record = format!("ROW|{rowid}\nIMAGE|{raw}\nVALUE|{wire}\nEND");
    parse_snapshot(record.lines()).map(Some)
}
