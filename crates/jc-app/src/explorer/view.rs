use super::{ExplorerNode, NodeId, ProjectExplorer};

#[derive(Default)]
pub struct ExplorerView {
    selected: Option<NodeId>,
    ancestors: Vec<NodeId>,
    generation: u64,
    scroll: usize,
    manual_scroll: bool,
}
pub struct ExplorerPresentation {
    pub title: String,
    pub message: String,
    pub rows: Vec<ExplorerRow>,
    pub selected: usize,
    pub total: usize,
    /// Global ordinal of the first visible row, independent of selection.
    pub first: usize,
    pub generation: u64,
    pub revision: u64,
}
pub struct ExplorerRow {
    pub id: NodeId,
    pub label: String,
    pub depth: usize,
    /// Visual indentation cells, capped independently of the semantic depth.
    pub indent: usize,
    pub is_dir: bool,
    pub expanded: bool,
    pub selected: bool,
}

pub(super) fn escaped(bytes: &[u8], width: usize) -> String {
    let limit = width.min(256);
    let mut output = String::with_capacity(limit);
    let mut at = 0;
    while at < bytes.len() && output.len() < limit {
        let len = match bytes[at] {
            0..=0x7f => 1,
            0xc2..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf4 => 4,
            _ => 0,
        };
        let ch = if len > 0 && len <= bytes.len() - at {
            std::str::from_utf8(&bytes[at..at + len])
                .ok()
                .and_then(|s| s.chars().next())
        } else {
            None
        };
        let token = match ch {
            Some('\\') => "\\\\".into(),
            Some(ch @ ' '..='~') => ch.to_string(),
            Some(ch) if ch.is_control() && (ch as u32) <= 127 => format!("\\x{:02X}", ch as u32),
            Some(ch) => format!("\\u{{{:X}}}", ch as u32),
            None => format!("\\x{:02X}", bytes[at]),
        };
        if token.len() > limit - output.len() {
            for _ in 0..(limit - output.len()).min(3) {
                output.push('.');
            }
            break;
        }
        output.push_str(&token);
        at += if ch.is_some() { len } else { 1 };
    }
    output
}
impl ExplorerView {
    pub fn selected(&self) -> Option<NodeId> {
        self.selected
    }
    pub fn select(&mut self, id: NodeId) {
        // A current frame may select before the first prepare for this root.
        // normalize still rejects an ID from any other catalog generation.
        self.generation = id.generation;
        self.selected = Some(id);
        self.manual_scroll = false;
    }
    fn normalize(&mut self, catalog: &ProjectExplorer) {
        if self.generation != catalog.generation() {
            self.generation = catalog.generation();
            self.selected = catalog.root_id();
            self.ancestors.clear();
            self.scroll = 0;
            self.manual_scroll = false;
        }
        let visible = self
            .selected
            .is_some_and(|id| catalog.visible_nodes().any(|n| n.id == id));
        if !visible {
            self.manual_scroll = false;
            self.selected = self
                .ancestors
                .iter()
                .rev()
                .copied()
                .find(|id| catalog.visible_nodes().any(|n| n.id == *id))
                .or(catalog.root_id());
        }
        self.ancestors.clear();
        let mut parent = self.selected;
        while let Some(id) = parent {
            let Some(node) = catalog.node(id) else {
                break;
            };
            self.ancestors.push(id);
            parent = node.parent;
        }
        self.ancestors.reverse();
    }
    pub fn move_selection(&mut self, catalog: &ProjectExplorer, delta: isize) {
        self.manual_scroll = false;
        self.normalize(catalog);
        let total = catalog.visible_nodes().count();
        let index = catalog
            .visible_nodes()
            .position(|n| Some(n.id) == self.selected)
            .unwrap_or(0);
        let next = super::shift_index(index, delta, total.saturating_sub(1));
        self.selected = catalog.visible_nodes().nth(next).map(|n| n.id);
        self.normalize(catalog);
    }
    pub fn first(&mut self, catalog: &ProjectExplorer) {
        self.manual_scroll = false;
        self.selected = catalog.root_id();
        self.normalize(catalog);
    }
    pub fn last(&mut self, catalog: &ProjectExplorer) {
        self.manual_scroll = false;
        self.normalize(catalog);
        self.selected = catalog.visible_nodes().last().map(|n| n.id);
        self.normalize(catalog);
    }
    pub fn parent(&mut self, catalog: &ProjectExplorer) {
        self.manual_scroll = false;
        self.normalize(catalog);
        if let Some(parent) = self
            .selected
            .and_then(|id| catalog.node(id))
            .and_then(ExplorerNode::parent)
        {
            self.selected = Some(parent);
        }
        self.normalize(catalog);
    }
    /// Viewport origin, never the selected node's ordinal.
    pub fn scroll(&self) -> usize {
        self.scroll
    }
    /// Move only the viewport. Keyboard selection explicitly reveals itself again.
    pub fn scroll_by(
        &mut self,
        catalog: &ProjectExplorer,
        delta: isize,
        visible_rows: usize,
    ) -> bool {
        self.normalize(catalog);
        let count = visible_rows.min(128);
        if count == 0 {
            return false;
        }
        let max = catalog.visible_nodes().count().saturating_sub(count);
        let target = super::shift_index(self.scroll.min(max), delta, max);
        self.scroll_to(catalog, target, count)
    }
    /// Absolute scrollbar intention; clamps before publication and never opens a node.
    pub fn scroll_to(
        &mut self,
        catalog: &ProjectExplorer,
        first: usize,
        visible_rows: usize,
    ) -> bool {
        self.normalize(catalog);
        let count = visible_rows.min(128);
        if count == 0 {
            return false;
        }
        let next = first.min(catalog.visible_nodes().count().saturating_sub(count));
        let changed = self.scroll != next;
        self.scroll = next;
        self.manual_scroll = true;
        changed
    }
    pub fn prepare(
        &mut self,
        catalog: &ProjectExplorer,
        max_rows: usize,
        max_cols: usize,
    ) -> ExplorerPresentation {
        self.normalize(catalog);
        let total = catalog.visible_nodes().count();
        let selected = catalog
            .visible_nodes()
            .position(|n| Some(n.id) == self.selected)
            .unwrap_or(0);
        let count = max_rows.min(128).min(total);
        let cols = max_cols.min(256);
        self.scroll = self.scroll.min(total.saturating_sub(count));
        if count > 0 && !self.manual_scroll {
            if selected < self.scroll {
                self.scroll = selected;
            } else if selected >= self.scroll + count {
                self.scroll = selected + 1 - count;
            }
        }
        let rows = catalog
            .visible_nodes()
            .skip(self.scroll)
            .take(count)
            .map(|node| {
                let name = if node.parent.is_none() {
                    catalog
                        .root()
                        .and_then(|r| r.file_name())
                        .unwrap_or_else(|| catalog.root().unwrap().as_os_str())
                } else {
                    node.relative.file_name().unwrap_or_default()
                };
                let prefix = if node.is_dir {
                    if node.expanded {
                        "[-] "
                    } else {
                        "[+] "
                    }
                } else {
                    "    "
                };
                let indent = node.depth.min(cols.saturating_sub(12) / 2) * 2;
                let mut label = " ".repeat(indent);
                if prefix.len() <= cols - label.len() {
                    label.push_str(prefix);
                    label.push_str(&escaped(name.as_encoded_bytes(), cols - label.len()));
                }
                ExplorerRow {
                    id: node.id,
                    label,
                    depth: node.depth,
                    indent,
                    is_dir: node.is_dir,
                    expanded: node.expanded,
                    selected: Some(node.id) == self.selected,
                }
            })
            .collect();
        ExplorerPresentation {
            title: crate::source_services::display_ui_label("Explorador", cols),
            message: crate::source_services::display_ui_label(
                &format!(
                    "{} | {} nodos | {} examinados/{} omitidos",
                    catalog.message,
                    catalog.nodes.len(),
                    catalog.examined,
                    catalog.skipped
                ),
                cols,
            ),
            rows,
            selected,
            total,
            first: self.scroll,
            generation: catalog.generation(),
            revision: catalog.revision(),
        }
    }
}
