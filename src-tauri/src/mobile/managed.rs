//! Connecting to the user's Treq-managed instance, ported from the desktop
//! frontend's `useMobileRemoteConnection`, `managed-ssh-connection.ts` and
//! `remote-cert-lifecycle.ts`.
//!
//! The device's ed25519 key (OS keystore, behind biometrics) is registered
//! with the control plane, which issues a 20-minute SSH certificate for it.
//! The certificate is renewed at 80% of its lifetime; a renewal refusal cuts
//! the endpoint off until the user reauthenticates.

use crate::lock_ext::LockExt as _;
use std::{
  collections::HashMap,
  sync::{Arc, Mutex},
  time::Duration,
};

use crate::core::{
  control_plane,
  managed::{self as shared, renewal_delay_ms, Lease, READY_POLL, READY_TIMEOUT},
  remote_control_plane::{SshEndpoint, SshTransport},
  remote_ssh_transport::{CutoffReason, DEVICE_KEYSTORE_KEY_REFERENCE},
};
use async_trait::async_trait;

use crate::mobile::backend::{self, AppEvent, Connection, ConnectionKind};

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
  let info = crate::core::remote_device_key::ensure_device_key(&backend::get().app).await?;
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
  let (lease, endpoint) = issue(&instance.instance_id, false).await?;
  activate(lease, endpoint.clone()).await;
  Ok(Connection {
    endpoint,
    kind: ConnectionKind::Managed,
  })
}

/// Key ids the control plane registered for this device's keys, by
/// fingerprint. The id never changes for a key, so a connect skips the
/// registration call (an Edge Function round trip) after the first.
static REGISTERED_KEYS: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

/// Issues with the cached registration of `fingerprint` when there is one,
/// registering first otherwise. A failed issue with a cached id (the key
/// was revoked, or another account signed in) registers again and issues
/// once more. `fresh` ignores the cache, for reauthentication.
async fn issue_with_registered_key<T, Reg, RegFut, Iss, IssFut>(
  keys: &Mutex<Option<HashMap<String, String>>>,
  fingerprint: &str,
  fresh: bool,
  mut register: Reg,
  mut issue: Iss,
) -> Result<T, String>
where
  Reg: FnMut() -> RegFut,
  RegFut: std::future::Future<Output = Result<String, String>>,
  Iss: FnMut(String) -> IssFut,
  IssFut: std::future::Future<Output = Result<T, String>>,
{
  let cached = if fresh {
    None
  } else {
    keys
      .lock_or_recover()
      .as_ref()
      .and_then(|keys| keys.get(fingerprint).cloned())
  };
  if let Some(key_id) = cached {
    if let Ok(issued) = issue(key_id).await {
      return Ok(issued);
    }
  }
  let key_id = register().await?;
  keys
    .lock_or_recover()
    .get_or_insert_with(HashMap::new)
    .insert(fingerprint.to_string(), key_id.clone());
  issue(key_id).await
}

/// Registers the device key (once per key) and issues a certificate for it.
async fn issue(instance_id: &str, fresh: bool) -> Result<(Lease, SshEndpoint), String> {
  let supabase = &backend::get().supabase;
  let (public_key, fingerprint) = device_public_key().await?;
  issue_with_registered_key(
    &REGISTERED_KEYS,
    &fingerprint,
    fresh,
    || async {
      control_plane::register_client_key(
        supabase,
        &public_key,
        DEVICE_KEY_COMMENT,
        &format!("register:{fingerprint}"),
      )
      .await
      .map(|key| key.id)
      .map_err(|e| e.to_string())
    },
    |key_id| async move {
      shared::issue(
        supabase,
        instance_id,
        &key_id,
        DEVICE_KEYSTORE_KEY_REFERENCE,
      )
      .await
    },
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
    TASKS.lock_or_recover().relay_sync = Some(sync);
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
  let mut tasks = TASKS.lock_or_recover();
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
    TASKS.lock_or_recover().lease = Some(lease.clone());
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
    .lock_or_recover()
    .lease
    .as_ref()
    .map(|l| l.instance_id.clone())
    .or_else(|| {
      backend::get()
        .connection_to(backend::MANAGED_MACHINE)
        .and_then(|c| c.endpoint.instance_id)
    })
    .ok_or("This host is not a managed instance.")?;
  let (lease, endpoint) = issue(&instance_id, true).await?;
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
  let lease = TASKS.lock_or_recover().lease.clone();
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
  let mut tasks = TASKS.lock_or_recover();
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
  use std::sync::atomic::{AtomicUsize, Ordering};

  async fn run(
    keys: &Mutex<Option<HashMap<String, String>>>,
    fresh: bool,
    registrations: &AtomicUsize,
    valid_key: &'static str,
  ) -> Result<String, String> {
    issue_with_registered_key(
      keys,
      "fp",
      fresh,
      || async {
        registrations.fetch_add(1, Ordering::SeqCst);
        Ok(valid_key.to_string())
      },
      |key_id| async move {
        if key_id == valid_key {
          Ok(format!("cert for {key_id}"))
        } else {
          Err("unknown key".to_string())
        }
      },
    )
    .await
  }

  #[tokio::test]
  async fn registers_once_then_issues_with_the_cached_key() {
    let keys = Mutex::new(None);
    let registrations = AtomicUsize::new(0);
    run(&keys, false, &registrations, "k1").await.unwrap();
    run(&keys, false, &registrations, "k1").await.unwrap();
    assert_eq!(registrations.load(Ordering::SeqCst), 1);
  }

  #[tokio::test]
  async fn registers_again_when_the_cached_key_is_refused() {
    let keys = Mutex::new(None);
    let registrations = AtomicUsize::new(0);
    run(&keys, false, &registrations, "k1").await.unwrap();
    let issued = run(&keys, false, &registrations, "k2").await.unwrap();
    assert_eq!(issued, "cert for k2");
    assert_eq!(registrations.load(Ordering::SeqCst), 2);
    run(&keys, false, &registrations, "k2").await.unwrap();
    assert_eq!(registrations.load(Ordering::SeqCst), 2);
  }

  #[tokio::test]
  async fn reauthentication_always_registers() {
    let keys = Mutex::new(None);
    let registrations = AtomicUsize::new(0);
    run(&keys, false, &registrations, "k1").await.unwrap();
    run(&keys, true, &registrations, "k1").await.unwrap();
    assert_eq!(registrations.load(Ordering::SeqCst), 2);
  }
}
