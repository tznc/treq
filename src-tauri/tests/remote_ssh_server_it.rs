#![expect(clippy::expect_used, reason = "test code")]
//! Real-`sshd` counterpart to `core::remote_ssh_transport`'s `mod tests`.
//!
//! Every test in that module runs against an in-process mock
//! `russh::server` - real code under test, but a fake server. This file
//! drives the same production transport (`SshConnectionPool`,
//! `HostKeyVerifier`, `exec_command`, `RemotePtyChannel`) against a *real*,
//! independently implemented `sshd` (the `linuxserver/openssh-server`
//! container started as a GitHub Actions service in
//! `.github/workflows/remote-ssh-server-it.yml`).
//!
//! ## What this suite proves against a live OpenSSH daemon
//!
//! - publickey auth and certificate auth through the production native
//!   transport (`authenticate_publickey` / `authenticate_openssh_cert`);
//! - host-key pinning accept/reject before authentication;
//! - connection pooling / reuse across repeated execs;
//! - exec-channel argument quoting, stdout/stderr, exit codes, deadlines;
//! - OpenSSH user-certificate accept (trusted CA + valid principal +
//!   current validity window) and reject (wrong CA, invalid principal,
//!   not-yet-valid, expired);
//! - PTY start in a selected directory, input/output, resize, and close;
//! - the WebSocket relay transport used for managed Sprites, through a
//!   local WebSocket-to-TCP bridge standing in for `remote-ssh-relay` and
//!   the Sprites TCP proxy: certificate auth, host-key pinning, pooling,
//!   and token refusal all run through the relayed byte stream.
//!
//! Certificates are issued with `ssh-keygen -s`, which writes the same
//! `ssh-ed25519-cert-v01@openssh.com` user-certificate format the Edge
//! Function signer (`supabase/functions/_shared/remote/ssh-cert.ts`)
//! produces: principals, valid-after/before, and the standard permit-*
//! extensions including `permit-pty`. The Deno signer is not executed
//! here; the wire format and the server's `TrustedUserCAKeys` check are.
//!
//! ## What remains mocked / out of scope
//!
//! - The far-end `treq` binary is the CI stub shim, not the real CLI.
//!   JSON command payloads and real JJ/Git behavior are still gap #11
//!   in `remote_e2e_README.md`.
//! - Client-key *revocation* as OpenSSH understands it (KRL /
//!   `RevokedKeys`) is not configured on this sshd. sshd cannot see a
//!   Treq control-plane key revocation. The production cutoff
//!   (`SshConnectionPool::force_cutoff`) is therefore tested against this
//!   same real connection, separately from certificate expiry. See
//!   `client_cutoff_tears_down_a_live_connection_without_sshd_krl`.
//! - Silent certificate renewal, Supabase issuance, and managed-VM
//!   bootstrap of `TrustedUserCAKeys` are not this job.
//!
//! ## Test-key safety
//!
//! All keys are throwaway material: the committed `id_ed25519` pair only
//! authorizes the ephemeral CI container; the CA and cert-only client key
//! are generated per job and never uploaded as artifacts. Tests never
//! print private key bytes, CA seeds, or certificate private material.
//!
//! ## Running this suite
//!
//! Gated on `TREQ_SSH_SERVER_IT=1` plus connection details. With no
//! opt-in, every test prints "SKIP: ..." and passes - it never fakes an
//! assertion against no server.
//!
//! - `TREQ_SSH_SERVER_IT=1`: explicit opt-in.
//! - `TREQ_SSH_IT_HOST` (default `127.0.0.1`).
//! - `TREQ_SSH_IT_PORT` (default `2222`).
//! - `TREQ_SSH_IT_USERNAME` (default `treq`).
//! - `TREQ_SSH_IT_KEY_PATH`: OpenSSH private key in `authorized_keys`.
//! - `TREQ_SSH_IT_HOST_KEY_ALGORITHM` (default `ssh-ed25519`).
//! - `TREQ_SSH_IT_HOST_KEY_FINGERPRINT`: live SHA256 host-key fingerprint.
//! - `TREQ_SSH_IT_CA_KEY_PATH`: test-only user CA private key whose public
//!   half is in the server's `TrustedUserCAKeys`. Required for certificate
//!   tests; those skip if unset.
//! - `TREQ_SSH_IT_CERT_KEY_PATH`: client key *not* in `authorized_keys`,
//!   used only with a signed certificate.
//! - `TREQ_SSH_IT_WORKSPACE_DIR`: directory created on the server for PTY
//!   working-directory tests.

