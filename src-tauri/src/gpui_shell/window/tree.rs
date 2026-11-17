//! The workspace stack tree the sidebar draws, as `lib/workspace-tree.ts`
//! builds it: workspaces under the workspace whose branch they target,
//! siblings newest first, merged ones last.

use std::collections::{HashMap, HashSet};

use crate::{core::WorkspaceSidebarStatus, local_db::Workspace};

/// One workspace in display order (`FlattenedWorkspaceNode`), with the git
/// detail the row's status dot reads.
#[derive(Debug, Clone)]
pub struct Node {
  pub workspace: Workspace,
  pub depth: u32,
  pub conflicted: bool,
  /// `has_changes` from `useWorkspaceGitDetail`; false until read.
  pub has_changes: bool,
}

/// A workspace's sidebar status plus its git detail.
#[derive(Debug, Clone)]
pub struct Row {
  pub status: WorkspaceSidebarStatus,
  pub has_changes: bool,
}

/// `useWorkspaceSidebarRows`'s `statuses`: every workspace, with its status
/// when one was read (else unconflicted) and its git detail.
pub fn rows(
  workspaces: &[Workspace],
  statuses: &[WorkspaceSidebarStatus],
  has_changes: &HashMap<i64, bool>,
) -> Vec<Row> {
  // The first status of each id, as a search from the front finds it.
  let mut by_id: HashMap<i64, &WorkspaceSidebarStatus> = HashMap::with_capacity(statuses.len());
  for status in statuses {
    by_id.entry(status.current.id).or_insert(status);
  }
  workspaces
    .iter()
    .map(|workspace| {
      let status = by_id
        .get(&workspace.id)
        .map(|s| (*s).clone())
        .unwrap_or_else(|| WorkspaceSidebarStatus {
          current: workspace.clone(),
          has_conflicts: false,
          last_activity_at: None,
        });
      Row {
        has_changes: has_changes.get(&workspace.id).copied().unwrap_or(false),
        status,
      }
    })
    .collect()
}

struct TreeNode {
  row: Row,
  children: Vec<usize>,
}

/// `flattenWorkspaceTree(buildWorkspaceTree(rows, isMerged))`.
pub fn flatten(rows: &[Row], is_merged: impl Fn(&str) -> bool) -> Vec<Node> {
  let nodes: Vec<TreeNode> = rows
    .iter()
    .map(|row| TreeNode {
      row: row.clone(),
      children: Vec::new(),
    })
    .collect();
  // The last workspace with a branch name wins, as the Map in React does.
  let by_branch: HashMap<String, usize> = nodes
    .iter()
    .enumerate()
    .map(|(i, n)| (n.row.status.current.branch_name.clone(), i))
    .collect();
  let mut children: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
  for (i, node) in nodes.iter().enumerate() {
    let workspace = &node.row.status.current;
    let Some(target) = workspace.target_branch.as_deref() else {
      continue;
    };
    if target == workspace.branch_name {
      continue;
    }
    if let (Some(&parent), Some(&child)) = (
      by_branch.get(target),
      by_branch.get(workspace.branch_name.as_str()),
    ) {
      if child == i {
        children[parent].push(child);
      }
    }
  }
  let mut nodes: Vec<TreeNode> = nodes
    .into_iter()
    .zip(children)
    .map(|(mut node, children)| {
      node.children = children;
      node
    })
    .collect();
  let mut roots: Vec<usize> = (0..nodes.len())
    .filter(|&i| by_branch.get(nodes[i].row.status.current.branch_name.as_str()) == Some(&i))
    .filter(|&i| {
      let workspace = &nodes[i].row.status.current;
      match workspace.target_branch.as_deref() {
        None => true,
        Some(target) => target == workspace.branch_name || !by_branch.contains_key(target),
      }
    })
    .collect();
  // Cycles no root reaches are flattened to the top level.
  let mut reached = HashSet::new();
  let mut pending = roots.clone();
  while let Some(i) = pending.pop() {
    if reached.insert(i) {
      pending.extend(nodes[i].children.iter().copied());
    }
  }
  for (i, node) in nodes.iter_mut().enumerate() {
    let canonical = by_branch.get(node.row.status.current.branch_name.as_str()) == Some(&i);
    if canonical && !reached.contains(&i) {
      node.children.clear();
      roots.push(i);
    }
  }
  let order = |list: &mut Vec<usize>, nodes: &[TreeNode]| {
    list.sort_by(|&a, &b| {
      let (a, b) = (&nodes[a].row.status, &nodes[b].row.status);
      let (merged_a, merged_b) = (
        is_merged(&a.current.branch_name),
        is_merged(&b.current.branch_name),
      );
      merged_a
        .cmp(&merged_b)
        .then_with(|| activity(b).cmp(activity(a)))
        .then_with(|| a.current.branch_name.cmp(&b.current.branch_name))
    });
  };
  order(&mut roots, &nodes);
  for i in 0..nodes.len() {
    let mut list = std::mem::take(&mut nodes[i].children);
    order(&mut list, &nodes);
    nodes[i].children = list;
  }
  let mut out = Vec::with_capacity(nodes.len());
  let mut seen = HashSet::new();
  fn walk(
    i: usize,
    depth: u32,
    nodes: &[TreeNode],
    out: &mut Vec<Node>,
    seen: &mut HashSet<usize>,
  ) {
    if !seen.insert(i) {
      return;
    }
    let row = &nodes[i].row;
    out.push(Node {
      workspace: row.status.current.clone(),
      depth,
      conflicted: row.status.has_conflicts,
      has_changes: row.has_changes,
    });
    for &child in &nodes[i].children {
      walk(child, depth + 1, nodes, out, seen);
    }
  }
  for &root in &roots {
    walk(root, 0, &nodes, &mut out, &mut seen);
  }
  out
}

