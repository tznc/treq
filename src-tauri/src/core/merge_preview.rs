//! What the merge page shows before a workspace is merged: its commits ahead
//! of the target branch and its diff.

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
}

/// Reads the workspace's preview. Like React, it shows neither commits nor
/// diff when one of them fails to read.
pub fn merge_preview(repo_path: &str, workspace_id: i64) -> Preview {
  let workspace = local_db::get_workspace_by_id(repo_path, workspace_id)
    .ok()
    .flatten();
  let Some(ws) = &workspace else {
    return Preview {
      error: Some(format!("Workspace {workspace_id} not found")),
      ..Preview::default()
    };
  };
  let commits = crate::jj::jj_get_commits_ahead(
    &super::context_menu::full_workspace_path(ws),
    target_branch(ws),
  )
  .map_err(|e| e.to_string());
  let diff = super::workspace_diff(repo_path, workspace_id);
  match (commits, diff) {
    (Ok(commits), Ok(diff)) => Preview {
      workspace,
      commits: Some(commits),
      diff: Some(diff),
      error: None,
    },
    (Err(err), _) | (_, Err(err)) => Preview {
      workspace,
      error: Some(err),
      ..Preview::default()
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

    let preview = merge_preview(repo, 7);

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
