use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TabId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tab {
    pub id: TabId,
    pub title: String,
    pub dirty: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseRequest {
    Closed(Tab),
    NeedsSave(TabId),
    NotFound,
}

#[derive(Debug, Default, Clone)]
pub struct TabManager {
    tabs: Vec<Tab>,
    active: Option<usize>,
    closed: VecDeque<Tab>,
    next_id: u64,
}

impl TabManager {
    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }
    pub fn active(&self) -> Option<&Tab> {
        self.active.and_then(|index| self.tabs.get(index))
    }
    pub fn active_id(&self) -> Option<TabId> {
        self.active().map(|tab| tab.id)
    }
    pub fn can_reopen(&self) -> bool {
        !self.closed.is_empty()
    }

    pub fn open(&mut self, title: impl Into<String>) -> TabId {
        let id = TabId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        self.tabs.push(Tab {
            id,
            title: title.into(),
            dirty: false,
        });
        self.active = Some(self.tabs.len() - 1);
        id
    }

    pub fn set_dirty(&mut self, id: TabId, dirty: bool) -> bool {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) else {
            return false;
        };
        tab.dirty = dirty;
        true
    }

    pub fn select(&mut self, id: TabId) -> bool {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return false;
        };
        self.active = Some(index);
        true
    }

    pub fn select_index(&mut self, one_based: usize) -> bool {
        if one_based == 0 || one_based > self.tabs.len() {
            return false;
        }
        self.active = Some(one_based - 1);
        true
    }

    pub fn select_last(&mut self) -> bool {
        if self.tabs.is_empty() {
            return false;
        }
        self.active = Some(self.tabs.len() - 1);
        true
    }

    pub fn next_tab(&mut self) -> Option<TabId> {
        if self.tabs.is_empty() {
            self.active = None;
            return None;
        }
        let index = (self.active.unwrap_or(0) + 1) % self.tabs.len();
        self.active = Some(index);
        self.active_id()
    }

    pub fn previous_tab(&mut self) -> Option<TabId> {
        if self.tabs.is_empty() {
            self.active = None;
            return None;
        }
        let current = self.active.unwrap_or(0);
        let index = if current == 0 {
            self.tabs.len() - 1
        } else {
            current - 1
        };
        self.active = Some(index);
        self.active_id()
    }

    pub fn request_close(&mut self, id: TabId) -> CloseRequest {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return CloseRequest::NotFound;
        };
        if self.tabs[index].dirty {
            return CloseRequest::NeedsSave(id);
        }
        CloseRequest::Closed(self.remove_at(index))
    }

    pub fn discard_and_close(&mut self, id: TabId) -> CloseRequest {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return CloseRequest::NotFound;
        };
        CloseRequest::Closed(self.remove_at(index))
    }

    pub fn reopen_last(&mut self) -> Option<TabId> {
        let mut tab = self.closed.pop_back()?;
        tab.dirty = false;
        let id = tab.id;
        self.tabs.push(tab);
        self.active = Some(self.tabs.len() - 1);
        Some(id)
    }

    fn remove_at(&mut self, index: usize) -> Tab {
        let tab = self.tabs.remove(index);
        self.closed.push_back(tab.clone());
        while self.closed.len() > 32 {
            self.closed.pop_front();
        }
        self.active = if self.tabs.is_empty() {
            None
        } else if index >= self.tabs.len() {
            Some(self.tabs.len() - 1)
        } else {
            Some(index)
        };
        tab
    }
}
