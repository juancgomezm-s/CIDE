//! Reviewed project-wide rename with physical source coordinates.
mod indirect;
mod parameters;

use crate::{editor::project_propath_dirs_for, rename};
use jc_app::{
    source_services::{load_propath, ProjectRenameEdit},
    Cancellation,
};
use jc_files::{list_directory_in_root, read_in_root, ContentFingerprint};
use jc_languages::syntax::{scan, LexerState, SyntaxKind, SyntaxProfile};
use jc_text::{probe_encoding, TextEncoding};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    io::ErrorKind,
    ops::Range,
    path::{Path, PathBuf},
};

const MAX_FILE_BYTES: usize = 256 * 1024;
const MAX_TOTAL_BYTES: usize = 8 * 1024 * 1024;
const MAX_FILES: usize = 256;
const MAX_DIRECTORIES: usize = 64;
const MAX_DIRECTIVES: usize = 256;
const MAX_INCLUDE_DEPTH: usize = 16;
const MAX_EXPANDED_BYTES: usize = 16 * 1024 * 1024;
const MAX_EXPANSION_NODES: usize = 4096;
const MAX_EDITS: usize = 64;
const MAX_SEGMENTS: usize = 16_384;
const LIMIT: &str = "El proyecto supera los límites de la revisión de renombrado ABL";
const CANCELLED: &str = "Se canceló la revisión de renombrado ABL";
const UNSUPPORTED: &str = "El renombrado requiere inclusiones estáticas en su propio renglón";

struct Macro {
    range: Range<usize>,
    kind: parameters::Directive,
}

fn is_include(path: &Path) -> bool {
    extension(path).eq_ignore_ascii_case("i")
}

fn extension(path: &Path) -> &str {
    path.extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
}

fn directives(bytes: &[u8], encoding: TextEncoding) -> Result<Vec<Macro>, String> {
    let mut state = LexerState::default();
    let mut starts = Vec::new();
    let mut too_many = false;
    let offset = rename::bom_prefix(bytes, encoding);
    scan(
        SyntaxProfile::Abl,
        &mut state,
        &bytes[offset..],
        offset,
        true,
        |span| {
            if too_many || !matches!(span.kind, SyntaxKind::Preprocessor | SyntaxKind::String) {
                return;
            }
            for position in span.source {
                if bytes[position] == b'{' {
                    let escapes = bytes[..position]
                        .iter()
                        .rev()
                        .take_while(|&&byte| byte == b'~')
                        .count();
                    if escapes % 2 != 0 {
                        continue;
                    }
                    if starts.len() == MAX_DIRECTIVES {
                        too_many = true;
                        return;
                    }
                    starts.push(position);
                }
            }
        },
    );
    if state.limited() || too_many {
        return Err(LIMIT.into());
    }
    let mut result = Vec::new();
    let mut previous = 0;
    for start in starts {
        if start < previous {
            continue;
        }
        let end = parameters::end(bytes, start)?;
        let range = start..end;
        let kind = parameters::parse(bytes, range.clone())?;
        result.push(Macro { range, kind });
        previous = end;
    }
    Ok(result)
}

