//! Connecting to the user's Treq-managed instance, ported from the desktop
//! frontend's `useMobileRemoteConnection`, `managed-ssh-connection.ts` and
//! `remote-cert-lifecycle.ts`.
//!
//! The device's ed25519 key (OS keystore, behind biometrics) is registered
//! with the control plane, which issues a 20-minute SSH certificate for it.
//! The certificate is renewed at 80% of its lifetime; a renewal refusal cuts
//! the endpoint off until the user reauthenticates.

use std::{
  sync::{Arc, Mutex},
  time::Duration,
};

use async_trait::async_trait;
use treq_lib::core::{
  control_plane,
  managed::{self as shared, renewal_delay_ms, Lease, READY_POLL, READY_TIMEOUT},
  remote_control_plane::{SshEndpoint, SshTransport},
  remote_ssh_transport::{CutoffReason, DEVICE_KEYSTORE_KEY_REFERENCE},
};

use crate::backend::{self, AppEvent, Connection, ConnectionKind};

pub const NO_MANAGED_INSTANCE: &str =
  "No managed instance for this account yet. Set one up from Treq on desktop, then connect here.";
pub const SECURE_STORAGE_PREFIX: &str = "secure_storage_unavailable:";
const DEVICE_KEY_COMMENT: &str = "treq-mobile-device";
const RELAY_TOKEN_SYNC: Duration = Duration::from_secs(60);

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
  let supabase = &backend::get().supabase;
  progress("Checking the managed instance");
  let mut instance = control_plane::instance_status(supabase)
    .await
    .map_err(|e| e.to_string())?
    .instance
    .ok_or_else(|| NO_MANAGED_INSTANCE.to_string())?;

  if instance.status == "suspended" {
    progress("Waking the managed instance");
    let key =
      control_plane::wake_idempotency_key(&instance.instance_id, &uuid::Uuid::new_v4().to_string());
    control_plane::wake(supabase, &instance.instance_id, &key)
      .await
      .map_err(|e| e.to_string())?;
    instance.status = "waking".into();
  }
  if instance.status != "ready" {
    progress("Waiting for the managed instance");
    shared::wait_for_ready(supabase, READY_POLL, READY_TIMEOUT, &|_| {}).await?;
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
  let supabase = &backend::get().supabase;
  let (public_key, fingerprint) = device_public_key().await?;
  let key = control_plane::register_client_key(
    supabase,
    &public_key,
    DEVICE_KEY_COMMENT,
    &format!("register:{fingerprint}"),
  )
  .await
  .map_err(|e| e.to_string())?;
  shared::issue(
    supabase,
    instance_id,
    &key.id,
    DEVICE_KEYSTORE_KEY_REFERENCE,
  )
  .await
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
  backend::get().put_connection(Connection {
    endpoint,
    kind: ConnectionKind::Managed,
  });
  let renewal = tokio::spawn(shared::renewal_loop(
    backend::get().supabase.clone(),
    lease.clone(),
    DEVICE_KEYSTORE_KEY_REFERENCE.to_string(),
    Arc::new(Hooks),
  ));
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

/// What the shared renewal loop does to this app's connection.
struct Hooks;

#[async_trait]
impl shared::RenewalHooks for Hooks {
  fn renewed(&self, lease: &Lease, endpoint: SshEndpoint) {
    backend::get().update_endpoint(endpoint);
    TASKS.lock().unwrap().lease = Some(lease.clone());
  }

  async fn cut_off(&self, lease: &Lease, reason: CutoffReason) {
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
  }
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
        .connection_to(backend::MANAGED_MACHINE)
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
  if renewal_delay_ms(lease.issued_at, lease.expires_at, shared::now_ms()) == 0 {
    return true;
  }
  match control_plane::instance_status(&backend::get().supabase)
    .await
    .map(|s| s.instance)
  {
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
