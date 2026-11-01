//! Guards the workspace stack against cycles: a workspace's target (stack
//! parent) chain must never lead back to the workspace itself.

use crate::local_db::Workspace;

/// Whether pointing `branch` at `new_target` would form a stack cycle: the
/// target chain starting at `new_target` reaches `branch`. Self-targets count.
pub fn would_create_stack_cycle(workspaces: &[Workspace], branch: &str, new_target: &str) -> bool {
  let _ = (workspaces, branch, new_target);
  false
}

/// [`would_create_stack_cycle`] as an error, for the write paths.
pub fn ensure_no_stack_cycle(
  workspaces: &[Workspace],
  branch: &str,
  new_target: &str,
) -> Result<(), String> {
  if would_create_stack_cycle(workspaces, branch, new_target) {
    return Err(format!(
      "Retargeting {branch} onto {new_target} would create a stack cycle"
    ));
  }
  Ok(())
}

#[cfg(test)]
pub(crate) fn test_workspace(branch: &str, target: Option<&str>) -> Workspace {
  Workspace {
    id: 0,
    repo_path: String::new(),
    workspace_name: branch.to_string(),
    workspace_path: branch.to_string(),
    branch_name: branch.to_string(),
    created_at: String::new(),
    refreshed_at: None,
    metadata: None,
    target_branch: target.map(str::to_string),
    not_on_remote: false,
    title: branch.to_string(),
    moved_files: None,
    description: None,
    sparse_patterns: None,
    archived: false,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn ws(branch: &str, target: Option<&str>) -> Workspace {
    test_workspace(branch, target)
  }

  #[test]
  fn self_target_is_a_cycle() {
    assert!(would_create_stack_cycle(&[ws("a", Some("main"))], "a", "a"));
  }

  #[test]
  fn targeting_a_direct_child_is_a_cycle() {
    let w = [ws("a", Some("main")), ws("b", Some("a"))];
    assert!(would_create_stack_cycle(&w, "a", "b"));
  }

  #[test]
  fn targeting_a_deeper_descendant_is_a_cycle() {
    let w = [
      ws("a", Some("main")),
      ws("b", Some("a")),
      ws("c", Some("b")),
    ];
    assert!(would_create_stack_cycle(&w, "a", "c"));
  }

  #[test]
  fn targeting_a_sibling_stack_or_trunk_is_not_a_cycle() {
    let w = [
      ws("a", Some("main")),
      ws("b", Some("main")),
      ws("c", Some("b")),
    ];
    assert!(!would_create_stack_cycle(&w, "a", "c"));
    assert!(!would_create_stack_cycle(&w, "a", "main"));
    assert!(ensure_no_stack_cycle(&w, "a", "c").is_ok());
  }

  #[test]
  fn a_new_branch_that_an_existing_workspace_targets_is_checked() {
    let w = [ws("child", Some("new"))];
    assert!(would_create_stack_cycle(&w, "new", "child"));
  }

  #[test]
  fn ensure_reports_the_cycle() {
    let w = [ws("a", Some("main")), ws("b", Some("a"))];
    assert_eq!(
      ensure_no_stack_cycle(&w, "a", "b").unwrap_err(),
      "Retargeting a onto b would create a stack cycle"
    );
  }
}
