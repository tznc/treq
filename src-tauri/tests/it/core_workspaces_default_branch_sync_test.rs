use crate::common::{JjVerifier, TestRepo};
use std::path::Path;
use treq_lib::core::MergeCommit;

const PROP_BRANCH: &str = "feat/prop-sync";

fn workspace_path(repo_path: &str, workspace_name: &str) -> String {
  Path::new(repo_path)
    .join(".treq")
    .join("workspaces")
    .join(workspace_name)
    .to_string_lossy()
    .to_string()
}

fn resolve_bookmark_tip_ids(cwd: &str, branch: &str) -> Vec<String> {
  let _ = TestRepo::jj_update_stale(cwd);
  let revset = format!("bookmarks(exact:{})", branch);
  let mut ids = TestRepo::jj_commit_ids_in_revset(cwd, &revset)
    .unwrap_or_else(|e| panic!("Failed to resolve bookmark tips for '{branch}' in '{cwd}': {e}"));
  ids.sort();
  ids
}

fn read_file(base: &str, relative_path: &str) -> String {
  std::fs::read_to_string(Path::new(base).join(relative_path))
    .unwrap_or_else(|e| panic!("Failed to read file '{relative_path}' at '{base}': {e}"))
}

#[test]
fn home_commit_on_branch_syncs_matching_workspace_branch() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  TestRepo::run_git(&repo.repo_path, &["checkout", "-b", PROP_BRANCH])
    .expect("Failed to create branch");

  let branch_workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    PROP_BRANCH,
    Some("prop sync workspace".to_string()),
    None,
    Some(PROP_BRANCH),
    None,
    None,
  )
  .expect("Failed to create branch workspace");
  TestRepo::run_git(&repo.repo_path, &["checkout", PROP_BRANCH])
    .expect("Failed to checkout branch after workspace creation");
  let branch_workspace_root = workspace_path(&repo.repo_path, &branch_workspace.workspace_path);

  TestRepo::write_workspace_file(
    &repo.repo_path,
    "home-branch-sync.txt",
    "home commit content\n",
  )
  .expect("Failed to write file in home repo");

  treq_lib::core::commit_workspace(&repo.repo_path, Option::<i64>::None, "home branch commit")
    .expect("Home commit should succeed");

  let home_tip = resolve_bookmark_tip_ids(&repo.repo_path, PROP_BRANCH);
  let workspace_tip = resolve_bookmark_tip_ids(&branch_workspace_root, PROP_BRANCH);
  assert_eq!(
    home_tip, workspace_tip,
    "home and workspace branch tips should match after home commit"
  );
}

#[test]
fn workspace_commit_on_branch_syncs_home_when_home_on_same_branch() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  TestRepo::run_git(&repo.repo_path, &["checkout", "-b", PROP_BRANCH])
    .expect("Failed to create branch");

  let branch_workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    PROP_BRANCH,
    Some("prop sync workspace".to_string()),
    None,
    Some(PROP_BRANCH),
    None,
    None,
  )
  .expect("Failed to create branch workspace");
  TestRepo::run_git(&repo.repo_path, &["checkout", PROP_BRANCH])
    .expect("Failed to checkout branch after workspace creation");
  let branch_workspace_root = workspace_path(&repo.repo_path, &branch_workspace.workspace_path);

  TestRepo::write_workspace_file(
    &branch_workspace_root,
    "workspace-branch-commit.txt",
    "workspace branch commit content\n",
  )
  .expect("Failed to write workspace file");

  treq_lib::core::commit_workspace(
    &repo.repo_path,
    branch_workspace.id,
    "workspace branch commit",
  )
  .expect("Workspace commit should succeed");

  let home_tip = resolve_bookmark_tip_ids(&repo.repo_path, PROP_BRANCH);
  let workspace_tip = resolve_bookmark_tip_ids(&branch_workspace_root, PROP_BRANCH);
  assert_eq!(
    home_tip, workspace_tip,
    "home and workspace branch tips should match after workspace commit"
  );
}

