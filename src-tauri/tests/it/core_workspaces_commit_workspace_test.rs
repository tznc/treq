use crate::common::TestRepo;

fn assert_raw_jj_log_has_working_copy_commit(workspace_path: &str, expected_message: &str) {
  let log = TestRepo::jj_log_descriptions(workspace_path, "::@", Some(15)).expect("jj log failed");

  assert!(
    log.iter().any(|(desc, _)| desc == expected_message),
    "expected '{expected_message}' in jj log, got:\n{log:?}"
  );
  assert!(
    log.iter().any(|(desc, _)| desc.is_empty()),
    "expected jj log to include empty working copy commit, got:\n{log:?}"
  );
}

fn assert_workspace_status_is_clean(workspace_path: &str) {
  assert!(
    TestRepo::jj_working_copy_is_clean(workspace_path),
    "expected clean workspace status after commit"
  );
}

fn assert_workspace_list_commits_hides_working_copy(
  repo_path: &str,
  workspace_id: i64,
  expected_message: Option<&str>,
) {
  let log = treq_lib::core::list_commits(repo_path, Some(workspace_id), false, None, None)
    .expect("list_commits failed");

  if let Some(expected_message) = expected_message {
    assert!(
      log
        .commits
        .iter()
        .any(|c| c.description.contains(expected_message)),
      "expected '{expected_message}' in workspace commit log, got: {:?}",
      log
        .commits
        .iter()
        .map(|c| &c.description)
        .collect::<Vec<_>>()
    );
  }
  assert!(
    log.commits.iter().all(|c| !c.is_working_copy),
    "expected workspace log to exclude working copy commits, got: {:?}",
    log
      .commits
      .iter()
      .map(|c| (c.description.clone(), c.is_working_copy))
      .collect::<Vec<_>>()
  );
  assert!(
    log.commits.iter().all(|c| !c.description.trim().is_empty()),
    "expected workspace log to exclude empty working copy commits, got: {:?}",
    log
      .commits
      .iter()
      .map(|c| &c.description)
      .collect::<Vec<_>>()
  );
  assert!(
    log.tentative_working_copy.is_none(),
    "expected no tentative working copy after commit, got: {:?}",
    log.tentative_working_copy
  );
}

#[test]
fn test_create_commit_basic() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/commit-basic",
    Some("basic commit".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let ws_dir = repo.workspaces_dir().join(&workspace.workspace_path);
  let ws_dir_str = ws_dir.to_str().expect("utf-8");
  TestRepo::write_workspace_file(ws_dir_str, "data.txt", "hello\n").expect("Failed to write file");

  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "add data")
    .expect("create_commit failed");

  assert_workspace_status_is_clean(ws_dir_str);
  assert_raw_jj_log_has_working_copy_commit(ws_dir_str, "add data");
  assert_workspace_list_commits_hides_working_copy(&repo.repo_path, workspace.id, Some("add data"));
}

#[test]
fn test_create_commit_unknown_workspace_errors() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let err = treq_lib::core::commit_workspace(&repo.repo_path, i64::MAX, "nope")
    .expect_err("should error on unknown workspace id");
  assert!(
    err.contains("Workspace not found"),
    "expected 'Workspace not found' in error, got: {err}"
  );
}

#[test]
fn test_create_commit_empty_workspace() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/commit-empty",
    Some("empty commit test".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "empty commit")
    .expect("create_commit on empty workspace should succeed");

  let ws_dir = repo.workspaces_dir().join(&workspace.workspace_path);
  let ws_dir_str = ws_dir.to_str().expect("utf-8");

  // A commit with no file changes must not persist in history — only the live,
  // description-less working copy is exempt from the empty-commit policy.
  assert_workspace_status_is_clean(ws_dir_str);
  let log = TestRepo::jj_log_descriptions(ws_dir_str, "::@", Some(15)).expect("jj log failed");
  assert!(
    !log.iter().any(|(desc, _)| desc == "empty commit"),
    "expected empty commit to be discarded from history, got:\n{log:?}"
  );
  assert_workspace_list_commits_hides_working_copy(&repo.repo_path, workspace.id, None);
}

