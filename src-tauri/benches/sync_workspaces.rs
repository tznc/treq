#![allow(
  clippy::unwrap_used,
  clippy::expect_used,
  clippy::string_slice,
  reason = "test code"
)]
use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use std::path::PathBuf;
use std::process::Command;

#[path = "../tests/common/mod.rs"]
mod e2e_test_helpers;
use e2e_test_helpers::TestRepo;

fn create_bench_workspaces(repo: &TestRepo, count: usize) -> Vec<(i64, String, String, PathBuf)> {
  (1..=count)
    .map(|index| {
      let workspace = treq_lib::core::create_workspace(
        &repo.repo_path,
        &format!("feat/sync-bench-{index}"),
        Some(format!("sync bench workspace {index}")),
        None,
        None,
        None,
        None,
      )
      .expect("failed to create benchmark workspace");

      let workspace_path = repo.workspaces_dir().join(&workspace.workspace_path);
      (
        workspace.id,
        workspace.workspace_name,
        workspace.workspace_path,
        workspace_path,
      )
    })
    .collect()
}

fn setup_sync_workspaces_repo() -> TestRepo {
  let repo = TestRepo::new().expect("failed to init test repo");
  let workspaces = create_bench_workspaces(&repo, 5);

  for (_, workspace_name, _, _) in &workspaces[..2] {
    let output = Command::new("jj")
      .current_dir(&repo.repo_path)
      .args(["workspace", "forget", workspace_name.as_str()])
      .output()
      .expect("failed to execute jj workspace forget");
    assert!(
      output.status.success(),
      "jj workspace forget should succeed: {}",
      String::from_utf8_lossy(&output.stderr)
    );
  }

  let (_, _, _, deleted_workspace_path) = &workspaces[2];
  TestRepo::remove_dir_all_path(deleted_workspace_path)
    .expect("failed to delete benchmark workspace directory");

  repo
}

fn bench_sync_workspaces(c: &mut Criterion) {
  let mut group = c.benchmark_group("sync_workspaces");
  group.sample_size(10);

  group.bench_function("5_created_2_forgotten_1_deleted", |b| {
    b.iter_batched_ref(
      setup_sync_workspaces_repo,
      |repo| {
        treq_lib::core::sync_workspaces(std::hint::black_box(&repo.repo_path))
          .expect("sync_workspaces failed");
      },
      BatchSize::PerIteration,
    );
  });

  group.finish();
}

criterion_group!(benches, bench_sync_workspaces);
criterion_main!(benches);
