use crate::common::TestRepo;

fn get_change_id(cwd: &str, rev: &str) -> Result<String, String> {
  TestRepo::jj_change_id(cwd, rev)
}

#[test]
fn test_list_conflicted_files_no_conflicts() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/no-conflicts",
    Some("no conflicts test".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  let workspace_path_str = workspace_path.to_str().unwrap();

  // Write a clean file and commit — no conflicts
  TestRepo::write_workspace_file(workspace_path_str, "clean.txt", "no conflicts here")
    .expect("Failed to write file");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "clean commit")
    .expect("Failed to commit");

  let result = treq_lib::jj::get_conflicted_files(workspace_path_str, None)
    .expect("get_conflicted_files should succeed on clean workspace");

  assert_eq!(
    result,
    Vec::<String>::new(),
    "Expected no conflicted files in a clean workspace"
  );
}

#[test]
fn test_list_conflicted_files_with_conflicts() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/with-conflicts",
    Some("conflict test".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  let workspace_path_str = workspace_path.to_str().unwrap();

  // In workspace: create conflict.txt and commit → sibling change A
  TestRepo::write_workspace_file(workspace_path_str, "conflict.txt", "workspace version\n")
    .expect("Failed to write conflict.txt in workspace");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "workspace commit")
    .expect("Failed to commit in workspace");

  let ws_change_id =
    get_change_id(workspace_path_str, "@-").expect("Failed to get workspace change_id");

  // In main repo working copy: create conflict.txt with different content and commit → sibling change B
  // Both A and B descend from the same parent (init), so rebasing A onto B creates a conflict.
  repo
    .create_file("conflict.txt", "main version\n")
    .expect("Failed to write conflict.txt in main repo");
  treq_lib::jj::jj_commit(&repo.repo_path, "main commit").expect("Failed to commit in main repo");

  let main_change_id = get_change_id(&repo.repo_path, "@-").expect("Failed to get main change_id");

  // In workspace: create a merge commit with both changes as parents.
  // Since ws_change and main_change both add conflict.txt with different content
  // from a common ancestor that doesn't have it, jj creates a conflict in @.
  TestRepo::jj_new(workspace_path_str, &[&ws_change_id, &main_change_id])
    .expect("Failed to create merge commit in workspace");

  // get_conflicted_files should now return conflict.txt
  let result = treq_lib::jj::get_conflicted_files(workspace_path_str, None)
    .expect("get_conflicted_files should not error on workspace with conflicts");

  assert!(
    result.contains(&"conflict.txt".to_string()),
    "Expected conflict.txt in conflicted files, got: {:?}",
    result
  );
}

#[test]
fn test_workspace_status_invariant_no_conflicts_for_divergent_non_conflicting_edits() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let base_workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feature-base",
    Some("base".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create base workspace");
  let base_path = repo.workspaces_dir().join(&base_workspace.workspace_path);
  let base_path_str = base_path.to_str().unwrap();

  let stacked_workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/divergent",
    Some("stacked".to_string()),
    None,
    Some("feature-base"),
    None,
    None,
  )
  .expect("Failed to create stacked workspace");
  let stacked_path = repo
    .workspaces_dir()
    .join(&stacked_workspace.workspace_path);
  let stacked_path_str = stacked_path.to_str().unwrap();

  TestRepo::write_workspace_file(base_path_str, "base-only.txt", "base change\n")
    .expect("Failed to write base-only.txt");
  treq_lib::core::commit_workspace(&repo.repo_path, base_workspace.id, "base commit")
    .expect("Failed to commit base workspace");

  TestRepo::write_workspace_file(stacked_path_str, "stacked-only.txt", "stacked change\n")
    .expect("Failed to write stacked-only.txt");
  treq_lib::core::commit_workspace(&repo.repo_path, stacked_workspace.id, "stacked commit")
    .expect("Failed to commit stacked workspace");

  let status = treq_lib::core::workspace_status(&repo.repo_path, Some(stacked_workspace.id))
    .expect("workspace_status should succeed");
  assert!(
    !status.partial.has_conflicts,
    "Divergent non-conflicting edits must not set has_conflicts"
  );
  assert!(
    status.conflicted_files.is_empty(),
    "Divergent non-conflicting edits must not report conflicted files: {:?}",
    status.conflicted_files
  );
}

