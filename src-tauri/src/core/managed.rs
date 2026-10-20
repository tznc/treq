//! Connecting to the account's Treq-managed cloud workspace, shared by
//! desktop and mobile (the frontend's `managed-ssh-connection.ts` and
//! `remote-cert-lifecycle.ts`).
//!
//! A device key is registered with the control plane, which issues a
//! 20-minute SSH certificate for it. The certificate is renewed at 80% of
//! its lifetime while the Supabase session lasts; a renewal the control
//! plane refuses cuts the endpoint off until the user reauthenticates.
//! What a cutoff does to open connections, and what a renewed certificate
//! replaces, is the caller's ([`RenewalHooks`]).

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;

use super::{
  control_plane::{self, InstanceStatus},
  remote_control_plane::{IssueCertificateResponse, SshAuthentication, SshEndpoint},
  remote_ssh_transport::CutoffReason,
  supabase::{ApiError, Supabase},
};

/// `MANAGED_REPOSITORIES_ROOT`: the managed SSH user's home, where cloned
/// repositories live.
pub const MANAGED_REPOSITORIES_ROOT: &str = "/home/treq";
pub const READY_POLL: Duration = Duration::from_secs(2);
pub const READY_TIMEOUT: Duration = Duration::from_secs(600);
const RENEWAL_REMAINING_FRACTION: f64 = 0.2;

/// A certificate lease on a managed endpoint. Times are epoch milliseconds.
#[derive(Debug, Clone, PartialEq)]
pub struct Lease {
  pub instance_id: String,
  pub key_id: String,
  pub endpoint_id: String,
  pub issued_at: i64,
  pub expires_at: i64,
}

pub fn now_ms() -> i64 {
  chrono::Utc::now().timestamp_millis()
}

/// Milliseconds until a lease should renew: at `issued + 0.8 × lifetime`.
pub fn renewal_delay_ms(issued_at_ms: i64, expires_at_ms: i64, now_ms: i64) -> i64 {
  let lifetime = (expires_at_ms - issued_at_ms).max(0) as f64;
  let renew_at = expires_at_ms as f64 - lifetime * RENEWAL_REMAINING_FRACTION;
  (renew_at as i64 - now_ms).max(0)
}

/// Maps a certificate-renewal error to a cutoff, or `None` to retry.
pub fn classify_renewal_error(error: &ApiError) -> Option<CutoffReason> {
  let message = error.message.to_lowercase();
  match error.status {
    401 => Some(CutoffReason::SessionEnded),
    404 if message.contains("key does not belong") => Some(CutoffReason::KeyRevoked),
    404 => Some(CutoffReason::InstanceInaccessible),
    409 if message.contains("revoked") => Some(CutoffReason::KeyRevoked),
    409 => Some(CutoffReason::InstanceInaccessible),
    _ => None,
  }
}

/// Retry backoff after the `attempt`-th failed renewal.
pub fn renewal_backoff(attempt: u32) -> Duration {
  Duration::from_secs((15u64 << attempt.min(4)).min(120))
}

/// The code `report_cutoff` and the frontend's cutoff store use.
pub fn reason_code(reason: CutoffReason) -> &'static str {
  match reason {
    CutoffReason::SessionEnded => "session_ended",
    CutoffReason::KeyRevoked => "key_revoked",
    CutoffReason::InstanceInaccessible => "instance_inaccessible",
    CutoffReason::CertificateExpired => "certificate_expired",
  }
}

pub fn parse_time_ms(iso: &str) -> i64 {
  chrono::DateTime::parse_from_rfc3339(iso)
    .map(|t| t.timestamp_millis())
    .unwrap_or_else(|_| now_ms() + 20 * 60 * 1000)
}

/// `composeManagedEndpoint`: the server's endpoint, authenticated with the
/// key at `key_reference` and the certificate issued for it.
pub fn compose_endpoint(response: IssueCertificateResponse, key_reference: &str) -> SshEndpoint {
  let mut endpoint = response.endpoint;
  endpoint.authentication = SshAuthentication::Certificate {
    key_reference: key_reference.to_string(),
    certificate: Some(response.certificate),
  };
  endpoint
}

