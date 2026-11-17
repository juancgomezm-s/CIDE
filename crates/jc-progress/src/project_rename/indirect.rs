//! Resolve indirect calls only through a uniquely initialized global handle.
use crate::rename::{self, Token};
use jc_text::TextEncoding;
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::ErrorKind,
    ops::Range,
    path::{Component, Path, PathBuf},
};

const UNRESOLVED: &str = "No se puede demostrar el destino de una llamada indirecta ABL";

pub(super) struct Lowered {
    pub(super) bytes: Vec<u8>,
    pub(super) calls: Vec<Call>,
}

pub(super) struct Call {
    pub(super) program: PathBuf,
    pub(super) name: Range<usize>,
    pub(super) function: bool,
}

struct Binding {
    program: PathBuf,
    initialized_at: usize,
    deleted_at: Option<usize>,
}

#[derive(Default)]
struct Analysis {
    declarations: HashMap<Vec<u8>, (usize, bool)>,
    bindings: HashMap<Vec<u8>, Binding>,
    known_uses: HashSet<usize>,
    masks: Vec<(Range<usize>, bool)>,
    calls: Vec<Call>,
}

fn identifier(text: &[u8]) -> bool {
    text.len() <= 32
        && text
            .first()
            .is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_')
        && text
            .iter()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
        && !text.ends_with(b"-")
}

fn quoted(text: &[u8]) -> Option<&[u8]> {
    let (&quote, rest) = text.split_first()?;
    if matches!(quote, b'\'' | b'"') {
        rest.strip_suffix(&[quote])
    } else {
        None
    }
}

fn literal(token: &Token<'_>) -> bool {
    quoted(token.text).is_some()
        || [b"?".as_slice(), b"TRUE", b"FALSE", b"YES", b"NO"]
            .iter()
            .any(|word| token.is(word))
        || token.text.iter().any(u8::is_ascii_digit)
            && token
                .text
                .iter()
                .all(|byte| byte.is_ascii_digit() || *byte == b'.')
}

fn literal_arguments(tokens: &[Token<'_>]) -> bool {
    tokens.is_empty()
        || tokens.len() % 2 == 1
            && tokens.iter().enumerate().all(|(index, token)| {
                if index % 2 == 0 {
                    literal(token)
                } else {
                    token.is(b",")
                }
            })
}

fn procedure_arguments(tokens: &[Token<'_>]) -> bool {
    tokens.is_empty()
        || matches!(tokens, [open, tail @ .., close]
            if open.is(b"(") && close.is(b")") && literal_arguments(tail))
}

fn program_path<'a>(tokens: &[Token<'_>], bytes: &'a [u8]) -> Option<&'a str> {
    let first = tokens.first()?;
    let last = tokens.last()?;
    let path = &bytes[first.range.start..last.range.end];
    let path = quoted(path).unwrap_or(path);
    if path.is_empty()
        || path.len() > 240
        || !path
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-/\\".contains(byte))
    {
        return None;
    }
    let text = std::str::from_utf8(path).ok()?;
    let path = Path::new(text);
    let extension = path.extension()?.to_str()?;
    (path
        .components()
        .all(|part| matches!(part, Component::Normal(_) | Component::CurDir))
        && ["p", "w", "abl"]
            .iter()
            .any(|ext| extension.eq_ignore_ascii_case(ext)))
    .then_some(text)
}

fn declaration<'a, 'src>(tokens: &'a [Token<'src>]) -> Option<&'a Token<'src>> {
    let [define, variable, name, as_word, datatype, tail @ ..] = tokens else {
        return None;
    };
    (define.is(b"DEFINE")
        && variable.is(b"VARIABLE")
        && identifier(name.text)
        && as_word.is(b"AS")
        && datatype.is(b"HANDLE")
        && (tail.is_empty() || matches!(tail, [flag] if flag.is(b"NO-UNDO"))))
    .then_some(name)
}