#[test]
fn test_workspace_status_invariant_unresolved_conflicts_require_conflicted_files() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/conflict",
    Some("conflict".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");
  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  let workspace_path_str = workspace_path.to_str().unwrap();

  TestRepo::write_workspace_file(workspace_path_str, "README.md", "workspace side\n")
    .expect("Failed to write workspace README");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "workspace commit")
    .expect("Failed to commit workspace");
  let ws_change_id =
    get_change_id(workspace_path_str, "@-").expect("Failed to get workspace change_id");

  repo
    .create_file("README.md", "main side\n")
    .expect("Failed to write main README");
  treq_lib::jj::jj_commit(&repo.repo_path, "main commit").expect("Failed to commit in main");
  let main_change_id = get_change_id(&repo.repo_path, "@-").expect("Failed to get main change_id");

  TestRepo::jj_new(workspace_path_str, &[&ws_change_id, &main_change_id])
    .expect("Failed to create unresolved conflict");

  let status = treq_lib::core::workspace_status(&repo.repo_path, Some(workspace.id))
    .expect("workspace_status should succeed");
  let conflicted_files = treq_lib::jj::get_conflicted_files(workspace_path_str, None)
    .expect("get_conflicted_files should succeed for conflict workspace");
  assert!(
    conflicted_files.contains(&"README.md".to_string()),
    "Expected README.md in conflicted files, got: {:?}",
    conflicted_files
  );

  assert!(
    status.partial.has_conflicts,
    "Unresolved conflict must set has_conflicts=true"
  );
  assert!(
    status.conflicted_files.contains(&"README.md".to_string()),
    "Unresolved conflict must include README.md in conflicted_files: {:?}",
    status.conflicted_files
  );
}

#[test]
fn test_list_conflicted_files_preserves_deleted_side_conflict_path() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  repo
    .create_file("README.md", "line1\nline2\n")
    .expect("Failed to create base README");
  treq_lib::jj::jj_commit(&repo.repo_path, "base commit").expect("Failed to commit base");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/deleted-side-conflict",
    Some("deleted side conflict".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");
  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  let workspace_path_str = workspace_path.to_str().unwrap();

  TestRepo::write_workspace_file(workspace_path_str, "README.md", "line1\n")
    .expect("Failed to update workspace README");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "workspace commit")
    .expect("Failed to commit workspace");
  let ws_change_id =
    get_change_id(workspace_path_str, "@-").expect("Failed to get workspace change_id");

  repo
    .create_file("README.md", "line1\nline2 modified on main\n")
    .expect("Failed to modify README on main");
  treq_lib::jj::jj_commit(&repo.repo_path, "main modify").expect("Failed to commit main");
  let main_change_id = get_change_id(&repo.repo_path, "@-").expect("Failed to get main change_id");

  TestRepo::jj_new(workspace_path_str, &[&ws_change_id, &main_change_id])
    .expect("Failed to create merge commit with deleted-side content conflict");

  let conflicted_files = treq_lib::jj::get_conflicted_files(workspace_path_str, None)
    .expect("get_conflicted_files should succeed");
  assert!(
    conflicted_files.contains(&"README.md".to_string()),
    "Expected deleted-side conflict path to be preserved, got: {:?}",
    conflicted_files
  );
}

#[test]
fn test_list_conflicted_files_is_deterministic_and_deduped() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/multi-conflicts",
    Some("multi conflict test".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");
  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  let workspace_path_str = workspace_path.to_str().unwrap();

  TestRepo::write_workspace_file(workspace_path_str, "z.txt", "workspace z\n")
    .expect("Failed to write z.txt in workspace");
  TestRepo::write_workspace_file(workspace_path_str, "a.txt", "workspace a\n")
    .expect("Failed to write a.txt in workspace");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "workspace commit")
    .expect("Failed to commit workspace");
  let ws_change_id =
    get_change_id(workspace_path_str, "@-").expect("Failed to get workspace change_id");

  repo
    .create_file("z.txt", "main z\n")
    .expect("Failed to write z.txt in main");
  repo
    .create_file("a.txt", "main a\n")
    .expect("Failed to write a.txt in main");
  treq_lib::jj::jj_commit(&repo.repo_path, "main commit").expect("Failed to commit in main");
  let main_change_id = get_change_id(&repo.repo_path, "@-").expect("Failed to get main change_id");

  TestRepo::jj_new(workspace_path_str, &[&ws_change_id, &main_change_id])
    .expect("Failed to create merge commit with conflicts");

  let conflicted_files = treq_lib::jj::get_conflicted_files(workspace_path_str, None)
    .expect("get_conflicted_files should succeed");
  assert_eq!(
    conflicted_files,
    vec!["a.txt".to_string(), "z.txt".to_string()],
    "Expected deterministic sorted, deduplicated conflicted files"
  );
}

