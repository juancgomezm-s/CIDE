use super::{fold, Catalog, Context, Token, MAX_LOCAL_NAMES};
use jc_app::TaskFailure;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
enum Record {
    Database(Vec<String>),
    Temp(usize),
}

#[derive(Default)]
struct TempTable {
    base: Option<Record>,
    fields: Vec<String>,
    indexes: Vec<String>,
    inherited_indexes: BTreeSet<String>,
}

struct Binding {
    name: String,
    kind: &'static str,
    record: Option<Record>,
}

#[derive(Default)]
struct Scope {
    records: BTreeMap<String, Binding>,
    values: BTreeMap<String, Binding>,
}

struct Routine {
    kind: &'static str,
    blocks: usize,
    scope: Scope,
}

#[derive(Default)]
pub(super) struct Symbols {
    global: Scope,
    local: Option<Routine>,
    tables: Vec<TempTable>,
    declarations: usize,
}

impl Symbols {
    fn scope(&mut self) -> &mut Scope {
        self.local
            .as_mut()
            .map_or(&mut self.global, |routine| &mut routine.scope)
    }

    fn register(&mut self, binding: Binding) -> Result<(), TaskFailure> {
        self.declarations += 1;
        if self.declarations > MAX_LOCAL_NAMES {
            return Err(TaskFailure::LimitExceeded);
        }
        let scope = self.scope();
        let target = match binding.kind {
            "Tabla temporal" | "Buffer" | "Tabla de trabajo" => &mut scope.records,
            _ => &mut scope.values,
        };
        let key = fold(&binding.name);
        if let Some(existing) = target.get_mut(&key) {
            if existing.kind != binding.kind || !matches!(binding.kind, "Función" | "Procedimiento")
            {
                existing.kind = "";
                existing.record = None;
            }
        } else {
            target.insert(key, binding);
        }
        Ok(())
    }

    fn record(&self, key: &str) -> Option<&Binding> {
        self.local
            .as_ref()
            .and_then(|routine| routine.scope.records.get(key))
            .or_else(|| self.global.records.get(key))
    }

    fn values(&self, run: bool) -> impl Iterator<Item = (&String, &Binding)> {
        // RUN resolves internal procedures independently of local values.
        let local = self
            .local
            .as_ref()
            .filter(|_| !run)
            .map(|routine| &routine.scope.values);
        local.into_iter().flatten().chain(
            self.global
                .values
                .iter()
                .filter(move |(key, _)| !local.is_some_and(|scope| scope.contains_key(*key))),
        )
    }

    pub(super) fn record_names(&self, prefix: &str) -> Vec<(String, String)> {
        let local = self.local.as_ref().map(|routine| &routine.scope.records);
        local
            .into_iter()
            .flatten()
            .chain(
                self.global
                    .records
                    .iter()
                    .filter(move |(key, _)| !local.is_some_and(|scope| scope.contains_key(*key))),
            )
            .filter(|(key, entry)| key.starts_with(prefix) && !entry.kind.is_empty())
            .map(|(_, entry)| {
                (
                    format!("{} · {}", entry.kind, entry.name),
                    entry.name.clone(),
                )
            })
            .collect()
    }

    pub(super) fn has_record(&self, key: &str) -> bool {
        self.record(key).is_some()
    }

    pub(super) fn value_names(&self, prefix: &str, run: bool) -> Vec<(String, String)> {
        self.values(run)
            .filter(|(key, entry)| {
                key.starts_with(prefix)
                    && !entry.kind.is_empty()
                    && (entry.kind == "Procedimiento") == run
            })
            .map(|(_, entry)| {
                (
                    format!("{} · {}", entry.kind, entry.name),
                    entry.name.clone(),
                )
            })
            .collect()
    }

    pub(super) fn local_members(
        &self,
        catalog: &Catalog,
        names: &[String],
        prefix: &str,
        indexes: bool,
    ) -> Option<Vec<(String, String)>> {
        let [name] = names else { return None };
        let binding = self.record(&fold(name))?;
        let mut suggestions = Vec::new();
        let mut filters = Vec::new();
        let mut current = binding.record.as_ref();
        while let Some(record) = current {
            match record {
                Record::Database(names) => {
                    suggestions.extend(catalog.members_where(names, prefix, indexes, |key| {
                        filters
                            .iter()
                            .all(|filter: &&BTreeSet<String>| filter.contains(key))
                    }));
                    break;
                }
                Record::Temp(index) => {
                    let table = &self.tables[*index];
                    let (names, kind) = if indexes {
                        (&table.indexes, "Índice")
                    } else {
                        (&table.fields, "Campo")
                    };
                    suggestions.extend(names.iter().filter_map(|name| {
                        let key = fold(name);
                        (key.starts_with(prefix)
                            && filters.iter().all(|filter| filter.contains(&key)))
                        .then(|| (format!("{kind} · {name}"), name.clone()))
                    }));
                    if indexes && !table.inherited_indexes.is_empty() {
                        filters.push(&table.inherited_indexes);
                    }
                    current = table.base.as_ref();
                }
            }
        }
        let mut seen = BTreeSet::new();
        suggestions.retain(|(_, name)| seen.insert(fold(name)));
        Some(suggestions)
    }

    fn binding_target(
        &self,
        catalog: &Catalog,
        names: Vec<String>,
        temp_only: bool,
        buffer: bool,
    ) -> Option<Record> {
        if let [name] = names.as_slice() {
            let key = fold(name);
            if temp_only || !buffer || catalog.table_matches(None, &key, true).next().is_none() {
                if let Some(local) = self.record(&key) {
                    return match &local.record {
                        Some(Record::Temp(index)) if local.kind == "Tabla temporal" => {
                            Some(Record::Temp(*index))
                        }
                        _ => None,
                    };
                }
                if temp_only {
                    return None;
                }
            }
        } else if temp_only {
            return None;
        }
        Some(Record::Database(names))
    }

