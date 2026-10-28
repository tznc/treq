use super::{
  convert_to_jj_branch_format, get_full_workspace_path, passes_guard,
  resolve_bookmark_conflict_if_needed, AutoRebaseResult, WorkspaceBookmarkConflict,
};
use crate::jj::{self, JjRebaseResult};
use crate::local_db::{self, Workspace};
use crate::result_ext::ResultExt;
use std::collections::{HashMap, HashSet};

fn format_workspace_rebase_message(workspace_name: &str, message: &str) -> Option<String> {
  let relevant_details = message
    .lines()
    .filter(|line| !line.starts_with("Skipped rebase of ") || !line.ends_with(" already in place"))
    .collect::<Vec<_>>()
    .join("\n");

  (!relevant_details.is_empty())
    .then(|| format!("Workspace '{}': {}", workspace_name, relevant_details))
}

fn resolve_rooted_subtree_ordered(
  workspaces: &[Workspace],
  start_workspace_id: i64,
) -> Result<(Workspace, Vec<Workspace>), String> {
  let by_id: HashMap<i64, &Workspace> = workspaces.iter().map(|w| (w.id, w)).collect();
  let by_branch: HashMap<String, &Workspace> = workspaces
    .iter()
    .map(|w| (w.branch_name.clone(), w))
    .collect();
  let start = by_id
    .get(&start_workspace_id)
    .ok_or_else(|| format!("Workspace {} not found", start_workspace_id))?;

  let mut root = *start;
  let mut seen = HashSet::new();
  seen.insert(root.branch_name.clone());
  while let Some(target_branch) = &root.target_branch {
    if target_branch == &root.branch_name {
      break;
    }
    let Some(parent) = by_branch.get(target_branch) else {
      break;
    };
    if !seen.insert(parent.branch_name.clone()) {
      break;
    }
    root = parent;
  }

  let mut children_by_branch: HashMap<String, Vec<&Workspace>> = HashMap::new();
  for ws in workspaces {
    if let Some(target) = &ws.target_branch {
      children_by_branch
        .entry(target.clone())
        .or_default()
        .push(ws);
    }
  }
  for children in children_by_branch.values_mut() {
    children.sort_by(|a, b| a.branch_name.cmp(&b.branch_name));
  }

  let mut ordered = Vec::new();
  let mut stack = vec![root];
  let mut visited = HashSet::new();
  while let Some(node) = stack.pop() {
    if !visited.insert(node.branch_name.clone()) {
      continue;
    }
    ordered.push(node.clone());
    if let Some(children) = children_by_branch.get(&node.branch_name) {
      for child in children.iter().rev() {
        stack.push(child);
      }
    }
  }

  Ok((root.clone(), ordered))
}

