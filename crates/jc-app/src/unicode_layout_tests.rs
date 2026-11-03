use super::source_graphemes::SourceGraphemes;
use super::unicode_layout::*;
use jc_core::Document;
use jc_text::{grapheme, TextEncoding};
fn boundaries(text: &str) -> Vec<usize> {
    let mut out = vec![0];
    let mut end = 0;
    for g in grapheme::graphemes(text) {
        end += g.len();
        out.push(end);
    }
    out
}
fn check(text: &str) {
    let doc = Document::from_bytes(text.as_bytes().to_vec());
    let expected = boundaries(text);
    let mut cursor = SourceGraphemes::new(&doc, 0..doc.len(), 0).unwrap();
    let mut actual = vec![0];
    while let Some(byte) = cursor.adjacent(true).unwrap() {
        actual.push(byte);
    }
    assert_eq!(actual, expected);
    let mut cursor = SourceGraphemes::new(&doc, 0..doc.len(), doc.len()).unwrap();
    let mut actual = vec![doc.len()];
    while let Some(byte) = cursor.adjacent(false).unwrap() {
        actual.push(byte);
    }
    assert_eq!(actual, expected.into_iter().rev().collect::<Vec<_>>());
}
#[test]
fn unicode_range_cursor_matches_uax29_through_fragments_long_clusters_and_regional_context() {
    for shift in 0..4 {
        let mut text = "a".repeat(4096 - shift);
        text.push_str("e\u{301}界😀👨‍👩‍👧‍👦🇲🇽\r\n");
        text.push_str(&"🇲🇽🇺🇸".repeat(700));
        text.push_str("\rZ");
        check(&text);
    }
    check(&format!("a{}Z", "\u{301}".repeat(4000)));
}
#[test]
fn unicode_invalid_source_groups_and_literal_bom_remain_atomic_without_lossy_decode() {
    let bytes = b"A\xe1\x80B\xf0\x9f\xef\xbb\xbfZ".to_vec();
    let doc = Document::from_bytes(bytes.clone());
    let mut cursor = SourceGraphemes::new(&doc, 0..doc.len(), 0).unwrap();
    let mut got = vec![0];
    while let Some(byte) = cursor.adjacent(true).unwrap() {
        got.push(byte);
    }
    assert_eq!(got, [0, 1, 3, 4, 6, 9, 10]);
    let mut cursor = SourceGraphemes::new(&doc, 0..doc.len(), doc.len()).unwrap();
    for expected in got[..got.len() - 1].iter().rev() {
        assert_eq!(cursor.adjacent(false).unwrap(), Some(*expected));
    }
    assert_eq!(doc.to_bytes(), bytes);
}
#[test]
fn unicode_visual_columns_cells_clipping_tabs_and_complete_source_ranges() {
    let text = "Ae\u{301}界😀👨‍👩‍👧‍👦🇲🇽\tZ";
    let doc = Document::from_bytes(text.as_bytes().to_vec());
    let mut layout = UnicodeLayout::default();
    let range = 0..doc.len();
    let row = layout
        .row(
            &doc,
            TextEncoding::Utf8,
            range.clone(),
            0,
            30,
            RowMode::Plain,
        )
        .unwrap();
    assert_eq!(
        row.iter().map(|a| (a.column, a.width)).collect::<Vec<_>>(),
        [
            (0, 1),
            (1, 1),
            (2, 2),
            (4, 2),
            (6, 2),
            (8, 2),
            (10, 1),
            (11, 1),
            (12, 1)
        ]
    );
    assert_eq!(row[1].source, 1..4);
    assert_eq!(row[2].source, 4..7);
    assert_eq!(
        layout
            .byte_at(&doc, TextEncoding::Utf8, range.clone(), 3)
            .unwrap(),
        4
    );
    assert_eq!(
        layout
            .column(&doc, TextEncoding::Utf8, range.clone(), 2)
            .unwrap(),
        1
    );
    let clipped = layout
        .row(&doc, TextEncoding::Utf8, range, 3, 2, RowMode::Plain)
        .unwrap();
    assert_eq!(
        (
            clipped[0].column,
            clipped[0].width,
            clipped[0].glyph.clone()
        ),
        (0, 1, Glyph::Scalar(' '))
    );
    assert!(clipped.iter().all(|a| a.width == 1));
}
#[test]
fn unicode_cache_warm_ranges_and_suffix_edits_stay_bounded_and_match_cold_reference() {
    let mut doc = Document::from_bytes("a".repeat(512 * 1024).into_bytes());
    let mut layout = UnicodeLayout::default();
    layout
        .row(
            &doc,
            TextEncoding::Utf8,
            0..doc.len(),
            doc.len() - 40,
            40,
            RowMode::Plain,
        )
        .unwrap();
    let cold = layout.scanned_bytes;
    layout
        .row(
            &doc,
            TextEncoding::Utf8,
            0..doc.len(),
            doc.len() - 40,
            40,
            RowMode::Plain,
        )
        .unwrap();
    assert!(layout.scanned_bytes - cold < 16 * 1024);
    assert!(layout.memory_bytes() <= CACHE_BYTES);
    let at = doc.len() - 10;
    doc.replace(at..at, "界e\u{301}".as_bytes()).unwrap();
    let before = layout.scanned_bytes;
    let warm = layout
        .row(
            &doc,
            TextEncoding::Utf8,
            0..doc.len(),
            at - 2,
            20,
            RowMode::Plain,
        )
        .unwrap();
    assert!(
        layout.scanned_bytes - before < 64 * 1024,
        "suffix update rescanned prefix"
    );
    let reference = UnicodeLayout::default()
        .row(
            &doc,
            TextEncoding::Utf8,
            0..doc.len(),
            at - 2,
            20,
            RowMode::Plain,
        )
        .unwrap();
    assert_eq!(
        warm.iter()
            .map(|a| (&a.glyph, a.column, a.width, a.source.clone()))
            .collect::<Vec<_>>(),
        reference
            .iter()
            .map(|a| (&a.glyph, a.column, a.width, a.source.clone()))
            .collect::<Vec<_>>()
    );
    assert!(doc.undo());
    let width = layout
        .column(&doc, TextEncoding::Utf8, 0..doc.len(), doc.len())
        .unwrap();
    assert_eq!(width, 512 * 1024);
}
#[test]
fn unicode_visual_escape_budget_and_legacy_mapping_are_explicit() {
    let doc = Document::from_bytes(vec![0xe1, 0x80, b'A', 0]);
    let mut layout = UnicodeLayout::default();
    let row = layout
        .row(&doc, TextEncoding::Utf8, 0..4, 0, 100, RowMode::Plain)
        .unwrap();
    let mut displayed = String::new();
    for atom in row {
        atom.glyph.with_str(|s| displayed.push_str(s));
    }
    assert_eq!(displayed, "\\xE1\\x80A\\u{0}");
    assert_eq!(doc.to_bytes(), [0xe1, 0x80, b'A', 0]);
    let doc = Document::from_bytes(vec![0xe9, b'\t', 0x80]);
    let row = layout
        .row(&doc, TextEncoding::Windows1252, 0..3, 0, 10, RowMode::Plain)
        .unwrap();
    assert_eq!(row.last().unwrap().source, 2..3);
    assert_eq!(row.last().unwrap().column, 4);
    let source = format!("e{}", "\u{301}".repeat(1000));
    let doc = Document::from_bytes(source.as_bytes().to_vec());
    let row = UnicodeLayout::default()
        .row(
            &doc,
            TextEncoding::Utf8,
            0..doc.len(),
            0,
            40,
            RowMode::Plain,
        )
        .unwrap();
    assert!(row[0].marker);
    assert_eq!(row[0].source, 0..doc.len());
}

#[test]
fn unicode_cold_prefix_is_lazy_and_large_cluster_row_has_explicit_text_quota() {
    let doc = Document::from_bytes(vec![b'a'; 4 * 1024 * 1024]);
    let mut layout = UnicodeLayout::default();
    layout
        .row(
            &doc,
            TextEncoding::Utf8,
            0..doc.len(),
            0,
            80,
            RowMode::Plain,
        )
        .unwrap();
    assert!(layout.scanned_bytes < 4096);
    assert!(layout.memory_bytes() < 4096);
    let cluster = format!("e{}", "\u{301}".repeat(511));
    let text = cluster.repeat(70);
    let doc = Document::from_bytes(text.as_bytes().to_vec());
    let mut layout = UnicodeLayout::default();
    assert!(layout
        .row(
            &doc,
            TextEncoding::Utf8,
            0..doc.len(),
            0,
            80,
            RowMode::Plain
        )
        .unwrap_err()
        .contains("64 KiB"));
    assert!(layout
        .row(&doc, TextEncoding::Utf8, 0..doc.len(), 0, 5, RowMode::Plain)
        .is_ok());
    assert_eq!(doc.to_bytes(), text.as_bytes());
    assert!(layout.memory_bytes() <= CACHE_BYTES);
}
