//! Byte-exact single-path Git patch serialization from an accepted alignment.
use crate::{
    diff_model::{DiffKind, DiffLine, DiffSide, DiffStatus, OwnedDiffModel},
    revision::path_valid,
};
use gix::bstr::ByteSlice;
use std::{fmt, ops::Range};

const CONTEXT: usize = 3;
const MAX_PATH_BYTES: usize = 4096;
const MAX_INPUT_BYTES: usize = 4 * 1024 * 1024;
const MAX_INPUT_LINES: usize = 200_000;
const MAX_OUTPUT_BYTES: usize = 8 * 1024 * 1024;

/// `None` means that the source is absent, which differs from a present empty
/// file. Present modes are 100644 and 100755 in octal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PatchOptions {
    pub before_mode: Option<u32>,
    pub after_mode: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchError {
    InvalidPath,
    UnsupportedMode,
    InvalidPresence,
    Incomplete(DiffStatus),
    InputLimit,
    Binary,
    BareCarriageReturn,
    NoChanges,
    OutputLimit,
    Cancelled,
}

impl fmt::Display for PatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPath => f.write_str("La ruta no es segura para un parche Git"),
            Self::UnsupportedMode => f.write_str("El parche requiere archivos ordinarios"),
            Self::InvalidPresence => {
                f.write_str("La presencia no coincide con las fuentes del parche")
            }
            Self::Incomplete(status) => match status {
                DiffStatus::Limited(limit) => {
                    write!(f, "La comparación excedió el límite de {}", limit.label())
                }
                _ => f.write_str("La comparación no está completa"),
            },
            Self::InputLimit => f.write_str("Las fuentes exceden la cuota del parche"),
            Self::Binary => f.write_str("El parche de texto no admite estos bytes de control"),
            Self::BareCarriageReturn => {
                f.write_str("El parche no admite retornos de carro sin salto de línea")
            }
            Self::NoChanges => f.write_str("El par no tiene cambios para exportar"),
            Self::OutputLimit => f.write_str("El parche excede el presupuesto de salida"),
            Self::Cancelled => f.write_str("Se canceló la exportación del parche"),
        }
    }
}

impl std::error::Error for PatchError {}

fn validate_path(path: &[u8]) -> Result<(), PatchError> {
    path_valid(path, MAX_PATH_BYTES).map_err(|_| PatchError::InvalidPath)?;
    for part in path.split(|byte| *byte == b'/') {
        gix::validate::path::component(part.as_bstr(), None, Default::default())
            .map_err(|_| PatchError::InvalidPath)?;
    }
    Ok(())
}

fn check_cancelled(cancelled: &mut impl FnMut() -> bool) -> Result<(), PatchError> {
    if cancelled() {
        Err(PatchError::Cancelled)
    } else {
        Ok(())
    }
}

fn validate_source(bytes: &[u8], cancelled: &mut impl FnMut() -> bool) -> Result<(), PatchError> {
    for (block, chunk) in bytes.chunks(4096).enumerate() {
        check_cancelled(cancelled)?;
        for (index, &byte) in chunk.iter().enumerate() {
            match byte {
                b'\r' if bytes.get(block * 4096 + index + 1) != Some(&b'\n') => {
                    return Err(PatchError::BareCarriageReturn);
                }
                b'\r' | b'\n' | b'\t' | 0x0c => {}
                0x00..=0x1f | 0x7f => return Err(PatchError::Binary),
                _ => {}
            }
        }
    }
    Ok(())
}

/// Use Git's C-style path spelling. Quote spaces too so both prefixed paths in
/// the `diff --git` header can be parsed independently.
fn prefixed_path(prefix: &[u8], path: &[u8]) -> Vec<u8> {
    if path.iter().all(|byte| (b'!'..=b'~').contains(byte)) {
        return [prefix, path].concat();
    }
    let mut quoted = Vec::with_capacity(2 + prefix.len() + path.len() * 4);
    quoted.push(b'"');
    quoted.extend_from_slice(prefix);
    for &byte in path {
        if (b' '..=b'~').contains(&byte) {
            quoted.push(byte);
        } else {
            quoted.extend_from_slice(&[
                b'\\',
                b'0' + (byte >> 6),
                b'0' + ((byte >> 3) & 7),
                b'0' + (byte & 7),
            ]);
        }
    }
    quoted.push(b'"');
    quoted
}

