//! What the workspace terminal pane (`WorkspaceTerminalPane`) reads and
//! runs: PTY sessions, agent start commands, chat logs, and the live
//! terminal state that outlives any one view of it.

use std::{
  collections::HashMap,
  sync::{Arc, Mutex},
  time::Instant,
};

use futures::channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};

use super::{
  logs_data::LogsHost,
  terminal_emu::{Emulator, Output},
};
use crate::{
  core::agent_command::{self, AgentPaths, AutoCommand},
  gpui_shell::{native::Backend, popover::PopoverHost},
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

#[derive(Debug, Clone, PartialEq)]
pub struct Prepared {
  pub command: String,
  pub skill_write_warning: Option<String>,
}

pub trait TerminalHost: LogsHost + PopoverHost {
  /// Starts a PTY unless one with the id runs; its output arrives through
  /// [`TerminalHost::listen`].
  fn spawn_pty(&self, spawn: &Spawn) -> Result<(), String>;
  fn pty_exists(&self, id: &str) -> bool;
  fn write_pty(&self, id: &str, data: &str) -> Result<(), String>;
  fn resize_pty(&self, id: &str, rows: u16, cols: u16) -> Result<(), String>;
  fn close_pty(&self, id: &str) -> Result<(), String>;
  /// Calls `sink` with each chunk of the PTY's output until dropped.
  fn listen(&self, id: &str, sink: Box<dyn Fn(String) + Send + Sync>) -> Listening;
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
}

pub type Host = Arc<dyn TerminalHost>;

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
    use tauri::{Emitter, Manager};
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
    let app = self.app().clone();
    let event = format!("pty-data-{}", spawn.id);
    let emit = Box::new(move |data: String| {
      if let Err(error) = app.emit(&event, data) {
        log::warn!("pty emit failed: event={event}, error={error}");
      }
    });
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
        let _ = manager.set_auto_command(&id, &command);
        let _ = manager.write_to_session(&id, &command);
      });
    }
    Ok(())
  }

  fn pty_exists(&self, id: &str) -> bool {
    use tauri::Manager;
    self
      .app()
      .state::<crate::AppState>()
      .pty_manager
      .session_exists(id)
  }

  fn write_pty(&self, id: &str, data: &str) -> Result<(), String> {
    use tauri::Manager;
    self
      .app()
      .state::<crate::AppState>()
      .pty_manager
      .write_to_session(id, data)
  }

  fn resize_pty(&self, id: &str, rows: u16, cols: u16) -> Result<(), String> {
    use tauri::Manager;
    self
      .app()
      .state::<crate::AppState>()
      .pty_manager
      .resize_session(id, rows, cols)
  }

  fn close_pty(&self, id: &str) -> Result<(), String> {
    use tauri::Manager;
    let manager = self.app().state::<crate::AppState>().pty_manager.clone();
    let id = id.to_string();
    std::thread::spawn(move || {
      if let Err(error) = manager.close_session(&id) {
        log::warn!("pty close failed: session_id={id}, error={error}");
      }
    });
    Ok(())
  }

  fn listen(&self, id: &str, sink: Box<dyn Fn(String) + Send + Sync>) -> Listening {
    use tauri::Listener;
    let app = self.app().clone();
    let handler = app.listen(format!("pty-data-{id}"), move |event| {
      if let Ok(data) = serde_json::from_str::<String>(event.payload()) {
        sink(data);
      }
    });
    Listening::new(move || app.unlisten(handler))
  }

  fn sessions(&self, repo_path: &str) -> Result<Vec<crate::local_db::Session>, String> {
    crate::local_db::get_sessions(repo_path)
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
    let model = crate::local_db::get_session_model(&start.repo_path, start.session_id)
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
}

/// `repoHash` in `agent-pty-id.ts`.
pub fn repo_hash(repo_path: &str) -> u64 {
  repo_path
    .chars()
    .fold(0u64, |hash, c| (hash * 31 + u64::from(c)) % 2_147_483_647)
}

fn base36(mut n: u64) -> String {
  let digits = b"0123456789abcdefghijklmnopqrstuvwxyz";
  let mut out = Vec::new();
  loop {
    out.push(digits[(n % 36) as usize]);
    n /= 36;
    if n == 0 {
      break;
    }
  }
  out.reverse();
  String::from_utf8(out).unwrap_or_default()
}

/// `agentPtySessionId`: PTYs are shared by every window, so the id carries
/// a hash of the repository path.
pub fn agent_pty_id(repo_path: &str, session_id: i64) -> String {
  format!("session-{}-{session_id}", base36(repo_hash(repo_path)))
}

/// `paneSessionId`: supporting repositories' ids are offset so they don't
/// clash with the main repository's.
pub fn pane_session_id(repo_path: &str, main_repo_path: &str, session_id: i64) -> i64 {
  if repo_path == main_repo_path {
    session_id
  } else {
    (repo_hash(repo_path) as i64 + 1) * 1_000_000 + session_id
  }
}

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
  pub fn push(&self, chunk: &str) -> Vec<String> {
    let from_process = {
      let mut pending = self.pending_echo.lock_or_recover();
      let (left, process) = consume_echo(&pending, chunk);
      *pending = left;
      !process.is_empty()
    };
    let outputs = {
      let mut emulator = self.emulator.lock_or_recover();
      emulator.feed(chunk.as_bytes());
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
        for reply in terminal.push(&chunk) {
          let _ = writer.write_pty(&pty, &reply);
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
    assert_eq!(repo_hash("/repo"), {
      let mut hash = 0u64;
      for c in "/repo".chars() {
        hash = (hash * 31 + c as u64) % 2_147_483_647;
      }
      hash
    });
    assert_eq!(
      agent_pty_id("/repo", 7),
      format!("session-{}-7", base36(repo_hash("/repo")))
    );
    assert_eq!(base36(0), "0");
    assert_eq!(base36(35), "z");
    assert_eq!(base36(36), "10");
    assert_eq!(pane_session_id("/repo", "/repo", 3), 3);
    assert_eq!(
      pane_session_id("/api", "/repo", 3),
      (repo_hash("/api") as i64 + 1) * 1_000_000 + 3
    );
    assert_eq!(shell_pty_id("/a/b c", 5), "shell--a-b-c-5");
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
}
