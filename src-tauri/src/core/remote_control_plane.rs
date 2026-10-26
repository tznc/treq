//! Control-plane contracts: SSH endpoint model, trusted host keys, and the
//! request/response shapes Supabase Edge Functions will expose.
//!
//! Nothing in this module performs network I/O or talks to Supabase. It only
//! fixes the wire shapes so the desktop client, the Edge Functions (Phase 2+),
//! and the Supabase schema (see `supabase/migrations`) agree on one contract.
//! Every mutating request carries an idempotency key, matching the PRD's
//! "Every mutating control-plane request includes an idempotency key"
//! requirement.

use serde::{Deserialize, Serialize};

use crate::core::remote_provider::{ManagedInstanceRecord, RegionCode, SizePreset};

/// A verified SSH host public key, pinned by the control plane. Managed host
/// trust is tracked independently of the user's global `known_hosts` file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustedHostKey {
  /// e.g. "ssh-ed25519", "rsa-sha2-512".
  pub algorithm: String,
  pub fingerprint_sha256: String,
  pub comment: Option<String>,
}

/// How the client authenticates to an [`SshEndpoint`].
///
/// `key_reference` always names key material on this device: a path to an
/// OpenSSH private key (or to its `.pub` file) on desktop, or
/// [`crate::core::remote_ssh_transport::DEVICE_KEYSTORE_KEY_REFERENCE`] for
/// the mobile device key held in the OS keystore. It is never a
/// control-plane key ID, and the private key itself never appears here.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SshAuthentication {
  /// A short-lived certificate signed by the Treq CA, presented alongside
  /// the user's own private key. Used for managed instances.
  Certificate {
    key_reference: String,
    /// The OpenSSH certificate text the control plane issued for this key
    /// (`ssh-ed25519-cert-v01@openssh.com ...`). The client carries it
    /// inline so a renewed certificate reaches the next connection without
    /// being written to disk. When absent, the transport reads the
    /// `<key>-cert.pub` file next to the private key, which is where
    /// `ssh-keygen -s` puts a certificate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    certificate: Option<String>,
  },
  /// Direct authentication with a user-selected public key. Treq never
  /// generates or holds the private half.
  PublicKey { key_reference: String },
}

// Written by hand so `{:?}` on an endpoint never prints the certificate
// text. A certificate is not secret, but it names the key and principals
// and adds nothing useful to a log line.
impl std::fmt::Debug for SshAuthentication {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Certificate {
        key_reference,
        certificate,
      } => f
        .debug_struct("Certificate")
        .field("key_reference", key_reference)
        .field(
          "certificate",
          &certificate.as_ref().map(|_| "<inline certificate>"),
        )
        .finish(),
      Self::PublicKey { key_reference } => f
        .debug_struct("PublicKey")
        .field("key_reference", key_reference)
        .finish(),
    }
  }
}

/// Where an [`SshEndpoint`] came from. `ExplicitAlias` requires the user to
/// have explicitly selected alias mode; discovering an alias from
/// `~/.ssh/config` (see `core::remote::list_configured_hosts`) never creates
/// trust by itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SshEndpointSource {
  Managed { provider: String, generation: u64 },
  UserManaged,
  ExplicitAlias { alias: String },
}

/// A fully trusted, connectable SSH endpoint. Repository identity references
/// `id` and a canonical remote path rather than only a host string, so a
/// managed instance's hostname can change across reprovisioning while the
/// endpoint identity (and its generation) stays stable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SshEndpoint {
  pub id: String,
  pub instance_id: Option<String>,
  pub source: SshEndpointSource,
  pub hostname: String,
  pub port: u16,
  pub username: String,
  pub host_keys: Vec<TrustedHostKey>,
  pub authentication: SshAuthentication,
  /// How the client reaches sshd. Absent in older serialized endpoints,
  /// which all dial directly, so it defaults to [`SshTransport::Direct`].
  #[serde(default, skip_serializing_if = "SshTransport::is_direct")]
  pub transport: SshTransport,
  /// What holds the machine's persistent terminals: the Treq daemon unless
  /// the user picked tmux. Absent in older serialized endpoints.
  #[serde(
    default,
    skip_serializing_if = "crate::core::pty_remote_supervisor::PtyBackend::is_default"
  )]
  pub terminal_backend: crate::core::pty_remote_supervisor::PtyBackend,
}