fn append(output: &mut Vec<u8>, bytes: &[u8]) -> Result<(), PatchError> {
    let target = output
        .len()
        .checked_add(bytes.len())
        .filter(|&length| length <= MAX_OUTPUT_BYTES)
        .ok_or(PatchError::OutputLimit)?;
    if target > output.capacity() {
        let capacity = output
            .capacity()
            .saturating_mul(2)
            .clamp(256, MAX_OUTPUT_BYTES);
        output
            .try_reserve_exact(capacity.max(target) - output.len())
            .map_err(|_| PatchError::OutputLimit)?;
    }
    output.extend_from_slice(bytes);
    Ok(())
}

struct Emitter<'a, F> {
    output: Vec<u8>,
    model: &'a OwnedDiffModel,
    cancelled: F,
}

impl<F: FnMut() -> bool> Emitter<'_, F> {
    fn append(&mut self, bytes: &[u8]) -> Result<(), PatchError> {
        append(&mut self.output, bytes)
    }

    fn lines(&mut self, side: DiffSide, prefix: u8, lines: Range<usize>) -> Result<(), PatchError> {
        for line in lines {
            check_cancelled(&mut self.cancelled)?;
            let bytes = self
                .model
                .line_bytes(side, DiffLine(line))
                .ok_or(PatchError::Incomplete(self.model.status()))?;
            append(&mut self.output, &[prefix])?;
            append(&mut self.output, bytes)?;
            if !bytes.ends_with(b"\n") {
                self.append(b"\n\\ No newline at end of file\n")?;
            }
        }
        Ok(())
    }

    fn hunks(&mut self) -> Result<(), PatchError> {
        let spans = self.model.spans();
        let mut changes = spans
            .iter()
            .enumerate()
            .filter(|(_, span)| span.kind == DiffKind::Changed)
            .peekable();
        while let Some((first_index, first)) = changes.next() {
            check_cancelled(&mut self.cancelled)?;
            let (mut last_index, mut last) = (first_index, first);
            // The three context lines from each hunk touch or overlap. Keep
            // that shared context in one hunk to avoid duplicate source lines.
            while let Some(&(index, next)) = changes.peek() {
                if next.left.start.0 - last.left.end.0 > CONTEXT * 2 {
                    break;
                }
                let _ = changes.next();
                last_index = index;
                last = next;
            }
            let lead = first_index.checked_sub(1).map_or(0, |index| {
                let equal = &spans[index].left;
                CONTEXT.min(equal.end.0 - equal.start.0)
            });
            let tail = spans
                .get(last_index + 1)
                .map_or(0, |span| CONTEXT.min(span.left.end.0 - span.left.start.0));
            let old_start = first.left.start.0 - lead;
            let new_start = first.right.start.0 - lead;
            let old_end = last.left.end.0 + tail;
            let new_end = last.right.end.0 + tail;
            let old_count = old_end - old_start;
            let new_count = new_end - new_start;
            // A zero-line range identifies the line before the insertion or
            // deletion. Nonempty ranges identify their first line, one-based.
            self.append(
                format!(
                    "@@ -{},{old_count} +{},{new_count} @@\n",
                    old_start + usize::from(old_count != 0),
                    new_start + usize::from(new_count != 0),
                )
                .as_bytes(),
            )?;
            self.lines(DiffSide::Left, b' ', old_start..first.left.start.0)?;
            for span in &spans[first_index..=last_index] {
                let left = span.left.start.0..span.left.end.0;
                match span.kind {
                    DiffKind::Equal => self.lines(DiffSide::Left, b' ', left)?,
                    DiffKind::Changed => {
                        self.lines(DiffSide::Left, b'-', left)?;
                        self.lines(DiffSide::Right, b'+', span.right.start.0..span.right.end.0)?;
                    }
                }
            }
            self.lines(DiffSide::Left, b' ', last.left.end.0..old_end)?;
        }
        Ok(())
    }
}

