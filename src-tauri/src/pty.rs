use crate::lock_ext::LockExt;
use crate::result_ext::ResultExt;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::sync::{Arc, Mutex};
use std::thread;

/// Receives each chunk of a session's output, as the bytes read.
pub type PtyOutput = Box<dyn Fn(&[u8]) + Send + 'static>;

/// Process a chunk of bytes, handling incomplete UTF-8 sequences at boundaries.
///
/// - `pending`: mutable buffer containing incomplete bytes from the previous chunk
/// - `new_bytes`: the new bytes read from the PTY
///
/// Returns a valid UTF-8 String, potentially leaving trailing incomplete bytes in `pending`.
#[derive(Default)]
pub(crate) struct Utf8StreamDecoder {
  pending: Vec<u8>,
}

impl Utf8StreamDecoder {
  pub(crate) fn new() -> Self {
    Self::default()
  }

  pub(crate) fn push(&mut self, new_bytes: &[u8]) -> String {
    self.pending.extend_from_slice(new_bytes);
    let mut output = String::new();

    loop {
      match std::str::from_utf8(&self.pending) {
        Ok(valid) => {
          output.push_str(valid);
          self.pending.clear();
          break;
        }
        Err(error) => {
          let valid_up_to = error.valid_up_to();
          output.push_str(std::str::from_utf8(&self.pending[..valid_up_to]).unwrap_or_default());
          match error.error_len() {
            None => {
              self.pending.drain(..valid_up_to);
              break;
            }
            Some(invalid_len) => {
              output.push('�');
              self.pending.drain(..valid_up_to + invalid_len);
            }
          }
        }
      }
    }

    output
  }

  /// Whether a character is waiting for its remaining bytes.
  pub(crate) fn has_pending(&self) -> bool {
    !self.pending.is_empty()
  }

  pub(crate) fn finish(&mut self) -> String {
    let output = String::from_utf8_lossy(&self.pending).into_owned();
    self.pending.clear();
    output
  }
}

/// Strip ANSI escape sequences from a string.
#[allow(clippy::expect_used, reason = "constant pattern")]
pub fn strip_ansi_codes(s: &str) -> String {
  // Match CSI sequences (including private modes like ?1h), OSC sequences, and charset designations
  static ANSI_RE: OnceLock<Regex> = OnceLock::new();
  let re = ANSI_RE.get_or_init(|| {
    Regex::new(r"\x1b\[[\x20-\x3f]*[\x30-\x3f]*[\x40-\x7e]|\x1b\][^\x07]*\x07|\x1b\([A-Z]|\x1b[=>]")
      .expect("ANSI pattern is a valid regex")
  });
  re.replace_all(s, "").to_string()
}

/// Check if a line's content overlaps with the auto_command text.
/// Uses 20-char sliding window substring matching (char-based, not byte-based).
pub fn line_matches_auto_command(stripped_line: &str, auto_command: &str) -> bool {
  let trimmed = stripped_line.trim();
  let chars: Vec<char> = trimmed.chars().collect();
  if chars.len() < 20 {
    return false;
  }
  for i in 0..=(chars.len() - 20) {
    let window: String = chars[i..i + 20].iter().collect();
    if auto_command.contains(&window) {
      return true;
    }
  }
  false
}

/// Hides the shell's echo of a command typed into a PTY, and nothing else.
///
/// Output before the echo (the shell prompt, blank lines) is hidden. The
/// first line that overlaps the command starts the echo; the shell then
/// echoes each remaining line of the command once, in order (a quoted prompt
/// can span several lines). After the last one, the filter is done and all
/// output passes through untouched, so an agent that repeats the prompt or
/// prints blank lines at startup keeps that output.
pub struct EchoFilter {
  command: String,
  lines: Vec<String>,
  state: EchoState,
}

enum EchoState {
  BeforeEcho,
  Echoing { next_line: usize },
  Done,
}

impl EchoFilter {
  pub fn new(command: &str) -> Self {
    let command = command.trim_end_matches(['\r', '\n']).to_string();
    let lines = command
      .split('\n')
      .map(|line| line.trim_end_matches('\r').trim_end().to_string())
      .collect();
    Self {
      command,
      lines,
      state: EchoState::BeforeEcho,
    }
  }

  /// Whether this filter was built for `command`.
  pub fn is_for(&self, command: &str) -> bool {
    self.command == command.trim_end_matches(['\r', '\n'])
  }

