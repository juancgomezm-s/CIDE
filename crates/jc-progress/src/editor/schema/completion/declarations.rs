use super::classes::{Hierarchy, Imports};
use super::references::{binding_context, ReferenceScan};
use super::{fold, Catalog, Context, Site, SourceItem, Target, Token, MAX_LOCAL_NAMES};
use jc_app::{TaskFailure, TaskItem, TaskOutput};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
enum Record {
    Database(Vec<String>),
    Temp(usize),
}

#[derive(Default)]
struct TempTable {
    base: Option<Record>,
    fields: Vec<Member>,
    indexes: Vec<Member>,
    inherited_indexes: BTreeSet<String>,
}

struct Member {
    name: String,
    site: Site,
}

pub(super) struct Definition<'a> {
    kind: &'static str,
    name: &'a str,
    site: &'a Site,
}

impl Definition<'_> {
    pub(super) fn site(&self) -> &Site {
        self.site
    }

    pub(super) fn item(self, range: std::ops::Range<usize>) -> SourceItem {
        self.site
            .item(format!("{} · {}", self.kind, self.name), range)
    }
}

struct Binding {
    name: String,
    kind: &'static str,
    record: Option<Record>,
    object_type: Option<String>,
    site: Site,
}

impl Binding {
    fn definition(&self) -> Option<Definition<'_>> {
        (!self.kind.is_empty()).then_some(Definition {
            kind: self.kind,
            name: &self.name,
            site: &self.site,
        })
    }
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

const MAX_OUTLINE_ROWS: usize = 63;

#[derive(Default)]
struct Outline {
    rows: Vec<(Site, String)>,
    truncated: bool,
    skipped_include: bool,
}

impl Outline {
    fn add(&mut self, kind: &str, name: &str, site: Site) {
        if self.truncated {
            return;
        }
        if self
            .rows
            .iter()
            .any(|(existing, _)| existing.offset == site.offset && existing.file == site.file)
        {
            return;
        }
        if self.rows.len() == MAX_OUTLINE_ROWS {
            self.truncated = true;
            return;
        }
        let name: String = name
            .chars()
            .filter(|ch| !ch.is_control())
            .take(64)
            .collect();
        self.rows.push((site, format!("{name} · {kind}")));
    }
}

#[derive(Default)]
pub(super) struct Symbols {
    global: Scope,
    local: Option<Routine>,
    tables: Vec<TempTable>,
    declarations: usize,
    references: Option<ReferenceScan>,
    outline: Option<Outline>,
    imports: Option<Imports>,
    hierarchy: Option<Hierarchy>,
}

impl Symbols {
    pub(super) fn track_hierarchy(&mut self, method: &str) {
        self.hierarchy = Some(Hierarchy::new(method));
    }

    pub(super) fn into_hierarchy(self) -> (Hierarchy, Imports) {
        (
            self.hierarchy.expect("active class analysis"),
            self.imports.unwrap_or_default(),
        )
    }

    pub(super) fn track_imports(&mut self) {
        self.imports = Some(Imports::default());
    }

    pub(super) fn imports(&self) -> &Imports {
        self.imports.as_ref().expect("active type analysis")
    }
    pub(super) fn track_outline(&mut self) {
        self.outline = Some(Outline::default());
    }

    pub(super) fn publish_outline(self, output: &mut TaskOutput) -> Result<(), TaskFailure> {
        let outline = self.outline.expect("active outline");
        for (site, label) in outline.rows {
            output.push(TaskItem::Source(site.item(label, 0..0)))?;
        }
        let notice = match (outline.truncated, outline.skipped_include) {
            (true, true) => Some(format!(
                "Primeros {MAX_OUTLINE_ROWS} · inclusiones omitidas"
            )),
            (true, false) => Some(format!("Se muestran los primeros {MAX_OUTLINE_ROWS}")),
            (false, true) => Some("Hay inclusiones que no se pudieron leer".into()),
            (false, false) => None,
        };
        if let Some(notice) = notice {
            output.push(TaskItem::Message(notice))?;
        }
        Ok(())
    }

    pub(super) fn skip_outline_include(&mut self) -> bool {
        if let Some(outline) = &mut self.outline {
            outline.skipped_include = true;
            return true;
        }
        false
    }

