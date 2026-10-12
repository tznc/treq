mod common;

use common::TestRepo;

const BRANCH: &str = "feat/sync-after-commit";

/// Syncing (pull + push) right after a commit must leave the workspace exactly as
/// the commit left it: bookmark on the described commit, `@` a fresh empty child
/// of it, and only the described commit on the remote.
///
/// Before the fix, `jj_working_copy_needs_sync` treated "bookmark != @" as
/// "needs sync", so the pull leg moved `@` back onto the bookmark and discarded
/// the working-copy commit, and `sync_home_and_workspace_for_branch` pointed the
/// bookmark at `@` — the empty working-copy commit — so an empty commit got
/// pushed to the remote.
#[test]
fn sync_after_commit_keeps_working_copy_and_pushes_real_commit() {
  let repo = TestRepo::with_remote().expect("Failed to create test repo with remote");

  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    BRANCH,
    Some("sync after commit".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  let workspace_path_str = workspace_path.to_str().expect("utf8 workspace path");

  TestRepo::write_workspace_file(workspace_path_str, "synced.txt", "content\n")
    .expect("Failed to write file");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "Real commit")
    .expect("Failed to commit");

  let bookmark_after_commit =
    treq_lib::jj::jj_get_commit_id(workspace_path_str, BRANCH).expect("resolve bookmark");
  let wc_after_commit = treq_lib::jj::jj_get_commit_id(workspace_path_str, "@").expect("resolve @");
  assert_ne!(
    bookmark_after_commit, wc_after_commit,
    "commit should leave an empty working-copy commit on top of the bookmark"
  );

  // Sync = pull then push, exactly what the Sync button does.
  treq_lib::core::pull_workspace_from_remote(&repo.repo_path, Some(workspace.id), "git")
    .expect("Pull should succeed");
  treq_lib::core::push_workspace_to_remote(&repo.repo_path, Some(workspace.id))
    .expect("Push should succeed");

  let bookmark_after_sync =
    treq_lib::jj::jj_get_commit_id(workspace_path_str, BRANCH).expect("resolve bookmark");
  let wc_after_sync = treq_lib::jj::jj_get_commit_id(workspace_path_str, "@").expect("resolve @");
  let wc_parent_after_sync =
    treq_lib::jj::jj_get_commit_id(workspace_path_str, "@-").expect("resolve @-");

  assert_eq!(
    bookmark_after_sync, bookmark_after_commit,
    "sync must not move the bookmark off the committed change"
  );
  assert_eq!(
    wc_after_sync, wc_after_commit,
    "sync must not discard the empty working-copy commit"
  );
  assert_eq!(
    wc_parent_after_sync, bookmark_after_sync,
    "working copy must stay a child of the bookmark after sync"
  );

  // The remote must have the described commit as its tip, not an empty commit.
  let remote_log = TestRepo::run_git(
    &repo.repo_path,
    &[
      "log",
      "--format=%s",
      "-1",
      &format!("refs/remotes/origin/{}", BRANCH),
    ],
  )
  .expect("Failed to read remote branch log");
  assert_eq!(
    remote_log.trim(),
    "Real commit",
    "remote tip should be the real commit, not an empty working-copy commit"
  );
}
