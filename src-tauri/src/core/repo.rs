use crate::lock_ext::LockExt;
use crate::result_ext::ResultExt;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use crate::jj;

use super::workspaces::RemoteSyncStatus;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RepoStatus {
  pub has_changes: bool,
  pub has_conflicts: bool,
  pub remote_sync: RemoteSyncStatus,
  pub fetch_error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RepoBranch {
  pub current_branch: Option<String>,
  pub display_ref: String,
  pub is_detached: bool,
}

static REPO_COMMIT_LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();

/// Mutex shared with `commit_repo` and `commit_workspace` so jj commits for one repo path never run concurrently.
pub(crate) fn commit_lock_for_repo(repo_path: &str) -> Arc<Mutex<()>> {
  let locks = REPO_COMMIT_LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
  let mut guard = locks.lock_or_recover();
  guard
    .entry(repo_path.to_string())
    .or_insert_with(|| Arc::new(Mutex::new(())))
    .clone()
}

/// Create a new commit on the home repo from the current jj working copy (colocated layout).
pub fn commit_repo(repo_path: &str, message: &str) -> Result<String, String> {
  let lock = commit_lock_for_repo(repo_path);
  let _guard = lock.lock_or_recover();
  let result =
    jj::jj_commit(repo_path, message).map_err(|e| format!("Failed to create commit: {}", e))?;
  jj::jj_drop_autosave_ancestors(repo_path).warn_on_err("jj drop autosave ancestors");
  Ok(result)
}

/// Returns the current checkout branch information for the home repo.
///
/// Imports colocated git refs into jj (same as [`list_repo_branches`]), then resolves the current
/// checkout strictly from the working copy state.
pub fn get_repo_current_branch(repo_path: &str) -> Result<RepoBranch, String> {
  jj::jj_util_import_git_refs(repo_path).map_err(|e| e.to_string())?;
  // A jj rewrite/ref export can occasionally leave colocated Git HEAD detached at the
  // default branch tip. Repair only that unambiguous case; preserve intentional detached
  // checkouts at any other commit.
  let _ = jj::repair_detached_home_head_at_default_branch(repo_path).map_err(|e| e.to_string())?;
  let resolved_branch = jj::resolve_home_repo_branch(repo_path).map_err(|e| e.to_string())?;
  let detached_head = read_detached_head_commit(repo_path)?;
  let is_detached = detached_head.is_some() || resolved_branch == "HEAD";

  let current_branch = if is_detached {
    None
  } else {
    Some(resolved_branch.clone())
  };

  let display_ref = if let Some(head) = detached_head {
    head
  } else if resolved_branch == "HEAD" {
    jj::jj_get_commit_id(repo_path, "@")
      .map_err(|e| e.to_string())?
      .chars()
      .take(12)
      .collect()
  } else {
    resolved_branch
  };

  Ok(RepoBranch {
    current_branch,
    display_ref,
    is_detached,
  })
}

/// Returns the repository default branch using jj's canonical resolver.
pub fn get_repo_default_branch(repo_path: &str) -> Result<String, String> {
  jj::get_default_branch(repo_path).map_err(|e| e.to_string())
}

fn read_detached_head_commit(repo_path: &str) -> Result<Option<String>, String> {
  let head_path = Path::new(repo_path).join(".git").join("HEAD");
  let content =
    fs::read_to_string(&head_path).map_err(|e| format!("Failed to read .git/HEAD: {e}"))?;
  let trimmed = content.trim();
  if trimmed.starts_with("ref: ") {
    return Ok(None);
  }
  let short: String = trimmed.chars().take(12).collect();
  if short.is_empty() {
    return Err("Failed to resolve detached HEAD commit".to_string());
  }
  Ok(Some(short))
}

/// Returns the current status of the repository, including a git fetch.
///
/// Performs `jj git fetch` first so that remote sync status reflects the
/// latest remote state. Fetch failures are captured in `fetch_error` and do
/// not prevent the rest of the status from being returned.
///
/// Also tracks remote bookmarks for all workspaces (best-effort, errors
/// are silently ignored).
pub fn repo_status(repo_path: &str) -> Result<RepoStatus, String> {
  // Step 1: fetch — capture error but continue
  let fetch_error = jj::jj_git_fetch(repo_path).err().map(|e| e.to_string());
  // Keep jj view aligned with colocated git state before status checks.
  jj::jj_util_import_git_refs(repo_path).warn_on_err("jj util import git refs");

  // Step 2: default branch for conflict/change checks
  let default_branch = get_repo_default_branch(repo_path).unwrap_or_else(|_| "main".to_string());

  // Step 3: uncommitted changes
  let has_changes = jj::jj_get_changed_files(repo_path)
    .map(|files| files.iter().any(|file| !is_repo_noise_path(&file.path)))
    .unwrap_or(false);

  // Step 4: conflicts
  let has_conflicts = jj::get_conflicted_files(repo_path, Some(&default_branch))
    .map(|files| !files.is_empty())
    .unwrap_or(false);

  // Step 5: remote sync status for the checked-out branch (same branch pull/push use).
  // Do not use bookmarks on `@-`: the home WC can lag on the default-branch tip while
  // git HEAD is on a different branch that is ahead/behind its remote.
  let checked_out_branch = jj::resolve_home_repo_branch(repo_path)
    .ok()
    .filter(|branch| !branch.is_empty() && branch != "HEAD")
    .unwrap_or_else(|| default_branch.clone());

  let remote_sync = match jj::check_branch_exists(repo_path, &checked_out_branch) {
    Ok(status) if !status.remote_exists => RemoteSyncStatus::NotOnRemote,
    _ => match jj::jj_get_sync_status(repo_path, &checked_out_branch, false) {
      Ok((0, 0)) => RemoteSyncStatus::InSync,
      Ok((a, 0)) => RemoteSyncStatus::Ahead { count: a },
      Ok((0, b)) => RemoteSyncStatus::Behind { count: b },
      Ok((a, b)) => RemoteSyncStatus::Diverged {
        ahead: a,
        behind: b,
      },
      Err(_) => RemoteSyncStatus::NotOnRemote,
    },
  };

  Ok(RepoStatus {
    has_changes,
    has_conflicts,
    remote_sync,
    fetch_error,
  })
}

fn is_repo_noise_path(path: &str) -> bool {
  path == "node_modules"
    || path.starts_with("node_modules/")
    || path == ".treq"
    || path.starts_with(".treq/")
    || path == ".jj"
    || path.starts_with(".jj/")
    || path.starts_with(".jj")
}

/// Returns the list of local bookmarks (branches), after syncing colocated git into jj.
pub fn list_repo_branches(repo_path: &str) -> Result<Vec<jj::JjBranch>, String> {
  jj::jj_util_import_git_refs(repo_path).map_err(|e| e.to_string())?;
  jj::get_branches(repo_path).map_err(|e| e.to_string())
}

/// Fetches the repository's remotes, returning what the fetch reported.
pub fn fetch_repo(repo_path: &str) -> Result<String, String> {
  jj::jj_git_fetch(repo_path).map_err(|e| e.to_string())
}

/// Whether `branch` exists locally and on `origin`.
pub fn repo_branch_status(repo_path: &str, branch: &str) -> Result<jj::BranchStatus, String> {
  jj::check_branch_exists(repo_path, branch).map_err(|e| e.to_string())
}

/// Whether rebasing the home repo's `branch` onto `target_branch` would
/// likely conflict; `None` when that cannot be told.
pub fn dry_run_home_rebase(
  repo_path: &str,
  branch: &str,
  target_branch: &str,
) -> Option<jj::HomeRebaseDryRunResult> {
  jj::jj_dry_run_home_repo_rebase(repo_path, branch, target_branch).ok()
}

/// Rebases the home repo's `branch` onto `target_branch`.
pub fn rebase_home_branch(
  repo_path: &str,
  branch: &str,
  target_branch: &str,
) -> Result<jj::JjRebaseResult, String> {
  jj::jj_rebase_home_repo_branch(repo_path, branch, target_branch).map_err(|e| e.to_string())
}

/// Whether the user trusted the repo to run its workflows and scripts.
pub fn is_repo_trusted(repo_path: &str) -> bool {
  crate::local_db::is_repo_trusted(repo_path)
}

/// Records that the user trusts the repo.
pub fn trust_repo(repo_path: &str) -> Result<(), String> {
  crate::local_db::trust_repo(repo_path)
}

/// Switches the repository working copy to the given bookmark (branch).
pub fn switch_repo_branch(repo_path: &str, bookmark_name: &str) -> Result<String, String> {
  jj::jj_edit_bookmark(repo_path, bookmark_name).map_err(|e| e.to_string())
}

const SKIP_GITIGNORE_SUGGESTION_NAMES: &[&str] = &[".git", ".jj", ".treq", ".vscode"];

/// Suggests paths suitable for workspace symlinks / copy overlays.
///
/// Combines:
/// 1. Root-level entries that exist and are ignored by `.gitignore`
/// 2. Simple (non-glob) root patterns listed in `.gitignore`
///
/// Results are deduped and sorted. Internal treq/jj/git dirs are excluded.
pub fn list_gitignored_path_suggestions(repo_path: &str) -> Result<Vec<String>, String> {
  let repo = Path::new(repo_path);
  if !repo.exists() {
    return Ok(Vec::new());
  }

  let mut suggestions = std::collections::BTreeSet::new();

  for pattern in simple_gitignore_root_patterns(repo)? {
    suggestions.insert(pattern);
  }

  for name in list_ignored_root_entries(repo)? {
    suggestions.insert(name);
  }

  Ok(suggestions.into_iter().collect())
}

fn list_ignored_root_entries(repo: &Path) -> Result<Vec<String>, String> {
  let walker = ignore::WalkBuilder::new(repo)
    .max_depth(Some(1))
    .hidden(false)
    .git_ignore(true)
    .git_global(true)
    .git_exclude(true)
    .build();

  let all_entries: Vec<_> = fs::read_dir(repo)
    .map_err(|e| format!("Failed to read directory: {}", e))?
    .filter_map(|e| e.ok())
    .filter(|e| {
      e.file_name()
        .to_str()
        .map(|name| !SKIP_GITIGNORE_SUGGESTION_NAMES.contains(&name))
        .unwrap_or(false)
    })
    .map(|e| e.path())
    .collect();

  let non_ignored: std::collections::HashSet<_> = walker
    .filter_map(|e| e.ok())
    .map(|e| e.path().to_path_buf())
    .collect();

  let mut ignored = Vec::new();
  for entry_path in all_entries {
    if !non_ignored.contains(&entry_path) {
      if let Some(name) = entry_path.file_name().and_then(|n| n.to_str()) {
        ignored.push(name.to_string());
      }
    }
  }
  Ok(ignored)
}

fn simple_gitignore_root_patterns(repo: &Path) -> Result<Vec<String>, String> {
  let gitignore_path = repo.join(".gitignore");
  if !gitignore_path.exists() {
    return Ok(Vec::new());
  }
  let content =
    fs::read_to_string(&gitignore_path).map_err(|e| format!("Failed to read .gitignore: {}", e))?;

  let mut patterns = Vec::new();
  for line in content.lines() {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
      continue;
    }
    if line.contains('*') || line.contains('?') || line.contains('[') {
      continue;
    }
    let pattern = line.trim_start_matches('/').trim_end_matches('/');
    if pattern.is_empty() || pattern.contains("..") {
      continue;
    }
    // Only suggest single-segment root paths (e.g. node_modules, .venv).
    if pattern.contains('/') {
      continue;
    }
    if SKIP_GITIGNORE_SUGGESTION_NAMES.contains(&pattern) {
      continue;
    }
    patterns.push(pattern.to_string());
  }
  Ok(patterns)
}

