use crate::panels::PanelManager;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PrimaryFocus {
    Editor,
    Sidebar,
    BottomPanel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Overlay {
    Menu,
    Dialog,
    Find,
    Replace,
    CommandPalette,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EscapeOutcome {
    ClosedOverlay(Overlay),
    FocusedEditor,
    NoChange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusManager {
    primary: PrimaryFocus,
    overlays: Vec<Overlay>,
}

impl Default for FocusManager {
    fn default() -> Self {
        Self {
            primary: PrimaryFocus::Editor,
            overlays: Vec::new(),
        }
    }
}

impl FocusManager {
    pub fn primary(&self) -> PrimaryFocus { self.primary }
    pub fn active_overlay(&self) -> Option<Overlay> { self.overlays.last().copied() }
    pub fn overlays(&self) -> &[Overlay] { &self.overlays }

    pub fn focus(&mut self, focus: PrimaryFocus, panels: &PanelManager) -> bool {
        if focus == PrimaryFocus::Sidebar && !panels.sidebar_visible() { return false; }
        if focus == PrimaryFocus::BottomPanel && !panels.bottom_visible() { return false; }
        self.primary = focus;
        true
    }

    pub fn open_overlay(&mut self, overlay: Overlay) {
        self.overlays.push(overlay);
    }

    pub fn close_overlay(&mut self) -> Option<Overlay> {
        self.overlays.pop()
    }

    pub fn escape(&mut self) -> EscapeOutcome {
        if let Some(overlay) = self.overlays.pop() {
            return EscapeOutcome::ClosedOverlay(overlay);
        }
        if self.primary != PrimaryFocus::Editor {
            self.primary = PrimaryFocus::Editor;
            return EscapeOutcome::FocusedEditor;
        }
        EscapeOutcome::NoChange
    }

    pub fn next(&mut self, panels: &PanelManager) -> PrimaryFocus {
        self.cycle(panels, true)
    }

    pub fn previous(&mut self, panels: &PanelManager) -> PrimaryFocus {
        self.cycle(panels, false)
    }

    fn cycle(&mut self, panels: &PanelManager, forward: bool) -> PrimaryFocus {
        if self.active_overlay().is_some() {
            return self.primary;
        }
        let mut order = vec![PrimaryFocus::Editor];
        if panels.sidebar_visible() { order.push(PrimaryFocus::Sidebar); }
        if panels.bottom_visible() { order.push(PrimaryFocus::BottomPanel); }
        let current = order.iter().position(|value| *value == self.primary).unwrap_or(0);
        let next = if forward {
            (current + 1) % order.len()
        } else if current == 0 {
            order.len() - 1
        } else {
            current - 1
        };
        self.primary = order[next];
        self.primary
    }
}
