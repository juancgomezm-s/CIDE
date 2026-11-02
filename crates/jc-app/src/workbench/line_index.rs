//! Blocked source-byte line metadata. Coordinates remain usize at the boundary.
//!
//! A block stores at most 256 two-byte relative-offset/EOL records. Its native
//! base covers the entire usize domain without promoting or retaining a mirror.
//! Dense short lines amortize the catalog/header below four bytes per record;
//! lines separated by more than 16 KiB can require one allocation/header each
//! and cost more than a flat index; dense and pathological costs are measured separately.
use std::mem::size_of;
use std::ops::Range;

const BLOCK_ENTRIES: usize = 256;
const RELATIVE_MAX: usize = (u16::MAX >> 2) as usize;
const CATALOG_SLACK_BYTES: usize = 32 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct LineEntry {
    pub start: usize,
    pub ending: u8,
}

#[derive(Debug)]
struct Block {
    base: usize,
    first_line: usize,
    // Allows checking a whole-base shift for overflow without scanning records.
    max_relative: u16,
    records: Vec<u16>,
}

impl Block {
    fn new(first_line: usize, entry: LineEntry) -> Self {
        assert!(entry.ending <= 3);
        let records = vec![u16::from(entry.ending)];
        Self {
            base: entry.start,
            first_line,
            max_relative: 0,
            records,
        }
    }

    fn accepts(&self, entry: LineEntry) -> bool {
        self.records.len() < BLOCK_ENTRIES
            && entry
                .start
                .checked_sub(self.base)
                .is_some_and(|relative| relative <= RELATIVE_MAX)
    }

    fn push(&mut self, entry: LineEntry, final_record: bool) {
        assert!(entry.ending <= 3 && self.accepts(entry));
        let relative = u16::try_from(entry.start - self.base).expect("relative byte offset");
        self.max_relative = self.max_relative.max(relative);
        // Ordinary growth is bounded by the small block, never the whole index.
        if final_record {
            self.records.reserve_exact(1);
        }
        self.records.push((relative << 2) | u16::from(entry.ending));
    }

    fn entry(&self, slot: usize) -> LineEntry {
        let record = self.records[slot];
        LineEntry {
            start: self
                .base
                .checked_add(usize::from(record >> 2))
                .expect("source byte offset overflow"),
            ending: (record & 3) as u8,
        }
    }

    fn shift_base(&mut self, old_len: usize, new_len: usize) {
        let base = shift(self.base, old_len, new_len);
        base.checked_add(usize::from(self.max_relative))
            .expect("source byte offset overflow");
        self.base = base;
    }
}

pub(super) fn shift(value: usize, old_len: usize, new_len: usize) -> usize {
    if new_len >= old_len {
        value.checked_add(new_len - old_len)
    } else {
        value.checked_sub(old_len - new_len)
    }
    .expect("source byte offset overflow")
}

fn reserve_catalog(blocks: &mut Vec<Block>, needed: usize) {
    if needed <= blocks.capacity() {
        return;
    }
    // Proportional burst room, capped in logical header bytes, avoids doubling
    // every existing header when a single newline creates one more block.
    let available = (isize::MAX as usize / size_of::<Block>())
        .checked_sub(needed)
        .expect("block catalog capacity overflow");
    let slack = needed
        .div_ceil(32)
        .max(4)
        .min(CATALOG_SLACK_BYTES / size_of::<Block>())
        .min(available);
    let target = needed.checked_add(slack).expect("block catalog overflow");
    blocks.reserve_exact(target - blocks.len());
}

fn append(blocks: &mut Vec<Block>, line: usize, entry: LineEntry, final_record: bool) {
    if let Some(block) = blocks.last_mut().filter(|block| block.accepts(entry)) {
        block.push(entry, final_record);
    } else {
        let needed = blocks.len().checked_add(1).expect("block catalog overflow");
        reserve_catalog(blocks, needed);
        blocks.push(Block::new(line, entry));
    }
}

