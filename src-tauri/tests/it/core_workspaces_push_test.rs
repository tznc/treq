use crate::common::TestRepo;

fn remote_branch_messages(repo: &TestRepo, branch_name: &str) -> String {
  let remote_path = repo.remote_path();
  let remote_path_str = remote_path.to_str().expect("utf-8 path");
  TestRepo::run_git(
    remote_path_str,
    &["log", "--format=%s", &format!("refs/heads/{branch_name}")],
  )
  .expect("git log on remote failed")
}

#[test]
fn test_push_workspace_excludes_empty_commits_from_remote() {
  let repo = TestRepo::with_remote().expect("Failed to create test repo with remote");
  let workspace = repo
    .setup_workspace_with_pushed_commit("feat/push-no-empty", "first.txt", "hello\n")
    .expect("failed to create and push initial workspace commit");

  let ws_dir = repo.workspaces_dir().join(&workspace.workspace_path);
  let ws_dir_str = ws_dir.to_str().expect("utf-8");

  // Simulate an empty commit left behind in local history (e.g. by a squash/rebase
  // that absorbed its content elsewhere) by describing an unchanged commit directly
  // via jj, bypassing commit_workspace.
  TestRepo::jj_new(ws_dir_str, &["@"]).expect("jj new failed");
  TestRepo::jj_describe(ws_dir_str, "@", "stray empty").expect("jj describe failed");
  TestRepo::jj_new(ws_dir_str, &["@"]).expect("jj new failed");

  TestRepo::write_workspace_file(ws_dir_str, "second.txt", "world\n")
    .expect("failed to write file");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "add second")
    .expect("commit_workspace should succeed");

  treq_lib::core::push_workspace_to_remote(&repo.repo_path, Some(workspace.id))
    .expect("push_workspace_to_remote should succeed");

  let remote_messages = remote_branch_messages(&repo, &workspace.branch_name);
  assert!(
    !remote_messages.contains("stray empty"),
    "expected empty commit to never reach the remote, got:\n{remote_messages}"
  );
  assert!(
    remote_messages.contains("add second"),
    "expected the real commit to reach the remote, got:\n{remote_messages}"
  );

  // The empty commit must also be gone locally, not merely excluded from the push.
  let local_log =
    TestRepo::jj_log_descriptions(ws_dir_str, "::@", Some(20)).expect("jj log failed");
  assert!(
    !local_log.iter().any(|(desc, _)| desc == "stray empty"),
    "expected empty commit to be discarded locally too, got:\n{local_log:?}"
  );
}

#[test]
fn test_push_workspace_excludes_empty_merge_commit_from_remote() {
  let repo = TestRepo::with_remote().expect("Failed to create test repo with remote");
  let workspace = repo
    .setup_workspace_with_pushed_commit("feat/push-no-empty-merge", "shared.txt", "same\n")
    .expect("failed to create and push initial workspace commit");
  let ws_dir = repo.workspaces_dir().join(&workspace.workspace_path);
  let ws_dir_str = ws_dir.to_str().expect("utf-8");

  repo
    .remote_commit_on_branch(
      repo.default_branch(),
      "shared.txt",
      "same\n",
      "advance main with equivalent content",
    )
    .expect("failed to advance main");

  let remote_merge_tip = repo
    .remote_undescribed_merge(&workspace.branch_name, repo.default_branch())
    .expect("merge main into workspace branch remotely");
  treq_lib::jj::jj_git_fetch(&repo.repo_path).expect("fetch main");

  treq_lib::core::push_workspace_to_remote(&repo.repo_path, Some(workspace.id))
    .expect("push_workspace_to_remote should succeed");

  let remote_messages = remote_branch_messages(&repo, &workspace.branch_name);
  let remote_tip = TestRepo::run_git(
    repo.remote_path().to_str().expect("utf-8 remote path"),
    &[
      "rev-parse",
      &format!("refs/heads/{}", workspace.branch_name),
    ],
  )
  .expect("resolve remote workspace tip");
  let local_commit = TestRepo::run_jj_cli(
    ws_dir_str,
    &[
      "log",
      "-r",
      &workspace.branch_name,
      "--no-graph",
      "-T",
      "commit_id",
    ],
  )
  .expect("resolve local workspace tip");
  assert_eq!(remote_tip.trim(), local_commit.trim());
  assert_ne!(remote_tip.trim(), remote_merge_tip.trim());
  assert!(remote_messages.contains("Add shared.txt"));
}

#[test]
fn push_refuses_conflicted_commit() {
  let repo = TestRepo::with_remote().expect("Failed to create test repo with remote");
  let workspace = repo
    .create_workspace_with_commit("feat/push-conflict", "conflict.txt", "workspace\n", None)
    .expect("create workspace with commit");
  let ws_dir = repo.workspace_full_path(&workspace);

  repo
    .create_file("conflict.txt", "main\n")
    .expect("write main conflict.txt");
  treq_lib::jj::jj_commit(&repo.repo_path, "main commit").expect("commit main");
  let rebase = treq_lib::jj::jj_rebase_workspace_bookmark_onto(
    &ws_dir,
    &workspace.branch_name,
    repo.default_branch(),
  )
  .expect("rebase");
  assert!(rebase.success, "rebase failed: {}", rebase.message);

  let err = treq_lib::core::push_workspace_to_remote(&repo.repo_path, Some(workspace.id))
    .expect_err("push of a conflicted commit must fail");
  assert!(err.contains("conflict"), "unexpected error: {err}");

  let remote_path = repo.remote_path();
  let remote_refs = TestRepo::run_git(
    remote_path.to_str().expect("utf-8 path"),
    &["for-each-ref", "--format=%(refname)"],
  )
  .expect("list remote refs");
  assert!(
    !remote_refs.contains(&workspace.branch_name),
    "conflicted branch reached the remote:\n{remote_refs}"
  );
}

#[test]
fn push_home_repo_refuses_detached_head() {
  let repo = TestRepo::with_remote().expect("Failed to create test repo with remote");
  TestRepo::run_git(&repo.repo_path, &["checkout", "--detach"]).expect("detach HEAD");

  let err = treq_lib::core::push_workspace_to_remote(&repo.repo_path, None)
    .expect_err("push with detached HEAD must fail");
  assert!(err.contains("not on a branch"), "unexpected error: {err}");
}
