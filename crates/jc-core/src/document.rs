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
}

pub struct Document {
    original: Vec<u8>,
    added: Vec<u8>,
    state: State,
    undo: Vec<State>,
    redo: Vec<State>,
    saved_revision: u64,
    next_revision: u64,
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
            },
            undo: Vec::new(),
            redo: Vec::new(),
            saved_revision: 0,
            next_revision: 1,
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
    pub fn is_modified(&self) -> bool {
        self.state.revision != self.saved_revision
    }
    pub fn mark_saved(&mut self) {
        self.saved_revision = self.state.revision;
    }
    pub fn piece_count(&self) -> usize {
        self.state.pieces.len()
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
        self.validate_range(&range)?;
        if self.range_matches(&range, bytes) {
            return Ok(());
        }

        let new_len = (self.len() - (range.end - range.start))
            .checked_add(bytes.len())
            .ok_or(EditError::CapacityOverflow)?;
        self.added
            .len()
            .checked_add(bytes.len())
            .ok_or(EditError::CapacityOverflow)?;
        let following_revision = self
            .next_revision
            .checked_add(1)
            .ok_or(EditError::CapacityOverflow)?;

        let mut pieces = Vec::new();
        self.append_range(0..range.start, &mut pieces);
        if !bytes.is_empty() {
            push_coalesced(
                &mut pieces,
                Piece {
                    source: Source::Added,
                    start: self.added.len(),
                    len: bytes.len(),
                },
            );
        }
        self.append_range(range.end..self.len(), &mut pieces);

        self.added.extend_from_slice(bytes);
        let previous = std::mem::replace(
            &mut self.state,
            State {
                pieces,
                len: new_len,
                revision: self.next_revision,
            },
        );
        self.next_revision = following_revision;
        self.undo.push(previous);
        self.redo.clear();
        Ok(())
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.undo.pop() else {
            return false;
        };
        let current = std::mem::replace(&mut self.state, previous);
        self.redo.push(current);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };
        let current = std::mem::replace(&mut self.state, next);
        self.undo.push(current);
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
