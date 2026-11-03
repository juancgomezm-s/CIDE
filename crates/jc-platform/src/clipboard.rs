//! Explicit-action CF_UNICODETEXT transport, not a clipboard monitor/history.
//! Call only outside State/Workbench borrows: clipboard APIs may synchronously
//! contact a delayed-rendering owner and pump messages. No API runs at startup.
//!
//! Privacy: never log/store clipboard contents here. Read returns an owned bounded
//! String to the caller; write publishes only the supplied text. Neither changes
//! documents, normalizes EOL, guesses a source code page, or preserves rectangle
//! metadata. The caller must validate source encoding/revision before editing.
//!
//! Limits: 2 MiB each for native UTF-16 allocation (including terminator/padding)
//! and UTF-8 output. These are allocation quotas, not total process-RAM limits.
//! Strict reads accept zero padding after NUL but reject nonzero trailing data;
//! producers leaving arbitrary allocator padding can therefore be rejected.
//! OpenClipboard contention returns immediately: no sleeps or busy retry loops.
//! OS delayed rendering itself has no hard deadline promised by this adapter.
//! After EmptyClipboard, a failed SetClipboardData may leave the clipboard empty;
//! there is no atomic rollback of other applications' previous clipboard formats.
//!
//! Primary ABI/ownership references:
//! https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-openclipboard
//! https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getclipboarddata
//! https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setclipboarddata
//! https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-globalunlock
//! https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-globalsize
type Hwnd = *mut std::ffi::c_void;
use std::{ffi::c_void, marker::PhantomData, ptr::NonNull, rc::Rc};

const CF_UNICODETEXT: u32 = 13;
const GMEM_MOVEABLE: u32 = 0x2;
const GMEM_ZEROINIT: u32 = 0x40;
const MAX_BYTES: usize = 2 * 1024 * 1024;
type Handle = *mut c_void;

#[link(name = "user32")]
extern "system" {
    fn OpenClipboard(owner: Hwnd) -> i32;
    fn CloseClipboard() -> i32;
    fn IsClipboardFormatAvailable(format: u32) -> i32;
    fn GetClipboardData(format: u32) -> Handle;
    fn EmptyClipboard() -> i32;
    fn SetClipboardData(format: u32, memory: Handle) -> Handle;
}
#[link(name = "kernel32")]
extern "system" {
    fn GlobalAlloc(flags: u32, bytes: usize) -> Handle;
    fn GlobalSize(memory: Handle) -> usize;
    fn GlobalLock(memory: Handle) -> *mut c_void;
    fn GlobalUnlock(memory: Handle) -> i32;
    fn GlobalFree(memory: Handle) -> Handle;
    fn SetLastError(error: u32);
    fn GetLastError() -> u32;
}

fn failed(operation: &str) -> String {
    format!("{operation}: {}", std::io::Error::last_os_error())
}
fn validate_allocation(bytes: usize) -> Result<usize, String> {
    if !(2..=MAX_BYTES).contains(&bytes) || bytes % 2 != 0 {
        return Err("La reserva UTF-16 del portapapeles no es válida o supera 2 MiB".into());
    }
    Ok(bytes / 2)
}

fn encode(text: &str) -> Result<Vec<u16>, String> {
    if text.len() > MAX_BYTES {
        return Err("El texto UTF-8 del portapapeles supera 2 MiB".into());
    }
    if text.as_bytes().contains(&0) {
        return Err("El texto del portapapeles contiene un NUL intermedio".into());
    }
    let count = text
        .encode_utf16()
        .count()
        .checked_add(1)
        .ok_or("Desbordamiento de longitud UTF-16 del portapapeles")?;
    let bytes = count
        .checked_mul(2)
        .ok_or("Desbordamiento de longitud UTF-16 del portapapeles")?;
    validate_allocation(bytes)?;
    let mut encoded = Vec::new();
    encoded
        .try_reserve_exact(count)
        .map_err(|_| "No se pudo reservar memoria para codificar el portapapeles")?;
    encoded.extend(text.encode_utf16());
    encoded.push(0);
    Ok(encoded)
}

