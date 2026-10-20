//! Tauri command surface for Phase 5 typed remote commands: probe/clone/init,
//! read-only review commands, remote mutations, and agent lifecycle.
//!
//! Every command here takes a fully-typed [`crate::core::remote::TreqCommandRequest`]
//! (allow-listed at the type level — Tauri's own JSON deserialization rejects
//! any shape that is not one of the enum's known variants) rather than a raw
//! command string, so the "no frontend-provided arbitrary command enters an
//! exec channel" requirement holds at the IPC boundary too, not only inside
//! the exec transport.
//!
//! `remote_dispatch_local` reuses exactly the same core dispatch
//! (`execute_local_request`) that the CLI itself calls, so results share one
//! DTO with the local Tauri path. `remote_dispatch_over_ssh` runs the same
//! typed request against a real `SshEndpoint` over the Phase 4 native
//! transport. Neither is wired into any component yet — that is Phase 6's
//! job — but both are real, callable, and tested.

use crate::core::remote::{MutationRetryOutcome, RemoteCommandError, TreqCommandRequest};
use crate::core::remote_control_plane::SshEndpoint;
use crate::core::remote_ssh_transport::{
  CancellationToken, CutoffReason, ExecLimits, SshConnectionPool,
};
use crate::AppState;
use serde::Serialize;
use std::sync::Arc;
use tauri::{Emitter, State};

/// Shared connection pool for Phase 5 typed SSH exec commands, so repeated
/// calls from the UI reuse one multiplexed connection per endpoint instead
/// of reconnecting on every command (PRD: "native SSH transport reuses a
/// connection for multiple structured commands").
pub struct RemoteExecState(pub Arc<SshConnectionPool>);

impl Default for RemoteExecState {
  fn default() -> Self {
    Self(Arc::new(SshConnectionPool::new()))
  }
}

/// Runs a typed request against the local machine, through the same
/// dispatch the CLI and the SSH exec channel both use.
#[tauri::command]
pub async fn remote_dispatch_local(
  request: TreqCommandRequest,
) -> Result<serde_json::Value, String> {
  tauri::async_runtime::spawn_blocking(move || crate::core::remote::execute_local_request(request))
    .await
    .map_err(|e| format!("Failed to join remote_dispatch_local task: {e}"))?
}

/// Runs a typed request against a real `SshEndpoint` over the pooled native
/// SSH transport, returning the raw JSON result. Errors preserve the CLI's
/// own structured code where available (see `RemoteCommandError`).
#[tauri::command]
pub async fn remote_dispatch_over_ssh(
  endpoint: SshEndpoint,
  request: TreqCommandRequest,
  state: State<'_, RemoteExecState>,
  app_state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
  crate::commands::feature_preview::require_remote_for_endpoint(&app_state, &endpoint)?;
  let pool = state.0.clone();
  let cancellation = CancellationToken::new();
  crate::core::remote::execute_remote_command::<serde_json::Value>(
    &pool,
    &endpoint,
    request,
    ExecLimits::default(),
    &cancellation,
  )
  .await
  .map_err(remote_command_error_to_string)
}

fn remote_command_error_to_string(error: RemoteCommandError) -> String {
  error.to_string()
}

/// Serializable mirror of [`CutoffReason`] for the Tauri IPC boundary and
/// the `remote://cutoff` event payload (PRD "Hard cutoff on revocation or
/// expiry": "the UI blocks further interaction with that instance behind a
/// reauthentication prompt").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CutoffReasonDto {
  SessionEnded,
  KeyRevoked,
  InstanceInaccessible,
  CertificateExpired,
}

impl From<CutoffReason> for CutoffReasonDto {
  fn from(reason: CutoffReason) -> Self {
    match reason {
      CutoffReason::SessionEnded => Self::SessionEnded,
      CutoffReason::KeyRevoked => Self::KeyRevoked,
      CutoffReason::InstanceInaccessible => Self::InstanceInaccessible,
      CutoffReason::CertificateExpired => Self::CertificateExpired,
    }
  }
}