/// Polls the instance until it is ready. `progress` sees every status.
pub async fn wait_for_ready(
  supabase: &Supabase,
  poll: Duration,
  timeout: Duration,
  progress: &(dyn Fn(&InstanceStatus) + Send + Sync),
) -> Result<InstanceStatus, String> {
  let deadline = tokio::time::Instant::now() + timeout;
  loop {
    let status = control_plane::instance_status(supabase)
      .await
      .map_err(|e| e.to_string())?;
    progress(&status);
    let state = status
      .instance
      .as_ref()
      .map(|i| i.status.clone())
      .unwrap_or_default();
    match state.as_str() {
      "ready" => return Ok(status),
      "failed" | "deleted" => {
        return Err(format!(
          "Managed instance provisioning failed (status: {state})."
        ))
      }
      _ if tokio::time::Instant::now() >= deadline => {
        let state = if state.is_empty() { "unknown" } else { &state };
        return Err(format!(
          "Timed out waiting for the managed instance to become ready (last status: {state})."
        ));
      }
      _ => tokio::time::sleep(poll).await,
    }
  }
}

/// Issues a certificate for `key_id` and returns its lease and the endpoint
/// it authenticates.
pub async fn issue(
  supabase: &Supabase,
  instance_id: &str,
  key_id: &str,
  key_reference: &str,
) -> Result<(Lease, SshEndpoint), String> {
  let issued_at = now_ms();
  let response = control_plane::issue_certificate(supabase, instance_id, key_id, false)
    .await
    .map_err(|e| e.to_string())?;
  let lease = Lease {
    instance_id: instance_id.to_string(),
    key_id: key_id.to_string(),
    endpoint_id: response.endpoint.id.clone(),
    issued_at,
    expires_at: parse_time_ms(&response.expires_at),
  };
  Ok((lease, compose_endpoint(response, key_reference)))
}

/// What a renewal loop does to the caller's connection.
#[async_trait]
pub trait RenewalHooks: Send + Sync {
  /// A renewed certificate: swap `endpoint` in for the active one.
  fn renewed(&self, lease: &Lease, endpoint: SshEndpoint);
  /// The certificate can no longer be renewed: close what uses `lease`.
  async fn cut_off(&self, lease: &Lease, reason: CutoffReason);
}

/// Renews `lease` until a cutoff, then reports it. Abort the task to stop.
pub async fn renewal_loop(
  supabase: Arc<Supabase>,
  mut lease: Lease,
  key_reference: String,
  hooks: Arc<dyn RenewalHooks>,
) {
  let mut attempt = 0u32;
  let reason = loop {
    let delay = if attempt == 0 {
      renewal_delay_ms(lease.issued_at, lease.expires_at, now_ms())
    } else {
      renewal_backoff(attempt - 1).as_millis() as i64
    };
    tokio::time::sleep(Duration::from_millis(delay as u64)).await;

    if supabase.auth.access_token().await.is_none() {
      break CutoffReason::SessionEnded;
    }
    let issued_at = now_ms();
    match control_plane::issue_certificate(&supabase, &lease.instance_id, &lease.key_id, true).await
    {
      Ok(response) => {
        lease.issued_at = issued_at;
        lease.expires_at = parse_time_ms(&response.expires_at);
        hooks.renewed(&lease, compose_endpoint(response, &key_reference));
        attempt = 0;
      }
      Err(error) => {
        if let Some(reason) = classify_renewal_error(&error) {
          break reason;
        }
        log::warn!("certificate renewal failed, retrying: {error}");
        let remaining = lease.expires_at - now_ms();
        if remaining <= renewal_backoff(attempt).as_millis() as i64 {
          tokio::time::sleep(Duration::from_millis(remaining.max(0) as u64)).await;
          break CutoffReason::CertificateExpired;
        }
        attempt += 1;
      }
    }
  };
  hooks.cut_off(&lease, reason).await;
  control_plane::report_cutoff(
    &supabase,
    &lease.instance_id,
    &lease.endpoint_id,
    reason_code(reason),
  )
  .await;
}

#[cfg(test)]
mod tests {
  use std::sync::Mutex;

  use serde_json::json;
  use wiremock::{
    matchers::{body_partial_json, method, path},
    Mock, MockServer, ResponseTemplate,
  };

  use super::*;
  use crate::core::supabase::test_support::client;

  fn error(status: u16, message: &str) -> ApiError {
    ApiError {
      status,
      code: "x".into(),
      message: message.into(),
      correlation_id: None,
    }
  }

  #[test]
  fn renews_at_eighty_percent_of_the_lifetime() {
    assert_eq!(renewal_delay_ms(0, 1000, 0), 800);
    assert_eq!(renewal_delay_ms(0, 1000, 900), 0);
  }

  #[test]
  fn classifies_renewal_refusals() {
    assert_eq!(
      classify_renewal_error(&error(401, "")),
      Some(CutoffReason::SessionEnded)
    );
    assert_eq!(
      classify_renewal_error(&error(404, "Key does not belong to this user")),
      Some(CutoffReason::KeyRevoked)
    );
    assert_eq!(
      classify_renewal_error(&error(404, "Instance does not belong to this user")),
      Some(CutoffReason::InstanceInaccessible)
    );
    assert_eq!(
      classify_renewal_error(&error(409, "Key has been revoked")),
      Some(CutoffReason::KeyRevoked)
    );
    assert_eq!(classify_renewal_error(&error(502, "")), None);
  }