fn resolve_include(root: &Path, dirs: &[PathBuf], name: &str) -> Result<PathBuf, String> {
    for dir in dirs {
        // The first PROPATH candidate owns the binding.
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

struct Source {
    bytes: Vec<u8>,
    encoding: TextEncoding,
    includes: Vec<Macro>,
}

impl Source {
    fn edits(
        &self,
        path: &Path,
        ranges: &BTreeSet<(usize, usize)>,
        edits: &mut Vec<ProjectRenameEdit>,
    ) -> Result<(), String> {
        if edits.len().saturating_add(ranges.len()) > MAX_EDITS {
            return Err(LIMIT.into());
        }
        let version = ContentFingerprint::of(&self.bytes);
        let mut line = 1;
        let mut offset = 0;
        for &(start, end) in ranges {
            let range = start..end;
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
                path: path.to_owned(),
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

type Ranges = BTreeSet<(usize, usize)>;
type Changes<'a> = BTreeMap<&'a Path, Ranges>;

#[derive(Clone)]
struct Segment<'a> {
    path: &'a Path,
    from: usize,
    expanded: Range<usize>,
    selection: Option<usize>,
}

#[derive(Default, Clone)]
struct Expansion<'a> {
    bytes: Vec<u8>,
    segments: Vec<Segment<'a>>,
}

impl<'a> Expansion<'a> {
    fn append(
        &mut self,
        path: &'a Path,
        source: &[u8],
        range: Range<usize>,
        selection: Option<usize>,
    ) -> Result<(), String> {
        let start = self.bytes.len();
        if start + range.len() > MAX_FILE_BYTES || self.segments.len() == MAX_SEGMENTS {
            return Err(LIMIT.into());
        }
        if !range.is_empty() {
            self.bytes.extend_from_slice(&source[range.clone()]);
            self.segments.push(Segment {
                path,
                from: range.start,
                expanded: start..self.bytes.len(),
                selection: selection
                    .filter(|cursor| range.contains(cursor))
                    .map(|cursor| start + cursor - range.start),
            });
        }
        Ok(())
    }

    fn substitute(&mut self, value: &Self, selected: bool) -> Result<(), String> {
        let start = self.bytes.len();
        if start + value.bytes.len() > MAX_FILE_BYTES
            || self.segments.len().saturating_add(value.segments.len()) > MAX_SEGMENTS
        {
            return Err(LIMIT.into());
        }
        self.bytes.extend_from_slice(&value.bytes);
        self.segments.extend(value.segments.iter().map(|segment| {
            Segment {
                path: segment.path,
                from: segment.from,
                expanded: start + segment.expanded.start..start + segment.expanded.end,
                selection: segment
                    .selection
                    .or_else(|| selected.then_some(segment.expanded.start))
                    .map(|position| start + position),
            }
        }));
        Ok(())
    }

    fn space(&mut self) -> Result<(), String> {
        if self.bytes.len() == MAX_FILE_BYTES {
            return Err(LIMIT.into());
        }
        self.bytes.push(b' ');
        Ok(())
    }

    fn locate(&self, range: Range<usize>) -> Result<(&'a Path, (usize, usize)), String> {
        let index = self
            .segments
            .partition_point(|segment| segment.expanded.end <= range.start);
        let segment = self
            .segments
            .get(index)
            .filter(|segment| {
                segment.expanded.start <= range.start && range.end <= segment.expanded.end
            })
            .ok_or("El símbolo combina fragmentos de parámetros y no tiene un reemplazo único")?;
        let start = segment.from + range.start - segment.expanded.start;
        Ok((segment.path, (start, start + range.len())))
    }
}

type Arguments<'a> = BTreeMap<String, Expansion<'a>>;

struct Project<'a> {
    root: &'a Path,
    sources: &'a BTreeMap<PathBuf, Source>,
    cancelled: &'a dyn Fn() -> bool,
    nodes: usize,
    target: &'a Path,
    cursor: usize,
}

impl<'a> Project<'a> {
    fn step(&mut self) -> Result<(), String> {
        if (self.cancelled)() {
            return Err(CANCELLED.into());
        }
        self.nodes += 1;
        if self.nodes > MAX_EXPANSION_NODES {
            return Err(LIMIT.into());
        }
        Ok(())
    }

    fn selection(&self, path: &Path) -> Option<usize> {
        (path == self.target).then_some(self.cursor)
    }

    fn reference(
        &mut self,
        path: &'a Path,
        range: &Range<usize>,
        name: &str,
        arguments: &Arguments<'a>,
        expansion: &mut Expansion<'a>,
    ) -> Result<(), String> {
        self.step()?;
        let value = arguments.get(name).ok_or_else(|| {
            format!(
                "El parámetro {{{name}}} no tiene un valor explícito en {}",
                path.display()
            )
        })?;
        expansion.substitute(
            value,
            self.selection(path)
                .is_some_and(|cursor| range.contains(&cursor)),
        )
    }

    fn argument(
        &mut self,
        path: &'a Path,
        range: Range<usize>,
        arguments: &Arguments<'a>,
    ) -> Result<Expansion<'a>, String> {
        let source = &self.sources[path].bytes;
        let mut result = Expansion::default();
        let mut from = range.start;
        let mut position = from;
        while position < range.end {
            if source[position] != b'{' {
                position += 1;
                continue;
            }
            let end = parameters::end(source, position)?;
            if end > range.end {
                return Err("La referencia de un parámetro atraviesa su valor".into());
            }
            let reference = position..end;
            let parameters::Directive::Reference(name) =
                parameters::parse(source, reference.clone())?
            else {
                return Err("El valor de un parámetro contiene otra inclusión".into());
            };
            result.append(path, source, from..position, self.selection(path))?;
            self.reference(path, &reference, &name, arguments, &mut result)?;
            if result.bytes.len() > parameters::MAX_VALUE_BYTES {
                return Err("El valor expandido de un parámetro supera 4096 bytes".into());
            }
            position = end;
            from = end;
        }
        result.append(path, source, from..range.end, self.selection(path))?;
        if result.bytes.len() > parameters::MAX_VALUE_BYTES {
            return Err("El valor expandido de un parámetro supera 4096 bytes".into());
        }
        Ok(result)
    }

    fn expand(
        &mut self,
        path: &'a Path,
        dirs: &[PathBuf],
        chain: &mut Vec<&'a Path>,
        arguments: &Arguments<'a>,
        expansion: &mut Expansion<'a>,
    ) -> Result<(), String> {
        self.step()?;
        if chain.contains(&path) {
            return Err(format!(
                "Inclusión circular por PROPATH: {}",
                path.display()
            ));
        }
        if chain.len() == MAX_INCLUDE_DEPTH {
            return Err(LIMIT.into());
        }
        let source = &self.sources[path];
        let bom = rename::bom_prefix(&source.bytes, source.encoding);
        let mut from = if chain.is_empty() { 0 } else { bom };
        chain.push(path);
        for directive in &source.includes {
            expansion.append(
                path,
                &source.bytes,
                from..directive.range.start,
                self.selection(path),
            )?;
            match &directive.kind {
                parameters::Directive::Reference(name) => {
                    self.reference(path, &directive.range, name, arguments, expansion)?;
                }
                parameters::Directive::Include {
                    name,
                    arguments: passed,
                } => {
                    if !standalone(&source.bytes, &directive.range, bom) {
                        return Err(format!("{}: {UNSUPPORTED}", path.display()));
                    }
                    let name = std::str::from_utf8(&source.bytes[name.clone()])
                        .map_err(|_| UNSUPPORTED)?;
                    let resolved = resolve_include(self.root, dirs, name)?;
                    let (target, _) = self.sources.get_key_value(&resolved).ok_or_else(|| {
                        format!(
                            "El include queda fuera de las fuentes revisadas: {}",
                            resolved.display()
                        )
                    })?;
                    let mut local = Arguments::new();
                    for argument in passed {
                        local.insert(
                            argument.name.clone(),
                            self.argument(path, argument.value.clone(), arguments)?,
                        );
                    }
                    self.expand(target, dirs, chain, &local, expansion)?;
                    // The AVM inserts a space after each included file.
                    expansion.space()?;
                }
            }
            from = directive.range.end;
        }
        expansion.append(
            path,
            &source.bytes,
            from..source.bytes.len(),
            self.selection(path),
        )?;
        chain.pop();
        Ok(())
    }
}

struct Unit<'a> {
    expected: Vec<u8>,
    replacements: Ranges,
    dirs: Vec<PathBuf>,
    declaration: Option<rename::ProjectDeclaration>,
    changes: Changes<'a>,
    calls: Vec<BoundCall<'a>>,
}

