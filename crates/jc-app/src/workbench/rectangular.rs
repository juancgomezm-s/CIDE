//! Bounded, one-action rectangular selections. Geometry never owns source bytes.
use super::{TextReplacement, Workbench, WorkbenchError};
use crate::rectangle::{LineLayout, RectangleError, VisualColumn, VisualPosition};
use crate::{PaneId, RectangularSelection, TabId};
use jc_files::FileError;

const MAX_ROWS: usize = 1024;
const MAX_BYTES: usize = 2 * 1024 * 1024;
const MAX_LINE_BYTES: usize = 64 * 1024;
const MAX_COLUMN: usize = 4096;

#[derive(Debug, Clone)]
pub(crate) enum Clipboard {
    Text(String),
    Rectangle(Vec<String>),
}
impl Clipboard {
    pub(super) fn has_text(&self) -> bool {
        match self {
            Self::Text(text) => !text.is_empty(),
            Self::Rectangle(rows) => rows.iter().any(|row| !row.is_empty()),
        }
    }
}

/// Ordinals of logical lines and visual cells, never scalar or byte positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RectangleBounds {
    pub first_line: usize,
    pub last_line: usize,
    pub left: usize,
    pub right: usize,
    pub head_column: usize,
    pub scroll_column: usize,
}

impl From<RectangleError> for WorkbenchError {
    fn from(error: RectangleError) -> Self {
        Self::Rectangle(error.to_string())
    }
}
fn rejected(message: &str) -> WorkbenchError {
    WorkbenchError::Rectangle(message.into())
}

impl Workbench {
    pub(super) fn active_rectangle(&self) -> Option<RectangleBounds> {
        self.rectangle_for_pane(self.panes.active())
    }

    pub fn rectangle_for_pane(&self, pane: PaneId) -> Option<RectangleBounds> {
        let view = self.panes.view(pane)?;
        let state = view.state()?;
        let rect = state.rectangle?;
        let tab = view.location()?.tab;
        let document = self.document_by_tab(tab)?;
        if rect.revision != document.session.document().revision() {
            return None;
        }
        let first = self
            .position_for_tab(tab, state.anchor.unwrap_or(state.cursor))?
            .line;
        let last = self.position_for_tab(tab, state.cursor)?.line;
        Some(RectangleBounds {
            first_line: first.min(last),
            last_line: first.max(last),
            left: rect.anchor_column.0.min(rect.head_column.0),
            right: rect.anchor_column.0.max(rect.head_column.0),
            head_column: rect.head_column.0,
            scroll_column: rect.scroll_column.0,
        })
    }

    fn rectangle_line(
        &self,
        tab: TabId,
        line: usize,
        charged: &mut usize,
    ) -> Result<LineLayout, WorkbenchError> {
        let document = self
            .document_by_tab(tab)
            .ok_or(WorkbenchError::NoDocument)?;
        let encoding = document
            .session
            .encoding()
            .ok_or(FileError::EncodingRequired)?;
        let (range, _) = self
            .line_range_for_tab(tab, line)
            .ok_or_else(|| rejected("rectangle line is outside the document"))?;
        if range.len() > MAX_LINE_BYTES || range.len() > MAX_BYTES.saturating_sub(*charged) {
            return Err(rejected("rectangle exceeds its line/source budget"));
        }
        *charged += range.len();
        let bytes = document
            .session
            .document()
            .read_range(range.clone())
            .map_err(FileError::from)?;
        Ok(LineLayout::new(&bytes, encoding, range.start)?)
    }

    fn check_rectangle_budget(
        &self,
        tab: TabId,
        rect: RectangleBounds,
    ) -> Result<(), WorkbenchError> {
        if rect.last_line.saturating_sub(rect.first_line) >= MAX_ROWS || rect.right > MAX_COLUMN {
            return Err(rejected(
                "rectangle exceeds 1024 rows or 4096 visual columns",
            ));
        }
        let mut charged = 0;
        for line in rect.first_line..=rect.last_line {
            let (range, _) = self
                .line_range_for_tab(tab, line)
                .ok_or_else(|| rejected("rectangle line is outside the document"))?;
            if range.len() > MAX_LINE_BYTES || range.len() > MAX_BYTES.saturating_sub(charged) {
                return Err(rejected("rectangle exceeds its line/source budget"));
            }
            charged += range.len();
        }
        Ok(())
    }

