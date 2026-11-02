//! Text interpretation for byte-preserving JC-IDE documents.
//!
//! This crate never owns or rewrites the canonical source bytes. It provides
//! explicit decoding, strict encoding for inserted text, line-ending analysis,
//! and conversion between source byte offsets and logical text positions.

mod encoding;
mod index;
mod profile;

pub use encoding::{
    decode, decode_fragment, encode, probe_encoding, DecodeIssue, DecodedText, EncodeError,
    EncodingProbe, EncodingSource, TextEncoding,
};
pub use index::{PositionError, TextIndex, TextPosition};
pub use profile::{inspect_text, LineEndingInfo, LineEndingKind, TextInspection};