#[test]
fn test_list_conflicted_files_none_target_defaults_to_repo_default_branch() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/default-main-target",
    Some("default main target".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");
  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  let workspace_path_str = workspace_path.to_str().unwrap();

  TestRepo::write_workspace_file(workspace_path_str, "conflict.txt", "workspace version\n")
    .expect("Failed to write conflict.txt in workspace");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "workspace commit")
    .expect("Failed to commit in workspace");

  repo
    .create_file("conflict.txt", "main version\n")
    .expect("Failed to write conflict.txt in main repo");
  treq_lib::jj::jj_commit(&repo.repo_path, "main commit").expect("Failed to commit in main repo");

  let default_branch =
    treq_lib::jj::get_default_branch(&repo.repo_path).expect("default branch should resolve");

  let via_none = treq_lib::jj::get_conflicted_files(workspace_path_str, None)
    .expect("get_conflicted_files(None) should succeed");
  let via_default = treq_lib::jj::get_conflicted_files(workspace_path_str, Some(&default_branch))
    .expect("get_conflicted_files(Some(default_branch)) should succeed");

  assert_eq!(
    via_none, via_default,
    "None target must default to repo default-branch diff semantics"
  );
}

/// Conflicts that live only in the committed tip (clean working copy on top of a
/// conflicted tree) must still be discovered — including when the "target" for the
/// conflict query is the workspace's own branch tip (same tree as WC, so a
/// pairwise tree-diff emits nothing). Detection must walk the tree's own conflicts.
#[test]
fn test_detects_conflicts_in_committed_tip_when_tree_diff_is_empty() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/committed-conflict",
    Some("committed conflict".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");
  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  let workspace_path_str = workspace_path.to_str().unwrap();

  TestRepo::write_workspace_file(workspace_path_str, "shared.txt", "workspace side\n")
    .expect("Failed to write workspace shared.txt");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "workspace commit")
    .expect("Failed to commit workspace");
  let ws_change_id =
    get_change_id(workspace_path_str, "@-").expect("Failed to get workspace change_id");

  repo
    .create_file("shared.txt", "main side\n")
    .expect("Failed to write main shared.txt");
  treq_lib::jj::jj_commit(&repo.repo_path, "main commit").expect("Failed to commit main");
  let main_change_id = get_change_id(&repo.repo_path, "@-").expect("Failed to get main change_id");

  // Conflicted merge becomes the working-copy commit.
  TestRepo::jj_new(workspace_path_str, &[&ws_change_id, &main_change_id])
    .expect("Failed to create conflicted merge");

  // Point the workspace bookmark at the conflicted tip, then create an empty
  // working-copy child so the conflict lives only in "committed" history while
  // the WC itself has no dirty files.
  TestRepo::jj_set_bookmark(workspace_path_str, "feat/committed-conflict", "@")
    .expect("Failed to point bookmark at conflicted tip");
  TestRepo::jj_new(workspace_path_str, &["@"])
    .expect("Failed to create empty WC on conflicted tip");

  let changed = treq_lib::jj::jj_get_changed_files(workspace_path_str)
    .expect("jj_get_changed_files should succeed");
  assert!(
    changed.is_empty(),
    "WC must be clean so the conflict is committed-only, got {:?}",
    changed
  );
  assert!(
    treq_lib::jj::workspace_has_unresolved_conflicts(workspace_path_str)
      .expect("workspace_has_unresolved_conflicts should succeed"),
    "conflicted tip must leave WC tree unresolved"
  );

  // Querying with the workspace branch as target makes before/after trees
  // identical — pairwise diff finds nothing. Tree.conflicts() must still win.
  let conflicted_files =
    treq_lib::jj::get_conflicted_files(workspace_path_str, Some("feat/committed-conflict"))
      .expect("get_conflicted_files should succeed for committed-tip conflicts");
  assert!(
    conflicted_files.contains(&"shared.txt".to_string()),
    "committed-tip conflict must be reported even when tree-diff is empty, got {:?}",
    conflicted_files
  );

  let status = treq_lib::core::workspace_status(&repo.repo_path, Some(workspace.id))
    .expect("workspace_status should succeed");
  assert!(
    status.partial.has_conflicts,
    "workspace status must flag conflicts that live in committed tip"
  );
  assert!(
    status.conflicted_files.contains(&"shared.txt".to_string()),
    "workspace status must list committed-tip conflicted files, got {:?}",
    status.conflicted_files
  );
}