    fn validate_rectangle(
        &self,
        tab: TabId,
        rect: RectangleBounds,
        previous: Option<RectangleBounds>,
    ) -> Result<(), WorkbenchError> {
        self.check_rectangle_budget(tab, rect)?;
        let mut charged = 0;
        for line in rect.first_line..=rect.last_line {
            if previous.is_some_and(|old| (old.first_line..=old.last_line).contains(&line)) {
                continue; // Same revision: already validated, no steady-state re-decode.
            }
            self.rectangle_line(tab, line, &mut charged)?;
        }
        Ok(())
    }

    fn rectangle_seed(&self) -> Result<(usize, usize, RectangularSelection), WorkbenchError> {
        let editor = self.active_editor().ok_or(WorkbenchError::NoDocument)?;
        if let Some(rect) = editor.rectangle {
            return Ok((editor.anchor.unwrap_or(editor.cursor), editor.cursor, rect));
        }
        let anchor = editor.anchor.unwrap_or(editor.cursor);
        let first = self
            .position_for_tab(editor.tab_id, anchor)
            .ok_or_else(|| {
                rejected("rectangle requires text boundaries outside the BOM and CRLF")
            })?;
        let head = self.active_position().ok_or(WorkbenchError::NoDocument)?;
        let mut charged = 0;
        let a = self.rectangle_line(editor.tab_id, first.line, &mut charged)?;
        let h = self.rectangle_line(editor.tab_id, head.line, &mut charged)?;
        Ok((
            anchor,
            editor.cursor,
            RectangularSelection {
                anchor_column: a.visual_column_at_byte(anchor)?,
                head_column: h.visual_column_at_byte(editor.cursor)?,
                scroll_column: VisualColumn(0),
                revision: editor.session.document().revision(),
            },
        ))
    }

    /// Convert a linear selection's endpoints, or begin at the caret.
    pub fn start_rectangle(&mut self) -> Result<(), WorkbenchError> {
        self.extend_rectangle(0, 0)
    }

    /// Relative visual-cell/line movement. Failure leaves the old selection intact.
    pub fn extend_rectangle(&mut self, dx: isize, dy: isize) -> Result<(), WorkbenchError> {
        let previous = self.active_rectangle();
        let (anchor, old_cursor, rect) = self.rectangle_seed()?;
        let editor = self.active_editor().ok_or(WorkbenchError::NoDocument)?;
        let tab = editor.tab_id;
        let anchor_line = self
            .position_for_tab(tab, anchor)
            .ok_or(WorkbenchError::NoDocument)?
            .line;
        let head_line = self
            .position_for_tab(tab, old_cursor)
            .ok_or(WorkbenchError::NoDocument)?
            .line;
        let line_count = self
            .line_count_for_tab(tab)
            .ok_or(WorkbenchError::NoDocument)?;
        let line = head_line
            .saturating_add_signed(dy)
            .min(line_count.saturating_sub(1));
        let column = rect.head_column.0.saturating_add_signed(dx);
        self.assign_rectangle(
            VisualPosition {
                line: anchor_line,
                column: rect.anchor_column,
            },
            VisualPosition {
                line,
                column: VisualColumn(column),
            },
            previous,
            Some(anchor),
        )
    }

    /// Absolute visual input, admitted atomically by the same geometry as keys.
    pub fn set_rectangle(
        &mut self,
        anchor: VisualPosition,
        head: VisualPosition,
    ) -> Result<(), WorkbenchError> {
        let previous = self.active_rectangle();
        let known = self.active_editor().and_then(|editor| {
            let byte = editor.anchor.unwrap_or(editor.cursor);
            (previous.is_some()
                && editor.rectangle?.anchor_column == anchor.column
                && self.position_for_tab(editor.tab_id, byte)?.line == anchor.line)
                .then_some(byte)
        });
        self.assign_rectangle(anchor, head, previous, known)
    }