#[test]
fn home_pull_on_branch_syncs_matching_workspace_branch() {
  let repo = TestRepo::with_remote().expect("Failed to create test repo with remote");
  TestRepo::run_git(&repo.repo_path, &["checkout", "-b", PROP_BRANCH])
    .expect("Failed to create branch");
  repo
    .push_branch(PROP_BRANCH)
    .expect("Failed to push branch to remote");

  let branch_workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    PROP_BRANCH,
    Some("prop sync workspace".to_string()),
    None,
    Some(PROP_BRANCH),
    None,
    None,
  )
  .expect("Failed to create branch workspace");
  TestRepo::run_git(&repo.repo_path, &["checkout", PROP_BRANCH])
    .expect("Failed to checkout branch after workspace creation");
  let branch_workspace_root = workspace_path(&repo.repo_path, &branch_workspace.workspace_path);

  repo
    .remote_commit_on_branch(
      PROP_BRANCH,
      "remote-branch-sync.txt",
      "remote branch update\n",
      "Remote branch update",
    )
    .expect("Failed to create remote commit on branch");

  let pull_result = treq_lib::core::pull_workspace_from_remote(&repo.repo_path, None, "git")
    .expect("Home pull should succeed");
  assert!(pull_result.success, "Home pull should report success");

  let home_tip = resolve_bookmark_tip_ids(&repo.repo_path, PROP_BRANCH);
  let workspace_tip = resolve_bookmark_tip_ids(&branch_workspace_root, PROP_BRANCH);
  assert_eq!(
    home_tip, workspace_tip,
    "home and workspace branch tips should match after home pull"
  );
}

#[test]
fn workspace_pull_on_branch_syncs_home_when_home_on_same_branch() {
  let repo = TestRepo::with_remote().expect("Failed to create test repo with remote");
  TestRepo::run_git(&repo.repo_path, &["checkout", "-b", PROP_BRANCH])
    .expect("Failed to create branch");
  repo
    .push_branch(PROP_BRANCH)
    .expect("Failed to push branch to remote");

  let branch_workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    PROP_BRANCH,
    Some("prop sync workspace".to_string()),
    None,
    Some(PROP_BRANCH),
    None,
    None,
  )
  .expect("Failed to create branch workspace");
  TestRepo::run_git(&repo.repo_path, &["checkout", PROP_BRANCH])
    .expect("Failed to checkout branch after workspace creation");

  repo
    .remote_commit_on_branch(
      PROP_BRANCH,
      "workspace-pull-sync.txt",
      "workspace pull sync content\n",
      "Remote update for workspace pull",
    )
    .expect("Failed to create remote commit on branch");

  let pull_result =
    treq_lib::core::pull_workspace_from_remote(&repo.repo_path, Some(branch_workspace.id), "git")
      .expect("Workspace pull should succeed");
  assert!(pull_result.success, "Workspace pull should report success");

  let branch_workspace_root = workspace_path(&repo.repo_path, &branch_workspace.workspace_path);
  let home_tip = resolve_bookmark_tip_ids(&repo.repo_path, PROP_BRANCH);
  let workspace_tip = resolve_bookmark_tip_ids(&branch_workspace_root, PROP_BRANCH);
  assert_eq!(
    home_tip, workspace_tip,
    "home and workspace branch tips should match after workspace pull"
  );
}

#[test]
fn workspace_merge_same_branch_syncs_home_branch_tip_and_working_copy() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  TestRepo::run_git(&repo.repo_path, &["checkout", "-b", PROP_BRANCH])
    .expect("Failed to create branch");

  let branch_workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    PROP_BRANCH,
    Some("branch workspace".to_string()),
    None,
    Some(PROP_BRANCH),
    None,
    None,
  )
  .expect("Failed to create branch workspace");
  treq_lib::local_db::update_workspace_target_branch(
    &repo.repo_path,
    branch_workspace.id,
    PROP_BRANCH,
  )
  .expect("Failed to set branch workspace target branch");

  let feature_workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/merge-to-prop-sync",
    Some("feature merge target prop branch".to_string()),
    None,
    Some(PROP_BRANCH),
    None,
    None,
  )
  .expect("Failed to create feature workspace");
  let feature_workspace_root = workspace_path(&repo.repo_path, &feature_workspace.workspace_path);

  TestRepo::write_workspace_file(
    &feature_workspace_root,
    "merged-into-prop-sync.txt",
    "merged branch content\n",
  )
  .expect("Failed to write feature workspace file");
  treq_lib::core::commit_workspace(&repo.repo_path, feature_workspace.id, "feature commit")
    .expect("Failed to commit in feature workspace");

  treq_lib::core::merge_workspace(
    &repo.repo_path,
    feature_workspace.id,
    "Merge feature into prop branch",
    MergeCommit::Merge,
  )
  .expect("Failed to merge feature workspace");

  let branch_workspace_root = workspace_path(&repo.repo_path, &branch_workspace.workspace_path);
  let home_tip = resolve_bookmark_tip_ids(&repo.repo_path, PROP_BRANCH);
  let workspace_tip = resolve_bookmark_tip_ids(&branch_workspace_root, PROP_BRANCH);
  assert_eq!(
    home_tip, workspace_tip,
    "home and branch-workspace bookmark tips should match after merge"
  );

  let merged_content = read_file(&repo.repo_path, "merged-into-prop-sync.txt");
  assert_eq!(
    merged_content, "merged branch content\n",
    "home working copy should reflect merged content after workspace update"
  );
}