/// `activityTimestamp`: the working-copy tip's time, else creation.
fn activity(status: &WorkspaceSidebarStatus) -> &str {
  status
    .last_activity_at
    .as_deref()
    .filter(|t| !t.is_empty())
    .unwrap_or(&status.current.created_at)
}

/// `getDescendants`: every workspace stacked (transitively) on `branch`.
pub fn descendants<'a>(workspaces: &'a [Workspace], branch: &str) -> Vec<&'a Workspace> {
  let mut children: HashMap<&str, Vec<&Workspace>> = HashMap::new();
  for workspace in workspaces {
    if let Some(target) = workspace.target_branch.as_deref() {
      children.entry(target).or_default().push(workspace);
    }
  }
  let mut out = Vec::new();
  let mut stack = vec![branch.to_string()];
  let mut visited: HashSet<String> = HashSet::from([branch.to_string()]);
  while let Some(current) = stack.pop() {
    for child in children.get(current.as_str()).into_iter().flatten() {
      if visited.insert(child.branch_name.clone()) {
        out.push(*child);
        stack.push(child.branch_name.clone());
      }
    }
  }
  out
}

/// `getEntireStack`: the stack's root and every workspace on it.
pub fn entire_stack<'a>(workspaces: &'a [Workspace], branch: &str) -> Vec<&'a Workspace> {
  let by_branch: HashMap<&str, &Workspace> = workspaces
    .iter()
    .map(|w| (w.branch_name.as_str(), w))
    .collect();
  let mut current = branch.to_string();
  let mut visited = HashSet::new();
  loop {
    visited.insert(current.clone());
    let Some(workspace) = by_branch.get(current.as_str()) else {
      break;
    };
    let Some(target) = workspace.target_branch.as_deref() else {
      break;
    };
    if !by_branch.contains_key(target) || visited.contains(target) {
      break;
    }
    current = target.to_string();
  }
  let Some(root) = workspaces.iter().find(|w| w.branch_name == current) else {
    return Vec::new();
  };
  let mut stack = vec![root];
  stack.extend(descendants(workspaces, &current));
  stack
}

#[cfg(test)]
pub(super) mod tests {
  use super::*;

  pub fn workspace(id: i64, branch: &str, target: Option<&str>) -> Workspace {
    Workspace {
      id,
      repo_path: "/repo".into(),
      workspace_name: branch.into(),
      workspace_path: format!(".treq/workspaces/{branch}"),
      branch_name: branch.into(),
      created_at: format!("2026-10-{:02}T00:00:00Z", id),
      refreshed_at: None,
      metadata: None,
      target_branch: target.map(str::to_string),
      title: String::new(),
      description: None,
      moved_files: None,
      not_on_remote: false,
      sparse_patterns: None,
      archived: false,
    }
  }

  fn order(nodes: &[Node]) -> Vec<(String, u32)> {
    nodes
      .iter()
      .map(|n| (n.workspace.branch_name.clone(), n.depth))
      .collect()
  }

  #[test]
  fn stacks_workspaces_under_their_targets_newest_first() {
    let workspaces = [
      workspace(1, "a", Some("main")),
      workspace(2, "b", Some("a")),
      workspace(3, "c", Some("main")),
      workspace(4, "d", Some("a")),
    ];
    let rows = rows(&workspaces, &[], &HashMap::new());
    let nodes = flatten(&rows, |_| false);
    assert_eq!(
      order(&nodes),
      [
        ("c".into(), 0),
        ("a".into(), 0),
        ("d".into(), 1),
        ("b".into(), 1)
      ]
    );
  }

  #[test]
  fn merged_workspaces_sort_last_and_cycles_stay_visible() {
    let workspaces = [
      workspace(1, "a", Some("b")),
      workspace(2, "b", Some("a")),
      workspace(3, "merged", None),
      workspace(4, "c", None),
    ];
    let rows = rows(&workspaces, &[], &HashMap::new());
    let nodes = flatten(&rows, |b| b == "merged");
    assert_eq!(nodes.last().unwrap().workspace.branch_name, "merged");
    assert_eq!(nodes.len(), 4, "a cycle is flattened, not dropped");
  }

  #[test]
  fn finds_descendants_and_whole_stacks() {
    let workspaces = [
      workspace(1, "a", Some("main")),
      workspace(2, "b", Some("a")),
      workspace(3, "c", Some("b")),
      workspace(4, "x", Some("main")),
    ];
    let ids = |list: Vec<&Workspace>| list.iter().map(|w| w.id).collect::<Vec<_>>();
    assert_eq!(ids(descendants(&workspaces, "b")), [3]);
    assert_eq!(ids(entire_stack(&workspaces, "c")), [1, 2, 3]);
    assert_eq!(ids(entire_stack(&workspaces, "x")), [4]);
  }
}
