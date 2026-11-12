use super::{boundaries, Position, RESULT_LIMIT};
use crate::{
    source_services::{SourceAction, SourceItem},
    TaskItem, TextReplacement,
};
use jc_text::{DecodedScalars, TextEncoding};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
};

type Span = (Position, Position);

fn position(value: &Value) -> Option<Position> {
    Some(Position {
        line: usize::try_from(value["line"].as_u64()?).ok()?,
        column: usize::try_from(value["character"].as_u64()?).ok()?,
    })
}

fn span(value: &Value) -> Option<Span> {
    let start = position(&value["start"])?;
    let end = position(&value["end"])?;
    (start <= end).then_some((start, end))
}

fn coordinates(
    bytes: &[u8],
    encoding: TextEncoding,
    spans: impl Iterator<Item = Span>,
) -> BTreeMap<Position, usize> {
    let requested: BTreeSet<_> = spans.flat_map(|(start, end)| [start, end]).collect();
    let mut requested = requested.into_iter().peekable();
    let mut result = BTreeMap::new();
    for (offset, position) in boundaries(bytes, encoding) {
        while requested.peek().is_some_and(|key| *key < position) {
            requested.next();
        }
        if requested.peek() == Some(&position) {
            result.insert(position, offset);
            requested.next();
        }
        if requested.peek().is_none() {
            break;
        }
    }
    result
}

fn byte_range(offsets: &BTreeMap<Position, usize>, (start, end): Span) -> Option<Range<usize>> {
    Some(*offsets.get(&start)?..*offsets.get(&end)?)
}

fn display(text: &str, limit: usize) -> String {
    text.chars()
        .take(limit)
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect()
}

fn insert_text(value: &Value) -> Option<&str> {
    value.as_str().filter(|text| {
        text.len() <= 4096
            && !text
                .chars()
                .any(|ch| ch.is_control() && !matches!(ch, '\r' | '\n' | '\t'))
    })
}

struct Completion<'a> {
    label: &'a str,
    text: &'a str,
    span: Option<Span>,
    additional: Vec<(Span, &'a str)>,
}

fn completion<'a>(item: &'a Value, defaults: &'a Value, at: Position) -> Option<Completion<'a>> {
    let format = item
        .get("insertTextFormat")
        .or_else(|| defaults.get("insertTextFormat"));
    if format.is_some_and(|format| format.as_u64() != Some(1)) {
        return None;
    }
    let label = item["label"].as_str().filter(|label| !label.is_empty())?;
    let edit = item.get("textEdit");
    let supplied_range = match edit {
        Some(edit) => Some(edit.get("range").unwrap_or(&edit["replace"])),
        None => defaults
            .get("editRange")
            .map(|range| range.get("replace").unwrap_or(range)),
    };
    let primary = match supplied_range {
        Some(value) => {
            let (start, end) = span(value)?;
            if start.line != at.line || end.line != at.line || !(start..=end).contains(&at) {
                return None;
            }
            Some((start, end))
        }
        None => None,
    };
    let text = if let Some(edit) = edit {
        insert_text(&edit["newText"])?
    } else {
        insert_text(
            item.get("textEditText")
                .or_else(|| item.get("insertText"))
                .unwrap_or(&item["label"]),
        )?
    };
    let additional = match item.get("additionalTextEdits") {
        None => Vec::new(),
        Some(value) => value
            .as_array()
            .filter(|edits| edits.len() <= 8)?
            .iter()
            .map(|edit| Some((span(&edit["range"])?, insert_text(&edit["newText"])?)))
            .collect::<Option<Vec<_>>>()?,
    };
    if text.len() + additional.iter().map(|(_, text)| text.len()).sum::<usize>() > 4096 {
        return None;
    }
    Some(Completion {
        label,
        text,
        span: primary,
        additional,
    })
}

