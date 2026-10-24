//! What the workspace terminal pane (`WorkspaceTerminalPane`) reads and
//! runs: PTY sessions, agent start commands, chat logs, and the live
//! terminal state that outlives any one view of it.

use crate::result_ext::ResultExt;
use std::{
  collections::HashMap,
  sync::{Arc, Mutex},
  time::Instant,
};

use futures::channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use serde::Deserialize;

use super::{
  logs_data::LogsHost,
  terminal_emu::{Emulator, Output},
};
use crate::{
  core::{
    agent_command::{self, AgentPaths, AutoCommand},
    remote::TreqCommandRequest,
    remote_control_plane::SshEndpoint,
    remote_pty::PtyLaunchSpec,
  },
  gpui_shell::{native::Backend, popover::PopoverHost, remote_repo::Transport},
  lock_ext::LockExt,
};

/// A PTY to start, as `pty_create_session` takes it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Spawn {
  pub id: String,
  pub repo_path: Option<String>,
  pub workspace_id: Option<i64>,
  pub working_dir: Option<String>,
  /// The agent's start command: typed once the shell is up, its echo hidden.
  pub auto_command: Option<String>,
}

/// An agent session's start, as `useAgentAutoCommand` prepares it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AgentStart {
  pub agent: String,
  pub repo_path: String,
  pub workspace_path: Option<String>,
  pub session_id: i64,
  pub pending_prompt: Option<String>,
  pub permission_mode: Option<String>,
}

/// Where a persistent remote shell or agent runs (`RemoteTerminalTarget`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteTarget {
  pub endpoint: SshEndpoint,
  pub repository_id: String,
  pub workspace_id: String,
  pub remote_working_directory: String,
  /// The persistent session's label on the host.
  pub label: String,
  /// What starts when no session has the label yet; the login shell if
  /// unset.
  #[serde(default)]
  pub launch: Option<PtyLaunchSpec>,
  /// Picking up a running session: only the loading text differs.
  #[serde(default)]
  pub reattach: bool,
}

/// Local ids of remote sessions start with this (`makeSessionId`).
pub const REMOTE_PTY_PREFIX: &str = "remote-pty-";

/// `makeSessionId`: a fresh local id per attach, since the backend never
/// reuses one; the label names the session on the host.
pub fn remote_pty_id(target: &RemoteTarget, millis: u128) -> String {
  format!(
    "{REMOTE_PTY_PREFIX}{}-{}-{}-{millis}",
    target.endpoint.id, target.workspace_id, target.label
  )
}

pub fn is_remote_pty(id: &str) -> bool {
  id.starts_with(REMOTE_PTY_PREFIX)
}

/// The pane's column id for a remote session (`handleOpenRemoteSession`):
/// it shares the shell slot.
pub fn remote_column_id(label: &str, millis: u128) -> String {
  let label: String = label
    .chars()
    .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
    .collect();
  format!("shell-remote-{label}-{millis}")
}

/// Whether a `PtyList` answer has `label` still running: `None` when the
/// answer can't be read.
pub fn session_running(list: &serde_json::Value, label: &str) -> Option<bool> {
  let sessions = list.as_array()?;
  Some(sessions.iter().any(|s| {
    s.get("label").and_then(|l| l.as_str()) == Some(label)
      && s.get("running").and_then(|r| r.as_bool()) == Some(true)
  }))
}

#[derive(Debug, Clone, PartialEq)]
pub struct Prepared {
  pub command: String,
  pub skill_write_warning: Option<String>,
}

