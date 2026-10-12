use crate::common::{TestRepo, FAILING_WORKFLOW, PASSING_WORKFLOW};
use treq_lib::core;

#[test]
fn test_list_workflows_empty_for_fresh_repo() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let result = core::list_workflows_sync(&repo.repo_path).expect("Failed to list workflows");
  assert!(result.is_empty());
}

#[test]
fn test_list_workflows_sees_yaml_file() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  repo
    .write_workflow("ci.yaml", PASSING_WORKFLOW)
    .expect("Failed to write workflow");
  let result = core::list_workflows_sync(&repo.repo_path).expect("Failed to list workflows");
  assert_eq!(result.len(), 1);
  assert_eq!(result[0].name, "Passing CI");
}

#[test]
fn test_list_workflows_multiple_files_sorted() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  repo
    .write_workflow("b.yaml", PASSING_WORKFLOW)
    .expect("Failed to write b.yaml");
  repo
    .write_workflow("a.yaml", FAILING_WORKFLOW)
    .expect("Failed to write a.yaml");
  let result = core::list_workflows_sync(&repo.repo_path).expect("Failed to list workflows");
  assert_eq!(result.len(), 2);
  assert!(result[0].filename < result[1].filename);
}

#[test]
fn test_run_workflow_job_success() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  repo
    .write_workflow("ci.yaml", PASSING_WORKFLOW)
    .expect("Failed to write workflow");
  treq_lib::local_db::trust_repo(&repo.repo_path).expect("Failed to trust repo");
  let result = core::run_workflow_job_sync(&repo.repo_path, "ci.yaml", "greet", 0, &repo.repo_path)
    .expect("Failed to run job");
  assert!(result.success);
  assert!(!result.steps.is_empty());
}

#[test]
fn test_run_workflow_job_stops_at_first_failure() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  repo
    .write_workflow("ci.yaml", FAILING_WORKFLOW)
    .expect("Failed to write workflow");
  treq_lib::local_db::trust_repo(&repo.repo_path).expect("Failed to trust repo");
  let result = core::run_workflow_job_sync(&repo.repo_path, "ci.yaml", "check", 0, &repo.repo_path)
    .expect("Failed to run job");
  assert!(!result.success);
  assert_eq!(result.steps.len(), 1);
}

#[test]
fn test_run_workflow_job_unknown_job_error() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  repo
    .write_workflow("ci.yaml", PASSING_WORKFLOW)
    .expect("Failed to write workflow");
  treq_lib::local_db::trust_repo(&repo.repo_path).expect("Failed to trust repo");
  let err = core::run_workflow_job_sync(
    &repo.repo_path,
    "ci.yaml",
    "nonexistent",
    0,
    &repo.repo_path,
  )
  .unwrap_err();
  assert!(err.contains("nonexistent"));
}

#[test]
fn test_run_workflow_runs_all_jobs() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  let content = "name: Multi Job\non:\n  workflow_dispatch: {}\njobs:\n  job1:\n    steps:\n      - name: step1\n        run: echo a\n  job2:\n    steps:\n      - name: step2\n        run: echo b\n";
  repo
    .write_workflow("multi.yaml", content)
    .expect("Failed to write workflow");
  treq_lib::local_db::trust_repo(&repo.repo_path).expect("Failed to trust repo");
  let results = core::run_workflow_sync(&repo.repo_path, "multi.yaml", 0, &repo.repo_path)
    .expect("Failed to run workflow");
  assert_eq!(results.len(), 2);
  assert!(results.iter().all(|r| r.success));
}

fn workspace_log_descriptions(repo_path: &str, workspace_id: i64) -> Vec<String> {
  treq_lib::core::list_commits(repo_path, Some(workspace_id), false, None, None)
    .expect("list_commits failed")
    .commits
    .into_iter()
    .map(|c| c.description)
    .collect()
}

#[test]
fn passing_check_creates_autosave_commit_for_dirty_workspace() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  repo
    .write_workflow("ci.yaml", PASSING_WORKFLOW)
    .expect("Failed to write workflow");
  treq_lib::local_db::trust_repo(&repo.repo_path).expect("Failed to trust repo");
  let workspace = repo
    .create_workspace_simple("feat/autosave-pass")
    .expect("Failed to create workspace");
  let ws_path = repo.workspace_full_path(&workspace);
  TestRepo::write_workspace_file(&ws_path, "good.txt", "checked\n").expect("Failed to write file");

  let result =
    core::run_workflow_job_sync(&repo.repo_path, "ci.yaml", "greet", workspace.id, &ws_path)
      .expect("Failed to run job");
  assert!(result.success);

  let descriptions = workspace_log_descriptions(&repo.repo_path, workspace.id);
  assert!(
    descriptions
      .iter()
      .any(|d| d.starts_with("treq-autosave: good.txt")),
    "expected an autosave commit listing the dirty file, got: {descriptions:?}"
  );
}

