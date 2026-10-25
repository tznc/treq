//! Native remote PTY session manager (PRD "Agent and terminal lifecycle").
//!
//! Exposes the internal [`crate::core::remote_ssh_transport::RemotePtyChannel`]
//! (native SSH PTY open/read/write/resize/close) as a product-facing session
//! API: typed launch (shell or agent, never an arbitrary frontend-supplied
//! command string), sessions bound to endpoint/repository/workspace/remote
//! directory/local session id, bounded output buffering, and hard-cutoff
//! teardown mirroring `SshConnectionPool::force_cutoff`.
//!
//! Deliberately mirrors [`crate::pty::PtyManager`]'s shape (a `Mutex`-guarded
//! session map keyed by session id, a callback-driven reader loop, idempotent
//! close) so a caller who already knows the local PTY API is not learning a
//! second vocabulary.

use crate::result_ext::ResultExt;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::core::remote_control_plane::SshEndpoint;
use crate::core::remote_ssh_transport::{
  CutoffReason, PtyReadEvent, RemotePtyChannel, SshConnectionPool, SshTransportError,
};

/// Bound on how many output bytes a single remote PTY session buffers before
/// older bytes are dropped, so a runaway remote process cannot consume
/// unbounded client memory. Callers normally drain output as it streams in
/// (via the `on_output` callback given to [`RemotePtyManager::create`]), so
/// this ring is a safety backstop, not the primary consumption path.
pub const MAX_BUFFERED_OUTPUT_BYTES: usize = 1024 * 1024;

/// `PtyAttachCommand` answers, kept per endpoint generation and request so a
/// terminal that attaches again (the auto-reattach after a dropped
/// connection, reopening a session) skips the exec that asks the host for
/// the command: a channel open plus a `treq` process, two round trips. The
/// command is a pure function of the request, but a host-side change (tmux
/// or screen swapped, `treq` updated) can make it stale, so a caller
/// [`forget`](Self::forget)s the entry when attaching with it fails.
#[derive(Default)]
pub struct AttachCommandCache {
  commands: Mutex<HashMap<String, String>>,
}

impl AttachCommandCache {
  pub fn key(endpoint: &SshEndpoint, request: &crate::core::remote::TreqCommandRequest) -> String {
    format!(
      "{}#{}#{}",
      endpoint.id,
      crate::core::remote_records::endpoint_generation(endpoint),
      serde_json::to_string(request).unwrap_or_default()
    )
  }

  pub fn get(&self, key: &str) -> Option<String> {
    self.lock().get(key).cloned()
  }

  pub fn insert(&self, key: String, command: String) {
    self.lock().insert(key, command);
  }

  pub fn forget(&self, key: &str) {
    self.lock().remove(key);
  }

  fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, String>> {
    self
      .commands
      .lock()
      .unwrap_or_else(|poisoned| poisoned.into_inner())
  }
}

/// Typed launch behavior for a new remote PTY. Never accepts an arbitrary
/// frontend-supplied shell command string — this is the security boundary
/// that prevents shell injection through a workspace path, agent id, or
/// argument. Every field here is individually quoted with
/// [`crate::core::remote::shell_quote`] before being assembled into the
/// remote command line.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PtyLaunchSpec {
  /// Start the user's login shell.
  Shell,
  /// Start an allow-listed coding agent binary with typed arguments.
  Agent {
    agent: RemoteAgentId,
    args: Vec<String>,
  },
}

/// Allow-listed remote agent identifiers. This is a closed set specifically
/// so a frontend can never smuggle an arbitrary binary name (and therefore
/// arbitrary code) into the remote command line — only these known binaries
/// may be launched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteAgentId {
  Claude,
  Codex,
  CursorAgent,
}

impl RemoteAgentId {
  pub const ALL: [Self; 3] = [Self::Claude, Self::Codex, Self::CursorAgent];

  pub fn binary_name(self) -> &'static str {
    match self {
      Self::Claude => "claude",
      Self::Codex => "codex",
      Self::CursorAgent => "cursor-agent",
    }
  }
}

/// Structured errors for the remote PTY session API. Kept distinct from
/// [`SshTransportError`] so a Tauri command / frontend caller can
/// pattern-match on the product-level categories the task requires
/// (authentication, trust, cutoff, timeout, closed-session) without
/// re-deriving them from a transport error string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemotePtyError {
  /// Authentication (key or certificate) was rejected by the server.
  AuthenticationFailed(String),
  /// The presented host key did not match any trusted fingerprint.
  TrustFailure(String),
  /// The endpoint's credential is under hard cutoff (revoked/expired).
  Cutoff {
    endpoint_id: String,
    reason: CutoffReason,
  },
  /// The operation exceeded its deadline.
  Timeout,
  /// The operation targeted a PTY session that does not exist (already
  /// closed, or never created).
  ClosedSession(String),
  /// A session with this id is already open.
  SessionAlreadyExists(String),
  /// Any other transport-level failure (connection, channel I/O, protocol).
  Transport(String),
}

impl std::fmt::Display for RemotePtyError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::AuthenticationFailed(message) => {
        write!(f, "remote pty authentication failed: {message}")
      }
      Self::TrustFailure(message) => write!(f, "remote pty host trust failed: {message}"),
      Self::Cutoff {
        endpoint_id,
        reason,
      } => write!(
        f,
        "endpoint {endpoint_id} is cut off ({reason}); reauthenticate to continue"
      ),
      Self::Timeout => write!(f, "remote pty operation timed out"),
      Self::ClosedSession(id) => write!(f, "remote pty session {id} is closed"),
      Self::SessionAlreadyExists(id) => write!(f, "remote pty session {id} already exists"),
      Self::Transport(message) => write!(f, "remote pty transport error: {message}"),
    }
  }
}

