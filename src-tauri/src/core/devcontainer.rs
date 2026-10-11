//! Devcontainer environments: run a repository's terminals and agent sessions
//! inside the container its `.devcontainer` config describes.
//!
//! Treq treats the container like a remote host that shares the local
//! filesystem. It starts the container with the Dev Containers CLI and
//! bind-mounts the repository (which holds `.treq/workspaces`) and the system
//! temp directory (which holds agent prompt files) at their host paths, so
//! every path Treq computes on the host is valid inside the container. A PTY
//! then runs `devcontainer exec` instead of a local shell.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

/// Repo setting that selects where new terminals and agent sessions run.
pub const ENVIRONMENT_SETTING: &str = "environment";
pub const ENVIRONMENT_LOCAL: &str = "local";
pub const ENVIRONMENT_DEVCONTAINER: &str = "devcontainer";

/// Label that marks a container as the one Treq started for a repository.
/// It keeps Treq's container (with its extra mounts) apart from one an editor
/// started for the same folder.
const ID_LABEL_KEY: &str = "treq.repo";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DevcontainerStatus {
  /// The repository has a devcontainer config.
  pub configured: bool,
  /// Path of the config, relative to the repository.
  pub config_path: Option<String>,
  /// The Dev Containers CLI (`devcontainer`) is on PATH.
  pub cli_available: bool,
  /// Treq's container for this repository is running.
  pub running: bool,
}

/// The devcontainer config of `repo_path`, relative to it, if there is one.
pub fn config_path(repo_path: &Path) -> Option<&'static str> {
  [".devcontainer/devcontainer.json", ".devcontainer.json"]
    .into_iter()
    .find(|relative| repo_path.join(relative).is_file())
}

/// The repository a working directory belongs to: the parent of
/// `.treq/workspaces/<name>` for a workspace, else the directory itself.
pub fn repo_root_for(working_dir: &Path) -> PathBuf {
  for ancestor in working_dir.ancestors() {
    if ancestor
      .file_name()
      .is_some_and(|name| name == "workspaces")
      && ancestor
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|name| name == ".treq")
    {
      if let Some(repo) = ancestor.parent().and_then(Path::parent) {
        return repo.to_path_buf();
      }
    }
  }
  working_dir.to_path_buf()
}

fn id_label(repo_path: &str) -> String {
  format!("{ID_LABEL_KEY}={repo_path}")
}

fn bind_mount(path: &str) -> String {
  format!("type=bind,source={path},target={path}")
}

fn temp_dir_string() -> String {
  let temp = std::env::temp_dir();
  let temp = std::fs::canonicalize(&temp).unwrap_or(temp);
  temp.to_string_lossy().trim_end_matches('/').to_string()
}

/// Arguments for `devcontainer up` that start (or reuse) Treq's container.
pub fn up_args(repo_path: &str, temp_dir: &str) -> Vec<String> {
  vec![
    "up".into(),
    "--workspace-folder".into(),
    repo_path.into(),
    "--id-label".into(),
    id_label(repo_path),
    "--mount".into(),
    bind_mount(repo_path),
    "--mount".into(),
    bind_mount(temp_dir),
  ]
}

/// Program and arguments for a PTY that opens a login shell inside Treq's
/// container, in `working_dir` (a host path, mounted at the same path).
pub fn exec_shell_command(
  repo_path: &str,
  working_dir: &str,
  pty_session_id: &str,
) -> (String, Vec<String>) {
  let script = format!(
    "export TERM=xterm-256color TREQ_PTY_SESSION_ID={}; cd {} && exec \"${{SHELL:-/bin/sh}}\" -l",
    crate::core::remote::shell_quote(pty_session_id),
    crate::core::remote::shell_quote(working_dir),
  );
  (
    "devcontainer".into(),
    vec![
      "exec".into(),
      "--workspace-folder".into(),
      repo_path.into(),
      "--id-label".into(),
      id_label(repo_path),
      "sh".into(),
      "-c".into(),
      script,
    ],
  )
}

fn cli_available() -> bool {
  crate::binary_paths::detect_binary("devcontainer").is_some()
}

fn is_running(repo_path: &str) -> bool {
  Command::new("docker")
    .args(["ps", "-q", "--filter"])
    .arg(format!("label={}", id_label(repo_path)))
    .env("PATH", crate::binary_paths::get_extended_path())
    .output()
    .ok()
    .filter(|output| output.status.success())
    .is_some_and(|output| !output.stdout.trim_ascii().is_empty())
}

