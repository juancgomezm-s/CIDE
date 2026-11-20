//! Bounded metadata-only tab strips. A painted hit retains identities, not an
//! editor buffer. Selection and closing reuse the existing Workbench/close flow.
use super::{ffi::*, line_edit::EditTarget, paint, wide, Action, State};
use jc_app::{DocumentId, PaneId, TabId};

const MAX_VISIBLE: usize = 32;
#[derive(Default)]
pub(super) struct Scroll {
    first: usize,
    observed: Option<TabId>,
    capacity: usize,
    catalog_len: usize,
}
#[derive(Clone, Copy, PartialEq, Eq)]
struct Geometry {
    width: i32,
    height: i32,
    cw: i32,
    ch: i32,
    epoch: u64,
    comparing: bool,
}
impl Geometry {
    fn current(s: &State) -> Self {
        Self {
            width: s.width,
            height: s.height,
            cw: s.cell_width,
            ch: s.cell_height,
            epoch: s.interaction_epoch,
            comparing: s.git_code.active,
        }
    }
}
#[derive(Clone, Copy)]
enum Target {
    Select(TabId, DocumentId, u64),
    Close(TabId, DocumentId, u64),
    Previous,
    Next,
    List,
    New,
}
#[derive(Clone)]
struct Button {
    rect: Rect,
    label: Vec<u16>,
    selected: bool,
    dirty: bool,
    target: Target,
    tab_background: Option<Rect>,
}
#[derive(Clone)]
struct Strip {
    pane: PaneId,
    rect: Rect,
    geometry: Geometry,
    view: Option<EditTarget>,
    active_pane: PaneId,
    catalog_len: usize,
    first: usize,
    identities: Vec<TabId>,
    buttons: Vec<Button>,
}
#[derive(Default, Clone)]
pub(super) struct Bars {
    strips: Vec<Strip>,
}
fn index(pane: PaneId) -> usize {
    usize::from(pane == PaneId::Secondary)
}
fn inside(r: Rect, x: i32, y: i32) -> bool {
    x >= r.left && x < r.right && y >= r.top && y < r.bottom
}