#[test]
fn passing_check_autosave_message_ellipsizes_after_two_files() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  repo
    .write_workflow("ci.yaml", PASSING_WORKFLOW)
    .expect("Failed to write workflow");
  treq_lib::local_db::trust_repo(&repo.repo_path).expect("Failed to trust repo");
  let workspace = repo
    .create_workspace_simple("feat/autosave-many-files")
    .expect("Failed to create workspace");
  let ws_path = repo.workspace_full_path(&workspace);
  TestRepo::write_workspace_file(&ws_path, "a.txt", "a\n").expect("Failed to write a.txt");
  TestRepo::write_workspace_file(&ws_path, "b.txt", "b\n").expect("Failed to write b.txt");
  TestRepo::write_workspace_file(&ws_path, "c.txt", "c\n").expect("Failed to write c.txt");

  let result =
    core::run_workflow_job_sync(&repo.repo_path, "ci.yaml", "greet", workspace.id, &ws_path)
      .expect("Failed to run job");
  assert!(result.success);

  let descriptions = workspace_log_descriptions(&repo.repo_path, workspace.id);
  assert!(
    descriptions
      .iter()
      .any(|d| d.starts_with("treq-autosave: a.txt, b.txt, … N1 more")),
    "expected autosave to list two files then remaining count, got: {descriptions:?}"
  );
}

#[test]
fn failing_check_does_not_create_autosave_commit() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  repo
    .write_workflow("ci.yaml", FAILING_WORKFLOW)
    .expect("Failed to write workflow");
  treq_lib::local_db::trust_repo(&repo.repo_path).expect("Failed to trust repo");
  let workspace = repo
    .create_workspace_simple("feat/autosave-fail")
    .expect("Failed to create workspace");
  let ws_path = repo.workspace_full_path(&workspace);
  TestRepo::write_workspace_file(&ws_path, "bad.txt", "unchecked\n").expect("Failed to write file");

  let result =
    core::run_workflow_job_sync(&repo.repo_path, "ci.yaml", "check", workspace.id, &ws_path)
      .expect("Failed to run job");
  assert!(!result.success);

  let descriptions = workspace_log_descriptions(&repo.repo_path, workspace.id);
  assert!(
    descriptions.iter().all(|d| !d.contains("treq-autosave:")),
    "failing check must not autosave, got: {descriptions:?}"
  );
}

#[test]
fn passing_check_skips_autosave_when_working_copy_is_clean() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  repo
    .write_workflow("ci.yaml", PASSING_WORKFLOW)
    .expect("Failed to write workflow");
  treq_lib::local_db::trust_repo(&repo.repo_path).expect("Failed to trust repo");
  let workspace = repo
    .create_workspace_simple("feat/autosave-clean")
    .expect("Failed to create workspace");
  let ws_path = repo.workspace_full_path(&workspace);

  let result =
    core::run_workflow_job_sync(&repo.repo_path, "ci.yaml", "greet", workspace.id, &ws_path)
      .expect("Failed to run job");
  assert!(result.success);

  let descriptions = workspace_log_descriptions(&repo.repo_path, workspace.id);
  assert!(
    descriptions.iter().all(|d| !d.contains("treq-autosave:")),
    "clean working copy must not create an autosave, got: {descriptions:?}"
  );
}

#[test]
fn manual_commit_drops_autosave_commits_and_keeps_the_checked_changes() {
  let repo = TestRepo::new().expect("Failed to create test repo");
  repo
    .write_workflow("ci.yaml", PASSING_WORKFLOW)
    .expect("Failed to write workflow");
  treq_lib::local_db::trust_repo(&repo.repo_path).expect("Failed to trust repo");
  let workspace = repo
    .create_workspace_simple("feat/autosave-drop")
    .expect("Failed to create workspace");
  let ws_path = repo.workspace_full_path(&workspace);
  TestRepo::write_workspace_file(&ws_path, "good.txt", "first\n").expect("Failed to write file");

  let first =
    core::run_workflow_job_sync(&repo.repo_path, "ci.yaml", "greet", workspace.id, &ws_path)
      .expect("Failed to run first job");
  assert!(first.success);

  TestRepo::write_workspace_file(&ws_path, "good.txt", "second\n").expect("Failed to write file");
  let second =
    core::run_workflow_job_sync(&repo.repo_path, "ci.yaml", "greet", workspace.id, &ws_path)
      .expect("Failed to run second job");
  assert!(second.success);

  let before = workspace_log_descriptions(&repo.repo_path, workspace.id);
  let autosave_count = before
    .iter()
    .filter(|d| d.contains("treq-autosave:"))
    .count();
  assert!(
    autosave_count >= 1,
    "expected at least one autosave before the manual commit, got: {before:?}"
  );

  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "ship the good changes")
    .expect("manual commit failed");

  let after = workspace_log_descriptions(&repo.repo_path, workspace.id);
  assert!(
    after.iter().any(|d| d.contains("ship the good changes")),
    "manual commit should remain, got: {after:?}"
  );
  assert!(
    after.iter().all(|d| !d.contains("treq-autosave:")),
    "autosaves must be dropped after a manual commit, got: {after:?}"
  );

  let contents = std::fs::read_to_string(std::path::Path::new(&ws_path).join("good.txt"))
    .expect("failed to read saved file");
  assert_eq!(contents, "second\n");
}