  pub fn seen_echo(&self) -> bool {
    !matches!(self.state, EchoState::BeforeEcho)
  }

  pub fn is_done(&self) -> bool {
    matches!(self.state, EchoState::Done)
  }

  /// Returns true when `stripped_line` (ANSI codes removed) should be shown.
  pub fn show(&mut self, stripped_line: &str) -> bool {
    let text = stripped_line.trim_end();
    match self.state {
      EchoState::BeforeEcho => {
        if line_matches_auto_command(text, &self.command) {
          self.advance(1);
        }
        false
      }
      EchoState::Echoing { next_line } => {
        // The echo of a continuation line may carry a PS2 prompt ("> ",
        // "quote> ") in front of the command text.
        if text.ends_with(self.lines[next_line].as_str())
          || line_matches_auto_command(text, &self.command)
        {
          self.advance(next_line + 1);
          false
        } else {
          self.state = EchoState::Done;
          true
        }
      }
      EchoState::Done => true,
    }
  }

  fn advance(&mut self, next_line: usize) {
    self.state = if next_line >= self.lines.len() {
      EchoState::Done
    } else {
      EchoState::Echoing { next_line }
    };
  }
}

/// Returns every live process's (pid, ppid) pair, read from `ps` rather
/// than `/proc` so this works on both Linux and macOS.
#[cfg(unix)]
fn all_pid_ppid_pairs() -> Vec<(u32, u32)> {
  let Ok(output) = std::process::Command::new("ps")
    .args(["-e", "-o", "pid=,ppid="])
    .output()
  else {
    return Vec::new();
  };
  String::from_utf8_lossy(&output.stdout)
    .lines()
    .filter_map(|line| {
      let mut fields = line.split_whitespace();
      let pid = fields.next()?.parse().ok()?;
      let ppid = fields.next()?.parse().ok()?;
      Some((pid, ppid))
    })
    .collect()
}

/// Walks the live process table for every descendant of `root` (children,
/// grandchildren, ...), breadth-first. A shell under job control puts each
/// job it launches into its own new process group — the group a `setsid()`
/// session leader starts in does not propagate to what it forks — so
/// signaling `root`'s process group misses forked children entirely.
/// Walking parent/child links instead catches everything regardless of how
/// process groups were rearranged.
#[cfg(unix)]
fn descendant_pids(root: u32) -> Vec<u32> {
  let pairs = all_pid_ppid_pairs();
  let mut frontier = vec![root];
  let mut descendants = Vec::new();
  while let Some(parent) = frontier.pop() {
    for &(pid, ppid) in &pairs {
      if ppid == parent && pid != root {
        descendants.push(pid);
        frontier.push(pid);
      }
    }
  }
  descendants
}

/// Terminates a pty child's whole process tree: SIGTERM to every descendant
/// plus the child itself, a short grace period, then SIGKILL for whatever
/// is still alive. Descendants are re-discovered after the grace period
/// (not just re-checked) since a still-alive process may itself have
/// forked more children in the meantime.
#[cfg(unix)]
fn terminate_process_tree(root: u32, child: &mut Box<dyn Child + Send>) {
  const GRACE_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(20);
  const GRACE_PERIOD: std::time::Duration = std::time::Duration::from_millis(200);

  fn signal_all(pids: &[u32], root: u32, signal: libc::c_int) {
    for &pid in pids {
      // SAFETY: `kill` takes plain integers and touches no Rust memory; a
      // pid that already exited only makes it return an error, which is ignored.
      unsafe {
        libc::kill(pid as libc::pid_t, signal);
      }
    }
    // SAFETY: as above.
    unsafe {
      libc::kill(root as libc::pid_t, signal);
    }
  }

  fn any_alive(pids: &[u32]) -> bool {
    pids.iter().any(|&pid| {
      // SAFETY: signal 0 only checks existence; `kill` touches no Rust memory.
      unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
    })
  }

  let descendants = descendant_pids(root);
  signal_all(&descendants, root, libc::SIGTERM);

  let deadline = std::time::Instant::now() + GRACE_PERIOD;
  loop {
    let root_done = matches!(child.try_wait(), Ok(Some(_)) | Err(_));
    if root_done && !any_alive(&descendants) {
      return;
    }
    if std::time::Instant::now() >= deadline {
      break;
    }
    thread::sleep(GRACE_POLL_INTERVAL);
  }

  signal_all(&descendant_pids(root), root, libc::SIGKILL);
}

