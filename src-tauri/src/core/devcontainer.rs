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

/// Repo setting that turns off installing Claude Code in the container.
pub const CLAUDE_CODE_SETTING: &str = "devcontainer_claude_code";

const NODE_FEATURE: &str = "ghcr.io/devcontainers/features/node:1";
const CLAUDE_CODE_FEATURE: &str = "ghcr.io/anthropics/devcontainer-features/claude-code:1";

/// Volume that keeps Claude Code's sign-in and settings across container
/// rebuilds and across every repository's container.
const CLAUDE_CONFIG_VOLUME: &str = "treq-claude-config";
pub const CLAUDE_CONFIG_DIR: &str = "/treq/claude-config";

/// Features Treq adds to the repository's own so agents can run: Claude Code,
/// plus Node (which the Claude Code feature needs) unless the config has it.
/// Features the config already names are left to the config.
pub fn additional_features(config_json: &str) -> Option<String> {
  let mut features = serde_json::Map::new();
  if !config_json.contains("anthropics/devcontainer-features/claude-code") {
    if !config_json.contains("devcontainers/features/node") {
      features.insert(NODE_FEATURE.into(), serde_json::json!({}));
    }
    features.insert(CLAUDE_CODE_FEATURE.into(), serde_json::json!({}));
  }
  (!features.is_empty()).then(|| serde_json::Value::Object(features).to_string())
}

/// Arguments for `devcontainer up` that start (or reuse) Treq's container.
/// `claude_code` carries the config text when Claude Code should be set up.
pub fn up_args(repo_path: &str, temp_dir: &str, claude_code: Option<&str>) -> Vec<String> {
  let mut args: Vec<String> = vec![
    "up".into(),
    "--workspace-folder".into(),
    repo_path.into(),
    "--id-label".into(),
    id_label(repo_path),
    "--mount".into(),
    bind_mount(repo_path),
    "--mount".into(),
    bind_mount(temp_dir),
  ];
  if let Some(config_json) = claude_code {
    args.push("--mount".into());
    args.push(format!(
      "type=volume,source={CLAUDE_CONFIG_VOLUME},target={CLAUDE_CONFIG_DIR}"
    ));
    if let Some(features) = additional_features(config_json) {
      args.push("--additional-features".into());
      args.push(features);
    }
  }
  args
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
      "--remote-env".into(),
      format!("CLAUDE_CONFIG_DIR={CLAUDE_CONFIG_DIR}"),
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
/// With `claude_code`, also installs Claude Code and mounts its config
/// volume. Blocks until `devcontainer up` exits.
pub fn up(repo_path: &str, claude_code: bool) -> Result<DevcontainerStatus, String> {
  let Some(config) = config_path(Path::new(repo_path)) else {
    return Err("This repository has no .devcontainer/devcontainer.json".into());
  };
  if !cli_available() {
    return Err(
      "The Dev Containers CLI is not installed. Install it with `npm install -g @devcontainers/cli`."
        .into(),
    );
  }
  let config_json = std::fs::read_to_string(Path::new(repo_path).join(config)).unwrap_or_default();
  let output = Command::new("devcontainer")
    .args(up_args(
      repo_path,
      &temp_dir_string(),
      claude_code.then_some(config_json.as_str()),
    ))
    .env("PATH", crate::binary_paths::get_extended_path())
    .output()
    .map_err(|e| format!("Failed to run devcontainer up: {e}"))?;
  if !output.status.success() {
    return Err(up_failure_message(&output.stdout, &output.stderr));
  }
  if claude_code {
    give_config_volume_to_remote_user(&output.stdout)?;
  }
  Ok(status(repo_path))
}

/// Docker creates the config volume owned by root; the container's remote
/// user must own it for Claude Code to save its sign-in.
fn give_config_volume_to_remote_user(up_stdout: &[u8]) -> Result<(), String> {
  let Some((container_id, remote_user)) = up_result(up_stdout) else {
    return Ok(());
  };
  let output = Command::new("docker")
    .args(["exec", "-u", "root", &container_id, "chown", "-R"])
    .arg(&remote_user)
    .arg(CLAUDE_CONFIG_DIR)
    .env("PATH", crate::binary_paths::get_extended_path())
    .output()
    .map_err(|e| format!("Failed to prepare the Claude Code config volume: {e}"))?;
  if !output.status.success() {
    return Err(format!(
      "Failed to prepare the Claude Code config volume: {}",
      String::from_utf8_lossy(&output.stderr).trim()
    ));
  }
  Ok(())
}

/// The container id and remote user from `devcontainer up`'s JSON result.
fn up_result(stdout: &[u8]) -> Option<(String, String)> {
  let value = String::from_utf8_lossy(stdout)
    .lines()
    .rev()
    .find_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())?;
  Some((
    value.get("containerId")?.as_str()?.to_string(),
    value.get("remoteUser")?.as_str()?.to_string(),
  ))
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
    let args = up_args("/src/app", "/tmp", None);
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
  fn up_adds_claude_code_and_config_volume() {
    let args = up_args("/src/app", "/tmp", Some(r#"{"image":"debian"}"#));
    assert_eq!(
      &args[9..],
      [
        "--mount",
        "type=volume,source=treq-claude-config,target=/treq/claude-config",
        "--additional-features",
        r#"{"ghcr.io/anthropics/devcontainer-features/claude-code:1":{},"ghcr.io/devcontainers/features/node:1":{}}"#,
      ]
    );
  }

  #[test]
  fn additional_features_keeps_features_the_config_names() {
    assert_eq!(
      additional_features(
        r#"{"features":{"ghcr.io/devcontainers/features/node:1":{"version":"24"}}}"#
      ),
      Some(r#"{"ghcr.io/anthropics/devcontainer-features/claude-code:1":{}}"#.to_string())
    );
    assert_eq!(
      additional_features(
        r#"{"features":{"ghcr.io/anthropics/devcontainer-features/claude-code:1":{}}}"#
      ),
      None
    );
  }

  #[test]
  fn reads_container_and_user_from_up_result() {
    let stdout =
      b"log\n{\"outcome\":\"success\",\"containerId\":\"abc\",\"remoteUser\":\"vscode\"}\n";
    assert_eq!(up_result(stdout), Some(("abc".into(), "vscode".into())));
    assert_eq!(up_result(b"not json"), None);
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
        "--remote-env"
      ]
    );
    assert_eq!(args[6], "CLAUDE_CONFIG_DIR=/treq/claude-config");
    assert_eq!(
      args[9],
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
