//! The Treq control-plane Edge Functions (`remote-instance` and
//! `remote-ssh-trust`), as the frontend's `remote-control-plane.ts` calls
//! them. Desktop and mobile share these.

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};

use super::{
  remote_control_plane::IssueCertificateResponse,
  supabase::{ApiError, Supabase},
};

/// The fields of a managed instance Treq reads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManagedInstance {
  pub instance_id: String,
  /// A `ManagedInstanceState`, as in `"ready"`.
  pub status: String,
  #[serde(default)]
  pub generation: u64,
  #[serde(default)]
  pub endpoint_id: Option<String>,
  #[serde(default)]
  pub provider_resource_id: Option<String>,
  #[serde(default)]
  pub disk_quota_gb: f64,
  /// When the owner's Pro ended. The lapse sweep deletes the instance
  /// `LAPSE_GRACE_DAYS` later.
  #[serde(default)]
  pub lapsed_at: Option<String>,
}

/// Where the cloud workspace's sshd listens, from the status response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EndpointSummary {
  pub id: String,
  pub username: String,
  pub hostname: String,
  pub port: u16,
}

/// `InstanceStatusResponse`. Its `endpoint` is a database row, kept whole
/// for the frontend; [`InstanceStatus::endpoint_summary`] reads it.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct InstanceStatus {
  #[serde(default)]
  pub instance: Option<ManagedInstance>,
  #[serde(default)]
  pub endpoint: Option<Value>,
}

impl InstanceStatus {
  pub fn endpoint_summary(&self) -> Option<EndpointSummary> {
    serde_json::from_value(self.endpoint.clone()?).ok()
  }
}

async fn instance<T: DeserializeOwned>(
  supabase: &Supabase,
  action: &str,
  mut body: Value,
) -> Result<T, ApiError> {
  body["action"] = json!(action);
  supabase.invoke("remote-instance", &body).await
}

async fn trust<T: DeserializeOwned>(
  supabase: &Supabase,
  action: &str,
  mut body: Value,
) -> Result<T, ApiError> {
  body["action"] = json!(action);
  supabase.invoke("remote-ssh-trust", &body).await
}

pub async fn instance_status(supabase: &Supabase) -> Result<InstanceStatus, ApiError> {
  instance(supabase, "status", json!({})).await
}

/// Creates the account's cloud workspace (the control plane allows one).
pub async fn ensure(supabase: &Supabase, idempotency_key: &str) -> Result<Value, ApiError> {
  instance(
    supabase,
    "ensure",
    json!({ "idempotency_key": idempotency_key }),
  )
  .await
}

/// `wakeIdempotencyKey`.
pub fn wake_idempotency_key(instance_id: &str, attempt_id: &str) -> String {
  format!("wake-{instance_id}-{attempt_id}")
}

pub async fn wake(
  supabase: &Supabase,
  instance_id: &str,
  idempotency_key: &str,
) -> Result<Value, ApiError> {
  instance(
    supabase,
    "wake",
    json!({ "instance_id": instance_id, "idempotency_key": idempotency_key }),
  )
  .await
}

pub async fn reprovision(
  supabase: &Supabase,
  instance_id: &str,
  idempotency_key: &str,
) -> Result<Value, ApiError> {
  instance(
    supabase,
    "reprovision",
    json!({ "instance_id": instance_id, "idempotency_key": idempotency_key }),
  )
  .await
}