    pub(super) fn statement(
        &mut self,
        context: &Context<'_>,
        end: &[u8],
        catalog: &Catalog,
    ) -> Result<(), TaskFailure> {
        let tokens = &context.tokens;
        let word = |index| tokens.get(index).and_then(|token| context.word(token));
        let is = |index, keyword| at(context, tokens, index, keyword);
        if is(0, b"END") {
            if let Some(routine) = &mut self.local {
                if is(1, routine.kind.as_bytes()) || tokens.len() == 1 && routine.blocks == 0 {
                    self.local = None;
                } else {
                    routine.blocks = routine.blocks.saturating_sub(1);
                }
            }
        } else if let Some((keyword, kind)) = [
            ("PROCEDURE", "Procedimiento"),
            ("FUNCTION", "Función"),
            ("METHOD", ""),
            ("CONSTRUCTOR", ""),
            ("DESTRUCTOR", ""),
        ]
        .into_iter()
        .find(|(keyword, _)| is(0, keyword.as_bytes()))
        {
            if !kind.is_empty() && self.local.is_none() {
                if let Some(name) = word(1) {
                    self.register(Binding {
                        name,
                        kind,
                        record: None,
                    })?;
                }
            }
            if end == b":" && self.local.is_none() {
                self.local = Some(Routine {
                    kind: keyword,
                    blocks: 0,
                    scope: Scope::default(),
                });
            }
        } else if end == b"." && (is(0, b"DEFINE") || is(0, b"DEF")) {
            self.define(context, catalog)?;
        } else if end == b":" {
            if let Some(routine) = &mut self.local {
                let block = [
                    b"DO".as_slice(),
                    b"FOR",
                    b"REPEAT",
                    b"CASE",
                    b"CATCH",
                    b"FINALLY",
                ];
                if block.iter().any(|keyword| is(0, keyword))
                    || (1..tokens.len()).any(|i| {
                        (is(i - 1, b"THEN") || is(i - 1, b"ELSE") || is(0, b"ON"))
                            && block[..3].iter().any(|keyword| is(i, keyword))
                    })
                {
                    routine.blocks += 1;
                }
            }
        }
        Ok(())
    }

    fn define(&mut self, context: &Context<'_>, catalog: &Catalog) -> Result<(), TaskFailure> {
        let tokens = &context.tokens;
        let word = |index| tokens.get(index).and_then(|token| context.word(token));
        let is = |index, keyword| at(context, tokens, index, keyword);
        let Some(index) = (1..tokens.len()).find(|&index| {
            ![
                b"NEW".as_slice(),
                b"SHARED",
                b"GLOBAL",
                b"PRIVATE",
                b"PROTECTED",
                b"PUBLIC",
                b"STATIC",
                b"SERIALIZABLE",
                b"NON-SERIALIZABLE",
                b"INPUT",
                b"OUTPUT",
                b"INPUT-OUTPUT",
            ]
            .iter()
            .any(|keyword| is(index, keyword))
        }) else {
            return Ok(());
        };
        let Some(name) = word(index + 1) else {
            return Ok(());
        };
        let start = index + 2;
        let (kind, record) = if is(index, b"BUFFER") {
            let temp_only = is(start + 1, b"TEMP-TABLE");
            let record = is(start, b"FOR")
                .then(|| qualified(context, start + 1 + usize::from(temp_only)))
                .flatten()
                .and_then(|names| self.binding_target(catalog, names, temp_only, true));
            ("Buffer", record)
        } else if is(index, b"TEMP-TABLE") {
            let mut table = TempTable::default();
            // LIKE after a FIELD belongs to that field, not to the table.
            let first_member = (start..tokens.len())
                .find(|&i| is(i, b"FIELD") || is(i, b"INDEX"))
                .unwrap_or(tokens.len());
            let like = (start..first_member).find(|&i| is(i, b"LIKE") || is(i, b"LIKE-SEQUENTIAL"));
            if let Some(i) = like {
                table.base = qualified(context, i + 1)
                    .and_then(|names| self.binding_target(catalog, names, false, false));
                for i in i + 2..first_member {
                    if is(i, b"USE-INDEX") {
                        if let Some(name) = word(i + 1) {
                            table.inherited_indexes.insert(fold(&name));
                        }
                    }
                }
            }
            for i in first_member..tokens.len() {
                if let Some(member) = word(i + 1) {
                    if is(i, b"FIELD") && (is(i + 2, b"AS") || is(i + 2, b"LIKE")) {
                        table.fields.push(member);
                    } else if is(i, b"INDEX") {
                        table.indexes.push(member);
                    }
                }
            }
            let record = Record::Temp(self.tables.len());
            self.tables.push(table);
            ("Tabla temporal", Some(record))
        } else if is(index, b"WORK-TABLE") {
            ("Tabla de trabajo", None)
        } else if is(index, b"VARIABLE") {
            ("Variable", None)
        } else if is(index, b"PARAMETER") && (is(start, b"AS") || is(start, b"LIKE")) {
            ("Parámetro", None)
        } else {
            return Ok(());
        };
        self.register(Binding { name, kind, record })
    }
}

fn at(context: &Context<'_>, tokens: &[Token], index: usize, keyword: &[u8]) -> bool {
    tokens
        .get(index)
        .is_some_and(|token| context.bytes(token).eq_ignore_ascii_case(keyword))
}

fn qualified(context: &Context<'_>, start: usize) -> Option<Vec<String>> {
    let tokens = context.tokens.get(start..)?;
    let end = if at(context, tokens, 1, b".") { 3 } else { 1 };
    context.components(tokens.get(..end)?)
}
