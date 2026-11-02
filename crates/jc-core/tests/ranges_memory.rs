use jc_core::{Document, EditError};

#[test]
fn a_large_discarded_transition_does_not_charge_new_small_undo_steps() {
    let mut doc = Document::default();
    doc.set_history_bytes_limit(4096);
    doc.insert(0, &vec![b'a'; 65536]).unwrap();
    assert!(!doc.can_undo());
    doc.insert(0, b"x").unwrap();
    assert!(doc.can_undo());
    assert!(doc.history_bytes() <= 4096);
    assert!(doc.undo());
    assert_eq!(doc.to_bytes(), vec![b'a'; 65536]);
    assert!(doc.redo());
    assert_eq!(doc.len(), 65537);
}

#[test]
fn segment_reads_match_every_flat_slice_after_fragmentation_and_undo() {
    let mut doc = Document::from_bytes((0..80).collect());
    for n in 0..30 {
        doc.replace(n..n + 1, &[n as u8, 255, n as u8]).unwrap();
    }
    for _ in 0..3 {
        let flat = doc.to_bytes();
        let snapshot = doc.read_snapshot();
        assert_eq!(snapshot.revision(), doc.revision());
        for start in 0..=flat.len() {
            for end in start..=flat.len() {
                let range = start..end;
                assert_eq!(doc.read_range(range.clone()).unwrap(), flat[range.clone()]);
                let borrowed: Vec<_> = snapshot
                    .chunks(range.clone())
                    .unwrap()
                    .flatten()
                    .copied()
                    .collect();
                assert_eq!(borrowed, flat[range]);
            }
        }
        assert!(doc.undo());
    }
    assert!(matches!(
        doc.read_range(0..usize::MAX),
        Err(EditError::InvalidRange { .. })
    ));
}

#[test]
fn collecting_unreachable_sources_preserves_all_retained_undo_redo_and_saved_revision() {
    let mut doc = Document::from_bytes(b"\xff\r\noriginal".to_vec());
    let original = doc.to_bytes();
    doc.insert(0, b"prefix").unwrap();
    doc.mark_saved();
    let saved = doc.to_bytes();
    doc.replace(2..5, b"changed").unwrap();
    let changed = doc.to_bytes();
    doc.insert(4, b"discarded branch").unwrap();
    doc.undo();
    let revision = doc.revision();
    let geometry = doc.last_change().cloned();
    doc.reclaim_unused_storage();
    assert_eq!(doc.revision(), revision);
    assert_eq!(doc.last_change(), geometry.as_ref());
    assert_eq!(doc.to_bytes(), changed);
    assert!(doc.redo());
    assert!(doc.undo());
    assert!(doc.undo());
    assert_eq!(doc.to_bytes(), saved);
    assert!(!doc.is_modified());
    assert!(doc.undo());
    assert_eq!(doc.to_bytes(), original);
    assert!(doc.redo());
    assert_eq!(doc.to_bytes(), saved);
}

#[test]
fn byte_budget_bounds_history_and_reclaims_dead_bytes_without_erasing_surviving_steps() {
    let mut doc = Document::from_bytes(vec![0; 1024]);
    doc.set_history_bytes_limit(8192);
    for byte in 1..=100u8 {
        doc.replace(0..1024, &vec![byte; 1024]).unwrap();
        assert!(doc.history_bytes() <= 8192);
    }
    let steps = doc.undo_steps();
    assert!(steps > 0 && steps < 100);
    doc.reclaim_unused_storage();
    assert!(doc.retained_source_bytes() < 16384);
    for expected in (100 - steps as u8..100).rev() {
        assert!(doc.undo());
        assert_eq!(doc.to_bytes(), vec![expected; 1024]);
    }
    assert!(!doc.can_undo());
    for _ in 0..steps {
        assert!(doc.redo());
    }
    assert_eq!(doc.to_bytes(), vec![100; 1024]);
}

#[test]
fn disabling_history_keeps_append_storage_bounded_during_repeated_large_replacement() {
    let mut doc = Document::from_bytes(vec![0; 65536]);
    doc.set_history_limit(0);
    for byte in 1..=100u8 {
        doc.replace(0..65536, &vec![byte; 65536]).unwrap();
        assert!(doc.retained_added_bytes() <= 131072);
    }
    assert_eq!(doc.undo_steps(), 0);
    assert_eq!(doc.history_bytes(), 0);
    assert_eq!(doc.to_bytes(), vec![100; 65536]);
}
