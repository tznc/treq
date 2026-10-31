mod batch;
mod subtree;

use crate::core::rebase_guard::{self, TargetSync};
use crate::jj::{self, BookmarkConflictCommit, JjRebaseResult};
use crate::local_db::{self, Workspace};
use serde::{Deserialize, Serialize};

pub use batch::check_and_rebase_all;
pub use subtree::rebase_root_subtree_from_workspace_force;

/// Reconstruct full workspace path from a Workspace object.
/// The workspace_path in DB may be just the directory name, so we prepend
/// {repo_path}/.treq/workspaces/ if it's not already an absolute path.
pub(crate) fn get_full_workspace_path(workspace: &Workspace) -> String {
  if workspace.workspace_path.starts_with("/") {
    workspace.workspace_path.clone()
  } else {
    format!(
      "{}/.treq/workspaces/{}",
      workspace.repo_path, workspace.workspace_path
    )
  }
}

/// Result for auto-rebase operation on a group of workspaces
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoRebaseResult {
  pub target_branch: String,
  pub workspaces_rebased: Vec<String>,
  pub rebase_result: JjRebaseResult,
  pub bookmark_conflicts: Vec<WorkspaceBookmarkConflict>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceBookmarkConflict {
  pub workspace_id: i64,
  pub workspace_name: String,
  pub workspace_path: String,
  pub branch_name: String,
  pub bookmark: String,
  pub commits: Vec<BookmarkConflictCommit>,
}

/// Records what holds `workspace` back from rebasing onto `sync`'s target,
/// and whether the rebase may run. A stale target only warns.
pub(crate) fn passes_guard(repo_path: &str, workspace: &Workspace, sync: &TargetSync) -> bool {
  let hold = sync.hold();
  let blocked = hold.as_ref().is_some_and(|h| h.blocks());
  if let Some(hold) = &hold {
    tracing::info!(
      "Rebase of workspace '{}': {}",
      workspace.workspace_name,
      hold.message()
    );
  }
  rebase_guard::record(repo_path, workspace.id, hold);
  !blocked
}

/// Other workspaces' working-copy commits. A rebase leaves them out of the
/// commits it moves, so a child stacked on its parent's working copy (as
/// older stacks were) moves onto the parent's bookmark instead and stops
/// picking up the parent's uncommitted changes.
pub(crate) const OTHER_WORKING_COPIES: &str = "(working_copies() ~ @)";

/// Moves a branch with no commits of its own (its bookmark is an ancestor of
/// `jj_target`) up to `jj_target`, carrying its working copy along. Commit
/// rebases have nothing to move for such a branch, since the bookmark never
/// rides the working copy. Returns whether it moved.
pub(crate) fn fast_forward_branch_without_commits(
  workspace_path: &str,
  branch: &str,
  jj_target: &str,
  conflict_marker_style: &str,
) -> Result<bool, String> {
  let behind = jj::jj_log_revset_commit_ids(
    workspace_path,
    &format!("({branch} & ::({jj_target})) ~ ({jj_target})"),
  )
  .map_err(|e| format!("Failed to compare '{branch}' with '{jj_target}': {e}"))?;
  if behind.is_empty() {
    return Ok(false);
  }
  jj::jj_set_bookmark(workspace_path, branch, jj_target)
    .map_err(|e| format!("Failed to move '{branch}' to '{jj_target}': {e}"))?;
  jj::jj_rebase_with_revset(workspace_path, "@", branch, branch, conflict_marker_style)
    .map_err(|e| format!("Failed to move the working copy onto '{branch}': {e}"))?;
  Ok(true)
}

/// Convert git remote branch format to jj format using centralized logic
pub(crate) fn convert_to_jj_branch_format(branch: &str, repo_path: &str) -> String {
  jj::convert_git_branch_to_jj_format_public(branch, repo_path)
}

struct BookmarkConflictResolution {
  _was_conflicted: bool,
  _local_commits_rebased: usize,
}

/// If `branch_name` is conflicted (local and remote diverged), resolve it by:
/// 1. Capturing local-only mutable commits
/// 2. Pointing the local bookmark to the remote tip
/// 3. Rebasing any local commits onto the new tip
///
/// Returns early (not conflicted) if bookmark is clean.
fn resolve_bookmark_conflict_if_needed(
  workspace_path: &str,
  branch_name: &str,
  _conflict_marker_style: &str,
) -> Result<BookmarkConflictResolution, String> {
  if !jj::jj_is_bookmark_conflicted(workspace_path, branch_name) {
    return Ok(BookmarkConflictResolution {
      _was_conflicted: false,
      _local_commits_rebased: 0,
    });
  }

  let remote_ref = format!("{}@origin", branch_name);
  let resolution =
    jj::jj_resolve_bookmark_conflict_losslessly(workspace_path, branch_name, &remote_ref)
      .map_err(|e| format!("Failed to resolve bookmark conflict: {e}"))?;
  let commits_rebased = resolution.preserved_change_ids.len();

  log::debug!(
    "Resolved bookmark conflict for '{}': rebased {} local commit(s) onto remote tip",
    branch_name,
    commits_rebased
  );

  Ok(BookmarkConflictResolution {
    _was_conflicted: true,
    _local_commits_rebased: commits_rebased,
  })
}

/// Rebase workspaces targeting a specific branch if they have changes
pub fn rebase_workspaces_for_target(
  repo_path: &str,
  target_branch: &str,
  conflict_marker_style: &str,
) -> Result<Option<AutoRebaseResult>, String> {
  // Get all workspaces targeting this branch
  let workspaces = local_db::get_workspaces_by_target_branch(repo_path, target_branch)?;

  if workspaces.is_empty() {
    return Ok(None);
  }

  let sync = rebase_guard::sync_target(repo_path, target_branch);

  // Convert target branch to jj format (origin/main -> main@origin)
  let jj_target_branch = convert_to_jj_branch_format(target_branch, repo_path);

  // Get current target commit
  let current_target_commit = jj::jj_get_commit_id(repo_path, &jj_target_branch)
    .map_err(|e| format!("Failed to get target commit: {}", e))?;

  // Filter workspaces that need rebasing (where last_rebased_commit != current_commit)
  let workspaces_needing_rebase: Vec<&Workspace> = workspaces
    .iter()
    .filter(|w| local_db::last_rebased_commit_of(w).as_ref() != Some(&current_target_commit))
    .collect();

  if workspaces_needing_rebase.is_empty() {
    return Ok(None); // All workspaces already up-to-date
  }

  // Rebase each workspace from its directory so revset resolution includes the working copy.
  let mut workspace_branches = Vec::new();
  let mut all_success = true;
  let mut combined_messages = Vec::new();
  let mut bookmark_conflicts = Vec::new();
  // Bookkeeping is written once after the loop, not once per workspace.
  let mut rebased_ids = Vec::new();

  for workspace in &workspaces_needing_rebase {
    let full_path = get_full_workspace_path(workspace);

    if workspace.branch_name == target_branch {
      match jj::jj_sync_working_copy_if_safe(&full_path, &workspace.branch_name) {
        Ok(true) => {
          log::info!(
                        "Auto-synced working copy for workspace '{}' [rebase_workspaces_for_target::same_branch]",
                        workspace.workspace_name
                    );
        }
        Ok(false) => {
          // Skipped (already synced or has uncommitted changes) - this is fine
        }
        Err(jj::JjError::BookmarkConflict(info)) => {
          bookmark_conflicts.push(WorkspaceBookmarkConflict {
            workspace_id: workspace.id,
            workspace_name: workspace.workspace_name.clone(),
            workspace_path: full_path.clone(),
            branch_name: workspace.branch_name.clone(),
            bookmark: info.bookmark.clone(),
            commits: info.commits.clone(),
          });
          tracing::warn!(
            "Warning: Working copy for workspace '{}' has conflicted bookmark '{}'",
            workspace.workspace_name,
            info.bookmark
          );
        }
        Err(e) => {
          tracing::warn!(
            "Warning: Failed to auto-sync working copy for workspace '{}': {}",
            workspace.workspace_name,
            e
          );
        }
      }

      rebased_ids.push(workspace.id);
      workspace_branches.push(workspace.branch_name.clone());
      combined_messages.push(format!(
        "Workspace '{}': synced with '{}'",
        workspace.workspace_name, target_branch
      ));
      continue;
    }

    if !passes_guard(repo_path, workspace, &sync) {
      combined_messages.push(format!(
        "Workspace '{}': skipped, see its Code tab",
        workspace.workspace_name
      ));
      continue;
    }

    // Resolve bookmark conflicts before building revsets like roots(target..branch_name).
    if let Err(e) =
      resolve_bookmark_conflict_if_needed(&full_path, &workspace.branch_name, conflict_marker_style)
    {
      tracing::warn!(
        "Warning: Failed to resolve bookmark conflict for workspace '{}': {}",
        workspace.workspace_name,
        e
      );
      all_success = false;
      combined_messages.push(format!(
        "Workspace '{}': Failed to resolve conflict - {}",
        workspace.workspace_name, e
      ));
      continue;
    }

    match fast_forward_branch_without_commits(
      &full_path,
      &workspace.branch_name,
      &jj_target_branch,
      conflict_marker_style,
    ) {
      Ok(false) => {}
      Ok(true) => {
        workspace_branches.push(workspace.branch_name.clone());
        combined_messages.push(format!(
          "Workspace '{}': moved up to '{}'",
          workspace.workspace_name, target_branch
        ));
        rebased_ids.push(workspace.id);
        continue;
      }
      Err(e) => {
        all_success = false;
        combined_messages.push(format!("Workspace '{}': {}", workspace.workspace_name, e));
        continue;
      }
    }

    // Use roots() from the workspace directory and rebase committed bookmark history only.
    let rebase_base = jj::jj_resolve_workspace_rebase_base(&full_path, &jj_target_branch)
      .unwrap_or_else(|_| jj_target_branch.clone());
    let revset = format!(
      "roots(({}..{}) ~ {})",
      rebase_base, workspace.branch_name, OTHER_WORKING_COPIES
    );

    let rebase_result = jj::jj_rebase_with_revset(
      &full_path, // Run from workspace directory
      &revset,
      &jj_target_branch,
      &workspace.branch_name, // Set bookmark after rebase
      conflict_marker_style,
    );

    match rebase_result {
      Ok(result) => {
        workspace_branches.push(workspace.branch_name.clone());
        all_success = all_success && result.success;
        combined_messages.push(format!(
          "Workspace '{}': {}",
          workspace.workspace_name, result.message
        ));

        // WC placement is handled inside jj_rebase_with_revset; a separate jj_sync_working_copy_if_safe would clobber the fresh WC commit.

        rebased_ids.push(workspace.id);
      }
      Err(e) => {
        tracing::warn!(
          "Warning: Failed to rebase workspace '{}': {}",
          workspace.workspace_name,
          e
        );
        all_success = false;
        combined_messages.push(format!(
          "Workspace '{}': Failed - {}",
          workspace.workspace_name, e
        ));
      }
    }
  }

  local_db::update_workspaces_last_rebased_commit(repo_path, &rebased_ids, &current_target_commit)?;

  Ok(Some(AutoRebaseResult {
    target_branch: target_branch.to_string(),
    workspaces_rebased: workspace_branches,
    rebase_result: jj::JjRebaseResult {
      success: all_success,
      message: combined_messages.join("\n"),
    },
    bookmark_conflicts,
  }))
}

/// Called after a commit - rebase workspaces that target the committed branch
pub fn rebase_after_commit(
  repo_path: &str,
  committed_branch: &str,
) -> Result<Option<AutoRebaseResult>, String> {
  // Rebase all workspaces targeting this branch using default "diff" markers.
  rebase_workspaces_for_target(repo_path, committed_branch, "diff")
}

/// Rebase a single workspace if it's in detached HEAD state or needs rebasing
/// Returns Some(result) if rebase was performed, None if no rebase needed
/// If force is true, bypasses the needs_rebase check and always performs rebase
pub fn rebase_single_workspace(
  repo_path: &str,
  workspace_id: i64,
  default_branch: &str,
  force: bool,
  conflict_marker_style: &str,
) -> Result<Option<AutoRebaseResult>, String> {
  // Get the specific workspace from DB
  let workspace = local_db::get_workspace_by_id(repo_path, workspace_id)?
    .ok_or_else(|| format!("Workspace {} not found", workspace_id))?;

  // Use workspace target_branch if set, otherwise use default_branch
  let target_branch = workspace
    .target_branch
    .as_ref()
    .unwrap_or(&default_branch.to_string())
    .clone();

  // Skip if branch_name == target_branch (self-rebase)
  if workspace.branch_name == target_branch {
    return Ok(None);
  }

  let full_path = get_full_workspace_path(&workspace);
  // Fetch first so the needs-rebase check sees the target as origin has it.
  let sync = rebase_guard::sync_target(repo_path, &target_branch);
  // Skip the needs-rebase check only when force=true.
  if !force {
    let is_descendant = match jj::jj_workspace_parent_descends_from_target(
      &full_path,
      &workspace.branch_name,
      &target_branch,
    ) {
      Ok(value) => value,
      Err(jj::JjError::IoError(message))
        if message.contains("could not be resolved")
          || message.contains("did not resolve to a commit") =>
      {
        return Ok(None);
      }
      Err(e) => return Err(format!("Failed to check workspace ancestry: {}", e)),
    };
    if is_descendant {
      // Up to date with the last-fetched target; still say when that fetch failed.
      let stale = sync.hold().filter(|h| !h.blocks());
      rebase_guard::record(repo_path, workspace.id, stale);
      return Ok(None);
    }
  }
  if !passes_guard(repo_path, &workspace, &sync) {
    return Ok(None);
  }

  // Convert target branch to jj format
  let jj_target_branch = convert_to_jj_branch_format(&target_branch, repo_path);
  // Still update last_rebased_commit for bookkeeping after successful rebase.
  let current_target_commit = jj::jj_get_commit_id(repo_path, &jj_target_branch)
    .map_err(|e| format!("Failed to get target commit: {}", e))?;

  // Resolve bookmark conflicts before building revsets like roots(target..branch_name).
  resolve_bookmark_conflict_if_needed(&full_path, &workspace.branch_name, conflict_marker_style)
    .map_err(|e| format!("Failed to resolve bookmark conflict: {}", e))?;

  if fast_forward_branch_without_commits(
    &full_path,
    &workspace.branch_name,
    &jj_target_branch,
    conflict_marker_style,
  )? {
    local_db::update_workspace_last_rebased_commit(
      repo_path,
      workspace.id,
      &current_target_commit,
    )?;
    return Ok(Some(AutoRebaseResult {
      target_branch: target_branch.clone(),
      workspaces_rebased: vec![workspace.branch_name],
      rebase_result: JjRebaseResult {
        success: true,
        message: format!("Moved up to '{target_branch}'"),
      },
      bookmark_conflicts: Vec::new(),
    }));
  }

  // Rebase mutable committed history only, excluding @ to keep working copy anchored.
  let rebase_base = jj::jj_resolve_workspace_rebase_base(&full_path, &jj_target_branch)
    .unwrap_or_else(|_| jj_target_branch.clone());
  let revset = format!(
    "roots(mutable() & ({}..{}) ~ @ ~ {})",
    rebase_base, workspace.branch_name, OTHER_WORKING_COPIES
  );
  let rebase_result = jj::jj_rebase_with_revset(
    &full_path, // Run from workspace directory
    &revset,
    &jj_target_branch,
    &workspace.branch_name, // Set bookmark after rebase
    conflict_marker_style,
  )
  .map_err(|e| format!("Rebase failed: {}", e))?;

  // Auto-sync working copy when safe (empty working copy) from workspace dir.
  let mut bookmark_conflicts = Vec::new();
  match jj::jj_sync_working_copy_if_safe(&full_path, &workspace.branch_name) {
    Ok(true) => {
      log::info!(
        "Auto-synced working copy for workspace '{}' [rebase_single_workspace::after_rebase]",
        workspace.workspace_name
      );
    }
    Ok(false) => {} // Skipped - this is fine
    Err(jj::JjError::BookmarkConflict(info)) => {
      bookmark_conflicts.push(WorkspaceBookmarkConflict {
        workspace_id: workspace.id,
        workspace_name: workspace.workspace_name.clone(),
        workspace_path: full_path.clone(),
        branch_name: workspace.branch_name.clone(),
        bookmark: info.bookmark.clone(),
        commits: info.commits.clone(),
      });
      tracing::warn!(
        "Warning: Working copy for workspace '{}' has conflicted bookmark '{}'",
        workspace.workspace_name,
        info.bookmark
      );
    }
    Err(e) => {
      tracing::warn!(
        "Warning: Failed to auto-sync working copy for workspace '{}': {}",
        workspace.workspace_name,
        e
      );
      // For single-workspace rebase, log sync failures without failing successful bookmark rebase.
    }
  }

  local_db::update_workspace_last_rebased_commit(repo_path, workspace.id, &current_target_commit)?;

  Ok(Some(AutoRebaseResult {
    target_branch,
    workspaces_rebased: vec![workspace.branch_name],
    rebase_result,
    bookmark_conflicts,
  }))
}
