//! Windows smart Git HTTP transport. TLS verification is delegated to WinHTTP.
use gix::error::ErrorExt;
use gix_transport::client::{
    AuthenticationRequired,
    blocking_io::http::{GetResponse, Http, PostBodyDataKind, PostResponse},
};
use std::{
    any::Any,
    ffi::c_void,
    io::{self, BufRead, Cursor, Read, Write},
    ptr::{null, null_mut},
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
};
use windows_sys::Win32::Networking::WinHttp::*;

const HEADER_LIMIT: usize = 32 * 1024;

#[derive(Clone)]
pub(super) struct WinHttp {
    deadline: Instant,
    max_request: usize,
    max_response: usize,
}

pub(super) struct HttpResponse {
    pub(super) headers: Vec<u8>,
    pub(super) body: Vec<u8>,
}

struct Request {
    url: String,
    headers: Vec<u16>,
    payload: Vec<u8>,
}

impl Drop for Request {
    fn drop(&mut self) {
        // These include the ephemeral Authorization header for this request.
        for value in &mut self.headers {
            unsafe { std::ptr::write_volatile(value, 0) };
        }
    }
}

struct Handle(*mut c_void);

impl Handle {
    fn from_raw(raw: *mut c_void) -> io::Result<Self> {
        if raw.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(raw))
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { WinHttpCloseHandle(self.0) };
    }
}

struct Endpoint {
    host: Vec<u16>,
    path: Vec<u16>,
    port: u16,
    secure: bool,
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}

fn endpoint(url: &str) -> io::Result<Endpoint> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidInput, "URL Git HTTPS no admitida");
    if url.len() > 4096
        || url
            .chars()
            .any(|c| c.is_control() || matches!(c, '\\' | '#'))
    {
        return Err(invalid());
    }
    let encoded = wide(url);
    let mut parts = URL_COMPONENTS {
        dwStructSize: std::mem::size_of::<URL_COMPONENTS>() as u32,
        dwHostNameLength: u32::MAX,
        dwUserNameLength: u32::MAX,
        dwPasswordLength: u32::MAX,
        dwUrlPathLength: u32::MAX,
        dwExtraInfoLength: u32::MAX,
        ..Default::default()
    };
    if unsafe { WinHttpCrackUrl(encoded.as_ptr(), (encoded.len() - 1) as u32, 0, &mut parts) } == 0
        || parts.dwUserNameLength != 0
        || parts.dwPasswordLength != 0
        || parts.dwHostNameLength == 0
        || parts.dwHostNameLength > 253
    {
        return Err(invalid());
    }
    let text = |pointer: *const u16, count: u32| {
        if count == 0 {
            Ok(String::new())
        } else if pointer.is_null() || count > 4096 {
            Err(invalid())
        } else {
            String::from_utf16(unsafe { std::slice::from_raw_parts(pointer, count as usize) })
                .map_err(|_| invalid())
        }
    };
    let host = text(parts.lpszHostName, parts.dwHostNameLength)?;
    let secure = parts.nScheme == WINHTTP_INTERNET_SCHEME_HTTPS;
    let local_fixture = cfg!(test)
        && parts.nScheme == WINHTTP_INTERNET_SCHEME_HTTP
        && host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
    if !secure && !local_fixture {
        return Err(invalid());
    }
    let mut path = text(parts.lpszUrlPath, parts.dwUrlPathLength)?;
    if path.is_empty() {
        path.push('/');
    }
    path.push_str(&text(parts.lpszExtraInfo, parts.dwExtraInfoLength)?);
    Ok(Endpoint {
        host: wide(&host),
        path: wide(&path),
        port: parts.nPort,
        secure,
    })
}

