//! Experimental metadata candidate, deliberately outside the editor's modules.
//!
//! A block stores at most 256 two-byte relative-offset/EOL records. Its native
//! base covers the entire usize domain without promoting or retaining a mirror.
//! Dense short lines amortize the catalog/header below four bytes per record;
//! lines separated by more than 16 KiB can require one allocation/header each
//! and be substantially worse than the flat index. Measurements must decide.
use std::mem::size_of;
use std::ops::Range;

const BLOCK_ENTRIES: usize = 256;
const RELATIVE_MAX: usize = (u16::MAX >> 2) as usize;
const CATALOG_SLACK_BYTES: usize = 32 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
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
    fn new(first_line: usize, entry: Entry) -> Self {
        assert!(entry.ending <= 3);
        let records = vec![u16::from(entry.ending)];
        Self {
            base: entry.start,
            first_line,
            max_relative: 0,
            records,
        }
    }

    fn accepts(&self, entry: Entry) -> bool {
        self.records.len() < BLOCK_ENTRIES
            && entry
                .start
                .checked_sub(self.base)
                .is_some_and(|relative| relative <= RELATIVE_MAX)
    }

    fn push(&mut self, entry: Entry, final_record: bool) {
        assert!(entry.ending <= 3 && self.accepts(entry));
        let relative = u16::try_from(entry.start - self.base).expect("relative byte offset");
        self.max_relative = self.max_relative.max(relative);
        // Ordinary growth is bounded by the small block, never the whole index.
        if final_record {
            self.records.reserve_exact(1);
        }
        self.records.push((relative << 2) | u16::from(entry.ending));
    }

    fn entry(&self, slot: usize) -> Entry {
        let record = self.records[slot];
        Entry {
            start: self
                .base
                .checked_add(usize::from(record >> 2))
                .expect("source byte offset overflow"),
            ending: (record & 3) as u8,
        }
    }

    fn shift_base(&mut self, old_len: usize, new_len: usize) {
        let base = shifted(self.base, old_len, new_len);
        base.checked_add(usize::from(self.max_relative))
            .expect("source byte offset overflow");
        self.base = base;
    }
}

fn shifted(value: usize, old_len: usize, new_len: usize) -> usize {
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

fn append(blocks: &mut Vec<Block>, line: usize, entry: Entry, final_record: bool) {
    if let Some(block) = blocks.last_mut().filter(|block| block.accepts(entry)) {
        block.push(entry, final_record);
    } else {
        let needed = blocks.len().checked_add(1).expect("block catalog overflow");
        reserve_catalog(blocks, needed);
        blocks.push(Block::new(line, entry));
    }
}

fn packed_blocks(first_line: usize, entries: impl Iterator<Item = Entry>) -> Vec<Block> {
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
pub struct BlockLineIndex {
    blocks: Vec<Block>,
    len: usize,
}

impl BlockLineIndex {
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

    /// Logical retained capacity including root, all allocated catalog headers
    /// (also spare headers), and record allocations. Not allocator/private RAM
    /// or transient peak: local rebuilds/catalog relocation can coexist briefly.
    pub fn memory_bytes(&self) -> usize {
        size_of::<Self>()
            + self.blocks.capacity() * size_of::<Block>()
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

    pub fn get(&self, line: usize) -> Option<Entry> {
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

    pub fn entries(&self) -> impl ExactSizeIterator<Item = Entry> + '_ {
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

    /// Native bases already represent any usize offset. A distant next record
    /// starts another block instead of truncating or promoting a second format.
    pub fn widen_for(&mut self, _max_offset: usize) {}

    pub fn push(&mut self, start: usize, ending: u8) {
        self.append_entry(Entry { start, ending }, false);
    }

    pub fn finish(&mut self, start: usize) {
        self.append_entry(Entry { start, ending: 0 }, true);
    }

    fn append_entry(&mut self, entry: Entry, final_record: bool) {
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
                    entry.start = shifted(entry.start, old_len, new_len);
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
            block.first_line = shifted(block.first_line, self.len, needed);
        }
        self.len = needed;
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
    type Item = Entry;

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