#![expect(
  clippy::unwrap_used,
  clippy::let_underscore_must_use,
  clippy::print_stderr,
  reason = "test code reports progress on stderr"
)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Once;
use std::time::Duration;

use treq_lib::core::remote_control_plane::{
  SshAuthentication, SshEndpoint, SshEndpointSource, SshTransport, TrustedHostKey,
};
use treq_lib::core::remote_ssh_transport::{
  exec_command, CancellationToken, CutoffReason, ExecLimits, RemotePtyChannel, SshConnectionPool,
  SshTransportError,
};

struct ItConfig {
  endpoint: SshEndpoint,
}

fn it_config() -> Option<ItConfig> {
  static PRINT_BANNER: Once = Once::new();
  static INIT_TRACING: Once = Once::new();
  // Diagnostic only: `HostKeyVerifier` logs a rejection via `tracing::warn!`
  // rather than putting the presented fingerprint in the error a test sees
  // (`ConnectionFailed("... Disconnected")` is deliberately generic - see
  // `check_server_key`). Without a subscriber that warning is silently
  // dropped, so a host-key pin mismatch and every other cause of the same
  // generic error are indistinguishable from CI output alone.
  INIT_TRACING.call_once(|| {
    let _ = tracing_subscriber::fmt()
      .with_env_filter(tracing_subscriber::EnvFilter::new(
        "treq_lib::core::remote_ssh_transport=debug",
      ))
      .with_test_writer()
      .try_init();
  });

  if std::env::var("TREQ_SSH_SERVER_IT").as_deref() != Ok("1") {
    PRINT_BANNER.call_once(|| {
      eprintln!(
        "[remote-ssh-server-it] SKIP: TREQ_SSH_SERVER_IT=1 not set. Real-sshd \
         tests in remote_ssh_server_it.rs do not run. See \
         .github/workflows/remote-ssh-server-it.yml and this file's header."
      );
    });
    return None;
  }

  let hostname = std::env::var("TREQ_SSH_IT_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
  let port: u16 = std::env::var("TREQ_SSH_IT_PORT")
    .ok()
    .and_then(|v| v.parse().ok())
    .unwrap_or(2222);
  let username = std::env::var("TREQ_SSH_IT_USERNAME").unwrap_or_else(|_| "treq".to_string());
  let key_reference = std::env::var("TREQ_SSH_IT_KEY_PATH").ok()?;
  let algorithm =
    std::env::var("TREQ_SSH_IT_HOST_KEY_ALGORITHM").unwrap_or_else(|_| "ssh-ed25519".to_string());
  let fingerprint_sha256 = std::env::var("TREQ_SSH_IT_HOST_KEY_FINGERPRINT").ok()?;
  let host_keys: Vec<TrustedHostKey> = fingerprint_sha256
    .split(',')
    .map(str::trim)
    .filter(|fp| !fp.is_empty())
    .map(|fp| TrustedHostKey {
      algorithm: algorithm.clone(),
      fingerprint_sha256: fp.to_string(),
      comment: None,
    })
    .collect();
  if host_keys.is_empty() {
    return None;
  }

  Some(ItConfig {
    endpoint: SshEndpoint {
      id: "remote-ssh-server-it".to_string(),
      instance_id: None,
      source: SshEndpointSource::UserManaged,
      hostname,
      port,
      username,
      host_keys,
      authentication: SshAuthentication::PublicKey { key_reference },
      transport: Default::default(),
    },
  })
}

fn cert_env() -> Option<(PathBuf, PathBuf, String)> {
  let ca = PathBuf::from(std::env::var("TREQ_SSH_IT_CA_KEY_PATH").ok()?);
  let key = PathBuf::from(std::env::var("TREQ_SSH_IT_CERT_KEY_PATH").ok()?);
  if !ca.is_file() || !key.is_file() {
    return None;
  }
  let username = std::env::var("TREQ_SSH_IT_USERNAME").unwrap_or_else(|_| "treq".to_string());
  Some((ca, key, username))
}

macro_rules! require_it {
  () => {
    match it_config() {
      Some(cfg) => cfg,
      None => {
        eprintln!(
          "[remote-ssh-server-it] SKIP {}: missing TREQ_SSH_SERVER_IT=1 / \
           TREQ_SSH_IT_KEY_PATH / TREQ_SSH_IT_HOST_KEY_FINGERPRINT",
          module_path!()
        );
        return;
      }
    }
  };
}

macro_rules! require_cert_it {
  () => {{
    let cfg = require_it!();
    match cert_env() {
      Some(cert) => (cfg, cert),
      None => {
        eprintln!(
          "[remote-ssh-server-it] SKIP {}: missing TREQ_SSH_IT_CA_KEY_PATH / \
           TREQ_SSH_IT_CERT_KEY_PATH (certificate tests are CI-only)",
          module_path!()
        );
        return;
      }
    }
  }};
}

fn copy_cert_client_key(src: &Path, dest_dir: &Path) -> PathBuf {
  let dest = dest_dir.join("id_ed25519");
  std::fs::copy(src, &dest).expect("copy cert-only private key");
  let src_pub = PathBuf::from(format!("{}.pub", src.display()));
  if src_pub.is_file() {
    std::fs::copy(&src_pub, dest_dir.join("id_ed25519.pub")).expect("copy cert-only public key");
  }
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o600)).unwrap();
  }
  dest
}

