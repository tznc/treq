//! Reads a user-managed host's SSH host-key fingerprint before it is trusted.
//!
//! Adding a self-managed host no longer asks the user to type the expected
//! fingerprint. Instead the setup flow probes the host, shows the presented
//! fingerprint, and pins it only after the user confirms (trust on first use,
//! as OpenSSH does). The probe always rejects the key during key exchange, so
//! it never authenticates and never sends client credentials.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::client::{self, Handler};
use russh::keys::PublicKeyOrCertificate;

use super::remote_ssh_transport::fingerprint_of;
use crate::lock_ext::LockExt;

const PROBE_TIMEOUT: Duration = Duration::from_secs(15);

/// Records the presented host key, then rejects it to end the handshake.
struct ProbeHandler {
  presented: Arc<Mutex<Option<Result<String, String>>>>,
}

impl Handler for ProbeHandler {
  type Error = russh::Error;

  async fn check_server_key(
    &mut self,
    server_public_key: &PublicKeyOrCertificate,
  ) -> Result<bool, Self::Error> {
    let result = fingerprint_of(server_public_key)
      .ok_or_else(|| "the host presented a certificate, which Treq does not pin".to_string());
    *self.presented.lock_or_recover() = Some(result);
    Ok(false)
  }
}

/// Connects to `hostname:port`, returns the presented host key's
/// `SHA256:<base64>` fingerprint, and disconnects before authentication.
pub async fn probe_host_key_fingerprint(hostname: &str, port: u16) -> Result<String, String> {
  let presented = Arc::new(Mutex::new(None));
  let handler = ProbeHandler {
    presented: presented.clone(),
  };
  let config = Arc::new(client::Config::default());
  let connect = client::connect(config, (hostname, port), handler);
  // The handshake always fails; only a missing recorded key means no host answered.
  let outcome = tokio::time::timeout(PROBE_TIMEOUT, connect).await;
  if let Some(result) = presented.lock_or_recover().take() {
    return result;
  }
  match outcome {
    Err(_) => Err(format!("timed out connecting to {hostname}:{port}")),
    Ok(Err(error)) => Err(format!("could not reach {hostname}:{port}: {error}")),
    Ok(Ok(_)) => Err(format!("{hostname}:{port} did not present a host key")),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use russh::keys::PrivateKey;
  use russh::server::{self, Server as _};

  #[derive(Clone)]
  struct NoopServer;

  struct NoopHandler;

  impl server::Handler for NoopHandler {
    type Error = russh::Error;
  }

  impl server::Server for NoopServer {
    type Handler = NoopHandler;
    fn new_client(&mut self, _: Option<std::net::SocketAddr>) -> NoopHandler {
      NoopHandler
    }
  }

  async fn start_server() -> (u16, PrivateKey) {
    use getrandom::SysRng;
    use rand_core::UnwrapErr;
    let host_key =
      PrivateKey::random(&mut UnwrapErr(SysRng), russh::keys::Algorithm::Ed25519).unwrap();
    let mut config = server::Config::default();
    config.keys.push(host_key.clone());
    let config = Arc::new(config);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
      while let Ok((socket, peer)) = listener.accept().await {
        let handler = NoopServer.new_client(Some(peer));
        let config = config.clone();
        tokio::spawn(async move {
          let _ = server::run_stream(config, socket, handler).await;
        });
      }
    });
    (port, host_key)
  }

  #[tokio::test]
  async fn returns_the_presented_host_key_fingerprint() {
    let (port, host_key) = start_server().await;
    let expected = host_key
      .public_key()
      .fingerprint(russh::keys::HashAlg::Sha256)
      .to_string();
    let fingerprint = probe_host_key_fingerprint("127.0.0.1", port).await.unwrap();
    assert_eq!(fingerprint, expected);
    assert!(fingerprint.starts_with("SHA256:"));
  }

  #[tokio::test]
  async fn reports_an_unreachable_host() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let error = probe_host_key_fingerprint("127.0.0.1", port)
      .await
      .unwrap_err();
    assert!(error.contains("could not reach"), "{error}");
  }
}
