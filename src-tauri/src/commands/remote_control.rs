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

use crate::core::remote_ssh_transport::{CutoffReason, SshConnectionPool};
use crate::result_ext::ResultExt;
use serde::Serialize;
use std::sync::Arc;
use tauri::Emitter;

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
  app
    .emit(
      REMOTE_CUTOFF_EVENT,
      RemoteCutoffEvent {
        endpoint_id: endpoint_id.to_string(),
        reason: reason.into(),
      },
    )
    .ignore_closed();
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
    app
      .emit(
        REMOTE_CUTOFF_EVENT,
        RemoteCutoffEvent {
          endpoint_id,
          reason,
        },
      )
      .ignore_closed();
  }
}

#[cfg(test)]
mod tests {
  use crate::core::remote::{execute_local_request, TreqCommandRequest};

  #[test]
  fn dispatch_local_rejects_arbitrary_unlisted_shapes_at_the_type_boundary() {
    // The request is always a `TreqCommandRequest` variant, so a raw string
    // cannot reach `execute_local_request`; JSON deserialization of the enum
    // rejects an unknown tag rather than accepting it.
    let raw = serde_json::json!({ "kind": "ExecArbitraryShellCommand", "cmd": "rm -rf /" });
    let parsed = serde_json::from_value::<TreqCommandRequest>(raw);
    assert!(parsed.is_err());
  }

  #[test]
  fn dispatch_local_runs_a_real_probe_against_the_filesystem() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_str().unwrap().to_string();
    let value = execute_local_request(TreqCommandRequest::ProbeRepo { repo: path }).unwrap();
    assert_eq!(value["exists"], serde_json::Value::Bool(true));
    assert_eq!(value["is_repo"], serde_json::Value::Bool(false));
  }
}
