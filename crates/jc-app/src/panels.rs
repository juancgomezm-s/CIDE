#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SidebarView {
    Explorer,
    Database,
    Git,
    GitStatus,
    Symbols,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BottomPanel {
    Problems,
    Output,
    SearchResults,
    Diff,
    Terminal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelManager {
    sidebar_visible: bool,
    sidebar: SidebarView,
    bottom_visible: bool,
    bottom: BottomPanel,
}

impl Default for PanelManager {
    fn default() -> Self {
        Self {
            sidebar_visible: true,
            sidebar: SidebarView::Explorer,
            bottom_visible: false,
            bottom: BottomPanel::Problems,
        }
    }
}

impl PanelManager {
    pub fn sidebar_visible(&self) -> bool {
        self.sidebar_visible
    }
    pub fn bottom_visible(&self) -> bool {
        self.bottom_visible
    }
    pub fn sidebar(&self) -> SidebarView {
        self.sidebar
    }
    pub fn bottom(&self) -> BottomPanel {
        self.bottom
    }

    pub fn show_sidebar(&mut self, view: SidebarView) {
        self.sidebar = view;
        self.sidebar_visible = true;
    }

    pub fn hide_sidebar(&mut self) {
        self.sidebar_visible = false;
    }

    pub fn show_bottom(&mut self, panel: BottomPanel) {
        self.bottom = panel;
        self.bottom_visible = true;
    }

    pub fn hide_bottom(&mut self) {
        self.bottom_visible = false;
    }

    pub fn toggle_bottom(&mut self) {
        self.bottom_visible = !self.bottom_visible;
    }
}