#[test]
fn test_resolving_markers_in_wc_clears_conflict_status_before_commit() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/resolve-wc-before-commit",
    Some("resolve in working copy".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");
  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  let workspace_path_str = workspace_path.to_str().unwrap();

  TestRepo::write_workspace_file(workspace_path_str, "README.md", "workspace side\n")
    .expect("Failed to write workspace README");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "workspace commit")
    .expect("Failed to commit workspace");
  let ws_change_id =
    get_change_id(workspace_path_str, "@-").expect("Failed to get workspace change_id");

  repo
    .create_file("README.md", "main side\n")
    .expect("Failed to write main README");
  treq_lib::jj::jj_commit(&repo.repo_path, "main commit").expect("Failed to commit main");
  let main_change_id = get_change_id(&repo.repo_path, "@-").expect("Failed to get main change_id");

  TestRepo::jj_new(workspace_path_str, &[&ws_change_id, &main_change_id])
    .expect("Failed to create unresolved merge conflict");

  let before = treq_lib::core::workspace_status(&repo.repo_path, Some(workspace.id))
    .expect("workspace_status before resolve");
  assert!(
    before.partial.has_conflicts,
    "merge conflict must set has_conflicts before resolve"
  );
  assert!(
    before.conflicted_files.contains(&"README.md".to_string()),
    "merge conflict must list README.md before resolve, got {:?}",
    before.conflicted_files
  );

  // Editing markers away must clear conflict status immediately — Review must
  // treat the file as a normal uncommitted change before any resolve commit.
  TestRepo::write_workspace_file(workspace_path_str, "README.md", "resolved content\n")
    .expect("Failed to write resolved README");

  let after_wc = treq_lib::core::workspace_status(&repo.repo_path, Some(workspace.id))
    .expect("workspace_status after WC resolve");
  assert!(
    !after_wc.partial.has_conflicts,
    "has_conflicts must clear once markers are resolved in the working copy"
  );
  assert!(
    after_wc.conflicted_files.is_empty(),
    "conflicted_files must clear once markers are resolved in the working copy, got {:?}",
    after_wc.conflicted_files
  );
  assert!(
    after_wc.partial.has_changes,
    "resolved content must remain as uncommitted changes"
  );
  assert!(
    !treq_lib::jj::workspace_has_unresolved_conflicts(workspace_path_str)
      .expect("workspace_has_unresolved_conflicts should succeed"),
    "snapshotted WC must report no unresolved conflicts after marker resolve"
  );

  let sidebar = treq_lib::core::list_workspace_statuses(&repo.repo_path)
    .expect("list_workspace_statuses after WC resolve");
  let sidebar_status = sidebar
    .iter()
    .find(|s| s.current.id == workspace.id)
    .expect("workspace should appear in sidebar statuses");
  assert!(
    !sidebar_status.has_conflicts,
    "sidebar has_conflicts must clear once markers are resolved in the working copy"
  );
}

#[test]
fn test_resolve_and_commit_clears_merge_conflict_from_status() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/resolve-merge-conflict",
    Some("resolve merge conflict".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");
  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  let workspace_path_str = workspace_path.to_str().unwrap();

  TestRepo::write_workspace_file(workspace_path_str, "README.md", "workspace side\n")
    .expect("Failed to write workspace README");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "workspace commit")
    .expect("Failed to commit workspace");
  let ws_change_id =
    get_change_id(workspace_path_str, "@-").expect("Failed to get workspace change_id");

  repo
    .create_file("README.md", "main side\n")
    .expect("Failed to write main README");
  treq_lib::jj::jj_commit(&repo.repo_path, "main commit").expect("Failed to commit main");
  let main_change_id = get_change_id(&repo.repo_path, "@-").expect("Failed to get main change_id");

  TestRepo::jj_new(workspace_path_str, &[&ws_change_id, &main_change_id])
    .expect("Failed to create unresolved merge conflict");

  let before = treq_lib::core::workspace_status(&repo.repo_path, Some(workspace.id))
    .expect("workspace_status before resolve");
  assert!(
    before.partial.has_conflicts,
    "merge conflict must set has_conflicts before resolve"
  );
  assert!(
    before.conflicted_files.contains(&"README.md".to_string()),
    "merge conflict must list README.md before resolve, got {:?}",
    before.conflicted_files
  );

  // Resolve by writing clean content (same as editing conflict markers away).
  TestRepo::write_workspace_file(workspace_path_str, "README.md", "resolved content\n")
    .expect("Failed to write resolved README");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "resolve conflict")
    .expect("Failed to commit resolution");

  let after = treq_lib::core::workspace_status(&repo.repo_path, Some(workspace.id))
    .expect("workspace_status after resolve commit");
  assert!(
    !after.partial.has_conflicts,
    "has_conflicts must clear after resolve+commit"
  );
  assert!(
    after.conflicted_files.is_empty(),
    "conflicted_files must clear after resolve+commit, got {:?}",
    after.conflicted_files
  );

  let sidebar = treq_lib::core::list_workspace_statuses(&repo.repo_path)
    .expect("list_workspace_statuses after resolve");
  let sidebar_status = sidebar
    .iter()
    .find(|s| s.current.id == workspace.id)
    .expect("workspace should appear in sidebar statuses");
  assert!(
    !sidebar_status.has_conflicts,
    "sidebar has_conflicts must clear after resolve+commit"
  );
}

