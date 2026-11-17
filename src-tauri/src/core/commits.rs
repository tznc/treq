use std::collections::HashSet;
use std::path::Path;

use crate::jj;
use crate::local_db;

fn append_target_branch_commits(
  result: &mut jj::JjLogResult,
  target_commits: Vec<jj::JjLogCommit>,
) {
  let existing_ids: HashSet<String> = result.commits.iter().map(|c| c.commit_id.clone()).collect();

  for mut commit in target_commits {
    if existing_ids.contains(&commit.commit_id) {
      continue;
    }
    commit.on_target_only = true;
    result.commits.push(commit);
  }
}

/// The workspaces `workspace` is stacked on, nearest first, from one read
/// of the workspace list.
fn collect_stacked_workspaces(
  repo_path: &str,
  workspace: &local_db::Workspace,
) -> Result<Vec<local_db::Workspace>, String> {
  let workspaces =
    local_db::get_workspaces(repo_path).map_err(|e| format!("Failed to get workspaces: {}", e))?;
  let by_branch: std::collections::HashMap<String, local_db::Workspace> = workspaces
    .into_iter()
    .map(|workspace| (workspace.branch_name.clone(), workspace))
    .collect();

  let mut stacked = Vec::new();
  let mut current_branch = workspace.branch_name.clone();
  let mut visited = HashSet::new();

  while visited.insert(current_branch.clone()) {
    let Some(next_branch) = by_branch
      .get(&current_branch)
      .and_then(|workspace| workspace.target_branch.clone())
    else {
      break;
    };
    if visited.contains(&next_branch) {
      break;
    }
    let Some(next) = by_branch.get(&next_branch) else {
      break;
    };
    stacked.push(next.clone());
    current_branch = next_branch;
  }

  Ok(stacked)
}

/// Lists commits for a workspace by its database ID, or for the home repo
/// when no workspace ID is provided.
///
/// # Arguments
/// * `repo_path`                     - Path to the repository root
/// * `workspace_id`                  - ID of the workspace to list commits for, or `None` for the home repo
/// * `include_target_branch_history` - When true, append target-branch history after the
///   current-branch commits (marked with `on_target_only = true`). Applies to both
///   workspace and home-repo listings.
///
/// Returns a single `commits` list: current-branch history first, then optional
/// target-branch history when `include_target_branch_history` is true.
/// * `target_branch_limit`           - Max commits to load for target-branch history; defaults to 10 when `include_target_branch_history` is true
/// * `limit`                         - Max commits for the home-repo log only; ignored when `workspace_id` is set
///
/// # Returns
/// The parsed log result on success, or an error string.
pub fn list_commits(
  repo_path: &str,
  workspace_id: Option<i64>,
  include_target_branch_history: bool,
  target_branch_limit: Option<usize>,
  limit: Option<usize>,
) -> Result<jj::JjLogResult, String> {
  match workspace_id {
    Some(id) => {
      let workspace = local_db::get_workspace_by_id(repo_path, id)
        .map_err(|e| format!("Failed to get workspace: {}", e))?
        .ok_or_else(|| format!("Workspace not found: {}", id))?;
      let default_branch = jj::get_default_branch(repo_path)
        .map_err(|e| format!("Failed to resolve default branch: {}", e))?;

      let workspace_dir = Path::new(repo_path)
        .join(".treq")
        .join("workspaces")
        .join(&workspace.workspace_path);
      let workspace_dir_str = workspace_dir
        .to_str()
        .ok_or("Failed to convert workspace path to string")?;

      let target_branch = workspace
        .target_branch
        .as_deref()
        .unwrap_or(&default_branch);
      let is_default_branch_workspace = workspace.branch_name == default_branch;

      let mut result = jj::jj_get_log(
        workspace_dir_str,
        target_branch,
        Some(is_default_branch_workspace),
        None,
      )
      .map_err(|e| format!("Failed to list commits: {}", e))?;

      if let Ok(stacked) = collect_stacked_workspaces(repo_path, &workspace) {
        for stacked_workspace in stacked {
          let stacked_workspace_dir = Path::new(repo_path)
            .join(".treq")
            .join("workspaces")
            .join(&stacked_workspace.workspace_path);
          let stacked_workspace_dir_str = stacked_workspace_dir
            .to_str()
            .ok_or("Failed to convert stacked workspace path to string")?;
          if let Some(tentative) = jj::jj_get_tentative_working_copy(
            stacked_workspace_dir_str,
            &stacked_workspace.branch_name,
          )
          .map_err(|e| format!("Failed to list stacked working copy: {}", e))?
          {
            result.commits.push(tentative.commit);
          }
        }
      }

      if include_target_branch_history && !is_default_branch_workspace {
        let target_limit = target_branch_limit.unwrap_or(10);
        let target_commits = jj::jj_get_target_branch_log(repo_path, target_branch, target_limit)
          .map_err(|e| format!("Failed to list target branch history: {}", e))?;
        append_target_branch_commits(&mut result, target_commits);
      }

      Ok(result)
    }
    None => {
      let branch = jj::resolve_home_repo_branch(repo_path)
        .map_err(|e| format!("Failed to get active branch: {}", e))?;
      let default_branch = jj::get_default_branch(repo_path)
        .map_err(|e| format!("Failed to resolve default branch: {}", e))?;
      if branch != default_branch {
        let mut result =
          jj::jj_get_home_repo_diverged_log(repo_path, &branch, &default_branch, limit)
            .map_err(|e| format!("Failed to list commits: {}", e))?;
        if include_target_branch_history {
          // Also show default-branch history when the branch has no target-ahead commits.
          let target_limit = target_branch_limit.unwrap_or(10);
          let target_commits =
            jj::jj_get_target_branch_log(repo_path, &default_branch, target_limit)
              .map_err(|e| format!("Failed to list target branch history: {}", e))?;
          append_target_branch_commits(&mut result, target_commits);
        } else {
          result.commits.retain(|c| !c.on_target_only);
        }
        Ok(result)
      } else {
        jj::jj_get_log(repo_path, &branch, Some(true), limit)
          .map_err(|e| format!("Failed to list commits: {}", e))
      }
    }
  }
}