fn packed_blocks(first_line: usize, entries: impl Iterator<Item = LineEntry>) -> Vec<Block> {
    let mut blocks = Vec::new();
    for (offset, entry) in entries.enumerate() {
        let line = first_line
            .checked_add(offset)
            .expect("line ordinal overflow");
        append(&mut blocks, line, entry, false);
    }
    blocks
}

/// A single persistent catalog. Temporary boundary rebuilding touches only the
/// affected blocks and the replacement, not the surviving suffix's records.
#[derive(Debug, Default)]
pub(super) struct LineIndex {
    blocks: Vec<Block>,
    len: usize,
}

impl LineIndex {
    pub fn len(&self) -> usize {
        self.len
    }

    /// Allocated record slots; span restrictions can prevent reusing some slots.
    pub fn capacity(&self) -> usize {
        self.blocks
            .iter()
            .map(|block| block.records.capacity())
            .sum()
    }

    /// Logical retained heap capacity including all allocated catalog headers
    /// (also spare headers), and record allocations. Not allocator/private RAM
    /// or transient peak: local rebuilds/catalog relocation can coexist briefly.
    /// The inline root is excluded, matching TextProjection statistics.
    pub fn memory_bytes(&self) -> usize {
        self.blocks.capacity() * size_of::<Block>()
            + self
                .blocks
                .iter()
                .map(|block| block.records.capacity() * size_of::<u16>())
                .sum::<usize>()
    }

    fn block_for_line(&self, line: usize) -> usize {
        assert!(line < self.len);
        self.blocks
            .partition_point(|block| block.first_line <= line)
            - 1
    }

    pub fn get(&self, line: usize) -> Option<LineEntry> {
        if line >= self.len {
            return None;
        }
        let block = &self.blocks[self.block_for_line(line)];
        Some(block.entry(line - block.first_line))
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
        self.entries_range(0..self.len)
    }