fn decode(units: &[u16]) -> Result<String, String> {
    decode_limited(units, MAX_BYTES)
}
fn decode_limited(units: &[u16], limit: usize) -> Result<String, String> {
    let bytes = units
        .len()
        .checked_mul(2)
        .ok_or("Desbordamiento de longitud UTF-16 del portapapeles")?;
    validate_allocation(bytes)?;
    let end = units
        .iter()
        .position(|unit| *unit == 0)
        .ok_or("Al texto del portapapeles le falta el terminador NUL")?;
    if units[end + 1..].iter().any(|unit| *unit != 0) {
        return Err(
            "El portapapeles contiene un NUL intermedio o datos finales distintos de cero".into(),
        );
    }
    // Preflight strict decoding and the UTF-8 expansion before allocating it.
    let mut utf8_bytes = 0usize;
    for scalar in char::decode_utf16(units[..end].iter().copied()) {
        let scalar = scalar.map_err(|_| "El texto del portapapeles no es UTF-16 válido")?;
        utf8_bytes = utf8_bytes
            .checked_add(scalar.len_utf8())
            .filter(|bytes| *bytes <= limit.min(MAX_BYTES))
            .ok_or(
                "El texto decodificado del portapapeles supera el límite de bytes del destino",
            )?;
    }
    let mut decoded = String::new();
    decoded
        .try_reserve_exact(utf8_bytes)
        .map_err(|_| "No se pudo reservar memoria para decodificar el portapapeles")?;
    for scalar in char::decode_utf16(units[..end].iter().copied()) {
        let scalar = scalar.map_err(|_| "El texto del portapapeles no es UTF-16 válido")?;
        // Never silently grow beyond the preflight reservation. Under the
        // Windows ownership contract this is the same immutable input as above.
        if scalar.len_utf8() > utf8_bytes.saturating_sub(decoded.len()) {
            return Err(
                "El texto del portapapeles cambió durante la decodificación estricta".into(),
            );
        }
        decoded.push(scalar);
    }
    Ok(decoded)
}

struct Open {
    active: bool,
    // Open/Close and cleanup remain on the caller's UI thread.
    _thread: PhantomData<Rc<()>>,
}
impl Open {
    fn acquire(hwnd: Hwnd) -> Result<Self, String> {
        if hwnd.is_null() {
            return Err("El portapapeles requiere una ventana nativa propietaria".into());
        }
        if unsafe { OpenClipboard(hwnd) } == 0 {
            return Err(failed("OpenClipboard"));
        }
        Ok(Self {
            active: true,
            _thread: PhantomData,
        })
    }
    fn close(mut self) -> Result<(), String> {
        if unsafe { CloseClipboard() } == 0 {
            // Drop makes one final nonblocking cleanup attempt, not a retry loop.
            return Err(failed("CloseClipboard"));
        }
        self.active = false;
        Ok(())
    }
}
impl Drop for Open {
    fn drop(&mut self) {
        if self.active {
            unsafe {
                CloseClipboard();
            }
        }
    }
}

struct Memory {
    handle: NonNull<c_void>,
    owned: bool,
}
impl Memory {
    fn allocate(bytes: usize) -> Result<Self, String> {
        validate_allocation(bytes)?;
        let handle = NonNull::new(unsafe { GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, bytes) })
            .ok_or_else(|| failed("GlobalAlloc"))?;
        Ok(Self {
            handle,
            owned: true,
        })
    }
    fn size(&self) -> Result<usize, String> {
        let bytes = unsafe { GlobalSize(self.handle.as_ptr()) };
        if bytes == 0 {
            return Err(failed("GlobalSize"));
        }
        validate_allocation(bytes)?;
        Ok(bytes)
    }
    fn lock(&self) -> Result<Locked<'_>, String> {
        let pointer = NonNull::new(unsafe { GlobalLock(self.handle.as_ptr()) })
            .ok_or_else(|| failed("GlobalLock"))?;
        let lock = Locked {
            memory: self,
            pointer,
            active: true,
        };
        if pointer.as_ptr() as usize % std::mem::align_of::<u16>() != 0 {
            return Err("La memoria del portapapeles no está alineada para UTF-16".into());
        }
        Ok(lock)
    }
}
impl Drop for Memory {
    fn drop(&mut self) {
        if self.owned {
            unsafe {
                GlobalFree(self.handle.as_ptr());
            }
        }
    }
}
struct Locked<'a> {
    memory: &'a Memory,
    pointer: NonNull<c_void>,
    active: bool,
}
impl Locked<'_> {
    fn unlock(mut self) -> Result<(), String> {
        // Zero is also the successful final unlock; clear last-error first.
        unsafe {
            SetLastError(0);
        }
        let result = unsafe { GlobalUnlock(self.memory.handle.as_ptr()) };
        let error = unsafe { GetLastError() };
        if result == 0 && error != 0 {
            return Err(format!("Falló GlobalUnlock (error de Windows {error})"));
        }
        self.active = false;
        Ok(())
    }
}
impl Drop for Locked<'_> {
    fn drop(&mut self) {
        if self.active {
            unsafe {
                GlobalUnlock(self.memory.handle.as_ptr());
            }
        }
    }
}