/// Moves a specific commit from a source workspace into an existing target workspace.
///
/// Squashes the specified commit's changes into the target workspace's working copy.
///
/// # Arguments
/// * `repo_path`            - Path to the repository root
/// * `source_workspace_id`  - ID of the workspace that owns the commit
/// * `commit_change_id`     - The short change-id of the commit to move
/// * `target_workspace_id`  - ID of the destination workspace
///
/// # Returns
/// `Ok(())` on success, or an error string.
pub fn move_commit_to_existing_workspace(
  repo_path: &str,
  source_workspace_id: i64,
  commit_change_id: &str,
  target_workspace_id: i64,
) -> Result<(), String> {
  // Resolve full path of the source workspace
  let source = local_db::get_workspace_by_id(repo_path, source_workspace_id)
    .map_err(|e| format!("Failed to get source workspace: {}", e))?
    .ok_or_else(|| format!("Source workspace not found: {}", source_workspace_id))?;

  // Resolve target workspace
  let target = local_db::get_workspace_by_id(repo_path, target_workspace_id)
    .map_err(|e| format!("Failed to get target workspace: {}", e))?
    .ok_or_else(|| format!("Target workspace not found: {}", target_workspace_id))?;

  let source_full_path = Path::new(repo_path)
    .join(".treq")
    .join("workspaces")
    .join(&source.workspace_path);
  let source_full_path_str = source_full_path
    .to_str()
    .ok_or("Failed to convert source workspace path to string")?
    .to_string();

  if source_workspace_id == target_workspace_id {
    return Err("invalid_arguments: cannot move a commit into its own workspace".to_string());
  }
  ensure_rewritable(repo_path, &source, commit_change_id)?;
  let target_workspace_dir = Path::new(repo_path)
    .join(".treq")
    .join("workspaces")
    .join(&target.workspace_path);

  // Squash the commit into the target working copy and drop it from the source history
  jj::squash_commit_to_workspace(
    &source_full_path_str,
    commit_change_id,
    &target_workspace_dir.to_string_lossy(),
  )
  .map_err(|e| format!("Failed to move commit to target workspace: {}", e))?;

  Ok(())
}