fn indirect_name<'a, 'src>(tokens: &'a [Token<'src>]) -> Option<(Range<usize>, &'a [Token<'src>])> {
    match tokens {
        [value, open, text, close, rest @ ..]
            if value.is(b"VALUE")
                && open.is(b"(")
                && close.is(b")")
                && quoted(text.text).is_some_and(identifier) =>
        {
            Some((text.range.start + 1..text.range.end - 1, rest))
        }
        [name, rest @ ..] if identifier(name.text) && !name.is(b"VALUE") => {
            Some((name.range.clone(), rest))
        }
        _ => None,
    }
}

fn resolve_program(root: &Path, dirs: &[PathBuf], name: &str) -> Result<PathBuf, String> {
    for dir in dirs {
        let source = dir.join(name);
        let compiled = source.with_extension("r");
        match fs::symlink_metadata(&compiled) {
            Ok(_) => {
                return Err(format!(
                    "El r-code oculta el fuente de la llamada: {}",
                    compiled.display()
                ))
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(format!("{}: {error}", compiled.display())),
        }
        match source.canonicalize() {
            Ok(path) if path.starts_with(root) && path.is_file() => return Ok(path),
            Ok(_) => return Err(format!("Programa fuera del proyecto o inválido: {name}")),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(format!("No se pudo resolver {name}: {error}")),
        }
    }
    Err(format!(
        "El PROPATH del proyecto no resuelve el programa {name}"
    ))
}

impl Analysis {
    fn global_declaration(&mut self, name: &Token<'_>) {
        let entry = self
            .declarations
            .entry(name.text.to_ascii_uppercase())
            .or_insert((name.range.start, true));
        entry.1 &= entry.0 == name.range.start;
        self.known_uses.insert(name.range.start);
    }

    fn initialize(
        &mut self,
        root: &Path,
        dirs: &[PathBuf],
        command: &[Token<'_>],
        bytes: &[u8],
        statement: Range<usize>,
    ) -> Result<(), String> {
        let [_, path @ .., persistent, set, handle] = command else {
            return Err(UNRESOLVED.into());
        };
        let key = handle.text.to_ascii_uppercase();
        if !persistent.is(b"PERSISTENT")
            || !set.is(b"SET")
            || !self
                .declarations
                .get(&key)
                .is_some_and(|&(position, unique)| position < statement.start && unique)
            || self.bindings.contains_key(&key)
        {
            return Err(UNRESOLVED.into());
        }
        let name = program_path(path, bytes).ok_or(UNRESOLVED)?;
        let program = resolve_program(root, dirs, name)
            .map_err(|error| format!("{UNRESOLVED}: {name} ({error})"))?;
        self.bindings.insert(
            key,
            Binding {
                program,
                initialized_at: statement.end,
                deleted_at: None,
            },
        );
        self.known_uses.insert(handle.range.start);
        self.masks.push((statement, false));
        Ok(())
    }

    fn call(
        &mut self,
        handle: &Token<'_>,
        name: Range<usize>,
        function: bool,
        statement: Range<usize>,
    ) -> Result<(), String> {
        let binding = self
            .bindings
            .get(&handle.text.to_ascii_uppercase())
            .filter(|binding| {
                binding.initialized_at <= statement.start
                    && binding
                        .deleted_at
                        .is_none_or(|offset| statement.start < offset)
            })
            .ok_or(UNRESOLVED)?;
        self.calls.push(Call {
            program: binding.program.clone(),
            name,
            function,
        });
        self.known_uses.insert(handle.range.start);
        self.masks.push((statement, function));
        Ok(())
    }

    fn delete(
        &mut self,
        command: &[Token<'_>],
        depth: usize,
        delimiter: &Token<'_>,
    ) -> Result<(), String> {
        let [delete, procedure, handle] = command else {
            return Ok(());
        };
        if !delete.is(b"DELETE") || !procedure.is(b"PROCEDURE") {
            return Ok(());
        }
        let Some(binding) = self.bindings.get_mut(&handle.text.to_ascii_uppercase()) else {
            return Ok(());
        };
        if depth != 0 || !delimiter.is(b".") || binding.deleted_at.is_some() {
            return Err(UNRESOLVED.into());
        }
        binding.deleted_at = Some(delete.range.start);
        self.known_uses.insert(handle.range.start);
        self.masks
            .push((delete.range.start..delimiter.range.end, false));
        Ok(())
    }

    fn procedure(&mut self, command: &[Token<'_>], statement: Range<usize>) -> Result<(), String> {
        let (name, remainder) = indirect_name(&command[1..]).ok_or(UNRESOLVED)?;
        match remainder {
            [in_word, handle, arguments @ ..]
                if in_word.is(b"IN") && procedure_arguments(arguments) =>
            {
                self.call(handle, name, false, statement)
            }
            _ => Err(UNRESOLVED.into()),
        }
    }

    fn function(&mut self, expression: &[Token<'_>]) -> Result<(), String> {
        let [function, open, name, in_word, handle, tail @ ..] = expression else {
            return Err(UNRESOLVED.into());
        };
        if !open.is(b"(") || !in_word.is(b"IN") || !quoted(name.text).is_some_and(identifier) {
            return Err(UNRESOLVED.into());
        }
        let end = tail
            .iter()
            .position(|token| token.is(b")"))
            .ok_or(UNRESOLVED)?;
        if end != 0 && !(tail[0].is(b",") && end > 1 && literal_arguments(&tail[1..end])) {
            return Err(UNRESOLVED.into());
        }
        self.call(
            handle,
            name.range.start + 1..name.range.end - 1,
            true,
            function.range.start..tail[end].range.end,
        )
    }
}

fn terminal(token: &Token<'_>, bytes: &[u8]) -> bool {
    token.is(b".")
        && (bytes[token.range.end..].is_empty()
            || bytes[token.range.end].is_ascii_whitespace()
            || bytes[token.range.end..].starts_with(b"/*"))
}

/// Preserve all source offsets. Replace complete reviewed calls with whitespace
/// or a scalar literal so the ordinary binding resolver can inspect the rest.
pub(super) fn lower(
    root: &Path,
    dirs: &[PathBuf],
    bytes: &[u8],
    encoding: TextEncoding,
) -> Result<Lowered, String> {
    let bom = rename::bom_prefix(bytes, encoding);
    let source = &bytes[bom..];
    let tokens = rename::tokenize(source)?;
    let mut analysis = Analysis::default();
    let mut start = 0;
    let mut depth = 0usize;
    let mut nested_dynamic = false;
    for (end, delimiter) in tokens.iter().enumerate() {
        if !terminal(delimiter, source) && !delimiter.is(b":") {
            continue;
        }
        let statement = &tokens[start..end];
        start = end + 1;
        nested_dynamic |= depth != 0 && statement.iter().any(|token| token.is(b"DYNAMIC-FUNCTION"));
        if depth == 0 && delimiter.is(b".") {
            if let Some(name) = declaration(statement) {
                analysis.global_declaration(name);
            }
        }
        if let Some(run) = statement.iter().position(|token| token.is(b"RUN")) {
            let command = &statement[run..];
            let range = command[0].range.start..delimiter.range.end;
            if command.iter().any(|token| token.is(b"PERSISTENT")) {
                if run != 0 || depth != 0 || !delimiter.is(b".") {
                    return Err(UNRESOLVED.into());
                }
                analysis.initialize(root, dirs, command, source, range)?;
            } else if command.iter().any(|token| token.is(b"IN"))
                || command.get(1).is_some_and(|token| token.is(b"VALUE"))
            {
                if run != 0 || depth != 0 || !delimiter.is(b".") {
                    return Err(UNRESOLVED.into());
                }
                analysis.procedure(command, range)?;
            }
        } else {
            analysis.delete(statement, depth, delimiter)?;
        }
        if delimiter.is(b":") {
            depth += 1;
        } else if statement.first().is_some_and(|first| first.is(b"END")) {
            depth = depth.saturating_sub(1);
        }
    }
    for (index, token) in tokens.iter().enumerate() {
        if token.is(b"DYNAMIC-FUNCTION") {
            if nested_dynamic {
                return Err(
                    "La llamada indirecta debe ejecutarse en la secuencia principal del programa"
                        .into(),
                );
            }
            analysis.function(&tokens[index..])?;
        }
        if token.is(b"PROPATH") && !(index > 0 && tokens[index - 1].is(b"FROM"))
            || token.is(b"OS-CHDIR")
        {
            return Err("El fuente puede cambiar las rutas de ejecución del proyecto".into());
        }
        if !analysis.known_uses.contains(&token.range.start)
            && analysis
                .bindings
                .contains_key(&token.text.to_ascii_uppercase())
        {
            // Dynamic calls can occur before the textual traversal reaches the
            // handle operand, so their operands are already registered here.
            return Err(UNRESOLVED.into());
        }
    }
    if tokens[start..].iter().any(|token| token.is(b"RUN")) {
        return Err(UNRESOLVED.into());
    }
    if analysis
        .bindings
        .keys()
        .any(|handle| !analysis.declarations[handle].1)
    {
        return Err(UNRESOLVED.into());
    }

    let mut lowered = bytes.to_vec();
    for (range, function) in analysis.masks {
        let start = range.start + bom;
        let end = range.end + bom;
        for byte in &mut lowered[start..end] {
            if !matches!(*byte, b'\r' | b'\n') {
                *byte = b' ';
            }
        }
        if function {
            lowered[start] = b'0';
        }
    }
    for call in &mut analysis.calls {
        call.name.start += bom;
        call.name.end += bom;
    }
    Ok(Lowered {
        bytes: lowered,
        calls: analysis.calls,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indirect_literal_uses_pin_the_source_and_reject_a_shadow_or_handle_write() {
        let root =
            std::env::temp_dir().join(format!("cide-indirect-rename-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let first = root.join("first");
        let second = root.join("second");
        fs::create_dir(&first).unwrap();
        fs::create_dir(&second).unwrap();
        let path = second.join("host.p");
        let shadow = first.join("host.r");
        fs::write(&path, b"").unwrap();
        let dirs = [first.clone(), second.clone()];
        let root = root.canonicalize().unwrap();
        let source = b"\xef\xbb\xbfDEFINE VARIABLE hostHandle AS HANDLE NO-UNDO.\r\n\
            RUN host.p PERSISTENT SET hostHandle.\r\n\
            RUN VALUE('process') IN hostHandle (7).\r\n\
            DISPLAY DYNAMIC-FUNCTION(\"lengthOf\" IN hostHandle, 7).\r\n\
            DELETE PROCEDURE hostHandle.\r\n";
        let result = lower(&root, &dirs, source, TextEncoding::Utf8).unwrap();
        assert_eq!(result.bytes.len(), source.len());
        assert_eq!(result.calls.len(), 2);
        assert_eq!(&source[result.calls[0].name.clone()], b"process");
        assert_eq!(&source[result.calls[1].name.clone()], b"lengthOf");
        assert!(!result.calls[0].function && result.calls[1].function);
        assert_eq!(result.calls[0].program, path.canonicalize().unwrap());
        let mut changed = source.to_vec();
        changed.extend_from_slice(b"ASSIGN hostHandle = ?.");
        assert!(lower(&root, &dirs, &changed, TextEncoding::Utf8).is_err());
        for statement in [
            "RUN process IN hostHandle.",
            "DISPLAY DYNAMIC-FUNCTION('lengthOf' IN hostHandle, 7).",
        ] {
            let nested = format!(
                "DEFINE VARIABLE hostHandle AS HANDLE NO-UNDO.\n\
                RUN host.p PERSISTENT SET hostHandle.\n\
                PROCEDURE nested:\n{statement}\nEND PROCEDURE."
            );
            assert!(lower(&root, &dirs, nested.as_bytes(), TextEncoding::Utf8).is_err());
        }
        fs::write(&shadow, b"").unwrap();
        let error = lower(&root, &dirs, source, TextEncoding::Utf8)
            .err()
            .unwrap();
        assert!(error.contains("r-code"), "{error}");
        fs::remove_file(&shadow).unwrap();
        fs::remove_file(&path).unwrap();
        fs::remove_dir(&first).unwrap();
        fs::remove_dir(&second).unwrap();
        fs::remove_dir(&root).unwrap();
    }
}