pub struct PtySession {
  generation: u64,
  writer: Box<dyn Write + Send>,
  master: Box<dyn MasterPty + Send>,
  child: Box<dyn Child + Send>,
  auto_command: Arc<Mutex<Option<String>>>,
  /// Label of the Tauri window that opened this session, so a window's own
  /// teardown can close only its own PTYs (`close_all_for_window`) without
  /// touching another window's terminals. Not required to be set: sessions
  /// created without a caller-facing window (tests, and any future
  /// non-window caller) simply never match a window-scoped close.
  window_label: Option<String>,
}

/// Windows shells (PowerShell/cmd) submit a line on carriage return; a bare `\n`
/// with no preceding `\r` is inserted into PSReadLine's buffer instead of
/// submitting it, so callers on Windows never see command output. Insert the
/// missing `\r` before any bare `\n`, leaving existing `\r\n` untouched.
fn translate_line_endings_for_windows(data: &[u8]) -> Vec<u8> {
  let mut out = Vec::with_capacity(data.len());
  let mut prev = 0u8;
  for &b in data {
    if b == b'\n' && prev != b'\r' {
      out.push(b'\r');
    }
    out.push(b);
    prev = b;
  }
  out
}

impl PtySession {
  pub fn write(&mut self, data: &[u8]) -> std::io::Result<()> {
    if cfg!(windows) {
      let translated = translate_line_endings_for_windows(data);
      self.writer.write_all(&translated)?;
    } else {
      self.writer.write_all(data)?;
    }
    self.writer.flush()
  }

  pub fn resize(&mut self, rows: u16, cols: u16) -> std::io::Result<()> {
    self
      .master
      .resize(PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
      })
      .map_err(std::io::Error::other)
  }

  fn shutdown(&mut self) -> Result<(), String> {
    // `self.child.kill()` below only signals the shell itself: anything it
    // forked (an agent, a subshell, a backgrounded job) is left running,
    // and once the pty and its reader thread are gone it's an orphan with
    // nothing left to reap it. Walk and signal its whole descendant tree
    // first, on Unix, giving it a brief grace period to exit cleanly
    // before force-killing what's left.
    #[cfg(unix)]
    if let Some(pid) = self.process_id() {
      terminate_process_tree(pid, &mut self.child);
    }

    if let Err(err) = self.child.kill() {
      let err_text = err.to_string().to_lowercase();
      if !err_text.contains("no such process") && !err_text.contains("not found") {
        return Err(format!("Failed to kill PTY child: {err}"));
      }
    }

    if let Err(err) = self.child.wait() {
      let err_text = err.to_string().to_lowercase();
      if !err_text.contains("no child processes")
        && !err_text.contains("already")
        && !err_text.contains("not found")
      {
        return Err(format!("Failed to wait PTY child: {err}"));
      }
    }

    Ok(())
  }

  fn process_id(&self) -> Option<u32> {
    self.child.process_id()
  }
}

#[derive(Clone)]
pub struct PtyManager {
  sessions: Arc<Mutex<HashMap<String, PtySession>>>,
  creating: Arc<Mutex<HashSet<String>>>,
  next_generation: Arc<AtomicU64>,
}

