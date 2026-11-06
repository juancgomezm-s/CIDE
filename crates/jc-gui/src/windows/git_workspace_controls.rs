//! Theme-matched Git layout controls, bounded rails and versioned pointer capture.
use super::*;
use jc_app::Modifiers;

#[derive(Clone, Copy)]
pub(super) struct Rail {
    pub track: Rect,
    pub thumb: Rect,
    pub max: usize,
    pub horizontal: bool,
}
#[derive(Clone, Copy)]
enum DragKind {
    Divider(Divider),
    Rail(Rail),
}
pub(super) struct GitDrag {
    stamp: Stamp,
    kind: DragKind,
    grab: i32,
}
impl Workspace {
    pub(in crate::windows) fn select_files(&mut self) {
        self.active = Column::Files;
    }
    pub(in crate::windows) fn select_comparison(&mut self) {
        self.active = Column::Comparison;
    }
    pub(super) fn diff_mode(&self) -> DiffView {
        if self.preferences.parallel {
            DiffView::Parallel
        } else {
            DiffView::Unified
        }
    }
    pub(in crate::windows) fn preference_bytes(&self) -> [u8; 7] {
        self.preferences.encode()
    }
    pub(in crate::windows) fn restore_preferences(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.preferences = Preferences::decode(bytes)
            .map_err(|_| "Distribución Git recuperada inválida".to_string())?;
        self.menu = None;
        self.drag = None;
        self.invalidate();
        Ok(())
    }
    pub(in crate::windows) fn dragging(&self) -> bool {
        self.drag.is_some()
    }
    pub(in crate::windows) fn end_drag(&mut self) {
        self.drag = None;
    }
}
fn rail(
    track: Rect,
    horizontal: bool,
    total: usize,
    page: usize,
    position: usize,
    min_thumb: i32,
) -> Option<Rail> {
    let length = if horizontal {
        track.right - track.left
    } else {
        track.bottom - track.top
    };
    if length <= 0 || total <= page || page == 0 {
        return None;
    }
    let max = total - page;
    let size =
        ((length as u64 * page as u64 / total as u64) as i32).clamp(min_thumb.min(length), length);
    let offset = ((length - size) as u64 * position.min(max) as u64 / max as u64) as i32;
    let thumb = if horizontal {
        rect(
            track.left + offset,
            track.top,
            track.left + offset + size,
            track.bottom,
        )
    } else {
        rect(
            track.left,
            track.top + offset,
            track.right,
            track.top + offset + size,
        )
    };
    Some(Rail {
        track,
        thumb,
        max,
        horizontal,
    })
}
pub(super) struct Projection {
    pub cells: Vec<CodeRow>,
    pub first: usize,
    pub total: usize,
    pub extent: usize,
    pub column: usize,
    pub page: usize,
    pub columns: usize,
}
pub(super) struct ProjectionView {
    pub hunk: usize,
    pub first: usize,
    pub encoding: TextEncoding,
    pub geometry: (Rect, i32, i32),
    pub mode: DiffView,
    pub column: usize,
    pub cell_width: i32,
}
pub(super) fn project(model: &jc_app::diff_model::OwnedDiffModel, v: ProjectionView) -> Projection {
    let (area, top, height) = v.geometry;
    let page = ((area.bottom - top).max(0) / height.max(1)).clamp(0, 128) as usize;
    let width = if v.mode == DiffView::Parallel {
        (area.right - area.left) / 2
    } else {
        area.right - area.left
    };
    let columns = ((width - if v.mode == DiffView::Parallel { 48 } else { 90 })
        / v.cell_width.max(1))
    .max(1) as usize;
    let (cells, first, total, extent) = compare_rows::rows(
        model, v.hunk, v.first, page, v.encoding, v.geometry, v.mode, v.column,
    );
    let column = v.column.min(extent.saturating_sub(columns));
    let cells = if column != v.column {
        drop(cells);
        compare_rows::rows(
            model, v.hunk, first, page, v.encoding, v.geometry, v.mode, column,
        )
        .0
    } else {
        cells
    };
    Projection {
        cells,
        first,
        total,
        extent,
        column,
        page,
        columns,
    }
}
impl Workspace {
    pub(super) fn set_projection(&mut self, p: Projection) -> Vec<CodeRow> {
        self.diff_first = p.first;
        self.diff_total = p.total;
        self.diff_extent = p.extent;
        self.diff_column = p.column;
        self.diff_page = p.page;
        self.diff_columns = p.columns;
        p.cells
    }
}
impl State {
    pub(in crate::windows) fn prepare_git_controls(&mut self, frame: &mut Frame) {
        let l = Layout::new(self);
        let p = |v| self.dpi.pixels(v);
        let column_count = usize::from(l.history.is_some())
            + usize::from(l.files.right > l.files.left)
            + usize::from(l.preview.right > l.preview.left);
        let compact = column_count < 3;
        let mut controls = vec![(
            p(if compact { 92 } else { 104 }),
            "Paneles ▾",
            Intent::DistributionMenu,
            self.git_workspace.menu.is_some(),
        )];
        if compact {
            // Area tabs are navigation for hidden panes, not redundant controls
            // above a full-size workspace. All three fit at 320 logical pixels.
            controls.extend([
                (
                    p(70),
                    "Historial",
                    Intent::Column(Column::History),
                    self.git_workspace.active == Column::History,
                ),
                (
                    p(68),
                    "Archivos",
                    Intent::Column(Column::Files),
                    self.git_workspace.active == Column::Files,
                ),
                (
                    p(60),
                    "Comparar",
                    Intent::Column(Column::Comparison),
                    self.git_workspace.active == Column::Comparison,
                ),
            ]);
        }
        if !compact || l.toolbar.right >= p(800) {
            controls.push((
                p(104),
                if self.git_workspace.preferences.parallel {
                    "Paralela"
                } else {
                    "Unificada"
                },
                Intent::DiffView(if self.git_workspace.preferences.parallel {
                    DiffView::Unified
                } else {
                    DiffView::Parallel
                }),
                false,
            ));
        }
        let mut left = p(10);
        for (width, name, intent, selected) in controls {
            let r = rect(
                left,
                l.toolbar.top + p(7),
                left + width,
                l.toolbar.top + p(35),
            );
            if r.right <= l.toolbar.right - p(6) {
                frame.buttons.push((r, name.into(), selected));
                frame.hits.actions.push((r, intent));
            }
            left += width + p(4);
        }
        for (r, d) in &l.dividers {
            frame.dividers.push(*r);
            frame.hits.actions.push((*r, Intent::Divider(*d)));
        }
        if l.preview.right > l.preview.left
            && (!frame.code.is_empty() || !frame.annotations.is_empty())
        {
            let start = frame
                .code
                .iter()
                .map(|r| r.rect.top)
                .chain(frame.annotations.iter().map(|r| r.rect.top))
                .min()
                .unwrap_or(l.preview.bottom);
            let v = rect(
                l.preview.right - p(10),
                start,
                l.preview.right,
                l.preview.bottom - p(14),
            );
            if let Some(r) = rail(
                v,
                false,
                self.git_workspace.diff_total,
                self.git_workspace.diff_page,
                self.git_workspace.diff_first,
                p(24),
            ) {
                frame.rails.push(r);
                frame.hits.actions.push((r.track, Intent::Rail(r)));
            }
            let extent = self.git_workspace.diff_extent.max(
                self.git_workspace
                    .diff_column
                    .saturating_add(self.git_workspace.diff_columns),
            );
            let h = rect(
                l.preview.left,
                l.preview.bottom - p(12),
                l.preview.right - p(12),
                l.preview.bottom,
            );
            if let Some(r) = rail(
                h,
                true,
                extent,
                self.git_workspace.diff_columns,
                self.git_workspace.diff_column,
                p(28),
            ) {
                frame.rails.push(r);
                frame.hits.actions.push((r.track, Intent::Rail(r)));
            }
        }
        if let Some(selected) = self.git_workspace.menu {
            let width = p(300).min((l.rect.right - p(16)).max(0));
            let left = p(10).min((l.rect.right - width).max(0));
            let top = l.toolbar.bottom;
            for (index, arrangement) in Arrangement::ALL.into_iter().enumerate() {
                let name = arrangement.label();
                let r = rect(
                    left,
                    top + index as i32 * p(30),
                    left + width,
                    (top + (index + 1) as i32 * p(30)).min(l.rect.bottom),
                );
                if r.bottom > r.top {
                    frame.overlays.push((r, name.into(), index == selected));
                    frame.hits.actions.push((r, Intent::Arrange(arrangement)));
                }
            }
        }
        frame.hits.stamp = self.git_workspace_stamp();
    }
    fn git_column(&mut self, column: Column) {
        self.git_workspace.active = column;
        self.git_workspace.focus_diff = column == Column::Comparison;
        self.git_workspace.menu = None;
        self.git_workspace.invalidate();
    }
    pub(super) fn git_control_action(&mut self, intent: Intent, x: i32, y: i32) -> Option<Action> {
        match intent {
            Intent::DistributionMenu => {
                self.git_workspace.menu = if self.git_workspace.menu.is_some() {
                    None
                } else {
                    Some(match self.git_workspace.preferences.arrangement {
                        Arrangement::One => 0,
                        Arrangement::TwoFilesLeft => 1,
                        Arrangement::TwoFilesRight => 2,
                        Arrangement::Three => 3,
                    })
                };
            }
            Intent::Arrange(a) => {
                self.git_workspace.preferences.arrangement = a;
                self.git_workspace.menu = None;
            }
            Intent::DiffView(mode) => {
                self.git_workspace.preferences.parallel = mode == DiffView::Parallel;
                self.git_workspace.diff_first = 0;
                self.git_workspace.diff_column = 0;
            }
            Intent::Column(c) => self.git_column(c),
            Intent::Divider(d) => {
                let stamp = self.git_workspace_stamp();
                self.git_workspace.drag = Some(GitDrag {
                    stamp,
                    kind: DragKind::Divider(d),
                    grab: 0,
                });
                return Some(Action::CapturePointer);
            }
            Intent::Rail(r) => {
                let point = if r.horizontal { x } else { y };
                let start = if r.horizontal {
                    r.thumb.left
                } else {
                    r.thumb.top
                };
                let size = if r.horizontal {
                    r.thumb.right - r.thumb.left
                } else {
                    r.thumb.bottom - r.thumb.top
                };
                let grab = if point >= start && point < start + size {
                    point - start
                } else {
                    size / 2
                };
                let stamp = self.git_workspace_stamp();
                self.git_workspace.drag = Some(GitDrag {
                    stamp,
                    kind: DragKind::Rail(r),
                    grab,
                });
                self.update_git_drag(x, y);
                return Some(Action::CapturePointer);
            }
            _ => return None,
        }
        self.git_workspace.invalidate();
        Some(Action::None)
    }
    pub(in crate::windows) fn update_git_drag(&mut self, x: i32, y: i32) -> bool {
        let Some(mut drag) = self.git_workspace.drag.take() else {
            return false;
        };
        if self.git_panel.is_none() || drag.stamp != self.git_workspace_stamp() {
            return false;
        }
        match drag.kind {
            DragKind::Divider(d) => {
                self.git_workspace.preferences.drag(d, x, self.width);
            }
            DragKind::Rail(r) => {
                let (point, start, length, size) = if r.horizontal {
                    (
                        x,
                        r.track.left,
                        r.track.right - r.track.left,
                        r.thumb.right - r.thumb.left,
                    )
                } else {
                    (
                        y,
                        r.track.top,
                        r.track.bottom - r.track.top,
                        r.thumb.bottom - r.thumb.top,
                    )
                };
                let available = (length - size).max(1);
                let pos = (point - start - drag.grab).clamp(0, available) as u64;
                let value = (pos * r.max as u64 / available as u64) as usize;
                if r.horizontal {
                    self.git_workspace.diff_column = value;
                } else {
                    self.git_workspace.diff_first = value;
                }
            }
        }
        self.git_workspace.invalidate();
        drag.stamp = self.git_workspace_stamp();
        self.git_workspace.drag = Some(drag);
        true
    }
    pub(in crate::windows) fn git_horizontal_wheel(&mut self, x: i32, y: i32, delta: i32) -> bool {
        let l = Layout::new(self);
        if !inside(l.preview, x, y) {
            return false;
        }
        self.git_workspace.wheel = self.git_workspace.wheel.saturating_add(delta);
        let steps = -(self.git_workspace.wheel / 120) as isize * 4;
        self.git_workspace.wheel %= 120;
        if steps == 0 {
            return false;
        }
        self.git_workspace.diff_column = self
            .git_workspace
            .diff_column
            .saturating_add_signed(steps)
            .min(
                self.git_workspace
                    .diff_extent
                    .saturating_sub(self.git_workspace.diff_columns),
            );
        self.git_workspace.invalidate();
        true
    }
    fn git_single_column(&self) -> bool {
        let l = Layout::new(self);
        usize::from(l.history.is_some())
            + usize::from(l.files.right > l.files.left)
            + usize::from(l.preview.right > l.preview.left)
            == 1
    }
    pub(in crate::windows) fn git_controls_key(
        &mut self,
        code: usize,
        modifiers: Modifiers,
    ) -> bool {
        if modifiers != Modifiers::NONE {
            return false;
        }
        if self.git_workspace.menu.is_none()
            && (self.git_blame_key(code)
                || (self.git_workspace.blame_query.is_none() && self.git_path_history_key(code)))
        {
            return true;
        }
        if let Some(selected) = self.git_workspace.menu {
            match code {
                0x1b => self.git_workspace.menu = None,
                0x26 => self.git_workspace.menu = Some(selected.saturating_sub(1)),
                0x28 => self.git_workspace.menu = Some((selected + 1).min(3)),
                0x0d => {
                    let a = [
                        Arrangement::One,
                        Arrangement::TwoFilesLeft,
                        Arrangement::TwoFilesRight,
                        Arrangement::Three,
                    ][selected.min(3)];
                    self.git_workspace.preferences.arrangement = a;
                    self.git_workspace.menu = None;
                }
                _ => {}
            }
            self.git_workspace.invalidate();
            return true;
        }
        match code {
            0x56 => {
                self.git_workspace.preferences.parallel = !self.git_workspace.preferences.parallel;
                self.git_workspace.diff_first = 0;
                self.git_workspace.diff_column = 0;
            }
            0x25 | 0x27 if self.git_workspace.focus_diff => {
                let max = self
                    .git_workspace
                    .diff_extent
                    .saturating_sub(self.git_workspace.diff_columns);
                self.git_workspace.diff_column = self
                    .git_workspace
                    .diff_column
                    .saturating_add_signed(if code == 0x25 { -4 } else { 4 })
                    .min(max);
            }
            0x09 if self.git_single_column() => {
                self.git_column(match self.git_workspace.active {
                    Column::History => Column::Files,
                    Column::Files => Column::Comparison,
                    Column::Comparison => Column::History,
                });
            }
            _ if self.git_workspace.blame_query.is_none()
                && self.git_workspace.active == Column::History
                && !self.git_workspace.focus_diff =>
            {
                let total = self
                    .git_status
                    .result()
                    .and_then(|r| r.info())
                    .map_or(0, |i| i.commits.len());
                if total == 0 {
                    return false;
                }
                let selected = self.git_workspace.commit.unwrap_or(0);
                let delta = match code {
                    0x26 => -1,
                    0x28 => 1,
                    0x21 => -10,
                    0x22 => 10,
                    0x24 => isize::MIN,
                    0x23 => isize::MAX,
                    0x0d => {
                        if let Some(id) = self
                            .git_status
                            .result()
                            .and_then(|r| r.info())
                            .and_then(|i| i.commits.get(selected))
                            .map(|c| c.id.clone())
                        {
                            self.request_git_history(id);
                        }
                        return true;
                    }
                    _ => return false,
                };
                self.git_workspace.commit =
                    Some(selected.saturating_add_signed(delta).min(total - 1));
            }
            _ => return false,
        }
        self.git_workspace.invalidate();
        true
    }
}
pub(super) unsafe fn draw(dc: Hdc, frame: &Frame, font: Handle) -> Result<(), String> {
    SelectObject(dc, font);
    for r in &frame.dividers {
        paint::fill(dc, *r, PANEL)?;
        let x = (r.left + r.right) / 2;
        paint::fill(dc, rect(x, r.top, x + 1, r.bottom), BORDER)?;
    }
    for r in &frame.rails {
        paint::fill(dc, r.track, shell::SURFACE)?;
        paint::fill(dc, r.thumb, shell::RAISED)?;
    }
    if let Some(cell) = frame.code.iter().find(|r| r.parallel) {
        let x = cell.rect.right;
        paint::fill(
            dc,
            rect(x, cell.rect.top, x + 1, frame.preview.bottom),
            shell::SURFACE,
        )?;
    }
    for (r, name, selected) in &frame.overlays {
        let color = if *selected {
            SELECTION
        } else if r.top < frame.toolbar.bottom {
            PANEL
        } else {
            HEADER
        };
        paint::fill(dc, *r, color)?;
        text(
            dc,
            rect(r.left + 7, r.top + 5, r.right - 5, r.bottom),
            name,
            shell::TEXT,
            color,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use jc_app::diff_model::{DiffLimits, OwnedDiffModel};
    use std::sync::Arc;
    fn state() -> State {
        let mut s = State::new(&crate::GuiOptions::default()).unwrap();
        s.width = 1200;
        s.height = 800;
        s.git_panel = Some(GitIntent::Status);
        s
    }
    fn publish(s: &mut State) -> Frame {
        let f = s.prepare_git_workspace().unwrap();
        s.git_workspace.hits = Some(f.hits.clone());
        f
    }
    #[test]
    fn compact_toolbar_keeps_hidden_areas_clickable_in_one_row() {
        let mut s = state();
        s.width = s.dpi.pixels(320);
        let before = s.workbench.active_bytes().unwrap();
        for expected in [Column::History, Column::Comparison, Column::Files] {
            let frame = publish(&mut s);
            let l = Layout::new(&s);
            let r = frame
                .hits
                .actions
                .iter()
                .find_map(|(r, intent)| {
                    matches!(intent, Intent::Column(c) if *c == expected).then_some(*r)
                })
                .unwrap();
            assert!(r.top >= l.toolbar.top && r.bottom <= l.toolbar.bottom);
            assert!(r.left >= 0 && r.right <= s.width);
            s.git_workspace_click(r.left + 1, r.top + 1);
            assert_eq!(s.git_workspace.active, expected);
        }
        assert_eq!(s.workbench.active_bytes().unwrap(), before);
    }
    #[test]
    fn full_toolbar_omits_redundant_area_tabs() {
        let mut s = state();
        let frame = publish(&mut s);
        assert!(!frame
            .hits
            .actions
            .iter()
            .any(|(_, a)| matches!(a, Intent::Column(_))));
        let menu = frame
            .hits
            .actions
            .iter()
            .find_map(|(r, a)| matches!(a, Intent::DistributionMenu).then_some(*r))
            .unwrap();
        let view = frame
            .hits
            .actions
            .iter()
            .find_map(|(r, a)| matches!(a, Intent::DiffView(_)).then_some(*r))
            .unwrap();
        assert_eq!(menu.top, view.top);
        assert!(menu.right < view.left);
        assert!(view.right <= s.dpi.pixels(230));
    }
    #[test]
    fn compact_requested_three_keeps_all_columns_reachable_without_editing() {
        let mut s = state();
        s.width = 320;
        let before = s.workbench.active_bytes().unwrap();
        for expected in [Column::Comparison, Column::History, Column::Files] {
            assert!(s.git_controls_key(0x09, Modifiers::NONE));
            assert_eq!(s.git_workspace.active, expected);
            let l = Layout::new(&s);
            assert!(match expected {
                Column::Comparison => l.preview.right > l.preview.left,
                Column::History => l.history.is_some(),
                Column::Files => l.files.right > l.files.left,
            });
        }
        assert_eq!(s.workbench.active_bytes().unwrap(), before);
    }
    #[test]
    fn divider_capture_and_layout_menu_reject_stale_targets_and_preserve_preferences() {
        let mut s = state();
        let before = s.workbench.active_bytes().unwrap();
        let f = publish(&mut s);
        let r = f
            .hits
            .actions
            .iter()
            .find_map(|(r, a)| matches!(a, Intent::Divider(_)).then_some(*r))
            .unwrap();
        assert!(matches!(
            s.git_workspace_click(r.left + 1, r.top + 10),
            Action::CapturePointer
        ));
        assert!(s.git_workspace.dragging());
        let original = s.git_workspace.preference_bytes();
        assert!(s.update_git_drag(340, r.top + 20));
        assert_ne!(s.git_workspace.preference_bytes(), original);
        s.width += 1;
        assert!(!s.update_git_drag(400, r.top + 20));
        assert!(!s.git_workspace.dragging());
        let bytes = s.git_workspace.preference_bytes();
        s.clear_git();
        assert_eq!(s.git_workspace.preference_bytes(), bytes);
        assert_eq!(s.workbench.active_bytes().unwrap(), before);
    }
    #[test]
    fn popup_keyboard_and_outside_click_do_not_activate_underlying_controls() {
        let mut s = state();
        let f = publish(&mut s);
        let r = f
            .hits
            .actions
            .iter()
            .find_map(|(r, a)| matches!(a, Intent::DistributionMenu).then_some(*r))
            .unwrap();
        s.git_workspace_click(r.left + 2, r.top + 2);
        assert!(s.git_workspace.menu.is_some());
        s.git_controls_key(0x24, Modifiers::NONE); // unhandled keys are inert while modal
        s.git_controls_key(0x26, Modifiers::NONE);
        s.git_controls_key(0x26, Modifiers::NONE);
        s.git_controls_key(0x26, Modifiers::NONE);
        s.git_controls_key(0x0d, Modifiers::NONE);
        assert_eq!(s.git_workspace.preferences.arrangement, Arrangement::One);
        let f = publish(&mut s);
        let close = f
            .hits
            .actions
            .iter()
            .find_map(|(r, a)| matches!(a, Intent::Close).then_some(*r))
            .unwrap();
        s.git_workspace.menu = Some(0);
        let _ = publish(&mut s);
        s.git_workspace_click(close.left + 1, close.top + 1);
        assert!(s.git_panel.is_some());
        assert!(s.git_workspace.menu.is_none());
    }
    #[test]
    fn horizontal_projection_reveals_text_and_normalizes_when_visible_lines_shorten() {
        let left = Arc::new(Vec::new());
        let right = Arc::new(format!("{}final\nshort\n", "x".repeat(900)).into_bytes());
        let model = OwnedDiffModel::compute(left, right, DiffLimits::default(), || false);
        let view = |first, column| ProjectionView {
            hunk: 0,
            first,
            encoding: TextEncoding::Utf8,
            geometry: (rect(0, 0, 360, 18), 0, 18),
            mode: DiffView::Parallel,
            column,
            cell_width: 8,
        };
        let long = project(&model, view(0, 850));
        assert!(long.column > 0);
        assert!(long.cells.iter().any(|c| c.text.contains("final")));
        let short = project(&model, view(1, long.column));
        assert_eq!(short.column, 0);
        assert!(short.cells.iter().any(|c| c.text == "short"));
        assert_eq!(short.page, 1);
        assert_eq!(short.first, 1);
    }
    #[test]
    fn rails_move_read_only_view_and_expire_after_geometry_changes() {
        let mut s = state();
        let before = s.workbench.active_bytes().unwrap();
        let text = (0..200)
            .map(|_| format!("{}\n", "z".repeat(900)))
            .collect::<String>();
        let model = OwnedDiffModel::compute(
            Arc::new(Vec::new()),
            Arc::new(text.into_bytes()),
            DiffLimits::default(),
            || false,
        );
        let mut f = s.prepare_git_workspace().unwrap();
        let l = Layout::new(&s);
        let projection = project(
            &model,
            ProjectionView {
                hunk: 0,
                first: 0,
                encoding: TextEncoding::Utf8,
                geometry: (
                    rect(
                        l.preview.left,
                        l.preview.top,
                        l.preview.right - 12,
                        l.preview.bottom - 14,
                    ),
                    l.preview.top + 148,
                    s.cell_height,
                ),
                mode: DiffView::Unified,
                column: 0,
                cell_width: s.cell_width,
            },
        );
        f.code = s.git_workspace.set_projection(projection);
        s.prepare_git_controls(&mut f);
        s.git_workspace.hits = Some(f.hits.clone());
        for r in f.rails.clone() {
            assert!(matches!(
                s.git_control_action(Intent::Rail(r), r.thumb.left + 1, r.thumb.top + 1),
                Some(Action::CapturePointer)
            ));
            assert!(s.update_git_drag(r.track.right - 1, r.track.bottom - 1));
            assert!(if r.horizontal {
                s.git_workspace.diff_column > 0
            } else {
                s.git_workspace.diff_first > 0
            });
            s.git_workspace.end_drag();
        }
        assert_eq!(s.workbench.active_bytes().unwrap(), before);
    }
}
