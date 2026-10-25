//! Typed commands to the connected host, through treq's SSH transport.
//!
//! Reads go through `execute_remote_read`; mutations through
//! `retry_after_reconnect`, which verifies state before any retry and never
//! blindly resends (see `core::remote`).

use std::{collections::HashMap, future::Future, sync::LazyLock};

use serde::de::DeserializeOwned;
use treq_lib::core::{
  remote::{
    execute_remote_read, retry_after_reconnect, MutationRetryOutcome, RemoteHostCheck,
    TreqCommandRequest,
  },
  remote_control_plane::SshEndpoint,
  remote_ssh_transport::{CancellationToken, ExecLimits},
};

use crate::{
  backend::{self, AppEvent},
  reads::Reads,
};

static READS: LazyLock<Reads> = LazyLock::new(Reads::default);

/// Outcome of a mutation, as the UI reports it.
#[derive(Debug, Clone, PartialEq)]
pub enum Mutation {
  /// Applied now, or found already applied after a reconnect.
  Applied(serde_json::Value),
  /// A network failure hid whether it applied. Never retried blindly.
  Ambiguous(String),
}

const CUTOFF_PREFIX: &str = "credential_cut_off";

fn note_cutoff(error: &str) {
  if error.starts_with(CUTOFF_PREFIX) {
    if let Ok(endpoint) = backend::get().endpoint() {
      backend::get().emit(AppEvent::Cutoff {
        endpoint_id: endpoint.id,
        reason: Some("session ended".into()),
      });
    }
  }
}

pub async fn read<T: DeserializeOwned>(request: TreqCommandRequest) -> Result<T, String> {
  let endpoint = backend::get().endpoint()?;
  read_on(&endpoint, request).await
}

/// [`read`] on a given machine's endpoint, active or not.
pub async fn read_on<T: DeserializeOwned>(
  endpoint: &SshEndpoint,
  request: TreqCommandRequest,
) -> Result<T, String> {
  let backend = backend::get();
  // After the host restarts or the network changes, the pooled connection
  // is dead but not yet known to be. A read is safe to send again, on a
  // fresh connection (done once inside).
  let result = execute_remote_read::<T>(
    &backend.pool,
    endpoint,
    request,
    ExecLimits::default(),
    &CancellationToken::new(),
  )
  .await
  .map_err(|e| e.to_string());
  if let Err(error) = &result {
    note_cutoff(error);
  }
  result
}

/// Names `request` on `endpoint`. The address is part of it, so a host
/// edited to point elsewhere never shows the old machine's data.
fn read_key(endpoint: &SshEndpoint, request: &TreqCommandRequest) -> String {
  format!(
    "{}#{}#{}@{}:{}#{}",
    endpoint.id,
    crate::store::endpoint_generation(endpoint),
    endpoint.username,
    endpoint.hostname,
    endpoint.port,
    serde_json::to_string(request).unwrap_or_default()
  )
}

/// The last result of `request` on the active machine this session, to show
/// while it is read again.
pub fn last<T: Clone + 'static>(request: &TreqCommandRequest) -> Option<T> {
  let endpoint = backend::get().endpoint().ok()?;
  READS.last(&read_key(&endpoint, request))
}

/// [`read`], remembered for [`last`]. With `join`, the same read already in
/// flight (a prefetch) is awaited instead of sent again; reloads pass
/// `false`, so a result read before a change never stands for after it.
pub async fn read_cached<T>(request: TreqCommandRequest, join: bool) -> Result<T, String>
where
  T: DeserializeOwned + Clone + Send + Sync + 'static,
{
  let endpoint = backend::get().endpoint()?;
  read_cached_on(endpoint, request, join).await
}

/// [`read_cached`] on a given machine's endpoint, active or not.
pub fn read_cached_on<T>(
  endpoint: SshEndpoint,
  request: TreqCommandRequest,
  join: bool,
) -> impl Future<Output = Result<T, String>> + Send + 'static
where
  T: DeserializeOwned + Clone + Send + Sync + 'static,
{
  let key = read_key(&endpoint, &request);
  READS.fetch(key, join, move || async move {
    read_on::<T>(&endpoint, request).await
  })
}

