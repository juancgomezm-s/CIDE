//! Selection metadata attached to canonical core transitions, never source text.
use super::WorkbenchError;
use crate::{
    panes::{RectangularSelection, ViewState},
    ViewId,
};
use jc_core::Document;
use jc_text::TextEncoding;
use std::{io, mem::size_of};

pub(super) const MAX_RECORDS: usize = 1024;
const MAX_BYTES: usize = 256 * 1024;

#[derive(Clone, Copy, Debug)]
pub(super) struct SelectionState {
    pub cursor: usize,
    pub anchor: Option<usize>,
    pub rectangle: Option<RectangularSelection>,
}
impl SelectionState {
    pub fn capture(view: &ViewState) -> Self {
        Self {
            cursor: view.cursor,
            anchor: view.anchor,
            rectangle: view.rectangle,
        }
    }
    /// Restore selection geometry only. Viewport and focus keep their one owner.
    pub fn restore(self, view: &mut ViewState, revision: u64) {
        view.cursor = self.cursor;
        view.anchor = self.anchor;
        view.rectangle = self.rectangle.map(|mut rectangle| {
            rectangle.revision = revision;
            rectangle
        });
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct SelectionRecord {
    pub before_revision: u64,
    pub after_revision: u64,
    pub view: ViewId,
    pub encoding: TextEncoding,
    pub before: SelectionState,
    pub after: SelectionState,
}

#[derive(Default, Debug)]
pub(super) struct SelectionHistory {
    records: Vec<SelectionRecord>,
}
fn capacity_error() -> WorkbenchError {
    WorkbenchError::Save("No se pudo reservar el historial de selección dentro de su cuota".into())
}
impl SelectionHistory {
    /// Must precede the content mutation. Subsequent `record` never allocates.
    pub fn reserve_for_edit(&mut self, source: &Document) -> Result<(), WorkbenchError> {
        self.prune(source);
        if source.history_limit() == 0 || source.history_bytes_limit() == 0 {
            self.records = Vec::new();
            return Ok(());
        }
        let needed = self.records.len().saturating_add(1).min(MAX_RECORDS);
        if needed
            .checked_mul(size_of::<SelectionRecord>())
            .is_none_or(|n| n > MAX_BYTES)
        {
            return Err(capacity_error());
        }
        if self.records.capacity() < needed {
            // Reserve in bounded blocks, never once per keystroke. The Vec is
            // unchanged on allocation failure, before any source publication.
            let ceiling = MAX_RECORDS.min(MAX_BYTES / size_of::<SelectionRecord>());
            let target = self
                .records
                .capacity()
                .saturating_mul(2)
                .max(16)
                .max(needed)
                .min(ceiling);
            if target < needed {
                return Err(capacity_error());
            }
            self.records
                .try_reserve_exact(target - self.records.len())
                .map_err(|_| capacity_error())?;
        }
        if self.memory_bytes() > MAX_BYTES {
            return Err(capacity_error());
        }
        Ok(())
    }

    /// A successful edit may trim old core transitions or clear its redo branch.
    /// Neither operation can leave dangling selection metadata. When more than
    /// 1024 transitions survive, only the most recent selection records remain.
    pub fn record(&mut self, record: SelectionRecord, source: &Document) {
        self.prune(source);
        if source.history_limit() == 0
            || record.before_revision >= record.after_revision
            || record.view.0 == 0
        {
            return;
        }
        if !source.history_transitions().any(|step| {
            step.before_revision == record.before_revision
                && step.after_revision == record.after_revision
        }) {
            return;
        }
        match self
            .records
            .binary_search_by_key(&record.after_revision, |item| item.after_revision)
        {
            Ok(index) => {
                if self.records[index].before_revision == record.before_revision {
                    self.records[index] = record;
                }
                return;
            }
            Err(index) if index != self.records.len() => return,
            _ => {}
        }
        // The caller has already reserved; never grow after publishing bytes.
        // The fallback also keeps this helper allocation-free for an omitted
        // reservation, without allowing metadata failure to unwind a live edit.
        if self.records.len() >= MAX_RECORDS || self.records.len() == self.records.capacity() {
            if self.records.is_empty() {
                return;
            }
            self.records.remove(0);
        }
        self.records.push(record);
    }

    /// Both inputs are ordered by after_revision; one streaming merge, no
    /// document/history clones and no per-record scan over the core history.
    pub fn prune(&mut self, source: &Document) {
        let mut retained = source.history_transitions().peekable();
        self.records.retain(|record| {
            while retained
                .peek()
                .is_some_and(|step| step.after_revision < record.after_revision)
            {
                retained.next();
            }
            retained.peek().is_some_and(|step| {
                step.before_revision == record.before_revision
                    && step.after_revision == record.after_revision
            })
        });
    }

    pub fn find(&self, before: u64, after: u64) -> Option<SelectionRecord> {
        let index = self
            .records
            .binary_search_by_key(&after, |item| item.after_revision)
            .ok()?;
        let record = self.records[index];
        (record.before_revision == before).then_some(record)
    }
    pub fn records(&self) -> &[SelectionRecord] {
        &self.records
    }

    /// Source lengths, retained transitions and restored ViewIds are validated
    /// by the recovery integrator after decoding the canonical documents.
    pub fn from_records(records: Vec<SelectionRecord>) -> io::Result<Self> {
        if records.len() > MAX_RECORDS
            || records
                .capacity()
                .checked_mul(size_of::<SelectionRecord>())
                .is_none_or(|n| n > MAX_BYTES)
            || records
                .iter()
                .any(|record| record.before_revision >= record.after_revision || record.view.0 == 0)
            || records
                .windows(2)
                .any(|pair| pair[0].after_revision >= pair[1].after_revision)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Historial de selección inválido o fuera de cuota",
            ));
        }
        Ok(Self { records })
    }
    pub fn memory_bytes(&self) -> usize {
        self.records
            .capacity()
            .saturating_mul(size_of::<SelectionRecord>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rectangle::VisualColumn;

    fn record(before: u64, after: u64) -> SelectionRecord {
        SelectionRecord {
            before_revision: before,
            after_revision: after,
            view: ViewId(7),
            encoding: TextEncoding::Utf8,
            before: SelectionState {
                cursor: 1,
                anchor: Some(0),
                rectangle: None,
            },
            after: SelectionState {
                cursor: 1,
                anchor: None,
                rectangle: None,
            },
        }
    }
    fn edit(source: &mut Document, selections: &mut SelectionHistory, byte: u8) -> (u64, u64) {
        selections.reserve_for_edit(source).unwrap();
        let bytes = selections.memory_bytes();
        let before = source.revision();
        source.replace(0..1, &[byte]).unwrap();
        let after = source.revision();
        selections.record(record(before, after), source);
        assert_eq!(
            selections.memory_bytes(),
            bytes,
            "Recording allocated after content publication"
        );
        (before, after)
    }

    #[test]
    fn selection_metadata_is_lazy_and_follows_undo_redo_and_branching() {
        let mut source = Document::from_bytes(b"0".to_vec());
        let mut selections = SelectionHistory::default();
        assert_eq!(selections.memory_bytes(), 0);
        let first = edit(&mut source, &mut selections, b'1');
        let second = edit(&mut source, &mut selections, b'2');
        assert!(source.undo());
        selections.prune(&source);
        assert!(selections.find(second.0, second.1).is_some());
        assert!(source.redo());
        selections.prune(&source);
        assert!(selections.find(first.0, first.1).is_some());
        assert!(source.undo());
        let branch = edit(&mut source, &mut selections, b'3');
        assert!(selections.find(second.0, second.1).is_none());
        assert!(selections.find(branch.0, branch.1).is_some());
        assert_eq!(source.to_bytes(), b"3");
        source.set_history_limit(1);
        selections.prune(&source);
        assert!(selections.find(first.0, first.1).is_none());
        assert_eq!(selections.records().len(), 1);
        source.compact_storage();
        selections.prune(&source);
        assert!(selections.records().is_empty());
    }

    #[test]
    fn no_op_and_disabled_or_budget_trimmed_core_history_create_no_records() {
        let mut source = Document::from_bytes(b"0".to_vec());
        let mut selections = SelectionHistory::default();
        edit(&mut source, &mut selections, b'0');
        assert!(selections.records().is_empty());
        edit(&mut source, &mut selections, b'1');
        source.set_history_bytes_limit(0);
        selections.prune(&source);
        assert!(selections.records().is_empty());
        source.set_history_limit(0);
        edit(&mut source, &mut selections, b'2');
        assert!(selections.records().is_empty());
        assert_eq!(selections.memory_bytes(), 0);
    }

    #[test]
    fn selection_record_count_and_reserved_capacity_stay_bounded() {
        let mut source = Document::from_bytes(b"0".to_vec());
        source.set_history_limit(2048);
        let mut selections = SelectionHistory::default();
        let oldest = edit(&mut source, &mut selections, b'1');
        let mut growths = 1;
        for index in 0..MAX_RECORDS {
            let previous_capacity = selections.memory_bytes();
            edit(
                &mut source,
                &mut selections,
                if index % 2 == 0 { b'2' } else { b'1' },
            );
            growths += usize::from(selections.memory_bytes() != previous_capacity);
        }
        assert!(
            growths <= 8,
            "Selection metadata reallocated per edit: {growths}"
        );
        assert_eq!(selections.records().len(), MAX_RECORDS);
        assert!(selections.find(oldest.0, oldest.1).is_none());
        assert!(selections.memory_bytes() <= MAX_BYTES);
        eprintln!(
            "SelectionState={}B SelectionRecord={}B maximum retained allocation={}B",
            size_of::<SelectionState>(),
            size_of::<SelectionRecord>(),
            selections.memory_bytes()
        );
        assert!(selections
            .records()
            .windows(2)
            .all(|pair| pair[0].after_revision < pair[1].after_revision));
    }

    #[test]
    fn metadata_decode_rejects_duplicates_bad_ids_revisions_and_excess_capacity() {
        assert!(SelectionHistory::from_records(vec![record(0, 1)]).is_ok());
        assert!(SelectionHistory::from_records(vec![record(0, 1), record(0, 1)]).is_err());
        assert!(SelectionHistory::from_records(vec![record(1, 1)]).is_err());
        assert!(SelectionHistory::from_records(vec![SelectionRecord {
            view: ViewId(0),
            ..record(0, 1)
        }])
        .is_err());
        assert!(SelectionHistory::from_records(vec![record(1, 2), record(0, 1)]).is_err());
        assert!(SelectionHistory::from_records(vec![record(0, 1); MAX_RECORDS + 1]).is_err());
        let large = Vec::with_capacity(MAX_BYTES / size_of::<SelectionRecord>() + 1);
        assert!(SelectionHistory::from_records(large).is_err());
    }

    #[test]
    fn capture_restore_preserves_orientation_rectangle_and_independent_viewport() {
        let before = ViewState {
            cursor: 2,
            anchor: Some(9),
            rectangle: Some(RectangularSelection {
                anchor_column: VisualColumn(7),
                head_column: VisualColumn(3),
                scroll_column: VisualColumn(2),
                revision: 4,
            }),
            ..ViewState::default()
        };
        let snapshot = SelectionState::capture(&before);
        let mut target = ViewState {
            scroll_line: 20,
            scroll_column: 30,
            manual_scroll: true,
            ..ViewState::default()
        };
        snapshot.restore(&mut target, 17);
        assert_eq!((target.cursor, target.anchor), (2, Some(9)));
        let rectangle = target.rectangle.unwrap();
        assert_eq!(
            (
                rectangle.anchor_column.0,
                rectangle.head_column.0,
                rectangle.scroll_column.0,
                rectangle.revision
            ),
            (7, 3, 2, 17)
        );
        assert_eq!(
            (
                target.scroll_line,
                target.scroll_column,
                target.manual_scroll
            ),
            (20, 30, true)
        );
        SelectionState {
            rectangle: None,
            ..snapshot
        }
        .restore(&mut target, 18);
        assert!(target.rectangle.is_none());
    }
}
