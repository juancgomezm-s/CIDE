//! Presentation of the bounded commit graph, with lane continuity across pages.
use jc_git::graph::{Boundary, GraphCursor, GraphNode, GraphPage, Head};
use std::sync::Arc;

const LANES: usize = 12;
const UP: u8 = 1;
const DOWN: u8 = 2;
const LEFT: u8 = 4;
const RIGHT: u8 = 8;

#[derive(Clone)]
pub(crate) struct GraphNext(Arc<Continuation>);

struct Continuation {
    cursor: GraphCursor,
    lanes: Vec<Option<String>>,
}

impl PartialEq for GraphNext {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for GraphNext {}

impl GraphNext {
    pub(crate) fn cursor(&self) -> &GraphCursor {
        &self.0.cursor
    }
}

pub struct GraphEntry {
    pub commit: GraphNode,
    pub rail: String,
}

pub struct CommitGraph {
    pub head: Option<Head>,
    pub start: Option<String>,
    pub entries: Vec<GraphEntry>,
    pub shallow: bool,
    pub metadata_limited: bool,
    pub lanes_limited: bool,
    pub window_limited: bool,
    pub(crate) next: Option<GraphNext>,
}

impl CommitGraph {
    pub fn has_next(&self) -> bool {
        self.next.is_some()
    }
    pub fn heading(&self) -> String {
        let mut text = match &self.head {
            Some(Head::Unborn { .. }) => "La rama aún no tiene commits".to_owned(),
            Some(Head::Attached { name, .. }) => {
                let name = name.strip_prefix(b"refs/heads/").unwrap_or(name);
                format!(
                    "Rama {}",
                    crate::source_services::display_ui_label(&String::from_utf8_lossy(name), 72)
                )
            }
            Some(Head::Detached { .. }) => "HEAD separado".to_owned(),
            None => "Ascendencia del commit".to_owned(),
        };
        if self.shallow {
            text.push_str(" · historial superficial");
        }
        if self.metadata_limited {
            text.push_str(" · textos abreviados");
        }
        if self.lanes_limited {
            text.push_str(" · carriles abreviados");
        }
        if self.window_limited {
            text.push_str(" · ascendencia fuera del límite");
        }
        text
    }

    pub(crate) fn project(page: GraphPage, previous: Option<&GraphNext>) -> Self {
        let mut lanes = previous.map_or_else(Vec::new, |next| next.0.lanes.clone());
        let mut lanes_limited = false;
        let entries = page
            .nodes
            .into_iter()
            .map(|commit| {
                let (rail, limited) = draw_row(&mut lanes, &commit);
                lanes_limited |= limited;
                GraphEntry { commit, rail }
            })
            .collect();
        Self {
            head: page.head,
            start: page.start,
            entries,
            shallow: page.shallow,
            metadata_limited: page.metadata_limited,
            lanes_limited,
            window_limited: page.window_limited,
            next: page
                .next
                .map(|cursor| GraphNext(Arc::new(Continuation { cursor, lanes }))),
        }
    }
}

pub struct GraphView {
    pub showing: bool,
    pub selected: usize,
}

impl Default for GraphView {
    fn default() -> Self {
        Self {
            showing: true,
            selected: 0,
        }
    }
}

impl GraphView {
    pub fn move_selection(&mut self, count: usize, delta: isize) {
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(count.saturating_sub(1));
    }

    pub fn first(&self, count: usize, height: usize) -> usize {
        self.selected
            .saturating_sub(height.saturating_sub(1))
            .min(count.saturating_sub(height))
    }
}

fn draw_row(lanes: &mut Vec<Option<String>>, node: &GraphNode) -> (String, bool) {
    let at = lane(lanes, &node.id);
    let mut flags = [0u8; LANES];
    for (i, id) in lanes.iter().take(LANES).enumerate() {
        if id.is_some() && i != at {
            flags[i] = UP | DOWN;
        }
    }
    lanes[at] = None;
    let mut limited = lanes.len() > LANES;
    if matches!(node.boundary, Boundary::Interior | Boundary::Window) {
        for id in &node.parents {
            let to = if !lanes.iter().any(|slot| slot.as_deref() == Some(id)) && lanes[at].is_none()
            {
                lanes[at] = Some(id.clone());
                at
            } else {
                lane(lanes, id)
            };
            limited |= to >= LANES;
            let start = at.min(to);
            let end = at.max(to);
            for (i, bits) in flags
                .iter_mut()
                .enumerate()
                .take(end.saturating_add(1))
                .skip(start)
            {
                if i > start {
                    *bits |= LEFT;
                }
                if i < end {
                    *bits |= RIGHT;
                }
                if i == to {
                    *bits |= DOWN;
                }
            }
        }
    }
    while lanes.last().is_some_and(Option::is_none) {
        lanes.pop();
    }
    let mut rail: String = flags
        .into_iter()
        .enumerate()
        .map(|(i, bits)| {
            if i == at {
                match node.boundary {
                    Boundary::Shallow | Boundary::Window => '○',
                    _ if node.parents.len() > 1 => '◆',
                    _ => '●',
                }
            } else {
                match bits {
                    0 => ' ',
                    2 | 3 => '│',
                    6 => '┐',
                    10 => '┌',
                    12 => '─',
                    14 => '┬',
                    7 => '┤',
                    11 => '├',
                    _ => '┼',
                }
            }
        })
        .collect();
    if limited {
        rail.push(if at >= LANES { '●' } else { '…' });
    } else {
        rail.push(' ');
    }
    (rail, limited)
}

fn lane(lanes: &mut Vec<Option<String>>, id: &str) -> usize {
    if let Some(index) = lanes.iter().position(|slot| slot.as_deref() == Some(id)) {
        return index;
    }
    if let Some(index) = lanes.iter().position(Option::is_none) {
        lanes[index] = Some(id.to_owned());
        return index;
    }
    lanes.push(Some(id.to_owned()));
    lanes.len() - 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_rail_branches_and_joins_across_a_page_boundary() {
        let node = |id: &str, parents: &[&str]| GraphNode {
            id: id.into(),
            parents: parents.iter().map(|parent| parent.to_string()).collect(),
            summary: Vec::new(),
            author: Vec::new(),
            commit_time: 0,
            boundary: if parents.is_empty() {
                Boundary::Root
            } else {
                Boundary::Interior
            },
        };
        let mut lanes = Vec::new();
        assert!(draw_row(&mut lanes, &node("merge", &["left", "right"]))
            .0
            .starts_with("◆┐"));
        let mut next_page = lanes.clone();
        assert!(draw_row(&mut next_page, &node("left", &["base"]))
            .0
            .starts_with("●│"));
        assert!(draw_row(&mut next_page, &node("right", &["base"]))
            .0
            .starts_with("├●"));
        assert!(draw_row(&mut next_page, &node("base", &[]))
            .0
            .starts_with("●"));
        assert!(next_page.is_empty());
    }
}
