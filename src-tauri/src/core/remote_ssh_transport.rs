//! Native Rust SSH transport (Phase 4: "Native desktop SSH").
//!
//! Replaces system `ssh` subprocesses with the pure-Rust [`russh`] client for
//! the managed transport path. This module owns exactly the responsibilities
//! listed under the PRD's "Native SSH transport" section:
//!
//! - strict host-key verification against Phase 3's [`TrustedHostKey`]s;
//! - user-key and certificate authentication ([`SshAuthentication`]);
//! - connection pooling and channel multiplexing;
//! - keepalives, reconnect, and stale-connection recovery;
//! - exec channels (non-interactive, JSON-producing) with deadlines,
//!   cancellation, and output limits;
//! - PTY channels for interactive shells and agents, with resize support.
//!
//! It consumes the domain types from [`crate::core::remote_control_plane`]
//! (`SshEndpoint`, `TrustedHostKey`, `SshEndpointSource`, `SshAuthentication`)
//! rather than redefining them. It does not implement JJ, Git, workspace, or
//! any other repository behavior: callers hand it a fully-formed
//! `treq <command> --format=json` argument vector and get back raw
//! stdout/stderr bytes plus an exit status; interpreting that JSON is Phase
//! 5's job.
//!
//! SFTP and port forwarding are out of scope, per the PRD.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use russh::client::{AuthResult, Handle, Handler as ClientHandlerTrait};
use russh::keys::{PrivateKey, PrivateKeyWithHashAlg};
use russh::{ChannelMsg, Disconnect};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex as AsyncMutex;
use tokio::sync::Notify;

use crate::core::remote_control_plane::{
  SshAuthentication, SshEndpoint, SshTransport, TrustedHostKey,
};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors surfaced by the native transport. Kept distinct from
/// [`crate::core::remote::TransportError`] (the older, subprocess-based
/// transport's error type) so a Phase 5 caller can pattern-match without
/// pulling in `std::process` semantics that no longer apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SshTransportError {
  /// The presented host key did not match any trusted fingerprint recorded
  /// for this endpoint. Never bypassed, never auto-trusted.
  HostKeyMismatch {
    endpoint_id: String,
    presented_fingerprint: String,
  },
  /// TCP connect, key exchange, or protocol negotiation failed.
  ConnectionFailed(String),
  /// Authentication (key or certificate) was rejected by the server.
  AuthenticationFailed(String),
  /// Key or certificate material could not be loaded from local disk.
  KeyMaterialUnavailable(String),
  /// The exec channel exceeded its total operation deadline.
  DeadlineExceeded,
  /// The exec channel produced more stdout/stderr bytes than the configured
  /// limit before completing.
  OutputLimitExceeded,
  /// The caller cancelled the operation (e.g. dropped a `CancellationToken`).
  Cancelled,
  /// The remote command exited non-zero. Carries stderr so a Phase 5 caller
  /// can surface diagnostics without re-parsing stdout.
  CommandFailed {
    exit_status: Option<u32>,
    stderr: String,
    /// Raw stdout bytes, UTF-8 lossily decoded. The Treq CLI still emits a
    /// structured `{"error":{"code":...,"message":...}}` body on stdout even
    /// on failure, so callers that want the CLI's own error code (per the
    /// PRD's "error codes survive transport mapping") parse this rather than
    /// falling back to a generic transport-level message.
    stdout: String,
  },
  /// stdout was not valid UTF-8 / valid JSON once decoded by the caller.
  ProtocolError(String),
  /// Underlying channel or session I/O failure, e.g. after a stale
  /// connection is detected mid-operation.
  ChannelError(String),
  /// The credential for this endpoint has been forced into the hard-cutoff
  /// state (PRD "Hard cutoff on revocation or expiry"): the client key was
  /// revoked, the certificate lapsed without a valid renewal, or the
  /// Supabase session ended. No further structured commands, shell, or
  /// agent traffic may be sent until the user reauthenticates and a fresh
  /// certificate is issued through [`SshConnectionPool::clear_cutoff`].
  CredentialCutOff {
    endpoint_id: String,
    reason: CutoffReason,
  },
}

impl std::fmt::Display for SshTransportError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::HostKeyMismatch { endpoint_id, .. } => {
        write!(f, "host key mismatch for endpoint {endpoint_id}")
      }
      Self::ConnectionFailed(message) => write!(f, "ssh connection failed: {message}"),
      Self::AuthenticationFailed(message) => write!(f, "ssh authentication failed: {message}"),
      Self::KeyMaterialUnavailable(message) => write!(f, "ssh key material unavailable: {message}"),
      Self::DeadlineExceeded => write!(f, "ssh operation exceeded its deadline"),
      Self::OutputLimitExceeded => write!(f, "ssh command output exceeded the configured limit"),
      Self::Cancelled => write!(f, "ssh operation was cancelled"),
      Self::CommandFailed {
        exit_status,
        stderr,
        ..
      } => {
        write!(f, "remote command failed (exit={exit_status:?}): {stderr}")
      }
      Self::ProtocolError(message) => write!(f, "invalid remote command protocol: {message}"),
      Self::ChannelError(message) => write!(f, "ssh channel error: {message}"),
      Self::CredentialCutOff {
        endpoint_id,
        reason,
      } => write!(
        f,
        "credential for endpoint {endpoint_id} is cut off ({reason}); reauthenticate to continue"
      ),
    }
  }
}

/// Why a credential was forced into the hard-cutoff state (PRD "Hard cutoff
/// on revocation or expiry"). Mirrors the renewal-refusal conditions from
/// "Silent renewal while the session is active": the Supabase session ended,
/// the client's public key was revoked, the instance is no longer accessible
/// to the user, or the certificate simply expired without a valid renewal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CutoffReason {
  SessionEnded,
  KeyRevoked,
  InstanceInaccessible,
  CertificateExpired,
}

impl std::fmt::Display for CutoffReason {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::SessionEnded => write!(f, "session ended"),
      Self::KeyRevoked => write!(f, "client key revoked"),
      Self::InstanceInaccessible => write!(f, "instance no longer accessible"),
      Self::CertificateExpired => write!(f, "certificate expired without renewal"),
    }
  }
}

impl std::error::Error for SshTransportError {}

/// [`ClientHandlerTrait::Error`] must implement `From<russh::Error>`.
impl From<russh::Error> for SshTransportError {
  fn from(error: russh::Error) -> Self {
    Self::ChannelError(error.to_string())
  }
}

// ---------------------------------------------------------------------------
// Metrics (Phase 7: "Client and transport telemetry")
// ---------------------------------------------------------------------------
//
// This repo has no existing metrics backend (no `metrics`/`prometheus`
// crate, no telemetry-export pipeline) - `tracing` is used for structured
// logs only. Rather than inventing a fake export path or bolting on an
// external dependency for a single module, this is a small in-process
// counters/histograms struct: cheap atomics, no locks on the hot path, and
// a plain snapshot a Tauri command (or a future diagnostics panel) can read.
// It records exactly the fields the PRD's "Client and transport telemetry"
// list enumerates, and nothing from the "Never log" list ever reaches it -
// every field here is a count or a duration, never key material, output, or
// prompts.

/// A minimal duration histogram: count, sum, and max, in milliseconds.
/// Deliberately not a full HDR histogram - this module does not need
/// percentiles, only "how many, how long in total, what was the worst one",
/// which is enough to drive an operational dashboard or a regression alert.
#[derive(Debug, Default)]
pub struct DurationStats {
  count: AtomicU64,
  sum_ms: AtomicU64,
  max_ms: AtomicU64,
}

impl DurationStats {
  fn record(&self, duration: Duration) {
    let ms = duration.as_millis().min(u128::from(u64::MAX)) as u64;
    self.count.fetch_add(1, Ordering::Relaxed);
    self.sum_ms.fetch_add(ms, Ordering::Relaxed);
    self.max_ms.fetch_max(ms, Ordering::Relaxed);
  }

  pub fn snapshot(&self) -> DurationStatsSnapshot {
    let count = self.count.load(Ordering::Relaxed);
    let sum_ms = self.sum_ms.load(Ordering::Relaxed);
    DurationStatsSnapshot {
      count,
      sum_ms,
      max_ms: self.max_ms.load(Ordering::Relaxed),
      avg_ms: if count > 0 {
        sum_ms as f64 / count as f64
      } else {
        0.0
      },
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DurationStatsSnapshot {
  pub count: u64,
  pub sum_ms: u64,
  pub max_ms: u64,
  pub avg_ms: f64,
}

/// Coarse exit categories for a completed exec channel (PRD: "exec channel
/// duration and exit category").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExecExitCategory {
  Success,
  CommandFailed,
  Timeout,
  Cancelled,
  OutputLimitExceeded,
  TransportError,
}

/// Process-wide (per-`SshConnectionPool`) transport telemetry. Every field
/// maps directly onto one bullet of the PRD's "Client and transport
/// telemetry" list; see the doc comment on each field.
#[derive(Debug, Default)]
pub struct SshTransportMetrics {
  /// "DNS and TCP connection duration" - TCP connect is not separable from
  /// DNS resolution with `russh::client::connect`'s address-tuple API, so
  /// this timer spans both, from the start of `connect()` to a successfully
  /// opened (pre-auth) transport.
  pub dns_tcp_connect: DurationStats,
  /// "SSH negotiation and authentication duration" - from a successfully
  /// opened transport through a completed `authenticate` call.
  pub ssh_negotiation_and_auth: DurationStats,
  /// "host-key mismatch count".
  pub host_key_mismatch_count: AtomicU64,
  /// "pooled connection reuse" - incremented every time `get_or_connect`
  /// returns an existing live connection instead of dialing a new one.
  pub pooled_connection_reuse_count: AtomicU64,
  /// "reconnect attempts" - incremented every time a dead/missing pooled
  /// connection triggers a fresh `connect()` call for a key that previously
  /// had one (a first-ever connection for a key is not a reconnect).
  pub reconnect_attempts: AtomicU64,
  /// "exec channel duration" - wall-clock time of one `exec_command` call,
  /// regardless of outcome.
  pub exec_channel_duration: DurationStats,
  /// "exec channel ... exit category" plus "timeout, cancellation, and
  /// output-limit failures", all recorded as counts keyed by category so a
  /// dashboard can chart the breakdown without re-deriving it from raw
  /// error strings.
  exec_exit_counts: std::sync::Mutex<HashMap<ExecExitCategory, u64>>,
  /// "PTY start and exit".
  pub pty_start_count: AtomicU64,
  pub pty_exit_count: AtomicU64,
  /// "remote Treq version mismatch" - incremented by a Phase 5+ caller once
  /// it compares the remote `treq --version` output against the client's
  /// expected version; this module does not itself know the CLI's version
  /// contract, so it only exposes the counter.
  pub remote_version_mismatch_count: AtomicU64,
  /// "post-reconnect state verifications before a mutation retry, and their
  /// outcome (already applied, safe to retry, ambiguous)" - one counter per
  /// outcome, incremented by `crate::core::remote::retry_after_reconnect`
  /// exactly when a network failure forced a verification read.
  pub post_reconnect_already_applied_count: AtomicU64,
  pub post_reconnect_retried_count: AtomicU64,
  pub post_reconnect_ambiguous_count: AtomicU64,
}

impl SshTransportMetrics {
  fn record_exec_exit(&self, category: ExecExitCategory) {
    let mut counts = self
      .exec_exit_counts
      .lock()
      .unwrap_or_else(|e| e.into_inner());
    *counts.entry(category).or_insert(0) += 1;
  }

  pub fn exec_exit_snapshot(&self) -> HashMap<ExecExitCategory, u64> {
    self
      .exec_exit_counts
      .lock()
      .unwrap_or_else(|e| e.into_inner())
      .clone()
  }

  pub fn snapshot(&self) -> SshTransportMetricsSnapshot {
    let exit_counts = self.exec_exit_snapshot();
    let count_of = |category: ExecExitCategory| exit_counts.get(&category).copied().unwrap_or(0);
    SshTransportMetricsSnapshot {
      dns_tcp_connect: self.dns_tcp_connect.snapshot(),
      ssh_negotiation_and_auth: self.ssh_negotiation_and_auth.snapshot(),
      host_key_mismatch_count: self.host_key_mismatch_count.load(Ordering::Relaxed),
      pooled_connection_reuse_count: self.pooled_connection_reuse_count.load(Ordering::Relaxed),
      reconnect_attempts: self.reconnect_attempts.load(Ordering::Relaxed),
      exec_channel_duration: self.exec_channel_duration.snapshot(),
      exec_success_count: count_of(ExecExitCategory::Success),
      exec_command_failed_count: count_of(ExecExitCategory::CommandFailed),
      exec_timeout_count: count_of(ExecExitCategory::Timeout),
      exec_cancelled_count: count_of(ExecExitCategory::Cancelled),
      exec_output_limit_exceeded_count: count_of(ExecExitCategory::OutputLimitExceeded),
      exec_transport_error_count: count_of(ExecExitCategory::TransportError),
      pty_start_count: self.pty_start_count.load(Ordering::Relaxed),
      pty_exit_count: self.pty_exit_count.load(Ordering::Relaxed),
      remote_version_mismatch_count: self.remote_version_mismatch_count.load(Ordering::Relaxed),
      post_reconnect_already_applied_count: self
        .post_reconnect_already_applied_count
        .load(Ordering::Relaxed),
      post_reconnect_retried_count: self.post_reconnect_retried_count.load(Ordering::Relaxed),
      post_reconnect_ambiguous_count: self.post_reconnect_ambiguous_count.load(Ordering::Relaxed),
    }
  }

  /// Records one verify-before-retry decision (PRD: "post-reconnect state
  /// verifications before a mutation retry, and their outcome"). Called by
  /// `crate::core::remote::retry_after_reconnect`'s `on_verification` hook.
  pub fn record_post_reconnect_verification(
    &self,
    outcome: crate::core::remote::PostReconnectVerification,
  ) {
    let counter = match outcome {
      crate::core::remote::PostReconnectVerification::AlreadyApplied => {
        &self.post_reconnect_already_applied_count
      }
      crate::core::remote::PostReconnectVerification::Retried => &self.post_reconnect_retried_count,
      crate::core::remote::PostReconnectVerification::Ambiguous => {
        &self.post_reconnect_ambiguous_count
      }
    };
    counter.fetch_add(1, Ordering::Relaxed);
    tracing::info!(outcome = ?outcome, "post-reconnect mutation verification");
  }
}

/// Plain-data snapshot of [`SshTransportMetrics`], serializable so a Tauri
/// command can hand it straight to the frontend (or a future diagnostics
/// panel) without exposing the atomics themselves.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SshTransportMetricsSnapshot {
  pub dns_tcp_connect: DurationStatsSnapshot,
  pub ssh_negotiation_and_auth: DurationStatsSnapshot,
  pub host_key_mismatch_count: u64,
  pub pooled_connection_reuse_count: u64,
  pub reconnect_attempts: u64,
  pub exec_channel_duration: DurationStatsSnapshot,
  pub exec_success_count: u64,
  pub exec_command_failed_count: u64,
  pub exec_timeout_count: u64,
  pub exec_cancelled_count: u64,
  pub exec_output_limit_exceeded_count: u64,
  pub exec_transport_error_count: u64,
  pub pty_start_count: u64,
  pub pty_exit_count: u64,
  pub remote_version_mismatch_count: u64,
  pub post_reconnect_already_applied_count: u64,
  pub post_reconnect_retried_count: u64,
  pub post_reconnect_ambiguous_count: u64,
}

// ---------------------------------------------------------------------------
// Host-key verification
// ---------------------------------------------------------------------------

/// Computes the SHA256 fingerprint string (`SHA256:<base64>`) for a server
/// host key or certificate, in the same shape as [`TrustedHostKey`]'s
/// `fingerprint_sha256`.
pub(crate) fn fingerprint_of(key: &russh::keys::PublicKeyOrCertificate) -> Option<String> {
  match key {
    russh::keys::PublicKeyOrCertificate::PublicKey { key, .. } => {
      Some(key.fingerprint(russh::keys::HashAlg::Sha256).to_string())
    }
    // Server host certificates are not part of the Phase 3 trust model
    // (`TrustedHostKey` pins bare host keys); rejecting them is the strict,
    // never-bypass default rather than silently trusting an unpinned CA.
    russh::keys::PublicKeyOrCertificate::Certificate(_) => None,
  }
}

/// Strict host-key verification against a fixed set of trusted fingerprints
/// for one endpoint/generation. Never falls back to trust-on-first-use and
/// never consults the OS `~/.ssh/known_hosts` — managed host trust is tracked
/// independently, per the PRD's "Host-key verification" section.
#[derive(Debug, Clone)]
pub struct HostKeyVerifier {
  endpoint_id: String,
  trusted_fingerprints: Vec<String>,
}

impl HostKeyVerifier {
  pub fn new(endpoint_id: impl Into<String>, host_keys: &[TrustedHostKey]) -> Self {
    Self {
      endpoint_id: endpoint_id.into(),
      trusted_fingerprints: host_keys
        .iter()
        .map(|key| key.fingerprint_sha256.clone())
        .collect(),
    }
  }

