//! Opens only an explicitly activated HTTP(S) link in the registered browser.
pub fn validate(url: &str) -> Result<(), String> {
    let prefix = if url
        .get(..8)
        .is_some_and(|s| s.eq_ignore_ascii_case("https://"))
    {
        8
    } else if url
        .get(..7)
        .is_some_and(|s| s.eq_ignore_ascii_case("http://"))
    {
        7
    } else {
        return Err("Solo se abren enlaces HTTP/HTTPS".into());
    };
    let tail = &url[prefix..];
    let authority = tail.split(['/', '?', '#']).next().unwrap_or_default();
    if url.len() > 4096
        || authority.is_empty()
        || authority.contains('@')
        || url
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || matches!(c, '\\' | '"' | '<' | '>'))
    {
        return Err("Enlace web inválido o con credenciales".into());
    }
    Ok(())
}
pub fn open(url: &str) -> Result<(), String> {
    validate(url)?;
    #[cfg(windows)]
    {
        use std::{ffi::c_void, ptr::null};
        #[link(name = "shell32")]
        extern "system" {
            fn ShellExecuteW(
                owner: *mut c_void,
                operation: *const u16,
                file: *const u16,
                parameters: *const u16,
                directory: *const u16,
                show: i32,
            ) -> isize;
        }
        let operation: Vec<u16> = "open".encode_utf16().chain(Some(0)).collect();
        let value: Vec<u16> = url.encode_utf16().chain(Some(0)).collect();
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                operation.as_ptr(),
                value.as_ptr(),
                null(),
                null(),
                1,
            )
        };
        if result <= 32 {
            Err(format!("No se pudo abrir el navegador ({result})"))
        } else {
            Ok(())
        }
    }
    #[cfg(not(windows))]
    {
        Err("La apertura de navegador aún no está disponible en esta plataforma".into())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn markdown_web_link_validation_never_launches_other_schemes() {
        for url in [
            "file:///x",
            "javascript:alert(1)",
            "https://user:pass@example.com/x",
            "https:///x",
            "https://example.com/ a",
            "https://example.com/\\x",
        ] {
            assert!(validate(url).is_err());
        }
        assert!(validate("https://example.com/gu%C3%ADa#inicio").is_ok());
    }
}
