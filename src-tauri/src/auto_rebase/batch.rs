use super::{
  convert_to_jj_branch_format, get_full_workspace_path, passes_guard,
  resolve_bookmark_conflict_if_needed, AutoRebaseResult, WorkspaceBookmarkConflict,
  OTHER_WORKING_COPIES,
};
use crate::jj::{self, JjRebaseResult};
use crate::local_db::{self, Workspace};
use std::collections::HashMap;

/// Check and rebase all workspaces in the repo, grouped by target branch
pub fn check_and_rebase_all(
  repo_path: &str,
  conflict_marker_style: &str,
) -> Result<Vec<AutoRebaseResult>, String> {
  // Get all workspaces
  let all_workspaces = local_db::get_workspaces(repo_path)?;

  // Group workspaces by their target_branch
  let mut grouped: HashMap<String, Vec<Workspace>> = HashMap::new();
  for workspace in all_workspaces {
    if let Some(target) = &workspace.target_branch {
      grouped.entry(target.clone()).or_default().push(workspace);
    }
  }

  // Rebase each group
  let mut results = Vec::new();
  let mut errors = Vec::new();

  for (target_branch, workspaces) in grouped {
    // Filter out workspaces where branch_name == target_branch (self-rebase)
    let workspaces: Vec<Workspace> = workspaces
      .into_iter()
      .filter(|w| w.branch_name != target_branch)
      .collect();

    if workspaces.is_empty() {
      continue;
    }

    let sync = crate::core::rebase_guard::sync_target(repo_path, &target_branch);

    // Convert target branch to jj format (origin/main -> main@origin)
    let jj_target_branch = convert_to_jj_branch_format(&target_branch, repo_path);

    // Get current target commit
    let current_target_commit = match jj::jj_get_commit_id(repo_path, &jj_target_branch) {
      Ok(commit) => commit,
      Err(e) => {
        errors.push(format!(
          "Failed to get commit ID for target '{}': {}",
          target_branch, e
        ));
        continue;
      }
    };

    // Filter workspaces that need rebasing
    let workspaces_needing_rebase: Vec<&Workspace> = workspaces
      .iter()
      .filter(|w| local_db::last_rebased_commit_of(w).as_ref() != Some(&current_target_commit))
      .collect();

    if workspaces_needing_rebase.is_empty() {
      continue; // All workspaces already up-to-date
    }

    // Rebase each workspace individually from its workspace directory
    let mut workspace_branches = Vec::new();
    let mut all_success = true;
    let mut combined_messages = Vec::new();
    let mut bookmark_conflicts = Vec::new();

    for workspace in &workspaces_needing_rebase {
      let full_path = get_full_workspace_path(workspace);
      if !passes_guard(repo_path, workspace, &sync) {
        combined_messages.push(format!(
          "Workspace '{}': skipped, see its Code tab",
          workspace.workspace_name
        ));
        continue;
      }

      // Resolve bookmark conflicts before building revsets like roots(target..branch_name).
      if let Err(e) = resolve_bookmark_conflict_if_needed(
        &full_path,
        &workspace.branch_name,
        conflict_marker_style,
      ) {
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

      // Rebase using roots() from workspace dir and only committed bookmark history.
      let rebase_base = jj::jj_resolve_workspace_rebase_base(&full_path, &jj_target_branch)
        .unwrap_or_else(|_| jj_target_branch.clone());
      let revset = format!(
        "roots(({}..{}) ~ {})",
        rebase_base, workspace.branch_name, OTHER_WORKING_COPIES
      );

      match jj::jj_rebase_with_revset(
        &full_path,
        &revset,
        &jj_target_branch,
        &workspace.branch_name, // Set bookmark after rebase
        conflict_marker_style,
      ) {
        Ok(result) => {
          workspace_branches.push(workspace.branch_name.clone());
          all_success = all_success && result.success;
          combined_messages.push(format!(
            "Workspace '{}': {}",
            workspace.workspace_name, result.message
          ));

          // Auto-sync working copy when safe (empty working copy) from workspace dir.
          match jj::jj_sync_working_copy_if_safe(&full_path, &workspace.branch_name) {
            Ok(true) => {
              log::info!(
                "Auto-synced working copy for workspace '{}' [check_and_rebase_all::after_rebase]",
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
            }
          }

          if let Err(e) = local_db::update_workspace_last_rebased_commit(
            repo_path,
            workspace.id,
            &current_target_commit,
          ) {
            tracing::warn!(
              "Warning: Failed to update last rebased commit for workspace '{}': {}",
              workspace.workspace_name,
              e
            );
          }
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

    if !workspace_branches.is_empty() {
      results.push(AutoRebaseResult {
        target_branch: target_branch.clone(),
        workspaces_rebased: workspace_branches,
        rebase_result: JjRebaseResult {
          success: all_success,
          message: combined_messages.join("\n"),
        },
        bookmark_conflicts,
      });
    }
  }

  // Log errors but don't fail the entire operation
  for error in &errors {
    tracing::warn!("Auto-rebase warning: {}", error);
  }

  Ok(results)
}
