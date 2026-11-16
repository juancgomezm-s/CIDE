use jc_core::{
    source::{ImmutableByteSource, ReadChunk, SourceReadError, MAX_READ_CHUNK_BYTES},
    Document, EditBatch, EditError, RecoveryLimits, Replacement,
};
use std::{
    io::{self, Read},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

struct Pattern {
    len: u64,
    fail: AtomicBool,
}

impl ImmutableByteSource for Pattern {
    fn len(&self) -> u64 {
        self.len
    }

    fn read_chunk(&self, offset: u64, size: usize) -> Result<ReadChunk, SourceReadError> {
        if self.fail.load(Ordering::Relaxed) {
            return Err(io::Error::other("lectura fallida de prueba").into());
        }
        let len = size.min((self.len - offset) as usize).min(8);
        let bytes: Arc<[u8]> = (0..len).map(|i| (offset + i as u64) as u8).collect();
        ReadChunk::new(bytes, 0..len, offset)
    }
}

#[cfg(target_pointer_width = "64")]
#[test]
fn edit_and_undo_above_four_gib_use_the_same_document_and_bounded_ranges() {
    let boundary = 1usize << 32;
    let source = Arc::new(Pattern {
        len: boundary as u64 + 33,
        fail: AtomicBool::new(false),
    });
    let mut doc = Document::from_source(source.clone()).unwrap();
    assert_eq!(doc.len(), boundary + 33);
    assert_eq!(doc.retained_source_bytes(), 0);
    assert!(matches!(
        doc.read_range(0..doc.len()),
        Err(EditError::SourceRead(_))
    ));
    assert_eq!(
        doc.read_range(boundary - 1..boundary + 2).unwrap(),
        [255, 0, 1]
    );
    doc.replace(boundary..boundary + 2, b"CIDE").unwrap();
    assert_eq!(
        doc.read_range(boundary - 1..boundary + 5).unwrap(),
        b"\xffCIDE\x02"
    );
    doc.reclaim_unused_storage();
    assert!(doc.undo());
    assert_eq!(doc.read_range(boundary..boundary + 2).unwrap(), [0, 1]);
    assert!(doc.redo());
    assert!(doc.is_paged());
    assert!(doc.retained_source_bytes() <= 16);

    let revision = doc.revision();
    source.fail.store(true, Ordering::Relaxed);
    let frame = doc
        .prepare_paged_read(boundary - 1..boundary + 5, 4)
        .unwrap();
    assert_eq!(frame.document_identity(), doc.recovery_identity());
    assert_eq!(frame.revision(), revision);
    assert_eq!(frame.range(), boundary - 1..boundary + 5);
    assert!(doc.prepare_paged_read(boundary..boundary + 4, 3).is_err());
    assert!(matches!(
        doc.apply_batch(EditBatch {
            expected_revision: revision,
            edits: vec![
                Replacement {
                    range: 0..0,
                    bytes: b"x".to_vec()
                },
                Replacement {
                    range: boundary - 1..boundary,
                    bytes: b"y".to_vec()
                },
            ],
        }),
        Err(EditError::SourceRead(_))
    ));
    assert_eq!(doc.revision(), revision);
    assert_eq!(doc.len(), boundary + 35);
    assert!(doc.undo());
    source.fail.store(false, Ordering::Relaxed);
    let mut prepared = Vec::new();
    frame
        .visit(|bytes| {
            prepared.extend_from_slice(bytes);
            Ok(())
        })
        .unwrap();
    assert_eq!(prepared, b"\xffCIDE\x02");
    assert_ne!(frame.revision(), doc.revision());
}

struct Fixture(Arc<[u8]>);

impl ImmutableByteSource for Fixture {
    fn len(&self) -> u64 {
        self.0.len() as u64
    }

    fn read_chunk(&self, offset: u64, size: usize) -> Result<ReadChunk, SourceReadError> {
        let start = offset as usize;
        let end = (start + size.min(MAX_READ_CHUNK_BYTES)).min(self.0.len());
        ReadChunk::new(self.0.clone(), start..end, offset)
    }
}

#[test]
fn recovery_streams_paged_original_and_revalidates_the_restored_arena() {
    let mut doc = Document::from_source(Arc::new(Fixture(Arc::from(&b"a\xff\r\nb"[..])))).unwrap();
    doc.replace(0..1, b"123").unwrap();
    doc.mark_saved();
    doc.insert(doc.len(), b"fin").unwrap();
    let mut saved = Vec::new();
    doc.write_to(&mut saved).unwrap();
    assert_eq!(saved, b"123\xff\r\nbfin");

    let mut wire = Vec::new();
    doc.write_recovery(&mut wire).unwrap();
    let load = |reader: &mut dyn Read, len| {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        assert_eq!(bytes.len() as u64, len);
        Ok(Arc::new(Fixture(bytes.into())) as Arc<dyn ImmutableByteSource>)
    };
    let mut restored = Document::read_recovery_with_source(
        &mut wire.as_slice(),
        RecoveryLimits::default(),
        1024,
        load,
    )
    .unwrap();
    assert!(restored.is_paged());
    assert!(restored.is_modified());
    assert_eq!(restored.read_range(0..restored.len()).unwrap(), saved);
    assert!(restored.undo());
    assert_eq!(
        restored.read_range(0..restored.len()).unwrap(),
        b"123\xff\r\nb"
    );
    assert!(!restored.is_modified());
    assert!(Document::read_recovery_with_source(
        &mut wire.as_slice(),
        RecoveryLimits::default(),
        1024,
        |_reader, _len| Ok(Arc::new(Fixture(Arc::from([])))),
    )
    .is_err());
}
