//! Streaming form of the existing content fingerprint. Chunk boundaries do not
//! change its value. File reads and save writes need no source-sized temporary.
use crate::{ExternalState, FileError};
use std::{
    fs::File,
    io::{self, Read, Write},
    path::Path,
};

const READ_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContentFingerprint {
    len: u64,
    first: u64,
    second: u64,
}
impl ContentFingerprint {
    pub(crate) fn words(self) -> (u64, u64, u64) {
        (self.len, self.first, self.second)
    }
    pub(crate) fn from_words(len: u64, first: u64, second: u64) -> Self {
        Self { len, first, second }
    }
    pub(crate) fn empty() -> Self {
        Self {
            len: 0,
            first: 0xcbf2_9ce4_8422_2325,
            second: 0x9e37_79b9_7f4a_7c15,
        }
    }
    pub(crate) fn update(&mut self, bytes: &[u8]) {
        self.len = self.len.saturating_add(bytes.len() as u64);
        for byte in bytes {
            self.first ^= u64::from(*byte);
            self.first = self.first.wrapping_mul(0x0000_0100_0000_01b3);
            self.second ^= u64::from(*byte).wrapping_add(0x9e37_79b9_7f4a_7c15);
            self.second = self
                .second
                .rotate_left(13)
                .wrapping_mul(0xbf58_476d_1ce4_e5b9);
        }
    }
    pub fn of(bytes: &[u8]) -> Self {
        Self::of_chunks([bytes])
    }
    /// Identical to `of` over the concatenated bytes, without flattening them.
    pub fn of_chunks<'a>(chunks: impl IntoIterator<Item = &'a [u8]>) -> Self {
        let mut fingerprint = Self::empty();
        for bytes in chunks {
            fingerprint.update(bytes);
        }
        fingerprint
    }
    /// Read to EOF with fixed-size scratch. Read errors discard the incomplete
    /// fingerprint; Interrupted and short reads preserve ordinary Read semantics.
    pub fn of_reader<R: Read + ?Sized>(reader: &mut R) -> io::Result<Self> {
        let mut fingerprint = Self::empty();
        let mut bytes = [0u8; READ_BYTES];
        loop {
            match reader.read(&mut bytes) {
                Ok(0) => return Ok(fingerprint),
                Ok(count) => fingerprint.update(&bytes[..count]),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            }
        }
    }
}

/// This remains a content check, not a filesystem CAS. Size/mtime observed to
/// change during hashing are rejected rather than publishing a mixed version.
/// The one-byte guard bounds reading even when another writer keeps appending.
pub(crate) fn fingerprint_file(path: &Path) -> Result<ContentFingerprint, FileError> {
    let mut file = File::open(path)?;
    let before = file.metadata()?;
    let fingerprint = ContentFingerprint::of_reader(
        &mut Read::by_ref(&mut file).take(before.len().saturating_add(1)),
    )?;
    let after = file.metadata()?;
    if fingerprint.len != before.len()
        || before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
    {
        return Err(FileError::ExternalModification(ExternalState::Changed));
    }
    Ok(fingerprint)
}

pub(crate) struct FingerprintingWriter<W> {
    inner: W,
    fingerprint: ContentFingerprint,
}
impl<W: Write> FingerprintingWriter<W> {
    pub(crate) fn new(inner: W) -> Self {
        Self {
            inner,
            fingerprint: ContentFingerprint::empty(),
        }
    }
    pub(crate) fn fingerprint(&self) -> ContentFingerprint {
        self.fingerprint
    }
}
impl<W: Write> Write for FingerprintingWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let count = self.inner.write(bytes)?;
        self.fingerprint.update(&bytes[..count]);
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fragmented<'a> {
        bytes: &'a [u8],
        interrupted: bool,
        largest_request: usize,
        fail_at: Option<usize>,
        read: usize,
    }
    impl Read for Fragmented<'_> {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            self.largest_request = self.largest_request.max(buffer.len());
            if !self.interrupted {
                self.interrupted = true;
                return Err(io::ErrorKind::Interrupted.into());
            }
            if self.fail_at.is_some_and(|at| self.read >= at) {
                return Err(io::Error::other("Fallo de lectura inyectado"));
            }
            let count = self.bytes.len().min(buffer.len()).min(7);
            buffer[..count].copy_from_slice(&self.bytes[..count]);
            self.bytes = &self.bytes[count..];
            self.read += count;
            Ok(count)
        }
    }
    #[test]
    fn streaming_fingerprint_handles_short_reads_interruption_and_failure_without_changing_signature(
    ) {
        let data: Vec<_> = (0..150_000).map(|i| (i % 256) as u8).collect();
        let mut reader = Fragmented {
            bytes: &data,
            interrupted: false,
            largest_request: 0,
            fail_at: None,
            read: 0,
        };
        assert_eq!(
            ContentFingerprint::of_reader(&mut reader).unwrap(),
            ContentFingerprint::of(&data)
        );
        assert!(reader.largest_request <= READ_BYTES);
        reader.bytes = &data;
        reader.read = 0;
        reader.fail_at = Some(70_000);
        assert!(ContentFingerprint::of_reader(&mut reader).is_err());
    }
    #[test]
    fn streaming_writer_hashes_only_accepted_bytes_across_short_writes() {
        struct Short(Vec<u8>);
        impl Write for Short {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                let count = bytes.len().min(3);
                self.0.extend_from_slice(&bytes[..count]);
                Ok(count)
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let bytes = b"\xef\xbb\xbfA\r\n\x00\xff";
        let mut writer = FingerprintingWriter::new(Short(Vec::new()));
        writer.write_all(bytes).unwrap();
        assert_eq!(writer.inner.0, bytes);
        assert_eq!(writer.fingerprint(), ContentFingerprint::of(bytes));
        // Recorded signature of the pre-streaming algorithm for these bytes.
        assert_eq!(
            writer.fingerprint(),
            ContentFingerprint {
                len: 8,
                first: 0x3031_d04a_0b64_7edd,
                second: 0x8eb0_7f0c_56df_1dbb,
            }
        );
    }
}
