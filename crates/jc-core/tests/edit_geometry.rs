use jc_core::{ByteEdit, Document};

#[test]
fn undo_redo_report_inverse_edit_geometry_across_a_branched_history() {
    let mut doc = Document::from_bytes(b"abcdef".to_vec());
    doc.replace(1..3, b"XYZ").unwrap();
    assert_eq!(
        doc.last_edit(),
        Some(&ByteEdit {
            replaced: 1..3,
            inserted_len: 3
        })
    );
    doc.insert(0, b"!").unwrap();
    doc.undo();
    assert_eq!(
        doc.last_edit(),
        Some(&ByteEdit {
            replaced: 0..1,
            inserted_len: 0
        })
    );
    doc.undo();
    assert_eq!(
        doc.last_edit(),
        Some(&ByteEdit {
            replaced: 1..4,
            inserted_len: 2
        })
    );
    doc.redo();
    assert_eq!(
        doc.last_edit(),
        Some(&ByteEdit {
            replaced: 1..3,
            inserted_len: 3
        })
    );
    doc.delete(4..6).unwrap();
    assert!(!doc.redo());
    assert_eq!(doc.last_edit(), None);
    doc.undo();
    assert_eq!(
        doc.last_edit(),
        Some(&ByteEdit {
            replaced: 4..4,
            inserted_len: 2
        })
    );
    assert_eq!(doc.to_bytes(), b"aXYZdef");
}