  /// Returns `Ok(())` when `presented` matches one of the trusted
  /// fingerprints, `Err` (never a bypassable warning) otherwise.
  pub fn verify(
    &self,
    presented: &russh::keys::PublicKeyOrCertificate,
  ) -> Result<(), SshTransportError> {
    let Some(fingerprint) = fingerprint_of(presented) else {
      return Err(SshTransportError::HostKeyMismatch {
        endpoint_id: self.endpoint_id.clone(),
        presented_fingerprint: "<certificate>".to_string(),
      });
    };
    if self
      .trusted_fingerprints
      .iter()
      .any(|trusted| trusted == &fingerprint)
    {
      Ok(())
    } else {
      Err(SshTransportError::HostKeyMismatch {
        endpoint_id: self.endpoint_id.clone(),
        presented_fingerprint: fingerprint,
      })
    }
  }
}

// ---------------------------------------------------------------------------
// Client handler
// ---------------------------------------------------------------------------

/// `russh::client::Handler` implementation. Holds only what is needed to
/// verify the server's host key; it performs no logging of key material and
/// carries no secrets itself (those live in [`ClientAuthenticator`]).
#[derive(Clone)]
struct TreqSshClientHandler {
  verifier: HostKeyVerifier,
  metrics: Arc<SshTransportMetrics>,
}

impl ClientHandlerTrait for TreqSshClientHandler {
  type Error = SshTransportError;

  async fn check_server_key(
    &mut self,
    server_public_key: &russh::keys::PublicKeyOrCertificate,
  ) -> Result<bool, Self::Error> {
    match self.verifier.verify(server_public_key) {
      Ok(()) => Ok(true),
      Err(error) => {
        self
          .metrics
          .host_key_mismatch_count
          .fetch_add(1, Ordering::Relaxed);
        // Redaction: log only the endpoint id and fingerprint (public,
        // non-sensitive values), never key material.
        tracing::warn!(
          endpoint_id = %self.verifier.endpoint_id,
          error = %error,
          "ssh host key verification failed"
        );
        // Returning Ok(false) lets russh reject the connection cleanly
        // instead of propagating an error that could look like a transport
        // fault. The `connect()` caller then sees `ConnectionFailed`
        // ("Disconnected"), so host-key mismatch tests accept that error
        // as well as `HostKeyMismatch`.
        Ok(false)
      }
    }
  }
}

// ---------------------------------------------------------------------------
// Authentication
// ---------------------------------------------------------------------------

/// Reserved [`SshAuthentication`] key reference for the mobile device key
/// that `core::remote_device_key` keeps in the OS keystore. It resolves only
/// through a pool built with [`SshConnectionPool::with_device_key_provider`],
/// which only the mobile app does; desktop fails with a clear error instead
/// of treating it as a file name under `~/.ssh`.
pub const DEVICE_KEYSTORE_KEY_REFERENCE: &str = "keystore:device";

/// Loads the device private key on demand for
/// [`DEVICE_KEYSTORE_KEY_REFERENCE`]. Called once per new connection; the
/// key is dropped as soon as authentication finishes.
pub type DeviceKeyProvider = Arc<
  dyn Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<PrivateKey, String>> + Send>>
    + Send
    + Sync,
>;

/// Loads client key material for an [`SshEndpoint`]'s [`SshAuthentication`].
///
/// Key material never lives in Supabase or any control-plane record (PRD:
/// "Treq never generates a user private key" / "Supabase stores public keys
/// ... only"). The `key_reference` string names a private key already on the
/// user's device. These forms are accepted:
///
/// - an absolute or `~`-relative path to an OpenSSH private key file, or to
///   its `.pub` / `-cert.pub` file (the desktop identity picker hands out the
///   `.pub` path, see `remote_local_keys`), in which case the private key is
///   the same path with that suffix removed;
/// - a bare filename, resolved under `~/.ssh/`;
/// - [`DEVICE_KEYSTORE_KEY_REFERENCE`], resolved through the pool's
///   [`DeviceKeyProvider`] (mobile only).
///
/// For certificate authentication, the certificate text normally arrives
/// inline in [`SshAuthentication::Certificate`]. Without it, the transport
/// reads `<key path>-cert.pub`, the file `ssh-keygen -s` writes. Either way
/// the certificate must certify the same public key as the private key, so
/// a mismatch is reported locally instead of as an opaque server rejection.
pub struct ClientAuthenticator;

impl ClientAuthenticator {
  fn resolve_private_key_path(key_reference: &str) -> PathBuf {
    let reference = key_reference
      .strip_suffix("-cert.pub")
      .or_else(|| key_reference.strip_suffix(".pub"))
      .unwrap_or(key_reference);
    let expanded = if let Some(rest) = reference.strip_prefix("~/") {
      dirs_home().join(rest)
    } else {
      PathBuf::from(reference)
    };
    if expanded.is_absolute() || expanded.components().count() > 1 {
      expanded
    } else {
      dirs_home().join(".ssh").join(expanded)
    }
  }

  async fn load_private_key(
    key_reference: &str,
    device_key: Option<&DeviceKeyProvider>,
  ) -> Result<PrivateKey, SshTransportError> {
    if key_reference == DEVICE_KEYSTORE_KEY_REFERENCE {
      let Some(provider) = device_key else {
        return Err(SshTransportError::KeyMaterialUnavailable(
          "the device keystore key is only available in the mobile app; \
           select a local SSH key on desktop"
            .to_string(),
        ));
      };
      return provider().await.map_err(|error| {
        SshTransportError::KeyMaterialUnavailable(format!(
          "failed to load the device key from the keystore: {error}"
        ))
      });
    }
    let path = Self::resolve_private_key_path(key_reference);
    russh::keys::load_secret_key(&path, None).map_err(|error| {
      // Never log the path's contents; the path itself is not secret.
      SshTransportError::KeyMaterialUnavailable(format!(
        "failed to load private key from {}: {error}",
        path.display()
      ))
    })
  }

  /// Returns the certificate to present with `key`: the inline text when
  /// the endpoint carries one, otherwise the `-cert.pub` file next to a
  /// file-based key. Rejects a certificate issued for a different key.
  fn load_certificate(
    key_reference: &str,
    inline_certificate: Option<&str>,
    key: &PrivateKey,
  ) -> Result<russh::keys::Certificate, SshTransportError> {
    let certificate = match inline_certificate {
      Some(text) => russh::keys::Certificate::from_openssh(text.trim()).map_err(|error| {
        SshTransportError::KeyMaterialUnavailable(format!(
          "failed to parse the issued certificate: {error}"
        ))
      })?,
      None if key_reference == DEVICE_KEYSTORE_KEY_REFERENCE => {
        return Err(SshTransportError::KeyMaterialUnavailable(
          "the device keystore key has no certificate file; the endpoint must carry the \
           issued certificate inline"
            .to_string(),
        ));
      }
      None => {
        let key_path = Self::resolve_private_key_path(key_reference);
        let cert_path = PathBuf::from(format!("{}-cert.pub", key_path.display()));
        russh::keys::load_openssh_certificate(&cert_path).map_err(|error| {
          SshTransportError::KeyMaterialUnavailable(format!(
            "failed to load certificate from {}: {error}",
            cert_path.display()
          ))
        })?
      }
    };
    if certificate.public_key() != key.public_key().key_data() {
      return Err(SshTransportError::KeyMaterialUnavailable(
        "the certificate was issued for a different key than the selected identity".to_string(),
      ));
    }
    Ok(certificate)
  }

  /// Authenticates `handle` per `authentication`, using `username` from the
  /// endpoint. Returns an error rather than silently downgrading to a
  /// weaker method on failure.
  async fn authenticate(
    handle: &mut Handle<TreqSshClientHandler>,
    username: &str,
    authentication: &SshAuthentication,
    device_key: Option<&DeviceKeyProvider>,
  ) -> Result<(), SshTransportError> {
    let result = match authentication {
      SshAuthentication::PublicKey { key_reference } => {
        let key = Arc::new(Self::load_private_key(key_reference, device_key).await?);
        let hash_alg = handle
          .best_supported_rsa_hash()
          .await
          .ok()
          .flatten()
          .flatten();
        handle
          .authenticate_publickey(username, PrivateKeyWithHashAlg::new(key, hash_alg))
          .await
          .map_err(|error| SshTransportError::AuthenticationFailed(error.to_string()))?
      }
      SshAuthentication::Certificate {
        key_reference,
        certificate,
      } => {
        let key = Self::load_private_key(key_reference, device_key).await?;
        let cert = Self::load_certificate(key_reference, certificate.as_deref(), &key)?;
        let key = Arc::new(key);
        handle
          .authenticate_openssh_cert(username, key, cert)
          .await
          .map_err(|error| SshTransportError::AuthenticationFailed(error.to_string()))?
      }
    };
    match result {
      AuthResult::Success => Ok(()),
      AuthResult::Failure { .. } => Err(SshTransportError::AuthenticationFailed(
        "server rejected the presented key or certificate".to_string(),
      )),
    }
  }
}

fn dirs_home() -> PathBuf {
  std::env::var("HOME")
    .map(PathBuf::from)
    .unwrap_or_else(|_| PathBuf::from("."))
}

// ---------------------------------------------------------------------------
// Connection pooling
// ---------------------------------------------------------------------------

/// Pool key derivation. Per the PRD: "Repository identity references the
/// endpoint ID ... generation", so the pool is keyed on more than a hostname
/// — endpoint id, generation, and the connection parameters that would
/// actually require a new TCP/SSH session if they changed. Two endpoints
/// that happen to share a hostname (e.g. after a reprovision that reused an
/// address) must never share a pooled connection across a generation
/// boundary, and a host-key rotation must never silently keep an old,
/// now-untrusted connection alive.
///
/// Authentication is deliberately not part of the key. A renewed
/// certificate changes `SshAuthentication::Certificate::certificate` but must
/// not tear down open exec/PTY channels (PRD "Silent renewal while the
/// session is active"), so the live connection is reused and the renewed
/// certificate is presented the next time this key has to reconnect.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PoolKey {
  endpoint_id: String,
  generation: u64,
  hostname: String,
  port: u16,
  username: String,
  host_key_fingerprints: Vec<String>,
  /// Relay URL for relayed endpoints; `None` for direct TCP.
  relay_url: Option<String>,
  /// Whether this is a Treq-managed instance, which sign-out cuts off.
  managed: bool,
}

impl PoolKey {
  pub fn for_endpoint(endpoint: &SshEndpoint) -> Self {
    let generation = match &endpoint.source {
      crate::core::remote_control_plane::SshEndpointSource::Managed { generation, .. } => {
        *generation
      }
      _ => 0,
    };
    let mut host_key_fingerprints: Vec<String> = endpoint
      .host_keys
      .iter()
      .map(|key| key.fingerprint_sha256.clone())
      .collect();
    host_key_fingerprints.sort();
    Self {
      endpoint_id: endpoint.id.clone(),
      generation,
      hostname: endpoint.hostname.clone(),
      port: endpoint.port,
      username: endpoint.username.clone(),
      host_key_fingerprints,
      relay_url: match &endpoint.transport {
        SshTransport::Direct => None,
        SshTransport::Relay { url } => Some(url.clone()),
      },
      managed: is_managed(endpoint),
    }
  }
}

fn is_managed(endpoint: &SshEndpoint) -> bool {
  matches!(
    endpoint.source,
    crate::core::remote_control_plane::SshEndpointSource::Managed { .. }
  )
}

struct PooledConnection {
  handle: Arc<Handle<TreqSshClientHandler>>,
  last_used: Instant,
  alive: Arc<AtomicBool>,
}

/// Pooled, multiplexed SSH connection manager. One authenticated connection
/// per [`PoolKey`] is reused for multiple exec and PTY channels rather than
/// reconnecting per command (PRD: "the native SSH transport reuses a
/// connection for multiple structured commands").
static R6_GATE: AsyncMutex<()> = AsyncMutex::const_new(());

pub struct SshConnectionPool {
  connections: AsyncMutex<HashMap<PoolKey, PooledConnection>>,
  /// One lock per key, held while that key's connection opens, so concurrent
  /// callers share one connection instead of each dialing (and leaking) one.
  connecting: AsyncMutex<HashMap<PoolKey, Arc<AsyncMutex<()>>>>,
  keepalive_interval: Duration,
  /// Bounds the dial (TCP or relay plus key exchange) and, separately,
  /// authentication, so an unresponsive host cannot hang a caller.
  connect_timeout: Duration,
  idle_timeout: Duration,
  pub metrics: Arc<SshTransportMetrics>,
  /// Endpoint IDs currently under hard cutoff (PRD "Hard cutoff on
  /// revocation or expiry"), keyed by `SshEndpoint::id`. Checked before
  /// opening or reusing any connection for that endpoint.
  cutoffs: AsyncMutex<HashMap<String, CutoffReason>>,
  /// Resolves [`DEVICE_KEYSTORE_KEY_REFERENCE`]. Set only by the mobile app
  /// (see `lib.rs`); `None` on desktop, where that reference is an error.
  device_key_provider: Option<DeviceKeyProvider>,
  /// The signed-in user's Supabase access token, sent to the
  /// `remote-ssh-relay` Edge Function when a relayed connection opens. The
  /// frontend pushes it on sign-in and every token refresh. It is read per
  /// connection, so a refreshed token needs no reconnect, and it is never
  /// logged or included in an error.
  relay_access_token: std::sync::RwLock<Option<String>>,
  /// Set by [`Self::cut_off_managed`] on sign-out: every managed endpoint is
  /// refused, pooled or not, until a signed-in user's token arrives.
  managed_cutoff: std::sync::RwLock<Option<CutoffReason>>,
}