pub trait TerminalHost:
  LogsHost
  + PopoverHost
  + super::file_browser_data::FileBrowserHost
  + super::workspace_ops::WorkspaceActionsHost
{
  /// Starts a PTY unless one with the id runs; its output arrives through
  /// [`TerminalHost::listen`].
  fn spawn_pty(&self, spawn: &Spawn) -> Result<(), String>;
  fn pty_exists(&self, id: &str) -> bool;
  fn write_pty(&self, id: &str, data: &str) -> Result<(), String>;
  fn resize_pty(&self, id: &str, rows: u16, cols: u16) -> Result<(), String>;
  fn close_pty(&self, id: &str) -> Result<(), String>;
  /// Calls `sink` with each chunk of the PTY's output until dropped.
  fn listen(&self, id: &str, sink: OutputSink) -> Listening;
  /// Calls `sink` with the exit status when a remote session's channel
  /// ends, until dropped.
  fn listen_exit(&self, id: &str, sink: Box<dyn Fn(Option<u32>) + Send + Sync>) -> Listening;
  /// `remote_pty_reattach`: attaches local session `id` to the target's
  /// persistent session, starting it first if none runs.
  fn remote_attach(
    &self,
    id: &str,
    target: &RemoteTarget,
    cols: u16,
    rows: u16,
  ) -> Result<(), String>;
  /// Whether the target's session still runs on the host (`PtyList`);
  /// `None` when the host can't be asked.
  fn remote_running(&self, target: &RemoteTarget) -> Option<bool>;
  /// `PtyStop`: ends the persistent session on the host.
  fn remote_stop(&self, target: &RemoteTarget) -> Result<(), String>;
  fn sessions(&self, repo_path: &str) -> Result<Vec<crate::local_db::Session>, String>;
  /// The repository's supporting repositories that still exist.
  fn supporting_repos(&self, repo_path: &str) -> Vec<String>;
  /// A shell's start command: puts the treq CLI on its PATH.
  fn shell_command(&self) -> Option<String>;
  fn prepare_agent(&self, start: &AgentStart) -> Result<Prepared, String>;
  fn register_chat(&self, start: &AgentStart, pty_id: &str, name: &str) -> Result<(), String>;
  fn record_chat_screen(
    &self,
    repo_path: &str,
    session_id: i64,
    screen: &str,
  ) -> Result<(), String>;
  /// `recordUserMessage`: what was sent to the agent, after `screen_before`.
  fn record_chat_message(
    &self,
    repo_path: &str,
    session_id: i64,
    screen_before: &str,
    text: &str,
  ) -> Result<(), String>;
  /// `read_file`: a `treq send` text attachment.
  fn read_send_asset(&self, path: &str) -> Result<String, String>;
  /// `write_send_review_image`: where the PNG landed.
  fn write_send_review_image(
    &self,
    repo_path: &str,
    name: &str,
    png: &[u8],
  ) -> Result<String, String>;
  /// A clipboard image pasted into a terminal: where it was saved.
  fn write_pasted_image(&self, bytes: &[u8]) -> Result<String, String>;
}

pub type Host = Arc<dyn TerminalHost>;

/// Receives a PTY's output, as the bytes read.
pub type OutputSink = Box<dyn Fn(&[u8]) + Send + Sync>;

/// Stops a [`TerminalHost::listen`] when dropped.
pub struct Listening(Option<Box<dyn FnOnce() + Send + Sync>>);

impl Listening {
  pub fn new(stop: impl FnOnce() + Send + Sync + 'static) -> Self {
    Self(Some(Box::new(stop)))
  }
}

impl Drop for Listening {
  fn drop(&mut self) {
    if let Some(stop) = self.0.take() {
      stop();
    }
  }
}