/// Signs `key_pub` with `ca` using OpenSSH's user-certificate format.
/// `validity` is `ssh-keygen -V` syntax (`+5m`, `-2d:-1d`, `+1d:+2d`).
fn sign_user_certificate(
  ca: &Path,
  key_pub: &Path,
  key_id: &str,
  principals: &str,
  validity: &str,
) {
  let output = Command::new("ssh-keygen")
    .args([
      "-s",
      &ca.to_string_lossy(),
      "-I",
      key_id,
      "-n",
      principals,
      "-V",
      validity,
      "-z",
      "1",
      &key_pub.to_string_lossy(),
    ])
    .output()
    .expect("ssh-keygen must be on PATH to issue test certificates");
  assert!(
    output.status.success(),
    "ssh-keygen -s failed (stdout/stderr omitted: may name key paths)"
  );
}

/// Builds the endpoint the way the app does after `issue_certificate`: the
/// key reference is the `.pub` path the desktop identity picker returns, and
/// the certificate travels inline. The `-cert.pub` file `ssh-keygen -s`
/// wrote is read and then removed, so a passing test proves the transport
/// used the inline certificate rather than the file next to the key.
fn cert_endpoint(cfg: &ItConfig, key_path: &Path, id_suffix: &str) -> SshEndpoint {
  let cert_path = PathBuf::from(format!("{}-cert.pub", key_path.display()));
  let certificate = std::fs::read_to_string(&cert_path).expect("read issued certificate");
  std::fs::remove_file(&cert_path).expect("remove certificate file");
  let mut endpoint = cfg.endpoint.clone();
  endpoint.id = format!("remote-ssh-server-it-cert-{id_suffix}");
  endpoint.authentication = SshAuthentication::Certificate {
    key_reference: format!("{}.pub", key_path.display()),
    certificate: Some(certificate),
  };
  endpoint
}

fn issue_and_endpoint(
  cfg: &ItConfig,
  ca: &Path,
  src_key: &Path,
  dest_dir: &Path,
  principals: &str,
  validity: &str,
  id_suffix: &str,
) -> SshEndpoint {
  let key_path = copy_cert_client_key(src_key, dest_dir);
  let pub_path = PathBuf::from(format!("{}.pub", key_path.display()));
  sign_user_certificate(ca, &pub_path, id_suffix, principals, validity);
  cert_endpoint(cfg, &key_path, id_suffix)
}

fn poison_host_keys(endpoint: &mut SshEndpoint) {
  for key in &mut endpoint.host_keys {
    key.fingerprint_sha256 = "SHA256:not-the-real-host-key".to_string();
  }
}

fn auth_rejected(error: &SshTransportError) -> bool {
  matches!(error, SshTransportError::AuthenticationFailed(_))
}

async fn collect_pty_bytes(pty: &RemotePtyChannel, wait: Duration) -> Vec<u8> {
  let mut buf = Vec::new();
  let deadline = tokio::time::Instant::now() + wait;
  loop {
    let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
    if remaining.is_zero() {
      break;
    }
    match tokio::time::timeout(remaining, pty.read_chunk()).await {
      Ok(Ok(Some(chunk))) => buf.extend_from_slice(&chunk),
      Ok(Ok(None)) | Ok(Err(_)) | Err(_) => break,
    }
  }
  buf
}

