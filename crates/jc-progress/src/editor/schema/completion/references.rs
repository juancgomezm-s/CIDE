use super::{fold, identifier, Catalog, Context, IncludeFile, Includes, Site, Symbols, Target};
use jc_app::{Cancellation, TaskFailure, TaskItem};
use jc_text::{decode_fragment, TextEncoding};
use std::{
    collections::{BTreeSet, VecDeque},
    ops::Range,
    path::{Path, PathBuf},
    sync::Arc,
};

const MAX_CANDIDATES: usize = 2048;
const MAX_RESULTS: usize = 48;
const MAX_PROJECT_FILES: usize = 512;
const MAX_PROJECT_DIRS: usize = 128;
const MAX_PROJECT_BYTES: usize = 16 * 1024 * 1024;
const MAX_SOURCE_BYTES: usize = 2 * 1024 * 1024;

pub(super) struct ReferenceScan {
    key: String,
    selected: Range<usize>,
    declarations: Vec<Declaration>,
    occurrences: Vec<Occurrence>,
    partial: bool,
}

struct Declaration {
    site: Site,
    identity: Site,
    global: bool,
}

#[derive(Default)]
struct Results {
    items: Vec<TaskItem>,
    locations: BTreeSet<(Option<PathBuf>, usize)>,
    truncated: bool,
    partial: bool,
    project_root: Option<PathBuf>,
}

impl Results {
    fn finish(mut self) -> Vec<TaskItem> {
        let notice = match (self.truncated, self.partial) {
            (true, true) => Some("Primeras 48 · búsqueda parcial"),
            (true, false) => Some("Se muestran las primeras 48"),
            (false, true) => Some("Búsqueda parcial por archivos omitidos o cuotas"),
            (false, false) => None,
        };
        if let Some(notice) = notice {
            self.items.push(TaskItem::Message(notice.into()));
        }
        self.items
    }
}

struct Occurrence {
    site: Site,
    binding: Option<Site>,
    forward: Option<bool>,
    line: usize,
}

impl ReferenceScan {
    pub(super) fn key(&self) -> &str {
        &self.key
    }

    pub(super) fn mark_partial(&mut self) {
        self.partial = true;
    }

    pub(super) fn matches(&self, key: &str) -> bool {
        self.key == key
    }

    pub(super) fn matches_token(&self, context: &Context<'_>) -> bool {
        let Some(token) = context.tokens.last() else {
            return false;
        };
        if !token.kind.is_word() {
            return false;
        }
        let bytes = context.bytes(token);
        if bytes.is_ascii() {
            return bytes.eq_ignore_ascii_case(self.key.as_bytes());
        }
        context
            .word(token)
            .is_some_and(|word| self.matches(&fold(&word)))
    }

    pub(super) fn declare(
        &mut self,
        site: Site,
        identity: Site,
        global: bool,
    ) -> Result<(), TaskFailure> {
        if self.declarations.len() == MAX_CANDIDATES {
            return Err(TaskFailure::LimitExceeded);
        }
        self.declarations.push(Declaration {
            site,
            identity,
            global,
        });
        Ok(())
    }

    pub(super) fn observe(
        &mut self,
        context: &mut Context<'_>,
        binding: Option<Site>,
        forward: Option<bool>,
    ) -> Result<(), TaskFailure> {
        if binding.is_none() && forward.is_none() {
            return Ok(());
        }
        if self.occurrences.len() == MAX_CANDIDATES {
            return Err(TaskFailure::LimitExceeded);
        }
        let site = context.site(context.tokens.last().expect("matched identifier"));
        let (start, line) = &mut context.reference_line;
        for (offset, byte) in context.bytes[*start..site.offset].iter().enumerate() {
            if *byte == b'\n'
                || (*byte == b'\r' && context.bytes.get(*start + offset + 1) != Some(&b'\n'))
            {
                *line += 1;
            }
        }
        *start = site.offset;
        self.occurrences.push(Occurrence {
            site,
            binding,
            forward,
            line: *line,
        });
        Ok(())
    }

    fn resolve(&self, occurrence: &Occurrence, symbols: &Symbols) -> Option<Site> {
        occurrence.binding.clone().or_else(|| {
            symbols
                .routine_definition(&self.key, occurrence.forward?)
                .map(|found| found.site().clone())
        })
    }

    fn identity(&self, symbols: &Symbols) -> Option<Site> {
        let selected = |site: &Site| site.file.is_none() && site.offset == self.selected.start;
        self.declarations
            .iter()
            .find(|entry| selected(&entry.site))
            .map(|entry| entry.identity.clone())
            .or_else(|| {
                self.occurrences
                    .iter()
                    .find(|entry| selected(&entry.site))
                    .and_then(|entry| self.resolve(entry, symbols))
            })
    }

    fn is_global(&self, identity: &Site) -> bool {
        self.declarations
            .iter()
            .any(|entry| entry.global && &entry.identity == identity)
    }

