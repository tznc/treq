use crate::common::TestRepo;
use treq_lib::core::rebase_guard::{self, RebaseHold};
use treq_lib::jj;

/// A workspace targeting the default branch with one committed file.
fn workspace_on_default(repo: &TestRepo, branch: &str) -> (treq_lib::local_db::Workspace, String) {
  let ws = repo
    .create_workspace_with_commit(branch, &format!("{}.txt", branch.replace('/', "-")), "work", None)
    .unwrap();
  let target = repo.default_branch().to_string();
  treq_lib::local_db::update_workspace_target_branch(&repo.repo_path, ws.id, &target).unwrap();
  let path = repo.workspace_full_path(&ws);
  (ws, path)
}

fn has_file(workspace_path: &str, revision: &str, file: &str) -> bool {
  TestRepo::jj_files_at_revision(workspace_path, revision)
    .unwrap()
    .iter()
    .any(|f| f == file)
}

#[test]
fn rebase_fetches_target_before_rebasing() {
  let repo = TestRepo::with_remote().unwrap();
  let (ws, path) = workspace_on_default(&repo, "feat/fetch-first");
  repo.remote_commit_file("advance.txt", "new", "Advance main").unwrap();

  // No manual fetch: the rebase itself must fetch.
  treq_lib::auto_rebase::rebase_single_workspace(&repo.repo_path, ws.id, "main", true, "diff")
    .unwrap();

  assert!(has_file(&path, "feat/fetch-first", "advance.txt"));
  assert_eq!(rebase_guard::hold(&repo.repo_path, ws.id), None);
}

#[test]
fn rebase_fast_forwards_local_target_bookmark_to_remote() {
  let repo = TestRepo::with_remote().unwrap();
  let default = repo.default_branch().to_string();
  let (ws, _) = workspace_on_default(&repo, "feat/ff");
  repo.remote_commit_file("advance.txt", "new", "Advance main").unwrap();

  treq_lib::auto_rebase::rebase_single_workspace(&repo.repo_path, ws.id, "main", true, "diff")
    .unwrap();

  assert!(has_file(&repo.repo_path, &default, "advance.txt"));
  let local = jj::jj_get_commit_id(&repo.repo_path, &default).unwrap();
  let remote = jj::jj_get_commit_id(&repo.repo_path, &format!("{default}@origin")).unwrap();
  assert_eq!(local, remote);
}

#[test]
fn rebase_skips_dirty_workspace() {
  let repo = TestRepo::with_remote().unwrap();
  let (ws, path) = workspace_on_default(&repo, "feat/dirty");
  TestRepo::write_workspace_file(&path, "wip.txt", "uncommitted").unwrap();
  repo.remote_commit_file("advance.txt", "new", "Advance main").unwrap();

  let result =
    treq_lib::auto_rebase::rebase_single_workspace(&repo.repo_path, ws.id, "main", true, "diff")
      .unwrap();

  assert!(result.is_none());
  assert!(!has_file(&path, "feat/dirty", "advance.txt"));
  assert_eq!(rebase_guard::hold(&repo.repo_path, ws.id), Some(RebaseHold::Dirty));
}

#[test]
fn batch_rebase_skips_dirty_workspace() {
  let repo = TestRepo::with_remote().unwrap();
  let (dirty, dirty_path) = workspace_on_default(&repo, "feat/batch-dirty");
  let (clean, clean_path) = workspace_on_default(&repo, "feat/batch-clean");
  TestRepo::write_workspace_file(&dirty_path, "wip.txt", "uncommitted").unwrap();
  repo.remote_commit_file("advance.txt", "new", "Advance main").unwrap();

  treq_lib::auto_rebase::check_and_rebase_all(&repo.repo_path, "diff").unwrap();

  assert!(!has_file(&dirty_path, "feat/batch-dirty", "advance.txt"));
  assert!(has_file(&clean_path, "feat/batch-clean", "advance.txt"));
  assert_eq!(rebase_guard::hold(&repo.repo_path, dirty.id), Some(RebaseHold::Dirty));
  assert_eq!(rebase_guard::hold(&repo.repo_path, clean.id), None);
}

#[test]
fn rebase_skips_when_target_bookmark_diverged_from_remote() {
  let repo = TestRepo::with_remote().unwrap();
  let default = repo.default_branch().to_string();
  let (ws, path) = workspace_on_default(&repo, "feat/diverged");
  // Local main gains a commit origin lacks; origin gains one local lacks.
  let other = repo
    .create_workspace_with_commit("feat/local-only", "local-only.txt", "x", None)
    .unwrap();
  let other_path = repo.workspace_full_path(&other);
  jj::jj_set_bookmark(&other_path, &default, "feat/local-only").unwrap();
  repo.remote_commit_file("advance.txt", "new", "Advance main").unwrap();

  let result =
    treq_lib::auto_rebase::rebase_single_workspace(&repo.repo_path, ws.id, "main", true, "diff")
      .unwrap();

  assert!(result.is_none());
  assert!(!has_file(&path, "feat/diverged", "advance.txt"));
  assert_eq!(
    rebase_guard::hold(&repo.repo_path, ws.id),
    Some(RebaseHold::TargetDiverged { bookmark: default })
  );
}

#[test]
fn rebase_uses_last_fetched_target_and_warns_when_fetch_fails() {
  let repo = TestRepo::with_remote().unwrap();
  let (ws, path) = workspace_on_default(&repo, "feat/offline");
  repo.remote_commit_file("advance.txt", "new", "Advance main").unwrap();
  jj::jj_git_fetch(&repo.repo_path).unwrap();
  let remote = repo.remote_path();
  let moved = remote.with_extension("gone");
  std::fs::rename(&remote, &moved).unwrap();

  let result =
    treq_lib::auto_rebase::rebase_single_workspace(&repo.repo_path, ws.id, "main", true, "diff");
  std::fs::rename(&moved, &remote).unwrap();

  assert!(result.unwrap().is_some());
  assert!(has_file(&path, "feat/offline", "advance.txt"));
  assert!(matches!(
    rebase_guard::hold(&repo.repo_path, ws.id),
    Some(RebaseHold::StaleTarget { .. })
  ));
}
