//! Treq control-plane Edge Functions (`remote-instance`, `remote-ssh-trust`),
//! ported from the desktop frontend's `remote-control-plane.ts`.

use serde::{de::DeserializeOwned, Deserialize};
use serde_json::{json, Value};
use treq_lib::core::remote_control_plane::IssueCertificateResponse;

use crate::{backend, config};

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionError {
  pub status: u16,
  pub code: String,
  pub message: String,
  pub correlation_id: Option<String>,
}

impl std::fmt::Display for FunctionError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "[{}] {}", self.code, self.message)?;
    if self.status > 0 {
      write!(f, "\nHTTP {}", self.status)?;
      if let Some(id) = &self.correlation_id {
        write!(f, " · Correlation ID: {id}")?;
      }
    }
    Ok(())
  }
}

#[derive(Deserialize, Default)]
struct ErrorBody {
  #[serde(default)]
  error: Option<String>,
  #[serde(default)]
  code: Option<String>,
  #[serde(default)]
  provider_error: Option<String>,
  #[serde(default)]
  correlation_id: Option<String>,
}

fn function_error(status: u16, header_id: Option<String>, body: ErrorBody) -> FunctionError {
  FunctionError {
    status,
    code: body
      .code
      .or(body.provider_error)
      .unwrap_or_else(|| format!("http_{status}")),
    message: body.error.unwrap_or_else(|| "Request failed".into()),
    correlation_id: body.correlation_id.or(header_id),
  }
}

fn transport_error(error: impl std::fmt::Display) -> FunctionError {
  FunctionError {
    status: 0,
    code: "network".into(),
    message: error.to_string(),
    correlation_id: None,
  }
}

async fn invoke<T: DeserializeOwned>(name: &str, body: Value) -> Result<T, FunctionError> {
  let env = config::env();
  let backend = backend::get();
  let token = backend
    .auth
    .access_token()
    .await
    .unwrap_or_else(|| env.supabase.anon_key.clone());
  let response = backend
    .http
    .post(format!("{}/functions/v1/{name}", env.supabase.url))
    .bearer_auth(token)
    .header("apikey", &env.supabase.anon_key)
    .json(&body)
    .send()
    .await
    .map_err(transport_error)?;
  let status = response.status();
  if !status.is_success() {
    let header_id = response
      .headers()
      .get("x-correlation-id")
      .and_then(|v| v.to_str().ok())
      .map(str::to_owned);
    let body = response.json::<ErrorBody>().await.unwrap_or_default();
    return Err(function_error(status.as_u16(), header_id, body));
  }
  response.json::<T>().await.map_err(transport_error)
}

/// The fields of a managed instance the mobile flow uses. The status
/// response's `endpoint` is a raw database row, so it is not parsed.
#[derive(Debug, Clone, Deserialize)]
pub struct ManagedInstance {
  pub instance_id: String,
  pub status: String,
  #[serde(default)]
  pub endpoint_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StatusResponse {
  instance: Option<ManagedInstance>,
}

pub async fn instance_status() -> Result<Option<ManagedInstance>, FunctionError> {
  invoke::<StatusResponse>("remote-instance", json!({ "action": "status" }))
    .await
    .map(|r| r.instance)
}

pub async fn wake(instance_id: &str) -> Result<(), FunctionError> {
  invoke::<Value>(
    "remote-instance",
    json!({
        "action": "wake",
        "instance_id": instance_id,
        "idempotency_key": format!("wake-{instance_id}-{}", uuid::Uuid::new_v4()),
    }),
  )
  .await
  .map(drop)
}

#[derive(Debug, Clone, Deserialize)]
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

pub async fn register_client_key(
  public_key: &str,
  comment: &str,
  idempotency_key: &str,
) -> Result<ClientKey, FunctionError> {
  let response = invoke::<RegisterResponse>(
    "remote-ssh-trust",
    json!({
        "action": "register_client_key",
        "public_key": public_key,
        "comment": comment,
        "idempotency_key": idempotency_key,
    }),
  )
  .await?;
  // An idempotent replay returns the key list instead of the key.
  let RegisterResponse { key, keys } = response;
  key
    .or_else(|| {
      keys
        .iter()
        .find(|k| k.comment.as_deref() == Some(comment))
        .or(keys.first())
        .cloned()
    })
    .ok_or_else(|| FunctionError {
      status: 0,
      code: "no_key".into(),
      message: "The control plane returned no client key".into(),
      correlation_id: None,
    })
}

pub async fn issue_certificate(
  instance_id: &str,
  key_id: &str,
  renewal: bool,
) -> Result<IssueCertificateResponse, FunctionError> {
  let mut body = json!({
      "action": "issue_certificate",
      "instance_id": instance_id,
      "key_id": key_id,
  });
  if renewal {
    body["renewal"] = json!(true);
  }
  invoke("remote-ssh-trust", body).await
}

/// Best-effort audit of a credential cutoff.
pub async fn report_cutoff(instance_id: &str, endpoint_id: &str, reason: &str) {
  let result = invoke::<Value>(
    "remote-ssh-trust",
    json!({
        "action": "report_cutoff",
        "instance_id": instance_id,
        "endpoint_id": endpoint_id,
        "reason": reason,
    }),
  )
  .await;
  if let Err(error) = result {
    log::warn!("report_cutoff failed: {error}");
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn error_codes_fall_back_to_provider_error_then_status() {
    let body = ErrorBody {
      error: Some("Key has been revoked".into()),
      provider_error: Some("sprites".into()),
      ..Default::default()
    };
    let error = function_error(409, Some("cid".into()), body);
    assert_eq!(error.code, "sprites");
    assert_eq!(
      error.to_string(),
      "[sprites] Key has been revoked\nHTTP 409 · Correlation ID: cid"
    );
    assert_eq!(
      function_error(500, None, ErrorBody::default()).code,
      "http_500"
    );
  }

  #[test]
  fn status_response_tolerates_a_raw_endpoint_row() {
    let response: StatusResponse = serde_json::from_value(json!({
        "instance": {"instance_id": "i1", "status": "ready", "generation": 3},
        "endpoint": {"id": "e", "hostname": "h", "port": 22, "username": "u", "source": "managed"}
    }))
    .unwrap();
    assert_eq!(response.instance.unwrap().status, "ready");
  }
}
