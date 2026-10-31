use crate::common::TestRepo;
use treq_lib::core::rebase_guard::{self, RebaseHold};
use treq_lib::jj;

/// A workspace targeting the default branch with one committed file.
fn workspace_on_default(repo: &TestRepo, branch: &str) -> (treq_lib::local_db::Workspace, String) {
  let ws = repo
    .create_workspace_with_commit(
      branch,
      &format!("{}.txt", branch.replace('/', "-")),
      "work",
      None,
    )
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
  repo
    .remote_commit_file("advance.txt", "new", "Advance main")
    .unwrap();

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
  repo
    .remote_commit_file("advance.txt", "new", "Advance main")
    .unwrap();

  treq_lib::auto_rebase::rebase_single_workspace(&repo.repo_path, ws.id, "main", true, "diff")
    .unwrap();

  assert!(has_file(&repo.repo_path, &default, "advance.txt"));
  let local = jj::jj_get_commit_id(&repo.repo_path, &default).unwrap();
  let remote = jj::jj_get_commit_id(&repo.repo_path, &format!("{default}@origin")).unwrap();
  assert_eq!(local, remote);
}

#[test]
fn rebase_still_rebases_a_workspace_with_its_own_uncommitted_changes() {
  let repo = TestRepo::with_remote().unwrap();
  let (ws, path) = workspace_on_default(&repo, "feat/own-wip");
  TestRepo::write_workspace_file(&path, "wip.txt", "uncommitted").unwrap();
  repo
    .remote_commit_file("advance.txt", "new", "Advance main")
    .unwrap();

  let result =
    treq_lib::auto_rebase::rebase_single_workspace(&repo.repo_path, ws.id, "main", true, "diff")
      .unwrap();

  assert!(result.is_some());
  assert!(has_file(&path, "feat/own-wip", "advance.txt"));
  assert_eq!(rebase_guard::hold(&repo.repo_path, ws.id), None);
}

/// A parent workspace with one commit, and a child stacked on it.
fn stack(
  repo: &TestRepo,
  parent: &str,
  child: &str,
) -> (String, treq_lib::local_db::Workspace, String) {
  let parent_ws = repo
    .create_workspace_with_commit(parent, "parent.txt", "parent", None)
    .unwrap();
  let child_ws = repo
    .create_workspace_with_commit(child, "child.txt", "child", Some(parent))
    .unwrap();
  treq_lib::local_db::update_workspace_target_branch(&repo.repo_path, child_ws.id, parent).unwrap();
  let child_path = repo.workspace_full_path(&child_ws);
  (repo.workspace_full_path(&parent_ws), child_ws, child_path)
}

#[test]
fn rebase_skips_workspace_whose_parent_is_dirty() {
  let repo = TestRepo::with_remote().unwrap();
  let (parent_path, child, _) = stack(&repo, "feat/parent", "feat/child");
  TestRepo::write_workspace_file(&parent_path, "wip.txt", "uncommitted").unwrap();

  let result =
    treq_lib::auto_rebase::rebase_single_workspace(&repo.repo_path, child.id, "main", true, "diff")
      .unwrap();

  assert!(result.is_none());
  assert_eq!(
    rebase_guard::hold(&repo.repo_path, child.id),
    Some(RebaseHold::ParentDirty {
      branch: "feat/parent".into()
    })
  );
}

