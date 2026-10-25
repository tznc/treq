use crate::common::TestRepo;
use std::path::Path;
use treq_lib::local_db::Workspace;

fn workspace_write_file(repo: &TestRepo, ws: &Workspace, path: &str, content: &str) {
  let dir = repo.workspaces_dir().join(&ws.workspace_path);
  let dir_str = dir.to_str().unwrap();
  TestRepo::write_workspace_file(dir_str, path, content).unwrap();
}

fn workspace_diff_paths(repo: &TestRepo, ws: &Workspace) -> Vec<String> {
  init_test_app_db(repo, Some("git"));
  treq_lib::core::workspace_diff(&repo.repo_path, ws.id)
    .unwrap()
    .committed_files
    .iter()
    .map(|f| f.path.clone())
    .collect()
}

fn workspace_diff_fixture_conflicted_files(repo: &TestRepo, ws: &Workspace) -> Vec<String> {
  let workspace_dir = repo.workspaces_dir().join(&ws.workspace_path);
  let workspace_dir_str = workspace_dir.to_str().unwrap();

  treq_lib::jj::get_conflicted_files(workspace_dir_str, None)
    .expect("get_conflicted_files should succeed")
}

fn test_app_data_dir(repo: &TestRepo) -> std::path::PathBuf {
  Path::new(&repo.repo_path).join(".treq")
}

fn init_test_app_db(repo: &TestRepo, conflict_marker_style: Option<&str>) {
  let app_dir = test_app_data_dir(repo);
  let db_path = app_dir.join("treq.db");
  let db = treq_lib::db::Database::new(db_path).expect("test app db should be openable");
  db.init().expect("test app db should initialize");
  if let Some(style) = conflict_marker_style {
    db.set_setting("conflict_marker_style", style)
      .expect("setting conflict marker style should succeed");
  }
}

#[test]
fn test_only_shows_workspace_changes() {
  let repo = TestRepo::new().unwrap();
  repo.commit_file("base.txt", "base", "base").unwrap();
  let ws = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/ws-only",
    None,
    None,
    None,
    None,
    None,
  )
  .unwrap();
  workspace_write_file(&repo, &ws, "new.txt", "new");
  treq_lib::core::commit_workspace(&repo.repo_path, ws.id, "add new").unwrap();

  let paths = workspace_diff_paths(&repo, &ws);
  assert!(paths.contains(&"new.txt".into()));
  assert!(!paths.contains(&"base.txt".into()));
  assert!(!paths.contains(&"README.md".into()));
}

#[test]
fn test_excludes_unmodified_target_files() {
  let repo = TestRepo::new().unwrap();
  for f in ["a.txt", "b.txt", "c.txt"] {
    repo.commit_file(f, f, f).unwrap();
  }
  let ws = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/mod-one",
    None,
    None,
    None,
    None,
    None,
  )
  .unwrap();
  workspace_write_file(&repo, &ws, "b.txt", "modified");
  treq_lib::core::commit_workspace(&repo.repo_path, ws.id, "mod b").unwrap();

  let paths = workspace_diff_paths(&repo, &ws);
  assert_eq!(paths, vec!["b.txt"]);
}

#[test]
fn test_empty_when_no_commits() {
  let repo = TestRepo::new().unwrap();
  repo.commit_file("x.txt", "x", "x").unwrap();
  let ws =
    treq_lib::core::create_workspace(&repo.repo_path, "feat/empty", None, None, None, None, None)
      .unwrap();

  init_test_app_db(&repo, Some("git"));
  let diff = treq_lib::core::workspace_diff(&repo.repo_path, ws.id).unwrap();

  assert!(diff.uncommitted_files.is_empty());
  assert!(diff.committed_files.is_empty());
  assert!(diff.hunks_by_file.is_empty());
  assert!(diff.conflicted_files.is_empty());
  assert!(!diff.too_large_to_render);
  assert!(diff.render_block_reason.is_none());
}

