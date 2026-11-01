use crate::{decode, probe_encoding, DecodedText, EncodingProbe, TextEncoding};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEndingKind {
    None,
    Lf,
    CrLf,
    Cr,
    Mixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineEndingInfo {
    pub kind: LineEndingKind,
    pub lf: usize,
    pub crlf: usize,
    pub cr: usize,
    pub final_newline: bool,
}

impl LineEndingInfo {
    pub fn inspect(bytes: &[u8]) -> Self {
        let mut lf = 0usize;
        let mut crlf = 0usize;
        let mut cr = 0usize;
        let mut i = 0usize;
        while i < bytes.len() {
            match bytes[i] {
                b'\r' if i + 1 < bytes.len() && bytes[i + 1] == b'\n' => {
                    crlf += 1;
                    i += 2;
                }
                b'\r' => {
                    cr += 1;
                    i += 1;
                }
                b'\n' => {
                    lf += 1;
                    i += 1;
                }
                _ => i += 1,
            }
        }
        let nonzero = usize::from(lf > 0) + usize::from(crlf > 0) + usize::from(cr > 0);
        let kind = match nonzero {
            0 => LineEndingKind::None,
            1 if crlf > 0 => LineEndingKind::CrLf,
            1 if lf > 0 => LineEndingKind::Lf,
            1 => LineEndingKind::Cr,
            _ => LineEndingKind::Mixed,
        };
        Self {
            kind,
            lf,
            crlf,
            cr,
            final_newline: bytes.ends_with(b"\n") || bytes.ends_with(b"\r"),
        }
    }

    pub fn preferred_bytes(self) -> &'static [u8] {
        if self.crlf >= self.lf && self.crlf >= self.cr && self.crlf > 0 {
            b"\r\n"
        } else if self.lf >= self.cr && self.lf > 0 {
            b"\n"
        } else if self.cr > 0 {
            b"\r"
        } else {
            b"\r\n"
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextInspection {
    pub probe: EncodingProbe,
    pub line_endings: LineEndingInfo,
    pub decoded: Option<DecodedText>,
}

pub fn inspect_text(bytes: &[u8], explicit: Option<TextEncoding>) -> TextInspection {
    let probe = probe_encoding(bytes, explicit);
    let decoded = probe.encoding.map(|encoding| decode(bytes, encoding));
    TextInspection {
        probe,
        line_endings: LineEndingInfo::inspect(bytes),
        decoded,
    }
}