impl TerminalHost for Backend {
  fn spawn_pty(&self, spawn: &Spawn) -> Result<(), String> {
    use tauri::Manager;
    let state = self.app().state::<crate::AppState>();
    if state.pty_manager.session_exists(&spawn.id) {
      return Ok(());
    }
    let launch = crate::commands::pty_commands::launch(
      &state,
      &spawn.id,
      None,
      spawn.repo_path.as_deref(),
      spawn.workspace_id,
      spawn.working_dir.clone(),
      None,
      None,
    )?;
    let id = spawn.id.clone();
    let emit = Box::new(move |data: &[u8]| crate::pty_stream::publish(&id, data));
    state.pty_manager.create_session(
      spawn.id.clone(),
      Some(self.label().to_string()),
      launch.working_dir,
      launch.shell,
      launch.shell_args,
      launch.initial_command,
      spawn.auto_command.clone(),
      emit,
    )?;
    if let Some(command) = &spawn.auto_command {
      // As React did: the shell is given a moment to start, then the
      // command is typed with its echo hidden.
      let manager = state.pty_manager.clone();
      let (id, command) = (spawn.id.clone(), agent_command::normalize_command(command));
      std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(100));
        manager.set_auto_command(&id, &command).ignore_closed();
        manager.write_to_session(&id, &command).ignore_closed();
      });
    }
    Ok(())
  }

  fn pty_exists(&self, id: &str) -> bool {
    use tauri::Manager;
    if is_remote_pty(id) {
      return self
        .app()
        .state::<crate::commands::remote_pty_commands::RemotePtyState>()
        .0
        .session_exists(id);
    }
    self
      .app()
      .state::<crate::AppState>()
      .pty_manager
      .session_exists(id)
  }

  fn write_pty(&self, id: &str, data: &str) -> Result<(), String> {
    use tauri::Manager;
    if is_remote_pty(id) {
      remote_channel(self, id, RemoteOp::Write(data.to_string()));
      return Ok(());
    }
    self
      .app()
      .state::<crate::AppState>()
      .pty_manager
      .write_to_session(id, data)
  }

  fn resize_pty(&self, id: &str, rows: u16, cols: u16) -> Result<(), String> {
    use tauri::Manager;
    if is_remote_pty(id) {
      remote_channel(self, id, RemoteOp::Resize { cols, rows });
      return Ok(());
    }
    self
      .app()
      .state::<crate::AppState>()
      .pty_manager
      .resize_session(id, rows, cols)
  }

  fn close_pty(&self, id: &str) -> Result<(), String> {
    use tauri::Manager;
    if is_remote_pty(id) {
      // Detach: the channel closes, the session keeps running on the host.
      remote_channel(self, id, RemoteOp::Close);
      return Ok(());
    }
    let manager = self.app().state::<crate::AppState>().pty_manager.clone();
    let id = id.to_string();
    std::thread::spawn(move || {
      if let Err(error) = manager.close_session(&id) {
        log::warn!("pty close failed: session_id={id}, error={error}");
      }
    });
    Ok(())
  }

  fn listen(&self, id: &str, sink: OutputSink) -> Listening {
    let subscription = crate::pty_stream::subscribe(id, sink.into());
    Listening::new(move || drop(subscription))
  }

  fn listen_exit(&self, id: &str, sink: Box<dyn Fn(Option<u32>) + Send + Sync>) -> Listening {
    use tauri::Listener;
    let app = self.app().clone();
    let handler = app.listen(format!("remote-pty-exit-{id}"), move |event| {
      let payload = serde_json::from_str::<serde_json::Value>(event.payload()).unwrap_or_default();
      sink(
        payload
          .get("exit_status")
          .and_then(|s| s.as_u64())
          .map(|s| s as u32),
      );
    });
    Listening::new(move || app.unlisten(handler))
  }

  fn remote_attach(
    &self,
    id: &str,
    target: &RemoteTarget,
    cols: u16,
    rows: u16,
  ) -> Result<(), String> {
    use tauri::Manager;
    self.with_db(|db| {
      crate::core::feature_preview::require_remote_for_source(db, &target.endpoint.source)
    })?;
    let manager = self
      .app()
      .state::<crate::commands::remote_pty_commands::RemotePtyState>()
      .0
      .clone();
    crate::gpui_shell::block_on(crate::commands::remote_pty_commands::reattach(
      self.app().clone(),
      &manager,
      &self.exec_pool(),
      crate::commands::remote_pty_commands::Reattach {
        session_id: id.to_string(),
        window_label: Some(self.label().to_string()),
        endpoint: target.endpoint.clone(),
        repository_id: target.repository_id.clone(),
        workspace_id: target.workspace_id.clone(),
        label: target.label.clone(),
        remote_working_directory: target.remote_working_directory.clone(),
        launch: target.launch.clone().unwrap_or(PtyLaunchSpec::Shell),
        cols,
        rows,
      },
    ))
  }

  fn remote_running(&self, target: &RemoteTarget) -> Option<bool> {
    let list = Transport::read(
      self,
      &target.endpoint,
      TreqCommandRequest::PtyList {
        repo: target.repository_id.clone(),
        workspace: Some(target.workspace_id.clone()),
      },
    )
    .ok()?;
    session_running(&list, &target.label)
  }

  fn remote_stop(&self, target: &RemoteTarget) -> Result<(), String> {
    Transport::read(
      self,
      &target.endpoint,
      TreqCommandRequest::PtyStop {
        repo: target.repository_id.clone(),
        workspace: target.workspace_id.clone(),
        label: target.label.clone(),
      },
    )
    .map(drop)
  }

  fn sessions(&self, repo_path: &str) -> Result<Vec<crate::local_db::Session>, String> {
    crate::core::sessions::list_sessions(&self.state_scope(repo_path))
  }

  fn supporting_repos(&self, repo_path: &str) -> Vec<String> {
    crate::core::supporting_repos::list(repo_path)
      .unwrap_or_default()
      .into_iter()
      .filter(|repo| repo.exists)
      .map(|repo| repo.path)
      .collect()
  }

  fn shell_command(&self) -> Option<String> {
    let dir = crate::commands::binary::get_treq_bin_dir().ok()?;
    Some(format!("export PATH=\"{dir}:$PATH\""))
  }

  fn prepare_agent(&self, start: &AgentStart) -> Result<Prepared, String> {
    let model = crate::core::sessions::session_model(&start.repo_path, start.session_id)
      .ok()
      .flatten();
    let bin_dir = crate::commands::binary::get_treq_bin_dir().ok();
    let supporting = self.supporting_repos(&start.repo_path);
    let prepared = agent_command::prepare_auto_command(
      &AgentPaths {
        workspace_path: start.workspace_path.as_deref(),
        repo_path: &start.repo_path,
        supporting_repo_paths: &supporting,
      },
      &AutoCommand {
        agent: &start.agent,
        permission_mode: start.permission_mode.as_deref(),
        session_model: model.as_deref(),
        pending_prompt: start.pending_prompt.as_deref(),
        treq_bin_dir: bin_dir.as_deref(),
      },
    )?;
    Ok(Prepared {
      command: prepared.command,
      skill_write_warning: prepared.skill_write_warning,
    })
  }

  fn register_chat(&self, start: &AgentStart, pty_id: &str, name: &str) -> Result<(), String> {
    crate::core::agent_chat::register_agent_chat(
      &start.repo_path,
      start.session_id,
      pty_id,
      name,
      &start.agent,
      None,
      start.pending_prompt.as_deref(),
    )
    .map(drop)
  }

  fn record_chat_screen(
    &self,
    repo_path: &str,
    session_id: i64,
    screen: &str,
  ) -> Result<(), String> {
    crate::core::agent_chat::record_screen(repo_path, session_id, screen).map(drop)
  }

  fn record_chat_message(
    &self,
    repo_path: &str,
    session_id: i64,
    screen_before: &str,
    text: &str,
  ) -> Result<(), String> {
    crate::core::agent_chat::record_user_message(repo_path, session_id, screen_before, text)
      .map(drop)
  }

  fn read_send_asset(&self, path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| e.to_string())
  }

  fn write_send_review_image(
    &self,
    repo_path: &str,
    name: &str,
    png: &[u8],
  ) -> Result<String, String> {
    crate::send_dispatch::write_send_review_image_bytes(repo_path, name, png)
      .map(|path| path.to_string_lossy().to_string())
  }

  fn write_pasted_image(&self, bytes: &[u8]) -> Result<String, String> {
    crate::send_dispatch::write_pasted_image(&crate::send_dispatch::pasted_image_dir(), bytes)
      .map(|path| path.to_string_lossy().to_string())
  }
}

