use super::*;

/// Build the revset string for jj_get_log based on context
pub(super) fn build_jj_get_log_revset(
  target_ref: &str,
  is_home_repo: bool,
  _limit: Option<usize>,
) -> String {
  let target_ref = format_revset_symbol(target_ref);
  if is_home_repo {
    // Walk full branch history in topological order; we cap after `iter()` because `latest(S, n)` is timestamp-based and breaks merge order.
    format!("::{}", target_ref)
  } else {
    // Match jj_get_commits_ahead: ancestors up to @- (exclude WC tip), drop empty commits.
    format!("({}..@-) ~ empty()", target_ref)
  }
}

pub(super) fn build_home_repo_fallback_revset(limit: Option<usize>) -> String {
  let n = limit.unwrap_or(15);
  format!("latest(::@, {})", n)
}

/// Revset for the common ancestor of `a` and `b` (git's merge-base).
pub fn fork_point_revset(a: &str, b: &str) -> String {
  format!(
    "fork_point({} | {})",
    format_revset_symbol(a),
    format_revset_symbol(b)
  )
}

pub(super) fn format_revset_symbol(symbol: &str) -> String {
  if symbol.starts_with('@') {
    return symbol.to_string();
  }
  if let Some((name, remote)) = symbol.split_once('@') {
    if !name.is_empty() && !remote.is_empty() {
      return revset::format_remote_symbol(name, remote);
    }
  }
  revset::format_symbol(symbol)
}

/// A squash merge rewrites a branch to a new commit while preserving its final
/// tree. When importing a tracked bookmark, jj represents that as a conflict
/// between the old local tip and the new remote tip. In that specific case the
/// remote commit is the resolved branch: retaining the old tip makes stacked
/// children compare against the unsquashed history as well as the squash.
pub(super) fn select_equivalent_remote_tip<T: PartialEq>(
  candidates: &[(String, T)],
  remote_id: Option<&String>,
) -> Option<String> {
  let remote_id = remote_id?;
  let remote_tree = candidates
    .iter()
    .find(|(id, _)| id == remote_id)
    .map(|(_, tree)| tree)?;
  candidates
    .iter()
    .any(|(id, tree)| id != remote_id && tree == remote_tree)
    .then(|| remote_id.clone())
}

/// In a fork clone, `<branch>@upstream` when it has moved ahead of the local
/// bookmark: the fork's own copy of the default branch goes stale, and work
/// must land on the original repository's tip.
fn upstream_tip_ahead_of(
  loaded: &LoadedWorkspaceRepo,
  workspace_path: &str,
  target_branch: &str,
  local: &jj_lib::backend::CommitId,
) -> Option<String> {
  if jj_base_remote(&workspace_repo_path(workspace_path)) != "upstream" {
    return None;
  }
  let symbol = RemoteRefSymbol {
    name: RefName::new(target_branch),
    remote: RemoteName::new("upstream"),
  };
  let remote = loaded.repo.view().get_remote_bookmark(symbol).target.as_normal()?.clone();
  let ahead = remote != *local && loaded.repo.index().is_ancestor(local, &remote).unwrap_or(false);
  ahead.then(|| remote.hex())
}