pub fn status(repo_path: &str) -> DevcontainerStatus {
  let config = config_path(Path::new(repo_path));
  let cli = cli_available();
  DevcontainerStatus {
    configured: config.is_some(),
    config_path: config.map(str::to_string),
    cli_available: cli,
    running: config.is_some() && is_running(repo_path),
  }
}

/// Starts Treq's container for `repo_path`, building it first if needed.
/// Blocks until `devcontainer up` exits.
pub fn up(repo_path: &str) -> Result<DevcontainerStatus, String> {
  if config_path(Path::new(repo_path)).is_none() {
    return Err("This repository has no .devcontainer/devcontainer.json".into());
  }
  if !cli_available() {
    return Err(
      "The Dev Containers CLI is not installed. Install it with `npm install -g @devcontainers/cli`."
        .into(),
    );
  }
  let output = Command::new("devcontainer")
    .args(up_args(repo_path, &temp_dir_string()))
    .env("PATH", crate::binary_paths::get_extended_path())
    .output()
    .map_err(|e| format!("Failed to run devcontainer up: {e}"))?;
  if !output.status.success() {
    return Err(up_failure_message(&output.stdout, &output.stderr));
  }
  Ok(status(repo_path))
}

/// `devcontainer up` prints a JSON result line on stdout whose `message`
/// names the failure; its stderr holds the build log. Prefer the message and
/// fall back to the log's last lines.
fn up_failure_message(stdout: &[u8], stderr: &[u8]) -> String {
  let message = String::from_utf8_lossy(stdout)
    .lines()
    .rev()
    .find_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
    .and_then(|value| value.get("message")?.as_str().map(str::to_string));
  if let Some(message) = message {
    return format!("Devcontainer failed to start: {message}");
  }
  let stderr = String::from_utf8_lossy(stderr);
  let tail: Vec<&str> = stderr.lines().rev().take(5).collect();
  let tail: Vec<&str> = tail.into_iter().rev().collect();
  format!("Devcontainer failed to start: {}", tail.join("\n"))
}

#[cfg(test)]
mod tests {
  use super::*;
  use tempfile::TempDir;

  #[test]
  fn finds_devcontainer_config() {
    let temp = TempDir::new().unwrap();
    assert_eq!(config_path(temp.path()), None);
    std::fs::write(temp.path().join(".devcontainer.json"), "{}").unwrap();
    assert_eq!(config_path(temp.path()), Some(".devcontainer.json"));
    std::fs::create_dir(temp.path().join(".devcontainer")).unwrap();
    std::fs::write(temp.path().join(".devcontainer/devcontainer.json"), "{}").unwrap();
    assert_eq!(
      config_path(temp.path()),
      Some(".devcontainer/devcontainer.json")
    );
  }

  #[test]
  fn resolves_repo_root_from_workspace_path() {
    assert_eq!(
      repo_root_for(Path::new("/src/app/.treq/workspaces/feature/sub")),
      PathBuf::from("/src/app")
    );
    assert_eq!(
      repo_root_for(Path::new("/src/app")),
      PathBuf::from("/src/app")
    );
  }

  #[test]
  fn up_mounts_repo_and_temp_dir_at_host_paths() {
    let args = up_args("/src/app", "/tmp");
    assert_eq!(
      args,
      vec![
        "up",
        "--workspace-folder",
        "/src/app",
        "--id-label",
        "treq.repo=/src/app",
        "--mount",
        "type=bind,source=/src/app,target=/src/app",
        "--mount",
        "type=bind,source=/tmp,target=/tmp",
      ]
    );
  }

  #[test]
  fn exec_opens_login_shell_in_working_dir() {
    let (program, args) = exec_shell_command("/src/app", "/src/app/.treq/workspaces/a b", "s-1");
    assert_eq!(program, "devcontainer");
    assert_eq!(
      &args[..6],
      [
        "exec",
        "--workspace-folder",
        "/src/app",
        "--id-label",
        "treq.repo=/src/app",
        "sh"
      ]
    );
    assert_eq!(
      args[7],
      "export TERM=xterm-256color TREQ_PTY_SESSION_ID='s-1'; cd '/src/app/.treq/workspaces/a b' && exec \"${SHELL:-/bin/sh}\" -l"
    );
  }

  #[test]
  fn up_failure_prefers_cli_message() {
    let stdout = br#"{"outcome":"error","message":"Command failed: docker build"}"#;
    assert_eq!(
      up_failure_message(stdout, b"log line"),
      "Devcontainer failed to start: Command failed: docker build"
    );
    assert_eq!(
      up_failure_message(b"", b"a\nb"),
      "Devcontainer failed to start: a\nb"
    );
  }
}
