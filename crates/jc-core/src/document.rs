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

const PIECE_BLOCK: usize = 128;
#[derive(Debug)]
struct PieceIndex {
    blocks: Vec<usize>,
    local: Vec<usize>,
}
impl PieceIndex {
    fn single(len: usize) -> Self {
        if len == 0 {
            Self {
                blocks: Vec::new(),
                local: Vec::new(),
            }
        } else {
            Self {
                blocks: vec![len],
                local: vec![len],
            }
        }
    }
    fn build(pieces: &[Piece]) -> Result<Self, EditError> {
        let mut result = Self {
            blocks: Vec::new(),
            local: Vec::new(),
        };
        result
            .blocks
            .try_reserve(pieces.len().div_ceil(PIECE_BLOCK))
            .map_err(|_| EditError::CapacityOverflow)?;
        result
            .local
            .try_reserve(pieces.len())
            .map_err(|_| EditError::CapacityOverflow)?;
        let mut total = 0;
        for block in pieces.chunks(PIECE_BLOCK) {
            let mut subtotal = 0;
            for piece in block {
                subtotal += piece.len;
                result.local.push(subtotal);
            }
            total += subtotal;
            result.blocks.push(total);
        }
        Ok(result)
    }
    fn locate(&self, offset: usize) -> (usize, usize) {
        let block = self.blocks.partition_point(|&end| end <= offset);
        if block == self.blocks.len() {
            return (self.local.len(), self.blocks.last().copied().unwrap_or(0));
        }
        let base = block.checked_sub(1).map_or(0, |i| self.blocks[i]);
        let from = block * PIECE_BLOCK;
        let local = &self.local[from..(from + PIECE_BLOCK).min(self.local.len())];
        let index = local.partition_point(|&end| end <= offset - base);
        (
            from + index,
            base + index.checked_sub(1).map_or(0, |i| local[i]),
        )
    }
    fn memory_bytes(&self) -> usize {
        (self.blocks.capacity() + self.local.capacity()) * std::mem::size_of::<usize>()
    }
}

