use super::{
  detect_repository_type, fs, inspect_repository_path, jj, resolve_workspace_path,
  validate_remote_path, validate_repository_path, Deserialize, DirectoryEntry, DirectoryListing,
  Path, PathBuf, RemoteRepoProbe, RepositoryInspection, Serialize,
};

/// An entry of [`TreqCommandRequest::ListFiles`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteFileEntry {
  pub name: String,
  /// Relative to the workspace, with `/` separators.
  pub path: String,
  pub is_directory: bool,
  /// "conflict", "workingCopy" or "committed" (for a directory, the most
  /// severe among its files); absent when untouched.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub status: Option<String>,
}

/// [`TreqCommandRequest::ListFiles`]: one directory of the workspace, with
/// hidden directories (`.git`, `.jj`) left out as the desktop tree does.
pub(super) fn list_workspace_files(
  repo: &str,
  workspace: Option<i64>,
  path: &str,
) -> Result<Vec<RemoteFileEntry>, String> {
  let path = path.trim().trim_matches('/');
  if Path::new(path)
    .components()
    .any(|c| !matches!(c, std::path::Component::Normal(_)))
  {
    return Err(format!(
      "invalid_arguments: {path} is not a path inside the workspace"
    ));
  }
  let root = resolve_workspace_path(repo, workspace)?;
  let dir = Path::new(&root).join(path);
  let mut entries: Vec<_> = crate::core::list_directory(&dir.to_string_lossy())?
    .into_iter()
    .filter(|e| !(e.is_directory && e.name.starts_with('.')))
    .collect();
  let target = crate::local_db::get_workspace_by_id(repo, workspace.unwrap_or(0))
    .ok()
    .flatten()
    .and_then(|w| w.target_branch);
  crate::core::files::annotate_entry_statuses(&root, &mut entries, target.as_deref());
  Ok(
    entries
      .into_iter()
      .map(|e| RemoteFileEntry {
        path: if path.is_empty() {
          e.name.clone()
        } else {
          format!("{path}/{}", e.name)
        },
        name: e.name,
        is_directory: e.is_directory,
        status: e.status,
      })
      .collect(),
  )
}

pub(super) fn is_repo_dir(path: &Path) -> bool {
  path.join(".jj").is_dir() || path.join(".git").exists()
}

/// [`TreqCommandRequest::ListDirectories`]: `path`'s subdirectories, with
/// `~` (or an empty path) as the user's home.
pub(super) fn list_directories(path: &str) -> Result<DirectoryListing, String> {
  let trimmed = path.trim();
  let home = || std::env::var_os("HOME").map(std::path::PathBuf::from);
  let dir = match trimmed {
    "" | "~" => home().ok_or("HOME is not set")?,
    _ => match trimmed.strip_prefix("~/") {
      Some(rest) => home().ok_or("HOME is not set")?.join(rest),
      None => std::path::PathBuf::from(trimmed),
    },
  };
  let dir = dir
    .canonicalize()
    .map_err(|e| format!("Cannot open {}: {e}", dir.display()))?;
  let read = fs::read_dir(&dir).map_err(|e| format!("Cannot open {}: {e}", dir.display()))?;
  let mut entries: Vec<DirectoryEntry> = read
    .flatten()
    .filter(|entry| entry.path().is_dir())
    .map(|entry| {
      let name = entry.file_name().to_string_lossy().to_string();
      let path = entry.path();
      DirectoryEntry {
        hidden: name.starts_with('.'),
        is_repo: is_repo_dir(&path),
        path: path.to_string_lossy().to_string(),
        name,
      }
    })
    .collect();
  entries.sort_by_key(|entry| entry.name.to_lowercase());
  Ok(DirectoryListing {
    path: dir.to_string_lossy().to_string(),
    parent: dir.parent().map(|p| p.to_string_lossy().to_string()),
    is_repo: is_repo_dir(&dir),
    entries,
  })
}

pub(super) fn probe_repo_path(repo_path: &str) -> Result<RemoteRepoProbe, String> {
  let trimmed = repo_path.trim();
  if trimmed.is_empty() {
    return Err("Remote path is required".to_string());
  }
  // The returned `path` is the expanded one, so callers open the repository
  // by a path every later command reads the same way.
  let expanded = expand_home(trimmed, std::env::var("HOME").ok().as_deref());
  let path = Path::new(&expanded);
  let exists = path.exists();
  let is_repo = exists && is_repo_dir(path);
  Ok(RemoteRepoProbe {
    host: String::new(),
    path: expanded.clone(),
    exists,
    is_repo,
    needs_clone: !is_repo,
  })
}

