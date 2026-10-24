//! VM-local persistent PTY supervisor (mobile PRD Phase 7, item 1).
//!
//! Backs the `pty-remote` CLI surface (`start`/`attach`/`resize`/`stop`/
//! `list`), the shared infrastructure both the mobile client (Phase 7) and
//! desktop (Phase 8) consume so a full interactive terminal survives an app
//! suspension, a dropped connection, or an app relaunch without losing the
//! running remote process.
//!
//! ## Implementation decision: tmux-backed, not a custom supervisor
//!
//! The PRD's open question asked whether to build a custom Treq-owned
//! supervisor (fork+setsid+pty+ring buffer) or lean on `tmux`/`screen`. This
//! module picks **tmux as the primary backend**, with `screen` as a fallback
//! when `tmux` is not installed (`detect_backend`), and returns a clear
//! `dependency_error` when neither is present rather than silently
//! degrading to a non-persistent session:
//!
//! - A custom supervisor duplicates a huge amount of battle-tested behavior
//!   (job control, signal forwarding, scrollback, resize propagation) that
//!   tmux/screen already get right; the PRD's own examples of what a
//!   session must survive (detach, reattach, resize) are tmux's core
//!   purpose.
//! - Session persistence with tmux is one detached session per
//!   (repo, workspace, label), keyed by a name derived from all three so `list`
//!   and `attach` are naturally idempotent — reattach-or-create.
//! - Resize does not need a dedicated wire message: when the SSH PTY
//!   channel's `window_change_request` changes the terminal size tmux is
//!   attached through, tmux resizes its virtual screen to match the
//!   attached client automatically (tmux's default `aggressive-resize off`
//!   behavior already tracks the (single) attached client). `resize_session`
//!   below exists for completeness / explicit resize-without-attach (e.g. a
//!   mobile client that wants to pre-size a session before attaching) but is
//!   not on the hot path of normal use.
//! - Trade-off accepted: this depends on `tmux` (or `screen`) being present
//!   on the remote VM. Treq's managed-instance provisioning path can
//!   guarantee this; a user-managed endpoint that lacks both gets a clear
//!   `dependency_error` from `start_session`/`build_attach_command` telling
//!   the user to install one, rather than silently falling back to a
//!   non-reattachable raw PTY. This resolves the PRD's open question in
//!   favor of "depend on it, fail loudly if absent" over "always work but
//!   never survive a reconnect".
//!
//! Mirrors `core::agent_supervisor`'s shape (pidfile-style JSON record
//! under `.treq/`, no public network port, only reachable through the
//! allow-listed CLI surface) but session state itself lives in tmux's own
//! server, not a Treq-owned file — Treq only tracks *naming* and *binding*
//! (repo/workspace/label -> tmux session name) so `list` can report
//! workspace-scoped sessions without shelling out to tmux for repos it
//! does not know about.

use crate::core::remote_pty::{build_launch_program, PtyLaunchSpec};
use serde::{Deserialize, Serialize};
use std::process::Command;

/// Which backend `start_session`/`build_attach_command` will use. Detected
/// once per call rather than cached, since a VM's installed tooling does not
/// change within a single CLI invocation's lifetime and caching would risk
/// staleness across long-lived callers (e.g. a desktop process that stays
/// up across an `apt remove tmux`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PtyBackend {
  Tmux,
  Screen,
}

/// Detects the best available backend. Prefers tmux (richer scripting for
/// list/resize/kill by name); falls back to screen; `None` when neither is
/// on `PATH`.
pub fn detect_backend() -> Option<PtyBackend> {
  // Every remote attach is a new process, and the login shell that
  // `detect_binary` asks for the full PATH takes a shell start. tmux found
  // on the PATH the process already has needs no other answer.
  #[cfg(unix)]
  if std::env::var_os("PATH")
    .and_then(|path| crate::binary_paths::find_executable_in("tmux", path))
    .is_some()
  {
    return Some(PtyBackend::Tmux);
  }
  if crate::binary_paths::detect_binary("tmux").is_some() {
    Some(PtyBackend::Tmux)
  } else if crate::binary_paths::detect_binary("screen").is_some() {
    Some(PtyBackend::Screen)
  } else {
    None
  }
}

