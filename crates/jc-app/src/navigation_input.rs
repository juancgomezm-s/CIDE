//! Shared frontend grammar. Source bounds/encoding remain Workbench's concern.
use jc_text::TextPosition;

/// Parse one-based `line[:column]` scalar coordinates, with a 64-byte input cap.
/// The omitted column is one. Does not clamp or inspect a source document.
pub fn parse_text_position(input: &str) -> Option<TextPosition> {
    if input.len() > 64 {
        return None;
    }
    let mut fields = input.trim().split(':');
    let line = fields.next()?.parse::<usize>().ok()?.checked_sub(1)?;
    let column = fields
        .next()
        .unwrap_or("1")
        .parse::<usize>()
        .ok()?
        .checked_sub(1)?;
    if fields.next().is_some() {
        return None;
    }
    Some(TextPosition { line, column })
}