#[cfg(test)]
mod gitignore_suggestion_tests {
  use super::*;
  use tempfile::TempDir;

  #[test]
  fn suggests_ignored_root_entries_and_gitignore_patterns() {
    let temp = TempDir::new().expect("temp dir");
    let repo = temp.path();
    // WalkBuilder git_ignore discovers ignores relative to a git root.
    fs::create_dir_all(repo.join(".git")).expect("mkdir .git");
    fs::write(
      repo.join(".gitignore"),
      "node_modules/\ntarget/\n*.log\nnested/cache/\n",
    )
    .expect("write gitignore");
    fs::create_dir_all(repo.join("node_modules")).expect("mkdir node_modules");
    fs::create_dir_all(repo.join("src")).expect("mkdir src");
    fs::write(repo.join("src/main.rs"), "fn main() {}\n").expect("write src");

    let suggestions =
      list_gitignored_path_suggestions(repo.to_str().unwrap()).expect("suggestions should succeed");
    assert!(
      suggestions.contains(&"node_modules".to_string()),
      "existing ignored dir should be suggested: {:?}",
      suggestions
    );
    assert!(
      suggestions.contains(&"target".to_string()),
      "gitignore pattern target/ should be suggested: {:?}",
      suggestions
    );
    assert!(
      !suggestions.iter().any(|s| s.contains('*')),
      "glob patterns must not be suggested"
    );
    assert!(
      !suggestions.iter().any(|s| s.contains('/')),
      "nested patterns must not be suggested"
    );
    assert!(!suggestions.contains(&"src".to_string()));
  }

  #[test]
  fn returns_empty_when_repo_missing() {
    let suggestions = list_gitignored_path_suggestions("/tmp/treq-does-not-exist-xyz")
      .expect("missing path should not error");
    assert!(suggestions.is_empty());
  }
}