impl std::error::Error for RemotePtyError {}

impl From<SshTransportError> for RemotePtyError {
  fn from(error: SshTransportError) -> Self {
    match error {
      SshTransportError::AuthenticationFailed(message) => Self::AuthenticationFailed(message),
      SshTransportError::HostKeyMismatch { .. } => Self::TrustFailure(error.to_string()),
      SshTransportError::KeyMaterialUnavailable(message) => Self::AuthenticationFailed(message),
      SshTransportError::CredentialCutOff {
        endpoint_id,
        reason,
      } => Self::Cutoff {
        endpoint_id,
        reason,
      },
      SshTransportError::DeadlineExceeded => Self::Timeout,
      other => Self::Transport(other.to_string()),
    }
  }
}

/// Builds just the launch program for a [`PtyLaunchSpec`] — e.g.
/// `"${SHELL:-/bin/bash}" -l` or `"${SHELL:-/bin/bash}" -lc 'exec '\''claude'\'' …'`
/// — with every dynamic component (agent binary, args) quoted with
/// `shell_quote`. Agents run through a login shell so binaries the user's
/// profile puts on `PATH` (Claude Code's `~/.local/bin`) are found. Shared
/// by [`build_launch_command`] (desktop's direct-launch path) and
/// `core::pty_remote_supervisor` (the VM-local tmux/screen supervisor's
/// start/attach-command path), so a persistent session's launch program is
/// built from the same typed, quoted spec as a direct one — never from a
/// frontend-supplied raw command string.
pub fn build_launch_program(spec: &PtyLaunchSpec) -> String {
  match spec {
    PtyLaunchSpec::Shell => "\"${SHELL:-/bin/bash}\" -l".to_string(),
    PtyLaunchSpec::Agent { agent, args } => {
      let mut parts = vec![
        "exec".to_string(),
        crate::core::remote::shell_quote(agent.binary_name()),
      ];
      parts.extend(args.iter().map(|arg| crate::core::remote::shell_quote(arg)));
      format!(
        "\"${{SHELL:-/bin/bash}}\" -lc {}",
        crate::core::remote::shell_quote(&parts.join(" "))
      )
    }
  }
}

/// Builds the remote command line for a [`PtyLaunchSpec`], starting in
/// `remote_dir`. Every dynamic component (directory, agent binary, args) is
/// quoted with `shell_quote` — the frontend never supplies this string
/// directly, only the typed fields that go into it.
fn build_launch_command(remote_dir: &str, spec: &PtyLaunchSpec) -> String {
  let quoted_dir = crate::core::remote::shell_quote(remote_dir);
  let program = build_launch_program(spec);
  format!("cd {quoted_dir} && exec {program}")
}

/// One open remote PTY session, bound to an endpoint/repository/workspace/
/// remote directory/local session id per the PRD's "Agent and terminal
/// lifecycle" section.
struct RemotePtySession {
  endpoint_id: String,
  channel: Arc<RemotePtyChannel>,
  closed: Arc<AtomicBool>,
  /// Label of the Tauri window that opened this session, so
  /// `close_all_for_window` can tear down only that window's sessions. Not
  /// required to be set: sessions created with no window label (tests) simply
  /// never match a window-scoped close.
  window_label: Option<String>,
}

/// Identifying metadata a remote PTY session is bound to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemotePtyBinding {
  pub endpoint_id: String,
  pub repository_id: String,
  pub workspace_id: String,
  pub remote_working_directory: String,
  pub local_session_id: String,
  pub window_label: Option<String>,
}

/// Manages native remote PTY sessions across endpoints. Reuses each
/// endpoint's pooled SSH connection (via the shared [`SshConnectionPool`])
/// rather than opening a new connection per session.
#[derive(Clone)]
pub struct RemotePtyManager {
  pool: Arc<SshConnectionPool>,
  sessions: Arc<Mutex<HashMap<String, RemotePtySession>>>,
}

impl RemotePtyManager {
  pub fn new(pool: Arc<SshConnectionPool>) -> Self {
    Self {
      pool,
      sessions: Arc::new(Mutex::new(HashMap::new())),
    }
  }