#[test]
fn batch_rebase_skips_children_of_a_dirty_parent_only() {
  let repo = TestRepo::with_remote().unwrap();
  let (parent_path, child, _) = stack(&repo, "feat/batch-parent", "feat/batch-child");
  let (own_wip, own_wip_path) = workspace_on_default(&repo, "feat/batch-own-wip");
  TestRepo::write_workspace_file(&parent_path, "wip.txt", "uncommitted").unwrap();
  TestRepo::write_workspace_file(&own_wip_path, "wip.txt", "uncommitted").unwrap();
  repo
    .remote_commit_file("advance.txt", "new", "Advance main")
    .unwrap();

  treq_lib::auto_rebase::check_and_rebase_all(&repo.repo_path, "diff").unwrap();

  assert!(has_file(&own_wip_path, "feat/batch-own-wip", "advance.txt"));
  assert_eq!(rebase_guard::hold(&repo.repo_path, own_wip.id), None);
  assert_eq!(
    rebase_guard::hold(&repo.repo_path, child.id),
    Some(RebaseHold::ParentDirty {
      branch: "feat/batch-parent".into()
    })
  );
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
  repo
    .remote_commit_file("advance.txt", "new", "Advance main")
    .unwrap();

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
  repo
    .remote_commit_file("advance.txt", "new", "Advance main")
    .unwrap();
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

#[test]
fn rebase_hold_is_stored_with_the_workspace() {
  // The CLI's post-commit rebase records holds the app must see.
  let repo = TestRepo::with_remote().unwrap();
  let (parent_path, child, _) = stack(&repo, "feat/stored-parent", "feat/stored-child");
  TestRepo::write_workspace_file(&parent_path, "wip.txt", "uncommitted").unwrap();

  treq_lib::auto_rebase::rebase_single_workspace(&repo.repo_path, child.id, "main", true, "diff")
    .unwrap();

  let stored = treq_lib::local_db::get_workspace_by_id(&repo.repo_path, child.id)
    .unwrap()
    .unwrap();
  assert_eq!(
    treq_lib::local_db::rebase_hold_of(&stored),
    Some(serde_json::json!({"ParentDirty": {"branch": "feat/stored-parent"}}))
  );
}

/// Snapshots the parent's working copy, as any read of that workspace does.
fn parent_wip_reaches_child(parent_path: &str, child_path: &str, child: &str) -> bool {
  TestRepo::write_workspace_file(parent_path, "wip.txt", "uncommitted").unwrap();
  jj::jj_is_working_copy_empty(parent_path).unwrap();
  has_file(child_path, child, "wip.txt")
}

#[test]
fn stacked_workspace_is_created_on_the_parent_bookmark_not_its_working_copy() {
  let repo = TestRepo::with_remote().unwrap();
  let (parent_path, _, child_path) = stack(&repo, "feat/new-parent", "feat/new-child");

  assert!(!parent_wip_reaches_child(
    &parent_path,
    &child_path,
    "feat/new-child"
  ));
}

/// A stack made before children were created on the parent's bookmark: the
/// child sits on the parent's working copy.
fn old_stack(repo: &TestRepo) -> (String, treq_lib::local_db::Workspace, String) {
  let (parent_path, child, child_path) = stack(repo, "feat/old-parent", "feat/old-child");
  // A stack made before children were created on the parent's bookmark.
  jj::jj_set_bookmark(&parent_path, "old-parent-wc", "@").unwrap();
  jj::jj_rebase_with_revset(
    &child_path,
    "roots(feat/old-parent..feat/old-child)",
    "old-parent-wc",
    "feat/old-child",
    "diff",
  )
  .unwrap();
  jj::jj_delete_bookmark(&parent_path, "old-parent-wc").unwrap();
  let parent_wc = jj::jj_get_commit_id(&parent_path, "@").unwrap();
  assert_eq!(
    jj::jj_log_revset_commit_ids(&child_path, &format!("{parent_wc} & ::feat/old-child"))
      .unwrap()
      .len(),
    1,
    "setup: the child sits on the parent's working copy"
  );
  (parent_path, child, child_path)
}

#[test]
fn rebase_moves_a_child_off_its_parent_working_copy() {
  let repo = TestRepo::with_remote().unwrap();
  let (parent_path, child, child_path) = old_stack(&repo);

  treq_lib::auto_rebase::rebase_single_workspace(&repo.repo_path, child.id, "main", true, "diff")
    .unwrap();

  assert!(!parent_wip_reaches_child(
    &parent_path,
    &child_path,
    "feat/old-child"
  ));
  assert!(has_file(&child_path, "feat/old-child", "child.txt"));
}

#[test]
fn force_rebase_moves_a_child_off_its_parent_working_copy() {
  let repo = TestRepo::with_remote().unwrap();
  let (parent_path, child, child_path) = old_stack(&repo);

  treq_lib::auto_rebase::rebase_root_subtree_from_workspace_force(
    &repo.repo_path,
    child.id,
    "main",
    "diff",
  )
  .unwrap();

  assert!(!parent_wip_reaches_child(
    &parent_path,
    &child_path,
    "feat/old-child"
  ));
  assert!(has_file(&child_path, "feat/old-child", "child.txt"));
}

#[test]
fn force_rebase_moves_a_workspace_without_a_stack_onto_its_target() {
  let repo = TestRepo::with_remote().unwrap();
  let (ws, path) = workspace_on_default(&repo, "feat/manual");
  repo
    .remote_commit_file("advance.txt", "new", "Advance main")
    .unwrap();

  let result = treq_lib::core::check_and_rebase_workspaces(
    &repo.repo_path,
    Some(ws.id),
    None,
    Some(true),
    "diff",
  )
  .unwrap();

  assert!(
    has_file(&path, "feat/manual", "advance.txt"),
    "{}",
    result.message
  );
}

#[test]
fn rebase_records_stale_target_when_fetch_fails_and_workspace_is_up_to_date() {
  let repo = TestRepo::with_remote().unwrap();
  let (ws, _) = workspace_on_default(&repo, "feat/offline");
  TestRepo::run_git(
    &repo.repo_path,
    &["remote", "set-url", "origin", "/nonexistent/remote.git"],
  )
  .unwrap();

  treq_lib::auto_rebase::rebase_single_workspace(&repo.repo_path, ws.id, "main", false, "diff")
    .unwrap();

  assert!(matches!(
    rebase_guard::hold(&repo.repo_path, ws.id),
    Some(RebaseHold::StaleTarget { .. })
  ));
}