/// Resolves the revset for a local target-branch bookmark.
/// When the bookmark is conflicted, prefers the commit matching the git local branch tip;
/// otherwise picks the newest non-immutable tip (author timestamp, then commit id).
pub(super) fn resolve_local_target_branch_revision(
  loaded: &LoadedWorkspaceRepo,
  workspace_path: &str,
  target_branch: &str,
) -> Result<String, JjError> {
  let target = loaded
    .repo
    .view()
    .get_local_bookmark(RefName::new(target_branch));
  let ids: Vec<_> = target.added_ids().cloned().collect();
  if ids.is_empty() {
    return Err(JjError::IoError(format!(
      "Local target branch '{}' was not found",
      target_branch
    )));
  }
  if ids.len() == 1 {
    if let Some(ahead) = upstream_tip_ahead_of(loaded, workspace_path, target_branch, &ids[0]) {
      return Ok(ahead);
    }
    return Ok(format_revset_symbol(target_branch));
  }

  // GitHub squash-and-merge replaces the remote tip with a commit that has
  // the same tree as the old branch tip. Resolve that imported bookmark
  // conflict to the squash commit before considering the (stale) git-local
  // branch or timestamp heuristics.
  let remote_symbol = RemoteRefSymbol {
    name: RefName::new(target_branch),
    remote: RemoteName::new("origin"),
  };
  let remote_id = loaded
    .repo
    .view()
    .get_remote_bookmark(remote_symbol)
    .target
    .added_ids()
    .next()
    .map(|id| id.hex());
  let candidates: Vec<_> = ids
    .iter()
    .filter_map(|id| {
      loaded
        .repo
        .store()
        .get_commit(id)
        .ok()
        .map(|commit| (id.hex(), commit.tree_ids().clone()))
    })
    .collect();
  if let Some(selected) = select_equivalent_remote_tip(&candidates, remote_id.as_ref()) {
    return Ok(selected);
  }

  // Prefer git local branch tip when it matches a conflict term.
  let repo_path = workspace_repo_path(workspace_path);
  if let Some(git_local) = git_local_branch_commit_hex(&repo_path, target_branch) {
    if let Some(matched) = ids.iter().find(|id| id.hex() == git_local) {
      return Ok(matched.hex());
    }
  }

  let immutable_revset = evaluate_revset(loaded, "immutable()").ok();
  let is_immutable = immutable_revset.as_ref().map(|r| r.containing_fn());
  let mut preferred_ids: Vec<_> = ids
    .iter()
    .filter(|id| match is_immutable.as_ref() {
      Some(check) => matches!(check(id), Ok(false)),
      None => true,
    })
    .cloned()
    .collect();
  if preferred_ids.is_empty() {
    preferred_ids = ids;
  }

  // Newest author timestamp, then commit id as tie-breaker.
  preferred_ids.sort_by(|a, b| {
    let a_ts = loaded
      .repo
      .store()
      .get_commit(a)
      .map(|c| c.author().timestamp.timestamp.0)
      .unwrap_or(i64::MIN);
    let b_ts = loaded
      .repo
      .store()
      .get_commit(b)
      .map(|c| c.author().timestamp.timestamp.0)
      .unwrap_or(i64::MIN);
    b_ts.cmp(&a_ts).then_with(|| a.hex().cmp(&b.hex()))
  });

  let selected = preferred_ids.first().map(|id| id.hex()).ok_or_else(|| {
    JjError::IoError("Failed to select conflicted local bookmark tip".to_string())
  })?;
  Ok(selected)
}

/// Find the newest workspace ancestor whose tree is identical to the target.
/// This recognizes the old tip represented by a GitHub squash commit, allowing
/// stacked child commits to be separated from the already-merged parent commits.
/// A squash commit is new, so the old tip is never the target's ancestor: only
/// the workspace's side is read, not the whole history below it.
pub(super) fn resolve_tree_equivalent_workspace_base(
  loaded: &LoadedWorkspaceRepo,
  target_revision: &str,
) -> Option<String> {
  let target = resolve_commit_by_revision(loaded, target_revision).ok()?;
  let target_symbol = format_revset_symbol(&target.id().hex());
  let target_descendants = evaluate_revset(loaded, &format!("{target_symbol}::")).ok()?;
  let is_target_descendant = target_descendants.containing_fn();
  let ancestors = evaluate_revset(loaded, &format!("{target_symbol}..@-")).ok()?;
  ancestors
    .iter()
    .commits(loaded.repo.store())
    .filter_map(Result::ok)
    .find(|commit| {
      commit.tree_ids() == target.tree_ids()
        && matches!(is_target_descendant(commit.id()), Ok(false))
    })
    .map(|commit| commit.id().hex())
}

