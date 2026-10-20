//! Tauri command surface for native remote PTY sessions (shell/agent) over
//! SSH — the product-facing API for `crate::core::remote_pty::RemotePtyManager`.
//! Mirrors `commands::pty_commands`'s event and error conventions so a
//! caller who already knows the local terminal API is not learning a second
//! vocabulary.

use crate::core::remote::TreqCommandRequest;
use crate::core::remote_control_plane::SshEndpoint;
use crate::core::remote_pty::{PtyLaunchSpec, RemotePtyBinding, RemotePtyError, RemotePtyManager};
use crate::core::remote_ssh_transport::{CancellationToken, ExecLimits};
use crate::pty::Utf8StreamDecoder;
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

/// Tauri event emitted with each output chunk from a remote PTY session,
/// named `remote-pty-data-<session_id>` — same per-session-suffixed naming
/// convention as the local `pty-data-<session_id>` event.
fn data_event_name(session_id: &str) -> String {
  format!("remote-pty-data-{session_id}")
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

/// Wraps a pair of text/exit sinks in the byte-level callbacks
/// `RemotePtyManager` expects, decoding output with one
/// [`Utf8StreamDecoder`] per session.
///
/// SSH delivers output in arbitrary chunks, so a multibyte character (an
/// emoji, a box-drawing glyph in a TUI) is often split across two chunks.
/// Decoding each chunk on its own turns both halves into replacement
/// characters. The decoder holds an incomplete trailing sequence until the
/// next chunk arrives, and flushes whatever is left before the exit event.
/// Both the fresh-session and the reattach paths go through this function
/// so they cannot drift apart again.
fn decoding_handlers<D, X>(
  emit_data: D,
  emit_exit: X,
) -> (
  impl Fn(Vec<u8>) + Send + 'static,
  impl FnOnce(Option<u32>) + Send + 'static,
)
where
  D: Fn(String) + Send + Sync + 'static,
  X: FnOnce(Option<u32>) + Send + 'static,
{
  let decoder = std::sync::Arc::new(std::sync::Mutex::new(Utf8StreamDecoder::new()));
  let emit_data = std::sync::Arc::new(emit_data);
  let data_decoder = decoder.clone();
  let data_sink = emit_data.clone();
  let on_output = move |chunk: Vec<u8>| {
    let decoded = data_decoder
      .lock()
      .unwrap_or_else(|e| e.into_inner())
      .push(&chunk);
    if !decoded.is_empty() {
      data_sink(decoded);
    }
  };
  let on_exit = move |exit_status: Option<u32>| {
    let trailing = decoder.lock().unwrap_or_else(|e| e.into_inner()).finish();
    if !trailing.is_empty() {
      emit_data(trailing);
    }
    emit_exit(exit_status);
  };
  (on_output, on_exit)
}

/// Callbacks that forward a session's decoded output and exit status as the
/// `remote-pty-data-<id>` / `remote-pty-exit-<id>` Tauri events. Never logs
/// raw terminal data (PRD "never log raw terminal output ... by default");
/// only the event name and session id appear in warnings.
fn tauri_event_handlers(
  app: tauri::AppHandle,
  session_id: String,
) -> (
  impl Fn(Vec<u8>) + Send + 'static,
  impl FnOnce(Option<u32>) + Send + 'static,
) {
  let data_event = data_event_name(&session_id);
  let exit_event = exit_event_name(&session_id);
  let app_for_data = app.clone();
  let sid_for_data = session_id.clone();
  decoding_handlers(
    move |text| {
      if let Err(error) = app_for_data.emit(&data_event, text) {
        log::warn!(
          "remote pty emit failed: session_id={}, event={}, error={}",
          sid_for_data,
          data_event,
          error
        );
      }
    },
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

/// `remote_pty_reattach`'s work, shared with the native terminal pane:
/// fetches the attach command, then opens the channel with it, emitting the
/// session's `remote-pty-data-<id>` / `remote-pty-exit-<id>` events.
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
  let attach_command = crate::core::remote::execute_remote_command::<String>(
    pool,
    &endpoint,
    TreqCommandRequest::PtyAttachCommand {
      repo: repository_id.clone(),
      workspace: workspace_id.clone(),
      label,
      remote_dir: remote_working_directory.clone(),
      launch,
      cols,
      rows,
    },
    ExecLimits::default(),
    &cancellation,
  )
  .await
  .map_err(|e| e.to_string())?;

  let binding = RemotePtyBinding {
    endpoint_id: endpoint.id.clone(),
    repository_id,
    workspace_id,
    remote_working_directory,
    local_session_id: session_id.clone(),
    window_label,
  };

  let (on_output, on_exit) = tauri_event_handlers(app, session_id);
  manager
    .create_with_command(
      binding,
      &endpoint,
      &attach_command,
      cols,
      rows,
      on_output,
      on_exit,
    )
    .await
    .map_err(remote_pty_error_to_string)
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::sync::{Arc, Mutex};

  #[test]
  fn decoding_handlers_keep_multibyte_characters_split_across_chunks() {
    let received = Arc::new(Mutex::new(String::new()));
    let exits = Arc::new(Mutex::new(Vec::new()));
    let sink = received.clone();
    let exit_sink = exits.clone();
    let (on_output, on_exit) = decoding_handlers(
      move |text| sink.lock().unwrap().push_str(&text),
      move |status| exit_sink.lock().unwrap().push(status),
    );

    let text = "ok ✓ 日本 🚀\n";
    // Feed one byte at a time so every multibyte character is split.
    for byte in text.as_bytes() {
      on_output(vec![*byte]);
    }
    on_exit(Some(0));

    assert_eq!(received.lock().unwrap().as_str(), text);
    assert_eq!(exits.lock().unwrap().as_slice(), &[Some(0)]);
  }

  #[test]
  fn decoding_handlers_flush_an_incomplete_tail_before_exit() {
    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    let data_events = events.clone();
    let exit_events = events.clone();
    let (on_output, on_exit) = decoding_handlers(
      move |text| data_events.lock().unwrap().push(text),
      move |_| exit_events.lock().unwrap().push("<exit>".to_string()),
    );

    // First two bytes of the three-byte "✓" and then the channel ends.
    on_output(vec![b'a', 0xE2, 0x9C]);
    on_exit(None);

    let events = events.lock().unwrap();
    assert_eq!(events[0], "a");
    assert_eq!(events[1], "\u{FFFD}");
    assert_eq!(events[2], "<exit>");
  }
}