impl From<CutoffReasonDto> for CutoffReason {
  fn from(reason: CutoffReasonDto) -> Self {
    match reason {
      CutoffReasonDto::SessionEnded => Self::SessionEnded,
      CutoffReasonDto::KeyRevoked => Self::KeyRevoked,
      CutoffReasonDto::InstanceInaccessible => Self::InstanceInaccessible,
      CutoffReasonDto::CertificateExpired => Self::CertificateExpired,
    }
  }
}

#[derive(Debug, Clone, Serialize)]
pub struct RemoteCutoffEvent {
  pub endpoint_id: String,
  pub reason: CutoffReasonDto,
}

/// Tauri event name emitted whenever an endpoint is forced into hard cutoff,
/// so the frontend can block interaction with that instance behind a
/// reauthentication prompt without polling.
pub const REMOTE_CUTOFF_EVENT: &str = "remote://cutoff";

/// Forces `endpoint_id` into the hard-cutoff state: tears down any open
/// exec/PTY channels for it and refuses further commands until
/// [`remote_clear_cutoff`] is called after reauthentication. Called by the
/// frontend's certificate-renewal loop when renewal is refused because the
/// Supabase session ended, the client key was revoked, or the instance is no
/// longer accessible — or when a certificate simply lapses unrenewed.
#[tauri::command]
pub async fn remote_force_cutoff(
  app: tauri::AppHandle,
  endpoint_id: String,
  reason: CutoffReasonDto,
) -> Result<(), String> {
  force_cutoff(&app, &endpoint_id, reason.into()).await;
  Ok(())
}

/// [`remote_force_cutoff`], for Rust callers (the certificate renewal loop).
pub async fn force_cutoff(app: &tauri::AppHandle, endpoint_id: &str, reason: CutoffReason) {
  use tauri::Manager;
  app
    .state::<RemoteExecState>()
    .0
    .force_cutoff(endpoint_id, reason)
    .await;
  // PRD "Hard cutoff on revocation or expiry": tears down open exec *and*
  // PTY channels for the endpoint. `force_cutoff` above only tears down the
  // pooled SSH connection itself; the PTY manager's own session map (which
  // holds a channel handle per session, not the pool) must be swept
  // separately so no PTY session for this endpoint outlives the cutoff.
  app
    .state::<crate::commands::remote_pty_commands::RemotePtyState>()
    .0
    .close_all_for_endpoint(endpoint_id)
    .await;
  let _ = app.emit(
    REMOTE_CUTOFF_EVENT,
    RemoteCutoffEvent {
      endpoint_id: endpoint_id.to_string(),
      reason: reason.into(),
    },
  );
}

/// Cuts off every managed endpoint on sign-out (PRD "Hard cutoff on
/// revocation or expiry": the Supabase session ended). Tears down pooled
/// managed connections and their PTY sessions, emits `remote://cutoff` for
/// each, and refuses new managed connections until the user signs in again.
#[tauri::command]
pub async fn remote_cut_off_managed(app: tauri::AppHandle) -> Result<(), String> {
  cut_off_managed(&app).await;
  Ok(())
}

/// [`remote_cut_off_managed`], for Rust callers (signing out).
pub async fn cut_off_managed(app: &tauri::AppHandle) {
  use tauri::Manager;
  let reason = CutoffReasonDto::SessionEnded;
  let ended = app
    .state::<RemoteExecState>()
    .0
    .cut_off_managed(reason.into())
    .await;
  let ptys = app.state::<crate::commands::remote_pty_commands::RemotePtyState>();
  for endpoint_id in ended {
    ptys.0.close_all_for_endpoint(&endpoint_id).await;
    let _ = app.emit(
      REMOTE_CUTOFF_EVENT,
      RemoteCutoffEvent {
        endpoint_id,
        reason,
      },
    );
  }
}

/// Clears a previously forced cutoff after the user reauthenticates and a
/// fresh certificate is issued through the normal registration and issuance
/// flow (PRD "The user regains access only by reauthenticating and obtaining
/// a new certificate").
#[tauri::command]
pub async fn remote_clear_cutoff(
  endpoint_id: String,
  state: State<'_, RemoteExecState>,
) -> Result<(), String> {
  state.0.clear_cutoff(&endpoint_id).await;
  Ok(())
}

