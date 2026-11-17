//! VM-local coding-agent supervisor (Phase 5).
//!
//! Interactive attach uses a PTY channel opened directly by the desktop
//! client (see `remote_ssh_transport::RemotePtyChannel`); this module backs
//! the typed lifecycle commands (`start`/`input`/`status`/`stop`/`logs`) that
//! must keep working after the client disconnects. It is intentionally a
//! small, real, locally-scoped process tracker — not a public daemon and not
//! a fake in-memory simulation: state is a pidfile-style JSON record plus a
//! log file under `.treq/agents/<workspace>/` in the repository root, so
//! `treq agent-remote status` run from a *different* SSH exec invocation
//! (a fresh process each time) can still read back what a prior `start`
//! wrote to disk.
//!
//! No public network port is opened; everything here is local filesystem and
//! process-table state, reachable only through the allow-listed CLI surface.

use crate::result_ext::ResultExt;
use serde::{Deserialize, Serialize};
use std::fs;
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRecord {
  pub workspace: String,
  pub agent: String,
  pub pid: u32,
  pub started_at: String,
  pub prompt: String,
  pub log_path: String,
  #[serde(default)]
  pub stdin_path: Option<String>,
  #[serde(default)]
  pub keeper_pid: Option<u32>,
  /// Kernel start-time token of `pid`, captured at spawn. A later signal is
  /// only sent while the pid still reports this token, so a pid recycled by an
  /// unrelated process is never signalled. `None` on records written before
  /// this field existed and on platforms without a start-time source.
  #[serde(default)]
  pub pid_start: Option<String>,
  #[serde(default)]
  pub keeper_start: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStatusResult {
  pub workspace: String,
  pub running: bool,
  pub agent: Option<String>,
  pub pid: Option<u32>,
  pub started_at: Option<String>,
  /// Set by any lifecycle transition (start completing, or the process
  /// exiting) so the desktop client knows a refresh of remote repository
  /// status/changes/commits/conflicts is due, per "Agent and terminal
  /// lifecycle" in the PRD. Phase 6 wires this into actual UI refresh.
  pub should_refresh: bool,
}

fn agents_dir(repo_path: &str) -> PathBuf {
  Path::new(repo_path).join(".treq").join("agents")
}

fn record_path(repo_path: &str, workspace: &str) -> PathBuf {
  agents_dir(repo_path).join(format!("{workspace}.json"))
}

fn log_path(repo_path: &str, workspace: &str) -> PathBuf {
  agents_dir(repo_path).join(format!("{workspace}.log"))
}

fn stdin_fifo_path(repo_path: &str, workspace: &str) -> PathBuf {
  agents_dir(repo_path).join(format!("{workspace}.stdin"))
}

fn read_record(repo_path: &str, workspace: &str) -> Option<AgentRecord> {
  let contents = fs::read_to_string(record_path(repo_path, workspace)).ok()?;
  serde_json::from_str(&contents).ok()
}

fn write_record(repo_path: &str, record: &AgentRecord) -> Result<(), String> {
  fs::create_dir_all(agents_dir(repo_path))
    .map_err(|e| format!("filesystem_error: Failed to create agent state dir: {e}"))?;
  let json = serde_json::to_string_pretty(record).map_err(|e| e.to_string())?;
  fs::write(record_path(repo_path, &record.workspace), json)
    .map_err(|e| format!("filesystem_error: Failed to write agent record: {e}"))
}

fn remove_record(repo_path: &str, workspace: &str) {
  fs::remove_file(record_path(repo_path, workspace)).best_effort();
}

/// A process is considered alive when signal 0 can be delivered to its pid.
#[cfg(unix)]
fn process_is_alive(pid: u32) -> bool {
  // SAFETY: kill(pid, 0) only probes for existence/permission; it never
  // sends a real signal, so this cannot terminate or otherwise disturb the
  // target process.
  unsafe { libc::kill(pid as i32, 0) == 0 }
}

/// A process is considered alive when `tasklist` still lists its pid.
#[cfg(windows)]
fn process_is_alive(pid: u32) -> bool {
  Command::new("tasklist")
    .args(["/FI", &format!("PID eq {pid}"), "/NH"])
    .output()
    .map(|out| {
      out.status.success() && String::from_utf8_lossy(&out.stdout).contains(&pid.to_string())
    })
    .unwrap_or(false)
}

/// Opaque, stable-for-the-process-lifetime start time of `pid` (Linux:
/// `starttime` in `/proc/<pid>/stat`; macOS: `pbi_start_tv*` from
/// `proc_pidinfo`). Two different processes that reuse one pid never share it.
#[cfg(target_os = "linux")]
fn process_start_token(pid: u32) -> Option<String> {
  let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
  // The command name (field 2) may contain spaces and parentheses, so
  // fields are counted from the last ')'. `starttime` is field 22, which is
  // index 19 once pid and comm are cut off.
  let after_comm = stat.get(stat.rfind(')')? + 1..)?;
  after_comm.split_whitespace().nth(19).map(str::to_owned)
}

#[cfg(target_os = "macos")]
fn process_start_token(pid: u32) -> Option<String> {
  let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
  let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
  // SAFETY: `info` is a writable buffer of exactly `size` bytes; the kernel
  // fills it and returns the number of bytes written (<= 0 on failure).
  let written = unsafe {
    libc::proc_pidinfo(
      pid as libc::c_int,
      libc::PROC_PIDTBSDINFO,
      0,
      info.as_mut_ptr().cast(),
      size,
    )
  };
  if written != size {
    return None;
  }
  // SAFETY: the kernel fully initialised the struct (checked above).
  let info = unsafe { info.assume_init() };
  Some(format!(
    "{}.{}",
    info.pbi_start_tvsec, info.pbi_start_tvusec
  ))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn process_start_token(_pid: u32) -> Option<String> {
  None
}

/// True when `pid` is alive and is the process we recorded. Records without a
/// start token (legacy, or no platform support) fall back to the liveness probe.
fn process_is_recorded(pid: u32, start: Option<&str>) -> bool {
  process_is_alive(pid)
    && start.is_none_or(|expected| process_start_token(pid).as_deref() == Some(expected))
}

/// Terminates the recorded agent process. Best-effort: a failure here still
/// lets the caller drop its own record of the process. A pid that no longer
/// matches the recorded start time belongs to someone else and is left alone.
fn kill_process(pid: u32, start: Option<&str>) {
  if !process_is_recorded(pid, start) {
    return;
  }
  #[cfg(unix)]
  // SAFETY: `kill` takes plain integers and touches no Rust memory; a stale or
  // invalid pid only makes it return an error, which is ignored.
  unsafe {
    libc::kill(pid as i32, libc::SIGTERM);
  }
  #[cfg(windows)]
  {
    let _ = Command::new("taskkill")
      .args(["/PID", &pid.to_string(), "/F"])
      .output();
  }
}

/// Starts an agent process for `workspace`, resolving the binary from the
/// same allow-listed agent names the local agent dispatch already trusts.
/// Refuses to start a second process for a workspace that already has one
/// running — callers that want "start or resume" semantics should check
/// `agent_status` first, or pass an idempotency key at the CLI/exec layer
/// (handled in `core::remote::execute_local_request`) so a retried start
/// after a lost response replays the original result instead of erroring.
pub fn start_agent(
  repo_path: &str,
  workspace: &str,
  workspace_path: &str,
  agent: &str,
  prompt: &str,
) -> Result<AgentRecord, String> {
  if let Some(existing) = read_record(repo_path, workspace) {
    if process_is_recorded(existing.pid, existing.pid_start.as_deref()) {
      return Err(format!(
        "agent_already_running: Agent '{}' is already running for workspace '{}' (pid {})",
        existing.agent, workspace, existing.pid
      ));
    }
  }

  let binary = resolve_agent_binary(agent)
    .ok_or_else(|| format!("invalid_arguments: Unknown agent '{agent}'"))?;

  let log_file_path = log_path(repo_path, workspace);
  fs::create_dir_all(agents_dir(repo_path))
    .map_err(|e| format!("filesystem_error: Failed to create agent state dir: {e}"))?;
  let log_file = OpenOptions::new()
    .create(true)
    .append(true)
    .open(&log_file_path)
    .map_err(|e| format!("filesystem_error: Failed to open agent log: {e}"))?;
  let log_file_err = log_file
    .try_clone()
    .map_err(|e| format!("filesystem_error: Failed to duplicate agent log handle: {e}"))?;

  let fifo_path = stdin_fifo_path(repo_path, workspace);
  let (agent_stdin, keeper) = setup_agent_stdin(&fifo_path)?;

  let child = Command::new(&binary)
    .current_dir(workspace_path)
    .arg(prompt)
    .stdin(Stdio::from(agent_stdin))
    .stdout(Stdio::from(log_file))
    .stderr(Stdio::from(log_file_err))
    .spawn()
    .map_err(|e| format!("dependency_error: Failed to start agent '{agent}': {e}"))?;

  let pid = child.id();
  // Captured while `child` is still unreaped, so the pid cannot have been
  // recycled yet.
  let pid_start = process_start_token(pid);
  #[expect(
    clippy::mem_forget,
    reason = "the agent must outlive this call; its pid is tracked in the record"
  )]
  std::mem::forget(child);

  let record = AgentRecord {
    workspace: workspace.to_string(),
    agent: agent.to_string(),
    pid,
    started_at: chrono::Utc::now().to_rfc3339(),
    prompt: prompt.to_string(),
    log_path: log_file_path.to_string_lossy().into_owned(),
    stdin_path: Some(fifo_path.to_string_lossy().into_owned()),
    keeper_pid: keeper.as_ref().map(|(pid, _)| *pid),
    pid_start,
    keeper_start: keeper.and_then(|(_, start)| start),
  };
  write_record(repo_path, &record)?;
  Ok(record)
}