/// The directory jj runs in for `workspace_id`, plus the workspace row.
/// `None` is the home repository: the repo root itself, with no workspace
/// lineage to guard.
fn workspace_target(
  repo_path: &str,
  workspace_id: Option<i64>,
) -> Result<(String, Option<local_db::Workspace>), String> {
  let Some(id) = workspace_id else {
    return Ok((repo_path.to_string(), None));
  };
  let workspace = local_db::get_workspace_by_id(repo_path, id)
    .map_err(|e| format!("Failed to get workspace: {}", e))?
    .ok_or_else(|| format!("Workspace not found: {}", id))?;
  let dir = Path::new(repo_path)
    .join(".treq")
    .join("workspaces")
    .join(&workspace.workspace_path)
    .to_str()
    .ok_or("Failed to convert workspace path to string")?
    .to_string();
  Ok((dir, Some(workspace)))
}

/// Abandons a specific commit from a workspace by change-id.
///
/// # Arguments
/// * `repo_path`         - Path to the repository root
/// * `workspace_id`      - ID of the workspace that owns the commit
/// * `commit_change_id`  - The short change-id of the commit to abandon
///
/// # Returns
/// The operation id of the abandon, for toast undo.
pub fn abandon_commit(
  repo_path: &str,
  workspace_id: impl Into<Option<i64>>,
  commit_change_id: &str,
) -> Result<String, String> {
  let (workspace_dir, workspace) = workspace_target(repo_path, workspace_id.into())?;
  let workspace_dir_str = workspace_dir.as_str();

  ensure_target_rewritable(repo_path, workspace.as_ref(), commit_change_id)?;

  let op_id = jj::jj_abandon(workspace_dir_str, commit_change_id)
    .map_err(|e| format!("Failed to abandon commit: {}", e))?;

  jj::update_stale_workspace(workspace_dir_str)
    .map_err(|e| format!("Failed to update workspace working copy: {}", e))?;

  Ok(op_id)
}

/// [`ensure_rewritable`] for a workspace; for the home repository (`None`),
/// refuses commits already pushed to the home branch's `origin` bookmark.
fn ensure_target_rewritable(
  repo_path: &str,
  workspace: Option<&local_db::Workspace>,
  revision: &str,
) -> Result<(), String> {
  match workspace {
    Some(workspace) => ensure_rewritable(repo_path, workspace, revision),
    None => {
      let branch = jj::resolve_home_repo_branch(repo_path)
        .map_err(|e| format!("Failed to get active branch: {}", e))?;
      jj::ensure_home_commit_rewritable(repo_path, revision, &branch).map_err(|e| e.to_string())
    }
  }
}

/// Refuses `revision` unless `workspace` may rewrite it: it must be in the
/// workspace's own history and not on the default branch or on the parent
/// branch a stacked workspace targets (those commits belong to another workspace).
pub(crate) fn ensure_rewritable(
  repo_path: &str,
  workspace: &local_db::Workspace,
  revision: &str,
) -> Result<(), String> {
  let workspace_dir = Path::new(repo_path)
    .join(".treq")
    .join("workspaces")
    .join(&workspace.workspace_path);
  let default_branch = jj::get_default_branch(repo_path).ok();
  jj::ensure_commit_rewritable(
    &workspace_dir.to_string_lossy(),
    revision,
    default_branch.as_deref(),
    workspace.target_branch.as_deref(),
  )
  .map_err(|e| e.to_string())
}