/// What a remote session's channel is asked, in the order it was asked.
enum RemoteOp {
  Write(String),
  Resize { cols: u16, rows: u16 },
  Close,
}

/// Sends `op` to the session's channel. Each session has a thread that
/// runs its ops in order, so keystrokes reach the host as typed and the UI
/// never waits on the network.
fn remote_channel(backend: &Backend, id: &str, op: RemoteOp) {
  use std::sync::mpsc::Sender;
  use tauri::Manager;
  static CHANNELS: std::sync::OnceLock<Mutex<HashMap<String, Sender<RemoteOp>>>> =
    std::sync::OnceLock::new();
  let channels = CHANNELS.get_or_init(Mutex::default);
  let closing = matches!(op, RemoteOp::Close);
  let mut channels = channels.lock_or_recover();
  let sender = channels.entry(id.to_string()).or_insert_with(|| {
    let manager = backend
      .app()
      .state::<crate::commands::remote_pty_commands::RemotePtyState>()
      .0
      .clone();
    let (tx, rx) = std::sync::mpsc::channel::<RemoteOp>();
    let id = id.to_string();
    std::thread::spawn(move || {
      while let Ok(op) = rx.recv() {
        let done = matches!(op, RemoteOp::Close);
        let result = crate::gpui_shell::block_on(async {
          match op {
            RemoteOp::Write(data) => manager.write(&id, data.as_bytes()).await,
            RemoteOp::Resize { cols, rows } => manager.resize(&id, cols, rows).await,
            RemoteOp::Close => manager.close(&id).await,
          }
        });
        if let Err(error) = result {
          // The channel may already be gone; never log what was typed.
          log::debug!("remote pty op failed: session_id={id}, error={error}");
        }
        if done {
          break;
        }
      }
    });
    tx
  });
  sender.send(op).ignore_closed();
  if closing {
    channels.remove(id);
  }
}

