//! Read-only paged byte source backed by an immutable file handle. Cache misses
//! can block and belong in workers, never rendering or frontend input preparation.
use jc_core::source::{ImmutableByteSource, ReadChunk, SourceReadError, MAX_READ_CHUNK_BYTES};
#[cfg(all(windows, target_pointer_width = "64"))]
use std::fs::File;
use std::{
    io,
    path::Path,
    sync::{Arc, Mutex, MutexGuard},
};

#[derive(Debug, Clone, Copy)]
/// Payload budget. At most128 small page records/Arc headers are accounted
/// separately; this is not a private-process RAM or OS page-cache guarantee.
pub struct PageCacheLimits {
    pub max_pages: usize,
}
impl Default for PageCacheLimits {
    fn default() -> Self {
        Self { max_pages: 128 }
    }
}
impl PageCacheLimits {
    fn valid(self) -> bool {
        (1..=128).contains(&self.max_pages)
    }
    pub fn budget_bytes(self) -> usize {
        self.max_pages.saturating_mul(MAX_READ_CHUNK_BYTES)
    }
}
#[derive(Debug, Clone, Copy, Default)]
pub struct PageCacheStatistics {
    pub source_len: u64,
    pub bytes_read: u64,
    pub read_calls: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub busy: u64,
    pub pages: usize,
    pub pinned_pages: usize,
    pub retained_bytes: usize,
    pub budget_bytes: usize,
}
struct Page {
    #[cfg(all(windows, target_pointer_width = "64"))]
    offset: u64,
    bytes: Arc<[u8]>,
    #[cfg(all(windows, target_pointer_width = "64"))]
    used: u64,
}
struct Cache {
    pages: Vec<Page>,
    #[cfg(all(windows, target_pointer_width = "64"))]
    tick: u64,
    statistics: PageCacheStatistics,
}
pub struct PagedFileSource {
    #[cfg(all(windows, target_pointer_width = "64"))]
    file: File,
    len: u64,
    #[cfg(all(windows, target_pointer_width = "64"))]
    limits: PageCacheLimits,
    cache: Mutex<Cache>,
}
impl PagedFileSource {
    /// Windows x64 only. The retained read handle denies writable handles and
    /// mappings while permitting pathname replacement; old bytes remain tied to
    /// that handle. No mmap/snapshot copy or whole-file scan is performed.
    pub fn open(path: &Path, limits: PageCacheLimits) -> Result<Self, SourceReadError> {
        if !limits.valid() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "La caché de fuente admite de1 a128 páginas",
            )
            .into());
        }
        #[cfg(all(windows, target_pointer_width = "64"))]
        {
            use std::os::windows::fs::OpenOptionsExt;
            let file = std::fs::OpenOptions::new()
                .read(true)
                .share_mode(0x1 | 0x4)
                .custom_flags(0x0020_0000 | 0x1000_0000)
                .open(path)?;
            Self::from_file(file, limits)
        }
        #[cfg(not(all(windows, target_pointer_width = "64")))]
        {
            let _ = path;
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "La fuente paginada requiere Windows de64 bits",
            )
            .into())
        }
    }

    /// The caller owns an exclusive private handle or a stable read handle.
    /// The source consumes it and never exposes a writable handle.
    #[cfg(all(windows, target_pointer_width = "64"))]
    pub(crate) fn from_file(file: File, limits: PageCacheLimits) -> Result<Self, SourceReadError> {
        use std::os::windows::fs::MetadataExt;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.file_attributes() & 0x400 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "La fuente debe ser un archivo regular sin punto de reanálisis",
            )
            .into());
        }
        let len = metadata.len();
        let mut pages = Vec::new();
        pages.try_reserve_exact(limits.max_pages).map_err(|_| {
            io::Error::new(
                io::ErrorKind::OutOfMemory,
                "No se pudo reservar el catálogo de páginas",
            )
        })?;
        Ok(Self {
            file,
            len,
            limits,
            cache: Mutex::new(Cache {
                pages,
                tick: 0,
                statistics: PageCacheStatistics {
                    source_len: len,
                    budget_bytes: limits.budget_bytes(),
                    ..Default::default()
                },
            }),
        })
    }
    fn lock(&self) -> Result<MutexGuard<'_, Cache>, SourceReadError> {
        self.cache
            .lock()
            .map_err(|_| io::Error::other("Se interrumpió el estado de la caché de fuente").into())
    }
    pub fn statistics(&self) -> Result<PageCacheStatistics, SourceReadError> {
        let cache = self.lock()?;
        Ok(PageCacheStatistics {
            pages: cache.pages.len(),
            pinned_pages: cache
                .pages
                .iter()
                .filter(|p| Arc::strong_count(&p.bytes) > 1)
                .count(),
            retained_bytes: cache.pages.iter().map(|p| p.bytes.len()).sum(),
            ..cache.statistics
        })
    }
    #[cfg(all(windows, target_pointer_width = "64"))]
    fn read_page(
        &self,
        offset: u64,
        bytes: &mut [u8],
        statistics: &mut PageCacheStatistics,
    ) -> Result<(), SourceReadError> {
        use std::os::windows::fs::FileExt;
        let mut filled = 0;
        while filled < bytes.len() {
            let at = offset
                .checked_add(filled as u64)
                .ok_or(SourceReadError::InvalidRange {
                    offset,
                    requested: bytes.len(),
                    source_len: self.len,
                })?;
            statistics.read_calls = statistics.read_calls.saturating_add(1);
            match self.file.seek_read(&mut bytes[filled..], at) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "La fuente terminó antes de su longitud inmutable",
                    )
                    .into())
                }
                Ok(count) => {
                    filled += count;
                    statistics.bytes_read = statistics.bytes_read.saturating_add(count as u64);
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }
}
impl ImmutableByteSource for PagedFileSource {
    fn len(&self) -> u64 {
        self.len
    }
    fn read_chunk(&self, offset: u64, max_len: usize) -> Result<ReadChunk, SourceReadError> {
        if offset > self.len
            || max_len > MAX_READ_CHUNK_BYTES
            || offset.checked_add(max_len as u64).is_none()
        {
            return Err(SourceReadError::InvalidRange {
                offset,
                requested: max_len,
                source_len: self.len,
            });
        }
        if offset == self.len || max_len == 0 {
            return Ok(ReadChunk::empty(offset));
        }
        #[cfg(all(windows, target_pointer_width = "64"))]
        {
            let page_offset = offset / MAX_READ_CHUNK_BYTES as u64 * MAX_READ_CHUNK_BYTES as u64;
            let mut cache = self.lock()?;
            cache.tick = cache.tick.saturating_add(1);
            let tick = cache.tick;
            let index =
                if let Some(index) = cache.pages.iter().position(|p| p.offset == page_offset) {
                    cache.statistics.cache_hits = cache.statistics.cache_hits.saturating_add(1);
                    index
                } else {
                    cache.statistics.cache_misses = cache.statistics.cache_misses.saturating_add(1);
                    if cache.pages.len() >= self.limits.max_pages {
                        let evict = cache
                            .pages
                            .iter()
                            .enumerate()
                            .filter(|(_, page)| Arc::strong_count(&page.bytes) == 1)
                            .min_by_key(|(_, page)| page.used)
                            .map(|(index, _)| index);
                        let Some(evict) = evict else {
                            cache.statistics.busy = cache.statistics.busy.saturating_add(1);
                            return Err(SourceReadError::Busy {
                                retained_bytes: cache.pages.iter().map(|p| p.bytes.len()).sum(),
                                budget_bytes: self.limits.budget_bytes(),
                            });
                        };
                        cache.pages.remove(evict);
                    }
                    let count = (self.len - page_offset).min(MAX_READ_CHUNK_BYTES as u64) as usize;
                    // One Arc allocation, not Vec followed by a second page copy.
                    // The full page is initialized before exposing it as [u8].
                    let mut page = Arc::<[u8]>::new_uninit_slice(count);
                    for byte in Arc::get_mut(&mut page).expect("new exclusive page") {
                        byte.write(0);
                    }
                    let mut page = unsafe { page.assume_init() };
                    self.read_page(
                        page_offset,
                        Arc::get_mut(&mut page).expect("new exclusive page"),
                        &mut cache.statistics,
                    )?;
                    cache.pages.push(Page {
                        offset: page_offset,
                        bytes: page,
                        used: tick,
                    });
                    cache.pages.len() - 1
                };
            let page = &mut cache.pages[index];
            page.used = tick;
            let start = (offset - page_offset) as usize;
            let end = start.saturating_add(max_len).min(page.bytes.len());
            ReadChunk::new(page.bytes.clone(), start..end, offset)
        }
        #[cfg(not(all(windows, target_pointer_width = "64")))]
        {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "La fuente paginada requiere Windows de64 bits",
            )
            .into())
        }
    }
}