/// Resolve the graph base to use when rebasing a workspace onto `target_revision`.
/// If a squash commit has the same tree as an ancestor in the workspace's old
/// lineage, that ancestor is the correct source boundary; commits below it have
/// already landed in the target as part of the squash.
pub fn jj_resolve_workspace_rebase_base(
  workspace_path: &str,
  target_revision: &str,
) -> Result<String, JjError> {
  let loaded = load_workspace_repo(workspace_path)?;
  Ok(
    resolve_tree_equivalent_workspace_base(&loaded, target_revision)
      .unwrap_or_else(|| target_revision.to_string()),
  )
}

/// Resolve a branch/bookmark to a concrete commit ID for rebase operations.
///
/// This avoids embedding conflicted bookmark names in revsets or move-commit targets.
pub fn resolve_branch_revision_for_rebase(
  workspace_path: &str,
  branch_name: &str,
) -> Result<String, JjError> {
  let loaded = load_workspace_repo(workspace_path)?;
  resolve_local_target_branch_revision(&loaded, workspace_path, branch_name)
}

pub fn jj_get_log(
  workspace_path: &str,
  target_branch: &str,
  is_home_repo: Option<bool>,
  limit: Option<usize>,
) -> Result<JjLogResult, JjError> {
  if target_branch.starts_with('-') || target_branch.contains('\0') || target_branch.is_empty() {
    return Err(JjError::IoError("Invalid target branch name".to_string()));
  }

  let cache_repo_path =
    derive_repo_path_from_workspace(workspace_path).unwrap_or_else(|| workspace_path.to_string());
  let workspace_branch = get_workspace_branch(workspace_path)?;
  let mut loaded = load_workspace_repo(workspace_path)?;
  if is_home_repo.unwrap_or(false) {
    import_git_head_if_needed(&mut loaded, workspace_path)?;
  }
  let wc_tree_override = snapshot_working_copy_tree(&mut loaded, workspace_path)
    .ok()
    .flatten();
  let selected_target_ref =
    resolve_local_target_branch_revision(&loaded, workspace_path, target_branch)?;
  let comparison_base = if is_home_repo.unwrap_or(false) {
    selected_target_ref.clone()
  } else {
    resolve_tree_equivalent_workspace_base(&loaded, &selected_target_ref)
      .unwrap_or_else(|| selected_target_ref.clone())
  };
  let revset_expr = build_jj_get_log_revset(&comparison_base, is_home_repo.unwrap_or(false), limit);
  let mut last_error = None;
  let mut revset = match evaluate_revset(&loaded, &revset_expr) {
    Ok(evaluated) => Some((revset_expr, evaluated)),
    Err(err) => {
      last_error = Some(err);
      None
    }
  };
  if revset.is_none() && is_home_repo.unwrap_or(false) {
    let revset_expr = build_home_repo_fallback_revset(limit);
    match evaluate_revset(&loaded, &revset_expr) {
      Ok(evaluated) => {
        let _ = &selected_target_ref;
        revset = Some((revset_expr, evaluated));
      }
      Err(err) => last_error = Some(err),
    }
  }
  let Some((revset_expr, revset)) = revset else {
    return Err(
      last_error.unwrap_or_else(|| JjError::IoError("Failed to evaluate log revset".to_string())),
    );
  };
  // Home-repo immutability uses the remote tracking bookmark (e.g. `main@origin`); unpushed commits stay mutable, pushed commits immutable; falls back to local bookmark when no remote ref exists.
  let immutable_ref = if is_home_repo.unwrap_or(false) {
    let remote_symbol = RemoteRefSymbol {
      name: RefName::new(target_branch),
      remote: RemoteName::new("origin"),
    };
    let remote_ref = loaded.repo.view().get_remote_bookmark(remote_symbol);
    if !remote_ref.target.is_absent() {
      format!("{}@origin", target_branch)
    } else {
      selected_target_ref
    }
  } else {
    selected_target_ref
  };
  let immutable_revset = evaluate_revset(
    &loaded,
    &format!("::{}", format_revset_symbol(&immutable_ref)),
  )?;
  let is_immutable = immutable_revset.containing_fn();
  let wc_commit_ids: HashSet<_> = loaded
    .repo
    .view()
    .wc_commit_ids()
    .values()
    .cloned()
    .collect();
  let log_limit = limit.unwrap_or(15);
  let fetch_cap = log_limit.saturating_add(10);
  let is_home = is_home_repo.unwrap_or(false);
  let commits: Vec<Commit> = if is_home {
    let mut commits: Vec<_> = revset
      .iter()
      .take(fetch_cap)
      .commits(loaded.repo.store())
      .collect::<Result<Vec<_>, _>>()
      .map_err(|e| {
        JjError::IoError(format!("Failed to iterate revset '{}': {}", revset_expr, e))
      })?;
    commits.retain(|commit| {
      !(commit_description_first_line(commit) == "(no description)"
        && loaded
          .repo
          .view()
          .local_bookmarks_for_commit(commit.id())
          .next()
          .is_none()
        && is_empty_commit(&loaded.repo, commit))
    });
    commits.truncate(log_limit);
    commits
  } else {
    revset
      .iter()
      .commits(loaded.repo.store())
      .collect::<Result<Vec<_>, _>>()
      .map_err(|e| JjError::IoError(format!("Failed to iterate revset '{}': {}", revset_expr, e)))?
  };
  let resolved_later = conflicts_resolved_by_descendants(
    &loaded.repo,
    &commits,
    get_workspace_wc_commit(&loaded)?.as_ref(),
    wc_tree_override
      .as_ref()
      .map(|(commit_id, tree)| (commit_id, tree)),
  );
  let mut commits = build_log_commits(
    &cache_repo_path,
    &loaded.repo,
    commits,
    &wc_commit_ids,
    wc_tree_override
      .as_ref()
      .map(|(commit_id, tree)| (commit_id, tree)),
    &is_immutable,
  );
  for commit in &mut commits {
    if resolved_later.contains(&commit.commit_id) {
      commit.has_conflicts = false;
    }
  }
  let tentative_working_copy =
    if is_home || snapshot_is_empty(&loaded, workspace_path, wc_tree_override.as_ref())? {
      None
    } else {
      get_workspace_wc_commit(&loaded).map(|wc_commit| {
        wc_commit.map(|commit| JjTentativeWorkingCopy {
          workspace_label: workspace_branch.clone(),
          commit: build_wc_log_commit(
            &cache_repo_path,
            &loaded.repo,
            &commit,
            &wc_commit_ids,
            wc_tree_override
              .as_ref()
              .map(|(commit_id, tree)| (commit_id, tree)),
            &is_immutable,
          ),
        })
      })?
    };

  Ok(JjLogResult {
    commits,
    tentative_working_copy,
    target_branch: target_branch.to_string(),
    workspace_branch,
    target_branch_commits: Vec::new(),
    merge_base_id: None,
  })
}

