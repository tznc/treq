//! Opens or creates the workspace for a GitHub issue, the same way Linear
//! issues get one: a branch named after the issue, targeting
//! the repo's default branch, with the issue recorded in workspace metadata.

use crate::local_db;

/// `github-<number>-<title-slug>`, built with the shared tracker branch naming.
pub fn github_issue_branch_name(number: u64, title: &str) -> String {
  crate::tracker::branch_name(&format!("github-{number}"), title)
}

/// Returns the issue's workspace and whether it was just created.
pub fn open_or_create_workspace_from_github_issue(
  repo_path: &str,
  number: u64,
  title: &str,
  url: &str,
) -> Result<(local_db::Workspace, bool), String> {
  let base_branch = crate::core::get_repo_default_branch(repo_path)
    .map_err(|e| format!("Failed to get repo default branch: {e}"))?;
  let (workspace, created) = crate::core::open_or_create_workspace_from_issue(
    repo_path,
    &github_issue_branch_name(number, title),
    &base_branch,
    title,
    None,
  )
  .map_err(|e| format!("Failed to create workspace for GitHub issue #{number}: {e}"))?;

  if let Err(e) = crate::core::merge_tracker_item_metadata(
    repo_path,
    workspace.id,
    "github",
    &format!("#{number}"),
    url,
    title,
  ) {
    log::warn!(
      "Failed to set GitHub issue metadata for workspace {}: {e}",
      workspace.id
    );
  }
  Ok((workspace, created))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn names_branch_after_issue_number_and_title() {
    assert_eq!(
      github_issue_branch_name(42, "Fix the login redirect!"),
      "github-42-fix-the-login-redirect"
    );
  }

  #[test]
  fn names_branch_after_number_alone_when_title_has_no_ascii() {
    assert_eq!(github_issue_branch_name(7, "  ✨  "), "github-7");
  }

  #[test]
  fn caps_long_titles_on_a_word_boundary() {
    let name = github_issue_branch_name(
      9,
      "Rework the ranking pipeline so it handles multi currency invoices correctly",
    );
    assert!(name.starts_with("github-9-rework-the-ranking-pipeline"));
    assert!(name.len() <= "github-9-".len() + 48);
    assert!(!name.ends_with('-'));
  }
}