/// The byte path to an endpoint's sshd. SSH (host-key pinning and client
/// authentication) runs end to end over either variant.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SshTransport {
  /// TCP to `hostname:port`.
  #[default]
  Direct,
  /// A WebSocket to Treq's `remote-ssh-relay` Edge Function, which forwards
  /// raw bytes to sshd on a managed Sprite. Sprites have no raw TCP ingress,
  /// so `hostname`/`port` then name sshd as seen from inside the Sprite and
  /// are not dialed. The URL carries only ids; the Supabase session token is
  /// sent in a header on each connection (see
  /// `SshConnectionPool::set_relay_access_token`).
  Relay { url: String },
}

impl SshTransport {
  pub fn is_direct(&self) -> bool {
    matches!(self, Self::Direct)
  }
}

/// Newtype wrapper for idempotency keys carried on mutating requests. A
/// repeated request with the same key must return the existing operation
/// rather than create a second one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdempotencyKey(pub String);

/// Normalized status of a recorded control-plane operation (provision, wake,
/// reprovision, delete, key registration, etc.), independent of instance
/// lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationStatus {
  Pending,
  InProgress,
  Succeeded,
  Failed,
}

/// Generic envelope returned for any mutating control-plane call. Repeated
/// calls with the same idempotency key return the same `operation_id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationResponse {
  pub operation_id: String,
  pub status: OperationStatus,
}