/// Get the most recent commits on a target branch (for display below active workspace commits).
///
/// Takes the first `limit` of `::{target_branch}` in topological order, as the
/// home-repo log does: `latest(::{target_branch}, {limit})` read every commit in
/// the branch's history for its timestamp.
pub fn jj_get_target_branch_log(
  workspace_path: &str,
  target_branch: &str,
  limit: usize,
) -> Result<Vec<JjLogCommit>, JjError> {
  if target_branch.starts_with('-') || target_branch.contains('\0') || target_branch.is_empty() {
    return Err(JjError::IoError("Invalid target branch name".to_string()));
  }

  let cache_repo_path =
    derive_repo_path_from_workspace(workspace_path).unwrap_or_else(|| workspace_path.to_string());
  // Over-fetch to allow dropping WC placeholders without truncating visible target history.
  let fetch_limit = limit.saturating_add(5);
  let loaded = load_workspace_repo(workspace_path)?;
  let selected_target_ref =
    resolve_local_target_branch_revision(&loaded, workspace_path, target_branch)?;
  let revset_expr = build_target_branch_revset(&selected_target_ref);
  let revset = evaluate_revset(&loaded, &revset_expr).map_err(|e| {
    JjError::IoError(format!(
      "Failed to evaluate target branch revset '{}': {}",
      revset_expr, e
    ))
  })?;
  let immutable_revset = evaluate_revset(
    &loaded,
    &format!("::{}", format_revset_symbol(&selected_target_ref)),
  )?;
  let is_immutable = immutable_revset.containing_fn();
  let wc_commit_ids: HashSet<_> = loaded
    .repo
    .view()
    .wc_commit_ids()
    .values()
    .cloned()
    .collect();
  let commits = revset
    .iter()
    .take(fetch_limit)
    .commits(loaded.repo.store())
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| JjError::IoError(format!("Failed to iterate revset '{}': {}", revset_expr, e)))?;
  let mut commits: Vec<_> = commits
    .into_iter()
    .filter(|commit| {
      !(wc_commit_ids.contains(commit.id())
        || commit_description_first_line(commit) == "(no description)"
          && loaded
            .repo
            .view()
            .local_bookmarks_for_commit(commit.id())
            .next()
            .is_none()
          && is_empty_commit(&loaded.repo, commit))
    })
    .collect();
  commits.truncate(limit);
  Ok(build_log_commits(
    &cache_repo_path,
    &loaded.repo,
    commits,
    &wc_commit_ids,
    None,
    &is_immutable,
  ))
}

