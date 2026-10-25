//! VM-local persistent PTY supervisor (mobile PRD Phase 7, item 1).
//!
//! Backs the `pty-remote` CLI surface (`start`/`attach`/`resize`/`stop`/
//! `list`), the shared infrastructure both the mobile client (Phase 7) and
//! desktop (Phase 8) consume so a full interactive terminal survives an app
//! suspension, a dropped connection, or an app relaunch without losing the
//! running remote process.
//!
//! ## Backends
//!
//! Each machine picks one ([`PtyBackend`], passed as `--backend`):
//!
//! - **Daemon** (default): Treq's own `core::pty_daemon`, started on demand
//!   as `treq pty-remote daemon`. It holds each session's PTY and screen, and
//!   replays the screen plus the last 250 lines on reattach. The host needs
//!   nothing but `treq`.
//! - **tmux**: one detached tmux session per (repo, workspace, label), for
//!   users who would rather not run the daemon. A host without tmux gets a
//!   clear `dependency_error` instead of a session that cannot survive a
//!   reconnect.
//!
//! Both key sessions by [`session_name`], derived from repo, workspace and
//! label, so `start` and `attach` are naturally idempotent (reattach or
//! create) and `list` is a filter over the backend's own session names: no
//! Treq-owned index can go stale. Resize needs no dedicated wire message on
//! either: the SSH PTY channel's `window_change_request` reaches the attached
//! client, which tmux follows and `treq pty-remote attach` forwards.
//! [`resize_session`] exists for resizing without attaching.
//!
//! "Reset terminals" ([`reset_sessions`]) ends every Treq session on the
//! host under a backend; for the daemon it also stops the daemon.

use crate::core::remote_pty::{build_launch_program, PtyLaunchSpec};
use crate::result_ext::ResultExt;
use serde::{Deserialize, Serialize};
use std::process::Command;

/// Which program holds a machine's persistent sessions. Chosen per machine
/// in the app (`SshEndpoint::terminal_backend`) and passed to every
/// `pty-remote` call as `--backend`, so the host keeps no setting of its own.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PtyBackend {
  /// Treq's own daemon (`core::pty_daemon`). Needs nothing on the host but
  /// `treq`.
  #[default]
  Daemon,
  /// tmux, for users who would rather not run the Treq daemon.
  Tmux,
}

impl PtyBackend {
  pub fn is_default(&self) -> bool {
    *self == Self::default()
  }

  pub fn as_str(self) -> &'static str {
    match self {
      Self::Daemon => "daemon",
      Self::Tmux => "tmux",
    }
  }

  /// Parses `--backend`. Absent means the daemon.
  pub fn parse(value: Option<&str>) -> Result<Self, String> {
    match value.map(str::trim) {
      None | Some("") | Some("daemon") => Ok(Self::Daemon),
      Some("tmux") => Ok(Self::Tmux),
      Some(other) => Err(format!(
        "invalid_arguments: unknown terminal backend '{other}' (expected daemon or tmux)"
      )),
    }
  }
}

/// Whether tmux is installed, for [`PtyBackend::Tmux`].
pub fn tmux_available() -> bool {
  // Every remote attach is a new process, and the login shell that
  // `detect_binary` asks for the full PATH takes a shell start. tmux found
  // on the PATH the process already has needs no other answer.
  #[cfg(unix)]
  if std::env::var_os("PATH")
    .and_then(|path| crate::binary_paths::detect_binary_in("tmux", path))
    .is_some()
  {
    return true;
  }
  crate::binary_paths::detect_binary("tmux").is_some()
}

/// One VM-local persistent PTY session's identity. The session name is
/// derived deterministically from `repo`, `workspace` and `label`, so
/// repeated `start` calls for the same triple are idempotent: the same
/// underlying session is reused rather than a duplicate created.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PtySessionInfo {
  pub session_name: String,
  pub workspace: String,
  pub label: String,
  /// True when the backend reports the session currently exists (it may
  /// have exited between calls, e.g. the shell/agent inside it quit).
  pub running: bool,
  /// The name a user gave the session (see [`rename_session`]).
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub title: Option<String>,
  /// Seconds since the session last printed anything, by the host's clock,
  /// so a client can tell a working agent from one waiting for input.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub idle_secs: Option<u64>,
}

/// The tmux user option holding a session's title.
const TITLE_OPTION: &str = "@treq-title";

/// How long [`rename_session`] waits for a session that is still starting:
/// `RENAME_WAIT_TRIES` checks, `RENAME_WAIT_STEP` apart.
const RENAME_WAIT_TRIES: u32 = 30;
const RENAME_WAIT_STEP: std::time::Duration = std::time::Duration::from_millis(100);

/// Longest title kept, in characters.
const TITLE_MAX_CHARS: usize = 80;

/// One line of `tmux list-sessions` as [`list_sessions`] formats it. tmux
/// prints control characters (a tab) as `_`, so fields are split by `|`,
/// which session names never contain; the title comes last and may.
const TMUX_LIST_FORMAT: &str = "#{session_name}|#{window_activity}|#{@treq-title}";

/// Prefix shared by every session this module creates. Listing filters the
/// backend's sessions by it, so unrelated tmux sessions on the host
/// are never reported or touched.
const SESSION_PREFIX: &str = "treq-pty-";

/// Number of hex characters of the repository hash kept in a session name.
/// Long enough that two repositories on one VM do not collide in practice,
/// short enough to keep `tmux ls` readable.
const REPO_SCOPE_LEN: usize = 12;

/// Stable short identifier for the repository a session belongs to.
///
/// Workspace ids are only unique within one repository. A session name
/// built from the workspace alone would let two repositories on the same VM
/// list, attach to, or stop each other's sessions. The path is canonicalized
/// first so `/srv/repo`, `/srv/repo/` and a symlink to it share one scope. A
/// path that cannot be canonicalized (for example a deleted checkout) falls
/// back to the trimmed string, which is still stable, so sessions left
/// behind can be listed and stopped.
pub fn repo_scope(repo: &str) -> String {
  use sha2::{Digest, Sha256};
  let trimmed = repo.trim();
  let canonical = std::fs::canonicalize(trimmed)
    .map(|path| path.to_string_lossy().into_owned())
    .unwrap_or_else(|_| {
      let without_slash = trimmed.trim_end_matches('/');
      if without_slash.is_empty() {
        trimmed.to_string()
      } else {
        without_slash.to_string()
      }
    });
  let digest = Sha256::digest(canonical.as_bytes());
  let mut hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
  hex.truncate(REPO_SCOPE_LEN);
  hex
}

/// Encodes one name component so it contains only `[A-Za-z0-9_]`. Every
/// other byte, `_` included, becomes `_xx` (two lowercase hex digits). The
/// result is safe to pass to `tmux -t` without quoting, and
/// `-` stays free as an unambiguous separator, so a workspace such as
/// `feat-x` or a label such as `shell-2` round-trips exactly through
/// [`parse_session_name`].
fn encode_component(value: &str) -> String {
  let mut out = String::with_capacity(value.len());
  for byte in value.bytes() {
    if byte.is_ascii_alphanumeric() {
      out.push(byte as char);
    } else {
      out.push_str(&format!("_{byte:02x}"));
    }
  }
  out
}

