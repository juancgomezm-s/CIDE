use jc_text::{decode_fragment, encode, DecodedScalars, TextEncoding};

#[test]
fn borrowed_decoding_preserves_invalid_groups_literal_bom_and_absolute_offsets() {
    let bytes = b"\xef\xbb\xbfA\xe1\x80B\xef\xbf\xbd\xef\xbb\xbf\xf0\x9f";
    let actual: Vec<_> = DecodedScalars::new(bytes, TextEncoding::Utf8, 0)
        .map(|s| (s.character, s.source_range, s.valid))
        .collect();
    assert_eq!(
        actual,
        vec![
            ('A', 3..4, true),
            ('\u{fffd}', 4..6, false),
            ('B', 6..7, true),
            ('\u{fffd}', 7..10, true),
            ('\u{feff}', 10..13, true),
            ('\u{fffd}', 13..14, false),
            ('\u{fffd}', 14..15, false),
        ]
    );
    let embedded: Vec<_> = DecodedScalars::new(&bytes[..4], TextEncoding::Utf8, 100)
        .map(|s| (s.character, s.source_range, s.valid))
        .collect();
    assert_eq!(
        embedded,
        vec![('\u{feff}', 100..103, true), ('A', 103..104, true)]
    );
    let owned = decode_fragment(bytes, TextEncoding::Utf8, 0);
    assert_eq!(owned.text, "A�B�\u{feff}��");
    assert_eq!(owned.source_offsets, vec![3, 4, 6, 7, 10, 13, 14, 15]);
    assert_eq!(
        owned
            .issues
            .iter()
            .map(|i| i.source_range.clone())
            .collect::<Vec<_>>(),
        vec![4..6, 13..14, 14..15]
    );
}

#[test]
fn explicit_single_byte_pages_roundtrip_every_valid_byte_with_exact_ranges() {
    let bytes: Vec<_> = (0..=255).collect();
    for encoding in [
        TextEncoding::Windows1252,
        TextEncoding::Iso8859_1,
        TextEncoding::Ibm850,
    ] {
        let scalars: Vec<_> = DecodedScalars::new(&bytes, encoding, 23).collect();
        assert_eq!(scalars.len(), 256);
        for (i, scalar) in scalars.iter().enumerate() {
            assert_eq!(scalar.source_range, (23 + i)..(24 + i));
            let undefined = encoding == TextEncoding::Windows1252
                && [0x81, 0x8d, 0x8f, 0x90, 0x9d].contains(&i);
            assert_eq!(scalar.valid, !undefined);
            if scalar.valid {
                assert_eq!(
                    encode(&scalar.character.to_string(), encoding).unwrap(),
                    vec![i as u8]
                );
            }
        }
    }
}