/// One VM-local persistent PTY session's identity. The session name is
/// derived deterministically from `repo`, `workspace` and `label`, so
/// repeated `start` calls for the same triple are idempotent: the same
/// underlying tmux/screen session is reused rather than a duplicate created.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PtySessionInfo {
  pub session_name: String,
  pub workspace: String,
  pub label: String,
  /// True when the backend reports the session currently exists (it may
  /// have exited between calls, e.g. the shell/agent inside it quit).
  pub running: bool,
}

/// Prefix shared by every session this module creates. Listing filters the
/// backend's sessions by it, so unrelated tmux/screen sessions on the host
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
/// result is safe to pass to `tmux -t` and `screen -S` without quoting, and
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

/// Builds the tmux/screen session name for a (repo, workspace, label)
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

/// Keeps only the sessions that belong to `scope` (and to `workspace`, when
/// given) out of the backend's raw session names. Split out from
/// [`list_sessions`] so the filtering is testable without a tmux server.
fn filter_sessions(
  names: impl IntoIterator<Item = String>,
  scope: &str,
  workspace: Option<&str>,
) -> Vec<PtySessionInfo> {
  names
    .into_iter()
    .filter_map(|name| {
      let parsed = parse_session_name(&name)?;
      if parsed.scope != scope || workspace.is_some_and(|w| w != parsed.workspace) {
        return None;
      }
      Some(PtySessionInfo {
        session_name: name,
        workspace: parsed.workspace,
        label: parsed.label,
        running: true,
      })
    })
    .collect()
}

fn no_backend_error() -> String {
  "dependency_error: Neither tmux nor screen is installed on this host; pty-remote requires one of them for persistent/reattachable sessions".to_string()
}

/// Starts (or, if one already exists, leaves running) a detached persistent
/// session for `repo`/`workspace`/`label` in `remote_dir`, running `launch`
/// (a login shell or an allow-listed agent binary). `launch` is always a
/// typed [`PtyLaunchSpec`], never a raw frontend-supplied command string, so
/// its program is built by [`build_launch_program`], which quotes every
/// dynamic component exactly as `core::remote_pty`'s direct-launch path
/// does. No code path here lets a caller-supplied string reach the shell
/// unquoted.
pub fn start_session(
  repo: &str,
  remote_dir: &str,
  workspace: &str,
  label: &str,
  launch: &PtyLaunchSpec,
  cols: u16,
  rows: u16,
) -> Result<PtySessionInfo, String> {
  let backend = detect_backend().ok_or_else(no_backend_error)?;
  let name = session_name(repo, workspace, label);

  if session_exists(backend, &name) {
    return Ok(PtySessionInfo {
      session_name: name,
      workspace: workspace.to_string(),
      label: label.to_string(),
      running: true,
    });
  }

  let command = build_launch_program(launch);
  let command = command.as_str();
  let quoted_dir = crate::core::remote::shell_quote(remote_dir);
  match backend {
    PtyBackend::Tmux => {
      let shell_cmd = format!("cd {quoted_dir} && exec {command}");
      let status = Command::new("tmux")
        .args([
          "new-session",
          "-d",
          "-s",
          &name,
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
    }
    PtyBackend::Screen => {
      let shell_cmd = format!("cd {quoted_dir} && exec {command}");
      let status = Command::new("screen")
        .args(["-dmS", &name, "bash", "-lc", &shell_cmd])
        .status()
        .map_err(|e| format!("dependency_error: Failed to spawn screen: {e}"))?;
      if !status.success() {
        return Err(format!(
          "dependency_error: screen -dmS exited with {status}"
        ));
      }
    }
  }

  Ok(PtySessionInfo {
    session_name: name,
    workspace: workspace.to_string(),
    label: label.to_string(),
    running: true,
  })
}

fn session_exists(backend: PtyBackend, name: &str) -> bool {
  match backend {
    PtyBackend::Tmux => Command::new("tmux")
      .args(["has-session", "-t", &format!("={name}")])
      .status()
      .map(|s| s.success())
      .unwrap_or(false),
    PtyBackend::Screen => Command::new("screen")
      .args(["-ls", name])
      .output()
      .map(|out| String::from_utf8_lossy(&out.stdout).contains(name))
      .unwrap_or(false),
  }
}

/// Lists persistent sessions for `repo`, optionally narrowed to
/// `workspace`. Reads backend-reported state directly (no Treq-owned index
/// file to go stale): every session this module creates follows
/// [`session_name`], so listing is a filter over the backend's own list.
pub fn list_sessions(repo: &str, workspace: Option<&str>) -> Result<Vec<PtySessionInfo>, String> {
  let Some(backend) = detect_backend() else {
    return Ok(vec![]);
  };
  let names = match backend {
    PtyBackend::Tmux => {
      let output = Command::new("tmux")
        .args(["list-sessions", "-F", "#{session_name}"])
        .output()
        .map_err(|e| format!("dependency_error: Failed to list tmux sessions: {e}"))?;
      if !output.status.success() {
        // tmux exits non-zero ("no server running") when there are no
        // sessions at all — that is an empty list, not an error.
        return Ok(vec![]);
      }
      String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|s| s.to_string())
        .collect::<Vec<_>>()
    }
    PtyBackend::Screen => {
      let output = Command::new("screen")
        .arg("-ls")
        .output()
        .map_err(|e| format!("dependency_error: Failed to list screen sessions: {e}"))?;
      String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
          let trimmed = line.trim();
          // Lines look like "12345.treq-pty-foo-bar\t(Detached)".
          trimmed.split_whitespace().next().and_then(|token| {
            token
              .split_once('.')
              .map(|(_, name)| name.to_string())
              .filter(|_| token.contains('.'))
          })
        })
        .collect::<Vec<_>>()
    }
  };

  Ok(filter_sessions(names, &repo_scope(repo), workspace))
}

