use super::super::{Context, Error, Limits, OperationCredential};
use gix::bstr::ByteSlice;
use gix_transport::client::git::ConnectMode;
use russh::{
    client,
    keys::{HashAlg, PrivateKeyWithHashAlg},
};
use std::{
    io::{self, Read, Write},
    path::Path,
    sync::Arc,
    time::Instant,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const SSH_KEY_MAX: u64 = 1024 * 1024;

#[cfg(test)]
#[path = "ssh_tests.rs"]
mod tests;

struct HostKeyPin {
    expected: String,
}

impl client::Handler for HostKeyPin {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_key: &russh::keys::PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let actual = server_key
            .public_key()
            .fingerprint(HashAlg::Sha256)
            .to_string();
        Ok(actual == self.expected)
    }
}

struct SessionGuard(Arc<client::Handle<HostKeyPin>>);

struct BlockingRead<'ctx, 'scope, R> {
    runtime: Arc<tokio::runtime::Runtime>,
    session: Arc<SessionGuard>,
    inner: R,
    ctx: &'ctx Context<'scope>,
    limit: usize,
    read: usize,
}

struct BlockingWrite<'ctx, 'scope, W> {
    runtime: Arc<tokio::runtime::Runtime>,
    session: Arc<SessionGuard>,
    inner: W,
    ctx: &'ctx Context<'scope>,
    limit: usize,
    written: usize,
}

fn io_cancel(error: Error) -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, error.to_string())
}

impl<R: AsyncRead + Unpin> Read for BlockingRead<'_, '_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.ctx.check().map_err(io_cancel)?;
        let remaining = self.limit.saturating_sub(self.read).saturating_add(1);
        let count = buffer.len().min(remaining);
        if count == 0 {
            return Err(io::Error::other("El flujo SSH Git excede el límite"));
        }
        let timeout = self.ctx.deadline.saturating_duration_since(Instant::now());
        if timeout.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        let result = self.runtime.block_on(async {
            tokio::time::timeout(timeout, self.inner.read(&mut buffer[..count])).await
        });
        let read = result
            .map_err(|_| io::ErrorKind::TimedOut)?
            .map_err(io::Error::other)?;
        self.read = self
            .read
            .checked_add(read)
            .ok_or(io::ErrorKind::InvalidData)?;
        if self.read > self.limit {
            return Err(io::Error::other("El flujo SSH Git excede el límite"));
        }
        let _keep_session_alive = &self.session.0;
        Ok(read)
    }
}

impl<W: AsyncWrite + Unpin> Write for BlockingWrite<'_, '_, W> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.ctx.check().map_err(io_cancel)?;
        let remaining = self.limit.saturating_sub(self.written);
        if remaining == 0 {
            return Err(io::Error::other("El envío SSH Git excede el límite"));
        }
        let timeout = self.ctx.deadline.saturating_duration_since(Instant::now());
        if timeout.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        let count = buffer.len().min(remaining);
        let result = self.runtime.block_on(async {
            tokio::time::timeout(timeout, self.inner.write(&buffer[..count])).await
        });
        let written = result
            .map_err(|_| io::ErrorKind::TimedOut)?
            .map_err(io::Error::other)?;
        self.written = self
            .written
            .checked_add(written)
            .ok_or(io::ErrorKind::InvalidData)?;
        let _keep_session_alive = &self.session.0;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.ctx.check().map_err(io_cancel)?;
        let timeout = self.ctx.deadline.saturating_duration_since(Instant::now());
        if timeout.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        self.runtime
            .block_on(tokio::time::timeout(timeout, self.inner.flush()))
            .map_err(|_| io::ErrorKind::TimedOut)?
            .map_err(io::Error::other)
    }
}

struct SecretBytes(Vec<u8>);
impl Drop for SecretBytes {
    fn drop(&mut self) {
        for byte in &mut self.0 {
            unsafe { std::ptr::write_volatile(byte, 0) };
        }
    }
}

fn read_private_key(path: &Path, ctx: &Context<'_>) -> Result<SecretBytes, Error> {
    let parent = path.parent().ok_or(Error::Unsupported)?.canonicalize()?;
    let name = path.file_name().ok_or(Error::Unsupported)?;
    let admitted = parent.join(name);
    let jc_fs::Capture::Present { bytes, token, .. } =
        crate::local_pair::capture(&parent, &admitted, SSH_KEY_MAX as usize, ctx)?
    else {
        return Err(Error::Missing);
    };
    if bytes.is_empty() || bytes.len() as u64 > SSH_KEY_MAX || !token.has_single_link() {
        return Err(Error::Unsupported);
    }
    let jc_fs::Capture::Present { token: current, .. } =
        crate::local_pair::capture(&parent, &admitted, SSH_KEY_MAX as usize, ctx)?
    else {
        return Err(Error::Stale);
    };
    if current != token {
        return Err(Error::Stale);
    }
    Ok(SecretBytes(bytes))
}

fn block_on_before<T>(
    runtime: &tokio::runtime::Runtime,
    ctx: &Context<'_>,
    future: impl std::future::Future<Output = T>,
) -> Result<T, Error> {
    ctx.check()?;
    let remaining = ctx.deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(Error::TimedOut);
    }
    runtime
        .block_on(tokio::time::timeout(remaining, future))
        .map_err(|_| Error::TimedOut)
}

