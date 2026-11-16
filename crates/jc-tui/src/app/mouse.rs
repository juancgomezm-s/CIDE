//! Versioned terminal hit metadata. No live document, decoded text or I/O in draw.
use super::{
    presentation::{EditorText, FrameLayout, ScrollBar},
    TuiApp,
};
use crate::state::UiOverlay;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use jc_app::{
    explorer::NodeId, BottomPanel, CommandId, MenuId, PaneId, PrimaryFocus, SidebarView, TabId,
    TaskStamp,
};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}
impl Rect {
    pub fn contains(self, x: u16, y: u16) -> bool {
        x >= self.x
            && y >= self.y
            && x < self.x.saturating_add(self.width)
            && y < self.y.saturating_add(self.height)
    }
    fn row(self, row: u16) -> Self {
        Self {
            y: self.y.saturating_add(row),
            height: 1,
            ..self
        }
    }
}
#[derive(Clone, PartialEq, Eq)]
struct SourceContext {
    stamp: TaskStamp,
    pane: PaneId,
    path: PathBuf,
    root: Option<PathBuf>,
    first: usize,
    column: usize,
    focus: PrimaryFocus,
    active: PaneId,
    cursor: usize,
    anchor: Option<usize>,
    invisibles: bool,
    rectangle: bool,
}
impl SourceContext {
    fn capture(app: &TuiApp, pane: PaneId) -> Option<Self> {
        let view = app.workbench.panes().view(pane)?;
        let state = view.state()?;
        let doc = app.workbench.document_by_tab(view.location()?.tab)?;
        Some(Self {
            stamp: TaskStamp {
                document: doc.id,
                revision: doc.session.document().revision(),
                encoding: doc.session.encoding()?,
                view: view.view_id(),
            },
            pane,
            path: doc.session.path().to_owned(),
            root: app.project_root.clone(),
            first: view.scroll_line(),
            column: app.workbench.horizontal_scroll_for_pane(pane)?,
            focus: app.focus.primary(),
            active: app.workbench.panes().active(),
            cursor: state.cursor,
            anchor: state.anchor,
            invisibles: app.workbench.invisibles_visible_for_tab(doc.tab_id),
            rectangle: state.rectangle.is_some(),
        })
    }
    fn same_source(&self, other: &Self) -> bool {
        self.stamp == other.stamp
            && self.pane == other.pane
            && self.path == other.path
            && self.root == other.root
            && self.focus == other.focus
            && self.active == other.active
            && self.invisibles == other.invisibles
            && self.cursor == other.cursor
            && self.anchor == other.anchor
            && self.rectangle == other.rectangle
    }
}
#[derive(Clone)]
struct SourceArea {
    area: Rect,
    text_x: u16,
    context: SourceContext,
    tab: TabId,
    rows: Vec<bool>,
}
impl SourceArea {
    fn byte_at(&self, app: &TuiApp, x: u16, y: u16, clamp: bool) -> Option<usize> {
        if !clamp && !self.area.contains(x, y) {
            return None;
        }
        let row = usize::from(
            y.clamp(
                self.area.y,
                self.area.y + self.area.height.saturating_sub(1),
            ) - self.area.y,
        );
        if !self.rows.get(row).copied().unwrap_or(false) {
            return None;
        }
        let x = x.clamp(self.area.x, self.area.x + self.area.width.saturating_sub(1));
        let column = self
            .context
            .column
            .saturating_add(usize::from(x.saturating_sub(self.text_x)));
        let line = self.context.first.saturating_add(row);
        if self.context.invisibles {
            let byte = app.workbench.display_byte_at(self.tab, line, column)?;
            let (content, _) = app.workbench.line_range_for_tab(self.tab, line)?;
            let byte = byte.clamp(content.start, content.end);
            let visual = app.workbench.visual_column_at_byte(self.tab, line, byte)?;
            app.workbench.visual_byte_at(self.tab, line, visual)
        } else {
            app.workbench.visual_byte_at(self.tab, line, column)
        }
    }
}
#[derive(Clone, PartialEq, Eq)]
struct FrameStamp {
    code: (bool, bool, bool, usize),
    graph_start: Option<String>,
    root: Option<PathBuf>,
    overlay: UiOverlay,
    focus: PrimaryFocus,
    active: PaneId,
    sidebar: (bool, SidebarView),
    bottom: (bool, BottomPanel),
    explorer: (u64, u64, usize),
    browser: (u64, u64),
    sources: [Option<SourceContext>; 2],
    tabs: Vec<TabId>,
}
impl FrameStamp {
    fn capture(app: &TuiApp) -> Self {
        Self {
            code: (
                app.git_code.active,
                app.git_code.choose_commit,
                app.git_code.unified,
                app.git_diff_scroll,
            ),
            graph_start: app.git_revision.graph().and_then(|g| g.start.clone()),
            root: app.project_root.clone(),
            overlay: app.overlay.clone(),
            focus: app.focus.primary(),
            active: app.workbench.panes().active(),
            sidebar: (app.panels.sidebar_visible(), app.panels.sidebar()),
            bottom: (app.panels.bottom_visible(), app.panels.bottom()),
            explorer: (
                app.explorer.generation(),
                app.explorer.revision(),
                app.explorer_view.scroll(),
            ),
            browser: (
                app.file_browser.catalog.generation(),
                app.file_browser.catalog.revision(),
            ),
            sources: [
                SourceContext::capture(app, PaneId::Primary),
                SourceContext::capture(app, PaneId::Secondary),
            ],
            tabs: app
                .workbench
                .tabs()
                .tabs()
                .iter()
                .take(128)
                .map(|tab| tab.id)
                .collect(),
        }
    }
    fn current(&self, app: &TuiApp, wheel: bool) -> bool {
        let mut now = Self::capture(app);
        if wheel {
            for (before, after) in self.sources.iter().zip(&mut now.sources) {
                if let (Some(before), Some(after)) = (before, after) {
                    if before.same_source(after) {
                        after.first = before.first;
                        after.column = before.column;
                    }
                }
            }
        }
        *self == now
    }
}
#[derive(Clone, Copy)]
enum Action {
    Code(super::git_code::Control),
    CodeCommit(usize),
    CodeBack,
    Menu(MenuId),
    Command(CommandId),
    Tab(usize, TabId),
    Source(usize),
    Scroll(PaneId, ScrollBar),
    Explorer(NodeId),
    ExplorerScroll(ScrollBar),
    Bottom(usize),
    Symbol(usize),
    SymbolCrumb(usize),
    GitStatus(usize),
    GitHunk(usize),
    Focus(PrimaryFocus),
    BrowserNode(NodeId),
    BrowserPath,
    BrowserActivate,
    BrowserAccept,
    BrowserParent,
    BrowserCancel,
}
struct Hit {
    area: Rect,
    action: Action,
}
#[derive(Clone, Copy)]
enum Wheel {
    Source(usize),
    Explorer(usize),
    Browser,
    Bottom,
    Menu,
    Symbols,
    GitStatus,
    GitHunks,
}
struct WheelHit {
    area: Rect,
    action: Wheel,
}
struct MouseFrame {
    stamp: FrameStamp,
    hits: Vec<Hit>,
    wheels: Vec<WheelHit>,
    sources: Vec<SourceArea>,
    live: bool,
    menu_open: bool,
}
struct LinearDrag {
    source: SourceArea,
    context: SourceContext,
    anchor: usize,
}
struct ScrollDrag {
    context: SourceContext,
    bar: ScrollBar,
    grab: u16,
    previous: u16,
}
struct ExplorerScrollDrag {
    stamp: FrameStamp,
    bar: ScrollBar,
    grab: u16,
    rows: usize,
}
#[derive(Default)]
pub(crate) struct MouseState {
    frame: Option<Box<MouseFrame>>,
    drag: Option<LinearDrag>,
    scroll_drag: Option<ScrollDrag>,
    explorer_drag: Option<ExplorerScrollDrag>,
}
impl MouseState {
    pub(super) fn frame_changed(&mut self) {
        self.frame = None;
    }
}

