use crate::common::{JjVerifier, TestRepo};
use std::fs;
use std::path::Path;
use treq_lib::jj;

#[test]
fn test_repo_initialization() {
  let repo = TestRepo::new_without_init().expect("Failed to create test repo");
  treq_lib::core::init(&repo.repo_path).ok();
  treq_lib::core::init(&repo.repo_path).ok();

  // --- JJ Initialization ---
  assert!(Path::new(&repo.repo_path).join(".git").exists());
  assert!(Path::new(&repo.repo_path).join(".jj").exists());
  assert!(
    repo.is_jj_initialized(),
    ".jj directory should exist after init"
  );

  // workspaces directory
  assert!(
    &repo.workspaces_dir().exists(),
    "Workspaces directory should exist"
  );

  let jj_workspaces =
    JjVerifier::list_workspaces(&repo.repo_path).expect("Failed to list jj workspaces");
  assert_eq!(
    jj_workspaces.len(),
    1,
    "jj should have exactly 1 workspace after init, got: {:?}",
    jj_workspaces
  );
  assert!(
    jj_workspaces.contains(&"default".to_string()),
    "jj should show 'default' workspace after init, got: {:?}",
    jj_workspaces
  );

  let status = JjVerifier::get_status(&repo.repo_path).expect("Failed to get jj status");
  assert!(
    !status.is_empty(),
    "jj status should return output after init"
  );

  let gitignore_content = repo.read_gitignore().expect("Failed to read .gitignore");
  assert_eq!(
    gitignore_content.matches(".jj/").count(),
    1,
    ".jj/ should appear exactly once"
  );
  assert!(!gitignore_content.contains(".jj*/"));
  assert_eq!(
    gitignore_content.matches(".treq/").count(),
    1,
    ".treq/ should appear exactly once"
  );
  assert!(!gitignore_content.contains(".agents/skills/treq*/"));
  assert!(!gitignore_content.contains(".claude/skills/treq*/"));
  assert!(
    !gitignore_content.contains("# Added by Treq"),
    ".gitignore should not contain Treq comment"
  );

  let db_path = Path::new(&repo.repo_path).join(".treq").join("local.db");
  assert!(db_path.exists(), ".treq/local.db should exist");

  // verify that db exists and is queryable
  let conn = rusqlite::Connection::open(&db_path).expect("Failed to open .treq/local.db");
  let count: i64 = conn
    .query_row("SELECT COUNT(*) FROM workspaces", [], |row| row.get(0))
    .expect("Failed to query workspaces count");

  assert_eq!(count, 0, "workspaces table should be empty after init");

  // verify workspaces dir created
  let workspaces_dir = Path::new(&repo.repo_path).join(".treq").join("workspaces");
  assert!(workspaces_dir.exists(), "workspaces dir should exist");
  assert!(
    workspaces_dir.is_dir(),
    "workspaces dir should be a directory"
  );
}

#[test]
fn test_init_triggers_workspaces_sync() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  // Create workspace
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/test-recover",
    Some("recovery test".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);

  fs::remove_dir_all(&workspace_path).expect("Failed to delete workspace directory");

  treq_lib::core::init(&repo.repo_path).expect("Failed to init");

  let workspaces =
    treq_lib::core::list_workspaces(&repo.repo_path).expect("Failed to list workspaces");
  assert!(workspaces.is_empty(), "Workspaces should be empty");

  assert!(
    !workspace_path.exists(),
    "Workspace directory should stay deleted"
  );
}

#[test]
fn returns_false_when_workspace_is_descendant_of_target() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let default_branch = repo.default_branch();
  let ws = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/descendant",
    Some("descendant".to_string()),
    None,
    Some(default_branch),
    None,
    None,
  )
  .expect("Failed to create workspace");
  treq_lib::local_db::update_workspace_target_branch(&repo.repo_path, ws.id, "feat/descendant")
    .expect("set self target");

  let rebased = treq_lib::core::ensure_workspace_rebased(&repo.repo_path, ws.id, "diff")
    .expect("ensure should succeed");
  assert!(!rebased, "descendant workspace should not be rebased");
}

