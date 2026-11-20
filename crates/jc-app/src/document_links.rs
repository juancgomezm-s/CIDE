//! Local inline Markdown links delivered through the source task controller.
use crate::{
    source_services::{FileResolution, SourceAction, SourceItem, SourceProviderDescriptor},
    Cancellation, TaskFailure, TaskItem, TaskOutput, TaskSnapshot,
};
use jc_languages::syntax::{scan, LexerState, SyntaxKind, SyntaxProfile};

pub(crate) const ID: &str = "core.markdown";

pub(crate) fn descriptor() -> SourceProviderDescriptor {
    SourceProviderDescriptor {
        id: ID,
        language: "markdown",
        symbols: None,
        references: Some(links),
        calls: None,
        callers: None,
        routines: None,
        dependencies: None,
        file_resolution: FileResolution::DocumentRelative,
        completion: None,
        rename: None,
        snippets: &[],
    }
}

fn destination(target: &str) -> Option<String> {
    let target = target
        .strip_prefix('<')
        .and_then(|s| s.strip_suffix('>'))
        .unwrap_or(target);
    let path = target.split('#').next()?;
    let mut decoded = Vec::with_capacity(path.len());
    let mut bytes = path.bytes();
    while let Some(byte) = bytes.next() {
        decoded.push(if byte == b'%' {
            let hi = char::from(bytes.next()?).to_digit(16)?;
            let lo = char::from(bytes.next()?).to_digit(16)?;
            (hi * 16 + lo) as u8
        } else {
            byte
        });
    }
    let path = String::from_utf8(decoded).ok()?;
    (!path.is_empty()
        && !path.starts_with(['/', '\\'])
        && !path.contains([':', '\\', '?', '*', '"', '<', '>', '|'])
        && !path.chars().any(char::is_control))
    .then_some(path)
}

fn links(
    snapshot: &TaskSnapshot,
    cancel: &Cancellation,
    output: &mut TaskOutput,
) -> Result<(), TaskFailure> {
    let bytes = snapshot.bytes();
    let mut state = LexerState::default();
    let mut next = 0;
    for (index, chunk) in bytes.chunks(4096).enumerate() {
        if cancel.is_cancelled() {
            return Err(TaskFailure::Cancelled);
        }
        let start = index * 4096;
        let mut result = Ok(());
        scan(
            SyntaxProfile::Markdown,
            &mut state,
            chunk,
            start,
            start + chunk.len() == bytes.len(),
            |token| {
                let at = token.source.start;
                if result.is_err()
                    || at < next
                    || token.kind != SyntaxKind::Link
                    || bytes[at] != b'['
                    || at > 0 && bytes[at - 1] == b'!'
                {
                    return;
                }
                let tail = &bytes[at + 1..bytes.len().min(at + 513)];
                let tail = &tail[..tail
                    .iter()
                    .position(|b| matches!(b, b'\r' | b'\n'))
                    .unwrap_or(tail.len())];
                let Some(label_end) = tail.iter().position(|&b| b == b']') else {
                    return;
                };
                let Some(target) = tail[label_end + 1..].strip_prefix(b"(") else {
                    return;
                };
                let Some(end) = target.iter().position(|&b| b == b')') else {
                    return;
                };
                let target_start = at + label_end + 3;
                next = target_start + end + 1;
                let decoded =
                    jc_text::decode_fragment(&target[..end], snapshot.stamp.encoding, target_start);
                if !decoded.issues.is_empty() {
                    return;
                }
                let target = decoded.text.trim();
                let action = destination(target).map_or_else(
                    || SourceAction::Unresolved("Solo se abren archivos locales relativos".into()),
                    SourceAction::RelativeFile,
                );
                let label = target
                    .chars()
                    .filter(|ch| !ch.is_control())
                    .take(120)
                    .collect::<String>();
                result = output.push(TaskItem::Source(SourceItem {
                    value_type: None,
                    range: at..next,
                    label: format!("Enlace: {label}"),
                    action,
                }));
            },
        );
        result?;
        if state.limited() {
            return Err(TaskFailure::LimitExceeded);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admits_only_relative_local_destinations() {
        assert_eq!(
            destination("../guía%20rápida.md#inicio").as_deref(),
            Some("../guía rápida.md")
        );
        for path in [
            "https://example.com",
            "%2Fetc",
            "C%3a/archivo",
            "%00.txt",
            "#inicio",
            "%ZZ",
        ] {
            assert_eq!(destination(path), None);
        }
    }
}