fn shell_quote_path(path: &str) -> Result<String, Error> {
    if path.is_empty()
        || path.len() > 4096
        || path.starts_with('~')
        || path.chars().any(char::is_control)
    {
        return Err(Error::Unsupported);
    }
    Ok(format!("'{}'", path.replace('\'', "'\\''")))
}

pub(super) fn connect<'ctx, 'scope>(
    url: &gix::url::Url,
    credentials: &OperationCredential,
    limits: Limits,
    ctx: &'ctx Context<'scope>,
    push: bool,
) -> Result<impl gix_transport::client::blocking_io::Transport + 'ctx, Error> {
    if url
        .user()
        .is_some_and(|user| user.chars().any(char::is_control))
        || url.password().is_some()
    {
        return Err(Error::Unsupported);
    }
    let host = url.host().ok_or(Error::Unsupported)?;
    if host.len() > 253 || host.chars().any(char::is_control) {
        return Err(Error::Unsupported);
    }
    let username = if credentials.username.is_empty() {
        url.user().ok_or(Error::Unsupported)?.to_owned()
    } else {
        credentials.username.clone()
    };
    if username.is_empty() || username.len() > 255 || username.chars().any(char::is_control) {
        return Err(Error::Unsupported);
    }
    let expected_pin = credentials
        .host_fingerprint_pin
        .as_deref()
        .filter(|pin| pin.starts_with("SHA256:") && pin.len() <= 128)
        .ok_or_else(|| Error::Engine("SSH requiere una huella de servidor fijada".into()))?
        .to_owned();
    let repository_path = url
        .path_argument_safe()
        .ok_or(Error::Unsupported)?
        .to_str()
        .map_err(|_| Error::Unsupported)?;
    let quoted_path = shell_quote_path(repository_path)?;
    let service = if push {
        "git-receive-pack"
    } else {
        "git-upload-pack"
    };
    let command = format!("{service} {quoted_path}");
    let port = url.port_or_default().unwrap_or(22);
    let address = (host.to_owned(), port);
    let timeout = ctx.deadline.saturating_duration_since(Instant::now());
    if timeout.is_zero() {
        return Err(Error::Limited);
    }
    let runtime = Arc::new(
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(super::super::engine)?,
    );
    let config = Arc::new(client::Config {
        inactivity_timeout: Some(timeout),
        ..Default::default()
    });
    let mut handle = block_on_before(
        &runtime,
        ctx,
        client::connect(
            config,
            address,
            HostKeyPin {
                expected: expected_pin,
            },
        ),
    )?
    .map_err(|_| Error::Engine("No se pudo establecer el transporte SSH Git".into()))?;

    if let Some(key_path) = &credentials.private_key {
        let bytes = read_private_key(key_path, ctx)?;
        let text = std::str::from_utf8(&bytes.0).map_err(|_| Error::Unsupported)?;
        let decoded = russh::keys::decode_secret_key(
            text,
            (!credentials.secret.expose().is_empty()).then_some(credentials.secret.expose()),
        )
        .map_err(|_| Error::Engine("La clave SSH no se pudo descifrar".into()))?;
        let key = Arc::new(decoded);
        let hash = key.algorithm().is_rsa().then_some(HashAlg::Sha256);
        let key = PrivateKeyWithHashAlg::new(key, hash);
        match block_on_before(&runtime, ctx, handle.authenticate_publickey(username, key))?
            .map_err(|_| Error::Engine("Falló la autenticación SSH con clave".into()))?
        {
            client::AuthResult::Success => {}
            client::AuthResult::Failure { .. } => {
                return Err(Error::Engine("El servidor rechazó la clave SSH".into()));
            }
        }
    } else {
        if credentials.secret.expose().is_empty() {
            return Err(Error::Engine(
                "SSH requiere contraseña o clave privada".into(),
            ));
        }
        match block_on_before(
            &runtime,
            ctx,
            handle.authenticate_password(username, credentials.secret.expose().to_owned()),
        )?
        .map_err(|_| Error::Engine("Falló la autenticación SSH con contraseña".into()))?
        {
            client::AuthResult::Success => {}
            client::AuthResult::Failure { .. } => {
                return Err(Error::Engine(
                    "El servidor rechazó las credenciales SSH".into(),
                ));
            }
        }
    }

    ctx.check()?;
    let channel = block_on_before(&runtime, ctx, handle.channel_open_session())?
        .map_err(|_| Error::Engine("No se pudo abrir el canal SSH Git".into()))?;
    block_on_before(&runtime, ctx, channel.exec(true, command))?
        .map_err(|_| Error::Engine("El servidor rechazó el servicio Git por SSH".into()))?;
    let stream = channel.into_stream();
    let (read, write) = tokio::io::split(stream);
    let session = Arc::new(SessionGuard(Arc::new(handle)));
    let read = BlockingRead {
        runtime: Arc::clone(&runtime),
        session: Arc::clone(&session),
        inner: read,
        ctx,
        limit: limits.max_bytes.saturating_add(512 * 1024),
        read: 0,
    };
    let write = BlockingWrite {
        runtime,
        session,
        inner: write,
        ctx,
        limit: limits.max_bytes.saturating_add(512 * 1024),
        written: 0,
    };
    let protocol = if push {
        gix_transport::Protocol::V1
    } else {
        gix_transport::Protocol::V2
    };
    Ok(gix_transport::client::git::blocking_io::Connection::new(
        read,
        write,
        protocol,
        repository_path,
        None::<(String, Option<u16>)>,
        ConnectMode::Process,
        false,
    )
    .custom_url(Some(url.to_bstring())))
}