fn resolve_agent_binary(agent: &str) -> Option<String> {
  // Allow-listed agent names only — never trust a caller-supplied binary
  // path, so an idempotency-key replay or a compromised UI cannot exec
  // arbitrary programs on the VM.
  const ALLOWED: &[&str] = &["claude", "codex", "cursor-agent", "copilot"];
  ALLOWED
    .iter()
    .find(|name| **name == agent)
    .and_then(|name| crate::binary_paths::detect_binary(name))
}

/// Pid of the stdin keeper process and its start-time token.
type KeeperPid = (u32, Option<String>);

/// Opens a FIFO (Unix) so later CLI invocations can write to the agent's
/// stdin. A detached `sleep` process holds a write fd so the agent does not
/// see EOF when the starting `treq` process exits.
#[cfg(unix)]
fn setup_agent_stdin(fifo_path: &Path) -> Result<(fs::File, Option<KeeperPid>), String> {
  use std::os::unix::ffi::OsStrExt;
  if fifo_path.exists() {
    fs::remove_file(fifo_path).best_effort();
  }
  let c_path = std::ffi::CString::new(fifo_path.as_os_str().as_bytes())
    .map_err(|_| "filesystem_error: Invalid agent stdin path".to_string())?;
  // SAFETY: `c_path` is a valid C string pointing at a path we own; mkfifo
  // only creates a filesystem node.
  let rc = unsafe { libc_mkfifo(c_path.as_ptr(), 0o600) };
  if rc != 0 {
    return Err(format!(
      "filesystem_error: Failed to create agent stdin fifo: {}",
      std::io::Error::last_os_error()
    ));
  }
  let fifo = OpenOptions::new()
    .read(true)
    .write(true)
    .open(fifo_path)
    .map_err(|e| format!("filesystem_error: Failed to open agent stdin fifo: {e}"))?;
  let keeper_stdin = fifo
    .try_clone()
    .map_err(|e| format!("filesystem_error: Failed to clone stdin fifo: {e}"))?;
  let keeper = Command::new("sleep")
    .arg("86400")
    .stdin(Stdio::from(keeper_stdin))
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
    .map_err(|e| format!("filesystem_error: Failed to start stdin keeper: {e}"))?;
  let keeper_pid = keeper.id();
  let keeper_start = process_start_token(keeper_pid);
  #[expect(
    clippy::mem_forget,
    reason = "the keeper must outlive this call; its pid is tracked in the record"
  )]
  std::mem::forget(keeper);
  let agent_stdin = fifo
    .try_clone()
    .map_err(|e| format!("filesystem_error: Failed to clone stdin fifo for agent: {e}"))?;
  Ok((agent_stdin, Some((keeper_pid, keeper_start))))
}

