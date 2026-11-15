use super::{name_at, Cancellation, Imports, Path, TextEncoding};
use crate::rename::{bom_prefix, tokenize};
use jc_app::TextReplacement;
use std::{collections::BTreeSet, ops::Range};

const IMPORT_LIMIT: usize = 16;
const SOURCE_LIMIT: usize = 256 * 1024;

struct Entry {
    name: String,
    range: Range<usize>,
}

pub(in crate::editor::schema::completion) fn organize_imports(
    root: &Path,
    source: &Path,
    bytes: &[u8],
    encoding: TextEncoding,
    cancel: &Cancellation,
) -> Result<Vec<TextReplacement>, String> {
    if bytes.len() > SOURCE_LIMIT {
        return Err("Organizar USING admite documentos de hasta 256 KiB".into());
    }
    let prefix = bom_prefix(bytes, encoding);
    let body = &bytes[prefix..];
    let tokens = tokenize(body)
        .map_err(|_| "El fuente contiene preprocesador o supera el límite de análisis de USING")?;
    let mut position = 0;
    let mut entries: Vec<Entry> = Vec::new();
    let mut short_names = BTreeSet::new();
    while let Some(start) = tokens.get(position).filter(|token| token.is(b"USING")) {
        if cancel.is_cancelled() {
            return Err("Se canceló la revisión de USING".into());
        }
        if entries.len() == IMPORT_LIMIT {
            return Err("Organizar USING admite hasta 16 imports por revisión".into());
        }
        let start_at = start.range.start;
        let name_start = tokens
            .get(position + 1)
            .ok_or("Falta el nombre de USING")?
            .range
            .start;
        let name = name_at(body, encoding, name_start).ok_or("El nombre de USING no es válido")?;
        if name.wildcard || !name.text.contains('.') {
            return Err(
                "Organizar USING requiere nombres de clase completos y sin comodines".into(),
            );
        }
        let end = tokens[position + 1..]
            .iter()
            .position(|token| token.range.start >= name.range.end)
            .map(|end| position + 1 + end)
            .ok_or("Falta el final de USING")?;
        let [from, propath, period] = tokens
            .get(end..end + 3)
            .ok_or("Falta FROM PROPATH en USING")?
        else {
            return Err("Falta FROM PROPATH en USING".into());
        };
        if !from.is(b"FROM") || !propath.is(b"PROPATH") || !period.is(b".") {
            return Err("Organizar USING requiere imports explícitos FROM PROPATH".into());
        }
        let range = start_at..period.range.end;
        let line_start = body[..range.start]
            .iter()
            .rposition(|byte| matches!(byte, b'\r' | b'\n'))
            .map_or(0, |i| i + 1);
        let trailing = body[range.end..]
            .split(|byte| matches!(byte, b'\r' | b'\n'))
            .next()
            .unwrap_or_default();
        let horizontal = |bytes: &[u8]| bytes.iter().all(|byte| matches!(byte, b' ' | b'\t'));
        if !horizontal(&body[line_start..range.start])
            || !horizontal(trailing)
            || tokens[position..end + 3]
                .windows(2)
                .any(|pair| !horizontal(&body[pair[0].range.end..pair[1].range.start]))
            || entries.last().is_some_and(|previous| {
                !body[previous.range.end..range.start]
                    .iter()
                    .all(u8::is_ascii_whitespace)
            })
        {
            return Err(
                "Los USING deben ocupar líneas propias, sin comentarios intercalados".into(),
            );
        }
        let short = name
            .text
            .rsplit('.')
            .next()
            .unwrap_or_default()
            .to_lowercase();
        if !short_names.insert(short) {
            return Err(
                "Hay dos imports con el mismo nombre corto; el orden podría cambiar la resolución"
                    .into(),
            );
        }
        let resolved = Imports::default()
            .load(root, source, &name.text, true, encoding, cancel)
            .map_err(|_| {
                format!(
                    "No se pudo comprobar {} en el PROPATH del proyecto",
                    name.text
                )
            })?;
        if resolved.is_none() {
            return Err(format!(
                "No se pudo resolver {} con el PROPATH del proyecto",
                name.text
            ));
        }
        entries.push(Entry {
            name: name.text.to_lowercase(),
            range,
        });
        position = end + 3;
    }
    if tokens[position..].iter().any(|token| token.is(b"USING")) {
        return Err(
            "Organizar USING requiere un único grupo de imports al inicio del archivo".into(),
        );
    }
    let mut order: Vec<_> = entries.iter().collect();
    order.sort_by(|left, right| left.name.cmp(&right.name));
    entries
        .iter()
        .zip(order)
        .filter(|(old, new)| old.range != new.range)
        .map(|(old, new)| {
            let decoded = jc_text::decode_fragment(
                &body[new.range.clone()],
                encoding,
                prefix + new.range.start,
            );
            if !decoded.issues.is_empty() {
                return Err("La codificación de USING no se pudo conservar".into());
            }
            Ok(TextReplacement {
                range: prefix + old.range.start..prefix + old.range.end,
                text: decoded.text,
            })
        })
        .collect()
}