    fn entries_range(&self, range: Range<usize>) -> Entries<'_> {
        assert!(range.start <= range.end && range.end <= self.len);
        let remaining = range.len();
        let block = if remaining == 0 {
            self.blocks.len()
        } else {
            self.block_for_line(range.start)
        };
        let slot = if remaining == 0 {
            0
        } else {
            range.start - self.blocks[block].first_line
        };
        Entries {
            blocks: &self.blocks,
            block,
            slot,
            remaining,
        }
    }

    pub fn upper_bound(&self, offset: usize) -> usize {
        let after = self.blocks.partition_point(|block| block.base <= offset);
        if after == 0 {
            return 0;
        }
        let block = &self.blocks[after - 1];
        let relative = offset - block.base;
        block.first_line
            + block
                .records
                .partition_point(|record| usize::from(record >> 2) <= relative)
    }

    pub fn push(&mut self, start: usize, ending: u8) {
        self.append_entry(LineEntry { start, ending }, false);
    }

    pub fn finish(&mut self, start: usize) {
        self.append_entry(LineEntry { start, ending: 0 }, true);
    }

    fn append_entry(&mut self, entry: LineEntry, final_record: bool) {
        let needed = self.len.checked_add(1).expect("line ordinal overflow");
        append(&mut self.blocks, self.len, entry, final_record);
        self.len = needed;
    }

    fn set_ending(&mut self, line: usize, ending: u8) {
        assert!(ending <= 3);
        let block = self.block_for_line(line);
        let slot = line - self.blocks[block].first_line;
        let record = &mut self.blocks[block].records[slot];
        *record = (*record & !3) | u16::from(ending);
    }

    pub fn shift_tail(&mut self, from: usize, old_len: usize, new_len: usize) {
        assert!(from <= self.len);
        if from == self.len || old_len == new_len {
            return;
        }
        let boundary = self.block_for_line(from);
        let first_line = self.blocks[boundary].first_line;
        if from == first_line {
            for block in &mut self.blocks[boundary..] {
                block.shift_base(old_len, new_len);
            }
            return;
        }
        let block = &self.blocks[boundary];
        let rebuilt = packed_blocks(
            first_line,
            (0..block.records.len()).map(|slot| {
                let mut entry = block.entry(slot);
                if first_line + slot >= from {
                    entry.start = shift(entry.start, old_len, new_len);
                }
                entry
            }),
        );
        // Large deletions may temporarily put shifted starts before unremoved
        // starts. Packing splits a negative relative span; ordinal lookup stays
        // valid until replace_after removes those starts. As in the flat index,
        // byte-order queries require the completed edit, not this intermediate.
        for block in &mut self.blocks[boundary + 1..] {
            block.shift_base(old_len, new_len);
        }
        let needed = (self.blocks.len() - 1)
            .checked_add(rebuilt.len())
            .expect("block catalog overflow");
        reserve_catalog(&mut self.blocks, needed);
        self.blocks.splice(boundary..boundary + 1, rebuilt);
    }

    /// Same containing-line/tail-EOL contract as the production flat index.
    pub fn replace_after(&mut self, range: Range<usize>, mut replacement: Self) {
        assert!(range.start > 0 && range.start <= range.end && range.end <= self.len);
        assert!(replacement.len > 0);
        let added = replacement.len - 1;
        let needed = self
            .len
            .checked_sub(range.len())
            .and_then(|len| len.checked_add(added))
            .expect("line ordinal overflow");
        replacement.set_ending(added, self.ending(range.end - 1));
        self.set_ending(range.start - 1, replacement.ending(0));
        if range.is_empty() && added == 0 {
            return;
        }

        // Include immediate neighbors so repeated boundary edits merge spare
        // slots rather than retaining a new tiny block for every keystroke.
        // At most four surviving boundary blocks are decoded into metadata;
        // wholly removed blocks and replacement records scale with the edit.
        let first = range.start.min(self.len - 1);
        let last = if range.is_empty() {
            first
        } else {
            range.end - 1
        };
        let begin = self.block_for_line(first).saturating_sub(1);
        let end = self
            .block_for_line(last)
            .saturating_add(2)
            .min(self.blocks.len());
        let first_line = self.blocks[begin].first_line;
        let last_block = &self.blocks[end - 1];
        let end_line = last_block.first_line + last_block.records.len();
        let rebuilt = packed_blocks(
            first_line,
            self.entries_range(first_line..range.start)
                .chain(replacement.entries().skip(1))
                .chain(self.entries_range(range.end..end_line)),
        );
        let rebuilt_len = rebuilt.len();
        let catalog_len = self
            .blocks
            .len()
            .checked_sub(end - begin)
            .and_then(|len| len.checked_add(rebuilt_len))
            .expect("block catalog overflow");
        reserve_catalog(&mut self.blocks, catalog_len);
        self.blocks.splice(begin..end, rebuilt);
        // Moving headers and adjusting ordinals remains O(number of blocks),
        // but no surviving suffix payload is visited or decoded here.
        for block in &mut self.blocks[begin + rebuilt_len..] {
            block.first_line = shift(block.first_line, self.len, needed);
        }
        self.len = needed;
        self.release_excess_catalog();
    }

    fn release_excess_catalog(&mut self) {
        // Payload capacity does not include spare catalog headers. A large
        // deletion can therefore evade the projection's record-slot heuristic.
        // Require both a substantial byte surplus and a large ratio, so normal
        // edits keep burst room and never shrink on each removed boundary.
        let spare = self.blocks.capacity() - self.blocks.len();
        if spare > CATALOG_SLACK_BYTES / size_of::<Block>()
            && self.blocks.capacity() > self.blocks.len().saturating_mul(2)
        {
            self.blocks.shrink_to_fit();
        }
    }

    pub fn shrink_to_fit(&mut self) {
        for block in &mut self.blocks {
            block.records.shrink_to_fit();
        }
        self.blocks.shrink_to_fit();
    }
}

struct Entries<'a> {
    blocks: &'a [Block],
    block: usize,
    slot: usize,
    remaining: usize,
}

impl Iterator for Entries<'_> {
    type Item = LineEntry;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let block = &self.blocks[self.block];
        let entry = block.entry(self.slot);
        self.remaining -= 1;
        self.slot += 1;
        if self.slot == block.records.len() {
            self.block += 1;
            self.slot = 0;
        }
        Some(entry)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl ExactSizeIterator for Entries<'_> {}
