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
  /// The size tier it runs as (`s`, `m`, ...).
  #[serde(default)]
  pub tier_id: Option<String>,
  /// A region code, as in `"us_east"`.
  #[serde(default)]
  pub region: Option<String>,
}

/// Where the cloud workspace's sshd listens, from the status response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EndpointSummary {
  pub id: String,
  pub username: String,
  pub hostname: String,
  pub port: u16,
}

/// The `remote-instance` status response. Its `endpoint` is a database row,
/// kept whole for the frontend; [`InstanceStatus::endpoint_summary`] reads it.
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

/// Creates the account's cloud workspace (the control plane allows one),
/// as `tier_id` when given, else the server's default tier, and on the
/// user's own `byos_kind` provider account when given (bring your own
/// sandbox).
pub async fn ensure(
  supabase: &Supabase,
  idempotency_key: &str,
  tier_id: Option<&str>,
  byos_kind: Option<&str>,
) -> Result<Value, ApiError> {
  let mut body = json!({ "idempotency_key": idempotency_key });
  if let Some(tier_id) = tier_id {
    body["tier_id"] = json!(tier_id);
  }
  if let Some(kind) = byos_kind {
    body["credential_source"] = json!("byos");
    body["provider_kind"] = json!(kind);
  }
  instance(supabase, "ensure", body).await
}

/// `action`'s response field `field`.
async fn instance_field<T: DeserializeOwned>(
  supabase: &Supabase,
  action: &str,
  field: &str,
) -> Result<T, ApiError> {
  let mut value: Value = instance(supabase, action, json!({})).await?;
  serde_json::from_value(value[field].take())
    .map_err(|e| ApiError::new("invalid_response", e.to_string()))
}

/// The user's saved provider keys (metadata only).
pub async fn list_provider_credentials(
  supabase: &Supabase,
) -> Result<Vec<super::cloud_catalog::ProviderKey>, ApiError> {
  instance_field(supabase, "list_provider_credentials", "credentials").await
}

/// Saves a provider key; the server checks it with the provider first.
pub async fn save_provider_credential(
  supabase: &Supabase,
  provider_kind: &str,
  api_key: &str,
) -> Result<Value, ApiError> {
  instance(
    supabase,
    "save_provider_credential",
    json!({ "provider_kind": provider_kind, "api_key": api_key }),
  )
  .await
}

pub async fn delete_provider_credential(
  supabase: &Supabase,
  provider_kind: &str,
) -> Result<Value, ApiError> {
  instance(
    supabase,
    "delete_provider_credential",
    json!({ "provider_kind": provider_kind }),
  )
  .await
}

/// Tiers on the providers the user has a key for, with BYOS prices.
pub async fn byos_catalog(
  supabase: &Supabase,
) -> Result<Vec<super::cloud_catalog::ByosMachineType>, ApiError> {
  instance_field(supabase, "byos_catalog", "machine_types").await
}

/// One repository's work a resize would lose, as the server's preflight
/// reports it (`_shared/remote/preflight.ts` `RepoPreflight`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PreflightRepository {
  pub path: String,
  pub vcs: String,
  /// Workspaces with uncommitted changes.
  pub dirty: u64,
  /// Commits that are on no remote.
  pub unpushed: u64,
  /// Agents still running.
  pub agents: u64,
  pub error: Option<String>,
}

/// What a resize to a new machine would lose.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preflight {
  pub repos: Vec<PreflightRepository>,
  pub requires_ack: bool,
}

impl Preflight {
  /// One line per repository that has anything to lose.
  pub fn summary_lines(&self) -> Vec<String> {
    self
      .repos
      .iter()
      .filter_map(|r| {
        let mut parts = Vec::new();
        if r.unpushed > 0 {
          parts.push(plural(r.unpushed, "unpushed commit", "unpushed commits"));
        }
        if r.dirty > 0 {
          parts.push(plural(
            r.dirty,
            "workspace with uncommitted changes",
            "workspaces with uncommitted changes",
          ));
        }
        if r.agents > 0 {
          parts.push(plural(r.agents, "running agent", "running agents"));
        }
        (!parts.is_empty()).then(|| format!("{}: {}", r.path, parts.join(", ")))
      })
      .collect()
  }
}

fn plural(n: u64, one: &str, many: &str) -> String {
  format!("{n} {}", if n == 1 { one } else { many })
}

/// A refused resize.
#[derive(Debug, Clone, PartialEq)]
pub enum ResizeError {
  /// The move to a new machine would lose this work; retry with
  /// `acknowledged_data_loss` to go ahead.
  UnpushedWork(Preflight),
  Api(ApiError),
}

impl std::fmt::Display for ResizeError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::UnpushedWork(_) => write!(f, "[unpushed_work] Resizing would lose unpushed work"),
      Self::Api(error) => error.fmt(f),
    }
  }
}

