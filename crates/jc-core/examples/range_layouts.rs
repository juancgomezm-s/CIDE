//! Isolated candidate comparison, not a production tree or an editor benchmark.
use std::hint::black_box;
use std::time::Instant;

struct Tree {
    bytes: usize,
    leaves: usize,
    left: Option<Box<Tree>>,
    right: Option<Box<Tree>>,
}
impl Tree {
    fn build(lengths: &[usize]) -> Self {
        if lengths.len() == 1 {
            return Self {
                bytes: lengths[0],
                leaves: 1,
                left: None,
                right: None,
            };
        }
        let middle = lengths.len() / 2;
        let left = Box::new(Self::build(&lengths[..middle]));
        let right = Box::new(Self::build(&lengths[middle..]));
        Self {
            bytes: left.bytes + right.bytes,
            leaves: lengths.len(),
            left: Some(left),
            right: Some(right),
        }
    }
    fn lookup(&self, offset: usize) -> usize {
        let Some(left) = &self.left else {
            return 0;
        };
        if offset < left.bytes {
            left.lookup(offset)
        } else {
            left.leaves + self.right.as_ref().unwrap().lookup(offset - left.bytes)
        }
    }
    fn grow(&mut self, index: usize) {
        self.bytes += 1;
        if let Some(left) = &mut self.left {
            if index < left.leaves {
                left.grow(index);
            } else {
                self.right.as_mut().unwrap().grow(index - left.leaves);
            }
        }
    }
}
struct Blocks {
    ends: Vec<usize>,
    local: Vec<usize>,
}
impl Blocks {
    fn build(lengths: &[usize]) -> Self {
        let mut ends = Vec::new();
        let mut local = Vec::new();
        let mut total = 0;
        for chunk in lengths.chunks(128) {
            let mut sum = 0;
            local.extend(chunk.iter().map(|n| {
                sum += n;
                sum
            }));
            total += sum;
            ends.push(total);
        }
        Self { ends, local }
    }
    fn lookup(&self, offset: usize) -> usize {
        let block = self.ends.partition_point(|&end| end <= offset);
        let base = block.checked_sub(1).map_or(0, |n| self.ends[n]);
        let from = block * 128;
        from + self.local[from..(from + 128).min(self.local.len())]
            .partition_point(|&end| end <= offset - base)
    }
    fn grow(&mut self, index: usize) {
        let block = index / 128;
        let end = ((block + 1) * 128).min(self.local.len());
        for value in &mut self.local[index..end] {
            *value += 1;
        }
        for end in &mut self.ends[block..] {
            *end += 1;
        }
    }
}
fn sample(mut operation: impl FnMut()) -> (u128, u128) {
    for _ in 0..5 {
        operation();
    }
    let mut times = Vec::new();
    for _ in 0..31 {
        let start = Instant::now();
        operation();
        times.push(start.elapsed().as_nanos());
    }
    times.sort_unstable();
    (times[15], times[29])
}
fn main() {
    println!(
        "pieces,layout,lookup_batch_p50_ns,lookup_batch_p95_ns,grow_200_ns,metadata_bytes_estimate"
    );
    for count in [16usize, 1024, 16384] {
        let lengths: Vec<_> = (0..count).map(|i| 8 + i % 17).collect();
        let mut total = 0;
        let mut ends: Vec<_> = lengths
            .iter()
            .map(|n| {
                total += n;
                total
            })
            .collect();
        let mut blocks = Blocks::build(&lengths);
        let mut tree = Tree::build(&lengths);
        let mut seed = 7u64;
        let queries: Vec<_> = (0..1000)
            .map(|_| {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                seed as usize % total
            })
            .collect();
        for &offset in &queries {
            let expected = ends.partition_point(|&end| end <= offset);
            assert_eq!(blocks.lookup(offset), expected);
            assert_eq!(tree.lookup(offset), expected);
        }
        let vector = sample(|| {
            for &offset in &queries {
                black_box(ends.partition_point(|&end| end <= black_box(offset)));
            }
        });
        let blocked = sample(|| {
            for &offset in &queries {
                black_box(blocks.lookup(black_box(offset)));
            }
        });
        let boxed = sample(|| {
            for &offset in &queries {
                black_box(tree.lookup(black_box(offset)));
            }
        });
        let indices: Vec<_> = queries.iter().take(200).map(|n| n % count).collect();
        let start = Instant::now();
        for &index in &indices {
            for end in &mut ends[index..] {
                *end += 1;
            }
        }
        let vector_edit = start.elapsed().as_nanos();
        let start = Instant::now();
        for &index in &indices {
            blocks.grow(index);
        }
        let block_edit = start.elapsed().as_nanos();
        let start = Instant::now();
        for &index in &indices {
            tree.grow(index);
        }
        let tree_edit = start.elapsed().as_nanos();
        let block_bytes =
            (blocks.ends.capacity() + blocks.local.capacity()) * std::mem::size_of::<usize>();
        for (name, time, edit, bytes) in [
            ("prefix-vector", vector, vector_edit, ends.capacity() * 8),
            ("blocks-128", blocked, block_edit, block_bytes),
            (
                "boxed-tree",
                boxed,
                tree_edit,
                (count * 2 - 1) * std::mem::size_of::<Tree>(),
            ),
        ] {
            println!("{count},{name},{},{},{edit},{bytes}", time.0, time.1);
        }
    }
}