/// Reverses [`encode_component`]. Returns `None` for input this module did
/// not produce (a truncated escape, invalid hex, or invalid UTF-8), so a
/// foreign session that happens to share the prefix is ignored.
fn decode_component(value: &str) -> Option<String> {
  let bytes = value.as_bytes();
  let mut out = Vec::with_capacity(bytes.len());
  let mut i = 0;
  while i < bytes.len() {
    let byte = bytes[i];
    if byte == b'_' {
      let hex = value.get(i + 1..i + 3)?;
      out.push(u8::from_str_radix(hex, 16).ok()?);
      i += 3;
    } else if byte.is_ascii_alphanumeric() {
      out.push(byte);
      i += 1;
    } else {
      return None;
    }
  }
  String::from_utf8(out).ok()
}

/// Builds the session name for a (repo, workspace, label)
/// triple: `treq-pty-<repo scope>-<workspace>-<label>`, with the workspace
/// and label encoded by [`encode_component`].
pub fn session_name(repo: &str, workspace: &str, label: &str) -> String {
  session_name_for_scope(&repo_scope(repo), workspace, label)
}

fn session_name_for_scope(scope: &str, workspace: &str, label: &str) -> String {
  format!(
    "{SESSION_PREFIX}{scope}-{}-{}",
    encode_component(workspace),
    encode_component(label)
  )
}

/// A session name split back into its repository scope, workspace and label.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedSessionName {
  scope: String,
  workspace: String,
  label: String,
}

/// Parses a name built by [`session_name`]. Returns `None` for any session
/// this module did not create, including names from older builds that had
/// no repository scope: those cannot be attributed to a repository, so they
/// are never shown to (or stopped from) the wrong one.
fn parse_session_name(name: &str) -> Option<ParsedSessionName> {
  let rest = name.strip_prefix(SESSION_PREFIX)?;
  let mut parts = rest.split('-');
  let scope = parts.next()?;
  let workspace = parts.next()?;
  let label = parts.next()?;
  if parts.next().is_some()
    || scope.len() != REPO_SCOPE_LEN
    || !scope.bytes().all(|b| b.is_ascii_hexdigit())
  {
    return None;
  }
  Some(ParsedSessionName {
    scope: scope.to_string(),
    workspace: decode_component(workspace)?,
    label: decode_component(label)?,
  })
}

/// A session as a backend lists it: its name, plus when it last printed
/// and its title.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ListedSession {
  name: String,
  /// Unix seconds of the last output.
  last_output: Option<u64>,
  title: Option<String>,
}

impl From<String> for ListedSession {
  fn from(name: String) -> Self {
    Self {
      name,
      ..Self::default()
    }
  }
}

/// Parses one line printed with [`TMUX_LIST_FORMAT`]. An unset title prints
/// as an empty field.
fn parse_tmux_list_line(line: &str) -> ListedSession {
  let mut fields = line.splitn(3, '|');
  let name = fields.next().unwrap_or_default().to_string();
  let last_output = fields.next().and_then(|t| t.trim().parse().ok());
  let title = fields
    .next()
    .map(str::trim)
    .filter(|t| !t.is_empty())
    .map(str::to_string);
  ListedSession {
    name,
    last_output,
    title,
  }
}

/// Keeps only the sessions that belong to `scope` (and to `workspace`, when
/// given) out of the backend's raw session list. Split out from
/// [`list_sessions`] so the filtering is testable without a tmux server.
fn filter_sessions<S: Into<ListedSession>>(
  sessions: impl IntoIterator<Item = S>,
  scope: &str,
  workspace: Option<&str>,
  now: u64,
) -> Vec<PtySessionInfo> {
  sessions
    .into_iter()
    .map(Into::into)
    .filter_map(|listed: ListedSession| {
      let parsed = parse_session_name(&listed.name)?;
      if parsed.scope != scope || workspace.is_some_and(|w| w != parsed.workspace) {
        return None;
      }
      Some(PtySessionInfo {
        session_name: listed.name,
        workspace: parsed.workspace,
        label: parsed.label,
        running: true,
        title: listed.title,
        idle_secs: listed.last_output.map(|at| now.saturating_sub(at)),
      })
    })
    .collect()
}

fn no_tmux_error() -> String {
  "dependency_error: tmux is not installed on this host; install it, or switch the machine's terminals to the Treq daemon".to_string()
}

fn require_tmux() -> Result<(), String> {
  if tmux_available() {
    Ok(())
  } else {
    Err(no_tmux_error())
  }
}

fn running_info(name: String, workspace: &str, label: &str) -> PtySessionInfo {
  PtySessionInfo {
    session_name: name,
    workspace: workspace.to_string(),
    label: label.to_string(),
    running: true,
    title: None,
    idle_secs: None,
  }
}

/// Starts (or, if one already exists, leaves running) a detached persistent
/// session for `repo`/`workspace`/`label` in `remote_dir`, running `launch`
/// (a login shell or an allow-listed agent binary). `launch` is always a
/// typed [`PtyLaunchSpec`], never a raw frontend-supplied command string, so
/// its program is built by [`build_launch_program`], which quotes every
/// dynamic component exactly as `core::remote_pty`'s direct-launch path
/// does. No code path here lets a caller-supplied string reach the shell
/// unquoted.
#[expect(
  clippy::too_many_arguments,
  reason = "flat argument list kept to match callers"
)]
pub fn start_session(
  backend: PtyBackend,
  repo: &str,
  remote_dir: &str,
  workspace: &str,
  label: &str,
  launch: &PtyLaunchSpec,
  cols: u16,
  rows: u16,
) -> Result<PtySessionInfo, String> {
  let name = session_name(repo, workspace, label);
  match backend {
    PtyBackend::Daemon => {
      daemon::start(&daemon::paths()?, &name, remote_dir, launch, cols, rows)?;
    }
    PtyBackend::Tmux => {
      require_tmux()?;
      if !tmux_session_exists(&name) {
        tmux_start(&name, remote_dir, launch, cols, rows)?;
      }
    }
  }
  Ok(running_info(name, workspace, label))
}

fn tmux_start(
  name: &str,
  remote_dir: &str,
  launch: &PtyLaunchSpec,
  cols: u16,
  rows: u16,
) -> Result<(), String> {
  let command = build_launch_program(launch);
  let quoted_dir = crate::core::remote::shell_quote(remote_dir);
  let shell_cmd = format!("cd {quoted_dir} && exec {command}");
  let status = Command::new("tmux")
    .args([
      "new-session",
      "-d",
      "-s",
      name,
      "-x",
      &cols.to_string(),
      "-y",
      &rows.to_string(),
      &shell_cmd,
      // The pane is the whole terminal: no tmux status line under it.
      ";",
      "set-option",
      "status",
      "off",
    ])
    .status()
    .map_err(|e| format!("dependency_error: Failed to spawn tmux: {e}"))?;
  if !status.success() {
    return Err(format!(
      "dependency_error: tmux new-session exited with {status}"
    ));
  }
  Ok(())
}

