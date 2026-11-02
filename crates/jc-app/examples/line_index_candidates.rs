//! Opt-in metadata comparison; no editor integration, decoded source or RAM proxy.
#[path = "support/block_line_index.rs"]
mod blocked;
#[path = "../src/workbench/line_index.rs"]
mod flat;

use std::{hint::black_box, ops::Range, time::Instant};

const SAMPLES: usize = 31;
const WARMUPS: usize = 5;
const QUERY_BATCH: usize = 4096;
const ROUNDTRIPS: usize = 512;
type Entry = (usize, u8);

/// Adapt module-private types without publishing another production API.
trait Index: Default {
    const NAME: &'static str;
    fn len(&self) -> usize;
    fn capacity(&self) -> usize;
    fn logical_bytes(&self) -> usize;
    fn get(&self, line: usize) -> Option<Entry>;
    fn entries(&self) -> Vec<Entry>;
    fn first_start(&self) -> Option<usize>;
    fn start(&self, line: usize) -> usize;
    fn ending(&self, line: usize) -> u8;
    fn upper_bound(&self, offset: usize) -> usize;
    fn push(&mut self, start: usize, ending: u8);
    fn finish(&mut self, start: usize);
    fn widen_for(&mut self, offset: usize);
    fn shift_tail(&mut self, from: usize, old_len: usize, new_len: usize);
    fn replace_after(&mut self, range: Range<usize>, replacement: Self);
    fn shrink_to_fit(&mut self);
}

macro_rules! adapt {
    ($ty:ty, $name:literal, $root:expr) => {
        impl Index for $ty {
            const NAME: &'static str = $name;
            fn len(&self) -> usize {
                self.len()
            }
            fn capacity(&self) -> usize {
                self.capacity()
            }
            fn logical_bytes(&self) -> usize {
                self.memory_bytes() + $root
            }
            fn get(&self, line: usize) -> Option<Entry> {
                self.get(line).map(|e| (e.start, e.ending))
            }
            fn entries(&self) -> Vec<Entry> {
                self.entries().map(|e| (e.start, e.ending)).collect()
            }
            fn first_start(&self) -> Option<usize> {
                self.first_start()
            }
            fn start(&self, line: usize) -> usize {
                self.start(line)
            }
            fn ending(&self, line: usize) -> u8 {
                self.ending(line)
            }
            fn upper_bound(&self, offset: usize) -> usize {
                self.upper_bound(offset)
            }
            fn push(&mut self, start: usize, ending: u8) {
                self.push(start, ending);
            }
            fn finish(&mut self, start: usize) {
                self.finish(start);
            }
            fn widen_for(&mut self, offset: usize) {
                self.widen_for(offset);
            }
            fn shift_tail(&mut self, from: usize, old_len: usize, new_len: usize) {
                self.shift_tail(from, old_len, new_len);
            }
            fn replace_after(&mut self, range: Range<usize>, replacement: Self) {
                self.replace_after(range, replacement);
            }
            fn shrink_to_fit(&mut self) {
                self.shrink_to_fit();
            }
        }
    };
}
// Flat reports allocated arrays only; block reports root + catalogue + arrays.
adapt!(
    flat::LineIndex,
    "flat",
    std::mem::size_of::<flat::LineIndex>()
);
adapt!(blocked::BlockLineIndex, "block", 0);

fn build<I: Index>(entries: &[Entry]) -> I {
    let mut index = I::default();
    let Some((&(last_start, last_ending), preceding)) = entries.split_last() else {
        return index;
    };
    for &(start, ending) in preceding {
        index.push(start, ending);
    }
    if last_ending == 0 {
        index.finish(last_start);
    } else {
        index.push(last_start, last_ending);
    }
    index
}