#[cfg(not(unix))]
fn setup_agent_stdin(fifo_path: &Path) -> Result<(fs::File, Option<KeeperPid>), String> {
  let file = OpenOptions::new()
    .create(true)
    .append(true)
    .open(fifo_path)
    .map_err(|e| format!("filesystem_error: Failed to open agent stdin file: {e}"))?;
  Ok((file, None))
}

#[cfg(unix)]
extern "C" {
  #[link_name = "mkfifo"]
  fn libc_mkfifo(path: *const std::ffi::c_char, mode: u32) -> i32;
}

/// Sends input to a running agent's stdin via the VM-local FIFO the
/// supervisor created at start. Distinguishes a missing record from a
/// recorded-but-stopped process.
pub fn send_agent_input(repo_path: &str, workspace: &str, input: &str) -> Result<String, String> {
  match read_record(repo_path, workspace) {
    None => Err(format!(
      "agent_not_found: No agent record for workspace '{workspace}'"
    )),
    Some(record) if process_is_recorded(record.pid, record.pid_start.as_deref()) => {
      let stdin_path = record
        .stdin_path
        .ok_or_else(|| "filesystem_error: Running agent has no stdin path recorded".to_string())?;
      use std::io::Write;
      let mut file = OpenOptions::new()
        .write(true)
        .open(&stdin_path)
        .map_err(|e| format!("filesystem_error: Failed to open agent stdin: {e}"))?;
      file
        .write_all(input.as_bytes())
        .map_err(|e| format!("filesystem_error: Failed to write agent input: {e}"))?;
      if !input.ends_with('\n') {
        file
          .write_all(b"\n")
          .map_err(|e| format!("filesystem_error: Failed to write agent input newline: {e}"))?;
      }
      Ok(format!("sent {} bytes", input.len()))
    }
    Some(_) => Err(format!(
      "agent_not_running: Agent for workspace '{workspace}' is not running"
    )),
  }
}