pub(super) fn build_target_branch_revset(target_ref: &str) -> String {
  format!("::{}", format_revset_symbol(target_ref))
}

/// Get commits that are in workspace but not in target branch
/// Uses revset: target_branch..@- (commits reachable from @- but not from target)
/// Note: Uses @- (parent of working copy) to exclude the empty working copy commit
pub fn jj_get_commits_ahead(
  workspace_path: &str,
  target_branch: &str,
) -> Result<JjCommitsAhead, JjError> {
  WorkspaceRepo::load(workspace_path)?.commits_ahead(target_branch)
}

pub(super) fn commits_ahead_in(
  loaded: &LoadedWorkspaceRepo,
  workspace_path: &str,
  target_branch: &str,
) -> Result<JjCommitsAhead, JjError> {
  if target_branch.starts_with('-') || target_branch.contains('\0') || target_branch.is_empty() {
    return Err(JjError::IoError("Invalid target branch name".to_string()));
  }
  let cache_repo_path =
    derive_repo_path_from_workspace(workspace_path).unwrap_or_else(|| workspace_path.to_string());
  let selected_target_ref =
    resolve_local_target_branch_revision(loaded, workspace_path, target_branch)?;
  let comparison_base = resolve_tree_equivalent_workspace_base(loaded, &selected_target_ref)
    .unwrap_or_else(|| selected_target_ref.clone());
  let revset_expr = format!("({}..@-) ~ empty()", format_revset_symbol(&comparison_base));
  let revset = evaluate_revset(loaded, &revset_expr).map_err(|e| {
    JjError::IoError(format!(
      "Failed to evaluate commits-ahead revset '{}': {}",
      revset_expr, e
    ))
  })?;
  let immutable_revset = evaluate_revset(
    loaded,
    &format!("::{}", format_revset_symbol(&selected_target_ref)),
  )?;
  let is_immutable = immutable_revset.containing_fn();
  let wc_commit_ids: HashSet<_> = loaded
    .repo
    .view()
    .wc_commit_ids()
    .values()
    .cloned()
    .collect();
  let commits = revset
    .iter()
    .commits(loaded.repo.store())
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| {
      JjError::IoError(format!(
        "Failed to iterate commits-ahead revset '{}': {}",
        revset_expr, e
      ))
    })?;
  let commits = build_log_commits(
    &cache_repo_path,
    &loaded.repo,
    commits,
    &wc_commit_ids,
    None,
    &is_immutable,
  );
  let total_count = commits.len();

  Ok(JjCommitsAhead {
    commits,
    total_count,
  })
}

