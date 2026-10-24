//! Keeps workspaces stacked the way their pull requests are on GitHub.
//!
//! A workspace stacks on another when its `target_branch` is the other's
//! branch. After each PR status poll, two things can move a workspace:
//!
//! - Its open PR was retargeted on GitHub. The workspace follows, unless the
//!   user restacked it in treq since treq last saw the PR's base.
//! - The workspace it stacks on had its PR merged. The workspace moves onto
//!   that workspace's own target, as deleting the base workspace would.

use std::collections::{HashMap, HashSet};

use super::workspaces;
use crate::github::PrInfo;
use crate::jj;
use crate::local_db::{self, Workspace};

/// One workspace to retarget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackMove {
  pub workspace_id: i64,
  pub new_target: String,
}

/// What one sync does: the workspaces to move, and the PR bases to remember
/// so the next sync can tell a GitHub retarget from a treq restack.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SyncPlan {
  pub moves: Vec<StackMove>,
  pub seen_bases: Vec<(i64, String)>,
}

/// Plans a sync from the polled PRs (by head branch) and the PR base each
/// workspace had when treq last looked.
pub fn plan_pr_base_sync(
  workspaces: &[Workspace],
  prs: &HashMap<String, Option<PrInfo>>,
  seen_bases: &HashMap<i64, String>,
  default_branch: &str,
) -> SyncPlan {
  let by_branch: HashMap<&str, &Workspace> = workspaces
    .iter()
    .map(|w| (w.branch_name.as_str(), w))
    .collect();
  let pr_of = |branch: &str| {
    prs
      .get(branch)
      .and_then(Option::as_ref)
      .filter(|pr| pr.head_ref_name == branch)
  };
  let merged_workspace = |branch: &str| {
    by_branch
      .get(branch)
      .copied()
      .filter(|_| pr_of(branch).is_some_and(|pr| pr.state.eq_ignore_ascii_case("merged")))
  };
  let target_of = |w: &Workspace| {
    w.target_branch
      .clone()
      .filter(|t| !t.is_empty() && *t != w.branch_name)
      .unwrap_or_else(|| default_branch.to_string())
  };
  // The first target below `target` whose workspace's PR is not merged.
  let past_merged = |target: &str| {
    let mut current = target.to_string();
    let mut visited = HashSet::new();
    while let Some(base) = merged_workspace(&current) {
      if !visited.insert(current.clone()) {
        return default_branch.to_string();
      }
      current = target_of(base);
    }
    current
  };

  let mut plan = SyncPlan::default();
  let mut ordered: Vec<&Workspace> = workspaces.iter().collect();
  ordered.sort_by_key(|w| w.id);
  for workspace in ordered {
    let target = target_of(workspace);
    let open_base = pr_of(&workspace.branch_name)
      .filter(|pr| pr.state.eq_ignore_ascii_case("open"))
      .map(|pr| pr.base_ref_name.as_str())
      .filter(|base| !base.is_empty() && *base != workspace.branch_name);
    let seen = seen_bases.get(&workspace.id).map(String::as_str);

    let new_target = if merged_workspace(&target).is_some() {
      Some(past_merged(&target))
    } else {
      // Follow GitHub only while treq still agrees with the base it last saw.
      open_base
        .filter(|base| seen.is_some_and(|seen| seen != *base && seen == target))
        .map(str::to_string)
    };
    if let Some(new_target) = new_target.filter(|t| *t != target && *t != workspace.branch_name) {
      plan.moves.push(StackMove {
        workspace_id: workspace.id,
        new_target,
      });
    }
    if let Some(base) = open_base.filter(|base| seen != Some(*base)) {
      plan.seen_bases.push((workspace.id, base.to_string()));
    }
  }
  plan
}

/// Applies [`plan_pr_base_sync`] to the repository; whether any workspace
/// moved. Best effort: a failed retarget is logged and the rest still run.
pub fn sync_pr_bases(repo_path: &str, prs: &HashMap<String, Option<PrInfo>>) -> bool {
  try_sync_pr_bases(repo_path, prs).unwrap_or_else(|err| {
    log::warn!("pr-stacks: sync {repo_path}: {err}");
    false
  })
}

