//! Reports which command-line tools Treq relies on are installed, for the
//! first-run checklist.

use serde::Serialize;

/// Executables the checklist reports on, in display order.
pub const PREREQUISITE_BINARIES: [&str; 6] =
  ["git", "claude", "codex", "cursor-agent", "copilot", "gh"];

/// What a machine Treq works on over SSH (or in a container) needs: the
/// local list plus jj, which the host runs itself, and tmux, which a host
/// set to tmux terminals needs.
pub const HOST_BINARIES: [&str; 8] = [
  "git",
  "jj",
  "tmux",
  "claude",
  "codex",
  "cursor-agent",
  "copilot",
  "gh",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct PrerequisiteStatus {
  pub binary: String,
  pub installed: bool,
}

/// Looks up every prerequisite on the user's PATH and the usual install
/// directories, the same way Treq finds `gh` and the agent CLIs.
pub fn check_prerequisites() -> Vec<PrerequisiteStatus> {
  check_prerequisites_with(crate::binary_paths::detect_binary)
}

/// Looks up every prerequisite with `find`, which returns a tool's path.
pub fn check_prerequisites_with(find: impl Fn(&str) -> Option<String>) -> Vec<PrerequisiteStatus> {
  check_with(&PREREQUISITE_BINARIES, find)
}

/// [`HOST_BINARIES`] on this machine: what `treq repo tools` reports to a
/// desktop app connected over SSH.
pub fn check_host_tools() -> Vec<PrerequisiteStatus> {
  check_with(&HOST_BINARIES, crate::binary_paths::detect_binary)
}

fn check_with(binaries: &[&str], find: impl Fn(&str) -> Option<String>) -> Vec<PrerequisiteStatus> {
  binaries
    .iter()
    .map(|binary| PrerequisiteStatus {
      binary: binary.to_string(),
      installed: find(binary).is_some(),
    })
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::binary_paths::detect_binary_in;
  use tempfile::TempDir;

  fn installed(statuses: &[PrerequisiteStatus], binary: &str) -> bool {
    statuses
      .iter()
      .find(|status| status.binary == binary)
      .unwrap_or_else(|| panic!("no status reported for {binary}"))
      .installed
  }

  /// A whole search PATH, holding a link to the `which` the lookup runs.
  #[cfg(unix)]
  fn search_dir() -> TempDir {
    let dir = TempDir::new().expect("temp dir");
    let which = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
      .map(|entry| entry.join("which"))
      .find(|candidate| candidate.is_file())
      .expect("`which` must be on PATH");
    std::os::unix::fs::symlink(which, dir.path().join("which")).expect("link which");
    dir
  }

  #[cfg(unix)]
  fn add_tool(dir: &TempDir, name: &str, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.path().join(name);
    std::fs::write(&path, "#!/bin/sh\n").expect("write fake tool");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).expect("chmod");
  }

  #[cfg(unix)]
  #[test]
  fn reports_tools_on_the_search_path_as_installed() {
    let dir = search_dir();
    add_tool(&dir, "claude", 0o755);
    add_tool(&dir, "gh", 0o755);

    let statuses = check_prerequisites_with(|name| detect_binary_in(name, dir.path()));

    assert!(installed(&statuses, "claude"));
    assert!(installed(&statuses, "gh"));
    assert!(!installed(&statuses, "git"));
    assert!(!installed(&statuses, "codex"));
    assert!(!installed(&statuses, "cursor-agent"));
    assert!(!installed(&statuses, "copilot"));
  }

  #[cfg(unix)]
  #[test]
  fn reports_non_executable_files_as_missing() {
    let dir = search_dir();
    add_tool(&dir, "git", 0o644);

    let statuses = check_prerequisites_with(|name| detect_binary_in(name, dir.path()));

    assert!(!installed(&statuses, "git"));
  }

  #[cfg(windows)]
  #[test]
  fn reports_exe_files_on_the_search_path_as_installed() {
    let dir = TempDir::new().expect("temp dir");
    std::fs::write(dir.path().join("codex.exe"), "").expect("write fake tool");

    let statuses = check_prerequisites_with(|name| detect_binary_in(name, dir.path()));

    assert!(installed(&statuses, "codex"));
    assert!(!installed(&statuses, "git"));
  }

  #[test]
  fn reports_every_tool_in_checklist_order() {
    let statuses = check_prerequisites_with(|_| None);

    let binaries: Vec<&str> = statuses.iter().map(|s| s.binary.as_str()).collect();
    assert_eq!(binaries, PREREQUISITE_BINARIES);
  }
}