fn header_block(headers: impl IntoIterator<Item = impl AsRef<str>>) -> io::Result<Vec<u16>> {
    let mut block = Vec::new();
    for header in headers {
        let header = header.as_ref();
        let (name, _) = header.split_once(':').ok_or(io::ErrorKind::InvalidInput)?;
        if name.is_empty()
            || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || header.chars().any(char::is_control)
            || [
                "host",
                "content-length",
                "transfer-encoding",
                "cookie",
                "proxy-authorization",
            ]
            .iter()
            .any(|blocked| name.eq_ignore_ascii_case(blocked))
            || block.len().saturating_add(header.len()).saturating_add(2) > HEADER_LIMIT
        {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        block.extend(header.encode_utf16());
        block.extend([13, 10]);
    }
    Ok(block)
}

fn status(handle: &Handle) -> io::Result<u32> {
    let mut value = 0u32;
    let mut size = 4;
    checked(unsafe {
        WinHttpQueryHeaders(
            handle.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            null(),
            (&mut value as *mut u32).cast(),
            &mut size,
            null_mut(),
        )
    })?;
    Ok(value)
}

fn read_headers(handle: &Handle) -> io::Result<String> {
    let mut size = 0;
    let result = unsafe {
        WinHttpQueryHeaders(
            handle.0,
            WINHTTP_QUERY_RAW_HEADERS_CRLF,
            null(),
            null_mut(),
            &mut size,
            null_mut(),
        )
    };
    if result == 0 && io::Error::last_os_error().raw_os_error() != Some(122) {
        return Err(io::Error::last_os_error());
    }
    if size as usize > HEADER_LIMIT {
        return Err(io::Error::other("Las cabeceras HTTP Git exceden el límite"));
    }
    let mut buffer = vec![0u16; (size as usize).div_ceil(2) + 1];
    checked(unsafe {
        WinHttpQueryHeaders(
            handle.0,
            WINHTTP_QUERY_RAW_HEADERS_CRLF,
            null(),
            buffer.as_mut_ptr().cast(),
            &mut size,
            null_mut(),
        )
    })?;
    let len = buffer
        .iter()
        .position(|&value| value == 0)
        .unwrap_or(buffer.len());
    String::from_utf16(&buffer[..len]).map_err(io::Error::other)
}

fn checked(result: i32) -> io::Result<()> {
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn reserve(bytes: &mut Vec<u8>, additional: usize, limit: usize) -> io::Result<()> {
    let length = bytes
        .len()
        .checked_add(additional)
        .ok_or(io::ErrorKind::InvalidData)?;
    if length > limit {
        return Err(io::Error::other(
            "La transferencia HTTP Git excede el límite",
        ));
    }
    if length > bytes.capacity() {
        let capacity = bytes.capacity().saturating_mul(2).max(length).min(limit);
        bytes
            .try_reserve_exact(capacity - bytes.len())
            .map_err(io::Error::other)?;
    }
    Ok(())
}

impl WinHttp {
    pub(super) fn new(timeout: Duration, max_request: usize, max_response: usize) -> Self {
        Self {
            deadline: Instant::now() + timeout,
            max_request,
            max_response,
        }
    }

    fn remaining_ms(&self) -> io::Result<i32> {
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        Ok(remaining.as_millis().clamp(1, i32::MAX as u128) as i32)
    }

    fn send(&self, method: &str, input: Request) -> io::Result<HttpResponse> {
        let timeout = self.remaining_ms()?;
        let endpoint = endpoint(&input.url)?;
        let length = u32::try_from(input.payload.len()).map_err(io::Error::other)?;
        if input.payload.len() > self.max_request {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let agent = wide("CIDE Git");
        let session = Handle::from_raw(unsafe {
            WinHttpOpen(
                agent.as_ptr(),
                WINHTTP_ACCESS_TYPE_NO_PROXY,
                null(),
                null(),
                0,
            )
        })?;
        checked(unsafe { WinHttpSetTimeouts(session.0, timeout, timeout, timeout, timeout) })?;
        let connection = Handle::from_raw(unsafe {
            WinHttpConnect(session.0, endpoint.host.as_ptr(), endpoint.port, 0)
        })?;
        let verb = wide(method);
        let flags = if endpoint.secure {
            WINHTTP_FLAG_SECURE
        } else {
            0
        };
        let request = Handle::from_raw(unsafe {
            WinHttpOpenRequest(
                connection.0,
                verb.as_ptr(),
                endpoint.path.as_ptr(),
                null(),
                null(),
                null(),
                flags,
            )
        })?;
        let disabled =
            WINHTTP_DISABLE_COOKIES | WINHTTP_DISABLE_REDIRECTS | WINHTTP_DISABLE_AUTHENTICATION;
        checked(unsafe {
            WinHttpSetOption(
                request.0,
                WINHTTP_OPTION_DISABLE_FEATURE,
                (&disabled as *const u32).cast(),
                4,
            )
        })?;
        let payload = if length == 0 {
            null()
        } else {
            input.payload.as_ptr().cast()
        };
        self.remaining_ms()?;
        checked(unsafe {
            WinHttpSendRequest(
                request.0,
                input.headers.as_ptr(),
                input.headers.len() as u32,
                payload,
                length,
                length,
                0,
            )
        })?;
        self.remaining_ms()?;
        checked(unsafe { WinHttpReceiveResponse(request.0, null_mut()) })?;
        self.remaining_ms()?;
        let status = status(&request)?;
        if status == 401 {
            let headers = read_headers(&request)?;
            let www_authenticate = headers
                .lines()
                .filter_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("www-authenticate")
                        .then(|| value.trim().into())
                })
                .collect();
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                AuthenticationRequired { www_authenticate },
            ));
        }
        if status != 200 {
            return Err(io::Error::other(format!(
                "El remoto Git respondió HTTP {status}"
            )));
        }
        let headers = read_headers(&request)?.into_bytes();
        let mut body = Vec::new();
        let mut buffer = [0; 32 * 1024];
        loop {
            self.remaining_ms()?;
            let mut read = 0u32;
            let count = buffer.len().min(
                self.max_response
                    .saturating_sub(body.len())
                    .saturating_add(1),
            );
            checked(unsafe {
                WinHttpReadData(
                    request.0,
                    buffer.as_mut_ptr().cast(),
                    count as u32,
                    &mut read,
                )
            })?;
            if read == 0 {
                break;
            }
            let read = read as usize;
            reserve(&mut body, read, self.max_response)?;
            body.extend_from_slice(&buffer[..read]);
        }
        Ok(HttpResponse { headers, body })
    }
}

