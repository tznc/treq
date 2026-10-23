use std::path::Path;

use crate::auto_rebase;
use crate::core::workspaces;
use crate::db::Database;
use crate::jj;
use crate::local_db;

fn full_workspace_path(repo_path: &str, workspace_path: &str) -> String {
  if workspace_path.starts_with('/') {
    workspace_path.to_string()
  } else {
    Path::new(repo_path)
      .join(".treq")
      .join("workspaces")
      .join(workspace_path)
      .to_string_lossy()
      .to_string()
  }
}

/// Try to recover the workspace's branch name by reading the workspace's git state,
/// then run the rebase if recovery succeeds. Returns true if a rebase was performed.
fn try_recover_workspace_branch_name(
  repo_path: &str,
  workspace_id: i64,
  stored_branch: &str,
  workspace_full_path: &str,
  conflict_style: &str,
) -> Result<bool, String> {
  // Import the workspace's own git state so jj-lib can see refs from its .git dir.
  let candidate =
    match jj::classify_workspace_bookmark_with_import(workspace_full_path, stored_branch) {
      Ok(jj::WorkspaceBookmarkState::Healthy | jj::WorkspaceBookmarkState::Conflicted) => {
        // Branch visible after git import — DB name was fine; proceed with stored name.
        stored_branch.to_string()
      }
      _ => {
        // Still missing after workspace-scoped import. No recovery possible.
        return Ok(false);
      }
    };

  if candidate != stored_branch {
    local_db::update_workspace_branch_name(repo_path, workspace_id, &candidate)
      .map_err(|e| format!("Failed to update workspace branch name: {}", e))?;
  }

  let result =
    auto_rebase::rebase_single_workspace(repo_path, workspace_id, "main", false, conflict_style)?;
  Ok(result.is_some())
}

pub fn ensure_workspace_rebased(
  repo_path: &str,
  workspace_id: i64,
  conflict_style: &str,
) -> Result<bool, String> {
  let workspace = local_db::get_workspace_by_id(repo_path, workspace_id)?
    .ok_or_else(|| format!("Workspace {} not found", workspace_id))?;

  let target_branch = workspace
    .target_branch
    .clone()
    .unwrap_or_else(|| "main".to_string());

  if workspace.branch_name == target_branch {
    return Ok(false);
  }

  // Classify bookmark state before revset work — revsets fail for conflicted or missing bookmarks.
  match jj::classify_workspace_bookmark(repo_path, &workspace.branch_name)
    .map_err(|e| format!("Failed to classify workspace bookmark: {}", e))?
  {
    jj::WorkspaceBookmarkState::Missing => {
      // Recover real branch name from workspace .git/HEAD when DB has sanitized dir form (dashes vs slashes).
      let workspace_full_path = full_workspace_path(repo_path, &workspace.workspace_path);
      let recovered = try_recover_workspace_branch_name(
        repo_path,
        workspace_id,
        &workspace.branch_name,
        &workspace_full_path,
        conflict_style,
      )?;
      if recovered {
        return Ok(true);
      }
      return Err(format!(
        "Workspace bookmark '{}' not found in repo — skipping auto-rebase. \
                 The workspace's stored bookmark name may be out of sync with jj state.",
        workspace.branch_name
      ));
    }
    jj::WorkspaceBookmarkState::Conflicted => {
      // rebase_single_workspace(force=true) resolves bookmark conflicts before any revset.
      let result = auto_rebase::rebase_single_workspace(
        repo_path,
        workspace_id,
        "main",
        true,
        conflict_style,
      )?;
      return Ok(result.is_some());
    }
    jj::WorkspaceBookmarkState::Healthy => {}
  }

  let workspace_full_path = full_workspace_path(repo_path, &workspace.workspace_path);
  let is_descendant = match jj::jj_workspace_parent_descends_from_target(
    &workspace_full_path,
    &workspace.branch_name,
    &target_branch,
  ) {
    Ok(value) => value,
    Err(jj::JjError::IoError(message))
      if message.contains("could not be resolved")
        || message.contains("did not resolve to a commit") =>
    {
      return Ok(false);
    }
    Err(err) => return Err(format!("Failed to check workspace ancestry: {}", err)),
  };

  if is_descendant {
    return Ok(false);
  }

  let result =
    auto_rebase::rebase_single_workspace(repo_path, workspace_id, "main", false, conflict_style)?;
  Ok(result.is_some())
}

