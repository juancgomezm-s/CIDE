//! One reviewed fast-forward publication over Git receive-pack v0/v1.
use crate::{Context, Error, Limits};
use gix::{ObjectId, bstr::ByteSlice, hash::Kind};
use gix_transport::{
    Protocol, Service,
    client::{MessageKind, WriteMode, blocking_io::Transport},
};
use std::io::{self, Read, Write};

const REPORT_LIMIT: usize = 16 * 1024;

struct CappedPack {
    bytes: Vec<u8>,
    limit: usize,
}

impl Write for CappedPack {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let length = self
            .bytes
            .len()
            .checked_add(bytes.len())
            .ok_or(io::ErrorKind::OutOfMemory)?;
        if length > self.limit {
            return Err(io::Error::other("Paquete Git fuera del límite"));
        }
        if length > self.bytes.capacity() {
            let capacity = self
                .bytes
                .capacity()
                .saturating_mul(2)
                .max(length)
                .min(self.limit);
            self.bytes
                .try_reserve_exact(capacity - self.bytes.len())
                .map_err(io::Error::other)?;
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) fn pack_v2(
    objects: &[(ObjectId, gix::objs::Kind, Vec<u8>)],
    hash: Kind,
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<Vec<u8>, Error> {
    if objects.len() > limits.max_entries.saturating_mul(4) {
        return Err(Error::Limited);
    }
    let mut pack = CappedPack {
        bytes: Vec::new(),
        limit: limits.max_bytes.saturating_add(512 * 1024),
    };
    pack.write_all(b"PACK")?;
    pack.write_all(&2u32.to_be_bytes())?;
    let count = u32::try_from(objects.len()).map_err(|_| Error::Limited)?;
    pack.write_all(&count.to_be_bytes())?;
    let mut input = 0usize;
    for (oid, kind, bytes) in objects {
        ctx.check()?;
        input = input.checked_add(bytes.len()).ok_or(Error::Limited)?;
        if input > limits.max_bytes {
            return Err(Error::Limited);
        }
        if oid.kind() != hash
            || gix::objs::compute_hash(hash, *kind, bytes).map_err(crate::engine)? != *oid
        {
            return Err(Error::Stale);
        }
        let kind = match kind {
            gix::objs::Kind::Commit => 1,
            gix::objs::Kind::Tree => 2,
            gix::objs::Kind::Blob => 3,
            gix::objs::Kind::Tag => 4,
        };
        let mut size = bytes.len();
        let mut first = (kind << 4) | (size as u8 & 0xf);
        size >>= 4;
        if size != 0 {
            first |= 0x80;
        }
        pack.write_all(&[first])?;
        while size != 0 {
            let mut byte = size as u8 & 0x7f;
            size >>= 7;
            if size != 0 {
                byte |= 0x80;
            }
            pack.write_all(&[byte])?;
        }
        let mut compressed =
            gix::zlib::stream::deflate::Write::new(&mut pack, gix::zlib::Compression::default());
        for chunk in bytes.chunks(64 * 1024) {
            ctx.check()?;
            compressed.write_all(chunk)?;
        }
        compressed.flush()?;
    }
    ctx.check()?;
    let mut digest = gix::hash::hasher(hash);
    digest.update(&pack.bytes);
    pack.write_all(digest.try_finalize().map_err(crate::engine)?.as_slice())?;
    Ok(pack.bytes)
}

fn validate_destination(destination: &str, limits: Limits) -> Result<(), Error> {
    if destination.len() > limits.max_path.min(1024)
        || !destination.starts_with("refs/heads/")
        || destination.chars().any(char::is_control)
    {
        return Err(Error::Unsupported);
    }
    gix::refs::FullName::try_from(gix::bstr::BString::from(destination))
        .map_err(|_| Error::Unsupported)?;
    Ok(())
}

pub(super) fn advertise(
    transport: &mut dyn Transport,
    destination: &str,
    hash: Kind,
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<Option<ObjectId>, Error> {
    ctx.check()?;
    validate_destination(destination, limits)?;
    let mut response = transport
        .handshake(Service::ReceivePack, &[])
        .map_err(|_| Error::Engine("No se pudo consultar receive-pack en el remoto Git".into()))?;
    if response.actual_protocol != Protocol::V1 || !response.capabilities.contains("report-status")
    {
        return Err(Error::Engine(
            "El remoto no ofrece report-status para confirmar el envío".into(),
        ));
    }
    let advertised_hash = response
        .capabilities
        .capability("object-format")
        .and_then(|cap| cap.value())
        .map(|name| name.as_bytes())
        .unwrap_or(b"sha1");
    if advertised_hash != hash.to_string().as_bytes() {
        return Err(Error::Unsupported);
    }
    let reader = response.refs.as_mut().ok_or(Error::Unsupported)?;
    let mut selected = None;
    let mut bytes = 0usize;
    let mut refs = 0usize;
    while let Some(line) = reader.readline() {
        ctx.check()?;
        let line = line
            .map_err(|_| Error::Engine("Se interrumpió la consulta de referencias remotas".into()))?
            .map_err(|_| Error::Engine("El remoto envió referencias inválidas".into()))?;
        let text = line.as_bstr().ok_or(Error::Unsupported)?.as_bytes();
        bytes = bytes.checked_add(text.len()).ok_or(Error::Limited)?;
        refs += 1;
        if bytes > limits.max_bytes || refs > limits.max_entries {
            return Err(Error::Limited);
        }
        let text = text.strip_suffix(b"\n").unwrap_or(text);
        let Some(space) = text.iter().position(|&b| b == b' ') else {
            return Err(Error::Unsupported);
        };
        let name = &text[space + 1..];
        let name = name.split(|&b| b == 0).next().unwrap_or_default();
        if name == destination.as_bytes() {
            let id = ObjectId::from_hex(&text[..space]).map_err(|_| Error::Unsupported)?;
            if id.kind() != hash || id.is_null() || selected.replace(id).is_some() {
                return Err(Error::Unsupported);
            }
        }
    }
    Ok(selected)
}

fn command(destination: &str, old: Option<ObjectId>, new: ObjectId) -> Vec<u8> {
    let old = old.unwrap_or_else(|| new.kind().null());
    let format = if new.kind() == Kind::Sha256 {
        " object-format=sha256"
    } else {
        ""
    };
    let text = format!("{old} {new} {destination}\0 report-status{format}\n");
    let mut packet = format!("{:04x}", text.len() + 4).into_bytes();
    packet.extend_from_slice(text.as_bytes());
    packet.extend_from_slice(b"0000");
    packet
}

fn read_report(mut reader: impl Read, destination: &str) -> Result<(), Error> {
    let uncertain = || {
        Error::Engine(
            "El remoto no confirmó el envío. Consulta la referencia antes de reintentar".into(),
        )
    };
    let mut bytes = Vec::new();
    reader
        .by_ref()
        .take((REPORT_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| uncertain())?;
    if bytes.len() > REPORT_LIMIT {
        return Err(uncertain());
    }
    let report = std::str::from_utf8(&bytes).map_err(|_| uncertain())?;
    let mut lines = report.lines();
    let Some(unpack) = lines.next() else {
        return Err(uncertain());
    };
    if unpack.starts_with("unpack ") && unpack != "unpack ok" {
        return Err(Error::Engine(
            "El remoto rechazó el paquete de objetos".into(),
        ));
    }
    if unpack != "unpack ok" {
        return Err(uncertain());
    }
    let Some(result) = lines.next() else {
        return Err(uncertain());
    };
    if let Some(rejection) = result.strip_prefix("ng ")
        && rejection
            .split_once(' ')
            .is_some_and(|(name, _)| name == destination)
    {
        return Err(Error::Engine(
            "El remoto rechazó la actualización de la rama".into(),
        ));
    }
    if result.strip_prefix("ok ") != Some(destination) || lines.next().is_some() {
        return Err(uncertain());
    }
    Ok(())
}

pub(super) fn push_pack(
    transport: &mut dyn Transport,
    destination: &str,
    old: Option<ObjectId>,
    new: ObjectId,
    pack: &[u8],
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<(), Error> {
    let hash = new.kind();
    if new.is_null() || old.is_some_and(|old| old.kind() != hash || old.is_null()) {
        return Err(Error::Unsupported);
    }
    if pack.len() > limits.max_bytes.saturating_add(512 * 1024) {
        return Err(Error::Limited);
    }
    if !pack.starts_with(b"PACK") || pack.len() < 12 + hash.len_in_bytes() {
        return Err(Error::Unsupported);
    }
    if advertise(transport, destination, hash, limits, ctx)? != old {
        return Err(Error::Stale);
    }
    if old == Some(new) {
        return Ok(());
    }
    ctx.check()?;
    let request = transport
        .request(WriteMode::Binary, MessageKind::Flush, false)
        .map_err(|_| Error::Engine("No se pudo abrir el envío Git".into()))?;
    let (mut writer, reader) = request.into_parts();
    writer
        .write_all(&command(destination, old, new))
        .and_then(|_| writer.write_all(pack))
        .and_then(|_| writer.flush())
        .map_err(|_| {
            Error::Engine(
                "Se interrumpió el envío. Consulta la referencia antes de reintentar".into(),
            )
        })?;
    drop(writer);
    // Once written, only the server's response can tell whether it published.
    read_report(reader, destination)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    use std::{
        sync::atomic::{AtomicBool, AtomicU8},
        time::{Duration, Instant},
    };

    #[test]
    fn update_command_pins_old_oid_and_requests_report_status() {
        let old = ObjectId::from_hex(b"0123456789012345678901234567890123456789").unwrap();
        let new = ObjectId::from_hex(b"1234567890123456789012345678901234567890").unwrap();
        let update = command("refs/heads/main", Some(old), new);
        let length = usize::from_str_radix(std::str::from_utf8(&update[..4]).unwrap(), 16).unwrap();
        assert_eq!(length, update.len() - 4);
        assert_eq!(
            &update[4..length],
            format!("{old} {new} refs/heads/main\0 report-status\n").as_bytes()
        );
        assert_eq!(&update[length..], b"0000");
    }

    #[test]
    fn success_requires_unpack_and_exact_ref_confirmation() {
        let destination = "refs/heads/main";
        assert!(read_report(b"unpack ok\nok refs/heads/main\n".as_slice(), destination).is_ok());
        for response in [
            "unpack ok\n",
            "unpack ok\nok refs/heads/other\n",
            "unpack invalid pack\nok refs/heads/main\n",
            "unpack ok\nng refs/heads/main non-fast-forward\n",
            "unpack ok\nok refs/heads/main\nok refs/heads/other\n",
        ] {
            assert!(
                read_report(response.as_bytes(), destination).is_err(),
                "{response}"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn push_over_smart_http_pins_tip_and_checks_server_ack() {
        use super::super::winhttp::WinHttp;
        use std::{
            io::{BufRead, BufReader},
            net::{TcpListener, TcpStream},
        };

        fn take_request(stream: &mut TcpStream) -> (String, Vec<u8>) {
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut reader = BufReader::new(stream);
            let mut request = String::new();
            assert_ne!(reader.read_line(&mut request).unwrap(), 0);
            let mut length = 0;
            loop {
                let mut line = String::new();
                assert_ne!(reader.read_line(&mut line).unwrap(), 0);
                if line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap();
                }
            }
            assert!(length <= 8192);
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            (request, body)
        }
        fn packet(value: &str) -> Vec<u8> {
            let mut result = format!("{:04x}", value.len() + 4).into_bytes();
            result.extend_from_slice(value.as_bytes());
            result
        }
        fn serve(stream: &mut TcpStream, content_type: &str, body: &[u8]) {
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            ).unwrap();
            stream.write_all(body).unwrap();
        }

        let hash = Kind::Sha1;
        let old = ObjectId::from_hex(b"0123456789012345678901234567890123456789").unwrap();
        let new = ObjectId::from_hex(b"1234567890123456789012345678901234567890").unwrap();
        let destination = "refs/heads/main";
        let stop = AtomicBool::new(false);
        let reason = AtomicU8::new(0);
        let timeout = Duration::from_secs(5);
        let ctx = Context {
            stop: &stop,
            reason: &reason,
            deadline: Instant::now() + timeout,
        };
        let limits = Limits {
            max_blob: 4096,
            max_entries: 64,
            max_path: 4096,
            max_bytes: 8192,
            timeout,
        };
        let content = b"payload".to_vec();
        let blob = gix::objs::compute_hash(hash, gix::objs::Kind::Blob, &content).unwrap();
        let pack = pack_v2(
            &[(blob, gix::objs::Kind::Blob, content)],
            hash,
            limits,
            &ctx,
        )
        .unwrap();
        let footer = &pack[pack.len() - hash.len_in_bytes()..];
        let mut digest = gix::hash::hasher(hash);
        digest.update(&pack[..pack.len() - hash.len_in_bytes()]);
        assert_eq!(footer, digest.try_finalize().unwrap().as_slice());
        let mut expected_body = command(destination, Some(old), new);
        expected_body.extend_from_slice(&pack);

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let url = format!("http://{}/repo", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut discovery, _) = listener.accept().unwrap();
            assert!(
                take_request(&mut discovery)
                    .0
                    .starts_with("GET /repo/info/refs?service=git-receive-pack ")
            );
            let mut refs = packet("# service=git-receive-pack\n");
            refs.extend_from_slice(b"0000");
            refs.extend(packet(&format!("{old} {destination}\0report-status\n")));
            refs.extend_from_slice(b"0000");
            serve(
                &mut discovery,
                "application/x-git-receive-pack-advertisement",
                &refs,
            );
            drop(discovery);
            let (mut send, _) = listener.accept().unwrap();
            let (request, body) = take_request(&mut send);
            assert!(request.starts_with("POST /repo/git-receive-pack "));
            assert_eq!(body, expected_body);
            let mut report = packet("unpack ok\n");
            report.extend(packet(&format!("ok {destination}\n")));
            report.extend_from_slice(b"0000");
            serve(&mut send, "application/x-git-receive-pack-result", &report);
        });
        let mut transport = gix_transport::client::blocking_io::http::Transport::new_http(
            WinHttp::new(timeout, 8192, 8192),
            gix::url::parse(url.as_bytes().as_bstr()).unwrap(),
            Protocol::V1,
            false,
        );
        push_pack(
            &mut transport,
            destination,
            Some(old),
            new,
            &pack,
            limits,
            &ctx,
        )
        .unwrap();
        server.join().unwrap();
    }
}