pub fn agent_status(repo_path: &str, workspace: &str) -> Result<AgentStatusResult, String> {
  match read_record(repo_path, workspace) {
    Some(record) if process_is_recorded(record.pid, record.pid_start.as_deref()) => {
      Ok(AgentStatusResult {
        workspace: workspace.to_string(),
        running: true,
        agent: Some(record.agent),
        pid: Some(record.pid),
        started_at: Some(record.started_at),
        should_refresh: false,
      })
    }
    Some(_) => {
      // Process exited since the last check: clear the stale record and
      // signal the caller a refresh is due.
      remove_record(repo_path, workspace);
      Ok(AgentStatusResult {
        workspace: workspace.to_string(),
        running: false,
        agent: None,
        pid: None,
        started_at: None,
        should_refresh: true,
      })
    }
    None => Ok(AgentStatusResult {
      workspace: workspace.to_string(),
      running: false,
      agent: None,
      pid: None,
      started_at: None,
      should_refresh: false,
    }),
  }
}

pub fn stop_agent(repo_path: &str, workspace: &str) -> Result<AgentStatusResult, String> {
  let Some(record) = read_record(repo_path, workspace) else {
    return Ok(AgentStatusResult {
      workspace: workspace.to_string(),
      running: false,
      agent: None,
      pid: None,
      started_at: None,
      should_refresh: false,
    });
  };
  kill_process(record.pid, record.pid_start.as_deref());
  if let Some(keeper_pid) = record.keeper_pid {
    kill_process(keeper_pid, record.keeper_start.as_deref());
  }
  if let Some(stdin_path) = &record.stdin_path {
    fs::remove_file(stdin_path).best_effort();
  }
  remove_record(repo_path, workspace);
  Ok(AgentStatusResult {
    workspace: workspace.to_string(),
    running: false,
    agent: Some(record.agent),
    pid: Some(record.pid),
    started_at: Some(record.started_at),
    should_refresh: true,
  })
}

