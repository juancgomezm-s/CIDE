//! Reviewed project-wide symbol rename for parameterless direct includes.
use crate::{editor::project_propath_dirs, rename};
use jc_app::{source_services::ProjectRenameEdit, Cancellation};
use jc_files::{list_directory_in_root, read_in_root, ContentFingerprint};
use jc_languages::syntax::{scan, LexerState, SyntaxKind, SyntaxProfile};
use jc_text::{probe_encoding, TextEncoding};
use std::{
    collections::VecDeque,
    io::ErrorKind,
    ops::Range,
    path::{Component, Path, PathBuf},
};

const MAX_FILE_BYTES: usize = 256 * 1024;
const MAX_TOTAL_BYTES: usize = 8 * 1024 * 1024;
const MAX_FILES: usize = 256;
const MAX_DIRECTORIES: usize = 64;
const MAX_DIRECTIVES: usize = 256;
const MAX_EDITS: usize = 64;
const LIMIT: &str = "El proyecto supera los límites de la revisión de renombrado ABL";
const CANCELLED: &str = "Se canceló la revisión de renombrado ABL";
const UNSUPPORTED: &str =
    "La revisión de proyecto admite un include directo, estático y sin parámetros por fuente";

struct Include {
    range: Range<usize>,
    name: String,
}

fn is_include(path: &Path) -> bool {
    extension(path).eq_ignore_ascii_case("i")
}

fn extension(path: &Path) -> &str {
    path.extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
}

fn directives(bytes: &[u8], encoding: TextEncoding) -> Result<Vec<Include>, String> {
    let mut state = LexerState::default();
    let mut spans: Vec<Range<usize>> = Vec::new();
    let mut too_many = false;
    let mut in_run = false;
    let mut indirect_call = false;
    let offset = rename::bom_prefix(bytes, encoding);
    scan(
        SyntaxProfile::Abl,
        &mut state,
        &bytes[offset..],
        offset,
        true,
        |span| {
            if too_many || indirect_call {
                return;
            }
            if span.kind != SyntaxKind::Preprocessor {
                let token = &bytes[span.source];
                if span.kind == SyntaxKind::Keyword && token.eq_ignore_ascii_case(b"RUN") {
                    in_run = true;
                } else if span.kind == SyntaxKind::Operator && token == b"." {
                    in_run = false;
                } else {
                    indirect_call = in_run
                        && span.kind == SyntaxKind::Keyword
                        && token.eq_ignore_ascii_case(b"IN")
                        || matches!(span.kind, SyntaxKind::Keyword | SyntaxKind::Plain)
                            && token.eq_ignore_ascii_case(b"DYNAMIC-FUNCTION");
                }
                return;
            }
            if let Some(last) = spans
                .last_mut()
                .filter(|last| last.end == span.source.start)
            {
                last.end = span.source.end;
            } else if spans.len() == MAX_DIRECTIVES {
                too_many = true;
            } else {
                spans.push(span.source);
            }
        },
    );
    if state.limited() || too_many {
        return Err(LIMIT.into());
    }
    if indirect_call {
        return Err(
            "El proyecto contiene una llamada indirecta que no permite comprobar el renombrado"
                .into(),
        );
    }
    spans
        .into_iter()
        .map(|range| {
            let raw = &bytes[range.clone()];
            // Require ASCII relative .i names so path resolution does not depend on
            // AVM code page transliteration or quoting rules.
            let name = std::str::from_utf8(raw)
                .ok()
                .and_then(|text| text.strip_prefix('{')?.strip_suffix('}'))
                .map(str::trim)
                .filter(|text| !text.is_empty() && text.len() <= 240)
                .filter(|text| {
                    text.bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"._-/\\".contains(&byte))
                })
                .filter(|text| {
                    is_include(Path::new(text))
                        && Path::new(text)
                            .components()
                            .all(|part| matches!(part, Component::Normal(_) | Component::CurDir))
                })
                .ok_or(UNSUPPORTED)?;
            Ok(Include {
                range,
                name: name.into(),
            })
        })
        .collect()
}

