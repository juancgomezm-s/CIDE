//! Read a bounded UTF-16 password from an explicitly named generic credential.
//! https://learn.microsoft.com/windows/win32/api/wincred/nf-wincred-credreadw

use std::{ffi::c_void, ptr};

const GENERIC: u32 = 1;
const MAX_BLOB_BYTES: usize = 5 * 512;

#[repr(C)]
struct Credential {
    flags: u32,
    kind: u32,
    target: *const u16,
    comment: *const u16,
    last_written: [u32; 2],
    blob_size: u32,
    blob: *mut u8,
    // The remaining CREDENTIALW fields are only accessed by Windows.
}

#[link(name = "advapi32")]
extern "system" {
    fn CredReadW(target: *const u16, kind: u32, flags: u32, value: *mut *mut Credential) -> i32;
    fn CredFree(value: *mut c_void);
}

struct Native(*mut Credential);

impl Drop for Native {
    fn drop(&mut self) {
        unsafe {
            let credential = &*self.0;
            let count = credential.blob_size as usize;
            if !credential.blob.is_null() && count <= MAX_BLOB_BYTES {
                for index in 0..count {
                    ptr::write_volatile(credential.blob.add(index), 0);
                }
            }
            CredFree(self.0.cast());
        }
    }
}

pub(super) struct Password(Vec<u16>);

impl Password {
    pub(super) fn read(reference: &str) -> Result<Self, String> {
        if reference.len() > 256
            || !reference
                .strip_prefix("cide/datadigger/")
                .is_some_and(|id| {
                    !id.is_empty()
                        && id
                            .bytes()
                            .all(|ch| ch.is_ascii_alphanumeric() || b"-_.".contains(&ch))
                })
        {
            return Err("Referencia de credencial SQL inválida".into());
        }
        let name: Vec<u16> = reference.encode_utf16().chain([0]).collect();
        let mut value = ptr::null_mut();
        let found = unsafe { CredReadW(name.as_ptr(), GENERIC, 0, &mut value) };
        if found == 0 || value.is_null() {
            return Err("No se encontró la credencial genérica SQL en el Administrador de credenciales de Windows".into());
        }
        let native = Native(value);
        let credential = unsafe { &*native.0 };
        if credential.blob.is_null()
            || credential.blob_size as usize > MAX_BLOB_BYTES
            || credential.blob_size % 2 != 0
        {
            return Err("La credencial SQL debe contener una contraseña UTF-16".into());
        }
        let bytes =
            unsafe { std::slice::from_raw_parts(credential.blob, credential.blob_size as usize) };
        let mut password = Self(
            bytes
                .chunks_exact(2)
                .map(|p| u16::from_le_bytes([p[0], p[1]]))
                .collect(),
        );
        if password.0.last() == Some(&0) {
            password.0.pop();
        }
        if password.0.contains(&0)
            || char::decode_utf16(password.0.iter().copied()).any(|ch| ch.is_err())
        {
            return Err(
                "La credencial SQL debe contener una contraseña UTF-16 sin NUL intermedio".into(),
            );
        }
        Ok(password)
    }

    pub(super) fn units(&self) -> &[u16] {
        &self.0
    }
}

impl Drop for Password {
    fn drop(&mut self) {
        wipe(&mut self.0);
    }
}

pub(super) fn wipe(buffer: &mut [u16]) {
    for unit in buffer {
        unsafe { ptr::write_volatile(unit, 0) };
    }
}