pub fn rebase_root_subtree_from_workspace_force(
  repo_path: &str,
  workspace_id: i64,
  default_branch: &str,
  conflict_marker_style: &str,
) -> Result<Option<AutoRebaseResult>, String> {
  let workspaces = local_db::get_workspaces(repo_path)?;
  let (root, scoped) = resolve_rooted_subtree_ordered(&workspaces, workspace_id)?;
  if scoped.is_empty() {
    return Ok(None);
  }

  let mut workspace_branches = Vec::new();
  let mut all_success = true;
  let mut combined_messages = Vec::new();
  let mut bookmark_conflicts = Vec::new();
  let mut summary_target_branch = default_branch.to_string();

  for workspace in &scoped {
    if workspace.id == root.id {
      combined_messages.push(format!(
        "Workspace '{}': Skipped root workspace",
        workspace.workspace_name
      ));
      continue;
    }

    let target_branch = workspace
      .target_branch
      .clone()
      .unwrap_or_else(|| default_branch.to_string());
    summary_target_branch = target_branch.clone();
    if workspace.branch_name == target_branch {
      combined_messages.push(format!(
        "Workspace '{}': Skipped self-target rebase",
        workspace.workspace_name
      ));
      continue;
    }

    let full_path = get_full_workspace_path(workspace);
    let sync = crate::core::rebase_guard::sync_target(repo_path, &target_branch);
    if !passes_guard(repo_path, workspace, &sync) {
      all_success = false;
      if let Some(hold) = crate::core::rebase_guard::hold(repo_path, workspace.id) {
        combined_messages.push(format!(
          "Workspace '{}': {}",
          workspace.workspace_name,
          hold.message()
        ));
      }
      continue;
    }
    if let Err(e) =
      resolve_bookmark_conflict_if_needed(&full_path, &workspace.branch_name, conflict_marker_style)
    {
      all_success = false;
      combined_messages.push(format!(
        "Workspace '{}': Failed to resolve conflict - {}",
        workspace.workspace_name, e
      ));
      continue;
    }

    match jj::jj_rebase_workspace_bookmark_onto_deferred_checkout(
      &full_path,
      &workspace.branch_name,
      &target_branch,
    ) {
      Ok(result) => {
        workspace_branches.push(workspace.branch_name.clone());
        all_success = all_success && result.success;
        if let Some(message) =
          format_workspace_rebase_message(&workspace.workspace_name, &result.message)
        {
          combined_messages.push(message);
        }

        if let Ok(current_target_commit) = jj::jj_get_commit_id(
          repo_path,
          &convert_to_jj_branch_format(&target_branch, repo_path),
        ) {
          local_db::update_workspace_last_rebased_commit(
            repo_path,
            workspace.id,
            &current_target_commit,
          )
          .warn_on_err("update workspace last rebased commit");
        }

        match jj::jj_sync_working_copy_if_safe(&full_path, &workspace.branch_name) {
          Ok(_) => {}
          Err(jj::JjError::BookmarkConflict(info)) => {
            bookmark_conflicts.push(WorkspaceBookmarkConflict {
              workspace_id: workspace.id,
              workspace_name: workspace.workspace_name.clone(),
              workspace_path: full_path.clone(),
              branch_name: workspace.branch_name.clone(),
              bookmark: info.bookmark.clone(),
              commits: info.commits.clone(),
            });
          }
          Err(_) => {}
        }
      }
      Err(e) => {
        all_success = false;
        combined_messages.push(format!(
          "Workspace '{}': Failed - {}",
          workspace.workspace_name, e
        ));
      }
    }
  }

  Ok(Some(AutoRebaseResult {
    target_branch: summary_target_branch,
    workspaces_rebased: workspace_branches,
    rebase_result: JjRebaseResult {
      success: all_success,
      message: combined_messages.join("\n"),
    },
    bookmark_conflicts,
  }))
}

#[cfg(test)]
mod tests {
  use super::{format_workspace_rebase_message, resolve_rooted_subtree_ordered};
  use crate::local_db::Workspace;

  fn ws(id: i64, branch: &str, target: Option<&str>) -> Workspace {
    Workspace {
      id,
      repo_path: "/tmp/repo".to_string(),
      workspace_name: branch.to_string(),
      workspace_path: format!("ws-{}", id),
      branch_name: branch.to_string(),
      created_at: "".to_string(),
      refreshed_at: None,
      metadata: None,
      target_branch: target.map(ToString::to_string),
      title: branch.to_string(),
      description: None,
      moved_files: None,
      not_on_remote: false,
      sparse_patterns: None,
      archived: false,
    }
  }

  #[test]
  fn rooted_subtree_scope_includes_siblings_and_cousins() {
    let workspaces = vec![
      ws(1, "A", Some("main")),
      ws(2, "B", Some("A")),
      ws(3, "C", Some("B")),
      ws(4, "D", Some("A")),
    ];

    let (_, from_b) = resolve_rooted_subtree_ordered(&workspaces, 2).unwrap();
    let (_, from_c) = resolve_rooted_subtree_ordered(&workspaces, 3).unwrap();
    let branches_from_b: Vec<String> = from_b.into_iter().map(|w| w.branch_name).collect();
    let branches_from_c: Vec<String> = from_c.into_iter().map(|w| w.branch_name).collect();
    assert_eq!(branches_from_b, vec!["A", "B", "C", "D"]);
    assert_eq!(branches_from_c, vec!["A", "B", "C", "D"]);
  }

  #[test]
  fn omits_noop_rebase_and_deferred_refresh_details() {
    let message = format_workspace_rebase_message(
      "fix-agent-terminal-escaping",
      "Skipped rebase of 1 commits already in place",
    );

    assert_eq!(message, None);
  }

  #[test]
  fn preserves_actual_rebase_details_without_deferred_refresh_note() {
    let message = format_workspace_rebase_message(
      "fix-agent-terminal-escaping",
      "Rebased 1 commits to destination\nSkipped rebase of 2 commits already in place",
    );

    assert_eq!(
      message,
      Some("Workspace 'fix-agent-terminal-escaping': Rebased 1 commits to destination".to_string())
    );
  }
}
