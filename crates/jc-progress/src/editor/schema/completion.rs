use super::{avm_path, decode, Catalog, SchemaController};
use crate::{EncodedText, SchemaRecord};
use jc_app::{
    source_services::{CatalogQuery, CatalogSource, SourceAction, SourceItem},
    Cancellation, TaskFailure, TaskItem, TaskOutput, TaskSnapshot, Workbench,
};
use jc_languages::syntax::SyntaxKind;
use jc_text::{decode_fragment, TextEncoding};
use std::{
    ops::Range,
    path::{Component, Path, PathBuf},
    sync::{atomic::Ordering, Arc, Weak},
};

mod declarations;
mod includes;

use declarations::Symbols;
use includes::IncludeFile;
use includes::Includes;

const MAX_SUGGESTIONS: usize = 32;
const MAX_STATEMENT_TOKENS: usize = 256;
const MAX_DEFINITION_TOKENS: usize = 4096;
const MAX_LOCAL_NAMES: usize = 512;

fn project_source<'a>(app: &'a Workbench, root: &Path) -> Option<&'a Path> {
    let path = app.active_path()?;
    if !(path.starts_with(root) || avm_path(path).starts_with(avm_path(root)))
        || path.components().any(|part| part == Component::ParentDir)
        || jc_languages::detect(&path.to_string_lossy()).id != "abl"
    {
        return None;
    }
    app.active_encoding()?;
    Some(path)
}

impl SchemaController {
    pub fn supports_completion(&self, app: &Workbench, root: Option<&Path>) -> bool {
        self.supports_definition(app, root)
            && self
                .catalog
                .as_ref()
                .is_some_and(|catalog| root == Some(catalog.root.as_path()))
    }

    pub fn supports_definition(&self, app: &Workbench, root: Option<&Path>) -> bool {
        root.and_then(|root| project_source(app, root)).is_some()
    }

    pub fn source(
        &self,
        app: &Workbench,
        root: Option<&Path>,
        require_schema: bool,
    ) -> Option<Arc<dyn CatalogSource>> {
        let root = root?;
        let source = project_source(app, root)?.to_owned();
        let catalog = self
            .catalog
            .as_ref()
            .filter(|catalog| catalog.root == root)
            .map(Arc::downgrade);
        if require_schema && catalog.is_none() {
            return None;
        }
        Some(Arc::new(Provider {
            catalog,
            root: root.to_owned(),
            source,
        }))
    }
}

struct Provider {
    catalog: Option<Weak<Catalog>>,
    root: PathBuf,
    source: PathBuf,
}

impl Provider {
    fn live_catalog(&self) -> Option<Arc<Catalog>> {
        self.catalog
            .as_ref()?
            .upgrade()
            .filter(|catalog| catalog.live.load(Ordering::Acquire))
    }
}

impl CatalogSource for Provider {
    fn is_current(&self) -> bool {
        self.catalog.is_none() || self.live_catalog().is_some()
    }

    fn query(
        &self,
        query: CatalogQuery,
        snapshot: &TaskSnapshot,
        cancel: &Cancellation,
        output: &mut TaskOutput,
    ) -> Result<(), TaskFailure> {
        let catalog = self.live_catalog();
        if self.catalog.is_some() && catalog.is_none() {
            return Err(TaskFailure::Cancelled);
        }
        match query {
            CatalogQuery::Complete => {
                let catalog = catalog.ok_or(TaskFailure::Failed)?;
                for item in catalog.suggest(
                    &self.source,
                    snapshot.bytes(),
                    snapshot.stamp.encoding,
                    cancel,
                )? {
                    output.push(TaskItem::Source(item))?;
                }
            }
            CatalogQuery::Definition { cursor } => {
                let empty;
                let catalog = if let Some(catalog) = catalog.as_deref() {
                    catalog
                } else {
                    empty = Catalog {
                        root: self.root.clone(),
                        live: true.into(),
                        records: Vec::new(),
                        codec: String::new(),
                        aliases: Vec::new(),
                        version: String::new(),
                    };
                    &empty
                };
                if let Some(item) = catalog.definition(
                    &self.source,
                    snapshot.bytes(),
                    snapshot.stamp.encoding,
                    cursor,
                    cancel,
                )? {
                    output.push(TaskItem::Source(item))?;
                }
            }
        }
        Ok(())
    }
}

struct Token {
    kind: SyntaxKind,
    range: Range<usize>,
}