fn tmux_session_exists(name: &str) -> bool {
  Command::new("tmux")
    .args(["has-session", "-t", &format!("={name}")])
    .status()
    .map(|s| s.success())
    .unwrap_or(false)
}

/// Every session tmux reports, or none when tmux is missing or has no
/// server running.
fn tmux_sessions() -> Vec<ListedSession> {
  if !tmux_available() {
    return vec![];
  }
  let Ok(output) = Command::new("tmux")
    // -u: print titles as UTF-8 whatever the host's locale; tmux replaces
    // non-ASCII with `_` otherwise.
    .args(["-u", "list-sessions", "-F", TMUX_LIST_FORMAT])
    .output()
  else {
    return vec![];
  };
  // tmux exits non-zero ("no server running") when there are no sessions
  // at all — that is an empty list, not an error.
  if !output.status.success() {
    return vec![];
  }
  String::from_utf8_lossy(&output.stdout)
    .lines()
    .map(parse_tmux_list_line)
    .collect()
}

/// Lists persistent sessions for `repo`, optionally narrowed to
/// `workspace`. Reads backend-reported state directly (no Treq-owned index
/// file to go stale): every session this module creates follows
/// [`session_name`], so listing is a filter over the backend's own list.
/// Never starts the daemon.
pub fn list_sessions(
  backend: PtyBackend,
  repo: &str,
  workspace: Option<&str>,
) -> Result<Vec<PtySessionInfo>, String> {
  let sessions = match backend {
    PtyBackend::Daemon => daemon::sessions(&daemon::paths()?)?,
    PtyBackend::Tmux => tmux_sessions(),
  };
  let now = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map_or(0, |d| d.as_secs());
  Ok(filter_sessions(sessions, &repo_scope(repo), workspace, now))
}

/// A title as stored: one line without control characters, trimmed and at
/// most [`TITLE_MAX_CHARS`] long. `None` when nothing is left.
pub fn clean_title(title: &str) -> Option<String> {
  let line: String = title
    .chars()
    .map(|c| if c.is_control() { ' ' } else { c })
    .collect();
  let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
  let line: String = line.chars().take(TITLE_MAX_CHARS).collect();
  let line = line.trim_end().to_string();
  (!line.is_empty()).then_some(line)
}

/// Names a session; an empty `title` clears the name. The backend keeps the
/// title with the session (tmux as a user option, the daemon in memory),
/// so it goes when the session ends.
pub fn rename_session(
  backend: PtyBackend,
  repo: &str,
  workspace: &str,
  label: &str,
  title: &str,
) -> Result<Option<String>, String> {
  let name = session_name(repo, workspace, label);
  let title = clean_title(title);
  match backend {
    PtyBackend::Daemon => {
      let paths = daemon::paths()?;
      // A client names a new session right after attaching, which can be
      // before the attach has created it: give it a moment to appear.
      for _ in 0..RENAME_WAIT_TRIES {
        match daemon::rename(&paths, &name, title.as_deref()) {
          Err(e) if e.starts_with("session_not_found") => std::thread::sleep(RENAME_WAIT_STEP),
          Err(e) => return Err(e),
          Ok(()) => return Ok(title),
        }
      }
      Err(format!("Terminal '{label}' is not running"))
    }
    PtyBackend::Tmux => {
      require_tmux()?;
      for _ in 0..RENAME_WAIT_TRIES {
        if tmux_session_exists(&name) {
          break;
        }
        std::thread::sleep(RENAME_WAIT_STEP);
      }
      // `=` for an exact match; the trailing `:` makes it a session target,
      // which set-option needs (a bare `=name` reads as a pane and fails).
      let target = format!("={name}:");
      let mut args = vec!["set-option", "-t", target.as_str()];
      match &title {
        Some(title) => args.extend([TITLE_OPTION, title.as_str()]),
        None => args.extend(["-u", TITLE_OPTION]),
      }
      let status = Command::new("tmux")
        .args(&args)
        .status()
        .map_err(|e| format!("dependency_error: Failed to run tmux: {e}"))?;
      if !status.success() {
        return Err(format!("Terminal '{label}' is not running"));
      }
      Ok(title)
    }
  }
}

/// Sessions under either backend, for counts that should not depend on
/// which one a machine uses (a workspace's running terminals).
pub fn list_all_sessions(repo: &str, workspace: Option<&str>) -> Vec<PtySessionInfo> {
  [PtyBackend::Daemon, PtyBackend::Tmux]
    .into_iter()
    .flat_map(|backend| list_sessions(backend, repo, workspace).unwrap_or_default())
    .collect()
}

/// Stops (kills) a persistent session. Idempotent: stopping a session that
/// does not exist is not an error.
pub fn stop_session(
  backend: PtyBackend,
  repo: &str,
  workspace: &str,
  label: &str,
) -> Result<(), String> {
  let name = session_name(repo, workspace, label);
  match backend {
    PtyBackend::Daemon => daemon::stop(&daemon::paths()?, &name),
    PtyBackend::Tmux => {
      if tmux_available() {
        tmux_kill(&name);
      }
      Ok(())
    }
  }
}

fn tmux_kill(name: &str) {
  // `=` asks tmux for an exact match. Without it tmux falls back to prefix
  // matching, so stopping `...-shell` could kill `...-shell_2d2`.
  Command::new("tmux")
    .args(["kill-session", "-t", &format!("={name}")])
    .status()
    .best_effort();
}

/// What `treq pty-remote status` reports: what holds Treq's terminals on
/// this host right now, whichever backend the app has picked for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalStatus {
  /// The Treq terminal daemon, while it runs. It starts with the first
  /// terminal and stops a minute after the last one ends.
  pub daemon: Option<DaemonStatus>,
  pub tmux_installed: bool,
  /// Treq's sessions in tmux, from machines set to tmux.
  pub tmux_sessions: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonStatus {
  pub pid: u32,
  /// The `treq` version the daemon runs.
  pub version: String,
  pub sessions: usize,
}

/// Reads both backends without starting the daemon.
pub fn terminal_status() -> Result<TerminalStatus, String> {
  let tmux_installed = tmux_available();
  Ok(TerminalStatus {
    daemon: daemon::status(&daemon::paths()?)?,
    tmux_installed,
    tmux_sessions: tmux_sessions()
      .iter()
      .filter(|session| parse_session_name(&session.name).is_some())
      .count(),
  })
}

/// One line per backend, for `treq pty-remote status` without `--format json`.
pub fn describe_status(status: &TerminalStatus) -> String {
  let plural = |n: usize| if n == 1 { "" } else { "s" };
  let daemon = match &status.daemon {
    Some(d) => format!(
      "Treq daemon: running (pid {}, treq {}), {} session{}",
      d.pid,
      d.version,
      d.sessions,
      plural(d.sessions)
    ),
    None => "Treq daemon: not running (starts with the first terminal)".to_string(),
  };
  let tmux = if status.tmux_installed {
    format!(
      "tmux: installed, {} Treq session{}",
      status.tmux_sessions,
      plural(status.tmux_sessions)
    )
  } else {
    "tmux: not installed".to_string()
  };
  format!("{daemon}\n{tmux}")
}