struct BoundCall<'a> {
    program: PathBuf,
    source: &'a Path,
    range: (usize, usize),
    function: bool,
    expanded: (usize, usize),
}

fn compilation_unit(path: &Path) -> bool {
    ["p", "w", "abl", "cls"]
        .iter()
        .any(|ext| extension(path).eq_ignore_ascii_case(ext))
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
    let (target_path, disk) = read_in_root(&root, active_source, MAX_FILE_BYTES)
        .map_err(|error| format!("Guarda el include antes del renombrado de proyecto: {error}"))?;
    if disk != bytes {
        return Err("Guarda el include antes del renombrado de proyecto".into());
    }
    let mut sources = BTreeMap::new();
    let mut remaining = MAX_TOTAL_BYTES - disk.len();
    let includes = directives(&disk, encoding)?;
    sources.insert(
        target_path.clone(),
        Source {
            bytes: disk,
            encoding,
            includes,
        },
    );
    let mut dirs = VecDeque::from([root.clone()]);
    let mut directories = 1;
    let mut files = 0;
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
            if path == target_path || !(compilation_unit(&path) || is_include(&path)) {
                continue;
            }
            files += 1;
            if files > MAX_FILES || remaining == 0 {
                return Err(LIMIT.into());
            }
            let (path, bytes) = read_in_root(&root, &path, MAX_FILE_BYTES.min(remaining))
                .map_err(|error| format!("{}: {error}", path.display()))?;
            if path == target_path {
                continue;
            }
            remaining -= bytes.len();
            let encoding = probe_encoding(&bytes, None).encoding.unwrap_or(encoding);
            let includes = directives(&bytes, encoding)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            sources.insert(
                path,
                Source {
                    bytes,
                    encoding,
                    includes,
                },
            );
        }
    }
    analyze(&root, &sources, &target_path, cursor, new_name, &cancelled).map(Some)
}

