use jc_core::{Document, EditBatch, EditError, Replacement};

fn edit(range: std::ops::Range<usize>, bytes: &[u8]) -> Replacement {
    Replacement {
        range,
        bytes: bytes.to_vec(),
    }
}

#[test]
fn batch_is_one_revision_and_one_undo_with_complete_inverse_mapping() {
    let mut doc = Document::from_bytes(b"abcdefghi".to_vec());
    doc.apply_batch(EditBatch {
        expected_revision: 0,
        edits: vec![edit(5..7, b"XYZ"), edit(0..1, b"12")],
    })
    .unwrap();
    assert_eq!(doc.to_bytes(), b"12bcdeXYZhi");
    assert_eq!(doc.undo_steps(), 1);
    assert_eq!(doc.revision(), 1);
    let change = doc.last_change().unwrap().clone();
    assert_eq!(change.map_offset(8), 10);
    assert!(doc.undo());
    assert_eq!(doc.to_bytes(), b"abcdefghi");
    assert_eq!(doc.last_change().unwrap(), &change.inverse());
    assert_eq!(doc.last_change().unwrap().map_offset(10), 8);
    assert!(!doc.is_modified());
    assert!(doc.redo());
    assert_eq!(doc.last_change().unwrap(), &change);
    assert_eq!(doc.to_bytes(), b"12bcdeXYZhi");
}

#[test]
fn rejected_batches_preserve_bytes_revision_history_storage_and_redo() {
    let mut doc = Document::from_bytes(b"abc\r\n\xff".to_vec());
    doc.insert(0, b"x").unwrap();
    doc.undo();
    let previous = doc.last_change().cloned();
    for edits in [
        vec![edit(0..1, b"ok"), edit(99..100, b"bad")],
        vec![edit(0..2, b"x"), edit(1..3, b"y")],
        vec![edit(1..1, b"x"), edit(1..1, b"y")],
    ] {
        assert!(doc
            .apply_batch(EditBatch {
                expected_revision: 0,
                edits
            })
            .is_err());
        assert_eq!(doc.to_bytes(), b"abc\r\n\xff");
        assert_eq!(doc.revision(), 0);
        assert_eq!(doc.undo_steps(), 0);
        assert_eq!(doc.redo_steps(), 1);
        assert_eq!(doc.retained_added_bytes(), 1);
        assert_eq!(doc.last_change(), previous.as_ref());
    }
    assert!(matches!(
        doc.apply_batch(EditBatch {
            expected_revision: 99,
            edits: vec![]
        }),
        Err(EditError::StaleRevision { .. })
    ));
    assert!(doc.redo());
    assert_eq!(doc.to_bytes(), b"xabc\r\n\xff");
}

#[test]
fn batch_history_matches_flat_bytes_through_undo_redo_and_branching() {
    let mut doc = Document::from_bytes((0..=255).collect());
    let mut expected = doc.to_bytes();
    let mut history = vec![expected.clone()];
    for n in 0..40 {
        let edits = vec![
            edit(1..3, &[n, 0]),
            edit(120..124, b"\r\n"),
            edit(240..240, b"tail"),
        ];
        for e in edits.iter().rev() {
            expected.splice(e.range.clone(), e.bytes.iter().copied());
        }
        doc.apply_batch(EditBatch {
            expected_revision: doc.revision(),
            edits,
        })
        .unwrap();
        assert_eq!(doc.to_bytes(), expected);
        history.push(expected.clone());
    }
    for expected in history.iter().rev().skip(1) {
        assert!(doc.undo());
        assert_eq!(&doc.to_bytes(), expected);
    }
    for expected in history.iter().skip(1) {
        assert!(doc.redo());
        assert_eq!(&doc.to_bytes(), expected);
    }
    doc.undo();
    doc.apply_batch(EditBatch {
        expected_revision: doc.revision(),
        edits: vec![edit(0..0, b"new")],
    })
    .unwrap();
    assert!(!doc.can_redo());
}