#[test]
fn test_multiple_workspace_commits() {
  let repo = TestRepo::new().unwrap();
  repo.commit_file("target.txt", "t", "t").unwrap();
  let ws =
    treq_lib::core::create_workspace(&repo.repo_path, "feat/multi", None, None, None, None, None)
      .unwrap();
  workspace_write_file(&repo, &ws, "f1.txt", "1");
  treq_lib::core::commit_workspace(&repo.repo_path, ws.id, "add f1").unwrap();
  workspace_write_file(&repo, &ws, "f2.txt", "2");
  treq_lib::core::commit_workspace(&repo.repo_path, ws.id, "add f2").unwrap();

  let paths = workspace_diff_paths(&repo, &ws);
  assert!(paths.contains(&"f1.txt".into()));
  assert!(paths.contains(&"f2.txt".into()));
  assert!(!paths.contains(&"target.txt".into()));
}

#[test]
fn test_commit_workspace_does_not_overwrite_last_rebased_target_commit() {
  let repo = TestRepo::new().unwrap();
  repo.commit_file("base.txt", "base", "base").unwrap();
  let ws = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/rebased-meta",
    None,
    None,
    None,
    None,
    None,
  )
  .unwrap();
  let target_commit =
    treq_lib::jj::jj_get_commit_id(&repo.repo_path, "@").expect("target commit should resolve");
  treq_lib::local_db::update_workspace_last_rebased_commit(&repo.repo_path, ws.id, &target_commit)
    .expect("should seed rebased target metadata");

  workspace_write_file(&repo, &ws, "local.txt", "local change");
  treq_lib::core::commit_workspace(&repo.repo_path, ws.id, "local change").unwrap();

  let workspace = treq_lib::local_db::get_workspace_by_id(&repo.repo_path, ws.id)
    .expect("workspace lookup should succeed")
    .expect("workspace should exist");
  let workspace_dir = repo.workspaces_dir().join(&workspace.workspace_path);
  let workspace_dir_str = workspace_dir
    .to_str()
    .expect("workspace path should be valid utf-8");
  let workspace_tip = treq_lib::jj::jj_get_commit_id(workspace_dir_str, &workspace.branch_name)
    .expect("workspace tip should resolve");

  let last_rebased_target =
    treq_lib::local_db::get_workspace_last_rebased_commit(&repo.repo_path, ws.id)
      .expect("should re-read rebased target commit")
      .expect("rebased target commit should still exist");

  assert!(!last_rebased_target.is_empty());
  assert!(!workspace_tip.is_empty());

  init_test_app_db(&repo, Some("git"));
  let diff = treq_lib::core::workspace_diff(&repo.repo_path, ws.id).unwrap();
  let committed_paths: Vec<_> = diff
    .committed_files
    .iter()
    .map(|f| f.path.as_str())
    .collect();
  assert!(
    committed_paths.contains(&"local.txt"),
    "workspace_diff should still include committed workspace changes, got {:?}",
    committed_paths
  );
}

#[test]
fn test_workspace_diff_when_wc_is_merge_commit() {
  let repo = TestRepo::new().unwrap();
  repo.commit_file("shared.txt", "target", "target").unwrap();

  let ws = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/merge-wc",
    None,
    None,
    None,
    None,
    None,
  )
  .unwrap();
  workspace_write_file(&repo, &ws, "shared.txt", "workspace");
  treq_lib::core::commit_workspace(&repo.repo_path, ws.id, "workspace shared change").unwrap();
  let ws_change_id = TestRepo::jj_change_id(
    repo
      .workspaces_dir()
      .join(&ws.workspace_path)
      .to_str()
      .unwrap(),
    "@-",
  )
  .expect("workspace change_id should resolve");

  TestRepo::remove_file_path(repo.temp_dir.path().join("shared.txt"))
    .expect("failed to delete shared.txt on main");
  treq_lib::jj::jj_commit(&repo.repo_path, "main delete").unwrap();
  let main_change_id =
    TestRepo::jj_change_id(&repo.repo_path, "@-").expect("main change_id should resolve");

  let ws_dir = repo.workspaces_dir().join(&ws.workspace_path);
  TestRepo::jj_new(ws_dir.to_str().unwrap(), &[&ws_change_id, &main_change_id])
    .expect("jj new should create unresolved conflict merge commit");

  init_test_app_db(&repo, Some("git"));
  let result = treq_lib::core::workspace_diff(&repo.repo_path, ws.id);
  let diff = result.expect("workspace_diff must succeed when @ has unresolved conflicts");
  let paths: Vec<String> = diff
    .committed_files
    .iter()
    .map(|f| f.path.clone())
    .collect();
  assert!(
    paths.contains(&"shared.txt".into()),
    "expected merged tree to include conflicted file, got {:?}",
    paths
  );

  assert!(
    diff.conflicted_files.contains(&"shared.txt".into()),
    "expected workspace_diff to report unresolved conflict path, got {:?}",
    diff.conflicted_files
  );
}