  #[test]
  fn backoff_doubles_up_to_two_minutes() {
    assert_eq!(renewal_backoff(0), Duration::from_secs(15));
    assert_eq!(renewal_backoff(1), Duration::from_secs(30));
    assert_eq!(renewal_backoff(3), Duration::from_secs(120));
    assert_eq!(renewal_backoff(9), Duration::from_secs(120));
  }

  fn endpoint_json() -> serde_json::Value {
    json!({
      "id": "e1", "instance_id": "i1",
      "source": {"type": "managed", "provider": "fly_sprites", "generation": 2},
      "hostname": "h", "port": 22, "username": "treq", "host_keys": [],
      "authentication": {"type": "public_key", "key_reference": "server-key"}
    })
  }

  fn certificate(expires_at: &str) -> serde_json::Value {
    json!({"certificate": "ssh-ed25519-cert AAA", "serial": "1", "expires_at": expires_at, "endpoint": endpoint_json()})
  }

  #[tokio::test]
  async fn waits_for_ready_and_fails_on_a_terminal_state() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
      .and(path("/functions/v1/remote-instance"))
      .respond_with(ResponseTemplate::new(200).set_body_json(json!({
        "instance": {"instance_id": "i1", "status": "failed"}
      })))
      .mount(&server)
      .await;
    let (supabase, _) = client(&server.uri());
    let seen = Mutex::new(Vec::new());
    let error = wait_for_ready(
      &supabase,
      Duration::from_millis(1),
      Duration::from_secs(1),
      &|s: &InstanceStatus| {
        seen
          .lock()
          .unwrap()
          .push(s.instance.clone().unwrap().status)
      },
    )
    .await
    .unwrap_err();
    assert_eq!(
      error,
      "Managed instance provisioning failed (status: failed)."
    );
    assert_eq!(*seen.lock().unwrap(), vec!["failed".to_string()]);
  }

  #[tokio::test]
  async fn issue_composes_the_endpoint_with_the_local_key() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
      .and(path("/functions/v1/remote-ssh-trust"))
      .and(body_partial_json(
        json!({"action": "issue_certificate", "key_id": "k1"}),
      ))
      .respond_with(ResponseTemplate::new(200).set_body_json(certificate("2030-01-01T00:00:00Z")))
      .mount(&server)
      .await;
    let (supabase, _) = client(&server.uri());
    let (lease, endpoint) = issue(&supabase, "i1", "k1", "/home/me/.ssh/id_ed25519")
      .await
      .unwrap();
    assert_eq!(lease.endpoint_id, "e1");
    assert_eq!(lease.expires_at, parse_time_ms("2030-01-01T00:00:00Z"));
    assert_eq!(
      endpoint.authentication,
      SshAuthentication::Certificate {
        key_reference: "/home/me/.ssh/id_ed25519".into(),
        certificate: Some("ssh-ed25519-cert AAA".into()),
      }
    );
  }

  #[derive(Default)]
  struct Recorder {
    renewed: Mutex<Vec<String>>,
    cut: Mutex<Vec<CutoffReason>>,
  }

  #[async_trait]
  impl RenewalHooks for Recorder {
    fn renewed(&self, _: &Lease, endpoint: SshEndpoint) {
      self.renewed.lock().unwrap().push(endpoint.id);
    }
    async fn cut_off(&self, _: &Lease, reason: CutoffReason) {
      self.cut.lock().unwrap().push(reason);
    }
  }

  #[tokio::test]
  async fn renewal_cuts_off_when_the_session_has_ended() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
      .and(path("/functions/v1/remote-ssh-trust"))
      .and(body_partial_json(
        json!({"action": "report_cutoff", "reason": "session_ended"}),
      ))
      .respond_with(ResponseTemplate::new(200).set_body_json(json!({"status": "ok"})))
      .expect(1)
      .mount(&server)
      .await;
    let (supabase, _) = client(&server.uri());
    let hooks = Arc::new(Recorder::default());
    let lease = Lease {
      instance_id: "i1".into(),
      key_id: "k1".into(),
      endpoint_id: "e1".into(),
      issued_at: 0,
      expires_at: 0,
    };
    renewal_loop(Arc::new(supabase), lease, "key".into(), hooks.clone()).await;
    assert_eq!(*hooks.cut.lock().unwrap(), vec![CutoffReason::SessionEnded]);
    assert!(hooks.renewed.lock().unwrap().is_empty());
  }
}
