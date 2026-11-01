use jc_core::{Document, EditError};
use std::io::{self, Write};

#[test]
fn preserves_all_byte_values_without_guessing_an_encoding() {
    let mut original: Vec<u8> = (0..=255).collect();
    original.extend_from_slice(b"\r\nline two\nline three\rlast without newline");
    let document = Document::from_bytes(original.clone());
    assert_eq!(document.to_bytes(), original);
    assert_eq!(document.len(), original.len());
    assert_eq!(document.piece_count(), 1);
    assert!(!document.is_modified());
    let mut streamed = Vec::new();
    document.write_to(&mut streamed).unwrap();
    assert_eq!(streamed, original);
}

#[test]
fn editing_legacy_bytes_keeps_crlf_and_undo_restores_exact_content() {
    let original = b"/* Ni\xf1o, caf\xe9 */\r\nDISPLAY \"\x82\xff\".".to_vec();
    let mut document = Document::from_bytes(original.clone());
    document.replace(5..7, &[0x81, 0x00, 0xfe]).unwrap();
    let mut expected = original.clone();
    expected.splice(5..7, [0x81, 0x00, 0xfe]);
    assert_eq!(document.to_bytes(), expected);
    assert!(document.is_modified());
    assert!(document.undo());
    assert_eq!(document.to_bytes(), original);
    assert!(!document.is_modified());
    assert!(document.redo());
    assert_eq!(document.to_bytes(), expected);
}

#[test]
fn replacement_crosses_original_and_added_pieces() {
    let mut document = Document::from_bytes(b"abcdef".to_vec());
    document.insert(2, b"XY").unwrap();
    assert_eq!(document.to_bytes(), b"abXYcdef");
    document.replace(1..6, &[0, 255]).unwrap();
    assert_eq!(document.to_bytes(), [b'a', 0, 255, b'e', b'f']);
    assert!(document.undo());
    assert_eq!(document.to_bytes(), b"abXYcdef");
    assert!(document.undo());
    assert_eq!(document.to_bytes(), b"abcdef");
    assert!(!document.undo());
    assert!(document.redo());
    assert!(document.redo());
    assert!(!document.redo());
    assert_eq!(document.to_bytes(), [b'a', 0, 255, b'e', b'f']);
}

#[test]
fn empty_documents_end_insertions_and_full_deletions_work() {
    let mut document = Document::default();
    assert!(document.is_empty());
    assert_eq!(document.piece_count(), 0);
    document.insert(0, &[0xff]).unwrap();
    document.insert(1, &[0x00]).unwrap();
    document.insert(0, b"prefix").unwrap();
    let bytes = document.to_bytes();
    document.delete(0..document.len()).unwrap();
    assert!(document.is_empty());
    assert_eq!(document.piece_count(), 0);
    assert!(document.undo());
    assert_eq!(document.to_bytes(), bytes);
    assert!(document.redo());
    assert!(document.is_empty());
}

#[test]
fn invalid_edits_preserve_content_revision_and_redo_branch() {
    let mut document = Document::from_bytes(b"abc".to_vec());
    document.insert(3, b"d").unwrap();
    document.undo();
    let revision = document.revision();
    for (start, end) in [(2, 1), (0, 4), (4, 4), (usize::MAX, usize::MAX)] {
        assert!(matches!(
            document.replace(start..end, b"bad"),
            Err(EditError::InvalidRange { .. })
        ));
        assert_eq!(document.to_bytes(), b"abc");
        assert_eq!(document.revision(), revision);
        assert!(!document.can_undo());
        assert!(document.can_redo());
        assert!(!document.is_modified());
    }
    assert!(document.redo());
    assert_eq!(document.to_bytes(), b"abcd");
}

#[test]
fn no_op_does_not_dirty_document_or_discard_redo() {
    let mut document = Document::from_bytes(b"abcdef".to_vec());
    document.insert(3, b"XY").unwrap();
    document.mark_saved();
    let revision = document.revision();
    document.replace(1..6, b"bcXYd").unwrap();
    document.delete(3..3).unwrap();
    document.insert(document.len(), b"").unwrap();
    assert_eq!(document.revision(), revision);
    assert!(!document.is_modified());
    document.insert(0, b"!").unwrap();
    assert!(document.undo());
    document.replace(0..document.len(), b"abcXYdef").unwrap();
    assert!(document.can_redo());
    assert!(document.redo());
    assert_eq!(document.to_bytes(), b"!abcXYdef");
}