/// Hands the native SSH pool the signed-in user's Supabase access token for
/// managed endpoints reached through the `remote-ssh-relay` Edge Function.
/// The frontend calls this on sign-in, on every token refresh, and with
/// `None` on sign-out. The token is kept in memory only and never logged.
#[tauri::command]
pub async fn remote_set_relay_access_token(
  token: Option<String>,
  state: State<'_, RemoteExecState>,
) -> Result<(), String> {
  state.0.set_relay_access_token(token);
  Ok(())
}

/// Frontend-facing shape of [`MutationRetryOutcome`], so the UI can render
/// each of the PRD's three post-reconnect cases (treat-as-complete,
/// retry-with-idempotency-key already performed, or surface-ambiguity) from
/// one typed response instead of inferring it from a thrown error string.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum MutationDispatchResult {
  /// The mutation's exec channel completed normally — whether on the first
  /// attempt or after a not-applied verdict triggered a same-idempotency-key
  /// retry. Carries the fresh response.
  Applied { value: serde_json::Value },
  /// A network failure interrupted the mutation, but post-reconnect state
  /// verification showed it had already applied. Nothing was resent.
  AlreadyApplied,
  /// A network failure interrupted the mutation and post-reconnect state
  /// verification could not determine whether it applied. The UI must ask
  /// the user rather than the client guessing.
  Ambiguous { reason: String },
}

/// Runs a typed *mutation* request against a real `SshEndpoint` with the
/// PRD's verify-before-retry behavior: a network failure while the command
/// was in flight never triggers a blind resend. See
/// `crate::core::remote::retry_after_reconnect` for the full decision logic.
/// Non-mutating (read/inspect) requests should keep using
/// `remote_dispatch_over_ssh`, which is always safe to retry.
#[tauri::command]
pub async fn remote_dispatch_mutation_over_ssh(
  endpoint: SshEndpoint,
  request: TreqCommandRequest,
  state: State<'_, RemoteExecState>,
  app_state: State<'_, AppState>,
) -> Result<MutationDispatchResult, String> {
  crate::commands::feature_preview::require_remote_for_endpoint(&app_state, &endpoint)?;
  let pool = state.0.clone();
  let cancellation = CancellationToken::new();
  let metrics = pool.metrics.clone();
  let outcome = crate::core::remote::retry_after_reconnect::<serde_json::Value, _>(
    &pool,
    &endpoint,
    request,
    ExecLimits::default(),
    &cancellation,
    move |verification| metrics.record_post_reconnect_verification(verification),
  )
  .await
  .map_err(remote_command_error_to_string)?;

  Ok(match outcome {
    MutationRetryOutcome::Applied(value) => MutationDispatchResult::Applied { value },
    MutationRetryOutcome::AlreadyApplied => MutationDispatchResult::AlreadyApplied,
    MutationRetryOutcome::Ambiguous { reason } => MutationDispatchResult::Ambiguous { reason },
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  #[tokio::test]
  async fn dispatch_local_rejects_arbitrary_unlisted_shapes_at_the_type_boundary() {
    // The request is always a `TreqCommandRequest` variant, so there is no
    // way to reach `remote_dispatch_local` with a raw string; the strongest
    // test we can run at this layer is that Tauri's own JSON deserialization
    // for the enum rejects an unknown tag rather than accepting it.
    let raw = serde_json::json!({ "kind": "ExecArbitraryShellCommand", "cmd": "rm -rf /" });
    let parsed = serde_json::from_value::<TreqCommandRequest>(raw);
    assert!(parsed.is_err());
  }

  #[tokio::test]
  async fn dispatch_local_runs_a_real_probe_against_the_filesystem() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_str().unwrap().to_string();
    let value = remote_dispatch_local(TreqCommandRequest::ProbeRepo { repo: path })
      .await
      .unwrap();
    assert_eq!(value["exists"], serde_json::Value::Bool(true));
    assert_eq!(value["is_repo"], serde_json::Value::Bool(false));
  }
}