/// Undo a repository operation identified by `operation_id` (hex).
/// The operation must still be the current head.
pub fn undo_repo_operation(
  repo_path: &str,
  workspace_id: Option<i64>,
  operation_id: &str,
) -> Result<String, String> {
  let workspace_dir = match workspace_id {
    Some(id) => {
      let workspace = local_db::get_workspace_by_id(repo_path, id)
        .map_err(|e| format!("Failed to get workspace: {}", e))?
        .ok_or_else(|| format!("Workspace not found: {}", id))?;
      Path::new(repo_path)
        .join(".treq")
        .join("workspaces")
        .join(&workspace.workspace_path)
        .to_str()
        .ok_or("Failed to convert workspace path to string")?
        .to_string()
    }
    None => repo_path.to_string(),
  };
  jj::jj_undo_operation(&workspace_dir, operation_id)
    .map_err(|e| format!("Failed to undo operation: {}", e))
}

/// Undoes the latest commit in a workspace's own lineage by change-id.
///
/// Only the workspace's current tip commit can be undone — not the working
/// copy, and not a commit inherited from the target branch. To undo an older
/// commit, undo newer commits first, one at a time.
///
/// # Arguments
/// * `repo_path`         - Path to the repository root
/// * `workspace_id`      - ID of the workspace that owns the commit
/// * `commit_change_id`  - The short change-id of the commit to undo
///
/// # Returns
/// `Ok(())` on success, or an error string.
pub fn undo_commit(
  repo_path: &str,
  workspace_id: i64,
  commit_change_id: &str,
) -> Result<(), String> {
  let workspace = local_db::get_workspace_by_id(repo_path, workspace_id)
    .map_err(|e| format!("Failed to get workspace: {}", e))?
    .ok_or_else(|| format!("Workspace not found: {}", workspace_id))?;

  let workspace_dir = Path::new(repo_path)
    .join(".treq")
    .join("workspaces")
    .join(&workspace.workspace_path);
  let workspace_dir_str = workspace_dir
    .to_str()
    .ok_or("Failed to convert workspace path to string")?;

  ensure_rewritable(repo_path, &workspace, commit_change_id)?;
  let default_branch = jj::get_default_branch(repo_path)
    .map_err(|e| format!("Failed to resolve default branch: {}", e))?;
  let target_branch = workspace
    .target_branch
    .as_deref()
    .unwrap_or(&default_branch);

  jj::jj_undo_commit(workspace_dir_str, target_branch, commit_change_id)
    .map_err(|e| format!("Failed to undo commit: {}", e))?;

  jj::update_stale_workspace(workspace_dir_str)
    .map_err(|e| format!("Failed to update workspace working copy: {}", e))?;

  Ok(())
}

/// Reverts a specific commit by change-id, creating a new commit that reverses
/// its changes on top of the workspace's current tip. Can target any real
/// commit reachable from the workspace except the working-copy commit itself.
///
/// # Arguments
/// * `repo_path`         - Path to the repository root
/// * `workspace_id`      - ID of the workspace that owns the commit
/// * `commit_change_id`  - The short change-id of the commit to revert
///
/// # Returns
/// `Ok(())` on success, or an error string.
pub fn revert_commit(
  repo_path: &str,
  workspace_id: impl Into<Option<i64>>,
  commit_change_id: &str,
) -> Result<(), String> {
  let (workspace_dir, _) = workspace_target(repo_path, workspace_id.into())?;
  let workspace_dir_str = workspace_dir.as_str();

  jj::jj_revert_commit(workspace_dir_str, commit_change_id)
    .map_err(|e| format!("Failed to revert commit: {}", e))?;

  jj::update_stale_workspace(workspace_dir_str)
    .map_err(|e| format!("Failed to update workspace working copy: {}", e))?;

  Ok(())
}

/// Returns the full (multi-line) description of a specific commit.
///
/// # Arguments
/// * `repo_path`         - Path to the repository root
/// * `workspace_id`      - ID of the workspace that owns the commit
/// * `commit_change_id`  - The short change-id of the commit
///
/// # Returns
/// The commit's full description on success, or an error string.
pub fn get_commit_description(
  repo_path: &str,
  workspace_id: impl Into<Option<i64>>,
  commit_change_id: &str,
) -> Result<String, String> {
  let (workspace_dir, _) = workspace_target(repo_path, workspace_id.into())?;
  let workspace_dir_str = workspace_dir.as_str();

  jj::jj_get_commit_description(workspace_dir_str, commit_change_id)
    .map_err(|e| format!("Failed to get commit description: {}", e))
}