fn replacement<I: Index>(entries: &[Entry]) -> I {
    assert!(!entries.is_empty());
    let mut index = I::default();
    for &(start, ending) in &entries[..entries.len() - 1] {
        index.push(start, ending);
    }
    assert_eq!(entries.last().unwrap().1, 0);
    index.finish(entries.last().unwrap().0);
    index
}

/// Reference state consists only of explicit metadata, never an index clone.
#[derive(Clone)]
struct Oracle {
    entries: Vec<Entry>,
    end: usize,
}
impl Oracle {
    fn regular(bytes: usize, stride: usize) -> Self {
        assert_eq!(bytes % stride, 0);
        let mut entries = Vec::with_capacity(bytes / stride + 1);
        entries.extend((0..bytes / stride).map(|i| (i * stride, 1)));
        entries.push((bytes, 0));
        Self {
            entries,
            end: bytes,
        }
    }
    fn upper_bound(&self, offset: usize) -> usize {
        self.entries.partition_point(|&(start, _)| start <= offset)
    }
    fn scalar(&mut self, line: usize, inverse: bool) {
        for entry in &mut self.entries[line + 1..] {
            entry.0 = if inverse { entry.0 - 1 } else { entry.0 + 1 };
        }
        self.end = if inverse { self.end - 1 } else { self.end + 1 };
    }
    fn newline(&mut self, line: usize, inverse: bool) {
        if inverse {
            let (_, ending) = self.entries.remove(line + 1);
            self.entries[line].1 = ending;
            for entry in &mut self.entries[line + 1..] {
                entry.0 -= 2;
            }
            self.end -= 2;
        } else {
            let (start, ending) = self.entries[line];
            for entry in &mut self.entries[line + 1..] {
                entry.0 += 2;
            }
            self.entries[line].1 = 1;
            self.entries.insert(line + 1, (start + 2, ending));
            self.end += 2;
        }
    }
    /// Replace a byte span between two starts by explicit finished-line widths.
    /// Rebuild the reference vector directly, independently of replace_after.
    fn span(&mut self, first: usize, last: usize, segments: &[Entry]) {
        assert!(first <= last && last < self.entries.len());
        let origin = self.entries[first].0;
        let removed_bytes = self.entries[last].0 - origin;
        let inserted_bytes: usize = segments.iter().map(|e| e.0).sum();
        let mut next = self.entries[..first].to_vec();
        let mut cursor = origin;
        for &(width, ending) in segments {
            assert!(width > 0 && (1..=3).contains(&ending));
            next.push((cursor, ending));
            cursor += width;
        }
        next.push((cursor, self.entries[last].1));
        for &(start, ending) in &self.entries[last + 1..] {
            next.push((start - removed_bytes + inserted_bytes, ending));
        }
        self.entries = next;
        self.end = self.end - removed_bytes + inserted_bytes;
    }
}

fn span_index<I: Index>(
    index: &mut I,
    oracle: &Oracle,
    first: usize,
    last: usize,
    segments: &[Entry],
) {
    let mut cursor = oracle.entries[first].0;
    let removed_bytes = oracle.entries[last].0 - cursor;
    let mut scanned = Vec::with_capacity(segments.len() + 1);
    for &(width, ending) in segments {
        scanned.push((cursor, ending));
        cursor += width;
    }
    scanned.push((cursor, 0));
    let inserted_bytes = cursor - oracle.entries[first].0;
    let replacement = replacement::<I>(&scanned);
    index.shift_tail(
        last + 1,
        oracle.end,
        oracle.end - removed_bytes + inserted_bytes,
    );
    index.replace_after(first + 1..last + 1, replacement);
}

