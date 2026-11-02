use std::fmt;
use std::io::{self, Write};
use std::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    InvalidRange {
        start: usize,
        end: usize,
        len: usize,
    },
    CapacityOverflow,
    StaleRevision {
        expected: u64,
        actual: u64,
    },
    OverlappingEdits,
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRange { start, end, len } => {
                write!(
                    f,
                    "invalid byte range {start}..{end} for document length {len}"
                )
            }
            Self::StaleRevision { expected, actual } => {
                write!(f, "expected revision {expected}, found {actual}")
            }
            Self::OverlappingEdits => write!(f, "overlapping or ambiguous edits"),
            Self::CapacityOverflow => write!(f, "document capacity or revision overflow"),
        }
    }
}

impl std::error::Error for EditError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    Original,
    Added,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Piece {
    source: Source,
    start: usize,
    len: usize,
}

#[derive(Debug)]
struct State {
    pieces: Vec<Piece>,
    len: usize,
    revision: u64,
    edit: Option<ChangeSet>,
}

/// Geometry of a single edit, in source bytes before it.
/// Undo reports the inverse geometry; no source text is copied or interpreted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ByteEdit {
    pub replaced: Range<usize>,
    pub inserted_len: usize,
}

/// A replacement in the coordinates of the expected revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replacement {
    pub range: Range<usize>,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditBatch {
    pub expected_revision: u64,
    pub edits: Vec<Replacement>,
}

/// Edits are ordered for sequential position mapping (rightmost first on commit).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeSet {
    pub before_revision: u64,
    pub after_revision: u64,
    pub edits: Vec<ByteEdit>,
}

impl ChangeSet {
    pub fn map_offset(&self, mut offset: usize) -> usize {
        for edit in &self.edits {
            offset = if offset < edit.replaced.start {
                offset
            } else if offset >= edit.replaced.end {
                offset - edit.replaced.len() + edit.inserted_len
            } else {
                edit.replaced.start + edit.inserted_len
            };
        }
        offset
    }

    pub fn inverse(&self) -> Self {
        Self {
            before_revision: self.after_revision,
            after_revision: self.before_revision,
            edits: self
                .edits
                .iter()
                .rev()
                .map(|edit| ByteEdit {
                    replaced: edit.replaced.start..edit.replaced.start + edit.inserted_len,
                    inserted_len: edit.replaced.len(),
                })
                .collect(),
        }
    }
}

pub struct Document {
    original: Vec<u8>,
    added: Vec<u8>,
    state: State,
    undo: Vec<State>,
    redo: Vec<State>,
    saved_revision: u64,
    next_revision: u64,
    history_limit: usize,
    last_edit: Option<ChangeSet>,
}

impl fmt::Debug for Document {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Document")
            .field("len", &self.len())
            .field("pieces", &self.piece_count())
            .field("revision", &self.revision())
            .field("modified", &self.is_modified())
            .field("undo_steps", &self.undo.len())
            .field("redo_steps", &self.redo.len())
            .finish()
    }
}

