//! Two editor groups and independent navigation history.
//!
//! This holds only TabId + byte offsets. The canonical document and undo stack
//! remain owned by Workbench. Both UI frontends consume the same pane model.

use crate::TabId;
use jc_core::ByteEdit;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneId {
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

#[derive(Debug, Clone, Default)]
pub struct PaneView {
    location: Option<NavLocation>,
    anchor: Option<usize>,
    scroll_line: usize,
    scroll_column: usize,
    pinned: bool,
    back: Vec<NavLocation>,
    forward: Vec<NavLocation>,
}

const HISTORY_LIMIT: usize = 128;

fn push_limited(stack: &mut Vec<NavLocation>, location: NavLocation) {
    if stack.last() != Some(&location) {
        stack.push(location);
        if stack.len() > HISTORY_LIMIT {
            stack.remove(0);
        }
    }
}

impl PaneView {
    pub fn anchor(&self) -> Option<usize> {
        self.anchor
    }
    pub fn scroll_line(&self) -> usize {
        self.scroll_line
    }
    pub fn scroll_column(&self) -> usize {
        self.scroll_column
    }
    pub fn location(&self) -> Option<NavLocation> {
        self.location
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

    fn navigate(&mut self, destination: NavLocation) {
        if self.location == Some(destination) {
            return;
        }
        if let Some(previous) = self.location {
            push_limited(&mut self.back, previous);
        }
        self.location = Some(destination);
        self.anchor = None;
        self.scroll_line = 0;
        self.scroll_column = 0;
        self.forward.clear();
    }

    fn back(&mut self) -> Option<NavLocation> {
        let previous = self.back.pop()?;
        if let Some(current) = self.location {
            push_limited(&mut self.forward, current);
        }
        self.location = Some(previous);
        self.anchor = None;
        Some(previous)
    }

    fn forward(&mut self) -> Option<NavLocation> {
        let next = self.forward.pop()?;
        if let Some(current) = self.location {
            push_limited(&mut self.back, current);
        }
        self.location = Some(next);
        self.anchor = None;
        Some(next)
    }

    fn forget_tab(&mut self, tab: TabId, fallback: Option<NavLocation>) {
        self.back.retain(|location| location.tab != tab);
        self.forward.retain(|location| location.tab != tab);
        if self.location.is_some_and(|location| location.tab == tab) {
            self.location = fallback;
            self.anchor = None;
            self.scroll_line = 0;
            self.scroll_column = 0;
            self.pinned = false;
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PaneManager {
    secondary_open: bool,
    active: PaneId,
    primary: PaneView,
    secondary: PaneView,
    scroll_linked: bool,
}

// Default group is primary. The secondary group exists only after SplitRight.
impl Default for PaneId {
    fn default() -> Self {
        Self::Primary
    }
}

impl PaneManager {
    pub fn is_split(&self) -> bool {
        self.secondary_open
    }

    pub fn active(&self) -> PaneId {
        self.active
    }

    pub fn view(&self, pane: PaneId) -> Option<&PaneView> {
        if pane == PaneId::Secondary && !self.secondary_open {
            return None;
        }
        Some(match pane {
            PaneId::Primary => &self.primary,
            PaneId::Secondary => &self.secondary,
        })
    }

    pub fn scroll_linked(&self) -> bool {
        self.scroll_linked && self.secondary_open
    }

    pub fn split_right(&mut self) -> bool {
        if self.secondary_open {
            return false;
        }
        self.secondary_open = true;
        // Both views initially show the same document; neither owns its bytes.
        self.secondary = PaneView {
            location: self.primary.location,
            anchor: self.primary.anchor,
            scroll_line: self.primary.scroll_line,
            scroll_column: self.primary.scroll_column,
            ..PaneView::default()
        };
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

    pub fn focus(&mut self, pane: PaneId) -> Option<NavLocation> {
        let location = self.view(pane)?.location()?;
        self.active = pane;
        Some(location)
    }

    pub fn navigate(&mut self, location: NavLocation, automatic: bool) -> PaneId {
        let target = if automatic && self.secondary_open && self.active_view().is_pinned() {
            self.active.other()
        } else {
            self.active
        };
        self.view_mut(target).navigate(location);
        self.active = target;
        target
    }

    /// Remember a cursor move without creating a navigation-history entry.
    pub fn update_cursor(&mut self, location: NavLocation) {
        let pane = self.active_view_mut();
        if pane.location.is_some_and(|old| old.tab == location.tab) {
            pane.location = Some(location);
        }
    }

    pub(crate) fn store_active_view(
        &mut self,
        location: NavLocation,
        anchor: Option<usize>,
        scroll_line: usize,
    ) {
        self.update_cursor(location);
        let view = self.active_view_mut();
        view.anchor = anchor;
        view.scroll_line = scroll_line;
    }

    pub(crate) fn set_scroll(&mut self, pane: PaneId, line: usize, column: usize) {
        let view = self.view_mut(pane);
        view.scroll_line = line;
        view.scroll_column = column;
    }

    pub(crate) fn rebase(&mut self, tab: TabId, edit: &ByteEdit) {
        for id in [PaneId::Primary, PaneId::Secondary] {
            let view = self.view_mut(id);
            if let Some(location) = &mut view.location {
                if location.tab == tab {
                    location.byte_offset = rebase_offset(location.byte_offset, edit);
                    view.anchor = view.anchor.map(|offset| rebase_offset(offset, edit));
                }
            }
            for location in view.back.iter_mut().chain(&mut view.forward) {
                if location.tab == tab {
                    location.byte_offset = rebase_offset(location.byte_offset, edit);
                }
            }
        }
    }

    pub fn toggle_pin(&mut self) -> Option<bool> {
        if !self.secondary_open {
            return None;
        }
        let active = self.active;
        let pinned = {
            let pane = self.active_view_mut();
            pane.pinned = !pane.pinned;
            pane.pinned
        };
        // Keep only one reference anchored at once: automatic navigation
        // must always have an unpinned destination.
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
        self.active_view().can_go_back()
    }

    pub fn can_forward(&self) -> bool {
        self.active_view().can_go_forward()
    }

    pub fn go_back(&mut self) -> Option<NavLocation> {
        self.active_view_mut().back()
    }

    pub fn go_forward(&mut self) -> Option<NavLocation> {
        self.active_view_mut().forward()
    }

    pub fn forget_tab(&mut self, tab: TabId, fallback: Option<NavLocation>) {
        self.primary.forget_tab(tab, fallback);
        self.secondary.forget_tab(tab, fallback);
    }

    fn active_view(&self) -> &PaneView {
        self.view(self.active).expect("active pane always exists")
    }

    fn active_view_mut(&mut self) -> &mut PaneView {
        self.view_mut(self.active)
    }

    fn view_mut(&mut self, pane: PaneId) -> &mut PaneView {
        match pane {
            PaneId::Primary => &mut self.primary,
            PaneId::Secondary => &mut self.secondary,
        }
    }
}

fn rebase_offset(offset: usize, edit: &ByteEdit) -> usize {
    if offset < edit.replaced.start {
        offset
    } else if offset >= edit.replaced.end {
        offset - edit.replaced.len() + edit.inserted_len
    } else {
        edit.replaced.start + edit.inserted_len
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