fn persistent_roundtrips<I: Index>(index: &mut I, oracle: &Oracle) {
    let mut state = 0x32f7_1dc9_u64;
    for round in 0..ROUNDTRIPS {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let line = match round {
            0 => 0,
            1 => oracle.entries.len() - 1,
            _ => state as usize % oracle.entries.len(),
        };
        let (insert, remove) = prepared::<I>(oracle.entries[line].0, true);
        forward(index, line, oracle.end, insert);
        assert_eq!(index.len(), oracle.entries.len() + 1);
        assert_eq!(index.get(line), Some((oracle.entries[line].0, 1)));
        assert_eq!(
            index.get(line + 1),
            Some((oracle.entries[line].0 + 2, oracle.entries[line].1))
        );
        inverse(index, line, oracle.end, remove);
        assert_eq!(index.len(), oracle.entries.len());
        assert_eq!(index.get(line), Some(oracle.entries[line]));
        assert_eq!(index.get(index.len() - 1), oracle.entries.last().copied());
    }
    assert_index(index, oracle);
}

fn multiblock_regressions<I: Index>() {
    // Always >=4096 entries; every nonempty removed span crosses >=2 blocks
    // of at most256 records, irrespective of earlier catalogue fragmentation.
    let mut oracle = Oracle {
        entries: (0..8192)
            .map(|line| (3 + line * 8, (line % 3 + 1) as u8))
            .chain(std::iter::once((3 + 8192 * 8, 0)))
            .collect(),
        end: 3 + 8192 * 8,
    };
    let original = oracle.clone();
    let mut index: I = build(&oracle.entries);
    let mut actions = Vec::new();
    let mut state = 0x6a11_ef41_u64;
    assert_index(&index, &oracle);
    for step in 0..96 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let first = (state as usize % ((oracle.entries.len() - 1024) / 256)) * 256 + 243;
        let (removed, inserted) = match step % 4 {
            0 => (0, 600),
            1 => (600, 700),
            2 => (512, 0),
            _ => (900, 650),
        };
        let last = first + removed;
        let old_segments: Vec<_> = (first..last)
            .map(|line| {
                (
                    oracle.entries[line + 1].0 - oracle.entries[line].0,
                    oracle.entries[line].1,
                )
            })
            .collect();
        let segments: Vec<_> = (0..inserted)
            .map(|line| (4 + (line + step) % 13, ((line + step) % 3 + 1) as u8))
            .collect();
        span_index(&mut index, &oracle, first, last, &segments);
        oracle.span(first, last, &segments);
        assert!(oracle.entries.len() >= 4096);
        assert_index(&index, &oracle);
        actions.push((first, inserted, old_segments));
    }
    for (first, inserted, old_segments) in actions.into_iter().rev() {
        span_index(&mut index, &oracle, first, first + inserted, &old_segments);
        oracle.span(first, first + inserted, &old_segments);
        assert_index(&index, &oracle);
    }
    assert_eq!(oracle.entries, original.entries);
    assert_eq!(oracle.end, original.end);
    index.shrink_to_fit();
    assert_index(&index, &oracle);
    persistent_roundtrips(&mut index, &oracle);
}

fn assert_index<I: Index>(index: &I, oracle: &Oracle) {
    assert_eq!(index.len(), oracle.entries.len(), "{} len", I::NAME);
    assert_eq!(index.entries(), oracle.entries, "{} entries/EOL", I::NAME);
    assert_eq!(index.first_start(), oracle.entries.first().map(|e| e.0));
    assert_eq!(index.get(index.len()), None);
    let mut offsets = vec![0, oracle.end, usize::MAX];
    let step = oracle.entries.len().div_ceil(4096).max(1);
    for (line, &(start, ending)) in oracle.entries.iter().enumerate().step_by(step) {
        assert_eq!(index.get(line), Some((start, ending)));
        assert_eq!(index.start(line), start);
        assert_eq!(index.ending(line), ending);
        // Exhaust small cases; cap probes in million-line comparison cases.
        offsets.push(start);
        if start > 0 {
            offsets.push(start - 1);
        }
        if let Some(&(next, _)) = oracle.entries.get(line + 1) {
            offsets.push(start + (next - start) / 2);
        }
    }
    for offset in offsets {
        assert_eq!(
            index.upper_bound(offset),
            oracle.upper_bound(offset),
            "{} upper_bound({offset})",
            I::NAME
        );
    }
}