pub(super) fn prepare(s: &mut State) -> Bars {
    let mut bars = Bars::default();
    let comparison_tab = if s.git_code.active {
        s.workbench.active_document().map(|doc| doc.tab_id)
    } else {
        None
    };
    for (pane, area) in paint::areas(s) {
        let rect = Rect {
            left: area.left,
            right: area.right,
            top: s.chrome_layout().top(),
            bottom: s.dpi.pixels(super::shell::TABS).min(s.height.max(0)),
        };
        let cw = s.cell_width.clamp(1, 8192);
        let available = (rect.right - rect.left).max(0);
        if available < cw * 2 || rect.bottom - rect.top < 4 {
            continue;
        }
        let tabs = s.workbench.tabs().tabs();
        let active = s
            .workbench
            .panes()
            .view(pane)
            .and_then(|v| v.location())
            .map(|v| v.tab);
        let active_index = active
            .and_then(|id| tabs.iter().position(|t| t.id == id))
            .unwrap_or(0);
        let arrow = cw * 2;
        let room = (available - 4 * arrow).max(0);
        let capacity = if room >= cw * 4 {
            ((room / (cw * 15)).max(1) as usize).min(MAX_VISIBLE)
        } else {
            0
        };
        let width = (room / capacity.max(1) as i32).min(cw * 26);
        let scroll = &mut s.tab_scroll[index(pane)];
        scroll.first = scroll.first.min(tabs.len().saturating_sub(capacity));
        if capacity == 0 {
            scroll.observed = None;
        } else if comparison_tab.is_some()
            || scroll.observed != active
            || scroll.capacity != capacity
            || scroll.catalog_len != tabs.len()
        {
            scroll.observed = active;
            if active_index < scroll.first {
                scroll.first = active_index;
            } else if active_index >= scroll.first + capacity {
                scroll.first = active_index.saturating_add(1).saturating_sub(capacity);
            }
        }
        scroll.capacity = capacity;
        scroll.catalog_len = tabs.len();
        let first = scroll.first;
        let end = first.saturating_add(capacity).min(tabs.len());
        let mut buttons = Vec::with_capacity(2 * capacity + 2);
        if capacity > 0 {
            buttons.push(Button {
                rect: Rect {
                    right: rect.left + arrow,
                    ..rect
                },
                label: wide("‹"),
                selected: false,
                dirty: false,
                target: Target::Previous,
                tab_background: None,
            });
            buttons.push(Button {
                rect: Rect {
                    left: rect.left + arrow,
                    right: rect.left + 2 * arrow,
                    ..rect
                },
                label: wide("›"),
                selected: false,
                dirty: false,
                target: Target::Next,
                tab_background: None,
            });
        }
        for (offset, tab) in tabs[first..end].iter().enumerate() {
            let Some(document) = s.workbench.document_by_tab(tab.id) else {
                continue;
            };
            let x = rect.left + 2 * arrow + offset as i32 * width;
            let close = cw * 2;
            let label = jc_app::source_services::display_ui_label(&tab.title, 64);
            let selected = Some(tab.id) == active;
            let revision = document.session.document().revision();
            let body = Rect {
                left: x,
                right: x + width - close,
                ..rect
            };
            let full = Rect {
                right: x + width - s.dpi.pixels(2),
                ..body
            };
            buttons.push(Button {
                rect: body,
                label: wide(&label),
                selected,
                dirty: tab.dirty,
                target: Target::Select(tab.id, document.id, revision),
                tab_background: Some(full),
            });
            buttons.push(Button {
                rect: Rect {
                    left: body.right,
                    right: full.right,
                    ..rect
                },
                label: wide(if tab.dirty && comparison_tab != Some(tab.id) {
                    "●"
                } else {
                    "×"
                }),
                selected,
                dirty: tab.dirty,
                target: Target::Close(tab.id, document.id, revision),
                tab_background: None,
            });
        }
        if capacity > 0 {
            buttons.push(Button {
                rect: Rect {
                    left: rect.right - 2 * arrow,
                    right: rect.right - arrow,
                    ..rect
                },
                label: wide("+"),
                selected: false,
                dirty: false,
                target: Target::New,
                tab_background: None,
            });
        }
        buttons.push(Button {
            rect: Rect {
                left: rect.right - arrow,
                ..rect
            },
            label: wide("⌄"),
            selected: false,
            dirty: false,
            target: Target::List,
            tab_background: None,
        });
        bars.strips.push(Strip {
            pane,
            rect,
            geometry: Geometry::current(s),
            view: s.view_edit_target(pane),
            active_pane: s.workbench.panes().active(),
            catalog_len: tabs.len(),
            first,
            identities: tabs[first..end].iter().map(|t| t.id).collect(),
            buttons,
        });
    }
    bars
}
impl Strip {
    fn current(&self, s: &State) -> bool {
        self.geometry == Geometry::current(s)
            && self.view == s.view_edit_target(self.pane)
            && self.active_pane == s.workbench.panes().active()
            && self.catalog_len == s.workbench.tabs().tabs().len()
            && self.identities.iter().enumerate().all(|(n, id)| {
                s.workbench
                    .tabs()
                    .tabs()
                    .get(self.first + n)
                    .is_some_and(|tab| tab.id == *id)
            })
            && paint::areas(s).iter().any(|(pane, area)| {
                *pane == self.pane && area.left == self.rect.left && area.right == self.rect.right
            })
    }
}
impl Bars {
    pub(super) fn tab_pixel(&self, pane: PaneId, id: TabId, close: bool) -> Option<(i32, i32)> {
        let strip = self.strips.iter().find(|s| s.pane == pane)?;
        let b = strip.buttons.iter().find(|b| match b.target {
            Target::Select(tab, ..) => tab == id && !close,
            Target::Close(tab, ..) => tab == id && close,
            _ => false,
        })?;
        Some((
            (b.rect.left + b.rect.right) / 2,
            (b.rect.top + b.rect.bottom) / 2,
        ))
    }
    pub(super) fn list_pixel(&self, pane: PaneId) -> Option<(i32, i32)> {
        let b = self
            .strips
            .iter()
            .find(|s| s.pane == pane)?
            .buttons
            .iter()
            .find(|b| matches!(b.target, Target::List))?;
        Some((
            (b.rect.left + b.rect.right) / 2,
            (b.rect.top + b.rect.bottom) / 2,
        ))
    }
}
impl State {
    /// None means the click did not address a tab strip. A rejected painted hit
    /// is consumed, so it cannot fall through into source selection.
    pub(super) fn tab_click(&mut self, x: i32, y: i32) -> Option<Action> {
        let bars = self.tab_hits.as_ref()?;
        let strip = bars.strips.iter().find(|strip| inside(strip.rect, x, y))?;
        let comparison_can_close = self.pointer_overlays_clear()
            && ((self.git_code.active && !self.open_diff_visible)
                || (self.open_diff_visible && self.omnicore_diff.is_some()));
        if !(self.pointer_source_available() || comparison_can_close) {
            return Some(Action::None);
        }
        if !strip.current(self) {
            self.status = "Cambió la barra de pestañas; vuelve a seleccionar".into();
            self.tab_hits = None;
            return Some(Action::None);
        }
        let pane = strip.pane;
        let Some(target) = strip
            .buttons
            .iter()
            .find(|b| inside(b.rect, x, y))
            .map(|b| b.target)
        else {
            return Some(Action::None);
        };
        if self.git_code.active && !matches!(target, Target::Close(..)) {
            return Some(Action::None);
        }
        match target {
            Target::New => {
                self.workbench.focus_editor_pane(pane);
                self.tab_hits = None;
                return Some(self.execute(jc_app::CommandId::FileNew));
            }
            Target::List => {
                self.workbench.focus_editor_pane(pane);
                self.explorer_focused = false;
                self.bottom.focused = false;
                self.open_selector(super::selector::SelectorKind::OpenTabs);
                self.tab_hits = None;
                return Some(Action::None);
            }
            Target::Previous | Target::Next => {
                let scroll = &mut self.tab_scroll[index(pane)];
                scroll.first = if matches!(target, Target::Previous) {
                    scroll.first.saturating_sub(1)
                } else {
                    scroll.first.saturating_add(1)
                };
            }
            Target::Select(tab, document, revision) | Target::Close(tab, document, revision) => {
                if !self.workbench.document_by_tab(tab).is_some_and(|d| {
                    d.id == document && d.session.document().revision() == revision
                }) {
                    self.status = "Cambió el documento de la pestaña; vuelve a seleccionar".into();
                    self.tab_hits = None;
                    return Some(Action::None);
                }
                if self.git_code.active {
                    return Some(
                        if self.workbench.active_document().map(|doc| doc.tab_id) == Some(tab) {
                            self.tab_hits = None;
                            self.close_git_code()
                        } else {
                            Action::None
                        },
                    );
                }
                if matches!(target, Target::Close(..)) && self.close_omnicore_diff_for(tab) {
                    self.tab_hits = None;
                    return Some(Action::None);
                }
                self.clear_omnicore_diff();
                let position = self
                    .workbench
                    .tabs()
                    .tabs()
                    .iter()
                    .position(|t| t.id == tab)?;
                self.cancel_pointer();
                self.workbench.focus_editor_pane(pane);
                self.workbench.select_tab(position + 1);
                self.git_follows_document();
                self.tab_scroll[index(pane)].observed = None;
                self.explorer_focused = false;
                self.bottom.focused = false;
                self.pending_surrogate = None;
                self.ensure_encoding_selector();
                if matches!(target, Target::Close(..)) {
                    self.tab_hits = None;
                    return Some(Action::CloseTab);
                }
                self.status = "Pestaña seleccionada · Ctrl+Tab siguiente · Ctrl+W cerrar".into();
            }
        }
        self.tab_hits = None;
        Some(Action::None)
    }
}
pub(super) unsafe fn draw(dc: Hdc, bars: &Bars) -> Result<(), String> {
    for strip in &bars.strips {
        paint::fill(dc, strip.rect, super::shell::SURFACE)?;
        for button in &strip.buttons {
            let background = if button.selected {
                super::shell::RAISED
            } else {
                super::shell::SURFACE
            };
            if let Some(full) = button.tab_background {
                super::shell::rounded_tab(dc, full, background)?;
                if !button.selected {
                    paint::fill(
                        dc,
                        Rect {
                            left: full.right - 1,
                            right: full.right,
                            top: full.top + 7,
                            bottom: full.bottom - 7,
                        },
                        super::shell::MUTED,
                    )?;
                }
            }
            let mut rect = button.rect;
            let alignment = if button.tab_background.is_some() {
                let padding = (rect.bottom - rect.top) / 5;
                rect.left += padding;
                rect.right -= padding / 2;
                0x8000 // DT_END_ELLIPSIS
            } else {
                1 // DT_CENTER
            };
            if rect.left >= rect.right {
                continue;
            }
            SetTextColor(
                dc,
                if button.dirty {
                    super::shell::ACCENT
                } else {
                    super::shell::TEXT
                },
            );
            let old_mode = SetBkMode(dc, 1);
            let result = DrawTextW(
                dc,
                button.label.as_ptr(),
                -1,
                &mut rect,
                alignment | 0x04 | 0x20 | 0x800,
            ); // VCENTER | SINGLELINE | NOPREFIX
            SetBkMode(dc, old_mode);
            if result == 0 {
                return Err(super::os_error("Dibujar texto de pestaña"));
            }
        }
    }
    Ok(())
}

