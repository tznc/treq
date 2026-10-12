mod e2e_test_helpers;

use e2e_test_helpers::TestRepo;

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

/// Same flow, but the workspace is cloned from a branch that already exists on
/// the remote, and sync runs twice to catch drift on a second round.
#[test]
fn sync_after_commit_on_remote_cloned_branch_keeps_working_copy() {
  let branch = "feature-remote";
  let repo = TestRepo::with_remote().expect("Failed to create test repo with remote");
  repo
    .remote_commit_on_branch(branch, "remote-only.txt", "remote\n", "Remote-only commit")
    .expect("Failed to advance remote branch");
  treq_lib::jj::jj_git_fetch(&repo.repo_path).expect("Failed to fetch");
  let workspace = repo
    .create_workspace_simple(branch)
    .expect("Failed to clone remote branch into workspace");
  let workspace_path = repo.workspace_full_path(&workspace);
  let ws = workspace_path.as_str();

  TestRepo::write_workspace_file(ws, "synced.txt", "content\n").expect("Failed to write file");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "Real commit")
    .expect("Failed to commit");

  let bookmark_after_commit = treq_lib::jj::jj_get_commit_id(ws, branch).expect("bookmark");
  let wc_after_commit = treq_lib::jj::jj_get_commit_id(ws, "@").expect("@");
  assert_ne!(bookmark_after_commit, wc_after_commit);

  for round in 0..2 {
    treq_lib::core::pull_workspace_from_remote(&repo.repo_path, Some(workspace.id), "git")
      .expect("Pull should succeed");
    treq_lib::core::push_workspace_to_remote(&repo.repo_path, Some(workspace.id))
      .expect("Push should succeed");

    let bookmark = treq_lib::jj::jj_get_commit_id(ws, branch).expect("bookmark");
    let wc = treq_lib::jj::jj_get_commit_id(ws, "@").expect("@");
    let wc_parent = treq_lib::jj::jj_get_commit_id(ws, "@-").expect("@-");
    assert_eq!(
      bookmark, bookmark_after_commit,
      "round {round}: bookmark moved"
    );
    assert_eq!(
      wc, wc_after_commit,
      "round {round}: working-copy commit replaced"
    );
    assert_eq!(
      wc_parent, bookmark,
      "round {round}: @ not a child of bookmark"
    );

    let remote_log = TestRepo::run_git(
      &repo.repo_path,
      &[
        "log",
        "--format=%s",
        "-1",
        &format!("refs/remotes/origin/{}", branch),
      ],
    )
    .expect("Failed to read remote branch log");
    assert_eq!(
      remote_log.trim(),
      "Real commit",
      "round {round}: remote tip wrong"
    );
  }
}

/// Commit, then sync twice; the bookmark, `@`, and remote tip must not drift.
fn assert_sync_keeps_commit(repo: &TestRepo, workspace: &treq_lib::local_db::Workspace) {
  let branch = workspace.branch_name.as_str();
  let workspace_path = repo.workspace_full_path(workspace);
  let ws = workspace_path.as_str();

  TestRepo::write_workspace_file(ws, "synced.txt", "content\n").expect("Failed to write file");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "Real commit")
    .expect("Failed to commit");
  let bookmark_after_commit = treq_lib::jj::jj_get_commit_id(ws, branch).expect("bookmark");
  let wc_after_commit = treq_lib::jj::jj_get_commit_id(ws, "@").expect("@");
  assert_ne!(bookmark_after_commit, wc_after_commit);

  for round in 0..2 {
    treq_lib::core::pull_workspace_from_remote(&repo.repo_path, Some(workspace.id), "git")
      .expect("Pull should succeed");
    treq_lib::core::push_workspace_to_remote(&repo.repo_path, Some(workspace.id))
      .expect("Push should succeed");

    let bookmark = treq_lib::jj::jj_get_commit_id(ws, branch).expect("bookmark");
    let wc = treq_lib::jj::jj_get_commit_id(ws, "@").expect("@");
    let wc_parent = treq_lib::jj::jj_get_commit_id(ws, "@-").expect("@-");
    assert_eq!(
      bookmark, bookmark_after_commit,
      "round {round}: bookmark moved"
    );
    assert_eq!(
      wc, wc_after_commit,
      "round {round}: working-copy commit replaced"
    );
    assert_eq!(
      wc_parent, bookmark,
      "round {round}: @ not a child of bookmark"
    );
    let remote_log = TestRepo::run_git(
      &repo.repo_path,
      &[
        "log",
        "--format=%s",
        "-1",
        &format!("refs/remotes/origin/{}", branch),
      ],
    )
    .expect("Failed to read remote branch log");
    assert_eq!(
      remote_log.trim(),
      "Real commit",
      "round {round}: remote tip wrong"
    );
  }
}

/// Mirrors `useCreateStackedWorkspace`: create (no source for the default
/// branch), then retarget via `update_workspace`.
fn stack_workspace(
  repo: &TestRepo,
  branch: &str,
  parent: Option<&str>,
) -> treq_lib::local_db::Workspace {
  let ws =
    treq_lib::core::create_workspace(&repo.repo_path, branch, None, None, parent, None, None)
      .expect("Failed to stack workspace");
  let target = parent.unwrap_or(repo.default_branch()).to_string();
  treq_lib::core::update_workspace(
    &repo.repo_path,
    ws.id,
    treq_lib::core::MaybeEmptyParam::Some(target),
    treq_lib::core::MaybeEmptyParam::Omitted,
  )
  .expect("Failed to set target branch")
}

#[test]
fn sync_after_commit_on_remote_branch_stacked_on_default() {
  let repo = TestRepo::with_remote().expect("Failed to create test repo with remote");
  repo
    .remote_commit_on_branch(
      "feature-remote",
      "remote-only.txt",
      "remote\n",
      "Remote-only commit",
    )
    .expect("Failed to advance remote branch");
  treq_lib::jj::jj_git_fetch(&repo.repo_path).expect("Failed to fetch");
  let workspace = stack_workspace(&repo, "feature-remote", None);
  assert_sync_keeps_commit(&repo, &workspace);
}

#[test]
fn sync_after_commit_on_branch_stacked_on_parent_workspace() {
  let repo = TestRepo::with_remote().expect("Failed to create test repo with remote");
  let parent = stack_workspace(&repo, "feat/parent", None);
  let parent_path = repo.workspace_full_path(&parent);
  TestRepo::write_workspace_file(&parent_path, "parent.txt", "parent\n").expect("write");
  treq_lib::core::commit_workspace(&repo.repo_path, parent.id, "Parent commit").expect("commit");
  treq_lib::core::push_workspace_to_remote(&repo.repo_path, Some(parent.id)).expect("push parent");

  let child = stack_workspace(&repo, "feat/child", Some("feat/parent"));
  assert_sync_keeps_commit(&repo, &child);
}