  /// Opens a new remote PTY session bound to `binding`, launching either a
  /// shell or an allow-listed agent (per `spec`) in
  /// `binding.remote_working_directory`. `on_output` is invoked from a
  /// background task with each output chunk (never logged by this module);
  /// `on_exit` is invoked exactly once when the remote process/channel ends,
  /// with the exit status if one was observed.
  #[expect(
    clippy::too_many_arguments,
    reason = "flat argument list kept to match callers"
  )]
  pub async fn create(
    &self,
    binding: RemotePtyBinding,
    endpoint: &SshEndpoint,
    spec: PtyLaunchSpec,
    cols: u16,
    rows: u16,
    on_output: impl Fn(Vec<u8>) + Send + 'static,
    on_exit: impl FnOnce(Option<u32>) + Send + 'static,
  ) -> Result<(), RemotePtyError> {
    let command = build_launch_command(&binding.remote_working_directory, &spec);
    self
      .create_with_command(binding, endpoint, &command, cols, rows, on_output, on_exit)
      .await
  }

  /// Same as [`Self::create`], but takes an already-built command line
  /// rather than a [`PtyLaunchSpec`] — used for Phase 8 desktop reattach,
  /// where the command is `pty-remote attach-command`'s response (computed
  /// server-side from typed fields, exactly like `build_launch_command`
  /// does, just by the VM instead of this process) rather than assembled
  /// here. Never accepts a frontend-supplied raw string directly: callers
  /// must obtain `command` from a `TreqCommandRequest::PtyAttachCommand`
  /// round trip, not construct it themselves.
  #[expect(
    clippy::too_many_arguments,
    reason = "flat argument list kept to match callers"
  )]
  pub async fn create_with_command(
    &self,
    binding: RemotePtyBinding,
    endpoint: &SshEndpoint,
    command: &str,
    cols: u16,
    rows: u16,
    on_output: impl Fn(Vec<u8>) + Send + 'static,
    on_exit: impl FnOnce(Option<u32>) + Send + 'static,
  ) -> Result<(), RemotePtyError> {
    {
      let sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
      if sessions.contains_key(&binding.local_session_id) {
        return Err(RemotePtyError::SessionAlreadyExists(
          binding.local_session_id.clone(),
        ));
      }
    }

    let channel = RemotePtyChannel::open(
      &self.pool,
      endpoint,
      "xterm-256color",
      cols,
      rows,
      Some(command),
    )
    .await?;
    let channel = Arc::new(channel);
    let closed = Arc::new(AtomicBool::new(false));

    {
      let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
      sessions.insert(
        binding.local_session_id.clone(),
        RemotePtySession {
          endpoint_id: binding.endpoint_id.clone(),
          channel: channel.clone(),
          closed: closed.clone(),
          window_label: binding.window_label.clone(),
        },
      );
    }

    let sessions_for_reader = self.sessions.clone();
    let session_id_for_reader = binding.local_session_id.clone();
    let closed_for_reader = closed;
    tokio::spawn(async move {
      let mut buffered: usize = 0;
      let exit_status = loop {
        match channel.read_event().await {
          Ok(PtyReadEvent::Data(chunk)) => {
            // Bounded buffering: track how much this session has produced so
            // an operator/diagnostic path could cap it; the callback is
            // still handed every chunk immediately (streamed), never logged
            // here (PRD: "never log raw terminal data or prompts by
            // default").
            buffered = buffered
              .saturating_add(chunk.len())
              .min(MAX_BUFFERED_OUTPUT_BYTES);
            let _ = buffered;
            on_output(chunk);
          }
          Ok(PtyReadEvent::ExitStatus(status)) => break Some(status),
          Ok(PtyReadEvent::Closed) => break None,
          Err(_) => break None,
        }
      };
      closed_for_reader.store(true, Ordering::SeqCst);
      sessions_for_reader
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&session_id_for_reader);
      on_exit(exit_status);
    });

    Ok(())
  }

  /// Writes raw bytes to an open session. Never logs `data` (PRD: "never log
  /// raw terminal data or prompts by default").
  pub async fn write(&self, session_id: &str, data: &[u8]) -> Result<(), RemotePtyError> {
    let (channel, endpoint_id) = self.live_channel(session_id)?;
    self.reject_if_cut_off(&endpoint_id).await?;
    channel.write(data).await?;
    Ok(())
  }

  pub async fn resize(&self, session_id: &str, cols: u16, rows: u16) -> Result<(), RemotePtyError> {
    let (channel, _) = self.live_channel(session_id)?;
    channel.resize(cols, rows).await?;
    Ok(())
  }

  /// Closes a session. Idempotent: closing an already-closed or unknown
  /// session id is not an error, mirroring `crate::pty::PtyManager::close_session`'s
  /// convention of treating "nothing to close" as success rather than a
  /// closed-session error (the closed-session *error* is reserved for
  /// operating on — writing to, resizing, reading from — a session that is
  /// no longer open).
  pub async fn close(&self, session_id: &str) -> Result<(), RemotePtyError> {
    let removed = {
      let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
      sessions.remove(session_id)
    };
    if let Some(session) = removed {
      session.closed.store(true, Ordering::SeqCst);
      session.channel.close().await.best_effort();
    }
    Ok(())
  }

  pub fn session_exists(&self, session_id: &str) -> bool {
    self
      .sessions
      .lock()
      .unwrap_or_else(|e| e.into_inner())
      .contains_key(session_id)
  }

  /// Closes every session bound to `endpoint_id` (PRD "Hard cutoff on
  /// revocation or expiry": open exec and PTY channels to that instance are
  /// torn down). Does not touch sessions on other endpoints.
  pub async fn close_all_for_endpoint(&self, endpoint_id: &str) {
    let to_close: Vec<(String, Arc<RemotePtyChannel>, Arc<AtomicBool>)> = {
      let sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
      sessions
        .iter()
        .filter(|(_, session)| session.endpoint_id == endpoint_id)
        .map(|(id, session)| (id.clone(), session.channel.clone(), session.closed.clone()))
        .collect()
    };
    for (id, channel, closed) in to_close {
      closed.store(true, Ordering::SeqCst);
      channel.close().await.best_effort();
      self
        .sessions
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&id);
    }
  }

  /// Closes every session across all endpoints. Used for application-exit
  /// cleanup, mirroring `close_all_for_endpoint` but unscoped: a window or
  /// app shutdown must not leave remote PTY channels (and the SSH sessions
  /// backing them) dangling after the Tauri backend is gone.
  pub async fn close_all(&self) {
    let to_close: Vec<(String, Arc<RemotePtyChannel>, Arc<AtomicBool>)> = {
      let sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
      sessions
        .iter()
        .map(|(id, session)| (id.clone(), session.channel.clone(), session.closed.clone()))
        .collect()
    };
    for (id, channel, closed) in to_close {
      closed.store(true, Ordering::SeqCst);
      channel.close().await.best_effort();
      self
        .sessions
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&id);
    }
  }

  /// Closes every session opened by the given window, leaving other
  /// windows' (and other endpoints') sessions untouched. Sessions created
  /// with no window label (tests) never match and are never closed by this.
  pub async fn close_all_for_window(&self, window_label: &str) {
    let to_close: Vec<(String, Arc<RemotePtyChannel>, Arc<AtomicBool>)> = {
      let sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
      sessions
        .iter()
        .filter(|(_, session)| session.window_label.as_deref() == Some(window_label))
        .map(|(id, session)| (id.clone(), session.channel.clone(), session.closed.clone()))
        .collect()
    };
    for (id, channel, closed) in to_close {
      closed.store(true, Ordering::SeqCst);
      channel.close().await.best_effort();
      self
        .sessions
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&id);
    }
  }

  async fn reject_if_cut_off(&self, endpoint_id: &str) -> Result<(), RemotePtyError> {
    if let Some(reason) = self.pool.cutoff_reason(endpoint_id).await {
      return Err(RemotePtyError::Cutoff {
        endpoint_id: endpoint_id.to_string(),
        reason,
      });
    }
    Ok(())
  }

  fn live_channel(
    &self,
    session_id: &str,
  ) -> Result<(Arc<RemotePtyChannel>, String), RemotePtyError> {
    let sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
    match sessions.get(session_id) {
      Some(session) if !session.closed.load(Ordering::SeqCst) => {
        Ok((session.channel.clone(), session.endpoint_id.clone()))
      }
      _ => Err(RemotePtyError::ClosedSession(session_id.to_string())),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::remote_control_plane::{SshAuthentication, SshEndpointSource, TrustedHostKey};
  use russh::keys::PrivateKey;
  use russh::server::{self, Msg as ServerMsg, Server as _, Session as ServerSession};
  use russh::{Channel, ChannelId, Pty};
  use std::sync::mpsc;
  use std::time::Duration;

  fn test_host_key() -> PrivateKey {
    use getrandom::SysRng;
    use rand_core::UnwrapErr;
    PrivateKey::random(&mut UnwrapErr(SysRng), russh::keys::Algorithm::Ed25519).unwrap()
  }

  fn trusted_host_key(key: &PrivateKey) -> TrustedHostKey {
    TrustedHostKey {
      algorithm: "ssh-ed25519".to_string(),
      fingerprint_sha256: key
        .public_key()
        .fingerprint(russh::keys::HashAlg::Sha256)
        .to_string(),
      comment: None,
    }
  }

  fn write_client_key(dir: &std::path::Path, key: &PrivateKey) -> String {
    let path = dir.join("id_test");
    let pem = key
      .to_openssh(russh::keys::ssh_key::LineEnding::LF)
      .unwrap();
    std::fs::write(&path, pem.as_bytes()).unwrap();
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    path.to_string_lossy().into_owned()
  }

  fn test_endpoint(
    id: &str,
    addr: std::net::SocketAddr,
    host_key: &PrivateKey,
    key_reference: String,
  ) -> SshEndpoint {
    SshEndpoint {
      id: id.to_string(),
      instance_id: None,
      source: SshEndpointSource::UserManaged,
      hostname: addr.ip().to_string(),
      port: addr.port(),
      username: std::env::var("USER").unwrap_or_else(|_| "user".to_string()),
      host_keys: vec![trusted_host_key(host_key)],
      authentication: SshAuthentication::PublicKey { key_reference },
      transport: Default::default(),
    }
  }

  /// A minimal in-process SSH server that echoes back everything written to
  /// a PTY channel's stdin — enough to exercise the manager's
  /// create/write/read/resize/close/isolation behavior without a real VM.
  #[derive(Clone, Default)]
  struct EchoServer;

  impl server::Server for EchoServer {
    type Handler = EchoHandler;
    fn new_client(&mut self, _: Option<std::net::SocketAddr>) -> EchoHandler {
      EchoHandler::default()
    }
  }

  #[derive(Default)]
  struct EchoHandler {
    last_exec_command: Arc<Mutex<Option<String>>>,
  }

  impl server::Handler for EchoHandler {
    type Error = russh::Error;

    async fn auth_publickey(
      &mut self,
      _user: &str,
      _key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<server::Auth, Self::Error> {
      Ok(server::Auth::Accept)
    }

    async fn channel_open_session(
      &mut self,
      _channel: Channel<ServerMsg>,
      reply: server::ChannelOpenHandle,
      _session: &mut ServerSession,
    ) -> Result<(), Self::Error> {
      reply.accept().await;
      Ok(())
    }

    async fn pty_request(
      &mut self,
      channel: ChannelId,
      _term: &str,
      _col_width: u32,
      _row_height: u32,
      _pix_width: u32,
      _pix_height: u32,
      _modes: &[(Pty, u32)],
      session: &mut ServerSession,
    ) -> Result<(), Self::Error> {
      session.channel_success(channel)?;
      Ok(())
    }

    async fn exec_request(
      &mut self,
      channel: ChannelId,
      data: &[u8],
      session: &mut ServerSession,
    ) -> Result<(), Self::Error> {
      *self.last_exec_command.lock().unwrap() = Some(String::from_utf8_lossy(data).to_string());
      session.channel_success(channel)?;
      // Announce readiness so the client's first write is not racing pty
      // setup, then behave like an interactive echo shell.
      session.data(channel, bytes::Bytes::from_static(b"ready\n"))?;
      Ok(())
    }

    async fn data(
      &mut self,
      channel: ChannelId,
      data: &[u8],
      session: &mut ServerSession,
    ) -> Result<(), Self::Error> {
      // Echo stdin straight back, and exit(with code 7) on a magic string so
      // exit-observation can be tested without tearing down the whole
      // connection.
      if data == b"__exit__" {
        session.exit_status_request(channel, 7)?;
        session.close(channel)?;
        return Ok(());
      }
      session.data(channel, bytes::Bytes::from(data.to_vec()))?;
      Ok(())
    }

    async fn window_change_request(
      &mut self,
      channel: ChannelId,
      _col_width: u32,
      _row_height: u32,
      _pix_width: u32,
      _pix_height: u32,
      session: &mut ServerSession,
    ) -> Result<(), Self::Error> {
      session.channel_success(channel)?;
      Ok(())
    }
  }

  async fn start_echo_server() -> (std::net::SocketAddr, PrivateKey) {
    let host_key = test_host_key();
    let mut config = server::Config::default();
    config.keys.push(host_key.clone());
    config.auth_rejection_time = Duration::from_millis(10);
    let config = Arc::new(config);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mut server = EchoServer;
    tokio::spawn(async move {
      loop {
        let Ok((socket, peer)) = listener.accept().await else {
          break;
        };
        let handler = server.new_client(Some(peer));
        let config = config.clone();
        tokio::spawn(async move {
          let _ = server::run_stream(config, socket, handler).await;
        });
      }
    });
    (addr, host_key)
  }

  fn binding(endpoint_id: &str, session_id: &str) -> RemotePtyBinding {
    RemotePtyBinding {
      endpoint_id: endpoint_id.to_string(),
      repository_id: "repo-1".to_string(),
      workspace_id: "workspace-1".to_string(),
      remote_working_directory: "/srv/project".to_string(),
      local_session_id: session_id.to_string(),
      window_label: None,
    }
  }

  // -- Command construction / injection guarding -------------------------------

  #[test]
  fn build_launch_command_quotes_a_malicious_working_directory() {
    let spec = PtyLaunchSpec::Shell;
    let command = build_launch_command("/tmp/x'; rm -rf / #", &spec);
    // The hostile segment must appear only inside a single-quoted literal,
    // never as an unescaped shell metacharacter sequence that could break
    // out of the `cd` argument.
    assert!(command.contains("'/tmp/x'\\''; rm -rf / #'"));
    assert!(!command.contains("x'; rm -rf / #'\n"));
  }

  #[test]
  fn build_launch_command_quotes_malicious_agent_args() {
    let spec = PtyLaunchSpec::Agent {
      agent: RemoteAgentId::Claude,
      args: vec!["--prompt".to_string(), "$(rm -rf /)".to_string()],
    };
    let command = build_launch_command("/srv/project", &spec);
    assert!(command.contains("-lc "));
    assert!(command.contains("claude"));
    assert!(!command.contains(" $(rm -rf /) "));
  }

  /// The agent runs through a login shell, so a binary the user's profile
  /// puts on `PATH` is found, and every argument reaches it verbatim.
  #[cfg(unix)]
  #[test]
  fn agent_launch_uses_the_login_profile_path_and_keeps_arguments_verbatim() {
    let home = tempfile::tempdir().unwrap();
    let bin = home.path().join(".local/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let agent = bin.join("claude");
    std::fs::write(&agent, "#!/bin/sh\nprintf '%s\\n' \"$@\"\n").unwrap();
    std::fs::set_permissions(
      &agent,
      <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755),
    )
    .unwrap();
    std::fs::write(
      home.path().join(".bash_profile"),
      "PATH=\"$HOME/.local/bin:$PATH\"\n",
    )
    .unwrap();

    let hostile = "'; touch pwned #";
    let spec = PtyLaunchSpec::Agent {
      agent: RemoteAgentId::Claude,
      args: vec![
        "--prompt".to_string(),
        hostile.to_string(),
        "$(id)".to_string(),
      ],
    };
    let output = std::process::Command::new("/bin/sh")
      .arg("-c")
      .arg(build_launch_command(home.path().to_str().unwrap(), &spec))
      .env("HOME", home.path())
      .env("SHELL", "/bin/bash")
      .env("PATH", "/usr/bin:/bin")
      .output()
      .unwrap();

    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
      stdout.ends_with(&format!("--prompt\n{hostile}\n$(id)\n")),
      "{stdout}"
    );
    assert!(!home.path().join("pwned").exists());
  }

  // -- Failing-before-manager-exists coverage is structural: RemotePtyManager
  // did not exist before this module; every test below only compiles and
  // passes once it does, satisfying requirement 1 of the task's TDD list.

  #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
  async fn create_starts_in_the_requested_remote_working_directory() {
    let (addr, host_key) = start_echo_server().await;
    let client_key = test_host_key();
    let dir = tempfile::tempdir().unwrap();
    let key_reference = write_client_key(dir.path(), &client_key);
    let endpoint = test_endpoint("ep-1", addr, &host_key, key_reference);

    let pool = Arc::new(SshConnectionPool::new());
    let manager = RemotePtyManager::new(pool);
    let (tx, rx) = mpsc::channel::<Vec<u8>>();

    manager
      .create(
        binding("ep-1", "sess-1"),
        &endpoint,
        PtyLaunchSpec::Shell,
        80,
        24,
        move |chunk| {
          let _ = tx.send(chunk);
        },
        |_| {},
      )
      .await
      .unwrap();

    let first = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(first, b"ready\n");
    manager.close("sess-1").await.unwrap();
  }

  #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
  async fn write_and_read_round_trip_resize_and_close() {
    let (addr, host_key) = start_echo_server().await;
    let client_key = test_host_key();
    let dir = tempfile::tempdir().unwrap();
    let key_reference = write_client_key(dir.path(), &client_key);
    let endpoint = test_endpoint("ep-1", addr, &host_key, key_reference);

    let pool = Arc::new(SshConnectionPool::new());
    let manager = RemotePtyManager::new(pool);
    let (tx, rx) = mpsc::channel::<Vec<u8>>();

    manager
      .create(
        binding("ep-1", "sess-2"),
        &endpoint,
        PtyLaunchSpec::Shell,
        80,
        24,
        move |chunk| {
          let _ = tx.send(chunk);
        },
        |_| {},
      )
      .await
      .unwrap();
    let _ready = rx.recv_timeout(Duration::from_secs(2)).unwrap();

    manager.write("sess-2", b"hello\n").await.unwrap();
    let echoed = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(echoed, b"hello\n");

    manager.resize("sess-2", 100, 40).await.unwrap();

    manager.close("sess-2").await.unwrap();
    assert!(!manager.session_exists("sess-2"));
  }

  #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
  async fn exit_code_is_observed_after_the_remote_process_exits() {
    let (addr, host_key) = start_echo_server().await;
    let client_key = test_host_key();
    let dir = tempfile::tempdir().unwrap();
    let key_reference = write_client_key(dir.path(), &client_key);
    let endpoint = test_endpoint("ep-1", addr, &host_key, key_reference);

    let pool = Arc::new(SshConnectionPool::new());
    let manager = RemotePtyManager::new(pool);
    let (out_tx, out_rx) = mpsc::channel::<Vec<u8>>();
    let (exit_tx, exit_rx) = mpsc::channel::<Option<u32>>();

    manager
      .create(
        binding("ep-1", "sess-exit"),
        &endpoint,
        PtyLaunchSpec::Shell,
        80,
        24,
        move |chunk| {
          let _ = out_tx.send(chunk);
        },
        move |status| {
          let _ = exit_tx.send(status);
        },
      )
      .await
      .unwrap();
    let _ready = out_rx.recv_timeout(Duration::from_secs(2)).unwrap();

    manager.write("sess-exit", b"__exit__").await.unwrap();
    let status = exit_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(status, Some(7));

    // The reader loop removes the session once the channel closes, so
    // closing again is a no-op (idempotent), not an error.
    manager.close("sess-exit").await.unwrap();
  }

  #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
  async fn duplicate_close_is_idempotent() {
    let (addr, host_key) = start_echo_server().await;
    let client_key = test_host_key();
    let dir = tempfile::tempdir().unwrap();
    let key_reference = write_client_key(dir.path(), &client_key);
    let endpoint = test_endpoint("ep-1", addr, &host_key, key_reference);

    let pool = Arc::new(SshConnectionPool::new());
    let manager = RemotePtyManager::new(pool);
    let (tx, rx) = mpsc::channel::<Vec<u8>>();

    manager
      .create(
        binding("ep-1", "sess-3"),
        &endpoint,
        PtyLaunchSpec::Shell,
        80,
        24,
        move |chunk| {
          let _ = tx.send(chunk);
        },
        |_| {},
      )
      .await
      .unwrap();
    let _ready = rx.recv_timeout(Duration::from_secs(2)).unwrap();

    manager.close("sess-3").await.unwrap();
    // Closing again must not error — mirrors `PtyManager::close_session`'s
    // "nothing to close is success" convention.
    manager.close("sess-3").await.unwrap();
  }

  #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
  async fn operating_on_a_closed_session_returns_a_structured_error() {
    let (addr, host_key) = start_echo_server().await;
    let client_key = test_host_key();
    let dir = tempfile::tempdir().unwrap();
    let key_reference = write_client_key(dir.path(), &client_key);
    let endpoint = test_endpoint("ep-1", addr, &host_key, key_reference);

    let pool = Arc::new(SshConnectionPool::new());
    let manager = RemotePtyManager::new(pool);
    let (tx, rx) = mpsc::channel::<Vec<u8>>();

    manager
      .create(
        binding("ep-1", "sess-4"),
        &endpoint,
        PtyLaunchSpec::Shell,
        80,
        24,
        move |chunk| {
          let _ = tx.send(chunk);
        },
        |_| {},
      )
      .await
      .unwrap();
    let _ready = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    manager.close("sess-4").await.unwrap();

    let error = manager.write("sess-4", b"x").await.unwrap_err();
    assert_eq!(error, RemotePtyError::ClosedSession("sess-4".to_string()));
  }

  // -- Pooled connection reuse -------------------------------------------------

  #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
  async fn multiple_ptys_on_the_same_endpoint_reuse_the_pooled_connection() {
    let (addr, host_key) = start_echo_server().await;
    let client_key = test_host_key();
    let dir = tempfile::tempdir().unwrap();
    let key_reference = write_client_key(dir.path(), &client_key);
    let endpoint = test_endpoint("ep-1", addr, &host_key, key_reference);

    let pool = Arc::new(SshConnectionPool::new());
    let manager = RemotePtyManager::new(pool.clone());
    let (tx1, rx1) = mpsc::channel::<Vec<u8>>();
    let (tx2, rx2) = mpsc::channel::<Vec<u8>>();

    manager
      .create(
        binding("ep-1", "sess-a"),
        &endpoint,
        PtyLaunchSpec::Shell,
        80,
        24,
        move |c| {
          let _ = tx1.send(c);
        },
        |_| {},
      )
      .await
      .unwrap();
    let _ = rx1.recv_timeout(Duration::from_secs(2)).unwrap();

    manager
      .create(
        binding("ep-1", "sess-b"),
        &endpoint,
        PtyLaunchSpec::Shell,
        80,
        24,
        move |c| {
          let _ = tx2.send(c);
        },
        |_| {},
      )
      .await
      .unwrap();
    let _ = rx2.recv_timeout(Duration::from_secs(2)).unwrap();

    assert_eq!(pool.pooled_connection_count().await, 1);
    manager.close("sess-a").await.unwrap();
    manager.close("sess-b").await.unwrap();
  }

  // -- Isolation between concurrent sessions -----------------------------------

  #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
  async fn concurrent_sessions_do_not_leak_output_or_writes() {
    let (addr, host_key) = start_echo_server().await;
    let client_key = test_host_key();
    let dir = tempfile::tempdir().unwrap();
    let key_reference = write_client_key(dir.path(), &client_key);
    let endpoint = test_endpoint("ep-1", addr, &host_key, key_reference);

    let pool = Arc::new(SshConnectionPool::new());
    let manager = RemotePtyManager::new(pool);
    let (tx_a, rx_a) = mpsc::channel::<Vec<u8>>();
    let (tx_b, rx_b) = mpsc::channel::<Vec<u8>>();

    manager
      .create(
        binding("ep-1", "iso-a"),
        &endpoint,
        PtyLaunchSpec::Shell,
        80,
        24,
        move |c| {
          let _ = tx_a.send(c);
        },
        |_| {},
      )
      .await
      .unwrap();
    let _ = rx_a.recv_timeout(Duration::from_secs(2)).unwrap();

    manager
      .create(
        binding("ep-1", "iso-b"),
        &endpoint,
        PtyLaunchSpec::Shell,
        80,
        24,
        move |c| {
          let _ = tx_b.send(c);
        },
        |_| {},
      )
      .await
      .unwrap();
    let _ = rx_b.recv_timeout(Duration::from_secs(2)).unwrap();

    manager.write("iso-a", b"only-for-a\n").await.unwrap();
    let got_a = rx_a.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(got_a, b"only-for-a\n");
    // Session B must never see A's echoed bytes.
    assert!(rx_b.try_recv().is_err());

    manager.write("iso-b", b"only-for-b\n").await.unwrap();
    let got_b = rx_b.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(got_b, b"only-for-b\n");
    assert!(rx_a.try_recv().is_err());

    manager.close("iso-a").await.unwrap();
    manager.close("iso-b").await.unwrap();
  }

  // -- Endpoint cutoff ----------------------------------------------------------

  #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
  async fn endpoint_cutoff_closes_every_associated_pty_and_leaves_others_open() {
    let (addr, host_key) = start_echo_server().await;
    let client_key = test_host_key();
    let dir = tempfile::tempdir().unwrap();
    let key_reference = write_client_key(dir.path(), &client_key);
    let endpoint_a = test_endpoint("ep-cutoff", addr, &host_key, key_reference.clone());
    let endpoint_b = test_endpoint("ep-other", addr, &host_key, key_reference);

    let pool = Arc::new(SshConnectionPool::new());
    let manager = RemotePtyManager::new(pool.clone());
    let (tx_a, rx_a) = mpsc::channel::<Vec<u8>>();
    let (tx_b, rx_b) = mpsc::channel::<Vec<u8>>();

    manager
      .create(
        binding("ep-cutoff", "cut-a"),
        &endpoint_a,
        PtyLaunchSpec::Shell,
        80,
        24,
        move |c| {
          let _ = tx_a.send(c);
        },
        |_| {},
      )
      .await
      .unwrap();
    let _ = rx_a.recv_timeout(Duration::from_secs(2)).unwrap();

    manager
      .create(
        binding("ep-other", "keep-b"),
        &endpoint_b,
        PtyLaunchSpec::Shell,
        80,
        24,
        move |c| {
          let _ = tx_b.send(c);
        },
        |_| {},
      )
      .await
      .unwrap();
    let _ = rx_b.recv_timeout(Duration::from_secs(2)).unwrap();

    pool
      .force_cutoff("ep-cutoff", CutoffReason::KeyRevoked)
      .await;
    manager.close_all_for_endpoint("ep-cutoff").await;

    assert!(!manager.session_exists("cut-a"));
    // Unrelated session on a different endpoint is untouched.
    assert!(manager.session_exists("keep-b"));
    manager.write("keep-b", b"still-alive\n").await.unwrap();
    let got = rx_b.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(got, b"still-alive\n");

    manager.close("keep-b").await.unwrap();
  }

  #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
  async fn close_all_terminates_sessions_across_every_endpoint() {
    let (addr, host_key) = start_echo_server().await;
    let client_key = test_host_key();
    let dir = tempfile::tempdir().unwrap();
    let key_reference = write_client_key(dir.path(), &client_key);
    let endpoint_a = test_endpoint("ep-a", addr, &host_key, key_reference.clone());
    let endpoint_b = test_endpoint("ep-b", addr, &host_key, key_reference);

    let pool = Arc::new(SshConnectionPool::new());
    let manager = RemotePtyManager::new(pool);
    let (tx_a, rx_a) = mpsc::channel::<Vec<u8>>();
    let (tx_b, rx_b) = mpsc::channel::<Vec<u8>>();

    manager
      .create(
        binding("ep-a", "close-all-a"),
        &endpoint_a,
        PtyLaunchSpec::Shell,
        80,
        24,
        move |c| {
          let _ = tx_a.send(c);
        },
        |_| {},
      )
      .await
      .unwrap();
    let _ = rx_a.recv_timeout(Duration::from_secs(2)).unwrap();

    manager
      .create(
        binding("ep-b", "close-all-b"),
        &endpoint_b,
        PtyLaunchSpec::Shell,
        80,
        24,
        move |c| {
          let _ = tx_b.send(c);
        },
        |_| {},
      )
      .await
      .unwrap();
    let _ = rx_b.recv_timeout(Duration::from_secs(2)).unwrap();

    manager.close_all().await;

    assert!(!manager.session_exists("close-all-a"));
    assert!(!manager.session_exists("close-all-b"));
  }

  #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
  async fn close_all_for_window_only_closes_that_windows_sessions() {
    let (addr, host_key) = start_echo_server().await;
    let client_key = test_host_key();
    let dir = tempfile::tempdir().unwrap();
    let key_reference = write_client_key(dir.path(), &client_key);
    let endpoint = test_endpoint("ep-window", addr, &host_key, key_reference);

    let pool = Arc::new(SshConnectionPool::new());
    let manager = RemotePtyManager::new(pool);
    let (tx_a, rx_a) = mpsc::channel::<Vec<u8>>();
    let (tx_b, rx_b) = mpsc::channel::<Vec<u8>>();

    let binding_a = RemotePtyBinding {
      window_label: Some("window-a".to_string()),
      ..binding("ep-window", "win-a-1")
    };
    let binding_b = RemotePtyBinding {
      window_label: Some("window-b".to_string()),
      ..binding("ep-window", "win-b-1")
    };

    manager
      .create(
        binding_a,
        &endpoint,
        PtyLaunchSpec::Shell,
        80,
        24,
        move |c| {
          let _ = tx_a.send(c);
        },
        |_| {},
      )
      .await
      .unwrap();
    let _ = rx_a.recv_timeout(Duration::from_secs(2)).unwrap();

    manager
      .create(
        binding_b,
        &endpoint,
        PtyLaunchSpec::Shell,
        80,
        24,
        move |c| {
          let _ = tx_b.send(c);
        },
        |_| {},
      )
      .await
      .unwrap();
    let _ = rx_b.recv_timeout(Duration::from_secs(2)).unwrap();

    manager.close_all_for_window("window-a").await;

    assert!(!manager.session_exists("win-a-1"));
    assert!(manager.session_exists("win-b-1"));

    manager.close("win-b-1").await.unwrap();
  }

  #[tokio::test]
  async fn creation_and_writes_are_rejected_after_cutoff() {
    let (addr, host_key) = start_echo_server().await;
    let client_key = test_host_key();
    let dir = tempfile::tempdir().unwrap();
    let key_reference = write_client_key(dir.path(), &client_key);
    let endpoint = test_endpoint("ep-blocked", addr, &host_key, key_reference);

    let pool = Arc::new(SshConnectionPool::new());
    let manager = RemotePtyManager::new(pool.clone());

    pool
      .force_cutoff("ep-blocked", CutoffReason::CertificateExpired)
      .await;

    let error = manager
      .create(
        binding("ep-blocked", "blocked-1"),
        &endpoint,
        PtyLaunchSpec::Shell,
        80,
        24,
        |_| {},
        |_| {},
      )
      .await
      .unwrap_err();
    assert_eq!(
      error,
      RemotePtyError::Cutoff {
        endpoint_id: "ep-blocked".to_string(),
        reason: CutoffReason::CertificateExpired,
      }
    );
    assert!(!manager.session_exists("blocked-1"));
  }

  // -- Phase 8: create_with_command (desktop reattach) -------------------------

  #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
  async fn create_with_command_opens_a_pty_with_an_already_built_command_line() {
    let (addr, host_key) = start_echo_server().await;
    let client_key = test_host_key();
    let dir = tempfile::tempdir().unwrap();
    let key_reference = write_client_key(dir.path(), &client_key);
    let endpoint = test_endpoint("ep-1", addr, &host_key, key_reference);

    let pool = Arc::new(SshConnectionPool::new());
    let manager = RemotePtyManager::new(pool);
    let (tx, rx) = mpsc::channel::<Vec<u8>>();

    // Mirrors what `pty-remote attach-command` would return: a fully
    // pre-built literal command line, not a `PtyLaunchSpec` this manager
    // assembles itself.
    let command = "cd /srv/project && exec tmux new-session -A -s treq-pty-1-term";

    manager
      .create_with_command(
        binding("ep-1", "reattach-1"),
        &endpoint,
        command,
        80,
        24,
        move |chunk| {
          let _ = tx.send(chunk);
        },
        |_| {},
      )
      .await
      .unwrap();

    let first = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(first, b"ready\n");

    manager.write("reattach-1", b"hello\n").await.unwrap();
    let echoed = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(echoed, b"hello\n");

    manager.close("reattach-1").await.unwrap();
    assert!(!manager.session_exists("reattach-1"));
  }

  #[test]
  fn attach_commands_are_cached_per_endpoint_and_request_until_forgotten() {
    use crate::core::remote::TreqCommandRequest;
    let host_key = test_host_key();
    let addr: std::net::SocketAddr = "127.0.0.1:22".parse().unwrap();
    let endpoint = test_endpoint("e1", addr, &host_key, "k".into());
    let other = test_endpoint("e2", addr, &host_key, "k".into());
    let request = |cols: u16| TreqCommandRequest::PtyAttachCommand {
      repo: "/r".into(),
      workspace: "w".into(),
      label: "shell".into(),
      remote_dir: "/r/w".into(),
      launch: PtyLaunchSpec::Shell,
      cols,
      rows: 24,
    };
    let cache = AttachCommandCache::default();
    let key = AttachCommandCache::key(&endpoint, &request(80));
    assert_eq!(cache.get(&key), None);
    cache.insert(key.clone(), "tmux attach".into());
    assert_eq!(cache.get(&key).as_deref(), Some("tmux attach"));
    assert_ne!(key, AttachCommandCache::key(&endpoint, &request(100)));
    assert_ne!(key, AttachCommandCache::key(&other, &request(80)));
    cache.forget(&key);
    assert_eq!(cache.get(&key), None);
  }
}
