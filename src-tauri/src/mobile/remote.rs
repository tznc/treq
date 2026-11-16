//! Typed commands to the connected host, through treq's SSH transport.
//!
//! Reads go through `execute_remote_read`; mutations through
//! `retry_after_reconnect`, which verifies state before any retry and never
//! blindly resends (see `core::remote`).

use crate::{lock_ext::LockExt as _, result_ext::ResultExt as _};
use std::{collections::HashMap, future::Future, sync::LazyLock};

use crate::core::{
  github_direct,
  github_http::GithubHttp,
  github_user_token::GithubTokenError,
  remote::{
    execute_remote_read, retry_after_reconnect, verification_for, MutationRetryOutcome,
    MutationVerificationOutcome, RemoteHostCheck, TreqCommandRequest,
  },
  remote_control_plane::SshEndpoint,
  remote_ssh_transport::{CancellationToken, ExecLimits},
};
use serde::de::DeserializeOwned;

use crate::mobile::{
  backend::{self, AppEvent},
  reads::Reads,
};

static READS: LazyLock<Reads> = LazyLock::new(Reads::default);

/// GitHub `owner/name` by machine and repository path, as the host's git
/// remote names it (`None`: no GitHub remote).
static FULL_NAMES: LazyLock<std::sync::Mutex<HashMap<String, Option<String>>>> =
  LazyLock::new(Default::default);

/// [`github_direct`]'s view of a connected machine and the signed-in user.
struct HostDirect {
  endpoint: SshEndpoint,
}

#[async_trait::async_trait]
impl github_direct::DirectDeps for HostDirect {
  async fn full_name(&self, repo: &str) -> Result<Option<String>, String> {
    let key = read_key(
      &self.endpoint,
      &TreqCommandRequest::GitRemoteInfo {
        repo: repo.to_string(),
      },
    );
    if let Some(known) = FULL_NAMES.lock_or_recover().get(&key) {
      return Ok(known.clone());
    }
    let remote: Option<crate::github::GitRemoteInfo> = read_on_host(
      &self.endpoint,
      TreqCommandRequest::GitRemoteInfo {
        repo: repo.to_string(),
      },
    )
    .await?;
    let full_name = remote.map(|remote| remote.full_name);
    FULL_NAMES.lock_or_recover().insert(key, full_name.clone());
    Ok(full_name)
  }

  async fn token(&self) -> Result<String, GithubTokenError> {
    backend::get()
      .github_tokens
      .token(chrono::Utc::now().timestamp())
      .await
  }

  async fn forget_token(&self) {
    backend::get().github_tokens.forget().await;
  }

  fn client(&self, token: String) -> GithubHttp {
    GithubHttp::new(token)
  }
}

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

/// [`read`] on a given machine's endpoint, active or not. A GitHub read
/// goes straight to GitHub as the signed-in user when it can
/// ([`crate::core::github_direct`]), and to the host's `gh` otherwise.
pub async fn read_on<T: DeserializeOwned>(
  endpoint: &SshEndpoint,
  request: TreqCommandRequest,
) -> Result<T, String> {
  if let TreqCommandRequest::GithubRead { repo, query } = &request {
    let deps = HostDirect {
      endpoint: endpoint.clone(),
    };
    if let Some(result) = github_direct::read(&deps, repo, query.clone()).await {
      return result.and_then(|value| serde_json::from_value(value).map_err(|e| e.to_string()));
    }
  }
  read_on_host(endpoint, request).await
}

async fn read_on_host<T: DeserializeOwned>(
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
    crate::mobile::store::endpoint_generation(endpoint),
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
    read_cached_on::<T>(endpoint, request, true)
      .await
      .best_effort();
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
  if let TreqCommandRequest::GithubWrite { repo, action, .. } = &request {
    let deps = HostDirect {
      endpoint: endpoint.clone(),
    };
    if let Some(result) = github_direct::write(&deps, repo, action.clone()).await {
      return result.map(Mutation::Applied);
    }
  }
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

/// "Check again" after an ambiguous result: reads the state the mutation
/// would have changed and decides from it, without sending the mutation
/// again. Its idempotency key stays unsettled, so a retry the user starts
/// after a "did not apply" still cannot apply twice.
pub async fn check_again(request: TreqCommandRequest) -> Result<Mutation, String> {
  let endpoint = backend::get().endpoint()?;
  check_with(&request, |read| async move {
    read_on::<serde_json::Value>(&endpoint, read).await
  })
  .await
}

/// [`check_again`] with the read supplied, for tests.
pub async fn check_with<F, Fut>(request: &TreqCommandRequest, read: F) -> Result<Mutation, String>
where
  F: FnOnce(TreqCommandRequest) -> Fut,
  Fut: Future<Output = Result<serde_json::Value, String>>,
{
  let Some(verification) = verification_for(request) else {
    return Ok(Mutation::Ambiguous(
      "the host has no state check for this change. The page shows its current state".into(),
    ));
  };
  let value = read(verification.read_request).await?;
  match (verification.check)(&value) {
    MutationVerificationOutcome::AlreadyApplied => Ok(Mutation::Applied(serde_json::Value::Null)),
    MutationVerificationOutcome::NotApplied => {
      Err("The change did not apply. Try it again.".into())
    }
    MutationVerificationOutcome::Ambiguous => Ok(Mutation::Ambiguous(
      "the host's state still does not show whether it applied".into(),
    )),
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

  /// "Check again" after an ambiguous result runs the mutation's
  /// verification read, never the mutation itself.
  #[test]
  fn check_again_reads_state_instead_of_resending() {
    let delete = TreqCommandRequest::DeleteWorkspace {
      repo: "/r".into(),
      workspace: "7".into(),
    };
    let sent = std::cell::RefCell::new(Vec::new());
    let read = |request: TreqCommandRequest, value: serde_json::Value| {
      sent.borrow_mut().push(request.is_mutation());
      async move { Ok::<_, String>(value) }
    };
    let gone = futures::executor::block_on(check_with(&delete, |r| read(r, serde_json::json!([]))));
    assert_eq!(gone, Ok(Mutation::Applied(serde_json::Value::Null)));
    let still = futures::executor::block_on(check_with(&delete, |r| {
      read(r, serde_json::json!([{ "id": 7 }]))
    }));
    assert!(still.is_err(), "not applied: {still:?}");
    assert_eq!(*sent.borrow(), vec![false, false]);

    // With no state check for the request, nothing is sent at all.
    let push = TreqCommandRequest::GitPush {
      repo: "/r".into(),
      workspace: Some("7".into()),
      idempotency_key: "push:1".into(),
    };
    let unknown =
      futures::executor::block_on(check_with(&push, |r| read(r, serde_json::Value::Null)));
    assert!(matches!(unknown, Ok(Mutation::Ambiguous(_))));
    assert_eq!(sent.borrow().len(), 2);
  }

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
