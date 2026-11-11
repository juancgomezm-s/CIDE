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
        if usize::from(width) * usize::from(height) > 131_072 {
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
    pub fn pane_area(self, pane: PaneId) -> (u16, u16) {
        if pane == PaneId::Secondary && self.two_columns {
            (
                self.editor_x + self.pane_width + 1,
                self.editor_width - self.pane_width - 1,
            )
        } else {
            (self.editor_x, self.pane_width)
        }
    }
    pub fn editor_rows(self) -> u16 {
        self.content_height.saturating_sub(2)
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ScrollBar {
    pub x: u16,
    pub y: u16,
    pub length: u16,
    pub vertical: bool,
    pub maximum: usize,
    pub thumb_length: u16,
    pub thumb_start: u16,
}
impl ScrollBar {
    fn new((x, y): (u16, u16), length: u16, vertical: bool, offset: usize, maximum: usize) -> Self {
        let thumb_length = if maximum == 0 {
            length
        } else {
            let track = u128::from(length);
            (track * track / (track + maximum as u128)).max(1) as u16
        };
        let travel = length.saturating_sub(thumb_length);
        let thumb_start = if maximum > 0 {
            (offset.min(maximum) as u128 * u128::from(travel) / maximum as u128) as u16
        } else {
            0
        };
        Self {
            x,
            y,
            length,
            vertical,
            maximum,
            thumb_length,
            thumb_start,
        }
    }
    pub fn pointer_offset(self, x: u16, y: u16) -> u16 {
        if self.vertical {
            y.saturating_sub(self.y)
        } else {
            x.saturating_sub(self.x)
        }
    }
    pub fn thumb_contains(self, offset: u16) -> bool {
        (self.thumb_start..self.thumb_start + self.thumb_length).contains(&offset)
    }
    pub fn target(self, offset: u16, grab: u16) -> usize {
        let travel = self.length.saturating_sub(self.thumb_length);
        if travel == 0 {
            return 0;
        }
        let thumb = offset.saturating_sub(grab).min(travel);
        (u128::from(thumb) * self.maximum as u128 / u128::from(travel)) as usize
    }
}

pub(crate) fn explorer_scrollbar(
    x: u16,
    y: u16,
    width: u16,
    height: u16,
    explorer: &jc_app::explorer::ExplorerPresentation,
) -> Option<ScrollBar> {
    let rows = height.saturating_sub(4).min(128);
    (width >= 2 && rows > 0 && explorer.total > usize::from(rows)).then(|| {
        ScrollBar::new(
            (x + width - 1, y + 2),
            rows,
            true,
            explorer.first,
            explorer.total.saturating_sub(usize::from(rows)),
        )
    })
}

#[derive(Debug, Default)]
pub(crate) struct EditorText {
    pub bars: Option<[ScrollBar; 2]>,
    pub line_count: Option<usize>,
    pub syntax: jc_app::SyntaxRows,
    pub position: Option<TextPosition>,
    pub rectangle: Option<jc_app::RectangleBounds>,
    pub rows: Vec<Vec<jc_app::VisualAtom>>,
    pub display_rows: Option<Vec<Vec<jc_app::DisplayCell>>>,
    pub cursor_column: Option<usize>,
}
pub(crate) struct Presentation<'a> {
    pub code_diff: Option<super::git_code::CodeDiffPresentation>,
    pub markdown: Option<jc_app::markdown_preview::MarkdownPresentation>,
    pub app: &'a TuiApp,
    pub layout: FrameLayout,
    pub primary: EditorText,
    pub secondary: EditorText,
    pub status_position: Option<TextPosition>,
    pub search_positions: Vec<(usize, Option<TextPosition>)>,
    pub git: GitPresentation,
    pub open_diff: Option<jc_app::open_diff_view::OpenDiffPresentation>,
    pub source: jc_app::source_services::SourcePresentation,
    pub snippet: Option<super::snippets::SnippetPresentation>,
    pub git_status: super::GitStatusPresentation,
    pub commit: Option<super::git_create_commit::CommitPresentation>,
    pub graph: Option<super::GitStatusPresentation>,
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
        self.validate_source();
        if self.explorer.root() != self.project_root.as_deref() {
            self.explorer.clear();
        }
        self.open_diff
            .validate_context(&self.workbench, &mut self.tasks);
        self.git_diff.tick(&self.workbench, &mut self.tasks);
        self.git_status
            .validate_context(self.project_root.as_deref());
        if self
            .git_revision
            .invalidate_context(self.project_root.as_deref())
        {
            self.git_history = Default::default();
        }
        self.validate_quick_open_context();
        self.syntax_pending = false;
        self.syntax_targets = [None, None];
        let layout = FrameLayout::new(self, width, height);
        self.last_frame_height = layout.height;
        self.file_browser.prepare(layout.height, layout.width);
        let mut glyph_budget = jc_app::MAX_PRESENTATION_TEXT_BYTES;
        let mut primary = EditorText::default();
        let mut secondary = EditorText::default();
        let mut search_positions = Vec::new();
        let mut status_position = None;
        if layout.width >= 40
            && layout.height >= 10
            && !self.open_diff_visible
            && self.markdown_ui.is_none()
            && !self.git_code.active
        {
            let number_width = self
                .workbench
                .active_document()
                .and_then(|d| self.workbench.line_count_for_tab(d.tab_id))
                .map_or(3, |n| (n.max(1).to_string().len() as u16).clamp(3, 7));
            let (_, active_width) = layout.pane_area(self.workbench.panes().active());
            self.workbench.reveal_active_pane(
                usize::from(layout.editor_rows()),
                if self.workbench.invisibles_visible() {
                    usize::from(active_width.saturating_sub(number_width + 3))
                        .min(crate::render::MAX_DISPLAY_COLUMNS)
                } else {
                    usize::from(active_width.saturating_sub(number_width + 3))
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
                let (x, width) = layout.pane_area(pane);
                let text_width = usize::from(width.saturating_sub(number + 3));
                let first = view.scroll_line().min(count.unwrap_or(0).saturating_sub(1));
                let page = usize::from(layout.editor_rows());
                let visible = first..count.unwrap_or(0).min(first + page);
                let all = self.workbench.invisibles_visible() && rectangle.is_none();
                let bars = count.filter(|_| page > 0 && text_width > 0).map(|count| {
                    let offset = self.workbench.horizontal_scroll_for_pane(pane).unwrap_or(0);
                    let columns = if all {
                        text_width.min(crate::render::MAX_DISPLAY_COLUMNS)
                    } else {
                        text_width
                    };
                    let horizontal_max = self
                        .workbench
                        .horizontal_scroll_max(location.tab, first, page, columns, all)
                        .max(offset);
                    [
                        ScrollBar::new(
                            (x + width - 1, 3),
                            layout.editor_rows(),
                            true,
                            first,
                            count.saturating_sub(page),
                        ),
                        ScrollBar::new(
                            (x + number + 2, 3 + layout.editor_rows()),
                            text_width as u16,
                            false,
                            offset,
                            horizontal_max,
                        ),
                    ]
                });
                let cursor_column = if all {
                    self.workbench.display_column_for_pane(pane)
                } else {
                    self.workbench.visual_column_for_pane(pane)
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
                            let row = if glyph_budget == 0 {
                                None
                            } else if let Some(rect) = rectangle {
                                self.workbench
                                    .rectangle_glyph_line(
                                        location.tab,
                                        line,
                                        rect.scroll_column,
                                        text_width,
                                        self.workbench.invisibles_visible(),
                                    )
                                    .ok()
                            } else {
                                self.workbench.visual_row(
                                    location.tab,
                                    line,
                                    view.scroll_column(),
                                    text_width,
                                )
                            };
                            let row = row.filter(|row| {
                                let bytes = row.iter().map(|a| a.glyph.byte_len()).sum::<usize>();
                                if bytes > glyph_budget {
                                    glyph_budget = 0;
                                    false
                                } else {
                                    glyph_budget -= bytes;
                                    true
                                }
                            });
                            row.unwrap_or_else(|| {
                                "[Layout Unicode no disponible: límite]"
                                    .chars()
                                    .take(text_width)
                                    .enumerate()
                                    .map(|(column, ch)| jc_app::VisualAtom {
                                        source: 0..0,
                                        column,
                                        width: 1,
                                        glyph: jc_app::Glyph::Scalar(ch),
                                        marker: true,
                                    })
                                    .collect()
                            })
                        })
                        .collect()
                };
                let syntax = self.workbench.syntax_rows(location.tab, first, page);
                self.syntax_pending |= syntax.pending;
                self.syntax_targets[usize::from(pane == PaneId::Secondary)] =
                    Some((pane, location.tab));
                let text = EditorText {
                    bars,
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
            let prepare = if self.git_history.immutable() {
                Self::prepare_historical_diff
            } else {
                Self::prepare_git_diff
            };
            let mut prepared = prepare(
                self,
                layout.width,
                usize::from(layout.bottom_height.saturating_sub(3)),
                if self.panels.sidebar_visible() {
                    usize::from(layout.content_height)
                } else {
                    0
                },
            );
            self.add_git_patch_status(&mut prepared, layout.width);
            prepared
        } else {
            GitPresentation::default()
        };
        let git_status = if self.panels.sidebar_visible()
            && self.panels.sidebar() == jc_app::SidebarView::GitStatus
        {
            let prepare = if self.git_history.active {
                Self::prepare_git_history
            } else {
                Self::prepare_local_git_status
            };
            prepare(
                self,
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
                usize::from(layout.content_height.saturating_sub(4))
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
        let source = self.source_presentation(
            if self.source_popup {
                usize::from(layout.height.saturating_sub(10)).min(16)
            } else {
                usize::from(layout.content_height.saturating_sub(2))
            },
            if self.source_popup {
                usize::from(layout.width.saturating_sub(8)).min(96)
            } else {
                usize::from(layout.sidebar_width.saturating_sub(2))
            },
        );
        let snippet = self.prepare_snippet(
            usize::from(layout.height.saturating_sub(8)),
            usize::from(layout.width.saturating_sub(8)),
        );
        let markdown = self.prepare_markdown(
            usize::from(layout.height.saturating_sub(7)),
            usize::from(layout.width.saturating_sub(6)),
        );
        let code_diff = self
            .git_code
            .active
            .then(|| self.prepare_git_code(layout.editor_width, layout.content_height));
        self.prepare_mouse(
            layout,
            &primary,
            &secondary,
            &explorer,
            (&source, &git_status, &git),
        );
        let commit = self.prepare_git_create_commit(layout.width, layout.height);
        let graph = self.prepare_git_graph(layout.width, layout.height.saturating_sub(3));
        Presentation {
            code_diff,
            markdown,
            snippet,
            source,
            open_diff,
            app: self,
            layout,
            primary,
            secondary,
            status_position,
            search_positions,
            git,
            git_status,
            commit,
            graph,
            explorer,
        }
    }
}
