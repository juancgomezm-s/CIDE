//! Bounded, byte-exact line comparison of two borrowed immutable sources.
//!
//! This read model never receives a Document/FileSession, decodes text, executes
//! Git, or proposes/applies edits. A frontend must obtain versioned snapshots
//! separately before handing their bytes to this synchronous, cancelable job.
use std::{mem::size_of, ops::Range, sync::Arc};

/// Zero-based line in a diff source, not a byte/scalar/display coordinate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DiffLine(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffSide {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffKind {
    Equal,
    Changed,
}

/// Half-open line ranges. Either side of a change can be empty (insert/delete).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffSpan {
    pub kind: DiffKind,
    pub left: Range<DiffLine>,
    pub right: Range<DiffLine>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLimit {
    InputBytes,
    Lines,
    Work,
    Metadata,
    Hunks,
}
impl DiffLimit {
    pub fn label(self) -> &'static str {
        match self {
            Self::InputBytes => "bytes de entrada",
            Self::Lines => "líneas",
            Self::Work => "trabajo de comparación",
            Self::Metadata => "metadatos",
            Self::Hunks => "bloques de diferencias",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffStatus {
    Exact,
    Limited(DiffLimit),
    Cancelled,
}

/// Combined-source quotas. Metadata includes the temporary Myers trace; it is
/// logical Vec capacity, not a bound on allocator overhead or process RAM.
#[derive(Debug, Clone, Copy)]
pub struct DiffLimits {
    pub max_input_bytes: usize,
    pub max_lines: usize,
    pub max_work: usize,
    pub max_metadata_bytes: usize,
    pub max_hunks: usize,
}
impl Default for DiffLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 16 * 1024 * 1024,
            max_lines: 200_000,
            max_work: 64 * 1024 * 1024,
            max_metadata_bytes: 16 * 1024 * 1024,
            max_hunks: 4096,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DiffStats {
    pub work_units: usize,
    pub compared_bytes: usize,
    pub peak_metadata_bytes: usize,
}

/// A completed alignment or an explicit failure without partial equivalences.
/// A source has zero diff lines when empty. Its terminator belongs to each line;
/// a final newline does not create a synthetic extra empty diff line.
#[derive(Debug)]
pub struct DiffModel<'a> {
    left: &'a [u8],
    right: &'a [u8],
    left_ends: Vec<usize>,
    right_ends: Vec<usize>,
    spans: Vec<DiffSpan>,
    status: DiffStatus,
    stats: DiffStats,
}
impl<'a> DiffModel<'a> {
    /// Call on bounded snapshots, outside the frontend's input/render critical
    /// path. Cancellation is checked during indexing, comparisons and tracing;
    /// the callback itself must be cheap and nonblocking.
    pub fn compute(
        left: &'a [u8],
        right: &'a [u8],
        limits: DiffLimits,
        cancelled: impl FnMut() -> bool,
    ) -> Self {
        let mut budget = Budget {
            limits,
            stats: DiffStats::default(),
            lines: 0,
            cancelled,
        };
        let result = build(left, right, &mut budget);
        let (left_ends, right_ends, spans, status) = match result {
            Ok((left_ends, right_ends, spans)) => (left_ends, right_ends, spans, DiffStatus::Exact),
            Err(status) => (Vec::new(), Vec::new(), Vec::new(), status),
        };
        Self {
            left,
            right,
            left_ends,
            right_ends,
            spans,
            status,
            stats: budget.stats,
        }
    }
    pub fn status(&self) -> DiffStatus {
        self.status
    }
    pub fn stats(&self) -> DiffStats {
        self.stats
    }
    pub fn spans(&self) -> &[DiffSpan] {
        &self.spans
    }
    /// Hunks contain changed lines only; context lives in adjacent Equal spans.
    pub fn hunks(&self) -> impl Iterator<Item = &DiffSpan> {
        self.spans.iter().filter(|s| s.kind == DiffKind::Changed)
    }
    pub fn source_bytes(&self, side: DiffSide) -> &'a [u8] {
        match side {
            DiffSide::Left => self.left,
            DiffSide::Right => self.right,
        }
    }
    fn ends(&self, side: DiffSide) -> &[usize] {
        match side {
            DiffSide::Left => &self.left_ends,
            DiffSide::Right => &self.right_ends,
        }
    }
    pub fn line_count(&self, side: DiffSide) -> Option<usize> {
        (self.status == DiffStatus::Exact).then(|| self.ends(side).len())
    }
    pub fn line_byte_range(&self, side: DiffSide, line: DiffLine) -> Option<Range<usize>> {
        if self.status != DiffStatus::Exact {
            return None;
        }
        let ends = self.ends(side);
        let end = *ends.get(line.0)?;
        let start = if line.0 == 0 { 0 } else { ends[line.0 - 1] };
        Some(start..end)
    }
    pub fn line_bytes(&self, side: DiffSide, line: DiffLine) -> Option<&'a [u8]> {
        let range = self.line_byte_range(side, line)?;
        Some(&self.source_bytes(side)[range])
    }
    /// Only exact equal spans have corresponding lines. Changed lines have no
    /// invented one-to-one counterpart, even when both changed ranges are equal.
    pub fn corresponding_line(&self, side: DiffSide, line: DiffLine) -> Option<DiffLine> {
        corresponding_line(&self.spans, side, line)
    }
}