/// Stops (kills) a persistent session. Idempotent: stopping a session that
/// does not exist is not an error.
pub fn stop_session(repo: &str, workspace: &str, label: &str) -> Result<(), String> {
  let Some(backend) = detect_backend() else {
    return Ok(());
  };
  let name = session_name(repo, workspace, label);
  match backend {
    PtyBackend::Tmux => {
      // `=` asks tmux for an exact match. Without it tmux falls back to
      // prefix matching, so stopping `...-shell` could kill `...-shell_2d2`.
      let _ = Command::new("tmux")
        .args(["kill-session", "-t", &format!("={name}")])
        .status();
    }
    PtyBackend::Screen => {
      let _ = Command::new("screen")
        .args(["-S", &name, "-X", "quit"])
        .status();
    }
  }
  Ok(())
}

/// Explicitly resizes a session's virtual terminal, independent of any
/// attached client. Normally unnecessary (see module docs: tmux/screen
/// auto-resize to the attached client's window), but useful to pre-size a
/// session before the first attach.
pub fn resize_session(
  repo: &str,
  workspace: &str,
  label: &str,
  cols: u16,
  rows: u16,
) -> Result<(), String> {
  let backend = detect_backend()
    .ok_or_else(|| "dependency_error: Neither tmux nor screen is installed".to_string())?;
  let name = session_name(repo, workspace, label);
  match backend {
    PtyBackend::Tmux => {
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
    }
    PtyBackend::Screen => {
      // `screen` has no headless resize-by-name equivalent; resize happens
      // implicitly on attach via the terminal's own size. Treated as a no-op
      // rather than an error so callers on a screen-only host are not
      // blocked.
    }
  }
  Ok(())
}

/// Builds the literal command line an SSH PTY channel execs to attach to
/// (creating first if necessary) a persistent session — the `pty-remote`
/// analogue of `core::remote_pty::build_launch_command`. `launch` is a typed
/// [`PtyLaunchSpec`], never a raw frontend-supplied command string: its
/// program is built by [`build_launch_program`], and every other dynamic
/// component (directory, session name) is quoted with `shell_quote` exactly
/// as that function does.
///
/// Uses `tmux new-session -A` ("attach if it exists, else create") so one
/// command line covers both "start a fresh session" and "reattach to a
/// running one" — the mobile/desktop client does not need to know in
/// advance which case applies.
#[allow(
  clippy::too_many_arguments,
  reason = "flat argument list kept to match callers"
)]
pub fn build_attach_command(
  repo: &str,
  remote_dir: &str,
  workspace: &str,
  label: &str,
  launch: &PtyLaunchSpec,
  cols: u16,
  rows: u16,
) -> Result<String, String> {
  let backend = detect_backend().ok_or_else(no_backend_error)?;
  Ok(attach_command_for_backend(
    backend,
    &session_name(repo, workspace, label),
    remote_dir,
    launch,
    cols,
    rows,
  ))
}