#[test]
fn test_workspace_diff_includes_uncommitted_changes_and_conflicts() {
  let repo = TestRepo::new().unwrap();
  repo.commit_file("shared.txt", "base", "base").unwrap();

  let ws = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/diff-contract",
    None,
    None,
    None,
    None,
    None,
  )
  .unwrap();
  let workspace_dir = repo.workspaces_dir().join(&ws.workspace_path);
  let workspace_dir_str = workspace_dir.to_str().unwrap();

  TestRepo::write_workspace_file(workspace_dir_str, "committed.txt", "committed").unwrap();
  treq_lib::core::commit_workspace(&repo.repo_path, ws.id, "add committed").unwrap();

  TestRepo::write_workspace_file(workspace_dir_str, "uncommitted.txt", "draft").unwrap();

  init_test_app_db(&repo, Some("git"));
  let diff = treq_lib::core::workspace_diff(&repo.repo_path, ws.id).unwrap();

  let committed_paths: Vec<_> = diff
    .committed_files
    .iter()
    .map(|f| f.path.as_str())
    .collect();
  assert!(committed_paths.contains(&"committed.txt"));
  assert!(!committed_paths.contains(&"uncommitted.txt"));

  assert!(diff
    .uncommitted_files
    .iter()
    .map(|f| f.path.as_str())
    .any(|x| x == "uncommitted.txt"));
  assert!(diff.conflicted_files.is_empty());
}

#[test]
fn test_workspace_diff_dedupes_committed_files_that_also_have_wc_changes() {
  let repo = TestRepo::new().unwrap();
  repo.commit_file("shared.txt", "base\n", "base").unwrap();

  let ws = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/overlap-committed",
    None,
    None,
    None,
    None,
    None,
  )
  .unwrap();
  let workspace_dir = repo.workspaces_dir().join(&ws.workspace_path);
  let workspace_dir_str = workspace_dir.to_str().unwrap();

  TestRepo::write_workspace_file(workspace_dir_str, "shared.txt", "committed\n").unwrap();
  TestRepo::write_workspace_file(workspace_dir_str, "committed-only.txt", "only committed\n")
    .unwrap();
  treq_lib::core::commit_workspace(&repo.repo_path, ws.id, "commit overlapping base").unwrap();

  TestRepo::write_workspace_file(workspace_dir_str, "shared.txt", "working copy\n").unwrap();
  TestRepo::write_workspace_file(workspace_dir_str, "uncommitted-only.txt", "only wc\n").unwrap();

  init_test_app_db(&repo, Some("git"));
  let diff = treq_lib::core::workspace_diff(&repo.repo_path, ws.id).unwrap();

  let committed_paths: Vec<_> = diff
    .committed_files
    .iter()
    .map(|f| f.path.as_str())
    .collect();
  let uncommitted_paths: Vec<_> = diff
    .uncommitted_files
    .iter()
    .map(|f| f.path.as_str())
    .collect();

  assert!(
    !committed_paths.contains(&"shared.txt"),
    "dirty overlapping files must not duplicate under committed: {:?}",
    committed_paths
  );
  assert!(
    committed_paths.contains(&"committed-only.txt"),
    "clean committed files should remain: {:?}",
    committed_paths
  );
  assert!(
    !committed_paths.contains(&"uncommitted-only.txt"),
    "WC-only files must not appear under committed: {:?}",
    committed_paths
  );
  assert!(
    uncommitted_paths.contains(&"shared.txt"),
    "overlapping dirty file must appear under uncommitted: {:?}",
    uncommitted_paths
  );
  assert!(uncommitted_paths.contains(&"uncommitted-only.txt"));
  assert!(!uncommitted_paths.contains(&"committed-only.txt"));
}