/// Worker/UI ownership of the same immutable alignment. Sources are shared owned
/// byte vectors, never documents or sessions; computing moves metadata, not bytes.
#[derive(Debug)]
pub struct OwnedDiffModel {
    left: Arc<Vec<u8>>,
    right: Arc<Vec<u8>>,
    left_ends: Vec<usize>,
    right_ends: Vec<usize>,
    spans: Vec<DiffSpan>,
    status: DiffStatus,
    stats: DiffStats,
}
impl OwnedDiffModel {
    /// Read-only presentation can retain a complete, non-minimal replacement
    /// when the detailed trace exceeds its budget. Strict callers keep compute.
    pub fn compute_display(
        left: Arc<Vec<u8>>,
        right: Arc<Vec<u8>>,
        limits: DiffLimits,
        mut cancelled: impl FnMut() -> bool,
    ) -> (Self, bool) {
        let detailed_limits = DiffLimits {
            max_work: limits.max_work / 2,
            ..limits
        };
        let detailed = Self::compute(left.clone(), right.clone(), detailed_limits, &mut cancelled);
        if !matches!(
            detailed.status(),
            DiffStatus::Limited(DiffLimit::Metadata | DiffLimit::Work)
        ) {
            return (detailed, false);
        }
        let previous_stats = detailed.stats();
        drop(detailed);
        let mut budget = Budget {
            limits: DiffLimits {
                max_work: limits.max_work.saturating_sub(previous_stats.work_units),
                ..limits
            },
            stats: DiffStats::default(),
            lines: 0,
            cancelled,
        };
        let result = (|| {
            budget.check()?;
            if left
                .len()
                .checked_add(right.len())
                .is_none_or(|n| n > limits.max_input_bytes)
            {
                return Err(DiffStatus::Limited(DiffLimit::InputBytes));
            }
            let a = index_lines(&left, &mut budget)?;
            let b = index_lines(&right, &mut budget)?;
            let mut prefix = 0;
            while prefix < a.len().min(b.len())
                && equal(
                    line(&left, &a, prefix),
                    line(&right, &b, prefix),
                    &mut budget,
                )?
            {
                prefix += 1;
            }
            let mut suffix = 0;
            while suffix < (a.len() - prefix).min(b.len() - prefix)
                && equal(
                    line(&left, &a, a.len() - suffix - 1),
                    line(&right, &b, b.len() - suffix - 1),
                    &mut budget,
                )?
            {
                suffix += 1;
            }
            let mut spans = Vec::new();
            let mut hunks = 0;
            if suffix > 0 {
                push_span(
                    &mut spans,
                    DiffKind::Equal,
                    a.len() - suffix..a.len(),
                    b.len() - suffix..b.len(),
                    &mut hunks,
                    &mut budget,
                )?;
            }
            if prefix < a.len() - suffix || prefix < b.len() - suffix {
                push_span(
                    &mut spans,
                    DiffKind::Changed,
                    prefix..a.len() - suffix,
                    prefix..b.len() - suffix,
                    &mut hunks,
                    &mut budget,
                )?;
            }
            if prefix > 0 {
                push_span(
                    &mut spans,
                    DiffKind::Equal,
                    0..prefix,
                    0..prefix,
                    &mut hunks,
                    &mut budget,
                )?;
            }
            spans.reverse();
            Ok((a, b, spans))
        })();
        let (left_ends, right_ends, spans, status) = match result {
            Ok((a, b, s)) => (a, b, s, DiffStatus::Exact),
            Err(status) => (Vec::new(), Vec::new(), Vec::new(), status),
        };
        budget.stats.work_units += previous_stats.work_units;
        budget.stats.compared_bytes += previous_stats.compared_bytes;
        budget.stats.peak_metadata_bytes = budget
            .stats
            .peak_metadata_bytes
            .max(previous_stats.peak_metadata_bytes);
        (
            Self {
                left,
                right,
                left_ends,
                right_ends,
                spans,
                status,
                stats: budget.stats,
            },
            true,
        )
    }
    pub fn compute(
        left: Arc<Vec<u8>>,
        right: Arc<Vec<u8>>,
        limits: DiffLimits,
        cancelled: impl FnMut() -> bool,
    ) -> Self {
        let DiffModel {
            left_ends,
            right_ends,
            spans,
            status,
            stats,
            ..
        } = DiffModel::compute(&left, &right, limits, cancelled);
        Self {
            left,
            right,
            left_ends,
            right_ends,
            spans,
            status,
            stats,
        }
    }
    pub fn status(&self) -> DiffStatus {
        self.status
    }
    pub fn stats(&self) -> DiffStats {
        self.stats
    }
    pub fn spans(&self) -> &[DiffSpan] {
        &self.spans
    }
    pub fn hunks(&self) -> impl Iterator<Item = &DiffSpan> {
        self.spans
            .iter()
            .filter(|span| span.kind == DiffKind::Changed)
    }
    pub fn source_bytes(&self, side: DiffSide) -> &[u8] {
        match side {
            DiffSide::Left => &self.left,
            DiffSide::Right => &self.right,
        }
    }
    fn ends(&self, side: DiffSide) -> &[usize] {
        match side {
            DiffSide::Left => &self.left_ends,
            DiffSide::Right => &self.right_ends,
        }
    }
    pub fn line_count(&self, side: DiffSide) -> Option<usize> {
        (self.status == DiffStatus::Exact).then(|| self.ends(side).len())
    }
    pub fn line_byte_range(&self, side: DiffSide, line: DiffLine) -> Option<Range<usize>> {
        if self.status != DiffStatus::Exact {
            return None;
        }
        let ends = self.ends(side);
        let end = *ends.get(line.0)?;
        Some(if line.0 == 0 { 0 } else { ends[line.0 - 1] }..end)
    }
    pub fn line_bytes(&self, side: DiffSide, line: DiffLine) -> Option<&[u8]> {
        let range = self.line_byte_range(side, line)?;
        Some(&self.source_bytes(side)[range])
    }
    pub fn corresponding_line(&self, side: DiffSide, line: DiffLine) -> Option<DiffLine> {
        corresponding_line(&self.spans, side, line)
    }
    /// Conservative logical ownership charge, including both source capacities.
    pub fn memory_bytes(&self) -> usize {
        size_of::<Self>()
            + self.left.capacity()
            + self.right.capacity()
            + 2 * (size_of::<Vec<u8>>() + 2 * size_of::<usize>())
            + (self.left_ends.capacity() + self.right_ends.capacity()) * size_of::<usize>()
            + self.spans.capacity() * size_of::<DiffSpan>()
    }
}