pub(crate) struct TabLabel {
    pub x: u16,
    pub width: u16,
    pub text: String,
    pub tab: TabId,
    pub index: usize,
}
pub(crate) fn tab_labels(app: &TuiApp, width: u16) -> Vec<TabLabel> {
    let mut x = 0;
    app.workbench
        .tabs()
        .tabs()
        .iter()
        .take(128)
        .enumerate()
        .map_while(|(index, tab)| {
            if x >= width {
                return None;
            }
            let dirty = if tab.dirty { "*" } else { "" };
            let text = jc_text::grapheme::clip(
                &format!(" {}:{}{} ", index + 1, tab.title, dirty),
                usize::from(width - x),
            );
            let span = jc_text::grapheme::width(&text).min(usize::from(width - x)) as u16;
            let label = TabLabel {
                x,
                width: span,
                text,
                tab: tab.id,
                index: index + 1,
            };
            x += span;
            Some(label)
        })
        .collect()
}
pub(crate) fn browser_bounds(width: u16, height: u16) -> Rect {
    let width2 = width.saturating_sub(2).clamp(4, 112);
    let height2 = height.saturating_sub(2).clamp(3, 26);
    Rect {
        x: width.saturating_sub(width2) / 2,
        y: height.saturating_sub(height2) / 2,
        width: width2,
        height: height2,
    }
}
pub(crate) fn browser_buttons(folder: bool) -> &'static [(&'static str, u8)] {
    if folder {
        &[
            ("[Entrar]", 0),
            ("[Elegir carpeta actual]", 1),
            ("[Subir]", 2),
            ("[Cancelar]", 3),
        ]
    } else {
        &[("[Abrir selección]", 0), ("[Subir]", 2), ("[Cancelar]", 3)]
    }
}