/// Only invoke for an explicit user Paste, never from painting, polling or
/// command availability. None means CF_UNICODETEXT is unavailable; Some("") is
/// a valid empty text payload. Errors never include clipboard contents.
pub fn read(owner: usize) -> Result<Option<String>, String> {
    read_limited(owner, MAX_BYTES)
}
pub fn read_limited(owner: usize, limit: usize) -> Result<Option<String>, String> {
    if limit == 0 || limit > MAX_BYTES {
        return Err("El límite de bytes del destino del portapapeles no es válido".into());
    }
    let clipboard = Open::acquire(owner as Hwnd)?;
    if unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT) } == 0 {
        clipboard.close()?;
        return Ok(None);
    }
    let text = {
        let handle = NonNull::new(unsafe { GetClipboardData(CF_UNICODETEXT) })
            .ok_or_else(|| failed("GetClipboardData"))?;
        let memory = Memory {
            handle,
            owned: false,
        };
        let bytes = memory.size()?; // Reject oversize before any text allocation.
        let lock = memory.lock()?;
        // GlobalLock provides a readable allocation of GlobalSize bytes. All
        // u16 bit patterns are valid; strict surrogate validation happens above.
        let units =
            unsafe { std::slice::from_raw_parts(lock.pointer.as_ptr().cast::<u16>(), bytes / 2) };
        let text = if limit == MAX_BYTES {
            decode(units)?
        } else {
            decode_limited(units, limit)?
        };
        lock.unlock()?;
        text
    }; // Borrowed system handle is neither freed nor retained after CloseClipboard.
    clipboard.close()?;
    Ok(Some(text))
}

/// Only invoke for explicit user Copy/Cut, outside model borrows. Complete all
/// validation/allocation before EmptyClipboard. A successful SetClipboardData
/// transfers ownership even if a later CloseClipboard reports failure.
pub fn write(owner: usize, text: &str) -> Result<(), String> {
    let encoded = encode(text)?;
    let mut memory = Memory::allocate(encoded.len() * 2)?;
    let allocated = memory.size()?;
    if allocated < encoded.len() * 2 {
        return Err("La reserva del portapapeles es menor que su contenido codificado".into());
    }
    {
        let lock = memory.lock()?;
        // GlobalSize may exceed the requested size. Explicitly zero ALL padding
        // so this writer's output obeys this reader's strict trailing-NUL rule.
        unsafe {
            std::ptr::write_bytes(lock.pointer.as_ptr().cast::<u8>(), 0, allocated);
            std::ptr::copy_nonoverlapping(
                encoded.as_ptr(),
                lock.pointer.as_ptr().cast::<u16>(),
                encoded.len(),
            );
        }
        lock.unlock()?;
    }
    let clipboard = Open::acquire(owner as Hwnd)?;
    if unsafe { EmptyClipboard() } == 0 {
        return Err(failed("EmptyClipboard"));
    }
    if unsafe { SetClipboardData(CF_UNICODETEXT, memory.handle.as_ptr()) }.is_null() {
        return Err(failed("SetClipboardData"));
    }
    memory.owned = false; // Transfer only after success; never free system data.
    clipboard.close()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selector_destination_budget_rejects_expansion_before_retaining_text() {
        let exact = "é".repeat(128);
        let units = encode(&exact).unwrap();
        assert_eq!(decode_limited(&units, 256).unwrap(), exact);
        assert!(decode_limited(&units, 255).is_err());
        assert!(decode_limited(&encode(&"€".repeat(86)).unwrap(), 256).is_err());
        assert_eq!(
            decode_limited(&encode("e\u{301}").unwrap(), 256).unwrap(),
            "e\u{301}"
        );
    }

    #[test]
    fn strict_unicode_roundtrip_keeps_mixed_eol_tabs_and_no_normalization() {
        assert_eq!(encode("é😀").unwrap(), [0xe9, 0xd83d, 0xde00, 0]);
        for text in ["", "café😀\r\nx\ny\rz\t", "e\u{301}", "\u{feff}literal BOM"] {
            assert_eq!(decode(&encode(text).unwrap()).unwrap(), text);
        }
        assert_eq!(decode(&[b'a' as u16, 0, 0, 0]).unwrap(), "a");
    }

    #[test]
    fn nul_malformed_surrogates_and_missing_terminators_are_rejected_without_truncation() {
        assert!(encode("a\0b").is_err());
        for units in [
            &[0xd800, 0][..],
            &[0xdc00, 0],
            &[0xd800, b'a' as u16, 0],
            &[b'a' as u16],
            &[b'a' as u16, 0, b'b' as u16, 0],
            &[0, 0x41, 0],
        ] {
            assert!(decode(units).is_err());
        }
        assert!(validate_allocation(3).is_err());
        assert!(validate_allocation(0).is_err());
    }

    #[test]
    fn native_and_utf8_budgets_are_separate_and_enforced_before_expansion() {
        let ascii = "a".repeat(MAX_BYTES / 2 - 1);
        let encoded = encode(&ascii).unwrap();
        assert_eq!(encoded.len() * 2, MAX_BYTES);
        assert_eq!(decode(&encoded).unwrap(), ascii);
        assert!(encode(&"a".repeat(MAX_BYTES / 2)).is_err());
        assert!(encode(&"a".repeat(MAX_BYTES + 1)).is_err());
        let mut expansion = vec![0x20ac; MAX_BYTES / 3 + 1];
        expansion.push(0);
        assert!(expansion.len() * 2 < MAX_BYTES);
        assert!(decode(&expansion).is_err()); // UTF-16 fits, UTF-8 does not.
        assert!(decode(&vec![0; MAX_BYTES / 2 + 1]).is_err());
    }
}