fn corresponding_line(spans: &[DiffSpan], side: DiffSide, line: DiffLine) -> Option<DiffLine> {
    let index = spans.partition_point(|span| {
        let range = match side {
            DiffSide::Left => &span.left,
            DiffSide::Right => &span.right,
        };
        range.end <= line
    });
    let span = spans.get(index)?;
    let (from, to) = match side {
        DiffSide::Left => (&span.left, &span.right),
        DiffSide::Right => (&span.right, &span.left),
    };
    (span.kind == DiffKind::Equal && from.contains(&line))
        .then(|| DiffLine(to.start.0 + line.0 - from.start.0))
}

type Stopped = DiffStatus;
struct Budget<F> {
    limits: DiffLimits,
    stats: DiffStats,
    lines: usize,
    cancelled: F,
}
impl<F: FnMut() -> bool> Budget<F> {
    fn check(&mut self) -> Result<(), Stopped> {
        if (self.cancelled)() {
            Err(DiffStatus::Cancelled)
        } else {
            Ok(())
        }
    }
    fn work(&mut self, count: usize) -> Result<(), Stopped> {
        self.check()?;
        if count > self.limits.max_work.saturating_sub(self.stats.work_units) {
            return Err(DiffStatus::Limited(DiffLimit::Work));
        }
        self.stats.work_units += count;
        Ok(())
    }
    fn capacity<T>(&mut self, vec: &mut Vec<T>, target: usize) -> Result<(), Stopped> {
        let extra = target.saturating_sub(vec.capacity());
        let charge = extra
            .checked_mul(size_of::<T>())
            .ok_or(DiffStatus::Limited(DiffLimit::Metadata))?;
        if charge
            > self
                .limits
                .max_metadata_bytes
                .saturating_sub(self.stats.peak_metadata_bytes)
        {
            return Err(DiffStatus::Limited(DiffLimit::Metadata));
        }
        let old = vec.capacity();
        vec.try_reserve_exact(target.saturating_sub(vec.len()))
            .map_err(|_| DiffStatus::Limited(DiffLimit::Metadata))?;
        let actual = (vec.capacity() - old) * size_of::<T>();
        self.stats.peak_metadata_bytes += actual;
        if self.stats.peak_metadata_bytes > self.limits.max_metadata_bytes {
            return Err(DiffStatus::Limited(DiffLimit::Metadata));
        }
        Ok(())
    }
    fn push<T>(&mut self, vec: &mut Vec<T>, item: T) -> Result<(), Stopped> {
        if vec.len() == vec.capacity() {
            // Small bounded growth keeps short diffs usable under small quotas.
            self.capacity(vec, vec.len().saturating_mul(2).max(4))?;
        }
        vec.push(item);
        Ok(())
    }
    fn line_end(&mut self, ends: &mut Vec<usize>, end: usize) -> Result<(), Stopped> {
        if self.lines >= self.limits.max_lines {
            return Err(DiffStatus::Limited(DiffLimit::Lines));
        }
        self.lines += 1;
        self.push(ends, end)
    }
}

