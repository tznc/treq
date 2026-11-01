/// Retry safety for existing and partially initialized workspace directories.
use crate::common::TestRepo;

// ─── Test A: orphaned directory recovery ─────────────────────────────────────
//
// Simulate: a previous workspace creation succeeded on disk (`.jj` exists)
// but the local_db record was removed — the dir is an orphan.
// Re-creating the workspace for the same branch must succeed and leave exactly
// one db record.
#[test]
fn test_create_workspace_recovers_orphaned_jj_dir() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let branch = "feat/orphan-recovery";

  let workspace_dir = repo.workspaces_dir().join("feat-orphan-recovery");
  let jj_dir = workspace_dir.join(".jj");
  std::fs::create_dir_all(&jj_dir).expect("create partial .jj directory");
  std::fs::write(workspace_dir.join("partial"), "incomplete").expect("write partial marker");

  // A second create for the same branch must succeed even though .jj is present.
  let ws2 = treq_lib::core::create_workspace(
    &repo.repo_path,
    branch,
    Some(branch.to_string()),
    None,
    None,
    None,
    None,
  )
  .unwrap_or_else(|e| {
    panic!(
      "create_workspace should recover from orphaned .jj but failed: {}",
      e
    )
  });

  // The new workspace lives in its own id-prefixed dir.
  assert!(repo
    .workspaces_dir()
    .join(&ws2.workspace_path)
    .join(".jj")
    .exists());
  assert_eq!(
    ws2.workspace_path,
    format!("{}-feat-orphan-recovery", ws2.id)
  );

  // Exactly one db row for this branch.
  let all = treq_lib::local_db::get_workspace_by_branch(&repo.repo_path, branch)
    .expect("Failed to query workspace by branch");
  assert!(
    all.is_some(),
    "Expected exactly one db record for branch after recovery"
  );
  assert_eq!(
    all.unwrap().id,
    ws2.id,
    "db record should be the newly created one"
  );
}

// ─── Test B: re-create over a fully-tracked workspace ────────────────────────
//
// If a workspace is registered, a repeated creation must fail without deleting
// its files or changing its bookmark.
#[test]
fn test_create_workspace_rejects_fully_tracked_workspace() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  let branch = "feat/replace-tracked";

  // First creation.
  let ws1 = treq_lib::core::create_workspace(
    &repo.repo_path,
    branch,
    Some(branch.to_string()),
    None,
    None,
    None,
    None,
  )
  .unwrap_or_else(|e| panic!("First create_workspace failed: {}", e));

  let jj_dir = repo.workspaces_dir().join(&ws1.workspace_path).join(".jj");
  assert!(jj_dir.exists(), ".jj must exist after first creation");
  let marker = jj_dir.parent().unwrap().join("keep-me.txt");
  std::fs::write(&marker, "existing work").expect("write marker");
  let before = crate::common::JjVerifier::get_commit_id_for_rev(&repo.repo_path, branch)
    .expect("read bookmark target")
    .expect("bookmark should resolve");

  let err = treq_lib::core::create_workspace(
    &repo.repo_path,
    branch,
    Some(branch.to_string()),
    None,
    None,
    None,
    None,
  )
  .expect_err("registered retry must fail");
  assert!(
    err.contains("already registered"),
    "unexpected error: {err}"
  );

  assert!(jj_dir.exists(), ".jj must exist after re-create");
  assert_eq!(
    std::fs::read_to_string(marker).expect("marker retained"),
    "existing work"
  );
  let after = crate::common::JjVerifier::get_commit_id_for_rev(&repo.repo_path, branch)
    .expect("read bookmark target")
    .expect("bookmark should resolve");
  assert_eq!(after, before, "bookmark must not move on retry");

  // Exactly one db record for this branch (no duplicate).
  let record = treq_lib::local_db::get_workspace_by_branch(&repo.repo_path, branch)
    .expect("Failed to query workspace by branch");
  assert!(
    record.is_some(),
    "Expected exactly one db record after re-create"
  );
  assert_eq!(record.unwrap().id, ws1.id, "original record must survive");
}