pub(super) fn completions(
    response: &Value,
    bytes: &[u8],
    encoding: TextEncoding,
    cursor: usize,
    at: Position,
) -> Result<(Vec<TaskItem>, usize), String> {
    let items = match response {
        Value::Null => &[],
        Value::Array(items) => items.as_slice(),
        _ => response["items"]
            .as_array()
            .ok_or("Lista de completado LSP inválida")?
            .as_slice(),
    };
    let candidates: Vec<_> = items
        .iter()
        .take(RESULT_LIMIT)
        .filter_map(|item| completion(item, &response["itemDefaults"], at))
        .collect();
    let offsets = coordinates(
        bytes,
        encoding,
        candidates.iter().flat_map(|item| {
            item.span
                .into_iter()
                .chain(item.additional.iter().map(|(span, _)| *span))
        }),
    );
    let mut prefix = cursor;
    for scalar in DecodedScalars::new(&bytes[..cursor], encoding, 0) {
        if !(scalar.character.is_alphanumeric() || matches!(scalar.character, '_' | '$')) {
            prefix = scalar.source_range.end;
        } else if prefix == cursor {
            prefix = scalar.source_range.start;
        }
    }
    let mut output = Vec::new();
    let mut omitted = items.len() - candidates.len();
    let mut budget = 40 * 1024;
    for candidate in candidates {
        let proposal = (|| {
            let range = match candidate.span {
                Some(span) => byte_range(&offsets, span)?,
                None => prefix..cursor,
            };
            if range.len() > 4096 {
                return None;
            }
            let mut edits = vec![TextReplacement {
                range: range.clone(),
                text: candidate.text.to_owned(),
            }];
            for (span, text) in candidate.additional {
                edits.push(TextReplacement {
                    range: byte_range(&offsets, span)?,
                    text: text.to_owned(),
                });
            }
            edits.sort_by_key(|edit| edit.range.start);
            if edits.windows(2).any(|pair| {
                pair[0].range.end > pair[1].range.start
                    || pair[0].range.start == pair[1].range.start
            }) {
                return None;
            }
            let label = display(candidate.label, 110);
            Some(SourceItem {
                range,
                label,
                action: SourceAction::LspCompletion(edits),
            })
        })();
        match proposal {
            Some(proposal) => {
                if !output
                    .iter()
                    .any(|item| matches!(item, TaskItem::Source(previous) if *previous == proposal))
                {
                    let size = proposal.allocation();
                    if size <= budget {
                        budget -= size;
                        output.push(TaskItem::Source(proposal));
                    } else {
                        omitted += 1;
                    }
                }
            }
            None => omitted += 1,
        }
    }
    Ok((output, omitted))
}

pub(super) fn diagnostics(
    response: &Value,
    bytes: &[u8],
    encoding: TextEncoding,
) -> Result<(Vec<TaskItem>, usize), String> {
    let items = response
        .as_array()
        .or_else(|| {
            (response["kind"] == "full")
                .then(|| response["items"].as_array())
                .flatten()
        })
        .ok_or("Informe de diagnósticos LSP inválido")?;
    let candidates: Vec<_> = items
        .iter()
        .take(RESULT_LIMIT)
        .filter_map(|item| Some((span(&item["range"])?, item["message"].as_str()?, item)))
        .collect();
    let offsets = coordinates(bytes, encoding, candidates.iter().map(|(span, _, _)| *span));
    let mut output = Vec::new();
    for (span, message, item) in candidates {
        let Some(range) = byte_range(&offsets, span) else {
            continue;
        };
        let severity = match item["severity"].as_u64() {
            Some(1) => "Error",
            Some(2) => "Advertencia",
            Some(3) => "Información",
            Some(4) => "Sugerencia",
            _ => "Diagnóstico",
        };
        let code = item["code"]
            .as_str()
            .map(|code| display(code, 48))
            .or_else(|| item["code"].as_i64().map(|code| code.to_string()));
        let code = code.map_or_else(String::new, |code| format!(" {code}"));
        output.push(TaskItem::Diagnostic {
            range,
            message: format!("LSP · {severity}{code} · {}", display(message, 240)),
        });
    }
    let omitted = items.len() - output.len();
    Ok((output, omitted))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn semantic_completion_maps_utf16_replace_and_additional_edits() {
        let bytes = "😀 obj.calcu\r\n".as_bytes();
        let data = json!({"items":[{
            "label":"calcular", "textEdit": {"newText":"calcular", "replace":{
                "start":{"line":0,"character":7},"end":{"line":0,"character":12}
            }}, "additionalTextEdits":[{"newText":"import lib;\r\n","range":{
                "start":{"line":0,"character":0},"end":{"line":0,"character":0}
            }}]
        },{"label":"snippet","insertTextFormat":2,"insertText":"${1:name}"}]});
        let (items, omitted) = completions(
            &data,
            bytes,
            TextEncoding::Utf8,
            14,
            Position {
                line: 0,
                column: 12,
            },
        )
        .unwrap();
        assert_eq!(omitted, 1);
        let TaskItem::Source(item) = &items[0] else {
            panic!("source completion")
        };
        assert_eq!(item.range, 9..14);
        let SourceAction::LspCompletion(edits) = &item.action else {
            panic!("semantic edits")
        };
        assert_eq!(edits[0].range, 0..0);
        assert_eq!(edits[1].text, "calcular");
    }

    #[test]
    fn diagnostics_convert_ranges_and_preserve_severity_and_codes() {
        let bytes = "\u{feff}😀\r\nerror".as_bytes();
        let data = json!({"kind":"full","items":[{
            "range":{"start":{"line":1,"character":0},"end":{"line":1,"character":5}},
            "severity":1,"code":2304,"message":"Error en\nprueba"
        },{"range":{"start":{"line":0,"character":1},"end":{"line":0,"character":2}},"message":"surrogate"}]});
        let (items, omitted) = diagnostics(&data, bytes, TextEncoding::Utf8).unwrap();
        assert_eq!(omitted, 1);
        assert_eq!(
            items,
            [TaskItem::Diagnostic {
                range: 9..14,
                message: "LSP · Error 2304 · Error en prueba".into()
            }]
        );
        assert!(diagnostics(&json!([]), bytes, TextEncoding::Utf8)
            .unwrap()
            .0
            .is_empty());
    }
}
