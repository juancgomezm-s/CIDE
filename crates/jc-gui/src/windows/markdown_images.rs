//! Explicit, bounded Markdown image loading. No source I/O or decoding in paint.
//! One lazy worker owns native codec use; only `poll` accepts a current batch.
//! Accepted decoded payload and in-flight/completed payload are each capped at
//! 16MiB; they can coexist (up to32MiB before shared-Arc deduplication). Encoded
//! captures, native codec allocations and frontend-held frames are separate;
//! this is a logical payload limit, never a process-RSS guarantee.
use jc_app::{TaskStamp, Wakeup};
use std::{
    ffi::c_void,
    path::{Component, PathBuf},
    ptr::null_mut,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Condvar, Mutex,
    },
    thread::{self, JoinHandle},
};

const FILE_BYTES: usize = 8 * 1024 * 1024;
const PIXEL_BYTES: usize = 16 * 1024 * 1024;
const PIXELS: usize = 4 * 1024 * 1024;
const DIMENSION: u32 = 4096;
const FILES: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ImageSource {
    Local(PathBuf),
    Remote(String),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ImageContext {
    pub stamp: TaskStamp,
    /// Canonical absolute confinement root for local resources.
    pub root: PathBuf,
    /// RGB canvas color; decoding composites alpha once, outside painting.
    pub background: [u8; 3],
}
#[derive(Debug)]
pub(super) struct DecodedImage {
    pub width: u32,
    pub height: u32,
    /// Top-down BGRA, alpha255; suitable for immutable StretchDIBits input.
    pub bgra: Vec<u8>,
}
#[derive(Clone, Debug)]
pub(super) struct ImageEntry {
    pub source: ImageSource,
    pub result: Result<Arc<DecodedImage>, String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Wanted {
    context: ImageContext,
    sources: Vec<ImageSource>,
}
struct Request {
    generation: u64,
    wanted: Wanted,
    /// Immutable entries already accepted by the owner under this exact context.
    /// Sharing them preserves geometry and avoids reading them again on reflow.
    known: Vec<ImageEntry>,
}
struct Completion {
    generation: u64,
    context: ImageContext,
    entries: Vec<ImageEntry>,
}
#[derive(Default)]
struct Queue {
    latest: Option<Request>,
    completion: Option<Completion>,
}
struct Shared {
    queue: Mutex<Queue>,
    changed: Condvar,
    generation: AtomicU64,
    stop: AtomicBool,
    wake: Wakeup,
}
pub(super) struct ImageController {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
    wanted: Option<Wanted>,
    entries: Vec<ImageEntry>,
    pending: bool,
}
impl ImageController {
    pub(super) fn new(wake: Wakeup) -> Self {
        Self {
            shared: Arc::new(Shared {
                queue: Mutex::new(Queue::default()),
                changed: Condvar::new(),
                generation: AtomicU64::new(0),
                stop: AtomicBool::new(false),
                wake,
            }),
            worker: None,
            wanted: None,
            entries: Vec::new(),
            pending: false,
        }
    }
    /// Called from an explicit UI/event admission route, never prepare/draw.
    /// Equal context+resource sets are deduplicated, including failed loads.
    pub(super) fn request(
        &mut self,
        mut context: ImageContext,
        mut sources: Vec<ImageSource>,
    ) -> Result<bool, String> {
        if !context.root.is_absolute() || context.root.as_os_str().len() > 4096 {
            return Err("Raíz de imágenes inválida".into());
        }
        if sources.len() > FILES {
            return Err("La vista admite hasta16 imágenes por lote".into());
        }
        for source in &mut sources {
            match source {
                ImageSource::Local(path)
                    if path.is_absolute()
                        && path.starts_with(&context.root)
                        && path.as_os_str().len() <= 4096
                        && !path
                            .components()
                            .any(|c| matches!(c, Component::ParentDir | Component::CurDir)) =>
                {
                    *path = std::mem::take(path).into_boxed_path().into_path_buf();
                }
                ImageSource::Remote(url)
                    if url.len() <= 4096
                        && (url
                            .get(..8)
                            .is_some_and(|s| s.eq_ignore_ascii_case("https://"))
                            || url
                                .get(..7)
                                .is_some_and(|s| s.eq_ignore_ascii_case("http://")))
                        && !url.chars().any(char::is_control) =>
                {
                    *url = std::mem::take(url).into_boxed_str().into_string();
                }
                _ => return Err("Destino de imagen fuera del alcance admitido".into()),
            }
        }
        // Resource order is stable display metadata, not a cache key per span.
        let mut unique = Vec::with_capacity(sources.len());
        for source in sources.drain(..) {
            if !unique.contains(&source) {
                unique.push(source);
            }
        }
        context.root = context.root.into_boxed_path().into_path_buf();
        let wanted = Wanted {
            context,
            sources: unique,
        };
        if self.wanted.as_ref() == Some(&wanted) {
            return Ok(false);
        }
        if self
            .wanted
            .as_ref()
            .is_some_and(|previous| previous.context == wanted.context)
        {
            self.entries
                .retain(|entry| wanted.sources.contains(&entry.source));
        } else {
            self.entries.clear();
        }
        let missing = wanted
            .sources
            .iter()
            .any(|source| !self.entries.iter().any(|entry| &entry.source == source));
        if self.worker.is_none() && missing {
            let shared = self.shared.clone();
            self.worker = Some(
                thread::Builder::new()
                    .name("cide-markdown-images".into())
                    .spawn(move || worker(shared))
                    .map_err(|e| format!("No se pudo iniciar lector de imágenes: {e}"))?,
            );
        }
        let generation = self
            .shared
            .generation
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                value.checked_add(1)
            })
            .map_err(|_| "Se agotó la generación de imágenes".to_string())?
            + 1;
        self.pending = missing;
        self.wanted = Some(wanted.clone());
        let mut queue = self.shared.queue.lock().unwrap_or_else(|e| e.into_inner());
        queue.completion = None;
        queue.latest = self.pending.then(|| Request {
            generation,
            wanted,
            known: self.entries.clone(),
        });
        drop(queue);
        self.shared.changed.notify_one();
        Ok(true)
    }
    pub(super) fn cancel(&mut self) {
        // A source frame can revisit this route while no Markdown view exists.
        // Invalidation and queue locking are needed only for an admitted owner.
        if self.wanted.is_none() && self.entries.is_empty() && !self.pending {
            return;
        }
        let _ = self
            .shared
            .generation
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                value.checked_add(1)
            });
        self.wanted = None;
        self.entries.clear();
        self.pending = false;
        let mut queue = self.shared.queue.lock().unwrap_or_else(|e| e.into_inner());
        queue.latest = None;
        queue.completion = None;
    }
    #[cfg(test)]
    pub(super) fn loading(&self) -> bool {
        self.pending
    }
    pub(super) fn get(&self, source: &ImageSource) -> Option<&ImageEntry> {
        self.entries.iter().find(|entry| &entry.source == source)
    }
    /// Sole completion acceptance. A new document/revision/root/theme cannot
    /// consume a previous image batch, even when paths are identical.
    pub(super) fn poll(&mut self, current: &ImageContext) -> bool {
        if self.wanted.as_ref().is_some_and(|w| &w.context != current) {
            self.cancel();
            return true;
        }
        let completion = self
            .shared
            .queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .completion
            .take();
        let Some(completion) = completion else {
            return false;
        };
        if completion.generation != self.shared.generation.load(Ordering::Acquire)
            || &completion.context != current
            || self
                .wanted
                .as_ref()
                .is_none_or(|w| w.context != completion.context)
        {
            return false;
        }
        let charge = completion
            .entries
            .iter()
            .filter_map(|entry| entry.result.as_ref().ok())
            .map(|image| image.bgra.capacity())
            .sum::<usize>();
        if completion.entries.len() > FILES || charge > PIXEL_BYTES {
            self.cancel();
            return true;
        }
        self.entries = completion.entries;
        self.pending = false;
        true
    }
}
impl Drop for ImageController {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        let _ = self
            .shared
            .generation
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                value.checked_add(1)
            });
        self.shared.changed.notify_one();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn worker(shared: Arc<Shared>) {
    let runtime = Runtime::new();
    loop {
        let request = {
            let mut queue = shared.queue.lock().unwrap_or_else(|e| e.into_inner());
            while queue.latest.is_none() && !shared.stop.load(Ordering::Acquire) {
                queue = shared
                    .changed
                    .wait(queue)
                    .unwrap_or_else(|e| e.into_inner());
            }
            if shared.stop.load(Ordering::Acquire) {
                return;
            }
            queue.latest.take().unwrap()
        };
        let mut cancelled = || {
            shared.stop.load(Ordering::Acquire)
                || shared.generation.load(Ordering::Acquire) != request.generation
        };
        let context = request.wanted.context;
        let mut entries = request.known;
        entries.reserve_exact(request.wanted.sources.len().saturating_sub(entries.len()));
        let mut retained = entries
            .iter()
            .filter_map(|entry| entry.result.as_ref().ok())
            .map(|image| image.bgra.capacity())
            .sum::<usize>();
        for source in request.wanted.sources {
            if cancelled() {
                break;
            }
            if entries.iter().any(|entry| entry.source == source) {
                continue;
            }
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if let Err(error) = &runtime {
                    return Err(error.clone());
                }
                if retained == PIXEL_BYTES {
                    return Err("Las imágenes visibles superan el presupuesto de16MiB".into());
                }
                let bytes = match &source {
                    ImageSource::Local(path) => match jc_fs::capture_in_root(
                        &context.root,
                        path,
                        FILE_BYTES,
                        &mut cancelled,
                    )
                    .map_err(|e| e.to_string())?
                    {
                        jc_fs::Capture::Present { bytes, .. } => bytes,
                        jc_fs::Capture::Absent { .. } => {
                            return Err("No se encontró la imagen local".into())
                        }
                    },
                    ImageSource::Remote(url) => {
                        super::markdown_image_http::download(url, &cancelled)?
                    }
                };
                if cancelled() {
                    return Err("Carga de imagen cancelada".into());
                }
                let (width, height) = dimensions(&bytes)?;
                if checked_size(width, height)? > PIXEL_BYTES.saturating_sub(retained) {
                    return Err("Las imágenes visibles superan el presupuesto de16MiB".into());
                }
                let mut image = decode(&bytes)?;
                if cancelled() {
                    return Err("Carga de imagen cancelada".into());
                }
                if image.bgra.capacity() > PIXEL_BYTES.saturating_sub(retained) {
                    return Err("Las imágenes visibles superan el presupuesto de16MiB".into());
                }
                composite(&mut image.bgra, context.background);
                retained += image.bgra.capacity();
                Ok(Arc::new(image))
            }))
            .unwrap_or_else(|_| Err("El lector de imágenes se recuperó de un fallo".into()));
            entries.push(ImageEntry { source, result });
        }
        if cancelled() {
            continue;
        }
        let mut queue = shared.queue.lock().unwrap_or_else(|e| e.into_inner());
        let published = if !cancelled() {
            queue.completion = Some(Completion {
                generation: request.generation,
                context,
                entries,
            });
            true
        } else {
            false
        };
        drop(queue);
        if published {
            shared.wake.signal();
        }
    }
}