/// Recursively sums file sizes under `root` (best effort: unreadable entries
/// are skipped rather than failing the whole walk) as the "disk used"
/// figure for base-disk-quota enforcement. This walks the repository root
/// rather than statvfs-ing the whole volume, since the quota is scoped to
/// what the user's repositories occupy, not the base image itself.
pub(super) fn directory_usage_bytes(root: &Path) -> u64 {
  let mut total = 0u64;
  let mut stack = vec![root.to_path_buf()];
  while let Some(dir) = stack.pop() {
    let Ok(entries) = fs::read_dir(&dir) else {
      continue;
    };
    for entry in entries.flatten() {
      let Ok(metadata) = entry.metadata() else {
        continue;
      };
      if metadata.is_dir() {
        stack.push(entry.path());
      } else {
        total += metadata.len();
      }
    }
  }
  total
}

/// Expands a leading `~` or `~/` to `home`. Commands run without a shell, so
/// nothing else would; `~user` and paths without a home are left as typed.
pub(crate) fn expand_home(path: &str, home: Option<&str>) -> String {
  let Some(home) = home.filter(|h| !h.is_empty()) else {
    return path.to_string();
  };
  let home = home.trim_end_matches('/');
  match path.strip_prefix('~') {
    Some("") => home.to_string(),
    Some(rest) if rest.starts_with('/') => format!("{home}{rest}"),
    _ => path.to_string(),
  }
}

/// What a host can run, returned by [`TreqCommandRequest::HostCheck`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteHostCheck {
  /// This treq CLI's version.
  pub treq_version: String,
  /// `root` with `~` expanded, and whether it exists.
  pub root: String,
  pub root_exists: bool,
  /// What holds persistent terminals and agents by default: `daemon`, the
  /// Treq terminal daemon, on Linux and macOS hosts. Older hosts reported
  /// `tmux` or `screen` here.
  pub terminal: Option<String>,
  /// Whether tmux is installed, for machines set to tmux terminals.
  #[serde(default)]
  pub tmux: bool,
  /// The allow-listed agent CLIs installed on the host.
  pub agents: Vec<crate::core::remote_pty::RemoteAgentId>,
}

impl RemoteHostCheck {
  /// Whether the host can keep persistent terminals under `backend`. A host
  /// whose treq predates the daemon reports the multiplexer it found in
  /// `terminal`, and keeps using it.
  pub fn holds_terminals(&self, backend: crate::core::pty_remote_supervisor::PtyBackend) -> bool {
    use crate::core::pty_remote_supervisor::PtyBackend;
    match backend {
      PtyBackend::Daemon => self.terminal.is_some(),
      PtyBackend::Tmux => self.tmux || self.terminal.as_deref() == Some("tmux"),
    }
  }
}

pub(super) fn host_check(root: &str) -> RemoteHostCheck {
  use crate::core::{pty_remote_supervisor::PtyBackend, remote_pty::RemoteAgentId};
  let root = expand_home(root.trim(), std::env::var("HOME").ok().as_deref());
  RemoteHostCheck {
    treq_version: env!("CARGO_PKG_VERSION").to_string(),
    root_exists: Path::new(&root).is_dir(),
    root,
    terminal: cfg!(unix).then(|| PtyBackend::Daemon.as_str().to_string()),
    tmux: crate::core::pty_remote_supervisor::tmux_available(),
    agents: RemoteAgentId::ALL
      .into_iter()
      .filter(|agent| crate::binary_paths::detect_binary(agent.binary_name()).is_some())
      .collect(),
  }
}

/// Machine-wide usage returned by [`TreqCommandRequest::MachineUsage`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineUsageReport {
  pub repository_count: usize,
  pub workspace_count: usize,
  pub disk_used_bytes: u64,
}

/// Repositories sit at most two levels below the root
/// (`<root>/<owner>/<name>` for managed clones, `<root>/<name>` otherwise).
pub(super) const MACHINE_USAGE_REPO_DEPTH: usize = 2;

pub(super) fn is_repository_dir(path: &Path) -> bool {
  path.join(".jj").is_dir() || path.join(".git").exists()
}