impl Default for SshConnectionPool {
  fn default() -> Self {
    Self::new()
  }
}

impl SshConnectionPool {
  pub fn new() -> Self {
    Self {
      connections: AsyncMutex::new(HashMap::new()),
      connecting: AsyncMutex::new(HashMap::new()),
      keepalive_interval: Duration::from_secs(30),
      connect_timeout: Duration::from_secs(20),
      idle_timeout: Duration::from_secs(600),
      metrics: Arc::new(SshTransportMetrics::default()),
      cutoffs: AsyncMutex::new(HashMap::new()),
      device_key_provider: None,
      relay_access_token: std::sync::RwLock::new(None),
      managed_cutoff: std::sync::RwLock::new(None),
    }
  }

  /// Sets (or clears, on sign-out) the token used to open relayed
  /// connections. See the field doc on `relay_access_token`.
  pub fn set_relay_access_token(&self, token: Option<String>) {
    let token = token.filter(|token| !token.is_empty());
    if token.is_some() {
      *self
        .managed_cutoff
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    }
    match self.relay_access_token.write() {
      Ok(mut guard) => *guard = token,
      Err(poisoned) => *poisoned.into_inner() = token,
    }
  }

  fn relay_access_token(&self) -> Option<String> {
    match self.relay_access_token.read() {
      Ok(guard) => guard.clone(),
      Err(poisoned) => poisoned.into_inner().clone(),
    }
  }

  #[cfg(test)]
  fn with_connect_timeout(mut self, timeout: Duration) -> Self {
    self.connect_timeout = timeout;
    self
  }

  /// Lets endpoints authenticate with [`DEVICE_KEYSTORE_KEY_REFERENCE`] by
  /// loading the device key through `provider` for each new connection.
  pub fn with_device_key_provider(mut self, provider: DeviceKeyProvider) -> Self {
    self.device_key_provider = Some(provider);
    self
  }

  /// Forces the endpoint into the hard-cutoff state and tears down any open
  /// exec/PTY channels for it (PRD "Hard cutoff on revocation or expiry": "open
  /// exec and PTY channels to that instance are torn down"). Disconnecting the
  /// underlying SSH session closes every channel multiplexed on it. Idempotent:
  /// calling this again for an endpoint already cut off just refreshes the
  /// reason and re-checks for (newly) open connections.
  pub async fn force_cutoff(&self, endpoint_id: &str, reason: CutoffReason) {
    {
      let mut cutoffs = self.cutoffs.lock().await;
      cutoffs.insert(endpoint_id.to_string(), reason);
    }
    let stale: Vec<PoolKey> = {
      let connections = self.connections.lock().await;
      connections
        .keys()
        .filter(|key| key.endpoint_id == endpoint_id)
        .cloned()
        .collect()
    };
    for key in stale {
      let removed = { self.connections.lock().await.remove(&key) };
      if let Some(pooled) = removed {
        pooled.alive.store(false, Ordering::SeqCst);
        let handle = pooled.handle.clone();
        let _ = handle
          .disconnect(
            Disconnect::ByApplication,
            "credential revoked or expired",
            "en",
          )
          .await;
      }
    }
    tracing::warn!(endpoint_id = %endpoint_id, reason = %reason, "ssh endpoint credential cut off");
  }

  /// Cuts off every managed endpoint when the user signs out: tears down
  /// each pooled managed connection (see [`Self::force_cutoff`]) and refuses
  /// new managed connections until [`Self::set_relay_access_token`] receives
  /// a token again. Returns the endpoint ids whose connections were torn
  /// down. User-managed endpoints are untouched.
  pub async fn cut_off_managed(&self, reason: CutoffReason) -> Vec<String> {
    *self
      .managed_cutoff
      .write()
      .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(reason);
    let mut endpoint_ids: Vec<String> = {
      let connections = self.connections.lock().await;
      connections
        .keys()
        .filter(|key| key.managed)
        .map(|key| key.endpoint_id.clone())
        .collect()
    };
    endpoint_ids.sort();
    endpoint_ids.dedup();
    for endpoint_id in &endpoint_ids {
      self.force_cutoff(endpoint_id, reason).await;
    }
    endpoint_ids
  }

  /// Clears a previously forced cutoff, e.g. after the user reauthenticates
  /// and a fresh certificate is issued through the normal registration and
  /// issuance flow. Does not itself reconnect; the next call reconnects with
  /// the (now fresh) endpoint credential.
  pub async fn clear_cutoff(&self, endpoint_id: &str) {
    self.cutoffs.lock().await.remove(endpoint_id);
  }

  /// Returns why `endpoint_id` is currently cut off, if it is.
  pub async fn cutoff_reason(&self, endpoint_id: &str) -> Option<CutoffReason> {
    self.cutoffs.lock().await.get(endpoint_id).copied()
  }

  /// Snapshot of this pool's transport telemetry (PRD "Client and transport
  /// telemetry"). Cheap to call; safe to poll from a Tauri command.
  pub fn metrics_snapshot(&self) -> SshTransportMetricsSnapshot {
    self.metrics.snapshot()
  }

  /// Returns a live authenticated connection for `endpoint`, reusing a
  /// pooled one when present and not marked dead. A dead or missing
  /// connection is transparently reconnected here — this is safe because
  /// establishing a connection is not a mutation with side effects on the
  /// remote system, unlike retrying an in-flight command (see
  /// `exec_command`'s cancellation handling, which never auto-retries).
  async fn get_or_connect(
    &self,
    endpoint: &SshEndpoint,
  ) -> Result<Arc<Handle<TreqSshClientHandler>>, SshTransportError> {
    let managed_cutoff = if is_managed(endpoint) {
      *self
        .managed_cutoff
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
    } else {
      None
    };
    if let Some(reason) = managed_cutoff.or(self.cutoff_reason(&endpoint.id).await) {
      return Err(SshTransportError::CredentialCutOff {
        endpoint_id: endpoint.id.clone(),
        reason,
      });
    }
    let key = PoolKey::for_endpoint(endpoint);
    if let Some(handle) = self.reuse(&key).await {
      return Ok(handle);
    }
    let key_lock = self
      .connecting
      .lock()
      .await
      .entry(key.clone())
      .or_default()
      .clone();
    let _connecting = key_lock.lock().await;
    // Another caller may have connected while this one waited.
    if let Some(handle) = self.reuse(&key).await {
      return Ok(handle);
    }
    let is_reconnect = {
      let mut connections = self.connections.lock().await;
      let dead = connections.remove(&key).is_some();
      if dead {
        tracing::info!(endpoint_id = %endpoint.id, "pooled ssh connection is dead, reconnecting");
      }
      dead
    };
    if is_reconnect {
      self
        .metrics
        .reconnect_attempts
        .fetch_add(1, Ordering::Relaxed);
    }

    let handle = self.connect(endpoint).await?;
    let alive = Arc::new(AtomicBool::new(true));
    let mut connections = self.connections.lock().await;
    connections.insert(
      key,
      PooledConnection {
        handle: handle.clone(),
        last_used: Instant::now(),
        alive,
      },
    );
    drop(connections);
    Ok(handle)
  }

  /// The pooled connection for `key`, when it is still alive.
  async fn reuse(&self, key: &PoolKey) -> Option<Arc<Handle<TreqSshClientHandler>>> {
    let mut connections = self.connections.lock().await;
    let existing = connections.get_mut(key)?;
    if !existing.alive.load(Ordering::SeqCst) {
      return None;
    }
    existing.last_used = Instant::now();
    let handle = existing.handle.clone();
    drop(connections);
    self
      .metrics
      .pooled_connection_reuse_count
      .fetch_add(1, Ordering::Relaxed);
    Some(handle)
  }

  async fn connect(
    &self,
    endpoint: &SshEndpoint,
  ) -> Result<Arc<Handle<TreqSshClientHandler>>, SshTransportError> {
    let verifier = HostKeyVerifier::new(endpoint.id.clone(), &endpoint.host_keys);
    let handler = TreqSshClientHandler {
      verifier,
      metrics: self.metrics.clone(),
    };

    let config = russh::client::Config {
      keepalive_interval: Some(self.keepalive_interval),
      keepalive_max: 3,
      nodelay: std::env::var_os("R6_NODELAY").is_some(),
      ..Default::default()
    };
    let config = Arc::new(config);

    let dial_started = Instant::now();
    let dial = async {
      match &endpoint.transport {
      SshTransport::Direct => {
        let address = (endpoint.hostname.as_str(), endpoint.port);
        tracing::debug!(endpoint_id = %endpoint.id, hostname = %endpoint.hostname, port = endpoint.port, "opening ssh connection");
        russh::client::connect(config, address, handler).await
      }
      SshTransport::Relay { url } => {
        // The URL holds only ids, so it is safe to log; the token is not.
        tracing::debug!(endpoint_id = %endpoint.id, relay_url = %url, "opening relayed ssh connection");
        let token = self.relay_access_token().ok_or_else(|| {
          SshTransportError::ConnectionFailed(
            "sign in to Treq to reach the managed VM".to_string(),
          )
        })?;
        let stream = crate::core::remote_ssh_ws_stream::connect_relay(url, &token)
          .await
          .map_err(SshTransportError::ConnectionFailed)?;
        russh::client::connect_stream(config, stream, handler).await
      }
      }
      .map_err(|error| SshTransportError::ConnectionFailed(error.to_string()))
    };
    let mut handle = tokio::time::timeout(self.connect_timeout, dial)
      .await
      .map_err(|_| {
        SshTransportError::ConnectionFailed(format!(
          "timed out after {}s connecting",
          self.connect_timeout.as_secs()
        ))
      })??;
    // "DNS and TCP connection duration": the interval up to a successfully
    // opened (pre-auth) transport, since `russh::client::connect` does DNS
    // resolution and the TCP dial internally and does not expose a
    // narrower hook between the two.
    self.metrics.dns_tcp_connect.record(dial_started.elapsed());

    let auth_started = Instant::now();
    tokio::time::timeout(
      self.connect_timeout,
      ClientAuthenticator::authenticate(
        &mut handle,
        &endpoint.username,
        &endpoint.authentication,
        self.device_key_provider.as_ref(),
      ),
    )
    .await
    .map_err(|_| {
      SshTransportError::AuthenticationFailed(format!(
        "timed out after {}s authenticating",
        self.connect_timeout.as_secs()
      ))
    })??;
    // "SSH negotiation and authentication duration": key exchange already
    // happened inside `connect()` above, but the negotiated session is not
    // usable until authentication completes, so this is the duration a
    // caller actually waits on before the connection is usable.
    self
      .metrics
      .ssh_negotiation_and_auth
      .record(auth_started.elapsed());

    tracing::info!(endpoint_id = %endpoint.id, "ssh connection established and authenticated");
    Ok(Arc::new(handle))
  }

  /// Marks the pooled connection for `endpoint` dead, e.g. after an I/O
  /// error observed mid-operation. The next `get_or_connect` call for the
  /// same key reconnects rather than reusing a stale session.
  pub async fn mark_dead(&self, endpoint: &SshEndpoint) {
    let key = PoolKey::for_endpoint(endpoint);
    let connections = self.connections.lock().await;
    if let Some(pooled) = connections.get(&key) {
      pooled.alive.store(false, Ordering::SeqCst);
    }
  }

  /// Drops idle pooled connections that have exceeded the idle timeout,
  /// closing them cleanly. Intended to be driven by a periodic background
  /// task; exposed here as a plain method so callers control scheduling.
  pub async fn sweep_idle(&self) {
    let now = Instant::now();
    // Take the stale connections out under the lock, then disconnect without
    // it: a slow disconnect must not block every other pool user.
    let stale: Vec<PooledConnection> = {
      let mut connections = self.connections.lock().await;
      let keys: Vec<PoolKey> = connections
        .iter()
        .filter(|(_, pooled)| now.duration_since(pooled.last_used) > self.idle_timeout)
        .map(|(key, _)| key.clone())
        .collect();
      keys
        .iter()
        .filter_map(|key| connections.remove(key))
        .collect()
    };
    for pooled in stale {
      let handle = pooled.handle.clone();
      let _ = handle
        .disconnect(Disconnect::ByApplication, "idle timeout", "en")
        .await;
    }
  }

  pub async fn pooled_connection_count(&self) -> usize {
    self.connections.lock().await.len()
  }
}

// ---------------------------------------------------------------------------
// Exec channels
// ---------------------------------------------------------------------------

/// Result of a completed exec channel invocation. stdout and stderr are kept
/// separate per the PRD's structured command protocol ("stdout contains only
/// the result JSON; stderr contains diagnostics").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecOutput {
  pub stdout: Vec<u8>,
  pub stderr: Vec<u8>,
  pub exit_status: Option<u32>,
}

impl ExecOutput {
  pub fn success(&self) -> bool {
    matches!(self.exit_status, Some(0))
  }
}

/// A cooperative cancellation flag for in-flight exec/PTY operations. A
/// caller drops or flips this to cancel; the transport never itself decides
/// to retry a mutation that isn't provably idempotent (PRD: "mutations
/// accept idempotency keys where retry could duplicate work") — cancellation
/// always surfaces as [`SshTransportError::Cancelled`], never a silent retry.
#[derive(Clone, Default)]
pub struct CancellationToken {
  cancelled: Arc<AtomicBool>,
  notify: Arc<Notify>,
}

impl CancellationToken {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn cancel(&self) {
    self.cancelled.store(true, Ordering::SeqCst);
    self.notify.notify_waiters();
  }

  pub fn is_cancelled(&self) -> bool {
    self.cancelled.load(Ordering::SeqCst)
  }

  async fn cancelled(&self) {
    if self.is_cancelled() {
      return;
    }
    self.notify.notified().await;
  }
}

/// Bounds applied to every exec channel invocation.
#[derive(Debug, Clone, Copy)]
pub struct ExecLimits {
  pub deadline: Duration,
  /// Maximum combined stdout+stderr bytes before the channel is aborted.
  pub max_output_bytes: usize,
}

impl Default for ExecLimits {
  fn default() -> Self {
    Self {
      deadline: Duration::from_secs(30),
      max_output_bytes: 8 * 1024 * 1024,
    }
  }
}