pub use crate::core::agent_launch::{agent_pty_id, pane_session_id};

/// A shell's PTY id: its directory with anything but letters and digits
/// dashed, and a timestamp.
pub fn shell_pty_id(dir: &str, millis: u128) -> String {
  let dir: String = dir
    .chars()
    .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
    .collect();
  format!("shell-{dir}-{millis}")
}

/// `consumePtyEcho`: drops the echo of what the user typed from an output
/// chunk, so local echo isn't taken for the process working. Returns the
/// echo still pending and the output the process wrote.
#[allow(
  clippy::string_slice,
  reason = "offsets are char_indices() positions plus char lengths"
)]
pub fn consume_echo<'a>(pending: &str, chunk: &'a str) -> (String, &'a str) {
  let echo: Vec<char> = pending.chars().collect();
  let bytes = chunk.as_bytes();
  let (mut e, mut c) = (0, 0);
  let mut chars = chunk.char_indices().peekable();
  while e < echo.len() {
    let Some(&(i, ch)) = chars.peek() else {
      break;
    };
    if echo[e] == '\r' && ch == '\r' && bytes.get(i + 1) == Some(&b'\n') {
      chars.next();
      chars.next();
      e += 1;
      c = i + 2;
    } else if (echo[e] == '\r' && ch == '\n') || echo[e] == ch {
      chars.next();
      e += 1;
      c = i + ch.len_utf8();
    } else {
      break;
    }
  }
  (echo[e..].iter().collect(), &chunk[c..])
}

/// A terminal's state, shared by the PTY's output listener and every view
/// drawing it; it lives as long as the PTY, so reopening the pane keeps the
/// scrollback.
pub struct Live {
  pub emulator: Mutex<Emulator>,
  pending_echo: Mutex<String>,
  /// When the process (not the echo of typing) last wrote output.
  pub last_output: Mutex<Option<Instant>>,
  watchers: Mutex<Vec<UnboundedSender<bool>>>,
  title: Mutex<Option<String>>,
  _listening: Mutex<Option<Listening>>,
}

impl Live {
  fn new(cols: usize, rows: usize) -> Self {
    Self {
      emulator: Mutex::new(Emulator::new(cols, rows)),
      pending_echo: Mutex::default(),
      last_output: Mutex::default(),
      watchers: Mutex::default(),
      title: Mutex::default(),
      _listening: Mutex::default(),
    }
  }

  /// Parses a chunk of output; returns what the terminal wants written back.
  #[cfg(test)]
  pub fn push(&self, chunk: &str) -> Vec<String> {
    self.push_bytes(chunk.as_bytes())
  }