/// Serialize the exact sources and alignment already present in `model`.
/// Sources may contain non-UTF-8 bytes. Payload CRLF/LF and missing final LF are
/// preserved, while metadata lines use LF. The returned bytes should be saved
/// without decoding or changing line endings. Call outside the paint path.
pub fn export_unified(
    path: &[u8],
    model: &OwnedDiffModel,
    options: PatchOptions,
    mut cancelled: impl FnMut() -> bool,
) -> Result<Vec<u8>, PatchError> {
    check_cancelled(&mut cancelled)?;
    validate_path(path)?;
    match model.status() {
        DiffStatus::Exact => {}
        DiffStatus::Cancelled => return Err(PatchError::Cancelled),
        status => return Err(PatchError::Incomplete(status)),
    }
    let before = model.source_bytes(DiffSide::Left);
    let after = model.source_bytes(DiffSide::Right);
    let before_mode = options.before_mode;
    let after_mode = options.after_mode;
    if before_mode.is_none() && after_mode.is_none()
        || before_mode.is_none() && !before.is_empty()
        || after_mode.is_none() && !after.is_empty()
    {
        return Err(PatchError::InvalidPresence);
    }
    if [before_mode, after_mode]
        .into_iter()
        .flatten()
        .any(|mode| !matches!(mode, 0o100644 | 0o100755))
    {
        return Err(PatchError::UnsupportedMode);
    }
    let lines = model
        .line_count(DiffSide::Left)
        .unwrap_or_default()
        .saturating_add(model.line_count(DiffSide::Right).unwrap_or_default());
    if before.len().saturating_add(after.len()) > MAX_INPUT_BYTES || lines > MAX_INPUT_LINES {
        return Err(PatchError::InputLimit);
    }
    let content_changes = model.hunks().next().is_some();
    if !content_changes && before_mode == after_mode {
        return Err(PatchError::NoChanges);
    }
    validate_source(before, &mut cancelled)?;
    validate_source(after, &mut cancelled)?;

    let old_path = prefixed_path(b"a/", path);
    let new_path = prefixed_path(b"b/", path);
    let mut emitter = Emitter {
        output: Vec::new(),
        model,
        cancelled,
    };
    for part in [b"diff --git ".as_slice(), &old_path, b" ", &new_path, b"\n"] {
        emitter.append(part)?;
    }
    match (before_mode, after_mode) {
        (None, Some(mode)) => emitter.append(format!("new file mode {mode:o}\n").as_bytes())?,
        (Some(mode), None) => emitter.append(format!("deleted file mode {mode:o}\n").as_bytes())?,
        (Some(old), Some(new)) if old != new => {
            emitter.append(format!("old mode {old:o}\nnew mode {new:o}\n").as_bytes())?;
        }
        _ => {}
    }
    if content_changes {
        let before = before_mode.map_or(b"/dev/null".as_slice(), |_| &old_path);
        let after = after_mode.map_or(b"/dev/null".as_slice(), |_| &new_path);
        for part in [b"--- ".as_slice(), before, b"\n+++ ", after, b"\n"] {
            emitter.append(part)?;
        }
        emitter.hunks()?;
    }
    check_cancelled(&mut emitter.cancelled)?;
    Ok(emitter.output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff_model::{DiffLimit, DiffLimits};
    use std::sync::Arc;

    const REGULAR: Option<u32> = Some(0o100644);
    const UPDATE: PatchOptions = PatchOptions {
        before_mode: REGULAR,
        after_mode: REGULAR,
    };

    fn model(before: &[u8], after: &[u8]) -> OwnedDiffModel {
        OwnedDiffModel::compute(
            Arc::new(before.to_vec()),
            Arc::new(after.to_vec()),
            DiffLimits::default(),
            || false,
        )
    }

    #[test]
    fn preserves_crlf_legacy_bytes_and_missing_final_newline() {
        let model = model(b"x\r\n\x80old", b"x\r\n\x80new\r\n");
        assert_eq!(
            export_unified(b"f", &model, UPDATE, || false).unwrap(),
            b"diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1,2 +1,2 @@\n x\r\n-\x80old\n\\ No newline at end of file\n+\x80new\r\n"
        );
    }

    #[test]
    fn merges_touching_context_and_keeps_separate_hunk_coordinates() {
        let joined = model(
            b"left\nold first\na\nb\nc\nd\ne\nf\nold second\nright\n",
            b"left\nnew first\na\nb\nc\nd\ne\nf\nnew second\nright\n",
        );
        let patch = export_unified(b"f", &joined, UPDATE, || false).unwrap();
        assert!(patch.ends_with(
            b"@@ -1,10 +1,10 @@\n left\n-old first\n+new first\n a\n b\n c\n d\n e\n f\n-old second\n+new second\n right\n"
        ));
        let separate = model(
            b"left\nold first\na\nb\nc\nd\ne\nf\ng\nold second\nright\n",
            b"left\nnew first\ninserted\na\nb\nc\nd\ne\nf\ng\nright\n",
        );
        let patch = export_unified(b"f", &separate, UPDATE, || false).unwrap();
        assert!(patch.ends_with(
            b"@@ -1,5 +1,6 @@\n left\n-old first\n+new first\n+inserted\n a\n b\n c\n@@ -7,5 +8,4 @@\n e\n f\n g\n-old second\n right\n"
        ));
    }

    #[test]
    fn distinguishes_empty_files_from_absence_and_handles_mode_changes() {
        let empty = model(b"", b"");
        let create = PatchOptions {
            before_mode: None,
            after_mode: REGULAR,
        };
        let delete = PatchOptions {
            before_mode: REGULAR,
            after_mode: None,
        };
        assert_eq!(
            export_unified(b"f", &empty, create, || false).unwrap(),
            b"diff --git a/f b/f\nnew file mode 100644\n"
        );
        assert_eq!(
            export_unified(b"f", &empty, delete, || false).unwrap(),
            b"diff --git a/f b/f\ndeleted file mode 100644\n"
        );
        assert_eq!(
            export_unified(b"f", &empty, UPDATE, || false),
            Err(PatchError::NoChanges)
        );
        let executable = PatchOptions {
            before_mode: REGULAR,
            after_mode: Some(0o100755),
        };
        let identical = model(b"abc\n", b"abc\n");
        assert_eq!(
            export_unified(b"f", &identical, executable, || false).unwrap(),
            b"diff --git a/f b/f\nold mode 100644\nnew mode 100755\n"
        );
        let nonempty = model(b"", b"data\n");
        assert_eq!(
            export_unified(b"f", &nonempty, create, || false).unwrap(),
            b"diff --git a/f b/f\nnew file mode 100644\n--- /dev/null\n+++ b/f\n@@ -0,0 +1,1 @@\n+data\n"
        );
    }

    #[test]
    fn quotes_non_ascii_and_space_and_rejects_unsafe_path_components() {
        let model = model(b"old\n", b"new\n");
        let patch = export_unified("d/café x.txt".as_bytes(), &model, UPDATE, || false).unwrap();
        assert!(patch.starts_with(
            b"diff --git \"a/d/caf\\303\\251 x.txt\" \"b/d/caf\\303\\251 x.txt\"\n--- \"a/d/caf\\303\\251 x.txt\"\n+++ \"b/d/caf\\303\\251 x.txt\"\n"
        ));
        for path in [
            b"../f".as_slice(),
            b"/f",
            b"d//f",
            b"d\\f",
            b"d/aux.txt",
            b"d/f:stream",
            b"d/.GiT/f",
            b"d/git~1/f",
            b"d/f\n--- injected",
        ] {
            assert_eq!(
                export_unified(path, &model, UPDATE, || false),
                Err(PatchError::InvalidPath),
                "{path:?}"
            );
        }
    }

    #[test]
    fn rejects_unrepresentable_and_incomplete_sources() {
        let unsupported = PatchOptions {
            before_mode: Some(0o120000),
            after_mode: REGULAR,
        };
        let absent = PatchOptions {
            before_mode: None,
            after_mode: REGULAR,
        };
        let changed = model(b"old\n", b"new\n");
        assert_eq!(
            export_unified(b"f", &changed, unsupported, || false),
            Err(PatchError::UnsupportedMode)
        );
        assert_eq!(
            export_unified(b"f", &changed, absent, || false),
            Err(PatchError::InvalidPresence)
        );
        assert_eq!(
            export_unified(b"f", &model(b"a\0", b"b"), UPDATE, || false),
            Err(PatchError::Binary)
        );
        assert_eq!(
            export_unified(b"f", &model(b"a\rb", b"b"), UPDATE, || false),
            Err(PatchError::BareCarriageReturn)
        );
        let limited = OwnedDiffModel::compute(
            Arc::new(vec![b'a']),
            Arc::new(vec![b'b']),
            DiffLimits {
                max_input_bytes: 1,
                ..DiffLimits::default()
            },
            || false,
        );
        assert_eq!(
            export_unified(b"f", &limited, UPDATE, || false),
            Err(PatchError::Incomplete(DiffStatus::Limited(
                DiffLimit::InputBytes
            )))
        );
    }

    #[test]
    fn cancellation_during_emission_returns_no_partial_patch() {
        let changed = model(b"context\nold\n", b"context\nnew\n");
        let mut checks = 0;
        assert_eq!(
            export_unified(b"f", &changed, UPDATE, || {
                checks += 1;
                checks > 5
            }),
            Err(PatchError::Cancelled)
        );
    }
}