/// Prepare owned replacement indices before entering any mutation timer.
fn prepared<I: Index>(start: usize, newline: bool) -> (Option<I>, Option<I>) {
    if newline {
        (
            Some(replacement(&[(start, 1), (start + 2, 0)])),
            Some(replacement(&[(start, 0)])),
        )
    } else {
        (None, None)
    }
}
fn forward<I: Index>(index: &mut I, line: usize, end: usize, insert: Option<I>) {
    index.shift_tail(line + 1, end, end + if insert.is_some() { 2 } else { 1 });
    if let Some(insert) = insert {
        index.replace_after(line + 1..line + 1, insert);
    }
}
fn inverse<I: Index>(index: &mut I, line: usize, end: usize, remove: Option<I>) {
    if let Some(remove) = remove {
        index.shift_tail(line + 2, end + 2, end);
        index.replace_after(line + 1..line + 2, remove);
    } else {
        index.shift_tail(line + 1, end + 1, end);
    }
}

fn regressions<I: Index>() {
    multiblock_regressions::<I>();
    let empty = I::default();
    assert_index(
        &empty,
        &Oracle {
            entries: vec![],
            end: 0,
        },
    );
    assert_index(
        &build::<I>(&[(0, 0)]),
        &Oracle {
            entries: vec![(0, 0)],
            end: 16 * 1024 * 1024,
        },
    );
    // Byte-coordinate metadata: BOM prefix, CRLF/CR/LF and final unterminated line.
    let mut oracle = Oracle {
        entries: vec![(3, 2), (9, 3), (10, 1), (17, 0)],
        end: 35,
    };
    let mut index: I = build(&oracle.entries);
    assert_index(&index, &oracle);
    let original = oracle.clone();
    // Deterministic retained changes (not just one edit followed immediately by undo).
    let mut actions = Vec::new();
    let mut state = 0x7b31_d42d_u64;
    for n in 0..96 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let line = (state as usize) % oracle.entries.len();
        let newline = n % 3 != 0;
        let end = oracle.end;
        let (insert, _) = prepared::<I>(oracle.entries[line].0, newline);
        forward(&mut index, line, end, insert);
        if newline {
            oracle.newline(line, false);
        } else {
            oracle.scalar(line, false);
        }
        assert_index(&index, &oracle);
        actions.push((line, end, newline));
    }
    for (line, end, newline) in actions.into_iter().rev() {
        let (_, remove) = prepared::<I>(oracle.entries[line].0, newline);
        inverse(&mut index, line, end, remove);
        if newline {
            oracle.newline(line, true);
        } else {
            oracle.scalar(line, true);
        }
        assert_index(&index, &oracle);
    }
    assert_eq!(oracle.entries, original.entries);
    assert_eq!(oracle.end, original.end);
    index.shrink_to_fit();
    assert_index(&index, &oracle);
    for entries in [
        vec![(0, 0)],
        vec![(3, 0)],
        vec![(0, 2), (usize::MAX - 8, 3), (usize::MAX, 0)],
        vec![
            (3, 1),
            ((u32::MAX >> 2) as usize, 2),
            (u32::MAX as usize, 0),
        ],
    ] {
        let oracle = Oracle {
            end: entries.last().unwrap().0,
            entries,
        };
        let mut index: I = build(&oracle.entries);
        index.widen_for(usize::MAX);
        assert_index(&index, &oracle);
        index.shrink_to_fit();
        assert_index(&index, &oracle);
    }
    // Width promotion by replacement itself, not only by push/widen_for.
    let mut index: I = build(&[(0, 2), (8, 0)]);
    index.replace_after(2..2, replacement(&[(8, 3), (usize::MAX, 0)]));
    assert_index(
        &index,
        &Oracle {
            entries: vec![(0, 2), (8, 3), (usize::MAX, 0)],
            end: usize::MAX,
        },
    );
    // Long metadata gap followed by small, reversible suffix shifts near usize::MAX.
    let oracle = Oracle {
        entries: vec![(0, 2), (usize::MAX - 16, 1), (usize::MAX - 8, 0)],
        end: usize::MAX - 8,
    };
    for newline in [false, true] {
        let mut index: I = build(&oracle.entries);
        let mut expected = oracle.clone();
        let (insert, remove) = prepared::<I>(oracle.entries[1].0, newline);
        forward(&mut index, 1, oracle.end, insert);
        if newline {
            expected.newline(1, false);
        } else {
            expected.scalar(1, false);
        }
        assert_index(&index, &expected);
        inverse(&mut index, 1, oracle.end, remove);
        assert_index(&index, &oracle);
    }
}

