//! View-only input: source bytes, undo and file sessions never change here.
use super::{Workbench, WorkbenchError};
use crate::{PaneId, TabId};
use jc_text::{PositionError, TextPosition};
impl Workbench {
    pub fn can_compare_open(&self) -> bool {
        let origin = |pane| {
            self.panes
                .view(pane)
                .and_then(|v| v.location())
                .and_then(|loc| self.document_by_tab(loc.tab))
                .filter(|d| d.session.encoding().is_some())
                .map(|d| d.id)
        };
        matches!((origin(PaneId::Primary),origin(PaneId::Secondary)),(Some(a),Some(b)) if a!=b)
    }
    /// Explicit navigation into an existing view, retaining pin and recording
    /// history in that group. Validate before changing focus or either caret.
    pub fn jump_to_pane_location(&mut self, pane: PaneId, tab: TabId, offset: usize) -> bool {
        if self
            .panes
            .view(pane)
            .and_then(|v| v.location())
            .is_none_or(|loc| loc.tab != tab)
            || self.position_for_tab(tab, offset).is_none()
        {
            return false;
        }
        self.panes.focus(pane);
        self.panes
            .navigate(crate::NavLocation::new(tab, offset), false);
        let Some(state) = self.panes.active_state_mut() else {
            return false;
        };
        state.cursor = offset;
        state.anchor = None;
        state.rectangle = None;
        state.manual_scroll = false;
        self.search_navigation = None;
        self.activate_location_document(crate::NavLocation::new(tab, offset))
    }
    pub fn byte_offset_for_tab(&self, tab: TabId, position: TextPosition) -> Option<usize> {
        self.with_text(tab, |text, source| text.offset(source, position))?
            .ok()
    }
    /// Both byte endpoints must be legal text boundaries in the active source.
    /// A collapsed selection has no anchor; this is not a navigation jump.
    pub fn set_linear_selection_bytes(
        &mut self,
        anchor: usize,
        head: usize,
    ) -> Result<(), WorkbenchError> {
        self.with_active_text(|text, source| {
            // Existing whole-file selection uses anchor0, including an initial
            // BOM. The caret/head still requires a legal textual boundary.
            if anchor != 0 {
                text.position(source, anchor)?;
            }
            text.position(source, head)?;
            Ok::<(), PositionError>(())
        })??;
        let state = self
            .panes
            .active_state_mut()
            .ok_or(WorkbenchError::NoDocument)?;
        state.rectangle = None;
        state.cursor = head;
        state.anchor = (anchor != head).then_some(anchor);
        state.manual_scroll = false;
        self.search_navigation = None;
        Ok(())
    }
    pub fn resume_caret_tracking(&mut self) {
        if let Some(state) = self.panes.active_state_mut() {
            state.manual_scroll = false;
        }
    }
    pub fn clamp_viewport_rows(&mut self, rows: usize) {
        for pane in [PaneId::Primary, PaneId::Secondary] {
            let Some(view) = self.panes.view(pane) else {
                continue;
            };
            let Some(location) = view.location() else {
                continue;
            };
            let max = self
                .line_count_for_tab(location.tab)
                .unwrap_or(1)
                .saturating_sub(rows.max(1));
            self.panes
                .set_scroll(pane, view.scroll_line().min(max), view.scroll_column());
        }
    }
    /// Manual vertical scrolling preserves focus/carets/selection, applies the
    /// actual clamped delta to a linked group and suspends automatic reveal.
    pub fn scroll_view_vertical(&mut self, pane: PaneId, delta: isize, rows: usize) -> bool {
        let Some(view) = self.panes.view(pane) else {
            return false;
        };
        let Some(location) = view.location() else {
            return false;
        };
        let old = view.scroll_line();
        let column = view.scroll_column();
        let max = self
            .line_count_for_tab(location.tab)
            .unwrap_or(1)
            .saturating_sub(rows.max(1));
        let next = if delta < 0 {
            old.saturating_sub(delta.unsigned_abs())
        } else {
            old.saturating_add(delta as usize)
        }
        .min(max);
        if next == old {
            return false;
        }
        self.panes.set_scroll(pane, next, column);
        self.panes.set_manual_scroll(pane, true);
        self.link_scroll_delta(pane, old, next, rows);
        true
    }
    pub fn horizontal_scroll_for_pane(&self, pane: PaneId) -> Option<usize> {
        let state = self.panes.view(pane)?.state()?;
        Some(
            state
                .rectangle
                .map_or(state.scroll_column, |rect| rect.scroll_column.0),
        )
    }
    /// Manually move the viewport without changing its caret or selection.
    pub fn scroll_view_horizontal(&mut self, pane: PaneId, delta: isize, max: usize) -> bool {
        let Some(old) = self.horizontal_scroll_for_pane(pane) else {
            return false;
        };
        let next = old.saturating_add_signed(delta).min(max);
        if next == old {
            return false;
        }
        self.panes.set_horizontal_scroll(pane, next);
        true
    }
    /// The horizontal track reflects the visible lines. Scan only candidates
    /// that can overflow, with a shared byte budget and a conservative upper
    /// bound for long rows whose exact widths are outside that budget.
    pub fn horizontal_scroll_max(
        &self,
        tab: TabId,
        first: usize,
        rows: usize,
        visible_columns: usize,
        invisibles: bool,
    ) -> usize {
        let count = self.line_count_for_tab(tab).unwrap_or(0);
        let mut width = visible_columns;
        let mut budget = 64 * 1024;
        for line in first..first.saturating_add(rows).min(count) {
            let Some((content, eol)) = self.line_range_for_tab(tab, line) else {
                break;
            };
            let upper = if invisibles {
                content.len().saturating_mul(12).saturating_add(12)
            } else {
                content.len().saturating_mul(4)
            };
            if upper <= width {
                continue;
            }
            let bytes = eol.end - content.start + 8;
            let exact = if bytes <= budget {
                budget -= bytes;
                if invisibles {
                    self.display_column_at_byte(tab, line, eol.end)
                } else {
                    self.visual_column_at_byte(tab, line, content.end)
                }
            } else {
                None
            };
            width = width.max(exact.unwrap_or(upper));
        }
        width.saturating_sub(visible_columns)
    }
    pub(super) fn link_scroll_delta(&mut self, pane: PaneId, old: usize, next: usize, rows: usize) {
        if !self.panes.scroll_linked() || old == next {
            return;
        }
        let other = if pane == PaneId::Primary {
            PaneId::Secondary
        } else {
            PaneId::Primary
        };
        let Some(view) = self.panes.view(other) else {
            return;
        };
        let max = view
            .location()
            .and_then(|loc| self.line_count_for_tab(loc.tab))
            .unwrap_or(1)
            .saturating_sub(rows.max(1));
        let line = if next >= old {
            view.scroll_line().saturating_add(next - old)
        } else {
            view.scroll_line().saturating_sub(old - next)
        }
        .min(max);
        self.panes.set_scroll(other, line, view.scroll_column());
        self.panes.set_manual_scroll(other, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jc_text::TextEncoding;
    #[test]
    fn viewport_and_byte_selection_preserve_source_history_and_linked_independent_views() {
        let mut app = Workbench::default();
        app.new_file("viewport.txt", TextEncoding::Utf8);
        app.insert_text(&"ñ\r\n".repeat(100)).unwrap();
        app.go_to_position(TextPosition { line: 0, column: 0 })
            .unwrap();
        app.split_editor_right();
        app.focus_editor_pane(PaneId::Primary);
        app.go_to_position(TextPosition { line: 0, column: 0 })
            .unwrap();
        app.reveal_active_pane(10, 40);
        let bytes = app.active_bytes().unwrap();
        let revision = app.active_document().unwrap().session.document().revision();
        let secondary = app.position_for_pane(PaneId::Secondary);
        app.toggle_scroll_link();
        assert!(app.scroll_view_vertical(PaneId::Primary, 20, 10));
        for pane in [PaneId::Primary, PaneId::Secondary] {
            assert_eq!(app.panes().view(pane).unwrap().scroll_line(), 20);
        }
        app.reveal_active_pane(10, 40);
        assert_eq!(app.panes().view(PaneId::Primary).unwrap().scroll_line(), 20);
        assert_eq!(app.position_for_pane(PaneId::Secondary), secondary);
        assert!(app.set_linear_selection_bytes(0, 3).is_err()); // CRLF interior
        app.set_linear_selection_bytes(0, 4).unwrap();
        assert_eq!(app.selection_for_pane(PaneId::Primary), Some(0..4));
        app.set_linear_selection_bytes(2, 2).unwrap();
        assert!(app.active_editor().unwrap().anchor.is_none());
        app.reveal_active_pane(10, 40);
        assert_eq!(app.panes().view(PaneId::Primary).unwrap().scroll_line(), 0);
        assert_eq!(app.active_bytes().unwrap(), bytes);
        assert_eq!(
            app.active_document().unwrap().session.document().revision(),
            revision
        );
        assert!(app.scroll_view_vertical(PaneId::Secondary, isize::MAX, 10));
        assert_eq!(
            app.panes().view(PaneId::Secondary).unwrap().scroll_line(),
            91
        );
        app.clamp_viewport_rows(1000);
        assert_eq!(
            app.panes().view(PaneId::Secondary).unwrap().scroll_line(),
            0
        );
        app.insert_text("X").unwrap();
        app.undo().unwrap();
        assert_eq!(app.active_bytes().unwrap(), bytes);
        // A BOM stays selectable by the existing whole-file anchor convention,
        // but never becomes a caret inside its three-byte representation.
        let mut bom = Workbench::default();
        bom.new_file("bom.txt", TextEncoding::Utf8);
        bom.insert_text("\u{feff}a\r\n").unwrap();
        bom.set_linear_selection_bytes(0, 4).unwrap();
        assert_eq!(bom.selection_for_pane(PaneId::Primary), Some(0..4));
        assert!(bom.set_linear_selection_bytes(0, 1).is_err());
        assert_eq!(bom.selection_for_pane(PaneId::Primary), Some(0..4));
    }
}