    pub(super) fn outline_include(&mut self, context: &Context<'_>, offset: usize, path: &str) {
        if let Some(outline) = &mut self.outline {
            outline.add(
                "Inclusión",
                path,
                Site {
                    offset,
                    file: context.file.clone(),
                    expansion: context.expansion,
                },
            );
        }
    }

    fn scope(&mut self) -> &mut Scope {
        self.local
            .as_mut()
            .map_or(&mut self.global, |routine| &mut routine.scope)
    }

    pub(super) fn track_references(&mut self, scan: ReferenceScan) {
        self.references = Some(scan);
    }

    pub(super) fn take_references(&mut self) -> Option<ReferenceScan> {
        self.references.take()
    }

    pub(super) fn note_reference_overflow(&mut self) {
        if let Some(scan) = &mut self.references {
            scan.mark_partial();
        }
    }

    pub(super) fn observe_reference(
        &mut self,
        context: &mut Context<'_>,
    ) -> Result<(), TaskFailure> {
        let Some(scan) = self
            .references
            .as_ref()
            .filter(|scan| scan.matches_token(context))
        else {
            return Ok(());
        };
        let (binding, forward) = binding_context(context, self, scan.key());
        self.references
            .as_mut()
            .unwrap()
            .observe(context, binding, forward)
    }