/// Runs `treq <command> --format=json` as a non-interactive exec channel on
/// the pooled connection for `endpoint`. No frontend-provided arbitrary
/// command ever reaches this method directly — callers must build `args`
/// through a typed request (see `crate::core::remote::TreqCommandRequest`),
/// never by interpolating raw UI text.
pub async fn exec_command(
  pool: &SshConnectionPool,
  endpoint: &SshEndpoint,
  args: &[String],
  limits: ExecLimits,
  cancellation: &CancellationToken,
) -> Result<ExecOutput, SshTransportError> {
  let started = Instant::now();
  if cancellation.is_cancelled() {
    pool.metrics.exec_channel_duration.record(started.elapsed());
    pool.metrics.record_exec_exit(ExecExitCategory::Cancelled);
    return Err(SshTransportError::Cancelled);
  }

  let handle = pool.get_or_connect(endpoint).await?;

  let run = async {
    // R6 experiment: R6_SERIAL=1 restores the old behaviour (channel opens
    // serialized behind the handle's mutex).
    let _gate = if std::env::var_os("R6_SERIAL").is_some() {
      Some(R6_GATE.lock().await)
    } else {
      None
    };
    let mut channel = handle
      .channel_open_session()
      .await
      .map_err(|error| SshTransportError::ChannelError(error.to_string()))?;
    drop(_gate);

    let command = build_remote_command_line(args);
    // The command line is assembled from separate typed arguments (see
    // `TreqCommandRequest::cli_args`) and passed as a single exec string per
    // the SSH exec protocol; no frontend text is interpolated as shell here
    // beyond the same argument-vector quoting the CLI already validates.
    channel
      .exec(true, command)
      .await
      .map_err(|error| SshTransportError::ChannelError(error.to_string()))?;

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut exit_status = None;

    loop {
      let Some(message) = channel.wait().await else {
        break;
      };
      match message {
        ChannelMsg::Data { data } => {
          stdout.extend_from_slice(&data);
        }
        ChannelMsg::ExtendedData { data, .. } => {
          stderr.extend_from_slice(&data);
        }
        ChannelMsg::ExitStatus {
          exit_status: status,
        } => {
          exit_status = Some(status);
        }
        // `Eof` only signals the stdout/stderr streams are done; a real
        // server (unlike the in-process mock this file's unit tests use)
        // commonly sends it *before* the `ExitStatus` channel request, not
        // after. Breaking here discarded that still-incoming exit status,
        // intermittently reporting `exit_status: None` for a command that
        // exited normally - caught by remote_ssh_server_it.rs's real-sshd
        // tests, not by the mock server below. `Close` is the one message
        // guaranteed to be last, so only it ends the loop.
        ChannelMsg::Eof => {}
        ChannelMsg::Close => {
          break;
        }
        _ => {}
      }
      if stdout.len() + stderr.len() > limits.max_output_bytes {
        return Err(SshTransportError::OutputLimitExceeded);
      }
    }

    Ok(ExecOutput {
      stdout,
      stderr,
      exit_status,
    })
  };

  let outcome = tokio::select! {
    biased;
    _ = cancellation.cancelled() => Err(SshTransportError::Cancelled),
    result = tokio::time::timeout(limits.deadline, run) => match result {
      Ok(inner) => inner,
      Err(_) => Err(SshTransportError::DeadlineExceeded),
    },
  };

  // A deadline blows past the connection's usability the same way a raw
  // I/O error does: the client cannot tell whether the still-open channel
  // will ever produce the rest of the response, so the next command must
  // not gamble on multiplexing a fresh channel onto it. Reconnecting is
  // exactly the reconnect-before-verify step the PRD's "Retrying after
  // network loss" section describes.
  if let Err(SshTransportError::ChannelError(_) | SshTransportError::DeadlineExceeded) = &outcome {
    pool.mark_dead(endpoint).await
  }

  pool.metrics.exec_channel_duration.record(started.elapsed());
  pool.metrics.record_exec_exit(match &outcome {
    Ok(output) if output.success() => ExecExitCategory::Success,
    Ok(_) => ExecExitCategory::CommandFailed,
    Err(SshTransportError::DeadlineExceeded) => ExecExitCategory::Timeout,
    Err(SshTransportError::Cancelled) => ExecExitCategory::Cancelled,
    Err(SshTransportError::OutputLimitExceeded) => ExecExitCategory::OutputLimitExceeded,
    Err(_) => ExecExitCategory::TransportError,
  });

  let output = outcome?;
  if !output.success() {
    return Err(SshTransportError::CommandFailed {
      exit_status: output.exit_status,
      stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
      stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
    });
  }
  Ok(output)
}

/// Builds the exec command line from an argument vector using the same
/// single-quote escaping `core::remote::shell_quote` uses, so a remote shell
/// that wraps `treq` (or plain `exec`) still sees each argument intact.
fn build_remote_command_line(args: &[String]) -> String {
  let mut parts = vec!["treq".to_string()];
  parts.extend(args.iter().map(|arg| crate::core::remote::shell_quote(arg)));
  parts.join(" ")
}

// ---------------------------------------------------------------------------
// PTY channels
// ---------------------------------------------------------------------------

/// An open interactive PTY channel (shell or agent process) on the pooled
/// connection for one endpoint. Read/write/resize/close mirror the shape of
/// [`crate::pty::PtySession`] so a later (Phase 6) consumer that already
/// knows the local PTY API is not learning a second vocabulary, even though
/// the two are separate transports.
pub struct RemotePtyChannel {
  // Split into read/write halves rather than one `AsyncMutex<Channel>`: the
  // read half's `wait()` needs `&mut self` and blocks for as long as no new
  // message has arrived, so a single shared lock around the whole channel
  // would let a concurrent read loop hold that lock indefinitely and
  // deadlock any caller of `write`/`resize`/`close` that runs while a read
  // is in flight — exactly the pattern a streaming consumer (read output in
  // a background task while writes/closes happen from elsewhere) needs.
  // `ChannelWriteHalf`'s operations only need `&self` (they just enqueue a
  // message), so they never contend with the read half's mutex at all.
  read_half: AsyncMutex<russh::ChannelReadHalf>,
  write_half: russh::ChannelWriteHalf<russh::client::Msg>,
  metrics: Arc<SshTransportMetrics>,
}

impl RemotePtyChannel {
  /// Opens a PTY channel and requests a shell (or, when `command` is set, an
  /// exec'd interactive process such as an agent) inside it.
  pub async fn open(
    pool: &SshConnectionPool,
    endpoint: &SshEndpoint,
    term: &str,
    cols: u16,
    rows: u16,
    command: Option<&str>,
  ) -> Result<Self, SshTransportError> {
    Self::open_in_directory(pool, endpoint, term, cols, rows, command, None).await
  }

  /// Same as [`open`], but starts the remote process in `working_directory`
  /// when set (PRD: a terminal is bound to `remote_working_directory`).
  /// OpenSSH has no working-directory channel request, so this execs a
  /// quoted `cd` followed by the shell or command.
  pub async fn open_in_directory(
    pool: &SshConnectionPool,
    endpoint: &SshEndpoint,
    term: &str,
    cols: u16,
    rows: u16,
    command: Option<&str>,
    working_directory: Option<&str>,
  ) -> Result<Self, SshTransportError> {
    let handle = pool.get_or_connect(endpoint).await?;
    let session = handle;
    let channel = session
      .channel_open_session()
      .await
      .map_err(|error| SshTransportError::ChannelError(error.to_string()))?;
    channel
      .request_pty(true, term, cols as u32, rows as u32, 0, 0, &[])
      .await
      .map_err(|error| SshTransportError::ChannelError(error.to_string()))?;
    match pty_startup_command(command, working_directory) {
      Some(command) => channel
        .exec(true, command.as_bytes())
        .await
        .map_err(|error| SshTransportError::ChannelError(error.to_string()))?,
      None => channel
        .request_shell(true)
        .await
        .map_err(|error| SshTransportError::ChannelError(error.to_string()))?,
    }
    pool.metrics.pty_start_count.fetch_add(1, Ordering::Relaxed);
    let (read_half, write_half) = channel.split();
    Ok(Self {
      read_half: AsyncMutex::new(read_half),
      write_half,
      metrics: pool.metrics.clone(),
    })
  }

  /// Writes raw bytes to the remote PTY (keystrokes, pasted input, etc.).
  /// Never logs `data`, per the PRD's "raw terminal output ... by default"
  /// never-log requirement. Never contends with an in-flight `read_chunk`.
  pub async fn write(&self, data: &[u8]) -> Result<(), SshTransportError> {
    let mut writer = self.write_half.make_writer();
    writer
      .write_all(data)
      .await
      .map_err(|error| SshTransportError::ChannelError(error.to_string()))?;
    Ok(())
  }

  /// Reads the next chunk of output, or `None` once the channel has closed.
  /// Callers own their own output buffering and redaction policy for what
  /// they do with the bytes (e.g. terminal echo vs. a log line).
  pub async fn read_chunk(&self) -> Result<Option<Vec<u8>>, SshTransportError> {
    let mut read_half = self.read_half.lock().await;
    loop {
      match read_half.wait().await {
        Some(ChannelMsg::Data { data }) => return Ok(Some(data.to_vec())),
        Some(ChannelMsg::ExtendedData { data, .. }) => return Ok(Some(data.to_vec())),
        Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => return Ok(None),
        Some(_) => continue,
      }
    }
  }

  /// Reads the next chunk of output along with an exit status, if the
  /// message that ended the wait was `ExitStatus`. Distinct from
  /// `read_chunk` only in that it surfaces `ExitStatus` instead of treating
  /// it as an ignorable message, so a caller that wants to observe a remote
  /// process's exit code does not have to guess it from the channel closing.
  pub async fn read_event(&self) -> Result<PtyReadEvent, SshTransportError> {
    let mut read_half = self.read_half.lock().await;
    loop {
      match read_half.wait().await {
        Some(ChannelMsg::Data { data }) => return Ok(PtyReadEvent::Data(data.to_vec())),
        Some(ChannelMsg::ExtendedData { data, .. }) => {
          return Ok(PtyReadEvent::Data(data.to_vec()))
        }
        Some(ChannelMsg::ExitStatus { exit_status }) => {
          return Ok(PtyReadEvent::ExitStatus(exit_status))
        }
        // Servers send `Eof` before the `ExitStatus` request (see
        // `exec_command`), so only `Close` ends the channel.
        Some(ChannelMsg::Eof) => continue,
        Some(ChannelMsg::Close) | None => return Ok(PtyReadEvent::Closed),
        Some(_) => continue,
      }
    }
  }

  pub async fn resize(&self, cols: u16, rows: u16) -> Result<(), SshTransportError> {
    self
      .write_half
      .window_change(cols as u32, rows as u32, 0, 0)
      .await
      .map_err(|error| SshTransportError::ChannelError(error.to_string()))
  }

  pub async fn close(&self) -> Result<(), SshTransportError> {
    let result = self
      .write_half
      .close()
      .await
      .map_err(|error| SshTransportError::ChannelError(error.to_string()));
    self.metrics.pty_exit_count.fetch_add(1, Ordering::Relaxed);
    result
  }
}

/// Builds the remote exec string for a PTY. `None` means a shell request
/// (no working directory, no explicit command). The directory is quoted
/// with [`crate::core::remote::shell_quote`]; `command` is caller-owned.
fn pty_startup_command(command: Option<&str>, working_directory: Option<&str>) -> Option<String> {
  match (command, working_directory) {
    (None, None) => None,
    (Some(command), None) => Some(command.to_string()),
    (None, Some(dir)) => Some(format!(
      "cd {} && exec ${{SHELL:-/bin/sh}} -l",
      crate::core::remote::shell_quote(dir)
    )),
    (Some(command), Some(dir)) => Some(format!(
      "cd {} && {}",
      crate::core::remote::shell_quote(dir),
      command
    )),
  }
}

