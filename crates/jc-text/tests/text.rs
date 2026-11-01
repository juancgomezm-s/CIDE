use jc_text::{
    decode, encode, inspect_text, probe_encoding, EncodingSource, LineEndingKind, PositionError,
    TextEncoding, TextIndex, TextPosition,
};

#[test]
fn invalid_utf8_is_not_guessed_as_a_legacy_code_page() {
    let bytes = b"ni\xf1o";
    let probe = probe_encoding(bytes, None);
    assert_eq!(probe.encoding, None);
    assert_eq!(probe.source, EncodingSource::UndeterminedLegacy);
}

#[test]
fn explicit_single_byte_encodings_roundtrip_representable_text() {
    for encoding in [
        TextEncoding::Windows1252,
        TextEncoding::Iso8859_1,
        TextEncoding::Ibm850,
    ] {
        let text = "Niño, café, pingüino";
        let bytes = encode(text, encoding).unwrap();
        let decoded = decode(&bytes, encoding);
        assert_eq!(decoded.text, text);
        assert!(decoded.issues.is_empty());
        assert_eq!(encode(&decoded.text, encoding).unwrap(), bytes);
    }
}

#[test]
fn cp1252_undefined_bytes_are_visible_issues_not_silent_replacements_on_disk() {
    let bytes = [b'A', 0x81, b'B'];
    let decoded = decode(&bytes, TextEncoding::Windows1252);
    assert_eq!(decoded.text, "A\u{fffd}B");
    assert_eq!(decoded.issues.len(), 1);
    assert_eq!(decoded.issues[0].source_range, 1..2);
    assert_eq!(decoded.source_offsets, vec![0, 1, 2, 3]);
}

#[test]
fn unsupported_inserted_characters_fail_strictly() {
    let error = encode("ABL 😀", TextEncoding::Windows1252).unwrap_err();
    assert_eq!(error.character, '😀');
    assert_eq!(error.scalar_index, 4);
}

#[test]
fn utf8_bom_is_interpreted_but_remains_part_of_source_byte_coordinates() {
    let bytes = b"\xef\xbb\xbfuno\r\ndos";
    let inspection = inspect_text(bytes, None);
    assert_eq!(inspection.probe.encoding, Some(TextEncoding::Utf8));
    assert_eq!(inspection.probe.source, EncodingSource::Utf8Bom);
    assert_eq!(inspection.probe.bom_len, 3);
    let decoded = inspection.decoded.unwrap();
    assert_eq!(decoded.text, "uno\r\ndos");
    assert_eq!(decoded.content_start, 3);
    assert_eq!(decoded.source_offsets[0], 3);
}

#[test]
fn mixed_line_endings_and_final_newline_are_reported_without_normalization() {
    let inspection = inspect_text(b"a\r\nb\nc\rd", Some(TextEncoding::Iso8859_1));
    assert_eq!(inspection.line_endings.kind, LineEndingKind::Mixed);
    assert_eq!(inspection.line_endings.crlf, 1);
    assert_eq!(inspection.line_endings.lf, 1);
    assert_eq!(inspection.line_endings.cr, 1);
    assert!(!inspection.line_endings.final_newline);

    let final_crlf = inspect_text(b"a\r\n", None);
    assert!(final_crlf.line_endings.final_newline);
    assert_eq!(final_crlf.line_endings.kind, LineEndingKind::CrLf);
}

#[test]
fn text_positions_roundtrip_to_source_bytes_for_utf8_and_crlf() {
    let decoded = decode("aé\r\nñx".as_bytes(), TextEncoding::Utf8);
    let index = TextIndex::new(&decoded);
    assert_eq!(index.line_count(), 2);
    let second_line = TextPosition { line: 1, column: 1 };
    let byte = index.position_to_source_byte(second_line).unwrap();
    assert_eq!(&"aé\r\nñx".as_bytes()[byte..], b"x");
    assert_eq!(index.source_byte_to_position(byte).unwrap(), second_line);

    let crlf_middle = "aé\r\nñx".find('\n').unwrap();
    assert_eq!(
        index.source_byte_to_position(crlf_middle),
        Err(PositionError::InsideLineEnding(crlf_middle))
    );
}

#[test]
fn ascii_is_explicitly_marked_ambiguous_but_safe_to_treat_as_utf8() {
    let probe = probe_encoding(b"DISPLAY x.", None);
    assert_eq!(probe.encoding, Some(TextEncoding::Utf8));
    assert_eq!(probe.source, EncodingSource::AsciiCompatible);
}