#[derive(Debug)]
struct State {
    pieces: Vec<Piece>,
    ends: PieceIndex,
    history_charge: usize,
    history_payload: usize,
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
    history_bytes_limit: usize,
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
                ends: PieceIndex::single(len),
                history_charge: std::mem::size_of::<State>()
                    + std::mem::size_of::<Piece>()
                    + 2 * std::mem::size_of::<usize>(),
                history_payload: 0,
                len,
                revision: 0,
                edit: None,
            },
            undo: Vec::new(),
            redo: Vec::new(),
            saved_revision: 0,
            next_revision: 1,
            history_limit: 1024,
            history_bytes_limit: 8 * 1024 * 1024,
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
    pub fn history_bytes_limit(&self) -> usize {
        self.history_bytes_limit
    }
    /// Conservative logical history charge: metadata and changed payload bytes.
    pub fn history_bytes(&self) -> usize {
        let metadata = self
            .undo
            .iter()
            .chain(&self.redo)
            .fold(0usize, |n, state| n.saturating_add(state.history_charge));
        let payload = self
            .undo
            .iter()
            .skip(1)
            .chain(&self.redo)
            .fold(0usize, |n, state| n.saturating_add(state.history_payload));
        metadata
            .saturating_add(payload)
            .saturating_add(if self.undo.is_empty() {
                0
            } else {
                self.state.history_payload
            })
    }
    pub fn set_history_bytes_limit(&mut self, limit: usize) {
        self.history_bytes_limit = limit;
        self.enforce_history_budget();
        self.reclaim_unused_storage();
    }
    pub fn set_history_limit(&mut self, limit: usize) {
        self.history_limit = limit;
        trim_history(&mut self.undo, limit);
        trim_history(&mut self.redo, limit);
        self.enforce_history_budget();
        self.reclaim_unused_storage();
    }
    fn enforce_history_budget(&mut self) {
        while self.history_bytes() > self.history_bytes_limit {
            if !self.undo.is_empty() {
                self.undo.remove(0);
            } else if !self.redo.is_empty() {
                self.redo.remove(0);
            } else {
                break;
            }
        }
        // Collection is independent of undo semantics and does not discard reachable states.
        let budget = if self.undo.is_empty() && self.redo.is_empty() {
            0
        } else {
            self.history_bytes_limit
        };
        let threshold = self.len().saturating_add(budget).saturating_add(64 * 1024);
        if self.added.len() > threshold || self.original.len() > threshold {
            self.reclaim_unused_storage();
        }
    }
    pub fn retained_source_bytes(&self) -> usize {
        self.original.capacity() + self.added.capacity()
    }

    /// Relocate only reachable source intervals; revisions, history and maps remain unchanged.
    pub fn reclaim_unused_storage(&mut self) {
        type RelocationMap = Vec<(Range<usize>, usize)>;
        fn collect(document: &Document, source: Source) -> (Option<Vec<u8>>, RelocationMap) {
            let mut ranges: Vec<_> = std::iter::once(&document.state)
                .chain(&document.undo)
                .chain(&document.redo)
                .flat_map(|state| &state.pieces)
                .filter(|piece| piece.source == source)
                .map(|p| p.start..p.start + p.len)
                .collect();
            ranges.sort_unstable_by_key(|range| range.start);
            let mut merged: Vec<Range<usize>> = Vec::new();
            for range in ranges {
                if let Some(last) = merged.last_mut().filter(|last| last.end >= range.start) {
                    last.end = last.end.max(range.end);
                } else {
                    merged.push(range);
                }
            }
            let source = match source {
                Source::Original => &document.original,
                Source::Added => &document.added,
            };
            // The entire arena is still reachable: its offsets are already the
            // desired mapping. Reusing it avoids a source-sized temporary just
            // to collect a different arena or change a history budget.
            if merged.len() == 1 && merged[0] == (0..source.len()) {
                return (None, Vec::new());
            }
            let mut bytes = Vec::with_capacity(merged.iter().map(|r| r.len()).sum());
            let mut mapping = Vec::with_capacity(merged.len());
            for range in merged {
                let base = bytes.len();
                bytes.extend_from_slice(&source[range.clone()]);
                mapping.push((range, base));
            }
            (Some(bytes), mapping)
        }
        let (original, original_map) = collect(self, Source::Original);
        let (added, added_map) = collect(self, Source::Added);
        for state in std::iter::once(&mut self.state)
            .chain(&mut self.undo)
            .chain(&mut self.redo)
        {
            for piece in &mut state.pieces {
                let map = match piece.source {
                    Source::Original if original.is_none() => continue,
                    Source::Added if added.is_none() => continue,
                    Source::Original => &original_map,
                    Source::Added => &added_map,
                };
                let index = map.partition_point(|(range, _)| range.start <= piece.start) - 1;
                piece.start = map[index].1 + piece.start - map[index].0.start;
            }
        }
        if let Some(original) = original {
            self.original = original;
        } else {
            self.original.shrink_to_fit();
        }
        if let Some(added) = added {
            self.added = added;
        } else {
            self.added.shrink_to_fit();
        }
    }
    pub fn compact_storage(&mut self) {
        let bytes = self.to_bytes();
        let len = bytes.len();
        self.original = bytes;
        self.added = Vec::new();
        self.state.pieces = if len == 0 {
            Vec::new()
        } else {
            vec![Piece {
                source: Source::Original,
                start: 0,
                len,
            }]
        };
        self.state.ends = PieceIndex::single(len);
        self.state.len = len;
        self.state.edit = None;
        self.state.history_payload = 0;
        self.last_edit = None;
        self.state.history_charge = std::mem::size_of::<State>()
            + self.state.pieces.capacity() * std::mem::size_of::<Piece>()
            + self.state.ends.memory_bytes();
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

    /// Borrow only the requested segments, locating the first piece in O(log P).
    pub fn range_chunks(&self, range: Range<usize>) -> Result<RangeChunks<'_>, EditError> {
        self.validate_range(&range)?;
        let (index, position) = self.state.ends.locate(range.start);
        Ok(RangeChunks {
            document: self,
            pieces: self.state.pieces[index..].iter(),
            position,
            range,
        })
    }

    pub fn read_range(&self, range: Range<usize>) -> Result<Vec<u8>, EditError> {
        let chunks = self.range_chunks(range.clone())?;
        let mut bytes = Vec::with_capacity(range.len());
        for chunk in chunks {
            bytes.extend_from_slice(chunk);
        }
        Ok(bytes)
    }

    /// A revision-consistent borrowed read. Rust prevents edits while it is held.
    pub fn read_snapshot(&self) -> DocumentRead<'_> {
        DocumentRead { document: self }
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
        let ends = PieceIndex::build(&pieces)?;
        for edit in batch.edits {
            self.added.extend_from_slice(&edit.bytes);
        }
        let history_charge = std::mem::size_of::<State>()
            + pieces
                .capacity()
                .saturating_mul(std::mem::size_of::<Piece>())
                .saturating_add(ends.memory_bytes())
                .saturating_add(state_change.edits.capacity() * std::mem::size_of::<ByteEdit>());
        let history_payload = state_change.edits.iter().fold(0usize, |n, e| {
            n.saturating_add(e.replaced.len())
                .saturating_add(e.inserted_len)
        });
        let previous = std::mem::replace(
            &mut self.state,
            State {
                pieces,
                ends,
                history_charge,
                history_payload,
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
        self.enforce_history_budget();
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
        self.enforce_history_budget();
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
        self.enforce_history_budget();
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
        let mut compared = 0;
        for chunk in self
            .range_chunks(range.clone())
            .expect("validated replacement")
        {
            if chunk != &bytes[compared..compared + chunk.len()] {
                return false;
            }
            compared += chunk.len();
        }
        compared == bytes.len()
    }

    fn append_range(&self, range: Range<usize>, pieces: &mut Vec<Piece>) {
        if range.is_empty() {
            return;
        }
        let (index, mut position) = self.state.ends.locate(range.start);
        for piece in &self.state.pieces[index..] {
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

pub struct RangeChunks<'a> {
    document: &'a Document,
    pieces: std::slice::Iter<'a, Piece>,
    position: usize,
    range: Range<usize>,
}
impl<'a> Iterator for RangeChunks<'a> {
    type Item = &'a [u8];
    fn next(&mut self) -> Option<Self::Item> {
        if self.position >= self.range.end {
            return None;
        }
        let piece = self.pieces.next()?;
        let start = self.range.start.saturating_sub(self.position);
        let end = piece.len.min(self.range.end - self.position);
        self.position += piece.len;
        Some(&self.document.piece_bytes(piece)[start..end])
    }
}
pub struct DocumentRead<'a> {
    document: &'a Document,
}
impl DocumentRead<'_> {
    pub fn revision(&self) -> u64 {
        self.document.revision()
    }
    pub fn len(&self) -> usize {
        self.document.len()
    }
    pub fn is_empty(&self) -> bool {
        self.document.is_empty()
    }
    pub fn chunks(&self, range: Range<usize>) -> Result<RangeChunks<'_>, EditError> {
        self.document.range_chunks(range)
    }
}

impl Default for Document {
    fn default() -> Self {
        Self::from_bytes(Vec::new())
    }
}