    fn assign_rectangle(
        &mut self,
        anchor: VisualPosition,
        head: VisualPosition,
        previous: Option<RectangleBounds>,
        known_anchor: Option<usize>,
    ) -> Result<(), WorkbenchError> {
        let editor = self.active_editor().ok_or(WorkbenchError::NoDocument)?;
        let tab = editor.tab_id;
        let revision = editor.session.document().revision();
        let scroll = editor
            .rectangle
            .map_or(VisualColumn(0), |r| r.scroll_column);
        let bounds = RectangleBounds {
            first_line: anchor.line.min(head.line),
            last_line: anchor.line.max(head.line),
            left: anchor.column.0.min(head.column.0),
            right: anchor.column.0.max(head.column.0),
            head_column: head.column.0,
            scroll_column: scroll.0,
        };
        self.validate_rectangle(tab, bounds, previous)?;
        let anchor_byte = match known_anchor {
            Some(byte) => byte,
            None => {
                self.rectangle_line(tab, anchor.line, &mut 0)?
                    .column(anchor.column)?
                    .byte
            }
        };
        let target = self
            .rectangle_line(tab, head.line, &mut 0)?
            .column(head.column)?
            .byte;
        let state = self
            .panes
            .active_state_mut()
            .ok_or(WorkbenchError::NoDocument)?;
        state.anchor = Some(anchor_byte);
        state.cursor = target;
        state.manual_scroll = false;
        state.rectangle = Some(RectangularSelection {
            anchor_column: anchor.column,
            head_column: head.column,
            scroll_column: scroll,
            revision,
        });
        // Visual movement within a TAB/virtual space may leave byte caret and
        // anchor unchanged. It is still new selection intent for deferred find.
        self.search_navigation = None;
        Ok(())
    }

    pub fn cancel_rectangle(&mut self) -> bool {
        if let Some(state) = self.panes.active_state_mut() {
            if state.rectangle.take().is_some() {
                state.anchor = None;
                return true;
            }
        }
        false
    }

    pub(super) fn require_linear_selection(&self) -> Result<(), WorkbenchError> {
        if self.active_rectangle().is_some() {
            Err(rejected(
                "command requires a linear selection; cancel the rectangle first",
            ))
        } else {
            Ok(())
        }
    }

    /// Only visible rows are prepared. Virtual cells use a presentation-only sentinel.
    pub fn rectangle_visible_line(
        &self,
        tab: TabId,
        line: usize,
        column: usize,
        width: usize,
    ) -> Result<Vec<(char, usize)>, WorkbenchError> {
        self.rectangle_grid_line(tab, line, column, width, false)
    }
    pub fn rectangle_glyph_line(
        &self,
        tab: TabId,
        line: usize,
        column: usize,
        width: usize,
        markers: bool,
    ) -> Result<Vec<crate::VisualAtom>, WorkbenchError> {
        let doc = self
            .document_by_tab(tab)
            .ok_or(WorkbenchError::NoDocument)?;
        let (content, _) = self
            .line_range_for_tab(tab, line)
            .ok_or_else(|| rejected("Rectangle visible line unavailable"))?;
        // Admission/editing use strict LineLayout. Presentation reuses the
        // document's sparse Unicode cache; never rebuild/decode a full line on
        // every frame. Source outside selected rows may be escaped diagnostically.
        doc.unicode
            .borrow_mut()
            .row(
                doc.session.document(),
                doc.session
                    .encoding()
                    .ok_or(jc_files::FileError::EncodingRequired)?,
                content,
                column,
                width,
                crate::unicode_layout::RowMode::Rectangle { markers },
            )
            .map_err(WorkbenchError::UnicodeLayout)
    }
    pub fn rectangle_grid_line(
        &self,
        tab: TabId,
        line: usize,
        column: usize,
        width: usize,
        markers: bool,
    ) -> Result<Vec<(char, usize)>, WorkbenchError> {
        Ok(self.rectangle_line(tab, line, &mut 0)?.expanded_cells(
            VisualColumn(column),
            width,
            markers,
        )?)
    }