pub(super) fn find_repositories(root: &Path) -> Vec<PathBuf> {
  let mut repositories = Vec::new();
  let mut stack = vec![(root.to_path_buf(), 0usize)];
  while let Some((dir, depth)) = stack.pop() {
    if depth > 0 && is_repository_dir(&dir) {
      repositories.push(dir);
      continue;
    }
    if depth == MACHINE_USAGE_REPO_DEPTH {
      continue;
    }
    let Ok(entries) = fs::read_dir(&dir) else {
      continue;
    };
    for entry in entries.flatten() {
      if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
        stack.push((entry.path(), depth + 1));
      }
    }
  }
  repositories
}

/// Counts repositories and their Treq workspaces under `root` and measures
/// the bytes they use. A missing root reports zero usage rather than an
/// error, since a fresh machine has no repositories directory yet.
pub(super) fn machine_usage(root: &str) -> Result<MachineUsageReport, String> {
  validate_remote_path(root)?;
  let root = Path::new(root.trim());
  if !root.is_dir() {
    return Ok(MachineUsageReport::default());
  }
  let repositories = find_repositories(root);
  let workspace_count = repositories
    .iter()
    .filter_map(|repo| repo.to_str())
    .map(|repo| {
      crate::core::workspaces::list_workspaces(repo)
        .map(|workspaces| workspaces.len())
        .unwrap_or(0)
    })
    .sum();
  Ok(MachineUsageReport {
    repository_count: repositories.len(),
    workspace_count,
    disk_used_bytes: directory_usage_bytes(root),
  })
}

/// Enforces the base disk quota (PRD "Resource quotas") before a
/// write-triggering mutation (repo init, clone, patch apply) proceeds.
/// Returns the distinct `disk_quota_exceeded: ...` structured error the PRD
/// requires instead of letting the write fail as a generic filesystem
/// error. `root` is the directory whose usage counts against the quota —
/// callers pass the repository root (or its parent, before it exists) so
/// the check reflects what the write is about to add to.
pub(super) fn enforce_disk_quota(root: &Path) -> Result<(), String> {
  enforce_disk_quota_against(root, crate::core::remote_provider::BASE_DISK_QUOTA_BYTES)
}

/// Same as [`enforce_disk_quota`] but against an arbitrary `quota_bytes`
/// ceiling, so tests can exercise the failure path without writing 5 GB of
/// fixture data to disk.
pub(super) fn enforce_disk_quota_against(root: &Path, quota_bytes: u64) -> Result<(), String> {
  let scan_root = if root.exists() {
    root.to_path_buf()
  } else {
    root
      .parent()
      .map(Path::to_path_buf)
      .unwrap_or_else(|| root.to_path_buf())
  };
  let used_bytes = directory_usage_bytes(&scan_root);
  crate::core::remote_provider::check_disk_quota_against(used_bytes, quota_bytes)
    .map_err(|err| err.to_string())
}

pub(super) fn init_repo_path(repo_path: &str) -> Result<RepositoryInspection, String> {
  let trimmed = repo_path.trim();
  if trimmed.is_empty() {
    return Err("Remote path is required".to_string());
  }
  let path = Path::new(trimmed);
  if !path.join(".jj").is_dir() && !path.join(".git").exists() {
    enforce_disk_quota(path)?;
    fs::create_dir_all(path)
      .map_err(|e| format!("filesystem_error: Failed to create {trimmed}: {e}"))?;
    jj::jj_git_init_colocated(trimmed)
      .map_err(|e| format!("jj_command_failed: Failed to initialize repository: {e}"))?;
  }
  if !path.join(".jj").is_dir() {
    // A plain Git repository: colocate jj with it, as opening one locally does.
    match crate::core::init(trimmed) {
      Ok(true) => {}
      Ok(false) => {
        return Err(format!(
          "jj_command_failed: Failed to initialize jj in {trimmed}"
        ))
      }
      Err(e) => return Err(format!("jj_command_failed: {e}")),
    }
  }
  inspect_repository_path(trimmed)
}