#[test]
fn test_workspace_diff_does_not_pick_up_target_branch_changes_after_force_rebase() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let default_branch = repo.default_branch();

  let ws = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/rebase-diff-regression",
    None,
    None,
    None,
    None,
    None,
  )
  .unwrap();
  let workspace_dir = repo.workspaces_dir().join(&ws.workspace_path);
  let workspace_dir_str = workspace_dir.to_str().unwrap();

  TestRepo::write_workspace_file(workspace_dir_str, "workspace.txt", "workspace change\n").unwrap();
  treq_lib::core::commit_workspace(&repo.repo_path, ws.id, "workspace change").unwrap();

  repo
    .commit_file("upstream.txt", "upstream change\n", "upstream change")
    .unwrap();

  treq_lib::core::check_and_rebase_workspaces(
    &repo.repo_path,
    Some(ws.id),
    Some(default_branch.to_string()),
    Some(true),
    "git",
  )
  .expect("force rebase should succeed");

  init_test_app_db(&repo, Some("git"));
  let diff = treq_lib::core::workspace_diff(&repo.repo_path, ws.id).unwrap();

  let committed_paths: Vec<_> = diff
    .committed_files
    .iter()
    .map(|f| f.path.as_str())
    .collect();
  assert!(
    committed_paths.contains(&"workspace.txt"),
    "workspace commit should still be shown after force rebase: {:?}",
    committed_paths
  );
  assert!(
    !committed_paths.contains(&"upstream.txt"),
    "target-branch commits should not leak into committed changes after force rebase: {:?}",
    committed_paths
  );

  let upstream_file = diff
    .committed_files
    .iter()
    .find(|file| file.path == "upstream.txt");
  assert!(
    upstream_file.is_none(),
    "upstream-only file should not appear in committed section after force rebase"
  );
}

#[test]
fn test_workspace_diff_reports_delete_modify_conflicts_from_jj_lib() {
  let repo = TestRepo::new().unwrap();
  repo.commit_file("shared.txt", "base", "base").unwrap();

  let ws = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/delete-modify-conflict",
    None,
    None,
    None,
    None,
    None,
  )
  .unwrap();
  let workspace_dir = repo.workspaces_dir().join(&ws.workspace_path);
  let workspace_dir_str = workspace_dir.to_str().unwrap();

  TestRepo::write_workspace_file(workspace_dir_str, "shared.txt", "workspace side\n").unwrap();
  treq_lib::core::commit_workspace(&repo.repo_path, ws.id, "workspace modify").unwrap();
  let ws_change_id =
    TestRepo::jj_change_id(workspace_dir_str, "@-").expect("workspace change_id should resolve");

  TestRepo::remove_file_path(repo.temp_dir.path().join("shared.txt")).unwrap();
  treq_lib::jj::jj_commit(&repo.repo_path, "main delete").unwrap();
  let main_change_id =
    TestRepo::jj_change_id(&repo.repo_path, "@-").expect("main change_id should resolve");

  TestRepo::jj_new(workspace_dir_str, &[&ws_change_id, &main_change_id])
    .expect("jj new should create unresolved delete/modify conflict");

  let conflicted_files = workspace_diff_fixture_conflicted_files(&repo, &ws);
  assert!(
    conflicted_files.contains(&"shared.txt".to_string()),
    "expected unresolved conflict list to include shared.txt, got {:?}",
    conflicted_files
  );

  init_test_app_db(&repo, Some("git"));
  let diff = treq_lib::core::workspace_diff(&repo.repo_path, ws.id).unwrap();
  assert!(
    !diff.conflicted_files.is_empty(),
    "workspace_diff should surface unresolved conflicts"
  );
  assert!(
    diff.conflicted_files.contains(&"shared.txt".to_string()),
    "workspace_diff conflicted_files should mirror jj-lib conflict list, got {:?}",
    diff.conflicted_files
  );

  let committed_paths: Vec<_> = diff
    .committed_files
    .iter()
    .map(|f| f.path.as_str())
    .collect();
  assert!(
    committed_paths.contains(&"shared.txt"),
    "committed diff should still render the conflicting file, got {:?}",
    committed_paths
  );
}