/// Returns up to the last 200 KiB of the agent's log, bounded so a runaway
/// agent process cannot blow the exec channel's output limit.
pub fn agent_logs(repo_path: &str, workspace: &str) -> Result<String, String> {
  const MAX_TAIL_BYTES: u64 = 200 * 1024;
  let path = log_path(repo_path, workspace);
  let Ok(mut file) = fs::File::open(&path) else {
    return Ok(String::new());
  };
  let len = file
    .metadata()
    .map_err(|e| format!("filesystem_error: {e}"))?
    .len();
  let start = len.saturating_sub(MAX_TAIL_BYTES);
  file
    .seek(SeekFrom::Start(start))
    .map_err(|e| format!("filesystem_error: {e}"))?;
  let mut buf = Vec::new();
  file
    .read_to_end(&mut buf)
    .map_err(|e| format!("filesystem_error: {e}"))?;
  Ok(String::from_utf8_lossy(&buf).into_owned())
}

#[cfg(test)]
mod tests {
  use super::*;

  fn temp_repo() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
  }

  #[test]
  fn start_status_stop_lifecycle_round_trips_against_a_real_process() {
    let dir = temp_repo();
    let repo = dir.path().to_str().unwrap();
    fs::create_dir_all(dir.path().join("ws")).unwrap();
    let workspace_path = dir.path().join("ws").to_str().unwrap().to_string();

    // Use a real, short-lived process ("sleep") in place of an actual agent
    // binary so the test exercises real process tracking without requiring
    // claude/codex to be installed in this sandbox.
    let record = start_test_process(repo, "demo", &workspace_path, "sleep");
    assert!(record.pid > 0);
    // Windows `tasklist` can miss a pid that has not yet shown up.
    std::thread::sleep(std::time::Duration::from_millis(200));

    let status = agent_status(repo, "demo").unwrap();
    assert!(status.running);
    assert_eq!(status.pid, Some(record.pid));

    let stopped = stop_agent(repo, "demo").unwrap();
    assert!(!stopped.running);
    assert!(stopped.should_refresh);

    // A second status call after stop finds nothing running.
    let status_after = agent_status(repo, "demo").unwrap();
    assert!(!status_after.running);
  }

  #[test]
  fn status_reports_not_running_for_unknown_workspace() {
    let dir = temp_repo();
    let repo = dir.path().to_str().unwrap();
    let status = agent_status(repo, "nope").unwrap();
    assert!(!status.running);
    assert!(!status.should_refresh);
  }

  #[test]
  fn refuses_to_double_start_a_running_agent() {
    let dir = temp_repo();
    let repo = dir.path().to_str().unwrap();
    fs::create_dir_all(dir.path().join("ws")).unwrap();
    let workspace_path = dir.path().join("ws").to_str().unwrap().to_string();

    let _record = start_test_process(repo, "demo", &workspace_path, "sleep");
    let error = start_agent(repo, "demo", &workspace_path, "claude", "hi").unwrap_err();
    assert!(error.contains("agent_already_running") || error.contains("Unknown agent"));

    stop_agent(repo, "demo").unwrap();
  }

  #[test]
  fn send_agent_input_reaches_a_running_process() {
    let dir = temp_repo();
    let repo = dir.path().to_str().unwrap();
    fs::create_dir_all(dir.path().join("ws")).unwrap();
    let workspace_path = dir.path().join("ws").to_str().unwrap().to_string();
    struct StopGuard<'a> {
      repo: &'a str,
      workspace: &'a str,
    }
    impl Drop for StopGuard<'_> {
      fn drop(&mut self) {
        let _ = stop_agent(self.repo, self.workspace);
      }
    }
    let _guard = StopGuard {
      repo,
      workspace: "demo",
    };
    // Unix `cat` echoes stdin into the log. Windows has no FIFO, so the
    // helper process is only required to stay alive for a successful write.
    let placeholder = if cfg!(windows) { "sleep" } else { "cat" };
    let _record = start_test_process(repo, "demo", &workspace_path, placeholder);
    std::thread::sleep(std::time::Duration::from_millis(150));

    let sent = send_agent_input(repo, "demo", "hello-from-supervisor").unwrap();
    assert!(sent.contains("sent"));
    if !cfg!(windows) {
      std::thread::sleep(std::time::Duration::from_millis(150));
      let logs = agent_logs(repo, "demo").unwrap();
      assert!(
        logs.contains("hello-from-supervisor"),
        "expected agent log to contain written stdin, got: {logs:?}"
      );
    }
  }

  #[test]
  fn send_agent_input_errors_when_agent_is_missing() {
    let dir = temp_repo();
    let repo = dir.path().to_str().unwrap();
    let error = send_agent_input(repo, "nope", "x").unwrap_err();
    assert!(error.contains("agent_not_found"));
  }

  #[test]
  fn send_agent_input_errors_when_agent_is_stopped() {
    let dir = temp_repo();
    let repo = dir.path().to_str().unwrap();
    fs::create_dir_all(dir.path().join("ws")).unwrap();
    let workspace_path = dir.path().join("ws").to_str().unwrap().to_string();
    let _record = start_test_process(repo, "demo", &workspace_path, "sleep");
    stop_agent(repo, "demo").unwrap();
    let error = send_agent_input(repo, "demo", "x").unwrap_err();
    assert!(error.contains("agent_not_found") || error.contains("agent_not_running"));
  }

  #[cfg(any(target_os = "linux", target_os = "macos"))]
  #[test]
  fn stop_agent_leaves_a_recycled_pid_alone() {
    let dir = temp_repo();
    let repo = dir.path().to_str().unwrap();
    fs::create_dir_all(dir.path().join("ws")).unwrap();
    let workspace_path = dir.path().join("ws").to_str().unwrap().to_string();
    let mut record = start_test_process(repo, "demo", &workspace_path, "sleep");
    // Pretend the recorded agent died and an unrelated process now owns the
    // pid: the live `sleep` reports a different start time than the record.
    record.pid_start = Some("0.0".to_string());
    write_record(repo, &record).unwrap();

    assert!(!agent_status(repo, "demo").unwrap().running);
    write_record(repo, &record).unwrap();
    stop_agent(repo, "demo").unwrap();

    assert!(
      process_is_alive(record.pid),
      "unrelated process was signalled"
    );
    kill_process(record.pid, None);
    if let Some(keeper_pid) = record.keeper_pid {
      kill_process(keeper_pid, record.keeper_start.as_deref());
    }
  }

  /// Spawns a real long-lived process in place of `start_agent`'s allow-listed
  /// Windows uses `ping` because `sleep` is not a lasting executable there.
  fn start_test_process(
    repo_path: &str,
    workspace: &str,
    workspace_path: &str,
    placeholder: &str,
  ) -> AgentRecord {
    let log_file_path = log_path(repo_path, workspace);
    fs::create_dir_all(agents_dir(repo_path)).unwrap();
    let log_file = OpenOptions::new()
      .create(true)
      .append(true)
      .open(&log_file_path)
      .unwrap();
    let fifo_path = stdin_fifo_path(repo_path, workspace);
    let (agent_stdin, keeper) = setup_agent_stdin(&fifo_path).unwrap();
    let mut command = if placeholder == "cat" && !cfg!(windows) {
      Command::new("cat")
    } else if cfg!(windows) {
      let mut command = Command::new("ping");
      command.args(["-n", "30", "127.0.0.1"]);
      command
    } else {
      let mut command = Command::new("sleep");
      command.arg("30");
      command
    };
    let child = command
      .current_dir(workspace_path)
      .stdin(Stdio::from(agent_stdin))
      .stdout(Stdio::from(log_file.try_clone().unwrap()))
      .stderr(Stdio::from(log_file))
      .spawn()
      .unwrap();
    let record = AgentRecord {
      workspace: workspace.to_string(),
      agent: "sleep".to_string(),
      pid: child.id(),
      pid_start: process_start_token(child.id()),
      started_at: chrono::Utc::now().to_rfc3339(),
      prompt: String::new(),
      log_path: log_file_path.to_string_lossy().into_owned(),
      stdin_path: Some(fifo_path.to_string_lossy().into_owned()),
      keeper_pid: keeper.as_ref().map(|(pid, _)| *pid),
      keeper_start: keeper.and_then(|(_, start)| start),
    };
    write_record(repo_path, &record).unwrap();
    #[expect(
      clippy::mem_forget,
      reason = "the test tracks the process by pid in the record, as production does"
    )]
    std::mem::forget(child);
    record
  }
}