pub(super) fn clone_repo_local(
  repo_url: &str,
  destination: &str,
) -> Result<RepositoryInspection, String> {
  if repo_url.trim().is_empty() {
    return Err("Repository URL is required".to_string());
  }
  validate_remote_path(destination)?;
  enforce_disk_quota(Path::new(destination))?;
  let interrupt = std::sync::atomic::AtomicBool::new(false);
  let clone_err = |e: &dyn std::fmt::Display| format!("git_command_failed: {e}");
  // Dropping an unfinished clone removes the partial destination.
  let mut prepare = gix::prepare_clone(repo_url, destination).map_err(|e| clone_err(&e))?;
  let (mut checkout, _) = prepare
    .fetch_then_checkout(gix::progress::Discard, &interrupt)
    .map_err(|e| clone_err(&e))?;
  // A remote HEAD naming a missing branch leaves nothing to check out. An
  // empty remote (no branches at all) still clones, as `git clone` does.
  let repo = checkout.repo();
  let head = repo.head().map_err(|e| clone_err(&e))?;
  let has_branches = repo
    .references()
    .map_err(|e| clone_err(&e))?
    .prefixed("refs/remotes/")
    .map_err(|e| clone_err(&e))?
    .filter_map(Result::ok)
    .any(|r| !r.name().as_bstr().ends_with(b"/HEAD"));
  if head.is_unborn() && has_branches {
    let branch = head
      .referent_name()
      .map(|name| name.shorten().to_string())
      .unwrap_or_default();
    return Err(format!(
      "git_command_failed: remote HEAD points at branch '{branch}', which does not exist"
    ));
  }
  checkout
    .main_worktree(gix::progress::Discard, &interrupt)
    .map_err(|e| clone_err(&e))?;
  inspect_repository_path(destination)
}

/// Fails with the same errors as `inspect_repository_path` for a missing or
/// non-repository path, but only stats the path: no jj load, ref import or
/// working-copy snapshot (which writes an op), so read commands stay read-only.
pub(super) fn require_existing_repo(repo: &str) -> Result<(), String> {
  let root = validate_repository_path(repo)?;
  detect_repository_type(&root).map(|_| ())
}

/// Applies a base64-encoded unified diff to `path` inside the given
/// workspace (or the repo root). Base64 keeps the exec argument vector a plain
/// string even though the payload contains newlines.
pub(super) fn apply_remote_patch(
  repo: &str,
  workspace: Option<i64>,
  path: &str,
  patch_base64: &str,
) -> Result<String, String> {
  let workspace_path = resolve_workspace_path(repo, workspace)?;
  enforce_disk_quota(Path::new(&workspace_path))?;
  let decoded = decode_base64(patch_base64.trim())
    .map_err(|e| format!("invalid_arguments: patch is not valid base64: {e}"))?;
  let patch = String::from_utf8(decoded)
    .map_err(|_| "invalid_arguments: patch is not UTF-8 text".to_string())?;
  jj::jj_apply_file_patch(&workspace_path, path, &patch).map_err(|e| match e {
    jj::JjError::IoError(message) if message.starts_with("Invalid patch:") => {
      format!("invalid_arguments: {message}")
    }
    other => format!("filesystem_error: {other}"),
  })?;
  Ok(format!("Applied patch to {path}"))
}

/// Minimal RFC 4648 base64 decoder (standard alphabet, `=` padding) so patch
/// payloads can travel through the plain-string exec argument vector without
/// pulling in an extra crate dependency.
pub(super) fn decode_base64(input: &str) -> Result<Vec<u8>, String> {
  fn value(byte: u8) -> Option<u8> {
    match byte {
      b'A'..=b'Z' => Some(byte - b'A'),
      b'a'..=b'z' => Some(byte - b'a' + 26),
      b'0'..=b'9' => Some(byte - b'0' + 52),
      b'+' => Some(62),
      b'/' => Some(63),
      _ => None,
    }
  }
  let cleaned: Vec<u8> = input.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
  let mut out = Vec::with_capacity(cleaned.len() / 4 * 3);
  for chunk in cleaned.chunks(4) {
    if chunk.len() < 2 {
      return Err("truncated base64 input".to_string());
    }
    let padding = chunk.iter().filter(|&&b| b == b'=').count();
    let vals: Vec<u8> = chunk
      .iter()
      .filter(|&&b| b != b'=')
      .map(|&b| value(b).ok_or_else(|| "invalid base64 character".to_string()))
      .collect::<Result<_, _>>()?;
    let mut buf = [0u8; 4];
    for (i, v) in vals.iter().enumerate() {
      buf[i] = *v;
    }
    out.push((buf[0] << 2) | (buf[1] >> 4));
    if padding < 2 && vals.len() > 2 {
      out.push((buf[1] << 4) | (buf[2] >> 2));
    }
    if padding < 1 && vals.len() > 3 {
      out.push((buf[2] << 6) | buf[3]);
    }
  }
  Ok(out)
}

// ---------------------------------------------------------------------------
// Native (Phase 4) SSH exec transport for typed commands
// ---------------------------------------------------------------------------
//
// Every reachable connection path, managed or user-managed, goes through the
// pooled native `russh` transport below and in
// `crate::core::remote_ssh_transport`, never a system `ssh` subprocess.
