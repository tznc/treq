//! A workspace's bookmark only moves on commit: it never sits on the working
//! copy, so uncommitted edits never move it (or the children stacked on it).
use crate::common::TestRepo;
use treq_lib::jj;

fn assert_bookmark_below_working_copy(workspace_path: &str, branch: &str) {
  let at = jj::jj_get_commit_id(workspace_path, "@").unwrap();
  let parent = jj::jj_get_commit_id(workspace_path, "@-").unwrap();
  let bookmark = jj::jj_get_commit_id(workspace_path, branch).unwrap();
  assert_ne!(bookmark, at, "'{branch}' sits on the working copy");
  assert_eq!(
    bookmark, parent,
    "'{branch}' is not the working copy's parent"
  );
}

#[test]
fn new_workspace_keeps_bookmark_below_working_copy() {
  let repo = TestRepo::new().unwrap();
  let ws = repo.create_workspace_simple("feat/fresh").unwrap();
  let path = repo.workspace_full_path(&ws);

  assert_bookmark_below_working_copy(&path, "feat/fresh");
}

#[test]
fn uncommitted_edits_do_not_move_a_fresh_bookmark() {
  let repo = TestRepo::new().unwrap();
  let ws = repo.create_workspace_simple("feat/fresh-edit").unwrap();
  let path = repo.workspace_full_path(&ws);
  let before = jj::jj_get_commit_id(&path, "feat/fresh-edit").unwrap();

  TestRepo::write_workspace_file(&path, "wip.txt", "uncommitted").unwrap();
  jj::jj_is_working_copy_empty(&path).unwrap();

  assert_eq!(
    jj::jj_get_commit_id(&path, "feat/fresh-edit").unwrap(),
    before
  );
}

#[test]
fn commit_moves_bookmark_to_the_new_commit() {
  let repo = TestRepo::new().unwrap();
  let ws = repo
    .create_workspace_with_commit("feat/committed", "a.txt", "a", None)
    .unwrap();
  let path = repo.workspace_full_path(&ws);

  assert_bookmark_below_working_copy(&path, "feat/committed");
  assert!(TestRepo::jj_files_at_revision(&path, "feat/committed")
    .unwrap()
    .iter()
    .any(|f| f == "a.txt"));
}

#[test]
fn sync_to_a_moved_bookmark_checks_out_above_it() {
  let repo = TestRepo::new().unwrap();
  let ws = repo
    .create_workspace_with_commit("feat/synced", "a.txt", "a", None)
    .unwrap();
  let path = repo.workspace_full_path(&ws);
  // The bookmark moves on without the working copy (another checkout committed).
  let other = repo
    .create_workspace_with_commit("feat/synced-next", "b.txt", "b", Some("feat/synced"))
    .unwrap();
  let other_path = repo.workspace_full_path(&other);
  jj::jj_set_bookmark(&other_path, "feat/synced", "feat/synced-next").unwrap();

  assert!(jj::jj_sync_working_copy_if_safe(&path, "feat/synced").unwrap());

  assert_bookmark_below_working_copy(&path, "feat/synced");
}

#[test]
fn child_without_commits_follows_its_parent_commit() {
  let repo = TestRepo::new().unwrap();
  let parent = repo
    .create_workspace_with_commit("feat/ff-parent", "a.txt", "a", None)
    .unwrap();
  let child = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/ff-child",
    None,
    None,
    Some("feat/ff-parent"),
    None,
    None,
  )
  .unwrap();
  let parent_path = repo.workspace_full_path(&parent);
  let child_path = repo.workspace_full_path(&child);

  TestRepo::write_workspace_file(&parent_path, "b.txt", "b").unwrap();
  treq_lib::core::commit_workspace(&repo.repo_path, parent.id, "parent moves on").unwrap();

  assert_eq!(
    jj::jj_get_commit_id(&child_path, "feat/ff-child").unwrap(),
    jj::jj_get_commit_id(&child_path, "feat/ff-parent").unwrap()
  );
  assert_bookmark_below_working_copy(&child_path, "feat/ff-child");
  assert!(std::path::Path::new(&child_path).join("b.txt").exists());
}

#[test]
fn moving_a_branch_without_commits_leaves_other_workspaces_alone() {
  let repo = TestRepo::new().unwrap();
  let default_branch = repo.default_branch().to_string();
  let fresh = repo.create_workspace_simple("feat/fresh-move").unwrap();
  let other = repo
    .create_workspace_with_commit("feat/bystander", "x.txt", "x", None)
    .unwrap();
  let fresh_path = repo.workspace_full_path(&fresh);
  let other_path = repo.workspace_full_path(&other);
  let other_before = jj::jj_get_commit_id(&other_path, "feat/bystander").unwrap();
  repo
    .commit_file("advance.txt", "advance", "advance main")
    .unwrap();

  jj::jj_rebase_workspace_bookmark_onto(&fresh_path, "feat/fresh-move", &default_branch).unwrap();

  assert_eq!(
    jj::jj_get_commit_id(&other_path, "feat/bystander").unwrap(),
    other_before
  );
  assert_bookmark_below_working_copy(&fresh_path, "feat/fresh-move");
}
