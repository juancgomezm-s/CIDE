//! Text interpretation for byte-preserving CIDE documents.
//!
//! This crate never owns or rewrites the canonical source bytes. It provides
//! explicit decoding, strict encoding for inserted text, line-ending analysis,
//! and conversion between source byte offsets and logical text positions.

mod encoding;
pub mod grapheme;
mod index;
mod profile;
mod unicode_search;
pub use unicode_search::{full_case_fold, is_search_word};

pub use encoding::{
    decode, decode_fragment, encode, probe_encoding, DecodeIssue, DecodedScalar, DecodedScalars,
    DecodedText, EncodeError, EncodingProbe, EncodingSource, TextEncoding,
};
pub use index::{PositionError, TextIndex, TextPosition};
pub use profile::{inspect_text, LineEndingInfo, LineEndingKind, TextInspection};