fn index_lines(
    bytes: &[u8],
    budget: &mut Budget<impl FnMut() -> bool>,
) -> Result<Vec<usize>, Stopped> {
    let mut ends = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        // No UTF-8 decoding or normalization. CRLF is one terminator.
        let end = (i + 1024).min(bytes.len());
        budget.work(end - i)?;
        while i < end {
            let byte = bytes[i];
            i += 1;
            if byte == b'\r' {
                if bytes.get(i) == Some(&b'\n') {
                    // A CR at the accounting block edge consumes one more byte.
                    if i == end {
                        budget.work(1)?;
                    }
                    i += 1;
                }
                budget.line_end(&mut ends, i)?;
            } else if byte == b'\n' {
                budget.line_end(&mut ends, i)?;
            }
        }
    }
    if ends.last().copied().unwrap_or(0) < bytes.len() {
        budget.line_end(&mut ends, bytes.len())?;
    }
    Ok(ends)
}
fn line<'a>(bytes: &'a [u8], ends: &[usize], i: usize) -> &'a [u8] {
    &bytes[if i == 0 { 0 } else { ends[i - 1] }..ends[i]]
}
fn equal(
    left: &[u8],
    right: &[u8],
    budget: &mut Budget<impl FnMut() -> bool>,
) -> Result<bool, Stopped> {
    budget.work(1)?;
    if left.len() != right.len() {
        return Ok(false);
    }
    for (a, b) in left.chunks(1024).zip(right.chunks(1024)) {
        budget.work(a.len())?;
        budget.stats.compared_bytes += a.len();
        if a != b {
            return Ok(false);
        }
    }
    Ok(true)
}