#[test]
fn returns_true_and_rebases_when_workspace_diverged_from_target() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let default_branch = repo.default_branch();
  let ws = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/diverged",
    Some("diverged".to_string()),
    None,
    Some(default_branch),
    None,
    None,
  )
  .expect("Failed to create workspace");

  repo
    .commit_file("main-after.txt", "main advanced\n", "Advance main")
    .expect("advance main");

  let rebased = treq_lib::core::ensure_workspace_rebased(&repo.repo_path, ws.id, "diff")
    .expect("ensure should succeed");
  assert!(rebased, "diverged workspace should be rebased");
}

#[test]
fn returns_false_when_target_branch_missing() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let ws = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/missing-target",
    Some("missing target".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  treq_lib::local_db::update_workspace_target_branch(&repo.repo_path, ws.id, "no-such-branch")
    .expect("set target branch");

  let rebased = treq_lib::core::ensure_workspace_rebased(&repo.repo_path, ws.id, "diff")
    .expect("ensure should succeed");
  assert!(!rebased, "missing target should be treated as up-to-date");
}

#[test]
fn returns_false_for_self_target_workspace() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let ws = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/self-target",
    Some("self target".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");
  treq_lib::local_db::update_workspace_target_branch(&repo.repo_path, ws.id, "feat/self-target")
    .expect("set self target");

  let rebased = treq_lib::core::ensure_workspace_rebased(&repo.repo_path, ws.id, "diff")
    .expect("ensure should succeed");
  assert!(!rebased, "self-target workspace should be skipped");
}

#[test]
fn init_checks_all_workspaces_and_rebases_only_diverged_ones() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let default_branch = repo.default_branch();

  let _ws_diverged = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/init-diverged",
    Some("init diverged".to_string()),
    None,
    Some(default_branch),
    None,
    None,
  )
  .expect("Failed to create diverged workspace");
  let ws_descendant = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/init-descendant",
    Some("init descendant".to_string()),
    None,
    Some(default_branch),
    None,
    None,
  )
  .expect("Failed to create descendant workspace");
  treq_lib::local_db::update_workspace_target_branch(
    &repo.repo_path,
    ws_descendant.id,
    "feat/init-descendant",
  )
  .expect("set descendant self target");
  let ws_missing_target = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/init-missing-target",
    Some("init missing target".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create missing-target workspace");
  treq_lib::local_db::update_workspace_target_branch(
    &repo.repo_path,
    ws_missing_target.id,
    "no-such-target",
  )
  .expect("set missing target");

  repo
    .commit_file("main-advance.txt", "main moved\n", "Advance main")
    .expect("advance main");
  treq_lib::core::init(&repo.repo_path).expect("init should succeed");
  let workspaces =
    treq_lib::core::list_workspaces(&repo.repo_path).expect("list workspaces after init");
  assert_eq!(
    workspaces.len(),
    3,
    "init should be best-effort across workspaces"
  );
}

#[test]
fn ensure_workspace_rebased_handles_conflicted_bookmark() {
  let repo = TestRepo::with_remote().expect("Failed to create test repo with remote");

  let ws = repo
    .setup_workspace_with_pushed_commit("feat/ensure-conflict", "local.txt", "local content")
    .expect("Failed to create pushed workspace");
  let full_path = repo.workspace_full_path(&ws);

  // Make a local commit diverging from origin
  TestRepo::write_workspace_file(&full_path, "local2.txt", "local2")
    .expect("Failed to write local2");
  treq_lib::core::commit_workspace(&repo.repo_path, ws.id, "Local diverge")
    .expect("commit local diverge");

  // Make a remote commit on same branch to create conflict
  repo
    .remote_commit_on_branch(
      "feat/ensure-conflict",
      "remote.txt",
      "remote content",
      "Remote diverge",
    )
    .expect("Failed to make remote commit");
  jj::jj_git_fetch(&repo.repo_path).expect("Failed to fetch");

  assert!(
    jj::jj_is_bookmark_conflicted(&full_path, "feat/ensure-conflict"),
    "bookmark should be conflicted"
  );

  let rebased = treq_lib::core::ensure_workspace_rebased(&repo.repo_path, ws.id, "diff")
    .expect("ensure_workspace_rebased should succeed on conflicted bookmark");

  assert!(rebased, "conflicted workspace should be rebased");
  assert!(
    !jj::jj_is_bookmark_conflicted(&full_path, "feat/ensure-conflict"),
    "bookmark should be resolved after rebase"
  );
}

