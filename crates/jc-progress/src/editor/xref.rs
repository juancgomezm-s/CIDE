use super::{
    avm_path, bounded_read, decode, invoke, load_profile, project_propath, verify_source,
    EditorProfile, JobDir, XREF_LIMIT,
};
use crate::{Action, BridgeRequest, CompletionStatus};
use jc_app::{
    source_services::{SourceAction, SourceItem},
    Cancellation, TaskItem, TaskOutput, TaskSnapshot,
};
use jc_files::ContentFingerprint;
use jc_text::TextEncoding;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::{Path, PathBuf},
};

const MAX_PROGRAMS: usize = 32;
const MAX_FILES: usize = 128;
const MAX_DIRS: usize = 128;
const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_SOURCE: usize = 2 * 1024 * 1024;
const MAX_HITS: usize = 48;

struct Source {
    bytes: Vec<u8>,
    encoding: TextEncoding,
    avm_path: String,
    version: ContentFingerprint,
}

impl Source {
    fn new(path: &Path, bytes: Vec<u8>, fallback: TextEncoding) -> Self {
        let encoding = jc_text::probe_encoding(&bytes, None)
            .encoding
            .unwrap_or(fallback);
        let avm_path = avm_path(path).to_string_lossy().replace('\\', "/");
        let version = ContentFingerprint::of(&bytes);
        Self {
            bytes,
            encoding,
            avm_path,
            version,
        }
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Object {
    Table(String),
    Field(String, String),
    Index(String, String),
}

impl Object {
    fn table(&self) -> &str {
        match self {
            Self::Table(table) | Self::Field(table, _) | Self::Index(table, _) => table,
        }
    }
    fn name(&self) -> &str {
        match self {
            Self::Table(table) => table.rsplit('.').next().unwrap_or(table),
            Self::Field(_, name) | Self::Index(_, name) => name,
        }
    }
    fn matches(&self, row: &Self) -> bool {
        match self {
            Self::Table(table) => table == row.table(),
            _ => self == row,
        }
    }
    fn at_name(self, name: &str) -> Option<Self> {
        if self.name() == name {
            Some(self)
        } else if self.table().rsplit('.').next() == Some(name) {
            Some(Self::Table(self.table().to_owned()))
        } else {
            None
        }
    }
}

struct Record<'a> {
    path: &'a Path,
    line: u32,
    kind: &'static str,
    object: Object,
}

pub(super) fn compilable(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            ["p", "w", "cls"]
                .iter()
                .any(|kind| ext.eq_ignore_ascii_case(kind))
        })
}

pub(super) fn search(
    root: &Path,
    source: &Path,
    snapshot: &TaskSnapshot,
    cursor: usize,
    cancel: &Cancellation,
    output: &mut TaskOutput,
) -> Result<(), String> {
    let root = root
        .canonicalize()
        .map_err(|err| format!("Raíz del proyecto: {err}"))?;
    if !compilable(source) {
        return Err("XREF requiere un programa o una clase ABL guardados".into());
    }
    let source = verify_source(&root, source, snapshot.bytes())?;
    let Some((selected, name)) =
        super::schema::reference_name_at(snapshot.bytes(), snapshot.stamp.encoding, cursor)
    else {
        return message(
            output,
            "Coloca el cursor sobre una tabla, campo o índice de base de datos",
        );
    };
    let profile = load_profile(&root)?;
    if !code_identifier(snapshot.bytes(), snapshot.stamp.encoding, &selected) {
        return message(output, "Coloca el cursor sobre un identificador ABL");
    }
    let row = source_line(snapshot.bytes(), selected.start);
    let (sources, mut partial) = collect_sources(&root, &source, snapshot, cancel)?;
    let active = &sources[&source];
    let compiled = compile(&root, &source, active, &profile, cancel)?;
    let name = name.to_lowercase();
    let targets = records(&compiled.0, &sources, &profile)
        .filter(|entry| entry.path == source && entry.line == row)
        .filter_map(|entry| entry.object.at_name(&name))
        .collect::<BTreeSet<_>>();
    if targets.len() != 1 {
        return message(
            output,
            if targets.is_empty() {
                "El compilador no identificó una tabla, campo o índice con ese nombre en la línea"
            } else {
                "El símbolo coincide con varios objetos XREF en la línea"
            },
        );
    }
    let target = targets.into_iter().next().expect("single XREF target");
    let mut locations = BTreeMap::new();
    let mut truncated = false;
    let mut version = compiled.1;
    for (path, unit) in std::iter::once((&source, active)).chain(
        sources
            .iter()
            .filter(|(path, _)| **path != source && compilable(path)),
    ) {
        if cancel.is_cancelled() {
            return Err("XREF cancelado".into());
        }
        let other;
        let xref = if path == &source {
            &compiled.0
        } else {
            match compile(&root, path, unit, &profile, cancel) {
                Ok(result) => {
                    other = result;
                    if version != other.1 {
                        partial = true;
                        continue;
                    }
                    &other.0
                }
                Err(_) if cancel.is_cancelled() => return Err("XREF cancelado".into()),
                Err(_) => {
                    partial = true;
                    continue;
                }
            }
        };
        for entry in records(xref, &sources, &profile).filter(|entry| target.matches(&entry.object))
        {
            let key = (entry.path.to_owned(), entry.line);
            if locations.contains_key(&key) {
                continue;
            }
            if locations.len() == MAX_HITS {
                truncated = true;
                break;
            }
            locations.insert(key, entry.kind);
        }
        if truncated {
            break;
        }
    }
    for ((path, line), kind) in locations {
        let file = &sources[&path];
        let Some(offset) = line_offset(&file.bytes, line, file.encoding) else {
            partial = true;
            continue;
        };
        if path != source && verify_source(&root, &path, &file.bytes).is_err() {
            partial = true;
            continue;
        }
        let relative: String = path
            .strip_prefix(&root)
            .unwrap_or(&path)
            .to_string_lossy()
            .chars()
            .filter(|ch| !ch.is_control())
            .take(96)
            .collect();
        output
            .push(TaskItem::Source(SourceItem {
                range: if path == source { offset..offset } else { 0..0 },
                label: format!("{relative} · lín {line} · {kind}"),
                action: if path == source {
                    SourceAction::Navigate
                } else {
                    SourceAction::ProjectLocation {
                        path,
                        offset,
                        encoding: file.encoding,
                        version: file.version,
                    }
                },
            }))
            .map_err(|_| "XREF superó la cuota de resultados")?;
    }
    verify_source(&root, &source, snapshot.bytes())?;
    if truncated {
        version.push_str(" · primeras 48 coincidencias");
    }
    if partial {
        version.push_str(" · búsqueda parcial por archivos omitidos o cuotas");
    }
    message(output, &format!("OpenEdge {version}"))
}