#[test]
fn test_workspace_diff_empty_uncommitted_and_conflicts_for_clean_workspace() {
  let repo = TestRepo::new().unwrap();
  repo.commit_file("clean.txt", "clean", "base").unwrap();

  let ws =
    treq_lib::core::create_workspace(&repo.repo_path, "feat/clean", None, None, None, None, None)
      .unwrap();
  workspace_write_file(&repo, &ws, "committed.txt", "committed");
  treq_lib::core::commit_workspace(&repo.repo_path, ws.id, "add committed").unwrap();

  init_test_app_db(&repo, Some("git"));
  let diff = treq_lib::core::workspace_diff(&repo.repo_path, ws.id).unwrap();

  assert!(diff.uncommitted_files.is_empty());
  assert!(diff.conflicted_files.is_empty());
}

#[test]
fn test_workspace_diff_does_not_leak_sibling_workspace_commits() {
  let repo = TestRepo::new().unwrap();
  repo.commit_file("base.txt", "base", "base").unwrap();

  let main_workspace =
    treq_lib::core::create_workspace(&repo.repo_path, "treq/root", None, None, None, None, None)
      .unwrap();
  let sibling_workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "treq/chicken",
    None,
    None,
    None,
    None,
    None,
  )
  .unwrap();

  let sibling_dir = repo
    .workspaces_dir()
    .join(&sibling_workspace.workspace_path);
  let sibling_dir_str = sibling_dir.to_str().unwrap();
  TestRepo::write_workspace_file(sibling_dir_str, "chicken.txt", "cluck").unwrap();
  treq_lib::core::commit_workspace(&repo.repo_path, sibling_workspace.id, "sibling change")
    .unwrap();

  let main_dir = repo.workspaces_dir().join(&main_workspace.workspace_path);
  assert!(
    TestRepo::jj_working_copy_is_clean(main_dir.to_str().unwrap()),
    "main workspace should remain clean"
  );

  init_test_app_db(&repo, Some("git"));
  let diff = treq_lib::core::workspace_diff(&repo.repo_path, main_workspace.id).unwrap();

  assert!(
    diff.committed_files.is_empty(),
    "main workspace review diff should not include sibling commits, got {:?}",
    diff
      .committed_files
      .iter()
      .map(|file| file.path.as_str())
      .collect::<Vec<_>>()
  );
  assert!(
    diff.uncommitted_files.is_empty(),
    "main workspace review diff should not report uncommitted files, got {:?}",
    diff
      .uncommitted_files
      .iter()
      .map(|file| file.path.as_str())
      .collect::<Vec<_>>()
  );
  assert!(
    diff.hunks_by_file.is_empty(),
    "main workspace review diff should not include hunks, got {:?}",
    diff.hunks_by_file
  );
  assert!(
    diff.conflicted_files.is_empty(),
    "main workspace review diff should not include conflicted files, got {:?}",
    diff.conflicted_files
  );
  assert!(!diff.too_large_to_render);
  assert!(diff.render_block_reason.is_none());

  let committed_paths: Vec<_> = diff
    .committed_files
    .iter()
    .map(|f| f.path.as_str())
    .collect();
  assert!(
    committed_paths.is_empty(),
    "main workspace review diff should not include sibling commits, got {:?}",
    committed_paths
  );
}

