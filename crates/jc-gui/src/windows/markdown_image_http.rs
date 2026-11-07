//! Explicit Markdown image downloads; invoked only by the image worker.
use std::{
    ffi::c_void,
    ptr::{null, null_mut},
    time::{Duration, Instant},
};
type Handle = *mut c_void;
const LIMIT: usize = 8 * 1024 * 1024;
#[repr(C)]
struct Components {
    size: u32,
    scheme: *mut u16,
    scheme_len: u32,
    scheme_id: i32,
    host: *mut u16,
    host_len: u32,
    port: u16,
    user: *mut u16,
    user_len: u32,
    password: *mut u16,
    password_len: u32,
    path: *mut u16,
    path_len: u32,
    extra: *mut u16,
    extra_len: u32,
}
#[link(name = "winhttp")]
extern "system" {
    fn WinHttpOpen(
        agent: *const u16,
        access: u32,
        proxy: *const u16,
        bypass: *const u16,
        flags: u32,
    ) -> Handle;
    fn WinHttpCloseHandle(handle: Handle) -> i32;
    fn WinHttpSetTimeouts(
        handle: Handle,
        resolve: i32,
        connect: i32,
        send: i32,
        receive: i32,
    ) -> i32;
    fn WinHttpCrackUrl(url: *const u16, len: u32, flags: u32, parts: *mut Components) -> i32;
    fn WinHttpConnect(session: Handle, host: *const u16, port: u16, reserved: u32) -> Handle;
    fn WinHttpOpenRequest(
        connection: Handle,
        method: *const u16,
        path: *const u16,
        version: *const u16,
        referer: *const u16,
        types: *const *const u16,
        flags: u32,
    ) -> Handle;
    fn WinHttpSetOption(handle: Handle, option: u32, value: *const c_void, len: u32) -> i32;
    fn WinHttpSendRequest(
        request: Handle,
        headers: *const u16,
        len: u32,
        optional: *const c_void,
        optional_len: u32,
        total: u32,
        context: usize,
    ) -> i32;
    fn WinHttpReceiveResponse(request: Handle, reserved: *mut c_void) -> i32;
    fn WinHttpQueryHeaders(
        request: Handle,
        info: u32,
        name: *const u16,
        buffer: *mut c_void,
        len: *mut u32,
        index: *mut u32,
    ) -> i32;
    fn WinHttpReadData(request: Handle, buffer: *mut c_void, count: u32, read: *mut u32) -> i32;
}
struct Owned(Handle);
impl Owned {
    fn new(handle: Handle) -> Result<Self, String> {
        if handle.is_null() {
            Err(error())
        } else {
            Ok(Self(handle))
        }
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe {
            WinHttpCloseHandle(self.0);
        }
    }
}
fn error() -> String {
    format!(
        "No se pudo cargar la imagen remota: {}",
        std::io::Error::last_os_error()
    )
}
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
struct Url {
    host: Vec<u16>,
    path: Vec<u16>,
    port: u16,
    secure: bool,
    origin: String,
}
fn scheme(url: &str) -> Option<bool> {
    if url
        .get(..8)
        .is_some_and(|s| s.eq_ignore_ascii_case("https://"))
    {
        Some(true)
    } else if url
        .get(..7)
        .is_some_and(|s| s.eq_ignore_ascii_case("http://"))
    {
        Some(false)
    } else {
        None
    }
}
fn parse(url: &str) -> Result<Url, String> {
    let authority = url
        .split_once("://")
        .map(|(_, tail)| tail.split(['/', '?', '#']).next().unwrap_or(""));
    if url.len() > 4096
        || url.chars().any(|c| c.is_control() || c == '\\')
        || scheme(url).is_none()
        || authority.is_some_and(|host| host.contains('@'))
    {
        return Err("Solo se admiten imágenes HTTP/HTTPS sin credenciales".into());
    }
    let encoded = wide(url);
    let mut c: Components = unsafe { std::mem::zeroed() };
    c.size = std::mem::size_of::<Components>() as u32;
    c.host_len = u32::MAX;
    c.user_len = u32::MAX;
    c.password_len = u32::MAX;
    c.path_len = u32::MAX;
    c.extra_len = u32::MAX;
    if unsafe { WinHttpCrackUrl(encoded.as_ptr(), (encoded.len() - 1) as u32, 0, &mut c) } == 0
        || ![1, 2].contains(&c.scheme_id)
        || c.host_len == 0
        || c.host_len > 253
        || c.user_len != 0
        || c.password_len != 0
    {
        return Err("Dirección de imagen remota inválida o con credenciales".into());
    }
    let take = |ptr: *mut u16, len: u32| -> Result<String, String> {
        if len == 0 {
            return Ok(String::new());
        }
        if ptr.is_null() || len > 4096 {
            return Err("Dirección de imagen fuera del límite".into());
        }
        String::from_utf16(unsafe { std::slice::from_raw_parts(ptr, len as usize) })
            .map_err(|_| "Dirección Unicode inválida".into())
    };
    let host = take(c.host, c.host_len)?;
    let mut path = take(c.path, c.path_len)?;
    if path.is_empty() {
        path.push('/');
    }
    path.push_str(&take(c.extra, c.extra_len)?);
    path.truncate(path.find('#').unwrap_or(path.len()));
    let scheme = if c.scheme_id == 2 { "https" } else { "http" };
    let authority = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.clone()
    };
    let origin = format!("{scheme}://{authority}:{}", c.port);
    Ok(Url {
        host: wide(&host),
        path: wide(&path),
        port: c.port,
        secure: c.scheme_id == 2,
        origin,
    })
}
unsafe fn header(request: Handle, kind: u32) -> Result<Option<String>, String> {
    let mut len = 0;
    let first = WinHttpQueryHeaders(request, kind, null(), null_mut(), &mut len, null_mut());
    if first == 0 {
        match std::io::Error::last_os_error().raw_os_error() {
            Some(12150) => return Ok(None), // header not found
            Some(122) => {}                 // requested bounded buffer size
            _ => return Err(error()),
        }
    }
    if len == 0 {
        return Ok(Some(String::new()));
    }
    if len > 8192 {
        return Err("Cabecera de imagen demasiado grande".into());
    }
    let mut units = vec![0u16; (len as usize).div_ceil(2) + 1];
    if WinHttpQueryHeaders(
        request,
        kind,
        null(),
        units.as_mut_ptr().cast(),
        &mut len,
        null_mut(),
    ) == 0
    {
        return Err(error());
    }
    let n = units.iter().position(|&v| v == 0).unwrap_or(units.len());
    String::from_utf16(&units[..n])
        .map(Some)
        .map_err(|_| "Cabecera de imagen inválida".into())
}
fn redirect(previous: &str, base: &Url, location: &str) -> Result<String, String> {
    if location.is_empty() || location.len() > 4096 || location.chars().any(char::is_control) {
        return Err("Destino de redirección inválido".into());
    }
    if scheme(location).is_none()
        && location
            .split_once(':')
            .is_some_and(|(prefix, _)| !prefix.contains(['/', '?', '#']))
    {
        return Err("Solo se siguen redirecciones HTTP/HTTPS".into());
    }
    let target = if scheme(location).is_some() {
        location.to_owned()
    } else if location.starts_with("//") {
        format!("{}:{location}", if base.secure { "https" } else { "http" })
    } else if location.starts_with('/') {
        format!("{}{}", base.origin, location)
    } else {
        let mut path = String::from_utf16(&base.path[..base.path.len() - 1])
            .map_err(|_| "Ruta de redirección inválida")?;
        path.truncate(path.find('?').unwrap_or(path.len()));
        if location.starts_with(['?', '#']) {
            format!("{}{path}{location}", base.origin)
        } else {
            let end = path.rfind('/').map_or(0, |n| n + 1);
            format!(
                "{}{}/{location}",
                base.origin,
                path[..end].trim_end_matches('/')
            )
        }
    };
    let parsed = parse(&target)?;
    if scheme(previous) == Some(true) && !parsed.secure {
        return Err("No se sigue una redirección HTTPS hacia HTTP".into());
    }
    Ok(target)
}
/// All timeouts/cancellation are cooperative around native WinHTTP calls. The
/// total deadline is checked between blocking calls; it is not a hard OS deadline.
pub(super) fn download(url: &str, cancelled: &dyn Fn() -> bool) -> Result<Vec<u8>, String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let check = || {
        if cancelled() {
            Err("Carga de imagen cancelada".to_string())
        } else if Instant::now() >= deadline {
            Err("Se agotó el tiempo de carga de la imagen".to_string())
        } else {
            Ok(())
        }
    };
    check()?;
    parse(url)?;
    let mut current = url.to_owned();
    let agent = wide("CIDE Markdown");
    // A private direct session avoids ambient PAC/WPAD, proxy credentials,
    // cookie jars and configuration writes. Proxy support needs its own setting.
    let session = Owned::new(unsafe { WinHttpOpen(agent.as_ptr(), 1, null(), null(), 0) })?;
    if unsafe { WinHttpSetTimeouts(session.0, 2000, 5000, 5000, 5000) } == 0 {
        return Err(error());
    }
    for hops in 0..=3 {
        check()?;
        let parsed = parse(&current)?;
        let connection =
            Owned::new(unsafe { WinHttpConnect(session.0, parsed.host.as_ptr(), parsed.port, 0) })?;
        let method = wide("GET");
        let request = Owned::new(unsafe {
            WinHttpOpenRequest(
                connection.0,
                method.as_ptr(),
                parsed.path.as_ptr(),
                null(),
                null(),
                null(),
                if parsed.secure { 0x00800000 } else { 0 },
            )
        })?;
        let disabled = 1u32 | 2 | 4; // cookies, automatic redirects and authentication
        if unsafe { WinHttpSetOption(request.0, 63, (&disabled as *const u32).cast(), 4) } == 0 {
            return Err(error());
        }
        let accept = wide("Accept: image/*\r\n");
        if unsafe {
            WinHttpSendRequest(
                request.0,
                accept.as_ptr(),
                (accept.len() - 1) as u32,
                null(),
                0,
                0,
                0,
            )
        } == 0
        {
            return Err(error());
        }
        check()?;
        if unsafe { WinHttpReceiveResponse(request.0, null_mut()) } == 0 {
            return Err(error());
        }
        check()?;
        let mut status = 0u32;
        let mut size = 4;
        if unsafe {
            WinHttpQueryHeaders(
                request.0,
                19 | 0x20000000,
                null(),
                (&mut status as *mut u32).cast(),
                &mut size,
                null_mut(),
            )
        } == 0
        {
            return Err(error());
        }
        if [301, 302, 303, 307, 308].contains(&status) {
            if hops == 3 {
                return Err("Demasiadas redirecciones de imagen".into());
            }
            let location =
                unsafe { header(request.0, 33) }?.ok_or("Redirección de imagen sin destino")?;
            current = redirect(&current, &parsed, &location)?;
            continue;
        }
        if !(200..300).contains(&status) {
            return Err(format!("El servidor de la imagen respondió HTTP {status}"));
        }
        if let Some(value) = unsafe { header(request.0, 5) }? {
            let length = value
                .trim()
                .parse::<u64>()
                .map_err(|_| "Content-Length de imagen inválido")?;
            if length > LIMIT as u64 {
                return Err("La imagen remota supera 8 MiB".into());
            }
        }
        let mut bytes = Vec::new();
        let mut chunk = [0u8; 65536];
        loop {
            check()?;
            let mut read = 0;
            // A full payload probes EOF into a stack byte; never grow a Vec to
            // 16MiB just to discover an oversized transfer's last byte.
            let remaining = (LIMIT - bytes.len()).min(chunk.len());
            if remaining == 0 {
                let mut guard = 0u8;
                if unsafe {
                    WinHttpReadData(request.0, (&mut guard as *mut u8).cast(), 1, &mut read)
                } == 0
                {
                    return Err(error());
                }
                if read != 0 {
                    return Err("La imagen remota supera 8 MiB".into());
                }
                break;
            }
            if unsafe {
                WinHttpReadData(
                    request.0,
                    chunk.as_mut_ptr().cast(),
                    remaining as u32,
                    &mut read,
                )
            } == 0
            {
                return Err(error());
            }
            if read == 0 {
                break;
            }
            let count = read as usize;
            if count > remaining {
                return Err("Longitud de transferencia de imagen inválida".into());
            }
            if bytes.capacity() < bytes.len() + count {
                let target = bytes
                    .capacity()
                    .saturating_mul(2)
                    .max(bytes.len() + count)
                    .min(LIMIT);
                bytes
                    .try_reserve_exact(target - bytes.len())
                    .map_err(|_| "No se pudo reservar la imagen remota")?;
                if bytes.capacity() > LIMIT {
                    return Err("La reserva de imagen remota supera 8 MiB".into());
                }
            }
            bytes.extend_from_slice(&chunk[..count]);
        }
        check()?;
        return Ok(bytes);
    }
    Err("No se pudo resolver la imagen remota".into())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn markdown_image_http_addresses_are_bounded_without_network() {
        for url in [
            "file:///x",
            "https://user:password@example.com/a.png",
            "https:///x",
            "http://example.com/\n",
            "https://example.com/\\x",
        ] {
            assert!(parse(url).is_err(), "{url}");
        }
        let p = parse("https://example.com:8443/a.png?q=1#parte").unwrap();
        assert_eq!(p.port, 8443);
        assert!(p.secure);
        assert_eq!(
            String::from_utf16(&p.path[..p.path.len() - 1]).unwrap(),
            "/a.png?q=1"
        );
        assert!(redirect("https://example.com", &p, "http://example.com/b.png").is_err());
        assert_eq!(
            redirect("https://example.com", &p, "/b.png").unwrap(),
            "https://example.com:8443/b.png"
        );
        assert!(download("https://example.com/a.png", &|| true).is_err());
    }
    fn response(status: &str, headers: &str, body: &[u8]) -> Vec<u8> {
        let mut bytes =
            format!("HTTP/1.1 {status}\r\nConnection: close\r\n{headers}\r\n").into_bytes();
        bytes.extend_from_slice(body);
        bytes
    }
    fn fixture(responses: Vec<Vec<u8>>) -> (String, std::thread::JoinHandle<Vec<String>>) {
        use std::{
            io::{Read, Write},
            net::TcpListener,
        };
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut requests = Vec::new();
            for response in responses {
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(
                                Instant::now() < deadline,
                                "loopback image request did not arrive"
                            );
                            std::thread::sleep(Duration::from_millis(1));
                        }
                        Err(e) => panic!("{e}"),
                    }
                };
                // Accepted sockets inherit the listener's nonblocking mode on
                // Windows; timed fixture reads must be explicitly blocking.
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut part = [0u8; 1024];
                while !bytes.windows(4).any(|b| b == b"\r\n\r\n") {
                    let n = stream.read(&mut part).unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&part[..n]);
                    assert!(bytes.len() <= 8192);
                }
                let request = String::from_utf8(bytes).unwrap();
                assert!(request.to_ascii_lowercase().contains("accept: image/*\r\n"));
                assert!(!request.to_ascii_lowercase().contains("authorization:"));
                assert!(!request.to_ascii_lowercase().contains("cookie:"));
                requests.push(request);
                // The downloader may close immediately after rejecting headers.
                let _ = stream.write_all(&response);
            }
            requests
        });
        (format!("http://{address}"), server)
    }
    #[test]
    fn markdown_image_http_loopback_png_redirect_errors_and_size_budget() {
        let png = include_bytes!("../../../../assets/branding/cide-gui-16.png");
        let headers = format!(
            "Content-Type: image/png\r\nContent-Length: {}\r\n",
            png.len()
        );
        let (base, server) = fixture(vec![response("200 OK", &headers, png)]);
        assert_eq!(
            download(&format!("{base}/image.png"), &|| false).unwrap(),
            png
        );
        assert_eq!(server.join().unwrap().len(), 1);
        let (base, server) = fixture(vec![
            response(
                "302 Found",
                "Location: /image.png\r\nContent-Length: 0\r\n",
                &[],
            ),
            response("200 OK", &headers, png),
        ]);
        assert_eq!(download(&format!("{base}/start"), &|| false).unwrap(), png);
        let requests = server.join().unwrap();
        assert!(requests[0].starts_with("GET /start "));
        assert!(requests[1].starts_with("GET /image.png "));
        let (base, server) = fixture(vec![response(
            "404 Not Found",
            "Content-Length: 0\r\n",
            &[],
        )]);
        assert!(download(&format!("{base}/missing"), &|| false)
            .unwrap_err()
            .contains("404"));
        server.join().unwrap();
        let (base, server) = fixture(vec![response(
            "200 OK",
            &format!("Content-Length: {}\r\n", LIMIT + 1),
            &[],
        )]);
        assert!(download(&format!("{base}/large"), &|| false)
            .unwrap_err()
            .contains("8 MiB"));
        server.join().unwrap();
        let (base, server) = fixture(vec![response(
            "200 OK",
            "Content-Type: image/png\r\n",
            &vec![0; LIMIT + 1],
        )]);
        assert!(download(&format!("{base}/stream"), &|| false)
            .unwrap_err()
            .contains("8 MiB"));
        server.join().unwrap();
    }
    #[test]
    fn markdown_image_http_redirect_bound_and_cooperative_cancellation() {
        use std::cell::Cell;
        let redirects = (0..4)
            .map(|_| {
                response(
                    "302 Found",
                    "Location: /again\r\nContent-Length: 0\r\n",
                    &[],
                )
            })
            .collect();
        let (base, server) = fixture(redirects);
        assert!(download(&format!("{base}/again"), &|| false)
            .unwrap_err()
            .contains("Demasiadas"));
        assert_eq!(server.join().unwrap().len(), 4);
        let (base, server) = fixture(vec![response("200 OK", "Content-Length: 5\r\n", b"pixel")]);
        let checks = Cell::new(0);
        let cancelled = || {
            checks.set(checks.get() + 1);
            checks.get() >= 5
        };
        assert!(download(&format!("{base}/cancel"), &cancelled)
            .unwrap_err()
            .contains("cancelada"));
        server.join().unwrap();
        let parsed = parse("HTTPS://example.com:8443/folder/a.png?q=1").unwrap();
        assert!(parsed.secure);
        assert!(parse("http://@example.com/a").is_err());
        assert_eq!(
            redirect("HTTPS://example.com/", &parsed, "other.png").unwrap(),
            "https://example.com:8443/folder/other.png"
        );
        assert!(redirect("HTTPS://example.com/", &parsed, "http://example.com/b.png").is_err());
        assert_eq!(
            redirect("https://example.com/", &parsed, "//cdn.example.com/a.png").unwrap(),
            "https://cdn.example.com/a.png"
        );
    }
}