#[tokio::test]
async fn exec_command_runs_the_stub_cli_over_a_real_ssh_connection() {
  let cfg = require_it!();
  let pool = SshConnectionPool::new();
  let cancellation = CancellationToken::new();

  let output = exec_command(
    &pool,
    &cfg.endpoint,
    &["ok".to_string(), "hello world".to_string()],
    ExecLimits::default(),
    &cancellation,
  )
  .await
  .expect("exec_command against a real sshd should succeed");

  assert_eq!(
    String::from_utf8_lossy(&output.stdout).trim(),
    "stub-ok:hello world",
    "argument containing a space must survive shell_quote's round trip \
     through a real exec channel, not just the in-process mock"
  );
}

#[tokio::test]
async fn exec_command_surfaces_the_stub_cli_nonzero_exit_and_stderr() {
  let cfg = require_it!();
  let pool = SshConnectionPool::new();
  let cancellation = CancellationToken::new();

  let error = exec_command(
    &pool,
    &cfg.endpoint,
    &["fail".to_string(), "boom".to_string()],
    ExecLimits::default(),
    &cancellation,
  )
  .await
  .expect_err("a nonzero exit from a real remote command must be reported as an error");

  match error {
    SshTransportError::CommandFailed {
      exit_status,
      stderr,
      ..
    } => {
      assert_eq!(exit_status, Some(7));
      assert!(
        stderr.contains("stub-fail:boom"),
        "stderr should carry the stub's message, got: {stderr:?}"
      );
    }
    other => panic!("expected CommandFailed, got {other:?}"),
  }
}

#[tokio::test]
async fn pool_reuses_one_connection_across_multiple_execs_against_a_real_server() {
  let cfg = require_it!();
  let pool = SshConnectionPool::new();

  for i in 0..3 {
    let cancellation = CancellationToken::new();
    let output = exec_command(
      &pool,
      &cfg.endpoint,
      &["ok".to_string(), format!("round-{i}")],
      ExecLimits::default(),
      &cancellation,
    )
    .await
    .unwrap_or_else(|error| panic!("exec {i} against real sshd failed: {error}"));
    assert_eq!(
      String::from_utf8_lossy(&output.stdout).trim(),
      format!("stub-ok:round-{i}")
    );
  }

  assert_eq!(
    pool.pooled_connection_count().await,
    1,
    "three sequential commands against the same endpoint must reuse one \
     pooled network connection to the real server, not open three"
  );
}

#[tokio::test]
async fn exec_command_rejects_a_real_server_whose_host_key_is_not_the_pinned_one() {
  let mut cfg = require_it!();
  poison_host_keys(&mut cfg.endpoint);
  cfg.endpoint.id = "remote-ssh-server-it-wrong-fingerprint".to_string();

  let pool = SshConnectionPool::new();
  let cancellation = CancellationToken::new();

  let error = exec_command(
    &pool,
    &cfg.endpoint,
    &["ok".to_string(), "should-not-run".to_string()],
    ExecLimits::default(),
    &cancellation,
  )
  .await
  .expect_err("a mismatched pinned host key must reject the real server, not connect to it");

  assert!(
    matches!(
      error,
      SshTransportError::HostKeyMismatch { .. } | SshTransportError::ConnectionFailed(_)
    ),
    "expected a host-key rejection, got {error:?}"
  );
}

#[tokio::test]
async fn exec_command_enforces_deadline_against_a_real_slow_command() {
  let cfg = require_it!();
  let pool = SshConnectionPool::new();
  let cancellation = CancellationToken::new();

  let error = exec_command(
    &pool,
    &cfg.endpoint,
    &["sleep".to_string(), "5".to_string()],
    ExecLimits {
      deadline: Duration::from_millis(300),
      ..ExecLimits::default()
    },
    &cancellation,
  )
  .await
  .expect_err("a command that outlives the deadline must time out, not hang the test");

  assert!(matches!(error, SshTransportError::DeadlineExceeded));
}

