use std::fmt;
use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextEncoding {
    Utf8,
    Windows1252,
    Iso8859_1,
    Ibm850,
}

impl TextEncoding {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Windows1252 => "Windows-1252",
            Self::Iso8859_1 => "ISO-8859-1",
            Self::Ibm850 => "IBM850",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodingSource {
    Explicit,
    Utf8Bom,
    ValidUtf8,
    AsciiCompatible,
    UndeterminedLegacy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncodingProbe {
    pub encoding: Option<TextEncoding>,
    pub source: EncodingSource,
    pub bom_len: usize,
}

pub fn probe_encoding(bytes: &[u8], explicit: Option<TextEncoding>) -> EncodingProbe {
    if let Some(encoding) = explicit {
        return EncodingProbe {
            encoding: Some(encoding),
            source: EncodingSource::Explicit,
            bom_len: usize::from(
                encoding == TextEncoding::Utf8 && bytes.starts_with(&[0xef, 0xbb, 0xbf]),
            ) * 3,
        };
    }
    if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        return EncodingProbe {
            encoding: Some(TextEncoding::Utf8),
            source: EncodingSource::Utf8Bom,
            bom_len: 3,
        };
    }
    if std::str::from_utf8(bytes).is_ok() {
        return EncodingProbe {
            encoding: Some(TextEncoding::Utf8),
            source: if bytes.is_ascii() {
                EncodingSource::AsciiCompatible
            } else {
                EncodingSource::ValidUtf8
            },
            bom_len: 0,
        };
    }
    EncodingProbe {
        encoding: None,
        source: EncodingSource::UndeterminedLegacy,
        bom_len: 0,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeIssue {
    pub source_range: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedText {
    pub text: String,
    pub source_offsets: Vec<usize>,
    pub issues: Vec<DecodeIssue>,
    pub content_start: usize,
}

pub fn decode(bytes: &[u8], encoding: TextEncoding) -> DecodedText {
    match encoding {
        TextEncoding::Utf8 => decode_utf8(bytes, true),
        TextEncoding::Windows1252 => decode_single_byte(bytes, encoding, cp1252_decode),
        TextEncoding::Iso8859_1 => decode_single_byte(bytes, encoding, |byte| Some(byte as u32)),
        TextEncoding::Ibm850 => decode_single_byte(bytes, encoding, cp850_decode),
    }
}

pub fn decode_fragment(bytes: &[u8], encoding: TextEncoding, source_start: usize) -> DecodedText {
    let mut decoded = if encoding == TextEncoding::Utf8 {
        decode_utf8(bytes, source_start == 0)
    } else {
        decode(bytes, encoding)
    };
    for offset in &mut decoded.source_offsets {
        *offset += source_start;
    }
    for issue in &mut decoded.issues {
        issue.source_range.start += source_start;
        issue.source_range.end += source_start;
    }
    decoded.content_start += source_start;
    decoded
}

fn decode_utf8(bytes: &[u8], strip_bom: bool) -> DecodedText {
    let content_start = if strip_bom && bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        3
    } else {
        0
    };
    let mut text = String::new();
    let mut source_offsets = Vec::new();
    let mut issues = Vec::new();
    let mut cursor = content_start;

    while cursor < bytes.len() {
        match std::str::from_utf8(&bytes[cursor..]) {
            Ok(valid) => {
                append_valid_utf8(valid, cursor, &mut text, &mut source_offsets);
                cursor = bytes.len();
            }
            Err(error) => {
                let valid_len = error.valid_up_to();
                if valid_len > 0 {
                    let valid = std::str::from_utf8(&bytes[cursor..cursor + valid_len])
                        .expect("valid_up_to identifies UTF-8");
                    append_valid_utf8(valid, cursor, &mut text, &mut source_offsets);
                    cursor += valid_len;
                }
                if cursor < bytes.len() {
                    let invalid_len = error.error_len().unwrap_or(1).min(bytes.len() - cursor);
                    source_offsets.push(cursor);
                    text.push('�');
                    issues.push(DecodeIssue {
                        source_range: cursor..cursor + invalid_len,
                    });
                    cursor += invalid_len;
                }
            }
        }
    }
    source_offsets.push(cursor);
    DecodedText {
        text,
        source_offsets,
        issues,
        content_start,
    }
}

fn append_valid_utf8(
    valid: &str,
    source_start: usize,
    text: &mut String,
    offsets: &mut Vec<usize>,
) {
    for (relative, ch) in valid.char_indices() {
        offsets.push(source_start + relative);
        text.push(ch);
    }
}

fn decode_single_byte(
    bytes: &[u8],
    _encoding: TextEncoding,
    map: fn(u8) -> Option<u32>,
) -> DecodedText {
    let mut text = String::with_capacity(bytes.len());
    let mut source_offsets = Vec::with_capacity(bytes.len() + 1);
    let mut issues = Vec::new();
    for (offset, byte) in bytes.iter().copied().enumerate() {
        source_offsets.push(offset);
        match map(byte).and_then(char::from_u32) {
            Some(ch) => text.push(ch),
            None => {
                text.push('�');
                issues.push(DecodeIssue {
                    source_range: offset..offset + 1,
                });
            }
        }
    }
    source_offsets.push(bytes.len());
    DecodedText {
        text,
        source_offsets,
        issues,
        content_start: 0,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodeError {
    pub encoding: TextEncoding,
    pub character: char,
    pub scalar_index: usize,
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} cannot represent U+{:04X} at scalar {}",
            self.encoding.name(),
            self.character as u32,
            self.scalar_index
        )
    }
}

impl std::error::Error for EncodeError {}

pub fn encode(text: &str, encoding: TextEncoding) -> Result<Vec<u8>, EncodeError> {
    if encoding == TextEncoding::Utf8 {
        return Ok(text.as_bytes().to_vec());
    }
    let mut result = Vec::with_capacity(text.len());
    for (index, ch) in text.chars().enumerate() {
        let byte = match encoding {
            TextEncoding::Utf8 => unreachable!(),
            TextEncoding::Iso8859_1 => {
                let value = ch as u32;
                (value <= 0xff).then_some(value as u8)
            }
            TextEncoding::Windows1252 => cp1252_encode(ch),
            TextEncoding::Ibm850 => cp850_encode(ch),
        }
        .ok_or(EncodeError {
            encoding,
            character: ch,
            scalar_index: index,
        })?;
        result.push(byte);
    }
    Ok(result)
}

fn cp1252_decode(byte: u8) -> Option<u32> {
    if !(0x80..0xa0).contains(&byte) {
        return Some(byte as u32);
    }
    const TABLE: [Option<u32>; 32] = [
        Some(8364),
        None,
        Some(8218),
        Some(402),
        Some(8222),
        Some(8230),
        Some(8224),
        Some(8225),
        Some(710),
        Some(8240),
        Some(352),
        Some(8249),
        Some(338),
        None,
        Some(381),
        None,
        None,
        Some(8216),
        Some(8217),
        Some(8220),
        Some(8221),
        Some(8226),
        Some(8211),
        Some(8212),
        Some(732),
        Some(8482),
        Some(353),
        Some(8250),
        Some(339),
        None,
        Some(382),
        Some(376),
    ];
    TABLE[(byte - 0x80) as usize]
}

fn cp1252_encode(ch: char) -> Option<u8> {
    let value = ch as u32;
    if value < 0x80 || (0xa0..=0xff).contains(&value) {
        return Some(value as u8);
    }
    const SPECIALS: &[(u32, u8)] = &[
        (8364, 0x80),
        (8218, 0x82),
        (402, 0x83),
        (8222, 0x84),
        (8230, 0x85),
        (8224, 0x86),
        (8225, 0x87),
        (710, 0x88),
        (8240, 0x89),
        (352, 0x8a),
        (8249, 0x8b),
        (338, 0x8c),
        (381, 0x8e),
        (8216, 0x91),
        (8217, 0x92),
        (8220, 0x93),
        (8221, 0x94),
        (8226, 0x95),
        (8211, 0x96),
        (8212, 0x97),
        (732, 0x98),
        (8482, 0x99),
        (353, 0x9a),
        (8250, 0x9b),
        (339, 0x9c),
        (382, 0x9e),
        (376, 0x9f),
    ];
    SPECIALS
        .iter()
        .find(|(mapped, _)| *mapped == value)
        .map(|(_, byte)| *byte)
}

fn cp850_decode(byte: u8) -> Option<u32> {
    if byte < 0x80 {
        return Some(byte as u32);
    }
    const TABLE: [u32; 128] = [
        199, 252, 233, 226, 228, 224, 229, 231, 234, 235, 232, 239, 238, 236, 196, 197, 201, 230,
        198, 244, 246, 242, 251, 249, 255, 214, 220, 248, 163, 216, 215, 402, 225, 237, 243, 250,
        241, 209, 170, 186, 191, 174, 172, 189, 188, 161, 171, 187, 9617, 9618, 9619, 9474, 9508,
        193, 194, 192, 169, 9571, 9553, 9559, 9565, 162, 165, 9488, 9492, 9524, 9516, 9500, 9472,
        9532, 227, 195, 9562, 9556, 9577, 9574, 9568, 9552, 9580, 164, 240, 208, 202, 203, 200,
        305, 205, 206, 207, 9496, 9484, 9608, 9604, 166, 204, 9600, 211, 223, 212, 210, 245, 213,
        181, 254, 222, 218, 219, 217, 253, 221, 175, 180, 173, 177, 8215, 190, 182, 167, 247, 184,
        176, 168, 183, 185, 179, 178, 9632, 160,
    ];
    Some(TABLE[(byte - 0x80) as usize])
}

fn cp850_encode(ch: char) -> Option<u8> {
    let value = ch as u32;
    if value < 0x80 {
        return Some(value as u8);
    }
    (0x80u16..=0xff)
        .find(|byte| cp850_decode(*byte as u8) == Some(value))
        .map(|byte| byte as u8)
}