#[link(name = "gdi32")]
extern "system" {
    fn SetBkMode(dc: Hdc, mode: i32) -> i32;
}

#[link(name = "user32")]
extern "system" {
    fn DrawTextW(dc: Hdc, text: *const u16, count: i32, rect: *mut Rect, format: u32) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;
    use jc_text::TextEncoding;
    fn state(count: usize) -> State {
        let mut s = State::new(&crate::GuiOptions::default()).unwrap();
        s.workbench = jc_app::Workbench::default();
        for i in 0..count {
            s.workbench
                .new_file(format!("pestaña-{i}-á.rs"), TextEncoding::Utf8);
        }
        s
    }
    fn click(s: &mut State, target: impl Fn(Target) -> bool) -> Option<Action> {
        let bars = prepare(s);
        let button = bars
            .strips
            .iter()
            .flat_map(|strip| &strip.buttons)
            .find(|b| target(b.target))
            .unwrap();
        let (x, y) = (button.rect.left + 1, button.rect.top + 5);
        s.tab_hits = Some(bars);
        s.tab_click(x, y)
    }
    #[test]
    fn tab_strip_overflow_reaches_hidden_tabs_and_keyboard_selection_reveals_active() {
        let mut s = state(40);
        s.width = 370;
        let selected = s.workbench.tabs().active_id().unwrap();
        let bars = prepare(&mut s);
        assert!(bars.strips[0]
            .buttons
            .iter()
            .any(|b| matches!(b.target,Target::Select(id,..) if id==selected)));
        for _ in 0..40 {
            click(&mut s, |t| matches!(t, Target::Previous));
            if s.tab_scroll[0].first == 0 {
                break;
            }
        }
        assert_eq!(s.tab_scroll[0].first, 0);
        let first = s.workbench.tabs().tabs()[0].id;
        click(&mut s, |t| matches!(t,Target::Select(id,..) if id==first));
        assert_eq!(s.workbench.tabs().active_id(), Some(first));
        s.workbench.last_tab();
        let bars = prepare(&mut s);
        assert!(bars.strips[0]
            .buttons
            .iter()
            .any(|b| matches!(b.target,Target::Select(id,..) if id==selected)));
        assert!(bars
            .strips
            .iter()
            .all(|s| s.identities.len() <= MAX_VISIBLE));
    }
    #[test]
    fn tab_strip_navigation_is_left_and_new_and_direct_list_are_right() {
        let mut s = state(3);
        s.width = 1600;
        let bars = prepare(&mut s);
        let strip = &bars.strips[0];
        let first = strip
            .buttons
            .iter()
            .find(|b| matches!(b.target, Target::Select(..)))
            .unwrap()
            .rect
            .left;
        let last = strip
            .buttons
            .iter()
            .filter(|b| matches!(b.target, Target::Close(..)))
            .map(|b| b.rect.right)
            .max()
            .unwrap();
        for b in &strip.buttons {
            match b.target {
                Target::Previous | Target::Next => assert!(b.rect.right <= first),
                Target::New | Target::List => assert!(b.rect.left >= last),
                _ => {}
            }
        }
        let count = s.workbench.tabs().tabs().len();
        click(&mut s, |target| matches!(target, Target::New));
        assert_eq!(s.workbench.tabs().tabs().len(), count + 1);
        assert_eq!(s.workbench.active_bytes().unwrap(), b"");
    }
    #[test]
    fn tab_strip_resize_reveals_active_without_resetting_manual_scroll() {
        let mut s = state(40);
        s.width = 1500;
        let active = s.workbench.tabs().active_id().unwrap();
        prepare(&mut s);
        s.width = 370;
        let bars = prepare(&mut s);
        assert!(bars.tab_pixel(PaneId::Primary, active, false).is_some());
        click(&mut s, |target| matches!(target, Target::Previous));
        let first = s.tab_scroll[0].first;
        prepare(&mut s);
        assert_eq!(s.tab_scroll[0].first, first);
        s.height = s.chrome_layout().top() + 1;
        assert!(prepare(&mut s).strips.is_empty());
    }
    #[test]
    fn tab_strip_rejects_stale_revision_resize_and_modal_without_touching_source() {
        let mut s = state(2);
        let bars = prepare(&mut s);
        let b = bars.strips[0]
            .buttons
            .iter()
            .find(|b| matches!(b.target, Target::Close(..)))
            .unwrap();
        let (x, y) = (b.rect.left + 1, b.rect.top + 5);
        s.tab_hits = Some(bars);
        s.width += 1;
        assert!(matches!(s.tab_click(x, y), Some(Action::None)));
        assert_eq!(s.workbench.tabs().tabs().len(), 2);
        let bars = prepare(&mut s);
        s.tab_hits = Some(bars);
        s.workbench.insert_text("cambio").unwrap();
        assert!(matches!(s.tab_click(x, y), Some(Action::None)));
        assert_eq!(s.workbench.tabs().tabs().len(), 2);
        let bars = prepare(&mut s);
        s.tab_hits = Some(bars);
        s.open_selector(super::super::selector::SelectorKind::Palette);
        assert!(matches!(s.tab_click(x, y), Some(Action::None)));
        assert_eq!(s.workbench.tabs().tabs().len(), 2);
    }
    #[test]
    fn tab_strip_split_click_preserves_independent_views_pin_bytes_and_undo() {
        let mut s = state(2);
        s.width = 1500;
        s.workbench.insert_text("base\r\n").unwrap();
        let tab = s.workbench.tabs().active_id().unwrap();
        s.workbench.split_editor_right();
        s.workbench.focus_editor_pane(PaneId::Secondary);
        s.workbench.toggle_reference_pin();
        s.workbench.focus_editor_pane(PaneId::Primary);
        let bars = prepare(&mut s);
        assert_eq!(bars.strips.len(), 2);
        let button = bars.strips[1]
            .buttons
            .iter()
            .find(|b| matches!(b.target,Target::Select(id,..) if id==tab))
            .unwrap();
        let (x, y) = (button.rect.left + 1, button.rect.top + 5);
        s.tab_hits = Some(bars);
        s.tab_click(x, y);
        assert_eq!(s.workbench.panes().active(), PaneId::Secondary);
        assert!(s
            .workbench
            .panes()
            .view(PaneId::Secondary)
            .unwrap()
            .is_pinned());
        assert_eq!(s.workbench.tabs().tabs().len(), 2);
        s.workbench.undo().unwrap();
        assert_eq!(
            s.workbench
                .active_document()
                .unwrap()
                .session
                .document()
                .len(),
            0
        );
        s.width = 330;
        assert_eq!(prepare(&mut s).strips.len(), 1);
    }
}