#[tokio::test]
async fn certificate_auth_runs_exec_through_the_production_transport() {
  let (cfg, (ca, src_key, username)) = require_cert_it!();
  let temp = tempfile::tempdir().unwrap();
  let endpoint = issue_and_endpoint(&cfg, &ca, &src_key, temp.path(), &username, "+5m", "valid");
  let pool = SshConnectionPool::new();
  let cancellation = CancellationToken::new();

  let output = exec_command(
    &pool,
    &endpoint,
    &["ok".to_string(), "from-cert".to_string()],
    ExecLimits::default(),
    &cancellation,
  )
  .await
  .expect("a valid user certificate signed by the trusted CA must authenticate");
  assert_eq!(
    String::from_utf8_lossy(&output.stdout).trim(),
    "stub-ok:from-cert"
  );
}

#[tokio::test]
async fn certificate_auth_rejects_a_cert_signed_by_the_wrong_ca() {
  let (cfg, (_ca, src_key, username)) = require_cert_it!();
  let temp = tempfile::tempdir().unwrap();
  let wrong_ca = temp.path().join("wrong-ca");
  let generated = Command::new("ssh-keygen")
    .args([
      "-t",
      "ed25519",
      "-N",
      "",
      "-C",
      "treq-it-untrusted-ca",
      "-f",
      &wrong_ca.to_string_lossy(),
    ])
    .output()
    .expect("ssh-keygen");
  assert!(generated.status.success());

  let client_dir = temp.path().join("client");
  std::fs::create_dir_all(&client_dir).unwrap();
  let endpoint = issue_and_endpoint(
    &cfg,
    &wrong_ca,
    &src_key,
    &client_dir,
    &username,
    "+5m",
    "wrong-ca",
  );
  let pool = SshConnectionPool::new();
  let cancellation = CancellationToken::new();
  let error = exec_command(
    &pool,
    &endpoint,
    &["ok".to_string(), "should-not-run".to_string()],
    ExecLimits::default(),
    &cancellation,
  )
  .await
  .expect_err("a certificate signed by an untrusted CA must be rejected");
  assert!(
    auth_rejected(&error),
    "expected auth failure, got {error:?}"
  );
}

#[tokio::test]
async fn certificate_auth_rejects_an_invalid_principal() {
  let (cfg, (ca, src_key, _username)) = require_cert_it!();
  let temp = tempfile::tempdir().unwrap();
  let endpoint = issue_and_endpoint(
    &cfg,
    &ca,
    &src_key,
    temp.path(),
    "not-the-login-user",
    "+5m",
    "bad-principal",
  );
  let pool = SshConnectionPool::new();
  let cancellation = CancellationToken::new();
  let error = exec_command(
    &pool,
    &endpoint,
    &["ok".to_string(), "should-not-run".to_string()],
    ExecLimits::default(),
    &cancellation,
  )
  .await
  .expect_err("a certificate whose principal does not match the login user must be rejected");
  assert!(
    auth_rejected(&error),
    "expected auth failure, got {error:?}"
  );
}

#[tokio::test]
async fn certificate_auth_rejects_a_not_yet_valid_cert() {
  let (cfg, (ca, src_key, username)) = require_cert_it!();
  let temp = tempfile::tempdir().unwrap();
  let endpoint = issue_and_endpoint(
    &cfg,
    &ca,
    &src_key,
    temp.path(),
    &username,
    "+1d:+2d",
    "not-yet-valid",
  );
  let pool = SshConnectionPool::new();
  let cancellation = CancellationToken::new();
  let error = exec_command(
    &pool,
    &endpoint,
    &["ok".to_string(), "should-not-run".to_string()],
    ExecLimits::default(),
    &cancellation,
  )
  .await
  .expect_err("a certificate whose valid-after is in the future must be rejected");
  assert!(
    auth_rejected(&error),
    "expected auth failure, got {error:?}"
  );
}

#[tokio::test]
async fn certificate_auth_rejects_an_expired_cert() {
  let (cfg, (ca, src_key, username)) = require_cert_it!();
  let temp = tempfile::tempdir().unwrap();
  let endpoint = issue_and_endpoint(
    &cfg,
    &ca,
    &src_key,
    temp.path(),
    &username,
    "-2d:-1d",
    "expired",
  );
  let pool = SshConnectionPool::new();
  let cancellation = CancellationToken::new();
  let error = exec_command(
    &pool,
    &endpoint,
    &["ok".to_string(), "should-not-run".to_string()],
    ExecLimits::default(),
    &cancellation,
  )
  .await
  .expect_err("an expired certificate must be rejected");
  assert!(
    auth_rejected(&error),
    "expected auth failure, got {error:?}"
  );
}

