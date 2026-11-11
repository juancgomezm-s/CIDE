//! One bounded row projection for buffer, committed and local Git comparisons.
use super::*;
use jc_app::diff_model::OwnedDiffModel;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum DiffView {
    #[default]
    Unified,
    Parallel,
}
const SOURCE: usize = 64 * 1024;
const COLUMNS: usize = 16384;
const VISIBLE: usize = 512;
const TEXT: usize = 4096;

/// Interpret a bounded source prefix without modifying or cloning the source.
/// Unicode clipping is atomic per grapheme; a partially hidden wide cluster is
/// represented by spaces, keeping subsequent text at its original visual column.
pub(in crate::windows) fn text(
    bytes: &[u8],
    encoding: TextEncoding,
    scroll: usize,
) -> (String, usize) {
    let bytes = bytes
        .strip_suffix(b"\r\n")
        .or_else(|| bytes.strip_suffix(b"\n"))
        .or_else(|| bytes.strip_suffix(b"\r"))
        .unwrap_or(bytes);
    let mut limited = bytes.len() > SOURCE;
    let mut decoded = String::new();
    for scalar in DecodedScalars::new(bytes, encoding, 0) {
        if scalar.source_range.end > SOURCE {
            break;
        }
        if !scalar.valid {
            for byte in &bytes[scalar.source_range] {
                decoded.push_str(&format!("\\x{byte:02X}"));
            }
        } else if scalar.character == '\t' || !scalar.character.is_control() {
            decoded.push(scalar.character);
        } else {
            decoded.push_str(&format!("\\u{{{:X}}}", scalar.character as u32));
        }
        // Even combining-only input has a fixed source/decode ceiling.
        if decoded.len() >= SOURCE * 4 {
            break;
        }
    }
    if bytes.len() > SOURCE {
        // The next source scalar may extend the final cluster (combining mark,
        // ZWJ sequence, etc.). Never publish that potentially partial cluster.
        let trim = jc_text::grapheme::graphemes(&decoded)
            .next_back()
            .map_or(0, str::len);
        decoded.truncate(decoded.len() - trim);
    }
    let scroll = scroll.min(COLUMNS);
    let end = scroll.saturating_add(VISIBLE).min(COLUMNS);
    let mut column = 0usize;
    let mut output = String::new();
    for cluster in jc_text::grapheme::graphemes(&decoded) {
        if column >= COLUMNS {
            limited = true;
            break;
        }
        let width = if cluster == "\t" {
            4 - column % 4
        } else {
            jc_text::grapheme::width(cluster)
        };
        let next = column.saturating_add(width);
        if next > scroll && column < end {
            if cluster == "\t" || column < scroll || next > end {
                let visible = next.min(end).saturating_sub(column.max(scroll));
                if output.len() + visible > TEXT {
                    limited = true;
                } else {
                    output.extend(std::iter::repeat_n(' ', visible));
                }
            } else if cluster.len() <= jc_text::grapheme::MAX_CLUSTER_BYTES
                && output.len() + cluster.len() <= TEXT
            {
                output.push_str(cluster);
            } else {
                limited = true;
            }
        }
        column = next;
    }
    if column > COLUMNS {
        limited = true;
    }
    if limited {
        const MARKER: &str = "[fila limitada] ";
        // Keep whole clusters when reserving the visible warning prefix.
        let keep = jc_text::grapheme::graphemes(&output)
            .scan(0usize, |used, cluster| {
                *used += cluster.len();
                Some(*used)
            })
            .take_while(|end| *end <= TEXT - MARKER.len())
            .last()
            .unwrap_or(0);
        output.truncate(keep);
        output.insert_str(0, MARKER);
    }
    (output, column.min(COLUMNS))
}