/// Starts reading `request` now, for a screen about to open.
pub fn prefetch_on<T>(endpoint: SshEndpoint, request: TreqCommandRequest)
where
  T: DeserializeOwned + Clone + Send + Sync + 'static,
{
  backend::spawn_detached(async move {
    let _ = read_cached_on::<T>(endpoint, request, true).await;
  });
}

/// Forgets every remembered read (sign-out, a forgotten host).
pub fn forget_reads() {
  READS.clear();
}

/// What the machine can run. `None` when its treq predates host checks.
pub async fn host_check(endpoint: &SshEndpoint) -> Result<Option<RemoteHostCheck>, String> {
  match read_on::<RemoteHostCheck>(endpoint, TreqCommandRequest::HostCheck { root: "~".into() })
    .await
  {
    Ok(check) => Ok(Some(check)),
    Err(error) if error.contains("unknown repo action") || error.contains("unknown variant") => {
      Ok(None)
    }
    Err(error) => Err(error),
  }
}

pub async fn mutate(request: TreqCommandRequest) -> Result<Mutation, String> {
  let backend = backend::get();
  let endpoint = backend.endpoint()?;
  let pool = backend.pool.clone();
  let result = retry_after_reconnect::<serde_json::Value, _>(
    &pool,
    &endpoint,
    request,
    ExecLimits::default(),
    &CancellationToken::new(),
    |verification| {
      pool
        .metrics
        .record_post_reconnect_verification(verification)
    },
  )
  .await
  .map_err(|e| e.to_string());
  match result {
    Ok(MutationRetryOutcome::Applied(value)) => Ok(Mutation::Applied(value)),
    Ok(MutationRetryOutcome::AlreadyApplied) => Ok(Mutation::Applied(serde_json::Value::Null)),
    Ok(MutationRetryOutcome::Ambiguous { reason }) => Ok(Mutation::Ambiguous(reason)),
    Err(error) => {
      note_cutoff(&error);
      Err(error)
    }
  }
}

/// Idempotency keys for one screen's mutations, as in the desktop client's
/// `remote-idempotency.ts`: a key is reused for the same inputs until the
/// outcome is known, so a retry after an ambiguous result cannot apply twice.
#[derive(Default)]
pub struct IdempotencyKeys {
  keys: HashMap<String, String>,
}

impl IdempotencyKeys {
  pub fn key_for(&mut self, prefix: &str, inputs: &[&str]) -> String {
    let fingerprint = serde_json::to_string(&(prefix, inputs)).unwrap_or_default();
    self
      .keys
      .entry(fingerprint)
      .or_insert_with(|| format!("{prefix}:{}", uuid::Uuid::new_v4()))
      .clone()
  }

  /// Forgets the key once the outcome is known (anything but ambiguous).
  pub fn settle(&mut self, prefix: &str, inputs: &[&str], outcome: &Result<Mutation, String>) {
    if matches!(outcome, Ok(Mutation::Applied(_))) {
      let fingerprint = serde_json::to_string(&(prefix, inputs)).unwrap_or_default();
      self.keys.remove(&fingerprint);
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn keys_are_reused_until_settled() {
    let mut keys = IdempotencyKeys::default();
    let first = keys.key_for("commit", &["repo", "1", "msg"]);
    assert!(first.starts_with("commit:"));
    assert_eq!(keys.key_for("commit", &["repo", "1", "msg"]), first);
    assert_ne!(keys.key_for("commit", &["repo", "1", "other"]), first);

    keys.settle(
      "commit",
      &["repo", "1", "msg"],
      &Ok(Mutation::Ambiguous("x".into())),
    );
    assert_eq!(keys.key_for("commit", &["repo", "1", "msg"]), first);
    keys.settle(
      "commit",
      &["repo", "1", "msg"],
      &Ok(Mutation::Applied(serde_json::Value::Null)),
    );
    assert_ne!(keys.key_for("commit", &["repo", "1", "msg"]), first);
  }
}
