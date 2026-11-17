//! Tauri command surface for native remote PTY sessions (shell/agent) over
//! SSH — the product-facing API for `crate::core::remote_pty::RemotePtyManager`.
//! Mirrors `commands::pty_commands`'s event and error conventions so a
//! caller who already knows the local terminal API is not learning a second
//! vocabulary.

use crate::core::remote::TreqCommandRequest;
use crate::core::remote_control_plane::SshEndpoint;
use crate::core::remote_pty::{
  AttachCommandCache, PtyLaunchSpec, RemotePtyBinding, RemotePtyError, RemotePtyManager,
};
use crate::core::remote_ssh_transport::{CancellationToken, ExecLimits};
use serde::Serialize;
use tauri::Emitter;

/// Shared remote PTY session manager, keyed by the same `RemoteExecState`
/// connection pool used by structured exec dispatch, so shell/agent PTYs
/// reuse the endpoint's pooled SSH connection rather than opening a new one.
pub struct RemotePtyState(pub RemotePtyManager);

impl RemotePtyState {
  pub fn new(exec_state: &crate::commands::remote_control::RemoteExecState) -> Self {
    Self(RemotePtyManager::new(exec_state.0.clone()))
  }
}

/// Tauri event emitted exactly once when a remote PTY session's process
/// exits (or its channel otherwise ends), named `remote-pty-exit-<session_id>`.
fn exit_event_name(session_id: &str) -> String {
  format!("remote-pty-exit-{session_id}")
}

#[derive(Debug, Clone, Serialize)]
pub struct RemotePtyExitPayload {
  pub exit_status: Option<u32>,
}

fn remote_pty_error_to_string(error: RemotePtyError) -> String {
  error.to_string()
}

/// Callbacks that hand a session's output to its terminal in process
/// (`crate::pty_stream`) and emit its exit status as the
/// `remote-pty-exit-<id>` Tauri event. Never logs raw terminal data (PRD
/// "never log raw terminal output ... by default"); only the event name and
/// session id appear in warnings.
///
/// Output goes on as the bytes SSH delivered: the terminal's parser holds a
/// multibyte character split across chunks until the rest arrives.
fn tauri_event_handlers(
  app: tauri::AppHandle,
  session_id: String,
) -> (
  impl Fn(Vec<u8>) + Send + 'static,
  impl FnOnce(Option<u32>) + Send + 'static,
) {
  let exit_event = exit_event_name(&session_id);
  let data_id = session_id.clone();
  (
    move |chunk: Vec<u8>| crate::pty_stream::publish(&data_id, &chunk),
    move |exit_status| {
      if let Err(error) = app.emit(&exit_event, RemotePtyExitPayload { exit_status }) {
        log::warn!(
          "remote pty exit emit failed: session_id={}, event={}, error={}",
          session_id,
          exit_event,
          error
        );
      }
    },
  )
}

// -- Phase 8: desktop parity with mobile's persistent-session reattach ------
//
// Desktop already had a working single-session remote PTY (the commands
// above); what it lacked was any notion of a session surviving past this
// process's lifetime. These two commands wrap the same `pty-remote`
// VM-local supervisor (Phase 7, item 1) mobile uses, via the same generic
// `TreqCommandRequest` dispatch (`core::remote::execute_remote_command`)
// `remote_dispatch_over_ssh` already exposes — `PtyList`/`PtyStop`/
// `PtyStart` need no dedicated command at all for that reason, only
// `remote_dispatch_over_ssh` with the right request. `remote_pty_reattach`
// is the one operation that is not just "run a typed request and return
// JSON": it must also open a live PTY channel with the command that
// request returns, so it gets its own command here.

/// A persistent session to attach a local session id to.
pub struct Reattach {
  pub session_id: String,
  pub window_label: Option<String>,
  pub endpoint: SshEndpoint,
  pub repository_id: String,
  pub workspace_id: String,
  pub label: String,
  pub remote_working_directory: String,
  pub launch: PtyLaunchSpec,
  pub cols: u16,
  pub rows: u16,
}

/// Attach commands the host answered, kept for re-attaching.
static ATTACH_COMMANDS: std::sync::LazyLock<AttachCommandCache> =
  std::sync::LazyLock::new(AttachCommandCache::default);

/// `remote_pty_reattach`'s work, shared with the native terminal pane:
/// fetches the attach command, then opens the channel with it, handing its output
/// to `crate::pty_stream` and emitting its `remote-pty-exit-<id>` event.
pub async fn reattach(
  app: tauri::AppHandle,
  manager: &RemotePtyManager,
  pool: &std::sync::Arc<crate::core::remote_ssh_transport::SshConnectionPool>,
  request: Reattach,
) -> Result<(), String> {
  let Reattach {
    session_id,
    window_label,
    endpoint,
    repository_id,
    workspace_id,
    label,
    remote_working_directory,
    launch,
    cols,
    rows,
  } = request;
  let cancellation = CancellationToken::new();
  let attach_request = TreqCommandRequest::PtyAttachCommand {
    backend: endpoint.terminal_backend,
    repo: repository_id.clone(),
    workspace: workspace_id.clone(),
    label,
    remote_dir: remote_working_directory.clone(),
    launch,
    cols,
    rows,
  };
  // Re-attaching the same session skips asking the host for the command.
  let cache_key = AttachCommandCache::key(&endpoint, &attach_request);
  let attach_command = match ATTACH_COMMANDS.get(&cache_key) {
    Some(command) => command,
    None => {
      let command = crate::core::remote::execute_remote_command::<String>(
        pool,
        &endpoint,
        attach_request,
        ExecLimits::default(),
        &cancellation,
      )
      .await
      .map_err(|e| e.to_string())?;
      ATTACH_COMMANDS.insert(cache_key.clone(), command.clone());
      command
    }
  };

  let binding = RemotePtyBinding {
    endpoint_id: endpoint.id.clone(),
    repository_id,
    workspace_id,
    remote_working_directory,
    local_session_id: session_id.clone(),
    window_label,
  };

  let (on_output, on_exit) = tauri_event_handlers(app, session_id);
  let attached = manager
    .create_with_command(
      binding,
      &endpoint,
      &attach_command,
      cols,
      rows,
      on_output,
      on_exit,
    )
    .await;
  if attached.is_err() {
    // The host may have changed under the cached command.
    ATTACH_COMMANDS.forget(&cache_key);
  }
  attached.map_err(remote_pty_error_to_string)
}