#[tokio::test]
async fn certificate_auth_rejects_host_key_mismatch_before_authentication() {
  let (cfg, (ca, src_key, username)) = require_cert_it!();
  let temp = tempfile::tempdir().unwrap();
  let mut endpoint = issue_and_endpoint(
    &cfg,
    &ca,
    &src_key,
    temp.path(),
    &username,
    "+5m",
    "hostkey-mismatch",
  );
  poison_host_keys(&mut endpoint);

  let pool = SshConnectionPool::new();
  let cancellation = CancellationToken::new();
  let error = exec_command(
    &pool,
    &endpoint,
    &["ok".to_string(), "should-not-run".to_string()],
    ExecLimits::default(),
    &cancellation,
  )
  .await
  .expect_err("host-key mismatch must fail before certificate authentication");
  assert!(
    matches!(
      error,
      SshTransportError::HostKeyMismatch { .. } | SshTransportError::ConnectionFailed(_)
    ),
    "expected a host-key rejection, got {error:?}"
  );
}

#[tokio::test]
async fn certificate_auth_reuses_one_pooled_connection_across_repeated_execs() {
  let (cfg, (ca, src_key, username)) = require_cert_it!();
  let temp = tempfile::tempdir().unwrap();
  let endpoint = issue_and_endpoint(&cfg, &ca, &src_key, temp.path(), &username, "+5m", "pooled");
  let pool = SshConnectionPool::new();

  for i in 0..3 {
    let cancellation = CancellationToken::new();
    let output = exec_command(
      &pool,
      &endpoint,
      &["ok".to_string(), format!("cert-round-{i}")],
      ExecLimits::default(),
      &cancellation,
    )
    .await
    .unwrap_or_else(|error| panic!("cert exec {i} failed: {error}"));
    assert_eq!(
      String::from_utf8_lossy(&output.stdout).trim(),
      format!("stub-ok:cert-round-{i}")
    );
  }
  assert_eq!(pool.pooled_connection_count().await, 1);
}

#[tokio::test]
async fn client_cutoff_tears_down_a_live_connection_without_sshd_krl() {
  // Distinction vs OpenSSH revocation: this sshd has no KRL / RevokedKeys
  // file, so it cannot enforce Treq control-plane client-key revocation.
  // Production cutoff is a client-side hard stop (PRD "Hard cutoff on
  // revocation or expiry") after the control plane refuses renewal.
  let cfg = require_it!();
  let pool = SshConnectionPool::new();
  let cancellation = CancellationToken::new();
  exec_command(
    &pool,
    &cfg.endpoint,
    &["ok".to_string(), "before-cutoff".to_string()],
    ExecLimits::default(),
    &cancellation,
  )
  .await
  .expect("pre-cutoff exec");
  assert_eq!(pool.pooled_connection_count().await, 1);

  pool
    .force_cutoff(&cfg.endpoint.id, CutoffReason::KeyRevoked)
    .await;
  assert_eq!(pool.pooled_connection_count().await, 0);

  let error = exec_command(
    &pool,
    &cfg.endpoint,
    &["ok".to_string(), "after-cutoff".to_string()],
    ExecLimits::default(),
    &cancellation,
  )
  .await
  .expect_err("cutoff must refuse locally without contacting sshd");
  assert_eq!(
    error,
    SshTransportError::CredentialCutOff {
      endpoint_id: cfg.endpoint.id.clone(),
      reason: CutoffReason::KeyRevoked,
    }
  );
}

#[tokio::test]
async fn pty_starts_in_the_selected_workspace_directory() {
  let cfg = require_it!();
  let Some(workspace) = std::env::var("TREQ_SSH_IT_WORKSPACE_DIR").ok() else {
    eprintln!("[remote-ssh-server-it] SKIP pty cwd: TREQ_SSH_IT_WORKSPACE_DIR unset");
    return;
  };
  let pool = SshConnectionPool::new();
  // `open_in_directory` has no built-in deadline (unlike `exec_command`'s
  // `ExecLimits`) - wrap it here so a server that never replies to the PTY
  // request fails this test in seconds instead of hanging the whole CI job
  // until its timeout.
  let pty = tokio::time::timeout(
    Duration::from_secs(10),
    RemotePtyChannel::open_in_directory(
      &pool,
      &cfg.endpoint,
      "xterm",
      80,
      24,
      Some("pwd"),
      Some(&workspace),
    ),
  )
  .await
  .expect("PTY exec in selected directory timed out")
  .expect("PTY exec in selected directory");
  let output = collect_pty_bytes(&pty, Duration::from_secs(3)).await;
  let text = String::from_utf8_lossy(&output);
  assert!(
    text.contains(workspace.trim_end_matches('/')),
    "PTY pwd must report the selected workspace, got {text:?}"
  );
  tokio::time::timeout(Duration::from_secs(5), pty.close())
    .await
    .expect("PTY close timed out")
    .unwrap();
}