fn message(output: &mut TaskOutput, text: &str) -> Result<(), String> {
    output
        .push(TaskItem::Message(text.into()))
        .map_err(|_| "XREF superó la cuota de salida".into())
}

fn collect_sources(
    root: &Path,
    active: &Path,
    snapshot: &TaskSnapshot,
    cancel: &Cancellation,
) -> Result<(BTreeMap<PathBuf, Source>, bool), String> {
    let encoding = snapshot.stamp.encoding;
    let mut remaining = MAX_BYTES - snapshot.bytes().len();
    let mut active_source = Source::new(active, snapshot.bytes().to_vec(), encoding);
    active_source.encoding = encoding;
    let mut sources = BTreeMap::from([(active.to_owned(), active_source)]);
    let mut dirs = VecDeque::from([root.to_owned()]);
    let (mut directories, mut programs, mut files, mut partial) = (1, 1, 1, false);
    'project: while let Some(dir) = dirs.pop_front() {
        let mut listing =
            match jc_files::list_directory_in_root(root, &dir, 2048, 512 * 1024, || {
                cancel.is_cancelled()
            }) {
                Ok(listing) => listing,
                Err(_) if cancel.is_cancelled() => return Err("XREF cancelado".into()),
                Err(_) => {
                    partial = true;
                    continue;
                }
            };
        partial |= listing.limited;
        listing.entries.sort_unstable_by(|a, b| a.name.cmp(&b.name));
        for entry in listing.entries {
            if cancel.is_cancelled() {
                return Err("XREF cancelado".into());
            }
            let path = dir.join(entry.name);
            if entry.is_dir {
                if matches!(
                    path.file_name().and_then(|name| name.to_str()),
                    Some(".cide" | "target" | "node_modules")
                ) {
                    continue;
                }
                if directories == MAX_DIRS {
                    partial = true;
                } else {
                    directories += 1;
                    dirs.push_back(path);
                }
                continue;
            }
            let program = compilable(&path);
            let include = path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("i"));
            if path == active || (!program && !include) {
                continue;
            }
            if files == MAX_FILES || remaining == 0 {
                partial = true;
                break 'project;
            }
            files += 1;
            if program && programs == MAX_PROGRAMS {
                partial = true;
                continue;
            }
            match jc_files::read_in_root(root, &path, MAX_SOURCE.min(remaining)) {
                Ok((path, bytes)) => {
                    programs += usize::from(program);
                    remaining -= bytes.len();
                    let file = Source::new(&path, bytes, encoding);
                    sources.insert(path, file);
                }
                Err(_) => partial = true,
            }
        }
    }
    Ok((sources, partial))
}

fn compile(
    root: &Path,
    path: &Path,
    source: &Source,
    profile: &EditorProfile,
    cancel: &Cancellation,
) -> Result<(String, String), String> {
    verify_source(root, path, &source.bytes)?;
    let job = JobDir::create().map_err(|err| format!("Directorio XREF: {err}"))?;
    let result = invoke(
        BridgeRequest {
            action: Action::Xref,
            source: Some(avm_path(path)),
            output_dir: avm_path(&job.path),
            propath: Some(project_propath(root, path)?),
        },
        root,
        profile,
        cancel,
    )?;
    verify_source(root, path, &source.bytes)?;
    let response = result.response?;
    if !result.status.success() || response.status != CompletionStatus::Ok {
        let detail = response
            .diagnostics
            .first()
            .map(|diag| decode(diag.message.as_bytes(), &response.cpinternal))
            .unwrap_or_else(|| "sin diagnóstico del compilador".into());
        return Err(format!("XREF no pudo compilar el archivo activo: {detail}"));
    }
    let bytes = bounded_read(&job.path.join("references.xrf"), XREF_LIMIT)?;
    Ok((
        decode(&bytes, &result.cpstream),
        response.avm_version.unwrap_or_default(),
    ))
}