#[test]
fn test_resolve_and_commit_clears_rebase_conflict_from_status() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let default_branch = repo.default_branch();

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/resolve-rebase-conflict",
    Some("resolve rebase conflict".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");
  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  let workspace_path_str = workspace_path.to_str().unwrap();

  // Add/add conflict: both sides create the same new path from a shared base.
  TestRepo::write_workspace_file(workspace_path_str, "conflict.txt", "workspace version\n")
    .expect("Failed to write conflict.txt in workspace");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "workspace commit")
    .expect("Failed to commit workspace");

  repo
    .create_file("conflict.txt", "main version\n")
    .expect("Failed to write conflict.txt on main");
  treq_lib::jj::jj_commit(&repo.repo_path, "main commit").expect("Failed to commit main");

  let rebase_result = treq_lib::jj::jj_rebase_workspace_bookmark_onto(
    workspace_path_str,
    &workspace.branch_name,
    default_branch,
  )
  .expect("Failed to rebase workspace onto main");
  assert!(
    rebase_result.success,
    "Expected rebase to succeed with recorded conflict, got: {}",
    rebase_result.message
  );

  let before = treq_lib::core::workspace_status(&repo.repo_path, Some(workspace.id))
    .expect("workspace_status before resolve");
  assert!(
    before.partial.has_conflicts,
    "rebase conflict must set has_conflicts before resolve"
  );
  assert!(
    before
      .conflicted_files
      .contains(&"conflict.txt".to_string()),
    "rebase conflict must list conflict.txt before resolve, got {:?}",
    before.conflicted_files
  );

  TestRepo::write_workspace_file(
    workspace_path_str,
    "conflict.txt",
    "resolved after rebase\n",
  )
  .expect("Failed to write resolved conflict.txt");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "resolve rebase conflict")
    .expect("Failed to commit resolution");

  let after = treq_lib::core::workspace_status(&repo.repo_path, Some(workspace.id))
    .expect("workspace_status after resolve commit");
  assert!(
    !after.partial.has_conflicts,
    "has_conflicts must clear after rebase resolve+commit"
  );
  assert!(
    after.conflicted_files.is_empty(),
    "conflicted_files must clear after rebase resolve+commit, got {:?}",
    after.conflicted_files
  );

  let sidebar = treq_lib::core::list_workspace_statuses(&repo.repo_path)
    .expect("list_workspace_statuses after resolve");
  let sidebar_status = sidebar
    .iter()
    .find(|s| s.current.id == workspace.id)
    .expect("workspace should appear in sidebar statuses");
  assert!(
    !sidebar_status.has_conflicts,
    "sidebar has_conflicts must clear after rebase resolve+commit"
  );
}