#[cfg(test)]
mod tests {
    use super::*;

    const LARGE_OFFSET: usize = (u32::MAX >> 2) as usize;

    fn assert_metadata(index: &LineIndex, expected: &[LineEntry]) {
        assert_eq!(index.len(), expected.len());
        assert_eq!(index.entries().collect::<Vec<_>>(), expected);
        assert_eq!(index.get(expected.len()), None);
        assert_eq!(
            index.first_start(),
            expected.first().map(|entry| entry.start)
        );
        assert_eq!(index.upper_bound(usize::MAX), expected.len());
        for (line, entry) in expected.iter().enumerate() {
            assert_eq!(index.get(line), Some(*entry));
            assert_eq!(index.upper_bound(entry.start), line + 1);
            if entry.start > 0 {
                assert_eq!(index.upper_bound(entry.start - 1), line);
            }
        }
    }

    // A separate coordinate oracle rebuilds expected metadata from byte widths,
    // not from the index's packing, catalog, or partial-block update algorithm.
    fn replace_span(
        index: &mut LineIndex,
        expected: &mut Vec<LineEntry>,
        first: usize,
        removed: usize,
        segments: &[(usize, u8)],
    ) {
        let last = first + removed;
        let start = expected[first].start;
        let removed_bytes = expected[last].start - start;
        let inserted_bytes: usize = segments.iter().map(|&(width, _)| width).sum();
        let old_end = expected.last().unwrap().start;
        let new_end = old_end - removed_bytes + inserted_bytes;
        let mut scan = LineIndex::default();
        let mut cursor = start;
        let mut next = expected[..first].to_vec();
        for &(width, ending) in segments {
            scan.push(cursor, ending);
            next.push(LineEntry {
                start: cursor,
                ending,
            });
            cursor += width;
        }
        scan.finish(cursor);
        next.push(LineEntry {
            start: cursor,
            ending: expected[last].ending,
        });
        next.extend(expected[last + 1..].iter().map(|entry| LineEntry {
            start: entry.start - removed_bytes + inserted_bytes,
            ending: entry.ending,
        }));
        index.shift_tail(last + 1, old_end, new_end);
        index.replace_after(first + 1..last + 1, scan);
        *expected = next;
        assert_eq!(index.len(), expected.len());
        assert_eq!(index.entries().collect::<Vec<_>>(), *expected);
    }

    #[test]
    fn multiblock_replacements_deletions_and_inverse_match_coordinate_oracle() {
        for origin in [3, LARGE_OFFSET + 1, usize::MAX - 2_000_000] {
            let mut index = indexed_lines(8193, origin);
            let mut expected: Vec<_> = index.entries().collect();
            let original = expected.clone();
            let mut inverses = Vec::new();
            let mut state = 0x6a11_ef41_u64;
            for step in 0..32 {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                let first = (state as usize % ((expected.len() - 1024) / 256)) * 256 + 243;
                let (removed, inserted) = match step % 4 {
                    0 => (0, 600),
                    1 => (600, 700),
                    2 => (512, 0),
                    _ => (900, 650),
                };
                let old: Vec<_> = (first..first + removed)
                    .map(|line| {
                        (
                            expected[line + 1].start - expected[line].start,
                            expected[line].ending,
                        )
                    })
                    .collect();
                let segments: Vec<_> = (0..inserted)
                    .map(|line| (4 + (line + step) % 13, ((line + step) % 3 + 1) as u8))
                    .collect();
                replace_span(&mut index, &mut expected, first, removed, &segments);
                assert_metadata(&index, &expected);
                inverses.push((first, inserted, old));
            }
            for (first, inserted, old) in inverses.into_iter().rev() {
                replace_span(&mut index, &mut expected, first, inserted, &old);
                assert_metadata(&index, &expected);
            }
            assert_eq!(expected, original);
            index.shrink_to_fit();
            assert_metadata(&index, &original);
        }
    }