#[test]
fn test_commit_home_repo_advances_branch() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let default_branch = repo.default_branch();

  let before = crate::common::JjVerifier::get_bookmark_commit_id(&repo.repo_path, default_branch)
    .expect("query bookmark")
    .expect("main bookmark should exist after init");

  repo
    .create_file("home.txt", "home\n")
    .expect("write home file");

  treq_lib::jj::jj_commit(&repo.repo_path, "home repo commit")
    .expect("jj_commit on home repo failed");

  let after = crate::common::JjVerifier::get_bookmark_commit_id(&repo.repo_path, default_branch)
    .expect("query bookmark")
    .expect("main bookmark should still exist");

  assert_ne!(before, after, "main bookmark should advance after commit");

  let log = treq_lib::core::list_commits(&repo.repo_path, None, false, None, None)
    .expect("list_commits failed");
  assert!(
    log
      .commits
      .iter()
      .any(|c| c.description.contains("home repo commit")),
    "expected 'home repo commit' in home repo log, got: {:?}",
    log
      .commits
      .iter()
      .map(|c| &c.description)
      .collect::<Vec<_>>()
  );
}

#[test]
fn test_commit_home_repo_leaves_branch_unconflicted_after_git_import() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let default_branch = repo.default_branch();
  repo
    .create_file("home.txt", "home\n")
    .expect("write home file");

  treq_lib::jj::jj_commit(&repo.repo_path, "home repo commit")
    .expect("jj_commit on home repo failed");
  // Any later read that imports git refs must agree with the bookmark jj moved.
  treq_lib::jj::jj_import_remaining_git_refs(&repo.repo_path).expect("import git refs");

  assert!(
    !treq_lib::jj::jj_is_bookmark_conflicted(&repo.repo_path, default_branch),
    "home branch bookmark became conflicted after committing"
  );
}

#[test]
fn test_commit_workspace_advances_branch() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/commit-advance",
    Some("advance test".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let ws_dir = repo.workspaces_dir().join(&workspace.workspace_path);
  let ws_dir_str = ws_dir.to_string_lossy().to_string();

  let before =
    crate::common::JjVerifier::get_bookmark_commit_id(&repo.repo_path, &workspace.branch_name)
      .expect("query bookmark")
      .expect("workspace bookmark should exist after create_workspace");

  crate::common::TestRepo::write_workspace_file(&ws_dir_str, "data.txt", "hello\n")
    .expect("write workspace file");

  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "workspace advance")
    .expect("commit_workspace failed");

  let after =
    crate::common::JjVerifier::get_bookmark_commit_id(&repo.repo_path, &workspace.branch_name)
      .expect("query bookmark")
      .expect("workspace bookmark should still exist");

  assert_ne!(
    before, after,
    "workspace bookmark should advance after commit"
  );

  let log = treq_lib::core::list_commits(&repo.repo_path, Some(workspace.id), false, None, None)
    .expect("list_commits failed");
  assert!(
    log
      .commits
      .iter()
      .any(|c| c.description.contains("workspace advance")),
    "expected 'workspace advance' in workspace log, got: {:?}",
    log
      .commits
      .iter()
      .map(|c| &c.description)
      .collect::<Vec<_>>()
  );
}

