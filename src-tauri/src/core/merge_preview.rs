//! What the merge page shows before a workspace is merged: its commits ahead
//! of the target branch, its diff, and what lands where.

use crate::core::MergeLandingPlan;
use crate::github::{GhRepoMergeSettings, PrInfo};
use crate::jj::{JjCommitsAhead, JjRevisionDiff};
use crate::local_db::{self, Workspace};

/// The branch the workspace merges into: its target, else `main`.
pub fn target_branch(workspace: &Workspace) -> &str {
  workspace
    .target_branch
    .as_deref()
    .filter(|b| !b.is_empty())
    .unwrap_or("main")
}

/// What the page reads from the backend.
#[derive(Debug, Clone, Default)]
pub struct Preview {
  pub workspace: Option<Workspace>,
  pub commits: Option<JjCommitsAhead>,
  pub diff: Option<JjRevisionDiff>,
  /// Why the commits or diff could not be read.
  pub error: Option<String>,
  /// What lands where, and which stacked workspaces move.
  pub plan: Option<MergeLandingPlan>,
  /// The stack's root branch, when the workspace is stacked on another.
  pub rollup_root: Option<String>,
  /// The workspace's PR, open or merged; the app fills it from its PR cache.
  pub github: Option<GithubPr>,
  /// The repository has a GitHub remote to push the target to.
  pub can_push: bool,
}

/// The workspace's pull request on GitHub, open or already merged.
#[derive(Debug, Clone, PartialEq)]
pub struct GithubPr {
  pub repo_full_name: String,
  pub pr: PrInfo,
  /// The repository's allowed merge methods; `None` when `gh` could not say.
  pub settings: Option<GhRepoMergeSettings>,
}

/// Reads the workspace's preview; with `rollup`, the commits and diff the
/// whole stack lands in its root. Like React, it shows neither commits nor
/// diff when one of them fails to read.
pub fn merge_preview(repo_path: &str, workspace_id: i64, rollup: bool) -> Preview {
  let workspace = local_db::get_workspace_by_id(repo_path, workspace_id)
    .ok()
    .flatten();
  let Some(ws) = &workspace else {
    return Preview {
      error: Some(format!("Workspace {workspace_id} not found")),
      ..Preview::default()
    };
  };
  let plan = super::get_merge_landing_plan(repo_path, workspace_id, rollup).ok();
  let rollup_root = plan
    .as_ref()
    .filter(|p| p.can_rollup)
    .and_then(|_| super::get_merge_landing_plan(repo_path, workspace_id, true).ok())
    .map(|p| p.target_branch);
  let target = plan.as_ref().map_or_else(
    || target_branch(ws).to_string(),
    |p| p.target_branch.clone(),
  );
  let commits =
    crate::jj::jj_get_commits_ahead(&super::context_menu::full_workspace_path(ws), &target)
      .map_err(|e| e.to_string());
  let diff = if rollup {
    super::merge_preview_diff(repo_path, workspace_id, true)
  } else {
    super::workspace_diff(repo_path, workspace_id)
  };
  let extras = Preview {
    plan,
    rollup_root,
    can_push: crate::github::get_git_remote_url_impl(repo_path)
      .ok()
      .flatten()
      .is_some(),
    ..Preview::default()
  };
  match (commits, diff) {
    (Ok(commits), Ok(diff)) => Preview {
      workspace,
      commits: Some(commits),
      diff: Some(diff),
      ..extras
    },
    (Err(err), _) | (_, Err(err)) => Preview {
      workspace,
      error: Some(err),
      ..extras
    },
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use tempfile::TempDir;

  #[test]
  fn reports_a_missing_workspace() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_str().unwrap();

    let preview = merge_preview(repo, 7, false);

    assert_eq!(preview.error.as_deref(), Some("Workspace 7 not found"));
    assert!(preview.workspace.is_none() && preview.commits.is_none() && preview.diff.is_none());
  }

  #[test]
  fn merges_into_main_when_the_workspace_has_no_target() {
    let workspace = |target: Option<&str>| Workspace {
      id: 1,
      repo_path: "/repo".into(),
      workspace_name: "a".into(),
      workspace_path: "a".into(),
      branch_name: "a".into(),
      created_at: String::new(),
      refreshed_at: None,
      metadata: None,
      target_branch: target.map(str::to_string),
      title: String::new(),
      description: None,
      moved_files: None,
      not_on_remote: false,
      sparse_patterns: None,
      archived: false,
    };

    assert_eq!(target_branch(&workspace(None)), "main");
    assert_eq!(target_branch(&workspace(Some(""))), "main");
    assert_eq!(target_branch(&workspace(Some("develop"))), "develop");
  }
}