#[test]
fn no_sync_when_home_on_different_branch() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let default_branch = repo.default_branch();

  treq_lib::core::create_workspace(
    &repo.repo_path,
    PROP_BRANCH,
    Some("branch workspace".to_string()),
    None,
    Some(default_branch),
    None,
    None,
  )
  .expect("Failed to create branch workspace");

  let branch_workspace = treq_lib::local_db::get_workspace_by_branch(&repo.repo_path, PROP_BRANCH)
    .expect("workspace lookup should succeed")
    .expect("workspace should exist");
  let branch_workspace_root = workspace_path(&repo.repo_path, &branch_workspace.workspace_path);

  TestRepo::write_workspace_file(
    &branch_workspace_root,
    "should-not-sync.txt",
    "workspace-only-content\n",
  )
  .expect("Failed to write workspace file");
  treq_lib::core::commit_workspace(
    &repo.repo_path,
    branch_workspace.id,
    "workspace commit while home on different branch",
  )
  .expect("Workspace commit should succeed");

  assert!(
    !Path::new(&repo.repo_path)
      .join("should-not-sync.txt")
      .exists(),
    "home working copy should not be force-synced when home is on a different branch"
  );
}

#[test]
fn dirty_workspace_main_holds_its_children_back() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let default_branch = repo.default_branch();

  let main_workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    default_branch,
    Some("root main workspace".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create main workspace");
  treq_lib::local_db::update_workspace_target_branch(
    &repo.repo_path,
    main_workspace.id,
    default_branch,
  )
  .expect("Failed to set main workspace target branch");

  let main_workspace_root = workspace_path(&repo.repo_path, default_branch);
  let shared_file = "shared-precedence.txt";

  TestRepo::write_workspace_file(
    &main_workspace_root,
    shared_file,
    "workspace-main-version\n",
  )
  .expect("Failed to write uncommitted workspace-main version");

  let child_workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/main-child",
    Some("child of main".to_string()),
    None,
    Some(default_branch),
    None,
    None,
  )
  .expect("Failed to create child workspace");
  let child_workspace_root = workspace_path(&repo.repo_path, &child_workspace.workspace_path);

  TestRepo::write_workspace_file(&repo.repo_path, shared_file, "home-main-version\n")
    .expect("Failed to write competing home version");
  treq_lib::core::commit_workspace(
    &repo.repo_path,
    Option::<i64>::None,
    "home main competing change",
  )
  .expect("Failed to commit home competing change");

  TestRepo::write_workspace_file(&child_workspace_root, "child-change.txt", "child commit\n")
    .expect("Failed to write child change");
  treq_lib::core::commit_workspace(&repo.repo_path, child_workspace.id, "child commit")
    .expect("Failed to commit child change");

  let rebase_result = treq_lib::core::check_and_rebase_workspaces(
    &repo.repo_path,
    Some(child_workspace.id),
    Some(default_branch.to_string()),
    Some(true),
    "git",
  )
  .expect("check_and_rebase_workspaces(force=true) should succeed");

  // A dirty parent holds its children back: its uncommitted changes never
  // reach them.
  assert!(!rebase_result.success, "{}", rebase_result.message);
  assert_eq!(
    treq_lib::core::rebase_guard::hold(&repo.repo_path, child_workspace.id),
    Some(treq_lib::core::rebase_guard::RebaseHold::ParentDirty {
      branch: default_branch.to_string()
    })
  );
  assert!(
    !Path::new(&child_workspace_root).join(shared_file).exists(),
    "child workspace must not pick up workspace-main's uncommitted version"
  );

  let workspace_main_content = read_file(&main_workspace_root, shared_file);
  assert_eq!(
    workspace_main_content, "workspace-main-version\n",
    "main workspace should retain uncommitted version"
  );

  let _ = main_workspace;
}

