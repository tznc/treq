use crate::common::TestRepo;

fn parse_millis(timestamp: &str) -> i64 {
  chrono::DateTime::parse_from_rfc3339(timestamp)
    .unwrap_or_else(|_| panic!("timestamp should be rfc3339, got {timestamp}"))
    .timestamp_millis()
}

fn workspace_commits(log: &treq_lib::jj::JjLogResult) -> Vec<&treq_lib::jj::JjLogCommit> {
  log
    .commits
    .iter()
    .filter(|commit| !commit.is_working_copy && !commit.on_target_only)
    .collect()
}

fn setup_stack(repo: &TestRepo, branch: &str, messages: &[&str]) -> (i64, String) {
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    branch,
    Some(branch.to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("workspace");
  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  let workspace_path_str = workspace_path.to_str().unwrap().to_string();

  for (index, message) in messages.iter().enumerate() {
    TestRepo::write_workspace_file(
      &workspace_path_str,
      &format!("file-{index}.txt"),
      &format!("{message}\n"),
    )
    .expect("write");
    treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, message).expect("commit");
  }

  (workspace.id, workspace_path_str)
}

#[test]
fn shift_oldest_commit_forward_moves_older_descendants_preserving_gaps() {
  let repo = TestRepo::new().expect("repo");
  let (workspace_id, _) = setup_stack(
    &repo,
    "feat/shift-forward",
    &["Commit A", "Commit B", "Commit C"],
  );

  let before = treq_lib::core::list_commits(&repo.repo_path, Some(workspace_id), false, None, None)
    .expect("list");
  let commits = workspace_commits(&before);
  assert_eq!(commits.len(), 3, "expected three workspace commits");
  // list is newest-first
  let newest = commits[0];
  let middle = commits[1];
  let oldest = commits[2];
  let gap_newest = parse_millis(&newest.timestamp) - parse_millis(&middle.timestamp);
  let gap_middle = parse_millis(&middle.timestamp) - parse_millis(&oldest.timestamp);
  let oldest_id = oldest.change_id.clone();
  let middle_id = middle.change_id.clone();
  let newest_id = newest.change_id.clone();
  let oldest_before = parse_millis(&oldest.timestamp);

  treq_lib::core::shift_commit_timestamp(&repo.repo_path, workspace_id, &oldest_id, 2, 0, 0, None)
    .expect("shift");

  let after = treq_lib::core::list_commits(&repo.repo_path, Some(workspace_id), false, None, None)
    .expect("list after");
  let by_id = |id: &str| {
    after
      .commits
      .iter()
      .find(|commit| commit.change_id == id)
      .unwrap_or_else(|| panic!("missing {id}"))
  };
  let oldest_after = parse_millis(&by_id(&oldest_id).timestamp);
  let middle_after = parse_millis(&by_id(&middle_id).timestamp);
  let newest_after = parse_millis(&by_id(&newest_id).timestamp);

  assert!(
    (oldest_after - oldest_before - 2 * 86_400_000).abs() < 2_000,
    "oldest should move ~2 days, before={oldest_before} after={oldest_after}"
  );
  assert!(
    middle_after > oldest_after,
    "middle must stay after oldest: {oldest_after} vs {middle_after}"
  );
  assert!(
    newest_after > middle_after,
    "newest must stay after middle: {middle_after} vs {newest_after}"
  );
  assert_eq!(middle_after - oldest_after, gap_middle.max(1000));
  assert_eq!(newest_after - middle_after, gap_newest.max(1000));
}

#[test]
fn shift_middle_commit_does_not_move_older_parent() {
  let repo = TestRepo::new().expect("repo");
  let (workspace_id, _) = setup_stack(
    &repo,
    "feat/shift-middle",
    &["Commit A", "Commit B", "Commit C"],
  );

  let before = treq_lib::core::list_commits(&repo.repo_path, Some(workspace_id), false, None, None)
    .expect("list");
  let commits = workspace_commits(&before);
  let newest = commits[0];
  let middle = commits[1];
  let oldest = commits[2];
  let oldest_before = parse_millis(&oldest.timestamp);
  let gap = parse_millis(&newest.timestamp) - parse_millis(&middle.timestamp);

  treq_lib::core::shift_commit_timestamp(
    &repo.repo_path,
    workspace_id,
    &middle.change_id,
    1,
    3,
    0,
    None,
  )
  .expect("shift");

  let after = treq_lib::core::list_commits(&repo.repo_path, Some(workspace_id), false, None, None)
    .expect("list after");
  let by_desc = |description: &str| {
    after
      .commits
      .iter()
      .find(|commit| commit.description == description)
      .unwrap()
  };
  let oldest_after = parse_millis(&by_desc("Commit A").timestamp);
  let middle_after = parse_millis(&by_desc("Commit B").timestamp);
  let newest_after = parse_millis(&by_desc("Commit C").timestamp);

  assert_eq!(
    oldest_after, oldest_before,
    "parent lineage should stay put"
  );
  assert!(middle_after > oldest_after);
  assert_eq!(newest_after - middle_after, gap.max(1000));
}