/// Ends every Treq session on the host under `backend`, whatever its
/// repository: the machine's "Reset terminals". For the daemon this also
/// stops the daemon and clears its directory; for tmux it leaves sessions
/// Treq did not create alone.
pub fn reset_sessions(backend: PtyBackend) -> Result<(), String> {
  match backend {
    PtyBackend::Daemon => daemon::reset(&daemon::paths()?),
    PtyBackend::Tmux => {
      for session in tmux_sessions()
        .into_iter()
        .filter(|session| parse_session_name(&session.name).is_some())
      {
        tmux_kill(&session.name);
      }
      Ok(())
    }
  }
}

/// Pastes `text` into a running session as one bracketed paste (so a TUI
/// such as Claude Code takes it as typed input, newlines included), then
/// presses Enter when `submit`. Errors when the session is not running.
pub fn paste_text(
  backend: PtyBackend,
  repo: &str,
  workspace: &str,
  label: &str,
  text: &str,
  submit: bool,
) -> Result<(), String> {
  let name = session_name(repo, workspace, label);
  match backend {
    PtyBackend::Daemon => daemon::paste(&daemon::paths()?, &name, label, text, submit),
    PtyBackend::Tmux => {
      require_tmux()?;
      if !tmux_session_exists(&name) {
        return Err(format!("session_not_found: {label} is not running"));
      }
      let target = format!("={name}:");
      let buffer = format!("treq-paste-{name}");
      run_tmux_with_input(&["load-buffer", "-b", &buffer, "-"], text)?;
      run_tmux(&["paste-buffer", "-p", "-d", "-b", &buffer, "-t", &target])?;
      if submit {
        // A TUI reads Enter straight after a paste as part of it.
        std::thread::sleep(std::time::Duration::from_millis(SUBMIT_DELAY_MS));
        run_tmux(&["send-keys", "-t", &target, "Enter"])?;
      }
      Ok(())
    }
  }
}

/// How long `paste_text` waits between the paste and Enter.
const SUBMIT_DELAY_MS: u64 = 300;

fn run_tmux(args: &[&str]) -> Result<(), String> {
  let output = Command::new("tmux")
    .args(args)
    .output()
    .map_err(|e| format!("dependency_error: Failed to run tmux: {e}"))?;
  tmux_result(output)
}

fn run_tmux_with_input(args: &[&str], input: &str) -> Result<(), String> {
  use std::io::Write as _;
  let mut child = Command::new("tmux")
    .args(args)
    .stdin(std::process::Stdio::piped())
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::piped())
    .spawn()
    .map_err(|e| format!("dependency_error: Failed to run tmux: {e}"))?;
  if let Some(mut stdin) = child.stdin.take() {
    stdin
      .write_all(input.as_bytes())
      .map_err(|e| format!("Failed to send text to tmux: {e}"))?;
  }
  let output = child
    .wait_with_output()
    .map_err(|e| format!("dependency_error: Failed to run tmux: {e}"))?;
  tmux_result(output)
}

fn tmux_result(output: std::process::Output) -> Result<(), String> {
  if output.status.success() {
    return Ok(());
  }
  let stderr = String::from_utf8_lossy(&output.stderr);
  if stderr.contains("can't find") || stderr.contains("no server running") {
    return Err("session_not_found: that terminal is not running".into());
  }
  Err(format!("tmux failed: {}", stderr.trim()))
}

/// Explicitly resizes a session's virtual terminal, independent of any
/// attached client. Normally unnecessary (both backends follow the attached
/// client's window), but useful to pre-size a session before the first
/// attach.
pub fn resize_session(
  backend: PtyBackend,
  repo: &str,
  workspace: &str,
  label: &str,
  cols: u16,
  rows: u16,
) -> Result<(), String> {
  let name = session_name(repo, workspace, label);
  match backend {
    PtyBackend::Daemon => daemon::resize(&daemon::paths()?, &name, cols, rows),
    PtyBackend::Tmux => {
      require_tmux()?;
      let status = Command::new("tmux")
        .args([
          "resize-window",
          "-t",
          &format!("={name}"),
          "-x",
          &cols.to_string(),
          "-y",
          &rows.to_string(),
        ])
        .status()
        .map_err(|e| format!("dependency_error: Failed to resize tmux session: {e}"))?;
      if !status.success() {
        return Err(format!(
          "dependency_error: tmux resize-window exited with {status}"
        ));
      }
      Ok(())
    }
  }
}

/// Builds the literal command line an SSH PTY channel execs to attach to
/// (creating first if necessary) a persistent session — the `pty-remote`
/// analogue of `core::remote_pty::build_launch_command`. `launch` is a typed
/// [`PtyLaunchSpec`], never a raw frontend-supplied command string: its
/// program is built by [`build_launch_program`], and every other dynamic
/// component (directory, session name) is quoted with `shell_quote` exactly
/// as that function does.
///
/// Both backends attach-or-create in one command line, so the
/// mobile/desktop client does not need to know in advance whether the
/// session is running: tmux through `new-session -A`, the daemon through
/// `treq pty-remote attach`, which starts the session when it is missing.
#[expect(
  clippy::too_many_arguments,
  reason = "flat argument list kept to match callers"
)]
pub fn build_attach_command(
  backend: PtyBackend,
  repo: &str,
  remote_dir: &str,
  workspace: &str,
  label: &str,
  launch: &PtyLaunchSpec,
  cols: u16,
  rows: u16,
) -> Result<String, String> {
  match backend {
    PtyBackend::Daemon => {
      let treq = std::env::current_exe()
        .map_err(|e| format!("dependency_error: Failed to find treq on this host: {e}"))?;
      Ok(daemon_attach_command(
        &treq.to_string_lossy(),
        repo,
        remote_dir,
        workspace,
        label,
        launch,
        cols,
        rows,
      ))
    }
    PtyBackend::Tmux => {
      require_tmux()?;
      Ok(tmux_attach_command(
        &session_name(repo, workspace, label),
        remote_dir,
        launch,
        cols,
        rows,
      ))
    }
  }
}

#[expect(
  clippy::too_many_arguments,
  reason = "flat argument list kept to match callers"
)]
fn daemon_attach_command(
  treq: &str,
  repo: &str,
  remote_dir: &str,
  workspace: &str,
  label: &str,
  launch: &PtyLaunchSpec,
  cols: u16,
  rows: u16,
) -> String {
  use crate::core::remote::shell_quote;
  let payload = crate::core::remote::pty_launch_payload(remote_dir, launch, cols, rows);
  format!(
    "exec {} pty-remote attach --repo {} --workspace {} --target {} --value {}",
    shell_quote(treq),
    shell_quote(repo),
    shell_quote(workspace),
    shell_quote(label),
    shell_quote(&payload),
  )
}