type Built = (Vec<usize>, Vec<usize>, Vec<DiffSpan>);
fn build(
    left: &[u8],
    right: &[u8],
    budget: &mut Budget<impl FnMut() -> bool>,
) -> Result<Built, Stopped> {
    budget.check()?;
    if left
        .len()
        .checked_add(right.len())
        .is_none_or(|n| n > budget.limits.max_input_bytes)
    {
        return Err(DiffStatus::Limited(DiffLimit::InputBytes));
    }
    let a = index_lines(left, budget)?;
    let b = index_lines(right, budget)?;
    let (n, m) = (a.len(), b.len());
    let max = n
        .checked_add(m)
        .filter(|&n| n <= isize::MAX as usize)
        .ok_or(DiffStatus::Limited(DiffLimit::Lines))?;
    let mut trace: Vec<Vec<usize>> = Vec::new();
    let mut distance = 0;
    'search: for d in 0..=max {
        budget.work(1)?;
        let mut layer = Vec::new();
        budget.capacity(&mut layer, d + 1)?;
        for j in 0..=d {
            budget.work(1)?;
            let k = 2 * j as isize - d as isize;
            let mut x = if d == 0 {
                0
            } else {
                let previous = &trace[d - 1];
                if j == 0 || j < d && previous[j - 1] < previous[j] {
                    previous[j]
                } else {
                    previous[j - 1] + 1
                }
            };
            let mut y = (x as isize - k) as usize;
            while x < n && y < m && equal(line(left, &a, x), line(right, &b, y), budget)? {
                x += 1;
                y += 1;
            }
            layer.push(x);
            if x >= n && y >= m {
                budget.push(&mut trace, layer)?;
                distance = d;
                break 'search;
            }
        }
        budget.push(&mut trace, layer)?;
    }
    let mut spans = Vec::new();
    let (mut x, mut y) = (n, m);
    let mut hunks = 0;
    for d in (1..=distance).rev() {
        budget.work(1)?;
        let k = x as isize - y as isize;
        let j = ((k + d as isize) / 2) as usize;
        let previous = &trace[d - 1];
        let (prev_x, prev_k) = if j == 0 || j < d && previous[j - 1] < previous[j] {
            (previous[j], k + 1)
        } else {
            (previous[j - 1], k - 1)
        };
        let prev_y = (prev_x as isize - prev_k) as usize;
        let same = (x - prev_x).min(y - prev_y);
        if same > 0 {
            push_span(
                &mut spans,
                DiffKind::Equal,
                x - same..x,
                y - same..y,
                &mut hunks,
                budget,
            )?;
            x -= same;
            y -= same;
        }
        if x == prev_x {
            push_span(
                &mut spans,
                DiffKind::Changed,
                x..x,
                y - 1..y,
                &mut hunks,
                budget,
            )?;
            y -= 1;
        } else {
            push_span(
                &mut spans,
                DiffKind::Changed,
                x - 1..x,
                y..y,
                &mut hunks,
                budget,
            )?;
            x -= 1;
        }
    }
    if x > 0 || y > 0 {
        push_span(&mut spans, DiffKind::Equal, 0..x, 0..y, &mut hunks, budget)?;
    }
    spans.reverse();
    Ok((a, b, spans))
}
fn push_span(
    spans: &mut Vec<DiffSpan>,
    kind: DiffKind,
    left: Range<usize>,
    right: Range<usize>,
    hunks: &mut usize,
    budget: &mut Budget<impl FnMut() -> bool>,
) -> Result<(), Stopped> {
    if let Some(last) = spans
        .last_mut()
        .filter(|s| s.kind == kind && s.left.start.0 == left.end && s.right.start.0 == right.end)
    {
        last.left.start = DiffLine(left.start);
        last.right.start = DiffLine(right.start);
        return Ok(());
    }
    if kind == DiffKind::Changed {
        if *hunks >= budget.limits.max_hunks {
            return Err(DiffStatus::Limited(DiffLimit::Hunks));
        }
        *hunks += 1;
    }
    budget.push(
        spans,
        DiffSpan {
            kind,
            left: DiffLine(left.start)..DiffLine(left.end),
            right: DiffLine(right.start)..DiffLine(right.end),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn display_dense_eol_changes_stay_visible_with_bounded_complete_sources() {
        let a = Arc::new(
            (0..1200)
                .map(|n| format!("linea {n}\n"))
                .collect::<String>()
                .into_bytes(),
        );
        let b = Arc::new(
            (0..1200)
                .map(|n| format!("linea {n}\r\n"))
                .collect::<String>()
                .into_bytes(),
        );
        let limits = DiffLimits {
            max_metadata_bytes: 4 * 1024 * 1024,
            ..Default::default()
        };
        assert_eq!(
            OwnedDiffModel::compute(a.clone(), b.clone(), limits, || false).status(),
            DiffStatus::Limited(DiffLimit::Metadata)
        );
        let (model, simplified) =
            OwnedDiffModel::compute_display(a.clone(), b.clone(), limits, || false);
        assert!(simplified);
        assert_eq!(model.status(), DiffStatus::Exact);
        assert_eq!(model.source_bytes(DiffSide::Left), &**a);
        assert_eq!(model.source_bytes(DiffSide::Right), &**b);
        assert_eq!(
            model.line_bytes(DiffSide::Right, DiffLine(1199)).unwrap(),
            b"linea 1199\r\n"
        );
        assert!(model.stats().peak_metadata_bytes <= limits.max_metadata_bytes);
        let (cancelled, _) = OwnedDiffModel::compute_display(a, b, limits, || true);
        assert_eq!(cancelled.status(), DiffStatus::Cancelled);
    }
    #[test]
    fn owned_alignment_retains_original_sources_and_exact_line_mapping() {
        let left = Arc::new(b"\xef\xbb\xbfa\xff\r\nb\nlast".to_vec());
        let right = Arc::new(b"\xef\xbb\xbfa\xff\r\nadded\rb\nlast".to_vec());
        let left_pointer = left.as_ptr();
        let right_pointer = right.as_ptr();
        let model = OwnedDiffModel::compute(left, right, DiffLimits::default(), || false);
        assert_eq!(model.status(), DiffStatus::Exact);
        assert_eq!(model.source_bytes(DiffSide::Left).as_ptr(), left_pointer);
        assert_eq!(model.source_bytes(DiffSide::Right).as_ptr(), right_pointer);
        assert_eq!(
            model.line_bytes(DiffSide::Left, DiffLine(0)),
            Some(b"\xef\xbb\xbfa\xff\r\n".as_slice())
        );
        assert_eq!(
            model.line_byte_range(DiffSide::Right, DiffLine(1)),
            Some(7..13)
        );
        assert_eq!(
            model.corresponding_line(DiffSide::Left, DiffLine(1)),
            Some(DiffLine(2))
        );
        assert_eq!(model.corresponding_line(DiffSide::Right, DiffLine(1)), None);
        assert_eq!(model.line_count(DiffSide::Right), Some(4));
        assert_eq!(model.hunks().count(), 1);
        assert!(
            model.memory_bytes()
                >= model.source_bytes(DiffSide::Left).len()
                    + model.source_bytes(DiffSide::Right).len()
        );
    }
    fn diff<'a>(left: &'a [u8], right: &'a [u8]) -> DiffModel<'a> {
        let result = DiffModel::compute(left, right, DiffLimits::default(), || false);
        assert_eq!(result.status(), DiffStatus::Exact);
        result
    }
    fn ranges(model: &DiffModel<'_>) -> Vec<(Range<usize>, Range<usize>)> {
        model
            .hunks()
            .map(|h| (h.left.start.0..h.left.end.0, h.right.start.0..h.right.end.0))
            .collect()
    }
    #[test]
    fn insert_delete_replace_and_line_correspondence_are_exact() {
        let inserted = diff(b"a\nb\nc\n", b"a\nx\nb\nc\n");
        assert_eq!(ranges(&inserted), vec![(1..1, 1..2)]);
        assert_eq!(
            inserted.corresponding_line(DiffSide::Left, DiffLine(1)),
            Some(DiffLine(2))
        );
        assert_eq!(
            inserted.corresponding_line(DiffSide::Right, DiffLine(1)),
            None
        );
        assert_eq!(ranges(&diff(b"a\nx\nb\n", b"a\nb\n")), vec![(1..2, 1..1)]);
        let changed = diff(b"a\nb\nc", b"a\nx\nc");
        assert_eq!(ranges(&changed), vec![(1..2, 1..2)]);
        assert_eq!(
            changed.corresponding_line(DiffSide::Left, DiffLine(1)),
            None
        );
        assert_eq!(
            changed.corresponding_line(DiffSide::Left, DiffLine(3)),
            None
        );
    }
    #[test]
    fn eol_bom_invalid_bytes_and_terminal_newline_remain_significant() {
        let source = b"\xef\xbb\xbf\xff\r\na\rb\nlast";
        let same = diff(source, source);
        assert!(same.hunks().next().is_none());
        assert_eq!(same.line_count(DiffSide::Left), Some(4));
        assert_eq!(
            same.line_bytes(DiffSide::Left, DiffLine(0)),
            Some(&source[..6])
        );
        assert_eq!(same.source_bytes(DiffSide::Left), source);
        assert_eq!(ranges(&diff(b"a\r\n", b"a\n")), vec![(0..1, 0..1)]);
        assert_eq!(ranges(&diff(b"a", b"a\n")), vec![(0..1, 0..1)]);
        assert_eq!(ranges(&diff(b"", b"\n")), vec![(0..0, 0..1)]);
        assert_eq!(diff(b"", b"").line_count(DiffSide::Right), Some(0));
    }
    #[test]
    fn repeated_lines_and_separate_hunks_keep_ordered_equal_mappings() {
        let model = diff(b"a\na\nb\na\nc\n", b"a\nb\na\nx\nc\n");
        let mut left = 0;
        let mut right = 0;
        for span in model.spans() {
            assert_eq!(span.left.start.0, left);
            assert_eq!(span.right.start.0, right);
            if span.kind == DiffKind::Equal {
                assert_eq!(span.left.end.0 - left, span.right.end.0 - right);
                for i in 0..(span.left.end.0 - left) {
                    assert_eq!(
                        model.line_bytes(DiffSide::Left, DiffLine(left + i)),
                        model.line_bytes(DiffSide::Right, DiffLine(right + i))
                    );
                }
            }
            left = span.left.end.0;
            right = span.right.end.0;
        }
        assert_eq!((left, right), (5, 5));
        assert_eq!(model.hunks().count(), 2);
    }
    #[test]
    fn small_repeated_line_corpus_agrees_with_an_independent_lcs_oracle() {
        let sequences: Vec<Vec<u8>> = (0..5)
            .flat_map(|len| {
                (0..(1 << len)).map(move |bits| {
                    (0..len)
                        .map(|i| if bits & (1 << i) == 0 { b'a' } else { b'b' })
                        .collect()
                })
            })
            .collect();
        for a in &sequences {
            for b in &sequences {
                let left: Vec<_> = a.iter().flat_map(|&ch| [ch, b'\n']).collect();
                let right: Vec<_> = b.iter().flat_map(|&ch| [ch, b'\n']).collect();
                let model = diff(&left, &right);
                let mut lcs = vec![vec![0; b.len() + 1]; a.len() + 1];
                for (i, left) in a.iter().enumerate() {
                    for (j, right) in b.iter().enumerate() {
                        lcs[i + 1][j + 1] = if left == right {
                            lcs[i][j] + 1
                        } else {
                            lcs[i][j + 1].max(lcs[i + 1][j])
                        };
                    }
                }
                let actual: usize = model
                    .hunks()
                    .map(|s| s.left.end.0 - s.left.start.0 + s.right.end.0 - s.right.start.0)
                    .sum();
                assert_eq!(actual, a.len() + b.len() - 2 * lcs[a.len()][b.len()]);
                for span in model.spans().iter().filter(|s| s.kind == DiffKind::Equal) {
                    let l = &a[span.left.start.0..span.left.end.0];
                    let r = &b[span.right.start.0..span.right.end.0];
                    assert_eq!(l, r);
                }
            }
        }
    }
    #[test]
    fn quotas_and_cancellation_never_publish_partial_equivalences() {
        for (limits, why) in [
            (
                DiffLimits {
                    max_input_bytes: 1,
                    ..DiffLimits::default()
                },
                DiffLimit::InputBytes,
            ),
            (
                DiffLimits {
                    max_lines: 1,
                    ..DiffLimits::default()
                },
                DiffLimit::Lines,
            ),
            (
                DiffLimits {
                    max_work: 1,
                    ..DiffLimits::default()
                },
                DiffLimit::Work,
            ),
            (
                DiffLimits {
                    max_metadata_bytes: 1,
                    ..DiffLimits::default()
                },
                DiffLimit::Metadata,
            ),
            (
                DiffLimits {
                    max_hunks: 0,
                    ..DiffLimits::default()
                },
                DiffLimit::Hunks,
            ),
        ] {
            let model = DiffModel::compute(b"a\n", b"b\n", limits, || false);
            assert_eq!(model.status(), DiffStatus::Limited(why));
            assert!(model.spans().is_empty());
            assert_eq!(model.line_count(DiffSide::Left), None);
            assert_eq!(model.corresponding_line(DiffSide::Right, DiffLine(0)), None);
        }
        let source = vec![b'x'; 1024 * 1024];
        let mut calls = 0;
        let model = DiffModel::compute(&source, &source, DiffLimits::default(), || {
            calls += 1;
            calls > 5
        });
        assert_eq!(model.status(), DiffStatus::Cancelled);
        assert!(model.spans().is_empty());
        assert!(model.stats().work_units <= 5 * 1024);
    }
    #[test]
    fn long_lines_and_adversarial_changes_obey_work_and_trace_budgets() {
        let source = vec![b'x'; 128 * 1024];
        let model = DiffModel::compute(
            &source,
            &source,
            DiffLimits {
                max_work: source.len() * 2 + 4096,
                ..DiffLimits::default()
            },
            || false,
        );
        assert_eq!(model.status(), DiffStatus::Limited(DiffLimit::Work));
        assert!(model.stats().compared_bytes <= 4096);
        let a = b"a\n".repeat(128);
        let b = b"b\n".repeat(128);
        let model = DiffModel::compute(
            &a,
            &b,
            DiffLimits {
                max_metadata_bytes: 16 * 1024,
                ..DiffLimits::default()
            },
            || false,
        );
        assert_eq!(model.status(), DiffStatus::Limited(DiffLimit::Metadata));
        assert!(model.stats().peak_metadata_bytes <= 16 * 1024);
        assert!(model.spans().is_empty());
    }
}