/// One event yielded by [`RemotePtyChannel::read_event`]: either an output
/// chunk, an observed exit status, or the channel ending without one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PtyReadEvent {
  Data(Vec<u8>),
  ExitStatus(u32),
  Closed,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::remote_control_plane::{SshEndpointSource, TrustedHostKey};
  use russh::keys::PrivateKey;
  use russh::server::{self, Msg as ServerMsg, Server as _, Session as ServerSession};
  use russh::{Channel, ChannelId};
  use std::sync::atomic::AtomicUsize;

  fn test_host_key() -> PrivateKey {
    use getrandom::SysRng;
    use rand_core::UnwrapErr;
    PrivateKey::random(&mut UnwrapErr(SysRng), russh::keys::Algorithm::Ed25519).unwrap()
  }

  fn trusted_host_key(key: &PrivateKey) -> TrustedHostKey {
    let fingerprint = key
      .public_key()
      .fingerprint(russh::keys::HashAlg::Sha256)
      .to_string();
    TrustedHostKey {
      algorithm: "ssh-ed25519".to_string(),
      fingerprint_sha256: fingerprint,
      comment: None,
    }
  }

  // -- Host-key verification -------------------------------------------------

  #[test]
  fn host_key_verifier_accepts_matching_fingerprint() {
    let host_key = test_host_key();
    let trusted = trusted_host_key(&host_key);
    let verifier = HostKeyVerifier::new("endpoint-1", std::slice::from_ref(&trusted));

    let presented = russh::keys::PublicKeyOrCertificate::PublicKey {
      key: host_key.public_key().clone(),
      hash_alg: None,
    };

    assert!(verifier.verify(&presented).is_ok());
  }

  #[test]
  fn host_key_verifier_rejects_mismatched_fingerprint() {
    let trusted_key = test_host_key();
    let presented_key = test_host_key();
    let trusted = trusted_host_key(&trusted_key);
    let verifier = HostKeyVerifier::new("endpoint-1", std::slice::from_ref(&trusted));

    let presented = russh::keys::PublicKeyOrCertificate::PublicKey {
      key: presented_key.public_key().clone(),
      hash_alg: None,
    };

    let error = verifier.verify(&presented).unwrap_err();
    assert!(matches!(error, SshTransportError::HostKeyMismatch { .. }));
  }

  #[test]
  fn host_key_verifier_rejects_when_no_trusted_keys_are_recorded() {
    let presented_key = test_host_key();
    let verifier = HostKeyVerifier::new("endpoint-1", &[]);

    let presented = russh::keys::PublicKeyOrCertificate::PublicKey {
      key: presented_key.public_key().clone(),
      hash_alg: None,
    };

    // No bypass flag exists: an endpoint with zero trusted keys rejects
    // every presented key rather than accepting on first use.
    assert!(verifier.verify(&presented).is_err());
  }

  // -- Pool key derivation ----------------------------------------------------

  fn sample_endpoint(generation: u64, hostname: &str, fingerprint: &str) -> SshEndpoint {
    SshEndpoint {
      id: "endpoint-1".to_string(),
      instance_id: Some("instance-1".to_string()),
      source: SshEndpointSource::Managed {
        provider: "fly_sprites".to_string(),
        generation,
      },
      hostname: hostname.to_string(),
      port: 22,
      username: "treq".to_string(),
      host_keys: vec![TrustedHostKey {
        algorithm: "ssh-ed25519".to_string(),
        fingerprint_sha256: fingerprint.to_string(),
        comment: None,
      }],
      authentication: SshAuthentication::PublicKey {
        key_reference: "id_ed25519".to_string(),
      },
      transport: Default::default(),
    }
  }

  #[test]
  fn pool_key_differs_across_generations_for_same_hostname() {
    let gen1 = sample_endpoint(1, "10.0.0.5", "SHA256:aaa");
    let gen2 = sample_endpoint(2, "10.0.0.5", "SHA256:bbb");
    assert_ne!(PoolKey::for_endpoint(&gen1), PoolKey::for_endpoint(&gen2));
  }

  #[test]
  fn pool_key_is_stable_for_identical_endpoints() {
    let a = sample_endpoint(1, "10.0.0.5", "SHA256:aaa");
    let b = sample_endpoint(1, "10.0.0.5", "SHA256:aaa");
    assert_eq!(PoolKey::for_endpoint(&a), PoolKey::for_endpoint(&b));
  }

  #[test]
  fn pool_key_ignores_host_key_fingerprint_ordering() {
    let mut a = sample_endpoint(1, "10.0.0.5", "SHA256:aaa");
    a.host_keys.push(TrustedHostKey {
      algorithm: "ssh-rsa".to_string(),
      fingerprint_sha256: "SHA256:zzz".to_string(),
      comment: None,
    });
    let mut b = a.clone();
    b.host_keys.reverse();
    assert_eq!(PoolKey::for_endpoint(&a), PoolKey::for_endpoint(&b));
  }

  // -- Cancellation -------------------------------------------------------------

  #[tokio::test]
  async fn cancellation_token_reports_cancelled_immediately_when_pre_cancelled() {
    let token = CancellationToken::new();
    token.cancel();
    assert!(token.is_cancelled());
    // `cancelled()` must resolve immediately rather than hang, even though
    // `cancel()` happened before anyone called `cancelled()`.
    tokio::time::timeout(Duration::from_millis(50), token.cancelled())
      .await
      .expect("cancelled() should resolve without waiting for a fresh notify");
  }

  // -- In-process mock SSH server for exec-channel behavior --------------------

  #[derive(Clone)]
  struct MockServer {
    host_key_algo: &'static str,
    reply: Arc<AtomicUsize>, // 0 = normal echo, 1 = slow (for deadline test), 2 = huge output
    call_count: Arc<AtomicUsize>,
    certificate_key_ids: Arc<std::sync::Mutex<Vec<String>>>,
  }

  impl server::Server for MockServer {
    type Handler = MockHandler;
    fn new_client(&mut self, _: Option<std::net::SocketAddr>) -> MockHandler {
      MockHandler {
        mode: self.reply.clone(),
        call_count: self.call_count.clone(),
        certificate_key_ids: self.certificate_key_ids.clone(),
      }
    }
  }

  struct MockHandler {
    mode: Arc<AtomicUsize>,
    /// Shared across every exec on the pooled connection (mode 4/5 only): a
    /// real client reuses one authenticated connection for the mutation, the
    /// verification read, and any retry, so this must live at the
    /// connection level, not be reset per-channel.
    call_count: Arc<AtomicUsize>,
    /// Key IDs of every certificate a client authenticated with, so tests
    /// can prove the certificate path (not plain publickey) was used.
    certificate_key_ids: Arc<std::sync::Mutex<Vec<String>>>,
  }

  impl server::Handler for MockHandler {
    type Error = russh::Error;

    async fn auth_publickey(
      &mut self,
      _user: &str,
      _key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<server::Auth, Self::Error> {
      Ok(server::Auth::Accept)
    }

    async fn auth_openssh_certificate(
      &mut self,
      _user: &str,
      certificate: &russh::keys::Certificate,
    ) -> Result<server::Auth, Self::Error> {
      self
        .certificate_key_ids
        .lock()
        .unwrap()
        .push(certificate.key_id().to_string());
      Ok(server::Auth::Accept)
    }

    async fn channel_open_session(
      &mut self,
      _channel: Channel<ServerMsg>,
      reply: server::ChannelOpenHandle,
      _session: &mut ServerSession,
    ) -> Result<(), Self::Error> {
      reply.accept().await;
      Ok(())
    }

    async fn pty_request(
      &mut self,
      channel: ChannelId,
      _term: &str,
      _col_width: u32,
      _row_height: u32,
      _pix_width: u32,
      _pix_height: u32,
      _modes: &[(russh::Pty, u32)],
      session: &mut ServerSession,
    ) -> Result<(), Self::Error> {
      session.channel_success(channel)?;
      Ok(())
    }

    async fn window_change_request(
      &mut self,
      channel: ChannelId,
      _col_width: u32,
      _row_height: u32,
      _pix_width: u32,
      _pix_height: u32,
      session: &mut ServerSession,
    ) -> Result<(), Self::Error> {
      session.channel_success(channel)?;
      Ok(())
    }

    async fn exec_request(
      &mut self,
      channel: ChannelId,
      data: &[u8],
      session: &mut ServerSession,
    ) -> Result<(), Self::Error> {
      let command = String::from_utf8_lossy(data).to_string();
      session.channel_success(channel)?;
      match self.mode.load(Ordering::SeqCst) {
        1 => {
          // Slow path: sleep past the client's deadline before ever
          // replying, to exercise deadline enforcement.
          tokio::time::sleep(Duration::from_secs(5)).await;
          session.data(channel, bytes::Bytes::from_static(b"{}"))?;
          session.exit_status_request(channel, 0)?;
        }
        2 => {
          // Oversized path: exceed the client's output limit.
          let chunk = vec![b'x'; 4096];
          for _ in 0..64 {
            session.data(channel, bytes::Bytes::from(chunk.clone()))?;
          }
          session.exit_status_request(channel, 0)?;
        }
        3 => {
          // Structured-failure path: a non-zero exit whose stdout still
          // carries the CLI's own `{"error":{code,message}}` body, the way
          // the real `treq` binary reports a failed command.
          let body = "{\"error\":{\"code\":\"workspace_not_found\",\"message\":\"Workspace 42 was not found\"}}";
          session.data(channel, bytes::Bytes::from(body.as_bytes().to_vec()))?;
          session.exit_status_request(channel, 1)?;
        }
        4 | 5 => {
          // Verify-before-retry path: the *first* exec against this server
          // (the mutation itself, e.g. `workspace create`) sleeps past the
          // client's deadline so the client observes a genuine
          // transport-layer failure - never able to tell whether the
          // mutation reached the VM. `exec_command` treats a deadline the
          // same as a channel I/O error and marks the pooled connection
          // dead, so every later exec (the verification read, and in mode 5
          // the retried mutation) arrives on a *fresh* connection - a new
          // `MockHandler`, but sharing this `MockServer`'s `call_count` - and
          // answers immediately, modeling reconnecting after a real network
          // failure rather than multiplexing a new channel onto the same
          // still-hung connection.
          //
          // Mode 4: verification read reports the mutation's effect is
          // already observable (`AlreadyApplied`).
          // Mode 5: verification read reports it is not yet observable
          // (`NotApplied`), and a subsequent retry of the same mutation
          // succeeds normally.
          let call_index = self.call_count.fetch_add(1, Ordering::SeqCst);
          if call_index == 0 {
            tokio::time::sleep(Duration::from_secs(5)).await;
            session.data(channel, bytes::Bytes::from_static(b"{}"))?;
            session.exit_status_request(channel, 0)?;
          } else if command.contains("workspace") && command.contains("list") {
            let body = if self.mode.load(Ordering::SeqCst) == 4 {
              // AlreadyApplied: the branch this test's mutation asked to
              // create is already present in the read-back state.
              "[{\"id\":1,\"branch_name\":\"feature-retry-test\"}]"
            } else {
              // NotApplied: it is not present yet.
              "[]"
            };
            session.data(channel, bytes::Bytes::from(body.as_bytes().to_vec()))?;
            session.exit_status_request(channel, 0)?;
          } else {
            // The retried mutation itself (mode 5 only) - succeeds.
            let body = "{\"id\":1,\"branch_name\":\"feature-retry-test\"}";
            session.data(channel, bytes::Bytes::from(body.as_bytes().to_vec()))?;
            session.exit_status_request(channel, 0)?;
          }
        }
        6 => {
          // A real sshd's order for a process that exits: output, `Eof`,
          // then the exit status.
          session.data(channel, bytes::Bytes::from_static(b"bye"))?;
          session.eof(channel)?;
          session.exit_status_request(channel, 7)?;
        }
        _ => {
          let response = format!("{{\"echo\":\"{command}\"}}");
          session.data(channel, bytes::Bytes::from(response.into_bytes()))?;
          session.exit_status_request(channel, 0)?;
        }
      }
      session.close(channel)?;
      Ok(())
    }
  }

  async fn start_mock_server(mode: usize) -> (std::net::SocketAddr, PrivateKey) {
    let (addr, host_key, _) = start_mock_server_with_certificate_log(mode).await;
    (addr, host_key)
  }

  async fn start_mock_server_with_certificate_log(
    mode: usize,
  ) -> (
    std::net::SocketAddr,
    PrivateKey,
    Arc<std::sync::Mutex<Vec<String>>>,
  ) {
    let certificate_key_ids = Arc::new(std::sync::Mutex::new(Vec::new()));
    let host_key = test_host_key();
    let mut config = server::Config::default();
    config.keys.push(host_key.clone());
    config.auth_rejection_time = Duration::from_millis(10);
    let config = Arc::new(config);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let mut server = MockServer {
      host_key_algo: "ssh-ed25519",
      reply: Arc::new(AtomicUsize::new(mode)),
      call_count: Arc::new(AtomicUsize::new(0)),
      certificate_key_ids: certificate_key_ids.clone(),
    };
    let _ = server.host_key_algo;

    tokio::spawn(async move {
      loop {
        let Ok((socket, peer)) = listener.accept().await else {
          break;
        };
        let handler = server.new_client(Some(peer));
        let config = config.clone();
        tokio::spawn(async move {
          let _ = server::run_stream(config, socket, handler).await;
        });
      }
    });

    (addr, host_key, certificate_key_ids)
  }

  fn write_client_key(dir: &std::path::Path, key: &PrivateKey) -> String {
    let path = dir.join("id_test");
    let pem = key
      .to_openssh(russh::keys::ssh_key::LineEnding::LF)
      .unwrap();
    std::fs::write(&path, pem.as_bytes()).unwrap();
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    path.to_string_lossy().into_owned()
  }

  fn test_endpoint(
    addr: std::net::SocketAddr,
    host_key: &PrivateKey,
    key_reference: String,
  ) -> SshEndpoint {
    let fingerprint = host_key
      .public_key()
      .fingerprint(russh::keys::HashAlg::Sha256)
      .to_string();
    SshEndpoint {
      id: "test-endpoint".to_string(),
      instance_id: None,
      source: SshEndpointSource::UserManaged,
      hostname: addr.ip().to_string(),
      port: addr.port(),
      username: std::env::var("USER").unwrap_or_else(|_| "user".to_string()),
      host_keys: vec![TrustedHostKey {
        algorithm: "ssh-ed25519".to_string(),
        fingerprint_sha256: fingerprint,
        comment: None,
      }],
      authentication: SshAuthentication::PublicKey { key_reference },
      transport: Default::default(),
    }
  }

  #[tokio::test]
  async fn concurrent_callers_share_one_new_connection() {
    let (addr, host_key) = start_mock_server(0).await;
    let client_key = test_host_key();
    let temp_dir = tempfile::tempdir().unwrap();
    let key_reference = write_client_key(temp_dir.path(), &client_key);
    let endpoint = test_endpoint(addr, &host_key, key_reference);
    let pool = Arc::new(SshConnectionPool::new());

    let calls = (0..5).map(|_| {
      let pool = pool.clone();
      let endpoint = endpoint.clone();
      tokio::spawn(async move {
        exec_command(
          &pool,
          &endpoint,
          &["repo".to_string(), "inspect".to_string()],
          ExecLimits::default(),
          &CancellationToken::new(),
        )
        .await
      })
    });
    for call in futures::future::join_all(calls).await {
      call.unwrap().unwrap();
    }

    assert_eq!(pool.metrics.snapshot().dns_tcp_connect.count, 1);
    assert_eq!(pool.pooled_connection_count().await, 1);
  }

  #[tokio::test]
  async fn connect_times_out_when_the_host_never_answers() {
    // Accepts TCP but never speaks SSH.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let _hold = tokio::spawn(async move {
      let mut sockets = Vec::new();
      while let Ok((socket, _)) = listener.accept().await {
        sockets.push(socket);
      }
    });
    let endpoint = test_endpoint(addr, &test_host_key(), "unused".to_string());
    let pool = SshConnectionPool::new().with_connect_timeout(Duration::from_millis(200));

    let error = exec_command(
      &pool,
      &endpoint,
      &["repo".to_string(), "inspect".to_string()],
      ExecLimits::default(),
      &CancellationToken::new(),
    )
    .await
    .unwrap_err();

    assert!(
      matches!(&error, SshTransportError::ConnectionFailed(message) if message.contains("timed out")),
      "{error:?}"
    );
  }

  #[tokio::test]
  async fn exec_command_returns_stdout_and_reuses_pooled_connection() {
    let (addr, host_key) = start_mock_server(0).await;
    let client_key = test_host_key();
    let temp_dir = tempfile::tempdir().unwrap();
    let key_reference = write_client_key(temp_dir.path(), &client_key);
    let endpoint = test_endpoint(addr, &host_key, key_reference);

    let pool = SshConnectionPool::new();
    let cancellation = CancellationToken::new();

    let output = exec_command(
      &pool,
      &endpoint,
      &["repo".to_string(), "inspect".to_string()],
      ExecLimits::default(),
      &cancellation,
    )
    .await
    .unwrap();
    assert!(output.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("echo"));

    // A second call against the same endpoint must reuse the pooled
    // connection rather than opening a new one.
    exec_command(
      &pool,
      &endpoint,
      &["repo".to_string(), "status".to_string()],
      ExecLimits::default(),
      &cancellation,
    )
    .await
    .unwrap();

    assert_eq!(pool.pooled_connection_count().await, 1);
  }

  #[tokio::test]
  async fn structured_cli_error_survives_the_exec_channel_boundary() {
    let (addr, host_key) = start_mock_server(3).await;
    let client_key = test_host_key();
    let temp_dir = tempfile::tempdir().unwrap();
    let key_reference = write_client_key(temp_dir.path(), &client_key);
    let endpoint = test_endpoint(addr, &host_key, key_reference);

    let pool = SshConnectionPool::new();
    let cancellation = CancellationToken::new();

    let error = crate::core::remote::execute_remote_command::<serde_json::Value>(
      &pool,
      &endpoint,
      crate::core::remote::TreqCommandRequest::ListWorkspaces {
        repo: "/srv/project".to_string(),
      },
      ExecLimits::default(),
      &cancellation,
    )
    .await
    .unwrap_err();

    // The CLI's own `workspace_not_found` code must reach the caller intact
    // rather than collapsing to a generic transport-failure string.
    match error {
      crate::core::remote::RemoteCommandError::Command { code, message } => {
        assert_eq!(code, "workspace_not_found");
        assert!(message.contains("42"));
      }
      other => panic!("expected a structured Command error, got {other:?}"),
    }
  }

  #[tokio::test]
  async fn exec_command_enforces_total_deadline() {
    let (addr, host_key) = start_mock_server(1).await;
    let client_key = test_host_key();
    let temp_dir = tempfile::tempdir().unwrap();
    let key_reference = write_client_key(temp_dir.path(), &client_key);
    let endpoint = test_endpoint(addr, &host_key, key_reference);

    let pool = SshConnectionPool::new();
    let cancellation = CancellationToken::new();
    let limits = ExecLimits {
      deadline: Duration::from_millis(200),
      ..ExecLimits::default()
    };

    let error = exec_command(
      &pool,
      &endpoint,
      &["repo".to_string(), "inspect".to_string()],
      limits,
      &cancellation,
    )
    .await
    .unwrap_err();
    assert_eq!(error, SshTransportError::DeadlineExceeded);
  }

  #[tokio::test]
  async fn exec_command_enforces_output_limit() {
    let (addr, host_key) = start_mock_server(2).await;
    let client_key = test_host_key();
    let temp_dir = tempfile::tempdir().unwrap();
    let key_reference = write_client_key(temp_dir.path(), &client_key);
    let endpoint = test_endpoint(addr, &host_key, key_reference);

    let pool = SshConnectionPool::new();
    let cancellation = CancellationToken::new();
    let limits = ExecLimits {
      deadline: Duration::from_secs(10),
      max_output_bytes: 1024,
    };

    let error = exec_command(
      &pool,
      &endpoint,
      &["repo".to_string(), "inspect".to_string()],
      limits,
      &cancellation,
    )
    .await
    .unwrap_err();
    assert_eq!(error, SshTransportError::OutputLimitExceeded);
  }

  #[tokio::test]
  async fn exec_command_is_cancellable() {
    let (addr, host_key) = start_mock_server(1).await;
    let client_key = test_host_key();
    let temp_dir = tempfile::tempdir().unwrap();
    let key_reference = write_client_key(temp_dir.path(), &client_key);
    let endpoint = test_endpoint(addr, &host_key, key_reference);

    let pool = SshConnectionPool::new();
    let cancellation = CancellationToken::new();
    cancellation.cancel();

    let error = exec_command(
      &pool,
      &endpoint,
      &["repo".to_string(), "inspect".to_string()],
      ExecLimits::default(),
      &cancellation,
    )
    .await
    .unwrap_err();
    assert_eq!(error, SshTransportError::Cancelled);
  }

  #[tokio::test]
  async fn exec_command_rejects_unknown_host_key() {
    let (addr, host_key) = start_mock_server(0).await;
    let client_key = test_host_key();
    let temp_dir = tempfile::tempdir().unwrap();
    let key_reference = write_client_key(temp_dir.path(), &client_key);
    let mut endpoint = test_endpoint(addr, &host_key, key_reference);
    // Corrupt the trusted fingerprint so it no longer matches the server's
    // real host key.
    endpoint.host_keys[0].fingerprint_sha256 = "SHA256:not-the-real-key".to_string();

    let pool = SshConnectionPool::new();
    let cancellation = CancellationToken::new();

    let error = exec_command(
      &pool,
      &endpoint,
      &["repo".to_string(), "inspect".to_string()],
      ExecLimits::default(),
      &cancellation,
    )
    .await
    .unwrap_err();
    assert!(matches!(error, SshTransportError::ConnectionFailed(_)));
  }

  // -- Certificate and key-source resolution -----------------------------------

  /// Signs a user certificate for `subject` with `ca`, the way the control
  /// plane's `issue_certificate` does, and returns its OpenSSH text.
  fn issue_test_certificate(subject: &PrivateKey, ca: &PrivateKey, key_id: &str) -> String {
    use russh::keys::ssh_key::certificate::{Builder, CertType};
    let now = std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .unwrap()
      .as_secs();
    let mut builder = Builder::new(
      vec![7u8; 32],
      subject.public_key().key_data().clone(),
      now - 60,
      now + 600,
    )
    .unwrap();
    builder.cert_type(CertType::User).unwrap();
    builder.key_id(key_id).unwrap();
    builder.all_principals_valid().unwrap();
    builder.sign(ca).unwrap().to_openssh().unwrap()
  }

  fn certificate_endpoint(
    addr: std::net::SocketAddr,
    host_key: &PrivateKey,
    key_reference: String,
    certificate: Option<String>,
  ) -> SshEndpoint {
    let mut endpoint = test_endpoint(addr, host_key, String::new());
    endpoint.authentication = SshAuthentication::Certificate {
      key_reference,
      certificate,
    };
    endpoint
  }

  async fn inspect(
    pool: &SshConnectionPool,
    endpoint: &SshEndpoint,
  ) -> Result<ExecOutput, SshTransportError> {
    exec_command(
      pool,
      endpoint,
      &["repo".to_string(), "inspect".to_string()],
      ExecLimits::default(),
      &CancellationToken::new(),
    )
    .await
  }

  fn assert_key_material_error(error: SshTransportError, expected: &str) {
    match error {
      SshTransportError::KeyMaterialUnavailable(message) => assert!(
        message.contains(expected),
        "expected {expected:?} in {message:?}"
      ),
      other => panic!("expected KeyMaterialUnavailable, got {other:?}"),
    }
  }

  #[test]
  fn resolves_a_public_key_reference_to_its_private_key_path() {
    assert_eq!(
      ClientAuthenticator::resolve_private_key_path("/keys/id_ed25519.pub"),
      PathBuf::from("/keys/id_ed25519")
    );
    assert_eq!(
      ClientAuthenticator::resolve_private_key_path("/keys/id_ed25519-cert.pub"),
      PathBuf::from("/keys/id_ed25519")
    );
    assert_eq!(
      ClientAuthenticator::resolve_private_key_path("/keys/id_ed25519"),
      PathBuf::from("/keys/id_ed25519")
    );
  }

  #[tokio::test]
  async fn authenticates_with_an_inline_certificate_and_a_pub_key_reference() {
    let (addr, host_key, certificate_log) = start_mock_server_with_certificate_log(0).await;
    let client_key = test_host_key();
    let ca_key = test_host_key();
    let temp_dir = tempfile::tempdir().unwrap();
    // The desktop identity picker hands out the `.pub` path; no
    // `-cert.pub` file exists, so the inline certificate must be used.
    let key_reference = format!("{}.pub", write_client_key(temp_dir.path(), &client_key));
    let certificate = issue_test_certificate(&client_key, &ca_key, "inline-cert");
    let endpoint = certificate_endpoint(addr, &host_key, key_reference, Some(certificate));

    let output = inspect(&SshConnectionPool::new(), &endpoint).await.unwrap();

    assert!(output.success());
    assert_eq!(*certificate_log.lock().unwrap(), vec!["inline-cert"]);
  }

  #[tokio::test]
  async fn falls_back_to_the_cert_file_next_to_the_key_without_an_inline_certificate() {
    let (addr, host_key, certificate_log) = start_mock_server_with_certificate_log(0).await;
    let client_key = test_host_key();
    let ca_key = test_host_key();
    let temp_dir = tempfile::tempdir().unwrap();
    let key_reference = write_client_key(temp_dir.path(), &client_key);
    std::fs::write(
      format!("{key_reference}-cert.pub"),
      issue_test_certificate(&client_key, &ca_key, "file-cert"),
    )
    .unwrap();
    let endpoint = certificate_endpoint(addr, &host_key, key_reference, None);

    inspect(&SshConnectionPool::new(), &endpoint).await.unwrap();

    assert_eq!(*certificate_log.lock().unwrap(), vec!["file-cert"]);
  }

  #[tokio::test]
  async fn rejects_a_certificate_issued_for_a_different_key() {
    let (addr, host_key) = start_mock_server(0).await;
    let client_key = test_host_key();
    let other_key = test_host_key();
    let ca_key = test_host_key();
    let temp_dir = tempfile::tempdir().unwrap();
    let key_reference = write_client_key(temp_dir.path(), &client_key);
    let certificate = issue_test_certificate(&other_key, &ca_key, "wrong-key");
    let endpoint = certificate_endpoint(addr, &host_key, key_reference, Some(certificate));

    let error = inspect(&SshConnectionPool::new(), &endpoint)
      .await
      .unwrap_err();

    assert_key_material_error(error, "different key");
  }

  #[tokio::test]
  async fn rejects_unparseable_inline_certificate_text() {
    let (addr, host_key) = start_mock_server(0).await;
    let client_key = test_host_key();
    let temp_dir = tempfile::tempdir().unwrap();
    let key_reference = write_client_key(temp_dir.path(), &client_key);
    let endpoint = certificate_endpoint(
      addr,
      &host_key,
      key_reference,
      Some("not a certificate".to_string()),
    );

    let error = inspect(&SshConnectionPool::new(), &endpoint)
      .await
      .unwrap_err();

    assert_key_material_error(error, "failed to parse the issued certificate");
  }

  #[tokio::test]
  async fn reports_a_missing_private_key_as_unavailable_key_material() {
    let (addr, host_key) = start_mock_server(0).await;
    let client_key = test_host_key();
    let ca_key = test_host_key();
    let temp_dir = tempfile::tempdir().unwrap();
    let missing = temp_dir.path().join("id_missing.pub");
    let certificate = issue_test_certificate(&client_key, &ca_key, "missing-key");
    let endpoint = certificate_endpoint(
      addr,
      &host_key,
      missing.to_string_lossy().into_owned(),
      Some(certificate),
    );

    let error = inspect(&SshConnectionPool::new(), &endpoint)
      .await
      .unwrap_err();

    assert_key_material_error(error, "failed to load private key");
  }

  #[tokio::test]
  async fn rejects_the_device_keystore_reference_without_a_device_key_provider() {
    let (addr, host_key) = start_mock_server(0).await;
    let client_key = test_host_key();
    let ca_key = test_host_key();
    let certificate = issue_test_certificate(&client_key, &ca_key, "device");
    let endpoint = certificate_endpoint(
      addr,
      &host_key,
      DEVICE_KEYSTORE_KEY_REFERENCE.to_string(),
      Some(certificate),
    );

    // A plain pool is what desktop builds use.
    let error = inspect(&SshConnectionPool::new(), &endpoint)
      .await
      .unwrap_err();

    assert_key_material_error(error, "only available in the mobile app");
  }

  #[tokio::test]
  async fn authenticates_the_device_keystore_reference_through_the_provider() {
    let (addr, host_key, certificate_log) = start_mock_server_with_certificate_log(0).await;
    let device_key = test_host_key();
    let ca_key = test_host_key();
    let certificate = issue_test_certificate(&device_key, &ca_key, "device-cert");
    let endpoint = certificate_endpoint(
      addr,
      &host_key,
      DEVICE_KEYSTORE_KEY_REFERENCE.to_string(),
      Some(certificate),
    );
    let provided = device_key.clone();
    let pool = SshConnectionPool::new().with_device_key_provider(Arc::new(move || {
      let key = provided.clone();
      Box::pin(async move { Ok(key) })
    }));

    inspect(&pool, &endpoint).await.unwrap();

    assert_eq!(*certificate_log.lock().unwrap(), vec!["device-cert"]);
  }

  #[tokio::test]
  async fn requires_an_inline_certificate_for_the_device_keystore_reference() {
    let (addr, host_key) = start_mock_server(0).await;
    let device_key = test_host_key();
    let endpoint = certificate_endpoint(
      addr,
      &host_key,
      DEVICE_KEYSTORE_KEY_REFERENCE.to_string(),
      None,
    );
    let pool = SshConnectionPool::new().with_device_key_provider(Arc::new(move || {
      let key = device_key.clone();
      Box::pin(async move { Ok(key) })
    }));

    let error = inspect(&pool, &endpoint).await.unwrap_err();

    assert_key_material_error(error, "must carry the issued certificate inline");
  }

  #[tokio::test]
  async fn uses_the_renewed_certificate_for_the_next_new_connection() {
    let (addr, host_key, certificate_log) = start_mock_server_with_certificate_log(0).await;
    let client_key = test_host_key();
    let ca_key = test_host_key();
    let temp_dir = tempfile::tempdir().unwrap();
    let key_reference = write_client_key(temp_dir.path(), &client_key);
    let first = certificate_endpoint(
      addr,
      &host_key,
      key_reference.clone(),
      Some(issue_test_certificate(&client_key, &ca_key, "first")),
    );
    let renewed = certificate_endpoint(
      addr,
      &host_key,
      key_reference,
      Some(issue_test_certificate(&client_key, &ca_key, "renewed")),
    );
    let pool = SshConnectionPool::new();

    inspect(&pool, &first).await.unwrap();
    // The open connection keeps running on the certificate it was
    // authenticated with; renewal must not force a reconnect.
    inspect(&pool, &renewed).await.unwrap();
    assert_eq!(*certificate_log.lock().unwrap(), vec!["first"]);

    // Once that connection goes away, the next one presents the renewed
    // certificate the caller passed in.
    pool.mark_dead(&renewed).await;
    inspect(&pool, &renewed).await.unwrap();
    assert_eq!(*certificate_log.lock().unwrap(), vec!["first", "renewed"]);
  }

  #[test]
  fn build_remote_command_line_quotes_arguments() {
    let line = build_remote_command_line(&[
      "repo".to_string(),
      "inspect".to_string(),
      "/srv/my app".to_string(),
    ]);
    assert_eq!(line, "treq 'repo' 'inspect' '/srv/my app'");
  }

  // -- Metrics ----------------------------------------------------------------

  #[test]
  fn duration_stats_tracks_count_sum_max_and_avg() {
    let stats = DurationStats::default();
    stats.record(Duration::from_millis(10));
    stats.record(Duration::from_millis(30));
    stats.record(Duration::from_millis(20));

    let snapshot = stats.snapshot();
    assert_eq!(snapshot.count, 3);
    assert_eq!(snapshot.sum_ms, 60);
    assert_eq!(snapshot.max_ms, 30);
    assert!((snapshot.avg_ms - 20.0).abs() < f64::EPSILON);
  }

  #[test]
  fn duration_stats_snapshot_is_zeroed_before_any_recording() {
    let stats = DurationStats::default();
    let snapshot = stats.snapshot();
    assert_eq!(snapshot.count, 0);
    assert_eq!(snapshot.sum_ms, 0);
    assert_eq!(snapshot.max_ms, 0);
    assert_eq!(snapshot.avg_ms, 0.0);
  }

  #[test]
  fn transport_metrics_exec_exit_counts_are_keyed_by_category() {
    let metrics = SshTransportMetrics::default();
    metrics.record_exec_exit(ExecExitCategory::Success);
    metrics.record_exec_exit(ExecExitCategory::Success);
    metrics.record_exec_exit(ExecExitCategory::Timeout);

    let snapshot = metrics.snapshot();
    assert_eq!(snapshot.exec_success_count, 2);
    assert_eq!(snapshot.exec_timeout_count, 1);
    assert_eq!(snapshot.exec_cancelled_count, 0);
  }

  #[test]
  fn transport_metrics_snapshot_reflects_counters() {
    let metrics = SshTransportMetrics::default();
    metrics
      .host_key_mismatch_count
      .fetch_add(2, Ordering::Relaxed);
    metrics
      .pooled_connection_reuse_count
      .fetch_add(5, Ordering::Relaxed);
    metrics.reconnect_attempts.fetch_add(1, Ordering::Relaxed);
    metrics.pty_start_count.fetch_add(3, Ordering::Relaxed);
    metrics.pty_exit_count.fetch_add(2, Ordering::Relaxed);
    metrics
      .remote_version_mismatch_count
      .fetch_add(1, Ordering::Relaxed);

    let snapshot = metrics.snapshot();
    assert_eq!(snapshot.host_key_mismatch_count, 2);
    assert_eq!(snapshot.pooled_connection_reuse_count, 5);
    assert_eq!(snapshot.reconnect_attempts, 1);
    assert_eq!(snapshot.pty_start_count, 3);
    assert_eq!(snapshot.pty_exit_count, 2);
    assert_eq!(snapshot.remote_version_mismatch_count, 1);
  }

  #[tokio::test]
  async fn exec_command_records_success_exit_category_and_duration() {
    let (addr, host_key) = start_mock_server(0).await;
    let client_key = test_host_key();
    let temp_dir = tempfile::tempdir().unwrap();
    let key_reference = write_client_key(temp_dir.path(), &client_key);
    let endpoint = test_endpoint(addr, &host_key, key_reference);

    let pool = SshConnectionPool::new();
    let cancellation = CancellationToken::new();

    exec_command(
      &pool,
      &endpoint,
      &["repo".to_string(), "inspect".to_string()],
      ExecLimits::default(),
      &cancellation,
    )
    .await
    .unwrap();

    let snapshot = pool.metrics_snapshot();
    assert_eq!(snapshot.exec_success_count, 1);
    assert_eq!(snapshot.exec_channel_duration.count, 1);
    // A second call against the same endpoint reuses the pooled connection
    // rather than reconnecting.
    exec_command(
      &pool,
      &endpoint,
      &["repo".to_string(), "status".to_string()],
      ExecLimits::default(),
      &cancellation,
    )
    .await
    .unwrap();
    let snapshot = pool.metrics_snapshot();
    assert_eq!(snapshot.pooled_connection_reuse_count, 1);
    assert_eq!(snapshot.dns_tcp_connect.count, 1);
  }

  #[tokio::test]
  async fn exec_command_records_deadline_exceeded_as_timeout_category() {
    let (addr, host_key) = start_mock_server(1).await;
    let client_key = test_host_key();
    let temp_dir = tempfile::tempdir().unwrap();
    let key_reference = write_client_key(temp_dir.path(), &client_key);
    let endpoint = test_endpoint(addr, &host_key, key_reference);

    let pool = SshConnectionPool::new();
    let cancellation = CancellationToken::new();
    let limits = ExecLimits {
      deadline: Duration::from_millis(200),
      ..ExecLimits::default()
    };

    let _ = exec_command(
      &pool,
      &endpoint,
      &["repo".to_string(), "inspect".to_string()],
      limits,
      &cancellation,
    )
    .await
    .unwrap_err();

    let snapshot = pool.metrics_snapshot();
    assert_eq!(snapshot.exec_timeout_count, 1);
  }

  #[tokio::test]
  async fn exec_command_rejects_unknown_host_key_and_records_mismatch() {
    let (addr, host_key) = start_mock_server(0).await;
    let client_key = test_host_key();
    let temp_dir = tempfile::tempdir().unwrap();
    let key_reference = write_client_key(temp_dir.path(), &client_key);
    let mut endpoint = test_endpoint(addr, &host_key, key_reference);
    endpoint.host_keys[0].fingerprint_sha256 = "SHA256:not-the-real-key".to_string();

    let pool = SshConnectionPool::new();
    let cancellation = CancellationToken::new();

    let _ = exec_command(
      &pool,
      &endpoint,
      &["repo".to_string(), "inspect".to_string()],
      ExecLimits::default(),
      &cancellation,
    )
    .await
    .unwrap_err();

    let snapshot = pool.metrics_snapshot();
    assert_eq!(snapshot.host_key_mismatch_count, 1);
  }

  #[test]
  fn pty_startup_command_cds_into_the_selected_directory() {
    assert_eq!(pty_startup_command(None, None), None);
    assert_eq!(
      pty_startup_command(Some("pwd"), None).as_deref(),
      Some("pwd")
    );
    assert_eq!(
      pty_startup_command(Some("pwd"), Some("/srv/workspace")).as_deref(),
      Some("cd '/srv/workspace' && pwd")
    );
    assert_eq!(
      pty_startup_command(None, Some("/srv/workspace")).as_deref(),
      Some("cd '/srv/workspace' && exec ${SHELL:-/bin/sh} -l")
    );
  }

  #[tokio::test]
  async fn pty_open_in_directory_execs_a_quoted_cd_before_the_command() {
    let (addr, host_key) = start_mock_server(0).await;
    let client_key = test_host_key();
    let temp_dir = tempfile::tempdir().unwrap();
    let key_reference = write_client_key(temp_dir.path(), &client_key);
    let endpoint = test_endpoint(addr, &host_key, key_reference);

    let pool = SshConnectionPool::new();
    let pty = RemotePtyChannel::open_in_directory(
      &pool,
      &endpoint,
      "xterm",
      80,
      24,
      Some("pwd"),
      Some("/srv/workspace"),
    )
    .await
    .unwrap();
    let chunk = pty.read_chunk().await.unwrap().unwrap();
    assert!(
      String::from_utf8_lossy(&chunk).contains("cd '/srv/workspace' && pwd"),
      "PTY exec must cd into the selected workspace before the command"
    );

    let _ = pty.close().await;
    let snapshot = pool.metrics_snapshot();
    assert_eq!(snapshot.pty_start_count, 1);
    assert_eq!(snapshot.pty_exit_count, 1);
  }

  #[tokio::test]
  async fn pty_read_event_reports_an_exit_status_sent_after_eof() {
    let (addr, host_key) = start_mock_server(6).await;
    let client_key = test_host_key();
    let temp_dir = tempfile::tempdir().unwrap();
    let key_reference = write_client_key(temp_dir.path(), &client_key);
    let endpoint = test_endpoint(addr, &host_key, key_reference);

    let pool = SshConnectionPool::new();
    let pty = RemotePtyChannel::open(&pool, &endpoint, "xterm", 80, 24, Some("exit 7"))
      .await
      .unwrap();
    assert!(matches!(
      pty.read_event().await.unwrap(),
      PtyReadEvent::Data(_)
    ));
    assert!(matches!(
      pty.read_event().await.unwrap(),
      PtyReadEvent::ExitStatus(7)
    ));
  }

  #[tokio::test]
  async fn pty_open_and_close_record_start_and_exit_counts() {
    let (addr, host_key) = start_mock_server(0).await;
    let client_key = test_host_key();
    let temp_dir = tempfile::tempdir().unwrap();
    let key_reference = write_client_key(temp_dir.path(), &client_key);
    let endpoint = test_endpoint(addr, &host_key, key_reference);

    let pool = SshConnectionPool::new();
    let pty = RemotePtyChannel::open(&pool, &endpoint, "xterm", 80, 24, None)
      .await
      .unwrap();

    let snapshot = pool.metrics_snapshot();
    assert_eq!(snapshot.pty_start_count, 1);
    assert_eq!(snapshot.pty_exit_count, 0);

    let _ = pty.close().await;
    let snapshot = pool.metrics_snapshot();
    assert_eq!(snapshot.pty_exit_count, 1);
  }

  // -- Client restart and reconnection (Phase 8 ungated coverage) -------------
  //
  // "Client restart and reconnection" from Phase 8's coverage list does not
  // require a real Fly VM - only a real SSH server that can go away and come
  // back, which the in-process mock server here already models. This
  // exercises the same path a desktop client takes after its own process
  // restart: the pool holds no live connection for the endpoint, so the next
  // command must open a fresh one and record it as a reconnect once a
  // previously-live connection is known to be dead.

  #[tokio::test]
  async fn exec_command_reconnects_after_pooled_connection_is_marked_dead() {
    let (addr, host_key) = start_mock_server(0).await;
    let client_key = test_host_key();
    let temp_dir = tempfile::tempdir().unwrap();
    let key_reference = write_client_key(temp_dir.path(), &client_key);
    let endpoint = test_endpoint(addr, &host_key, key_reference);

    let pool = SshConnectionPool::new();
    let cancellation = CancellationToken::new();

    // First command establishes and pools a connection - not a reconnect,
    // since there was nothing to reconnect to.
    exec_command(
      &pool,
      &endpoint,
      &["repo".to_string(), "inspect".to_string()],
      ExecLimits::default(),
      &cancellation,
    )
    .await
    .unwrap();
    assert_eq!(pool.pooled_connection_count().await, 1);
    assert_eq!(pool.metrics_snapshot().reconnect_attempts, 0);

    // Simulate the server side of that connection going away (mirrors a
    // client process restart just as plausibly as a server restart: either
    // way, the pool's cached session is no longer usable and must not be
    // reused blindly). Same effect as the client itself having just
    // restarted with an empty in-memory pool for this endpoint, then
    // discovering the old session is gone.
    pool.mark_dead(&endpoint).await;
    // `mark_dead` only flips the liveness flag; the stale entry is not
    // evicted from the map until the next `get_or_connect` call replaces
    // it, so the count is unchanged here.
    assert_eq!(pool.pooled_connection_count().await, 1);

    // The server is still listening (mock server accepts new TCP
    // connections indefinitely), so the next command must transparently
    // open a fresh connection to the same endpoint and record it as a
    // reconnect attempt rather than failing the caller.
    let output = exec_command(
      &pool,
      &endpoint,
      &["repo".to_string(), "status".to_string()],
      ExecLimits::default(),
      &cancellation,
    )
    .await
    .unwrap();
    assert!(output.success());
    assert_eq!(pool.pooled_connection_count().await, 1);
    assert_eq!(pool.metrics_snapshot().reconnect_attempts, 1);
  }

  /// TCP proxy that delays every chunk by `one_way` in each direction.
  async fn delay_proxy(target: std::net::SocketAddr, one_way: Duration) -> std::net::SocketAddr {
    delay_proxy_with(target, one_way, Arc::new(AtomicBool::new(false))).await
  }

  /// [`delay_proxy`] that silently drops everything while `blackhole` is set
  /// (a NAT mapping that expired: no FIN, no RST).
  async fn delay_proxy_with(
    target: std::net::SocketAddr,
    one_way: Duration,
    blackhole: Arc<AtomicBool>,
  ) -> std::net::SocketAddr {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
      loop {
        let Ok((client, _)) = listener.accept().await else {
          break;
        };
        let Ok(server) = tokio::net::TcpStream::connect(target).await else {
          break;
        };
        let (mut cr, mut cw) = client.into_split();
        let (mut sr, mut sw) = server.into_split();
        let (blackhole_a, blackhole_b) = (blackhole.clone(), blackhole.clone());
        let (a_tx, mut a_rx) = tokio::sync::mpsc::unbounded_channel::<(Instant, Vec<u8>)>();
        let (b_tx, mut b_rx) = tokio::sync::mpsc::unbounded_channel::<(Instant, Vec<u8>)>();
        tokio::spawn(async move {
          let mut buf = vec![0u8; 16384];
          while let Ok(n) = cr.read(&mut buf).await {
            if blackhole_a.load(Ordering::SeqCst) {
              continue;
            }
            if n == 0 || a_tx.send((Instant::now(), buf[..n].to_vec())).is_err() {
              break;
            }
          }
        });
        tokio::spawn(async move {
          while let Some((at, data)) = a_rx.recv().await {
            tokio::time::sleep_until((at + one_way).into()).await;
            if sw.write_all(&data).await.is_err() {
              break;
            }
          }
        });
        tokio::spawn(async move {
          let mut buf = vec![0u8; 16384];
          while let Ok(n) = sr.read(&mut buf).await {
            if blackhole_b.load(Ordering::SeqCst) {
              continue;
            }
            if n == 0 || b_tx.send((Instant::now(), buf[..n].to_vec())).is_err() {
              break;
            }
          }
        });
        tokio::spawn(async move {
          while let Some((at, data)) = b_rx.recv().await {
            tokio::time::sleep_until((at + one_way).into()).await;
            if cw.write_all(&data).await.is_err() {
              break;
            }
          }
        });
      }
    });
    addr
  }

  /// Round 6 measurement: parallel exec reads on one pooled connection at a
  /// 150 ms RTT (75 ms each way). Prints wall time for 1, 2, 4 and 6
  /// concurrent reads after the connection is warm.
  #[tokio::test]
  #[ignore = "measurement"]
  async fn r6_parallel_exec_at_mobile_rtt() {
    let (addr, host_key) = start_mock_server(0).await;
    let proxy = delay_proxy(addr, Duration::from_millis(75)).await;
    let client_key = test_host_key();
    let temp_dir = tempfile::tempdir().unwrap();
    let key_reference = write_client_key(temp_dir.path(), &client_key);
    let endpoint = test_endpoint(proxy, &host_key, key_reference);
    let pool = Arc::new(SshConnectionPool::new());
    let args = ["repo".to_string(), "inspect".to_string()];
    let started = Instant::now();
    pool.get_or_connect(&endpoint).await.unwrap();
    println!("R6 connect (kex+auth) at 150ms RTT: {:?}", started.elapsed());
    for n in [1usize, 2, 4, 6] {
      let started = Instant::now();
      let mut tasks = Vec::new();
      for _ in 0..n {
        let (pool, endpoint, args) = (pool.clone(), endpoint.clone(), args.clone());
        tasks.push(tokio::spawn(async move {
          exec_command(
            &pool,
            &endpoint,
            &args,
            ExecLimits::default(),
            &CancellationToken::new(),
          )
          .await
          .unwrap();
          Instant::now()
        }));
      }
      let mut done = Vec::new();
      for task in tasks {
        done.push(task.await.unwrap().duration_since(started));
      }
      println!("R6 {n} parallel exec: last done after {:?}, each {:?}", started.elapsed(), done);
    }
  }

  /// Round 6: connect plus first exec, repeated, on loopback. Run once with
  /// and once with `R6_NODELAY=1` to see Nagle/delayed-ACK stalls.
  #[tokio::test]
  #[ignore = "measurement"]
  async fn r6_connect_and_first_exec_loop() {
    let (addr, host_key) = start_mock_server(0).await;
    let client_key = test_host_key();
    let temp_dir = tempfile::tempdir().unwrap();
    let key_reference = write_client_key(temp_dir.path(), &client_key);
    let endpoint = test_endpoint(addr, &host_key, key_reference);
    let args = ["repo".to_string(), "inspect".to_string()];
    let mut times = Vec::new();
    for _ in 0..30 {
      let pool = SshConnectionPool::new();
      let started = Instant::now();
      exec_command(&pool, &endpoint, &args, ExecLimits::default(), &CancellationToken::new())
        .await
        .unwrap();
      times.push(started.elapsed().as_millis());
    }
    times.sort();
    println!(
      "R6 nodelay={} connect+exec ms: min {} median {} p90 {} max {}",
      std::env::var_os("R6_NODELAY").is_some(),
      times[0],
      times[15],
      times[27],
      times[29]
    );
  }

  /// Round 6: the path goes silent (NAT expiry, network change) without a
  /// FIN or RST. Prints how long the first command takes to fail and how.
  #[tokio::test]
  #[ignore = "measurement"]
  async fn r6_first_command_on_a_silent_connection() {
    let (addr, host_key) = start_mock_server(0).await;
    let blackhole = Arc::new(AtomicBool::new(false));
    let proxy = delay_proxy_with(addr, Duration::from_millis(5), blackhole.clone()).await;
    let client_key = test_host_key();
    let temp_dir = tempfile::tempdir().unwrap();
    let key_reference = write_client_key(temp_dir.path(), &client_key);
    let endpoint = test_endpoint(proxy, &host_key, key_reference);
    let pool = SshConnectionPool::new();
    let args = ["repo".to_string(), "inspect".to_string()];
    exec_command(&pool, &endpoint, &args, ExecLimits::default(), &CancellationToken::new())
      .await
      .unwrap();
    blackhole.store(true, Ordering::SeqCst);
    let started = Instant::now();
    let result = exec_command(
      &pool,
      &endpoint,
      &args,
      ExecLimits::default(),
      &CancellationToken::new(),
    )
    .await;
    println!(
      "R6 silent path: first exec failed after {:?}: {:?}",
      started.elapsed(),
      result.err().map(|e| e.to_string())
    );
  }

  /// Round 6 measurement: a pooled connection whose session ended (relay
  /// worker recycled, network switch) is not noticed until a command fails.
  /// Prints how the first command after the drop behaves.
  #[tokio::test]
  #[ignore = "measurement"]
  async fn r6_first_command_after_session_drop() {
    let (addr, host_key) = start_mock_server(0).await;
    let client_key = test_host_key();
    let temp_dir = tempfile::tempdir().unwrap();
    let key_reference = write_client_key(temp_dir.path(), &client_key);
    let endpoint = test_endpoint(addr, &host_key, key_reference);
    let pool = SshConnectionPool::new();
    let cancellation = CancellationToken::new();
    let args = ["repo".to_string(), "inspect".to_string()];
    exec_command(&pool, &endpoint, &args, ExecLimits::default(), &cancellation)
      .await
      .unwrap();
    // The session ends without the pool being told.
    {
      let key = PoolKey::for_endpoint(&endpoint);
      let connections = pool.connections.lock().await;
      let handle = connections.get(&key).unwrap().handle.clone();
      drop(connections);
      let _ = handle
        .disconnect(Disconnect::ByApplication, "test", "en")
        .await;
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    for attempt in 1..=3 {
      let started = Instant::now();
      let result =
        exec_command(&pool, &endpoint, &args, ExecLimits::default(), &cancellation).await;
      println!(
        "R6 attempt {attempt}: ok={} err={:?} in {:?} reconnects={}",
        result.is_ok(),
        result.as_ref().err().map(|e| e.to_string()),
        started.elapsed(),
        pool.metrics_snapshot().reconnect_attempts
      );
    }
  }

  // -- Hard cutoff on revocation or expiry -----------------------------------

  #[tokio::test]
  async fn force_cutoff_tears_down_the_open_connection_and_refuses_new_ones() {
    let (addr, host_key) = start_mock_server(0).await;
    let client_key = test_host_key();
    let temp_dir = tempfile::tempdir().unwrap();
    let key_reference = write_client_key(temp_dir.path(), &client_key);
    let endpoint = test_endpoint(addr, &host_key, key_reference);

    let pool = SshConnectionPool::new();
    let cancellation = CancellationToken::new();

    // Establish an open connection first, mirroring an in-flight session at
    // the moment revocation happens.
    exec_command(
      &pool,
      &endpoint,
      &["repo".to_string(), "inspect".to_string()],
      ExecLimits::default(),
      &cancellation,
    )
    .await
    .unwrap();
    assert_eq!(pool.pooled_connection_count().await, 1);
    assert_eq!(pool.cutoff_reason(&endpoint.id).await, None);

    pool
      .force_cutoff(&endpoint.id, CutoffReason::KeyRevoked)
      .await;

    // "open exec and PTY channels to that instance are torn down": the
    // pooled connection is dropped, not merely marked dead.
    assert_eq!(pool.pooled_connection_count().await, 0);
    assert_eq!(
      pool.cutoff_reason(&endpoint.id).await,
      Some(CutoffReason::KeyRevoked)
    );

    // "no further structured commands... are sent over the stale
    // credential": a fresh attempt against the same (still up) server must
    // be refused locally rather than reaching the network.
    let result = exec_command(
      &pool,
      &endpoint,
      &["repo".to_string(), "status".to_string()],
      ExecLimits::default(),
      &cancellation,
    )
    .await;
    assert_eq!(
      result,
      Err(SshTransportError::CredentialCutOff {
        endpoint_id: endpoint.id.clone(),
        reason: CutoffReason::KeyRevoked,
      })
    );
    // No connection was created for the refused attempt.
    assert_eq!(pool.pooled_connection_count().await, 0);
  }

  #[tokio::test]
  async fn clear_cutoff_restores_normal_access() {
    let (addr, host_key) = start_mock_server(0).await;
    let client_key = test_host_key();
    let temp_dir = tempfile::tempdir().unwrap();
    let key_reference = write_client_key(temp_dir.path(), &client_key);
    let endpoint = test_endpoint(addr, &host_key, key_reference);

    let pool = SshConnectionPool::new();
    let cancellation = CancellationToken::new();

    pool
      .force_cutoff(&endpoint.id, CutoffReason::CertificateExpired)
      .await;
    assert!(exec_command(
      &pool,
      &endpoint,
      &["repo".to_string(), "inspect".to_string()],
      ExecLimits::default(),
      &cancellation,
    )
    .await
    .is_err());

    // "The user regains access only by reauthenticating and obtaining a new
    // certificate": once that happens the caller clears the cutoff and
    // normal command dispatch resumes.
    pool.clear_cutoff(&endpoint.id).await;
    assert_eq!(pool.cutoff_reason(&endpoint.id).await, None);

    let output = exec_command(
      &pool,
      &endpoint,
      &["repo".to_string(), "inspect".to_string()],
      ExecLimits::default(),
      &cancellation,
    )
    .await
    .unwrap();
    assert!(output.success());
  }

  #[tokio::test]
  async fn cut_off_managed_tears_down_managed_endpoints_until_sign_in() {
    let (addr, host_key) = start_mock_server(0).await;
    let temp_dir = tempfile::tempdir().unwrap();
    let key_reference = write_client_key(temp_dir.path(), &test_host_key());
    let user_endpoint = test_endpoint(addr, &host_key, key_reference);
    let managed = SshEndpoint {
      id: "managed-endpoint".to_string(),
      source: SshEndpointSource::Managed {
        provider: "fly_sprites".to_string(),
        generation: 1,
      },
      ..user_endpoint.clone()
    };
    let pool = SshConnectionPool::new();
    let cancellation = CancellationToken::new();
    let run = |endpoint: &SshEndpoint| {
      let endpoint = endpoint.clone();
      let (pool, cancellation) = (&pool, &cancellation);
      async move {
        let args = ["repo".to_string(), "inspect".to_string()];
        exec_command(pool, &endpoint, &args, ExecLimits::default(), cancellation).await
      }
    };
    run(&managed).await.unwrap();
    run(&user_endpoint).await.unwrap();

    let cut = pool.cut_off_managed(CutoffReason::SessionEnded).await;

    assert_eq!(cut, vec![managed.id.clone()]);
    assert_eq!(pool.pooled_connection_count().await, 1);
    assert!(matches!(
      run(&managed).await,
      Err(SshTransportError::CredentialCutOff {
        reason: CutoffReason::SessionEnded,
        ..
      })
    ));
    assert!(run(&user_endpoint).await.is_ok());

    // Signing in again lifts the blanket block; the torn-down endpoint
    // still waits for its own reauthentication.
    let fresh = SshEndpoint {
      id: "other-managed".to_string(),
      ..managed.clone()
    };
    assert!(run(&fresh).await.is_err());
    pool.set_relay_access_token(Some("token".to_string()));
    assert!(run(&fresh).await.is_ok());
    assert!(run(&managed).await.is_err());
  }

  // -- Retrying after network loss (PRD "Structured command protocol" >
  // "Retrying after network loss") ---------------------------------------

  #[tokio::test]
  async fn retry_after_reconnect_treats_mutation_as_complete_when_state_already_shows_it_applied() {
    // Mode 4: the mutation's own exec sleeps past the deadline (a genuine
    // transport failure - the client cannot tell whether it landed), and the
    // verification read that follows reports the branch already exists.
    let (addr, host_key) = start_mock_server(4).await;
    let client_key = test_host_key();
    let temp_dir = tempfile::tempdir().unwrap();
    let key_reference = write_client_key(temp_dir.path(), &client_key);
    let endpoint = test_endpoint(addr, &host_key, key_reference);

    let pool = SshConnectionPool::new();
    let cancellation = CancellationToken::new();
    let limits = ExecLimits {
      deadline: Duration::from_millis(200),
      ..ExecLimits::default()
    };

    let mut verifications = Vec::new();
    let outcome = crate::core::remote::retry_after_reconnect::<serde_json::Value, _>(
      &pool,
      &endpoint,
      crate::core::remote::TreqCommandRequest::CreateWorkspace {
        repo: "/srv/project".to_string(),
        branch_name: "feature-retry-test".to_string(),
        source_branch: None,
        metadata: None,
        idempotency_key: "retry-key-1".to_string(),
      },
      limits,
      &cancellation,
      |v| verifications.push(v),
    )
    .await
    .unwrap();

    assert_eq!(
      outcome,
      crate::core::remote::MutationRetryOutcome::AlreadyApplied
    );
    assert_eq!(
      verifications,
      vec![crate::core::remote::PostReconnectVerification::AlreadyApplied]
    );
  }

  #[tokio::test]
  async fn retry_after_reconnect_retries_with_the_same_idempotency_key_when_state_shows_not_applied(
  ) {
    // Mode 5: same deadline-triggering first exec, but the verification read
    // reports the branch is absent, so the mutation must be retried - and
    // the retry (the mock's third exec) succeeds.
    let (addr, host_key) = start_mock_server(5).await;
    let client_key = test_host_key();
    let temp_dir = tempfile::tempdir().unwrap();
    let key_reference = write_client_key(temp_dir.path(), &client_key);
    let endpoint = test_endpoint(addr, &host_key, key_reference);

    let pool = SshConnectionPool::new();
    let cancellation = CancellationToken::new();
    let limits = ExecLimits {
      deadline: Duration::from_millis(200),
      ..ExecLimits::default()
    };

    let mut verifications = Vec::new();
    let outcome = crate::core::remote::retry_after_reconnect::<serde_json::Value, _>(
      &pool,
      &endpoint,
      crate::core::remote::TreqCommandRequest::CreateWorkspace {
        repo: "/srv/project".to_string(),
        branch_name: "feature-retry-test".to_string(),
        source_branch: None,
        metadata: None,
        idempotency_key: "retry-key-2".to_string(),
      },
      limits,
      &cancellation,
      |v| verifications.push(v),
    )
    .await
    .unwrap();

    match outcome {
      crate::core::remote::MutationRetryOutcome::Applied(value) => {
        assert_eq!(value["branch_name"], "feature-retry-test");
      }
      other => panic!("expected Applied after a not-applied verdict, got {other:?}"),
    }
    assert_eq!(
      verifications,
      vec![crate::core::remote::PostReconnectVerification::Retried]
    );
  }

  #[tokio::test]
  async fn retry_after_reconnect_surfaces_ambiguity_when_no_verification_recipe_exists() {
    // `RestoreFile` has no `verification_for` recipe (see its module doc):
    // even a genuine transport failure must surface as ambiguous rather than
    // silently guessing complete or not.
    let (addr, host_key) = start_mock_server(1).await;
    let client_key = test_host_key();
    let temp_dir = tempfile::tempdir().unwrap();
    let key_reference = write_client_key(temp_dir.path(), &client_key);
    let endpoint = test_endpoint(addr, &host_key, key_reference);

    let pool = SshConnectionPool::new();
    let cancellation = CancellationToken::new();
    let limits = ExecLimits {
      deadline: Duration::from_millis(200),
      ..ExecLimits::default()
    };

    let mut verifications = Vec::new();
    let outcome = crate::core::remote::retry_after_reconnect::<serde_json::Value, _>(
      &pool,
      &endpoint,
      crate::core::remote::TreqCommandRequest::RestoreFile {
        repo: "/srv/project".to_string(),
        workspace: None,
        path: "src/lib.rs".to_string(),
      },
      limits,
      &cancellation,
      |v| verifications.push(v),
    )
    .await
    .unwrap();

    match outcome {
      crate::core::remote::MutationRetryOutcome::Ambiguous { reason } => {
        assert!(reason.contains("no state check is available"));
      }
      other => panic!("expected Ambiguous, got {other:?}"),
    }
    assert_eq!(
      verifications,
      vec![crate::core::remote::PostReconnectVerification::Ambiguous]
    );
  }

  #[tokio::test]
  async fn retry_after_reconnect_never_verifies_a_structured_application_level_error() {
    // Mode 3: the CLI reaches the VM and returns a structured
    // `workspace_not_found` error. That is not "did it land?" uncertainty -
    // the CLI plainly said it did not run - so no verification read should
    // ever be issued, and the structured error must pass straight through.
    let (addr, host_key) = start_mock_server(3).await;
    let client_key = test_host_key();
    let temp_dir = tempfile::tempdir().unwrap();
    let key_reference = write_client_key(temp_dir.path(), &client_key);
    let endpoint = test_endpoint(addr, &host_key, key_reference);

    let pool = SshConnectionPool::new();
    let cancellation = CancellationToken::new();

    let mut verifications = Vec::new();
    let error = crate::core::remote::retry_after_reconnect::<serde_json::Value, _>(
      &pool,
      &endpoint,
      crate::core::remote::TreqCommandRequest::CreateWorkspace {
        repo: "/srv/project".to_string(),
        branch_name: "feature-retry-test".to_string(),
        source_branch: None,
        metadata: None,
        idempotency_key: "retry-key-3".to_string(),
      },
      ExecLimits::default(),
      &cancellation,
      |v| verifications.push(v),
    )
    .await
    .unwrap_err();

    match error {
      crate::core::remote::RemoteCommandError::Command { code, .. } => {
        assert_eq!(code, "workspace_not_found");
      }
      other => panic!("expected a structured Command error, got {other:?}"),
    }
    // No verification read was ever issued.
    assert!(verifications.is_empty());
  }
}