/// Returns the diverged commit view for a home-repo non-default branch.
///
/// - `commits` = commits unique to `current_branch`, followed by commits that
///   `target_branch` is ahead by (marked with `on_target_only = true`)
/// - `merge_base_id` = the commit ID of the latest common ancestor
pub fn jj_get_home_repo_diverged_log(
  repo_path: &str,
  current_branch: &str,
  target_branch: &str,
  limit: Option<usize>,
) -> Result<JjLogResult, JjError> {
  validate_branch_name(current_branch, "current")?;
  validate_branch_name(target_branch, "target")?;

  let cache_repo_path = repo_path.to_string();
  let mut loaded = load_workspace_repo(repo_path)?;
  import_colocated_git_state(&mut loaded, repo_path)?;

  let current_sym = format_revset_symbol(current_branch);
  let target_sym = format_revset_symbol(target_branch);

  // Commits unique to current branch
  let branch_revset = format!("({target_sym}..{current_sym}) ~ empty()");
  // Commits target is ahead by
  let target_ahead_revset = format!("({current_sym}..{target_sym}) ~ empty()");
  // Latest common ancestor (merge base)
  let merge_base_revset = format!("latest(::{current_sym} & ::{target_sym})");

  let wc_commit_ids: HashSet<_> = loaded
    .repo
    .view()
    .wc_commit_ids()
    .values()
    .cloned()
    .collect();

  let immutable_revset_expr = format!("::{target_sym}");
  let immutable_revset = evaluate_revset(&loaded, &immutable_revset_expr)?;
  let is_immutable = immutable_revset.containing_fn();

  // Branch-unique commits
  let branch_commits = match evaluate_revset(&loaded, &branch_revset) {
    Ok(revset) => {
      let cap = limit.unwrap_or(20).saturating_add(5);
      let mut commits: Vec<_> = revset
        .iter()
        .take(cap)
        .commits(loaded.repo.store())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| JjError::IoError(format!("Failed to iterate branch revset: {}", e)))?;
      commits.truncate(limit.unwrap_or(20));
      build_log_commits(
        &cache_repo_path,
        &loaded.repo,
        commits,
        &wc_commit_ids,
        None,
        &is_immutable,
      )
    }
    Err(_) => Vec::new(),
  };

  // Target-ahead commits (mark with on_target_only)
  let target_only_commits = match evaluate_revset(&loaded, &target_ahead_revset) {
    Ok(revset) => {
      let cap = limit.unwrap_or(20).saturating_add(5);
      let commits: Vec<_> = revset
        .iter()
        .take(cap)
        .commits(loaded.repo.store())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| JjError::IoError(format!("Failed to iterate target-ahead revset: {}", e)))?;
      let mut built = build_log_commits(
        &cache_repo_path,
        &loaded.repo,
        commits,
        &wc_commit_ids,
        None,
        &is_immutable,
      );
      for c in &mut built {
        c.on_target_only = true;
      }
      built
    }
    Err(_) => Vec::new(),
  };

  // Merge base
  let merge_base_id = evaluate_revset(&loaded, &merge_base_revset)
    .ok()
    .and_then(|revset| {
      revset
        .iter()
        .commits(loaded.repo.store())
        .next()
        .and_then(|r| r.ok())
        .map(|c| c.id().hex()[..12].to_string())
    });

  let workspace_branch =
    get_workspace_branch(repo_path).unwrap_or_else(|_| current_branch.to_string());

  let mut commits = branch_commits;
  let existing_ids: std::collections::HashSet<String> =
    commits.iter().map(|c| c.commit_id.clone()).collect();
  for commit in target_only_commits {
    if existing_ids.contains(&commit.commit_id) {
      continue;
    }
    commits.push(commit);
  }

  Ok(JjLogResult {
    commits,
    tentative_working_copy: None,
    target_branch: target_branch.to_string(),
    workspace_branch,
    target_branch_commits: Vec::new(),
    merge_base_id,
  })
}