#[test]
fn test_resolve_and_commit_clears_committed_tip_conflict_from_status() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/resolve-committed-tip",
    Some("resolve committed tip".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");
  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  let workspace_path_str = workspace_path.to_str().unwrap();

  TestRepo::write_workspace_file(workspace_path_str, "shared.txt", "workspace side\n")
    .expect("Failed to write workspace shared.txt");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "workspace commit")
    .expect("Failed to commit workspace");
  let ws_change_id =
    get_change_id(workspace_path_str, "@-").expect("Failed to get workspace change_id");

  repo
    .create_file("shared.txt", "main side\n")
    .expect("Failed to write main shared.txt");
  treq_lib::jj::jj_commit(&repo.repo_path, "main commit").expect("Failed to commit main");
  let main_change_id = get_change_id(&repo.repo_path, "@-").expect("Failed to get main change_id");

  TestRepo::jj_new(workspace_path_str, &[&ws_change_id, &main_change_id])
    .expect("Failed to create conflicted merge");
  TestRepo::jj_set_bookmark(workspace_path_str, "feat/resolve-committed-tip", "@")
    .expect("Failed to point bookmark at conflicted tip");
  TestRepo::jj_new(workspace_path_str, &["@"])
    .expect("Failed to create empty WC on conflicted tip");

  let before = treq_lib::core::workspace_status(&repo.repo_path, Some(workspace.id))
    .expect("workspace_status before resolve");
  assert!(
    before.partial.has_conflicts,
    "committed-tip conflict must set has_conflicts before resolve"
  );

  TestRepo::write_workspace_file(workspace_path_str, "shared.txt", "resolved tip\n")
    .expect("Failed to write resolved shared.txt");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "resolve committed tip")
    .expect("Failed to commit resolution");

  let after = treq_lib::core::workspace_status(&repo.repo_path, Some(workspace.id))
    .expect("workspace_status after resolve commit");
  assert!(
    !after.partial.has_conflicts,
    "has_conflicts must clear after committed-tip resolve+commit"
  );
  assert!(
    after.conflicted_files.is_empty(),
    "conflicted_files must clear after committed-tip resolve+commit, got {:?}",
    after.conflicted_files
  );

  let sidebar = treq_lib::core::list_workspace_statuses(&repo.repo_path)
    .expect("list_workspace_statuses after resolve");
  let sidebar_status = sidebar
    .iter()
    .find(|s| s.current.id == workspace.id)
    .expect("workspace should appear in sidebar statuses");
  assert!(
    !sidebar_status.has_conflicts,
    "sidebar has_conflicts must clear after committed-tip resolve+commit"
  );
}

#[test]
fn get_file_lines_reads_parent_revision_in_workspace() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let workspace = repo
    .create_workspace_with_commit("feat/read-parent", "a.txt", "committed\n", None)
    .expect("create workspace with commit");
  let ws_dir = repo.workspace_full_path(&workspace);
  TestRepo::write_workspace_file(&ws_dir, "a.txt", "edited\n").expect("edit a.txt");

  let parent = treq_lib::core::changes::get_file_lines(
    &repo.repo_path,
    Some(workspace.id),
    "a.txt",
    true,
    1,
    10,
  )
  .expect("read parent revision in workspace");
  assert_eq!(parent.lines, vec!["committed".to_string()]);
}

#[test]
fn get_file_lines_reads_parent_revision_in_home_repo() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  repo
    .create_file("a.txt", "committed\n")
    .expect("write a.txt");
  treq_lib::jj::jj_commit(&repo.repo_path, "add a").expect("commit");
  repo.create_file("a.txt", "edited\n").expect("edit a.txt");

  let parent = treq_lib::core::changes::get_file_lines(&repo.repo_path, None, "a.txt", true, 1, 10)
    .expect("read parent revision in home repo");
  assert_eq!(parent.lines, vec!["committed".to_string()]);
}

#[test]
fn get_file_lines_reads_conflicted_parent_with_markers() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let workspace = repo
    .create_workspace_with_commit(
      "feat/conflicted-parent",
      "shared.txt",
      "workspace side\n",
      None,
    )
    .expect("create workspace with commit");
  let ws_dir = repo.workspace_full_path(&workspace);
  let ws_change_id = get_change_id(&ws_dir, "@-").expect("workspace change id");

  repo
    .create_file("shared.txt", "main side\n")
    .expect("write main shared.txt");
  treq_lib::jj::jj_commit(&repo.repo_path, "main commit").expect("commit main");
  let main_change_id = get_change_id(&repo.repo_path, "@-").expect("main change id");

  TestRepo::jj_new(&ws_dir, &[&ws_change_id, &main_change_id]).expect("conflicted merge");
  TestRepo::jj_new(&ws_dir, &["@"]).expect("child of conflicted merge");

  let parent = treq_lib::core::changes::get_file_lines(
    &repo.repo_path,
    Some(workspace.id),
    "shared.txt",
    true,
    1,
    50,
  )
  .expect("read conflicted parent revision");
  assert!(
    parent.lines.iter().any(|line| line.starts_with("<<<<<<<")),
    "expected conflict markers, got {:?}",
    parent.lines
  );
  assert!(parent.lines.contains(&"workspace side".to_string()));
  assert!(parent.lines.contains(&"main side".to_string()));
}

