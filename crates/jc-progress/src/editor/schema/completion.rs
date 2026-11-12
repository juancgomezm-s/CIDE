use super::{avm_path, decode, Catalog, SchemaController};
use crate::{EncodedText, SchemaRecord};
use jc_app::{
    source_services::{CatalogCompletion, SourceAction, SourceItem},
    Cancellation, TaskFailure, TaskItem, TaskOutput, TaskSnapshot, Workbench,
};
use jc_languages::syntax::{scan, LexerState, SyntaxKind, SyntaxProfile};
use jc_text::{decode_fragment, TextEncoding};
use std::{
    collections::BTreeSet,
    ops::Range,
    path::{Component, Path},
    sync::{atomic::Ordering, Arc, Weak},
};

const MAX_SUGGESTIONS: usize = 32;
const MAX_STATEMENT_TOKENS: usize = 256;
const MAX_LOCAL_NAMES: usize = 512;

impl SchemaController {
    fn catalog_for(&self, app: &Workbench, root: Option<&Path>) -> Option<&Arc<Catalog>> {
        let catalog = self.catalog.as_ref()?;
        let path = app.active_path()?;
        let root = root?;
        if root != catalog.root
            || !(path.starts_with(root) || avm_path(path).starts_with(avm_path(root)))
            || path.components().any(|part| part == Component::ParentDir)
            || jc_languages::detect(&path.to_string_lossy()).id != "abl"
        {
            return None;
        }
        app.active_encoding()?;
        Some(catalog)
    }

    pub fn supports_completion(&self, app: &Workbench, root: Option<&Path>) -> bool {
        self.catalog_for(app, root).is_some()
    }

    pub fn completion(
        &self,
        app: &Workbench,
        root: Option<&Path>,
    ) -> Option<Arc<dyn CatalogCompletion>> {
        Some(Arc::new(Provider(Arc::downgrade(
            self.catalog_for(app, root)?,
        ))))
    }
}

struct Provider(Weak<Catalog>);

impl Provider {
    fn live_catalog(&self) -> Option<Arc<Catalog>> {
        self.0
            .upgrade()
            .filter(|catalog| catalog.live.load(Ordering::Acquire))
    }
}

impl CatalogCompletion for Provider {
    fn is_current(&self) -> bool {
        self.live_catalog().is_some()
    }

    fn complete(
        &self,
        snapshot: &TaskSnapshot,
        cancel: &Cancellation,
        output: &mut TaskOutput,
    ) -> Result<(), TaskFailure> {
        let catalog = self.live_catalog().ok_or(TaskFailure::Cancelled)?;
        for item in catalog.suggest(snapshot.bytes(), snapshot.stamp.encoding, cancel)? {
            output.push(TaskItem::Source(item))?;
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
    tokens: Vec<Token>,
    locals: BTreeSet<String>,
    overflow: bool,
    local_limit: bool,
}

enum Target {
    Tables(Option<String>),
    Fields(Vec<String>),
    Indexes(Vec<String>),
}

impl<'a> Context<'a> {
    fn new(bytes: &'a [u8], encoding: TextEncoding) -> Self {
        Self {
            bytes,
            encoding,
            tokens: Vec::new(),
            locals: BTreeSet::new(),
            overflow: false,
            local_limit: false,
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

    fn push(&mut self, token: Token) {
        let bytes = self.bytes(&token);
        if token.kind == SyntaxKind::Operator
            && (matches!(bytes, b":" | b";")
                || bytes == b"."
                    && self
                        .bytes
                        .get(token.range.end)
                        .is_some_and(u8::is_ascii_whitespace))
        {
            self.tokens.clear();
            self.overflow = false;
            return;
        }
        if token.kind == SyntaxKind::Preprocessor {
            // Includes and macros can supply a record name. Resume at the next
            // explicit context instead of carrying the preceding record across.
            self.tokens.clear();
            self.overflow = false;
            return;
        }
        if token.kind == SyntaxKind::Comment || bytes.iter().all(u8::is_ascii_whitespace) {
            return;
        }
        if token.kind == SyntaxKind::String {
            if let Some(previous) = self.tokens.last_mut().filter(|previous| {
                previous.kind == SyntaxKind::String && previous.range.end == token.range.start
            }) {
                previous.range.end = token.range.end;
                return;
            }
        }
        if self.tokens.len() == MAX_STATEMENT_TOKENS {
            self.overflow = true;
            return;
        }
        if self
            .tokens
            .first()
            .is_some_and(|first| self.bytes(first).eq_ignore_ascii_case(b"DEFINE"))
            && self.tokens.last().is_some_and(|previous| {
                [b"BUFFER".as_slice(), b"TEMP-TABLE", b"WORK-TABLE"]
                    .iter()
                    .any(|name| self.bytes(previous).eq_ignore_ascii_case(name))
            })
        {
            if self.locals.len() == MAX_LOCAL_NAMES {
                self.local_limit = true;
            } else if let Some(name) = self.word(&token) {
                self.locals.insert(fold(&name));
            }
        }
        self.tokens.push(token);
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
        None
    }

    fn is_local(&self, names: &[String]) -> bool {
        matches!(names, [table] if self.local_limit || self.locals.contains(&fold(table)))
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

    fn suggest(
        &self,
        bytes: &[u8],
        encoding: TextEncoding,
        cancel: &Cancellation,
    ) -> Result<Vec<SourceItem>, TaskFailure> {
        let mut context = Context::new(bytes, encoding);
        let mut lexer = LexerState::default();
        let mut protected = false;
        let bom =
            usize::from(encoding == TextEncoding::Utf8 && bytes.starts_with(b"\xef\xbb\xbf")) * 3;
        for (chunk, slice) in bytes[bom..].chunks(4096).enumerate() {
            if cancel.is_cancelled() {
                return Err(TaskFailure::Cancelled);
            }
            let offset = bom + chunk * 4096;
            scan(
                SyntaxProfile::Abl,
                &mut lexer,
                slice,
                offset,
                offset + slice.len() == bytes.len(),
                |span| {
                    if span.source.end == bytes.len() {
                        protected = matches!(
                            span.kind,
                            SyntaxKind::Comment | SyntaxKind::String | SyntaxKind::Preprocessor
                        );
                    }
                    context.push(Token {
                        kind: span.kind,
                        range: span.source,
                    });
                },
            );
        }
        if protected || lexer.limited() || context.overflow {
            return Ok(Vec::new());
        }
        let Some((target, range, prefix)) = context.target() else {
            return Ok(Vec::new());
        };
        let prefix = fold(&prefix);
        let suggestions = match target {
            Target::Tables(database) => {
                let database = database.map(|name| self.logical_database(&name));
                self.tables(&context, database.as_deref(), &prefix)
            }
            Target::Fields(ref names) | Target::Indexes(ref names) => {
                if context.is_local(names) {
                    return Ok(Vec::new());
                }
                self.members(names, &prefix, matches!(target, Target::Indexes(_)))
            }
        };
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
        context: &Context<'_>,
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
                && (context.local_limit
                    || context.locals.contains(&key)
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

    fn members(&self, names: &[String], prefix: &str, indexes: bool) -> Vec<(String, String)> {
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
                fold(&name)
                    .starts_with(prefix)
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
            root: PathBuf::new(),
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
    fn schema_completion_provider_expires_on_catalog_clear() {
        let catalog = Arc::new(catalog());
        let provider = Provider(Arc::downgrade(&catalog));
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
}