impl Document {
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        let len = bytes.len();
        let pieces = if len == 0 {
            Vec::new()
        } else {
            vec![Piece {
                source: Source::Original,
                start: 0,
                len,
            }]
        };
        Self {
            original: bytes,
            added: Vec::new(),
            state: State {
                pieces,
                len,
                revision: 0,
                edit: None,
            },
            undo: Vec::new(),
            redo: Vec::new(),
            saved_revision: 0,
            next_revision: 1,
            history_limit: 1024,
            last_edit: None,
        }
    }

    pub fn len(&self) -> usize {
        self.state.len
    }
    pub fn is_empty(&self) -> bool {
        self.state.len == 0
    }
    pub fn revision(&self) -> u64 {
        self.state.revision
    }
    pub fn last_edit(&self) -> Option<&ByteEdit> {
        self.last_edit
            .as_ref()
            .and_then(|change| (change.edits.len() == 1).then(|| &change.edits[0]))
    }
    pub fn last_change(&self) -> Option<&ChangeSet> {
        self.last_edit.as_ref()
    }
    pub fn is_modified(&self) -> bool {
        self.state.revision != self.saved_revision
    }
    pub fn mark_saved(&mut self) {
        self.saved_revision = self.state.revision;
    }
    pub fn piece_count(&self) -> usize {
        self.state.pieces.len()
    }
    pub fn history_limit(&self) -> usize {
        self.history_limit
    }
    pub fn undo_steps(&self) -> usize {
        self.undo.len()
    }
    pub fn redo_steps(&self) -> usize {
        self.redo.len()
    }
    pub fn retained_added_bytes(&self) -> usize {
        self.added.len()
    }
    pub fn set_history_limit(&mut self, limit: usize) {
        self.history_limit = limit;
        trim_history(&mut self.undo, limit);
        trim_history(&mut self.redo, limit);
    }
    pub fn compact_storage(&mut self) {
        let bytes = self.to_bytes();
        let len = bytes.len();
        self.original = bytes;
        self.added.clear();
        self.state.pieces = if len == 0 {
            Vec::new()
        } else {
            vec![Piece {
                source: Source::Original,
                start: 0,
                len,
            }]
        };
        self.state.len = len;
        self.undo.clear();
        self.redo.clear();
    }

    pub fn chunks(&self) -> impl Iterator<Item = &[u8]> {
        self.state
            .pieces
            .iter()
            .map(|piece| self.piece_bytes(piece))
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.len());
        for chunk in self.chunks() {
            bytes.extend_from_slice(chunk);
        }
        bytes
    }

    pub fn write_to<W: Write + ?Sized>(&self, writer: &mut W) -> io::Result<()> {
        for chunk in self.chunks() {
            writer.write_all(chunk)?;
        }
        Ok(())
    }

    pub fn insert(&mut self, offset: usize, bytes: &[u8]) -> Result<(), EditError> {
        self.replace(offset..offset, bytes)
    }

    pub fn delete(&mut self, range: Range<usize>) -> Result<(), EditError> {
        self.replace(range, &[])
    }

    pub fn replace(&mut self, range: Range<usize>, bytes: &[u8]) -> Result<(), EditError> {
        self.apply_batch(EditBatch {
            expected_revision: self.revision(),
            edits: vec![Replacement {
                range,
                bytes: bytes.to_vec(),
            }],
        })
    }

    pub fn apply_batch(&mut self, mut batch: EditBatch) -> Result<(), EditError> {
        if batch.expected_revision != self.revision() {
            return Err(EditError::StaleRevision {
                expected: batch.expected_revision,
                actual: self.revision(),
            });
        }
        batch
            .edits
            .sort_by_key(|edit| (edit.range.start, edit.range.end));
        for edit in &batch.edits {
            self.validate_range(&edit.range)?;
        }
        for pair in batch.edits.windows(2) {
            if pair[0].range.end > pair[1].range.start || pair[0].range.start == pair[1].range.start
            {
                return Err(EditError::OverlappingEdits);
            }
        }
        batch
            .edits
            .retain(|edit| !self.range_matches(&edit.range, &edit.bytes));
        if batch.edits.is_empty() {
            self.last_edit = None;
            return Ok(());
        }
        let mut new_len = self.len();
        let mut added_len = 0usize;
        for edit in &batch.edits {
            new_len = new_len
                .checked_sub(edit.range.len())
                .and_then(|n| n.checked_add(edit.bytes.len()))
                .ok_or(EditError::CapacityOverflow)?;
            added_len = added_len
                .checked_add(edit.bytes.len())
                .ok_or(EditError::CapacityOverflow)?;
        }
        let following_revision = self
            .next_revision
            .checked_add(1)
            .ok_or(EditError::CapacityOverflow)?;
        // Reserve every fallible allocation before publishing bytes or history.
        let capacity = batch
            .edits
            .len()
            .checked_mul(2)
            .and_then(|n| n.checked_add(self.piece_count()))
            .ok_or(EditError::CapacityOverflow)?;
        let mut pieces = Vec::new();
        pieces
            .try_reserve(capacity)
            .map_err(|_| EditError::CapacityOverflow)?;
        let mut edits = Vec::new();
        edits
            .try_reserve(batch.edits.len())
            .map_err(|_| EditError::CapacityOverflow)?;
        self.added
            .try_reserve(added_len)
            .map_err(|_| EditError::CapacityOverflow)?;
        if self.history_limit > 0 {
            self.undo
                .try_reserve(1)
                .map_err(|_| EditError::CapacityOverflow)?;
        }
        let mut position = 0;
        let mut added_position = self.added.len();
        for edit in &batch.edits {
            self.append_range(position..edit.range.start, &mut pieces);
            push_coalesced(
                &mut pieces,
                Piece {
                    source: Source::Added,
                    start: added_position,
                    len: edit.bytes.len(),
                },
            );
            added_position += edit.bytes.len();
            position = edit.range.end;
            edits.push(ByteEdit {
                replaced: edit.range.clone(),
                inserted_len: edit.bytes.len(),
            });
        }
        self.append_range(position..self.len(), &mut pieces);
        edits.reverse();
        let change = ChangeSet {
            before_revision: self.revision(),
            after_revision: self.next_revision,
            edits,
        };
        let mut state_edits = Vec::new();
        state_edits
            .try_reserve(change.edits.len())
            .map_err(|_| EditError::CapacityOverflow)?;
        state_edits.extend_from_slice(&change.edits);
        let state_change = ChangeSet {
            before_revision: change.before_revision,
            after_revision: change.after_revision,
            edits: state_edits,
        };
        for edit in batch.edits {
            self.added.extend_from_slice(&edit.bytes);
        }
        let previous = std::mem::replace(
            &mut self.state,
            State {
                pieces,
                len: new_len,
                revision: self.next_revision,
                edit: Some(state_change),
            },
        );
        self.next_revision = following_revision;
        if self.history_limit > 0 {
            self.undo.push(previous);
            trim_history(&mut self.undo, self.history_limit);
        }
        self.redo.clear();
        self.last_edit = Some(change);
        Ok(())
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) -> bool {
        self.last_edit = None;
        let Some(previous) = self.undo.pop() else {
            return false;
        };
        let current = std::mem::replace(&mut self.state, previous);
        self.last_edit = current.edit.as_ref().map(ChangeSet::inverse);
        if self.history_limit > 0 {
            self.redo.push(current);
            trim_history(&mut self.redo, self.history_limit);
        }
        true
    }

    pub fn redo(&mut self) -> bool {
        self.last_edit = None;
        let Some(next) = self.redo.pop() else {
            return false;
        };
        let current = std::mem::replace(&mut self.state, next);
        self.last_edit = self.state.edit.clone();
        if self.history_limit > 0 {
            self.undo.push(current);
            trim_history(&mut self.undo, self.history_limit);
        }
        true
    }

    fn validate_range(&self, range: &Range<usize>) -> Result<(), EditError> {
        if range.start > range.end || range.end > self.len() {
            Err(EditError::InvalidRange {
                start: range.start,
                end: range.end,
                len: self.len(),
            })
        } else {
            Ok(())
        }
    }

    fn piece_bytes(&self, piece: &Piece) -> &[u8] {
        let source = match piece.source {
            Source::Original => &self.original,
            Source::Added => &self.added,
        };
        &source[piece.start..piece.start + piece.len]
    }

    fn range_matches(&self, range: &Range<usize>, bytes: &[u8]) -> bool {
        if range.end - range.start != bytes.len() {
            return false;
        }
        let mut position = 0;
        let mut compared = 0;
        for piece in &self.state.pieces {
            let end = position + piece.len;
            if position >= range.end {
                break;
            }
            if end > range.start {
                let start_in_piece = range.start.saturating_sub(position);
                let end_in_piece = piece.len.min(range.end - position);
                let chunk = &self.piece_bytes(piece)[start_in_piece..end_in_piece];
                if chunk != &bytes[compared..compared + chunk.len()] {
                    return false;
                }
                compared += chunk.len();
            }
            position = end;
        }
        compared == bytes.len()
    }

    fn append_range(&self, range: Range<usize>, pieces: &mut Vec<Piece>) {
        if range.is_empty() {
            return;
        }
        let mut position = 0;
        for piece in &self.state.pieces {
            let end = position + piece.len;
            if position >= range.end {
                break;
            }
            if end > range.start {
                let offset = range.start.saturating_sub(position);
                let length = end.min(range.end) - position.max(range.start);
                push_coalesced(
                    pieces,
                    Piece {
                        source: piece.source,
                        start: piece.start + offset,
                        len: length,
                    },
                );
            }
            position = end;
        }
    }
}

fn trim_history(history: &mut Vec<State>, limit: usize) {
    if history.len() > limit {
        history.drain(..history.len() - limit);
    }
}

fn push_coalesced(pieces: &mut Vec<Piece>, piece: Piece) {
    if piece.len == 0 {
        return;
    }
    if let Some(last) = pieces.last_mut() {
        if last.source == piece.source && last.start + last.len == piece.start {
            last.len += piece.len;
            return;
        }
    }
    pieces.push(piece);
}

impl Default for Document {
    fn default() -> Self {
        Self::from_bytes(Vec::new())
    }
}
