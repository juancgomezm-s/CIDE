use crate::DecodedText;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextPosition {
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PositionError {
    SourceOffsetNotBoundary(usize),
    InsideLineEnding(usize),
    LineOutOfRange(usize),
    ColumnOutOfRange { line: usize, column: usize },
}

impl fmt::Display for PositionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceOffsetNotBoundary(offset) => {
                write!(f, "source byte {offset} is not a text boundary")
            }
            Self::InsideLineEnding(offset) => {
                write!(f, "source byte {offset} is inside a line ending")
            }
            Self::LineOutOfRange(line) => write!(f, "line {line} is out of range"),
            Self::ColumnOutOfRange { line, column } => {
                write!(f, "column {column} is out of range for line {line}")
            }
        }
    }
}

impl std::error::Error for PositionError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextIndex {
    source_offsets: Vec<usize>,
    line_starts: Vec<usize>,
    line_ends: Vec<usize>,
}

impl TextIndex {
    pub fn new(decoded: &DecodedText) -> Self {
        let chars: Vec<char> = decoded.text.chars().collect();
        let mut line_starts = vec![0usize];
        let mut line_ends = Vec::new();
        let mut i = 0usize;
        while i < chars.len() {
            if chars[i] == '\r' {
                line_ends.push(i);
                if i + 1 < chars.len() && chars[i + 1] == '\n' {
                    i += 2;
                } else {
                    i += 1;
                }
                line_starts.push(i);
            } else if chars[i] == '\n' {
                line_ends.push(i);
                i += 1;
                line_starts.push(i);
            } else {
                i += 1;
            }
        }
        line_ends.push(chars.len());
        Self {
            source_offsets: decoded.source_offsets.clone(),
            line_starts,
            line_ends,
        }
    }

    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    pub fn line_len(&self, line: usize) -> Option<usize> {
        let start = *self.line_starts.get(line)?;
        let end = *self.line_ends.get(line)?;
        Some(end - start)
    }

    pub fn source_byte_to_position(&self, offset: usize) -> Result<TextPosition, PositionError> {
        let scalar = self
            .source_offsets
            .binary_search(&offset)
            .map_err(|_| PositionError::SourceOffsetNotBoundary(offset))?;
        let line = self
            .line_starts
            .partition_point(|start| *start <= scalar)
            .saturating_sub(1);
        if scalar > self.line_ends[line] {
            return Err(PositionError::InsideLineEnding(offset));
        }
        Ok(TextPosition {
            line,
            column: scalar - self.line_starts[line],
        })
    }

    pub fn position_to_source_byte(&self, position: TextPosition) -> Result<usize, PositionError> {
        let Some(start) = self.line_starts.get(position.line).copied() else {
            return Err(PositionError::LineOutOfRange(position.line));
        };
        let end = self.line_ends[position.line];
        let scalar = start
            .checked_add(position.column)
            .filter(|scalar| *scalar <= end)
            .ok_or(PositionError::ColumnOutOfRange {
                line: position.line,
                column: position.column,
            })?;
        self.source_offsets
            .get(scalar)
            .copied()
            .ok_or(PositionError::ColumnOutOfRange {
                line: position.line,
                column: position.column,
            })
    }
}