// Regression test: committing in a parent workspace must not create divergent changes (??)
// in stacked child workspaces. jj_commit runs rebase_descendants() which rewrites the WC
// commits of every descendant workspace, but previously never reconciled those workspaces'
// on-disk trees to the new commit pointers. The next jj command in the child would then
// snapshot the stale on-disk tree and record an inverse diff, producing duplicate (divergent)
// change IDs marked ?? in jj log.
#[test]
fn test_create_commit_no_divergence_with_stacked_descendant() {
  let repo = TestRepo::new().expect("Failed to create test repo");

  // --- Set up parent workspace A ---
  let ws_a = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/parent",
    Some("parent workspace".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create parent workspace");
  let ws_a_dir = repo.workspaces_dir().join(&ws_a.workspace_path);
  let ws_a_dir_str = ws_a_dir.to_str().expect("utf-8");

  // Give A a real committed change so it has a bookmark tip B can stack on.
  TestRepo::write_workspace_file(ws_a_dir_str, "parent.txt", "parent v1\n")
    .expect("write parent file");
  treq_lib::core::commit_workspace(&repo.repo_path, ws_a.id, "parent change")
    .expect("commit parent workspace");

  // --- Set up stacked child workspace B ---
  let ws_b = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/child",
    Some("child workspace".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create child workspace");
  // B targets A's branch so it is a descendant of A's stack.
  treq_lib::local_db::update_workspace_target_branch(&repo.repo_path, ws_b.id, &ws_a.branch_name)
    .expect("Failed to set B's target_branch to A");

  let ws_b_dir = repo.workspaces_dir().join(&ws_b.workspace_path);
  let ws_b_dir_str = ws_b_dir.to_str().expect("utf-8");

  // Give B its own committed change descending from A's tip.
  TestRepo::write_workspace_file(ws_b_dir_str, "child.txt", "child v1\n")
    .expect("write child file");
  treq_lib::core::commit_workspace(&repo.repo_path, ws_b.id, "child change")
    .expect("commit child workspace");

  // --- Commit a second change in A ---
  // This causes jj_commit to call rebase_descendants(), which rewrites the WC pointer
  // of B (which descends from A). Without reconciliation, the next jj invocation in B
  // snapshots B's stale on-disk tree and creates divergent (??) change IDs.
  TestRepo::write_workspace_file(ws_a_dir_str, "parent.txt", "parent v2\n")
    .expect("write parent v2 file");
  treq_lib::core::commit_workspace(&repo.repo_path, ws_a.id, "parent change 2")
    .expect("commit parent workspace v2");

  // --- Assert: no divergent changes anywhere in the repo ---
  // A change id repeated across two commits means those commits diverged (jj
  // CLI's log output marks this with a trailing `??`).
  let change_ids =
    TestRepo::jj_change_ids_in_revset(ws_a_dir_str, "all()").expect("jj log all() failed");
  let mut seen = std::collections::HashSet::new();
  let duplicates: Vec<&String> = change_ids.iter().filter(|id| !seen.insert(*id)).collect();
  assert!(
    duplicates.is_empty(),
    "expected no divergent changes after committing in parent, but change ids repeated: {duplicates:?} (all: {change_ids:?})"
  );

  // --- Assert: B's working copy is clean (no phantom inverse-diff changes) ---
  assert_workspace_status_is_clean(ws_b_dir_str);
}

// Regression test: when a workspace's target_branch equals its own branch_name,
// the post-commit auto-rebase previously discarded the fresh empty working-copy
// commit that jj_commit created and re-pinned @ back to the committed commit.
#[test]
fn test_create_commit_keeps_empty_working_copy_when_self_targeted() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    repo.default_branch(),
    Some("sync clobber test".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  // Set the workspace's target_branch to itself so that post-commit
  // rebase_after_commit picks it up and exercises the jj_sync_working_copy_if_safe
  // code path (auto_rebase/mod.rs branch_name == target_branch branch).
  treq_lib::local_db::update_workspace_target_branch(
    &repo.repo_path,
    workspace.id,
    &workspace.branch_name,
  )
  .expect("Failed to set self-targeting target_branch");

  let ws_dir = repo.workspaces_dir().join(&workspace.workspace_path);
  let ws_dir_str = ws_dir.to_str().expect("utf-8");
  TestRepo::write_workspace_file(ws_dir_str, "data.txt", "hello\n").expect("Failed to write file");

  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "add data")
    .expect("create_commit failed");

  // @ must have advanced to a new empty working copy — not stayed on "add data".
  // Use the raw jj assertions: list_commits returns empty for a self-targeted workspace
  // (target..branch range is empty) so the important checks are via jj directly.
  assert_workspace_status_is_clean(ws_dir_str);
  assert_raw_jj_log_has_working_copy_commit(ws_dir_str, "add data");

  // The tentative_working_copy must be absent — that's the UI-visible symptom of the bug.
  let log = treq_lib::core::list_commits(&repo.repo_path, Some(workspace.id), false, None, None)
    .expect("list_commits failed");
  assert!(
    log.tentative_working_copy.is_none(),
    "expected no tentative working copy after commit, got: {:?}",
    log.tentative_working_copy
  );
}

#[test]
fn test_commit_workspace_abandons_stray_empty_commit_in_history() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let workspace = repo
    .create_workspace_with_commit("feat/abandon-empty", "first.txt", "hello\n", None)
    .expect("failed to create workspace with initial commit");

  let ws_dir = repo.workspaces_dir().join(&workspace.workspace_path);
  let ws_dir_str = ws_dir.to_str().expect("utf-8");

  // Simulate an empty commit left behind in history (e.g. by a squash/rebase that
  // absorbed its content elsewhere) by describing an unchanged commit directly via jj,
  // bypassing commit_workspace.
  TestRepo::jj_new(ws_dir_str, &["@"]).expect("jj new failed");
  TestRepo::jj_describe(ws_dir_str, "@", "stray empty").expect("jj describe failed");
  TestRepo::jj_new(ws_dir_str, &["@"]).expect("jj new failed");

  TestRepo::write_workspace_file(ws_dir_str, "second.txt", "world\n")
    .expect("failed to write file");

  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "add second")
    .expect("commit_workspace should succeed");

  let log = TestRepo::jj_log_descriptions(ws_dir_str, "::@", Some(20)).expect("jj log failed");

  assert!(
    !log.iter().any(|(desc, _)| desc == "stray empty"),
    "expected stray empty commit to be abandoned from history, got:\n{log:?}"
  );
  assert!(
    log.iter().any(|(desc, _)| desc == "add second"),
    "expected new commit to be present, got:\n{log:?}"
  );
}