pub async fn delete(
  supabase: &Supabase,
  instance_id: &str,
  idempotency_key: &str,
) -> Result<Value, ApiError> {
  instance(
    supabase,
    "delete",
    json!({ "instance_id": instance_id, "idempotency_key": idempotency_key }),
  )
  .await
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ClientKey {
  pub id: String,
  #[serde(default)]
  pub comment: Option<String>,
}

#[derive(Deserialize)]
struct RegisterResponse {
  #[serde(default)]
  key: Option<ClientKey>,
  #[serde(default)]
  keys: Vec<ClientKey>,
}

/// Registers a device's public key. An idempotent replay answers with the
/// key list instead of the key, so the one with `comment` is picked.
pub async fn register_client_key(
  supabase: &Supabase,
  public_key: &str,
  comment: &str,
  idempotency_key: &str,
) -> Result<ClientKey, ApiError> {
  let RegisterResponse { key, keys } = trust(
    supabase,
    "register_client_key",
    json!({
      "public_key": public_key,
      "comment": comment,
      "idempotency_key": idempotency_key,
    }),
  )
  .await?;
  key
    .or_else(|| {
      keys
        .iter()
        .find(|k| k.comment.as_deref() == Some(comment))
        .or(keys.first())
        .cloned()
    })
    .ok_or_else(|| ApiError::new("no_key", "The control plane returned no client key"))
}

pub async fn revoke_client_key(
  supabase: &Supabase,
  key_id: &str,
  idempotency_key: &str,
) -> Result<Value, ApiError> {
  trust(
    supabase,
    "revoke_client_key",
    json!({ "key_id": key_id, "idempotency_key": idempotency_key }),
  )
  .await
}

/// Issues (or, for the same instance and key, renews) a short-lived SSH
/// certificate.
pub async fn issue_certificate(
  supabase: &Supabase,
  instance_id: &str,
  key_id: &str,
  renewal: bool,
) -> Result<IssueCertificateResponse, ApiError> {
  let mut body = json!({ "instance_id": instance_id, "key_id": key_id });
  if renewal {
    body["renewal"] = json!(true);
  }
  trust(supabase, "issue_certificate", body).await
}

/// Best-effort audit of a credential cutoff that already happened here.
pub async fn report_cutoff(
  supabase: &Supabase,
  instance_id: &str,
  endpoint_id: &str,
  reason: &str,
) {
  let result = trust::<Value>(
    supabase,
    "report_cutoff",
    json!({ "instance_id": instance_id, "endpoint_id": endpoint_id, "reason": reason }),
  )
  .await;
  if let Err(error) = result {
    log::warn!("report_cutoff failed: {error}");
  }
}

#[cfg(test)]
mod tests {
  use wiremock::{
    matchers::{body_json, method, path},
    Mock, MockServer, ResponseTemplate,
  };

  use super::*;
  use crate::core::supabase::test_support::client;

  #[test]
  fn status_tolerates_a_raw_endpoint_row() {
    let status: InstanceStatus = serde_json::from_value(json!({
        "instance": {"instance_id": "i1", "status": "ready", "generation": 3, "disk_quota_gb": 10},
        "endpoint": {"id": "e", "hostname": "h", "port": 22, "username": "u", "source": "managed"}
    }))
    .unwrap();
    assert_eq!(status.instance.as_ref().unwrap().generation, 3);
    assert_eq!(
      status.endpoint_summary(),
      Some(EndpointSummary {
        id: "e".into(),
        username: "u".into(),
        hostname: "h".into(),
        port: 22
      })
    );
  }

  #[tokio::test]
  async fn sends_each_action_to_its_function() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
      .and(path("/functions/v1/remote-instance"))
      .and(body_json(json!({"action": "status"})))
      .respond_with(
        ResponseTemplate::new(200).set_body_json(json!({"instance": null, "endpoint": null})),
      )
      .mount(&server)
      .await;
    Mock::given(method("POST"))
      .and(path("/functions/v1/remote-instance"))
      .and(body_json(
        json!({"action": "delete", "instance_id": "i1", "idempotency_key": "k"}),
      ))
      .respond_with(
        ResponseTemplate::new(200).set_body_json(json!({"operation_id": "o", "status": "pending"})),
      )
      .expect(1)
      .mount(&server)
      .await;
    Mock::given(method("POST"))
      .and(path("/functions/v1/remote-ssh-trust"))
      .and(body_json(json!({
        "action": "register_client_key", "public_key": "ssh-ed25519 AAA",
        "comment": "treq-managed-vm", "idempotency_key": "r"
      })))
      .respond_with(ResponseTemplate::new(200).set_body_json(json!({
        "keys": [{"id": "k0", "comment": "other"}, {"id": "k1", "comment": "treq-managed-vm"}]
      })))
      .mount(&server)
      .await;
    let (supabase, _) = client(&server.uri());
    assert_eq!(
      instance_status(&supabase).await.unwrap(),
      InstanceStatus::default()
    );
    delete(&supabase, "i1", "k").await.unwrap();
    let key = register_client_key(&supabase, "ssh-ed25519 AAA", "treq-managed-vm", "r")
      .await
      .unwrap();
    assert_eq!(key.id, "k1");
  }
}

#[cfg(test)]
mod lapse_tests {
  use super::*;

  #[test]
  fn reads_when_the_instance_lapsed() {
    let status: InstanceStatus = serde_json::from_value(json!({
      "instance": {
        "instance_id": "i-1",
        "status": "suspended",
        "lapsed_at": "2026-11-01T12:00:00Z",
      },
    }))
    .unwrap();
    assert_eq!(
      status.instance.unwrap().lapsed_at.as_deref(),
      Some("2026-11-01T12:00:00Z")
    );
  }
}