    pub fn rectangle_position_from_byte(
        &self,
        tab: TabId,
        byte: usize,
    ) -> Result<VisualPosition, WorkbenchError> {
        let position = self
            .position_for_tab(tab, byte)
            .ok_or_else(|| rejected("rectangle byte is not a source boundary"))?;
        Ok(VisualPosition {
            line: position.line,
            column: self
                .rectangle_line(tab, position.line, &mut 0)?
                .visual_column_at_byte(byte)?,
        })
    }
    pub fn rectangle_byte_at(
        &self,
        tab: TabId,
        position: VisualPosition,
    ) -> Result<usize, WorkbenchError> {
        Ok(self
            .rectangle_line(tab, position.line, &mut 0)?
            .column(position.column)?
            .byte)
    }
    /// Map the scalar presentation, including padding past EOL, to TAB4 cells.
    pub fn rectangle_position_from_scalar(
        &self,
        tab: TabId,
        position: jc_text::TextPosition,
    ) -> Result<VisualPosition, WorkbenchError> {
        let (content, _) = self
            .line_range_for_tab(tab, position.line)
            .ok_or(WorkbenchError::NoDocument)?;
        let end = self
            .position_for_tab(tab, content.end)
            .ok_or(WorkbenchError::NoDocument)?;
        let byte = self
            .with_text(tab, |text, source| {
                text.offset(
                    source,
                    jc_text::TextPosition {
                        line: position.line,
                        column: position.column.min(end.column),
                    },
                )
            })
            .ok_or(WorkbenchError::NoDocument)??;
        let layout = self.rectangle_line(tab, position.line, &mut 0)?;
        let column = VisualColumn(
            layout
                .visual_column_at_byte(byte)?
                .0
                .saturating_add(position.column.saturating_sub(end.column)),
        );
        layout.column(column)?;
        Ok(VisualPosition {
            line: position.line,
            column,
        })
    }

    fn rectangle_copy_rows(&self) -> Result<Vec<String>, WorkbenchError> {
        let rect = self
            .active_rectangle()
            .ok_or_else(|| rejected("no rectangle selected"))?;
        let tab = self
            .active_document()
            .ok_or(WorkbenchError::NoDocument)?
            .tab_id;
        let mut charged = 0;
        let mut owned = 0;
        let mut rows = Vec::new();
        for line in rect.first_line..=rect.last_line {
            let layout = self.rectangle_line(tab, line, &mut charged)?;
            let row = layout.copy_columns(VisualColumn(rect.left), VisualColumn(rect.right))?;
            owned += row.capacity() + std::mem::size_of::<String>();
            if owned > MAX_BYTES {
                return Err(rejected("rectangular clipboard exceeds 2 MiB"));
            }
            rows.push(row);
        }
        rows.shrink_to_fit();
        Ok(rows)
    }

    pub(super) fn copy_rectangle(&mut self) -> Result<(), WorkbenchError> {
        self.clipboard = Some(Clipboard::Rectangle(self.rectangle_copy_rows()?));
        Ok(())
    }

    pub(super) fn cut_rectangle(&mut self) -> Result<(), WorkbenchError> {
        let rows = self.rectangle_copy_rows()?;
        self.replace_rectangle_rows(&[String::new()], None)?;
        self.clipboard = Some(Clipboard::Rectangle(rows));
        Ok(())
    }

    pub(super) fn insert_rectangle_text(&mut self, text: &str) -> Result<(), WorkbenchError> {
        if text.len() > MAX_BYTES {
            return Err(rejected("rectangular paste exceeds 2 MiB"));
        }
        // CRLF is one separator. Bare CR is a separator too, never source content.
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        let rows: Vec<String> = normalized
            .split('\n')
            .take(MAX_ROWS + 1)
            .map(str::to_owned)
            .collect();
        if rows.len() > MAX_ROWS {
            return Err(rejected("rectangular paste exceeds 1024 rows"));
        }
        self.replace_rectangle_rows(&rows, None)
    }