    fn register(&mut self, binding: Binding) -> Result<(), TaskFailure> {
        if let Some(outline) = &mut self.outline {
            outline.add(binding.kind, &binding.name, binding.site);
            if let Some(Record::Temp(_)) = binding.record {
                let table = self.tables.pop().expect("just declared temp-table");
                for (kind, members) in [("Campo", table.fields), ("Índice", table.indexes)] {
                    for member in members {
                        let name = format!("{}.{}", binding.name, member.name);
                        outline.add(kind, &name, member.site);
                    }
                }
            }
            return Ok(());
        }
        self.declarations += 1;
        if self.declarations > MAX_LOCAL_NAMES {
            return Err(TaskFailure::LimitExceeded);
        }
        let key = fold(&binding.name);
        let declared = self
            .references
            .as_ref()
            .filter(|scan| scan.matches(&key))
            .map(|_| binding.site.clone());
        let global = self.local.is_none();
        let scope = self.scope();
        let target = match binding.kind {
            "Tabla temporal" | "Buffer" | "Tabla de trabajo" => &mut scope.records,
            _ => &mut scope.values,
        };
        let mut identity = declared.clone();
        if let Some(existing) = target.get_mut(&key) {
            if existing.kind == binding.kind && matches!(binding.kind, "Función" | "Procedimiento")
            {
                if declared.is_some() {
                    identity = Some(existing.site.clone());
                }
            } else {
                existing.kind = "";
                existing.record = None;
                existing.object_type = None;
            }
        } else {
            target.insert(key, binding);
        }
        if let (Some(site), Some(identity), Some(scan)) = (declared, identity, &mut self.references)
        {
            scan.declare(site, identity, global)?;
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

    pub(super) fn definition(
        &self,
        target: &Target,
        key: &str,
        qualified: bool,
    ) -> Option<Definition<'_>> {
        match target {
            Target::Tables(None) => self.record(key)?.definition(),
            Target::Tables(Some(_)) => None,
            Target::Symbols { run: true } => self.routine_definition(key, true),
            Target::Symbols { run: false } => {
                let value = self
                    .local
                    .as_ref()
                    .and_then(|routine| routine.scope.values.get(key))
                    .or_else(|| self.global.values.get(key))
                    .filter(|binding| binding.kind != "Procedimiento");
                if qualified {
                    self.record(key).or(value)?.definition()
                } else {
                    value.or_else(|| self.record(key))?.definition()
                }
            }
            Target::Fields(names) | Target::Indexes(names) => {
                let [name] = names.as_slice() else {
                    return None;
                };
                let indexes = matches!(target, Target::Indexes(_));
                let mut record = self.record(&fold(name))?.record.as_ref();
                while let Some(Record::Temp(index)) = record {
                    let table = &self.tables[*index];
                    let (members, kind) = if indexes {
                        (&table.indexes, "Índice")
                    } else {
                        (&table.fields, "Campo")
                    };
                    if let Some(member) = members.iter().find(|member| fold(&member.name) == key) {
                        return Some(Definition {
                            kind,
                            name: &member.name,
                            site: &member.site,
                        });
                    }
                    if indexes
                        && !table.inherited_indexes.is_empty()
                        && !table.inherited_indexes.contains(key)
                    {
                        return None;
                    }
                    record = table.base.as_ref();
                }
                None
            }
        }
    }

    pub(super) fn object_type(&self, name: &str) -> Option<&str> {
        let key = fold(name);
        self.local
            .as_ref()
            .and_then(|routine| routine.scope.values.get(&key))
            .or_else(|| self.global.values.get(&key))?
            .object_type
            .as_deref()
    }

    pub(super) fn routine_definition(&self, key: &str, run: bool) -> Option<Definition<'_>> {
        let binding = self.global.values.get(key)?;
        let kind = if run { "Procedimiento" } else { "Función" };
        (binding.kind == kind)
            .then(|| binding.definition())
            .flatten()
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
                    suggestions.extend(names.iter().filter_map(|member| {
                        let name = &member.name;
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
            if buffer && !temp_only && catalog.records.is_empty() {
                return None;
            }
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
        if let Some(hierarchy) = &mut self.hierarchy {
            hierarchy.statement(context, end)?;
        }
        if end == b"." && is(0, b"USING") {
            if let Some(imports) = &mut self.imports {
                imports.add(context)?;
            }
        }
        if let Some(outline) = &mut self.outline {
            let header = [
                (b"CLASS".as_slice(), "Clase"),
                (b"INTERFACE", "Interfaz"),
                (b"ENUM", "Enumeración"),
                (b"METHOD", "Método"),
                (b"CONSTRUCTOR", "Constructor"),
                (b"DESTRUCTOR", "Destructor"),
            ]
            .into_iter()
            .find(|(keyword, _)| is(0, keyword));
            if let Some((keyword, kind)) = header {
                let name_at = if matches!(keyword, b"METHOD" | b"CONSTRUCTOR" | b"DESTRUCTOR") {
                    (1..tokens.len())
                        .find(|&i| is(i, b"("))
                        .map_or(1, |i| i - 1)
                } else {
                    1
                };
                if let Some(name) = word(name_at) {
                    outline.add(kind, &name, context.site(&tokens[name_at]));
                }
            }
        }
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
                        object_type: None,
                        site: context.site(&tokens[1]),
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
                b"PACKAGE-PRIVATE",
                b"PACKAGE-PROTECTED",
                b"STATIC",
                b"SERIALIZABLE",
                b"NON-SERIALIZABLE",
                b"INPUT",
                b"OUTPUT",
                b"INPUT-OUTPUT",
                b"RETURN",
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
                let is_field = is(i, b"FIELD") && (is(i + 2, b"AS") || is(i + 2, b"LIKE"));
                let members = if is_field {
                    &mut table.fields
                } else if is(i, b"INDEX") {
                    &mut table.indexes
                } else {
                    continue;
                };
                if let Some(name) = word(i + 1) {
                    let site = context.site(&tokens[i + 1]);
                    if let Some(scan) = self
                        .references
                        .as_mut()
                        .filter(|scan| scan.matches(&fold(&name)))
                    {
                        scan.declare(site.clone(), site.clone(), self.local.is_none())?;
                    }
                    members.push(Member { name, site });
                }
            }
            let record = Record::Temp(self.tables.len());
            self.tables.push(table);
            ("Tabla temporal", Some(record))
        } else if is(index, b"WORK-TABLE") {
            ("Tabla de trabajo", None)
        } else if is(index, b"VARIABLE") || is(index, b"VAR") {
            ("Variable", None)
        } else if is(index, b"PARAMETER") && (is(start, b"AS") || is(start, b"LIKE")) {
            ("Parámetro", None)
        } else {
            if let (Some(outline), Some(kind)) = (
                &mut self.outline,
                crate::source_services::definition_kind(context.bytes(&tokens[index])),
            ) {
                outline.add(kind, &name, context.site(&tokens[index + 1]));
            }
            return Ok(());
        };
        let object_type = if self.imports.is_some()
            && matches!(kind, "Variable" | "Parámetro")
            && is(start, b"AS")
        {
            let index = start + 1 + usize::from(is(start + 1, b"CLASS"));
            tokens
                .get(index)
                .and_then(|token| super::classes::declared_type(context, token.range.start))
        } else {
            None
        };
        self.register(Binding {
            name,
            kind,
            record,
            object_type,
            site: context.site(&tokens[index + 1]),
        })
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
