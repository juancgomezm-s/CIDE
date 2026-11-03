//! Preparing text may update caches/viewports; composing this borrowed presentation cannot.
use super::TuiApp;
use jc_app::{BottomPanel, PaneId, PrimaryFocus};
use jc_text::TextPosition;

#[derive(Debug, Default)]
pub(crate) struct GitPresentation {
    pub status: String,
    pub details: Vec<String>,
    pub hunks: Vec<GitHunkRow>,
    pub rows: Vec<GitDiffRow>,
    pub total_hunks: usize,
}
#[derive(Debug)]
pub(crate) struct GitHunkRow {
    pub index: usize,
    pub label: String,
    pub selected: bool,
}
#[derive(Debug, Clone, Copy)]
pub(crate) enum GitRowKind {
    Context,
    Deleted,
    Added,
    Header,
    Notice,
}
#[derive(Debug)]
pub(crate) struct GitDiffRow {
    pub kind: GitRowKind,
    pub left_line: Option<usize>,
    pub right_line: Option<usize>,
    pub text: String,
    pub eol: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct FrameLayout {
    pub width: u16,
    pub height: u16,
    pub bottom_height: u16,
    pub content_height: u16,
    pub sidebar_width: u16,
    pub editor_x: u16,
    pub editor_width: u16,
    pub pane_width: u16,
    pub two_columns: bool,
}
impl FrameLayout {
    fn new(app: &TuiApp, mut width: u16, mut height: u16) -> Self {
        // Bound a presentation even if an input backend reports absurd dimensions.
        if usize::from(width) * usize::from(height) > 1_048_576 {
            width = 80;
            height = 10;
        }
        let bottom_height = if app.panels.bottom_visible() {
            ((height.saturating_sub(4)) / 3).clamp(4, 9)
        } else {
            0
        };
        let content_height = height
            .saturating_sub(1)
            .saturating_sub(bottom_height)
            .saturating_sub(2);
        let sidebar_width = if app.panels.sidebar_visible() {
            (width / 4).clamp(18, 28).min(width.saturating_sub(20))
        } else {
            0
        };
        let editor_x = sidebar_width.saturating_add(u16::from(sidebar_width > 0));
        let editor_width = width.saturating_sub(editor_x);
        let two_columns = app.workbench.panes().is_split() && editor_width >= 65;
        let pane_width = if two_columns {
            (editor_width - 1) / 2
        } else {
            editor_width
        };
        Self {
            width,
            height,
            bottom_height,
            content_height,
            sidebar_width,
            editor_x,
            editor_width,
            pane_width,
            two_columns,
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct EditorText {
    pub line_count: Option<usize>,
    pub syntax: jc_app::SyntaxRows,
    pub position: Option<TextPosition>,
    pub rectangle: Option<jc_app::RectangleBounds>,
    pub rows: Vec<Vec<(char, usize)>>,
    pub display_rows: Option<Vec<Vec<jc_app::DisplayCell>>>,
    pub cursor_column: Option<usize>,
}
pub(crate) struct Presentation<'a> {
    pub app: &'a TuiApp,
    pub layout: FrameLayout,
    pub primary: EditorText,
    pub secondary: EditorText,
    pub status_position: Option<TextPosition>,
    pub search_positions: Vec<(usize, Option<TextPosition>)>,
    pub git: GitPresentation,
    pub open_diff: Option<jc_app::open_diff_view::OpenDiffPresentation>,
    pub git_status: super::GitStatusPresentation,
    pub explorer: jc_app::explorer::ExplorerPresentation,
}
impl Presentation<'_> {
    pub fn editor(&self, pane: PaneId) -> &EditorText {
        match pane {
            PaneId::Primary => &self.primary,
            PaneId::Secondary => &self.secondary,
        }
    }
    pub fn search_position(&self, index: usize) -> Option<TextPosition> {
        self.search_positions
            .iter()
            .find(|&&(i, _)| i == index)
            .and_then(|&(_, p)| p)
    }
}

impl TuiApp {
    pub(crate) fn prepare_frame(&mut self, width: u16, height: u16) -> Presentation<'_> {
        if self.explorer.root() != self.project_root.as_deref() {
            self.explorer.clear();
        }
        self.open_diff
            .validate_context(&self.workbench, &mut self.tasks);
        self.git_diff.tick(&self.workbench, &mut self.tasks);
        self.git_status
            .validate_context(self.project_root.as_deref(), self.git_executable.as_deref());
        self.validate_quick_open_context();
        self.syntax_pending = false;
        self.syntax_targets = [None, None];
        let layout = FrameLayout::new(self, width, height);
        let mut primary = EditorText::default();
        let mut secondary = EditorText::default();
        let mut search_positions = Vec::new();
        let mut status_position = None;
        if layout.width >= 40 && layout.height >= 10 && !self.open_diff_visible {
            let number_width = self
                .workbench
                .active_document()
                .and_then(|d| self.workbench.line_count_for_tab(d.tab_id))
                .map_or(3, |n| (n.max(1).to_string().len() as u16).clamp(3, 7));
            self.workbench.reveal_active_pane(
                usize::from(layout.content_height.saturating_sub(1)),
                if self.workbench.invisibles_visible() {
                    usize::from(layout.pane_width.saturating_sub(number_width + 2))
                        .min(crate::render::MAX_DISPLAY_COLUMNS)
                } else {
                    usize::from(layout.pane_width.saturating_sub(number_width + 2))
                },
            );
            for pane in [PaneId::Primary, PaneId::Secondary] {
                if !layout.two_columns && pane != self.workbench.panes().active() {
                    continue;
                }
                let Some(view) = self.workbench.panes().view(pane) else {
                    continue;
                };
                let Some(location) = view.location() else {
                    continue;
                };
                let rectangle = self.workbench.rectangle_for_pane(pane);
                let count = self.workbench.line_count_for_tab(location.tab);
                let position = self.workbench.position_for_pane(pane);
                let number = count.map_or(3, |n| (n.max(1).to_string().len() as u16).clamp(3, 7));
                let width = if pane == PaneId::Secondary && layout.two_columns {
                    layout.editor_width - layout.pane_width - 1
                } else {
                    layout.pane_width
                };
                let text_width = usize::from(width.saturating_sub(number + 2));
                let first = view.scroll_line().min(count.unwrap_or(0).saturating_sub(1));
                let visible = first
                    ..count
                        .unwrap_or(0)
                        .min(first + usize::from(layout.content_height.saturating_sub(1)));
                let all = self.workbench.invisibles_visible() && rectangle.is_none();
                let cursor_column = if all {
                    self.workbench.display_column_for_pane(pane)
                } else {
                    None
                };
                let display_rows = all.then(|| {
                    visible
                        .clone()
                        .map(|line| {
                            self.workbench
                                .display_row(
                                    location.tab,
                                    line,
                                    view.scroll_column(),
                                    text_width.min(crate::render::MAX_DISPLAY_COLUMNS),
                                )
                                .unwrap_or_default()
                        })
                        .collect()
                });
                let rows = if all {
                    Vec::new()
                } else {
                    visible
                        .map(|line| {
                            if let Some(rect) = rectangle {
                                self.workbench
                                    .rectangle_grid_line(
                                        location.tab,
                                        line,
                                        rect.scroll_column,
                                        text_width,
                                        self.workbench.invisibles_visible(),
                                    )
                                    .unwrap_or_else(|_| {
                                        "[rectangle layout unavailable]"
                                            .chars()
                                            .take(text_width)
                                            .map(|ch| (ch, usize::MAX))
                                            .collect()
                                    })
                            } else {
                                self.workbench
                                    .visible_line(
                                        location.tab,
                                        line,
                                        view.scroll_column(),
                                        text_width,
                                    )
                                    .unwrap_or_default()
                            }
                        })
                        .collect()
                };
                let syntax = self.workbench.syntax_rows(
                    location.tab,
                    first,
                    usize::from(layout.content_height.saturating_sub(1)),
                );
                self.syntax_pending |= syntax.pending;
                self.syntax_targets[usize::from(pane == PaneId::Secondary)] =
                    Some((pane, location.tab));
                let text = EditorText {
                    syntax,
                    line_count: count,
                    position,
                    rectangle,
                    rows,
                    display_rows,
                    cursor_column,
                };
                match pane {
                    PaneId::Primary => primary = text,
                    PaneId::Secondary => secondary = text,
                }
            }
            status_position = self.workbench.active_position();
            if self.panels.bottom_visible()
                && self.panels.bottom() == BottomPanel::SearchResults
                && !self.project_results
            {
                let count = self.workbench.search().matches.len();
                let available = usize::from(layout.bottom_height.saturating_sub(2));
                let selected = self.bottom_selected.min(count.saturating_sub(1));
                let start = if self.focus.primary() == PrimaryFocus::BottomPanel {
                    selected.saturating_sub(available.saturating_sub(1))
                } else {
                    count.saturating_sub(available)
                };
                search_positions = (start..count.min(start + available))
                    .map(|i| (i, self.workbench.search_match_position(i)))
                    .collect();
            }
        }
        let git = if self.panels.sidebar() == jc_app::SidebarView::Git
            || self.panels.bottom() == BottomPanel::Diff
        {
            self.prepare_git_diff(
                layout.width,
                usize::from(layout.bottom_height.saturating_sub(3)),
                if self.panels.sidebar_visible() {
                    usize::from(layout.content_height)
                } else {
                    0
                },
            )
        } else {
            GitPresentation::default()
        };
        let git_status = if self.panels.sidebar_visible()
            && self.panels.sidebar() == jc_app::SidebarView::GitStatus
        {
            self.git_status.prepare(
                layout.sidebar_width,
                super::git_status_list_height(layout.content_height),
            )
        } else {
            super::GitStatusPresentation::default()
        };
        let explorer = self.explorer_view.prepare(
            &self.explorer,
            if self.panels.sidebar_visible()
                && self.panels.sidebar() == jc_app::SidebarView::Explorer
            {
                usize::from(layout.content_height.saturating_sub(2))
            } else {
                0
            },
            usize::from(layout.sidebar_width.saturating_sub(2)),
        );
        if self.syntax_pending {
            self.syntax_wakeup.signal();
        }
        let open_diff = self.open_diff_visible.then(|| {
            self.open_diff_view.prepare(
                &self.open_diff,
                usize::from(layout.height.saturating_sub(6)),
                usize::from(layout.width.saturating_sub(2)),
                layout.width >= 92,
            )
        });
        Presentation {
            open_diff,
            app: self,
            layout,
            primary,
            secondary,
            status_position,
            search_positions,
            git,
            git_status,
            explorer,
        }
    }
}
