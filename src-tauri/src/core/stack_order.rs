//! Reorders a stack of workspaces: moves one up or down a level, or takes
//! it out, by retargeting each workspace onto the one below it.

use std::collections::{HashMap, HashSet};

use super::{pr_stacks::StackMove, workspaces};
use crate::jj;
use crate::local_db::{self, Workspace};

/// How a workspace moves in its stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reorder {
  /// Swaps it with the workspace above.
  Up,
  /// Swaps it with the workspace below.
  Down,
  /// Takes it out: the workspace above moves onto the one below, and it
  /// targets the default branch.
  Out,
}

/// The retargets that move workspace `id` in `chain`: bottom first, each
/// targeting the one below, the bottom one on its base. Bottom-up, so each
/// step targets a workspace already in its new place and none makes a
/// cycle. Empty when it can't move that way.
pub fn plan_reorder(
  chain: &[Workspace],
  id: i64,
  how: Reorder,
  default_branch: &str,
) -> Vec<StackMove> {
  let Some(i) = chain.iter().position(|w| w.id == id) else {
    return vec![];
  };
  let base = target_of(&chain[0], default_branch);
  let mut order: Vec<&Workspace> = chain.iter().collect();
  let mut out = None;
  match how {
    Reorder::Up if i + 1 < order.len() => order.swap(i, i + 1),
    Reorder::Down if i > 0 => order.swap(i, i - 1),
    Reorder::Out => out = Some(order.remove(i)),
    _ => return vec![],
  }
  let mut moves = Vec::new();
  let mut below = base;
  for w in order {
    if target_of(w, default_branch) != below {
      moves.push(StackMove {
        workspace_id: w.id,
        new_target: below.clone(),
      });
    }
    below = w.branch_name.clone();
  }
  if let Some(w) = out.filter(|w| target_of(w, default_branch) != default_branch) {
    moves.push(StackMove {
      workspace_id: w.id,
      new_target: default_branch.to_string(),
    });
  }
  moves
}

fn target_of(w: &Workspace, default_branch: &str) -> String {
  w.target_branch
    .clone()
    .filter(|t| !t.is_empty())
    .unwrap_or_else(|| default_branch.to_string())
}

/// The stack through workspace `id`, bottom first: an error when a
/// workspace in it has more than one workspace stacked on it.
fn linear_stack(workspaces: &[Workspace], id: i64) -> Result<Vec<Workspace>, String> {
  let live: Vec<&Workspace> = workspaces.iter().filter(|w| !w.archived).collect();
  let by_branch: HashMap<&str, &Workspace> =
    live.iter().map(|w| (w.branch_name.as_str(), *w)).collect();
  let mut w = *live
    .iter()
    .find(|w| w.id == id)
    .ok_or("Workspace not found")?;
  let mut seen = HashSet::from([w.branch_name.as_str()]);
  while let Some(parent) = w.target_branch.as_deref().and_then(|t| by_branch.get(t)) {
    if !seen.insert(parent.branch_name.as_str()) {
      break;
    }
    w = parent;
  }
  let mut chain = vec![w.clone()];
  let mut seen = HashSet::from([w.branch_name.as_str()]);
  loop {
    let above: Vec<&Workspace> = live
      .iter()
      .copied()
      .filter(|c| c.target_branch.as_deref() == Some(w.branch_name.as_str()))
      .collect();
    match above.as_slice() {
      [] => break,
      [next] if seen.insert(next.branch_name.as_str()) => {
        chain.push((*next).clone());
        w = next;
      }
      [_] => break,
      _ => return Err("Only a stack without branches can be reordered".into()),
    }
  }
  Ok(chain)
}

/// Moves workspace `id` in its stack in `repo_path`, retargeting (and so
/// rebasing) the workspaces [`plan_reorder`] names; the moves it made.
pub fn reorder_workspace(repo_path: &str, id: i64, how: Reorder) -> Result<Vec<StackMove>, String> {
  let workspaces = local_db::get_workspaces(repo_path)?;
  let default_branch = jj::get_default_branch(repo_path).map_err(|e| e.to_string())?;
  let chain = linear_stack(&workspaces, id)?;
  let moves = plan_reorder(&chain, id, how, &default_branch);
  let mut done: Vec<(i64, String)> = Vec::new();
  for step in &moves {
    let retarget = |id, target: &str| {
      workspaces::retarget_workspace(repo_path, id, target, &default_branch).map(drop)
    };
    if let Err(err) = retarget(step.workspace_id, &step.new_target) {
      // Put back what moved, so a failed reorder leaves the stack as it was.
      for (id, target) in done.iter().rev() {
        retarget(*id, target)
          .unwrap_or_else(|e| log::warn!("stack order: put back workspace {id}: {e}"));
      }
      return Err(err);
    }
    let was = chain
      .iter()
      .find(|w| w.id == step.workspace_id)
      .map(|w| target_of(w, &default_branch))
      .unwrap_or_else(|| default_branch.clone());
    done.push((step.workspace_id, was));
  }
  Ok(moves)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn ws(id: i64, branch: &str, target: &str) -> Workspace {
    Workspace {
      id,
      repo_path: "/tmp/repo".to_string(),
      workspace_name: branch.to_string(),
      workspace_path: format!("ws/{branch}"),
      branch_name: branch.to_string(),
      created_at: "2026-01-01T00:00:00.000Z".to_string(),
      refreshed_at: None,
      metadata: None,
      target_branch: Some(target.to_string()),
      title: branch.to_string(),
      description: None,
      moved_files: None,
      not_on_remote: false,
      sparse_patterns: None,
      archived: false,
    }
  }

  /// `a` on `main`, `b` on `a`, `c` on `b`.
  fn chain() -> Vec<Workspace> {
    vec![ws(1, "a", "main"), ws(2, "b", "a"), ws(3, "c", "b")]
  }

  fn mv(workspace_id: i64, new_target: &str) -> StackMove {
    StackMove {
      workspace_id,
      new_target: new_target.into(),
    }
  }

  #[test]
  fn moves_a_workspace_up_a_level() {
    assert_eq!(
      plan_reorder(&chain(), 2, Reorder::Up, "main"),
      [mv(3, "a"), mv(2, "c")]
    );
  }

  #[test]
  fn moves_a_workspace_down_a_level() {
    assert_eq!(
      plan_reorder(&chain(), 2, Reorder::Down, "main"),
      [mv(2, "main"), mv(1, "b"), mv(3, "a")]
    );
  }

  #[test]
  fn takes_a_workspace_out_of_the_middle() {
    assert_eq!(
      plan_reorder(&chain(), 2, Reorder::Out, "main"),
      [mv(3, "a"), mv(2, "main")]
    );
  }

  #[test]
  fn taking_out_the_bottom_moves_the_next_onto_the_base() {
    let chain = vec![ws(1, "a", "release"), ws(2, "b", "a")];
    assert_eq!(
      plan_reorder(&chain, 1, Reorder::Out, "main"),
      [mv(2, "release"), mv(1, "main")]
    );
  }

  #[test]
  fn the_ends_move_no_further() {
    assert!(plan_reorder(&chain(), 3, Reorder::Up, "main").is_empty());
    assert!(plan_reorder(&chain(), 1, Reorder::Down, "main").is_empty());
    assert!(plan_reorder(&chain(), 9, Reorder::Out, "main").is_empty());
  }
}