fn tmux_attach_command(
  name: &str,
  remote_dir: &str,
  launch: &PtyLaunchSpec,
  cols: u16,
  rows: u16,
) -> String {
  let command = build_launch_program(launch);
  let quoted_dir = crate::core::remote::shell_quote(remote_dir);
  let quoted_name = crate::core::remote::shell_quote(name);
  // The pane is the whole terminal: no tmux status line under it.
  format!(
    "cd {quoted_dir} && exec tmux new-session -A -s {quoted_name} -x {cols} -y {rows} {command} \\; set-option status off"
  )
}

/// `treq pty-remote attach`: what [`build_attach_command`]'s daemon command
/// line runs on the host. Bridges this process's terminal to the session,
/// starting it first when it is not running, until the session ends.
pub fn run_daemon_attach(
  repo: &str,
  remote_dir: &str,
  workspace: &str,
  label: &str,
  launch: &PtyLaunchSpec,
  cols: u16,
  rows: u16,
) -> Result<(), String> {
  daemon::attach(
    &session_name(repo, workspace, label),
    remote_dir,
    launch,
    cols,
    rows,
  )
}

/// `treq pty-remote daemon`: serves until reset or idle.
pub fn run_daemon() -> Result<(), String> {
  daemon::serve()
}

/// The daemon backend. On a host without unix sockets every call says the
/// daemon is unavailable; Treq supports Linux and macOS hosts.
#[cfg(unix)]
mod daemon {
  use super::{build_launch_program, DaemonStatus, ListedSession, PtyLaunchSpec};
  use crate::core::pty_daemon::{self, DaemonPaths, Request, StartSpec};

  pub(super) fn paths() -> Result<DaemonPaths, String> {
    DaemonPaths::default_paths()
  }

  fn spec(remote_dir: &str, launch: &PtyLaunchSpec, cols: u16, rows: u16) -> StartSpec {
    StartSpec {
      dir: remote_dir.to_string(),
      program: build_launch_program(launch),
      cols,
      rows,
    }
  }

  fn call(paths: &DaemonPaths, request: &Request) -> Result<serde_json::Value, String> {
    pty_daemon::call(pty_daemon::connect_or_spawn(paths)?, request)
  }

  pub(super) fn start(
    paths: &DaemonPaths,
    name: &str,
    remote_dir: &str,
    launch: &PtyLaunchSpec,
    cols: u16,
    rows: u16,
  ) -> Result<(), String> {
    call(
      paths,
      &Request::Start {
        name: name.to_string(),
        spec: spec(remote_dir, launch, cols, rows),
      },
    )
    .map(drop)
  }

  pub(super) fn sessions(paths: &DaemonPaths) -> Result<Vec<ListedSession>, String> {
    let Some(value) = pty_daemon::request_if_running(paths, &Request::List)? else {
      return Ok(vec![]);
    };
    let listed: Vec<pty_daemon::Listed> = serde_json::from_value(value)
      .map_err(|e| format!("daemon_error: unexpected session list: {e}"))?;
    Ok(
      listed
        .into_iter()
        .map(|l| ListedSession {
          name: l.name,
          last_output: l.last_output,
          title: l.title,
        })
        .collect(),
    )
  }

  pub(super) fn rename(paths: &DaemonPaths, name: &str, title: Option<&str>) -> Result<(), String> {
    let request = Request::Rename {
      name: name.to_string(),
      title: title.map(str::to_string),
    };
    match pty_daemon::request_if_running(paths, &request)? {
      Some(_) => Ok(()),
      None => Err(format!("session_not_found: {name} is not running")),
    }
  }

  pub(super) fn stop(paths: &DaemonPaths, name: &str) -> Result<(), String> {
    pty_daemon::request_if_running(
      paths,
      &Request::Stop {
        name: name.to_string(),
      },
    )
    .map(drop)
  }

  pub(super) fn paste(
    paths: &DaemonPaths,
    name: &str,
    label: &str,
    text: &str,
    submit: bool,
  ) -> Result<(), String> {
    let request = Request::Paste {
      name: name.to_string(),
      text: text.to_string(),
      submit,
    };
    match pty_daemon::request_if_running(paths, &request)? {
      Some(_) => Ok(()),
      None => Err(format!("session_not_found: {label} is not running")),
    }
  }

  pub(super) fn resize(
    paths: &DaemonPaths,
    name: &str,
    cols: u16,
    rows: u16,
  ) -> Result<(), String> {
    call(
      paths,
      &Request::Resize {
        name: name.to_string(),
        cols,
        rows,
      },
    )
    .map(drop)
  }

  pub(super) fn reset(paths: &DaemonPaths) -> Result<(), String> {
    pty_daemon::reset(paths)
  }

  pub(super) fn status(paths: &DaemonPaths) -> Result<Option<DaemonStatus>, String> {
    let Some(value) = pty_daemon::request_if_running(paths, &Request::Hello)? else {
      return Ok(None);
    };
    let hello: pty_daemon::Hello = serde_json::from_value(value)
      .map_err(|e| format!("daemon_error: unexpected hello from the terminal daemon: {e}"))?;
    Ok(Some(DaemonStatus {
      pid: hello.pid,
      version: hello.version,
      sessions: hello.sessions,
    }))
  }

  pub(super) fn attach(
    name: &str,
    remote_dir: &str,
    launch: &PtyLaunchSpec,
    cols: u16,
    rows: u16,
  ) -> Result<(), String> {
    pty_daemon::run_attach(&paths()?, name, spec(remote_dir, launch, cols, rows))
  }

  pub(super) fn serve() -> Result<(), String> {
    pty_daemon::serve(&paths()?).map(drop)
  }
}

#[cfg(not(unix))]
mod daemon {
  use super::*;

  pub(super) struct DaemonPaths;

  fn unsupported<T>() -> Result<T, String> {
    Err("dependency_error: the Treq terminal daemon runs on Linux and macOS hosts only; use tmux mode".to_string())
  }

