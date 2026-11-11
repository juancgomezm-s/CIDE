//! Bounded logical rows shared by native and terminal code comparisons.
use crate::diff_model::{DiffKind, DiffLine, DiffSpan, OwnedDiffModel};

pub struct DiffRow {
    pub left: Option<DiffLine>,
    pub right: Option<DiffLine>,
    pub changed: bool,
}

pub struct DiffPage {
    pub rows: Vec<DiffRow>,
    pub first: usize,
    pub total: usize,
}

fn count(span: &DiffSpan, parallel: bool) -> usize {
    let left = span.left.end.0 - span.left.start.0;
    let right = span.right.end.0 - span.right.start.0;
    if !parallel && span.kind == DiffKind::Changed {
        left + right
    } else {
        left.max(right)
    }
}

/// Navigate accepted change metadata without decoding source text.
pub fn change_target(
    model: &OwnedDiffModel,
    current: usize,
    forward: bool,
    parallel: bool,
) -> usize {
    let mut row = 0;
    let mut previous = None;
    for span in model.spans() {
        if span.kind == DiffKind::Changed {
            if forward && row > current {
                return row;
            }
            if row < current {
                previous = Some(row);
            }
        }
        row += count(span, parallel);
    }
    if forward {
        current
    } else {
        previous.unwrap_or(current)
    }
}

pub fn first_change(model: &OwnedDiffModel, parallel: bool) -> Option<usize> {
    let mut row = 0;
    for span in model.spans() {
        if span.kind == DiffKind::Changed {
            return Some(row);
        }
        row += count(span, parallel);
    }
    None
}

pub fn page(model: &OwnedDiffModel, first: usize, limit: usize, parallel: bool) -> DiffPage {
    let limit = limit.min(128);
    let total = model
        .spans()
        .iter()
        .map(|span| count(span, parallel))
        .sum::<usize>();
    let first = first.min(total.saturating_sub(limit));
    let mut result = DiffPage {
        rows: Vec::with_capacity(limit),
        first,
        total,
    };
    let mut skip = first;
    for span in model.spans() {
        let rows = count(span, parallel);
        if skip >= rows {
            skip -= rows;
            continue;
        }
        let left = span.left.end.0 - span.left.start.0;
        let right = span.right.end.0 - span.right.start.0;
        let changed = span.kind == DiffKind::Changed;
        for offset in skip..rows {
            if result.rows.len() == limit {
                return result;
            }
            let right_offset = if !parallel && changed {
                offset.saturating_sub(left)
            } else {
                offset
            };
            result.rows.push(DiffRow {
                left: (offset < left).then_some(DiffLine(span.left.start.0 + offset)),
                right: (right_offset < right && (parallel || !changed || offset >= left))
                    .then_some(DiffLine(span.right.start.0 + right_offset)),
                changed,
            });
        }
        skip = 0;
    }
    result
}
