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
  // Sorted, so the argument is the same whether or not another crate turns
  // on serde_json's `preserve_order` (GPUI does).
  let mut features = std::collections::BTreeMap::<String, serde_json::Value>::new();
  if !config_json.contains("anthropics/devcontainer-features/claude-code") {
    if !config_json.contains("devcontainers/features/node") {
      features.insert(NODE_FEATURE.into(), serde_json::json!({}));
    }
    features.insert(CLAUDE_CODE_FEATURE.into(), serde_json::json!({}));
  }
  (!features.is_empty()).then(|| serde_json::json!(features).to_string())
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
  // The container still works without the CLI, so a failed install only
  // logs: agents lose `treq` commands, not their terminal.
  if let Some((container_id, _)) = up_result(&output.stdout) {
    if let Err(e) = install_treq_cli(&container_id) {
      log::warn!("treq CLI not installed in devcontainer: {e}");
    }
  }
  Ok(status(repo_path))
}

/// Overrides the downloaded CLI with a local Linux build of `treq`, for
/// development builds whose version has no release yet.
const CLI_OVERRIDE_ENV: &str = "TREQ_DEVCONTAINER_CLI";
const CLI_INSTALL_PATH: &str = "/usr/local/bin/treq";

/// The release asset architecture for a container's `uname -m`.
fn cli_arch(uname_m: &str) -> Option<&'static str> {
  match uname_m.trim() {
    "x86_64" | "amd64" => Some("x86_64"),
    "aarch64" | "arm64" => Some("aarch64"),
    _ => None,
  }
}

fn cli_asset_url(version: &str, arch: &str) -> String {
  format!(
    "https://github.com/tznc/treq/releases/download/v{version}/treq-cli-{version}-linux-{arch}.tar.gz"
  )
}

fn docker(args: &[&str]) -> Result<String, String> {
  let output = Command::new("docker")
    .args(args)
    .env("PATH", crate::binary_paths::get_extended_path())
    .output()
    .map_err(|e| format!("docker {}: {e}", args[0]))?;
  if !output.status.success() {
    return Err(format!(
      "docker {}: {}",
      args[0],
      String::from_utf8_lossy(&output.stderr).trim()
    ));
  }
  Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// A container Treq started for a repository, as `docker ps -a` lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreqContainer {
  pub id: String,
  /// The repository it was started for (its `treq.repo` label).
  pub repo_path: String,
  pub running: bool,
}

/// Every container Treq started, running or not, newest first. Empty when
/// Docker is missing or not running.
pub fn containers() -> Vec<TreqContainer> {
  let format = format!("{{{{.ID}}}}\t{{{{.Label \"{ID_LABEL_KEY}\"}}}}\t{{{{.State}}}}");
  let filter = format!("label={ID_LABEL_KEY}");
  docker(&["ps", "-a", "--filter", &filter, "--format", &format])
    .map(|out| parse_containers(&out))
    .unwrap_or_default()
}

fn parse_containers(out: &str) -> Vec<TreqContainer> {
  out
    .lines()
    .filter_map(|line| {
      let mut parts = line.split('\t');
      let id = parts.next()?.trim();
      let repo_path = parts.next()?.trim();
      let state = parts.next().unwrap_or_default().trim();
      (!id.is_empty() && !repo_path.is_empty()).then(|| TreqContainer {
        id: id.to_string(),
        repo_path: repo_path.to_string(),
        running: state == "running",
      })
    })
    .collect()
}

/// [`crate::core::prerequisites::HOST_BINARIES`] inside a running
/// container, looked up by its login shell.
pub fn container_tools(container_id: &str) -> Result<Vec<crate::core::PrerequisiteStatus>, String> {
  let binaries = crate::core::prerequisites::HOST_BINARIES;
  let script = binaries
    .iter()
    .map(|b| format!("command -v {b} >/dev/null 2>&1 && echo {b}"))
    .collect::<Vec<_>>()
    .join("; ");
  let found = docker(&[
    "exec",
    container_id,
    "sh",
    "-lc",
    &format!("{script}; true"),
  ])?;
  let found: Vec<&str> = found.lines().map(str::trim).collect();
  Ok(
    binaries
      .iter()
      .map(|binary| crate::core::PrerequisiteStatus {
        binary: binary.to_string(),
        installed: found.contains(binary),
      })
      .collect(),
  )
}