    fn append(
        &self,
        symbols: &Symbols,
        display: &str,
        results: &mut Results,
        accepts: impl Fn(&Site) -> bool,
    ) {
        results.partial |= self.partial;
        for entry in &self.occurrences {
            if !self
                .resolve(entry, symbols)
                .is_some_and(|site| accepts(&site))
                || self.declarations.iter().any(|decl| decl.site == entry.site)
            {
                continue;
            }
            let path = entry.site.file.as_ref().map(|file| file.path.clone());
            if !results.locations.insert((path, entry.site.offset)) {
                continue;
            }
            if results.items.len() == MAX_RESULTS {
                results.truncated = true;
                break;
            }
            let label = format!("{display} · lín {}", entry.line);
            let mut item = entry.site.item(label, self.selected.clone());
            if let (Some(root), Some(file)) = (&results.project_root, &entry.site.file) {
                let path: String = file
                    .path
                    .strip_prefix(root)
                    .unwrap_or(&file.path)
                    .to_string_lossy()
                    .chars()
                    .filter(|ch| !ch.is_control())
                    .take(96)
                    .collect();
                item.label = format!("{path} · lín {}", entry.line);
            }
            results.items.push(TaskItem::Source(item));
        }
    }
}

pub(super) fn binding_context(
    context: &Context<'_>,
    symbols: &Symbols,
    key: &str,
) -> (Option<Site>, Option<bool>) {
    let Some(token) = context.tokens.last() else {
        return (None, None);
    };
    let before = &context.bytes[..token.range.start];
    if before.ends_with(b":") {
        // A handle or object member after ':' is not a lexical name here.
        return (None, None);
    }
    let end = token.range.end;
    let Some((target, range, _)) = context.target(end) else {
        return (None, None);
    };
    if range != token.range {
        return (None, None);
    }
    let tail = &context.bytes[end..];
    let qualified =
        tail.first() == Some(&b'.') && tail.get(1).is_some_and(|byte| identifier_byte(*byte));
    let next = tail
        .iter()
        .take(128)
        .find(|byte| !byte.is_ascii_whitespace());
    let routine = match target {
        Target::Symbols { run: true }
            if !qualified && (matches!(next, Some(b'.' | b'(')) || tail.is_empty()) =>
        {
            Some(true)
        }
        Target::Symbols { run: true } => return (None, None),
        Target::Symbols { run: false } if !qualified && next == Some(&b'(') => Some(false),
        _ => None,
    };
    let found = if let Some(run) = routine {
        symbols.routine_definition(key, run)
    } else {
        symbols.definition(&target, key, qualified)
    };
    (found.map(|found| found.site().clone()), routine)
}

fn identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-') || byte >= 0x80
}

pub(in crate::editor) fn name_at(
    bytes: &[u8],
    encoding: TextEncoding,
    cursor: usize,
) -> Option<(Range<usize>, String)> {
    let tail = bytes.get(cursor..)?;
    let bom = if encoding == TextEncoding::Utf8 && bytes.starts_with(b"\xef\xbb\xbf") {
        3
    } else {
        0
    };
    let before = bytes.get(bom..cursor)?;
    let left = before
        .iter()
        .rev()
        .take(129)
        .take_while(|&&byte| identifier_byte(byte))
        .count();
    let right = tail
        .iter()
        .take(129)
        .take_while(|&&byte| identifier_byte(byte))
        .count();
    if !(1..=128).contains(&(left + right)) {
        return None;
    }
    let range = cursor - left..cursor + right;
    let decoded = decode_fragment(&bytes[range.clone()], encoding, range.start);
    (decoded.issues.is_empty() && identifier(&decoded.text)).then_some((range, decoded.text))
}

impl Catalog {
    pub(super) fn references(
        &self,
        source: &Path,
        bytes: &[u8],
        encoding: TextEncoding,
        cursor: usize,
        cancel: &Cancellation,
    ) -> Result<Vec<TaskItem>, TaskFailure> {
        let Some((selected, name)) = name_at(bytes, encoding, cursor) else {
            return Ok(Vec::new());
        };
        let mut context = Context::new(bytes, encoding);
        let (references, symbols) = self.scan_references(
            &mut Includes::new(&self.root, source).with_locations(),
            &mut context,
            selected,
            &name,
            cancel,
        )?;
        let Some(identity) = references.identity(&symbols) else {
            return Ok(vec![TaskItem::Message("Símbolo ABL sin resolver".into())]);
        };
        let mut results = Results::default();
        references.append(&symbols, &name, &mut results, |site| site == &identity);
        if !results.truncated && references.is_global(&identity) {
            let declaration = identity.file.clone().or_else(|| {
                is_include(source)
                    .then(|| {
                        let path = source.canonicalize().ok()?;
                        Some(Arc::new(IncludeFile {
                            path,
                            encoding,
                            version: jc_files::ContentFingerprint::of(bytes),
                        }))
                    })
                    .flatten()
            });
            if let Some(file) = declaration {
                self.find_consumers(
                    source,
                    &references,
                    &name,
                    (file.as_ref(), identity.offset),
                    &mut results,
                    cancel,
                )?;
            }
        }
        Ok(results.finish())
    }