struct Post {
    backend: WinHttp,
    request: Option<Request>,
    written: bool,
    response: Option<HttpResponse>,
}

fn lock(post: &Mutex<Post>) -> io::Result<MutexGuard<'_, Post>> {
    post.lock()
        .map_err(|_| io::Error::other("Error interno de transporte Git HTTPS"))
}

pub(super) struct PostBody(Arc<Mutex<Post>>);

impl Write for PostBody {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let mut state = lock(&self.0)?;
        state.backend.remaining_ms()?;
        let limit = state.backend.max_request;
        let request = state.request.as_mut().ok_or(io::ErrorKind::BrokenPipe)?;
        reserve(&mut request.payload, data.len(), limit)?;
        request.payload.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for PostBody {
    fn drop(&mut self) {
        if let Ok(mut state) = self.0.lock() {
            state.written = true;
        }
    }
}

pub(super) struct ResponsePart {
    post: Option<(Arc<Mutex<Post>>, bool)>,
    bytes: Cursor<Vec<u8>>,
}

impl ResponsePart {
    fn ready(bytes: Vec<u8>) -> Self {
        Self {
            post: None,
            bytes: Cursor::new(bytes),
        }
    }

    fn pending(post: Arc<Mutex<Post>>, headers: bool) -> Self {
        Self {
            post: Some((post, headers)),
            bytes: Cursor::default(),
        }
    }

    fn ensure_ready(&mut self) -> io::Result<()> {
        if let Some((post, headers)) = &self.post {
            let mut state = lock(post)?;
            if !state.written {
                return Err(io::Error::other(
                    "El cuerpo Git HTTPS aún se está escribiendo",
                ));
            }
            if let Some(request) = state.request.take() {
                state.response = Some(state.backend.send("POST", request)?);
            }
            let response = state.response.as_mut().ok_or(io::ErrorKind::BrokenPipe)?;
            self.bytes = Cursor::new(if *headers {
                std::mem::take(&mut response.headers)
            } else {
                std::mem::take(&mut response.body)
            });
        }
        self.post = None;
        Ok(())
    }
}

impl Read for ResponsePart {
    fn read(&mut self, data: &mut [u8]) -> io::Result<usize> {
        self.ensure_ready()?;
        self.bytes.read(data)
    }
}

impl BufRead for ResponsePart {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.ensure_ready()?;
        self.bytes.fill_buf()
    }