    pub(super) fn paste_rectangle_rows(&mut self, rows: &[String]) -> Result<(), WorkbenchError> {
        if self.active_rectangle().is_some() {
            return self.replace_rectangle_rows(rows, None);
        }
        if self.active_editor().and_then(|e| e.selection()).is_some() {
            return Err(rejected("rectangular paste requires a caret or rectangle"));
        }
        // Compute a temporary target; failed admission never enables column mode.
        let (anchor, cursor, seed) = self.rectangle_seed()?;
        debug_assert_eq!(anchor, cursor);
        let line = self
            .active_position()
            .ok_or(WorkbenchError::NoDocument)?
            .line;
        if rows.is_empty() || rows.len() > MAX_ROWS {
            return Err(rejected("invalid rectangular clipboard height"));
        }
        let bounds = RectangleBounds {
            first_line: line,
            last_line: line.saturating_add(rows.len() - 1),
            left: seed.head_column.0,
            right: seed.head_column.0,
            head_column: seed.head_column.0,
            scroll_column: 0,
        };
        self.replace_rectangle_rows(rows, Some(bounds))
    }

    pub(super) fn delete_rectangle(&mut self, backwards: bool) -> Result<(), WorkbenchError> {
        let mut rect = self
            .active_rectangle()
            .ok_or_else(|| rejected("no rectangle selected"))?;
        if rect.left == rect.right {
            if backwards {
                if rect.left == 0 {
                    return Ok(());
                }
                rect.left -= 1;
            } else {
                rect.right = rect.right.saturating_add(1);
            }
        }
        self.replace_rectangle_rows(&[String::new()], Some(rect))
    }

    fn replace_rectangle_rows(
        &mut self,
        rows: &[String],
        bounds: Option<RectangleBounds>,
    ) -> Result<(), WorkbenchError> {
        let rect = bounds
            .or_else(|| self.active_rectangle())
            .ok_or_else(|| rejected("no rectangle selected"))?;
        let height = rect
            .last_line
            .saturating_sub(rect.first_line)
            .saturating_add(1);
        if rows.len() != 1 && rows.len() != height {
            return Err(rejected(
                "paste must contain one row or exactly the rectangle height",
            ));
        }
        let editor = self.active_editor().ok_or(WorkbenchError::NoDocument)?;
        let (id, tab, revision) = (
            editor.document_id,
            editor.tab_id,
            editor.session.document().revision(),
        );
        self.check_rectangle_budget(tab, rect)?;
        let encoding = editor
            .session
            .encoding()
            .ok_or(FileError::EncodingRequired)?;
        let mut charged = 0;
        let mut owned = 0;
        let mut edits = Vec::new();
        for (index, line) in (rect.first_line..=rect.last_line).enumerate() {
            let layout = self.rectangle_line(tab, line, &mut charged)?;
            let row = &rows[if rows.len() == 1 { 0 } else { index }];
            let edit =
                layout.replace_columns(VisualColumn(rect.left), VisualColumn(rect.right), row)?;
            let bytes = jc_text::encode(&edit.text, encoding)
                .map_err(|_| WorkbenchError::ClipboardEncoding)?;
            owned +=
                bytes.capacity() + edit.text.capacity() + std::mem::size_of::<TextReplacement>();
            if owned > MAX_BYTES {
                return Err(rejected("rectangular edits exceed 2 MiB staging"));
            }
            let original = editor
                .session
                .document()
                .read_range(edit.range.clone())
                .map_err(FileError::from)?;
            if original != bytes {
                edits.push(TextReplacement {
                    range: edit.range,
                    text: edit.text,
                });
            }
        }
        if edits.is_empty() {
            return Ok(());
        }
        let first = edits[0].range.start;
        self.apply_text_edits(id, revision, edits)?;
        let editor = self.active_editor_mut().ok_or(WorkbenchError::NoDocument)?;
        let end = editor
            .session
            .document()
            .last_change()
            .map_or(first, |change| change.map_offset(first));
        editor.state.cursor = end;
        editor.state.anchor = None;
        editor.state.rectangle = None;
        Ok(())
    }
}