#[test]
fn list_file_hunks_batch_matches_single_file_reads() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/hunk-batch",
    None,
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");
  let workspace_path = repo.workspace_full_path(&workspace);
  TestRepo::write_workspace_file(&workspace_path, "a.txt", "one\ntwo\n").unwrap();
  TestRepo::write_workspace_file(&workspace_path, "nested/b.txt", "three\n").unwrap();
  let paths: Vec<String> = ["a.txt", "nested/b.txt", "a.txt", ""]
    .map(String::from)
    .to_vec();

  let batch =
    treq_lib::core::list_file_hunks_batch(&repo.repo_path, Some(workspace.id), paths, "git", None)
      .expect("batch read should succeed");

  let read: Vec<&str> = batch.files.iter().map(|f| f.path.as_str()).collect();
  assert_eq!(
    read,
    ["a.txt", "nested/b.txt", ""],
    "deduplicated, in order"
  );
  for file in &batch.files[..2] {
    let single =
      treq_lib::core::list_file_hunks(&repo.repo_path, Some(workspace.id), &file.path, "git")
        .expect("single read should succeed");
    assert_eq!(
      serde_json::to_value(&file.hunks).unwrap(),
      serde_json::to_value(&single).unwrap()
    );
    assert!(!file.hunks.is_empty());
  }
  assert!(
    batch.files[2].error.is_some(),
    "an invalid path fails alone"
  );
}

#[test]
fn list_file_hunks_batch_token_follows_working_copy_edits() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/hunk-token",
    None,
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");
  let workspace_path = repo.workspace_full_path(&workspace);
  TestRepo::write_workspace_file(&workspace_path, "a.txt", "one\n").unwrap();
  let token = || {
    treq_lib::core::list_file_hunks_batch(
      &repo.repo_path,
      Some(workspace.id),
      vec!["a.txt".into()],
      "git",
      None,
    )
    .expect("batch read should succeed")
    .snapshot_token
  };

  let first = token();
  assert!(!first.is_empty());
  assert_eq!(token(), first, "an unchanged working copy keeps its token");
  TestRepo::write_workspace_file(&workspace_path, "a.txt", "one\ntwo\n").unwrap();
  assert_ne!(token(), first, "an edit changes it");
}

#[test]
fn list_file_hunks_batch_reads_the_diffs_trees() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let workspace = repo
    .create_workspace_simple("feat/hunk-trees")
    .expect("Failed to create workspace");
  let workspace_path = repo.workspace_full_path(&workspace);
  TestRepo::write_workspace_file(&workspace_path, "a.txt", "one\n").unwrap();
  let diff =
    treq_lib::core::workspaces::workspace_cli_diff(&repo.repo_path, workspace.id).expect("diff");
  let token = diff.working_copy_token.expect("a token");
  let before =
    treq_lib::core::list_file_hunks(&repo.repo_path, Some(workspace.id), "a.txt", "git").unwrap();
  // Edited after the list was read: the batch still shows the list's state.
  TestRepo::write_workspace_file(&workspace_path, "a.txt", "one\ntwo\n").unwrap();

  let batch = treq_lib::core::list_file_hunks_batch(
    &repo.repo_path,
    Some(workspace.id),
    vec!["a.txt".into()],
    "git",
    Some(&token),
  )
  .expect("batch read should succeed");

  assert_eq!(batch.snapshot_token, token);
  assert_eq!(
    serde_json::to_value(&batch.files[0].hunks).unwrap(),
    serde_json::to_value(&before).unwrap()
  );
  let fresh =
    treq_lib::core::list_file_hunks(&repo.repo_path, Some(workspace.id), "a.txt", "git").unwrap();
  assert_ne!(
    serde_json::to_value(&fresh).unwrap(),
    serde_json::to_value(&before).unwrap(),
    "a read without the token sees the edit"
  );
}

