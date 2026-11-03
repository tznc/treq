use crate::common::TestRepo;
use rusqlite::params;
use treq_lib::local_db::get_local_db_path;

fn set_workspace_refreshed_at(repo_path: &str, id: i64, ts: &str) {
  let db_path = get_local_db_path(repo_path);
  let conn = rusqlite::Connection::open(db_path).expect("open local db");
  conn
    .execute(
      "UPDATE workspaces SET refreshed_at = ?1 WHERE id = ?2",
      params![ts, id],
    )
    .expect("update refreshed_at");
}

#[test]
fn test_workspace_list_statuses_show_conflict_state() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let default_branch = repo.default_branch();

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "base",
    Some("sidebar conflict test".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  let workspace_path_str = workspace_path.to_str().expect("utf-8");

  let statuses = treq_lib::core::list_workspace_statuses(&repo.repo_path)
    .expect("Failed to list workspace statuses");
  let clean_status = statuses
    .iter()
    .find(|s| s.current.id == workspace.id)
    .expect("workspace status should exist");
  assert!(!clean_status.has_conflicts);

  // Add/add conflict setup: workspace and main both add conflict.txt from a common base
  // where the file doesn't exist. Rebasing workspace onto main should leave unresolved conflict.
  TestRepo::write_workspace_file(workspace_path_str, "conflict.txt", "workspace version\n")
    .expect("Failed to write conflict.txt in workspace");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "workspace commit")
    .expect("Failed to commit in workspace");

  repo
    .create_file("conflict.txt", "main version\n")
    .expect("Failed to write conflict.txt in main repo");
  treq_lib::jj::jj_commit(&repo.repo_path, "main commit").expect("Failed to commit in main repo");

  let rebase_result = treq_lib::jj::jj_rebase_workspace_bookmark_onto(
    workspace_path_str,
    &workspace.branch_name,
    default_branch,
  )
  .expect("Failed to rebase workspace onto main");
  assert!(
    rebase_result.success,
    "Expected rebase command to succeed with recorded conflict, got: {}",
    rebase_result.message
  );

  let conflicted_files = treq_lib::jj::get_conflicted_files(workspace_path_str, None)
    .expect("Failed to list conflicted files");
  assert!(
    conflicted_files.contains(&"conflict.txt".to_string()),
    "Expected conflict.txt in conflicted files, got: {:?}",
    conflicted_files
  );
  let conflicted_files_explicit_target =
    treq_lib::jj::get_conflicted_files(workspace_path_str, Some(default_branch))
      .expect("Failed to list conflicted files with explicit target");
  assert_eq!(
    conflicted_files_explicit_target, conflicted_files,
    "Expected target-default and explicit-target conflict views to match"
  );

  let statuses = treq_lib::core::list_workspace_statuses(&repo.repo_path)
    .expect("Failed to list workspace statuses");
  let conflicted_status = statuses
    .iter()
    .find(|s| s.current.id == workspace.id)
    .expect("workspace status should exist");
  assert!(conflicted_status.has_conflicts);
}

#[test]
fn test_workspace_list_statuses_ignore_untracked_noise() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/ignored-noise",
    Some("ignored noise".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  let workspace_path_str = workspace_path.to_str().expect("utf-8");

  TestRepo::write_workspace_file(
    workspace_path_str,
    "node_modules/pkg/index.js",
    "console.log('ignored');\n",
  )
  .expect("Failed to write ignored file");

  let statuses = treq_lib::core::list_workspace_statuses(&repo.repo_path)
    .expect("Failed to list workspace statuses");
  let status = statuses
    .iter()
    .find(|s| s.current.id == workspace.id)
    .expect("workspace status should exist");
  assert!(!status.has_conflicts);
}

#[test]
fn test_workspace_list_statuses_discovers_and_persists_workspace_when_db_is_empty() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/discovered",
    Some("discovered workspace".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  treq_lib::local_db::delete_workspace(&repo.repo_path, workspace.id)
    .expect("Failed to delete workspace from db");

  let statuses = treq_lib::core::list_workspace_statuses(&repo.repo_path)
    .expect("Failed to list workspace statuses");
  let status = statuses
    .iter()
    .find(|s| s.current.workspace_name == workspace.workspace_name)
    .expect("discovered workspace should be returned");

  assert_eq!(status.current.branch_name, "feat/discovered");
  assert!(status.current.refreshed_at.is_some());

  let persisted =
    treq_lib::local_db::get_workspace_by_path(&repo.repo_path, &workspace.workspace_path)
      .expect("db lookup should succeed")
      .expect("discovered workspace should be persisted");
  assert_eq!(persisted.branch_name, "feat/discovered");
  assert!(persisted.refreshed_at.is_some());
}