#[test]
fn saved_revision_survives_undo_redo_and_new_branches() {
    let mut document = Document::from_bytes(b"a".to_vec());
    document.insert(1, b"b").unwrap();
    document.mark_saved();
    let saved_revision = document.revision();
    document.insert(2, b"c").unwrap();
    let discarded_revision = document.revision();
    assert!(document.is_modified());
    document.undo();
    assert_eq!(document.revision(), saved_revision);
    assert!(!document.is_modified());
    document.undo();
    assert!(document.is_modified());
    document.redo();
    assert!(!document.is_modified());
    document.insert(2, b"d").unwrap();
    assert!(document.revision() > discarded_revision);
    assert!(!document.can_redo());
    assert!(document.is_modified());
    document.undo();
    assert!(!document.is_modified());
}

#[test]
fn consecutive_appends_coalesce_adjacent_added_bytes() {
    let mut document = Document::default();
    for byte in b"abcdef" {
        document.insert(document.len(), &[*byte]).unwrap();
    }
    assert_eq!(document.to_bytes(), b"abcdef");
    assert_eq!(document.piece_count(), 1);
    for remaining in (0..6).rev() {
        assert!(document.undo());
        assert_eq!(document.len(), remaining);
    }
    assert!(!document.undo());
}

#[test]
fn output_error_is_reported_without_marking_document_saved() {
    struct FailingWriter {
        remaining: usize,
    }
    impl Write for FailingWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if self.remaining == 0 {
                return Err(io::Error::other("simulated output failure"));
            }
            let n = self.remaining.min(buf.len());
            self.remaining -= n;
            Ok(n)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut document = Document::from_bytes(b"abc".to_vec());
    document.insert(1, b"xy").unwrap();
    let revision = document.revision();
    assert!(document
        .write_to(&mut FailingWriter { remaining: 2 })
        .is_err());
    assert_eq!(document.to_bytes(), b"axybc");
    assert_eq!(document.revision(), revision);
    assert!(document.is_modified());
    assert!(document.undo());
    assert_eq!(document.to_bytes(), b"abc");
}

#[test]
fn deterministic_edits_match_an_independent_flat_buffer_history() {
    let mut document = Document::from_bytes((0..=255).collect());
    let mut snapshots: Vec<(Vec<u8>, u64)> = vec![((0..=255).collect(), 0)];
    let mut cursor = 0usize;
    let mut next_revision = 1u64;
    let mut saved_revision = 0u64;
    let mut seed = 0x4a43_4944_4520_2026u64;
    let mut random = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };

    for step in 0..3000 {
        match random() % 8 {
            0 => {
                let expected = cursor > 0;
                assert_eq!(document.undo(), expected);
                if expected {
                    cursor -= 1;
                }
            }
            1 => {
                let expected = cursor + 1 < snapshots.len();
                assert_eq!(document.redo(), expected);
                if expected {
                    cursor += 1;
                }
            }
            2 => {
                document.mark_saved();
                saved_revision = snapshots[cursor].1;
            }
            _ => {
                let len = snapshots[cursor].0.len();
                let start = (random() as usize) % (len + 1);
                let end = start + (random() as usize) % (len - start + 1);
                let inserted: Vec<u8> = (0..random() % 9).map(|_| random() as u8).collect();
                let mut next = snapshots[cursor].0.clone();
                next.splice(start..end, inserted.iter().copied());
                document.replace(start..end, &inserted).unwrap();
                if next != snapshots[cursor].0 {
                    snapshots.truncate(cursor + 1);
                    snapshots.push((next, next_revision));
                    next_revision += 1;
                    cursor += 1;
                }
            }
        }
        assert_eq!(document.to_bytes(), snapshots[cursor].0, "step {step}");
        assert_eq!(document.len(), snapshots[cursor].0.len(), "step {step}");
        assert_eq!(document.revision(), snapshots[cursor].1, "step {step}");
        assert_eq!(document.can_undo(), cursor > 0, "step {step}");
        assert_eq!(
            document.can_redo(),
            cursor + 1 < snapshots.len(),
            "step {step}"
        );
        assert_eq!(
            document.is_modified(),
            snapshots[cursor].1 != saved_revision,
            "step {step}"
        );
    }
}