#[test]
fn list_file_hunks_batch_from_trees_matches_snapshot_reads() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let workspace = repo
    .create_workspace_with_commit(
      "feat/hunk-tree-match",
      "shared.txt",
      "workspace side\n",
      None,
    )
    .expect("create workspace with commit");
  let ws_dir = repo.workspace_full_path(&workspace);
  let ws_change_id = get_change_id(&ws_dir, "@-").expect("workspace change id");
  repo
    .create_file("shared.txt", "main side\n")
    .expect("write main shared.txt");
  treq_lib::jj::jj_commit(&repo.repo_path, "main commit").expect("commit main");
  let main_change_id = get_change_id(&repo.repo_path, "@-").expect("main change id");
  // The working copy's parent is a conflicted merge: its tree has several ids.
  TestRepo::jj_new(&ws_dir, &[&ws_change_id, &main_change_id]).expect("conflicted merge");
  TestRepo::jj_new(&ws_dir, &["@"]).expect("child of conflicted merge");
  TestRepo::write_workspace_file(&ws_dir, "shared.txt", "resolved\n").unwrap();
  TestRepo::write_workspace_file(&ws_dir, "added.txt", "new\n").unwrap();
  TestRepo::write_workspace_file(&ws_dir, "bin.dat", "a\0b\0c").unwrap();
  let diff =
    treq_lib::core::workspaces::workspace_cli_diff(&repo.repo_path, workspace.id).expect("diff");
  let token = diff.working_copy_token.expect("a token");
  assert!(token.contains(','), "a conflicted parent tree: {token}");
  let paths: Vec<String> = diff
    .uncommitted_files
    .iter()
    .map(|f| f.path.clone())
    .collect();
  assert_eq!(paths.len(), 3, "{paths:?}");
  let read = |token: Option<&str>| {
    treq_lib::core::list_file_hunks_batch(
      &repo.repo_path,
      Some(workspace.id),
      paths.clone(),
      "git",
      token,
    )
    .expect("batch read should succeed")
  };

  let from_trees = read(Some(&token));
  let from_snapshot = read(None);

  assert_eq!(from_trees.snapshot_token, from_snapshot.snapshot_token);
  for (a, b) in from_trees.files.iter().zip(&from_snapshot.files) {
    assert_eq!(a.path, b.path);
    assert_eq!(a.error, b.error);
    assert_eq!(
      serde_json::to_value(&a.hunks).unwrap(),
      serde_json::to_value(&b.hunks).unwrap(),
      "{}",
      a.path
    );
  }
}

#[test]
fn list_file_hunks_batch_snapshots_for_a_token_naming_no_trees() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let workspace = repo
    .create_workspace_simple("feat/hunk-bad-token")
    .expect("Failed to create workspace");
  let workspace_path = repo.workspace_full_path(&workspace);
  TestRepo::write_workspace_file(&workspace_path, "a.txt", "one\n").unwrap();
  let read = |token: &str| {
    treq_lib::core::list_file_hunks_batch(
      &repo.repo_path,
      Some(workspace.id),
      vec!["a.txt".into()],
      "git",
      Some(token),
    )
    .expect("batch read should succeed")
  };
  let fresh = read("").snapshot_token;

  for token in [
    "garbage:git",
    "00..00:git",
    "0123456789abcdef0123456789abcdef01234567..0123456789abcdef0123456789abcdef01234567:git",
    // Another marker style than the read's.
    &fresh.replace(":git", ":diff"),
  ] {
    let batch = read(token);
    assert_eq!(batch.snapshot_token, fresh, "{token}");
    assert!(batch.files[0].error.is_none(), "{token}");
    assert!(!batch.files[0].hunks.is_empty(), "{token}");
  }
}

#[test]
fn workspace_diff_names_each_files_content() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let workspace = repo
    .create_workspace_simple("feat/content-ids")
    .expect("Failed to create workspace");
  let workspace_path = repo.workspace_full_path(&workspace);
  TestRepo::write_workspace_file(&workspace_path, "a.txt", "one\n").unwrap();
  TestRepo::write_workspace_file(&workspace_path, "b.txt", "two\n").unwrap();
  let ids = || {
    let diff =
      treq_lib::core::workspaces::workspace_cli_diff(&repo.repo_path, workspace.id).expect("diff");
    diff
      .uncommitted_files
      .into_iter()
      .map(|f| (f.path, f.content_id.expect("a content id")))
      .collect::<std::collections::HashMap<_, _>>()
  };

  let first = ids();
  assert_eq!(ids(), first, "an unchanged working copy keeps its ids");
  TestRepo::write_workspace_file(&workspace_path, "a.txt", "one\nmore\n").unwrap();
  let second = ids();

  assert_ne!(
    second["a.txt"], first["a.txt"],
    "the edited file's id changes"
  );
  assert_eq!(second["b.txt"], first["b.txt"], "the other file's id stays");
}
