//! Parse include arguments without losing their physical source ranges.
use std::{
    collections::HashSet,
    ops::Range,
    path::{Component, Path},
};

const MAX_ARGUMENTS: usize = 32;
pub(super) const MAX_VALUE_BYTES: usize = 4096;
const MAX_DIRECTIVE_BYTES: usize = 128 * 1024;
const UNSUPPORTED: &str = "La inclusión usa parámetros fuera del subconjunto comprobable";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Argument {
    /// Uppercase named key or one-based decimal positional key.
    pub(super) name: String,
    /// Double quotes are removed by ABL. Single quotes remain source bytes.
    pub(super) value: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Directive {
    Include {
        name: Range<usize>,
        arguments: Vec<Argument>,
    },
    Reference(String),
}

/// Return the exclusive end of one outer directive. Quotes can contain braces,
/// and reference braces inside a quoted argument still form nested pairs.
pub(super) fn end(bytes: &[u8], start: usize) -> Result<usize, String> {
    if bytes.get(start) != Some(&b'{') {
        return Err(UNSUPPORTED.into());
    }
    let mut position = start + 1;
    let mut depth = 1usize;
    let mut quote = None;
    while let Some(&byte) = bytes.get(position) {
        if position - start >= MAX_DIRECTIVE_BYTES || byte == b'~' {
            return Err(UNSUPPORTED.into());
        }
        match byte {
            b'\'' | b'"' if quote.is_none() => quote = Some(byte),
            b'\'' | b'"' if quote == Some(byte) => quote = None,
            b'{' => {
                depth += 1;
                if depth > 16 {
                    return Err("Las referencias de parámetros superan 16 niveles".into());
                }
            }
            b'}' if quote.is_none() || depth > 1 => {
                depth -= 1;
                if depth == 0 {
                    return Ok(position + 1);
                }
            }
            _ => {}
        }
        position += 1;
    }
    Err("La inclusión o referencia tiene llaves o comillas sin cerrar".into())
}

fn named_key(bytes: &[u8]) -> Option<String> {
    (bytes.len() <= 64
        && bytes
            .first()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')))
    .then(|| String::from_utf8(bytes.to_ascii_uppercase()).expect("validated ASCII key"))
}

fn reference(bytes: &[u8]) -> Option<String> {
    if let Some(bytes) = bytes.strip_prefix(b"&") {
        return named_key(bytes);
    }
    if bytes.is_empty() || bytes[0] == b'0' || !bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let key = std::str::from_utf8(bytes).ok()?;
    let number = key.parse::<usize>().ok()?;
    (number <= MAX_ARGUMENTS).then(|| key.to_owned())
}

fn skip_space(bytes: &[u8], position: &mut usize) {
    while bytes.get(*position).is_some_and(u8::is_ascii_whitespace) {
        *position += 1;
    }
}

fn value(bytes: &[u8], position: &mut usize) -> Result<Range<usize>, String> {
    let start = *position;
    let quote = bytes
        .get(start)
        .copied()
        .filter(|byte| matches!(byte, b'\'' | b'"'));
    if quote.is_some() {
        *position += 1;
    }
    let content_start = *position;
    let mut reference_start = None;
    while let Some(&byte) = bytes.get(*position) {
        if *position - content_start > MAX_VALUE_BYTES || byte == b'~' {
            return Err(UNSUPPORTED.into());
        }
        if reference_start.is_none()
            && (quote == Some(byte) || quote.is_none() && byte.is_ascii_whitespace())
        {
            break;
        }
        match byte {
            b'{' if reference_start.is_none() => reference_start = Some(*position + 1),
            b'}' => {
                let begin = reference_start.take().ok_or(UNSUPPORTED)?;
                if reference(bytes[begin..*position].trim_ascii()).is_none() {
                    return Err(
                        "El parámetro contiene una referencia dinámica o anidada no admitida"
                            .into(),
                    );
                }
            }
            b'{' => return Err(UNSUPPORTED.into()),
            b'\'' | b'"' if reference_start.is_some() || quote.is_none() => {
                return Err(UNSUPPORTED.into())
            }
            _ => {}
        }
        *position += 1;
    }
    if reference_start.is_some() || *position - content_start > MAX_VALUE_BYTES {
        return Err(UNSUPPORTED.into());
    }
    let content_end = *position;
    if let Some(quote) = quote {
        if bytes.get(*position) != Some(&quote) {
            return Err("El argumento de inclusión tiene comillas sin cerrar".into());
        }
        *position += 1;
        if bytes
            .get(*position)
            .is_some_and(|byte| !byte.is_ascii_whitespace())
        {
            return Err("El argumento requiere separación y comillas simples, sin escapes".into());
        }
        if quote == b'"' {
            Ok(content_start..content_end)
        } else {
            Ok(start..*position)
        }
    } else if content_end == content_start {
        Err("Falta el valor de un argumento de inclusión".into())
    } else {
        Ok(start..content_end)
    }
}

/// `source` is the complete physical file and `range` includes both braces.
/// Nested references in argument values stay intact for caller expansion.
pub(super) fn parse(source: &[u8], range: Range<usize>) -> Result<Directive, String> {
    let raw = source.get(range.clone()).ok_or(UNSUPPORTED)?;
    if raw.len() > MAX_DIRECTIVE_BYTES
        || raw.len() < 2
        || raw[0] != b'{'
        || raw[raw.len() - 1] != b'}'
    {
        return Err(UNSUPPORTED.into());
    }
    let inner = &raw[1..raw.len() - 1];
    if let Some(key) = reference(inner.trim_ascii()) {
        return Ok(Directive::Reference(key));
    }
    let mut position = 0;
    skip_space(inner, &mut position);
    let name_start = position;
    while inner
        .get(position)
        .is_some_and(|byte| !byte.is_ascii_whitespace())
    {
        position += 1;
    }
    let name = &inner[name_start..position];
    let path = std::str::from_utf8(name)
        .map(Path::new)
        .map_err(|_| UNSUPPORTED)?;
    if name.is_empty()
        || name.len() > 240
        || !name
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-/\\".contains(byte))
        || !path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("i"))
        || !path
            .components()
            .all(|part| matches!(part, Component::Normal(_) | Component::CurDir))
    {
        return Err("El nombre del include debe ser una ruta relativa estática .i".into());
    }
    let offset = range.start + 1;
    let name = offset + name_start..offset + position;
    let mut arguments = Vec::new();
    let mut named = None;
    let mut names = HashSet::new();
    loop {
        skip_space(inner, &mut position);
        if position == inner.len() {
            break;
        }
        if arguments.len() == MAX_ARGUMENTS {
            return Err("Una inclusión admite hasta 32 argumentos en esta revisión".into());
        }
        let is_named = inner[position] == b'&';
        if named
            .replace(is_named)
            .is_some_and(|previous| previous != is_named)
        {
            return Err(
                "Una inclusión debe usar argumentos por posición o por nombre, sin mezclarlos"
                    .into(),
            );
        }
        let key = if is_named {
            position += 1;
            let start = position;
            while inner
                .get(position)
                .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            {
                position += 1;
            }
            let key = named_key(&inner[start..position]).ok_or(UNSUPPORTED)?;
            if !names.insert(key.clone()) {
                return Err("Hay nombres de argumentos duplicados en la inclusión".into());
            }
            skip_space(inner, &mut position);
            if inner.get(position) != Some(&b'=') {
                return Err("Falta el signo igual del argumento nombrado".into());
            }
            position += 1;
            skip_space(inner, &mut position);
            key
        } else {
            (arguments.len() + 1).to_string()
        };
        let value = value(inner, &mut position)?;
        arguments.push(Argument {
            name: key,
            value: offset + value.start..offset + value.end,
        });
    }
    Ok(Directive::Include { name, arguments })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn include(source: &[u8], range: Range<usize>) -> Vec<Argument> {
        let Directive::Include { name, arguments } = parse(source, range).unwrap() else {
            panic!("expected include")
        };
        assert_eq!(&source[name], b"shared.i");
        arguments
    }

    #[test]
    fn values_keep_physical_ranges_and_abl_quote_semantics() {
        let source = b"prefix {shared.i &name = \"worker\" &text='two words' &empty=\"\" &forward={&parent}} suffix";
        let arguments = include(source, 7..source.len() - 7);
        assert_eq!(
            arguments
                .iter()
                .map(|argument| argument.name.as_str())
                .collect::<Vec<_>>(),
            ["NAME", "TEXT", "EMPTY", "FORWARD"]
        );
        assert_eq!(
            arguments
                .iter()
                .map(|argument| &source[argument.value.clone()])
                .collect::<Vec<_>>(),
            [b"worker".as_slice(), b"'two words'", b"", b"{&parent}"]
        );
        let positional = b"{shared.i alpha \"two words\" 'quoted' {1}}";
        let arguments = include(positional, 0..positional.len());
        assert_eq!(
            arguments
                .iter()
                .map(|argument| argument.name.as_str())
                .collect::<Vec<_>>(),
            ["1", "2", "3", "4"]
        );
        assert_eq!(&positional[arguments[1].value.clone()], b"two words");
        assert_eq!(&positional[arguments[2].value.clone()], b"'quoted'");
        assert_eq!(
            parse(b"{ &worker }", 0..11).unwrap(),
            Directive::Reference("WORKER".into())
        );
        assert_eq!(
            parse(b"{2}", 0..3).unwrap(),
            Directive::Reference("2".into())
        );
    }

    #[test]
    fn rejects_ambiguous_or_untraceable_argument_expansion() {
        for source in [
            "{shared.i positional &named=value}",
            "{shared.i &named=value positional}",
            "{shared.i &name=a &NAME=b}",
            "{shared.i &name=}",
            "{shared.i &name=\"\"\"quoted\"\"\"}",
            "{shared.i &name=\"tilde~value\"}",
            "{shared.i &name={nested.i}}",
            "{shared.i &name={{1}}}",
            "{shared.i &name=\"missing}",
            "{{1} value}",
            "{../shared.i value}",
            "{shared.i &name = \"value\"&other=b}",
        ] {
            assert!(
                parse(source.as_bytes(), 0..source.len()).is_err(),
                "{source}"
            );
        }
        let source = format!("{{shared.i {}}}", "x".repeat(MAX_VALUE_BYTES + 1));
        assert!(parse(source.as_bytes(), 0..source.len()).is_err());
        let source = format!("{{shared.i {}}}", "x ".repeat(MAX_ARGUMENTS + 1));
        assert!(parse(source.as_bytes(), 0..source.len()).is_err());
    }

    #[test]
    fn directive_end_retains_forwarded_references_and_separates_neighbors() {
        let source = b"{shared.i &name=\"prefix{&outer}\" &body='{1}'}{2}";
        let first = end(source, 0).unwrap();
        let arguments = include(source, 0..first);
        assert_eq!(&source[arguments[0].value.clone()], b"prefix{&outer}");
        assert_eq!(&source[arguments[1].value.clone()], b"'{1}'");
        assert_eq!(end(source, first).unwrap(), source.len());
        assert_eq!(
            parse(source, first..source.len()).unwrap(),
            Directive::Reference("2".into())
        );
        let quoted_brace = b"{shared.i &literal=\"}\"}";
        assert_eq!(end(quoted_brace, 0).unwrap(), quoted_brace.len());
        for source in [
            b"{shared.i &name={&x}".as_slice(),
            b"{shared.i &name=\"missing}",
        ] {
            assert!(end(source, 0).is_err());
        }
    }
}