fn resolve_include(root: &Path, dirs: &[PathBuf], name: &str) -> Result<PathBuf, String> {
    for dir in dirs {
        // Resolve the first candidate. Project bytes are captured separately
        // through the confined reader and only the reviewed target is expanded.
        let candidate = dir.join(name);
        match candidate.canonicalize() {
            Ok(path) if path.starts_with(root) && path.is_file() => return Ok(path),
            Ok(_) => return Err(format!("Include fuera del proyecto o inválido: {name}")),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(format!("No se pudo resolver {name}: {error}")),
        }
    }
    Err(format!("El PROPATH del proyecto no resuelve {name}"))
}

fn standalone(bytes: &[u8], range: &Range<usize>, bom: usize) -> bool {
    let line_space = |byte: &u8| matches!(byte, b' ' | b'\t');
    bytes[bom..range.start]
        .iter()
        .rev()
        .take_while(|b| !matches!(b, b'\r' | b'\n'))
        .all(line_space)
        && bytes[range.end..]
            .iter()
            .take_while(|b| !matches!(b, b'\r' | b'\n'))
            .all(line_space)
}

struct Source<'a> {
    path: &'a Path,
    bytes: &'a [u8],
    encoding: TextEncoding,
}

impl Source<'_> {
    fn edits(
        &self,
        ranges: Vec<Range<usize>>,
        edits: &mut Vec<ProjectRenameEdit>,
    ) -> Result<(), String> {
        if edits.len().saturating_add(ranges.len()) > MAX_EDITS {
            return Err(LIMIT.into());
        }
        let version = ContentFingerprint::of(self.bytes);
        let mut line = 1;
        let mut offset = 0;
        for range in ranges {
            line += (offset..range.start)
                .filter(|&index| {
                    self.bytes[index] == b'\n'
                        || (self.bytes[index] == b'\r' && self.bytes.get(index + 1) != Some(&b'\n'))
                })
                .count();
            offset = range.start;
            let before = std::str::from_utf8(&self.bytes[range.clone()])
                .map_err(|_| "El nombre original del símbolo debe estar en ASCII")?;
            edits.push(ProjectRenameEdit {
                path: self.path.to_owned(),
                version,
                encoding: self.encoding,
                range,
                line,
                before: before.into(),
            });
        }
        Ok(())
    }
}

fn caller_ranges(
    caller: &Source<'_>,
    directive: Include,
    target: &Source<'_>,
    cursor: usize,
    new_name: &str,
    target_ranges: &[Range<usize>],
) -> Result<Vec<Range<usize>>, String> {
    let insertion = directive.range;
    if !standalone(
        caller.bytes,
        &insertion,
        rename::bom_prefix(caller.bytes, caller.encoding),
    ) {
        return Err(UNSUPPORTED.into());
    }
    let bom = rename::bom_prefix(target.bytes, target.encoding);
    let body = &target.bytes[bom..];
    let expanded_len = caller.bytes.len() - insertion.len() + body.len();
    if expanded_len > MAX_FILE_BYTES {
        return Err(LIMIT.into());
    }
    let mut expanded = Vec::with_capacity(expanded_len);
    expanded.extend_from_slice(&caller.bytes[..insertion.start]);
    expanded.extend_from_slice(body);
    expanded.extend_from_slice(&caller.bytes[insertion.end..]);
    let ranges = rename::propose(
        &expanded,
        caller.encoding,
        insertion.start + cursor - bom,
        new_name,
    )
    .map_err(|error| format!("{}: {error}", caller.path.display()))?;
    let include_end = insertion.start + body.len();
    let mut confirmed = Vec::new();
    let mut external = Vec::new();
    for range in ranges {
        if range.end <= insertion.start {
            external.push(range);
        } else if range.start >= include_end {
            external.push(
                insertion.end + range.start - include_end..insertion.end + range.end - include_end,
            );
        } else if range.start >= insertion.start && range.end <= include_end {
            confirmed.push(bom + range.start - insertion.start..bom + range.end - insertion.start);
        } else {
            return Err("La referencia atraviesa el límite del include".into());
        }
    }
    if confirmed != target_ranges {
        return Err(
            "El símbolo tiene un conjunto de referencias distinto en el fuente que lo incluye"
                .into(),
        );
    }
    Ok(external)
}

