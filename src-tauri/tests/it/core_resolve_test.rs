//! Tests for commit-level conflict resolution via short-lived resolve workspaces.

use std::fs;
use std::path::Path;

use treq_lib::core;
use treq_lib::jj;

use crate::common::TestRepo;

fn workspace_full_path(repo: &TestRepo, workspace: &treq_lib::local_db::Workspace) -> String {
  repo
    .workspaces_dir()
    .join(&workspace.workspace_path)
    .to_str()
    .unwrap()
    .to_string()
}

fn setup_rebase_conflict(repo: &TestRepo) -> treq_lib::local_db::Workspace {
  let default_branch = repo.default_branch();
  let workspace = core::create_workspace(
    &repo.repo_path,
    "feat/resolve-core",
    Some("resolve core".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("create workspace");
  let workspace_path = workspace_full_path(repo, &workspace);

  TestRepo::write_workspace_file(&workspace_path, "conflict.txt", "workspace version\n")
    .expect("write workspace conflict.txt");
  core::commit_workspace(&repo.repo_path, workspace.id, "workspace commit")
    .expect("commit workspace");

  repo
    .create_file("conflict.txt", "main version\n")
    .expect("write main conflict.txt");
  jj::jj_commit(&repo.repo_path, "main commit").expect("commit main");

  let rebase =
    jj::jj_rebase_workspace_bookmark_onto(&workspace_path, &workspace.branch_name, default_branch)
      .expect("rebase");
  assert!(
    rebase.success,
    "rebase should record conflict: {}",
    rebase.message
  );

  let status = core::workspace_status(&repo.repo_path, Some(workspace.id)).expect("status");
  assert!(
    status.partial.has_conflicts,
    "expected conflicts after rebase"
  );
  assert!(
    status.conflicted_files.iter().any(|f| f == "conflict.txt"),
    "expected conflict.txt, got {:?}",
    status.conflicted_files
  );

  workspace
}

#[test]
fn start_resolve_creates_edit_mode_workspace_without_touching_product_wc() {
  let repo = TestRepo::new().expect("test repo");
  let workspace = setup_rebase_conflict(&repo);
  let product_path = workspace_full_path(&repo, &workspace);

  // Dirty the product WC so we can prove resolve does not touch it.
  TestRepo::write_workspace_file(&product_path, "unrelated.txt", "keep me\n")
    .expect("dirty product wc");

  let log = core::list_commits(&repo.repo_path, Some(workspace.id), false, None, None)
    .expect("list commits");
  assert!(
    log
      .commits
      .iter()
      .any(|c| c.has_conflicts && !c.on_target_only),
    "expected at least one conflicted commit in log, got {:?}",
    log
      .commits
      .iter()
      .map(|c| (&c.change_id, c.has_conflicts, &c.description))
      .collect::<Vec<_>>()
  );

  let session = core::start_resolve_conflicts(
    &repo.repo_path,
    Some(workspace.id),
    None, // all conflicted commits
  )
  .expect("start_resolve_conflicts");

  assert!(!session.targets.is_empty(), "expected resolve targets");
  let target = &session.targets[0];
  assert!(
    Path::new(&target.resolve_path)
      .join("conflict.txt")
      .exists(),
    "conflict.txt should be materialized in resolve workspace"
  );

  let slug = jj::sanitize_workspace_name(&workspace.workspace_path);
  let expected_prefix = Path::new(&repo.repo_path)
    .join(".treq")
    .join("resolve")
    .join(&slug);
  assert_eq!(
    session.agent_cwd,
    expected_prefix.to_string_lossy().to_string(),
    "agent should start in .treq/resolve/<workspace-slug>"
  );
  assert!(
    target.resolve_path.starts_with(&session.agent_cwd),
    "change-id sandbox should live under agent_cwd: {} vs {}",
    target.resolve_path,
    session.agent_cwd
  );
  assert_eq!(
    Path::new(&target.resolve_path)
      .file_name()
      .and_then(|n| n.to_str()),
    Some(target.change_id.as_str()),
    "leaf dir should be the change id"
  );

  for name in ["AGENTS.md", "CLAUDE.md", "README.md"] {
    let path = Path::new(&session.agent_cwd).join(name);
    assert!(path.exists(), "missing instruction file {}", name);
    let body = fs::read_to_string(&path).expect("read instruction file");
    assert!(
      body.contains("treq resolve"),
      "{} should document treq resolve",
      name
    );
  }

  // Product WC dirty file must still exist and not be cleared.
  let unrelated = Path::new(&product_path).join("unrelated.txt");
  assert!(
    unrelated.exists(),
    "product working copy must remain untouched"
  );
  let content = fs::read_to_string(&unrelated).expect("read unrelated");
  assert_eq!(content, "keep me\n");

  // Resolve workspace must not appear as a normal sidebar workspace.
  let listed = core::list_workspaces(&repo.repo_path).expect("list workspaces");
  assert!(
    listed.iter().all(|w| !core::is_resolve_workspace(w)),
    "list_workspaces must hide resolve workspaces"
  );
}

#[test]
fn treq_resolve_side_two_clears_conflict_and_cleans_up_without_extra_commit() {
  let repo = TestRepo::new().expect("test repo");
  let workspace = setup_rebase_conflict(&repo);

  let before = core::list_commits(&repo.repo_path, Some(workspace.id), false, None, None)
    .expect("list before");
  let conflicted = before
    .commits
    .iter()
    .find(|c| c.has_conflicts && !c.on_target_only)
    .expect("conflicted commit")
    .clone();
  let commits_before = before
    .commits
    .iter()
    .filter(|c| !c.on_target_only && !c.is_working_copy)
    .count();

  let session = core::start_resolve_conflicts(
    &repo.repo_path,
    Some(workspace.id),
    Some(vec![conflicted.change_id]),
  )
  .expect("start resolve");
  let target = session.targets.first().expect("target");
  let resolve_path = target.resolve_path.clone();
  let change_id = target.change_id.clone();

  let result = core::resolve_commit(
    &repo.repo_path,
    &change_id,
    // Side2 keeps the workspace-side bytes for this add/add rebase conflict.
    // Side1 matches the new parent (main) and would collapse to an empty change.
    &[core::ResolveSide::Side2],
    None,
  )
  .expect("resolve_commit side 2");
  assert!(result.success, "{}", result.message);
  assert!(
    !Path::new(&resolve_path).exists(),
    "change-id resolve directory should be removed after success"
  );
  assert!(
    !Path::new(&session.agent_cwd).exists(),
    "slug resolve root should be removed once no change-id dirs remain"
  );

  let after =
    core::list_commits(&repo.repo_path, Some(workspace.id), false, None, None).expect("list after");

  let still_conflicted = after.commits.iter().any(|c| c.has_conflicts);
  assert!(!still_conflicted, "should not remain conflicted");

  let commits_after = after
    .commits
    .iter()
    .filter(|c| !c.on_target_only && !c.is_working_copy)
    .count();
  assert_eq!(
    commits_after, commits_before,
    "resolving must keep the change on the stack without adding a new commit"
  );

  let status = core::workspace_status(&repo.repo_path, Some(workspace.id)).expect("status");
  assert!(!status.partial.has_conflicts);
  assert!(status.partial.commits_ahead >= 1);
}

#[test]
fn resolve_commit_with_stdin_replacements_rewrites_tree() {
  let repo = TestRepo::new().expect("test repo");
  let workspace = setup_rebase_conflict(&repo);
  let conflicted = core::list_commits(&repo.repo_path, Some(workspace.id), false, None, None)
    .expect("list")
    .commits
    .into_iter()
    .find(|c| c.has_conflicts && !c.on_target_only)
    .expect("conflicted");

  let _session = core::start_resolve_conflicts(
    &repo.repo_path,
    Some(workspace.id),
    Some(vec![conflicted.change_id.clone()]),
  )
  .expect("start");

  let mut files = std::collections::HashMap::new();
  files.insert(
    "conflict.txt".to_string(),
    "fully replaced content\n".to_string(),
  );
  let result = core::resolve_commit(&repo.repo_path, &conflicted.change_id, &[], Some(files))
    .expect("resolve with replacements");
  assert!(result.success, "{}", result.message);

  let status = core::workspace_status(&repo.repo_path, Some(workspace.id)).expect("status");
  assert!(!status.partial.has_conflicts);
}