struct Context<'a> {
    bytes: &'a [u8],
    encoding: TextEncoding,
    file: Option<Arc<IncludeFile>>,
    tokens: Vec<Token>,
    overflow: bool,
}

#[derive(Clone)]
struct Site {
    offset: usize,
    file: Option<Arc<IncludeFile>>,
}

impl Site {
    fn item(&self, label: String, reference: Range<usize>) -> SourceItem {
        if let Some(file) = &self.file {
            let name: String = file
                .path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .chars()
                .filter(|ch| !ch.is_control())
                .take(48)
                .collect();
            SourceItem {
                range: reference,
                label: format!("{label} · {name}"),
                action: SourceAction::ProjectLocation {
                    path: file.path.clone(),
                    offset: self.offset,
                    encoding: file.encoding,
                    version: file.version,
                },
            }
        } else {
            SourceItem {
                range: self.offset..self.offset,
                label,
                action: SourceAction::Navigate,
            }
        }
    }
}

enum Target {
    Tables(Option<String>),
    Fields(Vec<String>),
    Indexes(Vec<String>),
    Symbols { run: bool },
}

impl<'a> Context<'a> {
    fn new(bytes: &'a [u8], encoding: TextEncoding) -> Self {
        Self {
            bytes,
            encoding,
            file: None,
            tokens: Vec::new(),
            overflow: false,
        }
    }

    fn site(&self, token: &Token) -> Site {
        Site {
            offset: token.range.start,
            file: self.file.clone(),
        }
    }

    fn bytes(&self, token: &Token) -> &[u8] {
        &self.bytes[token.range.clone()]
    }

    fn word(&self, token: &Token) -> Option<String> {
        if !matches!(token.kind, SyntaxKind::Plain | SyntaxKind::Keyword) || token.range.len() > 128
        {
            return None;
        }
        let decoded = decode_fragment(self.bytes(token), self.encoding, token.range.start);
        (decoded.issues.is_empty() && identifier(&decoded.text)).then_some(decoded.text)
    }

    fn starts_definition(&self) -> bool {
        self.tokens.first().is_some_and(|token| {
            let word = self.bytes(token);
            word.eq_ignore_ascii_case(b"DEFINE") || word.eq_ignore_ascii_case(b"DEF")
        })
    }

    fn push(
        &mut self,
        token: Token,
        symbols: &mut Symbols,
        catalog: &Catalog,
        certain: bool,
    ) -> Result<(), TaskFailure> {
        let bytes = self.bytes(&token);
        let after = &self.bytes[token.range.end..];
        if token.kind == SyntaxKind::Operator
            && (matches!(bytes, b":" | b";")
                || bytes == b"."
                    && (after.first().is_some_and(u8::is_ascii_whitespace)
                        || after.starts_with(b"/*")
                        || after.is_empty()
                            && self.tokens.first().is_some_and(|first| {
                                [b"DEFINE".as_slice(), b"DEF", b"END", b"FUNCTION"]
                                    .iter()
                                    .any(|word| self.bytes(first).eq_ignore_ascii_case(word))
                            })))
        {
            if certain && !self.overflow {
                symbols.statement(self, bytes, catalog)?;
            }
            self.tokens.clear();
            self.overflow = false;
            return Ok(());
        }
        if token.kind == SyntaxKind::Comment || bytes.iter().all(u8::is_ascii_whitespace) {
            return Ok(());
        }
        if token.kind == SyntaxKind::String {
            if let Some(previous) = self.tokens.last_mut().filter(|previous| {
                previous.kind == SyntaxKind::String && previous.range.end == token.range.start
            }) {
                previous.range.end = token.range.end;
                return Ok(());
            }
        }
        if self.tokens.len() == MAX_STATEMENT_TOKENS && !self.starts_definition() {
            self.overflow = true;
        }
        if self.tokens.len() == MAX_DEFINITION_TOKENS {
            return Err(TaskFailure::LimitExceeded);
        }
        if self.overflow {
            return Ok(());
        }
        self.tokens.push(token);
        Ok(())
    }