/// Sets (rewrites) the description of a specific commit by change-id.
///
/// # Arguments
/// * `repo_path`         - Path to the repository root
/// * `workspace_id`      - ID of the workspace that owns the commit
/// * `commit_change_id`  - The short change-id of the commit to describe
/// * `description`       - The new commit description
///
/// # Returns
/// `Ok(())` on success, or an error string.
pub fn describe_commit(
  repo_path: &str,
  workspace_id: impl Into<Option<i64>>,
  commit_change_id: &str,
  description: &str,
) -> Result<(), String> {
  let (workspace_dir, workspace) = workspace_target(repo_path, workspace_id.into())?;
  let workspace_dir_str = workspace_dir.as_str();

  ensure_target_rewritable(repo_path, workspace.as_ref(), commit_change_id)?;

  jj::jj_describe(workspace_dir_str, commit_change_id, description)
    .map_err(|e| format!("Failed to describe commit: {}", e))?;

  jj::update_stale_workspace(workspace_dir_str)
    .map_err(|e| format!("Failed to update workspace working copy: {}", e))?;

  Ok(())
}

/// Shift a mutable commit's author timestamp, then repair descendant timestamps
/// on this workspace's first-parent lineage so history stays strictly ordered.
pub fn shift_commit_timestamp(
  repo_path: &str,
  workspace_id: i64,
  commit_change_id: &str,
  days: i64,
  hours: i64,
  minutes: i64,
  to_day: Option<&str>,
) -> Result<(), String> {
  let workspace = local_db::get_workspace_by_id(repo_path, workspace_id)
    .map_err(|e| format!("Failed to get workspace: {}", e))?
    .ok_or_else(|| format!("Workspace not found: {}", workspace_id))?;

  let workspace_dir = Path::new(repo_path)
    .join(".treq")
    .join("workspaces")
    .join(&workspace.workspace_path);
  let workspace_dir_str = workspace_dir
    .to_str()
    .ok_or("Failed to convert workspace path to string")?;

  let default_branch = jj::get_default_branch(repo_path)
    .map_err(|e| format!("Failed to resolve default branch: {}", e))?;
  let target_branch = workspace
    .target_branch
    .as_deref()
    .unwrap_or(&default_branch);

  ensure_rewritable(repo_path, &workspace, commit_change_id)?;
  let shift = match to_day {
    Some(day) => {
      if days != 0 || hours != 0 || minutes != 0 {
        return Err("Provide either a day or a duration offset, not both".to_string());
      }
      let (year, month, day) = crate::commit_timestamps::parse_day(day)?;
      crate::commit_timestamps::TimestampShift::ToDay { year, month, day }
    }
    None => {
      if days == 0 && hours == 0 && minutes == 0 {
        return Err("No timestamp change specified".to_string());
      }
      crate::commit_timestamps::TimestampShift::Offset {
        days,
        hours,
        minutes,
      }
    }
  };

  jj::jj_shift_commit_timestamps(workspace_dir_str, commit_change_id, target_branch, &shift)
    .map_err(|e| format!("Failed to shift commit timestamps: {}", e))?;

  jj::update_stale_workspace(workspace_dir_str)
    .map_err(|e| format!("Failed to update workspace working copy: {}", e))?;

  Ok(())
}

/// Shift this workspace's mutable first-parent lineage so the newest real
/// commit lands at now, preserving gaps between commits.
pub fn shift_mutable_commits_to_now(repo_path: &str, workspace_id: i64) -> Result<(), String> {
  let workspace = local_db::get_workspace_by_id(repo_path, workspace_id)
    .map_err(|e| format!("Failed to get workspace: {}", e))?
    .ok_or_else(|| format!("Workspace not found: {}", workspace_id))?;

  let workspace_dir = Path::new(repo_path)
    .join(".treq")
    .join("workspaces")
    .join(&workspace.workspace_path);
  let workspace_dir_str = workspace_dir
    .to_str()
    .ok_or("Failed to convert workspace path to string")?;

  let default_branch = jj::get_default_branch(repo_path)
    .map_err(|e| format!("Failed to resolve default branch: {}", e))?;
  let target_branch = workspace
    .target_branch
    .as_deref()
    .unwrap_or(&default_branch);

  jj::jj_shift_mutable_lineage_to_now(workspace_dir_str, target_branch)
    .map_err(|e| format!("Failed to shift commits to now: {}", e))?;

  jj::update_stale_workspace(workspace_dir_str)
    .map_err(|e| format!("Failed to update workspace working copy: {}", e))?;

  Ok(())
}

