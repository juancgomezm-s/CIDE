//! Compact source-byte line metadata. Coordinates remain usize at the boundary.
use std::ops::Range;

const COMPACT_MAX: usize = (u32::MAX >> 2) as usize;

pub(super) fn shift(value: usize, old_len: usize, new_len: usize) -> usize {
    if new_len >= old_len {
        value + (new_len - old_len)
    } else {
        value - (old_len - new_len)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct LineEntry {
    pub start: usize,
    pub ending: u8,
}

/// The common case uses four bytes per line: absolute offset plus its two-bit
/// EOL kind. Larger sources promote without truncation to native-size offsets.
/// This is one index with two storage widths, not two live production indices.
#[derive(Debug)]
pub(super) enum LineIndex {
    Compact(Vec<u32>),
    Wide {
        starts: Vec<usize>,
        endings: Vec<u8>,
    },
}
impl Default for LineIndex {
    fn default() -> Self {
        Self::Compact(Vec::new())
    }
}
impl LineIndex {
    pub fn len(&self) -> usize {
        match self {
            Self::Compact(v) => v.len(),
            Self::Wide { starts, .. } => starts.len(),
        }
    }
    pub fn capacity(&self) -> usize {
        match self {
            Self::Compact(v) => v.capacity(),
            Self::Wide { starts, .. } => starts.capacity(),
        }
    }
    pub fn memory_bytes(&self) -> usize {
        match self {
            Self::Compact(v) => v.capacity() * std::mem::size_of::<u32>(),
            Self::Wide { starts, endings } => {
                starts.capacity() * std::mem::size_of::<usize>() + endings.capacity()
            }
        }
    }
    pub fn get(&self, line: usize) -> Option<LineEntry> {
        match self {
            Self::Compact(v) => v.get(line).map(|&value| LineEntry {
                start: (value >> 2) as usize,
                ending: (value & 3) as u8,
            }),
            Self::Wide { starts, endings } => starts.get(line).map(|&start| LineEntry {
                start,
                ending: endings[line],
            }),
        }
    }
    pub fn start(&self, line: usize) -> usize {
        self.get(line).expect("indexed line").start
    }
    pub fn ending(&self, line: usize) -> u8 {
        self.get(line).expect("indexed line").ending
    }
    pub fn first_start(&self) -> Option<usize> {
        self.get(0).map(|entry| entry.start)
    }
    pub fn entries(&self) -> impl ExactSizeIterator<Item = LineEntry> + '_ {
        (0..self.len()).map(|i| self.get(i).unwrap())
    }
    pub fn upper_bound(&self, offset: usize) -> usize {
        match self {
            Self::Compact(v) => v.partition_point(|&entry| (entry >> 2) as usize <= offset),
            Self::Wide { starts, .. } => starts.partition_point(|&start| start <= offset),
        }
    }
    pub fn widen_for(&mut self, max_offset: usize) {
        if max_offset <= COMPACT_MAX {
            return;
        }
        if let Self::Compact(v) = self {
            let mut starts = Vec::with_capacity(v.len());
            let mut endings = Vec::with_capacity(v.len());
            for &entry in v.iter() {
                starts.push((entry >> 2) as usize);
                endings.push((entry & 3) as u8);
            }
            *self = Self::Wide { starts, endings };
        }
    }
    fn packed(entry: LineEntry) -> u32 {
        assert!(entry.start <= COMPACT_MAX && entry.ending <= 3);
        (u32::try_from(entry.start).expect("compact byte offset") << 2) | u32::from(entry.ending)
    }
    pub fn push(&mut self, start: usize, ending: u8) {
        self.widen_for(start);
        match self {
            Self::Compact(v) => v.push(Self::packed(LineEntry { start, ending })),
            Self::Wide { starts, endings } => {
                starts.push(start);
                endings.push(ending);
            }
        }
    }
    /// The scanner knows this is the last record. Reserve only that record,
    /// avoiding a full geometric expansion for a final empty EOF line.
    pub fn finish(&mut self, start: usize) {
        self.widen_for(start);
        match self {
            Self::Compact(v) => {
                v.reserve_exact(1);
                v.push(Self::packed(LineEntry { start, ending: 0 }));
            }
            Self::Wide { starts, endings } => {
                starts.reserve_exact(1);
                endings.reserve_exact(1);
                starts.push(start);
                endings.push(0);
            }
        }
    }
    fn set_ending(&mut self, line: usize, ending: u8) {
        assert!(ending <= 3);
        match self {
            Self::Compact(v) => v[line] = (v[line] & !3) | u32::from(ending),
            Self::Wide { endings, .. } => endings[line] = ending,
        }
    }
    pub fn shift_tail(&mut self, from: usize, old_len: usize, new_len: usize) {
        self.widen_for(new_len);
        match self {
            Self::Compact(v) => {
                for entry in &mut v[from..] {
                    *entry = Self::packed(LineEntry {
                        start: shift((*entry >> 2) as usize, old_len, new_len),
                        ending: (*entry & 3) as u8,
                    });
                }
            }
            Self::Wide { starts, .. } => {
                for start in &mut starts[from..] {
                    *start = shift(*start, old_len, new_len);
                }
            }
        }
    }
    /// Replace starts after the containing line, retaining the tail's actual EOL.
    /// The scan's first entry begins inside that line; its final EOL is unknown.
    pub fn replace_after(&mut self, range: Range<usize>, mut replacement: Self) {
        assert!(range.start > 0 && replacement.len() > 0);
        let last = replacement.len() - 1;
        replacement.set_ending(last, self.ending(range.end - 1));
        self.set_ending(range.start - 1, replacement.ending(0));
        self.widen_for(replacement.start(last));
        match self {
            Self::Compact(v) => {
                v.splice(range, replacement.entries().skip(1).map(Self::packed));
            }
            Self::Wide { starts, endings } => {
                starts.splice(
                    range.clone(),
                    replacement.entries().skip(1).map(|entry| entry.start),
                );
                endings.splice(
                    range,
                    replacement.entries().skip(1).map(|entry| entry.ending),
                );
            }
        }
    }
    pub fn shrink_to_fit(&mut self) {
        match self {
            Self::Compact(v) => v.shrink_to_fit(),
            Self::Wide { starts, endings } => {
                starts.shrink_to_fit();
                endings.shrink_to_fit();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_offsets_and_endings_survive_width_promotion_and_tail_changes() {
        let mut index = LineIndex::default();
        index.push(3, 2);
        index.push(9, 1);
        index.push(COMPACT_MAX, 3);
        assert_eq!(index.upper_bound(8), 1);
        index.push(COMPACT_MAX + 1, 0);
        assert_eq!(
            index.get(0),
            Some(LineEntry {
                start: 3,
                ending: 2
            })
        );
        assert_eq!(
            index.get(2),
            Some(LineEntry {
                start: COMPACT_MAX,
                ending: 3
            })
        );
        index.shift_tail(2, COMPACT_MAX + 1, COMPACT_MAX + 4);
        assert_eq!(index.start(3), COMPACT_MAX + 4);
        index.shift_tail(2, COMPACT_MAX + 4, COMPACT_MAX + 1);
        assert_eq!(index.start(3), COMPACT_MAX + 1);
        index.push(usize::MAX, 2);
        assert_eq!(
            index.get(4),
            Some(LineEntry {
                start: usize::MAX,
                ending: 2
            })
        );
        assert_eq!(index.upper_bound(usize::MAX), 5);
    }
    #[test]
    fn replacing_line_boundaries_retains_the_continuing_tail_eol() {
        let mut index = LineIndex::default();
        for (start, ending) in [(0, 2), (5, 1), (9, 3), (15, 0)] {
            index.push(start, ending);
        }
        let mut replacement = LineIndex::default();
        replacement.push(2, 1);
        replacement.push(4, 2);
        replacement.push(8, 0);
        index.replace_after(1..3, replacement);
        let actual: Vec<_> = index.entries().map(|e| (e.start, e.ending)).collect();
        assert_eq!(actual, vec![(0, 1), (4, 2), (8, 3), (15, 0)]);
        let mut none = LineIndex::default();
        none.push(1, 0);
        index.replace_after(1..3, none);
        assert_eq!(
            index
                .entries()
                .map(|e| (e.start, e.ending))
                .collect::<Vec<_>>(),
            vec![(0, 3), (15, 0)]
        );
    }
}