fn try_sync_pr_bases(
  repo_path: &str,
  prs: &HashMap<String, Option<PrInfo>>,
) -> Result<bool, String> {
  if prs.values().all(Option::is_none) {
    return Ok(false);
  }
  let workspaces = local_db::get_workspaces(repo_path)?;
  let seen = local_db::get_workspace_pr_bases(repo_path)?;
  let default_branch = jj::get_default_branch(repo_path).map_err(|e| e.to_string())?;
  let plan = plan_pr_base_sync(&workspaces, prs, &seen, &default_branch);
  for (id, base) in &plan.seen_bases {
    local_db::set_workspace_pr_base(repo_path, *id, base)?;
  }
  let mut moved_any = false;
  for step in &plan.moves {
    let moved = workspaces::ensure_local_branch(repo_path, &step.new_target).and_then(|()| {
      workspaces::retarget_workspace(
        repo_path,
        step.workspace_id,
        &step.new_target,
        &default_branch,
      )
    });
    match moved {
      Ok(_) => {
        moved_any = true;
        log::info!(
          "pr-stacks: workspace {} now targets {}",
          step.workspace_id,
          step.new_target
        );
      }
      Err(err) => log::warn!(
        "pr-stacks: retarget workspace {} onto {}: {err}",
        step.workspace_id,
        step.new_target
      ),
    }
  }
  Ok(moved_any)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn ws(id: i64, branch: &str, target: Option<&str>) -> Workspace {
    Workspace {
      id,
      repo_path: "/tmp/repo".to_string(),
      workspace_name: branch.to_string(),
      workspace_path: format!("ws/{branch}"),
      branch_name: branch.to_string(),
      created_at: "2026-01-01T00:00:00.000Z".to_string(),
      refreshed_at: None,
      metadata: None,
      target_branch: target.map(str::to_string),
      title: branch.to_string(),
      description: None,
      moved_files: None,
      not_on_remote: false,
      sparse_patterns: None,
      archived: false,
    }
  }

  fn pr(head: &str, base: &str, state: &str) -> (String, Option<PrInfo>) {
    (
      head.to_string(),
      Some(PrInfo {
        number: 1,
        title: String::new(),
        state: state.into(),
        url: String::new(),
        head_ref_name: head.into(),
        base_ref_name: base.into(),
        merge_state_status: None,
        is_draft: false,
        body: String::new(),
      }),
    )
  }

  fn prs<const N: usize>(list: [(String, Option<PrInfo>); N]) -> HashMap<String, Option<PrInfo>> {
    list.into_iter().collect()
  }

  fn seen<const N: usize>(list: [(i64, &str); N]) -> HashMap<i64, String> {
    list
      .into_iter()
      .map(|(id, b)| (id, b.to_string()))
      .collect()
  }

  fn mv(workspace_id: i64, new_target: &str) -> StackMove {
    StackMove {
      workspace_id,
      new_target: new_target.to_string(),
    }
  }

  #[test]
  fn first_sight_of_a_pr_only_remembers_its_base() {
    let plan = plan_pr_base_sync(
      &[ws(1, "feat/b", Some("main"))],
      &prs([pr("feat/b", "feat/a", "OPEN")]),
      &seen([]),
      "main",
    );
    assert_eq!(plan.moves, []);
    assert_eq!(plan.seen_bases, [(1, "feat/a".to_string())]);
  }

  #[test]
  fn a_pr_retargeted_on_github_moves_its_workspace() {
    let plan = plan_pr_base_sync(
      &[ws(1, "feat/a", Some("main")), ws(2, "feat/b", Some("main"))],
      &prs([pr("feat/b", "feat/a", "OPEN")]),
      &seen([(2, "main")]),
      "main",
    );
    assert_eq!(plan.moves, [mv(2, "feat/a")]);
    assert_eq!(plan.seen_bases, [(2, "feat/a".to_string())]);
  }

  #[test]
  fn an_unchanged_pr_base_does_nothing() {
    let plan = plan_pr_base_sync(
      &[ws(2, "feat/b", Some("feat/a"))],
      &prs([pr("feat/b", "feat/a", "OPEN")]),
      &seen([(2, "feat/a")]),
      "main",
    );
    assert_eq!(plan, SyncPlan::default());
  }

  #[test]
  fn a_workspace_restacked_in_treq_keeps_its_target() {
    // The user dragged feat/b onto feat/x; GitHub still says main.
    let unchanged = plan_pr_base_sync(
      &[ws(2, "feat/b", Some("feat/x"))],
      &prs([pr("feat/b", "main", "OPEN")]),
      &seen([(2, "main")]),
      "main",
    );
    assert_eq!(unchanged, SyncPlan::default());

    // GitHub moves too, but treq no longer agreed with GitHub before.
    let retargeted = plan_pr_base_sync(
      &[ws(2, "feat/b", Some("feat/x"))],
      &prs([pr("feat/b", "feat/y", "OPEN")]),
      &seen([(2, "main")]),
      "main",
    );
    assert_eq!(retargeted.moves, []);
    assert_eq!(retargeted.seen_bases, [(2, "feat/y".to_string())]);
  }

  #[test]
  fn closed_and_merged_prs_are_not_followed() {
    let plan = plan_pr_base_sync(
      &[ws(1, "feat/a", Some("main")), ws(2, "feat/b", Some("main"))],
      &prs([pr("feat/a", "dev", "CLOSED"), pr("feat/b", "dev", "MERGED")]),
      &seen([(1, "main"), (2, "main")]),
      "main",
    );
    assert_eq!(plan, SyncPlan::default());
  }

  #[test]
  fn a_child_of_a_merged_base_moves_to_the_base_target() {
    let plan = plan_pr_base_sync(
      &[
        ws(1, "feat/a", Some("dev")),
        ws(2, "feat/b", Some("feat/a")),
      ],
      &prs([pr("feat/a", "dev", "MERGED")]),
      &seen([]),
      "main",
    );
    assert_eq!(plan.moves, [mv(2, "dev")]);
  }

  #[test]
  fn a_child_of_a_merged_base_with_no_target_moves_to_the_default_branch() {
    let plan = plan_pr_base_sync(
      &[ws(1, "feat/a", None), ws(2, "feat/b", Some("feat/a"))],
      &prs([pr("feat/a", "main", "MERGED")]),
      &seen([]),
      "main",
    );
    assert_eq!(plan.moves, [mv(2, "main")]);
  }

  #[test]
  fn merged_bases_are_skipped_down_a_multi_level_stack() {
    let plan = plan_pr_base_sync(
      &[
        ws(1, "feat/a", Some("main")),
        ws(2, "feat/b", Some("feat/a")),
        ws(3, "feat/c", Some("feat/b")),
        ws(4, "feat/d", Some("feat/c")),
      ],
      &prs([
        pr("feat/a", "main", "MERGED"),
        pr("feat/b", "feat/a", "MERGED"),
      ]),
      &seen([]),
      "main",
    );
    assert_eq!(plan.moves, [mv(2, "main"), mv(3, "main")]);
  }

  #[test]
  fn a_merged_base_moves_its_child_once_even_when_github_also_retargeted() {
    let plan = plan_pr_base_sync(
      &[
        ws(1, "feat/a", Some("main")),
        ws(2, "feat/b", Some("feat/a")),
      ],
      &prs([pr("feat/a", "main", "MERGED"), pr("feat/b", "main", "OPEN")]),
      &seen([(2, "feat/a")]),
      "main",
    );
    assert_eq!(plan.moves, [mv(2, "main")]);
    assert_eq!(plan.seen_bases, [(2, "main".to_string())]);
  }

  #[test]
  fn a_merged_or_closed_base_without_a_workspace_is_left_alone() {
    let plan = plan_pr_base_sync(
      &[
        ws(2, "feat/b", Some("feat/a")),
        ws(3, "feat/c", Some("feat/x")),
      ],
      &prs([
        pr("feat/a", "main", "MERGED"),
        pr("feat/x", "main", "CLOSED"),
      ]),
      &seen([]),
      "main",
    );
    assert_eq!(plan, SyncPlan::default());
  }

  #[test]
  fn a_merged_base_cycle_does_not_loop() {
    let plan = plan_pr_base_sync(
      &[
        ws(1, "feat/a", Some("feat/b")),
        ws(2, "feat/b", Some("feat/a")),
        ws(3, "feat/c", Some("feat/a")),
      ],
      &prs([
        pr("feat/a", "feat/b", "MERGED"),
        pr("feat/b", "feat/a", "MERGED"),
      ]),
      &seen([]),
      "main",
    );
    assert!(plan.moves.iter().all(|m| m.new_target == "main"));
  }
}