/// Supports a shared symbol selected in its standalone .i declaration. Normal
/// document renames can use the existing provider for other file extensions.
pub fn propose(
    root: &Path,
    active_source: &Path,
    bytes: &[u8],
    encoding: TextEncoding,
    cursor: usize,
    new_name: &str,
    cancel: &Cancellation,
) -> Result<Option<Vec<ProjectRenameEdit>>, String> {
    review(
        root,
        active_source,
        bytes,
        encoding,
        cursor,
        new_name,
        || cancel.is_cancelled(),
    )
}

fn review(
    root: &Path,
    active_source: &Path,
    bytes: &[u8],
    encoding: TextEncoding,
    cursor: usize,
    new_name: &str,
    cancelled: impl Fn() -> bool,
) -> Result<Option<Vec<ProjectRenameEdit>>, String> {
    if !is_include(active_source) {
        return Ok(None);
    }
    if cancelled() {
        return Err(CANCELLED.into());
    }
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    let (path, disk) = read_in_root(&root, active_source, MAX_FILE_BYTES)
        .map_err(|error| format!("Guarda el include antes del renombrado de proyecto: {error}"))?;
    if disk != bytes {
        return Err("Guarda el include antes del renombrado de proyecto".into());
    }
    if !rename::is_project_declaration(bytes, encoding, cursor)? {
        return Err(
            "Coloca el cursor sobre la declaración de una rutina o variable global del include"
                .into(),
        );
    }
    let ranges = rename::propose(bytes, encoding, cursor, new_name)?;
    let target = Source {
        path: &path,
        bytes,
        encoding,
    };
    let mut edits = Vec::new();
    let mut dirs = VecDeque::from([root.clone()]);
    let mut directories = 1;
    let mut files = 0;
    let mut remaining = MAX_TOTAL_BYTES - disk.len();
    let mut found = false;
    while let Some(directory) = dirs.pop_front() {
        let mut listing = list_directory_in_root(&root, &directory, 2048, 512 * 1024, &cancelled)
            .map_err(|error| {
            if cancelled() {
                CANCELLED.into()
            } else {
                error.to_string()
            }
        })?;
        if listing.limited {
            return Err(LIMIT.into());
        }
        listing
            .entries
            .sort_unstable_by(|left, right| left.name.cmp(&right.name));
        for entry in listing.entries {
            if cancelled() {
                return Err(CANCELLED.into());
            }
            let path = directory.join(&entry.name);
            if entry.is_dir {
                if path.file_name().is_some_and(|name| name == ".cide") {
                    continue;
                }
                directories += 1;
                if directories > MAX_DIRECTORIES {
                    return Err(LIMIT.into());
                }
                dirs.push_back(path);
                continue;
            }
            if path == target.path
                || !(jc_languages::detect(&path.to_string_lossy()).id == "abl"
                    || extension(&path).eq_ignore_ascii_case("abl"))
            {
                continue;
            }
            files += 1;
            if files > MAX_FILES || remaining == 0 {
                return Err(LIMIT.into());
            }
            let (path, bytes) = read_in_root(&root, &path, MAX_FILE_BYTES.min(remaining))
                .map_err(|error| format!("{}: {error}", path.display()))?;
            remaining -= bytes.len();
            if path == target.path {
                continue;
            }
            let encoding = probe_encoding(&bytes, None).encoding.unwrap_or(encoding);
            let caller = Source {
                path: &path,
                bytes: &bytes,
                encoding,
            };
            let includes = directives(&bytes, encoding)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            if includes.is_empty() {
                continue;
            }
            let propath = project_propath_dirs(&root, &path)?;
            let mut includes_target = false;
            for include in &includes {
                if cancelled() {
                    return Err(CANCELLED.into());
                }
                includes_target |= resolve_include(&root, &propath, &include.name)? == target.path;
            }
            if !includes_target {
                continue;
            }
            if includes.len() != 1
                || !matches!(
                    extension(&path).to_ascii_lowercase().as_str(),
                    "p" | "w" | "abl"
                )
            {
                return Err(format!("{}: {UNSUPPORTED}", path.display()));
            }
            let include = includes.into_iter().next().unwrap();
            let caller_edits = caller_ranges(&caller, include, &target, cursor, new_name, &ranges)?;
            caller.edits(caller_edits, &mut edits)?;
            found = true;
        }
    }
    if !found {
        return Err(
            "Ningún fuente del proyecto incluye esta declaración mediante el PROPATH".into(),
        );
    }
    target.edits(ranges, &mut edits)?;
    edits.sort_unstable_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.range.start.cmp(&right.range.start))
    });
    Ok(Some(edits))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn resolves_first_propath_and_previews_only_the_bound_procedure() {
        let root =
            std::env::temp_dir().join(format!("cide-project-rename-abl-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        for dir in [".cide", "first", "second"] {
            fs::create_dir(root.join(dir)).unwrap();
        }
        let propath = root.join(".cide/propath");
        fs::write(&propath, "first\nsecond\n").unwrap();
        let include = b"PROCEDURE calculate:\r\n MESSAGE 'calculate'.\r\nEND PROCEDURE.";
        let caller = b"/* calculate */\r\n{shared.i}\r\nRUN Calculate.";
        let local = b"PROCEDURE calculate:\nEND PROCEDURE.\nRUN calculate.";
        let target = root.join("first/shared.i");
        fs::write(&target, include).unwrap();
        fs::write(root.join("second/shared.i"), include).unwrap();
        fs::write(root.join("caller.p"), caller).unwrap();
        fs::write(root.join("unrelated.p"), local).unwrap();
        let run = || {
            review(
                &root,
                &target,
                include,
                TextEncoding::Utf8,
                10,
                "recalculate",
                || false,
            )
        };
        let edits = run().unwrap().unwrap();
        assert_eq!(edits.len(), 2);
        assert_eq!(edits[0].path.file_name().unwrap(), "caller.p");
        assert_eq!(edits[0].before, "Calculate");
        assert_eq!(edits[0].version, ContentFingerprint::of(caller));
        assert_eq!(&caller[edits[0].range.clone()], b"Calculate");
        assert_eq!(edits[1].path, target.canonicalize().unwrap());
        assert_eq!(edits[1].before, "calculate");

        fs::write(&propath, "second\nfirst\n").unwrap();
        assert!(run().is_err());
        fs::write(&propath, "first\nsecond\n").unwrap();
        fs::write(
            root.join("caller.p"),
            b"{shared.i &ARG=yes}\nRUN calculate.",
        )
        .unwrap();
        assert!(run().is_err());
        fs::write(
            root.join("caller.p"),
            b"{shared.i}\nPROCEDURE calculate:\nEND PROCEDURE.",
        )
        .unwrap();
        assert!(run().is_err());
        for file in [
            "caller.p",
            "unrelated.p",
            "first/shared.i",
            "second/shared.i",
            ".cide/propath",
        ] {
            fs::remove_file(root.join(file)).unwrap();
        }
        for dir in ["first", "second", ".cide"] {
            fs::remove_dir(root.join(dir)).unwrap();
        }
        fs::remove_dir(root).unwrap();
    }
}
