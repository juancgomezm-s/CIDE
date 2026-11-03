//! Narrow cluster widths, independent of byte/scalar/LSP coordinates. Font/bidi
//! shaping and terminal-specific ligatures across clusters are not inferred.
pub use unicode_segmentation::{GraphemeCursor, GraphemeIncomplete, UnicodeSegmentation};
use unicode_width::UnicodeWidthStr;
pub const MAX_CLUSTER_BYTES: usize = 1024;
pub fn graphemes(text: &str) -> impl DoubleEndedIterator<Item = &str> {
    text.graphemes(true)
}
pub fn width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}
pub fn previous_boundary(text: &str, byte: usize) -> Option<usize> {
    if byte > text.len() || !text.is_char_boundary(byte) {
        return None;
    }
    text.grapheme_indices(true)
        .map(|(i, _)| i)
        .take_while(|&i| i < byte)
        .last()
}
pub fn next_boundary(text: &str, byte: usize) -> Option<usize> {
    if byte > text.len() || !text.is_char_boundary(byte) {
        return None;
    }
    text.grapheme_indices(true)
        .map(|(i, g)| i + g.len())
        .find(|&end| end > byte)
}
pub fn clip(text: &str, columns: usize) -> String {
    let mut result = String::new();
    let mut used = 0;
    for cluster in graphemes(text) {
        let cells = width(cluster);
        if cluster.len() > MAX_CLUSTER_BYTES || cells > columns.saturating_sub(used) {
            break;
        }
        result.push_str(cluster);
        used += cells;
    }
    result
}
