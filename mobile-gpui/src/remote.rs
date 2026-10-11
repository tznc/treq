//! Typed commands to the connected host, through treq's SSH transport.
//!
//! Reads go through `execute_remote_command`; mutations through
//! `retry_after_reconnect`, which verifies state before any retry and never
//! blindly resends (see `core::remote`).

use std::collections::HashMap;

use serde::de::DeserializeOwned;
use treq_lib::core::{
  remote::{
    execute_remote_command, retry_after_reconnect, MutationRetryOutcome, TreqCommandRequest,
  },
  remote_ssh_transport::{CancellationToken, ExecLimits},
};

use crate::backend::{self, AppEvent};

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
  let backend = backend::get();
  let endpoint = backend.endpoint()?;
  let result = execute_remote_command::<T>(
    &backend.pool,
    &endpoint,
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