/// Rebase the home-repo branch `current_branch` onto `target_branch`.
///
/// This is the manual rebase affordance for home-repo branches (workspaces auto-rebase;
/// home-repo branches do not). Does not touch any workspace DB records.
pub fn jj_rebase_home_repo_branch(
  repo_path: &str,
  current_branch: &str,
  target_branch: &str,
) -> Result<JjRebaseResult, JjError> {
  validate_branch_name(current_branch, "current")?;
  validate_branch_name(target_branch, "target")?;

  {
    let mut loaded = load_workspace_repo(repo_path)?;
    import_colocated_git_state(&mut loaded, repo_path)?;
  }

  let current_sym = format_revset_symbol(current_branch);
  let target_sym = format_revset_symbol(target_branch);
  // Rebase all commits unique to current branch onto target
  let revset = format!("({target_sym}..{current_sym}) ~ empty()");

  jj_rebase_with_revset(repo_path, &revset, target_branch, current_branch, "git")
}

/// Dry-run a home-repo rebase: check whether rebasing `current_branch` onto `target_branch`
/// would produce conflicts, without mutating the repository.
///
/// Uses a file-overlap heuristic: if both branches changed the same file since their
/// common ancestor, a conflict is likely.
pub fn jj_dry_run_home_repo_rebase(
  repo_path: &str,
  current_branch: &str,
  target_branch: &str,
) -> Result<HomeRebaseDryRunResult, JjError> {
  validate_branch_name(current_branch, "current")?;
  validate_branch_name(target_branch, "target")?;

  let mut loaded = load_workspace_repo(repo_path)?;
  import_colocated_git_state(&mut loaded, repo_path)?;

  let current_sym = format_revset_symbol(current_branch);
  let target_sym = format_revset_symbol(target_branch);
  let merge_base_revset = format!("latest(::{current_sym} & ::{target_sym})");

  let merge_base_commit = {
    let revset = evaluate_revset(&loaded, &merge_base_revset)
      .map_err(|e| JjError::IoError(format!("Failed to find merge base: {}", e)))?;
    revset
      .iter()
      .commits(loaded.repo.store())
      .next()
      .ok_or_else(|| JjError::IoError("No common ancestor found".to_string()))?
      .map_err(|e| JjError::IoError(format!("Failed to load merge base commit: {}", e)))?
  };

  let current_revision = resolve_branch_revision_for_rebase(repo_path, current_branch)?;
  let target_revision = resolve_branch_revision_for_rebase(repo_path, target_branch)?;
  let current_commit = resolve_commit_by_revision(&loaded, &current_revision)?;
  let target_commit = resolve_commit_by_revision(&loaded, &target_revision)?;

  let merge_base_tree = merge_base_commit.tree();
  let current_tree = current_commit.tree();
  let target_tree = target_commit.tree();

  let diff_matcher = repo_root_matcher();

  // Files changed on current branch since merge base
  let current_changed: HashSet<String> = block_on(
    merge_base_tree
      .diff_stream(&current_tree, &diff_matcher)
      .collect::<Vec<_>>(),
  )
  .into_iter()
  .filter_map(|entry| {
    entry
      .values
      .ok()
      .map(|_| entry.path.as_internal_file_string().to_string())
  })
  .collect();

  // Files changed on target branch since merge base
  let target_changed: HashSet<String> = block_on(
    merge_base_tree
      .diff_stream(&target_tree, &diff_matcher)
      .collect::<Vec<_>>(),
  )
  .into_iter()
  .filter_map(|entry| {
    entry
      .values
      .ok()
      .map(|_| entry.path.as_internal_file_string().to_string())
  })
  .collect();

  let mut conflicted_files: Vec<String> = current_changed
    .intersection(&target_changed)
    .cloned()
    .collect();
  conflicted_files.sort();

  Ok(HomeRebaseDryRunResult {
    would_conflict: !conflicted_files.is_empty(),
    conflicted_files,
  })
}