fn records<'a>(
    xref: &'a str,
    sources: &'a BTreeMap<PathBuf, Source>,
    profile: &'a EditorProfile,
) -> impl Iterator<Item = Record<'a>> + 'a {
    xref.lines().filter_map(|line| {
        let line = line.replace('\\', "/");
        let (_, rest) = strip_path(&line, sources)?;
        let (path, rest) = strip_path(rest, sources)?;
        let mut fields = rest.split_ascii_whitespace();
        let line = fields.next()?.parse().ok()?;
        let kind = match fields.next()? {
            "ACCESS" => "Lectura",
            "UPDATE" => "Escritura",
            "SEARCH" => "Búsqueda",
            "REFERENCE" => "Referencia",
            _ => return None,
        };
        let table = fields.next()?;
        let (db, table) = table.split_once('.')?;
        if !valid_name(db) || !valid_name(table) {
            return None;
        }
        let db = profile
            .aliases
            .iter()
            .find(|alias| alias.name.eq_ignore_ascii_case(db))
            .map_or(db, |alias| alias.database.as_str());
        let table = format!("{db}.{table}").to_lowercase();
        let member = fields.next();
        if member
            .into_iter()
            .chain(fields)
            .any(|word| matches!(word, "TEMPTABLE" | "WORKFILE"))
        {
            return None;
        }
        let object = match kind {
            "Lectura" | "Escritura" => Object::Field(
                table,
                member.filter(|name| valid_name(name))?.to_lowercase(),
            ),
            "Búsqueda" if member.is_some_and(valid_name) => {
                Object::Index(table, member?.to_lowercase())
            }
            _ => Object::Table(table),
        };
        Some(Record {
            path,
            line,
            kind,
            object,
        })
    })
}

fn strip_path<'a, 'b>(
    text: &'a str,
    sources: &'b BTreeMap<PathBuf, Source>,
) -> Option<(&'b Path, &'a str)> {
    let text = text.trim_start_matches([' ', '\t']);
    if let Some(quoted) = text.strip_prefix('"') {
        let (name, rest) = quoted.split_once('"')?;
        if !rest.as_bytes().first().is_some_and(u8::is_ascii_whitespace) {
            return None;
        }
        let path = sources
            .iter()
            .find(|(_, file)| same_avm_path(name, &file.avm_path))?
            .0;
        return Some((path, rest));
    }
    let (path, length) = sources
        .iter()
        .filter_map(|(path, file)| {
            let len = file.avm_path.len();
            let prefix = text.get(..len)?;
            (same_avm_path(prefix, &file.avm_path)
                && text
                    .as_bytes()
                    .get(len)
                    .is_some_and(u8::is_ascii_whitespace))
            .then_some((path.as_path(), len))
        })
        .max_by_key(|(_, len)| *len)?;
    Some((path, &text[length..]))
}

fn same_avm_path(left: &str, right: &str) -> bool {
    if cfg!(windows) {
        left.eq_ignore_ascii_case(right)
    } else {
        left == right
    }
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '-'))
}

fn code_identifier(bytes: &[u8], encoding: TextEncoding, range: &std::ops::Range<usize>) -> bool {
    use jc_languages::syntax::{scan, LexerState, SyntaxKind, SyntaxProfile};
    let mut lexer = LexerState::default();
    let mut found = false;
    let bom = usize::from(encoding == TextEncoding::Utf8 && bytes.starts_with(b"\xef\xbb\xbf")) * 3;
    scan(
        SyntaxProfile::Abl,
        &mut lexer,
        &bytes[bom..range.end],
        bom,
        true,
        |span| {
            if span.source == *range && matches!(span.kind, SyntaxKind::Plain | SyntaxKind::Keyword)
            {
                found = true;
            }
        },
    );
    found
}

fn source_line(bytes: &[u8], offset: usize) -> u32 {
    1 + bytes[..offset]
        .iter()
        .filter(|&&byte| byte == b'\n')
        .count() as u32
}

fn line_offset(bytes: &[u8], line: u32, encoding: TextEncoding) -> Option<usize> {
    match line {
        0 => None,
        1 => Some(
            if encoding == TextEncoding::Utf8 && bytes.starts_with(b"\xef\xbb\xbf") {
                3
            } else {
                0
            },
        ),
        _ => bytes
            .iter()
            .enumerate()
            .filter(|(_, byte)| **byte == b'\n')
            .nth((line - 2) as usize)
            .map(|(index, _)| index + 1),
    }
}