  /// [`Self::push`] for the bytes a PTY read.
  pub fn push_bytes(&self, chunk: &[u8]) -> Vec<String> {
    let from_process = {
      let mut pending = self.pending_echo.lock_or_recover();
      if pending.is_empty() {
        !chunk.is_empty()
      } else {
        let text = String::from_utf8_lossy(chunk);
        let (left, process) = consume_echo(&pending, &text);
        let from_process = !process.is_empty();
        *pending = left;
        from_process
      }
    };
    let outputs = {
      let mut emulator = self.emulator.lock_or_recover();
      emulator.feed(chunk);
      emulator.drain()
    };
    let mut replies = Vec::new();
    for output in outputs {
      match output {
        Output::Reply(reply) => replies.push(reply),
        Output::Title(title) => *self.title.lock_or_recover() = Some(title),
        Output::Copy(_) | Output::Bell => {}
      }
    }
    if from_process {
      *self.last_output.lock_or_recover() = Some(Instant::now());
    }
    self
      .watchers
      .lock_or_recover()
      .retain(|watcher| watcher.unbounded_send(from_process).is_ok());
    replies
  }

  /// Notes what the user typed, so its echo isn't taken for output.
  pub fn typed(&self, data: &str) {
    self.pending_echo.lock_or_recover().push_str(data);
  }

  /// A message per output chunk: whether the process wrote it.
  pub fn watch(&self) -> UnboundedReceiver<bool> {
    let (tx, rx) = unbounded();
    self.watchers.lock_or_recover().push(tx);
    rx
  }

  pub fn screen_text(&self) -> String {
    self.emulator.lock_or_recover().screen_text()
  }
}

type Registry = Mutex<HashMap<String, Arc<Live>>>;

#[cfg(not(test))]
fn registry() -> &'static Registry {
  static LIVE: std::sync::OnceLock<Registry> = std::sync::OnceLock::new();
  LIVE.get_or_init(Mutex::default)
}

/// Each test thread keeps its own terminals.
#[cfg(test)]
fn registry() -> &'static Registry {
  thread_local! {
    static LIVE: &'static Registry = Box::leak(Box::default());
  }
  LIVE.with(|live| *live)
}

/// The live terminal for a PTY, listening to its output from now on.
pub fn attach(host: &Host, id: &str, cols: usize, rows: usize) -> (Arc<Live>, bool) {
  let mut live = registry().lock_or_recover();
  if let Some(existing) = live.get(id) {
    return (existing.clone(), false);
  }
  let terminal = Arc::new(Live::new(cols, rows));
  let weak = Arc::downgrade(&terminal);
  let (writer, pty) = (host.clone(), id.to_string());
  let listening = host.listen(
    id,
    Box::new(move |chunk| {
      if let Some(terminal) = weak.upgrade() {
        for reply in terminal.push_bytes(chunk) {
          writer.write_pty(&pty, &reply).ignore_closed();
        }
      }
    }),
  );
  *terminal._listening.lock_or_recover() = Some(listening);
  live.insert(id.to_string(), terminal.clone());
  (terminal, true)
}