#[test]
fn test_workspace_list_statuses_excludes_default_workspace_and_ignores_stale_db_rows() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let stale_id = treq_lib::local_db::add_workspace(
    &repo.repo_path,
    "stale".to_string(),
    "stale".to_string(),
    "stale-branch".to_string(),
    None,
    None,
    None,
  )
  .expect("Failed to insert stale workspace");
  set_workspace_refreshed_at(&repo.repo_path, stale_id, "2000-01-01T00:00:00Z");

  let statuses = treq_lib::core::list_workspace_statuses(&repo.repo_path)
    .expect("Failed to list workspace statuses");

  assert!(
    statuses
      .iter()
      .all(|s| s.current.workspace_name != "default"),
    "default workspace should not be returned"
  );
  assert!(
    statuses.iter().all(|s| s.current.workspace_name != "stale"),
    "stale db row should not be returned when jj does not discover it"
  );

  let persisted_stale = treq_lib::local_db::get_workspace_by_id(&repo.repo_path, stale_id)
    .expect("db lookup should succeed")
    .expect("stale row should remain persisted");
  assert_eq!(
    persisted_stale.refreshed_at.as_deref(),
    Some("2000-01-01T00:00:00Z")
  );
}

#[test]
fn test_workspace_list_statuses_preserves_existing_workspace_metadata_on_upsert() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let default_branch = repo.default_branch();

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/preserve-metadata",
    Some("initial description".to_string()),
    Some(vec!["src/lib.rs".to_string()]),
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  treq_lib::local_db::update_workspace_target_branch(&repo.repo_path, workspace.id, default_branch)
    .expect("Failed to set target branch");
  treq_lib::local_db::update_workspace_not_on_remote(&repo.repo_path, workspace.id, true)
    .expect("Failed to set not_on_remote");
  set_workspace_refreshed_at(&repo.repo_path, workspace.id, "2001-01-01T00:00:00Z");

  let before = treq_lib::local_db::get_workspace_by_id(&repo.repo_path, workspace.id)
    .expect("db lookup should succeed")
    .expect("workspace should exist");
  let before_created_at = before.created_at;

  let statuses = treq_lib::core::list_workspace_statuses(&repo.repo_path)
    .expect("Failed to list workspace statuses");
  let status = statuses
    .iter()
    .find(|s| s.current.id == workspace.id)
    .expect("workspace status should exist");

  assert_eq!(
    status.current.target_branch.as_deref(),
    Some(default_branch)
  );
  assert_eq!(
    status.current.description.as_deref(),
    Some("initial description")
  );
  assert_eq!(
    status.current.moved_files,
    Some(vec!["src/lib.rs".to_string()])
  );
  assert!(status.current.not_on_remote);
  // Nothing changed, so the poll leaves the row untouched.
  assert_eq!(
    status.current.refreshed_at.as_deref(),
    Some("2001-01-01T00:00:00Z")
  );
  assert_eq!(status.current.created_at, before_created_at);
}

#[test]
fn test_workspace_list_statuses_conflict_bit_matches_workspace_status_for_divergent_non_conflicting_edits(
) {
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
  let base_path_str = base_path.to_str().expect("utf-8");

  let stacked_workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/divergent-parity",
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
  let stacked_path_str = stacked_path.to_str().expect("utf-8");

  TestRepo::write_workspace_file(base_path_str, "base-only.txt", "base change\n")
    .expect("Failed to write base-only.txt");
  treq_lib::core::commit_workspace(&repo.repo_path, base_workspace.id, "base commit")
    .expect("Failed to commit base workspace");

  TestRepo::write_workspace_file(stacked_path_str, "stacked-only.txt", "stacked change\n")
    .expect("Failed to write stacked-only.txt");
  treq_lib::core::commit_workspace(&repo.repo_path, stacked_workspace.id, "stacked commit")
    .expect("Failed to commit stacked workspace");

  let sidebar_statuses = treq_lib::core::list_workspace_statuses(&repo.repo_path)
    .expect("Failed to list workspace statuses");
  let sidebar_conflict = sidebar_statuses
    .iter()
    .find(|s| s.current.id == stacked_workspace.id)
    .map(|s| s.has_conflicts)
    .expect("stacked workspace should be present in sidebar statuses");

  let detailed_status =
    treq_lib::core::workspace_status(&repo.repo_path, Some(stacked_workspace.id))
      .expect("workspace_status should succeed");
  let review_conflict = detailed_status.partial.has_conflicts;

  assert_eq!(
    sidebar_conflict, review_conflict,
    "sidebar conflict bit should match workspace_status conflict bit"
  );
}