/// Puts the `treq` CLI matching this app's version on the container's PATH,
/// so agents in it can call `treq` like they do on the host. Skips the
/// download when the container already has that version.
fn install_treq_cli(container_id: &str) -> Result<(), String> {
  let version = env!("CARGO_PKG_VERSION");
  let installed =
    docker(&["exec", container_id, CLI_INSTALL_PATH, "--version"]).unwrap_or_default();
  if installed.trim() == format!("treq {version}") {
    return Ok(());
  }
  if let Some(binary) = std::env::var_os(CLI_OVERRIDE_ENV) {
    let binary = binary.to_string_lossy();
    docker(&["cp", &binary, &format!("{container_id}:{CLI_INSTALL_PATH}")])?;
    return Ok(());
  }
  let uname = docker(&["exec", container_id, "uname", "-m"])?;
  let arch = cli_arch(&uname).ok_or_else(|| format!("no treq CLI build for {}", uname.trim()))?;
  let archive = download_cli(version, arch)?;
  let staged = "/tmp/treq-cli.tar.gz";
  docker(&[
    "cp",
    &archive.to_string_lossy(),
    &format!("{container_id}:{staged}"),
  ])?;
  docker(&[
    "exec",
    "-u",
    "root",
    container_id,
    "sh",
    "-c",
    &format!("tar -xzf {staged} -C /usr/local/bin treq && rm -f {staged}"),
  ])?;
  Ok(())
}

/// Downloads the release archive once per version and architecture into a
/// per-user cache, checks it against the release's `.sha256`, and returns its
/// path. A cached archive is used again only while it still matches.
fn download_cli(version: &str, arch: &str) -> Result<PathBuf, String> {
  let dir = cli_cache_dir()?;
  let archive = dir.join(format!("treq-cli-{version}-linux-{arch}.tar.gz"));
  let checksum = archive.with_extension("gz.sha256");
  if let (Ok(bytes), Ok(expected)) = (std::fs::read(&archive), std::fs::read_to_string(&checksum)) {
    if verify_sha256(&bytes, &expected).is_ok() {
      return Ok(archive);
    }
  }
  let url = cli_asset_url(version, arch);
  let checksum_url = format!("{url}.sha256");
  let (bytes, expected) = fetch_all(&[&url, &checksum_url]).and_then(|mut bodies| {
    let expected = String::from_utf8(bodies.pop().unwrap_or_default())
      .map_err(|_| format!("{checksum_url} is not text"))?;
    Ok((bodies.pop().unwrap_or_default(), expected))
  })?;
  verify_sha256(&bytes, &expected).map_err(|e| format!("download {url}: {e}"))?;
  // Write then rename so an interrupted download never looks complete.
  let partial = archive.with_extension("partial");
  std::fs::write(&partial, &bytes).map_err(|e| e.to_string())?;
  std::fs::rename(&partial, &archive).map_err(|e| e.to_string())?;
  std::fs::write(&checksum, expected).map_err(|e| e.to_string())?;
  Ok(archive)
}

/// `$XDG_CACHE_HOME/treq/cli` (else `~/.cache/treq/cli`), readable only by
/// the user: a shared temp dir would let another account plant the archive
/// that gets copied into the container.
fn cli_cache_dir() -> Result<PathBuf, String> {
  let base = std::env::var_os("XDG_CACHE_HOME")
    .filter(|v| !v.is_empty())
    .map(PathBuf::from)
    .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
    .ok_or("no cache directory: HOME is not set")?;
  let dir = base.join("treq").join("cli");
  std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
      .map_err(|e| format!("restrict {}: {e}", dir.display()))?;
  }
  Ok(dir)
}