/// Why [`init`] could not set up a repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitError {
  /// The folder at this path has no `.git` (or `.jj`) to open.
  NotGitRepository(String),
  /// Creating the local database or `.treq` directories failed.
  Setup(String),
}

impl std::fmt::Display for InitError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::NotGitRepository(path) => write!(
        f,
        "{path} is not a Git repository. Open a folder that contains a .git directory."
      ),
      Self::Setup(message) => f.write_str(message),
    }
  }
}

impl std::error::Error for InitError {}

impl From<String> for InitError {
  fn from(message: String) -> Self {
    Self::Setup(message)
  }
}

impl From<InitError> for String {
  fn from(error: InitError) -> Self {
    error.to_string()
  }
}

/// Whether `path` is a Git or jj repository [`init`] accepts.
pub fn is_repository(path: &str) -> bool {
  Path::new(path).join(".git").exists() || jj::is_jj_workspace(path)
}

/// Makes `path` a Git repository, as `git init` does.
pub fn init_git_repository(path: &str) -> Result<(), String> {
  gix::init(path)
    .map(drop)
    .map_err(|e| format!("Failed to initialize a Git repository: {e}"))
}

/// Initializes a repository for use with Treq.
///
/// Sets up both the local database (per-repo) and ensures JJ is initialized.
/// Creates the .treq/workspaces directory if it doesn't exist.
///
/// # Arguments
/// * `repo_path` - Path to the repository root
///
/// # Returns
/// Returns true if successful or already initialized, false if JJ initialization failed.
/// Fails with [`InitError::NotGitRepository`], before writing anything, when the
/// folder is not a Git (or jj) repository.
pub fn init(repo_path: &str) -> Result<bool, InitError> {
  match init_repo_state(repo_path)? {
    Some(conflict_style) => {
      rebase_workspaces(repo_path, &conflict_style);
      Ok(true)
    }
    None => Ok(false),
  }
}

/// [`init`], with the rebase of every workspace left to a background thread:
/// the caller gets the repository as soon as it is usable, and a workspace
/// that is behind its target is rebased while the window already draws it.
pub fn init_deferring_rebase(repo_path: &str) -> Result<bool, InitError> {
  let Some(conflict_style) = init_repo_state(repo_path)? else {
    return Ok(false);
  };
  let repo_path = repo_path.to_string();
  std::thread::spawn(move || rebase_workspaces(&repo_path, &conflict_style));
  Ok(true)
}

/// Everything [`init`] does but the rebase pass. The conflict marker style
/// the pass needs, or `None` when jj could not be initialized.
fn init_repo_state(repo_path: &str) -> Result<Option<String>, InitError> {
  if !is_repository(repo_path) {
    return Err(InitError::NotGitRepository(repo_path.to_string()));
  }

  let db_path = local_db::init_local_db(repo_path)?;
  let db = Database::new(db_path).map_err(|e| format!("Failed to open database: {}", e))?;
  db.init()
    .map_err(|e| format!("Failed to initialize database: {}", e))?;

  let workspaces_dir = Path::new(repo_path).join(".treq").join("workspaces");
  std::fs::create_dir_all(&workspaces_dir)
    .map_err(|e| format!("Failed to create workspaces directory: {}", e))?;

  match jj::ensure_jj_initialized(&db, repo_path) {
    Ok(_already_initialized) => {
      let _ = workspaces::sync_workspaces(repo_path);
      Ok(Some(super::resolve_conflict_marker_style_from_db(&db)))
    }
    Err(_) => Ok(None),
  }
}

/// Rebases each workspace that is behind its target branch.
fn rebase_workspaces(repo_path: &str, conflict_style: &str) {
  if let Ok(all_workspaces) = local_db::get_workspaces(repo_path) {
    for workspace in all_workspaces {
      if let Err(err) = ensure_workspace_rebased(repo_path, workspace.id, conflict_style) {
        tracing::warn!(
          "Init rebase warning: workspace '{}' ({}) failed: {}",
          workspace.workspace_name,
          workspace.id,
          err
        );
      }
    }
  }
}