#[tokio::test]
async fn pty_round_trips_input_output_resize_and_close() {
  let cfg = require_it!();
  let pool = SshConnectionPool::new();
  // See the deadline comment in pty_starts_in_the_selected_workspace_directory.
  let pty = tokio::time::timeout(
    Duration::from_secs(10),
    RemotePtyChannel::open_in_directory(
      &pool,
      &cfg.endpoint,
      "xterm",
      80,
      24,
      Some("sh -c 'IFS= read -r line; printf \"got:%s\\n\" \"$line\"; IFS= read -r _; stty size'"),
      None,
    ),
  )
  .await
  .expect("PTY open timed out")
  .expect("PTY open");

  pty.write(b"hello-pty\n").await.expect("write to PTY");
  let mut text = String::new();
  let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
  while tokio::time::Instant::now() < deadline {
    if let Ok(Ok(Some(chunk))) =
      tokio::time::timeout(Duration::from_millis(400), pty.read_chunk()).await
    {
      text.push_str(&String::from_utf8_lossy(&chunk));
      if text.contains("got:hello-pty") {
        break;
      }
    }
  }
  assert!(
    text.contains("got:hello-pty"),
    "PTY must echo the written line, got {text:?}"
  );

  pty.resize(40, 12).await.expect("resize");
  pty.write(b"go\n").await.expect("write after resize");
  let rest_deadline = tokio::time::Instant::now() + Duration::from_secs(4);
  while tokio::time::Instant::now() < rest_deadline {
    if let Ok(Ok(Some(chunk))) =
      tokio::time::timeout(Duration::from_millis(400), pty.read_chunk()).await
    {
      text.push_str(&String::from_utf8_lossy(&chunk));
      if text.contains("12 40") {
        break;
      }
    }
  }
  assert!(
    text.contains("12 40"),
    "after resize, stty size must report 12 40, got {text:?}"
  );

  tokio::time::timeout(Duration::from_secs(5), pty.close())
    .await
    .expect("PTY close timed out")
    .expect("close");
  // Unlike every other read in this test, this one had no deadline - if the
  // server never signals the channel closed after our close request, this
  // just hangs forever with no error, which is exactly what happened in CI:
  // the job ran to its 20-minute timeout with no further output after
  // "resize". Bound it like the others.
  let after_close = tokio::time::timeout(Duration::from_secs(5), pty.read_chunk()).await;
  assert!(
    matches!(after_close, Ok(Ok(None)) | Ok(Err(_)) | Err(_)),
    "closed PTY must not keep yielding data, got {after_close:?}"
  );
}

// ---------------------------------------------------------------------------
// WebSocket relay transport
// ---------------------------------------------------------------------------

const RELAY_IT_TOKEN: &str = "relay-it-session-token";

/// Stands in for the `remote-ssh-relay` Edge Function: accepts a WebSocket
/// only when the Authorization header carries `RELAY_IT_TOKEN`, then pipes
/// raw bytes to the real sshd over TCP, as the Sprites TCP proxy does.
/// Returns the relay URL.
async fn spawn_ws_to_tcp_bridge(sshd_host: String, sshd_port: u16) -> String {
  use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
  use tokio_tungstenite::tungstenite::http::StatusCode;

  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let addr = listener.local_addr().unwrap();
  tokio::spawn(async move {
    loop {
      let Ok((tcp, _)) = listener.accept().await else {
        return;
      };
      let sshd_host = sshd_host.clone();
      tokio::spawn(async move {
        #[expect(
          clippy::result_large_err,
          reason = "the callback signature is fixed by tungstenite's handshake callback"
        )]
        let check = |request: &Request, response: Response| -> Result<Response, ErrorResponse> {
          let authorized = request
            .headers()
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            == Some(&format!("Bearer {RELAY_IT_TOKEN}"));
          if authorized {
            Ok(response)
          } else {
            let mut refusal = ErrorResponse::new(None);
            *refusal.status_mut() = StatusCode::UNAUTHORIZED;
            Err(refusal)
          }
        };
        let Ok(ws) = tokio_tungstenite::accept_hdr_async(tcp, check).await else {
          return;
        };
        let Ok(mut sshd) = tokio::net::TcpStream::connect((sshd_host.as_str(), sshd_port)).await
        else {
          return;
        };
        let mut client = treq_lib::core::remote_ssh_ws_stream::WsByteStream::new(ws);
        let _ = tokio::io::copy_bidirectional(&mut client, &mut sshd).await;
      });
    }
  });
  format!("ws://{addr}/functions/v1/remote-ssh-relay?endpoint_id=it&key_id=it")
}

