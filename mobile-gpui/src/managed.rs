//! Connecting to the user's Treq-managed instance, ported from the desktop
//! frontend's `useMobileRemoteConnection`, `managed-ssh-connection.ts` and
//! `remote-cert-lifecycle.ts`.
//!
//! The device's ed25519 key (OS keystore, behind biometrics) is registered
//! with the control plane, which issues a 20-minute SSH certificate for it.
//! The certificate is renewed at 80% of its lifetime; a renewal refusal cuts
//! the endpoint off until the user reauthenticates.

use std::{sync::Mutex, time::Duration};

use treq_lib::core::{
  remote_control_plane::{IssueCertificateResponse, SshAuthentication, SshEndpoint, SshTransport},
  remote_ssh_transport::{CutoffReason, DEVICE_KEYSTORE_KEY_REFERENCE},
};

use crate::{
  backend::{self, AppEvent, Connection, ConnectionKind},
  control_plane::{self, FunctionError},
};

pub const NO_MANAGED_INSTANCE: &str =
  "No managed instance for this account yet. Set one up from Treq on desktop, then connect here.";
pub const SECURE_STORAGE_PREFIX: &str = "secure_storage_unavailable:";
const DEVICE_KEY_COMMENT: &str = "treq-mobile-device";
const READY_POLL: Duration = Duration::from_secs(2);
const READY_TIMEOUT: Duration = Duration::from_secs(600);
const RENEWAL_REMAINING_FRACTION: f64 = 0.2;
const RELAY_TOKEN_SYNC: Duration = Duration::from_secs(60);

/// A certificate lease on the managed endpoint.
#[derive(Debug, Clone)]
struct Lease {
  instance_id: String,
  key_id: String,
  endpoint_id: String,
  issued_at: i64,
  expires_at: i64,
}

/// Background tasks owned by the current managed connection.
#[derive(Default)]
struct Tasks {
  renewal: Option<tokio::task::JoinHandle<()>>,
  relay_sync: Option<tokio::task::JoinHandle<()>>,
  lease: Option<Lease>,
}

static TASKS: Mutex<Tasks> = Mutex::new(Tasks {
  renewal: None,
  relay_sync: None,
  lease: None,
});

/// Milliseconds until a lease should renew: at `issued + 0.8 × lifetime`.
pub fn renewal_delay_ms(issued_at_ms: i64, expires_at_ms: i64, now_ms: i64) -> i64 {
  let lifetime = (expires_at_ms - issued_at_ms).max(0) as f64;
  let renew_at = expires_at_ms as f64 - lifetime * RENEWAL_REMAINING_FRACTION;
  (renew_at as i64 - now_ms).max(0)
}

/// Maps a certificate-renewal error to a cutoff, or `None` to retry.
pub fn classify_renewal_error(error: &FunctionError) -> Option<CutoffReason> {
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

fn reason_code(reason: CutoffReason) -> &'static str {
  match reason {
    CutoffReason::SessionEnded => "session_ended",
    CutoffReason::KeyRevoked => "key_revoked",
    CutoffReason::InstanceInaccessible => "instance_inaccessible",
    CutoffReason::CertificateExpired => "certificate_expired",
  }
}

fn parse_time_ms(iso: &str) -> i64 {
  chrono::DateTime::parse_from_rfc3339(iso)
    .map(|t| t.timestamp_millis())
    .unwrap_or_else(|_| now_ms() + 20 * 60 * 1000)
}

fn now_ms() -> i64 {
  chrono::Utc::now().timestamp_millis()
}

#[cfg(mobile)]
async fn device_public_key() -> Result<(String, String), String> {
  let info = treq_lib::core::remote_device_key::ensure_device_key(&backend::get().app).await?;
  Ok((info.public_key, info.fingerprint_sha256))
}

#[cfg(not(mobile))]
async fn device_public_key() -> Result<(String, String), String> {
  Err("The device key is only available on Android and iOS.".into())
}