    fn record_start(&self) -> Option<usize> {
        self.tokens.iter().enumerate().rev().find_map(|(i, token)| {
            let word = self.bytes(token);
            if [b"FIND".as_slice(), b"CREATE"]
                .iter()
                .any(|key| word.eq_ignore_ascii_case(key))
            {
                let optional = self.tokens.get(i + 1).is_some_and(|next| {
                    [b"FIRST".as_slice(), b"LAST", b"NEXT", b"PREV", b"CURRENT"]
                        .iter()
                        .any(|modifier| self.bytes(next).eq_ignore_ascii_case(modifier))
                });
                return Some(i + 1 + usize::from(optional));
            }
            let record = [b"EACH".as_slice(), b"FIRST", b"LAST"]
                .iter()
                .any(|key| word.eq_ignore_ascii_case(key));
            let for_phrase = i.checked_sub(1).is_some_and(|previous| {
                let before = self.bytes(&self.tokens[previous]);
                before.eq_ignore_ascii_case(b"FOR")
                    || before == b","
                        && self.tokens[..previous]
                            .iter()
                            .any(|t| self.bytes(t).eq_ignore_ascii_case(b"FOR"))
            });
            (record && for_phrase).then_some(i + 1)
        })
    }

    fn components(&self, tokens: &[Token]) -> Option<Vec<String>> {
        let mut names = Vec::new();
        let mut previous_end = tokens.first()?.range.start;
        for (i, token) in tokens.iter().enumerate() {
            if previous_end != token.range.start {
                return None;
            }
            if i % 2 == 0 {
                names.push(self.word(token)?);
            } else if self.bytes(token) != b"." {
                return None;
            }
            previous_end = token.range.end;
        }
        (tokens.len() % 2 == 1 && names.len() <= 2).then_some(names)
    }

    fn record_table(&self, before: usize) -> Option<Vec<String>> {
        let start = self.record_start()?;
        let tokens = self.tokens.get(start..before)?;
        let end = if tokens.get(1).is_some_and(|token| self.bytes(token) == b".") {
            3
        } else {
            1
        };
        self.components(tokens.get(..end)?)
    }

    fn target(&self) -> Option<(Target, Range<usize>, String)> {
        let cursor = self.bytes.len();
        let mut before = self.tokens.len();
        let mut range = cursor..cursor;
        let mut prefix = String::new();
        let record_start = self.record_start();
        if let Some(last) = self.tokens.last().filter(|token| token.range.end == cursor) {
            if let Some(word) = self.word(last) {
                // Complete an empty slot after a complete record keyword on whitespace only.
                before -= 1;
                range = last.range.clone();
                prefix = word;
            } else if !matches!(self.bytes(last), b"." | b"(") {
                return None;
            }
        }
        let previous = self.tokens.get(..before)?;
        if previous
            .last()
            .is_some_and(|token| self.bytes(token).eq_ignore_ascii_case(b"USE-INDEX"))
        {
            return Some((
                Target::Indexes(self.record_table(before - 1)?),
                range,
                prefix,
            ));
        }
        if let Some(dot) = previous
            .last()
            .filter(|token| self.bytes(token) == b"." && token.range.end == range.start)
        {
            let mut start = before.checked_sub(2)?;
            if self.tokens[start].range.end != dot.range.start {
                return None;
            }
            while start >= 2
                && self.bytes(&self.tokens[start - 1]) == b"."
                && self.tokens[start - 1].range.end == self.tokens[start].range.start
                && self.tokens[start - 2].range.end == self.tokens[start - 1].range.start
            {
                start -= 2;
            }
            let names = self.components(self.tokens.get(start..before - 1)?)?;
            let target = if record_start == Some(start) {
                let [db]: [String; 1] = names.try_into().ok()?;
                Target::Tables(Some(db))
            } else {
                Target::Fields(names)
            };
            return Some((target, range, prefix));
        }
        if record_start == Some(before) {
            return Some((Target::Tables(None), range, prefix));
        }
        if previous
            .last()
            .is_some_and(|token| self.bytes(token).eq_ignore_ascii_case(b"WHERE"))
        {
            return Some((
                Target::Fields(self.record_table(before - 1)?),
                range,
                prefix,
            ));
        }
        let run = previous
            .last()
            .is_some_and(|token| self.bytes(token).eq_ignore_ascii_case(b"RUN"));
        (run || !prefix.is_empty()).then_some((Target::Symbols { run }, range, prefix))
    }
}

fn identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|ch| ch == '_' || ch.is_alphabetic())
        && chars.all(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '-'))
}

fn fold(name: &str) -> String {
    name.to_lowercase()
}

impl Catalog {
    fn name(&self, value: &EncodedText) -> Option<String> {
        let name = decode(value.as_bytes(), &self.codec);
        (name.len() <= 128 && identifier(&name)).then_some(name)
    }