async fn relay_endpoint(mut endpoint: SshEndpoint) -> SshEndpoint {
  let url = spawn_ws_to_tcp_bridge(endpoint.hostname.clone(), endpoint.port).await;
  endpoint.id = format!("{}-relay", endpoint.id);
  endpoint.transport = SshTransport::Relay { url };
  // The relay transport never dials these; poison them to prove it.
  endpoint.hostname = "sshd-inside-the-sprite.invalid".to_string();
  endpoint.port = 1;
  endpoint
}

#[tokio::test]
async fn certificate_auth_runs_exec_through_a_websocket_relay() {
  let (cfg, (ca, src_key, username)) = require_cert_it!();
  let temp = tempfile::tempdir().unwrap();
  let endpoint = issue_and_endpoint(&cfg, &ca, &src_key, temp.path(), &username, "+5m", "relay");
  let endpoint = relay_endpoint(endpoint).await;
  let pool = SshConnectionPool::new();
  pool.set_relay_access_token(Some(RELAY_IT_TOKEN.to_string()));
  let cancellation = CancellationToken::new();

  for round in 0..2 {
    let output = exec_command(
      &pool,
      &endpoint,
      &["ok".to_string(), format!("via-relay-{round}")],
      ExecLimits::default(),
      &cancellation,
    )
    .await
    .expect("certificate auth must work end to end through the relay");
    assert_eq!(
      String::from_utf8_lossy(&output.stdout).trim(),
      format!("stub-ok:via-relay-{round}")
    );
  }
  assert_eq!(
    pool.metrics_snapshot().pooled_connection_reuse_count,
    1,
    "the second exec must reuse the relayed connection"
  );
}

#[tokio::test]
async fn relay_transport_still_pins_the_host_key() {
  let cfg = require_it!();
  let mut endpoint = relay_endpoint(cfg.endpoint.clone()).await;
  poison_host_keys(&mut endpoint);
  let pool = SshConnectionPool::new();
  pool.set_relay_access_token(Some(RELAY_IT_TOKEN.to_string()));
  let error = exec_command(
    &pool,
    &endpoint,
    &["ok".to_string()],
    ExecLimits::default(),
    &CancellationToken::new(),
  )
  .await
  .expect_err("a relayed connection must still reject an unpinned host key");
  assert!(
    matches!(error, SshTransportError::ConnectionFailed(_)),
    "{error:?}"
  );
}

#[tokio::test]
async fn relay_transport_requires_a_valid_session_token() {
  let cfg = require_it!();
  let endpoint = relay_endpoint(cfg.endpoint.clone()).await;

  let no_token = SshConnectionPool::new();
  let error = exec_command(
    &no_token,
    &endpoint,
    &["ok".to_string()],
    ExecLimits::default(),
    &CancellationToken::new(),
  )
  .await
  .expect_err("no token means no relay connection");
  assert!(format!("{error:?}").contains("sign in"), "{error:?}");

  let wrong_token = SshConnectionPool::new();
  wrong_token.set_relay_access_token(Some("not-the-token".to_string()));
  let error = exec_command(
    &wrong_token,
    &endpoint,
    &["ok".to_string()],
    ExecLimits::default(),
    &CancellationToken::new(),
  )
  .await
  .expect_err("a refused relay handshake must fail the connection");
  let message = format!("{error:?}");
  assert!(message.contains("401"), "{message}");
  assert!(!message.contains("not-the-token"), "{message}");
}