fn composite(bytes: &mut [u8], rgb: [u8; 3]) {
    for pixel in bytes.chunks_exact_mut(4) {
        let alpha = u32::from(pixel[3]);
        for (channel, background) in pixel[..3].iter_mut().zip(rgb.into_iter().rev()) {
            *channel = ((u32::from(*channel) * alpha + u32::from(background) * (255 - alpha) + 127)
                / 255) as u8;
        }
        pixel[3] = 255;
    }
}
fn checked_size(width: u32, height: u32) -> Result<usize, String> {
    if width == 0 || height == 0 || width > DIMENSION || height > DIMENSION {
        return Err("La imagen debe medir entre1 y4096 píxeles por lado".into());
    }
    (width as usize)
        .checked_mul(height as usize)
        .filter(|n| *n <= PIXELS)
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(|| "La imagen supera4millones de píxeles".into())
}
/// Header admission precedes native decoder creation, including compressed input.
fn dimensions(bytes: &[u8]) -> Result<(u32, u32), String> {
    if bytes.is_empty() || bytes.len() > FILE_BYTES {
        return Err("Imagen vacía o mayor de8MiB".into());
    }
    let dimensions = if bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        && bytes.len() >= 24
        && &bytes[12..16] == b"IHDR"
    {
        Some((
            u32::from_be_bytes(bytes[16..20].try_into().unwrap()),
            u32::from_be_bytes(bytes[20..24].try_into().unwrap()),
        ))
    } else if (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")) && bytes.len() >= 10 {
        Some((
            u32::from(u16::from_le_bytes(bytes[6..8].try_into().unwrap())),
            u32::from(u16::from_le_bytes(bytes[8..10].try_into().unwrap())),
        ))
    } else if bytes.starts_with(b"BM") && bytes.len() >= 26 {
        let header = u32::from_le_bytes(bytes[14..18].try_into().unwrap());
        if header == 12 {
            Some((
                u32::from(u16::from_le_bytes(bytes[18..20].try_into().unwrap())),
                u32::from(u16::from_le_bytes(bytes[20..22].try_into().unwrap())),
            ))
        } else if header >= 40 {
            let width = i32::from_le_bytes(bytes[18..22].try_into().unwrap());
            let height = i32::from_le_bytes(bytes[22..26].try_into().unwrap());
            (width > 0).then_some((width as u32, height.unsigned_abs()))
        } else {
            None
        }
    } else if bytes.starts_with(&[0xff, 0xd8]) {
        jpeg_dimensions(bytes)
    } else {
        None
    };
    let (width, height) =
        dimensions.ok_or("Formato no admitido o cabecera incompleta; usa PNG, JPEG, GIF o BMP")?;
    checked_size(width, height)?;
    Ok((width, height))
}
fn jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut at = 2;
    while at < bytes.len() {
        if bytes[at] != 0xff {
            return None;
        }
        while bytes.get(at) == Some(&0xff) {
            at += 1;
        }
        let marker = *bytes.get(at)?;
        at += 1;
        if marker == 0xd9 || marker == 0xda {
            return None;
        }
        if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        let length = usize::from(u16::from_be_bytes(bytes.get(at..at + 2)?.try_into().ok()?));
        if length < 2 || at.checked_add(length)? > bytes.len() {
            return None;
        }
        if matches!(marker,0xc0..=0xc3|0xc5..=0xc7|0xc9..=0xcb|0xcd..=0xcf) && length >= 7 {
            return Some((
                u32::from(u16::from_be_bytes(bytes[at + 5..at + 7].try_into().ok()?)),
                u32::from(u16::from_be_bytes(bytes[at + 3..at + 5].try_into().ok()?)),
            ));
        }
        at += length;
    }
    None
}