impl TuiApp {
    pub(crate) fn invalidate_mouse(&mut self) {
        self.mouse.frame = None;
        self.mouse.drag = None;
        self.mouse.scroll_drag = None;
        self.mouse.explorer_drag = None;
    }
    fn mouse_source_available(&self) -> bool {
        matches!(self.overlay, UiOverlay::None)
            && !self.open_diff_visible
            && self.markdown_ui.is_none()
            && self.snippet_ui.is_none()
            && !self.source_popup
    }
    pub(crate) fn prepare_mouse(
        &mut self,
        layout: FrameLayout,
        primary: &EditorText,
        secondary: &EditorText,
        explorer: &jc_app::explorer::ExplorerPresentation,
        aux: (
            &jc_app::source_services::SourcePresentation,
            &super::GitStatusPresentation,
            &super::GitPresentation,
        ),
    ) {
        let (symbols, git_status, git) = aux;
        if self.mouse.explorer_drag.as_ref().is_some_and(|drag| {
            !self.mouse_source_available()
                || !drag.stamp.current(self, false)
                || !super::explorer_scrollbar(
                    0,
                    2,
                    layout.sidebar_width,
                    layout.content_height,
                    explorer,
                )
                .is_some_and(|bar| {
                    bar.x == drag.bar.x && bar.y == drag.bar.y && bar.length == drag.bar.length
                })
        }) {
            self.mouse.explorer_drag = None;
        }
        if self.mouse.drag.as_ref().is_some_and(|drag| {
            !self.mouse_source_available()
                || SourceContext::capture(self, drag.context.pane).as_ref() != Some(&drag.context)
        }) {
            self.mouse.drag = None;
        }
        if self.mouse.scroll_drag.as_ref().is_some_and(|drag| {
            let editor = if drag.context.pane == PaneId::Primary {
                primary
            } else {
                secondary
            };
            !self.mouse_source_available()
                || SourceContext::capture(self, drag.context.pane).as_ref() != Some(&drag.context)
                || !editor.bars.is_some_and(|bars| {
                    bars.iter().any(|bar| {
                        bar.vertical == drag.bar.vertical
                            && bar.x == drag.bar.x
                            && bar.y == drag.bar.y
                            && bar.length == drag.bar.length
                    })
                })
        }) {
            self.mouse.scroll_drag = None;
        }
        let mut frame = MouseFrame {
            stamp: FrameStamp::capture(self),
            hits: vec![],
            wheels: vec![],
            sources: vec![],
            live: true,
            menu_open: matches!(self.overlay, UiOverlay::Menu { .. }),
        };
        if layout.width < 40 || layout.height < 10 {
            self.mouse.frame = None;
            return;
        }
        if self.file_browser.active() {
            let area = browser_bounds(layout.width, layout.height);
            frame.hits.push(Hit {
                area: Rect {
                    x: area.x + 8,
                    y: area.y + 1,
                    width: area.width.saturating_sub(10),
                    height: 1,
                },
                action: Action::BrowserPath,
            });
            if let Some(browser) = &self.file_browser.frame {
                for (row, item) in browser
                    .rows
                    .iter()
                    .take(usize::from(area.height.saturating_sub(8)))
                    .enumerate()
                {
                    frame.hits.push(Hit {
                        area: Rect {
                            x: area.x + 1,
                            y: area.y + 3 + row as u16,
                            width: area.width - 2,
                            height: 1,
                        },
                        action: Action::BrowserNode(item.id),
                    });
                }
            }
            let mut x = area.x + 2;
            for &(label, action) in browser_buttons(
                self.file_browser.kind == Some(crate::state::PromptKind::OpenFolder),
            ) {
                let width = (jc_text::grapheme::width(label) as u16)
                    .min((area.x + area.width - 2).saturating_sub(x));
                if width == 0 {
                    break;
                }
                let action = match action {
                    0 => Action::BrowserActivate,
                    1 => Action::BrowserAccept,
                    2 => Action::BrowserParent,
                    _ => Action::BrowserCancel,
                };
                frame.hits.push(Hit {
                    area: Rect {
                        x,
                        y: area.y + area.height - 2,
                        width,
                        height: 1,
                    },
                    action,
                });
                x = x.saturating_add(width).saturating_add(1);
            }
            frame.wheels.push(WheelHit {
                area,
                action: Wheel::Browser,
            });
            self.mouse.frame = Some(Box::new(frame));
            return;
        }
        if matches!(self.overlay, UiOverlay::None | UiOverlay::Menu { .. }) {
            let mut x = 1u16;
            for menu in self.menus.menus() {
                let width = jc_text::grapheme::width(menu.title) as u16 + 2;
                frame.hits.push(Hit {
                    area: Rect {
                        x,
                        y: 0,
                        width: width.min(layout.width.saturating_sub(x)),
                        height: 1,
                    },
                    action: Action::Menu(menu.id),
                });
                x = x.saturating_add(width);
            }
        }
        if let UiOverlay::Menu { menu, selected } = self.overlay {
            let (x, y, width, height) =
                crate::render::menu_bounds(self, layout.width, layout.height, menu);
            let entries = self.menu_commands(menu);
            let available = usize::from(height.saturating_sub(2));
            let start = selected.saturating_sub(available.saturating_sub(1));
            for (row, command) in entries.into_iter().skip(start).take(available).enumerate() {
                if let Some(command) = command {
                    frame.hits.push(Hit {
                        area: Rect {
                            x: x + 1,
                            y: y + 1 + row as u16,
                            width: width.saturating_sub(2),
                            height: 1,
                        },
                        action: Action::Command(command),
                    });
                }
            }
            frame.wheels.push(WheelHit {
                area: Rect {
                    x,
                    y,
                    width,
                    height,
                },
                action: Wheel::Menu,
            });
            self.mouse.frame = Some(Box::new(frame));
            return;
        }
        if !matches!(self.overlay, UiOverlay::None) {
            self.mouse.frame = Some(Box::new(frame));
            return;
        }
        if self.git_code.active && self.graph_showing() {
            if let Some(graph) =
                self.prepare_git_graph(layout.width, layout.height.saturating_sub(3))
            {
                for (offset, row) in graph.rows.iter().enumerate() {
                    frame.hits.push(Hit {
                        area: Rect {
                            x: 0,
                            y: 2 + offset as u16,
                            width: layout.width,
                            height: 1,
                        },
                        action: Action::CodeCommit(row.ordinal),
                    });
                }
            }
            frame.hits.push(Hit {
                area: Rect {
                    x: 0,
                    y: layout.height - 1,
                    width: 9,
                    height: 1,
                },
                action: Action::CodeBack,
            });
            frame.hits.push(Hit {
                area: Rect {
                    x: 10,
                    y: layout.height - 1,
                    width: 20,
                    height: 1,
                },
                action: Action::Code(super::git_code::Control::Close),
            });
            self.mouse.frame = Some(Box::new(frame));
            return;
        }
        if self.git_code.active {
            for (x, y, label, control) in super::git_code::controls(
                layout.editor_width,
                self.git_code.unified || layout.editor_width < 65,
            ) {
                frame.hits.push(Hit {
                    area: Rect {
                        x: layout.editor_x + x,
                        y: 2 + y,
                        width: jc_text::grapheme::width(&label) as u16,
                        height: 1,
                    },
                    action: Action::Code(control),
                });
            }
        }
        for tab in tab_labels(self, layout.width) {
            frame.hits.push(Hit {
                area: Rect {
                    x: tab.x,
                    y: 1,
                    width: tab.width,
                    height: 1,
                },
                action: Action::Tab(tab.index, tab.tab),
            });
        }
        if !self.mouse_source_available() {
            self.mouse.frame = Some(Box::new(frame));
            return;
        }
        if layout.sidebar_width > 0 {
            let area = Rect {
                x: 0,
                y: 2,
                width: layout.sidebar_width,
                height: layout.content_height,
            };
            frame.hits.push(Hit {
                area: area.row(0),
                action: Action::Focus(PrimaryFocus::Sidebar),
            });
            match self.panels.sidebar() {
                SidebarView::Explorer => {
                    for (row, item) in explorer.rows.iter().enumerate() {
                        frame.hits.push(Hit {
                            area: area.row(2 + row as u16),
                            action: Action::Explorer(item.id),
                        });
                    }
                    if let Some(bar) =
                        super::explorer_scrollbar(area.x, area.y, area.width, area.height, explorer)
                    {
                        frame.hits.push(Hit {
                            area: Rect {
                                x: bar.x,
                                y: bar.y,
                                width: 1,
                                height: bar.length,
                            },
                            action: Action::ExplorerScroll(bar),
                        });
                    }
                    frame.wheels.push(WheelHit {
                        area,
                        action: Wheel::Explorer(usize::from(area.height.saturating_sub(4))),
                    });
                }
                SidebarView::Symbols => {
                    for (row, item) in symbols
                        .rows
                        .iter()
                        .take(usize::from(area.height.saturating_sub(2)))
                        .enumerate()
                    {
                        frame.hits.push(Hit {
                            area: area.row(2 + row as u16),
                            action: Action::Symbol(item.index),
                        });
                    }
                    frame.wheels.push(WheelHit {
                        area,
                        action: Wheel::Symbols,
                    });
                }
                SidebarView::GitStatus => {
                    let headers = area.height - super::git_status_list_height(area.height);
                    for (row, item) in git_status
                        .rows
                        .iter()
                        .take(usize::from(area.height - headers))
                        .enumerate()
                    {
                        frame.hits.push(Hit {
                            area: area.row(headers + row as u16),
                            action: Action::GitStatus(item.ordinal),
                        });
                    }
                    frame.wheels.push(WheelHit {
                        area,
                        action: Wheel::GitStatus,
                    });
                }
                SidebarView::Git => {
                    for (row, index) in crate::render::git_sidebar_targets(git, area.height) {
                        frame.hits.push(Hit {
                            area: area.row(row),
                            action: Action::GitHunk(index),
                        });
                    }
                    frame.wheels.push(WheelHit {
                        area,
                        action: Wheel::GitHunks,
                    });
                }
                _ => {}
            }
        }
        if layout.bottom_height > 0 {
            let area = Rect {
                x: 0,
                y: layout.height - 1 - layout.bottom_height + 1,
                width: layout.width,
                height: layout.bottom_height.saturating_sub(1),
            };
            frame.hits.push(Hit {
                area: area.row(0),
                action: Action::Focus(PrimaryFocus::BottomPanel),
            });
            if self.panels.bottom() != BottomPanel::Diff {
                let count = match self.panels.bottom() {
                    BottomPanel::Problems => self.workbench.problems().len(),
                    BottomPanel::Output => self.workbench.output().len(),
                    BottomPanel::SearchResults => self.search_result_count(),
                    BottomPanel::Terminal => 1,
                    BottomPanel::Diff => 0,
                };
                let available = usize::from(area.height.saturating_sub(1));
                let (start, _) = self.bottom_window(count, available);
                for (row, index) in (start..count).take(available).enumerate() {
                    frame.hits.push(Hit {
                        area: area.row(1 + row as u16),
                        action: Action::Bottom(index),
                    });
                }
            }
            frame.wheels.push(WheelHit {
                area,
                action: Wheel::Bottom,
            });
        }
        for (pane, text) in [(PaneId::Primary, primary), (PaneId::Secondary, secondary)] {
            if !layout.two_columns && pane != self.workbench.panes().active() {
                continue;
            }
            let Some(context) = SourceContext::capture(self, pane) else {
                continue;
            };
            if self.mouse_source_available() {
                for crumb in &text.symbol_path {
                    frame.hits.push(Hit {
                        area: Rect {
                            x: crumb.x,
                            y: 2,
                            width: crumb.width,
                            height: 1,
                        },
                        action: Action::SymbolCrumb(crumb.index),
                    });
                }
            }
            if let Some(bars) = text.bars {
                for bar in bars {
                    frame.hits.push(Hit {
                        area: Rect {
                            x: bar.x,
                            y: bar.y,
                            width: if bar.vertical { 1 } else { bar.length },
                            height: if bar.vertical { bar.length } else { 1 },
                        },
                        action: Action::Scroll(pane, bar),
                    });
                }
            }
            if self.workbench.rectangle_for_pane(pane).is_some() {
                continue;
            }
            let Some(view) = self.workbench.panes().view(pane) else {
                continue;
            };
            let Some(tab) = view.location().map(|location| location.tab) else {
                continue;
            };
            let (x, width) = layout.pane_area(pane);
            let number = text.line_count.map_or(3, |count| {
                (count.max(1).to_string().len() as u16).clamp(3, 7)
            });
            if width < 7 || layout.editor_rows() == 0 {
                continue;
            }
            let rows = if let Some(rows) = &text.display_rows {
                rows.iter().map(|_| true).collect()
            } else {
                text.rows
                    .iter()
                    .map(|row| !row.iter().any(|atom| atom.marker && atom.source.is_empty()))
                    .collect()
            };
            let area = SourceArea {
                area: Rect {
                    x,
                    y: 3,
                    width: width - 1,
                    height: layout.editor_rows(),
                },
                text_x: x + number + 2,
                context,
                tab,
                rows,
            };
            let index = frame.sources.len();
            frame.hits.push(Hit {
                area: area.area,
                action: Action::Source(index),
            });
            frame.wheels.push(WheelHit {
                area: Rect {
                    width: area.area.width + 1,
                    height: area.area.height + 1,
                    ..area.area
                },
                action: Wheel::Source(index),
            });
            frame.sources.push(area);
        }
        self.mouse.frame = Some(Box::new(frame));
    }
    pub(crate) fn handle_mouse(&mut self, event: MouseEvent) -> bool {
        match event.kind {
            MouseEventKind::Drag(MouseButton::Left) if self.mouse.explorer_drag.is_some() => {
                return self.drag_explorer_scrollbar(event, false);
            }
            MouseEventKind::Up(MouseButton::Left) if self.mouse.explorer_drag.is_some() => {
                return self.drag_explorer_scrollbar(event, true);
            }
            MouseEventKind::Drag(MouseButton::Left) if self.mouse.scroll_drag.is_some() => {
                return self.drag_scrollbar(event, false);
            }
            MouseEventKind::Up(MouseButton::Left) if self.mouse.scroll_drag.is_some() => {
                return self.drag_scrollbar(event, true);
            }
            MouseEventKind::Drag(MouseButton::Left) => return self.drag_mouse(event),
            MouseEventKind::Up(MouseButton::Left) => {
                let changed = self.drag_mouse(event);
                self.mouse.drag = None;
                return changed;
            }
            MouseEventKind::Down(MouseButton::Left)
            | MouseEventKind::ScrollUp
            | MouseEventKind::ScrollDown => {
                self.mouse.drag = None;
                self.mouse.scroll_drag = None;
                self.mouse.explorer_drag = None;
            }
            _ => return false,
        }
        let Some(mut frame) = self.mouse.frame.take() else {
            return false;
        };
        let wheel = matches!(
            event.kind,
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
        );
        if !frame.stamp.current(self, wheel) || !wheel && !frame.live {
            return false;
        }
        if wheel {
            if !event.modifiers.is_empty() {
                self.mouse.frame = Some(frame);
                return false;
            }
            let delta = if event.kind == MouseEventKind::ScrollUp {
                -3
            } else {
                3
            };
            let action = frame
                .wheels
                .iter()
                .find(|hit| hit.area.contains(event.column, event.row))
                .map(|hit| hit.action);
            let changed = match action {
                Some(Wheel::Source(index)) => {
                    let area = &frame.sources[index];
                    self.workbench.scroll_view_vertical(
                        area.context.pane,
                        delta,
                        usize::from(area.area.height),
                    )
                }
                Some(Wheel::Explorer(rows)) => {
                    self.explorer_view.scroll_by(&self.explorer, delta, rows)
                }
                Some(Wheel::Browser) => {
                    self.file_browser.path_focused = false;
                    self.file_browser
                        .view
                        .move_selection(&self.file_browser.catalog, delta);
                    true
                }
                Some(Wheel::Bottom) => {
                    let _ = self.focus.focus(PrimaryFocus::BottomPanel, &self.panels);
                    for _ in 0..3 {
                        self.handle_bottom_key(KeyEvent::new(
                            if delta < 0 {
                                KeyCode::Up
                            } else {
                                KeyCode::Down
                            },
                            KeyModifiers::NONE,
                        ));
                    }
                    true
                }
                Some(Wheel::Menu) => {
                    for _ in 0..3 {
                        self.handle_overlay_key(KeyEvent::new(
                            if delta < 0 {
                                KeyCode::Up
                            } else {
                                KeyCode::Down
                            },
                            KeyModifiers::NONE,
                        ));
                    }
                    true
                }
                Some(Wheel::Symbols) => {
                    self.source.select_relative(delta);
                    true
                }
                Some(Wheel::GitStatus) => {
                    if self.git_history.active {
                        self.git_history.selected =
                            self.git_history.selected.saturating_add_signed(delta);
                    } else {
                        self.git_status.move_selection(delta);
                    }
                    true
                }
                Some(Wheel::GitHunks) => {
                    if self.git_history.immutable() {
                        self.git_history.hunk = self.git_history.hunk.saturating_add_signed(delta);
                    } else {
                        self.git_diff.select_relative(delta);
                    }
                    self.git_diff_scroll = 0;
                    true
                }
                None => false,
            };
            if changed {
                frame.live = false;
                frame.stamp = FrameStamp::capture(self);
            }
            self.mouse.frame = Some(frame);
            return changed;
        }
        let action = frame
            .hits
            .iter()
            .rev()
            .find(|hit| hit.area.contains(event.column, event.row))
            .map(|hit| hit.action);
        match action {
            Some(Action::Menu(menu)) => {
                self.open_menu(menu_command(menu));
                true
            }
            Some(Action::Command(command)) => {
                if self.registry.is_enabled(command, &self.command_context()) {
                    self.close_overlay();
                    self.dispatch(command);
                }
                true
            }
            Some(Action::Tab(index, id)) => {
                if self
                    .workbench
                    .tabs()
                    .tabs()
                    .get(index - 1)
                    .is_some_and(|tab| tab.id == id)
                {
                    self.markdown_ui = None;
                    self.snippet_ui = None;
                    self.source_popup = false;
                    self.source_candidates = None;
                    self.open_diff_visible = false;
                    self.workbench.select_tab(index);
                    let _ = self.focus.focus(PrimaryFocus::Editor, &self.panels);
                }
                true
            }
            Some(Action::Source(index)) => {
                self.begin_mouse_source(frame.sources[index].clone(), event)
            }
            Some(Action::Scroll(pane, bar)) => self.begin_scrollbar(pane, bar, event),
            Some(Action::ExplorerScroll(bar)) => {
                if !event.modifiers.is_empty() {
                    return false;
                }
                let offset = bar.pointer_offset(event.column, event.row);
                let on_thumb = bar.thumb_contains(offset);
                let grab = if on_thumb {
                    offset - bar.thumb_start
                } else {
                    bar.thumb_length / 2
                };
                if !on_thumb {
                    self.explorer_view.scroll_to(
                        &self.explorer,
                        bar.target(offset, grab),
                        usize::from(bar.length),
                    );
                }
                self.mouse.explorer_drag = Some(ExplorerScrollDrag {
                    stamp: FrameStamp::capture(self),
                    bar,
                    grab,
                    rows: usize::from(bar.length),
                });
                true
            }
            Some(Action::Explorer(id)) => {
                self.explorer_view.select(id);
                let _ = self.focus.focus(PrimaryFocus::Sidebar, &self.panels);
                self.activate_explorer(false, false);
                true
            }
            Some(Action::Bottom(index)) => {
                self.bottom_selected = index;
                let _ = self.focus.focus(PrimaryFocus::BottomPanel, &self.panels);
                self.handle_bottom_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                true
            }
            Some(Action::Symbol(index)) => {
                let delta = index as isize - self.source.selected() as isize;
                self.source.select_relative(delta);
                let _ = self.focus.focus(PrimaryFocus::Sidebar, &self.panels);
                self.dispatch(CommandId::SourceAccept);
                true
            }
            Some(Action::SymbolCrumb(index)) => {
                if !self.mouse_source_available() {
                    return false;
                }
                match event.modifiers {
                    KeyModifiers::NONE => self.open_symbol_path(index, false),
                    KeyModifiers::CONTROL => self.open_symbol_path(index, true),
                    _ => false,
                }
            }
            Some(Action::GitStatus(index)) => {
                if self.git_history.active {
                    self.git_history.selected = index;
                } else {
                    self.git_status.first();
                    self.git_status.move_selection(index as isize);
                }
                let _ = self.focus.focus(PrimaryFocus::Sidebar, &self.panels);
                true
            }
            Some(Action::GitHunk(index)) => {
                if self.git_history.immutable() {
                    self.git_history.hunk = index;
                    self.git_diff_scroll = 0;
                } else {
                    self.git_diff
                        .select_relative(index as isize - self.git_diff.selected() as isize);
                    self.navigate_git_hunk();
                }
                true
            }
            Some(Action::Code(control)) => {
                self.git_code_control(control);
                true
            }
            Some(Action::CodeCommit(row)) => {
                if let Some(view) = self.git_history.graph.as_mut() {
                    if view.reference_selected.is_some() {
                        view.reference_selected = Some(row);
                    } else {
                        view.selected = row;
                    }
                    self.handle_git_graph_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                }
                true
            }
            Some(Action::CodeBack) => {
                self.handle_git_graph_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
                true
            }
            Some(Action::Focus(focus)) => {
                let _ = self.focus.focus(focus, &self.panels);
                true
            }
            Some(Action::BrowserNode(id)) => {
                if self.file_browser.catalog.node(id).is_some() {
                    self.file_browser.view.select(id);
                    self.file_browser.path_focused = false;
                }
                true
            }
            Some(Action::BrowserPath) => {
                self.browser_path_click(
                    event.column,
                    frame
                        .hits
                        .iter()
                        .find(|hit| matches!(hit.action, Action::BrowserPath))
                        .unwrap()
                        .area,
                );
                true
            }
            Some(
                Action::BrowserActivate
                | Action::BrowserAccept
                | Action::BrowserParent
                | Action::BrowserCancel,
            ) => {
                let key = match action.unwrap() {
                    Action::BrowserAccept => KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL),
                    Action::BrowserParent => KeyEvent::new(KeyCode::Up, KeyModifiers::ALT),
                    Action::BrowserCancel => KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
                    _ => KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                };
                if let UiOverlay::Prompt { kind, input } = self.overlay.clone() {
                    self.handle_browser_key(key, kind, input);
                }
                true
            }
            None if frame.menu_open => {
                self.close_overlay();
                true
            }
            None => {
                self.mouse.frame = Some(frame);
                false
            }
        }
    }
    fn begin_scrollbar(&mut self, pane: PaneId, bar: ScrollBar, event: MouseEvent) -> bool {
        if bar.maximum == 0 || !event.modifiers.is_empty() {
            return false;
        }
        let Some(context) = SourceContext::capture(self, pane) else {
            return false;
        };
        let offset = bar.pointer_offset(event.column, event.row);
        let on_thumb = bar.thumb_contains(offset);
        let mut drag = ScrollDrag {
            context,
            bar,
            grab: if on_thumb {
                offset - bar.thumb_start
            } else {
                bar.thumb_length / 2
            },
            previous: offset,
        };
        if !on_thumb {
            self.move_scrollbar(&mut drag, offset);
        }
        self.mouse.scroll_drag = Some(drag);
        true
    }
    fn drag_explorer_scrollbar(&mut self, event: MouseEvent, release: bool) -> bool {
        let Some(mut drag) = self.mouse.explorer_drag.take() else {
            return false;
        };
        if !self.mouse_source_available() || !drag.stamp.current(self, false) {
            return false;
        }
        let offset = drag.bar.pointer_offset(event.column, event.row);
        let changed = self.explorer_view.scroll_to(
            &self.explorer,
            drag.bar.target(offset, drag.grab),
            drag.rows,
        );
        drag.stamp = FrameStamp::capture(self);
        // Old row hit targets must not be usable until the scrolled list is painted.
        self.mouse.frame = None;
        if !release {
            self.mouse.explorer_drag = Some(drag);
        }
        changed
    }
    fn drag_scrollbar(&mut self, event: MouseEvent, release: bool) -> bool {
        let Some(mut drag) = self.mouse.scroll_drag.take() else {
            return false;
        };
        if !self.mouse_source_available()
            || SourceContext::capture(self, drag.context.pane).as_ref() != Some(&drag.context)
        {
            return false;
        }
        let offset = drag.bar.pointer_offset(event.column, event.row);
        let changed = offset != drag.previous && self.move_scrollbar(&mut drag, offset);
        drag.previous = offset;
        if !release {
            self.mouse.scroll_drag = Some(drag);
        }
        changed
    }
    fn move_scrollbar(&mut self, drag: &mut ScrollDrag, offset: u16) -> bool {
        let pane = drag.context.pane;
        let target = drag.bar.target(offset, drag.grab);
        let old = if drag.bar.vertical {
            drag.context.first
        } else {
            drag.context.column
        };
        let delta = if target >= old {
            isize::try_from(target - old).unwrap_or(isize::MAX)
        } else {
            -isize::try_from(old - target).unwrap_or(isize::MAX)
        };
        let changed = if drag.bar.vertical {
            self.workbench
                .scroll_view_vertical(pane, delta, usize::from(drag.bar.length))
        } else {
            self.workbench
                .scroll_view_horizontal(pane, delta, drag.bar.maximum)
        };
        if changed {
            drag.context = SourceContext::capture(self, pane).expect("scroll preserves the view");
        }
        changed
    }
    fn begin_mouse_source(&mut self, source: SourceArea, event: MouseEvent) -> bool {
        if event
            .modifiers
            .intersects(KeyModifiers::ALT | KeyModifiers::CONTROL)
        {
            return false;
        }
        let Some(head) = source.byte_at(self, event.column, event.row, false) else {
            return false;
        };
        self.workbench.focus_editor_pane(source.context.pane);
        let _ = self.focus.focus(PrimaryFocus::Editor, &self.panels);
        let Some(context) = SourceContext::capture(self, source.context.pane) else {
            return false;
        };
        let anchor = if event.modifiers.contains(KeyModifiers::SHIFT) {
            let byte = context.anchor.unwrap_or(context.cursor);
            if byte == 0 {
                0
            } else {
                let Some(position) = self.workbench.position_for_tab(source.tab, byte) else {
                    return false;
                };
                let Some(column) =
                    self.workbench
                        .visual_column_at_byte(source.tab, position.line, byte)
                else {
                    return false;
                };
                let Some(byte) = self
                    .workbench
                    .visual_byte_at(source.tab, position.line, column)
                else {
                    return false;
                };
                byte
            }
        } else {
            head
        };
        match self.workbench.set_linear_selection_bytes(anchor, head) {
            Ok(()) => {
                if let Some(context) = SourceContext::capture(self, source.context.pane) {
                    self.mouse.drag = Some(LinearDrag {
                        source,
                        context,
                        anchor,
                    });
                }
                self.status = "Mayús+clic extiende · Arrastrar selecciona · Rueda desplaza".into();
                true
            }
            Err(error) => {
                self.status = error.to_string();
                false
            }
        }
    }
    fn drag_mouse(&mut self, event: MouseEvent) -> bool {
        let Some(mut drag) = self.mouse.drag.take() else {
            return false;
        };
        if !self.mouse_source_available()
            || SourceContext::capture(self, drag.context.pane).as_ref() != Some(&drag.context)
        {
            return false;
        }
        let Some(head) = drag.source.byte_at(self, event.column, event.row, true) else {
            self.mouse.drag = Some(drag);
            return false;
        };
        match self.workbench.set_linear_selection_bytes(drag.anchor, head) {
            Ok(()) => {
                drag.context = SourceContext::capture(self, drag.context.pane).unwrap();
                self.mouse.drag = Some(drag);
                true
            }
            Err(error) => {
                self.status = error.to_string();
                false
            }
        }
    }
    fn browser_path_click(&mut self, x: u16, area: Rect) {
        let UiOverlay::Prompt { input, .. } = &self.overlay else {
            return;
        };
        let window = super::file_browser::path_window(
            input,
            self.file_browser.field.cursor,
            usize::from(area.width),
        );
        let column = usize::from(x.saturating_sub(area.x));
        self.file_browser.field.cursor = window
            .atoms
            .iter()
            .find(|atom| column < atom.column + atom.width)
            .map_or(input.len(), |atom| atom.range.start);
        self.file_browser.field.anchor = None;
        self.file_browser.path_focused = true;
    }
}
fn menu_command(menu: MenuId) -> CommandId {
    match menu {
        MenuId::File => CommandId::MenuFile,
        MenuId::Edit => CommandId::MenuEdit,
        MenuId::Search => CommandId::MenuSearch,
        MenuId::Project => CommandId::MenuProject,
        MenuId::Build => CommandId::MenuBuild,
        MenuId::Tools => CommandId::MenuTools,
        MenuId::View => CommandId::MenuView,
        MenuId::Help => CommandId::MenuHelp,
    }
}