/// The device key's OpenSSH public key line, to install on a self-managed
/// host's `authorized_keys`.
pub async fn device_public_key_line() -> Result<String, String> {
  device_public_key().await.map(|(key, _)| key)
}

/// Connects to the managed instance, waking it first if needed. `progress`
/// receives the step text the UI shows.
pub async fn connect(progress: impl Fn(&'static str) + Send + Sync) -> Result<Connection, String> {
  progress("Checking the managed instance");
  let mut instance = control_plane::instance_status()
    .await
    .map_err(|e| e.to_string())?
    .ok_or_else(|| NO_MANAGED_INSTANCE.to_string())?;

  if instance.status == "suspended" {
    progress("Waking the managed instance");
    control_plane::wake(&instance.instance_id)
      .await
      .map_err(|e| e.to_string())?;
    instance.status = "waking".into();
  }
  if instance.status != "ready" {
    progress("Waiting for the managed instance");
    let deadline = tokio::time::Instant::now() + READY_TIMEOUT;
    loop {
      match instance.status.as_str() {
        "ready" => break,
        "failed" | "deleted" => {
          return Err(format!(
            "Managed instance provisioning failed (status: {}).",
            instance.status
          ));
        }
        _ if tokio::time::Instant::now() >= deadline => {
          return Err(format!(
            "Timed out waiting for the managed instance (last status: {}).",
            instance.status
          ));
        }
        _ => {}
      }
      tokio::time::sleep(READY_POLL).await;
      instance = control_plane::instance_status()
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| NO_MANAGED_INSTANCE.to_string())?;
    }
  }

  progress("Getting a certificate");
  let (lease, endpoint) = issue(&instance.instance_id).await?;
  activate(lease, endpoint.clone()).await;
  Ok(Connection {
    endpoint,
    kind: ConnectionKind::Managed,
  })
}

/// Registers the device key and issues a certificate for it.
async fn issue(instance_id: &str) -> Result<(Lease, SshEndpoint), String> {
  let (public_key, fingerprint) = device_public_key().await?;
  let key = control_plane::register_client_key(
    &public_key,
    DEVICE_KEY_COMMENT,
    &format!("register:{fingerprint}"),
  )
  .await
  .map_err(|e| e.to_string())?;
  let issued_at = now_ms();
  let response = control_plane::issue_certificate(instance_id, &key.id, false)
    .await
    .map_err(|e| e.to_string())?;
  let lease = Lease {
    instance_id: instance_id.to_string(),
    key_id: key.id,
    endpoint_id: response.endpoint.id.clone(),
    issued_at,
    expires_at: parse_time_ms(&response.expires_at),
  };
  Ok((lease, with_device_certificate(response)))
}

fn with_device_certificate(response: IssueCertificateResponse) -> SshEndpoint {
  let mut endpoint = response.endpoint;
  endpoint.authentication = SshAuthentication::Certificate {
    key_reference: DEVICE_KEYSTORE_KEY_REFERENCE.to_string(),
    certificate: Some(response.certificate),
  };
  endpoint
}

async fn activate(lease: Lease, endpoint: SshEndpoint) {
  stop();
  if matches!(endpoint.transport, SshTransport::Relay { .. }) {
    sync_relay_token().await;
    let sync = tokio::spawn(async {
      loop {
        tokio::time::sleep(RELAY_TOKEN_SYNC).await;
        sync_relay_token().await;
      }
    });
    TASKS.lock().unwrap().relay_sync = Some(sync);
  }
  backend::get().set_connection(Some(Connection {
    endpoint,
    kind: ConnectionKind::Managed,
  }));
  let renewal = tokio::spawn(renewal_loop(lease.clone()));
  let mut tasks = TASKS.lock().unwrap();
  tasks.lease = Some(lease);
  tasks.renewal = Some(renewal);
}

/// Hands the relay the current Supabase access token.
async fn sync_relay_token() {
  let backend = backend::get();
  let token = backend.auth.access_token().await;
  backend.pool.set_relay_access_token(token);
}