#[test]
fn test_create_commit_excludes_bundled_codex_skill() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/codex-skill-untracked",
    None,
    None,
    None,
    None,
    None,
  )
  .expect("Failed to create workspace");

  let ws_dir = repo.workspaces_dir().join(&workspace.workspace_path);
  let ws_dir_str = ws_dir.to_str().expect("utf-8");
  TestRepo::write_workspace_file(ws_dir_str, "app.txt", "real work\n").expect("write app file");
  let installed = treq_lib::core::write_agent_cli_files("prompt", None, Some(ws_dir_str))
    .expect("install bundled skill");
  let skill_path = installed
    .agents_skill_path
    .expect("Codex skill should be installed in the workspace");
  let claude_skill_path = installed
    .claude_skill_path
    .expect("Claude skill should be installed in the workspace");
  assert!(
    std::path::Path::new(&skill_path).join("SKILL.md").exists(),
    "skill file should exist on disk during the session"
  );
  assert!(
    std::path::Path::new(&claude_skill_path)
      .join("SKILL.md")
      .exists(),
    "Claude skill file should exist on disk during the session"
  );

  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "add app")
    .expect("commit_workspace failed");

  let log = treq_lib::core::list_commits(&repo.repo_path, Some(workspace.id), false, None, None)
    .expect("list_commits failed");
  let committed = log
    .commits
    .iter()
    .find(|c| c.description.contains("add app"))
    .expect("committed revision with 'add app' should exist");
  let diff = treq_lib::core::get_commit_diff_with_conflict_style(
    &repo.repo_path,
    Some(workspace.id),
    &committed.change_id,
    "git",
  )
  .expect("commit diff failed");
  let committed_paths: Vec<&str> = diff
    .committed_files
    .iter()
    .map(|f| f.path.as_str())
    .collect();
  assert!(
    committed_paths.contains(&"app.txt"),
    "expected app.txt in the committed diff, got: {committed_paths:?}"
  );
  assert!(
    committed_paths
      .iter()
      .all(|path| !path.contains(".agents/skills/treq") && !path.contains(".claude/skills/treq")),
    "bundled agent skills must not be in the committed diff, got: {committed_paths:?}"
  );

  let tracked = treq_lib::jj::jj_get_tracked_files(ws_dir_str).expect("tracked files");
  assert!(
    tracked.iter().any(|path| path == "app.txt"),
    "expected app.txt to stay tracked, got: {tracked:?}"
  );
  assert!(
    tracked
      .iter()
      .all(|path| !path.contains(".agents/skills/treq") && !path.contains(".claude/skills/treq")),
    "bundled agent skills must not be tracked after commit, got: {tracked:?}"
  );

  let git_tree = TestRepo::run_git(
    &repo.repo_path,
    &["ls-tree", "-r", "--name-only", "feat/codex-skill-untracked"],
  )
  .expect("git ls-tree of workspace bookmark failed");
  assert!(
    git_tree.lines().any(|line| line == "app.txt"),
    "expected app.txt in the git tree, got:\n{git_tree}"
  );
  assert!(
    !git_tree.contains(".agents/skills/treq") && !git_tree.contains(".claude/skills/treq"),
    "bundled agent skills must not be tracked in git, got:\n{git_tree}"
  );
  assert!(
    std::path::Path::new(&skill_path).join("SKILL.md").exists()
      && std::path::Path::new(&claude_skill_path)
        .join("SKILL.md")
        .exists(),
    "ignored skills should remain on disk after commit"
  );
}