    fn scan_references(
        &self,
        includes: &mut Includes<'_>,
        context: &mut Context<'_>,
        selected: Range<usize>,
        name: &str,
        cancel: &Cancellation,
    ) -> Result<(ReferenceScan, Symbols), TaskFailure> {
        let mut symbols = Symbols::default();
        symbols.track_references(ReferenceScan {
            key: fold(name),
            selected,
            declarations: Vec::new(),
            occurrences: Vec::new(),
            partial: false,
        });
        includes.scan(context, &mut symbols, self, cancel)?;
        let references = symbols.take_references().expect("active reference search");
        Ok((references, symbols))
    }

    fn find_consumers(
        &self,
        source: &Path,
        references: &ReferenceScan,
        name: &str,
        declaration: (&IncludeFile, usize),
        results: &mut Results,
        cancel: &Cancellation,
    ) -> Result<(), TaskFailure> {
        let Ok(root) = self.root.canonicalize() else {
            results.partial = true;
            return Ok(());
        };
        if !declaration.0.path.starts_with(&root) {
            results.partial = true;
            return Ok(());
        }
        results.project_root = Some(root.clone());
        let source = source.canonicalize().ok();
        // A direct occurrence and a re-opened disk copy of the same file are one result.
        if let Some(path) = &source {
            for item in &results.items {
                if let TaskItem::Source(item) = item {
                    if matches!(item.action, super::SourceAction::Navigate) {
                        results
                            .locations
                            .insert((Some(path.clone()), item.range.start));
                    }
                }
            }
        }
        let mut queue = VecDeque::from([root.clone()]);
        let (mut directories, mut files, mut remaining) = (1, 0, MAX_PROJECT_BYTES);
        'project: while let Some(directory) = queue.pop_front() {
            let mut listing =
                match jc_files::list_directory_in_root(&root, &directory, 2048, 512 * 1024, || {
                    cancel.is_cancelled()
                }) {
                    Ok(listing) => listing,
                    Err(_) if cancel.is_cancelled() => return Err(TaskFailure::Cancelled),
                    Err(_) => {
                        results.partial = true;
                        continue;
                    }
                };
            results.partial |= listing.limited;
            listing.entries.sort_unstable_by(|a, b| a.name.cmp(&b.name));
            for entry in listing.entries {
                if cancel.is_cancelled() {
                    return Err(TaskFailure::Cancelled);
                }
                let path = directory.join(entry.name);
                if entry.is_dir {
                    if matches!(
                        path.file_name().and_then(|name| name.to_str()),
                        Some(".cide" | "target" | "node_modules")
                    ) {
                        continue;
                    }
                    if directories == MAX_PROJECT_DIRS {
                        results.partial = true;
                    } else {
                        directories += 1;
                        queue.push_back(path);
                    }
                    continue;
                }
                if !is_program(&path) || source.as_ref() == Some(&path) {
                    continue;
                }
                if files == MAX_PROJECT_FILES || remaining == 0 {
                    results.partial = true;
                    break 'project;
                }
                files += 1;
                let (path, bytes) =
                    match jc_files::read_in_root(&root, &path, MAX_SOURCE_BYTES.min(remaining)) {
                        Ok(file) => file,
                        Err(_) => {
                            results.partial = true;
                            continue;
                        }
                    };
                remaining -= bytes.len();
                let encoding = jc_text::probe_encoding(&bytes, None)
                    .encoding
                    .unwrap_or(declaration.0.encoding);
                let mut context = Context::new(&bytes, encoding);
                context.file = Some(Arc::new(IncludeFile {
                    version: jc_files::ContentFingerprint::of(&bytes),
                    encoding,
                    path: path.clone(),
                }));
                let mut includes = Includes::new(&self.root, &path)
                    .with_locations()
                    .with_byte_limit(remaining);
                let result = self.scan_references(
                    &mut includes,
                    &mut context,
                    references.selected.clone(),
                    name,
                    cancel,
                );
                remaining -= includes.bytes_read();
                let (scan, symbols) = match result {
                    Ok(result) => result,
                    Err(TaskFailure::Cancelled) => return Err(TaskFailure::Cancelled),
                    Err(_) => {
                        results.partial = true;
                        continue;
                    }
                };
                scan.append(&symbols, name, results, |site| {
                    site.offset == declaration.1
                        && site.file.as_deref().is_some_and(|file| {
                            file.path == declaration.0.path && file.version == declaration.0.version
                        })
                        && scan.is_global(site)
                });
                if results.truncated {
                    break 'project;
                }
            }
        }
        Ok(())
    }
}

fn is_include(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("i"))
}

fn is_program(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            ["p", "w", "cls"]
                .iter()
                .any(|kind| ext.eq_ignore_ascii_case(kind))
        })
}