/// Moves the cloud workspace to `tier_id`. A 409 `unpushed_work` comes
/// back as [`ResizeError::UnpushedWork`] with the server's preflight.
pub async fn resize(
  supabase: &Supabase,
  instance_id: &str,
  tier_id: &str,
  acknowledged_data_loss: bool,
  idempotency_key: &str,
) -> Result<Value, ResizeError> {
  let mut body = json!({
    "action": "resize",
    "instance_id": instance_id,
    "tier_id": tier_id,
    "idempotency_key": idempotency_key,
  });
  if acknowledged_data_loss {
    body["acknowledged_data_loss"] = json!(true);
  }
  let (status, value) = supabase
    .invoke_raw("remote-instance", &body)
    .await
    .map_err(ResizeError::Api)?;
  if (200..300).contains(&status) {
    return Ok(value);
  }
  if status == 409 && value["code"] == "unpushed_work" {
    let preflight = serde_json::from_value(value["preflight"].clone()).unwrap_or_default();
    return Err(ResizeError::UnpushedWork(preflight));
  }
  Err(ResizeError::Api(crate::core::supabase::api_error_from(
    status, value,
  )))
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

/// A cloud card process button.
#[derive(Debug, Clone, PartialEq)]
pub enum ProcessAction {
  Kill(u32),
  /// Keeps the workspace awake while the process runs (billed).
  Pin(u32),
  Unpin(String),
}

pub async fn process_action(
  supabase: &Supabase,
  instance_id: &str,
  action: &ProcessAction,
) -> Result<Value, ApiError> {
  let (name, mut body) = match action {
    ProcessAction::Kill(pid) => ("kill_process", json!({ "pid": pid })),
    ProcessAction::Pin(pid) => ("pin_process", json!({ "pid": pid })),
    ProcessAction::Unpin(pin_id) => ("unpin_process", json!({ "pin_id": pin_id })),
  };
  body["instance_id"] = json!(instance_id);
  instance(supabase, name, body).await
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
  async fn ensure_sends_the_tier_only_when_given() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
      .and(path("/functions/v1/remote-instance"))
      .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
      .mount(&server)
      .await;
    let (supabase, _) = client(&server.uri());
    ensure(&supabase, "k1", Some("m"), None).await.unwrap();
    ensure(&supabase, "k2", None, None).await.unwrap();
    let requests = server.received_requests().await.unwrap();
    let bodies: Vec<Value> = requests
      .iter()
      .map(|r| serde_json::from_slice(&r.body).unwrap())
      .collect();
    assert_eq!(
      bodies[0],
      json!({"action": "ensure", "idempotency_key": "k1", "tier_id": "m"})
    );
    assert_eq!(
      bodies[1],
      json!({"action": "ensure", "idempotency_key": "k2"})
    );
  }

  #[tokio::test]
  async fn process_actions_send_the_instance_and_process() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
      .and(path("/functions/v1/remote-instance"))
      .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
      .mount(&server)
      .await;
    let (supabase, _) = client(&server.uri());
    for action in [
      ProcessAction::Pin(500),
      ProcessAction::Unpin("p1".into()),
      ProcessAction::Kill(500),
    ] {
      process_action(&supabase, "i1", &action).await.unwrap();
    }
    let bodies: Vec<Value> = server
      .received_requests()
      .await
      .unwrap()
      .iter()
      .map(|r| serde_json::from_slice(&r.body).unwrap())
      .collect();
    assert_eq!(
      bodies,
      [
        json!({"action": "pin_process", "instance_id": "i1", "pid": 500}),
        json!({"action": "unpin_process", "instance_id": "i1", "pin_id": "p1"}),
        json!({"action": "kill_process", "instance_id": "i1", "pid": 500}),
      ]
    );
  }

  #[tokio::test]
  async fn resize_returns_the_preflight_on_unpushed_work() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
      .and(path("/functions/v1/remote-instance"))
      .and(body_json(json!({
        "action": "resize", "instance_id": "i1", "tier_id": "l", "idempotency_key": "k"
      })))
      .respond_with(ResponseTemplate::new(409).set_body_json(json!({
        "code": "unpushed_work",
        "error": "Resizing would lose unpushed work",
        "preflight": {"requires_ack": true, "repos": [
          {"path": "/home/treq/acme/api", "vcs": "jj", "dirty": 1, "unpushed": 2, "agents": 1, "error": null},
          {"path": "/home/treq/acme/web", "vcs": "jj", "dirty": 0, "unpushed": 0, "agents": 0, "error": null}
        ]}
      })))
      .mount(&server)
      .await;
    Mock::given(method("POST"))
      .and(path("/functions/v1/remote-instance"))
      .and(body_json(json!({
        "action": "resize", "instance_id": "i1", "tier_id": "l",
        "idempotency_key": "k2", "acknowledged_data_loss": true
      })))
      .respond_with(ResponseTemplate::new(200).set_body_json(json!({"operation_id": "o"})))
      .mount(&server)
      .await;
    let (supabase, _) = client(&server.uri());
    let ResizeError::UnpushedWork(preflight) =
      resize(&supabase, "i1", "l", false, "k").await.unwrap_err()
    else {
      panic!("expected unpushed work");
    };
    assert!(preflight.requires_ack);
    assert_eq!(preflight.repos[0].path, "/home/treq/acme/api");
    assert_eq!(
      preflight.summary_lines(),
      ["/home/treq/acme/api: 2 unpushed commits, 1 workspace with uncommitted changes, 1 running agent"]
    );
    let done = resize(&supabase, "i1", "l", true, "k2").await.unwrap();
    assert_eq!(done["operation_id"], "o");
  }

  #[tokio::test]
  async fn resize_reports_other_refusals_as_api_errors() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
      .and(path("/functions/v1/remote-instance"))
      .respond_with(
        ResponseTemplate::new(402).set_body_json(json!({"code": "pro_required", "error": "Pro"})),
      )
      .mount(&server)
      .await;
    let (supabase, _) = client(&server.uri());
    let ResizeError::Api(error) = resize(&supabase, "i1", "l", false, "k").await.unwrap_err()
    else {
      panic!("expected an api error");
    };
    assert!(error.is_pro_required());
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
