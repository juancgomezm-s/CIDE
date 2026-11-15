//! Bounded transports and protocol adapters for embedded Git.
use super::{Context, Error, Limits, OperationCredential};
#[cfg(windows)]
use gix::remote::Direction;
#[cfg(windows)]
use gix_transport::client::TransportWithoutIO;
use std::path::Path;

mod push_plan;
mod send_pack;
mod ssh;
#[cfg(windows)]
mod winhttp;

pub(super) type BoxedTransport<'ctx> =
    Box<dyn gix::protocol::transport::client::blocking_io::Transport + 'ctx>;

pub(super) fn connect_transport<'ctx>(
    repo: &gix::Repository,
    url: &gix::url::Url,
    push: bool,
    credential: Option<&OperationCredential>,
    limits: Limits,
    ctx: &'ctx Context<'_>,
) -> Result<BoxedTransport<'ctx>, Error> {
    ctx.check()?;
    match url.scheme {
        gix::url::Scheme::Https | gix::url::Scheme::Http => {
            #[cfg(windows)]
            {
                if url.user().is_some() || url.password().is_some() {
                    return Err(Error::Unsupported);
                }
                if url.scheme == gix::url::Scheme::Http && !is_loopback_host(url.host()) {
                    return Err(Error::Unsupported);
                }
                let remote = repo
                    .remote_at_without_url_rewrite(url.to_bstring().to_string())
                    .map_err(super::engine)?;
                let (safe_url, desired_version) = remote
                    .sanitized_url_and_version(if push {
                        Direction::Push
                    } else {
                        Direction::Fetch
                    })
                    .map_err(super::engine)?;
                let timeout = ctx
                    .deadline
                    .saturating_duration_since(std::time::Instant::now());
                let mut transport = gix_transport::client::blocking_io::http::Transport::new_http(
                    winhttp::WinHttp::new(
                        timeout,
                        limits.max_bytes.saturating_add(512 * 1024),
                        limits.max_bytes.saturating_add(512 * 1024),
                    ),
                    safe_url,
                    if push {
                        gix::protocol::transport::Protocol::V1
                    } else {
                        desired_version
                    },
                    false,
                );
                if let Some(credentials) = credential {
                    if !credentials.secret.expose().is_empty() {
                        transport
                            .set_identity(gix::sec::identity::Account {
                                username: credentials.username.clone(),
                                password: credentials.secret.expose().to_owned(),
                                oauth_refresh_token: None,
                            })
                            .map_err(super::engine)?;
                    }
                }
                Ok(Box::new(transport))
            }
            #[cfg(not(windows))]
            {
                let _ = (repo, push, credential, limits);
                Err(Error::Unsupported)
            }
        }
        gix::url::Scheme::Ssh => {
            let credentials = credential.ok_or_else(|| {
                Error::Engine("El remoto SSH requiere credenciales de esta operación".into())
            })?;
            let transport = ssh::connect(url, credentials, limits, ctx, push)?;
            Ok(Box::new(transport))
        }
        _ => Err(Error::Unsupported),
    }
}

#[cfg(windows)]
fn is_loopback_host(host: Option<&str>) -> bool {
    let Some(host) = host else { return false };
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

pub(super) fn prepare_network_push(
    root: &Path,
    requested_remote: &str,
    credential: Option<&OperationCredential>,
    limits: Limits,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<super::PushPlan, Error> {
    push_plan::prepare(root, requested_remote, credential, limits, cancelled)
}

pub(super) fn apply_network_push(
    plan: super::PushPlan,
    credential: Option<&OperationCredential>,
    cancelled: impl FnMut() -> bool + Send,
) -> Result<super::TransferResult, Error> {
    push_plan::apply(plan, credential, cancelled)
}

pub(super) fn fetch_clone_into_staging(
    repo: &gix::Repository,
    url: &gix::url::Url,
    credential: Option<&OperationCredential>,
    limits: Limits,
    ctx: &Context<'_>,
) -> Result<(Option<String>, Option<gix::ObjectId>), Error> {
    let (refs, branch) = super::network_fetch_into(repo, url, credential, limits, ctx)?;
    let branch = branch
        .filter(|name| name.starts_with("refs/heads/"))
        .or_else(|| {
            ["refs/heads/main", "refs/heads/master"]
                .into_iter()
                .find(|name| refs.contains_key(*name))
                .map(str::to_owned)
        })
        .or_else(|| {
            refs.keys()
                .find(|name| name.starts_with("refs/heads/"))
                .cloned()
        });
    let tip = branch
        .as_ref()
        .and_then(|name| refs.get(name).copied())
        .or_else(|| refs.get("HEAD").copied());
    Ok((branch, tip))
}