impl PtyManager {
  #[allow(
    clippy::new_without_default,
    reason = "constructed once at startup; a Default impl would add nothing"
  )]
  pub fn new() -> Self {
    PtyManager {
      sessions: Arc::new(Mutex::new(HashMap::new())),
      creating: Arc::new(Mutex::new(HashSet::new())),
      next_generation: Arc::new(AtomicU64::new(1)),
    }
  }

  #[allow(
    clippy::too_many_arguments,
    reason = "flat argument list kept to match callers"
  )]
  pub fn create_session(
    &self,
    session_id: String,
    window_label: Option<String>,
    working_dir: Option<String>,
    shell: Option<String>,
    shell_args: Vec<String>,
    initial_command: Option<String>,
    suppress_echo_for: Option<String>,
    callback: PtyOutput,
  ) -> Result<(), String> {
    {
      let mut creating = self.creating.lock_or_recover();
      if !creating.insert(session_id.clone()) {
        return Err(format!(
          "PTY session creation already in progress: {session_id}"
        ));
      }
    }
    let result = self.create_session_inner(
      session_id.clone(),
      window_label,
      working_dir,
      shell,
      shell_args,
      initial_command,
      suppress_echo_for,
      callback,
    );
    self.creating.lock_or_recover().remove(&session_id);
    result
  }

  #[allow(
    clippy::too_many_arguments,
    reason = "flat argument list kept to match callers"
  )]
  fn create_session_inner(
    &self,
    session_id: String,
    window_label: Option<String>,
    working_dir: Option<String>,
    shell: Option<String>,
    shell_args: Vec<String>,
    initial_command: Option<String>,
    suppress_echo_for: Option<String>,
    callback: PtyOutput,
  ) -> Result<(), String> {
    let stale = {
      let mut sessions = self.sessions.lock_or_recover();
      match sessions.get_mut(&session_id) {
        Some(session) => match session.child.try_wait() {
          Ok(Some(_)) => sessions.remove(&session_id),
          Ok(None) => return Err(format!("PTY session already exists: {session_id}")),
          Err(error) => return Err(format!("Failed to inspect PTY child: {error}")),
        },
        None => None,
      }
    };
    if let Some(mut stale) = stale {
      stale.shutdown()?;
    }
    let pty_system = native_pty_system();

    let pair = pty_system
      .openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
      })
      .map_err(|e| e.to_string())?;

    let shell_cmd = shell.unwrap_or_else(|| {
      std::env::var("SHELL").unwrap_or_else(|_| {
        if cfg!(windows) {
          "powershell.exe".to_string()
        } else {
          "/bin/bash".to_string()
        }
      })
    });

    let mut cmd = CommandBuilder::new(&shell_cmd);
    for arg in shell_args {
      cmd.arg(arg);
    }
    if let Some(dir) = working_dir {
      cmd.cwd(dir);
    }
    cmd.env("TERM", "xterm-256color");
    // Lets `treq send` target the terminal pane that spawned the command.
    cmd.env("TREQ_PTY_SESSION_ID", &session_id);

    // Set extended PATH so terminal can find jj, git, claude binaries
    cmd.env("PATH", crate::binary_paths::get_extended_path());

    let mut child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
    drop(pair.slave);

    let mut reader = match pair.master.try_clone_reader() {
      Ok(value) => value,
      Err(error) => {
        child.kill().best_effort();
        child.wait().best_effort();
        return Err(error.to_string());
      }
    };
    let writer = match pair.master.take_writer() {
      Ok(value) => value,
      Err(error) => {
        child.kill().best_effort();
        child.wait().best_effort();
        return Err(error.to_string());
      }
    };
    let master = pair.master;
    let generation = self.next_generation.fetch_add(1, Ordering::Relaxed);

    let auto_command: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(suppress_echo_for));
    let auto_command_reader = auto_command.clone();

    // Store session with master for resizing
    {
      let mut sessions = self.sessions.lock_or_recover();
      sessions.insert(
        session_id.clone(),
        PtySession {
          generation,
          writer,
          master,
          child,
          auto_command,
          window_label,
        },
      );
    }

    // Spawn reader thread
    let reader_sessions = self.sessions.clone();
    let reader_session_id = session_id.clone();
    thread::spawn(move || {
      let mut buffer = [0u8; 8192];
      let mut utf8_decoder = Utf8StreamDecoder::new();
      let mut line_buffer = String::new();
      let mut suppressed_tail = String::new();
      let mut echo_filter: Option<EchoFilter> = None;
      const MAX_FILTER_BUFFER: usize = 32 * 1024;

      loop {
        match reader.read(&mut buffer) {
          Ok(0) => {
            // EOF: flush any pending bytes
            {
              let data = utf8_decoder.finish();
              if !data.is_empty() {
                line_buffer.push_str(&data);
              }
            }
            // Flush remaining line buffer
            if !line_buffer.is_empty() {
              callback(line_buffer.as_bytes());
            }
            break;
          }
          Ok(n) => {
            // Output nothing reads or rewrites goes on as the bytes read; the
            // emulator takes a character split between reads in stride.
            if !cfg!(windows)
              && !utf8_decoder.has_pending()
              && auto_command_reader.lock_or_recover().is_none()
            {
              callback(&buffer[..n]);
              continue;
            }
            let mut data = utf8_decoder.push(&buffer[..n]);
            if data.is_empty() {
              continue;
            }

            // Windows console apps running under ConPTY (e.g. PowerShell/PSReadLine
            // at startup) query the cursor position via a VT100 Device Status Report
            // and block on the pty until something answers. The app's real terminal
            // (xterm.js) answers this itself; this raw reader has no such emulator
            // attached, so answer it here with a fixed position to unblock the child.
            const CURSOR_POS_QUERY: &str = "\x1b[6n";
            if cfg!(windows) && data.contains(CURSOR_POS_QUERY) {
              let mut sessions = reader_sessions.lock_or_recover();
              if let Some(session) = sessions.get_mut(&reader_session_id) {
                session.write(b"\x1b[1;1R").best_effort();
              }
              drop(sessions);
              data = data.replace(CURSOR_POS_QUERY, "");
              if data.is_empty() {
                continue;
              }
            }

            // Check if filtering is active
            let filter_cmd = {
              let guard = auto_command_reader.lock_or_recover();
              guard.clone()
            };

            let Some(filter_cmd) = filter_cmd else {
              // No filter active, pass through directly
              callback(data.as_bytes());
              continue;
            };
            let filter = match echo_filter.as_mut() {
              Some(filter) if filter.is_for(&filter_cmd) => filter,
              _ => echo_filter.insert(EchoFilter::new(&filter_cmd)),
            };

            // Filtering is active: buffer and process line by line
            line_buffer.push_str(&data);
            if !filter.seen_echo() {
              suppressed_tail.push_str(&data);
              if suppressed_tail.len() > MAX_FILTER_BUFFER {
                let start = suppressed_tail.len() - MAX_FILTER_BUFFER;
                let start = suppressed_tail.ceil_char_boundary(start);
                callback(suppressed_tail.split_off(start).as_bytes());
                suppressed_tail.clear();
                line_buffer.clear();
                *auto_command_reader.lock_or_recover() = None;
                echo_filter = None;
                continue;
              }
            }

            while let Some(newline_pos) = line_buffer.find('\n') {
              let line: String = line_buffer.drain(..=newline_pos).collect();

              if filter.show(&strip_ansi_codes(&line)) {
                callback(line.as_bytes());
              }
              if filter.seen_echo() {
                suppressed_tail.clear();
              }

              if filter.is_done() {
                *auto_command_reader.lock_or_recover() = None;
                if !line_buffer.is_empty() {
                  callback(std::mem::take(&mut line_buffer).as_bytes());
                }
                break;
              }
            }
            if echo_filter.as_ref().is_some_and(EchoFilter::is_done) {
              echo_filter = None;
            }
          }
          Err(_) => break,
        }
      }
      let removed = {
        let mut sessions = reader_sessions.lock_or_recover();
        if sessions.get(&reader_session_id).map(|s| s.generation) == Some(generation) {
          sessions.remove(&reader_session_id)
        } else {
          None
        }
      };
      if let Some(mut session) = removed {
        session.shutdown().best_effort();
      }
    });

    if let Some(command) = initial_command {
      let manager = self.clone();
      let initial_session_id = session_id;
      thread::spawn(move || {
        thread::sleep(std::time::Duration::from_millis(100));
        manager
          .write_to_generation(&initial_session_id, generation, &format!("{command}\n"))
          .ignore_closed();
      });
    }

    Ok(())
  }

  fn write_to_generation(
    &self,
    session_id: &str,
    generation: u64,
    data: &str,
  ) -> Result<(), String> {
    let mut sessions = self.sessions.lock_or_recover();
    match sessions.get_mut(session_id) {
      Some(session) if session.generation == generation => {
        session.write(data.as_bytes()).map_err(|e| e.to_string())
      }
      _ => Err("Session not found".to_string()),
    }
  }

  pub fn write_to_session(&self, session_id: &str, data: &str) -> Result<(), String> {
    let mut sessions = self.sessions.lock_or_recover();
    if let Some(session) = sessions.get_mut(session_id) {
      session.write(data.as_bytes()).map_err(|e| e.to_string())
    } else {
      Err("Session not found".to_string())
    }
  }

  pub fn resize_session(&self, session_id: &str, rows: u16, cols: u16) -> Result<(), String> {
    let mut sessions = self.sessions.lock_or_recover();
    if let Some(session) = sessions.get_mut(session_id) {
      session.resize(rows, cols).map_err(|e| e.to_string())
    } else {
      Err("Session not found".to_string())
    }
  }

  pub fn close_session(&self, session_id: &str) -> Result<(), String> {
    let mut session = {
      let mut sessions = self.sessions.lock_or_recover();
      sessions.remove(session_id)
    };

    if let Some(ref mut session) = session {
      session.shutdown()?;
    }

    Ok(())
  }

  /// Terminates every live session. Used for application-exit cleanup, so
  /// PTY children (and their reader threads) don't outlive the Tauri
  /// backend when a window closes without going through `pty_close` first
  /// (dashboard teardown, native "Close Window", full app quit).
  pub fn close_all(&self) {
    let drained: Vec<PtySession> = {
      let mut sessions = self.sessions.lock_or_recover();
      sessions.drain().map(|(_, session)| session).collect()
    };
    for mut session in drained {
      session.shutdown().best_effort();
    }
  }

  /// Terminates every session opened by the given window, leaving other
  /// windows' sessions untouched. Sessions created with no window label
  /// (tests, or any caller outside the normal Tauri command path) never
  /// match and are never closed by this.
  pub fn close_all_for_window(&self, window_label: &str) {
    let drained: Vec<PtySession> = {
      let mut sessions = self.sessions.lock_or_recover();
      #[allow(
        clippy::needless_collect,
        reason = "ends the shared borrow before removing"
      )]
      let ids: Vec<String> = sessions
        .iter()
        .filter(|(_, session)| session.window_label.as_deref() == Some(window_label))
        .map(|(id, _)| id.clone())
        .collect();
      ids
        .into_iter()
        .filter_map(|id| sessions.remove(&id))
        .collect()
    };
    for mut session in drained {
      session.shutdown().best_effort();
    }
  }

  pub fn set_auto_command(&self, session_id: &str, command: &str) -> Result<(), String> {
    let sessions = self.sessions.lock_or_recover();
    if let Some(session) = sessions.get(session_id) {
      let mut guard = session.auto_command.lock_or_recover();
      *guard = Some(command.to_string());
      Ok(())
    } else {
      Err("Session not found".to_string())
    }
  }

  pub fn session_exists(&self, session_id: &str) -> bool {
    let stale = {
      let mut sessions = self.sessions.lock_or_recover();
      match sessions.get_mut(session_id) {
        Some(session) => match session.child.try_wait() {
          Ok(None) => return true,
          Ok(Some(_)) | Err(_) => sessions.remove(session_id),
        },
        None => None,
      }
    };
    if let Some(mut session) = stale {
      session.shutdown().best_effort();
    }
    false
  }

  #[doc(hidden)]
  pub fn session_process_id(&self, session_id: &str) -> Option<u32> {
    let sessions = self.sessions.lock_or_recover();
    sessions
      .get(session_id)
      .and_then(|session| session.process_id())
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::sync::mpsc;
  use std::time::{Duration, Instant};

  fn shell() -> Option<String> {
    if cfg!(windows) {
      None
    } else {
      Some("/bin/sh".to_string())
    }
  }

  fn shown(filter: &mut EchoFilter, lines: &[&str]) -> Vec<String> {
    lines
      .iter()
      .filter(|line| filter.show(line))
      .map(|line| line.to_string())
      .collect()
  }

  #[test]
  fn echo_filter_hides_the_prompt_and_echo_then_shows_everything() {
    let mut filter = EchoFilter::new("claude --permission-mode plan -- 'Rework the ranking'\r");
    let shown = shown(
      &mut filter,
      &[
        "$ ",
        "",
        "$ claude --permission-mode plan -- 'Rework the ranking'",
        "> Rework the ranking",
        "",
        "claude --permission-mode plan -- 'Rework the ranking'",
      ],
    );
    assert_eq!(
      shown,
      [
        "> Rework the ranking",
        "",
        "claude --permission-mode plan -- 'Rework the ranking'"
      ]
    );
    assert!(filter.is_done());
  }

  #[test]
  fn echo_filter_hides_each_continuation_line_of_a_multiline_command() {
    let mut filter =
      EchoFilter::new("claude --model=opus -- 'Rework it\n\nLinear issue ENG-101'\r");
    let shown = shown(
      &mut filter,
      &[
        "% claude --model=opus -- 'Rework it",
        "quote> ",
        "quote> Linear issue ENG-101'",
        "Welcome",
      ],
    );
    assert_eq!(shown, ["Welcome"]);
  }

  #[test]
  fn echo_filter_ends_when_the_echo_stops_matching() {
    let mut filter = EchoFilter::new("claude --model=opus -- 'first line\nsecond line'");
    assert!(!filter.show("$ claude --model=opus -- 'first line"));
    assert!(filter.show("error: unterminated quote"));
    assert!(filter.is_done());
  }

  #[test]
  fn echo_filter_knows_which_command_it_was_built_for() {
    let filter = EchoFilter::new("claude --model=opus\r");
    assert!(filter.is_for("claude --model=opus"));
    assert!(!filter.is_for("codex"));
  }

  #[test]
  fn utf8_stream_decoder_preserves_characters_split_across_chunks() {
    let mut decoder = Utf8StreamDecoder::new();
    let separator = "⎯".as_bytes();

    assert_eq!(decoder.push(&separator[..1]), "");
    assert_eq!(decoder.push(&separator[1..2]), "");
    assert_eq!(decoder.push(&separator[2..]), "⎯");
    assert_eq!(decoder.finish(), "");
  }

  #[test]
  fn utf8_stream_decoder_recovers_after_invalid_bytes() {
    let mut decoder = Utf8StreamDecoder::new();

    assert_eq!(decoder.push(&[b'a', 0xff, b'b']), "a�b");
    assert_eq!(decoder.push("⎯".as_bytes()), "⎯");
  }

  #[test]
  fn rejects_duplicate_live_session_id() {
    let manager = PtyManager::new();
    manager
      .create_session(
        "duplicate".into(),
        None,
        None,
        shell(),
        Vec::new(),
        None,
        None,
        Box::new(|_| {}),
      )
      .unwrap();

    let duplicate = manager.create_session(
      "duplicate".into(),
      None,
      None,
      shell(),
      Vec::new(),
      None,
      None,
      Box::new(|_| {}),
    );

    assert!(duplicate.is_err());
    assert!(manager.session_exists("duplicate"));
    manager.close_session("duplicate").unwrap();
  }

  #[test]
  fn removes_session_after_child_eof() {
    let manager = PtyManager::new();
    manager
      .create_session(
        "eof".into(),
        None,
        None,
        shell(),
        Vec::new(),
        Some("exit".into()),
        None,
        Box::new(|_| {}),
      )
      .unwrap();

    // PowerShell's cold-start time on CI Windows runners can exceed 2s on its own,
    // before it even processes the queued "exit" — give it more headroom there.
    let timeout = if cfg!(windows) {
      Duration::from_secs(30)
    } else {
      Duration::from_secs(2)
    };
    let deadline = Instant::now() + timeout;
    while manager.session_exists("eof") && Instant::now() < deadline {
      thread::sleep(Duration::from_millis(10));
    }
    assert!(!manager.session_exists("eof"));
  }

  #[test]
  fn close_all_terminates_every_session() {
    let manager = PtyManager::new();
    for id in ["close-all-a", "close-all-b"] {
      manager
        .create_session(
          id.into(),
          None,
          None,
          shell(),
          Vec::new(),
          None,
          None,
          Box::new(|_| {}),
        )
        .unwrap();
    }
    assert!(manager.session_exists("close-all-a"));
    assert!(manager.session_exists("close-all-b"));

    manager.close_all();

    assert!(!manager.session_exists("close-all-a"));
    assert!(!manager.session_exists("close-all-b"));
  }

  #[test]
  fn close_all_for_window_only_closes_that_windows_sessions() {
    let manager = PtyManager::new();
    manager
      .create_session(
        "win-a-1".into(),
        Some("window-a".into()),
        None,
        shell(),
        Vec::new(),
        None,
        None,
        Box::new(|_| {}),
      )
      .unwrap();
    manager
      .create_session(
        "win-b-1".into(),
        Some("window-b".into()),
        None,
        shell(),
        Vec::new(),
        None,
        None,
        Box::new(|_| {}),
      )
      .unwrap();
    manager
      .create_session(
        "no-window".into(),
        None,
        None,
        shell(),
        Vec::new(),
        None,
        None,
        Box::new(|_| {}),
      )
      .unwrap();

    manager.close_all_for_window("window-a");

    assert!(!manager.session_exists("win-a-1"));
    assert!(manager.session_exists("win-b-1"));
    assert!(manager.session_exists("no-window"));

    manager.close_session("win-b-1").unwrap();
    manager.close_session("no-window").unwrap();
  }

  #[cfg(unix)]
  #[test]
  fn close_session_terminates_background_descendants() {
    let manager = PtyManager::new();
    let (tx, rx) = mpsc::channel::<String>();
    manager
      .create_session(
        "descendant".into(),
        None,
        None,
        shell(),
        Vec::new(),
        // Backgrounds a long-lived grandchild and prints its pid, so the
        // test can confirm it (not just the shell) is gone after close.
        Some("sleep 30 & echo GRANDCHILD_PID:$!".into()),
        None,
        Box::new(move |chunk| {
          let _ = tx.send(String::from_utf8_lossy(chunk).into_owned());
        }),
      )
      .unwrap();

    // The marker can land split across separate reads (e.g. "GRAND" then
    // "CHILD_PID:123\n"), so accumulate before searching rather than
    // matching against each chunk in isolation. It can also appear twice
    // (the pty's line-echo of the typed command, unexpanded, ahead of the
    // shell's real output), so take the first occurrence that parses as a
    // pid rather than assuming the first occurrence is the real one.
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut output = String::new();
    let mut grandchild_pid: Option<i32> = None;
    while grandchild_pid.is_none() && Instant::now() < deadline {
      if let Ok(chunk) = rx.recv_timeout(Duration::from_millis(200)) {
        output.push_str(&chunk);
        grandchild_pid = output
          .split("GRANDCHILD_PID:")
          .skip(1)
          .find_map(|rest| rest.split_whitespace().next().and_then(|s| s.parse().ok()));
      }
    }
    let grandchild_pid = grandchild_pid.expect("shell printed its background pid");

    // Alive prior to close: kill(pid, 0) checks existence without signaling.
    // SAFETY: signal 0 only checks existence; `kill` touches no Rust memory.
    assert_eq!(unsafe { libc::kill(grandchild_pid, 0) }, 0);

    manager.close_session("descendant").unwrap();

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut still_alive = true;
    while Instant::now() < deadline {
      // SAFETY: signal 0 only checks existence; `kill` touches no Rust memory.
      if unsafe { libc::kill(grandchild_pid, 0) } != 0 {
        still_alive = false;
        break;
      }
      thread::sleep(Duration::from_millis(20));
    }
    assert!(
      !still_alive,
      "background descendant {grandchild_pid} survived session close"
    );
  }

  #[test]
  fn schedules_initial_command_without_blocking_creation() {
    let manager = PtyManager::new();
    // Resolve the cached login-shell PATH first; the app warms it at startup.
    crate::binary_paths::get_extended_path();
    let started = Instant::now();
    manager
      .create_session(
        "prompt".into(),
        None,
        None,
        shell(),
        Vec::new(),
        Some("printf ready".into()),
        None,
        Box::new(|_| {}),
      )
      .unwrap();
    assert!(started.elapsed() < Duration::from_millis(50));
    manager.close_session("prompt").unwrap();
  }

  #[test]
  fn echo_suppression_releases_output_after_bounded_buffer() {
    let manager = PtyManager::new();
    let (tx, rx) = mpsc::channel();
    // `yes`/`head` aren't available under PowerShell; emit the same >32KB of
    // filler in one shot so the filter's MAX_FILTER_BUFFER release path is
    // exercised the same way on both platforms.
    let command = if cfg!(windows) {
      // Many small writes flush through ConPTY more reliably than one very long
      // line, which can sit behind console line-wrap handling before it's sent.
      "for($i=0;$i -lt 1000;$i++){Write-Output ('x' * 50)}".to_string()
    } else {
      "yes x | head -c 40000".to_string()
    };
    manager
      .create_session(
        "bounded-filter".into(),
        None,
        None,
        shell(),
        Vec::new(),
        Some(command),
        Some("echo-that-will-never-appear-0123456789".into()),
        Box::new(move |chunk| {
          let _ = tx.send(String::from_utf8_lossy(chunk).into_owned());
        }),
      )
      .unwrap();

    // Windows CI runs this alongside ~300 other tests contending for CPU/IO, on
    // top of PowerShell's own slower cold-start; give it much more headroom.
    let timeout = if cfg!(windows) {
      Duration::from_secs(30)
    } else {
      Duration::from_secs(2)
    };
    assert!(rx.recv_timeout(timeout).is_ok());
    manager.close_session("bounded-filter").unwrap();
  }
}