/// After a home pull that advances the home branch tip, the matching workspace's working
/// copy (@) must also be moved onto the new tip.  Without the fix, `HomeToWorkspace` sync
/// only updated the workspace bookmark but left @ behind, making
/// `jj_working_copy_needs_sync` permanently true and triggering the infinite
/// `[rebase_single_workspace::after_rebase]` log loop on every subsequent rebase call.
#[test]
fn home_pull_syncs_workspace_working_copy_to_new_tip() {
  let repo = TestRepo::with_remote().expect("Failed to create test repo with remote");
  TestRepo::run_git(&repo.repo_path, &["checkout", "-b", PROP_BRANCH])
    .expect("Failed to create branch");
  repo
    .push_branch(PROP_BRANCH)
    .expect("Failed to push branch to remote");

  let branch_workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    PROP_BRANCH,
    Some("sync wc workspace".to_string()),
    None,
    Some(PROP_BRANCH),
    None,
    None,
  )
  .expect("Failed to create branch workspace");
  TestRepo::run_git(&repo.repo_path, &["checkout", PROP_BRANCH])
    .expect("Failed to checkout branch after workspace creation");

  let branch_workspace_root = workspace_path(&repo.repo_path, &branch_workspace.workspace_path);

  // Advance the remote branch so a pull actually moves the bookmark.
  repo
    .remote_commit_on_branch(
      PROP_BRANCH,
      "home-pull-wc-sync.txt",
      "remote content\n",
      "Remote advance",
    )
    .expect("Failed to create remote commit");

  // Pull through the home repo path (workspace_id = None).
  let pull_result = treq_lib::core::pull_workspace_from_remote(&repo.repo_path, None, "git")
    .expect("Home pull should succeed");
  assert!(
    pull_result.success,
    "Home pull should report success: {}",
    pull_result.message
  );

  // The workspace working copy (@) should now sit directly on the branch
  // bookmark tip, which then needs no further sync.
  let wc_parent = TestRepo::jj_commit_ids_in_revset(&branch_workspace_root, "@-")
    .expect("Failed to resolve workspace @- commit id")
    .into_iter()
    .next()
    .expect("@- should resolve to exactly one commit");
  let bookmark_ids = resolve_bookmark_tip_ids(&branch_workspace_root, PROP_BRANCH);
  assert_eq!(
    bookmark_ids,
    vec![wc_parent],
    "workspace @ should sit on the branch bookmark after home pull (HomeToWorkspace sync)"
  );
  assert!(
    !treq_lib::jj::jj_working_copy_needs_sync(&branch_workspace_root, PROP_BRANCH).unwrap(),
    "a synced working copy must not ask for another sync"
  );
}

#[test]
fn home_commit_moves_branch_but_keeps_dirty_workspace_working_copy() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  TestRepo::run_git(&repo.repo_path, &["checkout", "-b", PROP_BRANCH])
    .expect("Failed to create branch");
  let branch_workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    PROP_BRANCH,
    None,
    None,
    Some(PROP_BRANCH),
    None,
    None,
  )
  .expect("Failed to create branch workspace");
  TestRepo::run_git(&repo.repo_path, &["checkout", PROP_BRANCH])
    .expect("Failed to checkout branch after workspace creation");
  let workspace_root = workspace_path(&repo.repo_path, &branch_workspace.workspace_path);

  TestRepo::write_workspace_file(&workspace_root, "dirty.txt", "dirty workspace content\n")
    .expect("Failed to dirty the workspace");

  TestRepo::write_workspace_file(&repo.repo_path, "home-dirty-safe-sync.txt", "home commit\n")
    .expect("Failed to write file in home repo");
  treq_lib::core::commit_workspace(&repo.repo_path, Option::<i64>::None, "home commit")
    .expect("Home commit should succeed");

  assert_eq!(
    resolve_bookmark_tip_ids(&workspace_root, PROP_BRANCH),
    resolve_bookmark_tip_ids(&repo.repo_path, PROP_BRANCH),
    "workspace branch should follow the home commit"
  );
  // The dirty working copy stays put above the old tip; only a snapshot
  // rewrites it.
  assert_ne!(
    JjVerifier::get_commit_id_for_rev(&workspace_root, "@-").unwrap(),
    JjVerifier::get_commit_id_for_rev(&workspace_root, PROP_BRANCH).unwrap(),
    "a dirty workspace working copy must not be moved onto the new tip"
  );
  assert_eq!(
    read_file(&workspace_root, "dirty.txt"),
    "dirty workspace content\n"
  );
}