    #[test]
    fn dispersed_newline_roundtrips_do_not_accumulate_an_allocation_per_edit() {
        let mut index = indexed_lines(8193, 3);
        let original: Vec<_> = index.entries().collect();
        let mut expected = original.clone();
        let mut state = 0x32f7_1dc9_u64;
        for round in 0..512 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let first = match round {
                0 => 0,
                1 => expected.len() - 1,
                _ => state as usize % expected.len(),
            };
            replace_span(&mut index, &mut expected, first, 0, &[(2, 1)]);
            replace_span(&mut index, &mut expected, first, 1, &[]);
        }
        assert_eq!(expected, original);
        assert_metadata(&index, &original);
        assert!(index.memory_bytes() <= original.len() * 4 + CATALOG_SLACK_BYTES);
    }

    #[test]
    fn large_deletion_releases_idle_catalog_and_can_regrow_with_correct_geometry() {
        for remaining in [1, 2] {
            let mut index = indexed_lines(400_001, 3);
            let mut expected: Vec<_> = index.entries().collect();
            let removed = expected.len() - remaining;
            replace_span(&mut index, &mut expected, 0, removed, &[]);
            assert_eq!(index.len(), remaining);
            assert_metadata(&index, &expected);
            let live_budget = remaining * (size_of::<Block>() + size_of::<u16>());
            assert!(
                index.memory_bytes() <= CATALOG_SLACK_BYTES + live_budget,
                "a mostly deleted index must not retain the old header catalog"
            );

            let segments: Vec<_> = (0..4096).map(|line| (8, (line % 3 + 1) as u8)).collect();
            replace_span(&mut index, &mut expected, 0, 0, &segments);
            assert_eq!(index.len(), remaining + segments.len());
            assert_metadata(&index, &expected);
            for line in 0..index.len() {
                assert_eq!(index.start(line), 3 + line * 8);
            }
        }
    }

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
            (120_000, LARGE_OFFSET + 1, std::mem::size_of::<u32>()),
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
            assert!(index.memory_bytes() <= before_bytes + entry_bytes + CATALOG_SLACK_BYTES);
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
            assert!(index.memory_bytes() <= before_bytes + entry_bytes + CATALOG_SLACK_BYTES);
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
    fn newline_bursts_retain_bounded_metadata_at_small_and_large_offsets() {
        for (origin, entry_bytes) in [
            (0, std::mem::size_of::<u32>()),
            (LARGE_OFFSET + 1, std::mem::size_of::<u32>()),
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
                assert!(bytes <= index.len() * entry_bytes + CATALOG_SLACK_BYTES);
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
    fn replacement_growth_preserves_full_width_offsets_and_endings() {
        let mut index = LineIndex::default();
        index.push(0, 2);
        index.push(LARGE_OFFSET, 0);
        index.shrink_to_fit();
        let mut append = LineIndex::default();
        append.push(LARGE_OFFSET, 3);
        append.push(usize::MAX, 0);
        index.replace_after(2..2, append);
        assert_eq!(
            index
                .entries()
                .map(|entry| (entry.start, entry.ending))
                .collect::<Vec<_>>(),
            vec![(0, 2), (LARGE_OFFSET, 3), (usize::MAX, 0)]
        );
        assert_eq!(index.upper_bound(usize::MAX), 3);
    }

    #[test]
    fn source_offsets_and_endings_survive_distant_blocks_and_tail_changes() {
        let mut index = LineIndex::default();
        index.push(3, 2);
        index.push(9, 1);
        index.push(LARGE_OFFSET, 3);
        assert_eq!(index.upper_bound(8), 1);
        index.push(LARGE_OFFSET + 1, 0);
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
                start: LARGE_OFFSET,
                ending: 3
            })
        );
        index.shift_tail(2, LARGE_OFFSET + 1, LARGE_OFFSET + 4);
        assert_eq!(index.start(3), LARGE_OFFSET + 4);
        index.shift_tail(2, LARGE_OFFSET + 4, LARGE_OFFSET + 1);
        assert_eq!(index.start(3), LARGE_OFFSET + 1);
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