type Native = *mut c_void;
#[repr(C)]
struct StartupInput {
    version: u32,
    callback: Native,
    suppress_thread: i32,
    suppress_codecs: i32,
}
#[repr(C)]
struct NativeRect {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}
#[repr(C)]
#[derive(Default)]
struct BitmapData {
    width: u32,
    height: u32,
    stride: i32,
    format: i32,
    scan: Native,
    reserved: usize,
}
#[link(name = "gdiplus")]
extern "system" {
    fn GdiplusStartup(token: *mut usize, input: *const StartupInput, output: Native) -> i32;
    fn GdiplusShutdown(token: usize);
    fn GdipCreateBitmapFromStream(stream: Native, bitmap: *mut Native) -> i32;
    fn GdipGetImageWidth(image: Native, width: *mut u32) -> i32;
    fn GdipGetImageHeight(image: Native, height: *mut u32) -> i32;
    fn GdipBitmapLockBits(
        bitmap: Native,
        rect: *const NativeRect,
        flags: u32,
        format: i32,
        data: *mut BitmapData,
    ) -> i32;
    fn GdipBitmapUnlockBits(bitmap: Native, data: *mut BitmapData) -> i32;
    fn GdipDisposeImage(image: Native) -> i32;
}
#[link(name = "ole32")]
extern "system" {
    fn CoInitializeEx(reserved: Native, mode: u32) -> i32;
    fn CoUninitialize();
    fn CreateStreamOnHGlobal(memory: Native, delete: i32, stream: *mut Native) -> i32;
}
#[link(name = "kernel32")]
extern "system" {
    fn GlobalAlloc(flags: u32, bytes: usize) -> Native;
    fn GlobalLock(memory: Native) -> Native;
    fn GlobalUnlock(memory: Native) -> i32;
    fn GlobalFree(memory: Native) -> Native;
}
struct Runtime {
    token: usize,
}
impl Runtime {
    fn new() -> Result<Self, String> {
        unsafe {
            if CoInitializeEx(null_mut(), 0) < 0 {
                return Err("No se pudo iniciar el contexto de imágenes".into());
            }
            let input = StartupInput {
                version: 1,
                callback: null_mut(),
                suppress_thread: 0,
                suppress_codecs: 1,
            };
            let mut token = 0;
            if GdiplusStartup(&mut token, &input, null_mut()) != 0 {
                CoUninitialize();
                return Err("No se pudo iniciar el decodificador de imágenes".into());
            }
            Ok(Self { token })
        }
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        unsafe {
            GdiplusShutdown(self.token);
            CoUninitialize();
        }
    }
}
struct Stream(Native);
impl Stream {
    unsafe fn set_len(&self, len: u64) -> Result<(), String> {
        let vtable = *(self.0 as *const *const Native);
        let set_size = std::mem::transmute::<Native, unsafe extern "system" fn(Native, u64) -> i32>(
            *vtable.add(6),
        );
        if set_size(self.0, len) < 0 {
            Err("No se pudo limitar la fuente de imagen".into())
        } else {
            Ok(())
        }
    }
}
impl Drop for Stream {
    fn drop(&mut self) {
        unsafe {
            let vtable = *(self.0 as *const *const Native);
            let release = std::mem::transmute::<Native, unsafe extern "system" fn(Native) -> u32>(
                *vtable.add(2),
            );
            release(self.0);
        }
    }
}
struct Bitmap(Native);
impl Drop for Bitmap {
    fn drop(&mut self) {
        unsafe {
            GdipDisposeImage(self.0);
        }
    }
}
struct Locked<'a> {
    bitmap: &'a Bitmap,
    data: BitmapData,
}
impl Drop for Locked<'_> {
    fn drop(&mut self) {
        unsafe {
            GdipBitmapUnlockBits(self.bitmap.0, &mut self.data);
        }
    }
}
/// Runtime must remain alive on this worker until all native objects are dropped.
fn decode(bytes: &[u8]) -> Result<DecodedImage, String> {
    let expected = dimensions(bytes)?;
    unsafe {
        let memory = GlobalAlloc(0x42, bytes.len());
        if memory.is_null() {
            return Err("No se pudo reservar memoria para la imagen".into());
        }
        let pointer = GlobalLock(memory);
        if pointer.is_null() {
            GlobalFree(memory);
            return Err("No se pudo fijar la memoria de imagen".into());
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), pointer.cast::<u8>(), bytes.len());
        GlobalUnlock(memory);
        let mut stream = null_mut();
        if CreateStreamOnHGlobal(memory, 1, &mut stream) < 0 || stream.is_null() {
            GlobalFree(memory);
            return Err("No se pudo crear la fuente de imagen".into());
        }
        let stream = Stream(stream);
        stream.set_len(bytes.len() as u64)?;
        let mut bitmap = null_mut();
        if GdipCreateBitmapFromStream(stream.0, &mut bitmap) != 0 || bitmap.is_null() {
            if !bitmap.is_null() {
                GdipDisposeImage(bitmap);
            }
            return Err("No se pudo decodificar la imagen".into());
        }
        let bitmap = Bitmap(bitmap);
        let (mut width, mut height) = (0, 0);
        if GdipGetImageWidth(bitmap.0, &mut width) != 0
            || GdipGetImageHeight(bitmap.0, &mut height) != 0
            || (width, height) != expected
        {
            return Err("La geometría decodificada no coincide con su cabecera".into());
        }
        let len = checked_size(width, height)?;
        let rectangle = NativeRect {
            x: 0,
            y: 0,
            width: width as i32,
            height: height as i32,
        };
        let mut data = BitmapData::default();
        if GdipBitmapLockBits(bitmap.0, &rectangle, 1, 0x0026_200a, &mut data) != 0 {
            return Err("No se pudieron leer los píxeles de imagen".into());
        }
        let locked = Locked {
            bitmap: &bitmap,
            data,
        };
        if locked.data.scan.is_null()
            || locked.data.width != width
            || locked.data.height != height
            || (locked.data.stride.unsigned_abs() as usize) != width as usize * 4
        {
            return Err("Stride de imagen inválido".into());
        }
        let mut bgra = vec![0; len];
        for y in 0..height as usize {
            let row =
                (locked.data.scan as *const u8).offset(y as isize * locked.data.stride as isize);
            std::ptr::copy_nonoverlapping(
                row,
                bgra[y * width as usize * 4..].as_mut_ptr(),
                width as usize * 4,
            );
        }
        Ok(DecodedImage {
            width,
            height,
            bgra,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
    fn bmp() -> Vec<u8> {
        let mut bytes = vec![0; 58];
        bytes[..2].copy_from_slice(b"BM");
        bytes[2..6].copy_from_slice(&58u32.to_le_bytes());
        bytes[10..14].copy_from_slice(&54u32.to_le_bytes());
        bytes[14..18].copy_from_slice(&40u32.to_le_bytes());
        bytes[18..22].copy_from_slice(&1u32.to_le_bytes());
        bytes[22..26].copy_from_slice(&1u32.to_le_bytes());
        bytes[26..28].copy_from_slice(&1u16.to_le_bytes());
        bytes[28..30].copy_from_slice(&24u16.to_le_bytes());
        bytes[54..58].copy_from_slice(&[3, 2, 1, 0]);
        bytes
    }
    fn png() -> Vec<u8> {
        fn crc(bytes: &[u8]) -> u32 {
            let mut crc = !0u32;
            for b in bytes {
                crc ^= u32::from(*b);
                for _ in 0..8 {
                    crc = (crc >> 1) ^ ((0u32.wrapping_sub(crc & 1)) & 0xedb8_8320);
                }
            }
            !crc
        }
        fn chunk(out: &mut Vec<u8>, name: &[u8; 4], body: &[u8]) {
            out.extend_from_slice(&(body.len() as u32).to_be_bytes());
            let start = out.len();
            out.extend_from_slice(name);
            out.extend_from_slice(body);
            out.extend_from_slice(&crc(&out[start..]).to_be_bytes());
        }
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut header = Vec::new();
        header.extend_from_slice(&1u32.to_be_bytes());
        header.extend_from_slice(&1u32.to_be_bytes());
        header.extend_from_slice(&[8, 6, 0, 0, 0]);
        chunk(&mut bytes, b"IHDR", &header);
        let pixels = [0, 255, 0, 0, 128];
        let (mut a, mut b) = (1u32, 0u32);
        for byte in pixels {
            a = (a + u32::from(byte)) % 65521;
            b = (b + a) % 65521;
        }
        let mut z = vec![0x78, 0x01, 0x01, 5, 0, 250, 255];
        z.extend_from_slice(&pixels);
        z.extend_from_slice(&((b << 16) | a).to_be_bytes());
        chunk(&mut bytes, b"IDAT", &z);
        chunk(&mut bytes, b"IEND", &[]);
        bytes
    }
    #[test]
    fn markdown_images_native_png_bmp_alpha_and_header_limits() {
        let _runtime = Runtime::new().unwrap();
        let image = decode(&bmp()).unwrap();
        assert_eq!((image.width, image.height), (1, 1));
        assert_eq!(&image.bgra, [3, 2, 1, 255].as_slice());
        let mut image = decode(&png()).unwrap();
        composite(&mut image.bgra, [20, 40, 60]);
        assert_eq!(&image.bgra, [30, 20, 138, 255].as_slice());
        assert!(decode(b"not an image").is_err());
        assert!(decode(&png()[..24]).is_err());
        let mut large = png();
        large[16..20].copy_from_slice(&4097u32.to_be_bytes());
        assert!(dimensions(&large).is_err());
        large[16..20].copy_from_slice(&4096u32.to_be_bytes());
        large[20..24].copy_from_slice(&4096u32.to_be_bytes());
        assert!(dimensions(&large).is_err());
        for n in 0..bmp().len().min(26) {
            assert!(dimensions(&bmp()[..n]).is_err());
        }
    }
    #[test]
    fn markdown_images_worker_poll_dedup_stale_context_and_confined_sources() {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../work/test-temp");
        std::fs::create_dir_all(&base).unwrap();
        let root = base.canonicalize().unwrap().join(format!(
            "markdown-images-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("pixel.bmp");
        std::fs::write(&path, bmp()).unwrap();
        let mut app = jc_app::Workbench::default();
        let tab = app.new_file("image.md", jc_text::TextEncoding::Utf8);
        let doc = app.document_by_tab(tab).unwrap();
        let stamp = TaskStamp {
            document: doc.id,
            revision: doc.session.document().revision(),
            encoding: jc_text::TextEncoding::Utf8,
            view: None,
        };
        let context = ImageContext {
            stamp,
            root: root.clone(),
            background: [20, 40, 60],
        };
        let source = ImageSource::Local(path.clone());
        let mut controller = ImageController::new(Wakeup::default());
        assert!(controller
            .request(context.clone(), vec![source.clone(), source.clone()])
            .unwrap());
        assert!(controller.get(&source).is_none());
        let deadline = Instant::now() + Duration::from_secs(5);
        while controller.loading() {
            controller.poll(&context);
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            controller
                .get(&source)
                .unwrap()
                .result
                .as_ref()
                .unwrap()
                .bgra,
            [3, 2, 1, 255]
        );
        assert!(!controller
            .request(context.clone(), vec![source.clone()])
            .unwrap());
        let mut other = context.clone();
        other.background = [0, 0, 0];
        assert!(controller.poll(&other));
        assert!(controller.get(&source).is_none());
        assert!(controller
            .request(
                context.clone(),
                vec![ImageSource::Local(root.join("../escape.bmp"))]
            )
            .is_err());
        assert!(controller
            .request(
                context.clone(),
                vec![ImageSource::Remote("file:///image.bmp".into())]
            )
            .is_err());
        assert!(controller
            .request(
                context.clone(),
                vec![ImageSource::Local(root.join("missing.png"))]
            )
            .unwrap());
        let deadline = Instant::now() + Duration::from_secs(5);
        while controller.loading() {
            controller.poll(&context);
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(controller
            .get(&ImageSource::Local(root.join("missing.png")))
            .unwrap()
            .result
            .is_err());
        drop(controller);
        assert_eq!(std::fs::read(&path).unwrap(), bmp());
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
    #[test]
    fn markdown_images_reflow_retains_subset_and_only_poll_publishes_new_sources() {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../work/test-temp");
        std::fs::create_dir_all(&base).unwrap();
        let base = base.canonicalize().unwrap();
        let root = base.join(format!(
            "markdown-images-reflow-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let first = root.join("first.bmp");
        let second = root.join("second.bmp");
        let third = root.join("third.bmp");
        for path in [&first, &second, &third] {
            std::fs::write(path, bmp()).unwrap();
        }
        let mut app = jc_app::Workbench::default();
        let tab = app.new_file("reflow.md", jc_text::TextEncoding::Utf8);
        let doc = app.document_by_tab(tab).unwrap();
        let stamp = TaskStamp {
            document: doc.id,
            revision: doc.session.document().revision(),
            encoding: jc_text::TextEncoding::Utf8,
            view: None,
        };
        let context = ImageContext {
            stamp,
            root: root.clone(),
            background: [20, 40, 60],
        };
        let a = ImageSource::Local(first.clone());
        let b = ImageSource::Local(second.clone());
        let c = ImageSource::Local(third.clone());
        let wake = Wakeup::default();
        let mut controller = ImageController::new(wake.clone());
        let wait = |wake: &Wakeup, ticket: u64| {
            let deadline = Instant::now() + Duration::from_secs(5);
            while wake.ticket() == ticket {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(1));
            }
        };
        let ticket = wake.ticket();
        assert!(controller
            .request(context.clone(), vec![a.clone(), b.clone()])
            .unwrap());
        wait(&wake, ticket);
        assert!(controller.get(&a).is_none());
        assert!(controller.poll(&context));
        let accepted = controller.get(&a).unwrap().result.as_ref().unwrap().clone();
        // A smaller visible set must not turn decoded geometry into placeholders
        // or reopen a deleted fixture merely because reflow changed the set.
        std::fs::remove_file(&first).unwrap();
        assert!(controller
            .request(context.clone(), vec![a.clone()])
            .unwrap());
        assert!(!controller.loading());
        assert!(!controller.poll(&context));
        assert!(Arc::ptr_eq(
            controller.get(&a).unwrap().result.as_ref().unwrap(),
            &accepted
        ));
        assert!(controller.get(&b).is_none());
        let ticket = wake.ticket();
        assert!(controller
            .request(context.clone(), vec![a.clone(), c.clone()])
            .unwrap());
        assert!(Arc::ptr_eq(
            controller.get(&a).unwrap().result.as_ref().unwrap(),
            &accepted
        ));
        assert!(controller.get(&c).is_none());
        wait(&wake, ticket);
        assert!(controller.get(&c).is_none());
        assert!(controller.poll(&context));
        assert!(controller.get(&c).unwrap().result.is_ok());
        assert!(Arc::ptr_eq(
            controller.get(&a).unwrap().result.as_ref().unwrap(),
            &accepted
        ));
        // Another root never inherits cached resources, even with the same stamp.
        let other = root.join("other");
        std::fs::create_dir(&other).unwrap();
        let other_path = other.join("pixel.bmp");
        std::fs::write(&other_path, bmp()).unwrap();
        let mut other_context = context.clone();
        other_context.root = other.clone();
        let other_source = ImageSource::Local(other_path.clone());
        assert!(controller
            .request(other_context.clone(), vec![other_source.clone()])
            .unwrap());
        assert!(controller.get(&a).is_none());
        assert!(controller.get(&c).is_none());
        assert!(controller.get(&other_source).is_none());
        assert!(controller.poll(&context)); // old owner context cancels the new root
        assert!(controller.get(&other_source).is_none());
        assert!(controller
            .request(other_context.clone(), vec![other_source.clone()])
            .unwrap());
        assert!(controller.get(&other_source).is_none());
        let deadline = Instant::now() + Duration::from_secs(5);
        while controller.loading() {
            controller.poll(&other_context);
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(controller.get(&other_source).unwrap().result.is_ok());
        drop(controller);
        assert_eq!(std::fs::read(&second).unwrap(), bmp());
        assert_eq!(std::fs::read(&third).unwrap(), bmp());
        assert_eq!(root.parent(), Some(base.as_path()));
        for path in [&second, &third, &other_path] {
            std::fs::remove_file(path).unwrap();
        }
        std::fs::remove_dir(other).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
