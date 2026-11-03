//! Universal line transforms through the existing F1 text transaction path.
//! Preparation reads only ASCII indentation prefixes and content tail bytes.
//! The affected-source cap is a preflight bound, not a bound on all F2 index,
//! normalization or cold scalar-layout work performed by apply_text_edits.
use super::{TextReplacement, Workbench, WorkbenchError};
use jc_core::Document;
use jc_files::FileError;
use jc_text::PositionError;
use std::ops::Range;

const MAX_JOIN_BOUNDARIES: usize = 1024;
const MAX_AFFECTED_SOURCE: usize = 2 * 1024 * 1024;
const PREFIX_CHUNK: usize = 4096;

fn rejected(message: &str) -> WorkbenchError {
    WorkbenchError::LineCommand(message.into())
}

fn indent_end(source: &Document, content: &Range<usize>) -> Result<usize, WorkbenchError> {
    let mut offset = content.start;
    while offset < content.end {
        let end = offset.saturating_add(PREFIX_CHUNK).min(content.end);
        for chunk in source.range_chunks(offset..end).map_err(FileError::from)? {
            for &byte in chunk {
                if !matches!(byte, b' ' | b'\t') {
                    return Ok(offset);
                }
                offset += 1;
            }
        }
    }
    Ok(offset)
}
fn tail(source: &Document, content: &Range<usize>) -> Result<Option<u8>, WorkbenchError> {
    if content.is_empty() {
        return Ok(None);
    }
    Ok(source
        .range_chunks(content.end - 1..content.end)
        .map_err(FileError::from)?
        .next()
        .and_then(|bytes| bytes.last().copied()))
}

impl Workbench {
    /// Join whole touched logical lines. A selection ending at the next line's
    /// start excludes that line; a single selected line has no internal boundary.
    /// Without a selection, join the caret's line with its next real line.
    pub fn join_lines(&mut self) -> Result<(), WorkbenchError> {
        self.require_linear_selection()?;
        let editor = self.active_editor().ok_or(WorkbenchError::NoDocument)?;
        let selection = editor.selection();
        let cursor = editor.cursor;
        let document = self.active_document().ok_or(WorkbenchError::NoDocument)?;
        let (id, tab, revision, source_len) = (
            document.id,
            document.tab_id,
            document.session.document().revision(),
            document.session.document().len(),
        );
        if document.session.encoding().is_none() {
            return Err(FileError::EncodingRequired.into());
        }
        let content_start = self
            .line_range_for_tab(tab, 0)
            .ok_or(PositionError::LineOutOfRange(0))?
            .0
            .start;
        let (first, mut last) = if let Some(range) = &selection {
            let first = self
                .line_ordinal_for_byte(tab, range.start.max(content_start))
                .ok_or(PositionError::SourceOffsetNotBoundary(range.start))?;
            let mut last = self
                .line_ordinal_for_byte(tab, range.end.max(content_start))
                .ok_or(PositionError::SourceOffsetNotBoundary(range.end))?;
            if last > first
                && self
                    .line_range_for_tab(tab, last)
                    .ok_or(PositionError::LineOutOfRange(last))?
                    .0
                    .start
                    == range.end
            {
                last -= 1;
            }
            (first, last)
        } else {
            let first = self
                .line_ordinal_for_byte(tab, cursor.max(content_start))
                .ok_or(PositionError::SourceOffsetNotBoundary(cursor))?;
            let count = self
                .line_count_for_tab(tab)
                .ok_or(WorkbenchError::NoDocument)?;
            (first, first.saturating_add(1).min(count.saturating_sub(1)))
        };
        // The empty EOF record created by a terminal EOL is not another real
        // line to consume: joining must not remove the document's final EOL.
        if last > first {
            let (content, separator) = self
                .line_range_for_tab(tab, last)
                .ok_or(PositionError::LineOutOfRange(last))?;
            if content.is_empty() && separator.is_empty() && content.start == source_len {
                last -= 1;
            }
        }
        if last <= first {
            return Ok(());
        }
        if last - first > MAX_JOIN_BOUNDARIES {
            return Err(rejected("Unir líneas supera 1024 separadores"));
        }
        let first_range = self
            .line_range_for_tab(tab, first)
            .ok_or(PositionError::LineOutOfRange(first))?;
        let last_range = self
            .line_range_for_tab(tab, last)
            .ok_or(PositionError::LineOutOfRange(last))?;
        let affected = last_range
            .1
            .end
            .checked_sub(first_range.0.start)
            .ok_or_else(|| rejected("Unir líneas encontró metadatos de línea inconsistentes"))?;
        if affected > MAX_AFFECTED_SOURCE {
            return Err(rejected("Unir líneas supera 2 MiB de fuente afectada"));
        }
        // Enforce both bounds from metadata before allocating the plan or
        // examining any indentation, including a long all-whitespace prefix.
        let slices = (first..=last)
            .map(|line| {
                self.line_range_for_tab(tab, line)
                    .ok_or(PositionError::LineOutOfRange(line))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let source = self
            .active_document()
            .ok_or(WorkbenchError::NoDocument)?
            .session
            .document();
        let mut previous_nonempty = !slices[0].0.is_empty();
        let mut previous_blank =
            tail(source, &slices[0].0)?.is_some_and(|b| matches!(b, b' ' | b'\t'));
        let mut edits = Vec::with_capacity(last - first);
        for pair in slices.windows(2) {
            let (previous, next) = (&pair[0], &pair[1]);
            if previous.1.is_empty() || previous.1.end != next.0.start {
                return Err(rejected(
                    "Unir líneas encontró metadatos de línea inconsistentes",
                ));
            }
            let next_start = indent_end(source, &next.0)?;
            let next_nonempty = next_start < next.0.end;
            let text = if previous_nonempty && !previous_blank && next_nonempty {
                " "
            } else {
                ""
            };
            edits.push(TextReplacement {
                range: previous.1.start..next_start,
                text: text.into(),
            });
            if next_nonempty {
                previous_nonempty = true;
                previous_blank = tail(source, &next.0)?.is_some_and(|b| matches!(b, b' ' | b'\t'));
            }
            // Blank/indent-only intermediate lines leave the accumulated left
            // state intact, so a\n\nb becomes a b rather than concatenating ab.
        }
        // One expected revision and one F1 batch; all content outside the small
        // EOL/prefix cuts remains in its original source pieces and encoding.
        self.apply_text_edits(id, revision, edits)
    }

    /// Same command engine as Enter: preferred EOL, encoding, linear-selection
    /// guard and one logical undo remain owned by insert_newline.
    pub fn split_line(&mut self) -> Result<(), WorkbenchError> {
        self.insert_newline()
    }
}