  pub(super) fn paths() -> Result<DaemonPaths, String> {
    unsupported()
  }
  pub(super) fn start(
    _: &DaemonPaths,
    _: &str,
    _: &str,
    _: &PtyLaunchSpec,
    _: u16,
    _: u16,
  ) -> Result<(), String> {
    unsupported()
  }
  pub(super) fn sessions(_: &DaemonPaths) -> Result<Vec<ListedSession>, String> {
    unsupported()
  }
  pub(super) fn rename(_: &DaemonPaths, _: &str, _: Option<&str>) -> Result<(), String> {
    unsupported()
  }
  pub(super) fn stop(_: &DaemonPaths, _: &str) -> Result<(), String> {
    unsupported()
  }
  pub(super) fn paste(_: &DaemonPaths, _: &str, _: &str, _: &str, _: bool) -> Result<(), String> {
    unsupported()
  }
  pub(super) fn resize(_: &DaemonPaths, _: &str, _: u16, _: u16) -> Result<(), String> {
    unsupported()
  }
  pub(super) fn reset(_: &DaemonPaths) -> Result<(), String> {
    unsupported()
  }
  pub(super) fn status(_: &DaemonPaths) -> Result<Option<DaemonStatus>, String> {
    Ok(None)
  }
  pub(super) fn attach(_: &str, _: &str, _: &PtyLaunchSpec, _: u16, _: u16) -> Result<(), String> {
    unsupported()
  }
  pub(super) fn serve() -> Result<(), String> {
    unsupported()
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::remote_pty::RemoteAgentId;

  #[test]
  fn session_name_encodes_unsafe_characters_into_a_tmux_safe_name() {
    let name = session_name("/srv/repo", "workspace/1", "my label!");
    let rest = name.strip_prefix(SESSION_PREFIX).unwrap();
    assert!(rest
      .bytes()
      .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'));
    // tmux treats `.` and `:` as target separators.
    assert!(!name.contains('.') && !name.contains(':'));
    assert!(name.ends_with("-workspace_2f1-my_20label_21"));
  }

  #[test]
  fn parse_session_name_round_trips_workspaces_and_labels_containing_dashes() {
    let name = session_name("/srv/repo", "feat-x_y", "shell-2");
    let parsed = parse_session_name(&name).unwrap();
    assert_eq!(parsed.scope, repo_scope("/srv/repo"));
    assert_eq!(parsed.workspace, "feat-x_y");
    assert_eq!(parsed.label, "shell-2");
  }

  #[test]
  fn parse_session_name_rejects_legacy_and_foreign_names() {
    assert_eq!(parse_session_name("treq-pty-ws1-term"), None);
    assert_eq!(parse_session_name("my-own-session"), None);
    assert_eq!(parse_session_name("treq-pty-0123456789ab-ws-_zz"), None);
  }

  #[test]
  fn session_name_differs_for_the_same_workspace_in_two_repos() {
    assert_ne!(
      session_name("/srv/repo-a", "root", "shell"),
      session_name("/srv/repo-b", "root", "shell")
    );
  }

  #[test]
  fn repo_scope_ignores_trailing_slashes_and_symlinks() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("repo");
    std::fs::create_dir(&real).unwrap();
    let real_str = real.to_str().unwrap();
    assert_eq!(repo_scope(real_str), repo_scope(&format!("{real_str}/")));

    #[cfg(unix)]
    {
      let link = dir.path().join("link");
      std::os::unix::fs::symlink(&real, &link).unwrap();
      assert_eq!(repo_scope(real_str), repo_scope(link.to_str().unwrap()));
    }

    assert_eq!(repo_scope("/gone/repo/"), repo_scope("/gone/repo"));
    assert_eq!(repo_scope("/gone/repo").len(), REPO_SCOPE_LEN);
  }

  #[test]
  fn filter_sessions_keeps_only_the_requested_repo_and_workspace() {
    let names = vec![
      session_name("/srv/a", "root", "shell"),
      session_name("/srv/a", "feat", "claude"),
      session_name("/srv/b", "root", "shell"),
      "treq-pty-root-shell".to_string(),
      "unrelated".to_string(),
    ];

    let all_a = filter_sessions(names.clone(), &repo_scope("/srv/a"), None, 0);
    let labels: Vec<_> = all_a
      .iter()
      .map(|s| (s.workspace.as_str(), s.label.as_str()))
      .collect();
    assert_eq!(labels, vec![("root", "shell"), ("feat", "claude")]);

    let root_a = filter_sessions(names.clone(), &repo_scope("/srv/a"), Some("root"), 0);
    assert_eq!(root_a.len(), 1);
    assert_eq!(
      root_a[0].session_name,
      session_name("/srv/a", "root", "shell")
    );

    let root_b = filter_sessions(names, &repo_scope("/srv/b"), Some("root"), 0);
    assert_eq!(root_b.len(), 1);
    assert_eq!(
      root_b[0].session_name,
      session_name("/srv/b", "root", "shell")
    );
  }

  #[test]
  fn tmux_list_lines_carry_title_and_idle_time() {
    let name = session_name("/srv/a", "root", "claude-1");
    let lines = [
      format!("{name}|1761350000|Fix login redirect"),
      format!("{}|1761350001|", session_name("/srv/a", "root", "shell-2")),
    ];
    let sessions = filter_sessions(
      lines.iter().map(|l| parse_tmux_list_line(l)),
      &repo_scope("/srv/a"),
      Some("root"),
      1761350010,
    );
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0].title.as_deref(), Some("Fix login redirect"));
    assert_eq!(sessions[0].idle_secs, Some(10));
    assert_eq!(sessions[1].title, None);
    // A title may itself contain the separator: only the first two split.
    let listed = parse_tmux_list_line(&format!("{name}|5|a | b"));
    assert_eq!(listed.title.as_deref(), Some("a | b"));
  }

  #[test]
  fn titles_are_one_trimmed_bounded_line() {
    assert_eq!(
      clean_title("  Fix\nlogin\t redirect "),
      Some("Fix login redirect".into())
    );
    assert_eq!(clean_title(" \n "), None);
    assert_eq!(
      clean_title(&"x".repeat(200)).unwrap().chars().count(),
      TITLE_MAX_CHARS
    );
  }

  #[test]
  fn sessions_from_older_hosts_have_no_title() {
    let info: PtySessionInfo = serde_json::from_value(serde_json::json!({
      "session_name": "s", "workspace": "w", "label": "l", "running": true
    }))
    .unwrap();
    assert_eq!((info.title, info.idle_secs), (None, None));
  }

  #[test]
  fn backend_flag_defaults_to_the_daemon_and_rejects_unknown_names() {
    assert_eq!(PtyBackend::parse(None), Ok(PtyBackend::Daemon));
    assert_eq!(PtyBackend::parse(Some("daemon")), Ok(PtyBackend::Daemon));
    assert_eq!(PtyBackend::parse(Some("tmux")), Ok(PtyBackend::Tmux));
    assert!(PtyBackend::parse(Some("screen"))
      .unwrap_err()
      .starts_with("invalid_arguments"));
    assert_eq!(
      serde_json::to_string(&PtyBackend::Tmux).unwrap(),
      "\"tmux\""
    );
  }

  #[test]
  fn daemon_attach_command_runs_treq_attach_with_every_value_quoted() {
    let malicious = "'; rm -rf / #";
    let launch = PtyLaunchSpec::Agent {
      agent: RemoteAgentId::Claude,
      args: vec![malicious.to_string()],
    };
    let command = daemon_attach_command(
      "/opt/treq bin/treq",
      "/srv/repo",
      &format!("/tmp/x{malicious}"),
      "7",
      &format!("term{malicious}"),
      &launch,
      80,
      24,
    );
    assert!(
      command.starts_with("exec '/opt/treq bin/treq' pty-remote attach --repo '/srv/repo' --workspace '7' --target 'term'\\''"),
      "{command}"
    );
    assert!(!command.contains(&format!(" {malicious}")));
    // The payload the attach parses back is the one start would send.
    let value = command.rsplit_once("--value ").unwrap().1;
    assert!(value.starts_with('\'') && value.ends_with('\''));
  }

  #[cfg(unix)]
  mod daemon_backend {
    use super::super::daemon;
    use super::*;
    use crate::core::pty_daemon::{self, DaemonPaths, Request};

    struct Served {
      paths: DaemonPaths,
      handle: Option<std::thread::JoinHandle<Result<bool, String>>>,
      _dir: tempfile::TempDir,
    }

    impl Served {
      fn start() -> Self {
        let dir = tempfile::Builder::new()
          .prefix("tsup")
          .tempdir_in("/tmp")
          .unwrap();
        let paths = DaemonPaths::new(dir.path().join("pty"));
        let serving = paths.clone();
        let handle = std::thread::spawn(move || pty_daemon::serve(&serving));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while pty_daemon::connect(&paths).unwrap().is_none() {
          assert!(std::time::Instant::now() < deadline, "daemon did not start");
          std::thread::sleep(std::time::Duration::from_millis(10));
        }
        Self {
          paths,
          handle: Some(handle),
          _dir: dir,
        }
      }
    }

    impl Drop for Served {
      fn drop(&mut self) {
        pty_daemon::request_if_running(&self.paths, &Request::Shutdown).ok();
        if let Some(handle) = self.handle.take() {
          handle.join().ok();
        }
      }
    }

    #[test]
    fn listing_without_a_daemon_is_empty_and_starts_nothing() {
      let dir = tempfile::Builder::new()
        .prefix("tsup")
        .tempdir_in("/tmp")
        .unwrap();
      let paths = DaemonPaths::new(dir.path().join("pty"));
      assert!(daemon::sessions(&paths).unwrap().is_empty());
      daemon::stop(&paths, "anything").unwrap();
      assert!(!paths.socket().exists());
    }

    #[test]
    fn start_list_paste_stop_round_trip_scoped_to_the_repository() {
      let served = Served::start();
      let repo = tempfile::tempdir().unwrap();
      let repo_path = repo.path().to_str().unwrap();
      let other = tempfile::tempdir().unwrap();
      let out = repo.path().join("pasted.txt");
      let name = session_name(repo_path, "3", "shell");

      daemon::start(
        &served.paths,
        &name,
        repo_path,
        &PtyLaunchSpec::Shell,
        80,
        24,
      )
      .unwrap();
      // Starting again reuses the session.
      daemon::start(
        &served.paths,
        &name,
        repo_path,
        &PtyLaunchSpec::Shell,
        80,
        24,
      )
      .unwrap();
      let names = daemon::sessions(&served.paths).unwrap();
      let listed = filter_sessions(names.clone(), &repo_scope(repo_path), Some("3"), 0);
      assert_eq!(listed.len(), 1);
      assert_eq!(listed[0].label, "shell");
      assert!(
        filter_sessions(names, &repo_scope(other.path().to_str().unwrap()), None, 0).is_empty()
      );

      std::thread::sleep(std::time::Duration::from_millis(300));
      let command = format!("printf 'a\\nb' > {}", out.display());
      daemon::paste(&served.paths, &name, "shell", &command, true).unwrap();
      let mut pasted = String::new();
      for _ in 0..50 {
        pasted = std::fs::read_to_string(&out).unwrap_or_default();
        if !pasted.is_empty() {
          break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
      }
      assert_eq!(pasted, "a\nb");

      daemon::resize(&served.paths, &name, 100, 40).unwrap();
      daemon::stop(&served.paths, &name).unwrap();
      assert!(daemon::sessions(&served.paths).unwrap().is_empty());
      assert!(daemon::paste(&served.paths, &name, "shell", "x", false)
        .unwrap_err()
        .starts_with("session_not_found"));
    }

    #[test]
    fn the_daemon_names_a_session_and_reports_its_idle_time() {
      let served = Served::start();
      let repo = tempfile::tempdir().unwrap();
      let repo_path = repo.path().to_str().unwrap();
      let name = session_name(repo_path, "3", "claude-1");
      daemon::start(
        &served.paths,
        &name,
        repo_path,
        &PtyLaunchSpec::Shell,
        80,
        24,
      )
      .unwrap();
      daemon::rename(&served.paths, &name, Some("Fix login")).unwrap();
      let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
      let session = loop {
        let listed = filter_sessions(
          daemon::sessions(&served.paths).unwrap(),
          &repo_scope(repo_path),
          Some("3"),
          crate::core::pty_daemon::unix_now(),
        );
        // The shell's prompt is its first output.
        if listed[0].idle_secs.is_some() {
          break listed[0].clone();
        }
        assert!(
          std::time::Instant::now() < deadline,
          "the shell printed nothing"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
      };
      assert_eq!(session.title.as_deref(), Some("Fix login"));
      assert!(session.idle_secs.is_some_and(|s| s < 10));
      daemon::rename(&served.paths, &name, None).unwrap();
      assert_eq!(daemon::sessions(&served.paths).unwrap()[0].title, None);
    }

    #[test]
    fn status_reports_no_daemon_and_starts_none() {
      let dir = tempfile::Builder::new()
        .prefix("tsup")
        .tempdir_in("/tmp")
        .unwrap();
      let paths = DaemonPaths::new(dir.path().join("pty"));
      assert_eq!(daemon::status(&paths).unwrap(), None);
      assert!(!paths.socket().exists());
    }

    #[test]
    fn status_reports_the_running_daemon_and_its_sessions() {
      let served = Served::start();
      daemon::start(
        &served.paths,
        "treq-pty-a",
        "/",
        &PtyLaunchSpec::Shell,
        80,
        24,
      )
      .unwrap();
      let status = daemon::status(&served.paths)
        .unwrap()
        .expect("daemon running");
      assert_eq!(status.sessions, 1);
      assert_eq!(status.version, env!("CARGO_PKG_VERSION"));
      assert_eq!(status.pid, std::process::id());
    }

    #[test]
    fn reset_ends_every_session_and_the_daemon() {
      let mut served = Served::start();
      daemon::start(
        &served.paths,
        "treq-pty-a",
        "/",
        &PtyLaunchSpec::Shell,
        80,
        24,
      )
      .unwrap();
      daemon::reset(&served.paths).unwrap();
      served.handle.take().unwrap().join().unwrap().unwrap();
      assert!(daemon::sessions(&served.paths).unwrap().is_empty());
      assert!(!served.paths.dir().exists());
    }
  }

  #[test]
  fn attach_command_quotes_a_malicious_working_directory() {
    let command = tmux_attach_command(
      &session_name("/srv/repo", "ws1", "term"),
      "/tmp/x'; rm -rf / #",
      &PtyLaunchSpec::Shell,
      80,
      24,
    );
    assert!(command.contains("'/tmp/x'\\''; rm -rf / #'"));
  }

  /// Hardening regression test: `launch` is a typed `PtyLaunchSpec`, not a
  /// raw string, so a malicious agent argument (shell metacharacters,
  /// attempted command chaining) can only ever appear individually
  /// `shell_quote`d in the built command line, never interpreted as shell
  /// syntax.
  #[test]
  fn attach_command_quotes_malicious_agent_arguments() {
    let malicious_arg = "'; rm -rf / #";
    let launch = PtyLaunchSpec::Agent {
      agent: RemoteAgentId::Claude,
      args: vec!["--prompt".to_string(), malicious_arg.to_string()],
    };
    let command = tmux_attach_command(
      &session_name("/srv/repo", "ws1", "term"),
      "/tmp",
      &launch,
      80,
      24,
    );
    assert!(command.contains("claude"));
    assert!(command.contains(" -lc "));
    assert!(!command.contains(&format!(" {malicious_arg} ")));
  }

  #[test]
  fn tmux_mode_errors_clearly_when_tmux_is_missing() {
    // Cannot force `detect_binary` to fail where tmux exists, so this only
    // runs on hosts that genuinely lack it.
    if tmux_available() {
      return;
    }
    let error = build_attach_command(
      PtyBackend::Tmux,
      "/srv/repo",
      "/tmp",
      "ws1",
      "term",
      &PtyLaunchSpec::Shell,
      80,
      24,
    )
    .unwrap_err();
    assert!(error.contains("dependency_error"));
  }

  #[test]
  fn paste_types_text_into_a_running_session_and_submits_it() {
    if !tmux_available() {
      tracing::warn!("skipping: tmux not installed in this environment");
      return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo_path = dir.path().to_str().unwrap();
    let (workspace, label) = ("test-ws-paste", "shell");
    let out = dir.path().join("pasted.txt");

    assert!(paste_text(
      PtyBackend::Tmux,
      repo_path,
      workspace,
      label,
      "echo hi",
      true
    )
    .unwrap_err()
    .contains("session_not_found"));

    start_session(
      PtyBackend::Tmux,
      repo_path,
      repo_path,
      workspace,
      label,
      &PtyLaunchSpec::Shell,
      80,
      24,
    )
    .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(500));
    let command = format!("printf 'a\\nb' > {}", out.display());
    paste_text(
      PtyBackend::Tmux,
      repo_path,
      workspace,
      label,
      &command,
      true,
    )
    .unwrap();
    let mut pasted = String::new();
    for _ in 0..50 {
      pasted = std::fs::read_to_string(&out).unwrap_or_default();
      if !pasted.is_empty() {
        break;
      }
      std::thread::sleep(std::time::Duration::from_millis(100));
    }
    stop_session(PtyBackend::Tmux, repo_path, workspace, label).unwrap();
    assert_eq!(pasted, "a\nb");
  }

  #[test]
  fn start_list_stop_lifecycle_round_trips_against_a_real_tmux_server() {
    if !tmux_available() {
      tracing::warn!("skipping: tmux not installed in this environment");
      return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo_path = dir.path().to_str().unwrap();
    let workspace = "test-ws-pty";
    let label = "term-1";

    // Ensure a clean slate in case a previous failed run left a session.
    let _ = stop_session(PtyBackend::Tmux, repo_path, workspace, label);

    let info = start_session(
      PtyBackend::Tmux,
      repo_path,
      repo_path,
      workspace,
      label,
      &PtyLaunchSpec::Shell,
      80,
      24,
    )
    .unwrap();
    assert!(info.running);
    assert_eq!(info.session_name, session_name(repo_path, workspace, label));

    // Starting again is idempotent: same session, no error.
    let info_again = start_session(
      PtyBackend::Tmux,
      repo_path,
      repo_path,
      workspace,
      label,
      &PtyLaunchSpec::Shell,
      80,
      24,
    )
    .unwrap();
    assert_eq!(info_again.session_name, info.session_name);

    let sessions = list_sessions(PtyBackend::Tmux, repo_path, Some(workspace)).unwrap();
    let listed = sessions
      .iter()
      .find(|s| s.session_name == info.session_name)
      .expect("session listed for its repo");
    assert_eq!(listed.workspace, workspace);
    assert_eq!(listed.label, label);

    // A different repository on the same host does not see the session.
    let other = tempfile::tempdir().unwrap();
    let other_sessions = list_sessions(
      PtyBackend::Tmux,
      other.path().to_str().unwrap(),
      Some(workspace),
    )
    .unwrap();
    assert!(other_sessions.is_empty());

    resize_session(PtyBackend::Tmux, repo_path, workspace, label, 100, 40).unwrap();

    stop_session(PtyBackend::Tmux, repo_path, workspace, label).unwrap();
    let sessions_after = list_sessions(PtyBackend::Tmux, repo_path, Some(workspace)).unwrap();
    assert!(!sessions_after
      .iter()
      .any(|s| s.session_name == info.session_name));

    // Stopping again is idempotent.
    stop_session(PtyBackend::Tmux, repo_path, workspace, label).unwrap();
  }

  #[test]
  fn stop_session_does_not_kill_a_session_whose_name_extends_the_target() {
    if !tmux_available() {
      tracing::warn!("skipping: tmux not installed in this environment");
      return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo_path = dir.path().to_str().unwrap();
    let workspace = "test-ws-prefix";
    let _ = stop_session(PtyBackend::Tmux, repo_path, workspace, "sh-2");
    start_session(
      PtyBackend::Tmux,
      repo_path,
      repo_path,
      workspace,
      "sh-2",
      &PtyLaunchSpec::Shell,
      80,
      24,
    )
    .unwrap();

    // `sh` encodes to a strict prefix of `sh-2`'s name. tmux's default
    // prefix matching would otherwise resolve it to the running session.
    stop_session(PtyBackend::Tmux, repo_path, workspace, "sh").unwrap();
    let sessions = list_sessions(PtyBackend::Tmux, repo_path, Some(workspace)).unwrap();
    assert!(sessions.iter().any(|s| s.label == "sh-2"));

    stop_session(PtyBackend::Tmux, repo_path, workspace, "sh-2").unwrap();
  }

  #[test]
  fn attach_command_reattaches_to_an_existing_session_without_recreating_it() {
    if !tmux_available() {
      tracing::warn!("skipping: tmux not installed in this environment");
      return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo_path = dir.path().to_str().unwrap();
    let workspace = "test-ws-reattach";
    let label = "term";
    let _ = stop_session(PtyBackend::Tmux, repo_path, workspace, label);

    let info = start_session(
      PtyBackend::Tmux,
      repo_path,
      repo_path,
      workspace,
      label,
      &PtyLaunchSpec::Shell,
      80,
      24,
    )
    .unwrap();

    let attach_cmd = build_attach_command(
      PtyBackend::Tmux,
      repo_path,
      repo_path,
      workspace,
      label,
      &PtyLaunchSpec::Shell,
      80,
      24,
    )
    .unwrap();
    assert!(attach_cmd.contains("new-session -A"));
    assert!(attach_cmd.contains(&info.session_name));

    stop_session(PtyBackend::Tmux, repo_path, workspace, label).unwrap();
  }
}
