use jc_app::source_services::{RelationMark, SourceAction, SourceItem, ValueType};
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Include,
    Module,
    Import,
    Buffer,
    Parameter,
    Variable,
    Table,
    WorkTable,
    Field,
    Variant,
    Index,
    Procedure,
    Function,
    Prototype,
    Method,
    Implementation,
    Macro,
    Type,
    Namespace,
    Data,
    Widget,
    Other,
    CallOutgoing,
    CallIncoming,
    CallUnresolved,
}

impl Kind {
    fn from_label(label: &str) -> Option<Self> {
        Some(match label {
            "Inclusión" => Self::Include,
            "Módulo" | "Módulo sin resolver" => Self::Module,
            "Importación" | "Importación dinámica" => Self::Import,
            "Búfer" | "Buffer" => Self::Buffer,
            "Parámetro" => Self::Parameter,
            "Variable" | "Constante" | "Propiedad" => Self::Variable,
            "Tabla temporal" | "Temp-table" => Self::Table,
            "Tabla de trabajo" => Self::WorkTable,
            "Campo" => Self::Field,
            "Variante" => Self::Variant,
            "Índice" => Self::Index,
            "Procedimiento" => Self::Procedure,
            "Función" => Self::Function,
            "Prototipo" => Self::Prototype,
            "Método" | "Constructor" | "Destructor" => Self::Method,
            "Implementación" => Self::Implementation,
            "Macro" => Self::Macro,
            "Clase" | "Interfaz" | "Enumeración" | "Tipo" | "Registro" | "Estructura" | "Rasgo" => {
                Self::Type
            }
            "Espacio de nombres" | "Paquete" => Self::Namespace,
            "Conjunto de datos" | "Consulta" => Self::Data,
            "Marco"
            | "Botón"
            | "Explorador de datos"
            | "Rectángulo"
            | "Imagen"
            | "Menú"
            | "Submenú" => Self::Widget,
            _ => return None,
        })
    }

    fn label(self) -> &'static str {
        match self {
            Self::Include => "Inclusiones",
            Self::Module => "Módulos",
            Self::Import => "Importaciones",
            Self::Buffer => "Búferes",
            Self::Parameter => "Parámetros",
            Self::Variable => "Variables y constantes",
            Self::Table => "Tablas temporales",
            Self::WorkTable => "Tablas de trabajo",
            Self::Field => "Campos",
            Self::Variant => "Variantes",
            Self::Index => "Índices",
            Self::Procedure => "Procedimientos",
            Self::Function => "Funciones",
            Self::Prototype => "Prototipos de función",
            Self::Method => "Métodos",
            Self::Implementation => "Implementaciones",
            Self::Macro => "Macros",
            Self::Type => "Clases y tipos",
            Self::Namespace => "Espacios de nombres",
            Self::Data => "Conjuntos y consultas",
            Self::Widget => "Controles",
            Self::Other => "Otros símbolos",
            Self::CallOutgoing => "Llama a",
            Self::CallIncoming => "Llamado por",
            Self::CallUnresolved => "Sin resolver",
        }
    }

    pub(super) fn icon(self) -> (&'static str, u32) {
        // Native BGR colors, with different glyphs so color is supplementary.
        match self {
            Self::Include => ("‹›", 0x00d9bc78),
            Self::Module => ("m", 0x00cbad8c),
            Self::Import => ("↗", 0x00d9bc78),
            Self::Buffer => ("b", 0x008bb5e5),
            Self::Parameter => ("p", 0x00bcd6a2),
            Self::Variable => ("···", 0x00b9d694),
            Self::Table | Self::WorkTable => ("▦", 0x0078c7ec),
            Self::Field => ("c", 0x0085c6e3),
            Self::Variant => ("◈", 0x00d5ace8),
            Self::Index => ("#", 0x009cb0f0),
            Self::Procedure => ("▷", 0x00e6b88a),
            Self::Function => ("ƒ", 0x00dfb2cb),
            Self::Prototype => ("ƒ", 0x00e6aeb2),
            Self::Method => ("•", 0x00dcadcd),
            Self::Implementation => ("↳", 0x00d5ace8),
            Self::Macro => ("!", 0x00bcd6a2),
            Self::Type => ("◇", 0x00d5ace8),
            Self::Namespace => ("∷", 0x00c3beb0),
            Self::Data => ("▤", 0x0090c6d8),
            Self::Widget => ("□", 0x00adaff1),
            Self::Other => ("·", 0x00bbb5b0),
            Self::CallOutgoing => relation_icon(RelationMark::Outgoing),
            Self::CallIncoming => relation_icon(RelationMark::Incoming),
            Self::CallUnresolved => relation_icon(RelationMark::Unresolved),
        }
    }
}