/// Returns the diff for a specific commit in a workspace.
///
/// # Arguments
/// * `repo_path`              - Path to the repository root
/// * `workspace_id`           - Optional workspace ID that owns the commit
/// * `commit_change_id`       - The short change-id of the commit to diff
///
/// Conflict marker style is resolved from app settings, defaulting to "git".
///
/// # Returns
/// The parsed revision diff on success, or an error string.
pub fn get_commit_diff(
  db: &std::sync::Mutex<crate::db::Database>,
  repo_path: &str,
  workspace_id: Option<i64>,
  commit_change_id: &str,
) -> Result<jj::JjRevisionDiff, String> {
  let conflict_marker_style = crate::core::resolve_conflict_marker_style(db);

  get_commit_diff_with_conflict_style(
    repo_path,
    workspace_id,
    commit_change_id,
    &conflict_marker_style,
  )
}

pub fn get_commit_diff_with_conflict_style(
  repo_path: &str,
  workspace_id: Option<i64>,
  commit_change_id: &str,
  conflict_marker_style: &str,
) -> Result<jj::JjRevisionDiff, String> {
  let workspace_dir = match workspace_id {
    Some(workspace_id) => {
      let workspace = local_db::get_workspace_by_id(repo_path, workspace_id)
        .map_err(|e| format!("Failed to get workspace: {}", e))?
        .ok_or_else(|| format!("Workspace not found: {}", workspace_id))?;
      Path::new(repo_path)
        .join(".treq")
        .join("workspaces")
        .join(&workspace.workspace_path)
    }
    None => Path::new(repo_path).to_path_buf(),
  };
  let workspace_dir_str = workspace_dir
    .to_str()
    .ok_or("Failed to convert workspace path to string")?;

  jj::jj_get_commit_diff(workspace_dir_str, commit_change_id, conflict_marker_style)
    .map_err(|e| format!("Failed to get commit diff: {}", e))
}

pub fn get_commit_file_diff(
  db: &std::sync::Mutex<crate::db::Database>,
  repo_path: &str,
  workspace_id: Option<i64>,
  commit_change_id: &str,
  file_path: &str,
) -> Result<jj::JjFileDiff, String> {
  let conflict_marker_style = crate::core::resolve_conflict_marker_style(db);

  get_commit_file_diff_with_conflict_style(
    repo_path,
    workspace_id,
    commit_change_id,
    file_path,
    &conflict_marker_style,
  )
}

pub fn get_commit_file_diff_with_conflict_style(
  repo_path: &str,
  workspace_id: Option<i64>,
  commit_change_id: &str,
  file_path: &str,
  conflict_marker_style: &str,
) -> Result<jj::JjFileDiff, String> {
  let workspace_dir = match workspace_id {
    Some(workspace_id) => {
      let workspace = local_db::get_workspace_by_id(repo_path, workspace_id)
        .map_err(|e| format!("Failed to get workspace: {}", e))?
        .ok_or_else(|| format!("Workspace not found: {}", workspace_id))?;
      Path::new(repo_path)
        .join(".treq")
        .join("workspaces")
        .join(&workspace.workspace_path)
    }
    None => Path::new(repo_path).to_path_buf(),
  };
  let workspace_dir_str = workspace_dir
    .to_str()
    .ok_or("Failed to convert workspace path to string")?;

  jj::jj_get_commit_file_diff(
    workspace_dir_str,
    commit_change_id,
    file_path,
    conflict_marker_style,
  )
  .map_err(|e| format!("Failed to get commit file diff: {}", e))
}
