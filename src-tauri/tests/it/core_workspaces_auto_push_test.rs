use crate::common::TestRepo;

fn remote_branch_messages(repo: &TestRepo, branch_name: &str) -> String {
  let remote_path = repo.remote_path();
  let remote_path_str = remote_path.to_str().expect("utf-8 path");
  TestRepo::run_git(
    remote_path_str,
    &["log", "--format=%s", &format!("refs/heads/{branch_name}")],
  )
  .unwrap_or_default()
}

fn set_auto_push(repo: &TestRepo, enabled: bool) {
  let app_db_path = std::path::Path::new(&repo.repo_path)
    .join(".treq")
    .join("treq.db");
  let app_db = treq_lib::db::Database::new(app_db_path).expect("test app db should open");
  app_db.init().expect("test app db should initialize");
  app_db
    .set_repo_setting(
      &repo.repo_path,
      "auto_push",
      if enabled { "true" } else { "false" },
    )
    .expect("set auto_push setting should succeed");
}

#[test]
fn test_commit_workspace_with_auto_push_disabled_by_default_does_not_push() {
  let repo = TestRepo::with_remote().expect("Failed to create test repo with remote");
  let workspace = repo
    .create_workspace_with_commit("feat/auto-push-default-off", "first.txt", "hello\n", None)
    .expect("failed to create and commit initial workspace commit");

  let ws_dir = repo.workspaces_dir().join(&workspace.workspace_path);
  let ws_dir_str = ws_dir.to_str().expect("utf-8");
  TestRepo::write_workspace_file(ws_dir_str, "second.txt", "world\n")
    .expect("failed to write file");

  treq_lib::core::commit_workspace_with_auto_push(&repo.repo_path, workspace.id, "add second")
    .expect("commit_workspace_with_auto_push should succeed");

  let remote_messages = remote_branch_messages(&repo, &workspace.branch_name);
  assert!(
        !remote_messages.contains("add second"),
        "expected commit to stay local when auto_push is unset (defaults to false), got remote log:\n{remote_messages}"
    );
}

#[test]
fn test_commit_workspace_with_auto_push_disabled_does_not_push() {
  let repo = TestRepo::with_remote().expect("Failed to create test repo with remote");
  let workspace = repo
    .create_workspace_with_commit("feat/auto-push-off", "first.txt", "hello\n", None)
    .expect("failed to create and commit initial workspace commit");
  set_auto_push(&repo, false);

  let ws_dir = repo.workspaces_dir().join(&workspace.workspace_path);
  let ws_dir_str = ws_dir.to_str().expect("utf-8");
  TestRepo::write_workspace_file(ws_dir_str, "second.txt", "world\n")
    .expect("failed to write file");

  treq_lib::core::commit_workspace_with_auto_push(&repo.repo_path, workspace.id, "add second")
    .expect("commit_workspace_with_auto_push should succeed");

  let remote_messages = remote_branch_messages(&repo, &workspace.branch_name);
  assert!(
    !remote_messages.contains("add second"),
    "expected commit to stay local when auto_push is false, got remote log:\n{remote_messages}"
  );
}

#[test]
fn test_commit_workspace_with_auto_push_enabled_pushes_to_remote() {
  let repo = TestRepo::with_remote().expect("Failed to create test repo with remote");
  let workspace = repo
    .create_workspace_with_commit("feat/auto-push-on", "first.txt", "hello\n", None)
    .expect("failed to create and commit initial workspace commit");
  set_auto_push(&repo, true);

  let ws_dir = repo.workspaces_dir().join(&workspace.workspace_path);
  let ws_dir_str = ws_dir.to_str().expect("utf-8");
  TestRepo::write_workspace_file(ws_dir_str, "second.txt", "world\n")
    .expect("failed to write file");

  treq_lib::core::commit_workspace_with_auto_push(&repo.repo_path, workspace.id, "add second")
    .expect("commit_workspace_with_auto_push should succeed");

  let remote_messages = remote_branch_messages(&repo, &workspace.branch_name);
  assert!(
        remote_messages.contains("add second"),
        "expected commit to reach the remote when auto_push is true, got remote log:\n{remote_messages}"
    );
}
