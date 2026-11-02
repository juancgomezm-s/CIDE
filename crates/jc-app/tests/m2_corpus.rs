use jc_app::{SearchStatus, TextReplacement, Workbench};
use jc_text::{decode, encode, TextEncoding, TextIndex};
use std::fs;

#[test]
fn expanded_corpus_matches_source_ranges_and_coordinates_through_fragmentation_and_undo() {
    for (index, encoding) in [
        TextEncoding::Utf8,
        TextEncoding::Windows1252,
        TextEncoding::Iso8859_1,
        TextEncoding::Ibm850,
    ]
    .into_iter()
    .enumerate()
    {
        let noise = if encoding == TextEncoding::Utf8 {
            "é😀e\u{301}界"
        } else {
            "niño été"
        };
        let block = format!("{} Straße\r\nNIÑO\rfin\n", noise.repeat(4093));
        let text = block.repeat(8);
        let original = encode(&text, encoding).unwrap();
        let path =
            std::env::temp_dir().join(format!("jc-m2-corpus-{}-{index}.txt", std::process::id()));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        std::io::Write::write_all(&mut file, &original).unwrap();
        drop(file);
        let mut app = Workbench::default();
        let tab = app.open_file(&path, Some(encoding)).unwrap();
        let expected: Vec<_> = text
            .match_indices("Straße")
            .map(|(i, word)| {
                let start = encode(&text[..i], encoding).unwrap().len();
                start..start + encode(word, encoding).unwrap().len()
            })
            .collect();
        let reference = TextIndex::new(&decode(&original, encoding));
        for fragmented in [false, true] {
            if fragmented {
                let d = app.active_document().unwrap();
                let (id, revision) = (d.id, d.session.document().revision());
                let edits = expected
                    .iter()
                    .map(|r| TextReplacement {
                        range: r.start..r.start,
                        text: " ".into(),
                    })
                    .collect();
                app.apply_text_edits(id, revision, edits).unwrap();
                app.undo().unwrap(); // restore original bytes with retained redo/piece geometry
            }
            app.set_search_query(if fragmented { "STRASSE" } else { "strasse" })
                .unwrap();
            while app.search_running() {
                app.poll_document_search();
            }
            assert_eq!(app.search().status, SearchStatus::Complete);
            let ranges: Vec<_> = app
                .search()
                .matches
                .iter()
                .map(|m| m.range.clone())
                .collect();
            assert_eq!(ranges, expected);
            for hit in &app.search().matches {
                assert_eq!(
                    Some(hit.position),
                    reference.source_byte_to_position(hit.range.start).ok()
                );
            }
            assert_eq!(app.active_bytes().unwrap(), original);
            assert_eq!(app.replace_all("street").unwrap(), 8);
            app.undo().unwrap();
            assert_eq!(app.active_bytes().unwrap(), original);
            assert_eq!(fs::read(&path).unwrap(), original);
            assert!(app.text_statistics(tab).unwrap().2 > 0);
        }
        drop(app);
        fs::remove_file(path).unwrap();
    }
}