fn analyze(
    root: &Path,
    sources: &BTreeMap<PathBuf, Source>,
    target: &Path,
    cursor: usize,
    new_name: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<ProjectRenameEdit>, String> {
    let mut project = Project {
        root,
        sources,
        cancelled,
        nodes: 0,
        target,
        cursor,
    };
    let mut units = BTreeMap::new();
    let mut remaining = MAX_EXPANDED_BYTES;
    let mut selected = None;
    let configured = load_propath(root)?;
    for (path, source) in sources.iter().filter(|(path, _)| compilation_unit(path)) {
        let dirs = project_propath_dirs_for(root, path, configured.as_deref())?;
        let mut expanded = Expansion::default();
        project
            .expand(
                path,
                &dirs,
                &mut Vec::new(),
                &Arguments::new(),
                &mut expanded,
            )
            .map_err(|error| format!("{}: {error}", path.display()))?;
        remaining = remaining.checked_sub(expanded.bytes.len()).ok_or(LIMIT)?;
        let lowered = indirect::lower(root, &dirs, &expanded.bytes, source.encoding)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        let mut unit = Unit {
            expected: expanded.bytes.clone(),
            replacements: Ranges::new(),
            dirs,
            declaration: None,
            changes: Changes::new(),
            calls: Vec::new(),
        };
        let mut selections = expanded
            .segments
            .iter()
            .filter_map(|segment| segment.selection);
        if let Some(position) = selections.next() {
            if selections.next().is_some() {
                return Err(
                    "La declaración seleccionada tiene varias instancias en el mismo programa"
                        .into(),
                );
            }
            let (kind, name) = rename::project_declaration(
                &lowered.bytes,
                source.encoding,
                position,
            )
            .map_err(|error| format!("{}: {error}", path.display()))?
            .ok_or(
                "Coloca el cursor sobre la declaración de una rutina o variable global del include",
            )?;
            let spelling = expanded.bytes[name].to_ascii_uppercase();
            if selected
                .as_ref()
                .is_some_and(|(previous_kind, previous_name)| {
                    previous_kind != &kind || previous_name != &spelling
                })
            {
                return Err("La declaración cambia de significado entre programas".into());
            }
            selected = Some((kind, spelling));
            unit.declaration = Some(kind);
            let ranges = rename::propose(&lowered.bytes, source.encoding, position, new_name)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            if ranges.len() > MAX_EDITS {
                return Err(LIMIT.into());
            }
            for range in ranges {
                unit.replacements.insert((range.start, range.end));
                let (path, range) = expanded.locate(range)?;
                unit.changes.entry(path).or_default().insert(range);
            }
        }
        for call in lowered.calls {
            let (source, range) = expanded.locate(call.name.clone())?;
            unit.calls.push(BoundCall {
                program: call.program,
                source,
                range,
                function: call.function,
                expanded: (call.name.start, call.name.end),
            });
        }
        units.insert(path.as_path(), unit);
    }
    let (selected_kind, old_name) = selected
        .ok_or("Ningún programa del proyecto incluye esta declaración mediante el PROPATH")?;
    let contexts: BTreeMap<_, _> = units
        .iter()
        .map(|(&path, unit)| (path, (unit.declaration, unit.dirs.clone())))
        .collect();
    let mut changes = Changes::new();
    for unit in units.values_mut() {
        if cancelled() {
            return Err(CANCELLED.into());
        }
        for call in &unit.calls {
            let kind = if call.function {
                rename::ProjectDeclaration::Function
            } else {
                rename::ProjectDeclaration::Procedure
            };
            let (start, end) = call.range;
            let name = &sources[call.source].bytes[start..end];
            let is_old = name.eq_ignore_ascii_case(&old_name);
            let is_new = name.eq_ignore_ascii_case(new_name.as_bytes());
            if kind != selected_kind || !(is_old || is_new) {
                continue;
            }
            let (declaration, dirs) = contexts
                .get(call.program.as_path())
                .ok_or("El programa de una llamada indirecta queda fuera de la revisión")?;
            if dirs != &unit.dirs {
                return Err(
                    "El PROPATH del programa persistente difiere del PROPATH de la llamada".into(),
                );
            }
            if *declaration != Some(kind) {
                continue;
            }
            if is_old {
                unit.replacements.insert(call.expanded);
                unit.changes
                    .entry(call.source)
                    .or_default()
                    .insert(call.range);
            } else {
                return Err(
                    "El nombre nuevo cambiaría el destino de otra llamada indirecta".into(),
                );
            }
        }
        for (&path, ranges) in &unit.changes {
            changes.entry(path).or_default().extend(ranges);
        }
    }
    verify_expansion(root, sources, &units, &changes, new_name, cancelled)?;
    let mut edits = Vec::new();
    for (path, ranges) in changes {
        sources[path].edits(path, &ranges, &mut edits)?;
    }
    Ok(edits)
}

fn replace_ranges(bytes: &[u8], ranges: &Ranges, name: &str) -> Result<Vec<u8>, String> {
    let mut result = Vec::new();
    let mut from = 0;
    for &(start, end) in ranges {
        if start < from || start >= end || end > bytes.len() {
            return Err("Los reemplazos del proyecto se solapan o cambiaron de posición".into());
        }
        result.extend_from_slice(&bytes[from..start]);
        result.extend_from_slice(name.as_bytes());
        from = end;
    }
    result.extend_from_slice(&bytes[from..]);
    if result.len() > MAX_FILE_BYTES {
        return Err(LIMIT.into());
    }
    Ok(result)
}

fn verify_expansion(
    root: &Path,
    sources: &BTreeMap<PathBuf, Source>,
    units: &BTreeMap<&Path, Unit<'_>>,
    changes: &Changes<'_>,
    name: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<(), String> {
    let mut updated = BTreeMap::new();
    for (path, source) in sources {
        if cancelled() {
            return Err(CANCELLED.into());
        }
        let bytes = match changes.get(path.as_path()) {
            Some(ranges) => replace_ranges(&source.bytes, ranges, name)?,
            None => source.bytes.clone(),
        };
        let includes = directives(&bytes, source.encoding)?;
        updated.insert(
            path.clone(),
            Source {
                bytes,
                encoding: source.encoding,
                includes,
            },
        );
    }
    let mut project = Project {
        root,
        sources: &updated,
        cancelled,
        nodes: 0,
        target: Path::new(""),
        cursor: 0,
    };
    for (path, unit) in units {
        let expected = replace_ranges(&unit.expected, &unit.replacements, name)?;
        let mut actual = Expansion::default();
        project.expand(
            path,
            &unit.dirs,
            &mut Vec::new(),
            &Arguments::new(),
            &mut actual,
        )?;
        if actual.bytes != expected {
            return Err(format!(
                "El símbolo se vincula de forma distinta o un parámetro cambia otros usos en {}",
                path.display()
            ));
        }
    }
    Ok(())
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
        let bridge = root.join("first/bridge.i");
        let call_fragment = root.join("first/call.i");
        fs::write(&bridge, "{shared.i}\n").unwrap();
        fs::write(&call_fragment, "RUN calculate.\n").unwrap();
        fs::write(root.join("caller.p"), "{bridge.i}\n{call.i}\n").unwrap();
        let edits = run().unwrap().unwrap();
        assert_eq!(edits.len(), 2);
        assert!(edits
            .iter()
            .any(|edit| edit.path.ends_with("first/call.i") && edit.line == 1));
        fs::write(root.join("unrelated.p"), "{../second/shared.i}\n{call.i}\n").unwrap();
        assert!(run().is_err()); // traversal may not escape a PROPATH entry
        fs::write(root.join("second/alternate.i"), include).unwrap();
        fs::write(root.join("unrelated.p"), "{alternate.i}\n{call.i}\n").unwrap();
        assert!(run().unwrap_err().contains("vincula de forma distinta"));
        fs::write(root.join("unrelated.p"), local).unwrap();
        fs::write(&bridge, "{bridge.i}\n").unwrap();
        assert!(run().unwrap_err().contains("Inclusión circular"));

        fs::write(
            root.join("caller.p"),
            b"{shared.i &ARG=yes}\nRUN calculate.",
        )
        .unwrap();
        assert_eq!(run().unwrap().unwrap().len(), 2);
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
            "first/bridge.i",
            "first/call.i",
            "second/shared.i",
            "second/alternate.i",
            ".cide/propath",
        ] {
            fs::remove_file(root.join(file)).unwrap();
        }
        for dir in ["first", "second", ".cide"] {
            fs::remove_dir(root.join(dir)).unwrap();
        }
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn parameterized_includes_keep_physical_arguments_and_reject_extra_effects() {
        let root = std::env::temp_dir().join(format!(
            "cide-rename-parameter-project-{}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let target = root.join("shared.i");
        let bridge = root.join("bridge.i");
        let main = root.join("main.p");
        let template = "PROCEDURE {&operation}:\nEND PROCEDURE.\n";
        let caller = "{bridge.i \"worker\"}\nRUN worker.\n";
        fs::write(&target, template).unwrap();
        fs::write(&bridge, "{shared.i &operation={1}}\n").unwrap();
        fs::write(&main, caller).unwrap();
        let run = |template: &str| {
            review(
                &root,
                &target,
                template.as_bytes(),
                TextEncoding::Utf8,
                12,
                "newWorker",
                || false,
            )
        };
        let edits = run(template).unwrap().unwrap();
        assert_eq!(edits.len(), 2);
        assert!(edits
            .iter()
            .all(|edit| edit.path.ends_with("main.p") && edit.before == "worker"));
        assert_eq!(&caller.as_bytes()[edits[0].range.clone()], b"worker");
        assert_eq!(caller.as_bytes()[edits[0].range.start - 1], b'"');
        assert_eq!((edits[0].line, edits[1].line), (1, 2));

        let other = root.join("other.p");
        fs::write(&other, "{bridge.i \"different\"}\nRUN different.\n").unwrap();
        assert!(run(template).unwrap_err().contains("cambia de significado"));
        fs::remove_file(&other).unwrap();
        fs::write(
            &main,
            format!("{caller}{{bridge.i \"different\"}}\nRUN different.\n"),
        )
        .unwrap();
        assert!(run(template).unwrap_err().contains("varias instancias"));
        fs::write(&main, caller).unwrap();

        let text_effect = "PROCEDURE {&operation}:\nMESSAGE \"{&operation}\".\nEND PROCEDURE.\n";
        fs::write(&target, text_effect).unwrap();
        assert!(run(text_effect).unwrap_err().contains("cambia otros usos"));
        fs::write(&target, template).unwrap();

        fs::write(&bridge, "{shared.i &operation=\"{&name}\"}\n").unwrap();
        fs::write(&main, "{bridge.i &name=\"worker\"}\nRUN worker.\n").unwrap();
        assert_eq!(run(template).unwrap().unwrap().len(), 2);
        fs::write(&bridge, "{shared.i}\n").unwrap();
        assert!(run(template)
            .unwrap_err()
            .contains("no tiene un valor explícito"));

        for path in [main, bridge, target] {
            fs::remove_file(path).unwrap();
        }
        fs::remove_dir(&root).unwrap();
    }

    #[test]
    fn indirect_calls_follow_only_the_program_that_owns_the_include() {
        let root = std::env::temp_dir().join(format!(
            "cide-rename-indirect-project-{}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let target = root.join("shared.i");
        let declaration = b"PROCEDURE worker:\nEND PROCEDURE.\n";
        fs::write(&target, declaration).unwrap();
        fs::write(root.join("one.p"), "{shared.i}\n").unwrap();
        fs::write(root.join("other.p"), declaration).unwrap();
        let driver = concat!(
            "DEFINE VARIABLE oneHandle AS HANDLE NO-UNDO.\n",
            "DEFINE VARIABLE otherHandle AS HANDLE NO-UNDO.\n",
            "RUN one.p PERSISTENT SET oneHandle.\n",
            "RUN other.p PERSISTENT SET otherHandle.\n",
            "RUN VALUE('worker') IN oneHandle.\n",
            "RUN worker IN otherHandle.\n",
        );
        fs::write(root.join("driver.p"), driver).unwrap();
        let review = || {
            super::review(
                &root,
                &target,
                declaration,
                TextEncoding::Utf8,
                11,
                "newWorker",
                || false,
            )
        };
        let edits = review().unwrap().unwrap();
        assert_eq!(edits.len(), 2);
        let call = edits
            .iter()
            .find(|edit| edit.path.ends_with("driver.p"))
            .unwrap();
        assert_eq!(&driver.as_bytes()[call.range.clone()], b"worker");
        assert_eq!(call.line, 5);
        fs::write(
            root.join("driver.p"),
            driver.replace("RUN VALUE('worker')", "RUN VALUE('newWorker')"),
        )
        .unwrap();
        assert!(review().unwrap_err().contains("nombre nuevo"));
        for file in ["shared.i", "one.p", "other.p", "driver.p"] {
            fs::remove_file(root.join(file)).unwrap();
        }
        fs::remove_dir(&root).unwrap();
    }
}
