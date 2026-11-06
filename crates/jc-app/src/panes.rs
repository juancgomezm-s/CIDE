//! Groups own persistent views; documents and their history live in Workbench.
mod recovery;
use crate::TabId;
use jc_core::ChangeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PaneId {
    #[default]
    Primary,
    Secondary,
}
impl PaneId {
    fn other(self) -> Self {
        match self {
            Self::Primary => Self::Secondary,
            Self::Secondary => Self::Primary,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ViewId(pub u64);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NavLocation {
    pub tab: TabId,
    pub byte_offset: usize,
}
impl NavLocation {
    pub const fn new(tab: TabId, byte_offset: usize) -> Self {
        Self { tab, byte_offset }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RectangularSelection {
    pub anchor_column: crate::rectangle::VisualColumn,
    pub head_column: crate::rectangle::VisualColumn,
    pub scroll_column: crate::rectangle::VisualColumn,
    pub revision: u64,
}
#[derive(Debug, Clone, Default)]
pub struct ViewState {
    pub cursor: usize,
    pub anchor: Option<usize>,
    pub rectangle: Option<RectangularSelection>,
    pub scroll_line: usize,
    pub scroll_column: usize,
    pub manual_scroll: bool,
}
impl ViewState {
    pub fn selection(&self) -> Option<std::ops::Range<usize>> {
        if self.rectangle.is_some() {
            return None;
        }
        let anchor = self.anchor?;
        (anchor != self.cursor).then_some(anchor.min(self.cursor)..anchor.max(self.cursor))
    }
}
#[derive(Debug, Clone)]
struct EditorView {
    id: ViewId,
    tab: TabId,
    state: ViewState,
}
#[derive(Debug, Clone, Default)]
pub struct PaneView {
    views: Vec<EditorView>,
    active: Option<ViewId>,
    pinned: bool,
    back: Vec<NavLocation>,
    forward: Vec<NavLocation>,
}
const HISTORY_LIMIT: usize = 128;
fn push_limited(stack: &mut Vec<NavLocation>, loc: NavLocation) {
    if stack.last() != Some(&loc) {
        stack.push(loc);
        if stack.len() > HISTORY_LIMIT {
            stack.remove(0);
        }
    }
}
impl PaneView {
    fn editor(&self) -> Option<&EditorView> {
        self.views.iter().find(|v| Some(v.id) == self.active)
    }
    fn editor_mut(&mut self) -> Option<&mut EditorView> {
        self.views.iter_mut().find(|v| Some(v.id) == self.active)
    }
    pub fn view_id(&self) -> Option<ViewId> {
        self.active
    }
    pub fn state(&self) -> Option<&ViewState> {
        Some(&self.editor()?.state)
    }
    pub fn anchor(&self) -> Option<usize> {
        self.state()?.anchor
    }
    pub fn scroll_line(&self) -> usize {
        self.state().map_or(0, |s| s.scroll_line)
    }
    pub fn scroll_column(&self) -> usize {
        self.state().map_or(0, |s| s.scroll_column)
    }
    pub fn location(&self) -> Option<NavLocation> {
        let v = self.editor()?;
        Some(NavLocation::new(v.tab, v.state.cursor))
    }
    pub fn is_pinned(&self) -> bool {
        self.pinned
    }
    pub fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }
    pub fn can_go_forward(&self) -> bool {
        !self.forward.is_empty()
    }
    fn select(&mut self, loc: NavLocation, next: &mut u64, jump: bool) {
        if let Some(view) = self.views.iter_mut().find(|v| v.tab == loc.tab) {
            self.active = Some(view.id);
            if jump {
                view.state.cursor = loc.byte_offset;
                view.state.anchor = None;
                view.state.rectangle = None;
                view.state.manual_scroll = false;
            }
        } else {
            let id = ViewId(*next);
            *next += 1;
            self.views.push(EditorView {
                id,
                tab: loc.tab,
                state: ViewState {
                    cursor: loc.byte_offset,
                    ..ViewState::default()
                },
            });
            self.active = Some(id);
        }
    }
}
#[derive(Debug, Clone)]
pub struct PaneManager {
    secondary_open: bool,
    active: PaneId,
    primary: PaneView,
    secondary: PaneView,
    scroll_linked: bool,
    next_view: u64,
}
impl Default for PaneManager {
    fn default() -> Self {
        Self {
            secondary_open: false,
            active: PaneId::Primary,
            primary: PaneView::default(),
            secondary: PaneView::default(),
            scroll_linked: false,
            next_view: 1,
        }
    }
}
impl PaneManager {
    pub(crate) fn state_for_view(&self, id: ViewId) -> Option<&ViewState> {
        self.primary
            .views
            .iter()
            .chain(&self.secondary.views)
            .find(|view| view.id == id)
            .map(|view| &view.state)
    }
    pub(crate) fn state_for_view_mut(&mut self, id: ViewId) -> Option<&mut ViewState> {
        self.primary
            .views
            .iter_mut()
            .chain(&mut self.secondary.views)
            .find(|view| view.id == id)
            .map(|view| &mut view.state)
    }
    pub(crate) fn tab_for_view(&self, id: ViewId) -> Option<TabId> {
        self.primary
            .views
            .iter()
            .chain(&self.secondary.views)
            .find(|view| view.id == id)
            .map(|view| view.tab)
    }
    pub fn is_split(&self) -> bool {
        self.secondary_open
    }
    pub fn active(&self) -> PaneId {
        self.active
    }
    pub fn view(&self, id: PaneId) -> Option<&PaneView> {
        if id == PaneId::Secondary && !self.secondary_open {
            return None;
        }
        Some(match id {
            PaneId::Primary => &self.primary,
            PaneId::Secondary => &self.secondary,
        })
    }
    fn view_mut(&mut self, id: PaneId) -> &mut PaneView {
        match id {
            PaneId::Primary => &mut self.primary,
            PaneId::Secondary => &mut self.secondary,
        }
    }
    pub(crate) fn active_state_mut(&mut self) -> Option<&mut ViewState> {
        Some(&mut self.view_mut(self.active).editor_mut()?.state)
    }
    pub(crate) fn remembered_cursor(&self, tab: TabId) -> Option<usize> {
        self.view(self.active)
            .and_then(|p| p.views.iter().find(|v| v.tab == tab))
            .map(|v| v.state.cursor)
    }
    pub fn scroll_linked(&self) -> bool {
        self.scroll_linked && self.secondary_open
    }
    pub fn split_right(&mut self) -> bool {
        if self.secondary_open {
            return false;
        }
        self.secondary_open = true;
        if let Some(source) = self.primary.editor().cloned() {
            self.secondary.select(
                NavLocation::new(source.tab, source.state.cursor),
                &mut self.next_view,
                false,
            );
            self.secondary.editor_mut().unwrap().state = source.state;
        }
        true
    }
    pub fn close_secondary(&mut self) -> bool {
        if !self.secondary_open {
            return false;
        }
        self.secondary_open = false;
        self.secondary = PaneView::default();
        self.active = PaneId::Primary;
        self.scroll_linked = false;
        true
    }
    pub fn focus(&mut self, id: PaneId) -> Option<NavLocation> {
        let loc = self.view(id)?.location()?;
        self.active = id;
        Some(loc)
    }
    pub fn navigate(&mut self, loc: NavLocation, automatic: bool) -> PaneId {
        let target = if automatic && self.secondary_open && self.view(self.active).unwrap().pinned {
            self.active.other()
        } else {
            self.active
        };
        let previous = self.view(target).and_then(PaneView::location);
        let mut next = self.next_view;
        let group = self.view_mut(target);
        if previous != Some(loc) {
            if let Some(prev) = previous {
                push_limited(&mut group.back, prev);
            }
            group.forward.clear();
        }
        group.select(loc, &mut next, automatic);
        self.next_view = next;
        self.active = target;
        target
    }
    pub fn update_cursor(&mut self, loc: NavLocation) {
        if let Some(v) = self.view_mut(self.active).editor_mut() {
            if v.tab == loc.tab {
                v.state.cursor = loc.byte_offset;
            }
        }
    }
    pub(crate) fn set_scroll(&mut self, id: PaneId, line: usize, column: usize) {
        if let Some(v) = self.view_mut(id).editor_mut() {
            v.state.scroll_line = line;
            v.state.scroll_column = column;
        }
    }
    pub(crate) fn set_manual_scroll(&mut self, id: PaneId, manual: bool) {
        if let Some(view) = self.view_mut(id).editor_mut() {
            view.state.manual_scroll = manual;
        }
    }
    pub(crate) fn set_horizontal_scroll(&mut self, id: PaneId, column: usize) {
        if let Some(view) = self.view_mut(id).editor_mut() {
            if let Some(selection) = &mut view.state.rectangle {
                selection.scroll_column = crate::rectangle::VisualColumn(column);
            } else {
                view.state.scroll_column = column;
            }
            view.state.manual_scroll = true;
        }
    }
    pub(crate) fn rebase(&mut self, tab: TabId, change: &ChangeSet, include_active: bool) {
        let active = self.active;
        for id in [PaneId::Primary, PaneId::Secondary] {
            let group = self.view_mut(id);
            for v in &mut group.views {
                if v.tab == tab && v.state.rectangle.take().is_some() {
                    v.state.anchor = None;
                }
                if v.tab == tab && (include_active || !(id == active && Some(v.id) == group.active))
                {
                    v.state.cursor = change.map_offset(v.state.cursor);
                    v.state.anchor = v.state.anchor.map(|n| change.map_offset(n));
                }
            }
            for loc in group.back.iter_mut().chain(&mut group.forward) {
                if loc.tab == tab {
                    loc.byte_offset = change.map_offset(loc.byte_offset);
                }
            }
        }
    }
    pub fn toggle_pin(&mut self) -> Option<bool> {
        if !self.secondary_open {
            return None;
        }
        let active = self.active;
        let group = self.view_mut(active);
        group.pinned = !group.pinned;
        let pinned = group.pinned;
        if pinned {
            self.view_mut(active.other()).pinned = false;
        }
        Some(pinned)
    }
    pub fn toggle_linked_scroll(&mut self) -> Option<bool> {
        if !self.secondary_open {
            return None;
        }
        self.scroll_linked = !self.scroll_linked;
        Some(self.scroll_linked)
    }
    pub fn can_back(&self) -> bool {
        self.view(self.active).unwrap().can_go_back()
    }
    pub fn can_forward(&self) -> bool {
        self.view(self.active).unwrap().can_go_forward()
    }
    pub fn go_back(&mut self) -> Option<NavLocation> {
        self.history(false)
    }
    pub fn go_forward(&mut self) -> Option<NavLocation> {
        self.history(true)
    }
    fn history(&mut self, forward: bool) -> Option<NavLocation> {
        let mut next = self.next_view;
        let group = self.view_mut(self.active);
        let loc = if forward {
            group.forward.pop()?
        } else {
            group.back.pop()?
        };
        if let Some(current) = group.location() {
            push_limited(
                if forward {
                    &mut group.back
                } else {
                    &mut group.forward
                },
                current,
            );
        }
        group.select(loc, &mut next, true);
        self.next_view = next;
        Some(loc)
    }
    pub(crate) fn clear_rectangles(&mut self, tab: TabId) {
        for group in [&mut self.primary, &mut self.secondary] {
            for view in &mut group.views {
                if view.tab == tab && view.state.rectangle.take().is_some() {
                    view.state.anchor = None;
                }
            }
        }
    }
    pub(crate) fn normalize_with(&mut self, tab: TabId, mut snap: impl FnMut(usize) -> usize) {
        for group in [&mut self.primary, &mut self.secondary] {
            for view in &mut group.views {
                if view.tab == tab {
                    view.state.cursor = snap(view.state.cursor);
                    view.state.anchor = view.state.anchor.map(|n| if n == 0 { 0 } else { snap(n) });
                }
            }
            for loc in group.back.iter_mut().chain(&mut group.forward) {
                if loc.tab == tab {
                    loc.byte_offset = snap(loc.byte_offset);
                }
            }
        }
    }
    pub(crate) fn view_count(&self, tab: TabId) -> usize {
        self.primary
            .views
            .iter()
            .chain(&self.secondary.views)
            .filter(|v| v.tab == tab)
            .count()
    }
    pub(crate) fn close_current(&mut self) -> Option<NavLocation> {
        let group = self.view_mut(self.active);
        let id = group.active?;
        group.views.retain(|v| v.id != id);
        group.active = group.views.last().map(|v| v.id);
        group.pinned = false;
        group.location()
    }
    pub fn forget_tab(&mut self, tab: TabId, fallback: Option<NavLocation>) {
        for id in [PaneId::Primary, PaneId::Secondary] {
            let mut next = self.next_view;
            let group = self.view_mut(id);
            let removed = group.location().is_some_and(|loc| loc.tab == tab);
            group.views.retain(|v| v.tab != tab);
            group.back.retain(|loc| loc.tab != tab);
            group.forward.retain(|loc| loc.tab != tab);
            if removed {
                group.active = None;
                group.pinned = false;
                if let Some(loc) = fallback {
                    group.select(loc, &mut next, false);
                }
            }
            self.next_view = next;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_views_share_tab_identity_not_document_copies() {
        let a = NavLocation::new(TabId(1), 12);
        let mut manager = PaneManager::default();
        manager.navigate(a, false);
        assert!(manager.split_right());
        assert_eq!(manager.view(PaneId::Primary).unwrap().location(), Some(a));
        assert_eq!(manager.view(PaneId::Secondary).unwrap().location(), Some(a));
        assert!(!manager.split_right());
        assert!(manager.close_secondary());
        assert!(manager.view(PaneId::Secondary).is_none());
    }

    #[test]
    fn pinned_reference_is_not_replaced_by_automatic_navigation() {
        let a = NavLocation::new(TabId(1), 0);
        let b = NavLocation::new(TabId(2), 9);
        let mut manager = PaneManager::default();
        manager.navigate(a, false);
        manager.split_right();
        manager.focus(PaneId::Secondary);
        assert_eq!(manager.toggle_pin(), Some(true));
        assert_eq!(manager.navigate(b, true), PaneId::Primary);
        assert_eq!(manager.view(PaneId::Secondary).unwrap().location(), Some(a));
        assert_eq!(manager.view(PaneId::Primary).unwrap().location(), Some(b));
    }

    #[test]
    fn navigation_history_is_group_local_and_tracks_positions() {
        let a = NavLocation::new(TabId(1), 3);
        let b = NavLocation::new(TabId(2), 14);
        let c = NavLocation::new(TabId(3), 8);
        let mut manager = PaneManager::default();
        manager.navigate(a, false);
        manager.split_right();
        manager.navigate(b, true);
        manager.update_cursor(NavLocation::new(TabId(2), 19));
        manager.navigate(c, true);
        assert_eq!(manager.go_back(), Some(NavLocation::new(TabId(2), 19)));
        assert_eq!(manager.go_forward(), Some(c));
        manager.focus(PaneId::Secondary);
        assert!(!manager.can_back());
        assert_eq!(manager.view(PaneId::Secondary).unwrap().location(), Some(a));
    }

    #[test]
    fn forgetting_a_closed_tab_clears_stale_history_and_pins() {
        let a = NavLocation::new(TabId(1), 0);
        let b = NavLocation::new(TabId(2), 5);
        let mut manager = PaneManager::default();
        manager.navigate(a, false);
        manager.split_right();
        manager.focus(PaneId::Secondary);
        manager.toggle_pin();
        manager.forget_tab(TabId(1), Some(b));
        assert_eq!(manager.view(PaneId::Secondary).unwrap().location(), Some(b));
        assert!(!manager.view(PaneId::Secondary).unwrap().is_pinned());
        assert!(!manager.can_back());
    }

    #[test]
    fn pinning_second_group_unpins_first_and_preserves_an_automatic_target() {
        let first = NavLocation::new(TabId(1), 0);
        let second = NavLocation::new(TabId(2), 0);
        let mut manager = PaneManager::default();
        manager.navigate(first, false);
        manager.split_right();
        manager.toggle_pin();
        assert!(manager.view(PaneId::Primary).unwrap().is_pinned());
        manager.focus(PaneId::Secondary);
        manager.toggle_pin();
        assert!(!manager.view(PaneId::Primary).unwrap().is_pinned());
        assert!(manager.view(PaneId::Secondary).unwrap().is_pinned());
        assert_eq!(manager.navigate(second, true), PaneId::Primary);
        assert_eq!(
            manager.view(PaneId::Secondary).unwrap().location(),
            Some(first)
        );
    }

    #[test]
    fn scroll_link_is_independent_of_reference_pin() {
        let mut manager = PaneManager::default();
        assert_eq!(manager.toggle_linked_scroll(), None);
        manager.split_right();
        assert_eq!(manager.toggle_linked_scroll(), Some(true));
        assert_eq!(manager.toggle_pin(), Some(true));
        assert!(manager.scroll_linked());
        assert!(manager.close_secondary());
        assert!(!manager.scroll_linked());
    }
}