fn emit<I: Index>(
    bytes: usize,
    stride: usize,
    operation: &str,
    sample: usize,
    elapsed: u128,
    batch: usize,
    index: &I,
) {
    println!(
        "{},{bytes},{stride},{operation},{sample},{batch},{elapsed},{},{},{}",
        I::NAME,
        index.len(),
        index.capacity(),
        index.logical_bytes()
    );
}

fn hot_queries<I: Index>(bytes: usize, stride: usize, oracle: &Oracle, index: &I) {
    let mut state = 0x512f_317a_u64;
    let queries: Vec<_> = (0..QUERY_BATCH)
        .map(|_| {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let line = state as usize % oracle.entries.len();
            let offset = (state.rotate_left(17) as usize) % (bytes + 1);
            (line, offset)
        })
        .collect();
    for &(line, offset) in &queries {
        assert_eq!(index.get(line), Some(oracle.entries[line]));
        assert_eq!(index.upper_bound(offset), oracle.upper_bound(offset));
    }
    for operation in ["get_hot", "upper_bound_hot"] {
        for round in 0..WARMUPS + SAMPLES {
            let start = Instant::now();
            if operation == "get_hot" {
                for &(line, _) in &queries {
                    black_box(index.get(black_box(line)));
                }
            } else {
                for &(_, offset) in &queries {
                    black_box(index.upper_bound(black_box(offset)));
                }
            }
            let elapsed = start.elapsed().as_nanos();
            if round >= WARMUPS {
                emit(
                    bytes,
                    stride,
                    operation,
                    round - WARMUPS,
                    elapsed,
                    QUERY_BATCH,
                    index,
                );
            }
        }
    }
}