/// Forgets a closed PTY's terminal.
pub fn detach(id: &str) {
  registry().lock_or_recover().remove(id);
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn names_ptys_like_react() {
    assert_eq!(pane_session_id("/repo", "/repo", 3), 3);
    assert_eq!(shell_pty_id("/a/b c", 5), "shell--a-b-c-5");
  }

  #[test]
  fn names_remote_sessions_like_react() {
    let target: RemoteTarget = serde_json::from_value(serde_json::json!({
      "endpoint": crate::gpui_shell::remote_repo::tests::repository().endpoint,
      "repositoryId": "/srv/app",
      "workspaceId": "root",
      "remoteWorkingDirectory": "/srv/app",
      "label": "shell-k1",
      "launch": { "type": "shell" },
      "reattach": false,
    }))
    .unwrap();
    assert_eq!(remote_pty_id(&target, 9), "remote-pty-ep-1-root-shell-k1-9");
    assert!(is_remote_pty(&remote_pty_id(&target, 9)));
    assert!(!is_remote_pty("shell--srv-app-9"));
    assert_eq!(remote_column_id("claude-k1", 5), "shell-remote-claude-k1-5");
  }

  #[test]
  fn reads_whether_a_remote_session_still_runs() {
    let list = serde_json::json!([
      { "label": "shell-a", "running": false },
      { "label": "shell-b", "running": true },
    ]);
    assert_eq!(session_running(&list, "shell-b"), Some(true));
    assert_eq!(session_running(&list, "shell-a"), Some(false));
    assert_eq!(session_running(&list, "gone"), Some(false));
    assert_eq!(session_running(&serde_json::json!({}), "shell-b"), None);
  }

  #[test]
  fn consumes_the_echo_of_typing() {
    assert_eq!(consume_echo("", "out"), (String::new(), "out"));
    assert_eq!(consume_echo("ls", "ls"), (String::new(), ""));
    assert_eq!(consume_echo("ls\r", "ls\r\nfile"), (String::new(), "file"));
    assert_eq!(consume_echo("ls\r", "ls\nfile"), (String::new(), "file"));
    assert_eq!(consume_echo("lsx", "lq"), ("sx".to_string(), "q"));
    assert_eq!(consume_echo("é", "éa"), (String::new(), "a"));
  }

  #[test]
  fn live_terminals_mark_process_output() {
    let live = Live::new(20, 2);
    let mut rx = live.watch();
    live.typed("a");
    assert!(live.push("a").is_empty());
    assert_eq!(rx.try_recv().ok(), Some(false));
    assert!(live.last_output.lock_or_recover().is_none());
    live.push("done");
    assert_eq!(rx.try_recv().ok(), Some(true));
    assert!(live.last_output.lock_or_recover().is_some());
    assert_eq!(live.screen_text(), "adone");
    assert_eq!(live.push("\x1b[6n"), vec!["\x1b[1;6R".to_string()]);
  }

  #[test]
  fn live_terminals_take_a_character_split_across_chunks() {
    let live = Live::new(20, 2);
    let tick = "✓".as_bytes();
    live.push_bytes(&tick[..1]);
    live.push_bytes(&tick[1..]);
    assert_eq!(live.screen_text(), "✓");
  }

  /// Feeding ~32 MB of colored output through the old path (decode to a
  /// string, JSON-encode it as the event payload, parse that, feed) and the
  /// new one (feed the bytes read). The old path leaves out the event bus
  /// itself, so it is a lower bound.
  /// `cargo test --features gpui-shell --lib pty_output_path -- --ignored --nocapture`
  #[test]
  #[ignore = "benchmark"]
  fn pty_output_path_throughput() {
    let line = "\x1b[38;5;196mERROR\x1b[0m \x1b[1;32m✓ passed\x1b[0m src/日本/main.rs:42:7 \x1b[4mwarning\x1b[24m\r\n";
    let mut data = Vec::new();
    while data.len() < 32 << 20 {
      data.extend_from_slice(line.as_bytes());
    }
    let chunks: Vec<&[u8]> = data.chunks(8192).collect();

    // What only the old path does: decode, encode as JSON, parse that.
    let mut decoder = crate::pty::Utf8StreamDecoder::new();
    let start = Instant::now();
    let mut total = 0;
    for chunk in &chunks {
      let payload = serde_json::to_string(&decoder.push(chunk)).unwrap();
      total += serde_json::from_str::<String>(&payload).unwrap().len();
    }
    let transport_time = start.elapsed();
    assert_eq!(total, data.len());

    let old = Live::new(120, 40);
    let mut decoder = crate::pty::Utf8StreamDecoder::new();
    let start = Instant::now();
    for chunk in &chunks {
      let text = decoder.push(chunk);
      let payload = serde_json::to_string(&text).unwrap();
      let text: String = serde_json::from_str(&payload).unwrap();
      old.push(&text);
    }
    let old_time = start.elapsed();

    let new = Live::new(120, 40);
    let start = Instant::now();
    for chunk in &chunks {
      new.push_bytes(chunk);
    }
    let new_time = start.elapsed();
    let mb = data.len() as f64 / (1 << 20) as f64;
    println!(
      "transport stages alone {transport_time:?}; old {old_time:?} ({:.0} MB/s), new {new_time:?} ({:.0} MB/s)",
      mb / old_time.as_secs_f64(),
      mb / new_time.as_secs_f64()
    );
  }
}
