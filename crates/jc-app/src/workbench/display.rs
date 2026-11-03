//! Display coordinates are independent of source scalars and rectangle columns.
use super::{invisible_layout::DisplayCell, Workbench};
use crate::{
    rectangle::{LineLayout, VisualColumn},
    PaneId, RectangleBounds, TabId,
};
use std::ops::Range;

impl Workbench {
    pub fn invisibles_visible(&self) -> bool {
        self.show_invisibles
    }
    pub(super) fn toggle_invisibles(&mut self) {
        self.show_invisibles = !self.show_invisibles;
        if !self.show_invisibles {
            for document in &self.documents {
                document.invisible.borrow_mut().clear();
            }
        }
        // The meaning of horizontal scroll changes between these presentations.
        // Reset both viewports, without changing caret/selection or document state.
        for pane in [PaneId::Primary, PaneId::Secondary] {
            if let Some(view) = self.panes.view(pane) {
                self.panes.set_scroll(pane, view.scroll_line(), 0);
            }
        }
    }
    fn display_ranges(&self, tab: TabId, line: usize) -> Option<(Range<usize>, Range<usize>)> {
        let (mut content, eol) = self.line_range_for_tab(tab, line)?;
        if line == 0 && content.start == 3 {
            content.start = 0;
        }
        Some((content, eol))
    }
    pub fn display_row(
        &self,
        tab: TabId,
        line: usize,
        scroll: usize,
        width: usize,
    ) -> Option<Vec<DisplayCell>> {
        let document = self.document_by_tab(tab)?;
        let (content, eol) = self.display_ranges(tab, line)?;
        document.invisible.borrow_mut().row(
            document.session.document(),
            document.session.encoding()?,
            content,
            eol,
            scroll,
            width,
        )
    }
    pub fn display_byte_at(&self, tab: TabId, line: usize, column: usize) -> Option<usize> {
        let document = self.document_by_tab(tab)?;
        let (content, eol) = self.display_ranges(tab, line)?;
        let byte = document.invisible.borrow_mut().byte_at(
            document.session.document(),
            document.session.encoding()?,
            content,
            eol,
            column,
        )?;
        // BOM is visible metadata before the first editable caret boundary.
        Some(byte.max(self.line_range_for_tab(tab, line)?.0.start))
    }
    pub fn display_column_for_pane(&self, pane: PaneId) -> Option<usize> {
        let view = self.panes.view(pane)?;
        let location = view.location()?;
        let document = self.document_by_tab(location.tab)?;
        let position = self.position_for_pane(pane)?;
        let (content, eol) = self.display_ranges(location.tab, position.line)?;
        let encoding = document.session.encoding()?;
        let mut cache = document.invisible.borrow_mut();
        let column = cache.column(
            document.session.document(),
            encoding,
            content.clone(),
            eol.clone(),
            location.byte_offset,
        )?;
        if let Some(rect) = self.rectangle_for_pane(pane) {
            let layout = self.rectangle_display_layout(location.tab, position.line)?;
            let target = layout.column(VisualColumn(rect.head_column)).ok()?;
            if target.is_virtual() {
                // Virtual rectangle cells follow the EOL indicator; they never
                // acquire source bytes or become copyable presentation marks.
                return Some(
                    column
                        .saturating_add(eol.len().saturating_mul(2))
                        .saturating_add(target.virtual_spaces),
                );
            }
            return Some(column.saturating_add(target.tab_left));
        }
        Some(column)
    }
    fn rectangle_display_layout(&self, tab: TabId, line: usize) -> Option<LineLayout> {
        let document = self.document_by_tab(tab)?;
        let (content, _) = self.line_range_for_tab(tab, line)?;
        if content.len() > crate::rectangle::MAX_LINE_BYTES {
            return None;
        }
        let bytes = document
            .session
            .document()
            .read_range(content.clone())
            .ok()?;
        LineLayout::new(&bytes, document.session.encoding()?, content.start).ok()
    }
    /// A partial TAB highlights its complete atomic marker. The rectangle still
    /// edits/copies the original visual-cell slice, never the marker width.
    pub fn rectangle_source_selection(
        &self,
        tab: TabId,
        line: usize,
        rect: RectangleBounds,
    ) -> Option<Range<usize>> {
        if !(rect.first_line..=rect.last_line).contains(&line) || rect.left == rect.right {
            return None;
        }
        let layout = self.rectangle_display_layout(tab, line)?;
        let left = layout.column(VisualColumn(rect.left)).ok()?;
        let right = layout.column(VisualColumn(rect.right)).ok()?;
        let end = right.byte.saturating_add(usize::from(right.inside_tab()));
        Some(left.byte..end)
    }
}
