//! Immutable byte-source admission for future paged document storage. This is
//! deliberately separate from Document until its read/transaction/projection
//! APIs can propagate fallible reads. No UI, encoding or path policy lives here.
use std::{
    fmt, io,
    ops::{Deref, Range},
    sync::Arc,
};

pub const MAX_READ_CHUNK_BYTES: usize = 64 * 1024;

#[derive(Debug)]
pub enum SourceReadError {
    Io(io::Error),
    Busy {
        retained_bytes: usize,
        budget_bytes: usize,
    },
    InvalidRange {
        offset: u64,
        requested: usize,
        source_len: u64,
    },
}
impl fmt::Display for SourceReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "No se pudo leer la fuente de bytes: {error}"),
            Self::Busy { retained_bytes, budget_bytes } => write!(f,
                "La fuente está ocupada: {retained_bytes} bytes retenidos de {budget_bytes}; libera lecturas pendientes"),
            Self::InvalidRange { offset, requested, source_len } => write!(f,
                "Lectura de {requested} bytes desde {offset} inválida para una fuente de {source_len} bytes (máximo64 KiB por lectura)"),
        }
    }
}
impl std::error::Error for SourceReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}
impl From<io::Error> for SourceReadError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// A small owned lease. Its shared bytes stay alive independently of the source
/// cache; a provider must charge the complete backing page while any lease lives.
#[derive(Clone)]
pub struct ReadChunk {
    bytes: Arc<[u8]>,
    range: Range<usize>,
    source_offset: u64,
}
impl ReadChunk {
    pub fn new(
        bytes: Arc<[u8]>,
        range: Range<usize>,
        source_offset: u64,
    ) -> Result<Self, SourceReadError> {
        if bytes.len() > MAX_READ_CHUNK_BYTES
            || range.start > range.end
            || range.end > bytes.len()
            || source_offset.checked_add(range.len() as u64).is_none()
        {
            return Err(SourceReadError::InvalidRange {
                offset: source_offset,
                requested: range.end.saturating_sub(range.start),
                source_len: bytes.len() as u64,
            });
        }
        Ok(Self {
            bytes,
            range,
            source_offset,
        })
    }
    pub fn empty(source_offset: u64) -> Self {
        Self {
            bytes: Arc::from([]),
            range: 0..0,
            source_offset,
        }
    }
    pub fn source_offset(&self) -> u64 {
        self.source_offset
    }
    pub fn as_slice(&self) -> &[u8] {
        &self.bytes[self.range.clone()]
    }
    pub fn len(&self) -> usize {
        self.range.len()
    }
    pub fn is_empty(&self) -> bool {
        self.range.is_empty()
    }
    /// Charged page payload, which can exceed this chunk's visible subrange.
    pub fn retained_bytes(&self) -> usize {
        self.bytes.len()
    }
}
impl Deref for ReadChunk {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}
impl AsRef<[u8]> for ReadChunk {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}
impl fmt::Debug for ReadChunk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReadChunk")
            .field("source_offset", &self.source_offset)
            .field("len", &self.len())
            .field("retained_bytes", &self.retained_bytes())
            .finish()
    }
}

/// Implementations must keep every byte stable for the source lifetime, including
/// when an external pathname is replaced. Reads may perform blocking I/O: invoke
/// them in bounded workers, never from draw or a frontend's input/prepare path.
/// A request returns at most64KiB and may stop at a page boundary or EOF. Repeated
/// calls with checked offset advancement cover larger ranges. Offset==len returns
/// an empty chunk; offsets beyond EOF and oversized requests fail explicitly.
pub trait ImmutableByteSource: Send + Sync {
    fn len(&self) -> u64;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn read_chunk(&self, offset: u64, max_len: usize) -> Result<ReadChunk, SourceReadError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn read_chunk_lease_preserves_shared_bytes_and_rejects_invalid_geometry() {
        let bytes: Arc<[u8]> = Arc::from(&b"\x00\xff\r\nabc"[..]);
        let chunk = ReadChunk::new(bytes.clone(), 1..5, (1u64 << 32) + 9).unwrap();
        assert_eq!(chunk.as_slice(), b"\xff\r\na");
        assert_eq!(chunk.source_offset(), (1u64 << 32) + 9);
        assert_eq!(chunk.retained_bytes(), 7);
        let clone = chunk.clone();
        drop(bytes);
        drop(chunk);
        assert_eq!(clone.as_slice(), b"\xff\r\na");
        assert!(ReadChunk::new(Arc::from(&b"abc"[..]), 0..4, 0).is_err());
        assert!(ReadChunk::new(Arc::from(&b"abc"[..]), 0..3, u64::MAX).is_err());
        assert!(ReadChunk::empty(u64::MAX).is_empty());
    }
}
