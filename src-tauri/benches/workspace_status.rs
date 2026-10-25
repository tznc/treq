#![expect(
  clippy::unwrap_used,
  clippy::let_underscore_must_use,
  clippy::expect_used,
  reason = "test code"
)]
use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use std::fs;
use std::process::Command;

#[path = "../tests/common/mod.rs"]
mod e2e_test_helpers;
use e2e_test_helpers::TestRepo;

fn create_remote_workspace(
  repo: &TestRepo,
  branch: &str,
) -> (treq_lib::local_db::Workspace, std::path::PathBuf) {
  let workspace = treq_lib::core::create_workspace(
    &repo.repo_path,
    branch,
    Some(format!("bench workspace for {branch}")),
    None,
    None,
    None,
    None,
  )
  .unwrap_or_else(|e| panic!("failed to create workspace '{branch}': {e}"));

  let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
  (workspace, workspace_path)
}

fn setup_workspace_status_ahead() -> (TestRepo, i64) {
  let repo = TestRepo::with_remote().expect("failed to init test repo with remote");
  let (workspace, workspace_path) = create_remote_workspace(&repo, "feat-status-ahead");
  let workspace_path_str = workspace_path
    .to_str()
    .expect("workspace path should be valid utf-8");
  treq_lib::jj::jj_bookmark_track(&repo.repo_path, repo.default_branch(), "origin")
    .expect("failed to track the default branch on origin");

  fs::write(workspace_path.join("initial.txt"), "initial content\n")
    .expect("failed to write initial file");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "Initial commit")
    .expect("failed to commit initial file");
  treq_lib::core::push_workspace_to_remote(&repo.repo_path, Some(workspace.id))
    .expect("failed to push workspace");
  treq_lib::jj::jj_git_fetch(&repo.repo_path).expect("failed to fetch after push");
  treq_lib::jj::jj_bookmark_track(workspace_path_str, &workspace.branch_name, "origin")
    .expect("failed to track remote bookmark after push");
  treq_lib::core::pull_workspace_from_remote(&repo.repo_path, Some(workspace.id), "git")
    .expect("failed to sync workspace after push");

  fs::write(
    workspace_path.join("committed-local.txt"),
    "committed local content\n",
  )
  .expect("failed to write committed local file");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "Local committed change")
    .expect("failed to commit local file");

  fs::write(
    workspace_path.join("working-copy-local.txt"),
    "working copy local content\n",
  )
  .expect("failed to write working copy file");

  (repo, workspace.id)
}

fn setup_workspace_status_behind() -> (TestRepo, i64) {
  let repo = TestRepo::with_remote().expect("failed to init test repo with remote");
  let (workspace, workspace_path) = create_remote_workspace(&repo, "feat-status-behind");
  let workspace_path_str = workspace_path
    .to_str()
    .expect("workspace path should be valid utf-8");
  treq_lib::jj::jj_bookmark_track(&repo.repo_path, repo.default_branch(), "origin")
    .expect("failed to track the default branch on origin");

  fs::write(workspace_path.join("initial.txt"), "initial content\n")
    .expect("failed to write initial file");
  treq_lib::core::commit_workspace(&repo.repo_path, workspace.id, "Initial commit")
    .expect("failed to commit initial file");
  treq_lib::core::push_workspace_to_remote(&repo.repo_path, Some(workspace.id))
    .expect("failed to push workspace");
  treq_lib::jj::jj_git_fetch(&repo.repo_path).expect("failed to fetch after push");
  treq_lib::jj::jj_bookmark_track(workspace_path_str, &workspace.branch_name, "origin")
    .expect("failed to track remote bookmark after push");
  treq_lib::core::pull_workspace_from_remote(&repo.repo_path, Some(workspace.id), "git")
    .expect("failed to sync workspace after push");

  fs::write(
    workspace_path.join("working-copy-local.txt"),
    "working copy local content\n",
  )
  .expect("failed to write working copy file");

  repo
    .remote_commit_on_branch(
      &workspace.branch_name,
      "remote-committed.txt",
      "remote committed content\n",
      "Remote committed change",
    )
    .expect("failed to commit on the remote branch");

  treq_lib::jj::jj_git_fetch(&repo.repo_path).expect("failed to fetch remote state");

  let set_output = Command::new("jj")
    .current_dir(workspace_path_str)
    .args([
      "bookmark",
      "set",
      &workspace.branch_name,
      "-r",
      &format!("{}@git-", workspace.branch_name),
      "--allow-backwards",
    ])
    .output()
    .expect("failed to move bookmark back");

  assert!(
    set_output.status.success(),
    "failed to move bookmark back: {}",
    String::from_utf8_lossy(&set_output.stderr)
  );

  (repo, workspace.id)
}

fn bench_workspace_status_ahead_remote(c: &mut Criterion) {
  let mut group = c.benchmark_group("workspace_status_ahead_remote");
  group.sample_size(10);

  group.bench_function("with_working_copy_and_committed_files", |b| {
    b.iter_batched_ref(
      setup_workspace_status_ahead,
      |(repo, workspace_id)| {
        treq_lib::core::workspace_status(
          std::hint::black_box(&repo.repo_path),
          std::hint::black_box(Some(*workspace_id)),
        )
        .expect("workspace_status failed");
      },
      BatchSize::PerIteration,
    );
  });

  group.finish();
}

fn bench_workspace_status_behind_remote(c: &mut Criterion) {
  let mut group = c.benchmark_group("workspace_status_behind_remote");
  group.sample_size(10);

  group.bench_function("with_working_copy_and_committed_files", |b| {
    b.iter_batched_ref(
      setup_workspace_status_behind,
      |(repo, workspace_id)| {
        treq_lib::core::workspace_status(
          std::hint::black_box(&repo.repo_path),
          std::hint::black_box(Some(*workspace_id)),
        )
        .expect("workspace_status failed");
      },
      BatchSize::PerIteration,
    );
  });

  group.finish();
}

criterion_group!(
  benches,
  bench_workspace_status_ahead_remote,
  bench_workspace_status_behind_remote
);
criterion_main!(benches);