#[test]
fn test_workspace_list_statuses_does_not_cross_assign_sibling_branch_name_when_exact_bookmark_missing(
) {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let checks = treq_lib::core::create_workspace(
    &repo.repo_path,
    "treq/feat-checks",
    Some("checks".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create checks workspace");

  let logging = treq_lib::core::create_workspace(
    &repo.repo_path,
    "treq/feat-logging",
    Some("logging".to_string()),
    None,
    Some("treq/feat-checks"),
    None,
    None,
  )
  .expect("Failed to create logging workspace");

  treq_lib::jj::jj_delete_bookmark(&repo.repo_path, "treq/feat-logging")
    .expect("Failed to delete logging bookmark");

  let statuses = treq_lib::core::list_workspace_statuses(&repo.repo_path)
    .expect("Failed to list workspace statuses");
  let logging_status = statuses
    .iter()
    .find(|s| s.current.workspace_name == logging.workspace_name)
    .expect("logging workspace should be returned");

  assert_eq!(
    checks.workspace_name,
    format!("{}-treq-feat-checks", checks.id)
  );
  assert_eq!(
    logging.workspace_name,
    format!("{}-treq-feat-logging", logging.id)
  );
  assert_ne!(logging_status.current.branch_name, "treq/feat-checks");
}

/// Regression: after a local commit, WC sits on an empty tip above the bookmark.
/// A divergent fetch then conflicted-bookmarks the parent — not the WC tip.
/// Discovery must keep the slash bookmark name (`feat/…`), not overwrite DB
/// `branch_name` with the dashed jj workspace name (`feat-…`), or GitHub PR
/// lookup and tip revsets fail with "Revision `feat-…` doesn't exist".
/// Status refresh also auto-resolves the conflict losslessly onto a linear tip.
#[test]
fn list_statuses_preserves_slash_branch_and_auto_resolves_conflicted_bookmark() {
  let repo = TestRepo::with_remote().expect("Failed to create test repo with remote");
  let branch = "claude/focused-fermat-ZdeFU";
  let ws = repo
    .setup_workspace_with_pushed_commit(branch, "local.txt", "local")
    .expect("Failed to create pushed workspace");
  let workspace_path = repo.workspace_full_path(&ws);

  TestRepo::write_workspace_file(&workspace_path, "local2.txt", "local2")
    .expect("Failed to write local diverge");
  treq_lib::core::commit_workspace(&repo.repo_path, ws.id, "local diverge")
    .expect("Failed to commit local diverge");
  let local_change =
    treq_lib::jj::jj_get_change_id(&workspace_path, "@-").expect("local change id before conflict");

  repo
    .remote_commit_on_branch(branch, "remote.txt", "remote", "remote diverge")
    .expect("Failed to make remote commit");
  treq_lib::jj::jj_git_fetch(&repo.repo_path).expect("Failed to fetch");

  assert!(
    treq_lib::jj::jj_is_bookmark_conflicted(&workspace_path, branch),
    "bookmark should be conflicted after divergent fetch"
  );
  assert_eq!(
    treq_lib::local_db::get_workspace_by_id(&repo.repo_path, ws.id)
      .expect("db lookup")
      .expect("workspace exists")
      .branch_name,
    branch,
    "precondition: DB still has slash branch name before discovery sync"
  );

  let statuses = treq_lib::core::list_workspace_statuses(&repo.repo_path)
    .expect("Failed to list workspace statuses");
  let status = statuses
    .iter()
    .find(|s| s.current.id == ws.id)
    .expect("workspace status should exist");

  assert_eq!(
    status.current.branch_name, branch,
    "discovery must not replace slash branch with sanitized workspace name"
  );
  assert_ne!(
    status.current.branch_name, status.current.workspace_name,
    "branch_name must stay distinct from dashed workspace_name"
  );

  let persisted = treq_lib::local_db::get_workspace_by_id(&repo.repo_path, ws.id)
    .expect("db lookup")
    .expect("workspace exists");
  assert_eq!(persisted.branch_name, branch);

  assert!(
    !treq_lib::jj::jj_is_bookmark_conflicted(&workspace_path, branch),
    "status refresh should auto-resolve conflicted bookmark losslessly"
  );
  let reachable = treq_lib::jj::jj_log_revset_commit_ids(
    &workspace_path,
    &format!("{} & ::{}", local_change, branch),
  )
  .expect("local change must remain reachable after lossless resolve");
  assert_eq!(
    reachable.len(),
    1,
    "local diverge must survive linear lossless resolve"
  );

  // Tip revsets must work again (the failure mode that broke Review / PR linking).
  treq_lib::jj::jj_get_commit_id(&workspace_path, branch)
    .expect("resolved bookmark tip must be resolvable");
}

#[test]
fn test_workspace_list_statuses_includes_last_activity_at_from_wc_tip() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let older = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/older",
    Some("older".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create older workspace");
  let older_path = repo.workspaces_dir().join(&older.workspace_path);
  let older_path_str = older_path.to_str().expect("utf-8");
  TestRepo::write_workspace_file(older_path_str, "older.txt", "old\n")
    .expect("Failed to write older.txt");
  treq_lib::core::commit_workspace(&repo.repo_path, older.id, "older commit")
    .expect("Failed to commit older workspace");

  let unix_secs = || {
    std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .expect("clock after epoch")
      .as_secs()
  };
  let older_second = unix_secs();
  while unix_secs() == older_second {
    std::thread::sleep(std::time::Duration::from_millis(10));
  }

  let newer = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/newer",
    Some("newer".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create newer workspace");
  let newer_path = repo.workspaces_dir().join(&newer.workspace_path);
  let newer_path_str = newer_path.to_str().expect("utf-8");
  TestRepo::write_workspace_file(newer_path_str, "newer.txt", "new\n")
    .expect("Failed to write newer.txt");
  treq_lib::core::commit_workspace(&repo.repo_path, newer.id, "newer commit")
    .expect("Failed to commit newer workspace");

  let statuses = treq_lib::core::list_workspace_statuses(&repo.repo_path)
    .expect("Failed to list workspace statuses");
  let older_status = statuses
    .iter()
    .find(|s| s.current.id == older.id)
    .expect("older status");
  let newer_status = statuses
    .iter()
    .find(|s| s.current.id == newer.id)
    .expect("newer status");

  let older_activity = older_status
    .last_activity_at
    .as_deref()
    .expect("older last_activity_at");
  let newer_activity = newer_status
    .last_activity_at
    .as_deref()
    .expect("newer last_activity_at");
  assert!(
    newer_activity > older_activity,
    "expected newer tip activity ({newer_activity}) > older ({older_activity})"
  );
}

#[test]
fn test_workspaces_have_changes_reads_each_working_copy_together() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let created: Vec<_> = (1..=3)
    .map(|i| {
      treq_lib::core::create_workspace(
        &repo.repo_path,
        &format!("feat/changes-{i}"),
        None,
        None,
        None,
        None,
        None,
      )
      .expect("Failed to create workspace")
    })
    .collect();
  let edited = repo.workspaces_dir().join(&created[1].workspace_path);
  TestRepo::write_workspace_file(edited.to_str().expect("utf-8"), "edit.txt", "agent edit\n")
    .expect("Failed to write workspace file");

  let changes = treq_lib::core::workspaces_have_changes(&repo.repo_path, &created);

  assert_eq!(
    changes,
    std::collections::HashMap::from([
      (created[0].id, false),
      (created[1].id, true),
      (created[2].id, false),
    ])
  );
  for workspace in &created {
    assert_eq!(
      treq_lib::core::workspace_has_changes(&repo.repo_path, workspace.id).ok(),
      Some(changes[&workspace.id]),
      "the same as read one at a time"
    );
  }
}
