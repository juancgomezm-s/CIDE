//! Compact source-byte line metadata. Coordinates remain usize at the boundary.
use std::ops::Range;

const COMPACT_MAX: usize = (u32::MAX >> 2) as usize;
const MAX_GROWTH_SLACK_BYTES: usize = 32 * 1024;

fn replacement_capacity(needed: usize, entry_bytes: usize, element_bytes: usize) -> usize {
    // Small bursts get reusable room without doubling a dense index. The cap
    // charges both arrays together for Wide, not 32 KiB for each allocation.
    let slack = needed
        .div_ceil(32)
        .max(16)
        .min(MAX_GROWTH_SLACK_BYTES / entry_bytes);
    let available = (isize::MAX as usize / element_bytes)
        .checked_sub(needed)
        .expect("line metadata capacity overflow");
    needed
        .checked_add(slack.min(available))
        .expect("line metadata capacity overflow")
}

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
        assert!(range.start > 0 && range.start <= range.end && range.end <= self.len());
        assert!(replacement.len() > 0);
        let last = replacement.len() - 1;
        let needed = self
            .len()
            .checked_sub(range.len())
            .and_then(|len| len.checked_add(last))
            .expect("line metadata length overflow");
        replacement.set_ending(last, self.ending(range.end - 1));
        self.set_ending(range.start - 1, replacement.ending(0));
        self.widen_for(replacement.start(last));
        match self {
            Self::Compact(v) => {
                if needed > v.capacity() {
                    let target = replacement_capacity(
                        needed,
                        std::mem::size_of::<u32>(),
                        std::mem::size_of::<u32>(),
                    );
                    v.reserve_exact(target - v.len());
                }
                v.splice(range, replacement.entries().skip(1).map(Self::packed));
            }
            Self::Wide { starts, endings } => {
                if needed > starts.capacity() || needed > endings.capacity() {
                    let target = replacement_capacity(
                        needed,
                        std::mem::size_of::<usize>() + std::mem::size_of::<u8>(),
                        std::mem::size_of::<usize>(),
                    );
                    if target > starts.capacity() {
                        starts.reserve_exact(target - starts.len());
                    }
                    if target > endings.capacity() {
                        endings.reserve_exact(target - endings.len());
                    }
                }
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

    fn indexed_lines(count: usize, origin: usize) -> LineIndex {
        let mut index = LineIndex::default();
        for line in 0..count {
            let ending = if line + 1 == count {
                0
            } else {
                (line % 3 + 1) as u8
            };
            index.push(origin + line * 8, ending);
        }
        index.shrink_to_fit();
        index
    }

    #[test]
    fn adding_one_boundary_does_not_double_a_dense_index_and_inverse_keeps_eol_offsets() {
        for (count, origin, entry_bytes) in [
            (300_000, 0, std::mem::size_of::<u32>()),
            (120_000, COMPACT_MAX + 1, std::mem::size_of::<usize>() + 1),
        ] {
            let mut index = indexed_lines(count, origin);
            let before_bytes = index.memory_bytes();
            let old_end = index.start(count - 1);
            index.shift_tail(3, old_end, old_end + 3);
            let mut insert = LineIndex::default();
            insert.push(origin + 10, 1);
            insert.push(origin + 14, 2);
            insert.push(origin + 20, 0);
            index.replace_after(2..3, insert);
            assert_eq!(index.len(), count + 1);
            assert!(index.memory_bytes() <= before_bytes + entry_bytes + MAX_GROWTH_SLACK_BYTES);
            assert_eq!(
                index.get(1),
                Some(LineEntry {
                    start: origin + 8,
                    ending: 1
                })
            );
            assert_eq!(
                index.get(2),
                Some(LineEntry {
                    start: origin + 14,
                    ending: 2
                })
            );
            assert_eq!(
                index.get(3),
                Some(LineEntry {
                    start: origin + 20,
                    ending: 3
                })
            );
            assert_eq!(
                index.get(4),
                Some(LineEntry {
                    start: origin + 27,
                    ending: 1
                })
            );
            assert_eq!(index.start(count), old_end + 3);

            index.shift_tail(4, old_end + 3, old_end);
            let mut inverse = LineIndex::default();
            inverse.push(origin + 10, 2);
            inverse.push(origin + 16, 0);
            index.replace_after(2..4, inverse);
            assert_eq!(index.len(), count);
            assert!(index.memory_bytes() <= before_bytes + entry_bytes + MAX_GROWTH_SLACK_BYTES);
            for line in 0..count {
                let ending = if line + 1 == count {
                    0
                } else {
                    (line % 3 + 1) as u8
                };
                assert_eq!(
                    index.get(line),
                    Some(LineEntry {
                        start: origin + line * 8,
                        ending
                    })
                );
            }
        }
    }

    #[test]
    fn newline_bursts_reuse_growth_room_in_compact_and_wide_indices() {
        for (origin, entry_bytes) in [
            (0, std::mem::size_of::<u32>()),
            (COMPACT_MAX + 1, std::mem::size_of::<usize>() + 1),
        ] {
            let base = 1600;
            let burst = 1000;
            let mut index = indexed_lines(base, origin);
            let old_end = index.start(base - 1);
            let mut previous_bytes = index.memory_bytes();
            let mut growths = 0;
            for added in 1..=burst {
                let mut append = LineIndex::default();
                append.push(old_end + added - 1, 1);
                append.push(old_end + added, 0);
                let end = index.len();
                index.replace_after(end..end, append);
                let bytes = index.memory_bytes();
                growths += usize::from(bytes != previous_bytes);
                previous_bytes = bytes;
                assert!(bytes <= index.len() * entry_bytes + MAX_GROWTH_SLACK_BYTES);
            }
            assert!(
                growths < burst / 4,
                "growth must be amortized across a burst"
            );
            for line in 0..base - 1 {
                assert_eq!(
                    index.get(line),
                    Some(LineEntry {
                        start: origin + line * 8,
                        ending: (line % 3 + 1) as u8
                    })
                );
            }
            assert_eq!(index.ending(base - 1), 1);
            for added in 1..=burst {
                assert_eq!(
                    index.get(base + added - 1),
                    Some(LineEntry {
                        start: old_end + added,
                        ending: if added == burst { 0 } else { 1 },
                    })
                );
            }
            assert_eq!(index.upper_bound(old_end + burst), base + burst);
        }
    }

    #[test]
    fn replacement_growth_can_promote_without_truncating_byte_offsets_or_endings() {
        let mut index = LineIndex::default();
        index.push(0, 2);
        index.push(COMPACT_MAX, 0);
        index.shrink_to_fit();
        let mut append = LineIndex::default();
        append.push(COMPACT_MAX, 3);
        append.push(usize::MAX, 0);
        index.replace_after(2..2, append);
        assert!(matches!(index, LineIndex::Wide { .. }));
        assert_eq!(
            index
                .entries()
                .map(|entry| (entry.start, entry.ending))
                .collect::<Vec<_>>(),
            vec![(0, 2), (COMPACT_MAX, 3), (usize::MAX, 0)]
        );
        assert_eq!(index.upper_bound(usize::MAX), 3);
    }

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