#[test]
fn test_excludes_uncommitted_changes() {
  let repo = TestRepo::new().unwrap();
  let ws =
    treq_lib::core::create_workspace(&repo.repo_path, "feat/wc", None, None, None, None, None)
      .unwrap();
  workspace_write_file(&repo, &ws, "committed.txt", "c");
  treq_lib::core::commit_workspace(&repo.repo_path, ws.id, "add committed").unwrap();
  workspace_write_file(&repo, &ws, "uncommitted.txt", "u");

  let paths = workspace_diff_paths(&repo, &ws);
  assert!(paths.contains(&"committed.txt".into()));
  assert!(!paths.contains(&"uncommitted.txt".into()));
}

#[test]
fn test_reports_rename_with_previous_path() {
  let repo = TestRepo::new().unwrap();
  repo
    .commit_file("old.txt", "hello", "add old on main")
    .unwrap();
  let ws =
    treq_lib::core::create_workspace(&repo.repo_path, "feat/rename", None, None, None, None, None)
      .unwrap();
  let workspace_dir = repo.workspaces_dir().join(&ws.workspace_path);
  TestRepo::rename_path(
    workspace_dir.join("old.txt"),
    workspace_dir.join("renamed.txt"),
  )
  .expect("rename should succeed");
  treq_lib::core::commit_workspace(&repo.repo_path, ws.id, "rename old").unwrap();

  init_test_app_db(&repo, Some("git"));
  let diff = treq_lib::core::workspace_diff(&repo.repo_path, ws.id).unwrap();
  let renamed = diff
    .committed_files
    .iter()
    .find(|f| f.path == "renamed.txt")
    .expect("renamed entry should exist");
  assert_eq!(renamed.status, "R");
  assert_eq!(renamed.previous_path.as_deref(), Some("old.txt"));
}

#[test]
fn test_workspace_diff_reports_copy_status() {
  let repo = TestRepo::new().unwrap();
  repo
    .commit_file("source.txt", "hello", "add source on main")
    .unwrap();
  let ws =
    treq_lib::core::create_workspace(&repo.repo_path, "feat/copy", None, None, None, None, None)
      .unwrap();
  let workspace_dir = repo.workspaces_dir().join(&ws.workspace_path);
  TestRepo::copy_path(
    workspace_dir.join("source.txt"),
    workspace_dir.join("copied.txt"),
  )
  .expect("copy should succeed");
  treq_lib::core::commit_workspace(&repo.repo_path, ws.id, "copy source").unwrap();

  init_test_app_db(&repo, Some("git"));
  let diff = treq_lib::core::workspace_diff(&repo.repo_path, ws.id).unwrap();
  let copied = diff
    .committed_files
    .iter()
    .find(|f| f.path == "copied.txt")
    .expect("copied entry should exist");
  assert!(
    copied.status == "C" || copied.status == "A",
    "expected C or A status for copied file, got {}",
    copied.status
  );
  if copied.status == "C" {
    assert_eq!(copied.previous_path.as_deref(), Some("source.txt"));
  }
}

#[test]
fn test_diff_of_workspace_stacked_after_a_home_repo_commit() {
  let repo = TestRepo::new().unwrap();
  TestRepo::write_workspace_file(&repo.repo_path, "home.txt", "home").unwrap();
  treq_lib::core::commit_workspace_with_auto_push(&repo.repo_path, None, "home commit")
    .expect("committing on the home repo should succeed");

  let ws = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/after-home-commit",
    None,
    None,
    None,
    None,
    None,
  )
  .unwrap();
  workspace_write_file(&repo, &ws, "ws.txt", "ws");

  init_test_app_db(&repo, Some("git"));
  let diff = treq_lib::core::workspace_diff(&repo.repo_path, ws.id);
  assert!(diff.is_ok(), "workspace diff failed: {:?}", diff.err());
}