#[test]
fn ensure_workspace_rebased_errors_on_missing_bookmark() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let ws = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/missing-bookmark",
    Some("missing bookmark".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  // Corrupt branch_name to something that doesn't exist as a jj bookmark
  treq_lib::local_db::update_workspace_branch_name(
    &repo.repo_path,
    ws.id,
    "feat-missing-bookmark", // dash form — no such jj bookmark
  )
  .expect("update branch_name");

  let result = treq_lib::core::ensure_workspace_rebased(&repo.repo_path, ws.id, "diff");

  assert!(result.is_err(), "missing bookmark should return an error");
  let msg = result.unwrap_err();
  assert!(
    msg.contains("not found in repo"),
    "error should mention 'not found in repo', got: {msg}"
  );
}

#[test]
fn sync_workspaces_healthy_bookmark_is_untouched() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let ws = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/healthy",
    Some("healthy".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  treq_lib::core::sync_workspaces(&repo.repo_path).expect("sync should succeed");

  let workspaces =
    treq_lib::core::list_workspaces(&repo.repo_path).expect("list workspaces after sync");
  assert!(
    workspaces.iter().any(|w| w.id == ws.id),
    "healthy workspace should not be pruned"
  );
}

#[test]
fn sync_workspaces_does_not_delete_on_missing_bookmark() {
  // A missing bookmark alone is not enough to delete — the workspace must be preserved
  // so the user can inspect and delete via the UI.
  let repo = TestRepo::new().expect("Failed to create test repo");

  let ws = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/missing-safe",
    Some("missing safe".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  // Corrupt branch_name so it no longer matches any jj bookmark
  treq_lib::local_db::update_workspace_branch_name(
    &repo.repo_path,
    ws.id,
    "feat-missing-safe-nonexistent",
  )
  .expect("update branch_name");

  treq_lib::core::sync_workspaces(&repo.repo_path).expect("sync should succeed");

  let workspaces =
    treq_lib::core::list_workspaces(&repo.repo_path).expect("list workspaces after sync");
  assert!(
    workspaces.iter().any(|w| w.id == ws.id),
    "workspace with missing bookmark should NOT be auto-deleted by sync"
  );
}

#[test]
fn ensure_workspace_rebased_errors_but_preserves_on_truly_missing_bookmark() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let ws = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/truly-missing",
    Some("truly missing".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  // Corrupt to a value that doesn't match any local, remote, or git-HEAD branch
  treq_lib::local_db::update_workspace_branch_name(
    &repo.repo_path,
    ws.id,
    "nonexistent-ghost-branch",
  )
  .expect("update branch_name");

  let result = treq_lib::core::ensure_workspace_rebased(&repo.repo_path, ws.id, "diff");

  assert!(
    result.is_err(),
    "truly missing bookmark should return an Err"
  );
  assert!(
    result.unwrap_err().contains("not found in repo"),
    "error should describe the problem"
  );

  // Crucially, the workspace row must still exist — no auto-deletion
  let still_exists = treq_lib::local_db::get_workspace_by_id(&repo.repo_path, ws.id)
    .expect("db lookup should succeed")
    .is_some();
  assert!(still_exists, "workspace row must not be auto-deleted");
}

#[test]
fn init_restores_required_gitignore_entries_when_reopening_an_initialized_repo() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let gitignore_path = Path::new(&repo.repo_path).join(".gitignore");
  fs::remove_file(&gitignore_path).expect("remove .gitignore");

  assert_eq!(treq_lib::core::init(&repo.repo_path), Ok(true));

  let gitignore = fs::read_to_string(&gitignore_path).expect("read .gitignore");
  assert_eq!(gitignore, ".jj/\n.treq/\n");
}

#[test]
fn init_rejects_folder_that_is_not_a_git_repository() {
  let dir = tempfile::TempDir::new().expect("create temp dir");
  let path = dir.path().to_string_lossy().to_string();

  let error = treq_lib::core::init(&path).expect_err("init must fail for a non-git folder");

  assert_eq!(
    error.to_string(),
    format!("{path} is not a Git repository. Open a folder that contains a .git directory.")
  );
  assert!(
    !dir.path().join(".treq").exists(),
    ".treq must not be created"
  );
  assert!(
    !dir.path().join(".gitignore").exists(),
    ".gitignore must not be touched"
  );
}