    fn consume(&mut self, amount: usize) {
        self.bytes.consume(amount);
    }
}

impl Http for WinHttp {
    type Headers = ResponsePart;
    type ResponseBody = ResponsePart;
    type PostBody = PostBody;

    fn get(
        &mut self,
        url: &str,
        _base_url: &str,
        headers: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> gix::Result<GetResponse<Self::Headers, Self::ResponseBody>> {
        let response = self
            .send(
                "GET",
                Request {
                    url: url.to_owned(),
                    headers: header_block(headers).map_err(ErrorExt::raise)?,
                    payload: Vec::new(),
                },
            )
            .map_err(ErrorExt::raise)?;
        Ok(GetResponse {
            headers: ResponsePart::ready(response.headers),
            body: ResponsePart::ready(response.body),
        })
    }

    fn post(
        &mut self,
        url: &str,
        _base_url: &str,
        headers: impl IntoIterator<Item = impl AsRef<str>>,
        _body: PostBodyDataKind,
    ) -> gix::Result<PostResponse<Self::Headers, Self::ResponseBody, Self::PostBody>> {
        let post = Arc::new(Mutex::new(Post {
            backend: self.clone(),
            request: Some(Request {
                url: url.to_owned(),
                headers: header_block(headers).map_err(ErrorExt::raise)?,
                payload: Vec::new(),
            }),
            written: false,
            response: None,
        }));
        Ok(PostResponse {
            headers: ResponsePart::pending(Arc::clone(&post), true),
            body: ResponsePart::pending(Arc::clone(&post), false),
            post_body: PostBody(post),
        })
    }

    fn configure(&mut self, _config: &dyn Any) -> gix::Result {
        // Trust configuration, auth, proxy and redirect rules are fixed here.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::BufReader, net::TcpListener};

    #[test]
    fn rejects_credentials_in_urls_and_header_injection() {
        assert!(endpoint("https://user:secret@example.invalid/repo").is_err());
        assert!(endpoint("http://example.invalid/repo").is_err());
        assert!(header_block(["Accept: x\r\nHost: injected.invalid"]).is_err());
        assert!(header_block(["Content-Length: 99999"]).is_err());
    }

    #[test]
    fn smart_http_post_framing_and_bounded_response() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let url = format!(
            "http://{}/repo/git-upload-pack",
            listener.local_addr().unwrap()
        );
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut reader = BufReader::new(&mut stream);
            let mut request = String::new();
            reader.read_line(&mut request).unwrap();
            assert!(request.starts_with("POST /repo/git-upload-pack "));
            let mut length = 0;
            loop {
                let mut line = String::new();
                assert_ne!(reader.read_line(&mut line).unwrap(), 0);
                if line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse::<usize>().unwrap();
                }
            }
            assert_eq!(length, 8);
            let mut bytes = [0; 8];
            reader.read_exact(&mut bytes).unwrap();
            assert_eq!(&bytes, b"00000000");
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/x-git-upload-pack-result\r\nContent-Length: 4\r\nConnection: close\r\n\r\n0000").unwrap();
        });
        let mut client = WinHttp::new(Duration::from_secs(5), 8, 4);
        let mut response = client
            .post(
                &url,
                &url,
                ["Content-Type: application/x-git-upload-pack-request"],
                PostBodyDataKind::Unbounded,
            )
            .unwrap();
        response.post_body.write_all(b"00000000").unwrap();
        assert!(response.post_body.write_all(b"1").is_err());
        drop(response.post_body);
        let headers = response
            .headers
            .lines()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(
            headers
                .iter()
                .any(|line| line.contains("application/x-git-upload-pack-result"))
        );
        let mut bytes = Vec::new();
        response.body.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"0000");
        server.join().unwrap();
    }
}