#[test]
fn test_workspace_diff_ignores_target_advance_before_rebase() {
  let repo = TestRepo::with_remote().expect("Failed to create test repo");

  let ws = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/target-moved",
    None,
    None,
    None,
    None,
    None,
  )
  .unwrap();
  workspace_write_file(&repo, &ws, "workspace.txt", "workspace change\n");
  treq_lib::core::commit_workspace(&repo.repo_path, ws.id, "workspace change").unwrap();

  // A PR merges on the remote and is fetched while the workspace is still
  // based on the old target tip.
  repo
    .remote_commit_file("upstream.txt", "upstream change\n", "upstream change")
    .unwrap();
  treq_lib::jj::jj_git_fetch(&repo.repo_path).unwrap();

  let paths = workspace_diff_paths(&repo, &ws);
  assert_eq!(
    paths,
    vec!["workspace.txt".to_string()],
    "committed diff should be measured from the fork point, not the moved target tip"
  );
}

#[test]
fn test_workspace_diff_updates_stale_working_copy_before_snapshotting() {
  let repo = TestRepo::new().unwrap();
  let ws = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/stale-reload",
    None,
    None,
    None,
    None,
    None,
  )
  .unwrap();
  workspace_write_file(&repo, &ws, "workspace.txt", "workspace change\n");
  treq_lib::core::commit_workspace(&repo.repo_path, ws.id, "workspace change").unwrap();
  repo
    .commit_file("advanced.txt", "advanced main\n", "advance main")
    .unwrap();
  // Rebases the workspace's commits without checking them out: its disk is
  // now behind its working-copy commit.
  let ws_dir = repo.workspace_full_path(&ws);
  treq_lib::jj::jj_rebase_workspace_bookmark_onto_deferred_checkout(
    &ws_dir,
    &ws.branch_name,
    repo.default_branch(),
  )
  .unwrap();

  init_test_app_db(&repo, Some("git"));
  let diff = treq_lib::core::workspace_diff(&repo.repo_path, ws.id).unwrap();

  assert!(
    diff.uncommitted_files.is_empty(),
    "a stale working copy must not be snapshotted as a revert: {:?}",
    diff.uncommitted_files
  );
  assert!(Path::new(&ws_dir).join("advanced.txt").exists());
}

#[test]
fn test_workspace_diff_rereads_the_committed_part_only_when_a_commit_moved() {
  let repo = TestRepo::new().unwrap();
  repo.commit_file("base.txt", "base", "base").unwrap();
  let ws = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/cached-committed",
    None,
    None,
    None,
    None,
    None,
  )
  .unwrap();
  workspace_write_file(&repo, &ws, "one.txt", "one\n");
  treq_lib::core::commit_workspace(&repo.repo_path, ws.id, "add one").unwrap();
  init_test_app_db(&repo, Some("git"));

  let read = || treq_lib::core::workspace_diff(&repo.repo_path, ws.id).unwrap();
  let first = read();
  let hunks_of = |diff: &treq_lib::jj::JjRevisionDiff| {
    diff
      .hunks_by_file
      .iter()
      .map(|f| (f.path.clone(), f.hunks.clone()))
      .collect::<Vec<_>>()
  };
  assert_eq!(hunks_of(&read()), hunks_of(&first), "nothing moved");

  // A working-copy edit leaves the committed files alone.
  workspace_write_file(&repo, &ws, "wip.txt", "wip\n");
  let edited = read();
  assert_eq!(hunks_of(&edited), hunks_of(&first));
  assert_eq!(edited.uncommitted_files.len(), 1);

  // A new commit changes the pair of commits, so it is read again.
  treq_lib::core::commit_workspace(&repo.repo_path, ws.id, "add wip").unwrap();
  let paths: Vec<String> = read()
    .committed_files
    .iter()
    .map(|f| f.path.clone())
    .collect();
  assert!(paths.contains(&"one.txt".to_string()));
  assert!(paths.contains(&"wip.txt".to_string()));
}