    fn logical_database(&self, qualifier: &str) -> String {
        let qualifier = fold(qualifier);
        self.aliases
            .iter()
            .find(|alias| fold(&alias.name) == qualifier)
            .map_or(qualifier, |alias| fold(&alias.database))
    }

    fn table_matches<'a>(
        &'a self,
        database: Option<&'a str>,
        table_prefix: &'a str,
        exact: bool,
    ) -> impl Iterator<Item = (&'a EncodedText, &'a EncodedText, String)> + 'a {
        self.records.iter().filter_map(move |record| {
            let SchemaRecord::Table {
                database: db,
                table,
            } = record
            else {
                return None;
            };
            let name = self.name(table)?;
            let key = fold(&name);
            if (if exact {
                key != table_prefix
            } else {
                !key.starts_with(table_prefix)
            }) || database
                .is_some_and(|db_name| self.name(db).is_none_or(|db| fold(&db) != db_name))
            {
                return None;
            }
            Some((db, table, name))
        })
    }

    fn definition(
        &self,
        source: &Path,
        bytes: &[u8],
        encoding: TextEncoding,
        cursor: usize,
        cancel: &Cancellation,
    ) -> Result<Option<SourceItem>, TaskFailure> {
        let tail = bytes.get(cursor..).ok_or(TaskFailure::InvalidRange)?;
        let end = cursor
            + tail
                .iter()
                .take(129)
                .take_while(|&&byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-') || byte >= 0x80
                })
                .count();
        if end - cursor > 128 {
            return Ok(None);
        }
        let mut context = Context::new(&bytes[..end], encoding);
        let mut symbols = Symbols::default();
        let protected = Includes::new(&self.root, source).with_locations().scan(
            &mut context,
            &mut symbols,
            self,
            cancel,
        )?;
        if protected || context.overflow {
            return Ok(None);
        }
        let Some((target, range, name)) = context.target() else {
            return Ok(None);
        };
        if range.is_empty() || !(range.start..=range.end).contains(&cursor) {
            return Ok(None);
        }
        let qualified = bytes
            .get(end..end + 2)
            .is_some_and(|next| next[0] == b'.' && !next[1].is_ascii_whitespace());
        let key = fold(&name);
        if let Some(found) = symbols.definition(&target, &key, qualified) {
            return Ok(Some(found.item(range)));
        }

        // Internal routines can be declared below their use. The second scan
        // only considers top-level routine declarations, never a later local.
        if matches!(target, Target::Symbols { .. }) && !qualified && end < bytes.len() {
            let mut all = Symbols::default();
            Includes::new(&self.root, source).with_locations().scan(
                &mut Context::new(bytes, encoding),
                &mut all,
                self,
                cancel,
            )?;
            let run = matches!(target, Target::Symbols { run: true });
            return Ok(all
                .routine_definition(&key, run)
                .map(|found| found.item(range)));
        }
        Ok(None)
    }

    fn suggest(
        &self,
        source: &Path,
        bytes: &[u8],
        encoding: TextEncoding,
        cancel: &Cancellation,
    ) -> Result<Vec<SourceItem>, TaskFailure> {
        let mut context = Context::new(bytes, encoding);
        let mut symbols = Symbols::default();
        let protected =
            Includes::new(&self.root, source).scan(&mut context, &mut symbols, self, cancel)?;
        if protected || context.overflow {
            return Ok(Vec::new());
        }
        let Some((target, range, prefix)) = context.target() else {
            return Ok(Vec::new());
        };
        let prefix = fold(&prefix);
        let mut suggestions = match target {
            Target::Tables(database) => {
                let mut suggestions = if database.is_none() {
                    symbols.record_names(&prefix)
                } else {
                    Vec::new()
                };
                let database = database.map(|name| self.logical_database(&name));
                suggestions.extend(self.tables(&symbols, database.as_deref(), &prefix));
                suggestions
            }
            Target::Fields(ref names) | Target::Indexes(ref names) => {
                let indexes = matches!(target, Target::Indexes(_));
                symbols
                    .local_members(self, names, &prefix, indexes)
                    .unwrap_or_else(|| self.members_where(names, &prefix, indexes, |_| true))
            }
            Target::Symbols { run } => symbols.value_names(&prefix, run),
        };
        suggestions.sort_by_cached_key(|(_, name)| fold(name));
        suggestions.truncate(MAX_SUGGESTIONS);
        if cancel.is_cancelled() {
            return Err(TaskFailure::Cancelled);
        }
        Ok(suggestions
            .into_iter()
            .map(|(label, text)| SourceItem {
                range: range.clone(),
                label,
                action: SourceAction::Replace(text),
            })
            .collect())
    }

    fn tables(
        &self,
        symbols: &Symbols,
        database: Option<&str>,
        prefix: &str,
    ) -> Vec<(String, String)> {
        let mut candidates = self
            .table_matches(database, prefix, false)
            .filter_map(|(db, _, table)| Some((self.name(db)?, table)))
            .collect::<Vec<_>>();
        candidates.sort_by_cached_key(|(db, table)| (fold(table), fold(db)));
        candidates.dedup();
        let mut suggestions = Vec::new();
        for (i, (db, table)) in candidates.iter().enumerate() {
            let key = fold(table);
            let adjacent_duplicate = |neighbor: Option<&(String, String)>| {
                neighbor.is_some_and(|(_, name)| fold(name) == key)
            };
            let qualified = database.is_none()
                && (symbols.has_record(&key)
                    || adjacent_duplicate(i.checked_sub(1).and_then(|i| candidates.get(i)))
                    || adjacent_duplicate(candidates.get(i + 1)));
            let text = if qualified {
                format!("{db}.{table}")
            } else {
                table.clone()
            };
            suggestions.push((format!("Tabla · {db}.{table}"), text));
            if suggestions.len() == MAX_SUGGESTIONS {
                break;
            }
        }
        suggestions
    }

    fn members_where(
        &self,
        names: &[String],
        prefix: &str,
        indexes: bool,
        filter: impl Fn(&str) -> bool,
    ) -> Vec<(String, String)> {
        let database = (names.len() == 2).then(|| self.logical_database(&names[0]));
        let table_key = fold(names.last().expect("record name"));
        let mut matches = self.table_matches(database.as_deref(), &table_key, true);
        let Some((selected_db, selected_table, _)) = matches.next() else {
            return Vec::new();
        };
        if matches.next().is_some() {
            return Vec::new();
        }
        let mut suggestions = self
            .records
            .iter()
            .filter_map(|record| {
                let (db, table, name, label) = match record {
                    SchemaRecord::Field {
                        database,
                        table,
                        name,
                        ..
                    } if !indexes => (database, table, name, "Campo"),
                    SchemaRecord::Index {
                        database,
                        table,
                        name,
                        ..
                    } if indexes => (database, table, name, "Índice"),
                    _ => return None,
                };
                if db != selected_db || table != selected_table {
                    return None;
                }
                let name = self.name(name)?;
                let key = fold(&name);
                (key.starts_with(prefix) && filter(&key))
                    .then(|| (format!("{label} · {name}"), name))
            })
            .collect::<Vec<_>>();
        suggestions.sort_by_cached_key(|(_, name)| fold(name));
        suggestions.dedup();
        suggestions.truncate(MAX_SUGGESTIONS);
        suggestions
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::DatabaseAlias;
    use std::{path::PathBuf, sync::atomic::AtomicBool, time::Duration};

    fn text(value: &str) -> EncodedText {
        EncodedText(value.as_bytes().to_vec())
    }

    fn catalog() -> Catalog {
        let mut records = Vec::new();
        for (db, field, index) in [
            ("ventas", "Nombre", "Numero"),
            ("archivo", "Anio", "PorAnio"),
        ] {
            records.extend([
                SchemaRecord::Database { name: text(db) },
                SchemaRecord::Table {
                    database: text(db),
                    table: text("cliente"),
                },
                SchemaRecord::Field {
                    database: text(db),
                    table: text("cliente"),
                    name: text(field),
                    data_type: text("character"),
                    extent: 0,
                    format: None,
                    label: None,
                },
                SchemaRecord::Index {
                    database: text(db),
                    table: text("cliente"),
                    name: text(index),
                    primary: true,
                    unique: true,
                },
            ]);
        }
        Catalog {
            root: std::env::current_dir().unwrap(),
            live: AtomicBool::new(true),
            records,
            codec: "UTF-8".into(),
            aliases: vec![DatabaseAlias {
                name: "actual".into(),
                database: "ventas".into(),
            }],
            version: "11.7".into(),
        }
    }

    fn replacements(catalog: &Catalog, source: &str) -> Vec<String> {
        catalog
            .suggest(
                &catalog.root.join("fuente.p"),
                source.as_bytes(),
                TextEncoding::Utf8,
                &Cancellation::new(Duration::from_secs(2)),
            )
            .unwrap()
            .into_iter()
            .map(|item| {
                let SourceAction::Replace(text) = item.action else {
                    panic!("expected a replacement")
                };
                assert_eq!(item.range.end, source.len());
                format!("{}{text}", &source[..item.range.start])
            })
            .collect()
    }

    #[test]
    fn schema_completion_scopes_tables_fields_and_indexes() {
        let catalog = catalog();
        assert_eq!(
            replacements(&catalog, "FOR EACH cli"),
            ["FOR EACH archivo.cliente", "FOR EACH ventas.cliente"]
        );
        for (source, expected) in [
            ("FOR EACH actual.cl", "FOR EACH actual.cliente"),
            (
                "\u{feff}FOR EACH actual.cl",
                "\u{feff}FOR EACH actual.cliente",
            ),
            (
                "{inicio.i}\nFOR EACH actual.cl",
                "{inicio.i}\nFOR EACH actual.cliente",
            ),
            ("DISPLAY ACTUAL.cliente.N", "DISPLAY ACTUAL.cliente.Nombre"),
            ("DISPLAY archivo.cliente.", "DISPLAY archivo.cliente.Anio"),
            (
                "FIND FIRST actual.cliente WHERE N",
                "FIND FIRST actual.cliente WHERE Nombre",
            ),
            (
                "FIND FIRST actual.cliente\n WHERE cliente.Nombre = 'a'\n USE-INDEX Nu",
                "FIND FIRST actual.cliente\n WHERE cliente.Nombre = 'a'\n USE-INDEX Numero",
            ),
        ] {
            assert_eq!(replacements(&catalog, source), [expected], "{source}");
        }
        for source in [
            "DISPLAY cliente.N",
            "FIND FIRST cliente USE-INDEX ",
            "FIND FIRST archivo.cliente USE-INDEX Nu",
            "FIND FIRST actual.cliente {&otro-registro} USE-INDEX Nu",
            "/* DISPLAY actual.cliente.N",
            "MESSAGE 'actual.cliente.N",
            "DEFINE TEMP-TABLE cliente.\n DISPLAY cliente.N",
            "FOR EACH actual.cliente.\n USE-INDEX Nu",
        ] {
            assert!(replacements(&catalog, source).is_empty(), "{source}");
        }
    }

    #[test]
    fn schema_definition_uses_exact_name_scope_and_member_origin() {
        let catalog = catalog();
        let source = "DEFINE TEMP-TABLE ttDatos FIELD Codigo AS INTEGER INDEX idxCodigo Codigo.\n\
                      DEFINE BUFFER bDatos FOR TEMP-TABLE ttDatos.\n\
                      DEFINE VARIABLE cantidad AS INTEGER.\n\
                      PROCEDURE uno:\n DEFINE VARIABLE cantidad AS INTEGER.\n DISPLAY cantidad.\n END.\n\
                      DISPLAY bDatos.Codigo cantidad.\n FIND FIRST bDatos USE-INDEX idxCodigo.\n\
                      RUN despues.\n PROCEDURE despues:\n END PROCEDURE.";
        let definition = |cursor| {
            catalog
                .definition(
                    &catalog.root.join("fuente.p"),
                    source.as_bytes(),
                    TextEncoding::Utf8,
                    cursor,
                    &Cancellation::new(Duration::from_secs(2)),
                )
                .unwrap()
        };
        for (reference, position, declaration, declaration_name) in [
            ("DISPLAY bDatos", 10, "BUFFER bDatos", 7),
            ("bDatos.Codigo", 9, "FIELD Codigo", 6),
            ("USE-INDEX idxCodigo", 13, "INDEX idxCodigo", 6),
            (
                "DISPLAY cantidad",
                10,
                "VARIABLE cantidad AS INTEGER.\n DISPLAY",
                9,
            ),
            ("Codigo cantidad", 10, "VARIABLE cantidad", 9),
            ("RUN despues", 7, "PROCEDURE despues", 10),
        ] {
            let cursor = source.find(reference).unwrap() + position;
            let item = definition(cursor).unwrap_or_else(|| panic!("{reference}"));
            assert_eq!(item.action, SourceAction::Navigate);
            assert_eq!(
                item.range.start,
                source.find(declaration).unwrap() + declaration_name,
                "{reference}"
            );
        }
        assert!(definition(source.find("DISPLAY bDatos").unwrap() + 7).is_none());
    }

    #[test]
    fn schema_completion_provider_expires_on_catalog_clear() {
        let catalog = Arc::new(catalog());
        let provider = Provider {
            catalog: Some(Arc::downgrade(&catalog)),
            root: catalog.root.clone(),
            source: PathBuf::new(),
        };
        let mut controller = SchemaController {
            pending: None,
            catalog: Some(catalog.clone()),
        };
        assert!(provider.is_current());
        controller.clear();
        // A worker may still hold its snapshot. It must not permit insertion.
        assert!(!provider.is_current());
        assert_eq!(Arc::strong_count(&catalog), 1);
    }

    #[test]
    fn schema_completion_resolves_scoped_records_and_ordered_nested_includes() {
        use crate::editor::JobDir;
        use std::fs;

        let fixture = JobDir::create().unwrap();
        let mut catalog = catalog();
        catalog.root = fixture.path.clone();
        for dir in [".cide", "primero", "segundo"] {
            fs::create_dir(fixture.path.join(dir)).unwrap();
        }
        fs::write(fixture.path.join(".cide/propath"), "primero\nsegundo\n").unwrap();
        fs::write(fixture.path.join("primero/externo.i"), "{interno.i}").unwrap();
        fs::write(
            fixture.path.join("segundo/externo.i"),
            "DEFINE VARIABLE incorrecta AS INTEGER.",
        )
        .unwrap();
        fs::write(
            fixture.path.join("primero/interno.i"),
            "DEFINE TEMP-TABLE ttCopia LIKE actual.cliente USE-INDEX Numero\n FIELD extra AS INTEGER INDEX porExtra extra.\n\
             DEFINE BUFFER copia FOR TEMP-TABLE ttCopia.\n DEFINE BUFFER original FOR actual.cliente.\n\
             DEFINE VARIABLE totalImportado AS INTEGER.\n PROCEDURE importar:\n DEFINE VARIABLE soloLocal AS INTEGER.\n END PROCEDURE.",
        ).unwrap();
        for (expression, expected) in [
            ("FOR EACH ttCo", "FOR EACH ttCopia"),
            ("DISPLAY copia.ex", "DISPLAY copia.extra"),
            ("DISPLAY copia.No", "DISPLAY copia.Nombre"),
            ("DISPLAY original.No", "DISPLAY original.Nombre"),
            (
                "FIND FIRST copia USE-INDEX po",
                "FIND FIRST copia USE-INDEX porExtra",
            ),
            (
                "FIND FIRST copia USE-INDEX Nu",
                "FIND FIRST copia USE-INDEX Numero",
            ),
            ("DISPLAY totalI", "DISPLAY totalImportado"),
            ("RUN imp", "RUN importar"),
        ] {
            assert_eq!(
                replacements(&catalog, &format!("{{externo.i}}\n{expression}")),
                [format!("{{externo.i}}\n{expected}")]
            );
        }
        for expression in ["DISPLAY incorrect", "DISPLAY solo", "DISPLAY copia.Anio"] {
            assert!(replacements(&catalog, &format!("{{externo.i}}\n{expression}")).is_empty());
        }
        assert_eq!(
            replacements(
                &catalog,
                "PROCEDURE x:\n DEFINE BUFFER b FOR actual.cliente.\n DISPLAY b.No"
            ),
            ["PROCEDURE x:\n DEFINE BUFFER b FOR actual.cliente.\n DISPLAY b.Nombre"]
        );
        assert!(replacements(
            &catalog,
            "PROCEDURE x:\n DEFINE BUFFER b FOR actual.cliente.\n END PROCEDURE.\n DISPLAY b.No"
        )
        .is_empty());
        let in_routine = "PROCEDURE x:\n DEFINE VARIABLE soloLocal AS INTEGER.\n IF TRUE THEN DO:\n END.\n DISPLAY solo";
        assert_eq!(
            replacements(&catalog, in_routine),
            [format!("{in_routine}Local")]
        );
        assert!(replacements(
            &catalog,
            "PROCEDURE x:\n DEFINE VARIABLE soloLocal AS INTEGER.\n IF TRUE THEN DO:\n END.\n END.\n DISPLAY solo",
        ).is_empty());
    }
}