async fn renewal_loop(mut lease: Lease) {
  let mut attempt = 0u32;
  loop {
    let delay = if attempt == 0 {
      renewal_delay_ms(lease.issued_at, lease.expires_at, now_ms())
    } else {
      renewal_backoff(attempt - 1).as_millis() as i64
    };
    tokio::time::sleep(Duration::from_millis(delay as u64)).await;

    if backend::get().auth.access_token().await.is_none() {
      cut_off(&lease, CutoffReason::SessionEnded).await;
      return;
    }
    let issued_at = now_ms();
    match control_plane::issue_certificate(&lease.instance_id, &lease.key_id, true).await {
      Ok(response) => {
        lease.issued_at = issued_at;
        lease.expires_at = parse_time_ms(&response.expires_at);
        backend::get().update_endpoint(with_device_certificate(response));
        TASKS.lock().unwrap().lease = Some(lease.clone());
        attempt = 0;
      }
      Err(error) => {
        if let Some(reason) = classify_renewal_error(&error) {
          cut_off(&lease, reason).await;
          return;
        }
        log::warn!("certificate renewal failed, retrying: {error}");
        let remaining = lease.expires_at - now_ms();
        if remaining <= renewal_backoff(attempt).as_millis() as i64 {
          tokio::time::sleep(Duration::from_millis(remaining.max(0) as u64)).await;
          cut_off(&lease, CutoffReason::CertificateExpired).await;
          return;
        }
        attempt += 1;
      }
    }
  }
}

async fn cut_off(lease: &Lease, reason: CutoffReason) {
  let backend = backend::get();
  backend.pool.force_cutoff(&lease.endpoint_id, reason).await;
  backend
    .ptys
    .close_all_for_endpoint(&lease.endpoint_id)
    .await;
  backend.emit(AppEvent::Cutoff {
    endpoint_id: lease.endpoint_id.clone(),
    reason: Some(reason.to_string()),
  });
  control_plane::report_cutoff(&lease.instance_id, &lease.endpoint_id, reason_code(reason)).await;
}

/// Issues a fresh certificate after a cutoff and lifts it.
pub async fn reauthenticate() -> Result<(), String> {
  let instance_id = TASKS
    .lock()
    .unwrap()
    .lease
    .as_ref()
    .map(|l| l.instance_id.clone())
    .or_else(|| {
      backend::get()
        .connection()
        .and_then(|c| c.endpoint.instance_id)
    })
    .ok_or("This host is not a managed instance.")?;
  let (lease, endpoint) = issue(&instance_id).await?;
  let endpoint_id = endpoint.id.clone();
  activate(lease, endpoint).await;
  backend::get().pool.clear_cutoff(&endpoint_id).await;
  backend::get().emit(AppEvent::Cutoff {
    endpoint_id,
    reason: None,
  });
  Ok(())
}

/// Whether the current managed connection must be rebuilt after the app
/// was suspended (stale lease, instance not ready, or a new endpoint).
pub async fn is_stale() -> bool {
  let lease = TASKS.lock().unwrap().lease.clone();
  let Some(lease) = lease else { return true };
  if renewal_delay_ms(lease.issued_at, lease.expires_at, now_ms()) == 0 {
    return true;
  }
  match control_plane::instance_status().await {
    Ok(Some(instance)) => {
      instance.status != "ready"
        || instance
          .endpoint_id
          .is_some_and(|id| id != lease.endpoint_id)
    }
    _ => true,
  }
}

/// Stops renewal and relay-token sync for the current managed connection.
pub fn stop() {
  let mut tasks = TASKS.lock().unwrap();
  if let Some(task) = tasks.renewal.take() {
    task.abort();
  }
  if let Some(task) = tasks.relay_sync.take() {
    task.abort();
  }
  tasks.lease = None;
}

#[cfg(test)]
mod tests {
  use super::*;

  fn error(status: u16, message: &str) -> FunctionError {
    FunctionError {
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
}