#[test]
fn shift_to_specific_day_keeps_clock_and_orders_descendants() {
  let repo = TestRepo::new().expect("repo");
  let (workspace_id, _) = setup_stack(&repo, "feat/shift-day", &["Day A", "Day B"]);

  let before = treq_lib::core::list_commits(&repo.repo_path, Some(workspace_id), false, None, None)
    .expect("list");
  let commits = workspace_commits(&before);
  let older = commits[1];
  let newer = commits[0];
  let older_dt = chrono::DateTime::parse_from_rfc3339(&older.timestamp).unwrap();
  let gap = parse_millis(&newer.timestamp) - parse_millis(&older.timestamp);
  let target_day = older_dt.date_naive() + chrono::Duration::days(5);
  let target_day = target_day.format("%Y-%m-%d").to_string();

  treq_lib::core::shift_commit_timestamp(
    &repo.repo_path,
    workspace_id,
    &older.change_id,
    0,
    0,
    0,
    Some(&target_day),
  )
  .expect("shift to day");

  let after = treq_lib::core::list_commits(&repo.repo_path, Some(workspace_id), false, None, None)
    .expect("list after");
  let older_after = after
    .commits
    .iter()
    .find(|commit| commit.description == "Day A")
    .unwrap();
  let newer_after = after
    .commits
    .iter()
    .find(|commit| commit.description == "Day B")
    .unwrap();
  let older_after_dt = chrono::DateTime::parse_from_rfc3339(&older_after.timestamp).unwrap();
  assert_eq!(older_after_dt.date_naive().to_string(), target_day);
  assert_eq!(older_after_dt.time(), older_dt.time());
  assert_eq!(
    parse_millis(&newer_after.timestamp) - parse_millis(&older_after.timestamp),
    gap.max(1000)
  );
}

#[test]
fn rejects_shift_to_day_before_parent() {
  let repo = TestRepo::new().expect("repo");
  let (workspace_id, _) = setup_stack(&repo, "feat/shift-day-before-parent", &["Day A"]);
  let before = treq_lib::core::list_commits(&repo.repo_path, Some(workspace_id), false, None, None)
    .expect("list");
  let older = workspace_commits(&before)[0];
  let err = treq_lib::core::shift_commit_timestamp(
    &repo.repo_path,
    workspace_id,
    &older.change_id,
    0,
    0,
    0,
    Some("2020-06-15"),
  );
  assert!(
    err.unwrap_err().contains("at or before the parent"),
    "should refuse a day that inverts parent/child order"
  );
}

#[test]
fn rejects_immutable_and_invalid_change_ids() {
  let repo = TestRepo::new().expect("repo");
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    "feat/shift-invalid",
    Some("invalid".to_string()),
    None,
    None,
    None,
    None,
  )
  .expect("workspace");

  let err = treq_lib::core::shift_commit_timestamp(
    &repo.repo_path,
    workspace.id,
    "-r malicious",
    1,
    0,
    0,
    None,
  );
  assert!(err.is_err(), "should reject change_id starting with '-'");

  let home = treq_lib::core::list_commits(&repo.repo_path, None, false, None, None).expect("home");
  // A default-branch commit: immutable to a workspace, whatever the home log
  // marks it (with no remote, nothing there is published).
  let immutable = home
    .commits
    .iter()
    .find(|commit| !commit.is_working_copy)
    .expect("default-branch home commit");
  // Editing via a workspace still must refuse the default-branch immutable commit
  // if we somehow passed its change id; use the workspace WC parent instead by
  // trying the initial commit through the workspace.
  let err = treq_lib::core::shift_commit_timestamp(
    &repo.repo_path,
    workspace.id,
    &immutable.change_id,
    1,
    0,
    0,
    None,
  );
  assert!(err.is_err(), "should reject immutable commit, got {err:?}");
}

#[test]
fn shift_mutable_commits_to_now_moves_newest_real_commit_and_preserves_gaps() {
  let repo = TestRepo::new().expect("repo");
  let (workspace_id, _) = setup_stack(
    &repo,
    "feat/shift-to-now",
    &["Commit A", "Commit B", "Commit C"],
  );

  let before = treq_lib::core::list_commits(&repo.repo_path, Some(workspace_id), false, None, None)
    .expect("list");
  let commits = workspace_commits(&before);
  let newest = commits[0];
  let middle = commits[1];
  let oldest = commits[2];
  let gap_newest = parse_millis(&newest.timestamp) - parse_millis(&middle.timestamp);
  let gap_middle = parse_millis(&middle.timestamp) - parse_millis(&oldest.timestamp);

  treq_lib::core::shift_commit_timestamp(
    &repo.repo_path,
    workspace_id,
    &oldest.change_id,
    2,
    0,
    0,
    None,
  )
  .expect("shift into the future");

  treq_lib::core::shift_mutable_commits_to_now(&repo.repo_path, workspace_id).expect("to now");

  let after = treq_lib::core::list_commits(&repo.repo_path, Some(workspace_id), false, None, None)
    .expect("list after");
  let by_desc = |description: &str| {
    after
      .commits
      .iter()
      .find(|commit| commit.description == description)
      .unwrap()
  };
  let oldest_after = parse_millis(&by_desc("Commit A").timestamp);
  let middle_after = parse_millis(&by_desc("Commit B").timestamp);
  let newest_after = parse_millis(&by_desc("Commit C").timestamp);
  let now = chrono::Utc::now().timestamp_millis();

  assert!(
    (newest_after - now).abs() < 5_000,
    "newest real commit should land at now, got {newest_after} vs {now}"
  );
  assert_eq!(middle_after - oldest_after, gap_middle.max(1000));
  assert_eq!(newest_after - middle_after, gap_newest.max(1000));
  assert!(oldest_after < middle_after);
  assert!(middle_after < newest_after);
}