#[allow(clippy::too_many_arguments)]
fn cell(
    model: &OwnedDiffModel,
    side: DiffSide,
    line: Option<usize>,
    kind: i8,
    area: Rect,
    encoding: TextEncoding,
    scroll: usize,
    parallel: bool,
) -> (CodeRow, usize) {
    let (value, extent) = line
        .and_then(|line| model.line_byte_range(side, DiffLine(line)))
        .map(|range| text(&model.source_bytes(side)[range], encoding, scroll))
        .unwrap_or_default();
    let number = line.map_or(String::new(), |line| line.saturating_add(1).to_string());
    (
        CodeRow {
            rect: area,
            old: if side == DiffSide::Left {
                number.clone()
            } else {
                String::new()
            },
            new: if side == DiffSide::Right {
                number
            } else {
                String::new()
            },
            text: value,
            kind: if line.is_some() { kind } else { 0 },
            parallel,
        },
        extent,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn rows(
    model: &OwnedDiffModel,
    hunk: usize,
    first: usize,
    page: usize,
    encoding: TextEncoding,
    geometry: (Rect, i32, i32),
    mode: DiffView,
    hscroll: usize,
) -> (Vec<CodeRow>, usize, usize, usize) {
    let (area, top, height) = geometry;
    let Some(change) = model.hunks().nth(hunk) else {
        return (Vec::new(), 0, 0, 0);
    };
    let spans = model.spans();
    let at = spans
        .iter()
        .position(|s| std::ptr::eq(s, change))
        .unwrap_or(0);
    let before = at
        .checked_sub(1)
        .and_then(|i| spans.get(i))
        .map_or(0, |s| (s.left.end.0 - s.left.start.0).min(3));
    let after = spans
        .get(at + 1)
        .map_or(0, |s| (s.left.end.0 - s.left.start.0).min(3));
    let left = change.left.end.0 - change.left.start.0;
    let right = change.right.end.0 - change.right.start.0;
    let changed = if mode == DiffView::Parallel {
        left.max(right)
    } else {
        left.saturating_add(right)
    };
    let total = before.saturating_add(changed).saturating_add(after);
    let page = page.min(128);
    if area.right <= area.left || height <= 0 || top >= area.bottom {
        return (Vec::new(), first.min(total.saturating_sub(page)), total, 0);
    }
    let page = page.min(
        ((area.bottom as i64 - top as i64 + height as i64 - 1) / height as i64).max(0) as usize,
    );
    let first = first.min(total.saturating_sub(page));
    let mut result = Vec::new();
    let mut extent = 0;
    let middle = area
        .left
        .saturating_add((area.right as i64 - area.left as i64).div_euclid(2) as i32);
    for (ordinal, n) in (first..total).take(page).enumerate() {
        let y = top.saturating_add((ordinal as i32).saturating_mul(height));
        let r = rect(
            area.left,
            y,
            area.right,
            y.saturating_add(height).min(area.bottom),
        );
        let (old, new, kind) = if n < before {
            (
                Some(change.left.start.0 - before + n),
                Some(change.right.start.0 - before + n),
                0,
            )
        } else if n < before + changed {
            let offset = n - before;
            if mode == DiffView::Parallel {
                (
                    (offset < left).then(|| change.left.start.0 + offset),
                    (offset < right).then(|| change.right.start.0 + offset),
                    1,
                )
            } else if offset < left {
                (Some(change.left.start.0 + offset), None, -1)
            } else {
                (None, Some(change.right.start.0 + offset - left), 1)
            }
        } else {
            let offset = n - before - changed;
            (
                Some(change.left.end.0 + offset),
                Some(change.right.end.0 + offset),
                0,
            )
        };
        if mode == DiffView::Parallel {
            let (left_cell, left_extent) = cell(
                model,
                DiffSide::Left,
                old,
                if kind == 0 { 0 } else { -1 },
                rect(r.left, r.top, middle, r.bottom),
                encoding,
                hscroll,
                true,
            );
            let (right_cell, right_extent) = cell(
                model,
                DiffSide::Right,
                new,
                kind,
                rect(middle, r.top, r.right, r.bottom),
                encoding,
                hscroll,
                true,
            );
            extent = extent.max(left_extent).max(right_extent);
            result.push(left_cell);
            result.push(right_cell);
        } else {
            let side = if new.is_some() {
                DiffSide::Right
            } else {
                DiffSide::Left
            };
            let (mut row, row_extent) =
                cell(model, side, new.or(old), kind, r, encoding, hscroll, false);
            extent = extent.max(row_extent);
            row.old = old.map_or(String::new(), |n| n.saturating_add(1).to_string());
            row.new = new.map_or(String::new(), |n| n.saturating_add(1).to_string());
            result.push(row);
        }
    }
    (result, first, total, extent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jc_app::diff_model::DiffLimits;
    use std::sync::Arc;
    fn model(a: &[u8], b: &[u8]) -> OwnedDiffModel {
        OwnedDiffModel::compute(
            Arc::new(a.to_vec()),
            Arc::new(b.to_vec()),
            DiffLimits::default(),
            || false,
        )
    }
    fn project(m: &OwnedDiffModel, mode: DiffView) -> Vec<CodeRow> {
        rows(
            m,
            0,
            0,
            128,
            TextEncoding::Utf8,
            (rect(10, 0, 810, 10000), 100, 18),
            mode,
            0,
        )
        .0
    }
    #[test]
    fn parallel_aligns_unbalanced_changes_and_context_with_explicit_blank_cells() {
        let m = model(
            b"antes\r\nviejo\r\ndespues\r\n",
            b"antes\r\nnuevo\r\nextra\r\ndespues\r\n",
        );
        let r = project(&m, DiffView::Parallel);
        assert_eq!(r.len(), 8);
        assert!(r.iter().all(|r| r.parallel));
        assert_eq!((r[0].text.as_str(), r[1].text.as_str()), ("antes", "antes"));
        assert_eq!((r[2].kind, r[3].kind), (-1, 1));
        assert_eq!((&*r[4].text, &*r[5].text), ("", "extra"));
        assert!(r[4].old.is_empty() && r[4].new.is_empty());
        assert_eq!(r[4].kind, 0);
        assert_eq!(r[4].rect.right, r[5].rect.left);
        assert_eq!(r[4].rect.top, r[5].rect.top);
        assert_eq!((&*r[6].text, &*r[7].text), ("despues", "despues"));
        let unified = project(&m, DiffView::Unified);
        assert_eq!(unified.len(), 5);
        assert!(unified.iter().all(|r| !r.parallel));
        assert_eq!(
            unified.iter().map(|r| r.kind).collect::<Vec<_>>(),
            vec![0, -1, 1, 1, 0]
        );
        assert_eq!(
            m.source_bytes(DiffSide::Left),
            b"antes\r\nviejo\r\ndespues\r\n"
        );
    }
    #[test]
    fn addition_and_deletion_keep_blank_side_and_invalid_bytes_visible() {
        for added in [true, false] {
            let bytes = b"uno\r\ndos\xff\n";
            let m = if added {
                model(b"", bytes)
            } else {
                model(bytes, b"")
            };
            let r = project(&m, DiffView::Parallel);
            assert_eq!(r.len(), 4);
            let (empty, real) = if added { (2, 3) } else { (3, 2) };
            assert_eq!(r[empty].kind, 0);
            assert!(r[empty].text.is_empty());
            assert_eq!(r[real].text, "dos\\xFF");
            assert_eq!(r[real].kind, if added { 1 } else { -1 });
        }
    }
    #[test]
    fn horizontal_scroll_uses_visual_tabs_graphemes_and_bounded_source() {
        for tail in ["e\u{301}", "👩\u{200d}💻"] {
            let prefix_bytes = if tail.starts_with('e') {
                SOURCE - 2
            } else {
                SOURCE - 6
            };
            let source = format!("{}{}", "\u{301}".repeat(prefix_bytes / 2), tail).into_bytes();
            let original = source.clone();
            let (display, _) = text(&source, TextEncoding::Utf8, 0);
            assert_eq!(display, "[fila limitada] ");
            assert_eq!(source, original);
        }
        assert_eq!(text(b"a\tb\r\n", TextEncoding::Utf8, 0).1, 5);
        assert_eq!(text("界e\u{301}z".as_bytes(), TextEncoding::Utf8, 0).1, 4);
        assert_eq!(text(b"a\tb\r\n", TextEncoding::Utf8, 2).0, "  b");
        assert_eq!(
            text("界e\u{301}z".as_bytes(), TextEncoding::Utf8, 1).0,
            " e\u{301}z"
        );
        assert_eq!(text(b"\xff\tX", TextEncoding::Utf8, 4).0, "    X");
        assert!(!text(b"x\x1b", TextEncoding::Utf8, 0).0.contains('\x1b'));
        assert!(text(&vec![b'a'; SOURCE * 2], TextEncoding::Utf8, 0).0.len() <= TEXT);
        assert!(
            text(&vec![b'a'; SOURCE * 2], TextEncoding::Utf8, usize::MAX)
                .0
                .contains("[fila limitada]")
        );
        let mut a = String::new();
        let mut b = String::new();
        for n in 0..500 {
            a.push_str(&format!("a{n}\n"));
            b.push_str(&format!("b{n}\n"));
        }
        let m = model(a.as_bytes(), b.as_bytes());
        let (r, first, total, extent) = rows(
            &m,
            0,
            usize::MAX,
            usize::MAX,
            TextEncoding::Utf8,
            (rect(0, 0, 800, 100000), 0, 18),
            DiffView::Parallel,
            0,
        );
        assert_eq!(r.len(), 256);
        assert!(extent > 0 && extent < COLUMNS);
        assert_eq!(first, total - 128);
        assert!(r.iter().all(|r| r.text.len() <= TEXT));
        let (last, first, total, _) = rows(
            &m,
            0,
            usize::MAX,
            128,
            TextEncoding::Utf8,
            (rect(0, 0, 800, 18), 0, 18),
            DiffView::Parallel,
            0,
        );
        assert_eq!(last.len(), 2);
        assert_eq!(first, total - 1);
    }

    #[test]
    fn horizontal_extent_comes_only_from_visible_rows() {
        let m = model(b"", b"corta\nesta linea es mucho mas larga\n");
        let geometry = (rect(0, 0, 800, 600), 0, 18);
        let (_, _, total, short) = rows(
            &m,
            0,
            0,
            1,
            TextEncoding::Utf8,
            geometry,
            DiffView::Parallel,
            0,
        );
        let (_, _, _, long) = rows(
            &m,
            0,
            1,
            1,
            TextEncoding::Utf8,
            geometry,
            DiffView::Parallel,
            0,
        );
        assert_eq!(total, 2);
        assert_eq!(short, 5);
        assert_eq!(long, 29);
    }
}