fn relation_icon(mark: RelationMark) -> (&'static str, u32) {
    let color = mark.color();
    let bgr = u32::from_le_bytes([color.r, color.g, color.b, 0]);
    (mark.icon(), bgr)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Item {
    Symbol(usize),
    Dependency(usize),
}

pub(super) struct Node {
    pub name: String,
    pub origin: Option<String>,
    pub kind: Kind,
    pub value_type: Option<ValueType>,
    pub item: Option<Item>,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub expanded: bool,
    pub depth: usize,
    pub unresolved: bool,
}

#[derive(Default)]
pub(super) struct Tree {
    pub nodes: Vec<Node>,
    roots: Vec<usize>,
    pub rows: Vec<usize>,
    pub selected: Option<usize>,
}

fn entry(item: &SourceItem, default: Kind) -> (Kind, &str) {
    let label = if matches!(item.action, SourceAction::ProjectLocation { .. }) {
        item.label
            .rsplit_once(" · ")
            .map_or(item.label.as_str(), |(text, _)| text)
    } else {
        &item.label
    };
    if let Some((name, kind)) = label.rsplit_once(" · ") {
        if let Some(kind) = Kind::from_label(kind) {
            return (kind, name);
        }
    }
    if let Some((kind, name)) = label.split_once(": ") {
        if let Some(kind) = Kind::from_label(kind) {
            return (kind, name);
        }
    }
    for (at, _) in label.match_indices(' ').rev() {
        if let Some(kind) = Kind::from_label(&label[..at]) {
            return (kind, &label[at + 1..]);
        }
    }
    (Kind::from_label(label).unwrap_or(default), label)
}

fn alphabetic(text: &str) -> String {
    let mut key = String::with_capacity(text.len());
    for ch in text.chars().flat_map(char::to_lowercase) {
        match ch {
            'á' => key.push('a'),
            'é' => key.push('e'),
            'í' => key.push('i'),
            'ó' => key.push('o'),
            'ú' | 'ü' => key.push('u'),
            'ñ' => key.push_str("n\u{10ffff}"),
            '\u{301}' | '\u{308}' => (),
            ch => key.push(ch),
        }
    }
    key
}

impl Tree {
    fn append(
        &mut self,
        parent: Option<usize>,
        name: &str,
        kind: Kind,
        item: Option<Item>,
        unresolved: bool,
    ) -> usize {
        let id = self.nodes.len();
        let depth = parent.map_or(0, |id| self.nodes[id].depth + 1);
        self.nodes.push(Node {
            name: name.into(),
            origin: None,
            kind,
            value_type: None,
            item,
            parent,
            children: Vec::new(),
            expanded: false,
            depth,
            unresolved,
        });
        if let Some(parent) = parent {
            self.nodes[parent].children.push(id);
        } else {
            self.roots.push(id);
        }
        id
    }

    fn folder(&mut self, parent: Option<usize>, name: &str, kind: Kind) -> usize {
        let children = parent.map_or(&self.roots, |id| &self.nodes[id].children);
        children
            .iter()
            .copied()
            .find(|&id| self.nodes[id].item.is_none() && self.nodes[id].name == name)
            .unwrap_or_else(|| self.append(parent, name, kind, None, false))
    }

    fn ancestry(&self, mut id: usize) -> Vec<String> {
        let mut names = Vec::new();
        loop {
            let node = &self.nodes[id];
            names.push(node.name.clone());
            let Some(parent) = node.parent else {
                return names;
            };
            id = parent;
        }
    }

    pub fn rebuild(&mut self, symbols: &[SourceItem], dependencies: &[SourceItem]) {
        let expanded: HashSet<_> = (0..self.nodes.len())
            .filter(|&id| self.nodes[id].expanded)
            .map(|id| self.ancestry(id))
            .collect();
        let selected = self.selected.map(|id| self.ancestry(id));
        let mut tree = Self::default();
        let mut locations = vec![None; symbols.len()];
        let inclusion_targets: HashSet<_> = dependencies
            .iter()
            .map(|item| (item.range.start, include_key(entry(item, Kind::Import).1)))
            .collect();
        for (i, item) in symbols.iter().enumerate() {
            let (kind, mut name) = entry(item, Kind::Other);
            if kind == Kind::Include
                && matches!(item.action, SourceAction::Navigate)
                && inclusion_targets.contains(&(item.range.start, include_key(name)))
            {
                continue;
            }
            let table = matches!(kind, Kind::Field | Kind::Index)
                .then(|| name.rsplit_once('.'))
                .flatten();
            let parent = symbols[..i]
                .iter()
                .enumerate()
                .rev()
                .find_map(|(index, container)| {
                    let id = locations[index]?;
                    let (outer_kind, outer_name) = entry(container, Kind::Other);
                    match (&container.action, &item.action) {
                        (SourceAction::ScopedSymbol { extent }, action)
                            if !matches!(action, SourceAction::ProjectLocation { .. })
                                && extent.contains(&item.range.start) =>
                        {
                            Some(id)
                        }
                        (left, right)
                            if matches!(outer_kind, Kind::Table | Kind::WorkTable)
                                && table.is_some_and(|(owner, _)| {
                                    owner.eq_ignore_ascii_case(outer_name)
                                })
                                && same_file(left, right) =>
                        {
                            Some(id)
                        }
                        _ => None,
                    }
                });
            if let Some((_, member)) = table.filter(|_| parent.is_some()) {
                name = member;
            }
            let category = tree.folder(parent, kind.label(), kind);
            let (parent, name) = if kind == Kind::Include {
                tree.directories(category, name, kind)
            } else {
                (category, name)
            };
            let id = tree.append(Some(parent), name, kind, Some(Item::Symbol(i)), false);
            tree.nodes[id].value_type = item.value_type;
            if let SourceAction::ProjectLocation { path, .. } = &item.action {
                tree.nodes[id].origin = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned());
            }
            locations[i] = Some(id);
        }
        for (i, item) in dependencies.iter().enumerate() {
            let (kind, name) = entry(item, Kind::Import);
            let category = tree.folder(None, kind.label(), kind);
            let (parent, name) = tree.directories(category, name, kind);
            tree.append(
                Some(parent),
                name,
                kind,
                Some(Item::Dependency(i)),
                matches!(item.action, SourceAction::Unresolved(_)),
            );
        }
        let keys: Vec<_> = tree
            .nodes
            .iter()
            .map(|node| alphabetic(&node.name))
            .collect();
        let order = |ids: &mut [usize]| ids.sort_by(|a, b| keys[*a].cmp(&keys[*b]));
        order(&mut tree.roots);
        for id in 0..tree.nodes.len() {
            order(&mut tree.nodes[id].children);
            let path = tree.ancestry(id);
            tree.nodes[id].expanded = expanded.contains(&path);
            if Some(&path) == selected.as_ref() {
                tree.selected = Some(id);
            }
        }
        tree.flatten();
        *self = tree;
    }

    fn directories<'a>(
        &mut self,
        mut parent: usize,
        name: &'a str,
        kind: Kind,
    ) -> (usize, &'a str) {
        let name = name.strip_prefix("./").unwrap_or(name);
        let namespaced = kind == Kind::Import && !name.contains(['/', '\\']);
        let mut parts = name
            .split(|ch| matches!(ch, '/' | '\\') || namespaced && ch == '.')
            .peekable();
        // Deep or irregular paths retain their full spelling in a single row.
        if parts.clone().count() > 6 || parts.clone().any(str::is_empty) {
            return (parent, name);
        }
        while let Some(part) = parts.next() {
            if parts.peek().is_none() {
                return (parent, part);
            }
            parent = self.folder(Some(parent), part, kind);
        }
        (parent, name)
    }

    fn flatten(&mut self) {
        self.rows.clear();
        let mut pending: Vec<_> = self.roots.iter().rev().copied().collect();
        while let Some(id) = pending.pop() {
            self.rows.push(id);
            if self.nodes[id].expanded {
                pending.extend(self.nodes[id].children.iter().rev());
            }
        }
        if self.selected.is_none_or(|id| !self.rows.contains(&id)) {
            self.selected = self.rows.first().copied();
        }
    }

    pub fn set_expanded(&mut self, id: usize, expanded: bool) {
        self.nodes[id].expanded = expanded && !self.nodes[id].children.is_empty();
        self.flatten();
    }
}

fn include_key(name: &str) -> String {
    name.strip_prefix("./")
        .unwrap_or(name)
        .replace('\\', "/")
        .to_ascii_lowercase()
}

fn same_file(left: &SourceAction, right: &SourceAction) -> bool {
    match (left, right) {
        (
            SourceAction::ProjectLocation { path: a, .. },
            SourceAction::ProjectLocation { path: b, .. },
        ) => a == b,
        (SourceAction::ProjectLocation { .. }, _) | (_, SourceAction::ProjectLocation { .. }) => {
            false
        }
        _ => true,
    }
}