fn measure<I: Index>(bytes: usize, stride: usize, oracle: &Oracle) {
    for round in 0..WARMUPS + SAMPLES {
        let start = Instant::now();
        let index: I = black_box(build(black_box(&oracle.entries)));
        let elapsed = start.elapsed().as_nanos();
        if round >= WARMUPS {
            emit(
                bytes,
                stride,
                "cold_build",
                round - WARMUPS,
                elapsed,
                1,
                &index,
            );
        }
        assert_eq!(index.len(), oracle.entries.len());
    }
    let mut index: I = build(&oracle.entries);
    // Record native construction retention separately from explicit compaction.
    emit(bytes, stride, "retained_built", 0, 0, 0, &index);
    index.shrink_to_fit();
    emit(bytes, stride, "retained_shrunk", 0, 0, 0, &index);
    hot_queries(bytes, stride, oracle, &index);
    drop(index);
    // Descriptive retention only: no invented timing for the 512-pair stress.
    let mut persistent: I = build(&oracle.entries);
    persistent.shrink_to_fit();
    emit(
        bytes,
        stride,
        "roundtrip512_retained_initial",
        0,
        0,
        0,
        &persistent,
    );
    persistent_roundtrips(&mut persistent, oracle);
    emit(
        bytes,
        stride,
        "roundtrip512_retained_final",
        0,
        0,
        0,
        &persistent,
    );
    drop(persistent);
    for (position, line) in [
        ("begin", 0),
        ("middle", oracle.entries.len() / 2),
        ("eof", oracle.entries.len() - 1),
    ] {
        for newline in [false, true] {
            let kind = if newline { "newline" } else { "scalar" };
            let forward_name = format!("{kind}_{position}_forward");
            let inverse_name = format!("{kind}_{position}_inverse");
            let mut expected = oracle.clone();
            if newline {
                expected.newline(line, false);
            } else {
                expected.scalar(line, false);
            }
            for round in 0..WARMUPS + SAMPLES {
                // Fresh, compact baseline each time: first-reservation edit,
                // not a hot edit after an unreported capacity warmup.
                let mut index: I = build(&oracle.entries);
                index.shrink_to_fit();
                let (insert, remove) = prepared::<I>(oracle.entries[line].0, newline);
                let start = Instant::now();
                forward(&mut index, line, bytes, insert);
                let elapsed = start.elapsed().as_nanos();
                if round >= WARMUPS {
                    emit(
                        bytes,
                        stride,
                        &forward_name,
                        round - WARMUPS,
                        elapsed,
                        1,
                        &index,
                    );
                }
                if round == 0 || round + 1 == WARMUPS + SAMPLES {
                    assert_index(&index, &expected);
                }
                let start = Instant::now();
                inverse(&mut index, line, bytes, remove);
                let elapsed = start.elapsed().as_nanos();
                if round >= WARMUPS {
                    emit(
                        bytes,
                        stride,
                        &inverse_name,
                        round - WARMUPS,
                        elapsed,
                        1,
                        &index,
                    );
                }
                if round == 0 || round + 1 == WARMUPS + SAMPLES {
                    assert_index(&index, oracle);
                }
            }
        }
    }
}

fn main() {
    regressions::<flat::LineIndex>();
    regressions::<blocked::BlockLineIndex>();
    eprintln!("metadata oracles passed: flat + block");
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.as_slice() == ["--check-only"] {
        return;
    }
    let cases: Vec<(usize, usize)> = match args.as_slice() {
        [] => [1, 4, 8].into_iter().flat_map(|mib| [8, 64].map(|stride| (mib, stride)))
            .chain([(8, 8192), (8, 65536)]).collect(),
        [mib, stride] | [mib, stride, _] => {
            let mib = mib.parse::<usize>().expect("MiB: 1, 4 or 8");
            let stride = stride.parse::<usize>().expect("stride: 8, 64, 8192 or 65536");
            assert!(([1, 4, 8].contains(&mib) && [8, 64].contains(&stride)) || (mib == 8 && [8192, 65536].contains(&stride)));vec![(mib, stride)]
        }
        _ => panic!("usage: line_index_candidates [--check-only | MiB stride [flat|block|flat-first|block-first]]"),
    };
    let order = args.get(2).map_or("flat-first", String::as_str);
    assert!(["flat", "block", "flat-first", "block-first"].contains(&order));
    println!("implementation,source_bytes,stride,operation,sample,batch_size,elapsed_ns,lines,capacity_slots,logical_bytes");
    for (mib, stride) in cases {
        let bytes = mib * 1024 * 1024;
        let oracle = Oracle::regular(bytes, stride);
        if order == "block-first" || order == "block" {
            measure::<blocked::BlockLineIndex>(bytes, stride, &oracle);
        }
        if order != "block" {
            measure::<flat::LineIndex>(bytes, stride, &oracle);
        }
        if order == "flat-first" {
            measure::<blocked::BlockLineIndex>(bytes, stride, &oracle);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn flat_metadata_matches_independent_oracle() {
        super::regressions::<super::flat::LineIndex>();
    }
    #[test]
    fn blocked_metadata_matches_independent_oracle() {
        super::regressions::<super::blocked::BlockLineIndex>();
    }
}