/// Checks `bytes` against a `sha256sum` line (`<hex>  <file>`) or a bare hex digest.
fn verify_sha256(bytes: &[u8], expected: &str) -> Result<(), String> {
  use sha2::{Digest, Sha256};
  let expected = expected
    .split_whitespace()
    .next()
    .ok_or("empty checksum")?
    .to_ascii_lowercase();
  let actual: String = Sha256::digest(bytes)
    .iter()
    .map(|b| format!("{b:02x}"))
    .collect();
  if actual == expected {
    Ok(())
  } else {
    Err(format!(
      "checksum mismatch: expected {expected}, got {actual}"
    ))
  }
}

/// GETs each URL in order. Runs its own runtime on a fresh thread, so it
/// works whether or not the caller is already inside a Tokio runtime.
fn fetch_all(urls: &[&str]) -> Result<Vec<Vec<u8>>, String> {
  std::thread::scope(|scope| {
    scope
      .spawn(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
          .enable_all()
          .build()
          .map_err(|e| e.to_string())?;
        let client = reqwest::Client::builder()
          .connect_timeout(std::time::Duration::from_secs(10))
          .timeout(std::time::Duration::from_secs(300))
          .build()
          .map_err(|e| e.to_string())?;
        runtime.block_on(async {
          let mut bodies = Vec::with_capacity(urls.len());
          for url in urls {
            let body = async {
              let response = client.get(*url).send().await?.error_for_status()?;
              response.bytes().await
            }
            .await
            .map_err(|e| format!("download {url}: {e}"))?;
            bodies.push(body.to_vec());
          }
          Ok(bodies)
        })
      })
      .join()
      .map_err(|_| "download thread panicked".to_string())?
  })
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
  #[test]
  fn lists_treq_containers_from_docker_ps() {
    let out = "abc123\t/home/me/app\trunning\ndef456\t/home/me/lib\texited\n\tbroken\t\n";
    assert_eq!(
      parse_containers(out),
      vec![
        TreqContainer {
          id: "abc123".into(),
          repo_path: "/home/me/app".into(),
          running: true,
        },
        TreqContainer {
          id: "def456".into(),
          repo_path: "/home/me/lib".into(),
          running: false,
        },
      ]
    );
  }

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
  fn status_reports_a_repo_without_config_as_unconfigured() {
    let temp = TempDir::new().unwrap();
    let status = status(temp.path().to_str().unwrap());
    assert!(!status.configured);
    assert_eq!(status.config_path, None);
    assert!(!status.running);
  }

  #[test]
  fn status_finds_the_config_and_reports_the_container_stopped() {
    let temp = TempDir::new().unwrap();
    std::fs::create_dir(temp.path().join(".devcontainer")).unwrap();
    std::fs::write(
      temp.path().join(".devcontainer/devcontainer.json"),
      r#"{"image":"alpine:3.20"}"#,
    )
    .unwrap();
    let status = status(temp.path().to_str().unwrap());
    assert!(status.configured);
    assert_eq!(
      status.config_path.as_deref(),
      Some(".devcontainer/devcontainer.json")
    );
    assert!(!status.running);
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

  #[test]
  fn maps_container_arch_to_cli_asset_arch() {
    assert_eq!(cli_arch("x86_64\n"), Some("x86_64"));
    assert_eq!(cli_arch("aarch64"), Some("aarch64"));
    assert_eq!(cli_arch("arm64"), Some("aarch64"));
    assert_eq!(cli_arch("riscv64"), None);
  }

  #[test]
  fn verifies_sha256sum_lines() {
    // sha256("abc")
    let digest = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    assert!(verify_sha256(b"abc", &format!("{digest}  treq-cli.tar.gz\n")).is_ok());
    assert!(verify_sha256(b"abc", &digest.to_uppercase()).is_ok());
    assert!(verify_sha256(b"abd", digest).is_err());
    assert!(verify_sha256(b"abc", "").is_err());
  }

  #[test]
  fn cli_asset_url_points_at_the_matching_release() {
    assert_eq!(
      cli_asset_url("0.4.0", "aarch64"),
      "https://github.com/tznc/treq/releases/download/v0.4.0/treq-cli-0.4.0-linux-aarch64.tar.gz"
    );
  }
}