fn attach_command_for_backend(
  backend: PtyBackend,
  name: &str,
  remote_dir: &str,
  launch: &PtyLaunchSpec,
  cols: u16,
  rows: u16,
) -> String {
  let command = build_launch_program(launch);
  let quoted_dir = crate::core::remote::shell_quote(remote_dir);
  let quoted_name = crate::core::remote::shell_quote(name);
  match backend {
    // The pane is the whole terminal: no tmux status line under it.
    PtyBackend::Tmux => format!(
      "cd {quoted_dir} && exec tmux new-session -A -s {quoted_name} -x {cols} -y {rows} {command} \\; set-option status off"
    ),
    PtyBackend::Screen => {
      format!("cd {quoted_dir} && exec screen -xRR {quoted_name} {command}")
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::remote_pty::RemoteAgentId;

  fn tmux_available() -> bool {
    detect_backend() == Some(PtyBackend::Tmux)
  }

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

    let all_a = filter_sessions(names.clone(), &repo_scope("/srv/a"), None);
    let labels: Vec<_> = all_a
      .iter()
      .map(|s| (s.workspace.as_str(), s.label.as_str()))
      .collect();
    assert_eq!(labels, vec![("root", "shell"), ("feat", "claude")]);

    let root_a = filter_sessions(names.clone(), &repo_scope("/srv/a"), Some("root"));
    assert_eq!(root_a.len(), 1);
    assert_eq!(
      root_a[0].session_name,
      session_name("/srv/a", "root", "shell")
    );

    let root_b = filter_sessions(names, &repo_scope("/srv/b"), Some("root"));
    assert_eq!(root_b.len(), 1);
    assert_eq!(
      root_b[0].session_name,
      session_name("/srv/b", "root", "shell")
    );
  }

  #[test]
  fn attach_command_quotes_a_malicious_working_directory() {
    let command = attach_command_for_backend(
      PtyBackend::Tmux,
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
    for backend in [PtyBackend::Tmux, PtyBackend::Screen] {
      let command = attach_command_for_backend(
        backend,
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
  }

  #[test]
  fn build_attach_command_errors_clearly_when_no_backend_detected() {
    // Cannot force `detect_binary` to fail where tmux/screen exist, so this
    // only runs on hosts that genuinely lack both.
    if detect_backend().is_some() {
      return;
    }
    let error = build_attach_command(
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
    let _ = stop_session(repo_path, workspace, label);

    let info = start_session(
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

    let sessions = list_sessions(repo_path, Some(workspace)).unwrap();
    let listed = sessions
      .iter()
      .find(|s| s.session_name == info.session_name)
      .expect("session listed for its repo");
    assert_eq!(listed.workspace, workspace);
    assert_eq!(listed.label, label);

    // A different repository on the same host does not see the session.
    let other = tempfile::tempdir().unwrap();
    let other_sessions = list_sessions(other.path().to_str().unwrap(), Some(workspace)).unwrap();
    assert!(other_sessions.is_empty());

    resize_session(repo_path, workspace, label, 100, 40).unwrap();

    stop_session(repo_path, workspace, label).unwrap();
    let sessions_after = list_sessions(repo_path, Some(workspace)).unwrap();
    assert!(!sessions_after
      .iter()
      .any(|s| s.session_name == info.session_name));

    // Stopping again is idempotent.
    stop_session(repo_path, workspace, label).unwrap();
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
    let _ = stop_session(repo_path, workspace, "sh-2");
    start_session(
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
    stop_session(repo_path, workspace, "sh").unwrap();
    let sessions = list_sessions(repo_path, Some(workspace)).unwrap();
    assert!(sessions.iter().any(|s| s.label == "sh-2"));

    stop_session(repo_path, workspace, "sh-2").unwrap();
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
    let _ = stop_session(repo_path, workspace, label);

    let info = start_session(
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

    stop_session(repo_path, workspace, label).unwrap();
  }
}
