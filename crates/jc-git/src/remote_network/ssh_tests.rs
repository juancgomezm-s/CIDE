use super::*;
use gix::bstr::ByteSlice;
use russh::client::Handler;
use russh::server::Server as _;

const HOST_PRIVATE_KEY: &str = "-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW\nQyNTUxOQAAACB1fQLwX1GbtiQS8QNnyQUHl3ISnCAUnMrxIPDr50A8ggAAAKBvSsfob0rH\n6AAAAAtzc2gtZWQyNTUxOQAAACB1fQLwX1GbtiQS8QNnyQUHl3ISnCAUnMrxIPDr50A8gg\nAAAECo+GpEzGeHkwviPpsmH7aHYdykUv3GGFkUMptgvUmzjHV9AvBfUZu2JBLxA2fJBQeX\nchKcIBScyvEg8OvnQDyCAAAAFmp1YW5jYXJsb3NAWmVwaHlydXNHMTQBAgMEBQYH\n-----END OPENSSH PRIVATE KEY-----\n";

#[test]
fn server_fingerprint_pin_accepts_only_the_exact_key() {
    let key = russh::keys::ssh_key::PublicKey::from_openssh(
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJdD7y3aLq454yWBdwLWbieU1ebz9/cu7/QEXn9OIeZJ fixture",
    )
    .unwrap();
    let fingerprint = key.fingerprint(HashAlg::Sha256).to_string();
    let presented = russh::keys::PublicKeyOrCertificate::from(key);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let accepted = runtime.block_on(
        HostKeyPin {
            expected: fingerprint.clone(),
        }
        .check_server_key(&presented),
    );
    assert!(accepted.unwrap());

    let rejected = runtime.block_on(
        HostKeyPin {
            expected: "SHA256:otra-huella-fijada".to_owned(),
        }
        .check_server_key(&presented),
    );
    assert!(!rejected.unwrap());
}

#[derive(Clone)]
struct FixtureServer;

impl russh::server::Server for FixtureServer {
    type Handler = FixtureHandler;

    fn new_client(&mut self, _: Option<std::net::SocketAddr>) -> Self::Handler {
        FixtureHandler
    }
}

struct FixtureHandler;

impl russh::server::Handler for FixtureHandler {
    type Error = russh::Error;

    async fn auth_password(
        &mut self,
        user: &str,
        password: &str,
    ) -> Result<russh::server::Auth, Self::Error> {
        Ok(if user == "cide-fixture" && password == "secret-fixture" {
            russh::server::Auth::Accept
        } else {
            russh::server::Auth::reject()
        })
    }

    async fn channel_open_session(
        &mut self,
        _channel: russh::Channel<russh::server::Msg>,
        reply: russh::server::ChannelOpenHandle,
        _session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        Ok(())
    }

    async fn exec_request(
        &mut self,
        channel: russh::ChannelId,
        command: &[u8],
        session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        assert_eq!(command, b"git-receive-pack '/fixture.git'");
        session.channel_success(channel)?;
        let oid = "0123456789012345678901234567890123456789";
        let line = format!("{oid} refs/heads/main\0report-status\n");
        let mut packet = format!("{:04x}", line.len() + 4).into_bytes();
        packet.extend_from_slice(line.as_bytes());
        packet.extend_from_slice(b"0000");
        session.data(channel, packet)?;
        Ok(())
    }
}

#[test]
fn ssh_loopback_authenticates_pins_host_and_reads_receive_pack_advertisement() {
    use std::{
        sync::atomic::{AtomicBool, AtomicU8},
        time::{Duration, Instant},
    };
    let host_key = russh::keys::decode_secret_key(HOST_PRIVATE_KEY, None).unwrap();
    let fingerprint = host_key
        .public_key()
        .fingerprint(HashAlg::Sha256)
        .to_string();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let server_config = std::sync::Arc::new(russh::server::Config {
            keys: vec![host_key],
            auth_rejection_time: Duration::ZERO,
            auth_rejection_time_initial: Some(Duration::ZERO),
            ..Default::default()
        });
        let server_task = tokio::spawn(async move {
            let mut server = FixtureServer;
            server.run_on_socket(server_config, &listener).await
        });

        let check = tokio::task::spawn_blocking(move || {
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
                max_entries: 128,
                max_path: 4096,
                max_bytes: 64 * 1024,
                timeout,
            };
            let credential = OperationCredential {
                username: "cide-fixture".into(),
                secret: crate::remote::SecretString::new("secret-fixture".into()),
                host_fingerprint_pin: Some(fingerprint),
                private_key: None,
            };
            let url = gix::url::parse(
                format!("ssh://cide-fixture@{address}/fixture.git")
                    .as_bytes()
                    .as_bstr(),
            )
            .unwrap();
            let mut transport = connect(&url, &credential, limits, &ctx, true).unwrap();
            let advertised = super::super::send_pack::advertise(
                &mut transport,
                "refs/heads/main",
                gix::hash::Kind::Sha1,
                limits,
                &ctx,
            )
            .unwrap();
            assert_eq!(
                advertised.unwrap().to_string(),
                "0123456789012345678901234567890123456789"
            );
        });
        check.await.unwrap();
        server_task.abort();
        let _ = server_task.await;
    });
}