// -- Instance lifecycle requests --------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProvisionInstanceRequest {
  pub region: RegionCode,
  pub size_preset: SizePreset,
  pub idempotency_key: IdempotencyKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WakeInstanceRequest {
  pub instance_id: String,
  pub idempotency_key: IdempotencyKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReprovisionInstanceRequest {
  pub instance_id: String,
  pub region: RegionCode,
  pub size_preset: SizePreset,
  pub idempotency_key: IdempotencyKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteInstanceRequest {
  pub instance_id: String,
  pub idempotency_key: IdempotencyKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceStatusResponse {
  pub instance: Option<ManagedInstanceRecord>,
  pub endpoint: Option<SshEndpoint>,
}

// -- Client keys and certificates -------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterClientKeyRequest {
  /// OpenSSH authorized_keys-format public key.
  pub public_key: String,
  pub comment: Option<String>,
  pub idempotency_key: IdempotencyKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevokeClientKeyRequest {
  pub key_id: String,
  pub idempotency_key: IdempotencyKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueCertificateRequest {
  pub instance_id: String,
  pub key_id: String,
  /// Set by the desktop client's silent-renewal loop so the control plane
  /// labels the resulting audit event as a renewal rather than first
  /// issuance (PRD "Silent renewal while the session is active"). The
  /// signing logic on the server is identical either way.
  #[serde(default, skip_serializing_if = "std::ops::Not::not")]
  pub renewal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueCertificateResponse {
  /// OpenSSH certificate text, signed by the server-side CA.
  pub certificate: String,
  pub serial: String,
  pub expires_at: String,
  pub endpoint: SshEndpoint,
}

/// Direct existing-key auth alternative (PRD "Existing keys without
/// certificates"): installs or removes a registered public key from the
/// managed VM's `authorized_keys`. Both directions are idempotent and
/// auditable on the server side.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallAuthorizedKeyRequest {
  pub instance_id: String,
  pub key_id: String,
  pub idempotency_key: IdempotencyKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoveAuthorizedKeyRequest {
  pub instance_id: String,
  pub key_id: String,
  pub idempotency_key: IdempotencyKey,
}

/// A recorded host-key rotation, per the PRD's "Reprovisioning may rotate
/// the host key" paragraph: old and new fingerprints, the generation the new
/// key was observed at, the provider resource it was scanned from, and who
/// initiated the transition that produced it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostKeyRotationRecord {
  pub endpoint_id: String,
  pub previous_fingerprint_sha256: Option<String>,
  pub new_fingerprint_sha256: String,
  pub generation: u64,
  pub provider_resource_id: Option<String>,
  pub initiating_principal: String,
  pub rotated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientKeyRecord {
  pub id: String,
  pub algorithm: String,
  pub fingerprint_sha256: String,
  pub comment: Option<String>,
  pub created_at: String,
  pub revoked_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListClientKeysResponse {
  pub keys: Vec<ClientKeyRecord>,
}

// -- User-managed endpoints ---------------------------------------------------

/// Registers a fully explicit user-owned VM endpoint. Every field is supplied
/// by the user; an `~/.ssh/config` alias may only prefill `alias` as an
/// autocomplete suggestion (see `core::remote::list_configured_hosts`) and
/// never implies trust on its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterEndpointRequest {
  pub display_name: String,
  pub hostname: String,
  pub port: u16,
  pub username: String,
  pub host_key_fingerprint: String,
  pub auth_identity_reference: String,
  /// Set only when the user explicitly chose alias mode for this endpoint.
  pub alias: Option<String>,
  pub idempotency_key: IdempotencyKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterRepositoryRequest {
  pub endpoint_id: String,
  pub remote_path: String,
  pub display_name: String,
  pub idempotency_key: IdempotencyKey,
}

// -- Catalog reads -------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListRegionsResponse {
  pub regions: Vec<RegionCode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListSizePresetsResponse {
  pub presets: Vec<SizePreset>,
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn provision_request_round_trips_through_json() {
    let request = ProvisionInstanceRequest {
      region: RegionCode::UsEast,
      size_preset: SizePreset::Small,
      idempotency_key: IdempotencyKey("abc-123".to_string()),
    };
    let json = serde_json::to_string(&request).unwrap();
    let round_tripped: ProvisionInstanceRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(request, round_tripped);
  }

  #[test]
  fn register_endpoint_request_alias_is_optional() {
    let request = RegisterEndpointRequest {
      display_name: "Dev box".to_string(),
      hostname: "10.0.0.5".to_string(),
      port: 22,
      username: "dev".to_string(),
      host_key_fingerprint: "SHA256:abc".to_string(),
      auth_identity_reference: "key:local-default".to_string(),
      alias: None,
      idempotency_key: IdempotencyKey("register-1".to_string()),
    };
    let json = serde_json::to_value(&request).unwrap();
    assert!(json.get("alias").unwrap().is_null());
  }

  #[test]
  fn deserializes_certificate_authentication_without_inline_certificate() {
    let auth: SshAuthentication =
      serde_json::from_str(r#"{"type":"certificate","key_reference":"~/.ssh/id_ed25519"}"#)
        .unwrap();
    assert_eq!(
      auth,
      SshAuthentication::Certificate {
        key_reference: "~/.ssh/id_ed25519".to_string(),
        certificate: None,
      }
    );
  }

  const ENDPOINT_WITHOUT_TRANSPORT: &str = r#"{
    "id": "ep-1",
    "instance_id": null,
    "source": {"type": "user_managed"},
    "hostname": "dev.example.com",
    "port": 22,
    "username": "me",
    "host_keys": [],
    "authentication": {"type": "public_key", "key_reference": "~/.ssh/id_ed25519"}
  }"#;

  #[test]
  fn endpoint_without_transport_defaults_to_direct_and_round_trips_unchanged() {
    let endpoint: SshEndpoint = serde_json::from_str(ENDPOINT_WITHOUT_TRANSPORT).unwrap();
    assert_eq!(endpoint.transport, SshTransport::Direct);
    let json = serde_json::to_value(&endpoint).unwrap();
    assert!(json.get("transport").is_none());
  }

  #[test]
  fn endpoint_relay_transport_round_trips() {
    let mut value: serde_json::Value = serde_json::from_str(ENDPOINT_WITHOUT_TRANSPORT).unwrap();
    value["transport"] = serde_json::json!({
      "type": "relay",
      "url": "wss://proj.supabase.co/functions/v1/remote-ssh-relay?endpoint_id=ep-1&key_id=k"
    });
    let endpoint: SshEndpoint = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(
      endpoint.transport,
      SshTransport::Relay {
        url: "wss://proj.supabase.co/functions/v1/remote-ssh-relay?endpoint_id=ep-1&key_id=k"
          .to_string()
      }
    );
    assert_eq!(serde_json::to_value(&endpoint).unwrap(), value);
  }

  #[test]
  fn omits_certificate_text_from_debug_output() {
    let auth = SshAuthentication::Certificate {
      key_reference: "keystore:device".to_string(),
      certificate: Some("ssh-ed25519-cert-v01@openssh.com AAAACERTBODY".to_string()),
    };
    let debug = format!("{auth:?}");
    assert!(!debug.contains("AAAACERTBODY"));
    assert!(debug.contains("keystore:device"));
  }
}
